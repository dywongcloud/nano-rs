//! Admin module for diagnostics and monitoring
//!
//! Provides visibility into the NANO runtime state including:
//! - Active isolates and worker pools
//! - App statistics and resource usage
//! - System-wide diagnostics
//! - Prometheus metrics endpoint
//! - HTTP Admin API with API key authentication

pub mod auth;
pub mod diagnostics;
pub mod handlers;
pub mod metrics;
pub mod server;

pub use auth::{api_key_middleware, api_key_middleware_forbidden, AdminAuth, AuthError};
pub use diagnostics::{DiagnosticsCollector, IsolateInfo, AppStats, SystemDiagnostics};
pub use handlers::*;
pub use metrics::metrics_handler;
pub use server::{AdminConfig, AdminServer, create_admin_router};
