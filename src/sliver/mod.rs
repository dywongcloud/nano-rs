//! Sliver Format Module
//!
//! Provides tar-based snapshot format for JavaScript isolates.
//!
//! A sliver is a container-image-like format for V8 isolates that includes:
//! - **Metadata**: JSON file with hostname, timestamps, version info
//! - **Heap**: Opaque V8 heap snapshot blob (version-specific to V8)
//! - **VFS**: Virtual filesystem contents preserving directory structure
//!
//! # Example
//!
//! ```rust
//! use nano::sliver::{SliverMetadata, pack_sliver, unpack_sliver};
//!
//! // Create metadata
//! let metadata = SliverMetadata::new("app.example.com", "1.1.0");
//!
//! // Pack into archive (with fake heap data)
//! let heap_data = vec![0u8; 1024];
//! let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
//!
//! // Unpack and verify
//! let unpacked = unpack_sliver(&archive).unwrap();
//! assert_eq!(unpacked.metadata.hostname, "app.example.com");
//! ```
//!
//! # Archive Structure
//!
//! ```text
//! app-v1.sliver (tar archive)
//! ├── meta.json          # Metadata: hostname, created_at, version
//! ├── heap.bin           # V8 heap snapshot (opaque blob)
//! ├── vfs/               # Virtual filesystem contents
//! │   ├── data/
//! │   │   └── config.json
//! │   └── assets/
//! │       └── logo.png
//! └── manifest.txt       # Human-readable manifest
//! ```
//!
//! # Design Principles
//!
//! - **Simple**: Standard tar format, inspectable with `tar -tf`
//! - **Opaque**: heap.bin is a version-specific blob, never parsed
//! - **Portable**: No host-specific paths or IDs
//! - **Evolvable**: Format allows future deltas and compression

// Submodules
mod error;
mod format;
mod metadata;
mod packer;
pub mod restore;
mod unpacker;
pub mod vfs_capture;

// Public exports
pub use error::{SliverError, SliverResult};
pub use format::{SliverFormat, FORMAT_VERSION, HEAP_FILENAME, MANIFEST_FILENAME, METADATA_FILENAME, VFS_PREFIX, SLIVER_EXTENSION};
pub use metadata::SliverMetadata;
pub use packer::{pack_sliver, SliverPacker};
pub use unpacker::{unpack_sliver, SliverUnpacker, UnpackedSliver};

// Re-export for documentation only
#[doc(hidden)]
pub use packer::pack_sliver as _pack_sliver_doc;
#[doc(hidden)]
pub use unpacker::unpack_sliver as _unpack_sliver_doc;

use std::collections::HashMap;
use std::io::Read;

/// Walk a VFS backend and collect all entries for serialization
///
/// This is a helper function that walks the file system and returns
/// a vector of (path, file) tuples suitable for pack_sliver().
pub async fn walk_vfs_for_snapshot<B>(backend: &B) -> crate::vfs::VfsResult<Vec<(crate::vfs::VfsPath, crate::vfs::VfsFile)>>
where
    B: crate::vfs::VfsBackend,
{
    // Note: This is a simplified implementation.
    // A full implementation would need to list all files in the backend.
    // For now, we return an empty list as the snapshot feature
    // will need additional backend methods to list files.
    
    // TODO: Add list_dir() method to VfsBackend trait for full implementation
    Ok(vec![])
}

/// Build a manifest string from a list of archive entries
pub fn build_manifest(entries: &[String]) -> String {
    let mut manifest = String::new();
    manifest.push_str("# Sliver Archive Manifest\n");
    manifest.push_str("# =========================\n\n");

    for entry in entries {
        manifest.push_str(entry);
        manifest.push('\n');
    }

    manifest
}

/// Validate a sliver archive without fully unpacking it
///
/// Performs quick validation:
/// - Checks tar structure is valid
/// - Verifies required files exist
/// - Validates metadata JSON
/// - Checks format version is supported
///
/// Returns Ok(()) if valid, Err if invalid
pub fn validate_sliver(archive_data: &[u8]) -> SliverResult<()> {
    use tar::Archive;
    
    let mut archive = Archive::new(archive_data);
    
    let mut has_metadata = false;
    let mut has_heap = false;
    let mut metadata_version: Option<String> = None;

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?;
        let path_str = path.to_string_lossy();

        if path_str == METADATA_FILENAME {
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content)?;
            let metadata: SliverMetadata = serde_json::from_slice(&content)?;
            metadata_version = Some(metadata.format_version);
            has_metadata = true;
        } else if path_str == HEAP_FILENAME {
            has_heap = true;
        }
    }

    if !has_metadata {
        return Err(SliverError::MissingMetadata {
            filename: METADATA_FILENAME.to_string(),
        });
    }

    if !has_heap {
        return Err(SliverError::MissingHeap {
            filename: HEAP_FILENAME.to_string(),
        });
    }

    if let Some(version) = metadata_version {
        if !format::SliverFormat::is_supported_version(&version) {
            return Err(SliverError::InvalidFormat { version });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_manifest() {
        let entries = vec![
            "meta.json".to_string(),
            "heap.bin".to_string(),
            "vfs/test.txt".to_string(),
        ];

        let manifest = build_manifest(&entries);
        assert!(manifest.contains("# Sliver Archive Manifest"));
        assert!(manifest.contains("meta.json"));
        assert!(manifest.contains("heap.bin"));
        assert!(manifest.contains("vfs/test.txt"));
    }

    #[test]
    fn test_validate_sliver_valid() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0u8; 100];
        
        let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
        
        let result = validate_sliver(&archive);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_sliver_missing_heap() {
        let mut builder = tar::Builder::new(Vec::new());
        
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let json = serde_json::to_vec(&metadata).unwrap();
        
        let mut header = tar::Header::new_gnu();
        header.set_path(METADATA_FILENAME).unwrap();
        header.set_size(json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, json.as_slice()).unwrap();
        
        builder.finish().unwrap();
        let archive = builder.into_inner().unwrap();

        let result = validate_sliver(&archive);
        assert!(matches!(result, Err(SliverError::MissingHeap { .. })));
    }

    #[test]
    fn test_module_exports() {
        // Test that all main types are exported
        let _ = SliverMetadata::new("test", "1.0.0");
        let _format_version = FORMAT_VERSION;
        let _heap_filename = HEAP_FILENAME;
        let _metadata_filename = METADATA_FILENAME;
    }
}
