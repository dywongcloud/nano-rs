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
pub mod types;
pub mod event_loop;
pub mod apis;

// Re-export handler types for convenience
pub use handler::{HandlerContext, execute_handler};

// Re-export timer and abort types
pub use types::{TimerId, TimerHandle, AbortSignalState, register_abort_state, get_abort_state, remove_abort_state};
pub use event_loop::TimerQueue;

// Re-export API bindings
pub use apis::{RuntimeAPIs, init_thread_timer_queue};
