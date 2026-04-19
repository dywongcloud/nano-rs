//! Structured JSON logging module for NANO Edge Runtime
//!
//! Provides rich structured JSON logging with contextual fields per request
//! including timestamp, level, event type, hostname, request_id, worker_id,
//! and isolate_id.
//!
//! # Usage
//!
//! ```rust
//! use nano::logging::{init_logging, NanoSpanExt};
//! use tracing::info_span;
//!
//! // Initialize logging at startup
//! init_logging();
//!
//! // Create a span with request context
//! let span = info_span!(
//!     "request",
//!     hostname = "api.example.com",
//!     request_id = "req_abc123"
//! );
//!
//! // Enter the span for automatic context propagation
//! let _enter = span.enter();
//!
//! // Log events will include the context automatically
//! tracing::info!(event = "request_start", "Request processing started");
//! ```
//!
//! # Output Format
//!
//! ```json
//! {
//!   "ts": "2026-04-19T17:57:00Z",
//!   "level": "INFO",
//!   "event": "request_start",
//!   "hostname": "api.example.com",
//!   "request_id": "req_abc123",
//!   "worker_id": 2,
//!   "isolate_id": "iso_7f8d9a",
//!   "message": "Request processing started",
//!   "fields": {}
//! }
//! ```

mod fields;
mod json_layer;

pub use fields::{extract_bool, extract_i64, extract_string, JsonVisitor};
pub use json_layer::{NanoJsonLayer, NanoSpanExt};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialize the structured JSON logging system
///
/// Sets up a tracing subscriber with:
/// - JSON formatting via NanoJsonLayer
/// - Environment-based level filtering via RUST_LOG
/// - Span context propagation for request tracking
///
/// This should be called once at application startup, before any other
/// logging or tracing is performed.
///
/// # Environment Variables
///
/// - `RUST_LOG`: Controls log level filtering (e.g., `info`, `debug`, `warn,nano=debug`)
///
/// # Panics
///
/// Panics if called multiple times (tracing subscriber already initialized).
///
/// # Example
///
/// ```rust
/// use nano::logging::init_logging;
///
/// fn main() {
///     init_logging();
///     tracing::info!("Logging initialized");
/// }
/// ```
pub fn init_logging() {
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("Failed to create env filter");

    let json_layer = NanoJsonLayer::new();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(json_layer)
        .init();
}

/// Initialize logging with a custom filter
///
/// Similar to `init_logging()` but allows specifying the default log level
/// when RUST_LOG is not set.
///
/// # Arguments
///
/// * `default_level` - The default log level to use if RUST_LOG is not set
///
/// # Example
///
/// ```rust
/// use nano::logging::init_logging_with_level;
///
/// fn main() {
///     init_logging_with_level("debug,nano=trace");
/// }
/// ```
pub fn init_logging_with_level(default_level: &str) {
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .expect("Failed to create env filter");

    let json_layer = NanoJsonLayer::new();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(json_layer)
        .init();
}

/// Create a request span with the standard context fields
///
/// Helper function to create a properly configured tracing span for
/// request handling. Sets up the hostname, request_id, and optional
/// worker_id/isolate_id for context propagation.
///
/// # Arguments
///
/// * `hostname` - The virtual host for this request
/// * `request_id` - Unique identifier for this request
///
/// # Returns
///
/// A `tracing::Span` configured with request context
///
/// # Example
///
/// ```rust
/// use nano::logging::create_request_span;
/// use tracing::Instrument;
///
/// async fn handle_request() {
///     let span = create_request_span("api.example.com", "req_abc123");
///     // Use instrument for async functions
///     async {
///         tracing::info!("Processing request");
///     }.instrument(span).await;
/// }
/// ```
pub fn create_request_span(
    hostname: impl Into<String>,
    request_id: impl Into<String>,
) -> tracing::Span {
    tracing::info_span!(
        "request",
        hostname = %hostname.into(),
        request_id = %request_id.into()
    )
}

/// Create a request span with all context fields
///
/// Extended version of `create_request_span` that also includes worker
/// and isolate identifiers for more detailed tracing.
///
/// # Arguments
///
/// * `hostname` - The virtual host for this request
/// * `request_id` - Unique identifier for this request
/// * `worker_id` - Worker thread/pool identifier
/// * `isolate_id` - V8 isolate identifier
///
/// # Returns
///
/// A `tracing::Span` configured with full request context
pub fn create_request_span_full(
    hostname: impl Into<String>,
    request_id: impl Into<String>,
    worker_id: u64,
    isolate_id: impl Into<String>,
) -> tracing::Span {
    tracing::info_span!(
        "request",
        hostname = %hostname.into(),
        request_id = %request_id.into(),
        worker_id = worker_id,
        isolate_id = %isolate_id.into()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We can't easily test init_logging() as it sets a global subscriber
    // and would conflict with other tests. These tests focus on the helpers.

    #[test]
    fn test_create_request_span() {
        let span = create_request_span("test.example.com", "req_test_123");
        assert_eq!(span.metadata().unwrap().name(), "request");
    }

    #[test]
    fn test_create_request_span_full() {
        let span = create_request_span_full("test.example.com", "req_test_123", 42, "iso_xyz789");
        assert_eq!(span.metadata().unwrap().name(), "request");
    }
}
