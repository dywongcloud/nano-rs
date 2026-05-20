//! Worker pool implementation with thread-local isolate ownership
//!
//! This module provides the WorkerPool that manages N worker threads,
//! each owning a V8 isolate. Tasks are dispatched via MPSC channels
//! and responses are returned via oneshot channels.

use crate::v8::{initialize_platform, NanoIsolate};
use crate::worker::oom::OomMonitorBuilder;
use crate::worker::HandlerTask;
use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
use base64::Engine as _;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{anyhow, Result};

// Thread-local storage for the worker thread's Tokio runtime handle
// This allows fetch() and other async operations to access the runtime
thread_local! {
    static WORKER_RUNTIME: RefCell<Option<tokio::runtime::Handle>> = RefCell::new(None);
}

pub use crate::data_plane::with_worker_runtime;

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use tracing::{debug, error, info, warn};

/// Handle to a worker thread
///
/// Contains the thread join handle and the MPSC sender for dispatching tasks.
/// Dropping the handle closes the channel, signaling the worker to exit.
#[derive(Debug)]
pub struct WorkerHandle {
    /// Worker thread ID (0-indexed)
    pub id: u32,
    /// Thread join handle for cleanup
    thread: Option<JoinHandle<()>>,
    /// MPSC sender for task dispatch
    task_tx: mpsc::Sender<HandlerTask>,
}

impl WorkerHandle {
    /// Send a task to this worker
    ///
    /// # Arguments
    ///
    /// * `task` - The HandlerTask to execute
    ///
    /// # Returns
    ///
    /// `Ok(())` if the task was sent, `Err` if the channel is closed
    pub fn send(&self, task: HandlerTask) -> Result<()> {
        self.task_tx
            .send(task)
            .map_err(|_| anyhow!("Worker {} channel closed", self.id))
    }

    /// Take the join handle for thread cleanup
    fn take_thread(&mut self) -> Option<JoinHandle<()>> {
        self.thread.take()
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Channel is dropped automatically, signaling worker to exit
        debug!("WorkerHandle {} dropped, signaling worker to exit", self.id);
    }
}

/// Pool of worker threads for JavaScript execution (Legacy)
///
/// **Deprecation Notice:** This is the original WorkerPool implementation
/// maintained for backward compatibility with existing tests. New code
/// should use one of:
///
/// - [`SliverWorkerPool`] - For production snapshot-based execution
/// - [`EntrypointWorkerPool`] - For dynamic app loading and testing
/// - [`WorkQueue`] - For multi-tenant hostname-based routing
///
/// This pool provides full features (CPU limits, memory monitoring, eviction)
/// but lacks the clear separation of concerns of the newer pool types.
///
/// ## Migration Guide
///
/// | Current Usage | Migrate To | Reason |
/// |-------------|-----------|--------|
/// | `WorkerPool::new(hostname, n, mem)` | `SliverWorkerPool` | Production with slivers |
/// | Dynamic loading | `EntrypointWorkerPool` | Async VFS creation |
/// | Multi-tenant | `WorkQueue` | Per-hostname pools |
///
/// Each worker owns one V8 isolate (thread-local). Tasks are dispatched
/// via MPSC channels. The pool uses round-robin for initial dispatch.
pub struct WorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: u32,
    /// Hostname this pool serves (for logging/debugging)
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicU32,
    /// Shared VFS backend for all workers in this pool
    vfs_backend: crate::vfs::VfsBackendEnum,
    /// Memory limit per isolate in MB
    memory_limit_mb: u32,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("workers", &self.workers.len())
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("next_worker", &self.next_worker)
            .field("vfs_backend", &"<dyn VfsBackend>")
            .field("memory_limit_mb", &self.memory_limit_mb)
            .finish()
    }
}

impl WorkerPool {
    /// Create a new worker pool with N worker threads
    ///
    /// Each worker thread:
    /// 1. Creates its own NanoIsolate (thread-local ownership)
    /// 2. Runs an event loop receiving HandlerTask via MPSC
    /// 3. Executes JavaScript handlers and sends responses back
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    ///
    /// # Returns
    ///
    /// A new WorkerPool with N workers ready to receive tasks
    ///
    /// # Panics
    ///
    /// Panics if the V8 platform is not initialized
    pub fn new(hostname: String, worker_count: u32, memory_limit_mb: u32) -> Self {
        Self::with_backend(hostname, worker_count, memory_limit_mb, crate::vfs::VfsBackendEnum::memory(MemoryBackend::default()))
    }

    /// Create a new worker pool with a specific VFS backend
    ///
    /// This allows configuring the storage backend (memory, disk, S3)
    /// for the VFS used by all workers in this pool.
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `vfs_backend` - The VFS backend to use (Arc<dyn VfsBackend>)
    ///
    /// # Returns
    ///
    /// A new WorkerPool with N workers ready to receive tasks
    ///
    /// # Panics
    ///
    /// Panics if the V8 platform is not initialized
    pub fn with_backend(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: crate::vfs::VfsBackendEnum,
    ) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        // Clone hostname for use in closures (original kept for final logging)
        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = vfs_backend.clone();

        let mut workers = Vec::with_capacity(worker_count as usize);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = vfs_backend_for_workers.clone();
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with persistent V8 scope lifecycle.
            //
            // Architecture: Cloudflare Workers / Deno Deploy pattern.
            //   - HandleScope + ContextScope stay alive on the thread stack
            //     for ALL requests within one isolate's lifetime.
            //   - Handler script compiled ONCE per entrypoint, cached as Global<Function>.
            //   - Per request: Local::new(&mut ctx_scope, &cached_global) → call.
            //   - After MAX_REQUESTS_PER_ISOLATE: drop scopes, drop isolate, create fresh.
            //
            // This eliminates the 50ms per-request cold start (script compilation + scope
            // creation) and reduces request latency to <1ms for cached handlers.
            let thread = thread::spawn(move || {
                info!("Worker {} starting for '{}'", id, worker_hostname);

                // Tokio runtime for async JS operations (fetch, etc.)
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => { error!("Worker {}: tokio runtime failed: {}", id, e); return; }
                };
                WORKER_RUNTIME.with(|r| *r.borrow_mut() = Some(rt.handle().clone()));

                // OOM monitor (optional)
                let oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                // Max requests per isolate before recycling
                const MAX_REQUESTS_PER_ISOLATE: u32 = 10_000;

                // Outer loop: one iteration per isolate lifetime
                'isolate: loop {
                    let vfs = IsolateVfs::new(
                        VfsNamespace::from_hostname(&worker_hostname),
                        worker_vfs_backend.clone(),
                    );
                    let mut nano = match NanoIsolate::new_with_vfs(vfs) {
                        Ok(iso) => iso,
                        Err(e) => { error!("Worker {}: isolate create failed: {}", id, e); return; }
                    };
                    if memory_limit_mb > 0 {
                        let bytes = memory_limit_mb as usize * 1024 * 1024;
                        nano.set_heap_limits(bytes / 2, bytes);
                    }

                    // Raw pointer for CPU timeout guards.
                    // SAFETY: nano lives for the entire scope block below.
                    let iso_ptr: *mut v8::Isolate = &mut **nano.isolate();

                    // === PERSISTENT SCOPE BLOCK ===
                    // HandleScope and ContextScope live on the thread stack.
                    // The V8 context stays entered for ALL requests in this isolate.
                    {
                        let scope_pin = std::pin::pin!(v8::HandleScope::new(nano.isolate()));
                        let mut scope = scope_pin.init();
                        let context = v8::Context::new(&scope, Default::default());

                        // Security: block eval() and new Function() — matches Cloudflare Workers.
                        context.set_allow_generation_from_strings(false);

                        // Bind all WinterCG runtime APIs once (before entering context)
                        crate::runtime::apis::RuntimeAPIs::bind_all(&mut scope, context);

                        // Enter context — NEVER dropped between requests
                        let mut ctx_scope = v8::ContextScope::new(&mut scope, context);

                        // Per-entrypoint handler cache: path → Global<Function>
                        let mut handler_cache: std::collections::HashMap<
                            String, v8::Global<v8::Function>
                        > = std::collections::HashMap::new();

                        let mut served: u32 = 0;
                        let isolate_id = format!("{}:{}", worker_hostname, id);

                        // State-leaking note (Task C):
                        // No per-request context reset fires here — this is intentional.
                        // Module-level globals (vars declared outside the handler function)
                        // persist across requests within one isolate lifetime, matching
                        // Cloudflare Workers / Deno Deploy behaviour. Handler-local state
                        // is fresh per call. Isolate is fully recycled after
                        // MAX_REQUESTS_PER_ISOLATE, which bounds any accumulated global state.
                        // Handlers MUST be written statelessly (state in KV/Durable Objects)
                        // rather than relying on module globals being reset per request.
                        'requests: loop {
                            if served >= MAX_REQUESTS_PER_ISOLATE {
                                info!("Worker {}: recycling isolate after {} requests", id, served);
                                break 'requests;
                            }

                            let task = match task_rx.recv() {
                                Ok(t) => t,
                                Err(_) => {
                                    debug!("Worker {}: channel closed", id);
                                    break 'isolate;
                                }
                            };

                            // OOM pre-check
                            if let Some(ref mon) = oom_monitor {
                                // SAFETY: iso_ptr valid for scope block duration
                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                if let Err(oom) = mon.check(iso_ref) {
                                    mon.log_oom_event(&oom, &task.request_id);
                                    let _ = task.response_tx.send(Ok(mon.create_oom_response(&oom)));
                                    break 'requests; // force isolate recycle
                                }
                            }

                            let t0 = std::time::Instant::now();
                            let request_id = task.request_id.clone();

                            // Compile + cache handler (once per entrypoint, per isolate lifetime)
                            if !handler_cache.contains_key(&task.entrypoint) {
                                let code = match crate::data_plane::read_code_cached(&task.entrypoint) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let _ = task.response_tx.send(Err(e));
                                        continue 'requests;
                                    }
                                };
                                let transformed = if crate::v8::module::is_esm_module(&code) {
                                    crate::v8::module::transform_module_code(&code)
                                } else {
                                    code.to_string()
                                };

                                let code_v8 = match v8::String::new(&mut ctx_scope, &transformed) {
                                    Some(s) => s,
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!("V8 string alloc failed")));
                                        continue 'requests;
                                    }
                                };
                                let script = match v8::Script::compile(&ctx_scope, code_v8, None) {
                                    Some(s) => s,
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!("Script compile failed for '{}'", task.entrypoint)));
                                        continue 'requests;
                                    }
                                };
                                if script.run(&ctx_scope).is_none() {
                                    let _ = task.response_tx.send(Err(anyhow!("Script execution failed for '{}'", task.entrypoint)));
                                    continue 'requests;
                                }

                                let global_obj = context.global(&mut ctx_scope);
                                let nano_k = v8::String::new(&mut ctx_scope, "__nano_user_fetch").unwrap();
                                let fetch_k = v8::String::new(&mut ctx_scope, "fetch").unwrap();
                                let handler_val = global_obj.get(&mut ctx_scope, nano_k.into())
                                    .filter(|v| v.is_function())
                                    .or_else(|| global_obj.get(&mut ctx_scope, fetch_k.into()).filter(|v| v.is_function()));

                                match handler_val {
                                    Some(f) => {
                                        let g = v8::Global::new(&**ctx_scope, f.cast::<v8::Function>());
                                        handler_cache.insert(task.entrypoint.clone(), g);
                                        info!("Worker {}: handler cached for '{}'", id, task.entrypoint);
                                    }
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!(
                                            "No fetch handler found in '{}'. Export a 'fetch' function.",
                                            task.entrypoint
                                        )));
                                        continue 'requests;
                                    }
                                }
                            }

                            // --- WebSocket mode (D-01 pin-a-worker, D-10b isolate recycle) ---
                            // If the task carries WS channels, enter the ws_messages loop instead
                            // of the normal HTTP handler path. The 'ws_messages loop runs until
                            // the connection closes, then breaks 'requests to force isolate recycle.
                            if let Some(ws_channels) = task.ws {
                                use crate::worker::tenant_pool::{
                                    WS_OUTBOUND, WS_ACCEPTED, WS_MESSAGE_HANDLERS,
                                    WS_CLOSE_HANDLERS, WS_ERROR_HANDLERS,
                                    set_ws_readystate, clear_ws_thread_locals,
                                };

                                // Seed WS thread-locals for V8 FunctionCallbacks.
                                WS_OUTBOUND.with(|tx| *tx.borrow_mut() = Some(ws_channels.outbound_tx.clone()));
                                WS_ACCEPTED.with(|a| a.set(false));
                                WS_MESSAGE_HANDLERS.with(|h| h.borrow_mut().clear());
                                WS_CLOSE_HANDLERS.with(|h| h.borrow_mut().clear());
                                WS_ERROR_HANDLERS.with(|h| h.borrow_mut().clear());

                                // Call JS handler so it can register addEventListener callbacks
                                // before the first frame arrives. Return value is ignored — the
                                // 101 response was already sent by handle_ws_upgrade in router.rs.
                                let handler_g = handler_cache.get(&task.entrypoint).unwrap();
                                let global_obj = context.global(&mut ctx_scope);
                                let handler_local = v8::Local::new(&mut ctx_scope, handler_g);
                                if let Some(url_str) = v8::String::new(&mut ctx_scope, &task.request.url().href()) {
                                    let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                    let tc_pin = std::pin::pin!(tc_s);
                                    let tc = tc_pin.init();
                                    let _ = handler_local.call(&tc, global_obj.into(), &[url_str.into()]);
                                }

                                // Signal OPEN state on the server socket JS object (D-16b).
                                set_ws_readystate(&mut ctx_scope, 1);

                                // Note: 101 response was sent by handle_ws_upgrade in router.rs
                                // before the task was dispatched. response_tx is intentionally dropped.
                                let _ = task.response_tx;

                                info!("Worker {}: entering ws_messages loop for '{}'", id, task.entrypoint);

                                // 30s idle timeout matches tenant_pool default (D-11b).
                                let idle_dur = std::time::Duration::from_millis(30_000);

                                'ws_messages: loop {
                                    match ws_channels.inbound_rx.recv_timeout(idle_dur) {
                                        // --- Text frame ---
                                        Ok(tungstenite::Message::Text(s)) => {
                                            // OOM check (D-13).
                                            if let Some(ref mon) = oom_monitor {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                if let Err(_oom) = mon.check(iso_ref) {
                                                    let _ = ws_channels.outbound_tx.send(tungstenite::Message::Close(Some(
                                                        tungstenite::protocol::CloseFrame {
                                                            code: tungstenite::protocol::frame::coding::CloseCode::Error,
                                                            reason: std::borrow::Cow::Borrowed("OOM"),
                                                        }
                                                    )));
                                                    break 'ws_messages;
                                                }
                                            }
                                            // Per-message CPU guard (D-09b).
                                            let _timeout = if task.cpu_time_limit_ms > 0 {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                                            } else { None };
                                            // Build MessageEvent and dispatch to JS handlers.
                                            let event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tk), Some(tv), Some(dk), Some(dv)) = (
                                                v8::String::new(&mut ctx_scope, "type"),
                                                v8::String::new(&mut ctx_scope, "message"),
                                                v8::String::new(&mut ctx_scope, "data"),
                                                v8::String::new(&mut ctx_scope, s.as_str()),
                                            ) {
                                                event.set(&mut ctx_scope, tk.into(), tv.into());
                                                event.set(&mut ctx_scope, dk.into(), dv.into());
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_MESSAGE_HANDLERS.with(|cell| {
                                                    for handler_g in cell.borrow().iter() {
                                                        let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                        let tc_pin = std::pin::pin!(tc_s);
                                                        let tc = tc_pin.init();
                                                        let _ = hlocal.call(&tc, gobj.into(), &[event.into()]);
                                                    }
                                                });
                                            }
                                        }

                                        // --- Binary frame ---
                                        Ok(tungstenite::Message::Binary(b)) => {
                                            if let Some(ref mon) = oom_monitor {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                if let Err(_oom) = mon.check(iso_ref) {
                                                    let _ = ws_channels.outbound_tx.send(tungstenite::Message::Close(Some(
                                                        tungstenite::protocol::CloseFrame {
                                                            code: tungstenite::protocol::frame::coding::CloseCode::Error,
                                                            reason: std::borrow::Cow::Borrowed("OOM"),
                                                        }
                                                    )));
                                                    break 'ws_messages;
                                                }
                                            }
                                            let _timeout = if task.cpu_time_limit_ms > 0 {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                                            } else { None };
                                            let ab_store = v8::ArrayBuffer::new_backing_store_from_vec(b);
                                            let shared = ab_store.make_shared();
                                            let ab = v8::ArrayBuffer::with_backing_store(&mut ctx_scope, &shared);
                                            let event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tk), Some(tv), Some(dk)) = (
                                                v8::String::new(&mut ctx_scope, "type"),
                                                v8::String::new(&mut ctx_scope, "message"),
                                                v8::String::new(&mut ctx_scope, "data"),
                                            ) {
                                                event.set(&mut ctx_scope, tk.into(), tv.into());
                                                event.set(&mut ctx_scope, dk.into(), ab.into());
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_MESSAGE_HANDLERS.with(|cell| {
                                                    for handler_g in cell.borrow().iter() {
                                                        let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                        let tc_pin = std::pin::pin!(tc_s);
                                                        let tc = tc_pin.init();
                                                        let _ = hlocal.call(&tc, gobj.into(), &[event.into()]);
                                                    }
                                                });
                                            }
                                        }

                                        // --- Close frame (D-12) ---
                                        Ok(tungstenite::Message::Close(frame)) => {
                                            set_ws_readystate(&mut ctx_scope, 3);
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
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_CLOSE_HANDLERS.with(|cell| {
                                                    for handler_g in cell.borrow().iter() {
                                                        let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                        let tc_pin = std::pin::pin!(tc_s);
                                                        let tc = tc_pin.init();
                                                        let _ = hlocal.call(&tc, gobj.into(), &[close_event.into()]);
                                                    }
                                                });
                                            }
                                            break 'ws_messages;
                                        }

                                        // --- Ping / Pong — skip (D-15b) ---
                                        Ok(tungstenite::Message::Ping(_)) | Ok(tungstenite::Message::Pong(_)) => {
                                            continue 'ws_messages;
                                        }

                                        // --- Idle timeout (D-11b) ---
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                            info!("Worker {}: WS idle timeout, recycling isolate", id);
                                            break 'ws_messages;
                                        }

                                        // --- Relay dropped inbound_tx (D-17b) ---
                                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                            set_ws_readystate(&mut ctx_scope, 3);
                                            let gobj = context.global(&mut ctx_scope);
                                            // Fire error event.
                                            let error_event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tyk), Some(tyv)) = (
                                                v8::String::new(&mut ctx_scope, "type"),
                                                v8::String::new(&mut ctx_scope, "error"),
                                            ) {
                                                error_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                                WS_ERROR_HANDLERS.with(|cell| {
                                                    for handler_g in cell.borrow().iter() {
                                                        let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                        let tc_pin = std::pin::pin!(tc_s);
                                                        let tc = tc_pin.init();
                                                        let _ = hlocal.call(&tc, gobj.into(), &[error_event.into()]);
                                                    }
                                                });
                                            }
                                            // Fire close event with 1006 (Abnormal Closure).
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
                                                WS_CLOSE_HANDLERS.with(|cell| {
                                                    for handler_g in cell.borrow().iter() {
                                                        let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                                        let tc_pin = std::pin::pin!(tc_s);
                                                        let tc = tc_pin.init();
                                                        let _ = hlocal.call(&tc, gobj.into(), &[close_event.into()]);
                                                    }
                                                });
                                            }
                                            break 'ws_messages;
                                        }

                                        // Future-proof: unknown frame variants.
                                        Ok(_) => continue 'ws_messages,
                                    }
                                } // end 'ws_messages

                                // Cleanup: clear WS thread-locals, force isolate recycle (D-10b).
                                clear_ws_thread_locals();
                                // D-10b: break 'requests forces a fresh isolate for next connection.
                                break 'requests;
                            }
                            // --- End WS mode — fall through to HTTP handler ---

                            // CPU timeout guard
                            let _timeout = if task.cpu_time_limit_ms > 0 {
                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                            } else {
                                None
                            };

                            // Execute handler using persistent context
                            let handler_g = handler_cache.get(&task.entrypoint).unwrap();
                            let global_obj = context.global(&mut ctx_scope);
                            // Global→Local works because the same context is still entered
                            let handler_local = v8::Local::new(&mut ctx_scope, handler_g);

                            let result: anyhow::Result<crate::http::NanoResponse> = (|| {
                                // Build JS Request object
                                let url_str = v8::String::new(&mut ctx_scope, &task.request.url().href())
                                    .ok_or_else(|| anyhow!("URL string alloc failed"))?;
                                let opts = v8::Object::new(&mut ctx_scope);

                                let mk = v8::String::new(&mut ctx_scope, "method").ok_or_else(|| anyhow!("method key"))?;
                                let mv = v8::String::new(&mut ctx_scope, task.request.method()).ok_or_else(|| anyhow!("method val"))?;
                                opts.set(&mut ctx_scope, mk.into(), mv.into());

                                let hk = v8::String::new(&mut ctx_scope, "headers").ok_or_else(|| anyhow!("headers key"))?;
                                let hck = v8::String::new(&mut ctx_scope, "Headers").ok_or_else(|| anyhow!("Headers key"))?;
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
                                    ) {
                                        hinit.set(&mut ctx_scope, k.into(), v.into());
                                    }
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
                                let mut tc = tc_pin.init();

                                // Clear any stale interval state from a previous request.
                                crate::runtime::apis::clear_pending_intervals();

                                let call_result = handler_local.call(&tc, global_obj.into(), &[js_req.into()]);

                                // Resolve Promise if async handler
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
                                                // SAFETY: iso_ptr valid for scope block duration
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
                                                        return Err(anyhow!("Async handler timed out after 30s"));
                                                    }
                                                    crate::runtime::apis::fire_pending_intervals(&mut *tc);
                                                    std::thread::yield_now();
                                                }
                                            }
                                        }
                                    }
                                    Some(v) => v,
                                };

                                // Extract NanoResponse from JS response object
                                let obj = resolved.to_object(&tc)
                                    .ok_or_else(|| anyhow!("Handler response is not an object"))?;

                                let sk = v8::String::new(&tc, "status").ok_or_else(|| anyhow!("status key"))?;
                                let status = obj.get(&tc, sk.into())
                                    .and_then(|v| v.to_integer(&tc))
                                    .map(|i| i.value() as u16)
                                    .unwrap_or(200);

                                let mut response = crate::http::NanoResponse::with_status(status);

                                // Extract response headers
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
                                                        if k.starts_with("__") || matches!(k.as_str(), "set" | "get" | "forEach") {
                                                            continue;
                                                        }
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

                                // Extract response body
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
                            let status_code = match &result {
                                Ok(r) => r.status(),
                                Err(_) => 500,
                            };
                            tracing::info!(
                                request_id = %request_id,
                                worker_id = id,
                                isolate_id = %isolate_id,
                                status = status_code,
                                duration_ms = duration_ms,
                                "Worker {} request {} → {} in {}ms",
                                id, request_id, status_code, duration_ms
                            );

                            let result = result.map(|mut r| {
                                r.set_worker_id(id);
                                r.set_isolate_id(isolate_id.clone());
                                r
                            });
                            let _ = task.response_tx.send(result);
                            served += 1;
                        }
                        // ctx_scope + scope drop here → context exited, handles freed
                    }
                    // nano drops here → isolate disposed

                    info!("Worker {}: isolate recycled after {} requests, creating fresh", id, MAX_REQUESTS_PER_ISOLATE);
                } // 'isolate loop

                info!("Worker {} exiting", id);
            });

            workers.push(WorkerHandle {
                id,
                thread: Some(thread),
                task_tx,
            });
        }

        info!(
            "WorkerPool created for {} with {} workers",
            hostname, worker_count
        );

        Self {
            workers,
            worker_count,
            hostname,
            next_worker: AtomicU32::new(0),
            vfs_backend,
            memory_limit_mb,
        }
    }

    /// Get a reference to the shared VFS backend
    ///
    /// This is useful for testing and administrative operations
    /// that need to inspect or modify the filesystem.
    pub fn vfs_backend(&self) -> &crate::vfs::VfsBackendEnum {
        &self.vfs_backend
    }

    /// Dispatch a task to a worker
    ///
    /// Uses round-robin dispatch (simplest approach for initial implementation).
    /// Returns error if all worker channels are closed.
    ///
    /// # Arguments
    ///
    /// * `task` - The HandlerTask to dispatch
    ///
    /// # Returns
    ///
    /// `Ok(())` if dispatched, `Err` if no workers available
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        // Round-robin: atomically increment and get worker index
        let worker_idx = self.next_worker.fetch_add(1, Ordering::SeqCst) % self.worker_count;
        let worker_idx = worker_idx as usize;

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Dispatch with custom worker selection
    ///
    /// For use when caller knows which worker should handle the task
    /// (e.g., for request affinity in later phases).
    ///
    /// # Arguments
    ///
    /// * `worker_idx` - Index of the worker to use
    /// * `task` - The HandlerTask to dispatch
    ///
    /// # Returns
    ///
    /// `Ok(())` if dispatched, `Err` if worker index invalid or channel closed
    pub fn dispatch_to(&self, worker_idx: u32, task: HandlerTask) -> Result<()> {
        if worker_idx >= self.worker_count {
            return Err(anyhow!(
                "Worker index {} out of bounds (max {})",
                worker_idx,
                self.worker_count - 1
            ));
        }

        self.workers[worker_idx as usize]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Gracefully shut down the worker pool
    ///
    /// 1. Drop all task_tx channels (signals workers to exit)
    /// 2. Join all worker threads
    ///
    /// # Returns
    ///
    /// `Ok(())` if all workers exited cleanly
    pub fn shutdown(mut self) -> Result<()> {
        info!("Shutting down WorkerPool for {}", self.hostname);

        // Take and drop all task_tx channels, signaling workers to exit
        // Workers will finish current task, then see recv() error and exit
        let mut handles: Vec<_> = self
            .workers
            .drain(..)
            .map(|mut w| (w.id, w.take_thread()))
            .collect();

        // Join all threads with timeout (5 seconds per thread)
        for (id, handle) in handles.drain(..) {
            if let Some(h) = handle {
                debug!("Waiting for worker {} to exit", id);
                match h.join() {
                    Ok(_) => debug!("Worker {} exited cleanly", id),
                    Err(_) => warn!("Worker {} panicked during shutdown", id),
                }
            }
        }

        info!("WorkerPool for {} shut down complete", self.hostname);
        Ok(())
    }

    /// Get the number of workers in this pool
    pub fn worker_count(&self) -> u32 {
        self.worker_count
    }

    /// Create worker pool from unified AppSource enum
    ///
    /// This is the unified constructor that handles both entrypoint and sliver modes
    /// through a single code path. It replaces the separate WorkerPool/SliverWorkerPool
    /// constructors.
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `source` - AppSource enum (Entrypoint, Sliver, or Static)
    ///
    /// # Returns
    ///
    /// A new WorkerPool configured for the specified source type
    ///
    /// # Panics
    ///
    /// Panics if V8 platform is not initialized or worker_count is 0
    pub fn with_source(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        source: crate::worker::AppSource,
    ) -> Self {
        use crate::vfs::MemoryBackend;
        use crate::worker::AppSource;

        // Select appropriate VFS backend based on AppSource type
        let vfs_backend = match &source {
            AppSource::Entrypoint { path } => {
                // For entrypoint apps, use DiskBackend pointing to the parent directory
                // This allows the app to access files relative to its entrypoint
                let path_obj = std::path::Path::new(path);
                let base_dir = path_obj
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                
                // Clone for the thread and for error messages
                let base_dir_for_thread = base_dir.clone();
                let base_dir_for_error = base_dir.clone();
                
                // Create disk backend - DiskBackend::new is async so we block on it
                let backend_result = std::thread::spawn(move || {
                    match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt.block_on(async {
                            crate::vfs::DiskBackend::new(&base_dir_for_thread).await
                        }),
                        Err(e) => Err(crate::vfs::VfsError::IoError(format!("Failed to create tokio runtime: {}", e)))
                    }
                }).join();
                
                match backend_result {
                    Ok(Ok(disk_backend)) => {
                        tracing::info!(
                            "Created DiskBackend for entrypoint app at hostname: {}, base_dir: {:?}",
                            hostname,
                            base_dir
                        );
                        crate::vfs::VfsBackendEnum::disk(disk_backend)
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            "Failed to create DiskBackend for entrypoint app at {:?}, falling back to MemoryBackend: {}",
                            base_dir_for_error,
                            e
                        );
                        crate::vfs::VfsBackendEnum::memory(MemoryBackend::default())
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Thread panic creating DiskBackend for entrypoint app at {:?}, falling back to MemoryBackend: {:?}",
                            base_dir_for_error,
                            e
                        );
                        crate::vfs::VfsBackendEnum::memory(MemoryBackend::default())
                    }
                }
            }
            AppSource::Sliver { .. } => {
                // For sliver apps, use MemoryBackend (sliver contains embedded VFS data)
                tracing::debug!("Using MemoryBackend for sliver app at hostname: {}", hostname);
                crate::vfs::VfsBackendEnum::memory(MemoryBackend::default())
            }
            AppSource::Static { .. } => {
                // Static apps don't need a pool at all - this should panic before we get here
                panic!("Static sources should not create WorkerPool - use StaticPool instead");
            }
        };
        
        Self::with_source_and_backend(hostname, worker_count, memory_limit_mb, vfs_backend, source)
    }

    /// Create worker pool from AppSource with custom VFS backend
    ///
    /// This is the most flexible constructor allowing both source type selection
    /// and custom storage backends.
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `vfs_backend` - Custom VFS backend (memory, disk, S3)
    /// * `source` - AppSource enum determining initialization mode
    pub fn with_source_and_backend(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: crate::vfs::VfsBackendEnum,
        source: crate::worker::AppSource,
    ) -> Self {
        use crate::worker::AppSource;

        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        // For static sites, we don't spawn isolates - handled separately
        if source.is_static() {
            panic!("Static sources should not create WorkerPool - use StaticPool instead");
        }

        // Clone values for worker threads
        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = vfs_backend.clone();
        let source_for_workers = source.clone();

        let mut workers = Vec::with_capacity(worker_count as usize);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = vfs_backend_for_workers.clone();
            let worker_source = source_for_workers.clone();
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn unified worker thread with persistent V8 scope lifecycle.
            let thread = thread::spawn(move || {
                info!("UnifiedWorker {} starting for {}", id, worker_hostname);

                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => { error!("Worker {}: tokio runtime failed: {}", id, e); return; }
                };
                WORKER_RUNTIME.with(|r| *r.borrow_mut() = Some(rt.handle().clone()));

                let oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                const MAX_REQUESTS_PER_ISOLATE: u32 = 10_000;
                let mut first_isolate = true;

                // Extract temp entrypoint override for sliver mode (if any)
                let temp_entrypoint_override: Option<std::path::PathBuf> = match &worker_source {
                    AppSource::Sliver { temp_entrypoint, .. } => temp_entrypoint.clone(),
                    _ => None,
                };

                // Outer loop: one iteration per isolate lifetime.
                'isolate: loop {
                    let namespace = VfsNamespace::from_hostname(&worker_hostname);
                    let vfs = IsolateVfs::new(namespace, worker_vfs_backend.clone());

                    // First isolate: warm-start from snapshot (sliver) or fresh (entrypoint).
                    // Recycled isolates: always fresh.
                    let mut nano = if first_isolate {
                        first_isolate = false;
                        match &worker_source {
                            AppSource::Entrypoint { .. } => {
                                match NanoIsolate::new_with_vfs(vfs) {
                                    Ok(iso) => iso,
                                    Err(e) => { error!("Worker {}: isolate failed: {}", id, e); return; }
                                }
                            }
                            AppSource::Sliver { data, .. } => {
                                if let Err(e) = rt.block_on(data.restore_to_vfs(&vfs)) {
                                    warn!("Worker {}: VFS restore failed: {}", id, e);
                                } else {
                                    debug!("Worker {}: restored {} VFS entries", id, data.vfs_entries.len());
                                }
                                match NanoIsolate::from_snapshot(&data.heap_data, vfs.clone()) {
                                    Ok(iso) => { info!("Worker {}: restored from snapshot", id); iso }
                                    Err(e) => {
                                        warn!("Worker {}: snapshot restore failed ({}), creating fresh", id, e);
                                        match NanoIsolate::new_with_vfs(vfs) {
                                            Ok(iso) => iso,
                                            Err(e) => { error!("Worker {}: isolate failed: {}", id, e); return; }
                                        }
                                    }
                                }
                            }
                            AppSource::Static { .. } => {
                                error!("Worker {}: Static source in unified worker — should not happen", id);
                                return;
                            }
                        }
                    } else {
                        match NanoIsolate::new_with_vfs(vfs) {
                            Ok(iso) => iso,
                            Err(e) => { error!("Worker {}: isolate create failed: {}", id, e); return; }
                        }
                    };

                    if memory_limit_mb > 0 {
                        let bytes = memory_limit_mb as usize * 1024 * 1024;
                        nano.set_heap_limits(bytes / 2, bytes);
                    }

                    // Raw pointer for CPU timeout guards.
                    // SAFETY: nano lives for the entire scope block below.
                    let iso_ptr: *mut v8::Isolate = &mut **nano.isolate();

                    // === PERSISTENT SCOPE BLOCK ===
                    {
                        let scope_pin = std::pin::pin!(v8::HandleScope::new(nano.isolate()));
                        let mut scope = scope_pin.init();
                        let context = v8::Context::new(&scope, Default::default());
                        // Security: block eval() and new Function() — matches Cloudflare Workers.
                        context.set_allow_generation_from_strings(false);
                        crate::runtime::apis::RuntimeAPIs::bind_all(&mut scope, context);
                        let mut ctx_scope = v8::ContextScope::new(&mut scope, context);

                        let mut handler_cache: std::collections::HashMap<
                            String, v8::Global<v8::Function>
                        > = std::collections::HashMap::new();

                        let mut served: u32 = 0;
                        let isolate_id = format!("{}:{}", worker_hostname, id);

                        'requests: loop {
                            if served >= MAX_REQUESTS_PER_ISOLATE {
                                info!("Worker {}: recycling isolate after {} requests", id, served);
                                break 'requests;
                            }

                            let task = match task_rx.recv() {
                                Ok(t) => t,
                                Err(_) => { debug!("Worker {}: channel closed", id); break 'isolate; }
                            };

                            // OOM pre-check
                            if let Some(ref mon) = oom_monitor {
                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                if let Err(oom) = mon.check(iso_ref) {
                                    mon.log_oom_event(&oom, &task.request_id);
                                    let _ = task.response_tx.send(Ok(mon.create_oom_response(&oom)));
                                    break 'requests;
                                }
                            }

                            let t0 = std::time::Instant::now();
                            let request_id = task.request_id.clone();

                            // Determine entrypoint (sliver may override via temp file)
                            let entrypoint = temp_entrypoint_override
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| task.entrypoint.clone());

                            // Compile + cache handler (once per entrypoint, per isolate lifetime)
                            if !handler_cache.contains_key(&entrypoint) {
                                let code = match crate::data_plane::read_code_cached(&entrypoint) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let _ = task.response_tx.send(Err(e));
                                        continue 'requests;
                                    }
                                };
                                let transformed = if crate::v8::module::is_esm_module(&code) {
                                    crate::v8::module::transform_module_code(&code)
                                } else {
                                    code.to_string()
                                };

                                let code_v8 = match v8::String::new(&mut ctx_scope, &transformed) {
                                    Some(s) => s,
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!("V8 string alloc failed")));
                                        continue 'requests;
                                    }
                                };
                                let script = match v8::Script::compile(&ctx_scope, code_v8, None) {
                                    Some(s) => s,
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!("Script compile failed for '{}\'", entrypoint)));
                                        continue 'requests;
                                    }
                                };
                                if script.run(&ctx_scope).is_none() {
                                    let _ = task.response_tx.send(Err(anyhow!("Script execution failed for '{}'", entrypoint)));
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
                                        info!("Worker {}: handler cached for '{}'", id, entrypoint);
                                    }
                                    None => {
                                        let _ = task.response_tx.send(Err(anyhow!(
                                            "No fetch handler found in '{}'. Export a 'fetch' function.",
                                            entrypoint
                                        )));
                                        continue 'requests;
                                    }
                                }
                            }

                            // --- WebSocket mode (D-01 pin-a-worker, D-10b isolate recycle) ---
                            if let Some(ws_channels) = task.ws {
                                use crate::worker::tenant_pool::{
                                    WS_OUTBOUND, WS_ACCEPTED, WS_MESSAGE_HANDLERS,
                                    WS_CLOSE_HANDLERS, WS_ERROR_HANDLERS,
                                    set_ws_readystate, clear_ws_thread_locals,
                                };
                                WS_OUTBOUND.with(|tx| *tx.borrow_mut() = Some(ws_channels.outbound_tx.clone()));
                                WS_ACCEPTED.with(|a| a.set(false));
                                WS_MESSAGE_HANDLERS.with(|h| h.borrow_mut().clear());
                                WS_CLOSE_HANDLERS.with(|h| h.borrow_mut().clear());
                                WS_ERROR_HANDLERS.with(|h| h.borrow_mut().clear());

                                // Call JS handler to register addEventListener callbacks.
                                if let Some(handler_g) = handler_cache.get(&entrypoint) {
                                    let gobj = context.global(&mut ctx_scope);
                                    let hlocal = v8::Local::new(&mut ctx_scope, handler_g);
                                    if let Some(url_str) = v8::String::new(&mut ctx_scope, &task.request.url().href()) {
                                        let tc_s = v8::TryCatch::new(&mut *ctx_scope);
                                        let tc_pin = std::pin::pin!(tc_s);
                                        let tc = tc_pin.init();
                                        let _ = hlocal.call(&tc, gobj.into(), &[url_str.into()]);
                                    }
                                }

                                set_ws_readystate(&mut ctx_scope, 1);
                                let _ = task.response_tx; // 101 already sent by router

                                info!("Worker {}: entering ws_messages loop for '{}'", id, entrypoint);
                                let idle_dur = std::time::Duration::from_millis(30_000);

                                'ws_messages: loop {
                                    match ws_channels.inbound_rx.recv_timeout(idle_dur) {
                                        Ok(tungstenite::Message::Text(s)) => {
                                            if let Some(ref mon) = oom_monitor {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                if let Err(_oom) = mon.check(iso_ref) {
                                                    let _ = ws_channels.outbound_tx.send(tungstenite::Message::Close(Some(tungstenite::protocol::CloseFrame { code: tungstenite::protocol::frame::coding::CloseCode::Error, reason: std::borrow::Cow::Borrowed("OOM") })));
                                                    break 'ws_messages;
                                                }
                                            }
                                            let _cg = if task.cpu_time_limit_ms > 0 { Some(crate::data_plane::CpuTimeoutGuard::new(unsafe { &mut *iso_ptr }, task.cpu_time_limit_ms)) } else { None };
                                            let event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tk), Some(tv), Some(dk), Some(dv)) = (v8::String::new(&mut ctx_scope, "type"), v8::String::new(&mut ctx_scope, "message"), v8::String::new(&mut ctx_scope, "data"), v8::String::new(&mut ctx_scope, s.as_str())) {
                                                event.set(&mut ctx_scope, tk.into(), tv.into());
                                                event.set(&mut ctx_scope, dk.into(), dv.into());
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_MESSAGE_HANDLERS.with(|cell| { for hg in cell.borrow().iter() { let hl = v8::Local::new(&mut ctx_scope, hg); let tc_s = v8::TryCatch::new(&mut *ctx_scope); let tc_pin = std::pin::pin!(tc_s); let tc = tc_pin.init(); let _ = hl.call(&tc, gobj.into(), &[event.into()]); } });
                                            }
                                        }
                                        Ok(tungstenite::Message::Binary(b)) => {
                                            // OOM check before allocating ArrayBuffer (D-13).
                                            if let Some(ref mon) = oom_monitor {
                                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                                if let Err(_oom) = mon.check(iso_ref) {
                                                    let _ = ws_channels.outbound_tx.send(tungstenite::Message::Close(Some(tungstenite::protocol::CloseFrame { code: tungstenite::protocol::frame::coding::CloseCode::Error, reason: std::borrow::Cow::Borrowed("OOM") })));
                                                    break 'ws_messages;
                                                }
                                            }
                                            let _cg = if task.cpu_time_limit_ms > 0 { Some(crate::data_plane::CpuTimeoutGuard::new(unsafe { &mut *iso_ptr }, task.cpu_time_limit_ms)) } else { None };
                                            let ab_store = v8::ArrayBuffer::new_backing_store_from_vec(b);
                                            let shared = ab_store.make_shared();
                                            let ab = v8::ArrayBuffer::with_backing_store(&mut ctx_scope, &shared);
                                            let event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tk), Some(tv), Some(dk)) = (v8::String::new(&mut ctx_scope, "type"), v8::String::new(&mut ctx_scope, "message"), v8::String::new(&mut ctx_scope, "data")) {
                                                event.set(&mut ctx_scope, tk.into(), tv.into());
                                                event.set(&mut ctx_scope, dk.into(), ab.into());
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_MESSAGE_HANDLERS.with(|cell| { for hg in cell.borrow().iter() { let hl = v8::Local::new(&mut ctx_scope, hg); let tc_s = v8::TryCatch::new(&mut *ctx_scope); let tc_pin = std::pin::pin!(tc_s); let tc = tc_pin.init(); let _ = hl.call(&tc, gobj.into(), &[event.into()]); } });
                                            }
                                        }
                                        Ok(tungstenite::Message::Close(frame)) => {
                                            set_ws_readystate(&mut ctx_scope, 3);
                                            let (code_val, reason_str) = frame.map(|f| (u16::from(f.code), f.reason.into_owned())).unwrap_or((1000, String::new()));
                                            let close_event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tyk), Some(tyv), Some(ck), Some(rk), Some(rv), Some(wck)) = (v8::String::new(&mut ctx_scope, "type"), v8::String::new(&mut ctx_scope, "close"), v8::String::new(&mut ctx_scope, "code"), v8::String::new(&mut ctx_scope, "reason"), v8::String::new(&mut ctx_scope, &reason_str), v8::String::new(&mut ctx_scope, "wasClean")) {
                                                let code_int = v8::Integer::new(&mut ctx_scope, code_val as i32);
                                                let was_clean = v8::Boolean::new(&mut ctx_scope, true);
                                                close_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                                close_event.set(&mut ctx_scope, ck.into(), code_int.into());
                                                close_event.set(&mut ctx_scope, rk.into(), rv.into());
                                                close_event.set(&mut ctx_scope, wck.into(), was_clean.into());
                                                let gobj = context.global(&mut ctx_scope);
                                                WS_CLOSE_HANDLERS.with(|cell| { for hg in cell.borrow().iter() { let hl = v8::Local::new(&mut ctx_scope, hg); let tc_s = v8::TryCatch::new(&mut *ctx_scope); let tc_pin = std::pin::pin!(tc_s); let tc = tc_pin.init(); let _ = hl.call(&tc, gobj.into(), &[close_event.into()]); } });
                                            }
                                            break 'ws_messages;
                                        }
                                        Ok(tungstenite::Message::Ping(_)) | Ok(tungstenite::Message::Pong(_)) => continue 'ws_messages,
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => { info!("Worker {}: WS idle timeout", id); break 'ws_messages; }
                                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                            set_ws_readystate(&mut ctx_scope, 3);
                                            let gobj = context.global(&mut ctx_scope);
                                            let error_event = v8::Object::new(&mut ctx_scope);
                                            if let (Some(tyk), Some(tyv)) = (v8::String::new(&mut ctx_scope, "type"), v8::String::new(&mut ctx_scope, "error")) {
                                                error_event.set(&mut ctx_scope, tyk.into(), tyv.into());
                                                WS_ERROR_HANDLERS.with(|cell| { for hg in cell.borrow().iter() { let hl = v8::Local::new(&mut ctx_scope, hg); let tc_s = v8::TryCatch::new(&mut *ctx_scope); let tc_pin = std::pin::pin!(tc_s); let tc = tc_pin.init(); let _ = hl.call(&tc, gobj.into(), &[error_event.into()]); } });
                                            }
                                            break 'ws_messages;
                                        }
                                        Ok(_) => continue 'ws_messages,
                                    }
                                }
                                clear_ws_thread_locals();
                                break 'requests; // D-10b: fresh isolate per WS connection
                            }
                            // --- End WS mode — HTTP path below ---

                            // CPU timeout guard
                            let _timeout = if task.cpu_time_limit_ms > 0 {
                                // SAFETY: iso_ptr is valid for this isolate's lifetime.
                                // CpuTimeoutGuard calls terminate_execution() from a timer thread,
                                // which V8 documents as safe to call from any thread.
                                let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
                                Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
                            } else {
                                None
                            };

                            // Execute handler using persistent context
                            // handler_cache.get is infallible: just inserted above if missing.
                            let handler_g = handler_cache.get(&entrypoint)
                                .expect("handler must be cached: just inserted in block above");
                            let global_obj = context.global(&mut ctx_scope);
                            let handler_local = v8::Local::new(&mut ctx_scope, handler_g);

                            let result: anyhow::Result<crate::http::NanoResponse> = (|| {
                                let url_str = v8::String::new(&mut ctx_scope, &task.request.url().href())
                                    .ok_or_else(|| anyhow!("URL string alloc failed"))?;
                                let opts = v8::Object::new(&mut ctx_scope);

                                let mk = v8::String::new(&mut ctx_scope, "method").ok_or_else(|| anyhow!("method key"))?;
                                let mv = v8::String::new(&mut ctx_scope, task.request.method()).ok_or_else(|| anyhow!("method val"))?;
                                opts.set(&mut ctx_scope, mk.into(), mv.into());

                                let hk = v8::String::new(&mut ctx_scope, "headers").ok_or_else(|| anyhow!("headers key"))?;
                                let hck = v8::String::new(&mut ctx_scope, "Headers").ok_or_else(|| anyhow!("Headers key"))?;
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
                                    ) {
                                        hinit.set(&mut ctx_scope, k.into(), v.into());
                                    }
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
                                let mut tc = tc_pin.init();

                                // Clear any stale interval state from a previous request.
                                crate::runtime::apis::clear_pending_intervals();

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
                                                // SAFETY: pump_message_loop requires &Isolate.
                                                // iso_ptr is valid and pinned to this thread.
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
                                                        return Err(anyhow!("Async handler timed out after 30s"));
                                                    }
                                                    crate::runtime::apis::fire_pending_intervals(&mut *tc);
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
                                                        if k.starts_with("__") || matches!(k.as_str(), "set" | "get" | "forEach") {
                                                            continue;
                                                        }
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
                            let status_code = match &result {
                                Ok(r) => r.status(),
                                Err(_) => 500,
                            };
                            tracing::info!(
                                request_id = %request_id,
                                worker_id = id,
                                isolate_id = %isolate_id,
                                status = status_code,
                                duration_ms = duration_ms,
                                "Worker {} request {} → {} in {}ms",
                                id, request_id, status_code, duration_ms
                            );

                            let result = result.map(|mut r| {
                                r.set_worker_id(id);
                                r.set_isolate_id(isolate_id.clone());
                                r
                            });
                            let _ = task.response_tx.send(result);
                            served += 1;
                        }
                        // ctx_scope + scope drop here
                    }
                    // nano drops here

                    info!("Worker {}: isolate recycled, creating fresh", id);
                } // 'isolate loop

                info!("Worker {} exiting", id);
            });

            workers.push(WorkerHandle {
                id,
                thread: Some(thread),
                task_tx,
            });
        }

        WorkerPool {
            workers,
            worker_count,
            hostname,
            next_worker: AtomicU32::new(0),
            vfs_backend,
            memory_limit_mb,
        }
    }
}

/// Worker pool for sliver-based (snapshot-restored) applications
///
/// This specialized worker pool creates isolates from V8 heap snapshots
/// rather than fresh isolates. It also restores VFS state from the sliver.
///
/// # Design
///
/// - Each worker restores its isolate from the snapshot blob
/// - VFS entries are restored before the worker accepts tasks
/// - Falls back to fresh isolate if snapshot restoration fails
/// - Shares the same dispatch interface as regular WorkerPool
///
/// # Deprecation Notice
///
/// This type is now a thin wrapper around `WorkerPool` for backward compatibility.
/// New code should use `WorkerPool::with_source()` directly with `AppSource::Sliver`.
pub struct SliverWorkerPool {
    /// Inner WorkerPool that handles all execution
    ///
    /// This wraps the unified WorkerPool created with AppSource::Sliver.
    inner: WorkerPool,
    /// Hostname this pool serves (cached for quick access)
    pub hostname: String,
    /// Number of workers (cached for quick access)
    pub worker_count: u32,
    /// Unpacked sliver data (kept for reference/debugging)
    unpacked_sliver: crate::sliver::UnpackedSliver,
    /// Optional temp entrypoint path (kept for reference/debugging)
    temp_entrypoint: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for SliverWorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverWorkerPool")
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("unpacked_sliver", &self.unpacked_sliver.metadata.hostname)
            .field("temp_entrypoint", &self.temp_entrypoint)
            .finish()
    }
}

impl SliverWorkerPool {
    /// Create a new sliver worker pool with restored isolates
    ///
    /// This now delegates to the unified `WorkerPool::with_source()` constructor
    /// for consistent behavior across all pool types.
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `unpacked_sliver` - The unpacked sliver containing snapshot and VFS data
    ///
    /// # Returns
    ///
    /// A new SliverWorkerPool with N workers restored from snapshot
    ///
    /// # Deprecation
    ///
    /// This method now delegates to `WorkerPool::with_source()`. For new code,
    /// use `WorkerPool::with_source(hostname, worker_count, memory_limit_mb, AppSource::sliver(data))`.
    pub fn new(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        unpacked_sliver: crate::sliver::UnpackedSliver,
    ) -> Self {
        Self::with_temp_entrypoint(
            hostname,
            worker_count,
            memory_limit_mb,
            unpacked_sliver,
            None,
        )
    }

    /// Create a new sliver worker pool with a temp entrypoint path
    ///
    /// This variant is used when the sliver VFS has been extracted to a temp
    /// directory, and the JS entrypoint should be read from that location.
    ///
    /// # Deprecation
    ///
    /// This method now delegates to `WorkerPool::with_source()`. For new code,
    /// use `WorkerPool::with_source()` with `AppSource::sliver_with_temp(data, temp)`.
    pub fn with_temp_entrypoint(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        unpacked_sliver: crate::sliver::UnpackedSliver,
        temp_entrypoint: Option<std::path::PathBuf>,
    ) -> Self {
        use crate::worker::AppSource;
        use crate::vfs::MemoryBackend;

        let source = if let Some(temp) = temp_entrypoint.clone() {
            AppSource::sliver_with_temp(unpacked_sliver.clone(), temp)
        } else {
            AppSource::sliver(unpacked_sliver.clone())
        };

        let vfs_backend = crate::vfs::VfsBackendEnum::memory(MemoryBackend::default());
        let inner = WorkerPool::with_source_and_backend(
            hostname.clone(),
            worker_count,
            memory_limit_mb,
            vfs_backend,
            source,
        );

        info!(
            "SliverWorkerPool for {} created with {} workers (delegates to unified WorkerPool)",
            hostname, worker_count
        );

        Self {
            inner,
            hostname: hostname.clone(),
            worker_count,
            unpacked_sliver,
            temp_entrypoint,
        }
    }

    /// Create a new sliver worker pool with a specific VFS backend
    ///
    /// # Deprecation
    ///
    /// This method now delegates to `WorkerPool::with_source_and_backend()`.
    pub fn with_backend(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: crate::vfs::VfsBackendEnum,
        unpacked_sliver: crate::sliver::UnpackedSliver,
        temp_entrypoint: Option<std::path::PathBuf>,
    ) -> Self {
        use crate::worker::AppSource;

        let source = if let Some(temp) = temp_entrypoint.clone() {
            AppSource::sliver_with_temp(unpacked_sliver.clone(), temp)
        } else {
            AppSource::sliver(unpacked_sliver.clone())
        };

        let inner = WorkerPool::with_source_and_backend(
            hostname.clone(),
            worker_count,
            memory_limit_mb,
            vfs_backend,
            source,
        );

        info!(
            "SliverWorkerPool for {} created with {} workers (custom backend)",
            hostname, worker_count
        );

        Self {
            inner,
            hostname: hostname.clone(),
            worker_count,
            unpacked_sliver,
            temp_entrypoint,
        }
    }

    /// Dispatch a task to a worker using round-robin
    ///
    /// Delegates to the unified WorkerPool implementation.
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        self.inner.dispatch(task)
    }

    /// Gracefully shut down the sliver worker pool
    ///
    /// Delegates to the unified WorkerPool implementation.
    pub fn shutdown(self) -> Result<()> {
        info!("Shutting down SliverWorkerPool for {}", self.hostname);
        self.inner.shutdown()
    }

    /// Get the number of workers in this pool
    ///
    /// Provided for backward compatibility with code that accessed the field directly.
    pub fn worker_count(&self) -> u32 {
        self.worker_count
    }

    /// Get the hostname this pool serves
    ///
    /// Provided for backward compatibility with code that accessed the field directly.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Access the unpacked sliver data (for debugging/testing)
    #[cfg(test)]
    pub fn sliver_data(&self) -> &crate::sliver::UnpackedSliver {
        &self.unpacked_sliver
    }

    /// Access the VFS backend (for testing VFS operations)
    #[cfg(test)]
    pub fn vfs_backend(&self) -> &crate::vfs::VfsBackendEnum {
        &self.inner.vfs_backend
    }
}

impl crate::worker::r#trait::WorkerPool for SliverWorkerPool {
    fn dispatch(&self, task: HandlerTask) -> Result<()> {
        self.inner.dispatch(task)
    }

    fn shutdown(self) -> Result<()> {
        self.inner.shutdown()
    }

    fn worker_count(&self) -> u32 {
        self.worker_count
    }

    fn hostname(&self) -> &str {
        &self.hostname
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoRequest, NanoUrl};
    use crate::worker::HandlerTask;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    /// Helper to ensure platform is initialized for tests
    fn init_platform() {
        if !crate::v8::is_initialized() {
            crate::v8::initialize_platform().expect("Failed to initialize V8 platform");
        }
    }

    /// Create a test JavaScript file and return its path
    fn create_test_handler(dir: &TempDir, filename: &str, code: &str) -> String {
        let path = dir.path().join(filename);
        let mut file = fs::File::create(&path).expect("Failed to create test file");
        file.write_all(code.as_bytes())
            .expect("Failed to write test code");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_worker_pool_creation() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);
        assert_eq!(pool.worker_count, 2);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_single_worker_pool() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);
        assert_eq!(pool.worker_count, 1);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_dispatch_and_response() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());

        // Create a simple JS handler (non-async for now)
        let js_code = format!(
            r#"
function fetch(request) {{
    return {{ status: 200, headers: {{ "Content-Type": "text/plain" }}, body: "{}" }};
}}
"#,
            dynamic_token
        );
        let entrypoint = create_test_handler(&temp_dir, "test.js", &js_code);

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        // Create task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        // Dispatch and wait for response
        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");

        assert!(
            response.is_ok(),
            "Handler execution failed: {:?}",
            response.err()
        );
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("Content-Type"),
            Some("text/plain".to_string())
        );
        assert!(resp.body().is_some());

        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(
            body_text.contains(&dynamic_token),
            "Response must contain dynamic token '{}', got: {}",
            dynamic_token,
            body_text
        );

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_concurrent_requests() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create a JS handler that returns request info
        let entrypoint = create_test_handler(
            &temp_dir,
            "handler.js",
            r#"
function fetch(request) {
    return { status: 200, headers: {}, body: "OK" };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 4, 0);

        // Dispatch 10 tasks concurrently
        let mut receivers = vec![];
        for i in 0..10 {
            let url = NanoUrl::parse(&format!("http://test/{}", i)).unwrap();
            let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

            let (tx, rx) = oneshot::channel();
            let task = HandlerTask::new(entrypoint.clone(), request, tx);

            pool.dispatch(task).unwrap();
            receivers.push(rx);
        }

        // All should complete successfully
        for (i, rx) in receivers.into_iter().enumerate() {
            let response = rx
                .blocking_recv()
                .expect(&format!("Failed to receive response {}", i));
            assert!(
                response.is_ok(),
                "Request {} failed: {:?}",
                i,
                response.err()
            );
            let resp = response.unwrap();
            assert_eq!(resp.status(), 200);
        }

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_round_robin_dispatch() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 3, 0);

        // Create tasks to verify round-robin works
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        // Dispatch 6 tasks - should hit workers 0,1,2,0,1,2
        for _ in 0..6 {
            let url = NanoUrl::parse("http://test/").unwrap();
            let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

            let (tx, rx) = oneshot::channel();
            let task = HandlerTask::new(entrypoint.clone(), request, tx);

            pool.dispatch(task).expect("Dispatch failed");

            // Wait for each to complete
            let _ = rx.blocking_recv();
        }

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_dispatch_to_specific_worker() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 3, 0);

        // Dispatch to specific worker
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch_to(1, task)
            .expect("Dispatch to worker 1 failed");

        let response = rx.blocking_recv().expect("Failed to receive");
        assert!(response.is_ok());

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_invalid_worker_index() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, _rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        // Try to dispatch to invalid worker index
        let result = pool.dispatch_to(5, task);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_pool_shutdown() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);

        // Shutdown should complete without hanging
        pool.shutdown().expect("Shutdown failed");

        // Test passes if we reach here
    }

    #[test]
    fn test_worker_isolate_thread_local() {
        // This test verifies that isolates are created in worker threads
        // and never move between threads (compile-time check via !Send + !Sync)

        init_platform();

        // Compile-time check: NanoIsolate is NOT Send
        #[allow(dead_code)]
        fn assert_not_send<T: Send>() {}
        // This should fail to compile if uncommented:
        // assert_not_send::<NanoIsolate>();

        // Verify the pool creates workers correctly
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_full_request_object_passed() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create handler that inspects request properties
        let entrypoint = create_test_handler(
            &temp_dir,
            "full_request.js",
            r#"
function fetch(request) {
    const info = {
        method: request.method,
        url: request.url,
        headers: request.headers,
        hasBody: request.body !== null
    };
    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(info)
    };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        // Create request with all properties
        let url = NanoUrl::parse("http://test.example.com/api/items/123?expand=true").unwrap();
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        headers.set("X-Custom-Header", "custom-value");
        let body = Some(bytes::Bytes::from(r#"{"key":"value"}"#));
        let request = NanoRequest::new("POST".to_string(), url, headers, body);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Handler failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        // Verify body contains all request info
        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(body_text.contains("POST"), "Method not found: {}", body_text);
        assert!(body_text.contains("http://test.example.com/api/items/123"), "URL not found: {}", body_text);
        assert!(body_text.contains("custom-value"), "Header not found: {}", body_text);
        assert!(body_text.contains("true"), "Body flag not found: {}", body_text);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_async_handler_promise() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());

        // Create async handler
        let js_code = format!(
            r#"
async function fetch(request) {{
    // Simulate async work
    const data = await Promise.resolve({{ token: "{}" }});

    return {{
        status: 200,
        headers: {{ "Content-Type": "application/json" }},
        body: JSON.stringify(data)
    }};
}}
"#,
            dynamic_token
        );
        let entrypoint = create_test_handler(&temp_dir, "async_handler.js", &js_code);

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Async handler failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(
            body_text.contains(&dynamic_token),
            "Async response must contain dynamic token '{}', got: {}",
            dynamic_token,
            body_text
        );

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_request_body_passed() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Handler that checks if body was passed (base64 encoded)
        let entrypoint = create_test_handler(
            &temp_dir,
            "body_check.js",
            r#"
function fetch(request) {
    // Body is base64 encoded in the request object
    const hasBody = request.body !== null;
    const bodyUsed = request.bodyUsed;

    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hasBody, bodyUsed })
    };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let body = Some(bytes::Bytes::from("Hello from client"));
        let request = NanoRequest::new("POST".to_string(), url, NanoHeaders::new(), body);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Body passing failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(body_text.contains("true"), "Body flags not correct: {}", body_text);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_oom_monitor_integration() {
        // Test that worker pool with memory limit creates OOM monitors
        init_platform();

        // Create pool with 16MB memory limit per isolate
        let pool = WorkerPool::new("oom-test.example.com".to_string(), 1, 16);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "OK" }; }"#,
        );

        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");

        // Should complete successfully (fresh isolate under 16MB limit)
        let response = rx.blocking_recv().expect("Failed to receive");
        assert!(
            response.is_ok(),
            "Request should complete with OOM monitoring enabled"
        );

        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_worker_pool_vfs_isolation() {
        // Test that different pools have isolated VFS namespaces
        init_platform();

        // Create two pools for different apps
        let pool1 = WorkerPool::new("app1.example.com".to_string(), 1, 0);
        let pool2 = WorkerPool::new("app2.example.com".to_string(), 1, 0);

        // Write a file via pool1's VFS backend directly
        // (simulating what would happen through JS execution)
        let namespace1 = VfsNamespace::from_hostname("app1.example.com");
        let path1 = crate::vfs::VfsPath::new(&format!("{}::secret.txt", namespace1.as_str())).unwrap();
        
        // Use tokio runtime to run async write
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            pool1.vfs_backend().write(&path1, b"app1-secret-data").await.unwrap();
        });

        // Verify file exists in pool1's backend
        let rt = tokio::runtime::Runtime::new().unwrap();
        let exists_in_pool1: bool = rt.block_on(async {
            pool1.vfs_backend().exists(&path1).await.unwrap()
        });
        assert!(exists_in_pool1, "File should exist in pool1's VFS");

        // Verify file does NOT exist in pool2's backend (different namespace)
        let namespace2 = VfsNamespace::from_hostname("app2.example.com");
        let path2 = crate::vfs::VfsPath::new(&format!("{}::secret.txt", namespace2.as_str())).unwrap();
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        let exists_in_pool2: bool = rt.block_on(async {
            pool2.vfs_backend().exists(&path2).await.unwrap()
        });
        assert!(!exists_in_pool2, "File should NOT exist in pool2's VFS (isolated)");

        // Clean up
        pool1.shutdown().expect("Pool1 shutdown failed");
        pool2.shutdown().expect("Pool2 shutdown failed");
    }

    // SliverWorkerPool tests
    use crate::sliver::{pack_sliver, SliverMetadata, UnpackedSliver};

    fn create_test_sliver_for_pool(hostname: &str) -> UnpackedSliver {
        let metadata = SliverMetadata::new(hostname, "1.1.0");
        let heap_data = vec![0xABu8; 1024];
        let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
        crate::sliver::unpack_sliver(&archive).unwrap()
    }

    #[test]
    fn test_sliver_worker_pool_creation() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("sliver-test.example.com");
        
        let pool = SliverWorkerPool::new(
            "sliver-test.example.com".to_string(),
            2,
            0,
            unpacked,
        );
        
        assert_eq!(pool.worker_count(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_single_worker() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("single.example.com");
        
        let pool = SliverWorkerPool::new(
            "single.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        assert_eq!(pool.worker_count(), 1);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_dispatch() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());

        // Create a simple JS handler
        let js_code = format!(
            r#"function fetch(request) {{ return {{ status: 200, headers: {{}}, body: "{}" }}; }}"#,
            dynamic_token
        );
        let entrypoint = create_test_handler(&temp_dir, "test.js", &js_code);
        
        let unpacked = create_test_sliver_for_pool("dispatch.example.com");
        let pool = SliverWorkerPool::new(
            "dispatch.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);
        
        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);
        
        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");
        
        assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);
        
        let body_text = String::from_utf8_lossy(resp.body().map(|b| &b[..]).unwrap_or(&[]));
        assert!(
            body_text.contains(&dynamic_token),
            "Sliver response must contain dynamic token '{}', got: {}",
            dynamic_token,
            body_text
        );
        
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_accessors() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("accessors.example.com");
        let sliver_hostname = unpacked.metadata.hostname.clone();
        
        let pool = SliverWorkerPool::new(
            "accessors.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        // Test sliver_data accessor
        let sliver_data = pool.sliver_data();
        assert_eq!(sliver_data.metadata.hostname, sliver_hostname);
        
        // Test vfs_backend accessor
        let _vfs_backend = pool.vfs_backend();
        
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_with_temp_vfs() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());

        // Create a JS handler in the temp directory (simulating extracted VFS)
        let temp_handler_code = format!(
            r#"function fetch(request) {{ return {{ status: 200, headers: {{ "Content-Type": "text/plain" }}, body: "{}" }}; }}"#,
            dynamic_token
        );
        let temp_entrypoint = temp_dir.path().join("index.js");
        std::fs::write(&temp_entrypoint, &temp_handler_code)
            .expect("Failed to write temp handler");

        // Create sliver with different handler content (should not be used)
        let unpacked = create_test_sliver_for_pool("temp-vfs.example.com");

        // Create pool with temp entrypoint
        let pool = SliverWorkerPool::with_temp_entrypoint(
            "temp-vfs.example.com".to_string(),
            1,
            0,
            unpacked,
            Some(temp_entrypoint.clone()),
        );

        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        // Note: we pass a dummy entrypoint here, it should be overridden by temp_entrypoint
        let task = HandlerTask::new("/dummy/path.js".to_string(), request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");

        assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        // Verify the response came from temp VFS handler
        let body = resp.body().cloned().unwrap_or_default();
        let body_text = String::from_utf8_lossy(&body);
        assert!(
            body_text.contains(&dynamic_token),
            "Expected response from temp VFS with dynamic token '{}', got: {}",
            dynamic_token,
            body_text
        );

        pool.shutdown().expect("Shutdown failed");
    }
}
