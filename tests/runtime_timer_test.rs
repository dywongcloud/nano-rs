//! Timer and AbortController integration tests
//!
//! These tests verify that the timer APIs (setTimeout, setInterval, clearTimeout,
//! clearInterval) and AbortController/AbortSignal work correctly when executed
//! in the V8 runtime.

use nano::runtime::apis::{init_thread_timer_queue, RuntimeAPIs};
use nano::runtime::TimerQueue;
use nano::v8::{initialize_platform, NanoIsolate};
use std::sync::Arc;

fn init_platform() {
    initialize_platform().expect("Failed to initialize V8 platform");
}

/// Helper to create a context with all APIs bound
fn create_test_context(isolate: &mut NanoIsolate) -> (v8::Global<v8::Context>, v8::OwnedIsolate) {
    let isolate_ptr = isolate.isolate();
    let mut owned_isolate = unsafe { std::ptr::read(isolate_ptr as *const v8::OwnedIsolate) };

    {
        let scope = &mut v8::HandleScope::new(&mut owned_isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Initialize timer queue
        let timer_queue = Arc::new(TimerQueue::new());
        init_thread_timer_queue(timer_queue);

        // Bind all APIs
        RuntimeAPIs::bind_all(scope, context);

        // Create persistent context handle
        let global_context = v8::Global::new(scope, context);

        // Return the global context and owned isolate
        return (global_context, owned_isolate);
    }
}

#[test]
fn test_abort_controller_exists() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that AbortController exists
    let code = r#"
        typeof AbortController === "function"
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "true", "AbortController should exist");
}

#[test]
fn test_abort_controller_basic() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
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
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(
        result_str, "PASS",
        "AbortController should correctly track abort state"
    );
}

#[test]
fn test_abort_controller_signal_has_abort_event() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that signal has addEventListener method
    let code = r#"
        const controller = new AbortController();
        const signal = controller.signal;
        typeof signal.addEventListener === "function" ? "PASS" : "FAIL";
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS", "Signal should have addEventListener");
}

#[test]
fn test_set_timeout_exists() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that setTimeout exists
    let code = r#"
        typeof setTimeout === "function"
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "true", "setTimeout should exist");
}

#[test]
fn test_set_interval_exists() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that setInterval exists
    let code = r#"
        typeof setInterval === "function"
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "true", "setInterval should exist");
}

#[test]
fn test_clear_timeout_exists() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that clearTimeout exists
    let code = r#"
        typeof clearTimeout === "function"
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "true", "clearTimeout should exist");
}

#[test]
fn test_clear_interval_exists() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that clearInterval exists
    let code = r#"
        typeof clearInterval === "function"
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "true", "clearInterval should exist");
}

#[test]
fn test_set_timeout_returns_number() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that setTimeout returns a numeric ID
    let code = r#"
        const id = setTimeout(() => {}, 100);
        typeof id === "number" && id > 0 ? "PASS" : "FAIL: " + typeof id + " = " + id;
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(
        result_str, "PASS",
        "setTimeout should return a positive number"
    );
}

#[test]
fn test_abort_controller_with_reason() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Initialize timer queue
    let timer_queue = Arc::new(TimerQueue::new());
    init_thread_timer_queue(timer_queue);

    // Bind APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test that abort() with reason works
    let code = r#"
        const controller = new AbortController();
        controller.abort("Custom reason");
        controller.signal.aborted === true ? "PASS" : "FAIL";
    "#;

    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS", "Abort with reason should work");
}
