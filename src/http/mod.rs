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
pub mod headers;
pub mod router;
pub mod server;
pub mod types;
pub mod url;
pub mod v8_bridge;

pub use config::ServerConfig;
pub use headers::NanoHeaders;
pub use router::{AppState, HandlerType, RouteTarget, VirtualHostRouter};
pub use server::start_server;
pub use types::{NanoRequest, NanoResponse};
pub use url::{NanoUrl, NanoUrlSearchParams};
pub use v8_bridge::{serialize_request_to_json, serialize_response_to_json};
