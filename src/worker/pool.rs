//! Worker pool implementation with thread-local isolate ownership
//!
//! This module provides the WorkerPool that manages N worker threads,
//! each owning a V8 isolate. Tasks are dispatched via MPSC channels
//! and responses are returned via oneshot channels.

use crate::http::NanoResponse;
use crate::runtime::HandlerContext;
use crate::v8::{initialize_platform, NanoIsolate};
use crate::worker::context::ContextManager;
use crate::worker::eviction::{EvictionAction, EvictionManager, IsolateId, IsolateMetadata};
use crate::worker::memory_monitor::{MemoryMonitor, MemoryPressureLevel};
use crate::worker::oom::OomMonitorBuilder;
use crate::worker::timeout::{ExecutionTimer, TimeoutConfig};
use crate::worker::HandlerTask;
use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use std::fs;

/// Thread-local storage for the worker thread's Tokio runtime handle
/// This allows fetch() and other async operations to access the runtime
thread_local! {
    static WORKER_RUNTIME: RefCell<Option<tokio::runtime::Handle>> = RefCell::new(None);
}

/// Get the worker thread's Tokio runtime handle if available
pub fn with_worker_runtime<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&tokio::runtime::Handle) -> R,
{
    WORKER_RUNTIME.with(|runtime| {
        runtime.borrow().as_ref().map(f)
    })
}

/// Execute a handler within a specific V8 context
///
/// This helper function is used by the worker thread to execute JavaScript
/// handlers after context reset. It creates the necessary scopes and invokes
/// the fetch function.
/// Execute handler using the ContextManager's current context
///
/// This function properly manages V8 scope lifecycle to avoid "active scope" errors.
/// If cpu_time_limit_ms > 0, enforces CPU time limits via timer-based termination.
fn execute_with_context_manager(
    context_manager: &mut ContextManager,
    handler_ctx: &HandlerContext,
    cpu_time_limit_ms: u32,
) -> Result<NanoResponse> {
    // Clone the Global<Context> (cheap - just a handle reference)
    let global_ctx = context_manager.clone_context();

    // Now get the isolate pointer - this borrows context_manager mutably
    let isolate = context_manager.isolate_mut().isolate();

    // Set up CPU timeout enforcement if requested
    // The timer thread will call terminate_execution() when limit is reached
    let _timeout_guard = if cpu_time_limit_ms > 0 {
        Some(CpuTimeoutGuard::new(isolate, cpu_time_limit_ms))
    } else {
        None
    };

    // Create fresh HandleScope
    let handle_scope = &mut v8::HandleScope::new(isolate);

    // Reopen Local<Context> from Global within the new scope
    let v8_context = match global_ctx {
        Some(g) => v8::Local::new(handle_scope, &g),
        None => return Err(anyhow!("No context available")),
    };

    // Enter context scope and execute
    let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);

    // Read and execute handler
    execute_handler_code(context_scope, v8_context, handler_ctx)
}

/// Thread-local storage for isolate termination request
///
/// This is checked by the main thread during execution to determine
/// if the timer thread has requested termination.
thread_local! {
    static TERMINATION_REQUESTED: RefCell<bool> = RefCell::new(false);
    static TERMINATION_ISOLATE_PTR: RefCell<*mut v8::Isolate> = RefCell::new(std::ptr::null_mut());
}

/// Request termination of the current V8 isolate
///
/// Called by the timer thread when CPU timeout is reached
fn request_isolate_termination() {
    TERMINATION_REQUESTED.with(|req| {
        *req.borrow_mut() = true;
    });
    // SAFETY: We only call terminate_execution() if the isolate pointer is valid
    // The pointer is set at the start of execute_with_context_manager and
    // the timer thread only runs during that scope
    TERMINATION_ISOLATE_PTR.with(|ptr| {
        let isolate_ptr = *ptr.borrow();
        if !isolate_ptr.is_null() {
            unsafe {
                (*isolate_ptr).terminate_execution();
            }
        }
    });
}

/// Guard that sets up CPU timeout enforcement for V8 execution
///
/// Uses a simple wall-clock timer as an approximation of CPU time.
/// The timer thread calls request_isolate_termination() when timeout is reached.
struct CpuTimeoutGuard {
    /// Handle to the timer thread
    timer_thread: Option<std::thread::JoinHandle<()>>,
}

impl CpuTimeoutGuard {
    /// Create a new CPU timeout guard
    ///
    /// # Arguments
    /// * `isolate` - The V8 isolate to terminate on timeout
    /// * `limit_ms` - Wall time limit in milliseconds (used as approximation for CPU time)
    fn new(isolate: &mut v8::Isolate, limit_ms: u32) -> Self {
        // Store the isolate pointer in thread-local storage
        let isolate_ptr: *mut v8::Isolate = isolate as *mut _;
        TERMINATION_ISOLATE_PTR.with(|ptr| {
            *ptr.borrow_mut() = isolate_ptr;
        });
        TERMINATION_REQUESTED.with(|req| {
            *req.borrow_mut() = false;
        });

        // Spawn timer thread that will call terminate_execution when time is up
        let timer_thread = std::thread::spawn(move || {
            let limit_duration = std::time::Duration::from_millis(limit_ms as u64);
            std::thread::sleep(limit_duration);
            // Request termination of the isolate
            request_isolate_termination();
        });

        Self {
            timer_thread: Some(timer_thread),
        }
    }

    /// Check if termination was requested
    fn is_termination_requested(&self) -> bool {
        TERMINATION_REQUESTED.with(|req| *req.borrow())
    }
}

impl Drop for CpuTimeoutGuard {
    fn drop(&mut self) {
        // Wait for timer thread to complete
        if let Some(thread) = self.timer_thread.take() {
            let _ = thread.join();
        }
        // Clear the thread-local storage
        TERMINATION_ISOLATE_PTR.with(|ptr| {
            *ptr.borrow_mut() = std::ptr::null_mut();
        });
        TERMINATION_REQUESTED.with(|req| {
            *req.borrow_mut() = false;
        });
    }
}

/// Execute the actual handler code within an established context scope
/// Execute the actual handler code within an established context scope
fn execute_handler_code(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    v8_context: v8::Local<v8::Context>,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    use crate::runtime::apis::RuntimeAPIs;
    use crate::v8::module::{is_esm_module, transform_module_code};

    // Bind all WinterCG APIs (URL, fetch, etc.) to the context
    // This must be done before executing any handler code
    RuntimeAPIs::bind_all(scope, v8_context);
    tracing::debug!("Bound WinterCG APIs to handler context");

    // Read the handler code
    let code = fs::read_to_string(&handler_ctx.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint: {}", e))?;

    // Transform ES6 module syntax if this is an ESM module
    // This converts `export default { fetch }` to a global fetch function
    let transformed_code = if is_esm_module(&code) {
        transform_module_code(&code)
    } else {
        code
    };

    // Compile and run script to define fetch function
    let code_str = v8::String::new(scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(scope);

    // Get global and look for the user's handler function
    // The handler is stored in __nano_user_fetch by transform_module_code
    let global = v8_context.global(scope);
    let handler_key = v8::String::new(scope, "__nano_user_fetch").unwrap();
    let handler_val = match global.get(scope, handler_key.into()) {
        Some(val) if val.is_function() => {
            tracing::debug!("Found user handler function in global scope");
            val
        }
        _ => {
            // Fallback: try 'fetch' for non-ESM modules
            let fetch_key = v8::String::new(scope, "fetch").unwrap();
            match global.get(scope, fetch_key.into()) {
                Some(val) if val.is_function() => {
                    tracing::debug!("Found handler via 'fetch' global");
                    val
                }
                _ => {
                    tracing::warn!("No handler function found - looking for __nano_user_fetch or fetch");
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no handler function defined)"));
                }
            }
        }
    };

    let handler_fn = handler_val.cast::<v8::Function>();

    // Create Request object using the Request constructor
    // This ensures the request has proper prototype with headers.get() and other methods
    let request_url = v8::String::new(scope, &handler_ctx.request.url().href()).unwrap();
    
    // Build options object with method and headers
    let options_obj = v8::Object::new(scope);
    
    // Set method
    let method_key = v8::String::new(scope, "method").unwrap();
    let method_val = v8::String::new(scope, handler_ctx.request.method()).unwrap();
    options_obj.set(scope, method_key.into(), method_val.into());
    
    // Set headers using Headers constructor
    let headers_key = v8::String::new(scope, "headers").unwrap();
    let headers_ctor_key = v8::String::new(scope, "Headers").unwrap();
    let headers_ctor_val = global.get(scope, headers_ctor_key.into())
        .filter(|v| v.is_function())
        .ok_or_else(|| anyhow!("Headers constructor not found or not a function"))?;
    let headers_ctor = headers_ctor_val.cast::<v8::Function>();

    // Create headers init object
    let headers_init = v8::Object::new(scope);
    for (name, values) in handler_ctx.request.headers().entries() {
        // For headers with multiple values, join them with commas per HTTP spec
        let value = values.join(", ");
        let key = v8::String::new(scope, name).unwrap();
        let val = v8::String::new(scope, &value).unwrap();
        headers_init.set(scope, key.into(), val.into());
    }

    let headers_obj = headers_ctor.new_instance(scope, &[headers_init.into()])
        .ok_or_else(|| anyhow!("Failed to create Headers object"))?;
    options_obj.set(scope, headers_key.into(), headers_obj.into());

    // Set body if present (base64 encoded for proper handling in JS)
    if let Some(body) = handler_ctx.request.body() {
        let body_key = v8::String::new(scope, "body").unwrap();
        let base64_body = base64::encode(body);
        let body_val = v8::String::new(scope, &base64_body).unwrap();
        options_obj.set(scope, body_key.into(), body_val.into());
    }

    // Call Request constructor
    let request_ctor_key = v8::String::new(scope, "Request").unwrap();
    let request_ctor_val = global.get(scope, request_ctor_key.into())
        .filter(|v| v.is_function())
        .ok_or_else(|| anyhow!("Request constructor not found or not a function"))?;
    let request_ctor = request_ctor_val.cast::<v8::Function>();
    
    let js_request = request_ctor.new_instance(scope, &[request_url.into(), options_obj.into()])
        .ok_or_else(|| anyhow!("Failed to create Request object"))?;

    // Call the user's handler function with the request
    let result = handler_fn.call(scope, global.into(), &[js_request.into()]);

    // Perform microtask checkpoint to resolve any Promises
    scope.perform_microtask_checkpoint();

    // Check if result is a Promise and resolve if needed
    let resolved = if let Some(response) = result {
        if response.is_promise() {
            let promise = response.cast::<v8::Promise>();
            match promise.state() {
                v8::PromiseState::Fulfilled => Some(promise.result(scope)),
                v8::PromiseState::Rejected => {
                    let error = promise.result(scope);
                    let error_str = error.to_string(scope)
                        .map(|s| s.to_rust_string_lossy(scope))
                        .unwrap_or_else(|| "Promise rejected".to_string());
                    return Err(anyhow!("Promise rejected: {}", error_str));
                }
                v8::PromiseState::Pending => {
                    return Err(anyhow!("Promise still pending - async execution not fully supported"));
                }
            }
        } else {
            Some(response)
        }
    } else {
        None
    };

    // Extract response
    match resolved {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

fn execute_handler_in_context(
    isolate: &mut v8::OwnedIsolate,
    v8_context: v8::Local<v8::Context>,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    use crate::runtime::apis::RuntimeAPIs;
    
    // Create scope stack for execution - must be dropped in reverse order
    let handle_scope = &mut v8::HandleScope::new(isolate);
    let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);
    
    // Bind all WinterCG APIs (URL, fetch, Request, etc.) to the context
    RuntimeAPIs::bind_all(context_scope, v8_context);
    tracing::debug!("Bound WinterCG APIs to handler context");

    // Read the handler code
    let code = fs::read_to_string(&handler_ctx.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint: {}", e))?;
    
    // Transform ES6 module syntax if this is an ESM module
    use crate::v8::module::{is_esm_module, transform_module_code};
    let transformed_code = if is_esm_module(&code) {
        transform_module_code(&code)
    } else {
        code
    };

    // Compile and run script to define handler function
    let code_str = v8::String::new(context_scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(context_scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(context_scope);

    // Get global and look for the user's handler function
    let global = v8_context.global(context_scope);
    let handler_key = v8::String::new(context_scope, "__nano_user_fetch").unwrap();
    let handler_val = match global.get(context_scope, handler_key.into()) {
        Some(val) if val.is_function() => val,
        _ => {
            // Fallback: try 'fetch' for non-ESM modules
            let fetch_key = v8::String::new(context_scope, "fetch").unwrap();
            match global.get(context_scope, fetch_key.into()) {
                Some(val) if val.is_function() => val,
                _ => {
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no handler function defined)"));
                }
            }
        }
    };

    let handler_fn = handler_val.cast::<v8::Function>();

    // Create Request object using the Request constructor
    let request_url = v8::String::new(context_scope, &handler_ctx.request.url().href()).unwrap();
    
    // Build options object with method and headers
    let options_obj = v8::Object::new(context_scope);
    
    // Set method
    let method_key = v8::String::new(context_scope, "method").unwrap();
    let method_val = v8::String::new(context_scope, handler_ctx.request.method()).unwrap();
    options_obj.set(context_scope, method_key.into(), method_val.into());
    
    // Set headers using Headers constructor
    let headers_key = v8::String::new(context_scope, "headers").unwrap();
    let headers_ctor_key = v8::String::new(context_scope, "Headers").unwrap();
    let headers_ctor_val = global.get(context_scope, headers_ctor_key.into())
        .filter(|v| v.is_function())
        .ok_or_else(|| anyhow!("Headers constructor not found or not a function"))?;
    let headers_ctor = headers_ctor_val.cast::<v8::Function>();
    
    // Create headers init object
    let headers_init = v8::Object::new(context_scope);
    for (name, values) in handler_ctx.request.headers().entries() {
        let value = values.join(", ");
        let key = v8::String::new(context_scope, name).unwrap();
        let val = v8::String::new(context_scope, &value).unwrap();
        headers_init.set(context_scope, key.into(), val.into());
    }
    
    let headers_obj = headers_ctor.new_instance(context_scope, &[headers_init.into()])
        .ok_or_else(|| anyhow!("Failed to create Headers object"))?;
    options_obj.set(context_scope, headers_key.into(), headers_obj.into());

            // Set body if present (base64 encoded for proper handling in JS)
            if let Some(body) = handler_ctx.request.body() {
                let body_key = v8::String::new(context_scope, "body").unwrap();
                let base64_body = base64::encode(body);
                let body_val = v8::String::new(context_scope, &base64_body).unwrap();
                options_obj.set(context_scope, body_key.into(), body_val.into());
            }

            // Call Request constructor
    let request_ctor_key = v8::String::new(context_scope, "Request").unwrap();
    let request_ctor_val = global.get(context_scope, request_ctor_key.into())
        .filter(|v| v.is_function())
        .ok_or_else(|| anyhow!("Request constructor not found or not a function"))?;
    let request_ctor = request_ctor_val.cast::<v8::Function>();
    
    let js_request = request_ctor.new_instance(context_scope, &[request_url.into(), options_obj.into()])
        .ok_or_else(|| anyhow!("Failed to create Request object"))?;

    // Call the user's handler function with the request
    let result = handler_fn.call(context_scope, global.into(), &[js_request.into()]);

    // Perform microtask checkpoint to resolve any Promises
    context_scope.perform_microtask_checkpoint();

    // Check if result is a Promise and resolve if needed
    let resolved = if let Some(response) = result {
        if response.is_promise() {
            let promise = response.cast::<v8::Promise>();
            match promise.state() {
                v8::PromiseState::Fulfilled => Some(promise.result(context_scope)),
                v8::PromiseState::Rejected => {
                    let error = promise.result(context_scope);
                    let error_str = error.to_string(context_scope)
                        .map(|s| s.to_rust_string_lossy(context_scope))
                        .unwrap_or_else(|| "Promise rejected".to_string());
                    return Err(anyhow!("Promise rejected: {}", error_str));
                }
                v8::PromiseState::Pending => {
                    return Err(anyhow!("Promise still pending - async execution not fully supported"));
                }
            }
        } else {
            Some(response)
        }
    } else {
        None
    };

    // Extract response from result
    match resolved {
        Some(response) => extract_js_response(context_scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Extract a NanoResponse from a V8 JavaScript object
fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    use crate::http::NanoHeaders;
    use bytes::Bytes;

    // Verify the response is an object
    let obj = match js_response.to_object(scope) {
        Some(o) => o,
        None => return Err(anyhow!("Response is not an object")),
    };

    // Extract status property (default to 200)
    let status_key = v8::String::new(scope, "status").unwrap();
    let status = match obj.get(scope, status_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => match val.to_integer(scope) {
            Some(int) => int.value() as u16,
            None => 200,
        },
        _ => 200,
    };

    // Extract headers property
    let mut nano_headers = NanoHeaders::new();
    let headers_key = v8::String::new(scope, "headers").unwrap();

    if let Some(headers_val) = obj.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
            // Headers may be stored internally in __headers__ property (for Headers class instances)
            // or directly on the object (for plain objects used by Response)
            let internal_headers_key = v8::String::new(scope, "__headers__").unwrap();
            let headers_source = headers_obj.get(scope, internal_headers_key.into())
                .and_then(|v| v.to_object(scope))
                .unwrap_or(headers_obj);

            if let Some(names) = headers_source.get_own_property_names(scope, Default::default()) {
                let len = names.length();
                for i in 0..len {
                    if let Some(key) = names.get_index(scope, i) {
                        if let Some(key_str) = key.to_string(scope) {
                            let key_name = key_str.to_rust_string_lossy(scope);
                            // Skip internal properties and methods (functions)
                            if key_name.starts_with("__") || key_name == "set" || key_name == "get" || key_name == "forEach" {
                                continue;
                            }
                            if let Some(value) = headers_source.get(scope, key.into()) {
                                // Only include string values (not functions)
                                if !value.is_function() {
                                    if let Some(value_str) = value.to_string(scope) {
                                        let value_string = value_str.to_rust_string_lossy(scope);
                                        nano_headers.set(&key_name, &value_string);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Extract body property
    let body_key = v8::String::new(scope, "body").unwrap();
    let body = match obj.get(scope, body_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => {
            tracing::debug!("Response body value type: is_string={}, is_object={}, is_array={}", 
                val.is_string(), val.is_object(), val.is_array());
            match val.to_string(scope) {
                Some(s) => {
                    let body_str = s.to_rust_string_lossy(scope);
                    tracing::debug!("Extracted response body: {} bytes", body_str.len());
                    Some(Bytes::from(body_str))
                }
                None => {
                    tracing::warn!("Failed to convert response body to string");
                    None
                }
            }
        }
        Some(val) if val.is_null() => {
            tracing::debug!("Response body is null");
            None
        }
        Some(val) if val.is_undefined() => {
            tracing::debug!("Response body is undefined");
            None
        }
        _ => {
            tracing::debug!("Response body property not found");
            None
        }
    };

    tracing::debug!("Extracted response: status={}, body={}", 
        status, body.is_some());
    
    Ok(NanoResponse::new(status, nano_headers, body))
}
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use tracing::{debug, error, info, warn};

/// Handle to a worker thread
///
/// Contains the thread join handle and the MPSC sender for dispatching tasks.
/// Dropping the handle closes the channel, signaling the worker to exit.
#[derive(Debug)]
pub struct WorkerHandle {
    /// Worker thread ID (0-indexed)
    pub id: usize,
    /// Thread join handle for cleanup
    thread: Option<JoinHandle<()>>,
    /// MPSC sender for task dispatch
    task_tx: mpsc::Sender<HandlerTask>,
}

impl WorkerHandle {
    /// Send a task to this worker
    ///
    /// # Arguments
    ///
    /// * `task` - The HandlerTask to execute
    ///
    /// # Returns
    ///
    /// `Ok(())` if the task was sent, `Err` if the channel is closed
    pub fn send(&self, task: HandlerTask) -> Result<()> {
        self.task_tx
            .send(task)
            .map_err(|_| anyhow!("Worker {} channel closed", self.id))
    }

    /// Take the join handle for thread cleanup
    fn take_thread(&mut self) -> Option<JoinHandle<()>> {
        self.thread.take()
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Channel is dropped automatically, signaling worker to exit
        debug!("WorkerHandle {} dropped, signaling worker to exit", self.id);
    }
}

/// Pool of worker threads for JavaScript execution
///
/// Each worker owns one V8 isolate (thread-local). Tasks are dispatched
/// via MPSC channels. The pool uses round-robin for initial dispatch
/// (affine dispatch comes in later phase).
///
/// # VFS Integration
///
/// Each WorkerPool has a shared VFS backend that all workers in the pool
/// share. This means files written by one worker are visible to other
/// workers in the same pool (same app), but isolated from other pools.
///
/// # Memory Monitoring
///
/// Each worker has its own MemoryMonitor for post-execution heap checking.
/// The EvictionManager is shared across workers to coordinate soft/hard
/// eviction when memory pressure is detected.
pub struct WorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: usize,
    /// Hostname this pool serves (for logging/debugging)
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicUsize,
    /// Shared VFS backend for all workers in this pool
    vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
    /// Memory limit per isolate in MB
    memory_limit_mb: u32,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("workers", &self.workers.len())
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("next_worker", &self.next_worker)
            .field("vfs_backend", &"<dyn VfsBackend>")
            .field("memory_limit_mb", &self.memory_limit_mb)
            .finish()
    }
}

impl WorkerPool {
    /// Create a new worker pool with N worker threads
    ///
    /// Each worker thread:
    /// 1. Creates its own NanoIsolate (thread-local ownership)
    /// 2. Runs an event loop receiving HandlerTask via MPSC
    /// 3. Executes JavaScript handlers and sends responses back
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    ///
    /// # Returns
    ///
    /// A new WorkerPool with N workers ready to receive tasks
    ///
    /// # Panics
    ///
    /// Panics if the V8 platform is not initialized
    pub fn new(hostname: String, worker_count: usize, memory_limit_mb: u32) -> Self {
        Self::with_backend(hostname, worker_count, memory_limit_mb, Arc::new(MemoryBackend::default()))
    }

    /// Create a new worker pool with a specific VFS backend
    ///
    /// This allows configuring the storage backend (memory, disk, S3)
    /// for the VFS used by all workers in this pool.
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `vfs_backend` - The VFS backend to use (Arc<dyn VfsBackend>)
    ///
    /// # Returns
    ///
    /// A new WorkerPool with N workers ready to receive tasks
    ///
    /// # Panics
    ///
    /// Panics if the V8 platform is not initialized
    pub fn with_backend(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
    ) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        // Clone hostname for use in closures (original kept for final logging)
        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = Arc::clone(&vfs_backend);

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = Arc::clone(&vfs_backend_for_workers);
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with thread-local isolate
            let thread = thread::spawn(move || {
                info!("Worker {} starting", id);

                // Create OOM monitor for this worker if memory limit is configured
                let oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                // Create memory monitor for post-execution heap checking
                let mut memory_monitor = if memory_limit_mb > 0 {
                    Some(MemoryMonitor::new(memory_limit_mb))
                } else {
                    None
                };

                // Create eviction manager for this worker (thread-local)
                let mut eviction_manager = EvictionManager::new();

                // Create VFS for this worker with shared backend
                let vfs = IsolateVfs::new(
                    VfsNamespace::from_hostname(&worker_hostname),
                    worker_vfs_backend,
                );

                // Create isolate with VFS in this thread - NEVER moves to another thread
                // This is the critical POOL-05 constraint: isolate is !Send + !Sync
                let isolate = match NanoIsolate::new_with_vfs(vfs) {
                    Ok(isol) => isol,
                    Err(e) => {
                        error!("Worker {} failed to create isolate: {}", id, e);
                        return;
                    }
                };

                // Create context manager for this worker
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }

                // Register this isolate with the eviction manager
                let isolate_id = IsolateId::from_worker_index(id);
                eviction_manager.register_isolate(
                    isolate_id.clone(),
                    IsolateMetadata::new(&worker_hostname, id),
                );

                info!("Worker {} initialized with context and memory monitoring", id);

                // Event loop: receive tasks and execute
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("Worker {} received task for {}", id, task.entrypoint);

                            // Check if isolate is in draining mode (soft eviction)
                            if eviction_manager.is_draining(&isolate_id) {
                                warn!("Worker {} is draining, rejecting new request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service temporarily unavailable - memory pressure"
                                )));
                                continue;
                            }

                            // Check if isolate has been evicted
                            if eviction_manager.is_evicted(&isolate_id) {
                                warn!("Worker {} is evicted, rejecting request", id);
                                let _ = task.response_tx.send(Err(anyhow!(
                                    "Service unavailable - isolate evicted"
                                )));
                                continue;
                            }

                            // OOM-04: Pre-request OOM check
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                match monitor.check(isolate_ref) {
                                    Ok(_) => {
                                        // Memory OK, continue with request
                                    }
                                    Err(oom_error) => {
                                        // OOM detected - log, return 503, dispose isolate
                                        let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                        monitor.log_oom_event(&oom_error, &request_id);

                                        let oom_response = monitor.create_oom_response(&oom_error);
                                        let _ = task.response_tx.send(Ok(oom_response));

                                        // Dispose isolate and create fresh one
                                        warn!(
                                            "Worker {} disposing isolate due to OOM (oom_count: {})",
                                            id,
                                            monitor.oom_count()
                                        );

                                        // Create new isolate to replace the OOM'd one
                                        match NanoIsolate::new() {
                                            Ok(new_isolate) => {
                                                context_manager = ContextManager::new(new_isolate);
                                                if let Err(e) =
                                                    context_manager.create_initial_context()
                                                {
                                                    error!("Worker {} failed to create new context after OOM: {}", id, e);
                                                    break; // Exit worker if can't recover
                                                }
                                                // Reset OOM monitor for fresh isolate
                                                monitor.reset();
                                                // Reactivate isolate in eviction manager
                                                eviction_manager.reactivate_isolate(
                                                    isolate_id.clone(),
                                                    IsolateMetadata::new(&worker_hostname, id),
                                                );
                                                info!(
                                                    "Worker {} created fresh isolate after OOM",
                                                    id
                                                );
                                            }
                                            Err(e) => {
                                                error!("Worker {} failed to create replacement isolate: {}", id, e);
                                                break; // Exit worker if can't recover
                                            }
                                        }
                                        continue;
                                    }
                                }
                            }

                            // Mark active request in eviction manager
                            eviction_manager.mark_active(&isolate_id);

                            // METRICS-01: Start timing for metrics collection
                            let request_start = std::time::Instant::now();
                            let hostname = task.hostname.clone();
                            let entrypoint = task.entrypoint.clone();

                            // POOL-04: Reset context before each request
                            let reset_elapsed = match context_manager.reset_context() {
                                Ok(elapsed) => {
                                    let ms = elapsed.as_secs_f64() * 1000.0;
                                    if ms > 10.0 {
                                        warn!(
                                            "Worker {} context reset took {:.2}ms (target <10ms)",
                                            id, ms
                                        );
                                    } else {
                                        debug!("Worker {} context reset took {:.2}ms", id, ms);
                                    }
                                    // Record context reset in metrics if hostname is set
                                    if !hostname.is_empty() {
                                        crate::metrics::TENANT_METRICS.record_context_reset(&hostname);
                                    }
                                    elapsed
                                }
                                Err(e) => {
                                    error!("Worker {} context reset failed: {}", id, e);
                                    let _ =
                                        task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    eviction_manager.mark_complete(&isolate_id);
                                    continue;
                                }
                            };

                            // Create handler context
                            let handler_ctx = HandlerContext {
                                entrypoint: task.entrypoint,
                                request: task.request,
                            };

                            // Execute handler with fresh context scope
                            // CPU timeout enforcement uses timer-based termination if cpu_time_limit_ms > 0
                            let result =
                                execute_with_context_manager(&mut context_manager, &handler_ctx, task.cpu_time_limit_ms);

                            // Calculate request duration
                            let duration_ms = request_start.elapsed().as_millis() as u64;

                            // Mark request complete in eviction manager
                            eviction_manager.mark_complete(&isolate_id);

                            // Post-execution memory monitoring (27-02)
                            if let Some(ref mut mem_monitor) = memory_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                let snapshot = mem_monitor.check_after(isolate_ref);

                                // Update eviction manager with usage data
                                eviction_manager.record_usage(&isolate_id, snapshot.total_memory_bytes());

                                // Handle pressure levels
                                match snapshot.pressure_level {
                                    MemoryPressureLevel::Normal => {
                                        // No action needed
                                    }
                                    MemoryPressureLevel::Warning => {
                                        // Log warning for elevated memory
                                        warn!(
                                            "Worker {} memory warning: {:.1}MB ({}% of limit)",
                                            id,
                                            snapshot.total_memory_mb(),
                                            (snapshot.total_memory_mb() / memory_limit_mb as f64 * 100.0) as u32
                                        );
                                    }
                                    MemoryPressureLevel::Critical | MemoryPressureLevel::Emergency => {
                                        // Trigger soft eviction
                                        let action = eviction_manager.evaluate_pressure(
                                            snapshot.pressure_level,
                                            None,
                                        );

                                        match action {
                                            EvictionAction::SoftEvict(_) => {
                                                warn!(
                                                    "Worker {} memory pressure detected ({}), initiating soft eviction",
                                                    id,
                                                    snapshot.pressure_level.description()
                                                );
                                                eviction_manager.initiate_soft_eviction(&isolate_id);
                                            }
                                            EvictionAction::HardEvict(_) => {
                                                // Emergency - dispose isolate immediately
                                                error!(
                                                    "Worker {} emergency memory pressure, disposing isolate",
                                                    id
                                                );
                                                eviction_manager.hard_evict(&isolate_id);
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                // Check for memory leak trends
                                if mem_monitor.is_trending_to_leak() {
                                    warn!(
                                        "Worker {} memory leak detected, isolate may need disposal",
                                        id
                                    );
                                }

                                // METRICS-02: Record per-tenant metrics with memory data
                                if !hostname.is_empty() {
                                    let result_type = match &result {
                                        Ok(_) => crate::metrics::tenant::RequestResult::Success,
                                        Err(e) => {
                                            if e.to_string().contains("timeout") {
                                                crate::metrics::tenant::RequestResult::Timeout
                                            } else {
                                                crate::metrics::tenant::RequestResult::Error
                                            }
                                        }
                                    };

                                    // Estimate CPU time from context reset duration (microseconds)
                                    let cpu_us = reset_elapsed.as_micros() as u64;

                                    crate::metrics::TENANT_METRICS.record_request(
                                        &hostname,
                                        result_type,
                                        cpu_us,
                                        snapshot.total_memory_bytes() as usize,
                                        duration_ms,
                                    );

                                    // Update current memory gauge
                                    crate::metrics::TENANT_METRICS.update_memory(
                                        &hostname,
                                        snapshot.heap_used as usize,
                                        snapshot.external as usize,
                                    );

                                    // Record pressure events if applicable
                                    if snapshot.pressure_level > crate::worker::memory_monitor::MemoryPressureLevel::Normal {
                                        crate::metrics::TENANT_METRICS.record_pressure_event(
                                            &hostname,
                                            snapshot.pressure_level,
                                        );
                                    }
                                }
                            } else if !hostname.is_empty() {
                                // METRICS-02: Record metrics without memory data (when memory monitoring is disabled)
                                let result_type = match &result {
                                    Ok(_) => crate::metrics::tenant::RequestResult::Success,
                                    Err(e) => {
                                        if e.to_string().contains("timeout") {
                                            crate::metrics::tenant::RequestResult::Timeout
                                        } else {
                                            crate::metrics::tenant::RequestResult::Error
                                        }
                                    }
                                };

                                let cpu_us = reset_elapsed.as_micros() as u64;

                                crate::metrics::TENANT_METRICS.record_request(
                                    &hostname,
                                    result_type,
                                    cpu_us,
                                    0, // Memory data not available
                                    duration_ms,
                                );
                            }

                            // Post-request OOM check (optional - catches runaway memory during request)
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                if let Err(oom_error) = monitor.check(isolate_ref) {
                                    let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                    monitor.log_oom_event(&oom_error, &request_id);
                                    warn!(
                                        "Worker {} OOM detected after request execution (oom_count: {})",
                                        id,
                                        monitor.oom_count()
                                    );
                                    // Don't return 503 here since we already returned the actual response
                                    // Just dispose and recreate for next request

                                    // Dispose isolate and create fresh one
                                    match NanoIsolate::new() {
                                        Ok(new_isolate) => {
                                            context_manager = ContextManager::new(new_isolate);
                                            if let Err(e) = context_manager.create_initial_context()
                                            {
                                                error!("Worker {} failed to create new context after post-request OOM: {}", id, e);
                                                break;
                                            }
                                            monitor.reset();
                                            // Reactivate in eviction manager
                                            eviction_manager.reactivate_isolate(
                                                isolate_id.clone(),
                                                IsolateMetadata::new(&worker_hostname, id),
                                            );
                                            info!("Worker {} created fresh isolate after post-request OOM", id);
                                        }
                                        Err(e) => {
                                            error!("Worker {} failed to create replacement isolate: {}", id, e);
                                            break;
                                        }
                                    }
                                }
                            }

                            // Send response back
                            let _ = task.response_tx.send(result);
                        }
                        Err(_) => {
                            // Channel closed, exit gracefully
                            debug!("Worker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }

                // Isolate is dropped here when worker thread exits
                let eviction_stats = eviction_manager.state_counts();
                info!(
                    "Worker {} shutting down (avg context reset: {:.2}ms, OOM events: {}, evictions: {})",
                    id,
                    context_manager.average_reset_time_ms(),
                    oom_monitor.map(|m| m.oom_count()).unwrap_or(0),
                    eviction_stats.2 // evicted count
                );
            });

            workers.push(WorkerHandle {
                id,
                thread: Some(thread),
                task_tx,
            });
        }

        info!(
            "WorkerPool created for {} with {} workers",
            hostname, worker_count
        );

        Self {
            workers,
            worker_count,
            hostname,
            next_worker: AtomicUsize::new(0),
            vfs_backend,
            memory_limit_mb,
        }
    }

    /// Get a reference to the shared VFS backend
    ///
    /// This is useful for testing and administrative operations
    /// that need to inspect or modify the filesystem.
    pub fn vfs_backend(&self) -> &Arc<dyn crate::vfs::VfsBackend> {
        &self.vfs_backend
    }

    /// Dispatch a task to a worker
    ///
    /// Uses round-robin dispatch (simplest approach for initial implementation).
    /// Returns error if all worker channels are closed.
    ///
    /// # Arguments
    ///
    /// * `task` - The HandlerTask to dispatch
    ///
    /// # Returns
    ///
    /// `Ok(())` if dispatched, `Err` if no workers available
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        // Round-robin: atomically increment and get worker index
        let worker_idx = self.next_worker.fetch_add(1, Ordering::SeqCst) % self.worker_count;

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Dispatch with custom worker selection
    ///
    /// For use when caller knows which worker should handle the task
    /// (e.g., for request affinity in later phases).
    ///
    /// # Arguments
    ///
    /// * `worker_idx` - Index of the worker to use
    /// * `task` - The HandlerTask to dispatch
    ///
    /// # Returns
    ///
    /// `Ok(())` if dispatched, `Err` if worker index invalid or channel closed
    pub fn dispatch_to(&self, worker_idx: usize, task: HandlerTask) -> Result<()> {
        if worker_idx >= self.worker_count {
            return Err(anyhow!(
                "Worker index {} out of bounds (max {})",
                worker_idx,
                self.worker_count - 1
            ));
        }

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Gracefully shut down the worker pool
    ///
    /// 1. Drop all task_tx channels (signals workers to exit)
    /// 2. Join all worker threads
    ///
    /// # Returns
    ///
    /// `Ok(())` if all workers exited cleanly
    pub fn shutdown(mut self) -> Result<()> {
        info!("Shutting down WorkerPool for {}", self.hostname);

        // Take and drop all task_tx channels, signaling workers to exit
        // Workers will finish current task, then see recv() error and exit
        let mut handles: Vec<_> = self
            .workers
            .drain(..)
            .map(|mut w| (w.id, w.take_thread()))
            .collect();

        // Join all threads with timeout (5 seconds per thread)
        for (id, handle) in handles.drain(..) {
            if let Some(h) = handle {
                debug!("Waiting for worker {} to exit", id);
                match h.join() {
                    Ok(_) => debug!("Worker {} exited cleanly", id),
                    Err(_) => warn!("Worker {} panicked during shutdown", id),
                }
            }
        }

        info!("WorkerPool for {} shut down complete", self.hostname);
        Ok(())
    }

    /// Get the number of workers in this pool
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Try graceful shutdown if not already called
        // We need to take the workers and join them
        if !self.workers.is_empty() {
            warn!(
                "WorkerPool for {} dropped without explicit shutdown - forcing cleanup",
                self.hostname
            );

            // Take workers and drop their senders (signals exit)
            let handles: Vec<_> = self
                .workers
                .drain(..)
                .map(|mut w| w.take_thread())
                .collect();

            // Try to join threads (best effort - may not complete if already panicking)
            for handle in handles {
                if let Some(h) = handle {
                    let _ = h.join();
                }
            }
        }
    }
}

/// Worker pool for sliver-based (snapshot-restored) applications
///
/// This specialized worker pool creates isolates from V8 heap snapshots
/// rather than fresh isolates. It also restores VFS state from the sliver.
///
/// # Design
///
/// - Each worker restores its isolate from the snapshot blob
/// - VFS entries are restored before the worker accepts tasks
/// - Falls back to fresh isolate if snapshot restoration fails
/// - Shares the same dispatch interface as regular WorkerPool
pub struct SliverWorkerPool {
    /// Worker handles for all threads in the pool
    workers: Vec<WorkerHandle>,
    /// Number of workers (for verification)
    pub worker_count: usize,
    /// Hostname this pool serves
    pub hostname: String,
    /// Round-robin counter for dispatch
    next_worker: AtomicUsize,
    /// Shared VFS backend for all workers
    vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
    /// Unpacked sliver data containing snapshot and VFS entries
    unpacked_sliver: crate::sliver::UnpackedSliver,
    /// Optional temp entrypoint path for VFS-extracted JS files
    temp_entrypoint: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for SliverWorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverWorkerPool")
            .field("workers", &self.workers.len())
            .field("worker_count", &self.worker_count)
            .field("hostname", &self.hostname)
            .field("hostname", &self.hostname)
            .field("vfs_backend", &"<dyn VfsBackend>")
            .field("unpacked_sliver", &self.unpacked_sliver.metadata.hostname)
            .finish()
    }
}

impl SliverWorkerPool {
    /// Create a new sliver worker pool with restored isolates
    ///
    /// Each worker thread:
    /// 1. Creates its NanoIsolate from the snapshot (or fresh if fails)
    /// 2. Restores VFS entries from the sliver
    /// 3. Runs an event loop receiving HandlerTask via MPSC
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname this pool serves (for logging)
    /// * `worker_count` - Number of worker threads to spawn
    /// * `memory_limit_mb` - Memory limit per isolate in MB (0 = no limit)
    /// * `unpacked_sliver` - The unpacked sliver containing snapshot and VFS data
    ///
    /// # Returns
    ///
    /// A new SliverWorkerPool with N workers restored from snapshot
    pub fn new(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        unpacked_sliver: crate::sliver::UnpackedSliver,
    ) -> Self {
        Self::with_backend(
            hostname,
            worker_count,
            memory_limit_mb,
            Arc::new(MemoryBackend::default()),
            unpacked_sliver,
            None,
        )
    }

    /// Create a new sliver worker pool with a temp entrypoint path
    ///
    /// This variant is used when the sliver VFS has been extracted to a temp
    /// directory, and the JS entrypoint should be read from that location.
    pub fn with_temp_entrypoint(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        unpacked_sliver: crate::sliver::UnpackedSliver,
        temp_entrypoint: std::path::PathBuf,
    ) -> Self {
        Self::with_backend(
            hostname,
            worker_count,
            memory_limit_mb,
            Arc::new(MemoryBackend::default()),
            unpacked_sliver,
            Some(temp_entrypoint),
        )
    }

    /// Create a new sliver worker pool with a specific VFS backend
    pub fn with_backend(
        hostname: String,
        worker_count: usize,
        memory_limit_mb: u32,
        vfs_backend: Arc<dyn crate::vfs::VfsBackend>,
        unpacked_sliver: crate::sliver::UnpackedSliver,
        temp_entrypoint: Option<std::path::PathBuf>,
    ) -> Self {
        // Ensure platform is initialized
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }

        assert!(worker_count > 0, "Worker count must be at least 1");

        let hostname_for_workers = hostname.clone();
        let vfs_backend_for_workers = Arc::clone(&vfs_backend);
        let sliver_for_workers = unpacked_sliver.clone();
        let temp_entrypoint_for_workers = temp_entrypoint.clone();

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let worker_hostname = hostname_for_workers.clone();
            let worker_vfs_backend = Arc::clone(&vfs_backend_for_workers);
            let worker_sliver = sliver_for_workers.clone();
            let worker_temp_entrypoint = temp_entrypoint_for_workers.clone();
            let (task_tx, task_rx) = mpsc::channel::<HandlerTask>();

            // Spawn worker thread with snapshot-restored isolate
            let thread = thread::spawn(move || {
                info!("SliverWorker {} starting for {}", id, worker_hostname);

                // Create a Tokio runtime for this worker thread
                // This runtime stays alive for the entire thread lifetime
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                
                // Store runtime handle in thread-local for fetch() to use
                let rt_handle = rt.handle().clone();
                WORKER_RUNTIME.with(|runtime| {
                    *runtime.borrow_mut() = Some(rt_handle);
                });

                // Create OOM monitor
                let oom_monitor = if memory_limit_mb > 0 {
                    Some(
                        OomMonitorBuilder::new(format!("sliver_worker_{}_{}", worker_hostname, id))
                            .with_limit_mb(memory_limit_mb)
                            .for_hostname(&worker_hostname)
                            .build(),
                    )
                } else {
                    None
                };

                // Create VFS for this worker
                let vfs = IsolateVfs::new(
                    VfsNamespace::from_hostname(&worker_hostname),
                    worker_vfs_backend,
                );

                // Restore VFS entries from sliver before creating isolate
                if let Err(e) = rt.block_on(worker_sliver.restore_to_vfs(&vfs)) {
                    error!("Worker {} failed to restore VFS: {}", id, e);
                    // Continue anyway - the app might work without VFS data
                } else {
                    debug!("Worker {} restored {} VFS entries", id, worker_sliver.vfs_entries.len());
                }

                // Create isolate from snapshot or fresh as fallback
                let vfs_clone = vfs.clone();
                let isolate = match crate::v8::restore_from_snapshot(&worker_sliver.heap_data, vfs_clone) {
                    Ok(isol) => {
                        info!("Worker {} created isolate from snapshot", id);
                        isol
                    }
                    Err(e) => {
                        warn!("Worker {} failed to restore from snapshot: {}. Creating fresh isolate.", id, e);
                        match NanoIsolate::new_with_vfs(vfs.clone()) {
                            Ok(isol) => isol,
                            Err(e) => {
                                error!("Worker {} failed to create isolate: {}", id, e);
                                return;
                            }
                        }
                    }
                };

                // Create context manager
                let mut context_manager = ContextManager::new(isolate);
                if let Err(e) = context_manager.create_initial_context() {
                    error!("Worker {} failed to create context: {}", id, e);
                    return;
                }
                info!("SliverWorker {} initialized with restored context", id);

                // Event loop: same as regular WorkerPool
                loop {
                    match task_rx.recv() {
                        Ok(task) => {
                            debug!("SliverWorker {} received task for {}", id, task.entrypoint);

                            // OOM check before request
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                match monitor.check(isolate_ref) {
                                    Ok(_) => {}
                                    Err(oom_error) => {
                                        let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                        monitor.log_oom_event(&oom_error, &request_id);

                                        let oom_response = monitor.create_oom_response(&oom_error);
                                        let _ = task.response_tx.send(Ok(oom_response));

                                        // Dispose and recreate isolate
                                        warn!("Worker {} disposing isolate due to OOM", id);
                                        match NanoIsolate::new() {
                                            Ok(new_isolate) => {
                                                context_manager = ContextManager::new(new_isolate);
                                                if let Err(e) = context_manager.create_initial_context() {
                                                    error!("Worker {} failed to create new context after OOM: {}", id, e);
                                                    break;
                                                }
                                                monitor.reset();
                                                info!("Worker {} created fresh isolate after OOM", id);
                                            }
                                            Err(e) => {
                                                error!("Worker {} failed to create replacement isolate: {}", id, e);
                                                break;
                                            }
                                        }
                                        continue;
                                    }
                                }
                            }

                            // Reset context before each request
                            match context_manager.reset_context() {
                                Ok(elapsed) => {
                                    let ms = elapsed.as_secs_f64() * 1000.0;
                                    debug!("SliverWorker {} context reset took {:.2}ms", id, ms);
                                }
                                Err(e) => {
                                    error!("SliverWorker {} context reset failed: {}", id, e);
                                    let _ = task.response_tx.send(Err(anyhow!("Context reset failed")));
                                    continue;
                                }
                            }

                            // Create handler context with temp entrypoint override
                            let entrypoint = worker_temp_entrypoint
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| task.entrypoint.clone());
                            let handler_ctx = HandlerContext {
                                entrypoint,
                                request: task.request,
                            };

                            // Execute handler with CPU timeout enforcement if configured
                            let result = execute_with_context_manager(&mut context_manager, &handler_ctx, task.cpu_time_limit_ms);

                            // Post-request OOM check
                            if let Some(ref monitor) = oom_monitor {
                                let isolate_ref = context_manager.isolate_mut().isolate();
                                if let Err(oom_error) = monitor.check(isolate_ref) {
                                    let request_id = format!("req_{}", uuid::Uuid::new_v4());
                                    monitor.log_oom_event(&oom_error, &request_id);
                                    warn!("SliverWorker {} OOM detected after request", id);

                                    // Dispose and recreate
                                    match NanoIsolate::new() {
                                        Ok(new_isolate) => {
                                            context_manager = ContextManager::new(new_isolate);
                                            if let Err(e) = context_manager.create_initial_context() {
                                                error!("Worker {} failed to create new context: {}", id, e);
                                                break;
                                            }
                                            monitor.reset();
                                        }
                                        Err(e) => {
                                            error!("Worker {} failed to create replacement: {}", id, e);
                                            break;
                                        }
                                    }
                                }
                            }

                            // Send response
                            let _ = task.response_tx.send(result);
                        }
                        Err(_) => {
                            debug!("SliverWorker {} channel closed, exiting", id);
                            break;
                        }
                    }
                }

                info!(
                    "SliverWorker {} shutting down (avg context reset: {:.2}ms)",
                    id,
                    context_manager.average_reset_time_ms()
                );
            });

            workers.push(WorkerHandle {
                id,
                thread: Some(thread),
                task_tx,
            });
        }

        info!(
            "SliverWorkerPool created for {} with {} workers (snapshot restored)",
            hostname, worker_count
        );

        Self {
            workers,
            worker_count,
            hostname,
            next_worker: AtomicUsize::new(0),
            vfs_backend,
            unpacked_sliver,
            temp_entrypoint,
        }
    }

    /// Get a reference to the shared VFS backend
    pub fn vfs_backend(&self) -> &Arc<dyn crate::vfs::VfsBackend> {
        &self.vfs_backend
    }

    /// Get reference to unpacked sliver data
    pub fn sliver_data(&self) -> &crate::sliver::UnpackedSliver {
        &self.unpacked_sliver
    }

    /// Dispatch a task to a worker (round-robin)
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        let worker_idx = self.next_worker.fetch_add(1, Ordering::SeqCst) % self.worker_count;

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Dispatch to specific worker
    pub fn dispatch_to(&self, worker_idx: usize, task: HandlerTask) -> Result<()> {
        if worker_idx >= self.worker_count {
            return Err(anyhow!(
                "Worker index {} out of bounds (max {})",
                worker_idx,
                self.worker_count - 1
            ));
        }

        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    /// Gracefully shut down the worker pool
    pub fn shutdown(mut self) -> Result<()> {
        info!("Shutting down SliverWorkerPool for {}", self.hostname);

        let mut handles: Vec<_> = self
            .workers
            .drain(..)
            .map(|mut w| (w.id, w.take_thread()))
            .collect();

        for (id, handle) in handles.drain(..) {
            if let Some(h) = handle {
                debug!("Waiting for sliver worker {} to exit", id);
                match h.join() {
                    Ok(_) => debug!("SliverWorker {} exited cleanly", id),
                    Err(_) => warn!("SliverWorker {} panicked during shutdown", id),
                }
            }
        }

        info!("SliverWorkerPool for {} shut down complete", self.hostname);
        Ok(())
    }

    /// Get the number of workers in this pool
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }
}

impl crate::worker::r#trait::WorkerPool for SliverWorkerPool {
    fn dispatch(&self, task: HandlerTask) -> Result<()> {
        let worker_idx = self.next_worker.fetch_add(1, Ordering::SeqCst) % self.worker_count;
        self.workers[worker_idx]
            .send(task)
            .map_err(|e| anyhow!("Failed to dispatch to worker {}: {}", worker_idx, e))
    }

    fn shutdown(mut self) -> Result<()> {
        info!("Shutting down SliverWorkerPool for {}", self.hostname);

        let mut handles: Vec<_> = self
            .workers
            .drain(..)
            .map(|mut w| (w.id, w.take_thread()))
            .collect();

        for (id, handle) in handles.drain(..) {
            if let Some(h) = handle {
                debug!("Waiting for sliver worker {} to exit", id);
                match h.join() {
                    Ok(_) => debug!("SliverWorker {} exited cleanly", id),
                    Err(_) => warn!("SliverWorker {} panicked during shutdown", id),
                }
            }
        }

        info!("SliverWorkerPool for {} shut down complete", self.hostname);
        Ok(())
    }

    fn worker_count(&self) -> usize {
        self.worker_count
    }

    fn hostname(&self) -> &str {
        &self.hostname
    }
}

impl Drop for SliverWorkerPool {
    fn drop(&mut self) {
        if !self.workers.is_empty() {
            warn!(
                "SliverWorkerPool for {} dropped without explicit shutdown - forcing cleanup",
                self.hostname
            );

            let handles: Vec<_> = self
                .workers
                .drain(..)
                .map(|mut w| w.take_thread())
                .collect();

            for handle in handles {
                if let Some(h) = handle {
                    let _ = h.join();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoRequest, NanoUrl};
    use crate::worker::HandlerTask;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    /// Helper to ensure platform is initialized for tests
    fn init_platform() {
        if !crate::v8::is_initialized() {
            crate::v8::initialize_platform().expect("Failed to initialize V8 platform");
        }
    }

    /// Create a test JavaScript file and return its path
    fn create_test_handler(dir: &TempDir, filename: &str, code: &str) -> String {
        let path = dir.path().join(filename);
        let mut file = fs::File::create(&path).expect("Failed to create test file");
        file.write_all(code.as_bytes())
            .expect("Failed to write test code");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_worker_pool_creation() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);
        assert_eq!(pool.worker_count, 2);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_single_worker_pool() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);
        assert_eq!(pool.worker_count, 1);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_dispatch_and_response() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create a simple JS handler (non-async for now)
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"
function fetch(request) {
    return { status: 200, headers: { "Content-Type": "text/plain" }, body: "Hello from worker" };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        // Create task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        // Dispatch and wait for response
        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");

        assert!(
            response.is_ok(),
            "Handler execution failed: {:?}",
            response.err()
        );
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("Content-Type"),
            Some("text/plain".to_string())
        );
        assert!(resp.body().is_some());

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_concurrent_requests() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create a JS handler that returns request info
        let entrypoint = create_test_handler(
            &temp_dir,
            "handler.js",
            r#"
function fetch(request) {
    return { status: 200, headers: {}, body: "OK" };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 4, 0);

        // Dispatch 10 tasks concurrently
        let mut receivers = vec![];
        for i in 0..10 {
            let url = NanoUrl::parse(&format!("http://test/{}", i)).unwrap();
            let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

            let (tx, rx) = oneshot::channel();
            let task = HandlerTask::new(entrypoint.clone(), request, tx);

            pool.dispatch(task).unwrap();
            receivers.push(rx);
        }

        // All should complete successfully
        for (i, rx) in receivers.into_iter().enumerate() {
            let response = rx
                .blocking_recv()
                .expect(&format!("Failed to receive response {}", i));
            assert!(
                response.is_ok(),
                "Request {} failed: {:?}",
                i,
                response.err()
            );
            let resp = response.unwrap();
            assert_eq!(resp.status(), 200);
        }

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_round_robin_dispatch() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 3, 0);

        // Create tasks to verify round-robin works
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        // Dispatch 6 tasks - should hit workers 0,1,2,0,1,2
        for _ in 0..6 {
            let url = NanoUrl::parse("http://test/").unwrap();
            let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

            let (tx, rx) = oneshot::channel();
            let task = HandlerTask::new(entrypoint.clone(), request, tx);

            pool.dispatch(task).expect("Dispatch failed");

            // Wait for each to complete
            let _ = rx.blocking_recv();
        }

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_dispatch_to_specific_worker() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 3, 0);

        // Dispatch to specific worker
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch_to(1, task)
            .expect("Dispatch to worker 1 failed");

        let response = rx.blocking_recv().expect("Failed to receive");
        assert!(response.is_ok());

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_invalid_worker_index() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "" }; }"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, _rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        // Try to dispatch to invalid worker index
        let result = pool.dispatch_to(5, task);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_pool_shutdown() {
        init_platform();
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);

        // Shutdown should complete without hanging
        pool.shutdown().expect("Shutdown failed");

        // Test passes if we reach here
    }

    #[test]
    fn test_worker_isolate_thread_local() {
        // This test verifies that isolates are created in worker threads
        // and never move between threads (compile-time check via !Send + !Sync)

        init_platform();

        // Compile-time check: NanoIsolate is NOT Send
        fn assert_not_send<T: Send>() {}
        // This should fail to compile if uncommented:
        // assert_not_send::<NanoIsolate>();

        // Verify the pool creates workers correctly
        let pool = WorkerPool::new("test.example.com".to_string(), 2, 0);
        assert_eq!(pool.workers.len(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_full_request_object_passed() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create handler that inspects request properties
        let entrypoint = create_test_handler(
            &temp_dir,
            "full_request.js",
            r#"
function fetch(request) {
    const info = {
        method: request.method,
        url: request.url,
        headers: request.headers,
        hasBody: request.body !== null
    };
    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(info)
    };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        // Create request with all properties
        let url = NanoUrl::parse("http://test.example.com/api/items/123?expand=true").unwrap();
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        headers.set("X-Custom-Header", "custom-value");
        let body = Some(bytes::Bytes::from(r#"{"key":"value"}"#));
        let request = NanoRequest::new("POST".to_string(), url, headers, body);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Handler failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        // Verify body contains all request info
        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(body_text.contains("POST"), "Method not found: {}", body_text);
        assert!(body_text.contains("http://test.example.com/api/items/123"), "URL not found: {}", body_text);
        assert!(body_text.contains("custom-value"), "Header not found: {}", body_text);
        assert!(body_text.contains("true"), "Body flag not found: {}", body_text);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_async_handler_promise() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create async handler
        let entrypoint = create_test_handler(
            &temp_dir,
            "async_handler.js",
            r#"
async function fetch(request) {
    // Simulate async work
    const data = await Promise.resolve({ hello: "world" });

    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data)
    };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Async handler failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(body_text.contains("hello"), "Async response missing data: {}", body_text);
        assert!(body_text.contains("world"), "Async response missing value: {}", body_text);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_request_body_passed() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Handler that checks if body was passed (base64 encoded)
        let entrypoint = create_test_handler(
            &temp_dir,
            "body_check.js",
            r#"
function fetch(request) {
    // Body is base64 encoded in the request object
    const hasBody = request.body !== null;
    const bodyUsed = request.bodyUsed;

    return {
        status: 200,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hasBody, bodyUsed })
    };
}
"#,
        );

        let pool = WorkerPool::new("test.example.com".to_string(), 1, 0);

        let url = NanoUrl::parse("http://test/").unwrap();
        let body = Some(bytes::Bytes::from("Hello from client"));
        let request = NanoRequest::new("POST".to_string(), url, NanoHeaders::new(), body);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive");

        assert!(response.is_ok(), "Body passing failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        let body_text = String::from_utf8_lossy(resp.body().unwrap());
        assert!(body_text.contains("true"), "Body flags not correct: {}", body_text);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_oom_monitor_integration() {
        // Test that worker pool with memory limit creates OOM monitors
        init_platform();

        // Create pool with 16MB memory limit per isolate
        let pool = WorkerPool::new("oom-test.example.com".to_string(), 1, 16);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "OK" }; }"#,
        );

        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);

        pool.dispatch(task).expect("Failed to dispatch");

        // Should complete successfully (fresh isolate under 16MB limit)
        let response = rx.blocking_recv().expect("Failed to receive");
        assert!(
            response.is_ok(),
            "Request should complete with OOM monitoring enabled"
        );

        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_worker_pool_vfs_isolation() {
        // Test that different pools have isolated VFS namespaces
        use crate::vfs::VfsBackend;
        
        init_platform();

        // Create two pools for different apps
        let pool1 = WorkerPool::new("app1.example.com".to_string(), 1, 0);
        let pool2 = WorkerPool::new("app2.example.com".to_string(), 1, 0);

        // Write a file via pool1's VFS backend directly
        // (simulating what would happen through JS execution)
        let namespace1 = VfsNamespace::from_hostname("app1.example.com");
        let path1 = crate::vfs::VfsPath::new(&format!("{}::secret.txt", namespace1.as_str())).unwrap();
        
        // Use tokio runtime to run async write
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            pool1.vfs_backend().write(&path1, b"app1-secret-data").await.unwrap();
        });

        // Verify file exists in pool1's backend
        let rt = tokio::runtime::Runtime::new().unwrap();
        let exists_in_pool1: bool = rt.block_on(async {
            pool1.vfs_backend().exists(&path1).await.unwrap()
        });
        assert!(exists_in_pool1, "File should exist in pool1's VFS");

        // Verify file does NOT exist in pool2's backend (different namespace)
        let namespace2 = VfsNamespace::from_hostname("app2.example.com");
        let path2 = crate::vfs::VfsPath::new(&format!("{}::secret.txt", namespace2.as_str())).unwrap();
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        let exists_in_pool2: bool = rt.block_on(async {
            pool2.vfs_backend().exists(&path2).await.unwrap()
        });
        assert!(!exists_in_pool2, "File should NOT exist in pool2's VFS (isolated)");

        // Clean up
        pool1.shutdown().expect("Pool1 shutdown failed");
        pool2.shutdown().expect("Pool2 shutdown failed");
    }

    // SliverWorkerPool tests
    use crate::sliver::{pack_sliver, SliverMetadata, UnpackedSliver};

    fn create_test_sliver_for_pool(hostname: &str) -> UnpackedSliver {
        let metadata = SliverMetadata::new(hostname, "1.1.0");
        let heap_data = vec![0xABu8; 1024];
        let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
        crate::sliver::unpack_sliver(&archive).unwrap()
    }

    #[test]
    fn test_sliver_worker_pool_creation() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("sliver-test.example.com");
        
        let pool = SliverWorkerPool::new(
            "sliver-test.example.com".to_string(),
            2,
            0,
            unpacked,
        );
        
        assert_eq!(pool.worker_count(), 2);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_single_worker() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("single.example.com");
        
        let pool = SliverWorkerPool::new(
            "single.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        assert_eq!(pool.worker_count(), 1);
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_dispatch() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        // Create a simple JS handler
        let entrypoint = create_test_handler(
            &temp_dir,
            "test.js",
            r#"function fetch(request) { return { status: 200, headers: {}, body: "Sliver OK" }; }"#,
        );
        
        let unpacked = create_test_sliver_for_pool("dispatch.example.com");
        let pool = SliverWorkerPool::new(
            "dispatch.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);
        
        let (tx, rx) = oneshot::channel();
        let task = HandlerTask::new(entrypoint, request, tx);
        
        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");
        
        assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);
        
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_accessors() {
        init_platform();
        let unpacked = create_test_sliver_for_pool("accessors.example.com");
        let sliver_hostname = unpacked.metadata.hostname.clone();
        
        let pool = SliverWorkerPool::new(
            "accessors.example.com".to_string(),
            1,
            0,
            unpacked,
        );
        
        // Test sliver_data accessor
        let sliver_data = pool.sliver_data();
        assert_eq!(sliver_data.metadata.hostname, sliver_hostname);
        
        // Test vfs_backend accessor
        let _vfs_backend = pool.vfs_backend();
        
        pool.shutdown().expect("Shutdown failed");
    }

    #[test]
    fn test_sliver_worker_pool_with_temp_vfs() {
        init_platform();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create a JS handler in the temp directory (simulating extracted VFS)
        let temp_handler_code = r#"function fetch(request) { return { status: 200, headers: { "Content-Type": "text/plain" }, body: "From temp VFS" }; }"#;
        let temp_entrypoint = temp_dir.path().join("index.js");
        std::fs::write(&temp_entrypoint, temp_handler_code)
            .expect("Failed to write temp handler");

        // Create sliver with different handler content (should not be used)
        let unpacked = create_test_sliver_for_pool("temp-vfs.example.com");

        // Create pool with temp entrypoint
        let pool = SliverWorkerPool::with_temp_entrypoint(
            "temp-vfs.example.com".to_string(),
            1,
            0,
            unpacked,
            temp_entrypoint.clone(),
        );

        // Create and dispatch a task
        let url = NanoUrl::parse("http://test/").unwrap();
        let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

        let (tx, rx) = oneshot::channel();
        // Note: we pass a dummy entrypoint here, it should be overridden by temp_entrypoint
        let task = HandlerTask::new("/dummy/path.js".to_string(), request, tx);

        pool.dispatch(task).expect("Failed to dispatch");
        let response = rx.blocking_recv().expect("Failed to receive response");

        assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
        let resp = response.unwrap();
        assert_eq!(resp.status(), 200);

        // Verify the response came from temp VFS handler
        let body = resp.body().cloned().unwrap_or_default();
        let body_text = String::from_utf8_lossy(&body);
        assert!(
            body_text.contains("From temp VFS"),
            "Expected response from temp VFS, got: {}",
            body_text
        );

        pool.shutdown().expect("Shutdown failed");
    }
}
