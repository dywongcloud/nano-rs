//! JavaScript handler execution for WinterCG requests
//!
//! This module provides the core interface for executing JavaScript handlers
//! that receive WinterCG Request objects and return Response objects.

use anyhow::{anyhow, Result};
use bytes::Bytes;
use std::fs;

use crate::http::{NanoHeaders, NanoRequest, NanoResponse};
use crate::http::v8_bridge::serialize_request_to_json;
use crate::runtime::async_support;

/// Context for executing a JavaScript handler
#[derive(Debug, Clone)]
pub struct HandlerContext {
    /// Path to the JavaScript entrypoint file
    pub entrypoint: String,
    /// The incoming HTTP request
    pub request: NanoRequest,
}

/// Execute a JavaScript handler in a V8 isolate
pub async fn execute_handler(
    isolate: &mut crate::v8::NanoIsolate,
    context: HandlerContext,
) -> Result<NanoResponse> {
    // Read the entrypoint file
    let code = fs::read_to_string(&context.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", context.entrypoint, e))?;

    // Serialize the request to JSON
    let request_json = serialize_request_to_json(&context.request);

    // Execute in V8 context
    execute_in_v8(isolate, &code, &request_json)
}

/// Execute a JavaScript handler with an explicit V8 context
///
/// This variant is used by WorkerPool to execute handlers with a pre-existing
/// V8 context (for context reset optimization).
///
/// # Arguments
///
/// * `isolate` - The V8 isolate
/// * `v8_context` - The V8 context to execute in
/// * `context` - The handler context with entrypoint and request
///
/// # Returns
///
/// Result containing NanoResponse or an error
pub fn execute_handler_with_context(
    isolate: &mut crate::v8::NanoIsolate,
    v8_context: v8::Local<v8::Context>,
    context: HandlerContext,
) -> Result<NanoResponse> {
    use crate::runtime::vfs_bindings;
    use crate::v8::module::{is_esm_module, transform_module_code};
    
    // Read the entrypoint file
    let code = fs::read_to_string(&context.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", context.entrypoint, e))?;

    // Check if this is an ESM module before consuming code
    let is_esm = is_esm_module(&code);
    
    // Transform ES6 module syntax only if this is an ESM module
    let transformed_code = if is_esm {
        transform_module_code(&code)
    } else {
        code
    };

    // Set up VFS context for Nano.fs API (must be before HandleScope borrows isolate)
    let vfs_ref = std::sync::Arc::new(isolate.vfs().clone());
    vfs_bindings::set_current_vfs(Some(vfs_ref));

    // Create HandleScope for the isolate (v147 API: pin! + init pattern)
    let scope = std::pin::pin!(v8::HandleScope::new(isolate.isolate()));
    let mut scope = scope.init();

    // Enter the provided context with ContextScope
    // v147 API: ContextScope::new takes &mut PinnedRef<HandleScope>
    let mut scope = v8::ContextScope::new(&mut scope, v8_context);

    // Get global object
    // v147 API: Use &**scope to dereference ContextScope to PinnedRef
    let global = v8_context.global(&**scope);

    // Compile and execute the script
    // v147 API: All V8 APIs expect &PinnedRef, use &**scope
    let code_string = v8::String::new(&**scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(&**scope, code_string, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;

    // Execute script to define the fetch function
    script.run(&**scope);

    // Look for the fetch function on global scope
    // For ESM modules, check __nano_user_fetch first (set by transform_module_code)
    let fetch_val = if is_esm {
        let fetch_key = v8::String::new(&**scope, "__nano_user_fetch").unwrap();
        global.get(&**scope, fetch_key.into())
            .filter(|val| !val.is_undefined() && !val.is_null())
    } else {
        None
    };

    let fetch_val = match fetch_val {
        Some(val) => val,
        None => {
            // Fall back to checking for global fetch function
            let fetch_key = v8::String::new(&**scope, "fetch").unwrap();
            match global.get(&**scope, fetch_key.into()) {
                Some(val) if !val.is_undefined() && !val.is_null() => val,
                _ => {
                    // Return a default response for now - handler doesn't define fetch
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no fetch function defined)"));
                }
            }
        }
    };

    // Verify it's actually a function
    if !fetch_val.is_function() {
        return Ok(NanoResponse::ok()
            .with_header("Content-Type", "text/plain")
            .with_body("Handler executed (fetch is not a function)"));
    }

    let fetch_fn = fetch_val.cast::<v8::Function>();

    // Serialize request to JSON and parse in V8
    let request_json = serialize_request_to_json(&context.request);

    // Get JSON.parse function
    let json_key = v8::String::new(scope, "JSON").unwrap();
    let json_val = match global.get(scope, json_key.into()) {
        Some(val) => val,
        None => return Err(anyhow!("JSON not found in global")),
    };

    let json_obj = match json_val.to_object(scope) {
        Some(obj) => obj,
        None => return Err(anyhow!("JSON is not an object")),
    };

    let parse_key = v8::String::new(scope, "parse").unwrap();
    let parse_fn_val = match json_obj.get(scope, parse_key.into()) {
        Some(val) if val.is_function() => val,
        _ => return Err(anyhow!("JSON.parse not found or not a function")),
    };

    let parse_fn = parse_fn_val.cast::<v8::Function>();

    // Create the JSON string and parse it
    let json_str = match v8::String::new(scope, &request_json) {
        Some(s) => s,
        None => return Err(anyhow!("Failed to create JSON string")),
    };

    let js_request = match parse_fn.call(scope, json_val.into(), &[json_str.into()]) {
        Some(req) => req,
        None => return Err(anyhow!("Failed to parse request JSON")),
    };

    // Call the fetch handler with the Request
    let result = fetch_fn.call(scope, global.into(), &[js_request]);

    // Extract the response
    match result {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Internal function to execute handler in V8
fn execute_in_v8(
    isolate: &mut crate::v8::NanoIsolate,
    code: &str,
    request_json: &str,
) -> Result<NanoResponse> {
    use crate::runtime::apis::RuntimeAPIs;
    use crate::runtime::vfs_bindings;
    use crate::v8::module::{is_esm_module, transform_module_code};
    
    // Check if this is an ESM module first
    let is_esm = is_esm_module(code);
    
    // Transform ES6 module syntax to V8-compatible code if needed
    let transformed_code = if is_esm {
        transform_module_code(code)
    } else {
        code.to_string()
    };

    // Create HandleScope for the isolate
    // Set up VFS context for Nano.fs API (must be before HandleScope borrows isolate)
    let vfs_ref = std::sync::Arc::new(isolate.vfs().clone());
    vfs_bindings::set_current_vfs(Some(vfs_ref));
    
    let scope = &mut v8::HandleScope::new(isolate.isolate());

    // Create context within the scope
    let v8_context = v8::Context::new(scope, Default::default());

    // Enter the context with ContextScope
    let scope = &mut v8::ContextScope::new(scope, v8_context);
    
    // Bind runtime APIs (Response, console, crypto, etc.)
    RuntimeAPIs::bind_all(scope, v8_context);

    // Get global object
    let global = v8_context.global(scope);

    // Compile and execute the script
    let code_string = v8::String::new(scope, &transformed_code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(scope, code_string, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;

    // Execute script to define the fetch function
    script.run(scope);

    // Look for the fetch function on global scope
    // For ESM modules, check __nano_user_fetch first (set by transform_module_code)
    let fetch_val = if is_esm {
        let fetch_key = v8::String::new(scope, "__nano_user_fetch").unwrap();
        global.get(scope, fetch_key.into())
            .filter(|val| !val.is_undefined() && !val.is_null())
    } else {
        None
    };
    
    let fetch_val = match fetch_val {
        Some(val) => val,
        None => {
            // Fall back to checking for global fetch function
            let fetch_key = v8::String::new(scope, "fetch").unwrap();
            match global.get(scope, fetch_key.into()) {
                Some(val) if !val.is_undefined() && !val.is_null() => val,
                _ => {
                    // Return a default response for now - handler doesn't define fetch
                    return Ok(NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Handler executed (no fetch function defined)"));
                }
            }
        }
    };

    // Verify it's actually a function
    if !fetch_val.is_function() {
        return Ok(NanoResponse::ok()
            .with_header("Content-Type", "text/plain")
            .with_body("Handler executed (fetch is not a function)"));
    }

    let fetch_fn = fetch_val.cast::<v8::Function>();

    // Get JSON.parse function to create the request object
    let json_key = v8::String::new(scope, "JSON").unwrap();
    let json_val = match global.get(scope, json_key.into()) {
        Some(val) => val,
        None => return Err(anyhow!("JSON not found in global")),
    };

    let json_obj = match json_val.to_object(scope) {
        Some(obj) => obj,
        None => return Err(anyhow!("JSON is not an object")),
    };

    let parse_key = v8::String::new(scope, "parse").unwrap();
    let parse_fn_val = match json_obj.get(scope, parse_key.into()) {
        Some(val) if val.is_function() => val,
        _ => return Err(anyhow!("JSON.parse not found or not a function")),
    };

    let parse_fn = parse_fn_val.cast::<v8::Function>();

    // Create the JSON string and parse it
    let json_str = match v8::String::new(scope, request_json) {
        Some(s) => s,
        None => return Err(anyhow!("Failed to create JSON string")),
    };

    let js_request = match parse_fn.call(scope, json_val.into(), &[json_str.into()]) {
        Some(req) => req,
        None => return Err(anyhow!("Failed to parse request JSON")),
    };

    // Convert plain headers object to Headers instance
    // Get the Headers constructor
    let headers_key = v8::String::new(scope, "Headers").unwrap();
    if let Some(headers_ctor) = global.get(scope, headers_key.into()) {
        if headers_ctor.is_function() {
            let headers_ctor_fn = headers_ctor.cast::<v8::Function>();
            
            // Get the headers from the request
            let req_headers_key = v8::String::new(scope, "headers").unwrap();
            if let Some(req_headers) = js_request.to_object(scope).and_then(|o| o.get(scope, req_headers_key.into())) {
                if !req_headers.is_null() && !req_headers.is_undefined() {
                    // Create new Headers(headers)
                    if let Some(new_headers) = headers_ctor_fn.call(scope, headers_ctor.into(), &[req_headers]) {
                        if let Some(req_obj) = js_request.to_object(scope) {
                            let _ = req_obj.set(scope, req_headers_key.into(), new_headers);
                        }
                    }
                }
            }
        }
    }

    // Call the fetch handler with the Request
    let result = fetch_fn.call(scope, global.into(), &[js_request]);

    // Extract the response (may be a Promise, so resolve it)
    match result {
        Some(response) => {
            // Check if response is a Promise and resolve if needed
            // Resolve using async event loop for Promises
            let resolved = if response.is_promise() {
                let promise = response.cast::<v8::Promise>();
                match async_support::resolve_promise_with_async(scope, promise) {
                    Ok(value) => Some(value),
                    Err(e) => return Err(e),
                }
            } else {
                Some(response)
            };
            
            match resolved {
                Some(response) => extract_js_response(scope, response),
                None => Err(anyhow!("Handler returned None")),
            }
        }
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Extract a NanoResponse from a V8 JavaScript Response object
pub fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    // Verify the response is an object
    let obj = match js_response.to_object(scope) {
        Some(o) => o,
        None => return Err(anyhow!("Response is not an object")),
    };

    // Extract status property (default to 200)
    let status_key = v8::String::new(scope, "status").unwrap();
    let status_val_opt = obj.get(scope, status_key.into());
    let status = match status_val_opt {
        Some(val) if !val.is_null() && !val.is_undefined() => {
            tracing::debug!("Status value found: is_number={}, is_int32={}, to_integer={:?}",
                val.is_number(), val.is_int32(), val.to_integer(scope).map(|i| i.value()));
            match val.to_integer(scope) {
                Some(int) => {
                    let s = int.value() as u16;
                    tracing::debug!("Status extracted from integer: {}", s);
                    s
                }
                None => {
                    // Try to_number as fallback
                    match val.to_number(scope) {
                        Some(num) => {
                            let s = num.value() as u16;
                            tracing::debug!("Status extracted from number: {}", s);
                            s
                        }
                        None => {
                            tracing::warn!("Failed to convert status to number, defaulting to 200");
                            200
                        }
                    }
                }
            }
        }
        Some(val) if val.is_null() => {
            tracing::debug!("Status value is null, defaulting to 200");
            200
        }
        Some(val) if val.is_undefined() => {
            tracing::debug!("Status value is undefined, defaulting to 200");
            200
        }
        _ => {
            tracing::debug!("Status property not found, defaulting to 200");
            200
        }
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

            // Get all property names
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
            tracing::debug!("Response body value: type check - is_string={}, is_object={}, is_array={}",
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
            tracing::debug!("Response body property not found or not accessible");
            None
        }
    };

    tracing::debug!("Final NanoResponse: status={}, has_body={}, body_len={}",
        status, body.is_some(), body.as_ref().map(|b| b.len()).unwrap_or(0));
    
    Ok(NanoResponse::new(status, nano_headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoUrl, NanoHeaders};
    use crate::v8::platform;

    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    #[test]
    fn test_handler_context_creation() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            NanoHeaders::new(),
            None,
        );

        let context = HandlerContext {
            entrypoint: "/app/index.js".to_string(),
            request,
        };

        assert_eq!(context.entrypoint, "/app/index.js");
        assert_eq!(context.request.method(), "GET");
    }

    #[test]
    fn test_extract_js_response_basic() {
        init_platform();

        let mut isolate = crate::v8::NanoIsolate::new().expect("Failed to create isolate");
        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Create a simple response object in JavaScript
        let code = r#"({ status: 200, headers: { "Content-Type": "text/plain" }, body: "Hello" })"#;
        let code_str = v8::String::new(scope, code).unwrap();
        let script = v8::Script::compile(scope, code_str, None).unwrap();
        let result = script.run(scope).expect("Script execution failed");

        let response = extract_js_response(scope, result);
        assert!(response.is_ok(), "Failed to extract response: {:?}", response.err());

        let nano_response = response.unwrap();
        assert_eq!(nano_response.status(), 200);
        assert_eq!(nano_response.headers().get("Content-Type"), Some("text/plain".to_string()));
    }

    #[test]
    fn test_extract_js_response_no_body() {
        init_platform();

        let mut isolate = crate::v8::NanoIsolate::new().expect("Failed to create isolate");
        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Create a response without body
        let code = r#"({ status: 204, headers: {} })"#;
        let code_str = v8::String::new(scope, code).unwrap();
        let script = v8::Script::compile(scope, code_str, None).unwrap();
        let result = script.run(scope).expect("Script execution failed");

        let response = extract_js_response(scope, result);
        assert!(response.is_ok());

        let nano_response = response.unwrap();
        assert_eq!(nano_response.status(), 204);
        assert!(nano_response.body().is_none());
    }

    #[test]
    fn test_extract_js_response_default_status() {
        init_platform();

        let mut isolate = crate::v8::NanoIsolate::new().expect("Failed to create isolate");
        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Create a response without explicit status (should default to 200)
        let code = r#"({ headers: {}, body: "test" })"#;
        let code_str = v8::String::new(scope, code).unwrap();
        let script = v8::Script::compile(scope, code_str, None).unwrap();
        let result = script.run(scope).expect("Script execution failed");

        let response = extract_js_response(scope, result);
        assert!(response.is_ok());

        let nano_response = response.unwrap();
        assert_eq!(nano_response.status(), 200);
    }
}
