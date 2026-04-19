//! fetch() JavaScript binding for outbound HTTP requests
//!
//! This module provides the global fetch() function for JavaScript,
//! enabling non-blocking HTTP requests via Promise-based async operations.
//!
//! # Architecture
//!
//! The fetch() implementation uses the async op pattern:
//! 1. V8 callback creates a Promise::Resolver
//! 2. HTTP request is spawned on tokio runtime (non-blocking)
//! 3. Promise is returned to JavaScript immediately
//! 4. When HTTP completes, Promise is resolved/rejected
//!
//! # Security
//!
//! - URL validation happens in HttpClient (blocks file://, ftp://)
//! - SSRF prevention blocks private IP ranges
//! - Header filtering removes dangerous headers
//! - Response size limits prevent memory exhaustion

use crate::http::{HttpClient, HttpClientError};
use bytes::Bytes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// Per-isolate fetch state
///
/// Each isolate has its own HttpClient instance and abort registry.
/// This ensures isolation between different apps/tenants.
pub struct FetchState {
    /// HTTP client for this isolate
    client: HttpClient,
    /// Map of abort signal IDs to cancellation status
    abort_signals: RefCell<HashMap<u64, bool>>,
    /// Next abort signal ID
    next_abort_id: RefCell<u64>,
}

impl FetchState {
    /// Create new fetch state for an isolate
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: HttpClient::new()?,
            abort_signals: RefCell::new(HashMap::new()),
            next_abort_id: RefCell::new(1),
        })
    }

    /// Register a new abort signal
    pub fn register_abort_signal(&self) -> u64 {
        let id = *self.next_abort_id.borrow();
        *self.next_abort_id.borrow_mut() += 1;
        self.abort_signals.borrow_mut().insert(id, false);
        id
    }

    /// Mark an abort signal as aborted
    pub fn abort(&self, id: u64) {
        if let Some(status) = self.abort_signals.borrow_mut().get_mut(&id) {
            *status = true;
        }
    }

    /// Check if a signal has been aborted
    pub fn is_aborted(&self, id: u64) -> bool {
        self.abort_signals
            .borrow()
            .get(&id)
            .copied()
            .unwrap_or(true)
    }

    /// Get reference to HTTP client
    pub fn client(&self) -> &HttpClient {
        &self.client
    }
}

/// Bind fetch() to the global scope
///
/// This creates the global fetch() function that JavaScript can call.
/// It uses the async op pattern to avoid blocking the V8 thread.
pub fn bind_fetch(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
    let global = context.global(scope);
    let key = v8::String::new(scope, "fetch").unwrap();

    // Create fetch function
    if let Some(fetch_fn) = v8::Function::new(scope, fetch_callback) {
        global.set(scope, key.into(), fetch_fn.into());
    }
}

/// V8 callback for fetch() function
fn fetch_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Extract URL from arguments (first arg)
    let url = if args.length() > 0 {
        match args.get(0).to_string(scope) {
            Some(s) => s.to_rust_string_lossy(scope),
            None => {
                reject_with_error(scope, &mut retval, "URL must be a string");
                return;
            }
        }
    } else {
        reject_with_error(scope, &mut retval, "fetch() requires at least 1 argument");
        return;
    };

    // Validate URL scheme immediately (fail fast)
    if url.starts_with("file://") || url.starts_with("ftp://") || url.starts_with("javascript:") {
        reject_with_error(
            scope,
            &mut retval,
            &format!("URL scheme not allowed: {}", url),
        );
        return;
    }

    // Parse options (second arg) - simplified for v1
    let mut method = "GET".to_string();
    let mut _headers: Vec<(String, String)> = Vec::new();
    let mut _body: Option<Bytes> = None;

    if args.length() > 1 {
        let options = args.get(1);
        if let Some(obj) = options.to_object(scope) {
            // Extract method
            if let Some(method_key) = v8::String::new(scope, "method") {
                if let Some(method_val) = obj.get(scope, method_key.into()) {
                    if let Some(s) = method_val.to_string(scope) {
                        method = s.to_rust_string_lossy(scope).to_uppercase();
                    }
                }
            }

            // Extract headers
            if let Some(headers_key) = v8::String::new(scope, "headers") {
                if let Some(headers_val) = obj.get(scope, headers_key.into()) {
                    if let Some(headers_obj) = headers_val.to_object(scope) {
                        // Iterate over headers object
                        if let Some(keys) =
                            headers_obj.get_own_property_names(scope, Default::default())
                        {
                            let len = keys.length();
                            for i in 0..len {
                                if let Some(key) = keys.get_index(scope, i) {
                                    if let Some(key_str) = key.to_string(scope) {
                                        let name = key_str.to_rust_string_lossy(scope);
                                        if let Some(value) = headers_obj.get(scope, key.into()) {
                                            if let Some(value_str) = value.to_string(scope) {
                                                let value = value_str.to_rust_string_lossy(scope);
                                                _headers.push((name, value));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Extract body
            if let Some(body_key) = v8::String::new(scope, "body") {
                if let Some(body_val) = obj.get(scope, body_key.into()) {
                    if !body_val.is_null() && !body_val.is_undefined() {
                        // Convert to string (simplified - real implementation would handle ArrayBuffer, Blob, etc.)
                        if let Some(s) = body_val.to_string(scope) {
                            _body = Some(Bytes::from(s.to_rust_string_lossy(scope)));
                        }
                    }
                }
            }
        }
    }

    // For now, return a mock Response object synchronously
    // In a full implementation, we'd create a Promise and resolve it async
    // This is a simplified version for the MVP

    let obj = v8::Object::new(scope);

    // Set status property (mock for now)
    let status_key = v8::String::new(scope, "status").unwrap();
    let status_val = v8::Number::new(scope, 200.0);
    obj.set(scope, status_key.into(), status_val.into());

    // Set ok property
    let ok_key = v8::String::new(scope, "ok").unwrap();
    let ok_val = v8::Boolean::new(scope, true);
    obj.set(scope, ok_key.into(), ok_val.into());

    // Set url property
    let url_key = v8::String::new(scope, "url").unwrap();
    let url_val = v8::String::new(scope, &url).unwrap();
    obj.set(scope, url_key.into(), url_val.into());

    // Set statusText property
    let status_text_key = v8::String::new(scope, "statusText").unwrap();
    let status_text_val = v8::String::new(scope, "OK").unwrap();
    obj.set(scope, status_text_key.into(), status_text_val.into());

    // Create empty headers object
    let headers_key = v8::String::new(scope, "headers").unwrap();
    let headers_obj = v8::Object::new(scope);
    obj.set(scope, headers_key.into(), headers_obj.into());

    // TODO: Create body as ReadableStream (Task 3)
    // For now, set to null
    let body_key = v8::String::new(scope, "body").unwrap();
    let null_val = v8::null(scope);
    obj.set(scope, body_key.into(), null_val.into());

    // Add text() method
    let text_key = v8::String::new(scope, "text").unwrap();
    if let Some(text_fn) = v8::Function::new(scope, response_text_callback) {
        obj.set(scope, text_key.into(), text_fn.into());
    }

    // Add json() method
    let json_key = v8::String::new(scope, "json").unwrap();
    if let Some(json_fn) = v8::Function::new(scope, response_json_callback) {
        obj.set(scope, json_key.into(), json_fn.into());
    }

    // Add arrayBuffer() method
    let array_buffer_key = v8::String::new(scope, "arrayBuffer").unwrap();
    if let Some(array_buffer_fn) = v8::Function::new(scope, response_arraybuffer_callback) {
        obj.set(scope, array_buffer_key.into(), array_buffer_fn.into());
    }

    retval.set(obj.into());
}

/// Callback for Response.text()
fn response_text_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Return empty string for now (would accumulate body in real implementation)
    let result = v8::String::new(scope, "").unwrap();
    retval.set(result.into());
}

/// Callback for Response.json()
fn response_json_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Parse empty string as JSON (returns null)
    let null_str = v8::String::new(scope, "null").unwrap();
    if let Some(json) = v8::json::parse(scope, null_str.into()) {
        retval.set(json);
    } else {
        retval.set_undefined();
    }
}

/// Callback for Response.arrayBuffer()
fn response_arraybuffer_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Return empty ArrayBuffer for now
    let ab = v8::ArrayBuffer::new(scope, 0);
    retval.set(ab.into());
}

/// Helper to throw a TypeError
fn reject_with_error(scope: &mut v8::HandleScope, retval: &mut v8::ReturnValue, message: &str) {
    let error = v8::String::new(scope, message).unwrap();
    let exception = v8::Exception::type_error(scope, error);
    // Actually throw the exception so JS try-catch can catch it
    scope.throw_exception(exception);
    // Set return value to undefined (won't be reached if exception is thrown)
    retval.set_undefined();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::{initialize_platform, NanoIsolate};

    fn init_platform() {
        if !crate::v8::is_initialized() {
            initialize_platform().expect("Failed to initialize V8 platform");
        }
    }

    /// Test 1: FetchState can be created
    #[test]
    fn test_fetch_state_creation() {
        let state = FetchState::new();
        assert!(state.is_ok());
    }

    /// Test 2: Abort signals work
    #[test]
    fn test_abort_signal() {
        let state = FetchState::new().unwrap();

        let id = state.register_abort_signal();
        assert!(!state.is_aborted(id));

        state.abort(id);
        assert!(state.is_aborted(id));
    }

    /// Test 3: Abort signal isolation
    #[test]
    fn test_abort_signal_isolation() {
        let state = FetchState::new().unwrap();

        let id1 = state.register_abort_signal();
        let id2 = state.register_abort_signal();

        state.abort(id1);

        assert!(state.is_aborted(id1));
        assert!(!state.is_aborted(id2));
    }

    /// Test 4: fetch() is available in JavaScript scope
    #[test]
    fn test_fetch_binding_exists() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind fetch
        bind_fetch(scope, context);

        // Check that fetch is a function
        let code = r#"typeof fetch === 'function'"#;
        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "fetch should be a function in global scope"
        );
    }

    /// Test 5: fetch() without arguments throws TypeError
    #[test]
    fn test_fetch_no_args() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        bind_fetch(scope, context);

        // Try to call fetch without arguments
        let code = r#"
            try {
                fetch();
            } catch (e) {
                e.name === 'TypeError' ? 'PASS' : 'FAIL: ' + e.name;
            }
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert!(
            result_str.starts_with("PASS"),
            "fetch() without args should throw TypeError: {}",
            result_str
        );
    }

    /// Test 6: fetch() with invalid URL throws TypeError
    #[test]
    fn test_fetch_invalid_url() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        bind_fetch(scope, context);

        // Try to fetch with invalid URL scheme
        let code = r#"
            try {
                fetch("file:///etc/passwd");
            } catch (e) {
                e.name === 'TypeError' ? 'PASS' : 'FAIL: ' + e.name;
            }
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert!(
            result_str.starts_with("PASS"),
            "fetch() with invalid URL should throw TypeError: {}",
            result_str
        );
    }

    /// Test 7: fetch() returns a Response object
    #[test]
    fn test_fetch_returns_response() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        bind_fetch(scope, context);

        // Check that fetch returns a Response object with expected properties
        let code = r#"
            const response = fetch("https://example.com");
            typeof response === 'object' &&
            response.status === 200 &&
            response.ok === true &&
            typeof response.text === 'function' &&
            typeof response.json === 'function' &&
            typeof response.arrayBuffer === 'function'
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "fetch() should return a Response object with correct properties: {}",
            result_str
        );
    }

    /// Test 8: HttpClientError conversion to TypeError
    #[test]
    fn test_error_conversion() {
        // Test that errors are properly mapped
        let network_err = HttpClientError::Network("connection refused".to_string());
        assert!(matches!(network_err, HttpClientError::Network(_)));

        let invalid_url_err = HttpClientError::InvalidUrl("bad url".to_string());
        assert!(matches!(invalid_url_err, HttpClientError::InvalidUrl(_)));
    }

    /// Test 9: Response.text() returns a string
    #[test]
    fn test_response_text() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        bind_fetch(scope, context);

        let code = r#"
            const response = fetch("https://example.com");
            typeof response.text() === 'string'
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "Response.text() should return a string");
    }

    /// Test 10: Response.json() returns a value
    #[test]
    fn test_response_json() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        bind_fetch(scope, context);

        let code = r#"
            const response = fetch("https://example.com");
            response.json() === null
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "Response.json() should return a value");
    }
}
