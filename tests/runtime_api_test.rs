//! Runtime API integration tests
//!
//! Tests the handler execution functionality for Phase 3 Plan 01.

use nano::runtime::{HandlerContext, execute_handler};
use nano::http::{NanoRequest, NanoUrl, NanoHeaders};
use nano::v8::{initialize_platform, NanoIsolate};

/// Test handler context creation
#[test]
fn test_handler_context_creation() {
    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );

    let context = HandlerContext {
        entrypoint: "/app/index.js".to_string(),
        request,
    };

    assert_eq!(context.entrypoint, "/app/index.js");
    assert_eq!(context.request.method(), "GET");
}

/// Test handler execution with a simple JavaScript file
#[tokio::test]
async fn test_execute_handler_no_fetch() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Create a simple JS file that doesn't define fetch
    let js_code = r#"console.log("Hello from handler");"#;
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join("test_handler_no_fetch.js");
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");

    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );

    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
    };

    let response = execute_handler(&mut isolate, context).await;
    
    // Should return a placeholder response since no fetch function defined
    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.status(), 200);
}

/// Test handler execution with a fetch function that returns a response
#[tokio::test]
async fn test_execute_handler_with_fetch() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Create a JS file that defines fetch and returns a response
    let js_code = r#"
        function fetch(request) {
            return {
                status: 200,
                headers: { "Content-Type": "application/json" },
                body: '{"success": true}'
            };
        }
    "#;
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join("test_handler_with_fetch.js");
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");

    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new(
        "POST".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );

    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
    };

    let response = execute_handler(&mut isolate, context).await;
    
    assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
    let response = response.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("Content-Type"), Some("application/json".to_string()));
}

/// Test handler execution with custom status code
#[tokio::test]
async fn test_execute_handler_custom_status() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Create a JS file that returns a 404 response
    let js_code = r#"
        function fetch(request) {
            return {
                status: 404,
                headers: { "Content-Type": "text/plain" },
                body: "Not Found"
            };
        }
    "#;
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join("test_handler_404.js");
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");

    let url = NanoUrl::parse("https://example.com/not-found").unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );

    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
    };

    let response = execute_handler(&mut isolate, context).await;
    
    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.status(), 404);
}

/// Test handler execution with request method access
#[tokio::test]
async fn test_execute_handler_request_access() {
    initialize_platform().expect("Failed to initialize V8");

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Create a JS file that accesses request properties
    let js_code = r#"
        function fetch(request) {
            return {
                status: 200,
                headers: { "X-Request-Method": request.method },
                body: "Method: " + request.method
            };
        }
    "#;
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join("test_handler_request.js");
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");

    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new(
        "DELETE".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );

    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
    };

    let response = execute_handler(&mut isolate, context).await;
    
    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("X-Request-Method"), Some("DELETE".to_string()));
}
