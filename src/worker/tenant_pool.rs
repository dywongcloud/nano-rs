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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use anyhow::{anyhow, Result};
use tracing::{error, info};

use crate::control_plane::ControlPlane;
use crate::vfs::{IsolateVfs, VfsBackendEnum, VfsNamespace};
use crate::worker::oom::OomMonitorBuilder;
use crate::worker::HandlerTask;
use crate::data_plane::set_worker_runtime;
use base64::Engine as _;

/// Maximum requests before recycling an isolate
const MAX_REQUESTS_PER_ISOLATE: u32 = 10_000;


/// A pool of isolates dedicated to a single tenant (hostname)
pub struct TenantPool {
    hostname: String,
    workers: Vec<TenantWorker>,
    next_worker: AtomicU64,
    #[allow(dead_code)]
    vfs_backend: VfsBackendEnum,
    #[allow(dead_code)]
    control_plane: Option<ControlPlane>,
}

struct TenantWorker {
    task_tx: mpsc::Sender<HandlerTask>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TenantPool {
    /// Create a new tenant pool for the given hostname
    pub fn new(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        control_plane: Option<ControlPlane>,
    ) -> Result<Self> {
        let mut workers = Vec::with_capacity(worker_count as usize);

        for id in 0..worker_count {
            let worker = Self::spawn_worker(
                id,
                hostname.clone(),
                memory_limit_mb,
                vfs_backend.clone(),
            )?;
            workers.push(worker);
        }

        info!(
            "Created tenant pool for '{}' with {} workers",
            hostname, worker_count
        );

        Ok(Self {
            hostname,
            workers,
            next_worker: AtomicU64::new(0),
            vfs_backend,
            control_plane,
        })
    }

    /// Spawn a worker thread with its own isolate
    fn spawn_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
    ) -> Result<TenantWorker> {
        let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
            mpsc::channel();

        let thread = thread::spawn(move || {
            Self::worker_loop(id, hostname, memory_limit_mb, vfs_backend, task_rx);
        });

        Ok(TenantWorker {
            task_tx,
            thread: Some(thread),
        })
    }

    /// Worker event loop - owns isolate for this tenant
    fn worker_loop(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
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
        Self::run_worker(id, hostname, memory_limit_mb, vfs_backend, task_rx);
    }

    fn run_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
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

                        let call_result = handler_local.call(&mut ctx_scope, global_obj.into(), &[js_req.into()]);
                        let resolved = match call_result {
                            None => return Err(anyhow!("Handler threw a JS exception")),
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
                                    ctx_scope.perform_microtask_checkpoint();
                                    match promise.state() {
                                        v8::PromiseState::Fulfilled => break promise.result(&mut ctx_scope),
                                        v8::PromiseState::Rejected => {
                                            let err = promise.result(&mut ctx_scope);
                                            let msg = err.to_string(&mut ctx_scope)
                                                .map(|s| s.to_rust_string_lossy(&mut ctx_scope))
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

                        let obj = resolved.to_object(&mut ctx_scope)
                            .ok_or_else(|| anyhow!("Handler response is not an object"))?;
                        let sk = v8::String::new(&mut ctx_scope, "status").ok_or_else(|| anyhow!("status key"))?;
                        let status = obj.get(&mut ctx_scope, sk.into())
                            .and_then(|v| v.to_integer(&mut ctx_scope))
                            .map(|i| i.value() as u16)
                            .unwrap_or(200);
                        let mut response = crate::http::NanoResponse::with_status(status);

                        let h2k = v8::String::new(&mut ctx_scope, "headers").ok_or_else(|| anyhow!("headers key"))?;
                        if let Some(hval) = obj.get(&mut ctx_scope, h2k.into()) {
                            if let Some(hobj) = hval.to_object(&mut ctx_scope) {
                                let ik = v8::String::new(&mut ctx_scope, "__headers__").ok_or_else(|| anyhow!("__headers__ key"))?;
                                let hsrc = hobj.get(&mut ctx_scope, ik.into())
                                    .and_then(|v| v.to_object(&mut ctx_scope))
                                    .unwrap_or(hobj);
                                if let Some(names) = hsrc.get_own_property_names(&mut ctx_scope, Default::default()) {
                                    for i in 0..names.length() {
                                        if let Some(key) = names.get_index(&mut ctx_scope, i) {
                                            if let Some(ks) = key.to_string(&mut ctx_scope) {
                                                let k = ks.to_rust_string_lossy(&mut ctx_scope);
                                                if k.starts_with("__") || matches!(k.as_str(), "set" | "get" | "forEach") { continue; }
                                                if let Some(val) = hsrc.get(&mut ctx_scope, key.into()) {
                                                    if !val.is_function() {
                                                        if let Some(vs) = val.to_string(&mut ctx_scope) {
                                                            response = response.with_header(&k, &vs.to_rust_string_lossy(&mut ctx_scope));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let b2k = v8::String::new(&mut ctx_scope, "body").ok_or_else(|| anyhow!("body key"))?;
                        if let Some(bval) = obj.get(&mut ctx_scope, b2k.into()) {
                            if !bval.is_null() && !bval.is_undefined() {
                                if let Some(bs) = bval.to_string(&mut ctx_scope) {
                                    response = response.with_body(bs.to_rust_string_lossy(&mut ctx_scope));
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
        // Channels are dropped, signaling workers to exit
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}


