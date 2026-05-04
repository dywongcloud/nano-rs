//! WASM Async Execution Verification Test
//!
//! This test verifies that the async event loop implementation actually allows
//! Promises to resolve to completion instead of returning "Promise still pending".

use nano::v8::initialize_platform;
use nano::v8::NanoIsolate;

/// Test that simple Promise resolution works with the async event loop
///
/// This test creates an isolate, executes JavaScript that returns a Promise,
/// and verifies the Promise resolves to the expected value.
#[test]
fn test_simple_promise_resolution() {
    initialize_platform();
    
    // Create a fresh isolate
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Create a context for execution
    {
        let isolate_ptr = isolate.isolate();
        let handle_scope = &mut v8::HandleScope::new(isolate_ptr);
        let context = v8::Context::new(handle_scope, Default::default());
        let context_scope = &mut v8::ContextScope::new(handle_scope, context);
        
        // Execute JavaScript that returns a resolved Promise
        let code = "Promise.resolve(42)";
        let source = v8::String::new(context_scope, code).unwrap();
        let script = v8::Script::compile(context_scope, source.into(), None)
            .expect("Failed to compile script");
        
        let result = script.run(context_scope);
        
        // The result should be a Promise
        assert!(result.is_some(), "Script should return a result");
        let result_val = result.unwrap();
        assert!(result_val.is_promise(), "Result should be a Promise");
        
        // Use the async support to resolve the Promise
        let promise = result_val.cast::<v8::Promise>();
        let resolved = nano::runtime::async_support::resolve_promise_with_async(
            context_scope, 
            promise
        );
        
        // The Promise should resolve successfully (not return "Promise still pending" error)
        assert!(resolved.is_ok(), 
            "Promise should resolve successfully. Got error: {:?}",
            resolved.err()
        );
        
        // The resolved value should be 42
        let value = resolved.unwrap();
        let int_opt = value.to_integer(context_scope);
        let int_val = int_opt.map(|i| i.value()).unwrap_or(-1) as i32;
        
        assert_eq!(int_val, 42, 
            "Resolved value should be 42, got {}", int_val);
    }
    
    println!("✅ Simple Promise resolution test passed!");
}

/// Test that async/await pattern works
///
/// This test executes an async function and verifies it completes.
#[test]
fn test_async_await_execution() {
    initialize_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    {
        let isolate_ptr = isolate.isolate();
        let handle_scope = &mut v8::HandleScope::new(isolate_ptr);
        let context = v8::Context::new(handle_scope, Default::default());
        let context_scope = &mut v8::ContextScope::new(handle_scope, context);
        
        // Execute an async function
        let code = r#"
            (async function() {
                const value = await Promise.resolve(100);
                return value;
            })()
        "#;
        
        let source = v8::String::new(context_scope, code).unwrap();
        let script = v8::Script::compile(context_scope, source.into(), None)
            .expect("Failed to compile script");
        
        let result = script.run(context_scope);
        
        assert!(result.is_some(), "Script should return a result");
        let result_val = result.unwrap();
        assert!(result_val.is_promise(), "Result should be a Promise (async function)");
        
        // Resolve the Promise
        let promise = result_val.cast::<v8::Promise>();
        let resolved = nano::runtime::async_support::resolve_promise_with_async(
            context_scope,
            promise
        );
        
        assert!(resolved.is_ok(),
            "Async function Promise should resolve. Got error: {:?}",
            resolved.err()
        );
        
        let value = resolved.unwrap();
        let int_opt = value.to_integer(context_scope);
        let int_val = int_opt.map(|i| i.value()).unwrap_or(-1) as i32;
        
        assert_eq!(int_val, 100,
            "Async function should return 100, got {}", int_val);
    }
    
    println!("✅ Async/await execution test passed!");
}

/// Test that Promise rejection is handled correctly
#[test]
fn test_promise_rejection() {
    initialize_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    {
        let isolate_ptr = isolate.isolate();
        let handle_scope = &mut v8::HandleScope::new(isolate_ptr);
        let context = v8::Context::new(handle_scope, Default::default());
        let context_scope = &mut v8::ContextScope::new(handle_scope, context);
        
        // Execute code that returns a rejected Promise
        let code = r#"Promise.reject(new Error("Test rejection"))"#;
        
        let source = v8::String::new(context_scope, code).unwrap();
        let script = v8::Script::compile(context_scope, source.into(), None)
            .expect("Failed to compile script");
        
        let result = script.run(context_scope);
        
        assert!(result.is_some(), "Script should return a result");
        let result_val = result.unwrap();
        assert!(result_val.is_promise(), "Result should be a Promise");
        
        // Resolve the Promise (should get rejection)
        let promise = result_val.cast::<v8::Promise>();
        let resolved = nano::runtime::async_support::resolve_promise_with_async(
            context_scope,
            promise
        );
        
        // Should get an error (rejection), not Ok
        assert!(resolved.is_err(),
            "Rejected Promise should return an error"
        );
        
        let err_msg = format!("{}", resolved.unwrap_err());
        assert!(err_msg.contains("Promise rejected"),
            "Error should contain 'Promise rejected', got: {}", err_msg);
    }
    
    println!("✅ Promise rejection test passed!");
}

/// Verify that "Promise still pending" is no longer returned
///
/// This test checks that the specific error message is gone.
#[test]
fn test_no_more_promise_still_pending() {
    initialize_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    {
        let isolate_ptr = isolate.isolate();
        let handle_scope = &mut v8::HandleScope::new(isolate_ptr);
        let context = v8::Context::new(handle_scope, Default::default());
        let context_scope = &mut v8::ContextScope::new(handle_scope, context);
        
        // Execute code that returns a Promise
        let code = "Promise.resolve(123)";
        
        let source = v8::String::new(context_scope, code).unwrap();
        let script = v8::Script::compile(context_scope, source.into(), None)
            .expect("Failed to compile script");
        
        let result = script.run(context_scope);
        let result_val = result.unwrap();
        let promise = result_val.cast::<v8::Promise>();
        
        let resolved = nano::runtime::async_support::resolve_promise_with_async(
            context_scope,
            promise
        );
        
        // Check that the error (if any) does NOT contain "Promise still pending"
        if let Err(ref e) = resolved {
            let err_str = format!("{}", e);
            assert!(!err_str.contains("Promise still pending"),
                "Error should NOT contain 'Promise still pending'. Got: {}",
                err_str
            );
        }
        
        // The Promise should actually resolve successfully
        assert!(resolved.is_ok(),
            "Promise should resolve successfully, not return 'Promise still pending'"
        );
    }
    
    println!("✅ 'Promise still pending' is no longer returned!");
}
