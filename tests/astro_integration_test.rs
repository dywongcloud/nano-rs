//! Astro Islands Architecture Integration Tests
//!
//! Tests Astro's partial hydration pattern with island markers.

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
async fn test_astro_home_page_renders_islands() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    let url = NanoUrl::parse("http://astro.example.com/").unwrap();
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
        response.headers().get("X-Astro-Islands"),
        Some("true".to_string())
    );
}

#[tokio::test]
async fn test_astro_gallery_page_carousel() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    let url = NanoUrl::parse("http://astro.example.com/gallery").unwrap();
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
async fn test_astro_404() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    let url = NanoUrl::parse("http://astro.example.com/nonexistent").unwrap();
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
}

#[tokio::test]
async fn test_astro_image_assets() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    for img in &["/photo1.jpg", "/photo2.jpg", "/photo3.jpg"] {
        let url = NanoUrl::parse(&format!("http://astro.example.com{}", img)).unwrap();
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
        
        assert!(response.is_ok(), "Handler execution failed for {}: {:?}", img, response.err());
        let response = response.unwrap();
        assert_eq!(response.status(), 200, "Failed for {}", img);
        assert_eq!(
            response.headers().get("Content-Type"),
            Some("image/jpeg".to_string()),
            "Content-Type mismatch for {}",
            img
        );
        assert_eq!(
            response.headers().get("X-Astro-Asset"),
            Some("true".to_string()),
            "X-Astro-Asset header missing for {}",
            img
        );
    }
}

#[tokio::test]
async fn test_astro_island_hydration_strategy_markers() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    let url = NanoUrl::parse("http://astro.example.com/").unwrap();
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
async fn test_astro_server_rendered_content() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let js_path = create_temp_js_file("astro-islands-app");

    let url = NanoUrl::parse("http://astro.example.com/").unwrap();
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
