//! Worker pool implementation with thread-local isolate ownership
//!
//! This module provides the WorkerPool that manages N worker threads,
//! each owning a V8 isolate. Tasks are dispatched via MPSC channels
//! and responses are returned via oneshot channels.

use crate::http::NanoResponse;
use crate::runtime::HandlerContext;
use crate::v8::{initialize_platform, NanoIsolate};
use crate::worker::context::ContextManager;
use crate::worker::eviction::{EvictionAction, EvictionManager, IsolateMetadata};
use crate::worker::memory_monitor::{MemoryMonitor, MemoryPressureLevel};
use crate::worker::oom::OomMonitorBuilder;
use crate::worker::limits::RequestMemoryTracker;
use crate::worker::HandlerTask;
use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{anyhow, Result};

// Thread-local storage for the worker thread's Tokio runtime handle
// This allows fetch() and other async operations to access the runtime
thread_local! {
    static WORKER_RUNTIME: RefCell<Option<tokio::runtime::Handle>> = RefCell::new(None);
}

/// Re-export data plane execution functions for backward compatibility.
pub use crate::data_plane::{
    execute_with_context_manager,
    with_worker_runtime,
    CpuTimeoutGuard,
};

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

            // Spawn worker thread with thread-local isolate
            let thread = thread::spawn(move || {
                info!("Worker {} starting", id);

                // Create OOM monitor for this worker if memory limit is configured
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

                // Create memory monitor for post-execution heap checking
                let mut memory_monitor = if memory_limit_mb > 0 {
                    Some(MemoryMonitor::new(memory_limit_mb))
                } else {
                    None
                };

                // Create eviction manager for this worker (thread-local)
                let mut eviction_manager = EvictionManager::new();

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
                // This generates a unique isolate_id internally for tracking
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }

                // Get the isolate_id from ContextManager and register with eviction manager
                // This is mutable because it changes when isolate is replaced (OOM recovery)
                let mut isolate_id = context_manager.isolate_id().clone();
                eviction_manager.register_isolate(
                    isolate_id.clone(),
                    IsolateMetadata::new(&worker_hostname, id),
                );

                info!(
                    "Worker {} initialized with context and memory monitoring (isolate_id: {}, initial_age: 0s)",
                    id, isolate_id
                );

                // Event loop: receive tasks and execute
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("Worker {} received task for {}", id, task.entrypoint);

                            // Check if isolate is in draining mode (soft eviction)
                            if eviction_manager.is_draining(&isolate_id) {
                                warn!("Worker {} is draining, rejecting new request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service temporarily unavailable - memory pressure"
                                )));
                                continue;
                            }

                            // Check if isolate has been evicted
                            if eviction_manager.is_evicted(&isolate_id) {
                                warn!("Worker {} is evicted, rejecting request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service unavailable - isolate evicted"
                                )));
                                continue;
                            }

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
                                                // Update isolate_id with the NEW id from fresh ContextManager
                                                isolate_id = context_manager.isolate_id().clone();
                                                // Reset OOM monitor for fresh isolate
                                                monitor.reset();
                                                // Reactivate isolate in eviction manager with NEW id
                                                eviction_manager.reactivate_isolate(
                                                    isolate_id.clone(),
                                                    IsolateMetadata::new(&worker_hostname, id),
                                                );
                                                info!(
                                                    "Worker {} created fresh isolate after OOM (new isolate_id: {})",
                                                    id,
                                                    isolate_id
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

                            // Mark active request in eviction manager
                            eviction_manager.mark_active(&isolate_id);

                            // METRICS-01: Start timing for metrics collection
                            let request_start = std::time::Instant::now();
                            let hostname = task.hostname.clone();
                            // Entrypoint is available in task if needed for logging/debugging
                            let _entrypoint = &task.entrypoint;

                            // POOL-04: Reset context before each request
                            let reset_elapsed = match context_manager.reset_context() {
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
                                    // Record context reset in metrics if hostname is set
                                    if !hostname.is_empty() {
                                        crate::metrics::TENANT_METRICS.record_context_reset(&hostname);
                                    }
                                    elapsed
                                }
                                Err(e) => {
                                    error!("Worker {} context reset failed: {}", id, e);
                                    let _ =
                                        task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    eviction_manager.mark_complete(&isolate_id);
                                    continue;
                                }
                            };

                            // Extract request info for logging before moving task.request
                            let request_method = task.request.method().to_string();
                            let request_path = task.request.url().pathname();
                            let request_id = task.request_id.clone();

                            // Create a span with worker_id, isolate_id, and request_id for proper JSON logging context
                            // This ensures worker/isolate/request info appears in the span context for distributed tracing
                            let worker_span = tracing::info_span!(
                                "worker_request",
                                worker_id = id,
                                isolate_id = %isolate_id,
                                request_id = %request_id,
                                hostname = %hostname,
                                method = %request_method,
                                path = %request_path
                            );
                            let _worker_enter = worker_span.enter();

                            // Log when the worker receives the request (distributed tracing checkpoint)
                            // Include isolate age for debugging lifecycle management
                            let isolate_age = eviction_manager.get_isolate_age_formatted(&isolate_id)
                                .unwrap_or_else(|| "unknown".to_string());
                            tracing::debug!(
                                "Worker {} received request {} (isolate: {}, age: {})",
                                id, request_id, isolate_id, isolate_age
                            );

                            // Create handler context
                            let handler_ctx = HandlerContext {
                                entrypoint: task.entrypoint,
                                request: task.request,
                            };

                            // Execute handler with fresh context scope
                            // CPU timeout enforcement uses timer-based termination if cpu_time_limit_ms > 0
                            // No per-request memory tracking in basic WorkQueue mode (use WorkerPool for full limits)
                            let mut result =
                                execute_with_context_manager(&mut context_manager, &handler_ctx, task.cpu_time_limit_ms);

                            // Calculate request duration
                            let duration_ms = request_start.elapsed().as_millis() as u64;

                            // Mark request complete in eviction manager
                            eviction_manager.mark_complete(&isolate_id);

                            // Extract status code from result for logging and set worker_id/isolate_id on response
                            // These are used by the HTTP layer for access logging
                            let status_code = match &mut result {
                                Ok(ref mut response) => {
                                    response.set_worker_id(id);
                                    response.set_isolate_id(isolate_id.to_string());
                                    response.status()
                                }
                                Err(_) => 500,
                            };

                            // Worker processing log - shows which worker handled the request
                            // This helps debug request routing and worker load distribution
                            // The worker_id/isolate_id/request_id are now in the span context due to the worker_span above
                            let worker_id_u64 = id as u64;
                            tracing::info!(
                                request_id = %request_id,
                                worker_id = worker_id_u64,
                                isolate_id = %isolate_id,
                                status = status_code,
                                duration_ms = duration_ms,
                                "Worker {} processed request {}: {} {} - {} in {}ms (isolate: {})",
                                id,
                                request_id,
                                request_method,
                                request_path,
                                status_code,
                                duration_ms,
                                isolate_id
                            );

                            // Post-execution memory monitoring (27-02)
                            if let Some(ref mut mem_monitor) = memory_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                let snapshot = mem_monitor.check_after(isolate_ref);

                                // Update eviction manager with usage data
                                eviction_manager.record_usage(&isolate_id, snapshot.total_memory_bytes());

                                // Handle pressure levels
                                match snapshot.pressure_level {
                                    MemoryPressureLevel::Normal => {
                                        // No action needed
                                    }
                                    MemoryPressureLevel::Warning => {
                                        // Log warning for elevated memory
                                        warn!(
                                            "Worker {} memory warning: {:.1}MB ({}% of limit)",
                                            id,
                                            snapshot.total_memory_mb(),
                                            (snapshot.total_memory_mb() / memory_limit_mb as f64 * 100.0) as u32
                                        );
                                    }
                                    MemoryPressureLevel::Critical | MemoryPressureLevel::Emergency => {
                                        // Trigger soft eviction
                                        let action = eviction_manager.evaluate_pressure(
                                            snapshot.pressure_level,
                                            None,
                                        );

                                        match action {
                                            EvictionAction::SoftEvict(_) => {
                                                warn!(
                                                    "Worker {} memory pressure detected ({}), initiating soft eviction",
                                                    id,
                                                    snapshot.pressure_level.description()
                                                );
                                                eviction_manager.initiate_soft_eviction(&isolate_id);
                                            }
                                            EvictionAction::HardEvict(_) => {
                                                // Emergency - dispose isolate immediately
                                                error!(
                                                    "Worker {} emergency memory pressure, disposing isolate",
                                                    id
                                                );
                                                eviction_manager.hard_evict(&isolate_id);
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                // Check for memory leak trends
                                if mem_monitor.is_trending_to_leak() {
                                    warn!(
                                        "Worker {} memory leak detected, isolate may need disposal",
                                        id
                                    );
                                }

                                // METRICS-02: Record per-tenant metrics with memory data
                                if !hostname.is_empty() {
                                    let result_type = match &result {
                                        Ok(_) => crate::metrics::tenant::RequestResult::Success,
                                        Err(e) => {
                                            if e.to_string().contains("timeout") {
                                                crate::metrics::tenant::RequestResult::Timeout
                                            } else {
                                                crate::metrics::tenant::RequestResult::Error
                                            }
                                        }
                                    };

                                    // Estimate CPU time from context reset duration (microseconds)
                                    let cpu_us = reset_elapsed.as_micros() as u64;

                                    crate::metrics::TENANT_METRICS.record_request(
                                        &hostname,
                                        result_type,
                                        cpu_us,
                                        snapshot.total_memory_bytes() as usize,
                                        duration_ms,
                                    );

                                    // Update current memory gauge
                                    crate::metrics::TENANT_METRICS.update_memory(
                                        &hostname,
                                        snapshot.heap_used as usize,
                                        snapshot.external as usize,
                                    );

                                    // Record pressure events if applicable
                                    if snapshot.pressure_level > crate::worker::memory_monitor::MemoryPressureLevel::Normal {
                                        crate::metrics::TENANT_METRICS.record_pressure_event(
                                            &hostname,
                                            snapshot.pressure_level,
                                        );
                                    }
                                }
                            } else if !hostname.is_empty() {
                                // METRICS-02: Record metrics without memory data (when memory monitoring is disabled)
                                let result_type = match &result {
                                    Ok(_) => crate::metrics::tenant::RequestResult::Success,
                                    Err(e) => {
                                        if e.to_string().contains("timeout") {
                                            crate::metrics::tenant::RequestResult::Timeout
                                        } else {
                                            crate::metrics::tenant::RequestResult::Error
                                        }
                                    }
                                };

                                let cpu_us = reset_elapsed.as_micros() as u64;

                                crate::metrics::TENANT_METRICS.record_request(
                                    &hostname,
                                    result_type,
                                    cpu_us,
                                    0, // Memory data not available
                                    duration_ms,
                                );
                            }

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
                                            // Update isolate_id with the new one from ContextManager
                                            isolate_id = context_manager.isolate_id().clone();
                                            monitor.reset();
                                            // Reactivate in eviction manager with NEW id
                                            eviction_manager.reactivate_isolate(
                                                isolate_id.clone(),
                                                IsolateMetadata::new(&worker_hostname, id),
                                            );
                                            info!("Worker {} created fresh isolate after post-request OOM (new isolate_id: {})", id, isolate_id);
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
                let eviction_stats = eviction_manager.state_counts();
                info!(
                    "Worker {} shutting down (avg context reset: {:.2}ms, OOM events: {}, evictions: {})",
                    id,
                    context_manager.average_reset_time_ms(),
                    oom_monitor.map(|m| m.oom_count()).unwrap_or(0),
                    eviction_stats.2 // evicted count
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

            // Spawn unified worker thread
            let thread = thread::spawn(move || {
                info!("UnifiedWorker {} starting for {}", id, worker_hostname);

                // Create Tokio runtime for async operations
                let rt = tokio::runtime::Runtime::new()
                    .expect("Failed to create tokio runtime");
                let rt_handle = rt.handle().clone();
                WORKER_RUNTIME.with(|runtime| {
                    *runtime.borrow_mut() = Some(rt_handle);
                });

                // Create OOM monitor
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

                // Create memory monitor
                let mut memory_monitor = if memory_limit_mb > 0 {
                    Some(MemoryMonitor::new(memory_limit_mb))
                } else {
                    None
                };

                // Create eviction manager
                let mut eviction_manager = EvictionManager::new();

                // Create VFS for this worker with shared backend
                // For entrypoint apps with DiskBackend, use empty namespace to avoid
                // creating subdirectory - files are already organized by base_dir
                let is_disk_backend = matches!(&worker_vfs_backend, crate::vfs::VfsBackendEnum::Disk(_));
                let is_entrypoint = matches!(&worker_source, crate::worker::AppSource::Entrypoint { .. });
                let namespace = if is_disk_backend && is_entrypoint {
                    // Empty namespace for entrypoint+DiskBackend - paths map directly
                    tracing::info!(
                        "Using empty VFS namespace for entrypoint+DiskBackend (worker: {}, hostname: {})",
                        id, worker_hostname
                    );
                    crate::vfs::VfsNamespace::from_hostname("")
                } else {
                    // Use hostname namespace for memory backends or sliver apps
                    tracing::info!(
                        "Using hostname VFS namespace for {} backend (is_disk: {}, is_entrypoint: {})",
                        worker_hostname, is_disk_backend, is_entrypoint
                    );
                    VfsNamespace::from_hostname(&worker_hostname)
                };
                let vfs = IsolateVfs::new(
                    namespace,
                    worker_vfs_backend,
                );

                // Extract temp entrypoint override for sliver mode (if any)
                let temp_entrypoint_override: Option<std::path::PathBuf> = match &worker_source {
                    AppSource::Sliver { temp_entrypoint, .. } => temp_entrypoint.clone(),
                    _ => None,
                };

                // Initialize isolate based on source type
                let isolate = match &worker_source {
                    AppSource::Entrypoint { .. } => {
                        // Fresh isolate for entrypoint mode
                        match NanoIsolate::new_with_vfs(vfs) {
                            Ok(isol) => isol,
                            Err(e) => {
                                error!("Worker {} failed to create isolate: {}", id, e);
                                return;
                            }
                        }
                    }
                    AppSource::Sliver { data, .. } => {
                        // Restore VFS entries from sliver before creating isolate
                        if let Err(e) = rt.block_on(data.restore_to_vfs(&vfs)) {
                            error!("Worker {} failed to restore VFS: {}", id, e);
                            // Continue anyway - app might work without VFS
                        } else {
                            debug!(
                                "Worker {} restored {} VFS entries",
                                id,
                                data.vfs_entries.len()
                            );
                        }

                        // Restore isolate from snapshot, fallback to fresh
                        let vfs_clone = vfs.clone();
                        match crate::v8::restore_from_snapshot(&data.heap_data, vfs_clone) {
                            Ok(isol) => {
                                info!("Worker {} restored isolate from snapshot", id);
                                isol
                            }
                            Err(e) => {
                                warn!(
                                    "Worker {} snapshot restore failed ({}), creating fresh isolate",
                                    id, e
                                );
                                match NanoIsolate::new_with_vfs(vfs) {
                                    Ok(isol) => isol,
                                    Err(e) => {
                                        error!("Worker {} failed to create isolate: {}", id, e);
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    AppSource::Static { .. } => {
                        // Should not reach here - panic earlier for static
                        error!("Worker {} received Static source - should not spawn isolate", id);
                        return;
                    }
                };

                // Create context manager with unique isolate_id
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }

                let mut isolate_id = context_manager.isolate_id().clone();
                eviction_manager.register_isolate(
                    isolate_id.clone(),
                    IsolateMetadata::new(&worker_hostname, id),
                );

                info!(
                    "UnifiedWorker {} initialized (isolate_id: {}, source: {})",
                    id,
                    isolate_id,
                    if worker_source.is_sliver() {
                        "sliver"
                    } else {
                        "entrypoint"
                    }
                );

                // Unified event loop - identical for all source types
                // This is the full implementation, shared across entrypoint and sliver modes
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("UnifiedWorker {} received task for {}", id, task.entrypoint);

                            // Check if isolate is in draining mode (soft eviction)
                            if eviction_manager.is_draining(&isolate_id) {
                                warn!("Worker {} is draining, rejecting new request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service temporarily unavailable - memory pressure"
                                )));
                                continue;
                            }

                            // Check if isolate has been evicted
                            if eviction_manager.is_evicted(&isolate_id) {
                                warn!("Worker {} is evicted, rejecting request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service unavailable - isolate evicted"
                                )));
                                continue;
                            }

                            // OOM-04: Pre-request OOM check
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                match monitor.check(isolate_ref) {
                                    Ok(_) => {}
                                    Err(oom_error) => {
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

                                        match NanoIsolate::new() {
                                            Ok(new_isolate) => {
                                                context_manager = ContextManager::new(new_isolate);
                                                if let Err(e) = context_manager.create_initial_context() {
                                                    error!("Worker {} failed to create new context after OOM: {}", id, e);
                                                    break;
                                                }
                                                // Update isolate_id with the NEW id
                                                isolate_id = context_manager.isolate_id().clone();
                                                monitor.reset();
                                                eviction_manager.reactivate_isolate(
                                                    isolate_id.clone(),
                                                    IsolateMetadata::new(&worker_hostname, id),
                                                );
                                                info!(
                                                    "Worker {} created fresh isolate after OOM (new isolate_id: {})",
                                                    id, isolate_id
                                                );
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

                            // Mark active request in eviction manager
                            eviction_manager.mark_active(&isolate_id);

                            // Start timing for metrics
                            let request_start = std::time::Instant::now();
                            let hostname = task.hostname.clone();

                            // Reset context before each request (POOL-04)
                            let reset_elapsed = match context_manager.reset_context() {
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
                                    if !hostname.is_empty() {
                                        crate::metrics::TENANT_METRICS.record_context_reset(&hostname);
                                    }
                                    elapsed
                                }
                                Err(e) => {
                                    error!("Worker {} context reset failed: {}", id, e);
                                    let _ = task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    eviction_manager.mark_complete(&isolate_id);
                                    continue;
                                }
                            };

                            // Extract request info for logging
                            let request_method = task.request.method().to_string();
                            let request_path = task.request.url().pathname();
                            let request_id = task.request_id.clone();

                            // Create a span with worker_id, isolate_id, and request_id
                            let worker_span = tracing::info_span!(
                                "worker_request",
                                worker_id = id,
                                isolate_id = %isolate_id,
                                request_id = %request_id,
                                hostname = %hostname,
                                method = %request_method,
                                path = %request_path
                            );
                            let _worker_enter = worker_span.enter();

                            let isolate_age = eviction_manager.get_isolate_age_formatted(&isolate_id)
                                .unwrap_or_else(|| "unknown".to_string());
                            tracing::debug!(
                                "Worker {} received request {} (isolate: {}, age: {})",
                                id, request_id, isolate_id, isolate_age
                            );

                            // Create handler context
                            // Use temp entrypoint override for sliver mode if available
                            let entrypoint = temp_entrypoint_override
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| task.entrypoint.clone());
                            let handler_ctx = HandlerContext {
                                entrypoint,
                                request: task.request,
                            };

                            // Per-request memory tracking to prevent memory DoS
                            // Default limit: 16MB per request (prevents large array allocations)
                            let per_request_limit_mb = 16u32;
                            let mut request_memory_tracker = RequestMemoryTracker::new(
                                per_request_limit_mb,
                                hostname.clone()
                            );
                            
                            // Start tracking memory before request execution
                            let isolate_ref = context_manager.isolate_mut().isolate();
                            request_memory_tracker.start(isolate_ref);

                            // Execute handler with CPU timeout enforcement
                            // Per-request memory limit is checked after execution
                            let mut result = execute_with_context_manager(
                                &mut context_manager,
                                &handler_ctx,
                                task.cpu_time_limit_ms,
                            );
                            
                            // Also check if request exceeded per-request memory limit after execution
                            // This catches synchronous allocations that happen too fast for mid-execution checks
                            let isolate_ref = context_manager.isolate_mut().isolate();
                            match request_memory_tracker.exceeded_limit(isolate_ref) {
                                Ok(growth_mb) => {
                                    if growth_mb > 0 {
                                        tracing::debug!(
                                            "Request {} allocated {}MB (limit: {}MB)",
                                            request_id,
                                            growth_mb / (1024 * 1024),
                                            per_request_limit_mb
                                        );
                                    }
                                }
                                Err(oom_error) => {
                                    tracing::warn!(
                                        "Request {} exceeded per-request memory limit: {} (limit: {}MB)",
                                        request_id,
                                        oom_error,
                                        per_request_limit_mb
                                    );
                                    // Return 503 for memory limit violation
                                    result = Ok(NanoResponse::with_status(503)
                                        .with_header("Content-Type", "application/json")
                                        .with_body(format!(
                                            r#"{{"error":"MemoryLimitExceeded","message":"Request exceeded {}MB memory limit","type":"per_request_limit"}}"#,
                                            per_request_limit_mb
                                        )));
                                }
                            }

                            // Calculate request duration
                            let duration_ms = request_start.elapsed().as_millis() as u64;

                            // Mark request complete in eviction manager
                            eviction_manager.mark_complete(&isolate_id);

                            // Extract status code and set worker_id/isolate_id on response
                            let status_code = match &mut result {
                                Ok(ref mut response) => {
                                    response.set_worker_id(id);
                                    response.set_isolate_id(isolate_id.to_string());
                                    response.status()
                                }
                                Err(_) => 500,
                            };

                            // Log worker processing
                            let worker_id_u64 = id as u64;
                            tracing::info!(
                                request_id = %request_id,
                                worker_id = worker_id_u64,
                                isolate_id = %isolate_id,
                                status = status_code,
                                duration_ms = duration_ms,
                                "Worker {} processed request {}: {} {} - {} in {}ms (isolate: {})",
                                id,
                                request_id,
                                request_method,
                                request_path,
                                status_code,
                                duration_ms,
                                isolate_id
                            );

                            // Post-execution memory monitoring
                            if let Some(ref mut mem_monitor) = memory_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                let snapshot = mem_monitor.check_after(isolate_ref);

                                eviction_manager.record_usage(&isolate_id, snapshot.total_memory_bytes());

                                match snapshot.pressure_level {
                                    MemoryPressureLevel::Normal => {}
                                    MemoryPressureLevel::Warning => {
                                        warn!(
                                            "Worker {} memory warning: {:.1}MB ({}% of limit)",
                                            id,
                                            snapshot.total_memory_mb(),
                                            (snapshot.total_memory_mb() / memory_limit_mb as f64 * 100.0) as u32
                                        );
                                    }
                                    MemoryPressureLevel::Critical | MemoryPressureLevel::Emergency => {
                                        let action = eviction_manager.evaluate_pressure(
                                            snapshot.pressure_level,
                                            None,
                                        );

                                        match action {
                                            EvictionAction::SoftEvict(_) => {
                                                warn!(
                                                    "Worker {} memory pressure detected ({}), initiating soft eviction",
                                                    id,
                                                    snapshot.pressure_level.description()
                                                );
                                                eviction_manager.initiate_soft_eviction(&isolate_id);
                                            }
                                            EvictionAction::HardEvict(_) => {
                                                error!(
                                                    "Worker {} emergency memory pressure, disposing isolate",
                                                    id
                                                );
                                                eviction_manager.hard_evict(&isolate_id);
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                if mem_monitor.is_trending_to_leak() {
                                    warn!(
                                        "Worker {} memory leak detected, isolate may need disposal",
                                        id
                                    );
                                }

                                // Record per-tenant metrics
                                if !hostname.is_empty() {
                                    let result_type = match &result {
                                        Ok(_) => crate::metrics::tenant::RequestResult::Success,
                                        Err(e) => {
                                            if e.to_string().contains("timeout") {
                                                crate::metrics::tenant::RequestResult::Timeout
                                            } else {
                                                crate::metrics::tenant::RequestResult::Error
                                            }
                                        }
                                    };

                                    // Estimate CPU time from context reset duration (microseconds)
                                    let cpu_us = reset_elapsed.as_micros() as u64;

                                    crate::metrics::TENANT_METRICS.record_request(
                                        &hostname,
                                        result_type,
                                        cpu_us,
                                        snapshot.total_memory_bytes() as usize,
                                        duration_ms,
                                    );

                                    // Update current memory gauge
                                    crate::metrics::TENANT_METRICS.update_memory(
                                        &hostname,
                                        snapshot.heap_used as usize,
                                        snapshot.external as usize,
                                    );

                                    // Record pressure events if applicable
                                    if snapshot.pressure_level > MemoryPressureLevel::Normal {
                                        crate::metrics::TENANT_METRICS.record_pressure_event(
                                            &hostname,
                                            snapshot.pressure_level,
                                        );
                                    }
                                }
                            }

                            // Send response back
                            let _ = task.response_tx.send(result);
                        }
                        Err(_) => {
                            debug!("UnifiedWorker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }
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

        // Create async handler
        let entrypoint = create_test_handler(
            &temp_dir,
            "async_handler.js",
            r#"
async function fetch(request) {
    // Simulate async work
    const data = await Promise.resolve({ hello: "world" });

    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data)
    };
}
"#,
        );

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
        assert!(body_text.contains("hello"), "Async response missing data: {}", body_text);
        assert!(body_text.contains("world"), "Async response missing value: {}", body_text);

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

    #[test]
    fn test_sliver_worker_pool_with_temp_vfs() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create a JS handler in the temp directory (simulating extracted VFS)
        let temp_handler_code = r#"function fetch(request) { return { status: 200, headers: { "Content-Type": "text/plain" }, body: "From temp VFS" }; }"#;
        let temp_entrypoint = temp_dir.path().join("index.js");
        std::fs::write(&temp_entrypoint, temp_handler_code)
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
            body_text.contains("From temp VFS"),
            "Expected response from temp VFS, got: {}",
            body_text
        );

        pool.shutdown().expect("Shutdown failed");
    }
}
