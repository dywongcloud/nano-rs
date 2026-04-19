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
use std::thread::{self, JoinHandle};

use crate::http::{NanoRequest, NanoResponse};
use crate::worker::HandlerTask;

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

            // Spawn worker thread
            let hostname_thread = hostname_owned.clone();
            let thread = thread::spawn(move || {
                tracing::info!("Worker {} started for {}", id, hostname_thread);

                // Worker loop - blocks on channel receive
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            // Execute the handler task
                            tracing::debug!("Worker {} received task", id);

                            // For now, return a simple response
                            // In full implementation, this would call the JS handler
                            let response = NanoResponse::ok()
                                .with_header("Content-Type", "text/plain")
                                .with_body(format!("Handler executed: {}", task.entrypoint));

                            // Send response back
                            let _ = task.response_tx.send(Ok(response));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoUrl};
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
        };

        // Should succeed (channel is empty)
        let result = pool.try_dispatch(task, 0);
        assert!(result.is_ok());
    }
}
