//! Integration tests for HMAC signing/verification
//!
//! Tests HMAC key generation, sign/verify roundtrip,
//! signature tampering detection, and JWK import/export.

use nano::v8::{initialize_platform, NanoIsolate};
use nano::runtime::apis::RuntimeAPIs;

fn init_platform() {
    initialize_platform().expect("Failed to initialize V8 platform");
}

#[test]
fn test_hmac_sign_verify_roundtrip() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            // Generate HMAC key
            const key = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            // Sign data
            const message = new TextEncoder().encode("Hello, HMAC World!");
            const signature = crypto.subtle.sign("HMAC", key, message);
            
            // Verify signature length (32 bytes for SHA-256)
            const correctLength = signature.length === 32;
            
            // Verify signature
            const valid = crypto.subtle.verify("HMAC", key, signature, message);
            
            correctLength && valid
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "HMAC sign/verify roundtrip should work");
}

#[test]
fn test_hmac_signature_tampering_detected() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            const key = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            const message = new TextEncoder().encode("Test message");
            const signature = crypto.subtle.sign("HMAC", key, message);
            
            // Tamper with signature
            signature[0] ^= 0xFF;
            
            // Verify should return false, not throw
            const valid = crypto.subtle.verify("HMAC", key, signature, message);
            
            valid === false
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "Should detect tampered HMAC signature");
}

#[test]
fn test_hmac_wrong_message_fails() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            const key = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            const message = new TextEncoder().encode("Original message");
            const signature = crypto.subtle.sign("HMAC", key, message);
            
            // Verify with different message
            const wrongMessage = new TextEncoder().encode("Different message");
            const valid = crypto.subtle.verify("HMAC", key, signature, wrongMessage);
            
            valid === false
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "HMAC should reject wrong message");
}

#[test]
fn test_hmac_different_hash_algorithms() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            // SHA-256 (32 bytes)
            const key256 = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            const msg256 = new TextEncoder().encode("Test");
            const sig256 = crypto.subtle.sign("HMAC", key256, msg256);
            const ok256 = sig256.length === 32 && crypto.subtle.verify("HMAC", key256, sig256, msg256);
            
            // SHA-384 (48 bytes)
            const key384 = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-384" },
                true,
                ["sign", "verify"]
            );
            const msg384 = new TextEncoder().encode("Test");
            const sig384 = crypto.subtle.sign("HMAC", key384, msg384);
            const ok384 = sig384.length === 48 && crypto.subtle.verify("HMAC", key384, sig384, msg384);
            
            // SHA-512 (64 bytes)
            const key512 = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-512" },
                true,
                ["sign", "verify"]
            );
            const msg512 = new TextEncoder().encode("Test");
            const sig512 = crypto.subtle.sign("HMAC", key512, msg512);
            const ok512 = sig512.length === 64 && crypto.subtle.verify("HMAC", key512, sig512, msg512);
            
            ok256 && ok384 && ok512
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "All HMAC hash algorithms should work");
}

#[test]
fn test_hmac_jwk_import_export() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            // Generate key
            const key = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            // Export to JWK
            const jwk = crypto.subtle.exportKey("jwk", key);
            
            // Verify JWK properties
            const ok1 = jwk.kty === "oct";
            const ok2 = jwk.alg === "HS256";
            const ok3 = jwk.ext === true;
            const ok4 = Array.isArray(jwk.key_ops);
            const ok5 = typeof jwk.k === "string";
            
            // Import the JWK back
            const importedKey = crypto.subtle.importKey(
                "jwk",
                jwk,
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            // Test that imported key works
            const message = new TextEncoder().encode("Test with imported key");
            const signature = crypto.subtle.sign("HMAC", importedKey, message);
            const valid = crypto.subtle.verify("HMAC", importedKey, signature, message);
            
            ok1 && ok2 && ok3 && ok4 && ok5 && valid
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "HMAC JWK import/export should work");
}

#[test]
fn test_hmac_key_usage_validation() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            // Create key with only 'sign' usage
            const signKey = crypto.subtle.generateKey(
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign"] // no verify
            );
            
            const message = new TextEncoder().encode("Test");
            const signature = crypto.subtle.sign("HMAC", signKey, message);
            
            // Try to verify with sign-only key should fail
            try {
                crypto.subtle.verify("HMAC", signKey, signature, message);
                "Should have failed";
            } catch (e) {
                "Verify blocked: " + e.message
            }
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert!(result_str.contains("Verify blocked"), "Should enforce key usage restrictions");
}

#[test]
fn test_hmac_import_from_jwk_object() {
    init_platform();
    
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    RuntimeAPIs::bind_all(scope, context);
    
    let code = r#"
        try {
            // Create JWK object manually
            const jwk = {
                kty: "oct",
                k: "YmFzZTY0dXJsLWVuY29kZWQta2V5LWF0LWxlYXN0LTMyLWJ5dGVzLWZvci1IUzI1Nh",
                alg: "HS256",
                ext: true,
                key_ops: ["sign", "verify"]
            };
            
            // Import the JWK
            const key = crypto.subtle.importKey(
                "jwk",
                jwk,
                { name: "HMAC", hash: "SHA-256" },
                true,
                ["sign", "verify"]
            );
            
            // Test the imported key
            const message = new TextEncoder().encode("Test message");
            const signature = crypto.subtle.sign("HMAC", key, message);
            const valid = crypto.subtle.verify("HMAC", key, signature, message);
            
            valid
        } catch (e) {
            "Error: " + e.message
        }
    "#;
    
    let code_string = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code_string, None)
        .expect("Script compilation failed");
    
    let result = script.run(scope).expect("Script execution failed");
    let result_str = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
    
    assert_eq!(result_str, "true", "Should import HMAC key from JWK object");
}
