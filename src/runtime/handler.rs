//! JavaScript handler execution for WinterCG requests
//!
//! This module provides the core interface for executing JavaScript handlers
//! that receive WinterCG Request objects and return Response objects. It
//! handles the full flow: Request → V8 JavaScript object → handler execution
//! → Response extraction → NanoResponse.
//!
//! # Handler Context
//!
//! The `HandlerContext` struct holds all state needed for a single handler
//! execution: the entrypoint path and the incoming request.
//!
//! # Execution Flow
//!
//! 1. Create `HandlerContext` with entrypoint and NanoRequest
//! 2. Call `execute_handler()` with an isolate and context
//! 3. Handler serializes Request to V8 JavaScript object
//! 4. Handler invokes the JavaScript fetch() function
//! 5. Handler extracts Response from V8 return value
//! 6. Returns NanoResponse for HTTP response

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
    // Read the entrypoint file before entering V8 scopes
    let code = fs::read_to_string(&context.entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", context.entrypoint, e))?;

    // Serialize the request to JSON before entering V8
    let request_json = serialize_request_to_json(&context.request);

    // Execute in V8 context
    execute_in_v8(isolate, &code, &request_json)
}

/// Internal function to execute handler in V8
fn execute_in_v8(
    isolate: &mut crate::v8::NanoIsolate,
    code: &str,
    request_json: &str,
) -> Result<NanoResponse> {
    // Create HandleScope for the isolate
    let scope = &mut v8::HandleScope::new(isolate.isolate());

    // Create context within the scope
    let v8_context = v8::Context::new(scope, Default::default());

    // Enter the context with ContextScope
    let scope = &mut v8::ContextScope::new(scope, v8_context);

    // Bind console.log to the global object
    bind_console_log(scope, v8_context);

    // Get global object
    let global = v8_context.global(scope);

    // Compile and execute the script
    let code_string = v8::String::new(scope, code)
        .ok_or_else(|| anyhow!("Failed to create code string"))?;
    let script = v8::Script::compile(scope, code_string, None)
        .ok_or_else(|| anyhow!("Script compilation failed"))?;

    // Execute script to define the fetch function
    script.run(scope);

    // Look for the fetch function on global scope
    let fetch_key = v8::String::new(scope, "fetch").unwrap();
    let fetch_val = global.get(scope, fetch_key.into())
        .ok_or_else(|| anyhow!("No fetch function defined in entrypoint"))?;

    let fetch_fn = fetch_val.cast::<v8::Function>();

    // Create the Request object from JSON
    let js_request = create_js_request_from_json(scope, request_json)?;

    // Call the fetch handler with the Request
    let result = fetch_fn.call(scope, global.into(), &[js_request.into()]);

    // Extract and return the response
    match result {
        Some(response) => extract_js_response(scope, response),
        None => Err(anyhow!("Handler returned None")),
    }
}

/// Create a JavaScript Request object from JSON string
fn create_js_request_from_json(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    json_str: &str,
) -> Result<v8::Local<v8::Object>> {
    // Get the global object
    let global = scope.get_current_context().global(scope);

    // Get JSON object from global
    let json_key = v8::String::new(scope, "JSON").unwrap();
    let json_val = global.get(scope, json_key.into())
        .ok_or_else(|| anyhow!("JSON not found in global"))?;
    let json_obj = json_val.to_object(scope)
        .ok_or_else(|| anyhow!("JSON is not an object"))?;

    // Get JSON.parse function
    let parse_key = v8::String::new(scope, "parse").unwrap();
    let parse_fn = json_obj.get(scope, parse_key.into())
        .ok_or_else(|| anyhow!("JSON.parse not found"))?
        .cast::<v8::Function>();

    // Create the JSON string argument
    let json_arg = v8::String::new(scope, json_str)
        .ok_or_else(|| anyhow!("Failed to create JSON string"))?;

    // Call JSON.parse(json_str) to get the Request object
    let result = parse_fn.call(scope, json_obj.into(), &[json_arg.into()]);

    result.ok_or_else(|| anyhow!("Failed to parse request JSON"))?
        .to_object(scope)
        .ok_or_else(|| anyhow!("Parsed result is not an object"))
}

/// Extract a NanoResponse from a V8 JavaScript Response object
fn extract_js_response(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    js_response: v8::Local<v8::Value>,
) -> Result<NanoResponse> {
    // Verify the response is an object
    let obj = js_response.to_object(scope)
        .ok_or_else(|| anyhow!("Response is not an object"))?;

    // Extract status property (default to 200)
    let status = {
        let status_key = v8::String::new(scope, "status").unwrap();
        let status_val = obj.get(scope, status_key.into());

        match status_val {
            Some(val) if !val.is_null() && !val.is_undefined() => {
                val.to_integer(scope)
                    .map(|i| i.value() as u16)
                    .unwrap_or(200)
            }
            _ => 200,
        }
    };

    // Extract headers property
    let mut nano_headers = NanoHeaders::new();
    {
        let headers_key = v8::String::new(scope, "headers").unwrap();
        let headers_val = obj.get(scope, headers_key.into());

        if let Some(headers_val) = headers_val {
            if let Some(headers_obj) = headers_val.to_object(scope) {
                // Get all property names
                let prop_names = headers_obj.get_own_property_names(scope, Default::default());
                if let Some(names) = prop_names {
                    let len = names.length();
                    for i in 0..len {
                        let key_val = names.get_index(scope, i);
                        if let Some(key) = key_val {
                            if let Some(key_str) = key.to_string(scope) {
                                let key_name = key_str.to_rust_string_lossy(scope);
                                let value_val = headers_obj.get(scope, key.into());
                                if let Some(value) = value_val {
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
    let body: Option<Bytes> = {
        let body_key = v8::String::new(scope, "body").unwrap();
        let body_val = obj.get(scope, body_key.into());

        match body_val {
            Some(val) if !val.is_null() && !val.is_undefined() => {
                val.to_string(scope)
                    .map(|s| Bytes::from(s.to_rust_string_lossy(scope)))
            }
            _ => None,
        }
    };

    Ok(NanoResponse::new(status, nano_headers, body))
}

/// Bind console.log to the global object
fn bind_console_log(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    context: v8::Local<v8::Context>,
) {
    let global = context.global(scope);
    let console = v8::Object::new(scope);

    if let Some(log_fn) = v8::Function::new(scope, console_log_callback) {
        let log_key = v8::String::new(scope, "log").unwrap();
        console.set(scope, log_key.into(), log_fn.into());
    }

    let console_key = v8::String::new(scope, "console").unwrap();
    global.set(scope, console_key.into(), console.into());
}

/// V8 function callback for console.log
fn console_log_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let mut output = Vec::new();
    for i in 0..args.length() {
        let arg = args.get(i);
        if let Some(arg_str) = arg.to_string(scope) {
            output.push(arg_str.to_rust_string_lossy(scope));
        }
    }

    if !output.is_empty() {
        println!("{}", output.join(" "));
    }
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
    fn test_create_js_request() {
        init_platform();

        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        let request = NanoRequest::new(
            "POST".to_string(),
            url,
            headers,
            Some(Bytes::from("test body")),
        );

        let json_str = serialize_request_to_json(&request);

        let mut isolate = crate::v8::NanoIsolate::new().expect("Failed to create isolate");
        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let result = create_js_request_from_json(scope, &json_str);
        assert!(result.is_ok(), "Failed to create JS request: {:?}", result.err());

        let js_obj = result.unwrap();
        assert!(!js_obj.is_null());
        assert!(!js_obj.is_undefined());
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
