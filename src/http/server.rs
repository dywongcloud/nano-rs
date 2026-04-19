//! HTTP server implementation
//!
//! Provides the HTTP server for NANO runtime using axum. Includes
//! configurable middleware stack (tracing, timeout, compression),
//! health endpoint, and virtual host routing.

use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    routing::{any, get},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::http::config::ServerConfig;
use crate::http::router::{virtual_host_handler, AppState, HandlerType, RouteTarget, VirtualHostRouter};

/// Application state shared across all request handlers
///
/// Currently empty but will hold shared resources in future phases
/// such as the worker pool, virtual host registry, and metrics.
#[derive(Debug)]
pub struct State {
    // Future fields:
    // - worker_pool: Arc<WorkerPool>
    // - virtual_hosts: Arc<RwLock<HashMap<String, App>>>
    // - metrics: Arc<Metrics>
}

impl State {
    /// Creates a new empty application state
    ///
    /// This is a placeholder for future state initialization
    /// once the worker pool and virtual host registry are implemented.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check handler
///
/// Returns HTTP 200 OK for load balancer and orchestrator health checks.
/// This endpoint indicates the server is running and accepting connections.
async fn health_handler() -> StatusCode {
    tracing::debug!("Health check received");
    StatusCode::OK
}

/// Create the axum application router with virtual host routing
///
/// Builds the router with:
/// 1. Health endpoint at /health (for load balancer checks)
/// 2. Virtual host routing for all other paths
/// 3. Middleware stack per D-01: Tracing → Timeout → Compression
///
/// # Returns
///
/// A configured `Router` ready to be passed to `axum::serve()`.
///
/// # Panics
///
/// This function does not panic.
#[allow(deprecated)]
pub fn create_app() -> Router {
    // Create default handler (per D-04)
    let default_target = RouteTarget {
        hostname: "default".to_string(),
        handler_type: HandlerType::StaticResponse("NANO Runtime".to_string()),
    };

    // Create router with example routes for testing
    let mut router = VirtualHostRouter::new(default_target);

    // Register example routes (will be configurable in Phase 5)
    router.register(
        "api.example.com".to_string(),
        RouteTarget {
            hostname: "api.example.com".to_string(),
            handler_type: HandlerType::StaticResponse("API Handler".to_string()),
        },
    );

    router.register(
        "blog.example.com".to_string(),
        RouteTarget {
            hostname: "blog.example.com".to_string(),
            handler_type: HandlerType::StaticResponse("Blog Handler".to_string()),
        },
    );

    tracing::info!("Virtual host router initialized with {} routes", router.route_count());

    // Create shared state with the router
    let state = Arc::new(AppState { router });

    // Build axum router with middleware
    Router::new()
        .route("/health", get(health_handler))  // Keep health endpoint
        .route("/{*path}", any(virtual_host_handler))  // Catch-all for virtual hosts (axum 0.8 syntax)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(CompressionLayer::new())
        .with_state(state)
}

/// Start the HTTP server
///
/// Binds to the configured address and starts serving requests.
/// This function runs until the server is shut down (e.g., via SIGTERM).
///
/// # Arguments
///
/// * `config` - Server configuration including port and host
///
/// # Errors
///
/// Returns an error if:
/// - The TCP listener cannot be bound to the configured address
/// - The server encounters an error while running
///
/// # Examples
///
/// ```rust,no_run
/// use nano::http::{start_server, ServerConfig};
///
/// # async fn example() -> anyhow::Result<()> {
/// let config = ServerConfig::default();
/// start_server(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn start_server(config: ServerConfig) -> Result<()> {
    let addr = config.socket_addr()
        .context("Failed to parse server address")?;

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    tracing::info!("HTTP server listening on {}", addr);

    let app = create_app();

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_app();

        let response = app
            .oneshot(Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
        
        // Verify socket_addr works
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_state_creation() {
        let _state = State::new();
        let _arc_state = Arc::new(State::new());
        // Just verify State can be created and wrapped in Arc
    }

    #[tokio::test]
    async fn test_app_creation() {
        let app = create_app();
        // Verify the app can be created without panicking
        let _ = app;
    }
}
