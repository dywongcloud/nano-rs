//! Next.js Static Export Integration Tests
//!
//! Tests Next.js static export pattern with page routing and asset serving.

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

fn load_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/frameworks/{}.js", name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path, e))
}

fn create_temp_js_file(fixture_name: &str) -> std::path::PathBuf {
    let code = load_fixture(fixture_name);
    let temp_dir = std::env::temp_dir();
    let file_name = format!("test_{}.js", fixture_name);
    let js_path = temp_dir.join(&file_name);
    std::fs::write(&js_path, code).expect("Failed to write test JS file");
    js_path
}

#[tokio::test]
async fn test_nextjs_home_page() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/").unwrap();
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
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("Content-Type"),
        Some("text/html; charset=utf-8".to_string())
    );
    assert_eq!(
        response.headers().get("X-Nextjs-Static"),
        Some("true".to_string())
    );
}

#[tokio::test]
async fn test_nextjs_about_page() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/about").unwrap();
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
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_nextjs_blog_post() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/blog/hello-world").unwrap();
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
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_nextjs_404() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/nonexistent-page").unwrap();
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
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.headers().get("X-Nextjs-Static"),
        Some("true".to_string())
    );
}

#[tokio::test]
async fn test_nextjs_static_css_asset() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/_next/static/css/pages/index.css").unwrap();
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
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("Content-Type"),
        Some("text/css".to_string())
    );
}

#[tokio::test]
async fn test_nextjs_static_js_asset() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("nextjs-static-app");

    let url = NanoUrl::parse("http://nextjs.example.com/_next/static/js/pages/index.js").unwrap();
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
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("Content-Type"),
        Some("application/javascript".to_string())
    );
}
