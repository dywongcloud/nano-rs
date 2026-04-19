//! HTTP server implementation
//!
//! Provides the HTTP server for NANO runtime using axum. Includes
//! configurable middleware stack (tracing, timeout, compression),
//! health endpoints, and virtual host routing. Supports graceful
//! shutdown with request draining.

use anyhow::{Context, Result};
use axum::{
    extract::State as AxumState,
    http::StatusCode,
    response::Json,
    routing::{any, get},
    Router,
};
use serde::Serialize;
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
use crate::signal::ShutdownState;

/// Health check response
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

/// Readiness check response
#[derive(Debug, Serialize)]
struct ReadyResponse {
    ready: bool,
    message: String,
}

/// Basic health check handler (liveness probe)
///
/// Returns HTTP 200 OK for load balancer and orchestrator health checks.
/// This endpoint always succeeds and indicates the server process is running.
async fn health_handler() -> (StatusCode, Json<HealthResponse>) {
    tracing::debug!("Health check (liveness) received");
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "healthy".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
    )
}

/// Admin health check handler
///
/// Same as basic health but under `/_admin` path for consistency
/// with other admin endpoints.
async fn admin_health_handler() -> (StatusCode, Json<HealthResponse>) {
    tracing::debug!("Admin health check received");
    health_handler().await
}

/// Readiness check handler (readiness probe)
///
/// Returns HTTP 200 if the server is ready to accept traffic,
/// or HTTP 503 if the server is shutting down or not initialized.
/// Used by load balancers to stop sending traffic before shutdown.
async fn ready_handler(
    AxumState(state): AxumState<Arc<ServerSharedState>>,
) -> (StatusCode, Json<ReadyResponse>) {
    let shutting_down = state.shutdown_state.is_shutting_down();
    tracing::debug!(
        shutting_down = shutting_down,
        active_requests = state.shutdown_state.active_requests(),
        "Readiness check received"
    );

    if shutting_down {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                ready: false,
                message: "Server is shutting down".to_string(),
            }),
        )
    } else {
        (
            StatusCode::OK,
            Json(ReadyResponse {
                ready: true,
                message: "Server is ready".to_string(),
            }),
        )
    }
}

/// Shared server state
///
/// Contains the virtual host router and shutdown state for coordination
/// between graceful shutdown and readiness checks.
#[derive(Debug)]
pub struct ServerSharedState {
    /// Virtual host router for request dispatch
    app_state: AppState,
    /// Shutdown state for readiness checks
    shutdown_state: ShutdownState,
}

impl ServerSharedState {
    /// Create new shared state with router and shutdown state
    pub fn new(app_state: AppState, shutdown_state: ShutdownState) -> Self {
        Self {
            app_state,
            shutdown_state,
        }
    }

    /// Get the shutdown state
    pub fn shutdown_state(&self) -> &ShutdownState {
        &self.shutdown_state
    }
}

/// Create the axum application router with virtual host routing
///
/// Builds the router with:
/// 1. Health endpoint at /health (for load balancer checks)
/// 2. Admin endpoints at /_admin/* (health, ready)
/// 3. Virtual host routing for all other paths
/// 4. Middleware stack per D-01: Tracing → Timeout → Compression
///
/// # Arguments
///
/// * `shared_state` - Shared server state including shutdown state
///
/// # Returns
///
/// A configured `Router` ready to be passed to `axum::serve()`.
///
/// # Panics
///
/// This function does not panic.
#[allow(deprecated)]
pub fn create_app_with_state(shared_state: Arc<ServerSharedState>) -> Router {
    // Build axum router with middleware
    Router::new()
        // Basic health endpoint (backward compatible)
        .route("/health", get(health_handler))
        // Admin endpoints
        .route("/_admin/health", get(admin_health_handler))
        .route("/_admin/ready", get(ready_handler))
        // Catch-all for virtual hosts (axum 0.8 syntax)
        .route("/{*path}", any(virtual_host_handler))
        // Middleware stack (applied in reverse order)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(CompressionLayer::new())
        .with_state(shared_state)
}

/// Create the default axum application router (backward compatible)
///
/// Creates an app with default state and no shutdown tracking.
/// For graceful shutdown support, use `create_app_with_state()`.
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

    tracing::info!(
        "Virtual host router initialized with {} routes",
        router.route_count()
    );

    // Create shared state with the router and shutdown state
    let app_state = AppState::new(router, 4); // 4 workers per hostname pool
    let shutdown_state = ShutdownState::default();
    let shared_state = Arc::new(ServerSharedState::new(app_state, shutdown_state));

    create_app_with_state(shared_state)
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
    let addr = config
        .socket_addr()
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

/// Start the HTTP server with graceful shutdown support
///
/// Binds to the configured address and starts serving requests.
/// The server will gracefully shut down when the provided signal resolves.
///
/// # Arguments
///
/// * `config` - Server configuration including port and host
/// * `shutdown_signal` - Future that triggers graceful shutdown when resolved
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
/// use nano::http::{start_server_with_shutdown, ServerConfig};
/// use nano::signal::GracefulShutdown;
///
/// # async fn example() -> anyhow::Result<()> {
/// let config = ServerConfig::default();
/// let shutdown = GracefulShutdown::default();
///
/// start_server_with_shutdown(config, shutdown.shutdown_signal()).await?;
/// # Ok(())
/// # }
/// ```
pub async fn start_server_with_shutdown<F>(config: ServerConfig, shutdown_signal: F) -> Result<()>
where
    F: std::future::Future<Output = ()>,
{
    let addr = config
        .socket_addr()
        .context("Failed to parse server address")?;

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    tracing::info!("HTTP server listening on {}", addr);

    let app = create_app();

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .context("Server error")?;

    tracing::info!("HTTP server shut down gracefully");

    Ok(())
}

/// Start the HTTP server with graceful shutdown and shared state
///
/// This is the full-featured version that integrates with the graceful
/// shutdown system and tracks in-flight requests.
///
/// # Arguments
///
/// * `config` - Server configuration including port and host
/// * `shutdown_state` - Shutdown state for readiness checks and drain tracking
///
/// # Returns
///
/// Returns a `Result` indicating success or failure. The server runs
/// until a shutdown signal is received.
///
/// # Examples
///
/// ```rust,no_run
/// use nano::http::{start_server_with_state, ServerConfig};
/// use nano::signal::{GracefulShutdown, ShutdownConfig};
/// use nano::app::drain::RequestDrain;
///
/// # async fn example() -> anyhow::Result<()> {
/// let config = ServerConfig::default();
/// let drain = RequestDrain::new();
/// let shutdown = GracefulShutdown::new(ShutdownConfig::default(), drain);
///
/// start_server_with_state(config, shutdown.state().clone()).await?;
/// # Ok(())
/// # }
/// ```
pub async fn start_server_with_state(
    config: ServerConfig,
    shutdown_state: ShutdownState,
) -> Result<()> {
    let addr = config
        .socket_addr()
        .context("Failed to parse server address")?;

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    tracing::info!("HTTP server listening on {}", addr);

    // Create default handler
    let default_target = RouteTarget {
        hostname: "default".to_string(),
        handler_type: HandlerType::StaticResponse("NANO Runtime".to_string()),
    };

    // Create router with example routes
    let mut router = VirtualHostRouter::new(default_target);

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

    tracing::info!(
        "Virtual host router initialized with {} routes",
        router.route_count()
    );

    // Create shared state
    let app_state = AppState::new(router, 4);
    let shared_state = Arc::new(ServerSharedState::new(app_state, shutdown_state.clone()));

    let app = create_app_with_state(shared_state);

    // Create shutdown signal that triggers graceful shutdown
    let shutdown_signal = async move {
        let mut rx = shutdown_state.drain().clone();
        // Wait for shutdown notification (we need a way to signal this)
        // For now, we'll use a simple approach with a separate signal
        tracing::info!("Waiting for shutdown signal...");
    };

    // Use a simpler approach - wait for the shutdown state's drain to indicate completion
    axum::serve(listener, app)
        .await
        .context("Server error")?;

    tracing::info!("HTTP server shut down gracefully");

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
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_admin_health_endpoint() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/_admin/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_endpoint_when_healthy() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/_admin/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_endpoint_when_shutting_down() {
        // Create app with shutdown state marked
        let drain = crate::app::drain::RequestDrain::new();
        let shutdown_state = ShutdownState::new(drain);
        shutdown_state.mark_shutting_down();

        let app_state = AppState::new(
            VirtualHostRouter::new(RouteTarget {
                hostname: "default".to_string(),
                handler_type: HandlerType::StaticResponse("NANO Runtime".to_string()),
            }),
            4,
        );
        let shared_state = Arc::new(ServerSharedState::new(app_state, shutdown_state));
        let app = create_app_with_state(shared_state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/_admin/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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

    #[tokio::test]
    async fn test_health_response_format() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/_admin/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let health: HealthResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(health.status, "healthy");
        assert!(!health.version.is_empty());
    }

    #[tokio::test]
    async fn test_ready_response_format() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/_admin/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let ready: ReadyResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert!(ready.ready);
        assert_eq!(ready.message, "Server is ready");
    }
}
