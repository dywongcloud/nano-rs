use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured JSON logging with env-filter support
    nano::logging::init_logging();

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
