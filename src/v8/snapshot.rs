//! V8 Snapshot Support for Fast Cold Starts
//!
//! This module provides infrastructure for loading pre-generated V8 snapshots.
//! Snapshots should be created at build/deploy time, not runtime.
//!
//! ## Architecture
//!
//! Build/Deploy Time (separate tool):
//!   Create isolate → Bind WinterTC APIs → create_blob() → Save to file
//!
//! Server Startup:
//!   Load snapshot file → Store in SnapshotCache
//!
//! Cold Start (Request 1):
//!   StartupData::new(snapshot_blob)
//!   → CreateParams::snapshot_blob()
//!   → Isolate::new(params) [~1ms vs ~50-100ms from scratch]
//!   → Execute user script → Store handler in global scope
//!   → Handle Request
//!
//! Warm Start (Requests 2-N):
//!   Reuse existing isolate
//!   → Handler already in global scope
//!   → Handle Request [~1-5ms]

use anyhow::{anyhow, Result};
use std::sync::Once;

static V8_INIT: Once = Once::new();

/// Ensure V8 platform is initialized (idempotent)
pub fn ensure_v8_initialized() {
    V8_INIT.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

/// Cache for pre-generated runtime snapshot.
///
/// Snapshots should be created at build time using a separate tool,
/// then loaded at server startup.
pub struct SnapshotCache {
    data: Vec<u8>,
}

impl SnapshotCache {
    /// Create a snapshot cache from pre-generated data.
    pub fn from_data(data: Vec<u8>) -> Self {
        tracing::info!("Loaded V8 snapshot ({} bytes)", data.len());
        Self { data }
    }

    /// Attempt to load snapshot from a file path.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let data = std::fs::read(path)
            .map_err(|e| anyhow!("Failed to read snapshot file: {}", e))?;
        
        if data.is_empty() {
            return Err(anyhow!("Snapshot file is empty"));
        }
        
        tracing::info!("Loaded V8 snapshot from {} ({} bytes)", path.display(), data.len());
        Ok(Self { data })
    }

    /// Get the snapshot data as a byte slice.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Check if snapshot has valid data.
    pub fn is_valid(&self) -> bool {
        !self.data.is_empty()
    }
}

/// Create an isolate from a snapshot.
///
/// This is the fast path for cold starts - the isolate is created with
/// WinterTC APIs already available, skipping the compilation/bind step.
pub fn create_isolate_from_snapshot(snapshot_data: &[u8]) -> v8::OwnedIsolate {
    ensure_v8_initialized();

    let startup_data: v8::StartupData = snapshot_data.to_vec().into();
    let create_params = v8::CreateParams::default().snapshot_blob(startup_data);

    v8::Isolate::new(create_params)
}

/// Lazy-initialized global snapshot cache.
use std::sync::OnceLock;
static GLOBAL_SNAPSHOT: OnceLock<SnapshotCache> = OnceLock::new();

/// Initialize the global snapshot cache from pre-generated data.
///
/// This should be called once at server startup, before any isolates are created.
pub fn init_global_snapshot(data: Vec<u8>) -> Result<()> {
    let snapshot = SnapshotCache::from_data(data);
    GLOBAL_SNAPSHOT
        .set(snapshot)
        .map_err(|_| anyhow!("Global snapshot already initialized"))?;
    Ok(())
}

/// Initialize the global snapshot cache from a file.
pub fn init_global_snapshot_from_file(path: &std::path::Path) -> Result<()> {
    let snapshot = SnapshotCache::from_file(path)?;
    GLOBAL_SNAPSHOT
        .set(snapshot)
        .map_err(|_| anyhow!("Global snapshot already initialized"))?;
    Ok(())
}

/// Get a reference to the global snapshot cache.
///
/// Returns None if snapshot hasn't been initialized.
pub fn global_snapshot() -> Option<&'static SnapshotCache> {
    GLOBAL_SNAPSHOT.get()
}

/// Check if the global snapshot has been initialized.
pub fn is_snapshot_initialized() -> bool {
    GLOBAL_SNAPSHOT.get().is_some()
}

/// Check if the global snapshot has valid data.
pub fn is_snapshot_valid() -> bool {
    GLOBAL_SNAPSHOT.get().map(|s| s.is_valid()).unwrap_or(false)
}

/// Build-time snapshot creation from a NanoIsolate.
///
/// This is used by the `sliver build` command to create deployable snapshots.
/// NOTE: In v147, runtime snapshot creation has limitations. This function
/// returns empty data as a placeholder. Pre-generated snapshots should be
/// created using a separate build-time tool.
pub fn create_snapshot_from_nano(_isolate: crate::v8::NanoIsolate) -> anyhow::Result<Vec<u8>> {
    // v147 API doesn't support easy runtime snapshot extraction
    // Snapshots should be pre-generated at build time using V8's snapshot_creator
    tracing::warn!("Runtime snapshot creation not implemented for v147 - returning empty snapshot");
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_snapshot() {
        let snapshot = SnapshotCache::from_data(vec![]);
        assert!(!snapshot.is_valid());
    }

    #[test]
    fn test_valid_snapshot() {
        let snapshot = SnapshotCache::from_data(vec![1, 2, 3, 4]);
        assert!(snapshot.is_valid());
        assert_eq!(snapshot.data().len(), 4);
    }
}
