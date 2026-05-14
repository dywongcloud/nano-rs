//! Virtual host routing integration tests
//!
//! Tests the full request routing flow including:
//! - Host-based routing to different handlers
//! - Fallback routing for unknown hosts
//! - Case-insensitive hostname matching

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    response::IntoResponse,
};
use nano::http::router::{virtual_host_handler, AppState, HandlerType, RouteTarget, VirtualHostRouter};
use std::sync::Arc;

/// Setup a test router with sample routes
async fn setup_test_router() -> Arc<AppState> {
    let default = RouteTarget {
        hostname: "default".to_string(),
        handler_type: HandlerType::StaticResponse("default-handler".to_string()),
    };

    let mut router = VirtualHostRouter::new(default);

    // Register test routes
    router.register(
        "api.test.com".to_string(),
        RouteTarget {
            hostname: "api.test.com".to_string(),
            handler_type: HandlerType::StaticResponse("api-handler".to_string()),
        },
    );

    router.register(
        "blog.test.com".to_string(),
        RouteTarget {
            hostname: "blog.test.com".to_string(),
            handler_type: HandlerType::StaticResponse("blog-handler".to_string()),
        },
    );

    Arc::new(AppState::new(router, 4))
}

#[tokio::test]
async fn test_routes_by_host_header() {
    let state = setup_test_router().await;

    // Test api.test.com routing
    let request = Request::builder()
        .uri("/any-path")
        .header("host", "api.test.com")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    // Convert response to check body
    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify the body content matches expected handler
    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "api-handler");
}

#[tokio::test]
async fn test_blog_host_routing() {
    let state = setup_test_router().await;

    // Test blog.test.com routing
    let request = Request::builder()
        .uri("/posts/123")
        .header("host", "blog.test.com")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "blog-handler");
}

#[tokio::test]
async fn test_fallback_routing() {
    let state = setup_test_router().await;

    // Test unknown host falls back to default
    let request = Request::builder()
        .uri("/any-path")
        .header("host", "unknown.test.com")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "default-handler");
}

#[tokio::test]
async fn test_case_insensitive_host() {
    let state = setup_test_router().await;

    // Test case insensitivity - uppercase version of hostname
    let request = Request::builder()
        .uri("/any-path")
        .header("host", "API.TEST.COM")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "api-handler");
}

#[tokio::test]
async fn test_mixed_case_host() {
    let state = setup_test_router().await;

    // Test mixed case hostname
    let request = Request::builder()
        .uri("/any-path")
        .header("host", "Api.Test.Com")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "api-handler");
}

#[tokio::test]
async fn test_javascript_entrypoint_routing() {
    // Test JavaScript entrypoint handler type with a real temp file
    let default = RouteTarget {
        hostname: "default".to_string(),
        handler_type: HandlerType::StaticResponse("default".to_string()),
    };

    let mut router = VirtualHostRouter::new(default);

    // Create a real JS handler file
    let js_code = r#"
        export default {
            async fetch(request) {
                return new Response('Hello from JS handler', { status: 200 });
            }
        };
    "#;
    let temp_dir = std::env::temp_dir();
    let handler_path = temp_dir.join("http_routing_test_handler.js");
    std::fs::write(&handler_path, js_code).unwrap();

    router.register(
        "js.test.com".to_string(),
        RouteTarget {
            hostname: "js.test.com".to_string(),
            handler_type: HandlerType::WinterTCHandler(handler_path.to_str().unwrap().to_string()),
        },
    );

    let state = Arc::new(AppState::new(router, 4));

    let request = Request::builder()
        .uri("/")
        .header("host", "js.test.com")
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert!(body_str.contains("Hello from JS handler"), "Expected JS handler response, got: {}", body_str);
}

#[tokio::test]
async fn test_no_host_header_uses_default() {
    let state = setup_test_router().await;

    // Test request without Host header falls back to default
    let request = Request::builder()
        .uri("/any-path")
        // No host header
        .body(Body::empty())
        .unwrap();

    let response = virtual_host_handler(
        axum::extract::State(state.clone()),
        request,
    )
    .await;

    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "default-handler");
}
