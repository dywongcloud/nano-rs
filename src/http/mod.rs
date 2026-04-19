//! HTTP server module
//!
//! Provides the HTTP layer for NANO runtime including:
//! - Configurable server binding
//! - Middleware stack (tracing, timeout, compression)
//! - Health endpoint for liveness checks
//!
//! Future phases add virtual host routing and WinterCG object handling.

pub mod config;
pub mod server;

pub use config::ServerConfig;
pub use server::start_server;
