use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

mod cli;

/// NANO Edge Runtime - Multi-tenant JavaScript edge runtime
#[derive(Debug, Parser)]
#[command(name = "nano-rs")]
#[command(about = "Multi-tenant JavaScript edge runtime (Rust + rusty_v8)")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the NANO HTTP server (default behavior)
    Run {
        /// Configuration file path
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// Sliver file to run directly (conflicts with --config)
        #[arg(long, value_name = "FILE", conflicts_with = "config")]
        sliver: Option<PathBuf>,

        /// Number of workers when using --sliver
        #[arg(short, long, default_value = "4")]
        workers: usize,
    },

    /// Sliver management commands (snapshot creation and management)
    #[command(subcommand)]
    Sliver(cli::SliverCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured JSON logging with env-filter support
    nano::logging::init_logging();

    let cli = Cli::parse();
    
    match cli.command {
        Some(Commands::Run { config, sliver, workers }) => {
            if let Some(sliver_path) = sliver {
                // Run from sliver file
                run_from_sliver(sliver_path, workers).await
            } else if let Some(config_path) = config {
                // Run with config file
                run_server_with_config(config_path).await
            } else {
                // Default behavior: run the server
                run_server().await
            }
        }
        None => {
            // Default behavior: run the server
            run_server().await
        }
        Some(Commands::Sliver(sliver_cmd)) => {
            // Execute sliver command
            handle_sliver_command(sliver_cmd).await
        }
    }
}

/// Run the NANO HTTP server with graceful shutdown
async fn run_server() -> Result<()> {
    tracing::info!("NANO Edge Runtime starting...");

    // Initialize V8 platform (once per process)
    nano::v8::platform::initialize_platform()
        .context("Failed to initialize V8 platform")?;
    tracing::info!("V8 platform initialized");

    // Set up graceful shutdown with signal handling
    let drain = nano::app::drain::RequestDrain::new();
    let (shutdown, mut shutdown_rx) = nano::signal::setup_shutdown(
        nano::signal::ShutdownConfig::default(),
        drain,
    );
    tracing::info!("Graceful shutdown initialized (timeout: {}s)", shutdown.state().drain().active_count());

    // Create app registry for sharing between main server and admin API
    let registry = Arc::new(RwLock::new(nano::app::registry::AppRegistry::default()));

    // Start HTTP server with virtual host routing
    let config = nano::http::ServerConfig::default();
    tracing::info!("Starting HTTP server on {}", config.socket_addr()?);

    let shutdown_state = shutdown.state().clone();
    let server_handle = tokio::spawn(async move {
        nano::http::start_server_with_state(config, shutdown_state).await
    });

    // Start Admin API server (optional)
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

    // Start Unix socket admin server (optional)
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

    // Shut down admin servers
    if let Some(admin_server) = admin_handle {
        tracing::info!("Shutting down Admin API server...");
        admin_server.shutdown().await;
    }

    if let Some(unix_server) = unix_socket_handle {
        tracing::info!("Shutting down Unix socket admin server...");
        unix_server.shutdown().await;
    }

    // Wait for main server
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

/// Run server from a sliver file
///
/// Loads the sliver, extracts the hostname and snapshot, creates a
/// SliverWorkerPool, and starts the HTTP server.
async fn run_from_sliver(sliver_path: PathBuf, workers: usize) -> Result<()> {
    tracing::info!("Starting NANO from sliver: {}", sliver_path.display());

    // Validate sliver file exists
    if !sliver_path.exists() {
        anyhow::bail!("Sliver file not found: {}", sliver_path.display());
    }

    // Initialize V8 platform
    nano::v8::platform::initialize_platform()
        .context("Failed to initialize V8 platform")?;
    tracing::info!("V8 platform initialized");

    // Read and unpack sliver
    let sliver_data = std::fs::read(&sliver_path)
        .with_context(|| format!("Failed to read sliver file: {}", sliver_path.display()))?;
    
    let unpacked = nano::sliver::unpack_sliver(&sliver_data)
        .with_context(|| format!("Failed to unpack sliver: {}", sliver_path.display()))?;
    
    tracing::info!(
        "Unpacked sliver for {}: {} bytes heap, {} VFS entries",
        unpacked.metadata.hostname,
        unpacked.heap_data.len(),
        unpacked.vfs_entries.len()
    );

    // Create app registry with sliver data
    let mut registry = nano::app::registry::AppRegistry::default();
    let hostname = registry.register_from_sliver(&sliver_path, None)
        .with_context(|| format!("Failed to register sliver: {}", sliver_path.display()))?;
    
    let _registry = Arc::new(RwLock::new(registry));
    tracing::info!("Registered sliver-based app: {}", hostname);

    // Create SliverWorkerPool
    let worker_pool = nano::worker::pool::SliverWorkerPool::new(
        hostname.clone(),
        workers,
        0, // No memory limit for now
        unpacked,
    );
    
    tracing::info!("Created SliverWorkerPool with {} workers for {}", workers, hostname);

    // Set up graceful shutdown
    let drain = nano::app::drain::RequestDrain::new();
    let (shutdown, mut shutdown_rx) = nano::signal::setup_shutdown(
        nano::signal::ShutdownConfig::default(),
        drain,
    );
    tracing::info!("Graceful shutdown initialized");

    // Start HTTP server with the sliver app
    // Note: This is a simplified version - in full implementation,
    // the server would be integrated with the SliverWorkerPool for request dispatch
    let config = nano::http::ServerConfig::default();
    tracing::info!("Starting HTTP server on {} for sliver app {}", config.socket_addr()?, hostname);

    let shutdown_state = shutdown.state().clone();
    let server_handle = tokio::spawn(async move {
        nano::http::start_server_with_state(config, shutdown_state).await
    });

    // Wait for shutdown signal
    tracing::info!("Waiting for shutdown signal (Ctrl+C or SIGTERM)...");
    let _ = shutdown_rx.recv().await;
    tracing::info!("Shutdown signal received, initiating graceful shutdown...");

    // Perform graceful shutdown
    shutdown.shutdown().await;

    // Shut down worker pool
    tracing::info!("Shutting down SliverWorkerPool...");
    worker_pool.shutdown().expect("Failed to shutdown worker pool");

    // Wait for server
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

    tracing::info!("NANO Edge Runtime (sliver mode) shutdown complete");
    Ok(())
}

/// Run server with configuration file
async fn run_server_with_config(config_path: PathBuf) -> Result<()> {
    tracing::info!("Starting NANO with config: {}", config_path.display());
    
    // Load and validate config
    let config = nano::config::load_config(&config_path)
        .with_context(|| format!("Failed to load config: {}", config_path.display()))?;
    
    tracing::info!("Loaded configuration with {} app(s)", config.apps.len());
    
    // Check for sliver-based apps in config
    let has_sliver_apps = config.apps.iter().any(|app| app.sliver.is_some());
    if has_sliver_apps {
        tracing::info!("Configuration contains sliver-based apps");
    }
    
    // For now, fall back to regular server startup
    // Full integration would create appropriate worker pools per app type
    run_server().await
}

/// Handle sliver management commands
async fn handle_sliver_command(cmd: cli::SliverCommand) -> Result<()> {
    use nano::sliver::{pack_sliver, SliverMetadata, unpack_sliver, validate_sliver};
    use cli::validation::{validate_hostname, validate_sliver_name, validate_tag};
    
    match cmd {
        cli::SliverCommand::Create(args) => {
            tracing::info!("Creating sliver for hostname: {}", args.hostname);
            
            // Validate hostname
            validate_hostname(&args.hostname)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            
            // Validate optional name
            if let Some(ref name) = args.name {
                validate_sliver_name(name)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            
            // Validate optional tag
            if let Some(ref tag) = args.tag {
                validate_tag(tag)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            
            // Determine output path
            let output = args.output.unwrap_or_else(|| {
                let name = args.name.as_ref().unwrap_or(&args.hostname);
                let tag = args.tag.as_ref().map(|t| format!("-{}", t)).unwrap_or_default();
                PathBuf::from(format!("{}{}.sliver", name, tag))
            });
            
            // Validate output path doesn't already exist
            if output.exists() {
                anyhow::bail!("Sliver file already exists: {}. Use --output to specify a different path.", output.display());
            }
            
            let sliver_name = args.name.clone();
            let sliver_tag = args.tag.clone();
            
            // Create metadata
            let mut metadata = SliverMetadata::new(&args.hostname, env!("CARGO_PKG_VERSION"));
            metadata.name = sliver_name.clone();
            if let Some(tag) = sliver_tag {
                metadata.description = Some(format!("Tag: {}", tag));
            }
            
            // Create V8 snapshot using the new snapshot creator API (v139+)
            // This creates a snapshottable isolate and serializes it
            let isolate = nano::v8::NanoIsolate::snapshot_creator()?;
            let heap_data = nano::v8::create_snapshot_from_nano(isolate)?;
            tracing::info!("Created heap snapshot: {} bytes", heap_data.len());
            
            // Capture VFS state (currently returns empty)
            // In the future, this would capture all files from the isolate's VFS
            let vfs_entries: Option<&[(nano::vfs::VfsPath, nano::vfs::VfsFile)]> = None;
            
            // Pack the sliver
            let archive_data = pack_sliver(&metadata, &heap_data, vfs_entries)?;
            
            // Write to output file
            std::fs::write(&output, &archive_data)
                .with_context(|| format!("Failed to write sliver to {}", output.display()))?;
            
            println!("Created sliver: {}", output.display());
            println!("  Hostname: {}", args.hostname);
            println!("  Name: {}", sliver_name.as_deref().unwrap_or(&args.hostname));
            println!("  Tag: {}", args.tag.as_deref().unwrap_or("none"));
            println!("  Size: {} bytes", archive_data.len());
            println!("  Heap: {} bytes", heap_data.len());
            
            // Validate the created sliver
            validate_sliver(&archive_data)
                .context("Created sliver failed validation")?;
            
            tracing::info!("Sliver created successfully: {}", output.display());
            
            Ok(())
        }
        cli::SliverCommand::List(args) => {
            tracing::info!("Listing slivers");
            
            // Find all .sliver files in current directory
            let mut found = false;
            let entries = std::fs::read_dir(".")
                .context("Failed to read current directory")?;
            
            println!("Slivers:");
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                
                if path.extension().and_then(|e| e.to_str()) == Some("sliver") {
                    found = true;
                    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    
                    if args.verbose {
                        // Try to read metadata from the sliver
                        match std::fs::read(&path) {
                            Ok(data) => {
                                match unpack_sliver(&data) {
                                    Ok(unpacked) => {
                                        println!("  {} ({} bytes)", name, data.len());
                                        for line in unpacked.metadata.summary().lines() {
                                            println!("    {}", line);
                                        }
                                    }
                                    Err(e) => {
                                        println!("  {} ({} bytes) [invalid: {}]", name, data.len(), e);
                                    }
                                }
                            }
                            Err(e) => {
                                println!("  {} [error reading: {}]", name, e);
                            }
                        }
                    } else {
                        let size = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        println!("  {} ({} bytes)", name, size);
                    }
                }
            }
            
            if !found {
                println!("  (none found in current directory)");
            }
            
            Ok(())
        }
        cli::SliverCommand::Delete(args) => {
            tracing::info!("Deleting sliver: {}", args.name);
            
            // Validate sliver name
            validate_sliver_name(&args.name)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            
            // Look for sliver file with the given name
            let sliver_path = PathBuf::from(format!("{}.sliver", args.name));
            
            if !sliver_path.exists() {
                anyhow::bail!("Sliver not found: {}", args.name);
            }
            
            if !args.force {
                print!("Delete sliver '{}'? [y/N] ", args.name);
                use std::io::Write;
                std::io::stdout().flush()?;
                
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Deletion cancelled.");
                    return Ok(());
                }
            }
            
            std::fs::remove_file(&sliver_path)
                .with_context(|| format!("Failed to delete sliver: {}", args.name))?;
            
            println!("Deleted sliver: {}", args.name);
            tracing::info!("Sliver deleted: {}", sliver_path.display());
            
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_run() {
        let cli = Cli::try_parse_from(["nano-rs", "run"]);
        assert!(cli.is_ok());
        
        if let Ok(Cli { command: Some(Commands::Run { .. }) }) = cli {
            // Parsed correctly
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_cli_parse_sliver_create() {
        let cli = Cli::try_parse_from([
            "nano-rs", "sliver", "create", "api.example.com",
            "--output", "./test.sliver"
        ]);
        assert!(cli.is_ok());
        
        if let Ok(Cli { command: Some(Commands::Sliver(cmd)) }) = cli {
            match cmd {
                cli::SliverCommand::Create(args) => {
                    assert_eq!(args.hostname, "api.example.com");
                    assert_eq!(args.output, Some(PathBuf::from("./test.sliver")));
                }
                _ => panic!("Expected Create command"),
            }
        } else {
            panic!("Expected Sliver command");
        }
    }

    #[test]
    fn test_cli_parse_sliver_list() {
        let cli = Cli::try_parse_from(["nano-rs", "sliver", "list"]);
        assert!(cli.is_ok());
        
        if let Ok(Cli { command: Some(Commands::Sliver(cmd)) }) = cli {
            match cmd {
                cli::SliverCommand::List(args) => {
                    assert!(!args.verbose);
                }
                _ => panic!("Expected List command"),
            }
        } else {
            panic!("Expected Sliver command");
        }
    }

    #[test]
    fn test_cli_parse_sliver_delete() {
        let cli = Cli::try_parse_from(["nano-rs", "sliver", "delete", "test-sliver"]);
        assert!(cli.is_ok());
        
        if let Ok(Cli { command: Some(Commands::Sliver(cmd)) }) = cli {
            match cmd {
                cli::SliverCommand::Delete(args) => {
                    assert_eq!(args.name, "test-sliver");
                    assert!(!args.force);
                }
                _ => panic!("Expected Delete command"),
            }
        } else {
            panic!("Expected Sliver command");
        }
    }

    #[test]
    fn test_cli_default_to_run() {
        // Default behavior (no subcommand) should run server
        let cli = Cli::try_parse_from(["nano-rs"]);
        assert!(cli.is_ok());
        assert!(matches!(cli.unwrap().command, None));
    }
}
