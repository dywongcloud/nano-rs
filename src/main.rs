use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured JSON logging with env-filter support
    nano::logging::init_logging();

    tracing::info!("NANO Edge Runtime starting...");

    // Initialize V8 platform (once per process)
    nano::v8::platform::initialize_platform()?;
    tracing::info!("V8 platform initialized");

    // Set up graceful shutdown with signal handling
    // This creates a shutdown coordinator that listens for SIGTERM/SIGINT
    // and provides graceful shutdown with request draining
    let drain = nano::app::drain::RequestDrain::new();
    let (shutdown, mut shutdown_rx) = nano::signal::setup_shutdown(
        nano::signal::ShutdownConfig::default(),
        drain,
    );
    tracing::info!("Graceful shutdown initialized (timeout: {}s)", shutdown.state().drain().active_count());

    // Create app registry for sharing between main server and admin API
    let registry = Arc::new(RwLock::new(nano::app::registry::AppRegistry::default()));

    // Start HTTP server with virtual host routing and graceful shutdown
    let config = nano::http::ServerConfig::default();
    tracing::info!("Starting HTTP server on {}", config.socket_addr()?);

    // Clone shutdown state for passing to server
    let shutdown_state = shutdown.state().clone();

    // Spawn the main HTTP server in a separate task with graceful shutdown support
    let server_handle = tokio::spawn(async move {
        nano::http::start_server_with_state(config, shutdown_state).await
    });

    // Start Admin API server (optional - only if API key is configured)
    let admin_api_key = std::env::var("NANO_ADMIN_API_KEY").unwrap_or_default();
    let unix_socket_path = std::env::var("NANO_ADMIN_UNIX_SOCKET").ok();
    
    let admin_handle = if !admin_api_key.is_empty() {
        let admin_config = nano::admin::server::AdminConfig::new(admin_api_key);
        let admin_state = nano::admin::server::AdminState::new(registry.clone());

        match nano::admin::server::start_admin_server(admin_config, admin_state).await {
            Ok(admin_server) => {
                tracing::info!("Admin API server started on {}", admin_server.local_addr);
                Some(admin_server)
            }
            Err(e) => {
                tracing::error!("Failed to start Admin API server: {}", e);
                None
            }
        }
    } else {
        tracing::info!("Admin API server not started (NANO_ADMIN_API_KEY not set)");
        None
    };

    // Start Unix socket admin server (optional - enabled via NANO_ADMIN_UNIX_SOCKET)
    let unix_socket_handle = if let Some(socket_path) = unix_socket_path {
        let unix_config = nano::admin::unix_socket::UnixSocketConfig::new(socket_path);
        let unix_auth = std::sync::Arc::new(nano::admin::auth::AdminAuth::new("unix-socket-unused"));
        let unix_state = nano::admin::server::AdminState::new(registry.clone());

        match nano::admin::unix_socket::start_unix_socket_server(unix_config, unix_state, unix_auth).await {
            Ok(unix_server) => {
                tracing::info!("Unix socket admin server started at {}", unix_server.socket_path().display());
                Some(unix_server)
            }
            Err(e) => {
                tracing::error!("Failed to start Unix socket admin server: {}", e);
                None
            }
        }
    } else {
        tracing::info!("Unix socket admin server not started (NANO_ADMIN_UNIX_SOCKET not set)");
        None
    };

    // Wait for shutdown signal
    tracing::info!("Waiting for shutdown signal (SIGTERM or Ctrl+C)...");
    let _ = shutdown_rx.recv().await;
    tracing::info!("Shutdown signal received, initiating graceful shutdown...");

    // Perform graceful shutdown
    shutdown.shutdown().await;

    // Shut down admin servers if running
    if let Some(admin_server) = admin_handle {
        tracing::info!("Shutting down Admin API server...");
        admin_server.shutdown().await;
    }

    if let Some(unix_server) = unix_socket_handle {
        tracing::info!("Shutting down Unix socket admin server...");
        unix_server.shutdown().await;
    }

    // Wait for main server to complete
    match server_handle.await {
        Ok(result) => {
            if let Err(e) = result {
                tracing::error!("Server error during shutdown: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("Server task panicked: {}", e);
        }
    }

    tracing::info!("NANO Edge Runtime shutdown complete");

    Ok(())
}
