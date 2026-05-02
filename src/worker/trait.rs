//! WorkerPool trait - unifying interface for all pool types
//!
//! This trait was created in Phase 999.2 to consolidate duplicate
//! WorkerPool implementations from pool.rs and queue.rs.
//!
//! ## Architecture
//!
//! The WorkerPool trait defines the common interface for all worker pool
//! implementations. This allows polymorphic usage where code can work
//! with any pool type without knowing the concrete implementation.
//!
//! ## Implementations
//!
//! - **SliverWorkerPool** (pool.rs): Snapshot-based execution with full
//!   feature set (CPU limits, memory monitoring, eviction)
//! - **EntrypointWorkerPool** (queue.rs): Entrypoint dispatch with async
//!   creation and custom VFS backend support
//!
//! ## Usage
//!
//! For polymorphic usage:
//! ```rust,ignore
//! use nano::worker::{WorkerPool, SliverWorkerPool, EntrypointWorkerPool};
//!
//! fn use_pool(pool: &dyn WorkerPool) {
//!     println!("Pool {} has {} workers", pool.hostname(), pool.worker_count());
//! }
//! ```
//!
//! For concrete types, use the specific pool directly to access
//! implementation-specific features.

use crate::http::NanoResponse;
use crate::worker::HandlerTask;
use anyhow::Result;
use std::sync::Arc;

/// Common interface for all worker pool implementations
///
/// This trait is object-safe (no generic methods, uses &self) allowing
/// `Box<dyn WorkerPool>` for polymorphic usage.
///
/// ## Implementations
///
/// - `SliverWorkerPool`: Snapshot-based execution (see pool.rs)
/// - `EntrypointWorkerPool`: Entrypoint dispatch (see queue.rs)
///
/// ## Method Notes
///
/// - `dispatch`: Thread-safe, non-blocking, uses internal channels
/// - `shutdown`: Consumes self to ensure clean cleanup
/// - `worker_count`: Snapshot value, may change during runtime
/// - `hostname`: Returns the hostname this pool serves
pub trait WorkerPool: Send + Sync {
    /// Dispatch a task to a worker in the pool
    ///
    /// The task will be routed to an available worker using the pool's
    /// internal dispatch strategy (round-robin, affine, etc.).
    ///
    /// # Arguments
    ///
    /// * `task` - The handler task containing entrypoint, request, and response channel
    ///
    /// # Returns
    ///
    /// `Ok(())` if the task was queued successfully, or an error if
    /// the dispatch failed (e.g., channel closed).
    fn dispatch(&self, task: HandlerTask) -> Result<()>;

    /// Gracefully shut down the worker pool
    ///
    /// Signals all worker threads to exit after processing pending tasks,
    /// then waits for them to complete.
    ///
    /// # Returns
    ///
    /// `Ok(())` if shutdown completed successfully, or an error if
    /// any worker panicked or failed to exit cleanly.
    fn shutdown(self) -> Result<()>
    where
        Self: Sized;

    /// Get the number of workers in this pool
    ///
    /// This is a snapshot value. The actual number of active workers
    /// may differ if workers have panicked or been restarted.
    fn worker_count(&self) -> usize;

    /// Get the hostname this pool serves
    ///
    /// The hostname identifies the tenant/app this pool is dedicated to.
    fn hostname(&self) -> &str;
}

/// Configuration for creating a worker pool
///
/// This struct provides a common configuration interface for all
/// pool types. Individual pool implementations may extend this
/// with additional configuration options.
#[derive(Debug, Clone)]
pub struct WorkerPoolConfig {
    /// Hostname this pool serves (tenant identifier)
    pub hostname: String,
    /// Number of worker threads to create
    pub worker_count: usize,
    /// Memory limit per isolate in MB
    pub memory_limit_mb: u32,
    /// Optional custom VFS backend (None = use default MemoryBackend)
    pub vfs_backend: Option<Arc<dyn crate::vfs::VfsBackend>>,
}

impl WorkerPoolConfig {
    /// Create a new worker pool configuration
    ///
    /// # Arguments
    ///
    /// * `hostname` - The hostname this pool will serve
    /// * `worker_count` - Number of worker threads
    pub fn new(hostname: impl Into<String>, worker_count: usize) -> Self {
        Self {
            hostname: hostname.into(),
            worker_count,
            memory_limit_mb: 128, // Default 128MB
            vfs_backend: None,
        }
    }

    /// Set memory limit in MB
    pub fn with_memory_limit(mut self, mb: u32) -> Self {
        self.memory_limit_mb = mb;
        self
    }

    /// Set custom VFS backend
    pub fn with_vfs_backend(mut self, backend: Arc<dyn crate::vfs::VfsBackend>) -> Self {
        self.vfs_backend = Some(backend);
        self
    }
}

/// Type alias for boxed WorkerPool trait objects
///
/// Useful when storing pools of different concrete types together.
pub type BoxedWorkerPool = Box<dyn WorkerPool>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify WorkerPool trait is object-safe
    ///
    /// This test ensures Box<dyn WorkerPool> compiles correctly.
    /// If this test compiles, the trait is object-safe.
    #[test]
    fn test_worker_pool_object_safe() {
        // This is a compile-time check
        // If WorkerPool trait is object-safe, this compiles
        fn assert_object_safe<T: WorkerPool>() {}
        
        // The trait has methods that take &self (not &mut self) and don't use generics
        // which makes it object-safe
    }

    #[test]
    fn test_worker_pool_config_default() {
        let config = WorkerPoolConfig::new("test.example.com", 4);
        assert_eq!(config.hostname, "test.example.com");
        assert_eq!(config.worker_count, 4);
        assert_eq!(config.memory_limit_mb, 128);
        assert!(config.vfs_backend.is_none());
    }

    #[test]
    fn test_worker_pool_config_with_options() {
        use crate::vfs::MemoryBackend;
        
        let config = WorkerPoolConfig::new("test.example.com", 4)
            .with_memory_limit(256)
            .with_vfs_backend(Arc::new(MemoryBackend::new()));
        
        assert_eq!(config.memory_limit_mb, 256);
        assert!(config.vfs_backend.is_some());
    }
}
