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
        Some(Commands::Run { config: _ }) | None => {
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

/// Handle sliver management commands
async fn handle_sliver_command(cmd: cli::SliverCommand) -> Result<()> {
    match cmd {
        cli::SliverCommand::Create(args) => {
            tracing::info!("Creating sliver for hostname: {}", args.hostname);
            
            // Determine output path
            let output = args.output.unwrap_or_else(|| {
                let name = args.name.as_ref().unwrap_or(&args.hostname);
                let tag = args.tag.as_ref().map(|t| format!("-{}", t)).unwrap_or_default();
                PathBuf::from(format!("{}{}.sliver", name, tag))
            });
            
            let name = args.name.unwrap_or_else(|| args.hostname.clone());
            
            // TODO: Implement actual sliver creation
            // For now, print what would happen
            println!("Creating sliver:");
            println!("  Hostname: {}", args.hostname);
            println!("  Name: {}", name);
            println!("  Tag: {}", args.tag.as_deref().unwrap_or("none"));
            println!("  Output: {}", output.display());
            println!("");
            println!("Note: Full implementation pending V8 SnapshotCreator integration");
            
            Ok(())
        }
        cli::SliverCommand::List(args) => {
            tracing::info!("Listing slivers");
            
            // TODO: Implement actual sliver listing
            println!("Slivers:");
            println!("  (none found)");
            
            if args.verbose {
                println!("\nUse 'nano-rs sliver list --verbose' for detailed info.");
            }
            
            Ok(())
        }
        cli::SliverCommand::Delete(args) => {
            tracing::info!("Deleting sliver: {}", args.name);
            
            if !args.force {
                // TODO: Implement confirmation prompt
                println!("Delete sliver '{}' ? (use --force to skip confirmation)", args.name);
                return Ok(());
            }
            
            // TODO: Implement actual deletion
            println!("Deleting sliver: {}", args.name);
            println!("Note: Full implementation pending");
            
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
