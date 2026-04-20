//! Sliver Unpacker
//!
//! Implements tar archive extraction for sliver snapshots.
//! Extracts metadata, heap blob, and VFS contents from an archive.

use std::io::Read;
use tar::Archive;

use crate::sliver::error::{SliverError, SliverResult};
use crate::sliver::format::{HEAP_FILENAME, METADATA_FILENAME, VFS_PREFIX};
use crate::sliver::metadata::SliverMetadata;
use crate::vfs::types::{VfsFile, VfsPath};

/// Unpacked sliver contents
///
/// This structure holds all the components extracted from a sliver archive.
#[derive(Debug, Clone)]
pub struct UnpackedSliver {
    /// Metadata from meta.json
    pub metadata: SliverMetadata,
    /// Opaque V8 heap blob from heap.bin
    pub heap_data: Vec<u8>,
    /// VFS entries extracted from vfs/ prefix
    pub vfs_entries: Vec<(VfsPath, VfsFile)>,
}

impl UnpackedSliver {
    /// Create a new unpacked sliver with the given components
    pub fn new(
        metadata: SliverMetadata,
        heap_data: Vec<u8>,
        vfs_entries: Vec<(VfsPath, VfsFile)>,
    ) -> Self {
        Self {
            metadata,
            heap_data,
            vfs_entries,
        }
    }

    /// Get the total size of all components
    pub fn total_size(&self) -> usize {
        let metadata_size = serde_json::to_vec(&self.metadata).map(|v| v.len()).unwrap_or(0);
        let heap_size = self.heap_data.len();
        let vfs_size: usize = self.vfs_entries.iter().map(|(_, f)| f.content.len()).sum();
        
        metadata_size + heap_size + vfs_size
    }

    /// Get a summary of the unpacked sliver
    pub fn summary(&self) -> String {
        format!(
            "Unpacked Sliver:\n  Hostname: {}\n  Format: {}\n  Heap: {} bytes\n  VFS: {} entries\n  Total: {} bytes",
            self.metadata.hostname,
            self.metadata.format_version,
            self.heap_data.len(),
            self.vfs_entries.len(),
            self.total_size()
        )
    }

    /// Restore VFS entries to an IsolateVfs
    ///
    /// This populates the isolate's VFS with all files extracted
    /// from the sliver archive.
    ///
    /// # Arguments
    /// * `vfs` - The IsolateVfs to populate
    ///
    /// # Returns
    /// SliverResult indicating success
    pub async fn restore_to_vfs(&self, vfs: &crate::vfs::IsolateVfs) -> SliverResult<()> {
        use crate::vfs::VfsBackend;

        tracing::info!(
            "Restoring {} VFS entries to isolate",
            self.vfs_entries.len()
        );

        for (path, file) in &self.vfs_entries {
            vfs.write(path, &file.content).await.map_err(|e| {
                SliverError::VfsRestore {
                    path: path.to_string(),
                    reason: e.to_string(),
                }
            })?;
        }

        tracing::info!("VFS restoration complete");
        Ok(())
    }
}

/// Unpacker for sliver archives
///
/// Extracts all components from a tar archive into structured data.
pub struct SliverUnpacker;

impl SliverUnpacker {
    /// Unpack a sliver archive from bytes
    ///
    /// Reads the tar archive and extracts all components:
    /// - meta.json → SliverMetadata
    /// - heap.bin → heap_data Vec<u8>
    /// - vfs/* → VFS entries
    ///
    /// # Arguments
    /// * `archive_data` - The tar archive bytes
    ///
    /// # Returns
    /// An UnpackedSliver containing all extracted components
    ///
    /// # Errors
    /// Returns SliverError if the archive is corrupted or missing required files
    pub fn unpack(archive_data: &[u8]) -> SliverResult<UnpackedSliver> {
        let mut archive = Archive::new(archive_data);
        
        let mut metadata: Option<SliverMetadata> = None;
        let mut heap_data: Option<Vec<u8>> = None;
        let mut vfs_entries: Vec<(VfsPath, VfsFile)> = Vec::new();

        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let path = entry.path()?;
            let path_str = path.to_string_lossy().to_string(); // Clone to release borrow

            // Read entry content (Entry implements Read trait)
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content)?;

            match path_str.as_ref() {
                METADATA_FILENAME => {
                    metadata = Some(SliverMetadata::from_json(&content)?);
                }
                HEAP_FILENAME => {
                    heap_data = Some(content);
                }
                path if path.starts_with(VFS_PREFIX) => {
                    // Extract VFS entry
                    let vfs_path_str = &path[VFS_PREFIX.len()..];
                    match VfsPath::new(vfs_path_str) {
                        Ok(vfs_path) => {
                            let file = VfsFile {
                                content,
                                created_at: std::time::SystemTime::now(),
                                modified_at: std::time::SystemTime::now(),
                                size: 0, // Will be set from content.len()
                            };
                            vfs_entries.push((vfs_path, file));
                        }
                        Err(_e) => {
                            return Err(SliverError::InvalidVfsPath {
                                path: vfs_path_str.to_string(),
                            });
                        }
                    }
                }
                _ => {
                    // Skip unknown entries (including manifest.txt)
                    // This allows forward compatibility with future format additions
                }
            }
        }

        // Validate required files
        let metadata = metadata.ok_or_else(|| SliverError::MissingMetadata {
            filename: METADATA_FILENAME.to_string(),
        })?;

        let heap_data = heap_data.ok_or_else(|| SliverError::MissingHeap {
            filename: HEAP_FILENAME.to_string(),
        })?;

        // Validate format version
        if !crate::sliver::format::SliverFormat::is_supported_version(&metadata.format_version) {
            return Err(SliverError::InvalidFormat {
                version: metadata.format_version.clone(),
            });
        }

        // Fix up VFS file sizes
        for (_, file) in &mut vfs_entries {
            file.size = file.content.len();
        }

        Ok(UnpackedSliver::new(metadata, heap_data, vfs_entries))
    }

    /// Unpack and validate a sliver archive
    ///
    /// Same as unpack() but performs additional validation:
    /// - Checks heap data is non-empty
    /// - Verifies VFS paths are valid
    pub fn unpack_and_validate(archive_data: &[u8]) -> SliverResult<UnpackedSliver> {
        let unpacked = Self::unpack(archive_data)?;

        // Additional validations
        if unpacked.heap_data.is_empty() {
            return Err(SliverError::CorruptedArchive {
                reason: "Heap data is empty".to_string(),
            });
        }

        Ok(unpacked)
    }
}

/// Convenience function to unpack a sliver
///
/// Alias for SliverUnpacker::unpack()
pub fn unpack_sliver(archive_data: &[u8]) -> SliverResult<UnpackedSliver> {
    SliverUnpacker::unpack(archive_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sliver::packer::pack_sliver;

    #[test]
    fn test_unpack_basic() {
        // Create a sliver
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0xABu8; 1024];
        
        let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
        
        // Unpack it
        let unpacked = unpack_sliver(&archive).unwrap();
        
        assert_eq!(unpacked.metadata.hostname, "app.example.com");
        assert_eq!(unpacked.heap_data, heap_data);
        assert!(unpacked.vfs_entries.is_empty());
    }

    #[test]
    fn test_unpack_with_vfs() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0xCDu8; 512];
        
        let vfs_entries = vec![
            (
                VfsPath::new("test.txt").unwrap(),
                VfsFile::new(b"Hello, World!".to_vec()),
            ),
            (
                VfsPath::new("data/config.json").unwrap(),
                VfsFile::new(b"{\"key\": \"value\"}".to_vec()),
            ),
        ];

        let archive = pack_sliver(&metadata, &heap_data, Some(&vfs_entries)).unwrap();
        let unpacked = unpack_sliver(&archive).unwrap();

        assert_eq!(unpacked.vfs_entries.len(), 2);
        
        // Find and verify entries
        let test_entry = unpacked.vfs_entries.iter()
            .find(|(p, _)| p.as_str() == "test.txt")
            .expect("test.txt should exist");
        assert_eq!(test_entry.1.content, b"Hello, World!");

        let config_entry = unpacked.vfs_entries.iter()
            .find(|(p, _)| p.as_str() == "data/config.json")
            .expect("config.json should exist");
        assert_eq!(config_entry.1.content, b"{\"key\": \"value\"}");
    }

    #[test]
    fn test_unpack_missing_metadata() {
        // Create invalid archive (just heap, no metadata)
        let mut builder = tar::Builder::new(Vec::new());
        
        let mut header = tar::Header::new_gnu();
        header.set_path(HEAP_FILENAME).unwrap();
        header.set_size(100);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &[0u8; 100][..]).unwrap();
        
        builder.finish().unwrap();
        let archive = builder.into_inner().unwrap();

        let result = unpack_sliver(&archive);
        assert!(matches!(result, Err(SliverError::MissingMetadata { .. })));
    }

    #[test]
    fn test_unpack_missing_heap() {
        // Create invalid archive (metadata but no heap)
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

        let result = unpack_sliver(&archive);
        assert!(matches!(result, Err(SliverError::MissingHeap { .. })));
    }

    #[test]
    fn test_unpack_invalid_version() {
        // Create archive with unsupported version
        let mut metadata = SliverMetadata::new("app.example.com", "1.1.0");
        metadata.format_version = "0.5".to_string(); // Unsupported
        
        let mut builder = tar::Builder::new(Vec::new());
        
        let json = serde_json::to_vec(&metadata).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_path(METADATA_FILENAME).unwrap();
        header.set_size(json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, json.as_slice()).unwrap();
        
        let mut header = tar::Header::new_gnu();
        header.set_path(HEAP_FILENAME).unwrap();
        header.set_size(100);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &[0u8; 100][..]).unwrap();
        
        builder.finish().unwrap();
        let archive = builder.into_inner().unwrap();

        let result = unpack_sliver(&archive);
        assert!(matches!(result, Err(SliverError::InvalidFormat { .. })));
    }

    #[test]
    fn test_unpack_corrupted_archive() {
        // Try to unpack garbage data
        let garbage = vec![0xFFu8; 100];
        let result = unpack_sliver(&garbage);
        
        // Should fail with some kind of error
        assert!(result.is_err());
    }

    #[test]
    fn test_unpacked_sliver_summary() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0u8; 1024];
        
        let unpacked = UnpackedSliver::new(metadata, heap_data, vec![]);
        
        let summary = unpacked.summary();
        assert!(summary.contains("app.example.com"));
        assert!(summary.contains("1.0")); // format version
        assert!(summary.contains("1024 bytes"));
        assert!(summary.contains("0 entries"));
    }
}
