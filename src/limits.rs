//! Static memory allocation limits per TigerStyle principle
//!
//! This module defines all runtime resource limits as compile-time constants.
//! Per TigerStyle: allocate all memory at startup, no dynamic allocation after initialization.
//!
//! ## Naming Convention
//!
//! All limits use big-endian naming (*_max at end) per TigerStyle:
//! - `heap_size_bytes_max` not `max_heap_size_bytes`
//! - `isolate_pool_size_max` not `max_isolate_pool_size`
//!
//! ## Resource Groups
//!
//! - **Isolate Pool**: Pre-allocated isolates with fixed heap sizes
//! - **Work Queue**: Bounded channel with hard maximum capacity
//! - **Buffer Pools**: Reused request/response buffers
//! - **Script/Execution**: Limits on code size and execution parameters
//! - **HTTP**: Request/response size limits

/// Isolate pool limits
///
/// These constants control the pre-allocated pool of V8 isolates.
/// All isolates are created at startup with fixed heap sizes.
pub mod isolate {
    /// Maximum number of isolates in the pool
    ///
    /// Rationale: Each isolate consumes ~130MB (128MB heap + overhead).
    /// 10,000 isolates = ~1.3GB total at startup. This provides sufficient
    /// concurrency for high-density hosting while remaining within
    /// reasonable server memory budgets.
    pub const POOL_SIZE_MAX: u32 = 10_000;

    /// Heap size per isolate in bytes
    ///
    /// Rationale: 128MB provides enough space for typical JavaScript apps
    /// while preventing runaway memory consumption. V8 will trigger GC
    /// before reaching this limit.
    pub const HEAP_SIZE_BYTES_PER_ISOLATE: u32 = 128 * 1024 * 1024; // 128 MB

    /// Maximum heap size for any isolate (enforced at creation)
    ///
    /// Rationale: Hard limit prevents memory exhaustion attacks.
    pub const HEAP_SIZE_BYTES_MAX: u32 = 256 * 1024 * 1024; // 256 MB
}

/// Work queue limits
///
/// These constants control the bounded work queue for task distribution.
pub mod queue {
    /// Maximum depth of the work queue
    ///
    /// Rationale: 10,000 pending requests provides backpressure against
    /// traffic spikes while preventing unbounded memory growth.
    pub const DEPTH_MAX: u32 = 10_000;

    /// Maximum number of workers per application
    ///
    /// Rationale: Limits resource allocation per tenant in multi-tenant
    /// environments. Prevents one app from monopolizing worker threads.
    pub const WORKERS_PER_APP_MAX: u32 = 64;

    /// Per-worker queue capacity
    ///
    /// Rationale: 256 slots per worker provides local backpressure
    /// without excessive memory usage per thread.
    pub const PER_WORKER_CAPACITY: u32 = 256;
}

/// Buffer pool limits
///
/// These constants control pooled and reused request/response buffers.
pub mod buffer {
    /// Number of request buffers to pre-allocate
    ///
    /// Rationale: Pre-allocated buffers eliminate per-request allocations.
    /// 1,000 buffers at 10MB each = ~10GB total (reused across requests).
    pub const REQUEST_POOL_SIZE: u32 = 1_000;

    /// Maximum size of request buffer in bytes
    ///
    /// Rationale: 10MB handles typical API requests. Larger requests
    /// should use streaming or file upload mechanisms.
    pub const REQUEST_SIZE_BYTES_MAX: u32 = 10 * 1024 * 1024; // 10 MB

    /// Maximum size of response buffer in bytes
    ///
    /// Rationale: 10MB handles typical API responses. Larger responses
    /// should use streaming mechanisms.
    pub const RESPONSE_SIZE_BYTES_MAX: u32 = 10 * 1024 * 1024; // 10 MB
}

/// Script and execution limits
///
/// These constants control JavaScript code size and execution parameters.
pub mod execution {
    /// Maximum script size in bytes
    ///
    /// Rationale: 10MB prevents DoS via huge script uploads while
    /// accommodating large bundled applications.
    pub const SCRIPT_SIZE_BYTES_MAX: u32 = 10 * 1024 * 1024; // 10 MB

    /// Maximum request execution timeout in milliseconds
    ///
    /// Rationale: 30 seconds prevents runaway scripts while allowing
    /// slow operations like external API calls to complete.
    pub const TIMEOUT_MS: u32 = 30_000; // 30 seconds

    /// Maximum event loop iterations per request
    ///
    /// Rationale: Prevents infinite async loops. 10,000 iterations
    /// handles complex async workflows while catching runaway code.
    pub const EVENT_LOOP_ITERATIONS_MAX: u32 = 10_000;

    /// Maximum call stack depth
    ///
    /// Rationale: 100 prevents stack overflow from deep recursion
    /// while supporting reasonable call chains.
    pub const CALL_STACK_DEPTH_MAX: u32 = 100;
}

/// HTTP request/response limits
///
/// These constants control HTTP message sizes and counts.
pub mod http {
    /// Maximum number of headers per request
    ///
    /// Rationale: 100 headers accommodates typical HTTP requests while
    /// preventing header exhaustion attacks.
    pub const HEADER_COUNT_MAX: u32 = 100;

    /// Maximum size of individual header in bytes
    ///
    /// Rationale: 8KB per header accommodates large cookies/auth tokens
    /// while preventing memory exhaustion.
    pub const HEADER_SIZE_BYTES_MAX: u32 = 8 * 1024; // 8 KB

    /// Maximum body size in bytes
    ///
    /// Rationale: 10MB handles typical API payloads. Larger uploads
    /// should use multipart/streaming mechanisms.
    pub const BODY_SIZE_BYTES_MAX: u32 = 10 * 1024 * 1024; // 10 MB
}

/// Memory statistics and validation
pub mod stats {
    use super::{buffer, isolate, queue};

    /// Calculate total static memory allocation at startup
    ///
    /// This provides a predictable baseline for capacity planning.
    pub const fn total_static_allocation_bytes() -> u64 {
        let isolate_memory = (isolate::POOL_SIZE_MAX as u64)
            * (isolate::HEAP_SIZE_BYTES_PER_ISOLATE as u64);

        let queue_memory = (queue::DEPTH_MAX as u64) * std::mem::size_of::<usize>() as u64;

        let buffer_memory = (buffer::REQUEST_POOL_SIZE as u64)
            * (buffer::REQUEST_SIZE_BYTES_MAX as u64);

        isolate_memory + queue_memory + buffer_memory
    }

    /// Calculate total static memory allocation in megabytes
    pub const fn total_static_allocation_mb() -> u64 {
        total_static_allocation_bytes() / (1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_are_reasonable() {
        // Verify isolate pool limits are positive
        assert!(isolate::POOL_SIZE_MAX > 0);
        assert!(isolate::HEAP_SIZE_BYTES_PER_ISOLATE > 0);

        // Verify queue depth is positive
        assert!(queue::DEPTH_MAX > 0);

        // Verify buffer limits are positive
        assert!(buffer::REQUEST_POOL_SIZE > 0);
        assert!(buffer::REQUEST_SIZE_BYTES_MAX > 0);

        // Verify execution limits are positive
        assert!(execution::TIMEOUT_MS > 0);
        assert!(execution::EVENT_LOOP_ITERATIONS_MAX > 0);
        assert!(execution::CALL_STACK_DEPTH_MAX > 0);

        // Verify HTTP limits are positive
        assert!(http::HEADER_COUNT_MAX > 0);
        assert!(http::HEADER_SIZE_BYTES_MAX > 0);
    }

    #[test]
    fn test_heap_size_within_max() {
        assert!(
            isolate::HEAP_SIZE_BYTES_PER_ISOLATE <= isolate::HEAP_SIZE_BYTES_MAX,
            "Per-isolate heap must not exceed maximum"
        );
    }

    #[test]
    fn test_static_allocation_calculated() {
        // Verify the calculation function works correctly
        // Note: These are MAXIMUM limits, not actual runtime allocations.
        // The runtime pool size is configurable and typically much smaller.
        let total_bytes = stats::total_static_allocation_bytes();
        let total_mb = stats::total_static_allocation_mb();

        assert!(total_bytes > 0, "Total allocation must be positive");
        assert_eq!(total_mb, total_bytes / (1024 * 1024));

        // Verify individual components
        let expected_isolate_bytes = (isolate::POOL_SIZE_MAX as u64)
            * (isolate::HEAP_SIZE_BYTES_PER_ISOLATE as u64);
        let expected_buffer_bytes = (buffer::REQUEST_POOL_SIZE as u64)
            * (buffer::REQUEST_SIZE_BYTES_MAX as u64);

        // Total should equal isolate memory + queue memory + buffer memory
        assert!(
            total_bytes >= expected_isolate_bytes + expected_buffer_bytes,
            "Total allocation should include isolates and buffers"
        );
    }
}
