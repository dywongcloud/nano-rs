//! JavaScript handler execution for WinterCG requests
//!
//! This module provides the core interface for executing JavaScript handlers
//! that receive WinterCG Request objects and return Response objects.

use anyhow::{anyhow, Result};
use bytes::Bytes;
use std::fs;

use crate::http::{NanoHeaders, NanoRequest, NanoResponse};
use crate::http::v8_bridge::serialize_request_to_json;

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
    // Read the entrypoint file
    let code = fs::read_to_string(&context.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", context.entrypoint, e))?;

    // Transform ES6 module syntax
    let transformed_code = transform_module_code(&code);

    // Create HandleScope for the isolate
    let scope = &mut v8::HandleScope::new(isolate.isolate());

    // Enter the provided context with ContextScope
    let scope = &mut v8::ContextScope::new(scope, v8_context);

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
    let fetch_key = v8::String::new(scope, "fetch").unwrap();
    let fetch_val = match global.get(scope, fetch_key.into()) {
        Some(val) if !val.is_undefined() && !val.is_null() => val,
        _ => {
            // Return a default response for now - handler doesn't define fetch
            return Ok(NanoResponse::ok()
                .with_header("Content-Type", "text/plain")
                .with_body("Handler executed (no fetch function defined)"));
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
        Some(val) => val,
        None => return Err(anyhow!("JSON.parse not found")),
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

/// Transform ES6 module syntax to be compatible with V8 Script execution
/// 
/// Converts `export default { fetch: ... }` to `var __nano_export = { ... };`
/// and extracts the fetch function to global scope.
fn transform_module_code(code: &str) -> String {
    // Check if this looks like ES6 module syntax with export default
    if code.contains("export default") {
        // Replace export default with var declaration
        let transformed = code.replace("export default", "var __nano_handler =");
        
        // Add code to extract fetch to global scope at the end
        format!("{}\n\n// Extract fetch from exported handler\nif (typeof __nano_handler === 'object' && __nano_handler.fetch) {{\n    var fetch = __nano_handler.fetch;\n}}", transformed)
    } else {
        // No transformation needed
        code.to_string()
    }
}

/// Internal function to execute handler in V8
fn execute_in_v8(
    isolate: &mut crate::v8::NanoIsolate,
    code: &str,
    request_json: &str,
) -> Result<NanoResponse> {
    use crate::runtime::apis::RuntimeAPIs;
    
    // Transform ES6 module syntax to V8-compatible code
    let transformed_code = transform_module_code(code);

    // Create HandleScope for the isolate
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
    let fetch_key = v8::String::new(scope, "fetch").unwrap();
    let fetch_val = match global.get(scope, fetch_key.into()) {
        Some(val) if !val.is_undefined() && !val.is_null() => val,
        _ => {
            // Return a default response for now - handler doesn't define fetch
            return Ok(NanoResponse::ok()
                .with_header("Content-Type", "text/plain")
                .with_body("Handler executed (no fetch function defined)"));
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
        Some(val) => val,
        None => return Err(anyhow!("JSON.parse not found")),
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

    // Call the fetch handler with the Request
    let result = fetch_fn.call(scope, global.into(), &[js_request]);

    // Extract the response
    match result {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Resolve a Promise and extract the result
fn resolve_promise<'s>(
    scope: &mut v8::ContextScope<'s, v8::HandleScope>,
    promise: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    // Check if it's a Promise
    if !promise.is_promise() {
        return Some(promise);
    }

    let promise = promise.cast::<v8::Promise>();
    
    // Check promise state
    match promise.state() {
        v8::PromiseState::Fulfilled => {
            // Promise is already resolved, get the result
            Some(promise.result(scope))
        }
        v8::PromiseState::Rejected => {
            // Promise was rejected
            let result = promise.result(scope);
            eprintln!("DEBUG: Promise rejected with: {:?}", result.to_string(scope).map(|s| s.to_rust_string_lossy(scope)));
            None
        }
        v8::PromiseState::Pending => {
            // Promise is still pending - this shouldn't happen for synchronous handlers
            // but async handlers would need special handling
            eprintln!("DEBUG: Promise is still pending");
            // For now, return None - in production we'd need to properly await
            None
        }
    }
}

/// Extract a NanoResponse from a V8 JavaScript Response object
fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    // First, resolve the Promise if needed
    let js_response = match resolve_promise(scope, js_response) {
        Some(response) => response,
        None => return Err(anyhow!("Failed to resolve Promise")),
    };
    
    // Verify the response is an object
    let obj = match js_response.to_object(scope) {
        Some(o) => o,
        None => return Err(anyhow!("Response is not an object")),
    };

    // Extract status property (default to 200)
    let status_key = v8::String::new(scope, "status").unwrap();
    let status = match obj.get(scope, status_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => {
            match val.to_integer(scope) {
                Some(int) => int.value() as u16,
                None => 200,
            }
        }
        _ => 200,
    };

    // Extract headers property
    let mut nano_headers = NanoHeaders::new();
    let headers_key = v8::String::new(scope, "headers").unwrap();

    if let Some(headers_val) = obj.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
            // Get all property names
            if let Some(names) = headers_obj.get_own_property_names(scope, Default::default()) {
                let len = names.length();
                for i in 0..len {
                    if let Some(key) = names.get_index(scope, i) {
                        if let Some(key_str) = key.to_string(scope) {
                            let key_name = key_str.to_rust_string_lossy(scope);
                            if let Some(value) = headers_obj.get(scope, key.into()) {
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

    // Extract body property
    let body_key = v8::String::new(scope, "body").unwrap();
    let body = match obj.get(scope, body_key.into()) {
        Some(val) if !val.is_null() && !val.is_undefined() => {
            match val.to_string(scope) {
                Some(s) => Some(Bytes::from(s.to_rust_string_lossy(scope))),
                None => None,
            }
        }
        _ => None,
    };

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
