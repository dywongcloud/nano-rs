//! JavaScript execution integration tests
//!
//! These tests verify end-to-end JavaScript execution in V8 isolates,
//! including the hello.js example file that demonstrates Phase 1 success.

use nano::v8::{execute_script, initialize_platform, NanoIsolate};

/// Test basic arithmetic execution
#[test]
fn test_basic_js_execution() {
    initialize_platform().expect("Failed to initialize V8 platform");
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let result = execute_script(&mut isolate, "1 + 1").expect("Script execution failed");
    assert_eq!(result, "2");
}

/// Test console.log output
#[test]
fn test_console_log_output() {
    initialize_platform().expect("Failed to initialize V8 platform");
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // This should print to stdout during test
    execute_script(&mut isolate, r#"console.log("test output")"#)
        .expect("Script with console.log failed");
    // Test passes if no panic and output visible
}

/// Test hello.js example file execution
#[test]
fn test_hello_js_file() {
    initialize_platform().expect("Failed to initialize V8 platform");
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let code = include_str!("../examples/hello.js");
    execute_script(&mut isolate, code).expect("hello.js execution failed");
}
