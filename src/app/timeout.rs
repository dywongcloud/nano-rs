//! Per-application request timeout enforcement
//!
//! This module provides timeout watchdog functionality to terminate
//! long-running JavaScript requests and prevent runaway execution.
//!
//! ## Architecture
//!
//! - `TimeoutWatchdog`: Guards a closure with a timeout deadline
//! - `TimeoutConfig`: Configuration for timeout behavior
//! - `TimeoutError`: Error type for timeout violations
//!
//! ## Implementation Strategy
//!
//! The watchdog uses a hybrid approach:
//! 1. Async timeout at the Rust level (tokio::time::timeout)
//! 2. V8 execution termination via v8::Isolate::terminate_execution()
//!
//! This ensures both async boundaries and synchronous JS loops are covered.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{debug, warn};

/// Error type for request timeout conditions
#[derive(Error, Debug, Clone, PartialEq)]
pub enum TimeoutError {
    /// Request exceeded the time limit
    #[error("Request timeout: exceeded {timeout_secs}s limit (ran for {elapsed_ms}ms)")]
    RequestTimeout {
        /// Configured timeout in seconds
        timeout_secs: u32,
        /// How long the request actually ran in milliseconds
        elapsed_ms: u64,
        /// Application hostname that timed out
        app_hostname: String,
    },

    /// Execution was cancelled externally
    #[error("Execution cancelled")]
    Cancelled,

    /// Watchdog internal error
    #[error("Watchdog error: {0}")]
    Internal(String),
}

impl TimeoutError {
    /// Get the configured timeout
    pub fn timeout_secs(&self) -> Option<u32> {
        match self {
            TimeoutError::RequestTimeout { timeout_secs, .. } => Some(*timeout_secs),
            _ => None,
        }
    }

    /// Get the elapsed time if available
    pub fn elapsed_ms(&self) -> Option<u64> {
        match self {
            TimeoutError::RequestTimeout { elapsed_ms, .. } => Some(*elapsed_ms),
            _ => None,
        }
    }

    /// Get the app hostname if available
    pub fn app_hostname(&self) -> Option<&str> {
        match self {
            TimeoutError::RequestTimeout { app_hostname, .. } => Some(app_hostname),
            _ => None,
        }
    }

    /// Returns true if this is a request timeout (not cancellation)
    pub fn is_timeout(&self) -> bool {
        matches!(self, TimeoutError::RequestTimeout { .. })
    }
}

/// Configuration for timeout behavior
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeoutConfig {
    /// Timeout duration in seconds
    pub timeout_secs: u32,
    /// Whether timeout is enabled
    pub enabled: bool,
    /// Grace period after timeout before forced termination (ms)
    pub grace_period_ms: u64,
}

impl TimeoutConfig {
    /// Create a new timeout config
    pub fn new(timeout_secs: u32) -> Self {
        Self {
            timeout_secs,
            enabled: true,
            grace_period_ms: 100, // 100ms grace period
        }
    }

    /// Create a disabled timeout config (no timeout)
    pub fn disabled() -> Self {
        Self {
            timeout_secs: 0,
            enabled: false,
            grace_period_ms: 0,
        }
    }

    /// Get the timeout as a Duration
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.timeout_secs as u64)
    }

    /// Check if this config represents a valid timeout
    pub fn is_valid(&self) -> bool {
        self.enabled && self.timeout_secs > 0
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self::new(30) // 30 second default
    }
}

/// Request watchdog that enforces time limits on execution
///
/// The watchdog tracks execution time and can signal cancellation
/// when the deadline is exceeded. It works with both async Rust code
/// and synchronous V8 execution via the `terminate_execution()` hook.
#[derive(Debug)]
pub struct TimeoutWatchdog {
    /// When the deadline expires
    deadline: Instant,
    /// How long until deadline
    timeout_duration: Duration,
    /// App hostname for error context
    app_hostname: String,
    /// Whether timeout has fired
    expired: Arc<AtomicBool>,
}

impl TimeoutWatchdog {
    /// Create a new watchdog with the given timeout
    ///
    /// # Arguments
    ///
    /// * `timeout_secs` - Timeout in seconds (must be > 0)
    /// * `app_hostname` - Hostname for error context
    ///
    /// # Example
    ///
    /// ```
    /// use nano::app::timeout::TimeoutWatchdog;
    ///
    /// let watchdog = TimeoutWatchdog::new(5, "app.example.com");
    /// // Use tolerance for timing-sensitive assertion
    /// let remaining = watchdog.remaining_ms();
    /// assert!(remaining >= 4990 && remaining <= 5000);
    /// ```
    pub fn new(timeout_secs: u32, app_hostname: impl Into<String>) -> Self {
        let timeout_duration = Duration::from_secs(timeout_secs as u64);
        Self {
            deadline: Instant::now() + timeout_duration,
            timeout_duration,
            app_hostname: app_hostname.into(),
            expired: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create watchdog from a TimeoutConfig
    pub fn from_config(config: &TimeoutConfig, app_hostname: impl Into<String>) -> Option<Self> {
        if !config.is_valid() {
            return None;
        }
        Some(Self::new(config.timeout_secs, app_hostname))
    }

    /// Check if the deadline has passed
    pub fn check_expired(&self) -> bool {
        let expired = Instant::now() > self.deadline;
        if expired {
            self.expired.store(true, Ordering::SeqCst);
        }
        expired
    }

    /// Get remaining time in milliseconds
    pub fn remaining_ms(&self) -> u64 {
        let now = Instant::now();
        if now >= self.deadline {
            0
        } else {
            let remaining = self.deadline - now;
            remaining.as_millis() as u64
        }
    }

    /// Get elapsed time since creation in milliseconds
    pub fn elapsed_ms(&self) -> u64 {
        let elapsed = self.timeout_duration.saturating_sub(Duration::from_millis(self.remaining_ms()));
        elapsed.as_millis() as u64
    }

    /// Cancel the operation manually
    pub fn cancel(&self) {
        self.expired.store(true, Ordering::SeqCst);
    }

    /// Check if the operation was cancelled
    pub fn is_cancelled(&self) -> bool {
        self.expired.load(Ordering::SeqCst)
    }

    /// Create a timeout error with current context
    pub fn timeout_error(&self) -> TimeoutError {
        TimeoutError::RequestTimeout {
            timeout_secs: self.timeout_duration.as_secs() as u32,
            elapsed_ms: self.elapsed_ms(),
            app_hostname: self.app_hostname.clone(),
        }
    }

    /// Run a future with timeout enforcement
    ///
    /// This is the primary async interface for the watchdog. It wraps
    /// a future with a timeout that will resolve with an error if the
    /// deadline is exceeded.
    ///
    /// # Type Parameters
    ///
    /// * `F` - The future type to execute
    /// * `T` - The output type of the future
    ///
    /// # Arguments
    ///
    /// * `f` - The future to execute
    ///
    /// # Returns
    ///
    /// `Ok(T)` if completed within timeout, `Err(TimeoutError)` if exceeded
    pub async fn run_future<F, T>(&self, f: F) -> Result<T, TimeoutError>
    where
        F: Future<Output = T>,
    {
        let remaining = self.remaining_ms();
        if remaining == 0 {
            return Err(self.timeout_error());
        }

        tokio::select! {
            result = f => {
                Ok(result)
            }
            _ = tokio::time::sleep(Duration::from_millis(remaining)) => {
                self.expired.store(true, Ordering::SeqCst);
                Err(self.timeout_error())
            }
        }
    }

    /// Run a blocking function with timeout enforcement
    ///
    /// For synchronous code (like V8 execution), this runs the function
    /// on a blocking thread with timeout. Note: this cannot actually
    /// terminate the thread, so for V8 use `terminate_execution()` instead.
    ///
    /// # Arguments
    ///
    /// * `f` - The blocking function to execute
    ///
    /// # Returns
    ///
    /// `Ok(T)` if completed within timeout, `Err(TimeoutError)` if exceeded
    pub async fn run_blocking<F, T>(&self, f: F) -> Result<T, TimeoutError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let remaining = self.remaining_ms();
        if remaining == 0 {
            return Err(self.timeout_error());
        }

        let (tx, rx) = oneshot::channel();
        let expired = self.expired.clone();

        // Spawn blocking task
        tokio::task::spawn_blocking(move || {
            let result = f();
            let _ = tx.send(result);
        });

        tokio::select! {
            result = rx => {
                match result {
                    Ok(val) => Ok(val),
                    Err(_) => Err(TimeoutError::Internal("Result channel closed".to_string())),
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(remaining)) => {
                expired.store(true, Ordering::SeqCst);
                Err(self.timeout_error())
            }
        }
    }
}

/// Guard that terminates V8 execution on timeout
///
/// This structure holds a reference to an isolate and terminates
/// its execution when dropped or when the timeout fires.
pub struct V8TerminationGuard {
    /// Thread-safe handle to the isolate
    isolate_handle: Option<v8::IsolateHandle>,
    /// Watchdog that tracks timeout
    watchdog: TimeoutWatchdog,
}

impl V8TerminationGuard {
    /// Create a new termination guard
    ///
    /// # Safety
    ///
    /// The isolate must remain alive for the duration of the guard.
    /// This is typically ensured by running within a single task/thread.
    pub unsafe fn new(
        isolate_handle: v8::IsolateHandle,
        timeout_secs: u32,
        app_hostname: impl Into<String>,
    ) -> Self {
        Self {
            isolate_handle: Some(isolate_handle),
            watchdog: TimeoutWatchdog::new(timeout_secs, app_hostname),
        }
    }

    /// Check if timeout has expired and terminate if so
    pub fn check_and_terminate(&self) -> bool {
        if self.watchdog.check_expired() {
            if let Some(ref handle) = self.isolate_handle {
                handle.terminate_execution();
                warn!("V8 execution terminated due to timeout");
            }
            true
        } else {
            false
        }
    }

    /// Get remaining time
    pub fn remaining_ms(&self) -> u64 {
        self.watchdog.remaining_ms()
    }

    /// Cancel the guard
    pub fn cancel(&self) {
        self.watchdog.cancel();
    }
}

impl Drop for V8TerminationGuard {
    fn drop(&mut self) {
        // Terminate execution when guard is dropped (if timeout expired)
        if self.watchdog.check_expired() {
            if let Some(handle) = self.isolate_handle.take() {
                handle.terminate_execution();
                debug!("V8 execution terminated on guard drop");
            }
        }
    }
}

/// Wraps a future with timeout using a watchdog
///
/// Convenience function for one-off timeout wrapping.
///
/// # Arguments
///
/// * `timeout_secs` - Timeout duration in seconds
/// * `app_hostname` - Hostname for error context
/// * `f` - Future to execute
///
/// # Returns
///
/// `Ok(T)` if completed within timeout, `Err(TimeoutError)` if exceeded
pub async fn with_timeout<F, T>(
    timeout_secs: u32,
    app_hostname: impl Into<String>,
    f: F,
) -> Result<T, TimeoutError>
where
    F: Future<Output = T>,
{
    let watchdog = TimeoutWatchdog::new(timeout_secs, app_hostname);
    watchdog.run_future(f).await
}

/// Wraps a blocking operation with timeout
///
/// Convenience function for one-off blocking timeout wrapping.
///
/// # Arguments
///
/// * `timeout_secs` - Timeout duration in seconds
/// * `app_hostname` - Hostname for error context
/// * `f` - Blocking function to execute
///
/// # Returns
///
/// `Ok(T)` if completed within timeout, `Err(TimeoutError)` if exceeded
pub async fn with_blocking_timeout<F, T>(
    timeout_secs: u32,
    app_hostname: impl Into<String>,
    f: F,
) -> Result<T, TimeoutError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let watchdog = TimeoutWatchdog::new(timeout_secs, app_hostname);
    watchdog.run_blocking(f).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_config() {
        let config = TimeoutConfig::new(5);
        assert_eq!(config.timeout_secs, 5);
        assert!(config.enabled);
        assert!(config.is_valid());

        let config = TimeoutConfig::disabled();
        assert!(!config.enabled);
        assert!(!config.is_valid());
    }

    #[test]
    fn test_watchdog_creation() {
        let watchdog = TimeoutWatchdog::new(5, "test.app");
        // Use tolerance for timing-sensitive assertion
        let remaining = watchdog.remaining_ms();
        assert!(remaining >= 4990 && remaining <= 5000, 
            "Expected remaining_ms around 5000, got {}", remaining);
        assert!(!watchdog.check_expired());
    }

    #[test]
    fn test_watchdog_remaining_decreases() {
        let watchdog = TimeoutWatchdog::new(1, "test.app");
        
        // Sleep briefly
        std::thread::sleep(Duration::from_millis(50));
        
        let remaining = watchdog.remaining_ms();
        assert!(remaining < 1000, "Remaining should decrease");
        assert!(remaining > 900, "Should still have most of the time");
    }

    #[test]
    fn test_watchdog_expires() {
        let watchdog = TimeoutWatchdog::new(0, "test.app");
        // With 0 seconds, should already be expired
        assert!(watchdog.check_expired());
    }

    #[test]
    fn test_timeout_error_properties() {
        let err = TimeoutError::RequestTimeout {
            timeout_secs: 5,
            elapsed_ms: 5100,
            app_hostname: "test.app".to_string(),
        };

        assert_eq!(err.timeout_secs(), Some(5));
        assert_eq!(err.elapsed_ms(), Some(5100));
        assert_eq!(err.app_hostname(), Some("test.app"));
        assert!(err.is_timeout());
    }

    #[test]
    fn test_cancelled_error() {
        let err = TimeoutError::Cancelled;
        assert!(!err.is_timeout());
        assert_eq!(err.timeout_secs(), None);
    }

    #[tokio::test]
    async fn test_run_future_success() {
        let watchdog = TimeoutWatchdog::new(5, "test.app");
        
        let result = watchdog.run_future(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            42
        }).await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_run_future_timeout() {
        let watchdog = TimeoutWatchdog::new(0, "test.app");
        
        let result = watchdog.run_future(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            42
        }).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimeoutError::RequestTimeout { .. }));
    }

    #[tokio::test]
    async fn test_run_blocking_success() {
        let watchdog = TimeoutWatchdog::new(5, "test.app");
        
        let result = watchdog.run_blocking(|| {
            std::thread::sleep(Duration::from_millis(10));
            42
        }).await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_timeout_success() {
        let result = with_timeout(5, "test.app", async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            "success"
        }).await;

        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_with_timeout_expires() {
        let result = with_timeout(0, "test.app", async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "should not reach"
        }).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimeoutError::RequestTimeout { .. }));
    }

    #[test]
    fn test_watchdog_cancel() {
        let watchdog = TimeoutWatchdog::new(5, "test.app");
        assert!(!watchdog.is_cancelled());
        
        watchdog.cancel();
        assert!(watchdog.is_cancelled());
    }
}
