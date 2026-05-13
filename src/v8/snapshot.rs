//! V8 Snapshot Integration
//!
//! This module provides integration with V8's snapshot capabilities.
//!
//! # V8 v139+ API Support
//!
//! rusty_v8 version 139+ exposes the `snapshot_creator()` API:
//! - `Isolate::snapshot_creator()` - PUBLIC (creates isolate for snapshotting)
//! - `OwnedIsolate::create_blob()` - PUBLIC (serializes to snapshot blob)
//!
//! This enables true runtime heap snapshot creation for fast sliver warm-starts.
//!
//! # Usage Flow
//!
//! 1. Create isolate via `snapshot_creator()` (not regular `Isolate::new()`)
//! 2. Set up context, load scripts, populate state
//! 3. Call `create_blob()` to serialize heap to blob
//! 4. Pack blob into sliver with metadata
//! 5. Later: Restore isolate from snapshot for instant warm-start

use thiserror::Error;

/// Errors that can occur during snapshot operations
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// Failed to create the V8 snapshot
    #[error("Failed to create snapshot: {0}")]
    CreationFailed(String),
    
    /// External reference handling error
    #[error("External reference error: {0}")]
    ExternalReferenceError(String),
    
    /// The isolate is not in a valid state for snapshotting
    #[error("Invalid isolate state for snapshot: {0}")]
    InvalidIsolateState(String),
    
    /// Snapshot operation not supported in current V8 version
    #[error("Snapshot operation requires V8 features not available in this version")]
    NotSupported,
    
    /// The isolate was not created with snapshot_creator()
    #[error("Isolate was not created with snapshot_creator() - cannot create blob")]
    NotSnapshotCreatorIsolate,
}

/// Result type for snapshot operations
pub type SnapshotResult<T> = std::result::Result<T, SnapshotError>;

/// Check if full heap snapshotting is supported
///
/// Returns true if the current V8 version supports capturing
/// a running isolate's heap state.
pub fn is_heap_snapshot_supported() -> bool {
    // v8 139+ exposes the snapshot_creator API
    true
}

/// Check if snapshot data contains the legacy cold sliver marker
///
/// # Design Rationale (Intentional Backward Compatibility)
///
/// Detects the legacy marker header used by early nano-rs versions
/// when the V8 snapshot API was not fully exposed. This function
/// enables graceful handling of legacy sliver files by identifying
/// them before attempting snapshot restoration.
///
/// Modern slivers always contain real V8 heap snapshots created via
/// `create_snapshot()`. This detection path exists only for backward
/// compatibility with older sliver files.
pub fn is_placeholder_snapshot(data: &[u8]) -> bool {
    data == b"NANO_SNAPSHOT_PLACEHOLDER_V1"
}

/// Creates a V8 heap snapshot from an isolate
///
/// NOTE: This function requires the isolate to have been created
/// via `snapshot_creator()`. The isolate must have a default context set.
///
/// # Arguments
///
/// * `isolate` - The isolate to snapshot (must be from snapshot_creator())
///
/// # Returns
///
/// A `Vec<u8>` containing the snapshot blob, or an error.
pub fn create_snapshot(isolate: v8::OwnedIsolate) -> SnapshotResult<Vec<u8>> {
    // Try to create blob from the isolate
    // Note: V8 requires a default context to be set before creating blob
    // If no context was set, create_blob will return None
    let startup_data = isolate.create_blob(v8::FunctionCodeHandling::Clear)
        .ok_or_else(|| SnapshotError::InvalidIsolateState(
            "Failed to create snapshot - ensure isolate has a context set".to_string()
        ))?;
    
    // Convert StartupData to Vec<u8>
    let data: Vec<u8> = startup_data.as_ref().to_vec();
    
    tracing::info!("Created V8 heap snapshot: {} bytes", data.len());
    Ok(data)
}

/// Creates a V8 heap snapshot from an isolate (for sliver creation)
///
/// This is a wrapper that works with NanoIsolate, but requires
/// the isolate was created with the snapshot workflow.
///
/// For sliver creation, use the workflow:
/// 1. Create isolate via Isolate::snapshot_creator()
/// 2. Load app, set up context
/// 3. Call this function to capture snapshot
/// 4. Pack into sliver
///
/// # Arguments
///
/// * `isolate` - NanoIsolate to snapshot
///
/// # Returns
///
/// A `Vec<u8>` containing the snapshot blob, or an error.
pub fn create_snapshot_from_nano(
    nano_isolate: crate::v8::isolate::NanoIsolate
) -> SnapshotResult<Vec<u8>> {
    // Extract the OwnedIsolate from NanoIsolate
    // This consumes the NanoIsolate and extracts the inner isolate
    let isolate = nano_isolate.into_inner();
    
    // Create the snapshot blob
    create_snapshot(isolate)
}

/// Restore an isolate from a heap snapshot blob
///
/// This function creates a new isolate initialized with the
/// provided snapshot data. This enables instant "warm" starts
/// with all compiled code and heap state pre-loaded.
///
/// # Arguments
/// * `snapshot_data` - The V8 heap snapshot blob (from heap.bin)
/// * `vfs` - The IsolateVfs to attach to the restored isolate
///
/// # Returns
/// A NanoIsolate restored from snapshot, or SnapshotError if restoration fails
pub fn restore_from_snapshot(
    snapshot_data: &[u8],
    vfs: crate::vfs::IsolateVfs,
) -> SnapshotResult<crate::v8::isolate::NanoIsolate> {
    // Check for legacy placeholder
    if is_placeholder_snapshot(snapshot_data) {
        return Err(SnapshotError::InvalidIsolateState(
            "Cannot restore from placeholder snapshot (legacy sliver)".to_string()
        ));
    }

    // Validate snapshot data is non-empty
    if snapshot_data.is_empty() {
        return Err(SnapshotError::InvalidIsolateState(
            "Snapshot data is empty".to_string()
        ));
    }

    // Attempt to create isolate from snapshot
    match crate::v8::isolate::NanoIsolate::from_snapshot(snapshot_data, vfs) {
        Ok(isolate) => {
            tracing::info!("Restored isolate from snapshot ({} bytes)", snapshot_data.len());
            Ok(isolate)
        }
        Err(e) => {
            tracing::error!("Failed to restore isolate from snapshot: {}", e);
            Err(SnapshotError::CreationFailed(e.to_string()))
        }
    }
}

/// Builder for creating snapshots with custom configuration
///
/// This provides a more flexible API for snapshot creation
/// when you need to customize the process.
pub struct SnapshotBuilder {
    function_code_handling: v8::FunctionCodeHandling,
}

impl SnapshotBuilder {
    /// Create a new snapshot builder with default settings
    pub fn new() -> Self {
        Self {
            function_code_handling: v8::FunctionCodeHandling::Clear,
        }
    }
    
    /// Set the function code handling strategy
    ///
    /// - `Clear`: Clear compiled code (smaller snapshots)
    /// - `Keep`: Keep compiled code (faster restore, larger snapshots)
    pub fn with_code_handling(mut self, handling: v8::FunctionCodeHandling) -> Self {
        self.function_code_handling = handling;
        self
    }
    
    /// Create a snapshot from an isolate
    ///
    /// # Arguments
    ///
    /// * `isolate` - The isolate to snapshot (must be from snapshot_creator())
    ///
    /// # Returns
    ///
    /// A `Vec<u8>` containing the snapshot blob, or an error.
    pub fn build(self, isolate: v8::OwnedIsolate) -> SnapshotResult<Vec<u8>> {
        // Format the handling for logging before moving it
        let handling_str = format!("{:?}", self.function_code_handling);
        let startup_data = isolate.create_blob(self.function_code_handling)
            .ok_or_else(|| SnapshotError::NotSnapshotCreatorIsolate)?;
        
        let data: Vec<u8> = startup_data.as_ref().to_vec();
        tracing::info!("Created V8 heap snapshot with {}: {} bytes", 
            handling_str, data.len());
        Ok(data)
    }
}

impl Default for SnapshotBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_snapshot_error_display() {
        let err = SnapshotError::CreationFailed("test error".to_string());
        assert_eq!(format!("{}", err), "Failed to create snapshot: test error");
        
        let err = SnapshotError::ExternalReferenceError("bad ref".to_string());
        assert_eq!(format!("{}", err), "External reference error: bad ref");
        
        let err = SnapshotError::InvalidIsolateState("no context".to_string());
        assert_eq!(format!("{}", err), "Invalid isolate state for snapshot: no context");
        
        let err = SnapshotError::NotSupported;
        assert_eq!(format!("{}", err), "Snapshot operation requires V8 features not available in this version");
        
        let err = SnapshotError::NotSnapshotCreatorIsolate;
        assert_eq!(format!("{}", err), "Isolate was not created with snapshot_creator() - cannot create blob");
    }
    
    #[test]
    fn test_is_heap_snapshot_supported() {
        // v139+ should return true
        assert!(is_heap_snapshot_supported());
    }
    
    #[test]
    fn test_is_placeholder_detection() {
        assert!(is_placeholder_snapshot(b"NANO_SNAPSHOT_PLACEHOLDER_V1"));
        assert!(!is_placeholder_snapshot(b"real data"));
        assert!(!is_placeholder_snapshot(&[]));
        assert!(!is_placeholder_snapshot(b"NANO_SNAPSHOT_PLACEHOLDER_V2"));
    }
    
    #[test]
    fn test_snapshot_builder_new() {
        let builder = SnapshotBuilder::new();
        let _ = builder.function_code_handling; // Access to verify it exists
    }
    
    #[test]
    fn test_snapshot_builder_default() {
        let builder: SnapshotBuilder = Default::default();
        let _ = builder.function_code_handling;
    }
    
    #[test]
    fn test_snapshot_builder_with_code_handling() {
        let builder = SnapshotBuilder::new()
            .with_code_handling(v8::FunctionCodeHandling::Keep);
        // Verify the builder was created successfully
        let _ = builder.function_code_handling;
    }
}
