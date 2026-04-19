//! HTTP server module
//!
//! Provides the HTTP layer for NANO runtime including:
//! - Configurable server binding
//! - Middleware stack (tracing, timeout, compression)
//! - Health endpoint for liveness checks
//! - Virtual host routing by hostname
//!
//! Future phases add WinterCG object handling.

pub mod config;
pub mod router;
pub mod server;

pub use config::ServerConfig;
pub use router::{AppState, HandlerType, RouteTarget, VirtualHostRouter};
pub use server::start_server;
