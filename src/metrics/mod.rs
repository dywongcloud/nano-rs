//! Prometheus metrics collection and exposition
//!
//! Provides thread-safe metrics collection (Counter, Gauge, Histogram)
//! and Prometheus text format rendering for the NANO edge runtime.
//!
//! # Metrics Registry
//!
//! The global [`METRICS`] registry stores all metrics and provides
//! a thread-safe interface for recording and reading values.
//!
//! # Example
//!
//! ```rust
//! use nano::metrics::{Counter, Gauge, Histogram};
//!
//! // Record a counter increment
//! METRICS.requests_total.inc(vec!["api.example.com", "200"]);
//!
//! // Record a histogram observation
//! METRICS.request_duration.observe(42.5, vec!["api.example.com"]);
//! ```

pub mod collector;
pub mod exporter;
pub mod types;

pub use collector::MetricsRegistry;
pub use exporter::PrometheusExporter;
pub use types::{Counter, Gauge, Histogram, MetricLabels};

use std::sync::LazyLock;

/// Global metrics registry singleton
///
/// This provides access to the metrics registry from anywhere in the codebase.
/// Use this to record metrics during request handling.
pub static METRICS: LazyLock<MetricsRegistry> = LazyLock::new(MetricsRegistry::new);

/// Request duration histogram buckets in milliseconds
///
/// These buckets cover the range from 1ms to 1s+ (infinity)
/// as specified in D-04 decision.
pub const REQUEST_DURATION_BUCKETS: &[f64] = &[
    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, f64::INFINITY,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registry_singleton() {
        // Verify the global METRICS can be accessed
        let _ = &*METRICS;
    }

    #[test]
    fn test_request_duration_buckets() {
        // Verify we have the expected number of buckets (10)
        assert_eq!(REQUEST_DURATION_BUCKETS.len(), 10);
        
        // Verify first bucket is 1ms
        assert_eq!(REQUEST_DURATION_BUCKETS[0], 1.0);
        
        // Verify last bucket is infinity
        assert!(REQUEST_DURATION_BUCKETS[9].is_infinite());
        
        // Verify buckets are sorted ascending
        for i in 1..REQUEST_DURATION_BUCKETS.len() - 1 {
            assert!(
                REQUEST_DURATION_BUCKETS[i] > REQUEST_DURATION_BUCKETS[i - 1],
                "Buckets should be sorted ascending"
            );
        }
    }
}
