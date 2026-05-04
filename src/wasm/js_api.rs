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
        
        // Check if WebAssembly already exists (it should in modern V8)
        let wasm_key = v8::String::new(scope, "WebAssembly").unwrap();
        
        if global.get(scope, wasm_key.into()).is_none() {
            // If for some reason WebAssembly is not available, create a stub
            // that will throw meaningful errors
            let wasm_obj = v8::Object::new(scope);
            
            // Create stub compile function
            let compile_stub = v8::FunctionTemplate::new(scope, |scope: &mut v8::PinnedRef<v8::HandleScope>, 
                _args: v8::FunctionCallbackArguments, 
                mut _retval: v8::ReturnValue| {
                let msg = v8::String::new(scope, "WebAssembly.compile is not available").unwrap();
                let error = v8::Exception::error(scope, msg);
                scope.throw_exception(error);
            });
            let compile_fn = compile_stub.get_function(scope).unwrap();
            let compile_name = v8::String::new(scope, "compile").unwrap();
            wasm_obj.set(scope, compile_name.into(), compile_fn.into());
            
            // Create stub instantiate function
            let instantiate_stub = v8::FunctionTemplate::new(scope, |scope: &mut v8::PinnedRef<v8::HandleScope>,
                _args: v8::FunctionCallbackArguments,
                mut _retval: v8::ReturnValue| {
                let msg = v8::String::new(scope, "WebAssembly.instantiate is not available").unwrap();
                let error = v8::Exception::error(scope, msg);
                scope.throw_exception(error);
            });
            let instantiate_fn = instantiate_stub.get_function(scope).unwrap();
            let instantiate_name = v8::String::new(scope, "instantiate").unwrap();
            wasm_obj.set(scope, instantiate_name.into(), instantiate_fn.into());
            
            // Set the stub WebAssembly object
            global.set(scope, wasm_key.into(), wasm_obj.into());
            
            tracing::warn!("Created stub WebAssembly API - V8 WebAssembly not available");
        } else {
            tracing::debug!("WebAssembly API already available in V8");
        }
        
        // Add our validate function which provides additional Rust-side validation
        if let Some(wasm_val) = global.get(scope, wasm_key.into()) {
            if let Some(wasm_obj) = wasm_val.to_object(scope) {
                // Create validate function
                let validate_fn = v8::FunctionTemplate::new(scope, wasm_validate_callback);
                let validate_func = validate_fn.get_function(scope).unwrap();
                let validate_name = v8::String::new(scope, "validate").unwrap();
                wasm_obj.set(scope, validate_name.into(), validate_func.into());
            }
        }
        
        tracing::debug!("Bound WebAssembly API");
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
