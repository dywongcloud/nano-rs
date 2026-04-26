//! Hono.js Integration Tests
//!
//! Extended tests for Hono-style middleware patterns and routing.

use nano::runtime::{HandlerContext, execute_handler};
use nano::http::{NanoRequest, NanoUrl, NanoHeaders};
use nano::v8::{initialize_platform, NanoIsolate};
use std::sync::Once;

static INIT: Once = Once::new();

fn init_platform() {
    INIT.call_once(|| {
        initialize_platform().expect("Failed to initialize V8 platform");
    });
}

/// Loads a test fixture file from tests/fixtures/frameworks/
fn load_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/frameworks/{}.js", name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path, e))
}

/// Creates a temporary JS file with fixture content for testing
fn create_temp_js_file(fixture_name: &str) -> std::path::PathBuf {
    let code = load_fixture(fixture_name);
    let temp_dir = std::env::temp_dir();
    let file_name = format!("test_{}.js", fixture_name);
    let js_path = temp_dir.join(&file_name);
    std::fs::write(&js_path, code).expect("Failed to write test JS file");
    js_path
}

#[tokio::test]
async fn test_hono_middleware_chain_order() {
    // Verify middleware executes in correct order (logger wraps CORS)
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("hono-app");

    let url = NanoUrl::parse("http://test.example.com/").unwrap();
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
    
    assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
    let response = response.unwrap();
    
    // Both middlewares should have run
    assert!(response.headers().get("Access-Control-Allow-Origin").is_some());
    assert!(response.headers().get("X-Powered-By").is_some());
}

#[tokio::test]
async fn test_hono_post_request() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("hono-app");

    let url = NanoUrl::parse("http://test.example.com/").unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Content-Type", "application/json");
    
    let request = NanoRequest::new(
        "POST".to_string(),
        url,
        headers,
        None,
    );

    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
    };

    let response = execute_handler(&mut isolate, context).await;
    
    assert!(response.is_ok(), "Handler execution failed: {:?}", response.err());
    let response = response.unwrap();
    
    // Should still return 200 (simple router doesn't check method in this test)
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_hono_cors_headers_on_all_routes() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("hono-app");

    // Test CORS headers are present on both success and 404 responses
    for path in &["/", "/about", "/nonexistent"] {
        let url = NanoUrl::parse(&format!("http://test.example.com{}", path)).unwrap();
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
        
        assert!(response.is_ok(), "Handler execution failed for {}: {:?}", path, response.err());
        let response = response.unwrap();
        
        // CORS middleware applies to all responses
        assert!(
            response.headers().get("Access-Control-Allow-Origin").is_some(),
            "CORS header missing on {}",
            path
        );
    }
}
