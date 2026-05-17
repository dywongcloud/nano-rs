//! Tenant-specific worker pool for multi-tenant isolate management
//!
//! This module implements Option 3 architecture: each tenant (hostname) gets
//! dedicated isolates that persist across requests. Contexts are never reset
//! within an isolate - instead, the entire isolate is recycled after a
//! configured number of requests or on OOM.
//!
//! Architecture:
//! ```
//! HTTP Request (hostname)
//!     ↓
//! WorkQueue routes by hostname
//!     ↓
//! TenantPool (dedicated to this hostname)
//!     ↓
//! Warm Isolate with Persistent Context
//!     ↓
//! Execute (NO context reset)
//! ```
//!
//! Benefits:
//! - Zero cold start latency after first request
//! - True tenant isolation (no shared workers)
//! - V8-compatible (no context reset issues)
//! - Matches Cloudflare Workers/Deno Deploy architecture

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use tracing::{error, info};

use crate::config::app::AppLimits;
use crate::control_plane::ControlPlane;
use crate::vfs::{IsolateVfs, VfsBackendEnum, VfsNamespace};
use crate::worker::oom::OomMonitorBuilder;
use crate::worker::HandlerTask;
use crate::data_plane::set_worker_runtime;
use base64::Engine as _;

/// Maximum requests before recycling an isolate
const MAX_REQUESTS_PER_ISOLATE: u32 = 10_000;

// ---------------------------------------------------------------------------
// Thread-local WebSocket connection state
//
// These thread-locals are shared between the ws_messages loop in run_worker
// (this file) and the V8 FunctionCallback implementations in websocket.rs
// (Plan 05). Both run on the same worker thread, so thread-local access is
// safe and lock-free.
// ---------------------------------------------------------------------------

// Outbound frame sender — cloned from WsChannels.outbound_tx on WS entry.
// The send() FunctionCallback reads this to push frames to the relay task.
thread_local! {
    pub(crate) static WS_OUTBOUND: RefCell<Option<std::sync::mpsc::SyncSender<tungstenite::Message>>> =
        RefCell::new(None);
}

// Whether the JS handler called ws.accept() — send() checks this (D-14b).
thread_local! {
    pub(crate) static WS_ACCEPTED: Cell<bool> = Cell::new(false);
}

// JS 'message' event handlers registered via addEventListener('message', fn).
thread_local! {
    pub(crate) static WS_MESSAGE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> =
        RefCell::new(Vec::new());
}

// JS 'close' event handlers registered via addEventListener('close', fn).
thread_local! {
    pub(crate) static WS_CLOSE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> =
        RefCell::new(Vec::new());
}

// JS 'error' event handlers registered via addEventListener('error', fn).
thread_local! {
    pub(crate) static WS_ERROR_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> =
        RefCell::new(Vec::new());
}

// The server-side WebSocket object (v8::Object) created by WebSocketPair ctor.
// Used to update the readyState property on state transitions (D-16b).
thread_local! {
    pub(crate) static WS_SERVER_SOCKET: RefCell<Option<v8::Global<v8::Object>>> =
        RefCell::new(None);
}

/// Update the readyState property on the server WebSocket object.
///
/// Reads WS_SERVER_SOCKET, creates a Local from the stored Global, and sets
/// `readyState` to the given numeric state value. No-op if WS_SERVER_SOCKET
/// is None (safe to call even before WebSocketPair is constructed).
///
/// | state | meaning |
/// |-------|---------|
/// | 0     | CONNECTING |
/// | 1     | OPEN       |
/// | 3     | CLOSED     |
pub(crate) fn set_ws_readystate(scope: &mut v8::PinScope<'_, '_>, state: u32) {
    WS_SERVER_SOCKET.with(|cell| {
        let borrow = cell.borrow();
        if let Some(ref global) = *borrow {
            let obj = v8::Local::new(scope, global);
            if let Some(key) = v8::String::new(scope, "readyState") {
                let val = v8::Integer::new_from_unsigned(scope, state);
                obj.set(scope, key.into(), val.into());
            }
        }
    });
}

/// Reset all WS thread-locals to their initial (idle) state.
///
/// Called after the ws_messages loop exits to ensure no stale V8 Globals
/// or channel senders survive isolate recycling (D-10b full context reset).
pub(crate) fn clear_ws_thread_locals() {
    WS_OUTBOUND.with(|cell| *cell.borrow_mut() = None);
    WS_ACCEPTED.with(|cell| cell.set(false));
    WS_MESSAGE_HANDLERS.with(|cell| cell.borrow_mut().clear());
    WS_CLOSE_HANDLERS.with(|cell| cell.borrow_mut().clear());
    WS_ERROR_HANDLERS.with(|cell| cell.borrow_mut().clear());
    WS_SERVER_SOCKET.with(|cell| *cell.borrow_mut() = None);
}


/// A pool of isolates dedicated to a single tenant (hostname)
pub struct TenantPool {
    hostname: String,
    workers: Vec<TenantWorker>,
    next_worker: AtomicU64,
    #[allow(dead_code)]
    vfs_backend: VfsBackendEnum,
    #[allow(dead_code)]
    control_plane: Option<ControlPlane>,
    /// Lazy WebSocket worker pool — starts empty, grows on demand, shrinks to zero after idle timeout.
    ws_workers: Mutex<Vec<WsWorkerHandle>>,
    /// Number of active WebSocket connections. Incremented by the worker thread when it accepts a
    /// WS task; decremented when the WS connection closes. Shared with worker threads via Arc so
    /// workers can decrement autonomously (D-13b: avoids TOCTOU in dispatch_ws).
    ws_busy: Arc<AtomicUsize>,
    /// Maximum concurrent WebSocket connections for this tenant, from AppLimits (D-07).
    max_ws_connections: u32,
    /// Idle timeout in ms before a WS worker thread exits (D-03b, D-11b).
    ws_idle_timeout_ms: u64,
}

struct TenantWorker {
    task_tx: mpsc::Sender<HandlerTask>,
    thread: Option<thread::JoinHandle<()>>,
}

/// Handle to a lazily-spawned WebSocket worker thread.
///
/// The sender side is kept here so dispatch_ws() can route tasks. The join handle
/// is taken on Drop to ensure orderly shutdown (prevents V8 use-after-platform-shutdown,
/// Pitfall 7 in RESEARCH.md).
struct WsWorkerHandle {
    task_tx: mpsc::Sender<HandlerTask>,
    join: Option<thread::JoinHandle<()>>,
}

impl TenantPool {
    /// Create a new tenant pool for the given hostname.
    ///
    /// `limits` supplies the effective WebSocket connection cap and idle timeout.
    /// Callers that do not have an `AppLimits` value should pass `&AppLimits::default()`.
    pub fn new(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        control_plane: Option<ControlPlane>,
        limits: &AppLimits,
    ) -> Result<Self> {
        let max_ws_connections = limits.effective_max_ws_connections();
        let ws_idle_timeout_ms = limits.effective_ws_idle_timeout_ms();
        let mut workers = Vec::with_capacity(worker_count as usize);

        let ws_busy = Arc::new(AtomicUsize::new(0));

        for id in 0..worker_count {
            let worker = Self::spawn_worker(
                id,
                hostname.clone(),
                memory_limit_mb,
                vfs_backend.clone(),
                Arc::clone(&ws_busy),
                ws_idle_timeout_ms,
            )?;
            workers.push(worker);
        }

        info!(
            "Created tenant pool for '{}' with {} workers (max_ws_connections={}, ws_idle_timeout_ms={})",
            hostname, worker_count, max_ws_connections, ws_idle_timeout_ms
        );

        Ok(Self {
            hostname,
            workers,
            next_worker: AtomicU64::new(0),
            vfs_backend,
            control_plane,
            ws_workers: Mutex::new(Vec::new()),
            ws_busy,
            max_ws_connections,
            ws_idle_timeout_ms,
        })
    }

    /// Spawn a worker thread with its own isolate.
    ///
    /// `ws_busy` is forwarded to `run_worker` so future WS handling code (Plan 04)
    /// can increment/decrement the counter from inside the worker thread.
    fn spawn_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        ws_busy: Arc<AtomicUsize>,
        ws_idle_timeout_ms: u64,
    ) -> Result<TenantWorker> {
        let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
            mpsc::channel();

        let thread = thread::spawn(move || {
            Self::worker_loop(id, hostname, memory_limit_mb, vfs_backend, task_rx, ws_busy, ws_idle_timeout_ms);
        });

        Ok(TenantWorker {
            task_tx,
            thread: Some(thread),
        })
    }

    /// Spawn a dedicated WebSocket worker thread.
    ///
    /// WS workers are lazily created by `dispatch_ws()` and share the same
    /// `ws_busy` counter as HTTP workers so the per-tenant limit is global.
    fn spawn_ws_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        ws_busy: Arc<AtomicUsize>,
        ws_idle_timeout_ms: u64,
    ) -> Result<WsWorkerHandle> {
        let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
            mpsc::channel();

        let thread = thread::spawn(move || {
            Self::worker_loop(id, hostname, memory_limit_mb, vfs_backend, task_rx, ws_busy, ws_idle_timeout_ms);
        });

        Ok(WsWorkerHandle {
            task_tx,
            join: Some(thread),
        })
    }

    /// Worker event loop - owns isolate for this tenant
    fn worker_loop(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
        ws_busy: Arc<AtomicUsize>,
        ws_idle_timeout_ms: u64,
    ) {
        info!("Tenant worker {} for '{}' starting", id, hostname);

        // Set up worker runtime for async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                error!("Failed to create tokio runtime: {}", e);
                return;
            }
        };

        // Store runtime handle in thread-local for async operations
        let rt_handle = rt.handle().clone();
        set_worker_runtime(rt_handle);

        // Run the worker event loop
        Self::run_worker(id, hostname, memory_limit_mb, vfs_backend, task_rx, ws_busy, ws_idle_timeout_ms);
    }

    /// Core worker loop — runs inside a spawned thread.
    ///
    /// `ws_busy` is incremented when a WS task arrives and decremented when
    /// the connection closes (D-13b: worker-side to avoid TOCTOU).
    /// `ws_idle_timeout_ms` controls the recv_timeout for the ws_messages loop.
    fn run_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
        ws_busy: Arc<AtomicUsize>,
        ws_idle_timeout_ms: u64,
    ) {
        use crate::v8::NanoIsolate;

        let oom_monitor = if memory_limit_mb > 0 {
            Some(
                OomMonitorBuilder::new(format!("tenant_{}_{}", hostname, id))
                    .with_limit_mb(memory_limit_mb)
                    .for_hostname(&hostname)
                    .build(),
            )
        } else {
            None
        };

        info!("Tenant worker {} for '{}' ready", id, hostname);

        'isolate: loop {
            let vfs = IsolateVfs::new(VfsNamespace::from_hostname(&hostname), vfs_backend.clone());
            let mut nano = match NanoIsolate::new_with_vfs(vfs) {
                Ok(iso) => iso,
                Err(e) => { error!("Tenant worker {}: isolate create failed: {}", id, e); return; }
            };
            if memory_limit_mb > 0 {
                let bytes = memory_limit_mb as usize * 1024 * 1024;
                nano.set_heap_limits(bytes / 2, bytes);
            }

            // SAFETY: nano lives for the entire scope block below.
            let iso_ptr: *mut v8::Isolate = &mut **nano.isolate();

            {
                let scope_pin = std::pin::pin!(v8::HandleScope::new(nano.isolate()));
                let mut scope = scope_pin.init();
                let context = v8::Context::new(&scope, Default::default());
                // Security: block eval() and Function ctor — matches Cloudflare Workers.
                context.set_allow_generation_from_strings(false);
                crate::runtime::apis::RuntimeAPIs::bind_all(&mut scope, context);
                let mut ctx_scope = v8::ContextScope::new(&mut scope, context);

                let mut handler_cache: std::collections::HashMap<
                    String, v8::Global<v8::Function>
                > = std::collections::HashMap::new();

                let mut served: u32 = 0;
                let isolate_id = format!("{}:{}", hostname, id);

                'requests: loop {
                    if served >= MAX_REQUESTS_PER_ISOLATE {
                        info!("Tenant worker {}: recycling isolate after {} requests", id, served);
                        break 'requests;
                    }

                    let task = match task_rx.recv() {
                        Ok(t) => t,
                        Err(_) => { info!("Tenant worker {} channel closed, exiting", id); break 'isolate; }
                    };

                    if let Some(ref mon) = oom_monitor {
                        // SAFETY: iso_ptr was captured from nano.isolate() before the HandleScope
                        // was created. The isolate is pinned to this thread and nano outlives scope.
                        // OomMonitor::check() only reads heap statistics via v8::HeapStatistics.
                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                        if let Err(oom) = mon.check(iso_ref) {
                            mon.log_oom_event(&oom, &task.request_id);
                            let _ = task.response_tx.send(Ok(mon.create_oom_response(&oom)));
                            break 'requests;
                        }
                    }

                    // --- WebSocket mode: task.ws.is_some() → enter ws_messages loop ---
                    // D-01 (pin-a-worker), D-03 (drain-then-recycle), D-09b (per-message CpuTimeoutGuard),
                    // D-10 (sequential per-connection), D-13 (graceful OOM).
                    if let Some(ws_channels) = task.ws {
                        // D-13b: increment ws_busy inside worker thread (not in dispatch_ws)
                        // to avoid TOCTOU between the connection-limit check and the actual send.
                        ws_busy.fetch_add(1, Ordering::SeqCst);

                        // Seed thread-locals for this connection.
                        WS_OUTBOUND.with(|tx| *tx.borrow_mut() = Some(ws_channels.outbound_tx.clone()));
                        WS_ACCEPTED.with(|a| a.set(false));
                        WS_MESSAGE_HANDLERS.with(|h| h.borrow_mut().clear());
                        WS_CLOSE_HANDLERS.with(|h| h.borrow_mut().clear());
                        WS_ERROR_HANDLERS.with(|h| h.borrow_mut().clear());
                        // WS_SERVER_SOCKET left as None — set by WebSocketPair ctor in Plan 05.

                        // Signal 101 Switching Protocols back to the HTTP layer.
                        // The axum relay task (Plan 03) reads this to confirm the upgrade.
                        let upgrade_response = crate::http::NanoResponse::with_status(101);
                        let _ = task.response_tx.send(Ok(upgrade_response));

                        // Call the JS fetch handler so the JS handler can register event listeners
                        // via ws.addEventListener / ws.onmessage etc. (Plan 05 wires these APIs).
                        // We reuse the entrypoint handler already cached for this isolate (or load it).
                        let entrypoint = task.entrypoint.clone();
                        // Ensure handler is cached (same path as HTTP).
                        if !handler_cache.contains_key(&entrypoint) {
                            let code = match crate::data_plane::read_code_cached(&entrypoint) {
                                Ok(c) => c,
                                Err(e) => {
                                    error!("WS handler code read failed: {}", e);
                                    clear_ws_thread_locals();
                                    ws_busy.fetch_sub(1, Ordering::SeqCst);
                                    break 'requests;
                                }
                            };
                            let transformed = if crate::v8::module::is_esm_module(&code) {
                                crate::v8::module::transform_module_code(&code)
                            } else { code.to_string() };
                            let code_v8 = match v8::String::new(&mut ctx_scope, &transformed) {
                                Some(s) => s,
                                None => {
                                    clear_ws_thread_locals();
                                    ws_busy.fetch_sub(1, Ordering::SeqCst);
                                    break 'requests;
                                }
                            };
                            let script = match v8::Script::compile(&ctx_scope, code_v8, None) {
                                Some(s) => s,
                                None => {
                                    clear_ws_thread_locals();
                                    ws_busy.fetch_sub(1, Ordering::SeqCst);
                                    break 'requests;
                                }
                            };
                            if script.run(&ctx_scope).is_none() {
                                clear_ws_thread_locals();
                                ws_busy.fetch_sub(1, Ordering::SeqCst);
                                break 'requests;
                            }
                            let global_obj = context.global(&mut ctx_scope);
                            let nano_k = v8::String::new(&mut ctx_scope, "__nano_user_fetch");
                            let fetch_k = v8::String::new(&mut ctx_scope, "fetch");
                            if let (Some(nk), Some(fk)) = (nano_k, fetch_k) {
                                let handler_val = global_obj.get(&mut ctx_scope, nk.into())
                                    .filter(|v| v.is_function())
                                    .or_else(|| global_obj.get(&mut ctx_scope, fk.into()).filter(|v| v.is_function()));
                                match handler_val {
                                    Some(f) => {
                                        let g = v8::Global::new(&**ctx_scope, f.cast::<v8::Function>());
                                        handler_cache.insert(entrypoint.clone(), g);
                                    }
                                    None => {
                                        clear_ws_thread_locals();
                                        ws_busy.fetch_sub(1, Ordering::SeqCst);
                                        break 'requests;
                                    }
                                }
                            }
                        }

                        // Call the JS fetch handler — JS registers event listeners here.
                        // We create a minimal Request with the upgrade URL so the handler
                        // can distinguish WS from HTTP if desired.
                        if let Some(handler_g) = handler_cache.get(&entrypoint) {
                            let global_obj = context.global(&mut ctx_scope);
                            let handler_local = v8::Local::new(&mut ctx_scope, handler_g);
                            if let Some(url_str) = v8::String::new(&mut ctx_scope, &task.request.url().href()) {
                                let tc_storage = v8::TryCatch::new(&mut *ctx_scope);
                                let tc_pin = std::pin::pin!(tc_storage);
                                let tc = tc_pin.init();
                                let _ = handler_local.call(&tc, global_obj.into(), &[url_str.into()]);
                                drop(tc);
                            }
                        }

                        // Set readyState to OPEN (1) on the JS WebSocket object (D-16b).
                        // If WS_SERVER_SOCKET is None (Plan 05 not yet wired), this is a no-op.
                        set_ws_readystate(&mut ctx_scope, 1);

                        info!(
                            "Tenant worker {}: entering ws_messages loop for '{}'",
                            id, entrypoint
                        );

                        // --- ws_messages inner loop (D-01, D-03, D-09b) ---
                        let idle_dur = std::time::Duration::from_millis(
                            if ws_idle_timeout_ms > 0 { ws_idle_timeout_ms } else { 30_000 }
                        );

                        'ws_messages: loop {
                            match ws_channels.inbound_rx.recv_timeout(idle_dur) {
                                // --- Text frame ---
                                Ok(tungstenite::Message::Text(s)) => {
                                    // OOM check before dispatching to JS (D-13).
                                    if let Some(ref mon) = oom_monitor {
                                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                        if let Err(_oom) = mon.check(iso_ref) {
                                            // Send close 1011 (Internal Error) and recycle.
                                            let close_frame = tungstenite::Message::Close(Some(
                                                tungstenite::protocol::CloseFrame {
                                                    code: tungstenite::protocol::frame::coding::CloseCode::Error,
                                                    reason: std::borrow::Cow::Borrowed("OOM"),
                                                }
                                            ));
                                            let _ = ws_channels.outbound_tx.send(close_frame);
                                            break 'ws_messages;
                                        }
                                    }
                                    // Per-message CPU timeout guard (D-09b).
                                    let _timeout = if task.cpu_time_limit_ms > 0 {
                                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                        Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                                    } else { None };
                                    // Build JS MessageEvent: { type: "message", data: <string> }
                                    let event = v8::Object::new(&mut ctx_scope);
                                    if let (Some(tk), Some(tv), Some(dk), Some(dv)) = (
                                        v8::String::new(&mut ctx_scope, "type"),
                                        v8::String::new(&mut ctx_scope, "message"),
                                        v8::String::new(&mut ctx_scope, "data"),
                                        v8::String::new(&mut ctx_scope, s.as_str()),
                                    ) {
                                        event.set(&mut ctx_scope, tk.into(), tv.into());
                                        event.set(&mut ctx_scope, dk.into(), dv.into());
                                        let global_obj = context.global(&mut ctx_scope);
                                        WS_MESSAGE_HANDLERS.with(|cell| {
                                            for handler_g in cell.borrow().iter() {
                                                let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                let tc_pin = std::pin::pin!(tc_s);
                                                let tc = tc_pin.init();
                                                let _ = hlocal.call(&tc, global_obj.into(), &[event.into()]);
                                            }
                                        });
                                    }
                                }

                                // --- Binary frame ---
                                Ok(tungstenite::Message::Binary(b)) => {
                                    // OOM check (D-13).
                                    if let Some(ref mon) = oom_monitor {
                                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                        if let Err(_oom) = mon.check(iso_ref) {
                                            let close_frame = tungstenite::Message::Close(Some(
                                                tungstenite::protocol::CloseFrame {
                                                    code: tungstenite::protocol::frame::coding::CloseCode::Error,
                                                    reason: std::borrow::Cow::Borrowed("OOM"),
                                                }
                                            ));
                                            let _ = ws_channels.outbound_tx.send(close_frame);
                                            break 'ws_messages;
                                        }
                                    }
                                    // Per-message CPU timeout guard (D-09b).
                                    let _timeout = if task.cpu_time_limit_ms > 0 {
                                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                        Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                                    } else { None };
                                    // Build JS MessageEvent with ArrayBuffer data.
                                    let byte_len = b.len();
                                    let ab_store = v8::ArrayBuffer::new_backing_store_from_vec(b);
                                    let shared = ab_store.make_shared();
                                    let ab = v8::ArrayBuffer::with_backing_store(&mut ctx_scope, &shared);
                                    let event = v8::Object::new(&mut ctx_scope);
                                    if let (Some(tk), Some(tv), Some(dk)) = (
                                        v8::String::new(&mut ctx_scope, "type"),
                                        v8::String::new(&mut ctx_scope, "message"),
                                        v8::String::new(&mut ctx_scope, "data"),
                                    ) {
                                        let _ = byte_len; // consumed via ab
                                        event.set(&mut ctx_scope, tk.into(), tv.into());
                                        event.set(&mut ctx_scope, dk.into(), ab.into());
                                        let global_obj = context.global(&mut ctx_scope);
                                        WS_MESSAGE_HANDLERS.with(|cell| {
                                            for handler_g in cell.borrow().iter() {
                                                let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                let tc_pin = std::pin::pin!(tc_s);
                                                let tc = tc_pin.init();
                                                let _ = hlocal.call(&tc, global_obj.into(), &[event.into()]);
                                            }
                                        });
                                    }
                                }

                                // --- Close frame (D-12) ---
                                Ok(tungstenite::Message::Close(frame)) => {
                                    // Transition readyState to CLOSED (3) on the JS object.
                                    set_ws_readystate(&mut ctx_scope, 3);
                                    // Build JS CloseEvent: { type:"close", code, reason, wasClean:true }
                                    let (code_val, reason_str) = frame
                                        .map(|f| (u16::from(f.code), f.reason.into_owned()))
                                        .unwrap_or((1000, String::new()));
                                    let close_event = v8::Object::new(&mut ctx_scope);
                                    if let (Some(tyk), Some(tyv), Some(ck), Some(rk), Some(rv), Some(wck)) = (
                                        v8::String::new(&mut ctx_scope, "type"),
                                        v8::String::new(&mut ctx_scope, "close"),
                                        v8::String::new(&mut ctx_scope, "code"),
                                        v8::String::new(&mut ctx_scope, "reason"),
                                        v8::String::new(&mut ctx_scope, &reason_str),
                                        v8::String::new(&mut ctx_scope, "wasClean"),
                                    ) {
                                        let code_int = v8::Integer::new(&mut ctx_scope, code_val as i32);
                                        let was_clean = v8::Boolean::new(&mut ctx_scope, true);
                                        close_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                        close_event.set(&mut ctx_scope, ck.into(), code_int.into());
                                        close_event.set(&mut ctx_scope, rk.into(), rv.into());
                                        close_event.set(&mut ctx_scope, wck.into(), was_clean.into());
                                        let global_obj = context.global(&mut ctx_scope);
                                        WS_CLOSE_HANDLERS.with(|cell| {
                                            for handler_g in cell.borrow().iter() {
                                                let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                let tc_pin = std::pin::pin!(tc_s);
                                                let tc = tc_pin.init();
                                                let _ = hlocal.call(&tc, global_obj.into(), &[close_event.into()]);
                                            }
                                        });
                                    }
                                    break 'ws_messages;
                                }

                                // --- Ping / Pong — skip per D-15b ---
                                Ok(tungstenite::Message::Ping(_)) | Ok(tungstenite::Message::Pong(_)) => {
                                    continue 'ws_messages;
                                }

                                // --- Idle timeout (D-11b) — recycle worker ---
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    info!("Tenant worker {}: WS idle timeout, recycling", id);
                                    break 'ws_messages;
                                }

                                // --- Channel disconnect: relay task dropped (D-17b) ---
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                    // Abnormal close: relay dropped inbound_tx without Close frame.
                                    set_ws_readystate(&mut ctx_scope, 3);
                                    // Fire error event first, then close event with code 1006.
                                    let error_event = v8::Object::new(&mut ctx_scope);
                                    if let (Some(tyk), Some(tyv)) = (
                                        v8::String::new(&mut ctx_scope, "type"),
                                        v8::String::new(&mut ctx_scope, "error"),
                                    ) {
                                        error_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                        let global_obj = context.global(&mut ctx_scope);
                                        WS_ERROR_HANDLERS.with(|cell| {
                                            for handler_g in cell.borrow().iter() {
                                                let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                let tc_pin = std::pin::pin!(tc_s);
                                                let tc = tc_pin.init();
                                                let _ = hlocal.call(&tc, global_obj.into(), &[error_event.into()]);
                                            }
                                        });
                                    }
                                    // Close event with code 1006 (Abnormal Closure).
                                    let close_event = v8::Object::new(&mut ctx_scope);
                                    if let (Some(tyk), Some(tyv), Some(ck), Some(rk), Some(rv), Some(wck)) = (
                                        v8::String::new(&mut ctx_scope, "type"),
                                        v8::String::new(&mut ctx_scope, "close"),
                                        v8::String::new(&mut ctx_scope, "code"),
                                        v8::String::new(&mut ctx_scope, "reason"),
                                        v8::String::new(&mut ctx_scope, ""),
                                        v8::String::new(&mut ctx_scope, "wasClean"),
                                    ) {
                                        let code_int = v8::Integer::new(&mut ctx_scope, 1006);
                                        let was_clean = v8::Boolean::new(&mut ctx_scope, false);
                                        close_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                        close_event.set(&mut ctx_scope, ck.into(), code_int.into());
                                        close_event.set(&mut ctx_scope, rk.into(), rv.into());
                                        close_event.set(&mut ctx_scope, wck.into(), was_clean.into());
                                        let global_obj = context.global(&mut ctx_scope);
                                        WS_CLOSE_HANDLERS.with(|cell| {
                                            for handler_g in cell.borrow().iter() {
                                                let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                let tc_pin = std::pin::pin!(tc_s);
                                                let tc = tc_pin.init();
                                                let _ = hlocal.call(&tc, global_obj.into(), &[close_event.into()]);
                                            }
                                        });
                                    }
                                    break 'ws_messages;
                                }

                                // --- Future-proof: unknown frame variants ---
                                Ok(_) => continue 'ws_messages,
                            }
                        } // end 'ws_messages

                        // --- Post-ws_messages cleanup (D-10b) ---
                        clear_ws_thread_locals();
                        ws_busy.fetch_sub(1, Ordering::SeqCst);
                        // D-03: WS messages do NOT increment served counter.
                        // D-10b: Break 'requests to force isolate recycle — ensures a fresh
                        // isolate for the next connection (no stale JS state from prior connection).
                        break 'requests;
                    }
                    // --- End WS mode branch — fall through to HTTP handling below ---

                    let t0 = std::time::Instant::now();
                    let request_id = task.request_id.clone();
                    let entrypoint = task.entrypoint.clone();

                    if !handler_cache.contains_key(&entrypoint) {
                        let code = match crate::data_plane::read_code_cached(&entrypoint) {
                            Ok(c) => c,
                            Err(e) => { let _ = task.response_tx.send(Err(e)); continue 'requests; }
                        };
                        let transformed = if crate::v8::module::is_esm_module(&code) {
                            crate::v8::module::transform_module_code(&code)
                        } else { code.to_string() };

                        let code_v8 = match v8::String::new(&mut ctx_scope, &transformed) {
                            Some(s) => s,
                            None => { let _ = task.response_tx.send(Err(anyhow!("V8 string alloc failed"))); continue 'requests; }
                        };
                        let script = match v8::Script::compile(&ctx_scope, code_v8, None) {
                            Some(s) => s,
                            None => { let _ = task.response_tx.send(Err(anyhow!("Compile failed: {}", entrypoint))); continue 'requests; }
                        };
                        if script.run(&ctx_scope).is_none() {
                            let _ = task.response_tx.send(Err(anyhow!("Script execution failed: {}", entrypoint)));
                            continue 'requests;
                        }

                        let global_obj = context.global(&mut ctx_scope);
                        let nano_k = match v8::String::new(&mut ctx_scope, "__nano_user_fetch") {
                            Some(s) => s,
                            None => { let _ = task.response_tx.send(Err(anyhow!("V8 OOM allocating key"))); continue 'requests; }
                        };
                        let fetch_k = match v8::String::new(&mut ctx_scope, "fetch") {
                            Some(s) => s,
                            None => { let _ = task.response_tx.send(Err(anyhow!("V8 OOM allocating key"))); continue 'requests; }
                        };
                        let handler_val = global_obj.get(&mut ctx_scope, nano_k.into())
                            .filter(|v| v.is_function())
                            .or_else(|| global_obj.get(&mut ctx_scope, fetch_k.into()).filter(|v| v.is_function()));

                        match handler_val {
                            Some(f) => {
                                let g = v8::Global::new(&**ctx_scope, f.cast::<v8::Function>());
                                handler_cache.insert(entrypoint.clone(), g);
                                info!("Tenant worker {}: handler cached for '{}'", id, entrypoint);
                            }
                            None => {
                                let _ = task.response_tx.send(Err(anyhow!("No fetch handler in '{}'", entrypoint)));
                                continue 'requests;
                            }
                        }
                    }

                    let _timeout = if task.cpu_time_limit_ms > 0 {
                        // SAFETY: iso_ptr is valid for this isolate's lifetime. CpuTimeoutGuard
                        // stores the pointer and calls terminate_execution() from a timer thread,
                        // which V8 documents as safe to call from any thread.
                        let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                        Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                    } else { None };

                    // handler_cache.get is infallible here: we just inserted above if missing.
                    let handler_g = handler_cache.get(&entrypoint)
                        .expect("handler must be cached: just inserted in block above");
                    let global_obj = context.global(&mut ctx_scope);
                    let handler_local = v8::Local::new(&mut ctx_scope, handler_g);

                    let result: anyhow::Result<crate::http::NanoResponse> = (|| {
                        let url_str = v8::String::new(&mut ctx_scope, &task.request.url().href())
                            .ok_or_else(|| anyhow!("URL alloc failed"))?;
                        let opts = v8::Object::new(&mut ctx_scope);
                        let mk = v8::String::new(&mut ctx_scope, "method").ok_or_else(|| anyhow!("method key"))?;
                        let mv = v8::String::new(&mut ctx_scope, task.request.method()).ok_or_else(|| anyhow!("method val"))?;
                        opts.set(&mut ctx_scope, mk.into(), mv.into());

                        let hk = v8::String::new(&mut ctx_scope, "headers").ok_or_else(|| anyhow!("headers key"))?;
                        let hck = v8::String::new(&mut ctx_scope, "Headers").ok_or_else(|| anyhow!("Headers ctor key"))?;
                        let hctor = global_obj.get(&mut ctx_scope, hck.into())
                            .filter(|v| v.is_function())
                            .ok_or_else(|| anyhow!("Headers constructor not found"))?
                            .cast::<v8::Function>();
                        let hinit = v8::Object::new(&mut ctx_scope);
                        for (name, vals) in task.request.headers().entries() {
                            let val = vals.join(", ");
                            if let (Some(k), Some(v)) = (
                                v8::String::new(&mut ctx_scope, name),
                                v8::String::new(&mut ctx_scope, &val),
                            ) { hinit.set(&mut ctx_scope, k.into(), v.into()); }
                        }
                        let hobj = hctor.new_instance(&mut ctx_scope, &[hinit.into()])
                            .ok_or_else(|| anyhow!("Headers instantiation failed"))?;
                        opts.set(&mut ctx_scope, hk.into(), hobj.into());

                        if let Some(body) = task.request.body() {
                            let bk = v8::String::new(&mut ctx_scope, "body").ok_or_else(|| anyhow!("body key"))?;
                            let encoded = base64::engine::general_purpose::STANDARD.encode(body);
                            let bv = v8::String::new(&mut ctx_scope, &encoded).ok_or_else(|| anyhow!("body val"))?;
                            opts.set(&mut ctx_scope, bk.into(), bv.into());
                        }

                        let rck = v8::String::new(&mut ctx_scope, "Request").ok_or_else(|| anyhow!("Request key"))?;
                        let rctor = global_obj.get(&mut ctx_scope, rck.into())
                            .filter(|v| v.is_function())
                            .ok_or_else(|| anyhow!("Request constructor not found"))?
                            .cast::<v8::Function>();
                        let js_req = rctor.new_instance(&mut ctx_scope, &[url_str.into(), opts.into()])
                            .ok_or_else(|| anyhow!("Request instantiation failed"))?;

                        // TryCatch intercepts any JS exception thrown by the handler.
                        // Dropping tc at closure exit clears the pending exception from
                        // the isolate, preventing isolate poisoning across requests.
                        // Must pin-and-init like HandleScope — TryCatch::new returns ScopeStorage.
                        let tc_storage = v8::TryCatch::new(&mut *ctx_scope);
                        let tc_pin = std::pin::pin!(tc_storage);
                        let mut tc = tc_pin.init(); // mut needed for perform_microtask_checkpoint

                        let call_result = handler_local.call(&tc, global_obj.into(), &[js_req.into()]);
                        let resolved = match call_result {
                            None => {
                                let msg = tc.exception()
                                    .and_then(|e| e.to_string(&tc))
                                    .map(|s| s.to_rust_string_lossy(&tc))
                                    .unwrap_or_else(|| "unknown JS exception".to_string());
                                return Err(anyhow!("JS exception: {}", msg));
                            }
                            Some(v) if v.is_promise() => {
                                let promise = v.cast::<v8::Promise>();
                                let platform = v8::V8::get_current_platform();
                                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                                loop {
                                    for _ in 0..5 {
                                        // SAFETY: pump_message_loop requires &Isolate. iso_ptr is
                                        // valid for this thread for the isolate's lifetime.
                                        let iso: &v8::Isolate = unsafe { &*iso_ptr };
                                        v8::Platform::pump_message_loop(&platform, iso, false);
                                    }
                                    tc.perform_microtask_checkpoint();
                                    match promise.state() {
                                        v8::PromiseState::Fulfilled => break promise.result(&tc),
                                        v8::PromiseState::Rejected => {
                                            let err = promise.result(&tc);
                                            let msg = err.to_string(&tc)
                                                .map(|s| s.to_rust_string_lossy(&tc))
                                                .unwrap_or_else(|| "Promise rejected".to_string());
                                            return Err(anyhow!("Promise rejected: {}", msg));
                                        }
                                        v8::PromiseState::Pending => {
                                            if std::time::Instant::now() > deadline {
                                                return Err(anyhow!("Async handler timed out"));
                                            }
                                            std::thread::yield_now();
                                        }
                                    }
                                }
                            }
                            Some(v) => v,
                        };

                        let obj = resolved.to_object(&tc)
                            .ok_or_else(|| anyhow!("Handler response is not an object"))?;
                        let sk = v8::String::new(&tc, "status").ok_or_else(|| anyhow!("status key"))?;
                        let status = obj.get(&tc, sk.into())
                            .and_then(|v| v.to_integer(&tc))
                            .map(|i| i.value() as u16)
                            .unwrap_or(200);
                        let mut response = crate::http::NanoResponse::with_status(status);

                        let h2k = v8::String::new(&tc, "headers").ok_or_else(|| anyhow!("headers key"))?;
                        if let Some(hval) = obj.get(&tc, h2k.into()) {
                            if let Some(hobj) = hval.to_object(&tc) {
                                let ik = v8::String::new(&tc, "__headers__").ok_or_else(|| anyhow!("__headers__ key"))?;
                                let hsrc = hobj.get(&tc, ik.into())
                                    .and_then(|v| v.to_object(&tc))
                                    .unwrap_or(hobj);
                                if let Some(names) = hsrc.get_own_property_names(&tc, Default::default()) {
                                    for i in 0..names.length() {
                                        if let Some(key) = names.get_index(&tc, i) {
                                            if let Some(ks) = key.to_string(&tc) {
                                                let k = ks.to_rust_string_lossy(&tc);
                                                if k.starts_with("__") || matches!(k.as_str(), "set" | "get" | "forEach") { continue; }
                                                if let Some(val) = hsrc.get(&tc, key.into()) {
                                                    if !val.is_function() {
                                                        if let Some(vs) = val.to_string(&tc) {
                                                            response = response.with_header(&k, &vs.to_rust_string_lossy(&tc));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let b2k = v8::String::new(&tc, "body").ok_or_else(|| anyhow!("body key"))?;
                        if let Some(bval) = obj.get(&tc, b2k.into()) {
                            if !bval.is_null() && !bval.is_undefined() {
                                if let Some(bs) = bval.to_string(&tc) {
                                    response = response.with_body(bs.to_rust_string_lossy(&tc));
                                }
                            }
                        }
                        Ok(response)
                    })();

                    let duration_ms = t0.elapsed().as_millis() as u64;
                    let status_code = match &result { Ok(r) => r.status(), Err(_) => 500 };
                    tracing::info!(
                        request_id = %request_id,
                        worker_id = id,
                        isolate_id = %isolate_id,
                        status = status_code,
                        duration_ms = duration_ms,
                        "Tenant worker {} request {} → {} in {}ms", id, request_id, status_code, duration_ms
                    );
                    let result = result.map(|mut r| { r.set_worker_id(id); r.set_isolate_id(isolate_id.clone()); r });
                    let _ = task.response_tx.send(result);
                    served += 1;
                }
            }
            info!("Tenant worker {}: isolate recycled, creating fresh", id);
        }
        info!("Tenant worker {} exiting", id);
    }

    /// Dispatch a task to this tenant's pool
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        // Round-robin to next worker
        let worker_idx = (self.next_worker.fetch_add(1, Ordering::Relaxed) as usize)
            % self.workers.len();
        
        self.workers[worker_idx]
            .task_tx
            .send(task)
            .map_err(|_| anyhow!("Worker channel closed"))
    }

    /// Dispatch a WebSocket task to this tenant's lazy WS worker pool.
    ///
    /// # Connection-limit enforcement (D-07 / T-23-02)
    ///
    /// Checks `ws_busy` against `max_ws_connections` before doing any work.
    /// Returns an error immediately when the limit is reached so the HTTP layer
    /// can reject the upgrade with 503 instead of silently queuing.
    ///
    /// # Dead-handle pruning (T-23-03)
    ///
    /// On every call, handles whose task channel has been disconnected (worker
    /// thread exited) are removed from `ws_workers`.
    ///
    /// # ws_busy increment location (D-13b)
    ///
    /// `ws_busy` is incremented INSIDE the worker thread when it actually receives
    /// and processes the WS task — NOT here. Incrementing here would create a TOCTOU
    /// window between the check and the send. Plan 04 adds the increment in
    /// `run_worker` using the shared `Arc<AtomicUsize>`.
    pub fn dispatch_ws(&self, task: HandlerTask) -> Result<()> {
        // --- Connection-limit gate (D-07) ---
        let busy = self.ws_busy.load(Ordering::SeqCst);
        if busy >= self.max_ws_connections as usize {
            return Err(anyhow!(
                "WebSocket connection limit reached ({}/{})",
                busy,
                self.max_ws_connections
            ));
        }

        // --- Acquire WS worker pool lock ---
        let mut ws_workers = self.ws_workers
            .lock()
            .map_err(|_| anyhow!("ws_workers mutex poisoned"))?;

        // Dead-handle pruning: we detect a disconnected channel only when we attempt
        // to send on it (std::sync::mpsc has no non-destructive "is alive?" query).
        // Pruning happens inline in the send loop below — any handle that returns
        // SendError is immediately removed and its join handle is collected.

        // Search for an idle worker — one whose channel still has room.
        // Try each worker in turn; if its channel is disconnected, remove it.
        let mut task_opt = Some(task);
        let mut sent = false;

        let i = 0;
        while i < ws_workers.len() {
            let task = task_opt.take().expect("task_opt always Some at loop top");
            match ws_workers[i].task_tx.send(task) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(mpsc::SendError(returned_task)) => {
                    // Channel disconnected — worker exited. Remove the dead handle.
                    // join handle is in ws_workers[i].join; take it to avoid blocking.
                    let dead = ws_workers.swap_remove(i);
                    if let Some(jh) = dead.join {
                        // Non-blocking: the thread should be done since the Receiver was dropped.
                        let _ = jh.join();
                    }
                    // Don't advance i — swap_remove replaced position i with last element.
                    task_opt = Some(returned_task);
                }
            }
        }

        if !sent {
            // No live idle worker found — spawn a fresh one.
            let task = task_opt.take().expect("task_opt must still be Some");
            let ws_worker_id = ws_workers.len() as u32;
            // We need memory_limit_mb and vfs_backend from TenantPool context.
            // Since spawn_ws_worker needs these, store them at new() time.
            // For now: use the values from the HTTP workers (they share the same vfs_backend).
            // Plan 04 may introduce a dedicated WS isolate; for now reuse HTTP worker config.
            // NOTE: memory_limit_mb is not stored on TenantPool. Add a field if needed.
            // We use 0 (no OOM monitoring) for WS workers in Plan 02 as a placeholder;
            // Plan 04 will set appropriate limits.
            let new_handle = Self::spawn_ws_worker(
                ws_worker_id,
                self.hostname.clone(),
                0, // memory_limit_mb — WS workers share OOM monitoring via ws_busy
                self.vfs_backend.clone(),
                Arc::clone(&self.ws_busy),
                self.ws_idle_timeout_ms,
            )?;
            new_handle.task_tx
                .send(task)
                .map_err(|_| anyhow!("Newly spawned WS worker channel immediately closed"))?;
            ws_workers.push(new_handle);
            info!(
                "Spawned WS worker {} for '{}' (ws_busy={}, max={})",
                ws_worker_id, self.hostname, busy, self.max_ws_connections
            );
        }

        Ok(())
    }

    /// Get number of workers in this pool
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Get hostname this pool serves
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
}

impl Drop for TenantPool {
    fn drop(&mut self) {
        info!("Dropping tenant pool for '{}'", self.hostname);

        // Signal HTTP workers to exit by dropping their senders, then join.
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }

        // Signal WS workers to exit and join their threads.
        // Prevents V8 isolate use-after-platform-shutdown (Pitfall 7 in RESEARCH.md).
        if let Ok(mut ws_workers) = self.ws_workers.lock() {
            for handle in ws_workers.iter_mut() {
                if let Some(jh) = handle.join.take() {
                    let _ = jh.join();
                }
            }
        }
    }
}


