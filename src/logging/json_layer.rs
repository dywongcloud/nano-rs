//! Custom JSON logging layer for NANO Edge Runtime
//!
//! Implements a tracing subscriber layer that outputs structured JSON logs
//! with contextual fields per request including timestamp, level, event type,
//! hostname, request_id, worker_id, and isolate_id.
//!
//! # Example Output
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
//!   "message": "Request started",
//!   "fields": {}
//! }
//! ```

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Mutex;

use chrono::Utc;
use serde_json::json;
use serde_json::Value;
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use super::fields::JsonVisitor;

/// Extension data stored with each span to carry request context
///
/// This structure holds the contextual fields that are inherited from
/// parent spans and passed through the span hierarchy. It enables
/// request tracing across async boundaries and worker thread dispatch.
#[derive(Debug, Clone)]
pub struct NanoSpanExt {
    /// The virtual hostname for this request
    pub hostname: Option<String>,
    /// The unique request identifier
    pub request_id: Option<String>,
    /// The worker pool identifier
    pub worker_id: Option<u64>,
    /// The V8 isolate identifier
    pub isolate_id: Option<String>,
}

impl NanoSpanExt {
    /// Create a new empty span extension
    pub fn new() -> Self {
        Self {
            hostname: None,
            request_id: None,
            worker_id: None,
            isolate_id: None,
        }
    }

    /// Merge with parent extension, taking values only if not already set
    pub fn merge_from_parent(&mut self, parent: &NanoSpanExt) {
        if self.hostname.is_none() {
            self.hostname = parent.hostname.clone();
        }
        if self.request_id.is_none() {
            self.request_id = parent.request_id.clone();
        }
        if self.worker_id.is_none() {
            self.worker_id = parent.worker_id;
        }
        if self.isolate_id.is_none() {
            self.isolate_id = parent.isolate_id.clone();
        }
    }
}

impl Default for NanoSpanExt {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom JSON logging layer for structured output
///
/// This layer implements the `tracing_subscriber::Layer` trait to provide
/// JSON-formatted logging with contextual fields from spans. It outputs
/// to stdout in a structured format suitable for log aggregation systems.
pub struct NanoJsonLayer {
    /// Buffer for stdout output (synchronized via Mutex)
    stdout: Mutex<std::io::Stdout>,
}

impl NanoJsonLayer {
    /// Create a new JSON logging layer
    ///
    /// Initializes the layer with stdout as the output target.
    pub fn new() -> Self {
        Self {
            stdout: Mutex::new(std::io::stdout()),
        }
    }

    /// Format a log event as JSON
    ///
    /// Extracts context from the span hierarchy and builds a JSON object
    /// with all required fields: ts, level, event, hostname, request_id,
    /// worker_id, isolate_id, and any additional event fields.
    fn format_event<S>(&self, event: &Event<'_>, ctx: Context<'_, S>) -> serde_json::Value
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let mut fields = BTreeMap::new();

        // Visit the event to extract all fields
        event.record(&mut JsonVisitor(&mut fields));

        // Extract span context
        let mut hostname = None;
        let mut request_id = None;
        let mut worker_id = None;
        let mut isolate_id = None;

        // Walk the span hierarchy to collect context
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(ext) = span.extensions().get::<NanoSpanExt>() {
                    hostname = hostname.or_else(|| ext.hostname.clone());
                    request_id = request_id.or_else(|| ext.request_id.clone());
                    worker_id = worker_id.or(ext.worker_id);
                    isolate_id = isolate_id.or_else(|| ext.isolate_id.clone());
                }
            }
        }

        // Extract message from fields if present, otherwise use event name
        let message = fields
            .remove("message")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| event.metadata().name().to_string());

        // Build the JSON output - only include worker_id/isolate_id at top level if set
        let mut log_entry = json!({
            "ts": Utc::now().to_rfc3339(),
            "level": event.metadata().level().to_string(),
            "event": event.metadata().name(),
            "hostname": hostname,
            "request_id": request_id,
            "message": message,
            "fields": fields,
        });

        // Only add worker_id and isolate_id to top level if they have values
        // These are set by the worker span, not the HTTP span
        if let Some(wid) = worker_id {
            log_entry["worker_id"] = json!(wid);
        }
        if let Some(iso) = isolate_id {
            log_entry["isolate_id"] = json!(iso);
        }

        log_entry
    }

    /// Write a JSON log line to stdout
    fn write_json(&self, value: &serde_json::Value) {
        if let Ok(mut stdout) = self.stdout.lock() {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string(value).unwrap_or_default()
            );
        }
    }
}

impl Default for NanoJsonLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for NanoJsonLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let json_value = self.format_event(event, ctx);
        self.write_json(&json_value);
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found");
        let mut extensions = span.extensions_mut();

        // Create span extension from attributes
        let mut span_ext = NanoSpanExt::new();

        // Visit span attributes to extract context fields
        let mut fields = BTreeMap::new();
        attrs.record(&mut JsonVisitor(&mut fields));

        // Extract known context fields from span attributes
        if let Some(Value::String(host)) = fields.get("hostname") {
            span_ext.hostname = Some(host.clone());
        }
        if let Some(Value::String(req_id)) = fields.get("request_id") {
            span_ext.request_id = Some(req_id.clone());
        }
        if let Some(Value::Number(worker)) = fields.get("worker_id") {
            if let Some(id) = worker.as_u64() {
                span_ext.worker_id = Some(id);
            }
        }
        if let Some(Value::String(iso)) = fields.get("isolate_id") {
            span_ext.isolate_id = Some(iso.clone());
        }

        // Try to inherit from parent span
        if let Some(parent_span) = span.parent() {
            if let Some(parent_ext) = parent_span.extensions().get::<NanoSpanExt>() {
                span_ext.merge_from_parent(parent_ext);
            }
        }

        extensions.insert(span_ext);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nano_span_ext_new() {
        let ext = NanoSpanExt::new();
        assert!(ext.hostname.is_none());
        assert!(ext.request_id.is_none());
        assert!(ext.worker_id.is_none());
        assert!(ext.isolate_id.is_none());
    }

    #[test]
    fn test_nano_span_ext_merge_from_parent() {
        let parent = NanoSpanExt {
            hostname: Some("example.com".to_string()),
            request_id: Some("req_123".to_string()),
            worker_id: Some(1),
            isolate_id: Some("iso_abc".to_string()),
        };

        let mut child = NanoSpanExt::new();
        child.merge_from_parent(&parent);

        assert_eq!(child.hostname, Some("example.com".to_string()));
        assert_eq!(child.request_id, Some("req_123".to_string()));
        assert_eq!(child.worker_id, Some(1));
        assert_eq!(child.isolate_id, Some("iso_abc".to_string()));
    }

    #[test]
    fn test_nano_span_ext_merge_preserves_child_values() {
        let parent = NanoSpanExt {
            hostname: Some("parent.com".to_string()),
            request_id: Some("req_parent".to_string()),
            worker_id: Some(1),
            isolate_id: Some("iso_parent".to_string()),
        };

        let mut child = NanoSpanExt {
            hostname: Some("child.com".to_string()),
            request_id: None,
            worker_id: Some(2),
            isolate_id: None,
        };

        child.merge_from_parent(&parent);

        // Child values should be preserved
        assert_eq!(child.hostname, Some("child.com".to_string()));
        assert_eq!(child.worker_id, Some(2));

        // Parent values should fill in gaps
        assert_eq!(child.request_id, Some("req_parent".to_string()));
        assert_eq!(child.isolate_id, Some("iso_parent".to_string()));
    }

    #[test]
    fn test_nano_json_layer_new() {
        let _layer = NanoJsonLayer::new();
        // Just verify it creates without panicking
    }
}
