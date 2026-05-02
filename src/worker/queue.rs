//! WorkQueue with bounded MPSC channel and affine dispatch
//!
//! Manages task distribution across worker pools with backpressure protection.
//! Affine dispatch ensures requests for the same hostname consistently route
//! to the same pool, improving cache locality.
//!
//! # Requirements
//!
//! - POOL-02: Bounded MPSC channel with 256-slot capacity
//! - POOL-03: Affine dispatch: hostname → pool index → worker thread
//!
//! # Decisions
//!
//! - **D-WQ-01:** 256-slot capacity per worker thread (not per pool)
//! - **D-WQ-02:** Case-insensitive hostname hashing per HTTP spec
//! - **D-WQ-03:** DefaultHasher for consistent hostname-to-pool mapping

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::http::{NanoHeaders, NanoResponse};
use crate::http::v8_bridge::serialize_request_to_json;
use crate::runtime::HandlerContext;
use crate::v8::initialize_platform;
use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
use crate::worker::HandlerTask;
use crate::worker::context::ContextManager;

use anyhow::anyhow;

/// Error types for queue operations
#[derive(Debug, Clone, PartialEq)]
pub enum QueueError {
    /// Channel is at capacity (bounded channel full)
    ChannelFull,
    /// Worker thread not found (invalid index)
    WorkerNotFound,
    /// Pool not found for hostname
    PoolNotFound,
    /// Send error (channel disconnected)
    SendError(String),
    /// Other errors
    Other(String),
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueError::ChannelFull => write!(f, "WorkQueue channel is full"),
            QueueError::WorkerNotFound => write!(f, "Worker thread not found"),
            QueueError::PoolNotFound => write!(f, "Pool not found for hostname"),
            QueueError::SendError(e) => write!(f, "Send error: {}", e),
            QueueError::Other(e) => write!(f, "Queue error: {}", e),
        }
    }
}

impl std::error::Error for QueueError {}

/// Statistics for monitoring WorkQueue performance
#[derive(Debug)]
pub struct QueueStats {
    /// Total tasks submitted
    pub tasks_submitted: AtomicU64,
    /// Total tasks completed
    pub tasks_completed: AtomicU64,
    /// Tasks dropped due to channel full
    pub tasks_dropped: AtomicU64,
    /// Number of active pools
    pub active_pools: AtomicUsize,
    /// Number of active workers
    pub active_workers: AtomicUsize,
}

impl Default for QueueStats {
    fn default() -> Self {
        Self {
            tasks_submitted: AtomicU64::new(0),
            tasks_completed: AtomicU64::new(0),
            tasks_dropped: AtomicU64::new(0),
            active_pools: AtomicUsize::new(0),
            active_workers: AtomicUsize::new(0),
        }
    }
}

impl QueueStats {
    /// Create new stats with all counters at zero
    pub fn new() -> Self {
        Self::default()
    }

    /// Get snapshot of current stats
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            tasks_submitted: self.tasks_submitted.load(Ordering::Relaxed),
            tasks_completed: self.tasks_completed.load(Ordering::Relaxed),
            tasks_dropped: self.tasks_dropped.load(Ordering::Relaxed),
            active_pools: self.active_pools.load(Ordering::Relaxed),
            active_workers: self.active_workers.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of queue statistics
#[derive(Debug, Clone, Copy)]
pub struct StatsSnapshot {
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_dropped: u64,
    pub active_pools: usize,
    pub active_workers: usize,
}

/// Handle to a worker thread
#[derive(Debug)]
pub struct WorkerHandle {
    /// Worker thread ID
    pub id: usize,
    /// The worker thread join handle
    pub thread: JoinHandle<()>,
    /// Task sender channel (bounded MPSC)
    pub task_tx: std::sync::mpsc::SyncSender<HandlerTask>,
}

/// A pool of worker threads for a specific hostname
#[derive(Debug)]
pub struct WorkerPool {
    /// Worker threads in this pool
    pub workers: Vec<WorkerHandle>,
    /// Number of workers in pool
    pub worker_count: usize,
    /// Hostname this pool serves
    pub hostname: String,
}

impl WorkerPool {
    /// Create a new worker pool with specified number of workers
    ///
    /// Each worker gets a bounded channel with 256-slot capacity per POOL-02.
    ///
    /// # Arguments
    ///
    /// * `hostname` - The hostname this pool serves
    /// * `worker_count` - Number of worker threads to create
    ///
    /// # Returns
    ///
    /// A new `WorkerPool` with workers ready to receive tasks
    pub fn new(hostname: &str, worker_count: usize) -> Self {
        let mut workers = Vec::with_capacity(worker_count);
        let channel_capacity = 256; // POOL-02 requirement
        let hostname_owned = hostname.to_string();

        for id in 0..worker_count {
            // Create bounded MPSC channel (256 slots per POOL-02)
            let (task_tx, task_rx) = sync_channel::<HandlerTask>(channel_capacity);

            // Spawn worker thread with V8 execution
            let hostname_thread = hostname_owned.clone();
            let thread = thread::spawn(move || {
                tracing::info!("Worker {} started for {}", id, hostname_thread);

                // Initialize V8 platform for this thread
                if let Err(e) = initialize_platform() {
                    tracing::error!("Worker {} failed to initialize V8: {}", id, e);
                    return;
                }

                // Create VFS for this worker
                let vfs_backend = Arc::new(MemoryBackend::new());
                let vfs = IsolateVfs::new(
                    VfsNamespace::from_hostname(&hostname_thread),
                    vfs_backend,
                );

                // Create isolate with VFS in this thread
                let isolate = match crate::v8::NanoIsolate::new_with_vfs(vfs) {
                    Ok(isol) => isol,
                    Err(e) => {
                        tracing::error!("Worker {} failed to create isolate: {}", id, e);
                        return;
                    }
                };

                // Create context manager for this worker
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    tracing::error!("Worker {} failed to create context: {}", id, e);
                    return;
                }
                
                // Inject runtime APIs into the context
                // First clone the context handle (cheap - just a reference)
                let global_ctx = match context_manager.clone_context() {
                    Some(g) => g,
                    None => {
                        tracing::error!("Worker {} failed to get context for API binding", id);
                        return;
                    }
                };
                
                // Now get isolate and create scopes
                {
                    let handle_scope = &mut v8::HandleScope::new(context_manager.isolate_mut().isolate());
                    let local_ctx = v8::Local::new(handle_scope, &global_ctx);
                    let context_scope = &mut v8::ContextScope::new(handle_scope, local_ctx);
                    crate::runtime::apis::RuntimeAPIs::bind_all(context_scope, local_ctx);
                }
                
                tracing::info!("Worker {} initialized with V8 context and runtime APIs", id);

                // Worker loop - blocks on channel receive
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            // Execute the JavaScript handler
                            tracing::debug!("Worker {} executing task for {}", id, task.entrypoint);

                            let handler_ctx = HandlerContext {
                                entrypoint: task.entrypoint.clone(),
                                request: task.request,
                            };

                            // Execute handler using context manager
                            let response = execute_with_context_manager(&mut context_manager, &handler_ctx);

                            // Send response back
                            let _ = task.response_tx.send(response);
                        }
                        Err(_) => {
                            // Channel closed, exit gracefully
                            tracing::info!("Worker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }

                tracing::info!("Worker {} stopped for {}", id, hostname_thread);
            });

            workers.push(WorkerHandle {
                id,
                thread,
                task_tx,
            });
        }

        tracing::info!(
            "Created WorkerPool for {} with {} workers ({} capacity each)",
            hostname,
            worker_count,
            channel_capacity
        );

        Self {
            workers,
            worker_count,
            hostname: hostname.to_string(),
        }
    }

    /// Try to dispatch a task to a specific worker without blocking
    ///
    /// # Arguments
    ///
    /// * `task` - The handler task to dispatch
    /// * `worker_index` - Index of the worker to send to
    ///
    /// # Returns
    ///
    /// `Ok(())` if task was sent, `Err(QueueError::ChannelFull)` if channel is full
    pub fn try_dispatch(&self, task: HandlerTask, worker_index: usize) -> Result<(), QueueError> {
        let idx = worker_index % self.workers.len();
        let worker = &self.workers[idx];

        match worker.task_tx.try_send(task) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(QueueError::ChannelFull),
            Err(TrySendError::Disconnected(_)) => Err(QueueError::SendError(
                "Worker channel disconnected".to_string(),
            )),
        }
    }

    /// Shutdown the worker pool gracefully
    ///
    /// Drops the senders, causing worker threads to exit after processing
    /// any pending tasks.
    pub fn shutdown(self) {
        tracing::info!("Shutting down WorkerPool for {}", self.hostname);

        // Drop the workers (which drops the senders)
        // Workers will exit their loops when channels close
        drop(self.workers);
    }
}

/// WorkQueue with bounded MPSC channels and affine dispatch
///
/// Manages per-hostname worker pools and routes requests consistently.
#[derive(Debug)]
pub struct WorkQueue {
    /// Map of hostname hash to worker pool
    pools: HashMap<u64, WorkerPool>,
    /// Default number of workers per pool
    workers_per_pool: usize,
    /// Bounded channel capacity (256 slots per POOL-02)
    channel_capacity: usize,
    /// Statistics for monitoring
    pub stats: QueueStats,
}

impl WorkQueue {
    /// Create a new WorkQueue
    ///
    /// # Arguments
    ///
    /// * `workers_per_pool` - Number of workers to create per hostname pool
    ///
    /// # Returns
    ///
    /// A new `WorkQueue` with empty pools HashMap
    pub fn new(workers_per_pool: usize) -> Self {
        Self {
            pools: HashMap::new(),
            workers_per_pool,
            channel_capacity: 256, // POOL-02 requirement
            stats: QueueStats::new(),
        }
    }

    /// Get or create a worker pool for a hostname
    ///
    /// Uses case-insensitive hostname hashing per D-WQ-02.
    ///
    /// # Arguments
    ///
    /// * `hostname` - The hostname to get/create pool for
    ///
    /// # Returns
    ///
    /// A mutable reference to the `WorkerPool` for this hostname
    pub fn get_or_create_pool(&mut self, hostname: &str) -> &mut WorkerPool {
        let hash = hash_hostname(hostname);

        if !self.pools.contains_key(&hash) {
            tracing::info!("Creating new WorkerPool for hostname: {}", hostname);
            let pool = WorkerPool::new(hostname, self.workers_per_pool);
            self.pools.insert(hash, pool);
            self.stats.active_pools.fetch_add(1, Ordering::Relaxed);
            self.stats
                .active_workers
                .fetch_add(self.workers_per_pool, Ordering::Relaxed);
        }

        self.pools.get_mut(&hash).expect("Pool should exist")
    }

    /// Dispatch a task to the appropriate worker pool
    ///
    /// Uses affine dispatch: same hostname always routes to same worker index.
    /// Returns HTTP 503 when channel is full (backpressure protection).
    ///
    /// # Arguments
    ///
    /// * `hostname` - The hostname to route by
    /// * `task` - The handler task to dispatch
    ///
    /// # Returns
    ///
    /// `Ok(())` if dispatched, `Err(QueueError::ChannelFull)` for backpressure
    pub fn dispatch(&mut self, hostname: &str, task: HandlerTask) -> Result<(), QueueError> {
        // Calculate worker index first (doesn't need pool reference)
        let hostname_hash = hash_hostname(hostname);

        // Get or create pool for this hostname
        let pool = self.get_or_create_pool(hostname);
        let worker_index = (hostname_hash % pool.worker_count as u64) as usize;

        // Try dispatch with bounded channel (consume the pool reference)
        let result = pool.try_dispatch(task, worker_index);

        // Update stats after pool borrow is released
        self.stats.tasks_submitted.fetch_add(1, Ordering::Relaxed);

        match result {
            Ok(()) => Ok(()),
            Err(QueueError::ChannelFull) => {
                self.stats.tasks_dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("Channel full for {} worker {}", hostname, worker_index);
                Err(QueueError::ChannelFull)
            }
            Err(e) => {
                self.stats.tasks_dropped.fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }

    /// Get pool for a hostname (returns None if not found)
    pub fn get_pool(&self, hostname: &str) -> Option<&WorkerPool> {
        let hash = hash_hostname(hostname);
        self.pools.get(&hash)
    }

    /// Shutdown all worker pools gracefully
    pub fn shutdown(self) {
        tracing::info!("Shutting down WorkQueue with {} pools", self.pools.len());
        for (hash, pool) in self.pools {
            tracing::debug!("Shutting down pool with hash: {}", hash);
            pool.shutdown();
        }
    }

    /// Get current statistics snapshot
    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }
}

/// Hash a hostname to a u64 value
///
/// Uses case-insensitive hashing per HTTP spec (D-WQ-02).
/// Uses std::collections::hash_map::DefaultHasher for consistency.
///
/// # Arguments
///
/// * `hostname` - The hostname to hash
///
/// # Returns
///
/// A u64 hash value for the lowercase hostname
pub fn hash_hostname(hostname: &str) -> u64 {
    let lowercase = hostname.to_lowercase();
    let mut hasher = DefaultHasher::new();
    lowercase.hash(&mut hasher);
    hasher.finish()
}

/// Execute a handler within a specific V8 context
///
/// This function properly manages V8 scope lifecycle to avoid "active scope" errors.
fn execute_with_context_manager(
    context_manager: &mut ContextManager,
    handler_ctx: &HandlerContext,
) -> anyhow::Result<NanoResponse> {
    use crate::runtime::apis::RuntimeAPIs;
    use crate::v8::module::{is_esm_module, transform_module_code};
    use std::fs;

    // Clone the Global<Context> (cheap - just a handle reference)
    let global_ctx = context_manager.clone_context();

    // Get VFS reference BEFORE the mutable borrow for isolate access
    // The VFS is stored in thread-local storage so VFS bindings can access it
    let vfs_opt = context_manager.vfs().cloned();

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

    // Set up VFS context for Nano.fs and require('fs') operations
    if let Some(vfs) = vfs_opt {
        let vfs_arc = std::sync::Arc::new(vfs);
        crate::runtime::vfs_bindings::set_current_vfs(Some(vfs_arc.clone()));
        crate::runtime::fs_polyfill::set_current_vfs(Some(vfs_arc));
    }

    // Read the handler code
    let code = fs::read_to_string(&handler_ctx.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint: {}", e))?;

    // Transform ES6 module syntax if this is an ESM module
    let transformed_code = if is_esm_module(&code) {
        transform_module_code(&code)
    } else {
        code
    };

    // Compile and run script to define fetch function
    let code_str = v8::String::new(context_scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(context_scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(context_scope);

    // Get global and look for the user's handler function
    // The handler is stored in __nano_user_fetch by transform_module_code
    let global = v8_context.global(context_scope);
    let handler_key = v8::String::new(context_scope, "__nano_user_fetch").unwrap();
    let handler_val = match global.get(context_scope, handler_key.into()) {
        Some(val) if val.is_function() => {
            tracing::debug!("Found user handler function in global scope");
            val
        }
        _ => {
            // Fallback: try 'fetch' for non-ESM modules
            let fetch_key = v8::String::new(context_scope, "fetch").unwrap();
            match global.get(context_scope, fetch_key.into()) {
                Some(val) if val.is_function() => {
                    tracing::debug!("Found handler via 'fetch' global");
                    val
                }
                _ => {
                    tracing::warn!("No handler function found - looking for __nano_user_fetch or fetch");
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no handler function defined)"));
                }
            }
        }
    };

    let fetch_fn = handler_val.cast::<v8::Function>();

    // Create Request instance using the Request constructor
    // This ensures the request has Request.prototype methods (text, json, arrayBuffer)
    let request_key = v8::String::new(context_scope, "Request").unwrap();
    let request_obj = if let Some(request_ctor) = global.get(context_scope, request_key.into()) {
        if request_ctor.is_function() {
            let request_fn = request_ctor.cast::<v8::Function>();
            
            // Create URL string
            let url_str = v8::String::new(context_scope, &handler_ctx.request.url_string()).unwrap();
            
            // Create init object with method, headers, and body
            let init_obj = v8::Object::new(context_scope);
            
            // Set method
            let method_key = v8::String::new(context_scope, "method").unwrap();
            let method_val = v8::String::new(context_scope, handler_ctx.request.method()).unwrap();
            let _ = init_obj.set(context_scope, method_key.into(), method_val.into());
            
            // Set headers using Headers constructor for proper Headers instance
            let headers_key = v8::String::new(context_scope, "headers").unwrap();
            let headers_ctor_key = v8::String::new(context_scope, "Headers").unwrap();
            let headers_obj = if let Some(headers_ctor) = global.get(context_scope, headers_ctor_key.into()) {
                if headers_ctor.is_function() {
                    // Create Headers instance
                    let headers_fn = headers_ctor.cast::<v8::Function>();
                    headers_fn.new_instance(context_scope, &[])
                } else {
                    None
                }
            } else {
                None
            };
            
            // Populate headers using Headers.set() method
            if let Some(headers_instance) = headers_obj {
                let set_key = v8::String::new(context_scope, "set").unwrap();
                if let Some(set_fn) = headers_instance.get(context_scope, set_key.into()) {
                    if set_fn.is_function() {
                        let set_method = set_fn.cast::<v8::Function>();
                        handler_ctx.request.headers().for_each(|name, value| {
                            let name_key = v8::String::new(context_scope, name).unwrap();
                            let value_str = v8::String::new(context_scope, value).unwrap();
                            let _ = set_method.call(context_scope, headers_instance.into(), &[name_key.into(), value_str.into()]);
                        });
                    }
                }
                let _ = init_obj.set(context_scope, headers_key.into(), headers_instance.into());
            } else {
                // Fallback: create plain headers object if Headers constructor not available
                let plain_headers = v8::Object::new(context_scope);
                handler_ctx.request.headers().for_each(|name, value| {
                    let name_key = v8::String::new(context_scope, name).unwrap();
                    let value_str = v8::String::new(context_scope, value).unwrap();
                    let _ = plain_headers.set(context_scope, name_key.into(), value_str.into());
                });
                let _ = init_obj.set(context_scope, headers_key.into(), plain_headers.into());
            }
            
            // Set body if present (as base64 string)
            if let Some(body) = handler_ctx.request.body() {
                let body_key = v8::String::new(context_scope, "body").unwrap();
                let base64_body = base64::encode(body);
                let body_val = v8::String::new(context_scope, &base64_body).unwrap();
                let _ = init_obj.set(context_scope, body_key.into(), body_val.into());
            }
            
            // Create Request instance
            let request_instance = request_fn.new_instance(context_scope, &[url_str.into(), init_obj.into()]);
            request_instance.map(|i| i.into())
        } else {
            None
        }
    } else {
        None
    };
    
    // Fallback: create plain object if Request constructor not available
    let request_value = match request_obj {
        Some(obj) => obj,
        None => {
            // Create plain object as fallback
            let obj = v8::Object::new(context_scope);
            
            let method_key = v8::String::new(context_scope, "method").unwrap();
            let method_val = v8::String::new(context_scope, handler_ctx.request.method()).unwrap();
            let _ = obj.set(context_scope, method_key.into(), method_val.into());
            
            let url_key = v8::String::new(context_scope, "url").unwrap();
            let url_val = v8::String::new(context_scope, &handler_ctx.request.url_string()).unwrap();
            let _ = obj.set(context_scope, url_key.into(), url_val.into());
            
            obj.into()
        }
    };

    // Call fetch function with request object
    let result = fetch_fn.call(context_scope, global.into(), &[request_value]);

    // Handle promise if needed
    if let Some(result_val) = result {
        // Check if it's a promise
        if result_val.is_promise() {
            let promise = result_val.cast::<v8::Promise>();
            // Run microtasks to settle promise
            context_scope.perform_microtask_checkpoint();

            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    let response_val = promise.result(context_scope);
                    return crate::runtime::handler::extract_js_response(context_scope, response_val);
                }
                v8::PromiseState::Rejected => {
                    let error = promise.result(context_scope);
                    let error_str = error.to_rust_string_lossy(context_scope);
                    return Ok(NanoResponse::new(500, NanoHeaders::new(), Some(format!("Promise rejected: {}", error_str).into())));
                }
                v8::PromiseState::Pending => {
                    return Ok(NanoResponse::new(500, NanoHeaders::new(), Some("Promise still pending".into())));
                }
            }
        } else {
            // Direct response (not a promise)
            return crate::runtime::handler::extract_js_response(context_scope, result_val);
        }
    }

    Ok(NanoResponse::new(500, NanoHeaders::new(), Some("Handler returned no result".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoRequest, NanoUrl};
    use tokio::sync::oneshot;

    fn create_dummy_request() -> NanoRequest {
        NanoRequest::new(
            "GET".to_string(),
            NanoUrl::parse("http://test/").unwrap(),
            NanoHeaders::new(),
            None,
        )
    }

    fn create_dummy_task() -> HandlerTask {
        let (tx, _rx) = oneshot::channel();
        HandlerTask {
            entrypoint: "/dev/null".to_string(),
            request: create_dummy_request(),
            response_tx: tx,
            hostname: "test.example.com".to_string(),
            start_time: std::time::Instant::now(),
            cpu_time_limit_ms: 0, // 0 means no limit for tests
        }
    }

    #[test]
    fn test_workqueue_creation() {
        let queue = WorkQueue::new(4);
        assert_eq!(queue.workers_per_pool, 4);
        assert_eq!(queue.channel_capacity, 256);
        assert_eq!(queue.pools.len(), 0);
    }

    #[test]
    fn test_get_or_create_pool() {
        let mut queue = WorkQueue::new(2);

        // Create pool for hostname
        let pool = queue.get_or_create_pool("test.example.com");
        assert_eq!(pool.hostname, "test.example.com");
        assert_eq!(pool.worker_count, 2);

        // Same hostname returns same pool
        let pool2 = queue.get_or_create_pool("test.example.com");
        assert_eq!(pool2.hostname, "test.example.com");

        // Stats updated
        assert_eq!(queue.stats.active_pools.load(Ordering::Relaxed), 1);
        assert_eq!(queue.stats.active_workers.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_hostname_hash_case_insensitive() {
        let hash1 = hash_hostname("Example.COM");
        let hash2 = hash_hostname("example.com");
        let hash3 = hash_hostname("EXAMPLE.COM");

        assert_eq!(hash1, hash2, "Hostname hashing should be case-insensitive");
        assert_eq!(hash2, hash3, "Hostname hashing should be case-insensitive");
    }

    #[test]
    fn test_multiple_hostname_pools() {
        let mut queue = WorkQueue::new(2);

        // Create pools for different hostnames
        queue.get_or_create_pool("app1.example.com");
        queue.get_or_create_pool("app2.example.com");

        assert_eq!(queue.stats.active_pools.load(Ordering::Relaxed), 2);
        assert_eq!(queue.stats.active_workers.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn test_affine_dispatch_consistency() {
        let mut queue = WorkQueue::new(4); // 4 workers per pool

        // Create pool
        let pool = queue.get_or_create_pool("app.example.com");
        let worker_count = pool.worker_count;

        // Calculate expected worker index for hostname
        let hostname_hash = hash_hostname("app.example.com");
        let expected_worker = (hostname_hash % worker_count as u64) as usize;

        // Verify same hostname always routes to same worker index
        for _ in 0..100 {
            let hash = hash_hostname("app.example.com");
            let worker_index = (hash % worker_count as u64) as usize;
            assert_eq!(
                worker_index, expected_worker,
                "Hostname should always route to same worker"
            );
        }
    }

    #[test]
    fn test_queue_error_display() {
        assert_eq!(
            QueueError::ChannelFull.to_string(),
            "WorkQueue channel is full"
        );
        assert_eq!(
            QueueError::WorkerNotFound.to_string(),
            "Worker thread not found"
        );
        assert_eq!(
            QueueError::PoolNotFound.to_string(),
            "Pool not found for hostname"
        );
        assert_eq!(
            QueueError::SendError("test".to_string()).to_string(),
            "Send error: test"
        );
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = QueueStats::new();
        let snapshot = stats.snapshot();

        assert_eq!(snapshot.tasks_submitted, 0);
        assert_eq!(snapshot.tasks_completed, 0);
        assert_eq!(snapshot.tasks_dropped, 0);
        assert_eq!(snapshot.active_pools, 0);
        assert_eq!(snapshot.active_workers, 0);
    }

    #[test]
    fn test_worker_pool_try_dispatch() {
        let pool = WorkerPool::new("test.local", 2);

        let (tx, _rx) = oneshot::channel();
        let task = HandlerTask {
            entrypoint: "/dev/null".to_string(),
            request: create_dummy_request(),
            response_tx: tx,
            hostname: "test.local".to_string(),
            start_time: std::time::Instant::now(),
            cpu_time_limit_ms: 0, // 0 means no limit for tests
        };

        // Should succeed (channel is empty)
        let result = pool.try_dispatch(task, 0);
        assert!(result.is_ok());
    }
}
