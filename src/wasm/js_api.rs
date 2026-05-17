//! WebAssembly JavaScript API implementation
//!
//! Exposes V8's built-in WebAssembly to JavaScript:
//! - WebAssembly.compile(bytes) -> Promise<Module>
//! - WebAssembly.instantiate(moduleOrBytes, imports) -> Promise<Instance>
//! - WebAssembly.validate(bytes) -> boolean
//! - WebAssembly.Module class
//! - WebAssembly.Instance class
//! - WebAssembly.Memory class
//! - WebAssembly.CompileError, RuntimeError
//!
//! # Compilation Cache
//!
//! `WebAssembly.compile` and `WebAssembly.instantiate` are NOT intercepted at the JS
//! level because `v8::WasmModuleObject::compile` (the rusty_v8 synchronous compile API)
//! returns `None` in this V8 build (v147). The API is marked `#[ignore]` in the existing
//! wasm_binary_debug_test. V8's own JS-level `WebAssembly.compile()` works correctly.
//!
//! The `global_wasm_cache` and `compile_module` (in `engine.rs`) provide process-global
//! caching for Rust-side compilation paths (e.g. sliver pre-compilation). The JS path
//! uses V8's native engine which has its own internal caching.

use v8;
use crate::wasm::WasmLoader;

/// WebAssembly JavaScript API binder
pub struct WebAssemblyAPI;

impl WebAssemblyAPI {
    /// Bind WebAssembly global to context
    pub fn bind(scope: &mut v8::PinnedRef<v8::HandleScope<()>>, context: v8::Local<v8::Context>) {
        // V8 already has WebAssembly built-in
        // We just need to ensure it's accessible on the global object
        // The native WebAssembly object is added by V8 automatically
        // We can optionally extend it with our custom functionality here

        let global = context.global(scope);

        // Enter context scope for V8 APIs that require HandleScope<Context>
        let mut ctx_scope = v8::ContextScope::new(scope, context);

        // Check if WebAssembly already exists (it should in modern V8)
        let wasm_key = v8::String::new(&mut ctx_scope, "WebAssembly").unwrap();

        if global.get(&mut ctx_scope, wasm_key.into()).is_none() {
            panic!("V8 WebAssembly is not available. This runtime requires V8 with WebAssembly support.");
        }

        tracing::debug!("WebAssembly API available in V8");

        // Add our validate function which provides additional Rust-side validation
        if let Some(wasm_val) = global.get(&mut ctx_scope, wasm_key.into()) {
            if let Some(wasm_obj) = wasm_val.to_object(&mut ctx_scope) {
                // Create validate function
                let validate_fn = v8::FunctionTemplate::new(&mut ctx_scope, wasm_validate_callback);
                let validate_func = validate_fn.get_function(&mut ctx_scope).unwrap();
                let validate_name = v8::String::new(&mut ctx_scope, "validate").unwrap();
                wasm_obj.set(&mut ctx_scope, validate_name.into(), validate_func.into());

                // Note: WebAssembly.compile and WebAssembly.instantiate are NOT overridden here.
                // The rusty_v8 v147 synchronous compile API (v8::WasmModuleObject::compile)
                // returns None in this V8 build — it is not suitable for use in FunctionCallbacks.
                // V8's native JS WebAssembly.compile() works correctly through the JS event loop.
                // Process-global caching is available via crate::wasm::engine::global_wasm_cache()
                // for Rust-side (non-JS) compilation paths.
            }
        }

        tracing::debug!("Bound WebAssembly API");
    }
}

/// Extract WASM bytes from a JS value (ArrayBuffer, Uint8Array, or any ArrayBufferView).
/// Returns None if the argument is not a supported BufferSource type.
/// Available for future JS-level cache interceptors when V8 compile API is usable.
#[allow(dead_code)]
pub(crate) fn extract_wasm_bytes(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    arg: v8::Local<v8::Value>,
) -> Option<Vec<u8>> {
    if arg.is_array_buffer() {
        let ab = arg.cast::<v8::ArrayBuffer>();
        let store = ab.get_backing_store();
        let len = ab.byte_length();
        let mut vec = Vec::with_capacity(len);
        for i in 0..len {
            if let Some(cell) = store.get(i) {
                vec.push(cell.get());
            }
        }
        Some(vec)
    } else if arg.is_uint8_array() {
        let ta = arg.cast::<v8::Uint8Array>();
        let len = ta.byte_length();
        let mut vec = Vec::with_capacity(len);
        for i in 0..len {
            if let Some(val) = ta.get_index(scope, i as u32) {
                if let Some(num) = val.to_integer(scope) {
                    vec.push(num.value() as u8);
                }
            }
        }
        Some(vec)
    } else if arg.is_array_buffer_view() {
        // Generic ArrayBufferView (Int8Array, Uint16Array, etc.)
        let view = arg.cast::<v8::ArrayBufferView>();
        let len = view.byte_length();
        let ab = view.buffer(scope)?;
        let store = ab.get_backing_store();
        let offset = view.byte_offset();
        let mut vec = Vec::with_capacity(len);
        for i in offset..(offset + len) {
            if let Some(cell) = store.get(i) {
                vec.push(cell.get());
            }
        }
        Some(vec)
    } else {
        None
    }
}

/// WebAssembly.validate() callback implementation
fn wasm_validate_callback(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let buffer = args.get(0);

    // Extract bytes from ArrayBuffer or TypedArray
    let bytes = if buffer.is_array_buffer() {
        let ab = buffer.cast::<v8::ArrayBuffer>();
        let store = ab.get_backing_store();
        let len = ab.byte_length();
        let mut vec = Vec::with_capacity(len);
        for i in 0..len {
            if let Some(cell) = store.get(i) {
                vec.push(cell.get());
            }
        }
        vec
    } else if buffer.is_uint8_array() {
        let ta = buffer.cast::<v8::Uint8Array>();
        let len = ta.byte_length();
        let mut vec = Vec::with_capacity(len);
        for i in 0..len {
            if let Some(val) = ta.get_index(scope, i as u32) {
                if let Some(num) = val.to_integer(scope) {
                    vec.push(num.value() as u8);
                }
            }
        }
        vec
    } else {
        // Invalid argument type
        let msg = v8::String::new(scope, "WebAssembly.validate: argument must be an ArrayBuffer or TypedArray").unwrap();
        let error = v8::Exception::type_error(scope, msg);
        scope.throw_exception(error);
        return;
    };

    // Validate using our Rust validator
    let valid = WasmLoader::validate(&bytes).is_ok();
    retval.set(v8::Boolean::new(scope, valid).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests require V8 isolate setup
    // These are basic unit tests for the validation logic

    #[test]
    fn test_validation_helper() {
        let valid_wasm = b"\0asm\x01\x00\x00\x00";
        assert!(WasmLoader::validate(valid_wasm).is_ok());

        let invalid_wasm = b"invalid";
        assert!(WasmLoader::validate(invalid_wasm).is_err());
    }
}
