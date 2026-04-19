//! Worker pool for multi-threaded JavaScript execution
//!
//! This module implements the WorkerPool architecture for NANO:
//!
//! ## Architecture
//!
//! - **WorkerPool**: Manages N worker threads, each owning one V8 isolate
//! - **WorkerHandle**: Handle to a worker thread with MPSC channel for task dispatch
//! - **HandlerTask**: Task sent to workers containing the JavaScript entrypoint and request
//!
//! ## Thread Safety
//!
//! Each worker thread creates and owns its `NanoIsolate` (thread-local ownership).
//! Isolates are `!Send + !Sync` via `PhantomData<*mut ()>`, preventing cross-thread
//! movement. This is critical for V8 stability (see POOL-05).
//!
//! ## Task Flow
//!
//! 1. HTTP layer creates a `HandlerTask` with entrypoint, request, and response channel
//! 2. WorkerPool dispatches the task via MPSC to a worker thread
//! 3. Worker thread executes the JavaScript handler using its isolate
//! 4. Response is sent back via oneshot channel
//!
//! ## Graceful Shutdown
//!
//! Dropping the `WorkerPool` signals workers to exit via MPSC channel closure.
//! All worker threads are joined to ensure clean isolate cleanup.

use crate::http::{NanoRequest, NanoResponse};
use std::sync::Arc;
use tokio::sync::oneshot;

pub mod pool;

// Re-export pool types
pub use pool::{WorkerPool, WorkerHandle};

/// Task sent to worker threads for JavaScript handler execution
///
/// This struct is `Send` so it can safely cross thread boundaries via MPSC channels.
/// The response is sent back via the oneshot channel.
#[derive(Debug)]
pub struct HandlerTask {
    /// Path to the JavaScript entrypoint file
    pub entrypoint: String,
    /// The incoming HTTP request (WinterCG-compatible)
    pub request: NanoRequest,
    /// Channel to send the response back to the caller
    pub response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
}

// Safety: NanoRequest is Clone + contains String/Bytes which are Send
// This explicit impl documents and verifies the Send contract
unsafe impl Send for HandlerTask {}

impl HandlerTask {
    /// Create a new handler task
    ///
    /// # Arguments
    ///
    /// * `entrypoint` - Path to the JavaScript file
    /// * `request` - The HTTP request to process
    /// * `response_tx` - Oneshot channel sender for the response
    pub fn new(
        entrypoint: String,
        request: NanoRequest,
        response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
    ) -> Self {
        Self {
            entrypoint,
            request,
            response_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoUrl};

    #[test]
    fn test_handler_task_creation() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            NanoHeaders::new(),
            None,
        );

        let (tx, _rx) = oneshot::channel();
        let task = HandlerTask::new(
            "/app/index.js".to_string(),
            request,
            tx,
        );

        assert_eq!(task.entrypoint, "/app/index.js");
        assert_eq!(task.request.method(), "GET");
    }

    #[test]
    fn test_handler_task_is_send() {
        // Compile-time check: HandlerTask must be Send
        fn assert_send<T: Send>() {}
        assert_send::<HandlerTask>();
    }
}
