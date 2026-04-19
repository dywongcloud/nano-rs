use anyhow::Result;

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

    // Start HTTP server with virtual host routing and graceful shutdown
    let config = nano::http::ServerConfig::default();
    tracing::info!("Starting HTTP server on {}", config.socket_addr()?);

    // Clone shutdown state for passing to server
    let shutdown_state = shutdown.state().clone();

    // Spawn the server in a separate task with graceful shutdown support
    let server_handle = tokio::spawn(async move {
        nano::http::start_server_with_state(config, shutdown_state).await
    });

    // Wait for shutdown signal
    tracing::info!("Waiting for shutdown signal (SIGTERM or Ctrl+C)...");
    let _ = shutdown_rx.recv().await;
    tracing::info!("Shutdown signal received, initiating graceful shutdown...");

    // Perform graceful shutdown
    shutdown.shutdown().await;

    // Wait for server to complete
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
