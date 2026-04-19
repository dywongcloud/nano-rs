//! Admin module for diagnostics and monitoring
//!
//! Provides visibility into the NANO runtime state including:
//! - Active isolates and worker pools
//! - App statistics and resource usage
//! - System-wide diagnostics

pub mod diagnostics;

pub use diagnostics::{DiagnosticsCollector, IsolateInfo, AppStats, SystemDiagnostics};
