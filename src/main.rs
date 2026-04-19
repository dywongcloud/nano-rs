use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt::init();

    tracing::info!("NANO Edge Runtime starting...");

    // Initialize V8 platform (once per process)
    nano::v8::platform::initialize_platform()?;
    tracing::info!("V8 platform initialized");

    // Start HTTP server with virtual host routing
    let config = nano::http::ServerConfig::default();
    tracing::info!("Starting HTTP server on {}", config.socket_addr()?);

    nano::http::start_server(config).await?;

    Ok(())
}
