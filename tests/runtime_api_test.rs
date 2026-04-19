//! Runtime API integration tests
//!
//! Tests the console, TextEncoder, and TextDecoder APIs in an integrated
//! manner with actual JavaScript execution.

use nano::runtime::RuntimeAPIs;
use nano::v8::{initialize_platform, NanoIsolate};

/// Test console.log output with tracing
#[test]
fn test_console_log_in_handler() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Bind all runtime APIs
    RuntimeAPIs::bind_all(scope, context);

    // Execute JavaScript that uses console.log
    let js_code = r#"
        console.log("Processing request:", "GET");
        console.warn("This is a warning");
        console.error("This is an error");
        "OK"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "OK");
}

/// Test TextEncoder and TextDecoder roundtrip
#[test]
fn test_text_encoder_decoder_roundtrip() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    // Bind all runtime APIs
    RuntimeAPIs::bind_all(scope, context);

    // Test roundtrip encoding and decoding
    let js_code = r#"
        const encoder = new TextEncoder();
        const decoder = new TextDecoder();
        
        const text = "Hello, UTF-8! 🎉";
        const bytes = encoder.encode(text);
        const decoded = decoder.decode(bytes);
        
        // Verify byte length
        const correctLength = bytes.length === 17; // "Hello, UTF-8! 🎉" in UTF-8
        const correctRoundtrip = decoded === text;
        
        correctLength && correctRoundtrip ? "PASS" : "FAIL"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS", "TextEncoder/Decoder roundtrip failed");
}

/// Test TextEncoder with empty string
#[test]
fn test_text_encoder_empty() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    RuntimeAPIs::bind_all(scope, context);

    let js_code = r#"
        const encoder = new TextEncoder();
        const bytes = encoder.encode("");
        bytes.length === 0 ? "PASS" : "FAIL"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS");
}

/// Test TextDecoder with empty input
#[test]
fn test_text_decoder_empty() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    RuntimeAPIs::bind_all(scope, context);

    let js_code = r#"
        const decoder = new TextDecoder();
        const empty = new Uint8Array([]);
        const decoded = decoder.decode(empty);
        decoded === "" ? "PASS" : "FAIL"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS");
}

/// Test console methods with multiple arguments
#[test]
fn test_console_multiple_args() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    RuntimeAPIs::bind_all(scope, context);

    let js_code = r#"
        console.log("arg1", "arg2", 123, true);
        "OK"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "OK");
}

/// Test encoding/decoding with various Unicode characters
#[test]
fn test_unicode_various() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    RuntimeAPIs::bind_all(scope, context);

    let js_code = r#"
        const encoder = new TextEncoder();
        const decoder = new TextDecoder();
        
        const tests = [
            "Hello World",           // ASCII
            "Привет мир",            // Cyrillic
            "こんにちは",            // Japanese
            "🎉🎊🎁",               // Emoji
            "\u{00E9}\u{00E8}",      // Accented chars
        ];
        
        let allPass = true;
        for (const text of tests) {
            const bytes = encoder.encode(text);
            const decoded = decoder.decode(bytes);
            if (decoded !== text) {
                allPass = false;
                console.log("FAIL:", text, "!=", decoded);
            }
        }
        
        allPass ? "PASS" : "FAIL"
    "#;

    let code_string = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code_string, None).expect("Script compilation failed");

    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);

    assert_eq!(result_str, "PASS");
}
