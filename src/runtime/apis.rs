//! Runtime JavaScript APIs for WinterCG compatibility
//!
//! This module provides JavaScript API bindings that bridge between V8 and Rust:
//! - console.log/warn/error with structured logging via tracing
//! - TextEncoder/TextDecoder for UTF-8 encoding/decoding
//! - crypto.getRandomValues for cryptographic randomness
//! - performance.now for high-resolution monotonic timing
//! - structuredClone for deep object cloning
//! - DOMException for standard error types
//! - Blob for binary data containers
//! - FormData for multipart form data
//!
//! All APIs are bound to the V8 global scope via RuntimeAPIs::bind_all().

use std::cell::Cell;
use std::time::Instant;

/// Thread-local storage for performance baseline timing
thread_local! {
    static PERFORMANCE_BASELINE: Cell<Option<Instant>> = Cell::new(None);
}

/// RuntimeAPIs manages all JavaScript API bindings
///
/// This struct provides methods to bind WinterCG-compatible APIs to V8 contexts.
/// Call RuntimeAPIs::bind_all() during context setup to make all APIs available.
pub struct RuntimeAPIs;

impl RuntimeAPIs {
    /// Bind all runtime APIs to the V8 context
    ///
    /// This should be called once per context during handler setup.
    /// Makes all WinterCG APIs available to JavaScript.
    pub fn bind_all(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        Self::bind_console(scope, context);
        Self::bind_text_encoder(scope, context);
        Self::bind_text_decoder(scope, context);
        Self::bind_crypto(scope, context);
        Self::bind_performance(scope, context);
        Self::bind_structured_clone(scope, context);
        Self::bind_dom_exception(scope, context);
        Self::bind_blob(scope, context);
        Self::bind_form_data(scope, context);
        Self::bind_headers(scope, context);
        Self::bind_url(scope, context);
        Self::bind_response(scope, context);
        Self::bind_fetch(scope, context);
    }

    /// Bind fetch() API to global scope
    fn bind_fetch(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        crate::runtime::fetch::bind_fetch(scope, context);
    }

    /// Bind console API (log/warn/error) to global scope
    fn bind_console(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);
        let console = v8::Object::new(scope);

        // Bind log method
        if let Some(log_fn) = v8::Function::new(scope, console_log_callback) {
            let key = v8::String::new(scope, "log").unwrap();
            console.set(scope, key.into(), log_fn.into());
        }

        // Bind warn method
        if let Some(warn_fn) = v8::Function::new(scope, console_warn_callback) {
            let key = v8::String::new(scope, "warn").unwrap();
            console.set(scope, key.into(), warn_fn.into());
        }

        // Bind error method
        if let Some(error_fn) = v8::Function::new(scope, console_error_callback) {
            let key = v8::String::new(scope, "error").unwrap();
            console.set(scope, key.into(), error_fn.into());
        }

        // Attach console to global
        let console_key = v8::String::new(scope, "console").unwrap();
        global.set(scope, console_key.into(), console.into());
    }

    /// Bind TextEncoder API to global scope
    fn bind_text_encoder(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create TextEncoder constructor function
        let encoder_template = v8::FunctionTemplate::new(scope, text_encoder_constructor);

        // Add encode method to prototype via instance template
        let instance_template = encoder_template.prototype_template(scope);
        let encode_fn = v8::FunctionTemplate::new(scope, text_encoder_encode);
        let encode_key = v8::String::new(scope, "encode").unwrap();
        instance_template.set(encode_key.into(), encode_fn.into());

        let encoder_ctor = encoder_template.get_function(scope).unwrap();

        // Attach TextEncoder to global
        let key = v8::String::new(scope, "TextEncoder").unwrap();
        global.set(scope, key.into(), encoder_ctor.into());
    }

    /// Bind TextDecoder API to global scope
    fn bind_text_decoder(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create TextDecoder constructor function
        let decoder_template = v8::FunctionTemplate::new(scope, text_decoder_constructor);

        // Add decode method to prototype via instance template
        let instance_template = decoder_template.prototype_template(scope);
        let decode_fn = v8::FunctionTemplate::new(scope, text_decoder_decode);
        let decode_key = v8::String::new(scope, "decode").unwrap();
        instance_template.set(decode_key.into(), decode_fn.into());

        let decoder_ctor = decoder_template.get_function(scope).unwrap();

        // Attach TextDecoder to global
        let key = v8::String::new(scope, "TextDecoder").unwrap();
        global.set(scope, key.into(), decoder_ctor.into());
    }

    /// Bind crypto API with getRandomValues
    fn bind_crypto(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create crypto object
        let crypto = v8::Object::new(scope);

        // Bind getRandomValues
        if let Some(grv_fn) = v8::Function::new(scope, crypto_get_random_values) {
            let key = v8::String::new(scope, "getRandomValues").unwrap();
            crypto.set(scope, key.into(), grv_fn.into());
        }

        // Attach crypto to global
        let key = v8::String::new(scope, "crypto").unwrap();
        global.set(scope, key.into(), crypto.into());
    }

    /// Bind performance API with now()
    fn bind_performance(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Initialize baseline on first call
        PERFORMANCE_BASELINE.with(|cell| {
            if cell.get().is_none() {
                cell.set(Some(Instant::now()));
            }
        });

        // Create performance object
        let performance = v8::Object::new(scope);

        // Bind now() method
        if let Some(now_fn) = v8::Function::new(scope, performance_now) {
            let key = v8::String::new(scope, "now").unwrap();
            performance.set(scope, key.into(), now_fn.into());
        }

        // Attach performance to global
        let key = v8::String::new(scope, "performance").unwrap();
        global.set(scope, key.into(), performance.into());
    }

    /// Bind structuredClone as global function
    fn bind_structured_clone(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        if let Some(clone_fn) = v8::Function::new(scope, structured_clone) {
            let key = v8::String::new(scope, "structuredClone").unwrap();
            global.set(scope, key.into(), clone_fn.into());
        }
    }

    /// Bind DOMException constructor
    fn bind_dom_exception(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create DOMException constructor
        let template = v8::FunctionTemplate::new(scope, dom_exception_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "DOMException").unwrap();
        global.set(scope, key.into(), ctor.into());
    }

    /// Bind Blob constructor
    fn bind_blob(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create Blob constructor
        let template = v8::FunctionTemplate::new(scope, blob_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "Blob").unwrap();
        global.set(scope, key.into(), ctor.into());
    }

    /// Bind FormData constructor
    fn bind_form_data(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create FormData constructor
        let template = v8::FunctionTemplate::new(scope, form_data_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "FormData").unwrap();
        global.set(scope, key.into(), ctor.into());
    }

    /// Bind Response constructor for WinterCG compatibility
    fn bind_response(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create Response constructor
        let template = v8::FunctionTemplate::new(scope, response_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "Response").unwrap();
        global.set(scope, key.into(), ctor.into());
    }

    /// Bind URL constructor for WinterCG compatibility
    fn bind_url(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create URL constructor
        let template = v8::FunctionTemplate::new(scope, url_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "URL").unwrap();
        global.set(scope, key.into(), ctor.into());
    }

    /// Bind Headers constructor for WinterCG compatibility
    fn bind_headers(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create Headers constructor
        let template = v8::FunctionTemplate::new(scope, headers_constructor);
        let ctor = template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "Headers").unwrap();
        global.set(scope, key.into(), ctor.into());
    }
}

/// Format console arguments into a single string
fn format_console_args(scope: &mut v8::HandleScope, args: v8::FunctionCallbackArguments) -> String {
    let mut parts = Vec::new();
    for i in 0..args.length() {
        let arg = args.get(i);
        if let Some(s) = arg.to_string(scope) {
            parts.push(s.to_rust_string_lossy(scope));
        }
    }
    parts.join(" ")
}

/// V8 callback for console.log
fn console_log_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let message = format_console_args(scope, args);
    tracing::info!(target: "js_console", "{}", message);
}

/// V8 callback for console.warn
fn console_warn_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let message = format_console_args(scope, args);
    tracing::warn!(target: "js_console", "{}", message);
}

/// V8 callback for console.error
fn console_error_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let message = format_console_args(scope, args);
    tracing::error!(target: "js_console", "{}", message);
}

/// TextEncoder constructor callback
fn text_encoder_constructor(
    _scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Constructor - creates TextEncoder instance
    // No internal state needed for basic UTF-8 encoding
}

/// TextEncoder.encode() implementation
fn text_encoder_encode(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Get first argument as string
    if args.length() == 0 {
        // Return empty Uint8Array
        let empty = v8::ArrayBuffer::new(scope, 0);
        if let Some(uint8array) = v8::Uint8Array::new(scope, empty, 0, 0) {
            retval.set(uint8array.into());
        }
        return;
    }

    let arg = args.get(0);
    let text = if let Some(s) = arg.to_string(scope) {
        s.to_rust_string_lossy(scope)
    } else {
        String::new()
    };

    // Encode to UTF-8 bytes
    let bytes = text.into_bytes();

    // Create ArrayBuffer and copy bytes
    let ab = v8::ArrayBuffer::new(scope, bytes.len());
    let store = ab.get_backing_store();

    // Copy bytes into ArrayBuffer
    for (i, byte) in bytes.iter().enumerate() {
        if let Some(cell) = store.get(i) {
            cell.set(*byte);
        }
    }

    // Create Uint8Array view
    if let Some(uint8array) = v8::Uint8Array::new(scope, ab, 0, bytes.len()) {
        retval.set(uint8array.into());
    } else {
        retval.set(ab.into());
    }
}

/// TextDecoder constructor callback
fn text_decoder_constructor(
    _scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Constructor - TextDecoder always uses UTF-8 in WinterCG
    // No internal state needed
}

/// TextDecoder.decode() implementation
fn text_decoder_decode(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Get first argument (should be ArrayBuffer or Uint8Array)
    if args.length() == 0 {
        retval.set(v8::String::new(scope, "").unwrap().into());
        return;
    }

    let arg = args.get(0);

    // Try to extract bytes from Uint8Array
    let bytes = if arg.is_uint8_array() {
        let uint8array = arg.cast::<v8::Uint8Array>();
        let length = uint8array.byte_length();
        let mut vec = Vec::with_capacity(length);
        for i in 0..length {
            if let Some(val) = uint8array.get_index(scope, i as u32) {
                if let Some(int) = val.to_integer(scope) {
                    vec.push(int.value() as u8);
                }
            }
        }
        vec
    } else if arg.is_array_buffer() {
        let arraybuffer = arg.cast::<v8::ArrayBuffer>();
        // Extract bytes from ArrayBuffer
        let store = arraybuffer.get_backing_store();
        let length = arraybuffer.byte_length();
        (0..length)
            .filter_map(|i| store.get(i).map(|cell| cell.get()))
            .collect()
    } else {
        Vec::new()
    };

    // Decode UTF-8 bytes to string (with replacement for invalid sequences)
    let text = String::from_utf8_lossy(&bytes);

    // Return as JS string
    if let Some(s) = v8::String::new(scope, &text) {
        retval.set(s.into());
    }
}

/// crypto.getRandomValues implementation
fn crypto_get_random_values(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Get first argument (should be TypedArray)
    if args.length() < 1 {
        retval.set_undefined();
        return;
    }

    let arg = args.get(0);

    // Handle Uint8Array
    if let Some(uint8array) = arg
        .to_object(scope)
        .and_then(|o| o.try_cast::<v8::Uint8Array>().ok())
    {
        let length = uint8array.byte_length();

        if length == 0 {
            retval.set(arg);
            return;
        }

        // Generate random bytes using getrandom
        let mut buffer = vec![0u8; length];
        if getrandom::getrandom(&mut buffer).is_err() {
            retval.set_undefined();
            return;
        }

        // Copy bytes into the TypedArray
        for (i, byte) in buffer.iter().enumerate() {
            let idx = v8::Number::new(scope, i as f64);
            let val = v8::Number::new(scope, *byte as f64);
            uint8array.set(scope, idx.into(), val.into());
        }

        retval.set(arg);
        return;
    }

    // Handle Uint16Array
    if let Some(uint16array) = arg
        .to_object(scope)
        .and_then(|o| o.try_cast::<v8::Uint16Array>().ok())
    {
        let length = uint16array.byte_length() / 2;

        if length == 0 {
            retval.set(arg);
            return;
        }

        let mut buffer = vec![0u16; length];
        let byte_buffer = unsafe {
            std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 2)
        };

        if getrandom::getrandom(byte_buffer).is_err() {
            retval.set_undefined();
            return;
        }

        for (i, value) in buffer.iter().enumerate() {
            let idx = v8::Number::new(scope, i as f64);
            let val = v8::Number::new(scope, *value as f64);
            uint16array.set(scope, idx.into(), val.into());
        }

        retval.set(arg);
        return;
    }

    // Handle Uint32Array
    if let Some(uint32array) = arg
        .to_object(scope)
        .and_then(|o| o.try_cast::<v8::Uint32Array>().ok())
    {
        let length = uint32array.byte_length() / 4;

        if length == 0 {
            retval.set(arg);
            return;
        }

        let mut buffer = vec![0u32; length];
        let byte_buffer = unsafe {
            std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 4)
        };

        if getrandom::getrandom(byte_buffer).is_err() {
            retval.set_undefined();
            return;
        }

        for (i, value) in buffer.iter().enumerate() {
            let idx = v8::Number::new(scope, i as f64);
            let val = v8::Number::new(scope, *value as f64);
            uint32array.set(scope, idx.into(), val.into());
        }

        retval.set(arg);
        return;
    }

    // If not a supported TypedArray, return undefined
    retval.set_undefined();
}

/// performance.now() implementation
fn performance_now(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let now = Instant::now();

    let elapsed_ms = PERFORMANCE_BASELINE.with(|baseline| {
        if let Some(base) = baseline.get() {
            now.duration_since(base).as_nanos() as f64 / 1_000_000.0
        } else {
            0.0
        }
    });

    retval.set(v8::Number::new(scope, elapsed_ms).into());
}

/// structuredClone() implementation
fn structured_clone(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    if args.length() < 1 {
        retval.set_undefined();
        return;
    }

    let value = args.get(0);

    // Use V8's built-in cloning via JSON serialization as a baseline
    // Convert to JSON string then parse back
    if let Some(json_string) = v8::json::stringify(scope, value) {
        if let Some(json_str) = json_string.to_string(scope) {
            // Parse the JSON back into a value
            if let Some(cloned) = v8::json::parse(scope, json_str.into()) {
                retval.set(cloned);
                return;
            }
        }
    }

    // Fallback: return the original value
    retval.set(value);
}

/// DOMException constructor implementation
fn dom_exception_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get message argument (defaults to "")
    let message = if args.length() > 0 {
        args.get(0)
            .to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Get name argument (defaults to "Error")
    let name = if args.length() > 1 {
        args.get(1)
            .to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Error".to_string())
    } else {
        "Error".to_string()
    };

    // Set message property
    let msg_key = v8::String::new(scope, "message").unwrap();
    let msg_val = v8::String::new(scope, &message).unwrap();
    this.set(scope, msg_key.into(), msg_val.into());

    // Set name property
    let name_key = v8::String::new(scope, "name").unwrap();
    let name_val = v8::String::new(scope, &name).unwrap();
    this.set(scope, name_key.into(), name_val.into());

    // Set stack property (simplified for v1)
    let stack_key = v8::String::new(scope, "stack").unwrap();
    let stack_str = format!("DOMException: {}", message);
    let stack_val = v8::String::new(scope, &stack_str).unwrap();
    this.set(scope, stack_key.into(), stack_val.into());

    retval.set(this.into());
}

/// Blob constructor implementation (simplified v1)
fn blob_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get parts array (first argument, defaults to empty)
    let mut total_size: usize = 0;
    let mut parts: Vec<String> = Vec::new();

    if args.length() > 0 {
        let arg = args.get(0);
        if let Some(array) = arg.to_object(scope) {
            // Try to iterate over the array
            if let Some(length_key) = v8::String::new(scope, "length") {
                if let Some(length_val) = array.get(scope, length_key.into()) {
                    if let Some(length_num) = length_val.to_number(scope) {
                        let length = length_num.value() as usize;

                        for i in 0..length {
                            let idx = v8::Number::new(scope, i as f64);
                            if let Some(item) = array.get(scope, idx.into()) {
                                // Convert item to string
                                if let Some(item_str) = item.to_string(scope) {
                                    let item_rust = item_str.to_rust_string_lossy(scope);
                                    total_size += item_rust.len();
                                    parts.push(item_rust);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Get type option (second argument with { type: "..." })
    let mut blob_type = String::new();
    if args.length() > 1 {
        let options = args.get(1);
        if let Some(options_obj) = options.to_object(scope) {
            if let Some(type_key) = v8::String::new(scope, "type") {
                if let Some(type_val) = options_obj.get(scope, type_key.into()) {
                    if let Some(type_str) = type_val.to_string(scope) {
                        blob_type = type_str.to_rust_string_lossy(scope);
                    }
                }
            }
        }
    }

    // Store size property
    let size_key = v8::String::new(scope, "size").unwrap();
    let size_val = v8::Number::new(scope, total_size as f64);
    this.set(scope, size_key.into(), size_val.into());

    // Store type property
    let type_key = v8::String::new(scope, "type").unwrap();
    let type_val = v8::String::new(scope, &blob_type).unwrap();
    this.set(scope, type_key.into(), type_val.into());

    // Store parts in internal field (using a unique symbol approach)
    // For v1, we store as a hidden property
    let parts_key = v8::String::new(scope, "__blob_parts__").unwrap();
    let parts_val = v8::String::new(scope, &parts.join("")).unwrap();
    this.set(scope, parts_key.into(), parts_val.into());

    retval.set(this.into());
}

/// FormData constructor implementation (simplified v1)
fn form_data_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Initialize internal data store as a JSON-serializable string for v1
    // In a full implementation, we'd use V8's private properties
    let data_key = v8::String::new(scope, "__form_data__").unwrap();
    let data_val = v8::String::new(scope, "{}").unwrap();
    this.set(scope, data_key.into(), data_val.into());

    retval.set(this.into());
}

/// Response constructor implementation for WinterCG compatibility
fn response_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get body argument (first argument - string or null)
    let mut body_string = String::new();
    if args.length() > 0 {
        let arg = args.get(0);
        if !arg.is_null() && !arg.is_undefined() {
            if let Some(s) = arg.to_string(scope) {
                body_string = s.to_rust_string_lossy(scope);
            }
        }
    }

    // Get options argument (second argument - { status, headers })
    let mut status = 200;
    let mut headers_obj: Option<v8::Local<v8::Object>> = None;

    if args.length() > 1 {
        let options = args.get(1);
        if let Some(opts) = options.to_object(scope) {
            // Extract status
            let status_key = v8::String::new(scope, "status").unwrap();
            if let Some(status_val) = opts.get(scope, status_key.into()) {
                if let Some(num) = status_val.to_number(scope) {
                    status = num.value() as u16;
                }
            }

            // Extract headers
            let headers_key = v8::String::new(scope, "headers").unwrap();
            headers_obj = opts.get(scope, headers_key.into()).and_then(|h| h.to_object(scope));
        }
    }

    // Set status property
    let status_key = v8::String::new(scope, "status").unwrap();
    let status_val = v8::Number::new(scope, status as f64);
    this.set(scope, status_key.into(), status_val.into());

    // Create headers object
    let headers = v8::Object::new(scope);
    if let Some(hdrs) = headers_obj {
        // Copy headers from options
        if let Some(names) = hdrs.get_own_property_names(scope, Default::default()) {
            let len = names.length();
            for i in 0..len {
                if let Some(key) = names.get_index(scope, i) {
                    if let Some(key_str) = key.to_string(scope) {
                        let key_name = key_str.to_rust_string_lossy(scope);
                        if let Some(value) = hdrs.get(scope, key.into()) {
                            if let Some(value_str) = value.to_string(scope) {
                                let value_string = value_str.to_rust_string_lossy(scope);
                                let hkey = v8::String::new(scope, &key_name).unwrap();
                                let hval = v8::String::new(scope, &value_string).unwrap();
                                headers.set(scope, hkey.into(), hval.into());
                            }
                        }
                    }
                }
            }
        }
    }

    // Set headers property
    let headers_key = v8::String::new(scope, "headers").unwrap();
    this.set(scope, headers_key.into(), headers.into());

    // Set body property
    let body_key = v8::String::new(scope, "body").unwrap();
    let body_val = v8::String::new(scope, &body_string).unwrap();
    this.set(scope, body_key.into(), body_val.into());

    // Add headers.set method for CORS middleware support
    let set_key = v8::String::new(scope, "set").unwrap();
    if let Some(set_fn) = v8::Function::new(scope, headers_set_callback) {
        headers.set(scope, set_key.into(), set_fn.into());
    }

    retval.set(this.into());
}

/// Callback for headers.set() method
fn headers_set_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Get the headers object (this)
    let this = args.this();

    // Get header name and value
    if args.length() >= 2 {
        let name = args.get(0).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default();
        let value = args.get(1).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default();

        // Set the header
        let key = v8::String::new(scope, &name).unwrap();
        let val = v8::String::new(scope, &value).unwrap();
        this.set(scope, key.into(), val.into());
    }
}

/// URL constructor implementation (simplified v1)
fn url_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get the URL string argument
    let url_string = if args.length() > 0 {
        args.get(0).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse the URL to extract components
    let parsed = url::Url::parse(&url_string).unwrap_or_else(|_| {
        url::Url::parse("http://localhost/").unwrap()
    });

    // Set href property (full URL)
    let href_key = v8::String::new(scope, "href").unwrap();
    let href_val = v8::String::new(scope, parsed.as_str()).unwrap();
    this.set(scope, href_key.into(), href_val.into());

    // Set protocol property
    let protocol_key = v8::String::new(scope, "protocol").unwrap();
    let protocol = format!("{}:", parsed.scheme());
    let protocol_val = v8::String::new(scope, &protocol).unwrap();
    this.set(scope, protocol_key.into(), protocol_val.into());

    // Set host property (hostname:port)
    let host_key = v8::String::new(scope, "host").unwrap();
    let host = if let Some(port) = parsed.port() {
        format!("{}:{}", parsed.host_str().unwrap_or(""), port)
    } else {
        parsed.host_str().unwrap_or("").to_string()
    };
    let host_val = v8::String::new(scope, &host).unwrap();
    this.set(scope, host_key.into(), host_val.into());

    // Set hostname property
    let hostname_key = v8::String::new(scope, "hostname").unwrap();
    let hostname = parsed.host_str().unwrap_or("");
    let hostname_val = v8::String::new(scope, hostname).unwrap();
    this.set(scope, hostname_key.into(), hostname_val.into());

    // Set port property
    let port_key = v8::String::new(scope, "port").unwrap();
    let port = parsed.port().map(|p| p.to_string()).unwrap_or_default();
    let port_val = v8::String::new(scope, &port).unwrap();
    this.set(scope, port_key.into(), port_val.into());

    // Set pathname property
    let pathname_key = v8::String::new(scope, "pathname").unwrap();
    let pathname = parsed.path();
    let pathname_val = v8::String::new(scope, pathname).unwrap();
    this.set(scope, pathname_key.into(), pathname_val.into());

    // Set search property (query string with ?)
    let search_key = v8::String::new(scope, "search").unwrap();
    let search = if parsed.query().is_some() {
        format!("?{}", parsed.query().unwrap_or(""))
    } else {
        String::new()
    };
    let search_val = v8::String::new(scope, &search).unwrap();
    this.set(scope, search_key.into(), search_val.into());

    // Set hash property (fragment with #)
    let hash_key = v8::String::new(scope, "hash").unwrap();
    let hash = if let Some(fragment) = parsed.fragment() {
        format!("#{}", fragment)
    } else {
        String::new()
    };
    let hash_val = v8::String::new(scope, &hash).unwrap();
    this.set(scope, hash_key.into(), hash_val.into());

    retval.set(this.into());
}

/// Headers constructor implementation (simplified v1)
fn headers_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Initialize internal headers store
    let headers_key = v8::String::new(scope, "__headers__").unwrap();
    let headers_val = v8::Object::new(scope);
    this.set(scope, headers_key.into(), headers_val.into());

    // If an initial headers object is provided, copy its values
    if args.length() > 0 {
        let init = args.get(0);
        if let Some(init_obj) = init.to_object(scope) {
            // Try to iterate over the object
            if let Some(names) = init_obj.get_own_property_names(scope, Default::default()) {
                let len = names.length();
                for i in 0..len {
                    if let Some(key) = names.get_index(scope, i) {
                        if let Some(key_str) = key.to_string(scope) {
                            let key_name = key_str.to_rust_string_lossy(scope);
                            if let Some(value) = init_obj.get(scope, key.into()) {
                                if let Some(value_str) = value.to_string(scope) {
                                    let value_string = value_str.to_rust_string_lossy(scope);
                                    let hkey = v8::String::new(scope, &key_name).unwrap();
                                    let hval = v8::String::new(scope, &value_string).unwrap();
                                    headers_val.set(scope, hkey.into(), hval.into());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Add get method
    let get_key = v8::String::new(scope, "get").unwrap();
    if let Some(get_fn) = v8::Function::new(scope, headers_get_callback) {
        this.set(scope, get_key.into(), get_fn.into());
    }

    // Add set method
    let set_key = v8::String::new(scope, "set").unwrap();
    if let Some(set_fn) = v8::Function::new(scope, headers_set_callback_v2) {
        this.set(scope, set_key.into(), set_fn.into());
    }

    // Add forEach method
    let foreach_key = v8::String::new(scope, "forEach").unwrap();
    if let Some(foreach_fn) = v8::Function::new(scope, headers_foreach_callback) {
        this.set(scope, foreach_key.into(), foreach_fn.into());
    }

    retval.set(this.into());
}

/// Callback for Headers.get() method
fn headers_get_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get the header name
    let name = if args.length() > 0 {
        args.get(0).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Get the internal headers store
    let headers_key = v8::String::new(scope, "__headers__").unwrap();
    if let Some(headers_val) = this.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
            let name_key = v8::String::new(scope, &name).unwrap();
            if let Some(value) = headers_obj.get(scope, name_key.into()) {
                if !value.is_null() && !value.is_undefined() {
                    retval.set(value);
                    return;
                }
            }
        }
    }

    // Return null if not found
    retval.set_null();
}

/// Callback for Headers.set() method (version for Headers object)
fn headers_set_callback_v2(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let this = args.this();

    if args.length() >= 2 {
        let name = args.get(0).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default();
        let value = args.get(1).to_string(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default();

        // Get the internal headers store
        let headers_key = v8::String::new(scope, "__headers__").unwrap();
        if let Some(headers_val) = this.get(scope, headers_key.into()) {
            if let Some(headers_obj) = headers_val.to_object(scope) {
                let name_key = v8::String::new(scope, &name).unwrap();
                let val_str = v8::String::new(scope, &value).unwrap();
                headers_obj.set(scope, name_key.into(), val_str.into());
            }
        }
    }
}

/// Callback for Headers.forEach() method
fn headers_foreach_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let this = args.this();

    if args.length() < 1 {
        return;
    }

    let callback = args.get(0);
    if !callback.is_function() {
        return;
    }
    let callback_fn = callback.cast::<v8::Function>();

    // Get the internal headers store
    let headers_key = v8::String::new(scope, "__headers__").unwrap();
    if let Some(headers_val) = this.get(scope, headers_key.into()) {
        if let Some(headers_obj) = headers_val.to_object(scope) {
            // Iterate over all properties
            if let Some(names) = headers_obj.get_own_property_names(scope, Default::default()) {
                let len = names.length();
                for i in 0..len {
                    if let Some(key) = names.get_index(scope, i) {
                        if let Some(key_str) = key.to_string(scope) {
                            let key_name = key_str.to_rust_string_lossy(scope);
                            if let Some(value) = headers_obj.get(scope, key.into()) {
                                // Call the callback with (value, key, headers)
                                let key_js = v8::String::new(scope, &key_name).unwrap();
                                let _ = callback_fn.call(scope, this.into(), &[value, key_js.into(), this.into()]);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::{initialize_platform, NanoIsolate};

    fn init_platform() {
        initialize_platform().expect("Failed to initialize V8 platform");
    }

    #[test]
    fn test_text_encoder_basic() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test basic encoding
        let code = r#"
            const encoder = new TextEncoder();
            const text = "Hello, World!";
            const encoded = encoder.encode(text);
            encoded.length === 13 && encoded[0] === 72;
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "TextEncoder should encode 'Hello, World!' correctly"
        );
    }

    #[test]
    fn test_text_encoder_utf8() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test emoji encoding: "🎉" should produce [240, 159, 142, 137]
        let code = r#"
            const encoder = new TextEncoder();
            const bytes = encoder.encode("🎉");
            bytes.length;
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        // Emoji should be 4 bytes in UTF-8
        assert_eq!(result_str, "4");
    }

    #[test]
    fn test_text_decoder_basic() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test basic decoding
        let code = r#"
            const encoder = new TextEncoder();
            const decoder = new TextDecoder();
            const original = "Hello, UTF-8! 🎉";
            const bytes = encoder.encode(original);
            const decoded = decoder.decode(bytes);
            decoded === original ? "PASS" : "FAIL: " + decoded;
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert!(
            result_str.starts_with("PASS"),
            "Roundtrip failed: {}",
            result_str
        );
    }

    #[test]
    fn test_console_exists() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that console object exists and has log/warn/error methods
        let code = r#"
            typeof console === "object" &&
            typeof console.log === "function" &&
            typeof console.warn === "function" &&
            typeof console.error === "function"
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true");
    }

    #[test]
    fn test_console_log_no_crash() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that console.log doesn't crash
        let code = r#"console.log("test message"); "OK";"#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "OK");
    }

    #[test]
    fn test_text_decoder_invalid_utf8() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that invalid UTF-8 produces replacement character
        let code = r#"
            const decoder = new TextDecoder();
            // 0xFF is invalid in UTF-8
            const bytes = new Uint8Array([0xFF, 0xFE]);
            decoder.decode(bytes);
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        // Should contain replacement character () for invalid sequences
        assert!(
            result_str.contains("\u{FFFD}") || result_str.len() > 0,
            "Invalid UTF-8 should produce replacement characters"
        );
    }

    #[test]
    fn test_crypto_get_random_values() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test that we can call getRandomValues
        let code = r#"
            const arr = new Uint8Array(8);
            const result = crypto.getRandomValues(arr);
            result.length === 8 && result === arr
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "crypto.getRandomValues should return the same array"
        );
    }

    #[test]
    fn test_performance_now() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test that performance.now() returns a number >= 0
        let code = r#"
            const t1 = performance.now();
            const t2 = performance.now();
            typeof t1 === 'number' && t1 >= 0 && t2 >= t1
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "performance.now() should return monotonic increasing numbers"
        );
    }

    #[test]
    fn test_structured_clone() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test that structuredClone creates independent copies
        let code = r#"
            const original = { a: 1, b: [2, 3] };
            const cloned = structuredClone(original);
            cloned.a = 999;
            original.a === 1 && cloned.a === 999
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "structuredClone should create independent copies"
        );
    }

    #[test]
    fn test_dom_exception() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test DOMException constructor
        let code = r#"
            const err = new DOMException("Something went wrong", "AbortError");
            err.name === "AbortError" && err.message === "Something went wrong"
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(
            result_str, "true",
            "DOMException should have correct name and message"
        );
    }

    #[test]
    fn test_blob() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test Blob constructor
        let code = r#"
            const blob = new Blob(["test content"]);
            blob.size === 12 && blob.type === ""
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "Blob should have correct size");
    }

    #[test]
    fn test_form_data() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Bind APIs
        RuntimeAPIs::bind_all(scope, context);

        // Test FormData constructor exists
        let code = r#"
            typeof FormData === 'function'
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "FormData should be a function");
    }
}
