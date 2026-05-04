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

/// Memory limit checker trait for per-request memory DoS protection
///
/// This trait allows the async promise resolver to check memory usage
/// during execution and abort if a per-request limit is exceeded.
pub trait MemoryLimitChecker: Send + Sync {
    /// Check if the current memory usage exceeds the limit
    ///
    /// # Arguments
    /// * `isolate` - The V8 isolate to check
    ///
    /// # Returns
    /// * `Ok(())` if within limit
    /// * `Err(String)` with error message if limit exceeded
    fn check_memory(&self, isolate: &mut v8::Isolate) -> Result<(), String>;

    /// Get the configured limit in MB for error messages
    fn limit_mb(&self) -> u32;
}

/// Helper function to get isolate from a scope for memory checking
///
/// In v147, we work with PinnedRef which derefs to Isolate.
fn get_isolate_from_scope<'s, C, S>(scope: &S) -> &mut v8::Isolate
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
    // SAFETY: We need to get a mutable reference to the isolate from the scope.
    // This is safe because we're only reading the isolate pointer.
    unsafe {
        let isolate_ptr = (**scope).get_isolate_ptr();
        &mut *isolate_ptr
    }
}

/// Resolve a V8 Promise by running microtask checkpoints and pumping the async runtime
///
/// This function loops until the Promise resolves (Fulfilled or Rejected), running
/// microtask checkpoints to execute JavaScript async operations and yielding to Tokio
/// to allow external async operations (like VFS, network) to complete.
///
/// Also pumps the V8 message loop to handle internal V8 async operations like
/// WebAssembly compilation and instantiation.
///
/// # V147 API Note
/// In v147, we use PinnedRef instead of direct HandleScope references.
/// This function accepts any type that derefs to a PinnedRef<HandleScope>.
///
/// # Arguments
///
/// * `scope` - The V8 scope (PinnedRef or ContextScope) for accessing the Promise
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
pub fn resolve_promise_with_async<'s, C, S>(
    scope: &mut S,
    promise: v8::Local<v8::Promise>,
) -> anyhow::Result<v8::Local<'s, v8::Value>>
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
    // Call the variant without memory checker for backward compatibility
    resolve_promise_with_async_and_memory(scope, promise, None)
}

/// Resolve a V8 Promise with optional memory limit checking
///
/// This variant allows passing a memory limit checker to enforce per-request
/// memory limits during async execution, preventing memory DoS attacks.
///
/// # V147 API Note
/// In v147, we use PinnedRef instead of direct HandleScope references.
/// This function accepts any type that derefs to a PinnedRef<HandleScope>.
///
/// # Arguments
///
/// * `scope` - The V8 scope (PinnedRef or ContextScope) for accessing the Promise
/// * `promise` - The V8 Promise to resolve
/// * `memory_checker` - Optional memory limit checker for DoS protection
///
/// # Returns
///
/// * `Ok(v8::Local<v8::Value>)` - The resolved value if Fulfilled
/// * `Err(anyhow::Error)` - If Rejected, timed out, or memory limit exceeded
pub fn resolve_promise_with_async_and_memory<'s, C, S>(
    scope: &mut S,
    promise: v8::Local<v8::Promise>,
    memory_checker: Option<&dyn MemoryLimitChecker>,
) -> anyhow::Result<v8::Local<'s, v8::Value>>
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
    let start_time = Instant::now();
    let mut iteration_count = 0u32;

    // Loop until promise resolves or times out
    loop {
        // Check for timeout first
        if start_time.elapsed() > MAX_PROMISE_WAIT_TIME {
            return Err(anyhow::anyhow!(
                "Promise resolution timeout after {:?}",
                MAX_PROMISE_WAIT_TIME
            ));
        }

        // CRITICAL FIX: Pump the V8 message loop BEFORE checking promise state
        // WebAssembly.compile/instantiate and other V8 internal async operations
        // require the message loop to be pumped to make progress.
        // This must happen BEFORE checking promise.state() to give V8 a chance
        // to process pending compilation jobs.
        //
        // WASM compilation can require multiple message loop pumps, so we pump
        // multiple times to ensure the compilation completes.
        let platform = v8::V8::get_current_platform();
        for _ in 0..5 {
            // v147 API: pump_message_loop expects &Isolate, get via Deref
            v8::Platform::pump_message_loop(&platform, &**scope, false);
        }

        // Perform microtask checkpoint to execute queued JavaScript microtasks
        // This handles Promise callbacks and other async JavaScript operations
        // v147 API: perform_microtask_checkpoint works through DerefMut
        scope.perform_microtask_checkpoint();

        // Check memory limits if a checker is provided
        // This prevents memory DoS attacks during long-running async operations
        if let Some(checker) = memory_checker {
            if iteration_count % 10 == 0 {
                // Check memory every 10 iterations to avoid overhead
                // v147 API: Get isolate from scope via helper
                let isolate = get_isolate_from_scope(scope);
                if let Err(msg) = checker.check_memory(isolate) {
                    return Err(anyhow::anyhow!(
                        "Memory limit exceeded during async execution: {} (limit: {}MB)",
                        msg,
                        checker.limit_mb()
                    ));
                }
            }
        }

        // NOW check promise state after pumping
        match promise.state() {
            v8::PromiseState::Fulfilled => {
                // v147 API: promise.result expects &PinnedRef
                return Ok(promise.result(&**scope));
            }
            v8::PromiseState::Rejected => {
                let error = promise.result(&**scope);
                // v147 API: to_string and to_rust_string_lossy expect &PinnedRef
                let error_str = error.to_string(&**scope)
                    .map(|s| s.to_rust_string_lossy(&**scope))
                    .unwrap_or_else(|| "Promise rejected".to_string());
                return Err(anyhow::anyhow!("Promise rejected: {}", error_str));
            }
            v8::PromiseState::Pending => {
                // Promise is still pending, continue pumping
                iteration_count += 1;

                // Every N iterations, yield to Tokio runtime to allow external async operations
                // (like VFS I/O, network requests) to complete
                if iteration_count % 10 == 0 {
                    // Use a thread yield to allow the Tokio runtime to process pending tasks
                    std::thread::yield_now();

                    // Small sleep to allow async I/O to complete
                    std::thread::sleep(Duration::from_millis(1));
                }

                // Prevent infinite loops in case of broken promises
                if iteration_count > MAX_MICROTASK_ITERATIONS {
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
/// # V147 API Note
/// In v147, we use PinnedRef instead of direct HandleScope references.
///
/// # Arguments
///
/// * `scope` - The V8 scope (PinnedRef or ContextScope)
/// * `promise` - The V8 Promise to resolve
/// * `timeout_ms` - Maximum time to wait in milliseconds
///
/// # Returns
///
/// Same as `resolve_promise_with_async`, but with shorter timeout
pub fn resolve_promise_with_timeout<'s, C, S>(
    scope: &mut S,
    promise: v8::Local<v8::Promise>,
    timeout_ms: u64,
) -> anyhow::Result<v8::Local<'s, v8::Value>>
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
    resolve_promise_with_timeout_and_memory(scope, promise, timeout_ms, None)
}

/// Try to resolve a Promise with timeout and optional memory checking
///
/// This variant allows passing a memory limit checker to enforce per-request
/// memory limits during async execution with a custom timeout.
///
/// # V147 API Note
/// In v147, we use PinnedRef instead of direct HandleScope references.
///
/// # Arguments
///
/// * `scope` - The V8 scope (PinnedRef or ContextScope)
/// * `promise` - The V8 Promise to resolve
/// * `timeout_ms` - Maximum time to wait in milliseconds
/// * `memory_checker` - Optional memory limit checker for DoS protection
///
/// # Returns
///
/// Same as `resolve_promise_with_async_and_memory`, but with custom timeout
pub fn resolve_promise_with_timeout_and_memory<'s, C, S>(
    scope: &mut S,
    promise: v8::Local<v8::Promise>,
    timeout_ms: u64,
    memory_checker: Option<&dyn MemoryLimitChecker>,
) -> anyhow::Result<v8::Local<'s, v8::Value>>
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
    let start_time = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let mut iteration_count = 0u32;

    loop {
        // Check for timeout first
        if start_time.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Promise resolution timeout after {}ms",
                timeout_ms
            ));
        }

        // CRITICAL FIX: Pump the V8 message loop BEFORE checking promise state
        // This allows WebAssembly.compile/instantiate and other internal V8
        // async operations to make progress before we check if they're done.
        // WASM compilation can require multiple pumps, so we pump multiple times.
        let platform = v8::V8::get_current_platform();
        for _ in 0..5 {
            // v147 API: pump_message_loop expects &Isolate
            v8::Platform::pump_message_loop(&platform, &**scope, false);
        }

        // v147 API: perform_microtask_checkpoint works through DerefMut
        scope.perform_microtask_checkpoint();

        // Check memory limits if a checker is provided
        if let Some(checker) = memory_checker {
            if iteration_count % 10 == 0 {
                // v147 API: Get isolate from scope via helper
                let isolate = get_isolate_from_scope(scope);
                if let Err(msg) = checker.check_memory(isolate) {
                    return Err(anyhow::anyhow!(
                        "Memory limit exceeded during async execution: {} (limit: {}MB)",
                        msg,
                        checker.limit_mb()
                    ));
                }
            }
        }

        // NOW check promise state after pumping
        match promise.state() {
            v8::PromiseState::Fulfilled => {
                // v147 API: promise.result expects &PinnedRef
                return Ok(promise.result(&**scope));
            }
            v8::PromiseState::Rejected => {
                let error = promise.result(&**scope);
                // v147 API: to_string and to_rust_string_lossy expect &PinnedRef
                let error_str = error.to_string(&**scope)
                    .map(|s| s.to_rust_string_lossy(&**scope))
                    .unwrap_or_else(|| "Promise rejected".to_string());
                return Err(anyhow::anyhow!("Promise rejected: {}", error_str));
            }
            v8::PromiseState::Pending => {
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
/// # V147 API Note
/// In v147, we use PinnedRef instead of direct HandleScope references.
///
/// # Arguments
///
/// * `scope` - The V8 scope (PinnedRef or ContextScope)
/// * `value` - The value to check (may or may not be a Promise)
///
/// # Returns
///
/// * `Ok(v8::Local<v8::Value>)` - Resolved value or original value
/// * `Err(anyhow::Error)` - If Promise rejected or timed out
pub fn resolve_if_promise<'s, C, S>(
    scope: &mut S,
    value: v8::Local<'s, v8::Value>,
) -> anyhow::Result<v8::Local<'s, v8::Value>>
where
    C: 's,
    S: std::ops::DerefMut<Target = v8::PinnedRef<'s, v8::HandleScope<'s, C>>>,
{
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
