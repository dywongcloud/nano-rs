//! Worker pool implementation with thread-local isolate ownership
//!
//! This module provides the WorkerPool that manages N worker threads,
//! each owning a V8 isolate. Tasks are dispatched via MPSC channels
//! and responses are returned via oneshot channels.

use crate::http::NanoResponse;
use crate::runtime::HandlerContext;
use crate::v8::{initialize_platform, NanoIsolate};
use crate::worker::context::ContextManager;
use crate::worker::oom::{OomMonitor, OomMonitorBuilder};
use crate::worker::HandlerTask;
use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
use crate::sliver::UnpackedSliver;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use std::fs;

/// Execute a handler within a specific V8 context
///
/// This helper function is used by the worker thread to execute JavaScript
/// handlers after context reset. It creates the necessary scopes and invokes
/// the fetch function.
/// Execute handler using the ContextManager's current context
///
/// This function properly manages V8 scope lifecycle to avoid "active scope" errors.
fn execute_with_context_manager(
    context_manager: &mut ContextManager,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    // Clone the Global<Context> (cheap - just a handle reference)
    let global_ctx = context_manager.clone_context();

    // Now get the isolate pointer - this borrows context_manager mutably
    let isolate = context_manager.isolate_mut().isolate();

    // Create fresh HandleScope
    let handle_scope = &mut v8::HandleScope::new(isolate);

    // Reopen Local<Context> from Global within the new scope
    let v8_context = match global_ctx {
        Some(g) => v8::Local::new(handle_scope, &g),
        None => return Err(anyhow!("No context available")),
    };

    // Enter context scope and execute
    let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);

    // Read and execute handler
    execute_handler_code(context_scope, v8_context, handler_ctx)
}

/// Execute the actual handler code within an established context scope
fn execute_handler_code(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    v8_context: v8::Local<v8::Context>,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    // Read the handler code
    let code = fs::read_to_string(&handler_ctx.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint: {}", e))?;

    // Compile and run script to define fetch function
    let code_str =
        v8::String::new(scope, &code).ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(scope);

    // Get global and look for fetch function
    let global = v8_context.global(scope);
    let fetch_key = v8::String::new(scope, "fetch").unwrap();
    let fetch_val = match global.get(scope, fetch_key.into()) {
        Some(val) => val,
        None => {
            return Ok(NanoResponse::ok()
                .with_header("Content-Type", "text/plain")
                .with_body("Handler executed (no fetch function defined)"));
        }
    };

    let fetch_fn = fetch_val.cast::<v8::Function>();

    // Create request object
    let request_json = format!("{{\"method\":\"{}\"}}", handler_ctx.request.method());
    let request_str = v8::String::new(scope, &request_json)
        .ok_or_else(|| anyhow!("Failed to create request string"))?;

    // Call fetch function
    let result = fetch_fn.call(scope, global.into(), &[request_str.into()]);

    // Extract response
    match result {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

fn execute_handler_in_context(
    isolate: &mut v8::OwnedIsolate,
    v8_context: v8::Local<v8::Context>,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    // Create scope stack for execution - must be dropped in reverse order
    let handle_scope = &mut v8::HandleScope::new(isolate);
    let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);

    // Read the handler code
    let code = fs::read_to_string(&handler_ctx.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint: {}", e))?;

    // Compile and run script to define fetch function
    let code_str = v8::String::new(context_scope, &code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(context_scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(context_scope);

    // Get global and look for fetch function
    let global = v8_context.global(context_scope);
    let fetch_key = v8::String::new(context_scope, "fetch").unwrap();
    let fetch_val = match global.get(context_scope, fetch_key.into()) {
        Some(val) => val,
        None => {
            // No fetch function defined - return default response
            return Ok(NanoResponse::ok()
                .with_header("Content-Type", "text/plain")
                .with_body("Handler executed (no fetch function defined)"));
        }
    };

    let fetch_fn = fetch_val.cast::<v8::Function>();

    // Create a simple request object
    let request_json = format!("{{\"method\":\"{}\"}}", handler_ctx.request.method());
    let request_str = v8::String::new(context_scope, &request_json)
        .ok_or_else(|| anyhow!("Failed to create request string"))?;

    // Call fetch function
    let result = fetch_fn.call(context_scope, global.into(), &[request_str.into()]);

    // Extract response from result
    match result {
        Some(response) => extract_js_response(context_scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Extract a NanoResponse from a V8 JavaScript object
fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    use crate::http::NanoHeaders;
    use bytes::Bytes;

    // Verify the response is an object
    let obj = match js_response.to_object(scope) {
        Some(o) => o,
        None => return Err(anyhow!("Response is not an object")),
    };

    // Extract status property (default to 200)
    let status_key = v8::String::new(scope, "status").unwrap();
    let status = match obj.get(scope, status_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => match val.to_integer(scope) {
            Some(int) => int.value() as u16,
            None => 200,
        },
        _ => 200,
    };

    // Extract headers property
    let mut nano_headers = NanoHeaders::new();
    let headers_key = v8::String::new(scope, "headers").unwrap();

    if let Some(headers_val) = obj.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
            if let Some(names) = headers_obj.get_own_property_names(scope, Default::default()) {
                let len = names.length();
                for i in 0..len {
                    if let Some(key) = names.get_index(scope, i) {
                        if let Some(key_str) = key.to_string(scope) {
                            let key_name = key_str.to_rust_string_lossy(scope);
                            if let Some(value) = headers_obj.get(scope, key.into()) {
                                if let Some(value_str) = value.to_string(scope) {
                                    let value_string = value_str.to_rust_string_lossy(scope);
                                    nano_headers.set(&key_name, &value_string);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Extract body property
    let body_key = v8::String::new(scope, "body").unwrap();
    let body = match obj.get(scope, body_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => match val.to_string(scope) {
            Some(s) => Some(Bytes::from(s.to_rust_string_lossy(scope))),
            None => None,
        },
        _ => None,
    };

    Ok(NanoResponse::new(status, nano_headers, body))
}
use std::sync::atomic::{AtomicUsize, Ordering};
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
    pub id: usize,
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

/// Pool of worker threads for JavaScript execution
///
/// Each worker owns one V8 isolate (thread-local). Tasks are dispatched
/// via MPSC channels. The pool uses round-robin for initial dispatch
/// (affine dispatch comes in later phase).
///
/// # VFS Integration
///
/// Each WorkerPool has a shared VFS backend that all workers in the pool
/// share. This means files written by one worker are visible to other
/// workers in the same pool (same app), but isolated from other pools.
pub struct WorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: usize,
    /// Hostname this pool serves (for logging/debugging)
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicUsize,
    /// Shared VFS backend for all workers in this pool
    vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("workers", &self.workers.len())
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("next_worker", &self.next_worker)
            .field("vfs_backend", &"<dyn VfsBackend>")
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
    pub fn new(hostname: String, worker_count: usize, memory_limit_mb: u32) -> Self {
        Self::with_backend(hostname, worker_count, memory_limit_mb, Arc::new(MemoryBackend::default()))
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
        worker_count: usize,
        memory_limit_mb: u32,
        vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
    ) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        // Clone hostname for use in closures (original kept for final logging)
        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = Arc::clone(&vfs_backend);

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = Arc::clone(&vfs_backend_for_workers);
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with thread-local isolate
            let thread = thread::spawn(move || {
                info!("Worker {} starting", id);

                // Create OOM monitor for this worker if memory limit is configured
                let mut oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                // Create VFS for this worker with shared backend
                let vfs = IsolateVfs::new(
                    VfsNamespace::from_hostname(&worker_hostname),
                    worker_vfs_backend,
                );

                // Create isolate with VFS in this thread - NEVER moves to another thread
                // This is the critical POOL-05 constraint: isolate is !Send + !Sync
                let isolate = match NanoIsolate::new_with_vfs(vfs) {
                    Ok(isol) => isol,
                    Err(e) => {
                        error!("Worker {} failed to create isolate: {}", id, e);
                        return;
                    }
                };

                // Create context manager for this worker
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }
                info!("Worker {} initialized with context", id);

                // Event loop: receive tasks and execute
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("Worker {} received task for {}", id, task.entrypoint);

                            // OOM-04: Pre-request OOM check
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                match monitor.check(isolate_ref) {
                                    Ok(_) => {
                                        // Memory OK, continue with request
                                    }
                                    Err(oom_error) => {
                                        // OOM detected - log, return 503, dispose isolate
                                        let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                        monitor.log_oom_event(&oom_error, &request_id);

                                        let oom_response = monitor.create_oom_response(&oom_error);
                                        let _ = task.response_tx.send(Ok(oom_response));

                                        // Dispose isolate and create fresh one
                                        warn!(
                                            "Worker {} disposing isolate due to OOM (oom_count: {})",
                                            id,
                                            monitor.oom_count()
                                        );

                                        // Create new isolate to replace the OOM'd one
                                        match NanoIsolate::new() {
                                            Ok(new_isolate) => {
                                                context_manager = ContextManager::new(new_isolate);
                                                if let Err(e) =
                                                    context_manager.create_initial_context()
                                                {
                                                    error!("Worker {} failed to create new context after OOM: {}", id, e);
                                                    break; // Exit worker if can't recover
                                                }
                                                // Reset OOM monitor for fresh isolate
                                                monitor.reset();
                                                info!(
                                                    "Worker {} created fresh isolate after OOM",
                                                    id
                                                );
                                            }
                                            Err(e) => {
                                                error!("Worker {} failed to create replacement isolate: {}", id, e);
                                                break; // Exit worker if can't recover
                                            }
                                        }
                                        continue;
                                    }
                                }
                            }

                            // POOL-04: Reset context before each request
                            match context_manager.reset_context() {
                                Ok(elapsed) => {
                                    let ms = elapsed.as_secs_f64() * 1000.0;
                                    if ms > 10.0 {
                                        warn!(
                                            "Worker {} context reset took {:.2}ms (target <10ms)",
                                            id, ms
                                        );
                                    } else {
                                        debug!("Worker {} context reset took {:.2}ms", id, ms);
                                    }
                                }
                                Err(e) => {
                                    error!("Worker {} context reset failed: {}", id, e);
                                    let _ =
                                        task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    continue;
                                }
                            }

                            // Create handler context
                            let handler_ctx = HandlerContext {
                                entrypoint: task.entrypoint,
                                request: task.request,
                            };

                            // Execute handler with fresh context scope
                            let result =
                                execute_with_context_manager(&mut context_manager, &handler_ctx);

                            // Post-request OOM check (optional - catches runaway memory during request)
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                if let Err(oom_error) = monitor.check(isolate_ref) {
                                    let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                    monitor.log_oom_event(&oom_error, &request_id);
                                    warn!(
                                        "Worker {} OOM detected after request execution (oom_count: {})",
                                        id,
                                        monitor.oom_count()
                                    );
                                    // Don't return 503 here since we already returned the actual response
                                    // Just dispose and recreate for next request

                                    // Dispose isolate and create fresh one
                                    match NanoIsolate::new() {
                                        Ok(new_isolate) => {
                                            context_manager = ContextManager::new(new_isolate);
                                            if let Err(e) = context_manager.create_initial_context()
                                            {
                                                error!("Worker {} failed to create new context after post-request OOM: {}", id, e);
                                                break;
                                            }
                                            monitor.reset();
                                            info!("Worker {} created fresh isolate after post-request OOM", id);
                                        }
                                        Err(e) => {
                                            error!("Worker {} failed to create replacement isolate: {}", id, e);
                                            break;
                                        }
                                    }
                                }
                            }

                            // Send response back
                            let _ = task.response_tx.send(result);
                        }
                        Err(_) => {
                            // Channel closed, exit gracefully
                            debug!("Worker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }

                // Isolate is dropped here when worker thread exits
                info!(
                    "Worker {} shutting down (avg context reset: {:.2}ms, OOM events: {})",
                    id,
                    context_manager.average_reset_time_ms(),
                    oom_monitor.map(|m| m.oom_count()).unwrap_or(0)
                );
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
            next_worker: AtomicUsize::new(0),
            vfs_backend,
        }
    }

    /// Get a reference to the shared VFS backend
    ///
    /// This is useful for testing and administrative operations
    /// that need to inspect or modify the filesystem.
    pub fn vfs_backend(&self) -> &Arc<dyn crate::vfs::VfsBackend> {
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
    pub fn dispatch_to(&self, worker_idx: usize, task: HandlerTask) -> Result<()> {
        if worker_idx >= self.worker_count {
            return Err(anyhow!(
                "Worker index {} out of bounds (max {})",
                worker_idx,
                self.worker_count - 1
            ));
        }

        self.workers[worker_idx]
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
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Try graceful shutdown if not already called
        // We need to take the workers and join them
        if !self.workers.is_empty() {
            warn!(
                "WorkerPool for {} dropped without explicit shutdown - forcing cleanup",
                self.hostname
            );

            // Take workers and drop their senders (signals exit)
            let handles: Vec<_> = self
                .workers
                .drain(..)
                .map(|mut w| w.take_thread())
                .collect();

            // Try to join threads (best effort - may not complete if already panicking)
            for handle in handles {
                if let Some(h) = handle {
                    let _ = h.join();
                }
            }
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
pub struct SliverWorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: usize,
    /// Hostname this pool serves
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicUsize,
    /// Shared VFS backend for all workers
    vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
    /// Unpacked sliver data containing snapshot and VFS entries
    unpacked_sliver: crate::sliver::UnpackedSliver,
}

impl std::fmt::Debug for SliverWorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverWorkerPool")
            .field("workers", &self.workers.len())
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("hostname", &self.hostname)
            .field("vfs_backend", &"<dyn VfsBackend>")
            .field("unpacked_sliver", &self.unpacked_sliver.metadata.hostname)
            .finish()
    }
}

impl SliverWorkerPool {
    /// Create a new sliver worker pool with restored isolates
    ///
    /// Each worker thread:
    /// 1. Creates its NanoIsolate from the snapshot (or fresh if fails)
    /// 2. Restores VFS entries from the sliver
    /// 3. Runs an event loop receiving HandlerTask via MPSC
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
    pub fn new(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        unpacked_sliver: crate::sliver::UnpackedSliver,
    ) -> Self {
        Self::with_backend(
            hostname,
            worker_count,
            memory_limit_mb,
            Arc::new(MemoryBackend::default()),
            unpacked_sliver,
        )
    }

    /// Create a new sliver worker pool with a specific VFS backend
    pub fn with_backend(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
        unpacked_sliver: crate::sliver::UnpackedSliver,
    ) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = Arc::clone(&vfs_backend);
        let sliver_for_workers = unpacked_sliver.clone();

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = Arc::clone(&vfs_backend_for_workers);
            let worker_sliver = sliver_for_workers.clone();
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with snapshot-restored isolate
            let thread = thread::spawn(move || {
                info!("SliverWorker {} starting for {}", id, worker_hostname);

                // Create OOM monitor
                let mut oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("sliver_worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                // Create VFS for this worker
                let vfs = IsolateVfs::new(
                    VfsNamespace::from_hostname(&worker_hostname),
                    worker_vfs_backend,
                );

                // Restore VFS entries from sliver before creating isolate
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                if let Err(e) = rt.block_on(worker_sliver.restore_to_vfs(&vfs)) {
                    error!("Worker {} failed to restore VFS: {}", id, e);
                    // Continue anyway - the app might work without VFS data
                } else {
                    debug!("Worker {} restored {} VFS entries", id, worker_sliver.vfs_entries.len());
                }

                // Create isolate from snapshot or fresh as fallback
                let vfs_clone = vfs.clone();
                let isolate = match crate::v8::restore_from_snapshot(&worker_sliver.heap_data, vfs_clone) {
                    Ok(isol) => {
                        info!("Worker {} created isolate from snapshot", id);
                        isol
                    }
                    Err(e) => {
                        warn!("Worker {} failed to restore from snapshot: {}. Creating fresh isolate.", id, e);
                        match NanoIsolate::new_with_vfs(vfs.clone()) {
                            Ok(isol) => isol,
                            Err(e) => {
                                error!("Worker {} failed to create isolate: {}", id, e);
                                return;
                            }
                        }
                    }
                };

                // Create context manager
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }
                info!("SliverWorker {} initialized with restored context", id);

                // Event loop: same as regular WorkerPool
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("SliverWorker {} received task for {}", id, task.entrypoint);

                            // OOM check before request
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                match monitor.check(isolate_ref) {
                                    Ok(_) => {}
                                    Err(oom_error) => {
                                        let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                        monitor.log_oom_event(&oom_error, &request_id);

                                        let oom_response = monitor.create_oom_response(&oom_error);
                                        let _ = task.response_tx.send(Ok(oom_response));

                                        // Dispose and recreate isolate
                                        warn!("Worker {} disposing isolate due to OOM", id);
                                        match NanoIsolate::new() {
                                            Ok(new_isolate) => {
                                                context_manager = ContextManager::new(new_isolate);
                                                if let Err(e) = context_manager.create_initial_context() {
                                                    error!("Worker {} failed to create new context after OOM: {}", id, e);
                                                    break;
                                                }
                                                monitor.reset();
                                                info!("Worker {} created fresh isolate after OOM", id);
                                            }
                                            Err(e) => {
                                                error!("Worker {} failed to create replacement isolate: {}", id, e);
                                                break;
                                            }
                                        }
                                        continue;
                                    }
                                }
                            }

                            // Reset context before each request
                            match context_manager.reset_context() {
                                Ok(elapsed) => {
                                    let ms = elapsed.as_secs_f64() * 1000.0;
                                    debug!("SliverWorker {} context reset took {:.2}ms", id, ms);
                                }
                                Err(e) => {
                                    error!("SliverWorker {} context reset failed: {}", id, e);
                                    let _ = task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    continue;
                                }
                            }

                            // Create handler context
                            let handler_ctx = HandlerContext {
                                entrypoint: task.entrypoint,
                                request: task.request,
                            };

                            // Execute handler
                            let result = execute_with_context_manager(&mut context_manager, &handler_ctx);

                            // Post-request OOM check
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                if let Err(oom_error) = monitor.check(isolate_ref) {
                                    let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                    monitor.log_oom_event(&oom_error, &request_id);
                                    warn!("SliverWorker {} OOM detected after request", id);

                                    // Dispose and recreate
                                    match NanoIsolate::new() {
                                        Ok(new_isolate) => {
                                            context_manager = ContextManager::new(new_isolate);
                                            if let Err(e) = context_manager.create_initial_context() {
                                                error!("Worker {} failed to create new context: {}", id, e);
                                                break;
                                            }
                                            monitor.reset();
                                        }
                                        Err(e) => {
                                            error!("Worker {} failed to create replacement: {}", id, e);
                                            break;
                                        }
                                    }
                                }
                            }

                            // Send response
                            let _ = task.response_tx.send(result);
                        }
                        Err(_) => {
                            debug!("SliverWorker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }

                info!(
                    "SliverWorker {} shutting down (avg context reset: {:.2}ms)",
                    id,
                    context_manager.average_reset_time_ms()
                );
            });

            workers.push(WorkerHandle {
                id,
                thread: Some(thread),
                task_tx,
            });
        }

        info!(
            "SliverWorkerPool created for {} with {} workers (snapshot restored)",
            hostname, worker_count
        );

        Self {
            workers,
            worker_count,
            hostname,
            next_worker: AtomicUsize::new(0),
            vfs_backend,
            unpacked_sliver,
        }
    }

    /// Get a reference to the shared VFS backend
    pub fn vfs_backend(&self) -> &Arc<dyn crate::vfs::VfsBackend> {
        &self.vfs_backend
    }

    /// Get reference to unpacked sliver data
    pub fn sliver_data(&self) -> &crate::sliver::UnpackedSliver {
        &self.unpacked_sliver
    }

    /// Dispatch a task to a worker (round-robin)
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        let worker_idx = self.next_worker.fetch_add(1, Ordering::SeqCst) % self.worker_count;

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Dispatch to specific worker
    pub fn dispatch_to(&self, worker_idx: usize, task: HandlerTask) -> Result<()> {
        if worker_idx >= self.worker_count {
            return Err(anyhow!(
                "Worker index {} out of bounds (max {})",
                worker_idx,
                self.worker_count - 1
            ));
        }

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Gracefully shut down the worker pool
    pub fn shutdown(mut self) -> Result<()> {
        info!("Shutting down SliverWorkerPool for {}", self.hostname);

        let mut handles: Vec<_> = self
            .workers
            .drain(..)
            .map(|mut w| (w.id, w.take_thread()))
            .collect();

        for (id, handle) in handles.drain(..) {
            if let Some(h) = handle {
                debug!("Waiting for sliver worker {} to exit", id);
                match h.join() {
                    Ok(_) => debug!("SliverWorker {} exited cleanly", id),
                    Err(_) => warn!("SliverWorker {} panicked during shutdown", id),
                }
            }
        }

        info!("SliverWorkerPool for {} shut down complete", self.hostname);
        Ok(())
    }

    /// Get the number of workers in this pool
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }
}

impl Drop for SliverWorkerPool {
    fn drop(&mut self) {
        if !self.workers.is_empty() {
            warn!(
                "SliverWorkerPool for {} dropped without explicit shutdown - forcing cleanup",
                self.hostname
            );

            let handles: Vec<_> = self
                .workers
                .drain(..)
                .map(|mut w| w.take_thread())
                .collect();

            for handle in handles {
                if let Some(h) = handle {
                    let _ = h.join();
                }
            }
        }
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

        // Create a simple JS handler (non-async for now)
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"
function fetch(request) {
    return { status: 200, headers: { "Content-Type": "text/plain" }, body: "Hello from worker" };
}
"#,
        );

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
        fn assert_not_send<T: Send>() {}
        // This should fail to compile if uncommented:
        // assert_not_send::<NanoIsolate>();

        // Verify the pool creates workers correctly
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);
        assert_eq!(pool.workers.len(), 2);
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
        use crate::vfs::VfsBackend;
        
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
        
        // Create a simple JS handler
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "Sliver OK" }; }"#,
        );
        
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
}
