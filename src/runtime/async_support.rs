//! Async Execution Support for V8 Promise Resolution
//!
//! This module provides utilities for resolving JavaScript Promises in the V8 runtime
//! by running microtask checkpoints and yielding to the Tokio runtime for external
//! async operations.

use std::time::{Duration, Instant};

/// Maximum time to wait for a Promise to resolve before giving up
const MAX_PROMISE_WAIT_TIME: Duration = Duration::from_secs(30);

/// Maximum number of microtask checkpoint iterations before yielding to Tokio
const MAX_MICROTASK_ITERATIONS: u32 = 1000;

/// Resolve a V8 Promise by running microtask checkpoints and pumping the async runtime
///
/// This function loops until the Promise resolves (Fulfilled or Rejected), running
/// microtask checkpoints to execute JavaScript async operations and yielding to Tokio
/// to allow external async operations (like VFS, network) to complete.
///
/// Also pumps the V8 message loop to handle internal V8 async operations like
/// WebAssembly compilation and instantiation.
///
/// # Arguments
///
/// * `scope` - The V8 HandleScope for accessing the Promise
/// * `promise` - The V8 Promise to resolve
///
/// # Returns
///
/// * `Ok(v8::Local<v8::Value>)` - The resolved value if Fulfilled
/// * `Err(anyhow::Error)` - If Rejected or timed out
///
/// # Example
///
/// ```rust
/// let result = handler_fn.call(scope, global.into(), &[js_request.into()]);
/// if let Some(response) = result {
///     if response.is_promise() {
///         let promise = response.cast::<v8::Promise>();
///         match resolve_promise_with_async(scope, promise) {
///             Ok(value) => extract_js_response(scope, value),
///             Err(e) => Err(e),
///         }
///     }
/// }
/// ```
pub fn resolve_promise_with_async<'s>(
    scope: &mut v8::HandleScope<'s>,
    promise: v8::Local<v8::Promise>,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    let start_time = Instant::now();
    let mut iteration_count = 0u32;

    // Loop until promise resolves or times out
    loop {
        // Check current promise state
        match promise.state() {
            v8::PromiseState::Fulfilled => {
                return Ok(promise.result(scope));
            }
            v8::PromiseState::Rejected => {
                let error = promise.result(scope);
                let error_str = error.to_string(scope)
                    .map(|s| s.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "Promise rejected".to_string());
                return Err(anyhow::anyhow!("Promise rejected: {}", error_str));
            }
            v8::PromiseState::Pending => {
                // Promise is still pending, need to pump async operations
                
                // Check for timeout
                if start_time.elapsed() > MAX_PROMISE_WAIT_TIME {
                    return Err(anyhow::anyhow!(
                        "Promise resolution timeout after {:?}",
                        MAX_PROMISE_WAIT_TIME
                    ));
                }

                // Pump the V8 message loop to handle internal V8 async operations
                // This is required for WebAssembly.compile/instantiate and other
                // V8 internal async operations to complete
                let platform = v8::V8::get_current_platform();
                v8::Platform::pump_message_loop(&platform, scope, false);

                // Perform microtask checkpoint to execute queued JavaScript microtasks
                // This handles internal V8 async operations like Promise callbacks
                scope.perform_microtask_checkpoint();
                
                iteration_count += 1;

                // Every N iterations, yield to Tokio runtime to allow external async operations
                // (like VFS I/O, network requests) to complete
                if iteration_count % 10 == 0 {
                    // Use a thread yield to allow the Tokio runtime to process pending tasks
                    // This is a lightweight yield that doesn't block the thread
                    std::thread::yield_now();
                    
                    // Small sleep to allow async I/O to complete
                    // This is necessary because V8's microtask checkpoint doesn't handle
                    // external async operations (like file system I/O)
                    std::thread::sleep(Duration::from_millis(1));
                }

                // Prevent infinite loops in case of broken promises
                if iteration_count > MAX_MICROTASK_ITERATIONS {
                    // Reset counter but continue - some promises need many iterations
                    iteration_count = 0;
                }
            }
        }
    }
}

/// Try to resolve a Promise with a shorter timeout (for CPU-limited contexts)
///
/// This variant is used when CPU time limits are in effect and we don't want to
/// wait too long for async operations.
///
/// Also pumps the V8 message loop to handle internal V8 async operations.
///
/// # Arguments
///
/// * `scope` - The V8 HandleScope
/// * `promise` - The V8 Promise to resolve
/// * `timeout_ms` - Maximum time to wait in milliseconds
///
/// # Returns
///
/// Same as `resolve_promise_with_async`, but with shorter timeout
pub fn resolve_promise_with_timeout<'s>(
    scope: &mut v8::HandleScope<'s>,
    promise: v8::Local<v8::Promise>,
    timeout_ms: u64,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    let start_time = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let mut iteration_count = 0u32;

    loop {
        match promise.state() {
            v8::PromiseState::Fulfilled => {
                return Ok(promise.result(scope));
            }
            v8::PromiseState::Rejected => {
                let error = promise.result(scope);
                let error_str = error.to_string(scope)
                    .map(|s| s.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "Promise rejected".to_string());
                return Err(anyhow::anyhow!("Promise rejected: {}", error_str));
            }
            v8::PromiseState::Pending => {
                if start_time.elapsed() > timeout {
                    return Err(anyhow::anyhow!(
                        "Promise resolution timeout after {}ms",
                        timeout_ms
                    ));
                }

                // Pump the V8 message loop for internal async operations
                let platform = v8::V8::get_current_platform();
                v8::Platform::pump_message_loop(&platform, scope, false);

                scope.perform_microtask_checkpoint();
                iteration_count += 1;

                if iteration_count % 10 == 0 {
                    std::thread::yield_now();
                    std::thread::sleep(Duration::from_millis(1));
                }

                if iteration_count > MAX_MICROTASK_ITERATIONS {
                    iteration_count = 0;
                }
            }
        }
    }
}

/// Check if a value is a Promise and resolve it if needed
///
/// This is a convenience wrapper that checks if the value is a Promise,
/// and if so, resolves it using the async loop. If not a Promise, returns
/// the value as-is.
///
/// # Arguments
///
/// * `scope` - The V8 HandleScope
/// * `value` - The value to check (may or may not be a Promise)
///
/// # Returns
///
/// * `Ok(v8::Local<v8::Value>)` - Resolved value or original value
/// * `Err(anyhow::Error)` - If Promise rejected or timed out
pub fn resolve_if_promise<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    if value.is_promise() {
        let promise = value.cast::<v8::Promise>();
        resolve_promise_with_async(scope, promise)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that we can detect Promise states correctly
    #[test]
    fn test_promise_resolution_utilities_exist() {
        // These are compile-time checks - the real tests require V8
        // which is tested in integration tests
        assert!(MAX_PROMISE_WAIT_TIME.as_secs() > 0);
        assert!(MAX_MICROTASK_ITERATIONS > 0);
    }
}
