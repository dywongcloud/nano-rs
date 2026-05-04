//! Unified Application Source Types
//!
//! This module defines the Source enum that allows WorkerPool to handle
//! both entrypoint (config) mode and sliver mode through a single code path.

use crate::sliver::UnpackedSliver;
use std::path::PathBuf;

/// Source of application code for worker initialization
///
/// This enum unifies the two previously separate paths:
/// - Entrypoint: Load JS from filesystem path (config mode)
/// - Sliver: Restore from snapshot (sliver mode)
///
/// Both paths now flow through the same WorkerPool implementation.
#[derive(Debug, Clone)]
pub enum AppSource {
    /// Load from JavaScript/WASM entrypoint file
    /// 
    /// Used in config mode where the app is defined by:
    /// - hostname: The virtual host
    /// - entrypoint: Path to .js/.wasm file (e.g., "./app.js")
    /// - vfs_backend: Storage backend for VFS
    Entrypoint {
        /// Path to the JavaScript/WASM entrypoint file
        path: String,
    },
    
    /// Restore from sliver snapshot
    ///
    /// Used in sliver mode where the app is packaged as:
    /// - unpacked_sliver: Contains snapshot + VFS entries
    /// - temp_entrypoint: Optional override path
    Sliver {
        /// Unpacked sliver data
        data: UnpackedSliver,
        /// Optional temp entrypoint for VFS-extracted files
        temp_entrypoint: Option<PathBuf>,
    },
    
    /// Static site (no JavaScript execution)
    ///
    /// Used for pure static file serving without isolate creation.
    /// Workers are not spawned for this source type.
    Static {
        /// Root directory for static files
        root: String,
    },
}

impl AppSource {
    /// Create a source from entrypoint path (JavaScript or WASM)
    /// 
    /// Supports both .js and .wasm files:
    /// - .js: JavaScript with handler function
    /// - .wasm: WebAssembly module with exports
    pub fn entrypoint(path: impl Into<String>) -> Self {
        Self::Entrypoint {
            path: path.into(),
        }
    }
    
    /// Create a source from unpacked sliver
    /// 
    /// Slivers can contain any app type (static, JS, WASM)
    /// and are restored from V8 snapshots for fast cold starts.
    pub fn sliver(data: UnpackedSliver) -> Self {
        Self::Sliver {
            data,
            temp_entrypoint: None,
        }
    }
    
    /// Create a source from sliver with temp entrypoint
    /// 
    /// Used when sliver VFS has been extracted to temp directory.
    pub fn sliver_with_temp(data: UnpackedSliver, temp: PathBuf) -> Self {
        Self::Sliver {
            data,
            temp_entrypoint: Some(temp),
        }
    }
    
    /// Create a static site source
    /// 
    /// Static sites serve files directly without V8 isolate creation.
    /// Can still be packaged as slivers for distribution.
    pub fn static_site(root: impl Into<String>) -> Self {
        Self::Static {
            root: root.into(),
        }
    }
    
    /// Check if this source requires V8 isolate (JS or WASM)
    pub fn needs_isolate(&self) -> bool {
        matches!(self, Self::Entrypoint { .. } | Self::Sliver { .. })
    }
    
    /// Check if this is an entrypoint source
    pub fn is_entrypoint(&self) -> bool {
        matches!(self, Self::Entrypoint { .. })
    }
    
    /// Check if this is a sliver source
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver { .. })
    }
    
    /// Check if this is a static source
    pub fn is_static(&self) -> bool {
        matches!(self, Self::Static { .. })
    }
    
    /// Get entrypoint path if this is an entrypoint source
    pub fn entrypoint_path(&self) -> Option<&str> {
        match self {
            Self::Entrypoint { path } => Some(path),
            _ => None,
        }
    }
    
    /// Get sliver data if this is a sliver source
    pub fn sliver_data(&self) -> Option<&UnpackedSliver> {
        match self {
            Self::Sliver { data, .. } => Some(data),
            _ => None,
        }
    }
    
    /// Get temp entrypoint path if this is a sliver source with override
    pub fn temp_entrypoint(&self) -> Option<&PathBuf> {
        match self {
            Self::Sliver { temp_entrypoint, .. } => temp_entrypoint.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_entrypoint_creation() {
        let source = AppSource::entrypoint("./app.js");
        assert!(source.is_entrypoint());
        assert!(!source.is_sliver());
        assert!(!source.is_static());
        assert_eq!(source.entrypoint_path(), Some("./app.js"));
    }
    
    #[test]
    fn test_wasm_entrypoint() {
        let source = AppSource::entrypoint("./app.wasm");
        assert!(source.is_entrypoint());
        assert!(source.needs_isolate());
        assert_eq!(source.entrypoint_path(), Some("./app.wasm"));
    }
    
    #[test]
    fn test_static_creation() {
        let source = AppSource::static_site("./static");
        assert!(source.is_static());
        assert!(!source.needs_isolate());
        assert!(!source.is_entrypoint());
        assert!(!source.is_sliver());
    }
}
