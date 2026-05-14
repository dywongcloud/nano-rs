//! Data Plane: Optimized request execution.
//!
//! Per TigerStyle:
//! - NO validation checks in hot path (control plane handles validation)
//! - NO dynamic allocations (pre-allocated by control plane)
//! - Minimal branching (lookup tables over conditionals)
//! - Zero-copy where possible
//! - CPU sprints through batches

use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use anyhow::{anyhow, Result};
use base64::Engine;
use bytes::Bytes;

use crate::http::NanoResponse;
use crate::runtime::{HandlerContext, async_support};
use crate::runtime::apis::RuntimeAPIs;
use crate::v8::module::{is_esm_module, transform_module_code};
use crate::worker::context::ContextManager;
use crate::worker::HandlerTask;

// Thread-local storage for the worker thread's Tokio runtime handle.
// This allows fetch() and other async operations to access the runtime.
thread_local! {
    static WORKER_RUNTIME: RefCell<Option<tokio::runtime::Handle>> = RefCell::new(None);
}

/// Get the worker thread's Tokio runtime handle if available.
pub fn with_worker_runtime<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&tokio::runtime::Handle) -> R,
{
    WORKER_RUNTIME.with(|runtime| {
        runtime.borrow().as_ref().map(f)
    })
}

/// Set the worker runtime handle for the current thread.
pub fn set_worker_runtime(handle: tokio::runtime::Handle) {
    WORKER_RUNTIME.with(|runtime| {
        *runtime.borrow_mut() = Some(handle);
    });
}

/// Code cache entry with modification time tracking.
struct CodeCacheEntry {
    code: Arc<str>,
    modified: SystemTime,
}

/// Thread-safe code cache to avoid disk reads on every request.
///
/// This significantly reduces latency for frequently accessed entrypoints
/// by caching the file contents in memory and only re-reading when the
/// file modification time changes.
static CODE_CACHE: RwLock<Option<HashMap<String, CodeCacheEntry>>> = RwLock::new(None);

/// Initialize the code cache on first use.
pub fn init_code_cache() {
    let mut cache = CODE_CACHE.write().unwrap();
    if cache.is_none() {
        *cache = Some(HashMap::new());
    }
}

/// Read code from cache or disk, with automatic cache invalidation.
///
/// This function caches file contents to avoid repeated disk reads,
/// which is a significant latency optimization (can save 1-5ms per request).
pub fn read_code_cached(entrypoint: &str) -> Result<Arc<str>> {
    // Fast path: check if we can read from cache
    {
        let cache_read = CODE_CACHE.read().unwrap();
        if let Some(cache) = cache_read.as_ref() {
            if let Some(entry) = cache.get(entrypoint) {
                // Check if file has been modified since we cached it
                if let Ok(metadata) = std::fs::metadata(entrypoint) {
                    if let Ok(modified) = metadata.modified() {
                        if modified == entry.modified {
                            // Cache hit - return cached code
                            return Ok(entry.code.clone());
                        }
                    }
                }
            }
        }
    }

    // Slow path: read from disk and update cache
    let code = std::fs::read_to_string(entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", entrypoint, e))?;

    let modified = std::fs::metadata(entrypoint)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());

    let code_arc: Arc<str> = code.into();

    // Update cache
    {
        let mut cache_write = CODE_CACHE.write().unwrap();
        if cache_write.is_none() {
            *cache_write = Some(HashMap::new());
        }
        if let Some(cache) = cache_write.as_mut() {
            cache.insert(entrypoint.to_string(), CodeCacheEntry {
                code: code_arc.clone(),
                modified,
            });
        }
    }

    Ok(code_arc)
}

// Thread-local storage for isolate termination request.
// This is checked by the main thread during execution to determine
// if the timer thread has requested termination.
thread_local! {
    static TERMINATION_REQUESTED: RefCell<bool> = RefCell::new(false);
    static TERMINATION_ISOLATE_PTR: RefCell<*mut v8::Isolate> = RefCell::new(std::ptr::null_mut());
}

/// Request termination of the current V8 isolate.
///
/// Called by the timer thread when CPU timeout is reached.
fn request_isolate_termination() {
    TERMINATION_REQUESTED.with(|req| {
        *req.borrow_mut() = true;
    });
    TERMINATION_ISOLATE_PTR.with(|ptr| {
        let isolate_ptr = *ptr.borrow();
        if !isolate_ptr.is_null() {
            unsafe {
                (*isolate_ptr).terminate_execution();
            }
        }
    });
}

/// Guard that sets up CPU timeout enforcement for V8 execution.
///
/// Uses a simple wall-clock timer as an approximation of CPU time.
/// The timer thread calls request_isolate_termination() when timeout is reached.
pub struct CpuTimeoutGuard {
    /// Handle to the timer thread
    timer_thread: Option<std::thread::JoinHandle<()>>,
}

impl CpuTimeoutGuard {
    /// Create a new CPU timeout guard.
    ///
    /// # Arguments
    /// * `isolate` - The V8 isolate to terminate on timeout
    /// * `limit_ms` - Wall time limit in milliseconds (used as approximation for CPU time)
    pub fn new(isolate: &mut v8::Isolate, limit_ms: u32) -> Self {
        let isolate_ptr: *mut v8::Isolate = isolate as *mut _;
        TERMINATION_ISOLATE_PTR.with(|ptr| {
            *ptr.borrow_mut() = isolate_ptr;
        });
        TERMINATION_REQUESTED.with(|req| {
            *req.borrow_mut() = false;
        });

        let timer_thread = std::thread::spawn(move || {
            let limit_duration = std::time::Duration::from_millis(limit_ms as u64);
            std::thread::sleep(limit_duration);
            request_isolate_termination();
        });

        Self {
            timer_thread: Some(timer_thread),
        }
    }
}

impl Drop for CpuTimeoutGuard {
    fn drop(&mut self) {
        if let Some(thread) = self.timer_thread.take() {
            let _ = thread.join();
        }
        TERMINATION_ISOLATE_PTR.with(|ptr| {
            *ptr.borrow_mut() = std::ptr::null_mut();
        });
        TERMINATION_REQUESTED.with(|req| {
            *req.borrow_mut() = false;
        });
    }
}

/// Data plane executes pre-validated batches.
///
/// # Invariants (enforced by control plane)
/// - All requests pre-validated
/// - All sizes within limits
/// - Isolates pre-allocated
#[derive(Debug, Default)]
pub struct DataPlane;

impl DataPlane {
    /// Create a new data plane executor.
    pub fn new() -> Self {
        Self
    }

    /// Execute a single pre-validated request on an isolate.
    ///
    /// NO VALIDATION CHECKS in hot path - control plane validated everything.
    /// This is the hot path - optimized for throughput.
    #[inline(always)]
    pub fn execute_single(
        &self,
        context_manager: &mut ContextManager,
        task: &HandlerTask,
    ) -> Result<NanoResponse> {
        let handler_ctx = HandlerContext {
            entrypoint: task.entrypoint.clone(),
            request: task.request.clone(),
        };
        execute_with_context_manager(context_manager, &handler_ctx, task.cpu_time_limit_ms)
    }

    /// Execute a batch of requests on the same isolate.
    ///
    /// Single context reset for the entire batch, then sequential execution.
    /// This amortizes context reset overhead across multiple requests.
    #[inline(always)]
    pub fn execute_batch(
        &self,
        context_manager: &mut ContextManager,
        tasks: &[HandlerTask],
    ) -> Vec<Result<NanoResponse>> {
        if tasks.is_empty() {
            return Vec::new();
        }

        // Reset context once for the entire batch
        let _ = context_manager.reset_context();

        // Execute all requests sequentially
        let mut results = Vec::with_capacity(tasks.len());
        for task in tasks {
            let handler_ctx = HandlerContext {
                entrypoint: task.entrypoint.clone(),
                request: task.request.clone(),
            };
            let result = execute_with_context_manager(context_manager, &handler_ctx, task.cpu_time_limit_ms);
            results.push(result);
        }

        results
    }
}

/// Execute handler using the ContextManager's current context.
///
/// This function properly manages V8 scope lifecycle to avoid "active scope" errors.
/// If cpu_time_limit_ms > 0, enforces CPU time limits via timer-based termination.
pub fn execute_with_context_manager(
    context_manager: &mut ContextManager,
    handler_ctx: &HandlerContext,
    cpu_time_limit_ms: u32,
) -> Result<NanoResponse> {
    // Clone the Global<Context> (cheap - just a handle reference)
    let global_ctx = context_manager.clone_context();

    // Get VFS reference BEFORE the mutable borrow for isolate access
    let vfs_opt = context_manager.vfs().cloned();

    // Now get the isolate pointer - this borrows context_manager mutably
    let isolate = context_manager.isolate_mut().isolate();

    // Set up CPU timeout enforcement if requested
    let _timeout_guard = if cpu_time_limit_ms > 0 {
        Some(CpuTimeoutGuard::new(isolate, cpu_time_limit_ms))
    } else {
        None
    };

    // Set up VFS context for Nano.fs API
    let vfs_ref = Arc::new(vfs_opt.unwrap_or_else(|| {
        crate::vfs::IsolateVfs::new(
            crate::vfs::VfsNamespace::from_hostname("default"),
            crate::vfs::VfsBackendEnum::memory(crate::vfs::MemoryBackend::default()),
        )
    }));
    crate::runtime::vfs_bindings::set_current_vfs(Some(vfs_ref));

    // v147 API: HandleScope requires pin! + init pattern
    unsafe {
        let mut scope_storage = v8::HandleScope::new(isolate);
        let scope_pin = Pin::new_unchecked(&mut scope_storage);

        let mut handle_scope: v8::PinnedRef<'static, v8::HandleScope> =
            std::mem::transmute(scope_pin.init());

        let v8_context: v8::Local<'static, v8::Context> = match global_ctx {
            Some(g) => std::mem::transmute(v8::Local::new(&mut handle_scope, &g)),
            None => return Err(anyhow!("No context available")),
        };

        let mut context_scope: v8::ContextScope<'static, 'static, v8::HandleScope<'static, v8::Context>> =
            std::mem::transmute(v8::ContextScope::new(&mut handle_scope, v8_context));

        let result = execute_handler_code(
            std::mem::transmute(&mut context_scope),
            std::mem::transmute(v8_context),
            handler_ctx
        );

        drop(context_scope);
        drop(handle_scope);
        drop(scope_storage);

        result
    }
}

/// Execute the actual handler code within an established context scope.
fn execute_handler_code<'a>(
    scope: &mut v8::ContextScope<'a, 'a, v8::HandleScope<'a, v8::Context>>,
    v8_context: v8::Local<'a, v8::Context>,
    handler_ctx: &HandlerContext,
) -> Result<NanoResponse> {
    // Bind all WinterTC APIs (URL, fetch, etc.) to the context
    RuntimeAPIs::bind_all(scope, v8_context);
    tracing::debug!("Bound WinterTC APIs to handler context");

    // Read the handler code from cache or disk
    let code = read_code_cached(&handler_ctx.entrypoint)?;

    // Transform ES6 module syntax if this is an ESM module
    let transformed_code: String = if is_esm_module(&code) {
        transform_module_code(&code)
    } else {
        code.to_string()
    };

    // Compile and run script to define fetch function
    let code_str = v8::String::new(scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(scope, code_str, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;
    script.run(scope);

    // Get global and look for the user's handler function
    let global = v8_context.global(scope);
    let handler_key = v8::String::new(scope, "__nano_user_fetch").unwrap();
    let handler_val = match global.get(scope, handler_key.into()) {
        Some(val) if val.is_function() => {
            tracing::debug!("Found user handler function in global scope");
            val
        }
        _ => {
            let fetch_key = v8::String::new(scope, "fetch").unwrap();
            match global.get(scope, fetch_key.into()) {
                Some(val) if val.is_function() => {
                    tracing::debug!("Found handler via 'fetch' global");
                    val
                }
                _ => {
                    tracing::warn!("No handler function found");
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no handler function defined)"));
                }
            }
        }
    };

    let handler_fn = handler_val.cast::<v8::Function>();

    // Create Request object using the Request constructor
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

    let headers_init = v8::Object::new(scope);
    for (name, values) in handler_ctx.request.headers().entries() {
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
        let base64_body = base64::engine::general_purpose::STANDARD.encode(body);
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

    // Resolve the result using async support
    let resolved = match result {
        Some(response) => {
            if response.is_promise() {
                match async_support::resolve_promise_with_async(
                    scope,
                    response.cast::<v8::Promise>()
                ) {
                    Ok(value) => Some(value),
                    Err(e) => return Err(e),
                }
            } else {
                Some(response)
            }
        }
        None => None,
    };

    // Extract response
    match resolved {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Extract a NanoResponse from a V8 JavaScript object.
fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    use crate::http::NanoHeaders;

    let obj = match js_response.to_object(scope) {
        Some(o) => o,
        None => return Err(anyhow!("Response is not an object")),
    };

    let status_key = v8::String::new(scope, "status").unwrap();
    let status = match obj.get(scope, status_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => match val.to_integer(scope) {
            Some(int) => int.value() as u16,
            None => 200,
        },
        _ => 200,
    };

    let mut nano_headers = NanoHeaders::new();
    let headers_key = v8::String::new(scope, "headers").unwrap();

    if let Some(headers_val) = obj.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
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
                            if key_name.starts_with("__") || key_name == "set" || key_name == "get" || key_name == "forEach" {
                                continue;
                            }
                            if let Some(value) = headers_source.get(scope, key.into()) {
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

    let body_key = v8::String::new(scope, "body").unwrap();
    let body = match obj.get(scope, body_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => {
            match val.to_string(scope) {
                Some(s) => {
                    let body_str = s.to_rust_string_lossy(scope);
                    Some(Bytes::from(body_str))
                }
                None => None,
            }
        }
        _ => None,
    };

    Ok(NanoResponse::new(status, nano_headers, body))
}

// Lookup table for HTTP status lines (eliminates branching)
const STATUS_LINES: &[&[u8]] = &[
    b"HTTP/1.1 200 OK\r\n",
    b"HTTP/1.1 201 Created\r\n",
    b"HTTP/1.1 204 No Content\r\n",
    b"HTTP/1.1 400 Bad Request\r\n",
    b"HTTP/1.1 404 Not Found\r\n",
    b"HTTP/1.1 500 Internal Server Error\r\n",
];

/// Get HTTP status line from lookup table.
///
/// Uses direct index - no branching in hot path.
#[inline(always)]
pub fn lookup_status_line(status: u16) -> &'static [u8] {
    let idx = match status {
        200 => 0,
        201 => 1,
        204 => 2,
        400 => 3,
        404 => 4,
        500 => 5,
        _ => 0, // Default to 200 OK for unrecognized statuses
    };
    STATUS_LINES[idx]
}
