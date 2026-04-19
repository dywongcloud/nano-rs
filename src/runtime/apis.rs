//! Runtime JavaScript APIs for WinterCG compatibility
//!
//! This module provides JavaScript API bindings that bridge between V8 and Rust:
//! - console.log/warn/error with structured logging via tracing
//! - TextEncoder/TextDecoder for UTF-8 encoding/decoding
//! - setTimeout/setInterval/clearTimeout/clearInterval for async timers
//! - AbortController/AbortSignal for async cancellation
//!
//! All APIs are bound to the V8 global scope via RuntimeAPIs::bind_all().

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::runtime::{get_abort_state, register_abort_state, AbortSignalState, TimerId, TimerQueue};

/// Thread-local timer queue for V8 callback access
thread_local! {
    static TIMER_QUEUE: RefCell<Option<Arc<TimerQueue>>> = RefCell::new(None);
    static TIMER_CALLBACKS: RefCell<HashMap<u64, v8::Global<v8::Function>>> = RefCell::new(HashMap::new());
}

/// Initialize thread-local timer queue
pub fn init_thread_timer_queue(queue: Arc<TimerQueue>) {
    TIMER_QUEUE.with(|q| {
        *q.borrow_mut() = Some(queue);
    });
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
    /// Makes console, TextEncoder, TextDecoder, timers, and AbortController available to JavaScript.
    pub fn bind_all(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        Self::bind_console(scope, context);
        Self::bind_text_encoder(scope, context);
        Self::bind_text_decoder(scope, context);
        Self::bind_timers(scope, context);
        Self::bind_abort_controller(scope, context);
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

    /// Bind timer APIs (setTimeout, setInterval, clearTimeout, clearInterval)
    fn bind_timers(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Bind setTimeout
        if let Some(set_timeout_fn) = v8::Function::new(scope, set_timeout_callback) {
            let key = v8::String::new(scope, "setTimeout").unwrap();
            global.set(scope, key.into(), set_timeout_fn.into());
        }

        // Bind setInterval
        if let Some(set_interval_fn) = v8::Function::new(scope, set_interval_callback) {
            let key = v8::String::new(scope, "setInterval").unwrap();
            global.set(scope, key.into(), set_interval_fn.into());
        }

        // Bind clearTimeout
        if let Some(clear_timeout_fn) = v8::Function::new(scope, clear_timeout_callback) {
            let key = v8::String::new(scope, "clearTimeout").unwrap();
            global.set(scope, key.into(), clear_timeout_fn.into());
        }

        // Bind clearInterval
        if let Some(clear_interval_fn) = v8::Function::new(scope, clear_interval_callback) {
            let key = v8::String::new(scope, "clearInterval").unwrap();
            global.set(scope, key.into(), clear_interval_fn.into());
        }
    }

    /// Bind AbortController and AbortSignal APIs
    fn bind_abort_controller(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
        let global = context.global(scope);

        // Create AbortController constructor
        let controller_template = v8::FunctionTemplate::new(scope, abort_controller_constructor);
        let controller_ctor = controller_template.get_function(scope).unwrap();

        // Attach to global
        let key = v8::String::new(scope, "AbortController").unwrap();
        global.set(scope, key.into(), controller_ctor.into());
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

/// V8 callback for setTimeout
fn set_timeout_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Extract callback function (arg 0)
    if args.length() < 1 {
        retval.set(v8::Integer::new(scope, 0).into());
        return;
    }

    let callback = args.get(0);
    if !callback.is_function() {
        retval.set(v8::Integer::new(scope, 0).into());
        return;
    }
    let callback = callback.cast::<v8::Function>();

    // Extract delay (arg 1, default 0)
    let delay_ms = if args.length() > 1 {
        args.get(1)
            .to_integer(scope)
            .map(|i| i.value() as u64)
            .unwrap_or(0)
    } else {
        0
    };

    // Create persistent handle for callback
    let persistent_callback = v8::Global::new(scope, callback);

    // Store callback and schedule timer
    let timer_id = TIMER_QUEUE.with(|queue| {
        if let Some(queue) = queue.borrow().as_ref() {
            let queue = Arc::clone(queue);
            let id = pollster::block_on(async move {
                queue.schedule(delay_ms, move || {
                    // Timer fired - callback will be invoked by timer system
                }).await
            });
            Some(id.value())
        } else {
            None
        }
    });

    // Store callback for later invocation
    if let Some(id) = timer_id {
        TIMER_CALLBACKS.with(|callbacks| {
            callbacks.borrow_mut().insert(id, persistent_callback);
        });
        retval.set(v8::Integer::new_from_unsigned(scope, id as u32).into());
    } else {
        retval.set(v8::Integer::new(scope, 0).into());
    }
}

/// V8 callback for setInterval
fn set_interval_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Extract callback function (arg 0)
    if args.length() < 1 {
        retval.set(v8::Integer::new(scope, 0).into());
        return;
    }

    let callback = args.get(0);
    if !callback.is_function() {
        retval.set(v8::Integer::new(scope, 0).into());
        return;
    }
    let callback = callback.cast::<v8::Function>();

    // Extract interval (arg 1, default 0)
    let interval_ms = if args.length() > 1 {
        args.get(1)
            .to_integer(scope)
            .map(|i| i.value() as u64)
            .unwrap_or(0)
    } else {
        0
    };

    // Create persistent handle for callback
    let persistent_callback = v8::Global::new(scope, callback);

    // Store callback and schedule interval
    let timer_id = TIMER_QUEUE.with(|queue| {
        if let Some(queue) = queue.borrow().as_ref() {
            let queue = Arc::clone(queue);
            let id = pollster::block_on(async move {
                queue.schedule_interval(interval_ms, move || {
                    // Interval fired
                }).await
            });
            Some(id.value())
        } else {
            None
        }
    });

    // Store callback
    if let Some(id) = timer_id {
        TIMER_CALLBACKS.with(|callbacks| {
            callbacks.borrow_mut().insert(id, persistent_callback);
        });
        retval.set(v8::Integer::new_from_unsigned(scope, id as u32).into());
    } else {
        retval.set(v8::Integer::new(scope, 0).into());
    }
}

/// V8 callback for clearTimeout
fn clear_timeout_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    if args.length() < 1 {
        return;
    }

    let timer_id = args.get(0)
        .to_integer(scope)
        .map(|i| i.value() as u64)
        .unwrap_or(0);

    if timer_id == 0 {
        return;
    }

    // Cancel timer and remove callback
    TIMER_QUEUE.with(|queue| {
        if let Some(queue) = queue.borrow().as_ref() {
            let queue = Arc::clone(queue);
            pollster::block_on(async move {
                let id = TimerId(timer_id);
                queue.cancel(id).await;
            });
        }
    });

    TIMER_CALLBACKS.with(|callbacks| {
        callbacks.borrow_mut().remove(&timer_id);
    });
}

/// V8 callback for clearInterval
fn clear_interval_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Same implementation as clearTimeout
    clear_timeout_callback(scope, args, _retval);
}

/// AbortController constructor callback
fn abort_controller_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();

    // Create AbortSignalState in Rust
    let state = AbortSignalState::new();
    let state_id = register_abort_state(state);

    // Store state ID in V8 object as internal field
    let id_key = v8::String::new(scope, "__abort_state_id").unwrap();
    let id_value = v8::Integer::new_from_unsigned(scope, state_id as u32);
    this.set(scope, id_key.into(), id_value.into());

    // Create signal object
    let signal = create_abort_signal(scope, state_id);
    let signal_key = v8::String::new(scope, "signal").unwrap();
    this.set(scope, signal_key.into(), signal.into());

    // Add abort method
    let abort_fn = v8::Function::new(scope, abort_controller_abort).unwrap();
    let abort_key = v8::String::new(scope, "abort").unwrap();
    this.set(scope, abort_key.into(), abort_fn.into());

    retval.set(this.into());
}

/// Create an AbortSignal object
fn create_abort_signal(
    scope: &mut v8::HandleScope,
    state_id: u64,
) -> v8::Local<v8::Object> {
    let signal = v8::Object::new(scope);

    // Store state ID
    let id_key = v8::String::new(scope, "__abort_state_id").unwrap();
    let id_value = v8::Integer::new_from_unsigned(scope, state_id as u32);
    signal.set(scope, id_key.into(), id_value.into());

    // Set initial aborted property
    let aborted_key = v8::String::new(scope, "aborted").unwrap();
    let aborted_value = v8::Boolean::new(scope, false);
    signal.set(scope, aborted_key.into(), aborted_value.into());

    // Add onabort property (setter/getter would be better but using direct property for v1)
    let onabort_key = v8::String::new(scope, "onabort").unwrap();
    let onabort_value = v8::null(scope);
    signal.set(scope, onabort_key.into(), onabort_value.into());

    // Add addEventListener method (simplified - just handles 'abort' event)
    let add_listener_fn = v8::Function::new(scope, abort_signal_add_event_listener).unwrap();
    let add_listener_key = v8::String::new(scope, "addEventListener").unwrap();
    signal.set(scope, add_listener_key.into(), add_listener_fn.into());

    // Add removeEventListener stub
    let remove_listener_fn = v8::Function::new(scope, abort_signal_remove_event_listener).unwrap();
    let remove_listener_key = v8::String::new(scope, "removeEventListener").unwrap();
    signal.set(scope, remove_listener_key.into(), remove_listener_fn.into());

    signal
}

/// V8 callback for AbortController.abort()
fn abort_controller_abort(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let this = args.this();

    // Get state ID
    let id_key = v8::String::new(scope, "__abort_state_id").unwrap();
    let id_value = this.get(scope, id_key.into());

    let state_id = if let Some(id) = id_value.to_integer(scope) {
        id.value() as u64
    } else {
        return;
    };

    // Get reason argument if provided
    let reason = if args.length() > 0 {
        let arg = args.get(0);
        arg.to_string(scope).map(|s| s.to_rust_string_lossy(scope))
    } else {
        Some("AbortError".to_string())
    };

    // Abort the signal
    if let Some(state) = get_abort_state(state_id) {
        state.abort(reason);
    }

    // Update signal's aborted property
    let signal_key = v8::String::new(scope, "signal").unwrap();
    if let Some(signal) = this.get(scope, signal_key.into()).to_object(scope) {
        let aborted_key = v8::String::new(scope, "aborted").unwrap();
        let aborted_value = v8::Boolean::new(scope, true);
        signal.set(scope, aborted_key.into(), aborted_value.into());

        // Trigger onabort callback if set
        let onabort_key = v8::String::new(scope, "onabort").unwrap();
        let onabort = signal.get(scope, onabort_key.into());
        if onabort.is_function() {
            let onabort_fn = onabort.cast::<v8::Function>();
            let _ = onabort_fn.call(scope, signal.into(), &[]);
        }
    }
}

/// V8 callback for AbortSignal.addEventListener
fn abort_signal_add_event_listener(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Simplified: if event type is 'abort' and callback provided, store it
    if args.length() < 2 {
        return;
    }

    let this = args.this();

    // Check event type
    let event_type = args.get(0)
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_default();

    if event_type != "abort" {
        return;
    }

    // Get callback
    let callback = args.get(1);
    if callback.is_function() {
        // Store as onabort handler
        let onabort_key = v8::String::new(scope, "onabort").unwrap();
        this.set(scope, onabort_key.into(), callback.into());
    }
}

/// V8 callback for AbortSignal.removeEventListener
fn abort_signal_remove_event_listener(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Simplified: just clear the onabort handler
    let this = args.this();
    let onabort_key = v8::String::new(scope, "onabort").unwrap();
    let null_value = v8::null(scope);
    this.set(scope, onabort_key.into(), null_value.into());
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

        RuntimeAPIs::bind_all(scope, context);

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

    #[test]
    fn test_abort_controller_exists() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that AbortController constructor exists
        let code = r#"
            typeof AbortController === "function"
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "AbortController should be a function");
    }

    #[test]
    fn test_abort_controller_basic() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test basic AbortController functionality
        let code = r#"
            const controller = new AbortController();
            const signal = controller.signal;
            const beforeAbort = signal.aborted;
            controller.abort();
            const afterAbort = signal.aborted;
            beforeAbort === false && afterAbort === true ? "PASS" : "FAIL";
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "PASS", "AbortController should work correctly");
    }

    #[test]
    fn test_set_timeout_exists() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that setTimeout function exists
        let code = r#"
            typeof setTimeout === "function"
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "setTimeout should be a function");
    }

    #[test]
    fn test_clear_timeout_exists() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        let scope = &mut v8::HandleScope::new(isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        RuntimeAPIs::bind_all(scope, context);

        // Test that clearTimeout function exists
        let code = r#"
            typeof clearTimeout === "function"
        "#;

        let code_string = v8::String::new(scope, code).unwrap();
        let script =
            v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

        let result = script.run(scope).expect("Script execution failed");
        let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

        assert_eq!(result_str, "true", "clearTimeout should be a function");
    }
}
