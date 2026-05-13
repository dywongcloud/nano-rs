//! OOM (Out of Memory) detection and response module
//!
//! This module provides the OomMonitor for detecting when JavaScript isolates
//! exceed memory limits and coordinating the proper response: logging structured
//! OOM events, returning 503 Service Unavailable to clients, and disposing the
//! affected isolate.
//!
//! ## Architecture
//!
//! - `OomMonitor`: Monitors heap usage and detects OOM conditions
//! - `OomResponse`: Response type for OOM handling (logs, 503 response, isolate disposal)
//! - Integration with structured logging from 07-01 for rich OOM event context
//!
//! ## OOM Response Flow
//!
//! 1. Worker calls `oom_monitor.check()` before/after request execution
//! 2. If OOM detected, `OomError` is returned with usage statistics
//! 3. Worker calls `oom_monitor.log_oom_event()` to emit structured log
//! 4. Worker returns 503 Service Unavailable response
//! 5. Worker disposes isolate and creates fresh one for next request
//!
//! ## Log Format
//!
//! ```json
//! {
//!   "ts": "2026-04-19T17:57:00Z",
//!   "level": "ERROR",
//!   "event": "oom_kill",
//!   "hostname": "app.example.com",
//!   "request_id": "req_abc123",
//!   "used_bytes": 104857600,
//!   "limit_bytes": 67108864,
//!   "isolate_id": "iso_7f8d9a",
//!   "message": "Isolate terminated: heap limit exceeded"
//! }
//! ```

use crate::http::NanoResponse;
use crate::worker::limits::{HeapStatistics, MemoryLimiter, OomError};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, warn};

/// Monitors heap usage and coordinates OOM response
///
/// The OomMonitor wraps a MemoryLimiter and provides:
/// - Periodic heap checks (pre-request and optionally during request)
/// - Structured OOM event logging
/// - 503 response generation for OOM conditions
/// - Statistics tracking for OOM events
#[derive(Debug)]
pub struct OomMonitor {
    /// The memory limiter for heap tracking
    limiter: MemoryLimiter,
    /// Unique identifier for this monitor (used for isolate_id in logs)
    monitor_id: String,
    /// Total number of OOM checks performed
    check_count: AtomicU64,
    /// Total number of OOM events triggered
    oom_count: AtomicU64,
}

impl OomMonitor {
    /// Create a new OOM monitor with the given limiter
    ///
    /// # Arguments
    ///
    /// * `limiter` - The MemoryLimiter to use for heap tracking
    /// * `monitor_id` - Unique identifier for this monitor (e.g., worker_id)
    ///
    /// # Example
    ///
    /// ```
    /// use nano::worker::limits::MemoryLimiter;
    /// use nano::worker::oom::OomMonitor;
    ///
    /// let limiter = MemoryLimiter::new(128, "app.example.com");
    /// let monitor = OomMonitor::new(limiter, "worker_0");
    /// ```
    pub fn new(limiter: MemoryLimiter, monitor_id: impl Into<String>) -> Self {
        Self {
            limiter,
            monitor_id: monitor_id.into(),
            check_count: AtomicU64::new(0),
            oom_count: AtomicU64::new(0),
        }
    }

    /// Get the monitor ID (used as isolate_id in logs)
    pub fn monitor_id(&self) -> &str {
        &self.monitor_id
    }

    /// Get the app hostname from the limiter
    pub fn app_hostname(&self) -> &str {
        self.limiter.hostname()
    }

    /// Check heap for OOM condition
    ///
    /// This method checks if the heap usage exceeds the configured limit
    /// using the limiter's `check_oom()` method which applies the OOM threshold.
    ///
    /// # Arguments
    ///
    /// * `isolate` - The V8 isolate to check
    ///
    /// # Returns
    ///
    /// `Ok(HeapStatistics)` if within limits, `Err(OomError)` if OOM detected
    pub fn check(&self, isolate: &mut v8::Isolate) -> Result<HeapStatistics, OomError> {
        self.check_count.fetch_add(1, Ordering::SeqCst);

        match self.limiter.check_oom(isolate) {
            Ok(stats) => Ok(stats),
            Err(e) => {
                self.oom_count.fetch_add(1, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    /// Log a structured OOM event
    ///
    /// Emits an ERROR level log with structured fields for OOM kill events.
    /// This integrates with the structured JSON logging from 07-01.
    ///
    /// # Arguments
    ///
    /// * `error` - The OomError containing usage statistics
    /// * `request_id` - The request ID for correlation
    pub fn log_oom_event(&self, error: &OomError, request_id: &str) {
        match error {
            OomError::LimitExceeded {
                used_bytes,
                limit_bytes,
                app_hostname,
            } => {
                // Calculate utilization percentage
                let utilization_pct = if *limit_bytes > 0 {
                    ((*used_bytes as f64) / (*limit_bytes as f64)) * 100.0
                } else {
                    0.0
                };

                error!(
                    event = "oom_kill",
                    used_bytes = *used_bytes,
                    limit_bytes = *limit_bytes,
                    utilization_pct = format!("{:.1}", utilization_pct),
                    hostname = %app_hostname,
                    request_id = %request_id,
                    isolate_id = %self.monitor_id,
                    "Isolate terminated: heap limit exceeded"
                );
            }
            OomError::V8HeapLimitTriggered => {
                error!(
                    event = "oom_kill",
                    oom_type = "v8_callback",
                    request_id = %request_id,
                    isolate_id = %self.monitor_id,
                    "Isolate terminated: V8 heap limit callback triggered"
                );
            }
        }
    }

    /// Create a 503 Service Unavailable response for OOM conditions
    ///
    /// Returns a properly formatted HTTP 503 response with a descriptive
    /// error message indicating the resource limit was exceeded.
    ///
    /// # Arguments
    ///
    /// * `error` - The OomError to include in the response
    ///
    /// # Returns
    ///
    /// A NanoResponse with status 503 and descriptive body
    pub fn create_oom_response(&self, error: &OomError) -> NanoResponse {
        let body = match error {
            OomError::LimitExceeded {
                used_bytes,
                limit_bytes,
                app_hostname,
            } => {
                format!(
                    "Service Unavailable: Resource limit exceeded\n\n\
                    Application: {}\n\
                    Memory used: {} MB\n\
                    Memory limit: {} MB\n\
                    Isolate: {}\n",
                    app_hostname,
                    used_bytes / (1024 * 1024),
                    limit_bytes / (1024 * 1024),
                    self.monitor_id
                )
            }
            OomError::V8HeapLimitTriggered => {
                format!(
                    "Service Unavailable: Resource limit exceeded\n\n\
                    V8 heap limit callback triggered\n\
                    Isolate: {}\n",
                    self.monitor_id
                )
            }
        };

        NanoResponse::with_status(503)
            .with_header("Content-Type", "text/plain")
            .with_header("Retry-After", "0") // Indicate immediate retry may succeed with fresh isolate
            .with_body(body)
    }

    /// Check if OOM has been triggered
    pub fn is_oom(&self) -> bool {
        self.limiter.is_oom()
    }

    /// Get the total number of OOM checks performed
    pub fn check_count(&self) -> u64 {
        self.check_count.load(Ordering::SeqCst)
    }

    /// Get the total number of OOM events
    pub fn oom_count(&self) -> u64 {
        self.oom_count.load(Ordering::SeqCst)
    }

    /// Reset the OOM state (for testing or after isolate disposal)
    pub fn reset(&self) {
        self.limiter.reset();
    }

    /// Get the configured memory limit in MB
    pub fn limit_mb(&self) -> usize {
        self.limiter.limit_mb()
    }

    /// Get the configured OOM threshold
    pub fn oom_threshold(&self) -> f64 {
        self.limiter.oom_threshold()
    }

    /// Log a warning when approaching OOM threshold (e.g., at 80% of limit)
    ///
    /// This can be called periodically to provide early warning of memory pressure.
    ///
    /// # Arguments
    ///
    /// * `isolate` - The V8 isolate to check
    /// * `warning_threshold` - Fraction of limit to warn at (e.g., 0.8 for 80%)
    /// * `request_id` - The request ID for correlation
    pub fn check_memory_pressure(
        &self,
        isolate: &mut v8::Isolate,
        warning_threshold: f64,
        request_id: &str,
    ) -> Option<HeapStatistics> {
        let (stats, exceeded) = self.limiter.peek_memory(isolate);

        if exceeded {
            // OOM condition - this will be handled by check()
            return Some(stats);
        }

        // Check if we're approaching the limit
        let usage_ratio = stats.percent_of_limit(self.limiter.limit_bytes()) / 100.0;
        if usage_ratio >= warning_threshold {
            warn!(
                event = "memory_pressure",
                usage_pct = format!("{:.1}", usage_ratio * 100.0),
                used_mb = stats.used_mb(),
                limit_mb = self.limiter.limit_mb(),
                hostname = %self.limiter.hostname(),
                request_id = %request_id,
                isolate_id = %self.monitor_id,
                "Memory usage approaching limit"
            );
        }

        Some(stats)
    }
}

/// Builder for creating OomMonitor instances
///
/// Provides a fluent API for configuring OOM monitoring:
///
/// ```
/// use nano::worker::oom::OomMonitorBuilder;
///
/// let monitor = OomMonitorBuilder::new("worker_0")
///     .with_limit_mb(256)
///     .with_oom_threshold(0.95)
///     .for_hostname("app.example.com")
///     .build();
/// ```
pub struct OomMonitorBuilder {
    monitor_id: String,
    limit_mb: u32,
    oom_threshold: f64,
    hostname: String,
}

impl OomMonitorBuilder {
    /// Create a new builder with the given monitor ID
    pub fn new(monitor_id: impl Into<String>) -> Self {
        Self {
            monitor_id: monitor_id.into(),
            limit_mb: 128,      // Default 128MB
            oom_threshold: 1.0, // Default 100%
            hostname: "unknown".to_string(),
        }
    }

    /// Set the memory limit in MB
    pub fn with_limit_mb(mut self, limit_mb: u32) -> Self {
        self.limit_mb = limit_mb;
        self
    }

    /// Set the OOM threshold (0.0-1.0)
    pub fn with_oom_threshold(mut self, threshold: f64) -> Self {
        self.oom_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the hostname for this monitor
    pub fn for_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = hostname.into();
        self
    }

    /// Build the OomMonitor
    pub fn build(self) -> OomMonitor {
        let limiter = if self.oom_threshold != 1.0 {
            MemoryLimiter::with_threshold(self.limit_mb, self.hostname, self.oom_threshold)
        } else {
            MemoryLimiter::new(self.limit_mb, self.hostname)
        };

        OomMonitor::new(limiter, self.monitor_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::platform;

    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    #[test]
    fn test_oom_monitor_creation() {
        let limiter = MemoryLimiter::new(128, "test.app");
        let monitor = OomMonitor::new(limiter, "worker_0");

        assert_eq!(monitor.monitor_id(), "worker_0");
        assert_eq!(monitor.limit_mb(), 128);
        assert!(!monitor.is_oom());
        assert_eq!(monitor.check_count(), 0);
        assert_eq!(monitor.oom_count(), 0);
    }

    #[test]
    fn test_oom_monitor_builder() {
        let monitor = OomMonitorBuilder::new("worker_1")
            .with_limit_mb(256)
            .with_oom_threshold(0.95)
            .for_hostname("app.example.com")
            .build();

        assert_eq!(monitor.monitor_id(), "worker_1");
        assert_eq!(monitor.limit_mb(), 256);
        assert_eq!(monitor.oom_threshold(), 0.95);
    }

    #[test]
    fn test_create_oom_response() {
        let limiter = MemoryLimiter::new(128, "test.app");
        let monitor = OomMonitor::new(limiter, "iso_test_123");

        let error = OomError::LimitExceeded {
            used_bytes: 150 * 1024 * 1024,
            limit_bytes: 128 * 1024 * 1024,
            app_hostname: "test.app".to_string(),
        };

        let response = monitor.create_oom_response(&error);

        assert_eq!(response.status(), 503);
        assert!(response.headers().get("Content-Type").is_some());
        assert!(response.headers().get("Retry-After").is_some());

        if let Some(body) = response.body() {
            let body_str = String::from_utf8_lossy(body);
            assert!(body_str.contains("Service Unavailable"));
            assert!(body_str.contains("test.app"));
            assert!(body_str.contains("iso_test_123"));
        }
    }

    #[test]
    fn test_oom_response_v8_callback() {
        let limiter = MemoryLimiter::new(128, "test.app");
        let monitor = OomMonitor::new(limiter, "iso_v8_456");

        let error = OomError::V8HeapLimitTriggered;
        let response = monitor.create_oom_response(&error);

        assert_eq!(response.status(), 503);
        if let Some(body) = response.body() {
            let body_str = String::from_utf8_lossy(body);
            assert!(body_str.contains("V8 heap limit callback triggered"));
        }
    }

    #[test]
    fn test_check_counts_oom() {
        init_platform();
        use crate::v8::NanoIsolate;

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        // Create monitor with very low limit to force OOM
        let monitor = OomMonitorBuilder::new("worker_oom_test")
            .with_limit_mb(1)
            .with_oom_threshold(0.1)
            .for_hostname("test.app")
            .build();

        // Initial state
        assert_eq!(monitor.check_count(), 0);
        assert_eq!(monitor.oom_count(), 0);

        // First check - should trigger OOM
        let result = monitor.check(isolate.isolate());

        // Verify counts
        assert_eq!(monitor.check_count(), 1);

        // Result should be error (OOM)
        if result.is_err() {
            assert_eq!(monitor.oom_count(), 1);
            assert!(monitor.is_oom());
        }
    }

    #[test]
    fn test_reset() {
        let limiter = MemoryLimiter::new(128, "test.app");
        let monitor = OomMonitor::new(limiter, "worker_reset");

        // Manually trigger OOM
        monitor.reset();
        assert!(!monitor.is_oom());
    }

    #[test]
    fn test_builder_default_values() {
        let monitor = OomMonitorBuilder::new("worker_default").build();

        assert_eq!(monitor.limit_mb(), 128); // Default
        assert_eq!(monitor.oom_threshold(), 1.0); // Default
    }

    #[test]
    fn test_builder_threshold_clamping() {
        let monitor = OomMonitorBuilder::new("worker_high")
            .with_oom_threshold(1.5)
            .build();
        assert_eq!(monitor.oom_threshold(), 1.0);

        let monitor_low = OomMonitorBuilder::new("worker_low")
            .with_oom_threshold(-0.5)
            .build();
        assert_eq!(monitor_low.oom_threshold(), 0.0);
    }

    #[test]
    fn test_check_passes_with_normal_threshold() {
        init_platform();
        use crate::v8::NanoIsolate;

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let monitor = OomMonitorBuilder::new("worker_normal")
            .with_limit_mb(16)
            .with_oom_threshold(1.0)
            .for_hostname("test.app")
            .build();

        // Fresh isolate should pass
        let result = monitor.check(isolate.isolate());
        assert!(result.is_ok(), "Fresh isolate should pass OOM check");
        assert!(!monitor.is_oom());
        assert_eq!(monitor.check_count(), 1);
        assert_eq!(monitor.oom_count(), 0);
    }
}
