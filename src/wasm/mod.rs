//! WebAssembly support for NANO runtime
//!
//! Provides WASM module loading and JavaScript API bindings.
//! Integrates with VFS for loading WASM files and supports sliver snapshots.

pub mod error;
pub mod loader;
pub mod sliver;
pub mod js_api;

pub use error::WasmError;
pub use loader::WasmLoader;
pub use sliver::{SliverWasmCache, CompiledWasmModule};
pub use js_api::WebAssemblyAPI;

/// WASM module handle - stores the raw WASM bytes
#[derive(Debug, Clone)]
pub struct WasmModule {
    bytes: Vec<u8>,
    path: Option<String>,
}

impl WasmModule {
    /// Create a new WASM module from bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, WasmError> {
        WasmLoader::validate(&bytes)?;
        Ok(Self { bytes, path: None })
    }
    
    /// Create a new WASM module from bytes with a path
    pub fn from_bytes_with_path(bytes: Vec<u8>, path: impl Into<String>) -> Result<Self, WasmError> {
        WasmLoader::validate(&bytes)?;
        Ok(Self { bytes, path: Some(path.into()) })
    }
    
    /// Get the WASM bytes
    pub fn bytes(&self) -> &[u8] { &self.bytes }
    
    /// Get the module path if available
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    
    /// Get the module size in bytes
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}
