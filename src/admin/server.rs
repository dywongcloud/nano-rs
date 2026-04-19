//! Admin HTTP server for NANO Edge Runtime
//!
//! Provides a separate HTTP server on port 8889 (default) for administrative
//! operations including app management, diagnostics, and metrics.
//!
//! # Security
//!
//! - All endpoints except /health and /ready require API key authentication
//! - API key is expected in the X-Admin-Key header
//! - Server runs on a separate port from the main HTTP traffic
//!
//! # Endpoints
//!
//! Public (no auth):
//! - `GET /admin/health` - Liveness probe
//! - `GET /admin/ready` - Readiness probe
//!
//! Protected (requires X-Admin-Key):
//! - `GET /admin/isolates` - List active isolates
//! - `GET /admin/apps` - List all apps
//! - `POST /admin/apps` - Create new app
//! - `GET /admin/apps/:host` - Get app by hostname
//! - `PATCH /admin/apps/:host` - Update app
//! - `DELETE /admin/apps/:host` - Delete app
//! - `POST /admin/apps/:host/activate` - Activate pending app
//! - `POST /admin/apps/:host/disable` - Disable app
//! - `POST /admin/apps/:host/enable` - Enable disabled app
//! - `POST /admin/apps/:host/reload` - Reload JS from disk
//! - `POST /admin/apps/:host/scale` - Adjust worker count
//! - `POST /admin/apps/:host/drain` - Drain and disable
//! - `GET /admin/metrics` - Prometheus metrics

use axum::{
    routing::{delete, get, patch, post},
    middleware,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;

use crate::admin::auth::{api_key_middleware, AdminAuth};
use crate::admin::handlers::{
    activate_app, create_app, delete_app, disable_app, drain_app, enable_app,
    get_app, health_handler, list_apps, list_isolates, ready_handler, reload_app,
    scale_app, update_app,
};
use crate::admin::metrics::metrics_handler;
use crate::app::registry::AppRegistry;
use crate::metrics::MetricsRegistry;

/// Admin server configuration
///
/// Configuration for the admin HTTP server including port, API key,
/// and optional Unix socket path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Port to bind the admin server (default: 8889)
    #[serde(default = "default_admin_port")]
    pub port: u16,

    /// Bind address (default: "0.0.0.0")
    #[serde(default = "default_admin_host")]
    pub host: String,

    /// API key for authentication (required, minimum 32 chars recommended)
    pub api_key: String,

    /// Path to TLS certificate (optional, for HTTPS)
    #[serde(default)]
    pub tls_cert_path: Option<String>,

    /// Path to TLS key (optional, for HTTPS)
    #[serde(default)]
    pub tls_key_path: Option<String>,
}

fn default_admin_port() -> u16 {
    8889
}

fn default_admin_host() -> String {
    "0.0.0.0".to_string()
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            port: default_admin_port(),
            host: default_admin_host(),
            api_key: String::new(),
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

impl AdminConfig {
    /// Create a new admin config with the specified API key
    ///
    /// # Arguments
    ///
    /// * `api_key` - The API key for authentication
    ///
    /// # Example
    ///
    /// ```rust
    /// use nano::admin::server::AdminConfig;
    ///
    /// let config = AdminConfig::new("my-secret-api-key-32-chars-min");
    /// assert_eq!(config.port, 8889);
    /// ```
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Validate the configuration
    ///
    /// Returns an error if:
    /// - API key is empty
    /// - API key is shorter than 32 characters (warning)
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, `Err(String)` with error message if invalid
    pub fn validate(&self) -> Result<(), String> {
        if self.api_key.is_empty() {
            return Err("Admin API key is required".to_string());
        }

        if self.api_key.len() < 32 {
            tracing::warn!(
                "Admin API key is only {} characters (32+ recommended)",
                self.api_key.len()
            );
        }

        Ok(())
    }

    /// Parse the host and port into a SocketAddr
    pub fn socket_addr(&self) -> Result<SocketAddr, String> {
        let addr_str = format!("{}:{}", self.host, self.port);
        addr_str
            .parse::<SocketAddr>()
            .map_err(|e| format!("Failed to parse admin socket address: {}", e))
    }
}

/// Shared state for admin server handlers
#[derive(Debug, Clone)]
pub struct AdminState {
    /// Application registry for managing apps
    pub registry: Arc<RwLock<AppRegistry>>,
    /// Metrics registry for exposing metrics
    pub metrics: Arc<MetricsRegistry>,
    /// Shutdown flag (when true, readiness returns 503)
    pub shutting_down: Arc<std::sync::atomic::AtomicBool>,
}

impl AdminState {
    /// Create new admin state
    pub fn new(registry: Arc<RwLock<AppRegistry>>) -> Self {
        Self {
            registry,
            metrics: Arc::new(MetricsRegistry::new()),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Check if the server is shutting down
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Mark the server as shutting down
    pub fn mark_shutting_down(&self) {
        self.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Admin server handle
///
/// Returned when starting the admin server, can be used to
/// gracefully shut down the server.
pub struct AdminServer {
    /// The shutdown signal sender
    shutdown_tx: tokio::sync::watch::Sender<()>,
    /// The local address the server is bound to
    pub local_addr: SocketAddr,
}

impl AdminServer {
    /// Trigger graceful shutdown
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Create the admin router with all endpoints
///
/// Builds the axum Router with:
/// 1. Public routes (health, ready) - no auth required
/// 2. Protected routes - API key authentication required
///
/// # Arguments
///
/// * `auth` - The AdminAuth state for API key validation
/// * `state` - Shared admin state (registry, metrics, etc.)
///
/// # Returns
///
/// A configured `Router` ready to be served.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::sync::RwLock;
/// use nano::admin::server::{create_admin_router, AdminConfig, AdminState};
/// use nano::admin::auth::AdminAuth;
/// use nano::app::registry::AppRegistry;
///
/// let config = AdminConfig::new("my-secret-key");
/// let auth = Arc::new(AdminAuth::new(config.api_key));
/// let registry = Arc::new(RwLock::new(AppRegistry::default()));
/// let state = AdminState::new(registry);
/// let router = create_admin_router(auth, state);
/// ```
pub fn create_admin_router(auth: Arc<AdminAuth>, state: AdminState) -> Router {
    // Public routes - no authentication required
    let public_routes = Router::new()
        .route("/admin/health", get(health_handler))
        .route("/admin/ready", get({
            let state = state.clone();
            move || ready_with_state(state.clone())
        }));

    // Protected routes - API key authentication required
    let protected_routes = Router::new()
        // Isolates
        .route("/admin/isolates", get({
            let state = state.clone();
            move |req| list_isolates_with_state(state.clone(), req)
        }))
        // Apps
        .route("/admin/apps", get({
            let state = state.clone();
            move |req| list_apps_with_state(state.clone(), req)
        }).post({
            let state = state.clone();
            move |req, body| create_app_with_state(state.clone(), req, body)
        }))
        .route(
            "/admin/apps/:hostname",
            get({
                let state = state.clone();
                move |path, req| get_app_with_state(state.clone(), path, req)
            })
            .patch({
                let state = state.clone();
                move |path, req, body| update_app_with_state(state.clone(), path, req, body)
            })
            .delete({
                let state = state.clone();
                move |path, req| delete_app_with_state(state.clone(), path, req)
            }),
        )
        // App lifecycle actions
        .route("/admin/apps/:hostname/activate", post({
            let state = state.clone();
            move |path, req| activate_app_with_state(state.clone(), path, req)
        }))
        .route("/admin/apps/:hostname/disable", post({
            let state = state.clone();
            move |path, req| disable_app_with_state(state.clone(), path, req)
        }))
        .route("/admin/apps/:hostname/enable", post({
            let state = state.clone();
            move |path, req| enable_app_with_state(state.clone(), path, req)
        }))
        .route("/admin/apps/:hostname/reload", post({
            let state = state.clone();
            move |path, req| reload_app_with_state(state.clone(), path, req)
        }))
        .route("/admin/apps/:hostname/scale", post({
            let state = state.clone();
            move |path, req, body| scale_app_with_state(state.clone(), path, req, body)
        }))
        .route("/admin/apps/:hostname/drain", post({
            let state = state.clone();
            move |path, req| drain_app_with_state(state.clone(), path, req)
        }))
        // Metrics
        .route("/admin/metrics", get(metrics_handler))
        // Apply auth middleware to protected routes
        .layer(middleware::from_fn_with_state(auth, api_key_middleware));

    // Combine routes and add tracing
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
}

// Wrapper functions to inject state

async fn ready_with_state(state: AdminState) -> impl axum::response::IntoResponse {
    use crate::admin::handlers::ready_handler_with_state;
    ready_handler_with_state(state.is_shutting_down()).await
}

async fn list_isolates_with_state(
    state: AdminState,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    list_isolates(axum::extract::State(state.registry)).await
}

async fn list_apps_with_state(
    state: AdminState,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    list_apps(axum::extract::State(state.registry)).await
}

async fn create_app_with_state(
    state: AdminState,
    _req: axum::extract::Request,
    body: axum::extract::Json<crate::admin::handlers::CreateAppRequest>,
) -> impl axum::response::IntoResponse {
    create_app(axum::extract::State(state.registry), body).await
}

async fn get_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    get_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn update_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
    body: axum::extract::Json<crate::admin::handlers::UpdateAppRequest>,
) -> impl axum::response::IntoResponse {
    update_app(axum::extract::Path(hostname), axum::extract::State(state.registry), body).await
}

async fn delete_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    delete_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn activate_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    activate_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn disable_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    disable_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn enable_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    enable_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn reload_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    reload_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

async fn scale_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
    body: axum::extract::Json<crate::admin::handlers::ScaleRequest>,
) -> impl axum::response::IntoResponse {
    scale_app(axum::extract::Path(hostname), axum::extract::State(state.registry), body).await
}

async fn drain_app_with_state(
    state: AdminState,
    axum::extract::Path(hostname): axum::extract::Path<String>,
    _req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    drain_app(axum::extract::Path(hostname), axum::extract::State(state.registry)).await
}

/// Start the admin server
///
/// Binds to the configured address and starts serving admin API requests.
/// The server runs until a shutdown signal is received.
///
/// # Arguments
///
/// * `config` - Admin server configuration
/// * `state` - Shared admin state
///
/// # Returns
///
/// `Ok(AdminServer)` on successful bind, `Err(String)` on failure.
///
/// # Example
///
/// ```rust,no_run
/// use nano::admin::server::{AdminConfig, AdminState, start_admin_server};
/// use nano::app::registry::AppRegistry;
/// use std::sync::Arc;
/// use tokio::sync::RwLock;
///
/// # async fn example() {
/// let config = AdminConfig::new("my-secret-key");
/// let registry = Arc::new(RwLock::new(AppRegistry::default()));
/// let state = AdminState::new(registry);
/// let server = start_admin_server(config, state).await.unwrap();
/// # }
/// ```
pub async fn start_admin_server(
    config: AdminConfig,
    state: AdminState,
) -> Result<AdminServer, String> {
    // Validate configuration
    config.validate()?;

    let addr = config.socket_addr()?;

    // Create listener
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind admin server to {}: {}", addr, e))?;

    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?;

    tracing::info!("Admin API server listening on {}", local_addr);

    // Create auth and router
    let auth = Arc::new(AdminAuth::new(config.api_key));
    let router = create_admin_router(auth, state);

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());

    // Spawn server task
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
                tracing::info!("Admin API server shutting down");
            })
            .await
            .unwrap_or_else(|e| {
                tracing::error!("Admin server error: {}", e);
            });
    });

    Ok(AdminServer {
        shutdown_tx,
        local_addr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::registry::AppRegistry;

    #[test]
    fn test_admin_config_default() {
        let config = AdminConfig::default();
        assert_eq!(config.port, 8889);
        assert_eq!(config.host, "0.0.0.0");
        assert!(config.api_key.is_empty());
        assert!(config.tls_cert_path.is_none());
        assert!(config.tls_key_path.is_none());
    }

    #[test]
    fn test_admin_config_new() {
        let config = AdminConfig::new("my-secret-key-32-chars-min");
        assert_eq!(config.api_key, "my-secret-key-32-chars-min");
        assert_eq!(config.port, 8889);
    }

    #[test]
    fn test_admin_config_builder() {
        let config = AdminConfig::new("key")
            .with_port(9999)
            .with_host("127.0.0.1");

        assert_eq!(config.port, 9999);
        assert_eq!(config.host, "127.0.0.1");
    }

    #[test]
    fn test_admin_config_validate_empty_key() {
        let config = AdminConfig::default();
        assert!(config.validate().is_err());
        assert!(config.validate().unwrap_err().contains("required"));
    }

    #[test]
    fn test_admin_config_validate_short_key() {
        let config = AdminConfig::new("short");
        assert!(config.validate().is_ok()); // Short key is allowed but warned
    }

    #[test]
    fn test_admin_config_socket_addr() {
        let config = AdminConfig::default();
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 8889);
        assert!(addr.ip().is_unspecified());
    }

    #[test]
    fn test_admin_state_new() {
        let registry = Arc::new(RwLock::new(AppRegistry::default()));
        let state = AdminState::new(registry);
        assert!(!state.is_shutting_down());
    }

    #[test]
    fn test_admin_state_shutdown() {
        let registry = Arc::new(RwLock::new(AppRegistry::default()));
        let state = AdminState::new(registry);
        state.mark_shutting_down();
        assert!(state.is_shutting_down());
    }

    #[test]
    fn test_create_admin_router() {
        let auth = Arc::new(AdminAuth::new("test-key"));
        let registry = Arc::new(RwLock::new(AppRegistry::default()));
        let state = AdminState::new(registry);
        let router = create_admin_router(auth, state);

        // Router should be created without panicking
        assert!(true);
    }

    #[tokio::test]
    async fn test_admin_config_from_env() {
        // This test just verifies the config can be created
        // Real env var testing would require setting env vars
        let config = AdminConfig::new("test-key-from-env");
        assert_eq!(config.api_key, "test-key-from-env");
    }
}
