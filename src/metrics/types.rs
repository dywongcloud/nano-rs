//! Core metric types for Prometheus-compatible metrics
//!
//! Implements Counter, Gauge, and Histogram with thread-safe
//! atomic operations for use in high-concurrency environments.
//!
//! # Thread Safety
//!
//! All metric types use [`std::sync::atomic`] types and are [`Send`]
//! and [`Sync`], allowing concurrent access from multiple threads.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Labels for a metric instance
///
/// Labels are key-value pairs that identify specific metric dimensions.
/// For example: `{"hostname" => "api.example.com", "status" => "200"}`
pub type MetricLabels = HashMap<String, String>;

/// A monotonically increasing counter
///
/// Counters are used to represent cumulative values that only increase,
/// such as request counts or error totals.
///
/// # Example
///
/// ```rust
/// use nano::metrics::Counter;
///
/// let counter = Counter::new();
/// counter.inc();
/// counter.inc_by(5);
/// assert_eq!(counter.get(), 6);
/// ```
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter initialized to 0
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Increment the counter by 1
    pub fn inc(&self) {
        self.inc_by(1);
    }

    /// Increment the counter by a specific value
    pub fn inc_by(&self, value: u64) {
        self.value.fetch_add(value, Ordering::Relaxed);
    }

    /// Get the current counter value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset the counter to 0 (useful for testing)
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// An instantaneous gauge value
///
/// Gauges represent values that can go up or down, such as memory usage
/// or number of active connections.
///
/// # Example
///
/// ```rust
/// use nano::metrics::Gauge;
///
/// let gauge = Gauge::new();
/// gauge.set(42);
/// gauge.inc();
/// gauge.dec_by(5);
/// assert_eq!(gauge.get(), 38);
/// ```
#[derive(Debug)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge initialized to 0
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Set the gauge to a specific value
    pub fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Increment the gauge by 1
    pub fn inc(&self) {
        self.inc_by(1);
    }

    /// Increment the gauge by a specific value
    pub fn inc_by(&self, value: u64) {
        self.value.fetch_add(value, Ordering::Relaxed);
    }

    /// Decrement the gauge by 1
    pub fn dec(&self) {
        self.dec_by(1);
    }

    /// Decrement the gauge by a specific value
    pub fn dec_by(&self, value: u64) {
        self.value.fetch_sub(value, Ordering::Relaxed);
    }

    /// Get the current gauge value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// A histogram for recording latency distributions
///
/// Histograms bucket observations into predefined ranges and track
/// the sum and count of all observations. Used for request latency.
///
/// # Example
///
/// ```rust
/// use nano::metrics::Histogram;
///
/// let buckets = vec![1.0, 10.0, 100.0, f64::INFINITY];
/// let histogram = Histogram::new(&buckets);
/// histogram.observe(5.0);
/// histogram.observe(50.0);
/// let (buckets, sum, count) = histogram.get();
/// assert_eq!(count, 2);
/// ```
#[derive(Debug)]
pub struct Histogram {
    /// Predefined bucket boundaries
    buckets: Vec<f64>,
    /// Counters for each bucket (indexed by upper bound)
    bucket_counts: Vec<AtomicU64>,
    /// Sum of all observed values
    sum: AtomicU64,
    /// Total count of observations
    count: AtomicU64,
}

impl Histogram {
    /// Create a new histogram with the given bucket boundaries
    ///
    /// # Arguments
    ///
    /// * `buckets` - Slice of bucket upper bounds. Must be sorted ascending
    ///               and end with infinity (or very large value).
    pub fn new(buckets: &[f64]) -> Self {
        let mut sorted_buckets = buckets.to_vec();
        sorted_buckets.sort_by(|a, b| a.partial_cmp(b).unwrap());

        Self {
            bucket_counts: (0..sorted_buckets.len())
                .map(|_| AtomicU64::new(0))
                .collect(),
            buckets: sorted_buckets,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record an observation in the histogram
    ///
    /// The value is placed in the appropriate bucket and added to the sum.
    pub fn observe(&self, value: f64) {
        // Find the bucket and increment it
        for (i, &bound) in self.buckets.iter().enumerate() {
            if value <= bound {
                self.bucket_counts[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }

        // Update sum (store as scaled integer to avoid f64 atomic issues)
        let scaled = (value * 1000.0) as u64; // Store in microseconds for precision
        self.sum.fetch_add(scaled, Ordering::Relaxed);

        // Increment total count
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the histogram values
    ///
    /// Returns (bucket_counts, sum, count) where:
    /// - bucket_counts is a vector of counts per bucket
    /// - sum is the total sum of all observations (in milliseconds)
    /// - count is the total number of observations
    pub fn get(&self) -> (Vec<u64>, f64, u64) {
        let counts: Vec<u64> = self
            .bucket_counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect();
        let sum_scaled = self.sum.load(Ordering::Relaxed);
        let count = self.count.load(Ordering::Relaxed);
        let sum_ms = sum_scaled as f64 / 1000.0; // Convert back to milliseconds

        (counts, sum_ms, count)
    }

    /// Get the bucket boundaries
    pub fn buckets(&self) -> &[f64] {
        &self.buckets
    }
}

/// A counter that supports multiple label combinations
///
/// CounterVec allows tracking counts for different label value combinations,
/// such as requests per hostname and status code.
///
/// # Example
///
/// ```rust
/// use nano::metrics::CounterVec;
///
/// let counter_vec = CounterVec::new(vec!["hostname", "status"]);
/// counter_vec.inc(vec!["api.example.com", "200"]);
/// counter_vec.inc(vec!["api.example.com", "500"]);
/// ```
#[derive(Debug)]
pub struct CounterVec {
    label_names: Vec<String>,
    counters: RwLock<HashMap<Vec<String>, Counter>>,
}

impl CounterVec {
    /// Create a new counter vector with the given label names
    pub fn new(label_names: Vec<&str>) -> Self {
        Self {
            label_names: label_names.iter().map(|s| s.to_string()).collect(),
            counters: RwLock::new(HashMap::new()),
        }
    }

    /// Increment the counter for the given label values
    ///
    /// # Arguments
    ///
    /// * `label_values` - Values for each label in order. Must match
    ///   the number of label names provided at construction.
    pub fn inc(&self, label_values: Vec<&str>) {
        self.inc_by(label_values, 1);
    }

    /// Increment the counter by a specific value for the given label values
    pub fn inc_by(&self, label_values: Vec<&str>, value: u64) {
        let key: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();

        // Fast path: try read lock
        {
            let counters = self.counters.read().unwrap();
            if let Some(counter) = counters.get(&key) {
                counter.inc_by(value);
                return;
            }
        }

        // Slow path: need to create counter
        let mut counters = self.counters.write().unwrap();
        let counter = counters.entry(key.clone()).or_insert_with(Counter::new);
        counter.inc_by(value);
    }

    /// Get all label combinations and their values
    pub fn get_all(&self) -> Vec<(Vec<String>, u64)> {
        let counters = self.counters.read().unwrap();
        counters.iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }

    /// Get the label names
    pub fn label_names(&self) -> &[String] {
        &self.label_names
    }
}

/// A gauge that supports multiple label combinations
///
/// GaugeVec allows tracking gauge values for different label combinations,
/// such as memory usage per hostname.
#[derive(Debug)]
pub struct GaugeVec {
    label_names: Vec<String>,
    gauges: RwLock<HashMap<Vec<String>, Gauge>>,
}

impl GaugeVec {
    /// Create a new gauge vector with the given label names
    pub fn new(label_names: Vec<&str>) -> Self {
        Self {
            label_names: label_names.iter().map(|s| s.to_string()).collect(),
            gauges: RwLock::new(HashMap::new()),
        }
    }

    /// Set the gauge value for the given label values
    pub fn set(&self, label_values: Vec<&str>, value: u64) {
        let key: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();

        // Fast path: try read lock
        {
            let gauges = self.gauges.read().unwrap();
            if let Some(gauge) = gauges.get(&key) {
                gauge.set(value);
                return;
            }
        }

        // Slow path: need to create gauge
        let mut gauges = self.gauges.write().unwrap();
        let gauge = gauges.entry(key.clone()).or_insert_with(Gauge::new);
        gauge.set(value);
    }

    /// Get all label combinations and their values
    pub fn get_all(&self) -> Vec<(Vec<String>, u64)> {
        let gauges = self.gauges.read().unwrap();
        gauges.iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }

    /// Get the label names
    pub fn label_names(&self) -> &[String] {
        &self.label_names
    }
}

/// A histogram that supports multiple label combinations
///
/// HistogramVec allows tracking latency distributions for different
/// label combinations, such as request duration per hostname.
#[derive(Debug)]
pub struct HistogramVec {
    label_names: Vec<String>,
    buckets: Vec<f64>,
    histograms: RwLock<HashMap<Vec<String>, Histogram>>,
}

impl HistogramVec {
    /// Create a new histogram vector with the given label names and buckets
    pub fn new(label_names: Vec<&str>, buckets: Vec<f64>) -> Self {
        Self {
            label_names: label_names.iter().map(|s| s.to_string()).collect(),
            buckets,
            histograms: RwLock::new(HashMap::new()),
        }
    }

    /// Record an observation for the given label values
    ///
    /// # Arguments
    ///
    /// * `label_values` - Values for each label in order
    /// * `value` - The value to observe (e.g., latency in milliseconds)
    pub fn observe(&self, label_values: Vec<&str>, value: f64) {
        let key: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();

        // Fast path: try read lock
        {
            let histograms = self.histograms.read().unwrap();
            if let Some(histogram) = histograms.get(&key) {
                histogram.observe(value);
                return;
            }
        }

        // Slow path: need to create histogram
        let mut histograms = self.histograms.write().unwrap();
        let histogram = histograms
            .entry(key.clone())
            .or_insert_with(|| Histogram::new(&self.buckets));
        histogram.observe(value);
    }

    /// Get all label combinations and their histogram data
    pub fn get_all(&self) -> Vec<(Vec<String>, Vec<u64>, f64, u64)> {
        let histograms = self.histograms.read().unwrap();
        histograms
            .iter()
            .map(|(k, h)| {
                let (counts, sum, count) = h.get();
                (k.clone(), counts, sum, count)
            })
            .collect()
    }

    /// Get the label names
    pub fn label_names(&self) -> &[String] {
        &self.label_names
    }

    /// Get the bucket boundaries
    pub fn buckets(&self) -> &[f64] {
        &self.buckets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_basic() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.inc_by(5);
        assert_eq!(counter.get(), 6);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_gauge_basic() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0);

        gauge.set(42);
        assert_eq!(gauge.get(), 42);

        gauge.inc();
        assert_eq!(gauge.get(), 43);

        gauge.dec_by(3);
        assert_eq!(gauge.get(), 40);
    }

    #[test]
    fn test_histogram_basic() {
        let buckets = vec![1.0, 10.0, 100.0, f64::INFINITY];
        let histogram = Histogram::new(&buckets);

        // Record some values
        histogram.observe(0.5); // Goes into bucket 1.0
        histogram.observe(5.0); // Goes into bucket 10.0
        histogram.observe(50.0); // Goes into bucket 100.0
        histogram.observe(500.0); // Goes into bucket inf

        let (counts, sum, count) = histogram.get();

        assert_eq!(counts[0], 1); // 0.5 <= 1.0
        assert_eq!(counts[1], 1); // 5.0 <= 10.0
        assert_eq!(counts[2], 1); // 50.0 <= 100.0
        assert_eq!(counts[3], 1); // 500.0 <= inf

        assert_eq!(count, 4);
        assert!(sum > 555.0 && sum < 556.0); // 0.5 + 5.0 + 50.0 + 500.0
    }

    #[test]
    fn test_counter_vec() {
        let counter_vec = CounterVec::new(vec!["hostname", "status"]);

        counter_vec.inc(vec!["api.example.com", "200"]);
        counter_vec.inc(vec!["api.example.com", "200"]);
        counter_vec.inc(vec!["api.example.com", "500"]);
        counter_vec.inc(vec!["blog.example.com", "200"]);

        let all = counter_vec.get_all();
        assert_eq!(all.len(), 3);

        // Find specific entries
        let mut found = false;
        for (labels, value) in &all {
            if labels == &vec!["api.example.com", "200"] {
                assert_eq!(*value, 2);
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn test_gauge_vec() {
        let gauge_vec = GaugeVec::new(vec!["hostname"]);

        gauge_vec.set(vec!["api.example.com"], 1000);
        gauge_vec.set(vec!["blog.example.com"], 2000);

        let all = gauge_vec.get_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_histogram_vec() {
        let buckets = vec![1.0, 10.0, f64::INFINITY];
        let hist_vec = HistogramVec::new(vec!["hostname"], buckets);

        hist_vec.observe(vec!["api.example.com"], 5.0);
        hist_vec.observe(vec!["api.example.com"], 50.0);
        hist_vec.observe(vec!["blog.example.com"], 5.0);

        let all = hist_vec.get_all();
        assert_eq!(all.len(), 2);

        // Check api.example.com has 2 observations
        let api_entry = all
            .iter()
            .find(|(k, _, _, _)| k == &vec!["api.example.com"])
            .unwrap();
        assert_eq!(api_entry.3, 2); // count
    }
}
