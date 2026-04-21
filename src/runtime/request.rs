//! Request object helpers for WinterCG compatibility
//!
//! This module provides Request prototype methods (text, json, arrayBuffer)
//! for reading request bodies. These methods decode base64-encoded bodies
//! from the serialized request object.

use anyhow::Result;
use base64::Engine;

/// Binds Request prototype methods (text, json, arrayBuffer) to the V8 context
pub fn bind_request_api(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    context: v8::Local<v8::Context>,
) {
    let global = context.global(scope);
    let request_key = v8::String::new(scope, "Request").unwrap();

    // If Request exists, add methods to prototype
    if let Some(request_ctor) = global.get(scope, request_key.into()) {
        if let Some(request_obj) = request_ctor.to_object(scope) {
            let prototype_key = v8::String::new(scope, "prototype").unwrap();
            if let Some(prototype) = request_obj.get(scope, prototype_key.into()) {
                if let Some(proto_obj) = prototype.to_object(scope) {
                    bind_request_method(scope, proto_obj, "text", request_text_callback);
                    bind_request_method(scope, proto_obj, "json", request_json_callback);
                    bind_request_method(scope, proto_obj, "arrayBuffer", request_arraybuffer_callback);
                }
            }
        }
    }
}

fn bind_request_method(
    scope: &mut v8::ContextScope<v8::HandleScope>,
    prototype: v8::Local<v8::Object>,
    name: &str,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    let name = v8::String::new(scope, name).unwrap();
    let func = v8::Function::new(scope, callback).unwrap();
    let _ = prototype.set(scope, name.into(), func.into());
}

/// Callback for Request.prototype.text()
/// Returns the decoded body as a string
fn request_text_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Get 'this' (the request object)
    let this = args.this();

    // Extract body from the request object (base64 encoded in body property)
    let body_key = v8::String::new(scope, "body").unwrap();
    if let Some(body_val) = this.get(scope, body_key.into()) {
        if !body_val.is_null() && !body_val.is_undefined() {
            if let Some(body_str) = body_val.to_string(scope) {
                let base64_body = body_str.to_rust_string_lossy(scope);
                // Decode base64 and return as string
                if let Ok(decoded) = base64_decode(&base64_body) {
                    let text = String::from_utf8_lossy(&decoded);
                    let result = v8::String::new(scope, &text).unwrap();
                    retval.set(result.into());
                    return;
                }
            }
        }
    }

    // Return empty string if no body
    let empty = v8::String::new(scope, "").unwrap();
    retval.set(empty.into());
}

/// Callback for Request.prototype.json()
/// Parses the body as JSON and returns the object
fn request_json_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let body_key = v8::String::new(scope, "body").unwrap();

    if let Some(body_val) = this.get(scope, body_key.into()) {
        if !body_val.is_null() && !body_val.is_undefined() {
            if let Some(body_str) = body_val.to_string(scope) {
                let base64_body = body_str.to_rust_string_lossy(scope);
                if let Ok(decoded) = base64_decode(&base64_body) {
                    let text = String::from_utf8_lossy(&decoded);

                    // Parse JSON using JSON.parse
                    let global = scope.get_current_context().global(scope);
                    let json_key = v8::String::new(scope, "JSON").unwrap();
                    if let Some(json_val) = global.get(scope, json_key.into()) {
                        if let Some(json_obj) = json_val.to_object(scope) {
                            let parse_key = v8::String::new(scope, "parse").unwrap();
                            if let Some(parse_fn) = json_obj.get(scope, parse_key.into()) {
                                if parse_fn.is_function() {
                                    let parse = parse_fn.cast::<v8::Function>();
                                    let text_str = v8::String::new(scope, &text).unwrap();
                                    if let Some(result) = parse.call(scope, json_val.into(), &[text_str.into()]) {
                                        retval.set(result);
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Return null on error
    retval.set(v8::null(scope).into());
}

/// Callback for Request.prototype.arrayBuffer()
/// Returns the decoded body as an ArrayBuffer
fn request_arraybuffer_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let body_key = v8::String::new(scope, "body").unwrap();

    if let Some(body_val) = this.get(scope, body_key.into()) {
        if !body_val.is_null() && !body_val.is_undefined() {
            if let Some(body_str) = body_val.to_string(scope) {
                let base64_body = body_str.to_rust_string_lossy(scope);
                if let Ok(decoded) = base64_decode(&base64_body) {
                    // Create ArrayBuffer from decoded bytes
                    let buffer = v8::ArrayBuffer::new(scope, decoded.len());
                    let store = buffer.get_backing_store();
                    
                    // Copy bytes into ArrayBuffer
                    for (i, byte) in decoded.iter().enumerate() {
                        if let Some(cell) = store.get(i) {
                            cell.set(*byte);
                        }
                    }
                    
                    retval.set(buffer.into());
                    return;
                }
            }
        }
    }

    // Return empty ArrayBuffer if no body
    let empty = v8::ArrayBuffer::new(scope, 0);
    retval.set(empty.into());
}

/// Decode base64 string to bytes
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|_| ())
}
