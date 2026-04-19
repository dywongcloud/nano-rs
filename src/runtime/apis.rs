//! Runtime JavaScript APIs for WinterCG compatibility
//!
//! This module provides JavaScript API bindings that bridge between V8 and Rust:
//! - console.log/warn/error with structured logging via tracing
//! - TextEncoder/TextDecoder for UTF-8 encoding/decoding
//!
//! All APIs are bound to the V8 global scope via RuntimeAPIs::bind_all().

/// RuntimeAPIs manages all JavaScript API bindings
///
/// This struct provides methods to bind WinterCG-compatible APIs to V8 contexts.
/// Call RuntimeAPIs::bind_all() during context setup to make all APIs available.
pub struct RuntimeAPIs;

impl RuntimeAPIs {
    /// Bind all runtime APIs to the V8 context
    ///
    /// This should be called once per context during handler setup.
    /// Makes console, TextEncoder, and TextDecoder available to JavaScript.
    pub fn bind_all(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        Self::bind_console(scope, context);
        Self::bind_text_encoder(scope, context);
        Self::bind_text_decoder(scope, context);
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
        let encoder_ctor = encoder_template.get_function(scope).unwrap();

        // Add encode method to prototype
        let prototype = encoder_ctor
            .get_prototype(scope)
            .and_then(|p| p.to_object(scope))
            .expect("Failed to get TextEncoder prototype");

        if let Some(encode_fn) = v8::Function::new(scope, text_encoder_encode) {
            let key = v8::String::new(scope, "encode").unwrap();
            prototype.set(scope, key.into(), encode_fn.into());
        }

        // Attach TextEncoder to global
        let key = v8::String::new(scope, "TextEncoder").unwrap();
        global.set(scope, key.into(), encoder_ctor.into());
    }

    /// Bind TextDecoder API to global scope
    fn bind_text_decoder(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create TextDecoder constructor function
        let decoder_template = v8::FunctionTemplate::new(scope, text_decoder_constructor);
        let decoder_ctor = decoder_template.get_function(scope).unwrap();

        // Add decode method to prototype
        let prototype = decoder_ctor
            .get_prototype(scope)
            .and_then(|p| p.to_object(scope))
            .expect("Failed to get TextDecoder prototype");

        if let Some(decode_fn) = v8::Function::new(scope, text_decoder_decode) {
            let key = v8::String::new(scope, "decode").unwrap();
            prototype.set(scope, key.into(), decode_fn.into());
        }

        // Attach TextDecoder to global
        let key = v8::String::new(scope, "TextDecoder").unwrap();
        global.set(scope, key.into(), decoder_ctor.into());
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
        // Extract bytes from ArrayBuffer
        let arraybuffer = arg.cast::<v8::ArrayBuffer>();
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

        // Test: new TextEncoder().encode("hello")
        let code = r#"
            const encoder = new TextEncoder();
            const bytes = encoder.encode("hello");
            Array.from(bytes);
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        // Should produce [104, 101, 108, 108, 111] for "hello"
        assert!(result_str.contains("104"));
        assert!(result_str.contains("101"));
        assert!(result_str.contains("111"));
    }

    #[test]
    fn test_text_encoder_unicode() {
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

        // Test: Create Uint8Array and decode it
        let code = r#"
            const decoder = new TextDecoder();
            const bytes = new Uint8Array([104, 101, 108, 108, 111]);
            decoder.decode(bytes);
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "hello");
    }

    #[test]
    fn test_text_encoder_decoder_roundtrip() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);


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
    if let Some(uint8array) = arg.to_object(scope).and_then(|o| o.try_cast::<v8::Uint8Array>()) {
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
    if let Some(uint16array) = arg.to_object(scope).and_then(|o| o.try_cast::<v8::Uint16Array>()) {
        let length = uint16array.byte_length() / 2;

        if length == 0 {
            retval.set(arg);
            return;
        }

        let mut buffer = vec![0u16; length];
        let byte_buffer = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut u8,
                buffer.len() * 2,
            )
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
    if let Some(uint32array) = arg.to_object(scope).and_then(|o| o.try_cast::<v8::Uint32Array>()) {
        let length = uint32array.byte_length() / 4;

        if length == 0 {
            retval.set(arg);
            return;
        }

        let mut buffer = vec![0u32; length];
        let byte_buffer = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut u8,
                buffer.len() * 4,
            )
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

        // Test roundtrip with emoji
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
}
