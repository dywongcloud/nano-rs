//! Isolate diagnostics handler for Admin API
//!
//! Provides ps-style listing of active V8 isolates across all worker pools.
//! Similar to `ps` command but for NANO runtime isolates.

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::admin::diagnostics::{DiagnosticsCollector, IsolateInfo, SystemDiagnostics};
use crate::app::registry::AppRegistry;

/// Isolate information for API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IsolateResponse {
    /// Hostname this isolate serves
    pub hostname: String,
    /// Worker thread/pool ID
    pub worker_id: usize,
    /// When the isolate was created (ISO 8601)
    pub created_at: String,
    /// Uptime in human-readable format
    pub uptime: String,
    /// Number of requests processed
    pub request_count: u64,
    /// Current memory usage in bytes (if available)
    pub memory_bytes: Option<usize>,
    /// Whether the isolate is currently processing a request
    pub busy: bool,
    /// Environment variable keys (not values, for privacy)
    pub env_keys: Vec<String>,
}

/// App summary information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSummary {
    /// Hostname
    pub hostname: String,
    /// Number of active workers
    pub worker_count: usize,
    /// Total requests served
    pub total_requests: u64,
    /// Average memory per isolate in MB
    pub avg_memory_mb: f64,
    /// Uptime of the oldest isolate
    pub uptime: String,
    /// Memory limit in MB
    pub memory_limit_mb: u32,
    /// Timeout in seconds
    pub timeout_secs: u32,
}

/// Isolates list response
#[derive(Debug, Serialize, Deserialize)]
pub struct IsolatesListResponse {
    /// Total number of isolates across all apps
    pub total_isolates: usize,
    /// Total requests since startup
    pub total_requests: u64,
    /// Number of apps
    pub app_count: usize,
    /// List of all isolates
    pub isolates: Vec<IsolateResponse>,
    /// Per-app summaries
    pub apps: Vec<AppSummary>,
    /// Timestamp of the snapshot
    pub timestamp: String,
}

/// Error response for isolate endpoint failures
#[derive(Debug, Serialize)]
pub struct IsolatesError {
    error: String,
    message: String,
    code: u16,
}

impl IsolatesError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: "InternalError".to_string(),
            message: message.into(),
            code: 500,
        }
    }

    pub fn into_response(self) -> (StatusCode, Json<Self>) {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(self))
    }
}

/// Format a Duration as human-readable string
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Format an Instant as ISO 8601 string
fn format_instant(instant: Instant) -> String {
    // Calculate the actual time by subtracting elapsed from now
    let now = Instant::now();
    let elapsed = instant.elapsed();
    let actual_time = std::time::SystemTime::now() - elapsed;
    
    // Convert to RFC3339 format
    actual_time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let nanos = d.subsec_nanos();
            format!(
                "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                1970 + secs / 31_536_000,
                (secs % 31_536_000) / 2_592_000 + 1,
                (secs % 2_592_000) / 86_400 + 1,
                (secs % 86_400) / 3_600,
                (secs % 3_600) / 60,
                secs % 60,
                nanos / 1_000_000
            )
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Format current timestamp as ISO 8601
fn current_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let nanos = now.subsec_nanos();
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1970 + secs / 31_536_000,
        (secs % 31_536_000) / 2_592_000 + 1,
        (secs % 2_592_000) / 86_400 + 1,
        (secs % 86_400) / 3_600,
        (secs % 3_600) / 60,
        secs % 60,
        nanos / 1_000_000
    )
}

/// Convert IsolateInfo to IsolateResponse
fn convert_isolate(info: &IsolateInfo) -> IsolateResponse {
    IsolateResponse {
        hostname: info.hostname.clone(),
        worker_id: info.worker_id,
        created_at: format_instant(info.created_at),
        uptime: info.uptime(),
        request_count: info.request_count,
        memory_bytes: info.memory_bytes,
        busy: info.busy,
        env_keys: info.env_keys.clone(),
    }
}

/// List all isolates handler
///
/// Returns a ps-style listing of all active V8 isolates across all
/// worker pools. Includes per-isolate details and app-level summaries.
///
/// # Arguments
///
/// * `State(registry)` - Shared app registry for looking up configurations
///
/// # Returns
///
/// JSON response with isolate and app information, or an error response.
///
/// # Example
///
/// ```text
/// GET /admin/isolates
///
/// Response:
/// {
///   "total_isolates": 8,
///   "total_requests": 1523,
///   "app_count": 2,
///   "isolates": [
///     {
///       "hostname": "api.example.com",
///       "worker_id": 0,
///       "created_at": "2026-04-19T21:41:09.123Z",
///       "uptime": "5m 30s",
///       "request_count": 42,
///       "memory_bytes": 16777216,
///       "busy": false,
///       "env_keys": ["API_KEY", "DB_URL"]
///     }
///   ],
///   "apps": [
///     {
///       "hostname": "api.example.com",
///       "worker_count": 4,
///       "total_requests": 892,
///       "avg_memory_mb": 32.5,
///       "uptime": "5m 30s",
///       "memory_limit_mb": 128,
///       "timeout_secs": 30
///     }
///   ],
///   "timestamp": "2026-04-19T21:46:39.456Z"
/// }
/// ```
pub async fn list_isolates(
    State(registry): State<Arc<RwLock<AppRegistry>>>,
) -> Result<Json<IsolatesListResponse>, (StatusCode, Json<IsolatesError>)> {
    tracing::debug!("Listing isolates");

    // Create diagnostics collector
    let collector = DiagnosticsCollector::new(registry);

    // Collect current system state
    let diagnostics = collector.collect().await;

    // Convert to response format
    let isolates: Vec<IsolateResponse> = diagnostics
        .isolates
        .iter()
        .map(convert_isolate)
        .collect();

    let apps: Vec<AppSummary> = diagnostics
        .app_stats
        .iter()
        .map(|app| AppSummary {
            hostname: app.hostname.clone(),
            worker_count: app.worker_count,
            total_requests: app.total_requests,
            avg_memory_mb: app.avg_memory_mb,
            uptime: app.uptime.clone(),
            memory_limit_mb: app.config.memory_limit_mb,
            timeout_secs: app.config.timeout_secs,
        })
        .collect();

    let response = IsolatesListResponse {
        total_isolates: diagnostics.total_isolates,
        total_requests: diagnostics.total_requests,
        app_count: diagnostics.app_stats.len(),
        isolates,
        apps,
        timestamp: current_timestamp(),
    };

    tracing::debug!(
        total_isolates = response.total_isolates,
        total_requests = response.total_requests,
        app_count = response.app_count,
        "Isolates listed successfully"
    );

    Ok(Json(response))
}

/// Get isolates for a specific hostname
///
/// Returns isolate information filtered by hostname.
///
/// # Arguments
///
/// * `hostname` - The hostname to filter by
/// * `registry` - Shared app registry
///
/// # Returns
///
/// JSON response with filtered isolate information.
pub async fn get_isolates_by_hostname(
    hostname: &str,
    registry: Arc<RwLock<AppRegistry>>,
) -> Result<Json<IsolatesListResponse>, (StatusCode, Json<IsolatesError>)> {
    tracing::debug!(hostname = hostname, "Getting isolates for hostname");

    let collector = DiagnosticsCollector::new(registry);
    let diagnostics = collector.collect().await;

    // Filter isolates by hostname
    let isolates: Vec<IsolateResponse> = diagnostics
        .isolates
        .iter()
        .filter(|i| i.hostname == hostname)
        .map(convert_isolate)
        .collect();

    // Filter apps by hostname
    let apps: Vec<AppSummary> = diagnostics
        .app_stats
        .iter()
        .filter(|a| a.hostname == hostname)
        .map(|app| AppSummary {
            hostname: app.hostname.clone(),
            worker_count: app.worker_count,
            total_requests: app.total_requests,
            avg_memory_mb: app.avg_memory_mb,
            uptime: app.uptime.clone(),
            memory_limit_mb: app.config.memory_limit_mb,
            timeout_secs: app.config.timeout_secs,
        })
        .collect();

    let response = IsolatesListResponse {
        total_isolates: isolates.len(),
        total_requests: apps.iter().map(|a| a.total_requests).sum(),
        app_count: apps.len(),
        isolates,
        apps,
        timestamp: current_timestamp(),
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, AppLimits};
    use std::collections::HashMap;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn test_current_timestamp_format() {
        let ts = current_timestamp();
        // Should be in ISO 8601 format with Z suffix
        assert!(ts.contains("T"));
        assert!(ts.ends_with("Z"));
        assert!(ts.len() > 20);
    }

    #[test]
    fn test_isolates_error() {
        let error = IsolatesError::new("Test error");
        assert_eq!(error.error, "InternalError");
        assert_eq!(error.message, "Test error");
        assert_eq!(error.code, 500);
    }

    #[test]
    fn test_isolate_response_serialization() {
        let isolate = IsolateResponse {
            hostname: "api.example.com".to_string(),
            worker_id: 0,
            created_at: "2026-04-19T21:41:09.123Z".to_string(),
            uptime: "5m 30s".to_string(),
            request_count: 42,
            memory_bytes: Some(16777216),
            busy: false,
            env_keys: vec!["API_KEY".to_string()],
        };

        let json = serde_json::to_string(&isolate).unwrap();
        assert!(json.contains("api.example.com"));
        assert!(json.contains("worker_id"));
        assert!(json.contains("request_count"));
    }

    #[test]
    fn test_app_summary_serialization() {
        let app = AppSummary {
            hostname: "api.example.com".to_string(),
            worker_count: 4,
            total_requests: 892,
            avg_memory_mb: 32.5,
            uptime: "5m 30s".to_string(),
            memory_limit_mb: 128,
            timeout_secs: 30,
        };

        let json = serde_json::to_string(&app).unwrap();
        assert!(json.contains("api.example.com"));
        assert!(json.contains("32.5"));
        assert!(json.contains("128"));
    }
}
