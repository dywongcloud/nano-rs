//! Worker pool architecture for multi-tenant JavaScript execution
//!
//! This module provides worker pool implementations for NANO, managing V8 isolate
//! execution across multiple threads with per-tenant isolation.
//!
//! ## Architecture Overview
//!
//! This module provides multiple worker pool implementations for different use cases:
//!
//! ### Pool Types
//!
//! - **SliverWorkerPool** ([`pool::SliverWorkerPool`]): For snapshot-based execution
//!   - Restores isolates from V8 snapshots (~1-2ms cold start)
//!   - Full feature set: CPU limits, memory monitoring, eviction
//!   - Use when: Running from sliver files (production deployment)
//!
//! - **EntrypointWorkerPool** ([`queue::EntrypointWorkerPool`]): For entrypoint dispatch
//!   - Creates fresh isolates from source files
//!   - Supports async creation with custom VFS backends
//!   - Use when: Dynamic app loading, development, testing
//!
//! - **WorkQueue** ([`queue::WorkQueue`]): Multi-hostname pool manager
//!   - Manages multiple EntrypointWorkerPools per hostname
//!   - Affine dispatch for cache locality
//!   - Use when: Multi-tenant hosting with dynamic pool creation
//!
//! - **WorkerPool** ([`pool::WorkerPool`]): Legacy pool (backward compatibility)
//!   - The original pool implementation from early phases
//!   - Maintained for existing tests and backward compatibility
//!   - New code should use SliverWorkerPool or EntrypointWorkerPool
//!
//! ### Trait-Based Design
//!
//! All pool types implement the [`WorkerPoolTrait`] for common operations:
//! - [`WorkerPoolTrait::dispatch`]: Send tasks to workers
//! - [`WorkerPoolTrait::shutdown`]: Graceful shutdown
//! - [`WorkerPoolTrait::worker_count`], [`WorkerPoolTrait::hostname`]: Introspection
//!
//! The trait is object-safe, enabling polymorphic usage:
//! ```rust,ignore
//! use nano::worker::{WorkerPoolTrait, SliverWorkerPool, EntrypointWorkerPool};
//!
//! fn use_pool(pool: &dyn WorkerPoolTrait) {
//!     println!("Pool {} has {} workers", pool.hostname(), pool.worker_count());
//! }
//! ```
//!
//! ### Pool Selection Guide
//!
//! | Use Case | Pool Type | Why |
//! |----------|-----------|-----|
//! | Production sliver execution | SliverWorkerPool | Fast snapshot restore, full features |
//! | Dynamic app loading | EntrypointWorkerPool | Async creation, VFS flexibility |
//! | Multi-tenant hosting | WorkQueue | Per-hostname pool management |
//! | Testing/development | EntrypointWorkerPool | Simple, no snapshot needed |
//! | Legacy compatibility | WorkerPool | Maintains backward compatibility |
//!
//! ### VFS Backend Configuration
//!
//! Both SliverWorkerPool and EntrypointWorkerPool support custom VFS backends:
//! - [`SliverWorkerPool::with_backend`]: Snapshot execution with custom storage
//! - [`EntrypointWorkerPool::with_backend`]: Entrypoint dispatch with custom storage
//! - [`WorkQueue::with_vfs_config`]: Async disk backend creation for multi-tenant
//!
//! ## Thread Safety
//!
//! Each worker thread creates and owns its `NanoIsolate` (thread-local ownership).
//! Isolates are `!Send + !Sync` via `PhantomData<*mut ()>`, preventing cross-thread
//! movement. This is critical for V8 stability (see POOL-05).
//!
//! ## Task Flow
//!
//! 1. HTTP layer creates a [`HandlerTask`] with entrypoint, request, and response channel
//! 2. WorkerPool dispatches the task via MPSC to a worker thread
//! 3. Worker thread executes the JavaScript handler using its isolate
//! 4. Response is sent back via oneshot channel
//!
//! ## Graceful Shutdown
//!
//! Calling [`WorkerPoolTrait::shutdown`] or dropping the pool signals workers to exit
//! via MPSC channel closure. All worker threads are joined to ensure clean isolate cleanup.

pub mod context;
pub mod cpu_tracker;
pub mod eviction;
pub mod limits;
pub mod memory_monitor;
pub mod oom;
pub mod pool;
pub mod queue;
pub mod timeout;
pub mod r#trait;

// Re-export types
pub use context::ContextManager;
pub use cpu_tracker::{CpuTimeError, CpuTimeSnapshot, CpuTracker};
pub use eviction::{EvictionAction, EvictionManager, EvictionPolicy, IsolateMetadata};
pub use limits::{HeapStatistics, MemoryLimiter, OomError};
pub use memory_monitor::{MemoryMonitor, MemoryMonitorConfig, MemoryPressureLevel, MemorySnapshot, MemoryTrend};
pub use oom::{OomMonitor, OomMonitorBuilder};
pub use pool::{SliverWorkerPool, WorkerHandle, WorkerPool};
pub use queue::{hash_hostname, EntrypointWorkerPool, QueueError, QueueStats, StatsSnapshot, WorkQueue};
pub use r#trait::{BoxedWorkerPool, WorkerPool as WorkerPoolTrait, WorkerPoolConfig};
pub use timeout::{ExecutionTimer, TimeoutConfig, TimeoutError};

use crate::http::{NanoRequest, NanoResponse};
use tokio::sync::oneshot;

/// Task sent to worker threads for JavaScript handler execution
///
/// This struct is `Send` so it can safely cross thread boundaries via MPSC channels.
/// The response is sent back via the oneshot channel.
#[derive(Debug)]
pub struct HandlerTask {
    /// Path to the JavaScript entrypoint file
    pub entrypoint: String,
    /// The incoming HTTP request (WinterCG-compatible)
    pub request: NanoRequest,
    /// Channel to send the response back to the caller
    pub response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
    /// Hostname (tenant identifier) for metrics tracking
    pub hostname: String,
    /// Start time for request duration tracking
    pub start_time: std::time::Instant,
    /// CPU time limit in milliseconds (0 = no limit)
    pub cpu_time_limit_ms: u32,
}

// Safety: NanoRequest is Clone + contains String/Bytes which are Send
// This explicit impl documents and verifies the Send contract
unsafe impl Send for HandlerTask {}

impl HandlerTask {
    /// Create a new handler task
    ///
    /// # Arguments
    ///
    /// * `entrypoint` - Path to the JavaScript file
    /// * `request` - The HTTP request to process
    /// * `response_tx` - Oneshot channel sender for the response
    pub fn new(
        entrypoint: String,
        request: NanoRequest,
        response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
    ) -> Self {
        Self {
            entrypoint,
            request,
            response_tx,
            hostname: String::new(),
            start_time: std::time::Instant::now(),
            cpu_time_limit_ms: 0,
        }
    }

    /// Create a new handler task with hostname for metrics tracking
    ///
    /// # Arguments
    ///
    /// * `entrypoint` - Path to the JavaScript file
    /// * `request` - The HTTP request to process
    /// * `response_tx` - Oneshot channel sender for the response
    /// * `hostname` - Tenant hostname for metrics tracking
    pub fn with_hostname(
        entrypoint: String,
        request: NanoRequest,
        response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
        hostname: String,
    ) -> Self {
        Self {
            entrypoint,
            request,
            response_tx,
            hostname,
            start_time: std::time::Instant::now(),
            cpu_time_limit_ms: 0,
        }
    }

    /// Create a new handler task with hostname and CPU limits
    ///
    /// # Arguments
    ///
    /// * `entrypoint` - Path to the JavaScript file
    /// * `request` - The HTTP request to process
    /// * `response_tx` - Oneshot channel sender for the response
    /// * `hostname` - Tenant hostname for metrics tracking
    /// * `cpu_time_limit_ms` - CPU time limit in milliseconds (0 = no limit)
    pub fn with_hostname_and_limits(
        entrypoint: String,
        request: NanoRequest,
        response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
        hostname: String,
        cpu_time_limit_ms: u32,
    ) -> Self {
        Self {
            entrypoint,
            request,
            response_tx,
            hostname,
            start_time: std::time::Instant::now(),
            cpu_time_limit_ms,
        }
    }

    /// Set CPU time limit
    pub fn with_cpu_limit(mut self, cpu_time_limit_ms: u32) -> Self {
        self.cpu_time_limit_ms = cpu_time_limit_ms;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoUrl};

    #[test]
    fn test_handler_task_creation() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, _rx) = oneshot::channel();
        let task = HandlerTask::new("/app/index.js".to_string(), request, tx);

        assert_eq!(task.entrypoint, "/app/index.js");
        assert_eq!(task.request.method(), "GET");
    }

    #[test]
    fn test_handler_task_is_send() {
        // Compile-time check: HandlerTask must be Send
        fn assert_send<T: Send>() {}
        assert_send::<HandlerTask>();
    }
}
