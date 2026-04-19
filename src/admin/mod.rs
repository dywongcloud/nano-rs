//! Admin module for diagnostics and monitoring
//!
//! Provides visibility into the NANO runtime state including:
//! - Active isolates and worker pools
//! - App statistics and resource usage
//! - System-wide diagnostics
//! - Prometheus metrics endpoint

pub mod diagnostics;
pub mod metrics;

pub use diagnostics::{DiagnosticsCollector, IsolateInfo, AppStats, SystemDiagnostics};
pub use metrics::metrics_handler;
