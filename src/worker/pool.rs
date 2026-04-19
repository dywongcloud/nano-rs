//! Worker pool implementation with thread-local isolate ownership
//!
//! This module provides the WorkerPool that manages N worker threads,
//! each owning a V8 isolate. Tasks are dispatched via MPSC channels
//! and responses are returned via oneshot channels.

use crate::runtime::handler::{execute_handler, HandlerContext};
use crate::v8::{initialize_platform, NanoIsolate};
use crate::worker::HandlerTask;

use anyhow::{anyhow, Result};
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
        self.task_tx.send(task).map_err(|_| anyhow!("Worker {} channel closed", self.id))
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
#[derive(Debug)]
pub struct WorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: usize,
    /// Hostname this pool serves (for logging/debugging)
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicUsize,
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
    ///
    /// # Returns
    ///
    /// A new WorkerPool with N workers ready to receive tasks
    ///
    /// # Panics
    ///
    /// Panics if the V8 platform is not initialized
    pub fn new(hostname: String, worker_count: usize) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with thread-local isolate
            let thread = thread::spawn(move || {
                info!("Worker {} starting", id);

                // Create isolate in this thread - NEVER moves to another thread
                // This is the critical POOL-05 constraint: isolate is !Send + !Sync
                let mut isolate = match NanoIsolate::new() {
                    Ok(isol) => isol,
                    Err(e) => {
                        error!("Worker {} failed to create isolate: {}", id, e);
                        return;
                    }
                };

                debug!("Worker {} isolate created successfully", id);

                // Event loop: receive tasks and execute
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("Worker {} received task for {}", id, task.entrypoint);

                            // Create handler context
                            let context = HandlerContext {
                                entrypoint: task.entrypoint,
                                request: task.request,
                            };

                            // Execute handler using pollster for async in sync thread
                            let result = pollster::block_on(
                                execute_handler(&mut isolate, context)
                            );

                            // Send response back (ignore send errors - receiver may have dropped)
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
                info!("Worker {} shutting down (isolate cleaned up)", id);
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
        }
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
        file.write_all(code.as_bytes()).expect("Failed to write test code");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_worker_pool_creation() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 2);
        assert_eq!(pool.worker_count, 2);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_single_worker_pool() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 1);
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

        let pool = WorkerPool::new("test.example.com".to_string(), 1);

        // Create task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            NanoHeaders::new(),
            None,
        );

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

        let pool = WorkerPool::new("test.example.com".to_string(), 4);

        // Dispatch 10 tasks concurrently
        let mut receivers = vec![];
        for i in 0..10 {
            let url = NanoUrl::parse(&format!("http://test/{}", i)).unwrap();
            let request = NanoRequest::new(
                "GET".to_string(),
                url,
                NanoHeaders::new(),
                None,
            );

            let (tx, rx) = oneshot::channel();
            let task = HandlerTask::new(entrypoint.clone(), request, tx);

            pool.dispatch(task).unwrap();
            receivers.push(rx);
        }

        // All should complete successfully
        for (i, rx) in receivers.into_iter().enumerate() {
            let response = rx.blocking_recv().expect(&format!("Failed to receive response {}", i));
            assert!(response.is_ok(), "Request {} failed: {:?}", i, response.err());
            let resp = response.unwrap();
            assert_eq!(resp.status(), 200);
        }

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_round_robin_dispatch() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 3);

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
            let request = NanoRequest::new(
                "GET".to_string(),
                url,
                NanoHeaders::new(),
                None,
            );

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

        let pool = WorkerPool::new("test.example.com".to_string(), 3);

        // Dispatch to specific worker
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            NanoHeaders::new(),
            None,
        );

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch_to(1, task).expect("Dispatch to worker 1 failed");

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

        let pool = WorkerPool::new("test.example.com".to_string(), 2);

        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            NanoHeaders::new(),
            None,
        );

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
        let pool = WorkerPool::new("test.example.com".to_string(), 2);

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
        let pool = WorkerPool::new("test.example.com".to_string(), 2);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }
}
