//! HTTP Request Handler for Sliver-based JS Execution
//!
//! This module bridges HTTP requests to the SliverWorkerPool for
//! WinterCG-compatible JavaScript execution from heap snapshots.

use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::http::{NanoHeaders, NanoRequest, NanoResponse};
use crate::worker::HandlerTask;

/// State for sliver-based request handling
#[derive(Clone)]
pub struct SliverHandlerState {
    /// The worker pool containing snapshot-restored isolates
    pub worker_pool: std::sync::Arc<crate::worker::SliverWorkerPool>,
    /// The JS entrypoint (e.g., "index.js" or "app.js")
    pub entrypoint: String,
}

/// Handle HTTP request by dispatching to sliver worker pool
///
/// This handler:
/// 1. Converts axum request to NanoRequest (WinterCG format)
/// 2. Creates a HandlerTask with the request
/// 3. Dispatches to SliverWorkerPool
/// 4. Waits for JS execution result
/// 5. Returns NanoResponse as HTTP response
pub async fn sliver_js_handler(
    State(state): State<SliverHandlerState>,
    request: Request<Body>,
) -> Response<Body> {
    let start = std::time::Instant::now();
    
    // Extract request components
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    
    // Read body (with 1MB limit)
    let body_bytes = match axum::body::to_bytes(request.into_body(), 1048576).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Bad request: failed to read body"))
                .unwrap();
        }
    };
    
    // Build full URL from request
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let path_and_query = uri.path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let full_url = format!("http://{}{}", host, path_and_query);
    
    // Parse URL for NanoRequest
    let nano_url = match crate::http::NanoUrl::parse(&full_url) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to parse URL '{}': {}", full_url, e);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Bad request: invalid URL"))
                .unwrap();
        }
    };
    
    // Convert headers
    let nano_headers = NanoHeaders::from_axum_headers(&headers);
    
    // Create NanoRequest
    let nano_request = NanoRequest::new(
        method.to_string(),
        nano_url,
        nano_headers,
        if body_bytes.is_empty() { None } else { Some(body_bytes) },
    );
    
    // Create oneshot channel for response
    let (tx, rx) = oneshot::channel();
    
    // Create handler task with hostname for metrics tracking
    let task = HandlerTask::with_hostname(
        state.entrypoint.clone(),
        nano_request,
        tx,
        state.worker_pool.hostname.clone(),
    );
    
    // Dispatch to worker pool
    if let Err(e) = state.worker_pool.dispatch(task) {
        tracing::error!("Failed to dispatch to worker pool: {}", e);
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::from(format!("Service unavailable: {}", e)))
            .unwrap();
    }
    
    // Wait for response
    let nano_response = match rx.await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            tracing::error!("Handler returned error: {}", e);
            NanoResponse::with_status(500)
                .with_header("Content-Type", "text/plain")
                .with_body(format!("Handler error: {}", e))
        }
        Err(_) => {
            tracing::error!("Handler channel closed unexpectedly");
            NanoResponse::with_status(500)
                .with_header("Content-Type", "text/plain")
                .with_body("Internal error: handler channel closed")
        }
    };
    
    // Convert to axum response
    let axum_response = nano_response.to_axum_response();
    
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    tracing::info!(
        "Sliver JS handler completed: {} {} → {} in {:.2}ms",
        method,
        path_and_query,
        nano_response.status(),
        duration_ms
    );
    
    axum_response
}
