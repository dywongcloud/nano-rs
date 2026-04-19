use anyhow::Result;

fn main() -> Result<()> {
    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt::init();

    tracing::info!("NANO Edge Runtime starting...");

    // Run the library entry point
    nano_rs::run()?;

    Ok(())
}
