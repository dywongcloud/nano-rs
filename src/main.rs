use anyhow::Result;

fn main() -> Result<()> {
    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt::init();

    tracing::info!("NANO Edge Runtime starting...");

    // Initialize V8 platform (once per process)
    nano::v8::platform::initialize_platform()?;

    // Create isolate with EPT fix sentinel
    let mut isolate = nano::v8::NanoIsolate::new()?;

    // Execute hello.js example
    let code = include_str!("../examples/hello.js");
    nano::v8::execute_script(&mut isolate, code)?;

    Ok(())
}
