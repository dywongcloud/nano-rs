//! JavaScript runtime APIs (Phase 3)
//!
//! This module implements the WinterCG-compatible JavaScript APIs:
//! - fetch() handler interface
//! - console.log/warn/error
//! - TextEncoder/TextDecoder
//! - setTimeout/setInterval
//! - AbortController/AbortSignal
//! - structuredClone()
//! - crypto.getRandomValues()
//! - performance.now()
//! - Blob and FormData
//! - DOMException
//!
//! These APIs bridge between JavaScript execution in V8 and the Rust
//! runtime, providing the standard WinterCG environment for edge functions.

pub mod handler;
pub mod apis;
pub mod fetch;
pub mod stream;

// Re-export handler types for convenience
pub use handler::{HandlerContext, execute_handler, execute_handler_with_context};

// Re-export APIs for handler
pub use apis::RuntimeAPIs;
pub use fetch::{bind_fetch, FetchState};
