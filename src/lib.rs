//! NANO Edge Runtime - Multi-tenant JavaScript edge runtime
//!
//! A Rust-based edge runtime using rusty_v8 to execute JavaScript in isolated
//! V8 contexts. Supports multi-tenancy with thread-local isolates and context
//! reset between requests for fast cold starts.

use anyhow::Result;

pub mod v8;
pub mod runtime;
pub mod http;
pub mod worker;
pub mod config;
pub mod app;
pub mod admin;
pub mod logging;
pub mod signal;
pub mod metrics;
pub mod vfs;
pub mod sliver;
pub mod wasm;
pub mod limits;

/// Library entry point for running the NANO runtime
///
/// This is called by the binary's main() function and can also be used
/// for integration testing.
pub fn run() -> Result<()> {
    tracing::info!("NANO runtime initialized");
    // Runtime initialization is handled by the CLI
    Ok(())
}
