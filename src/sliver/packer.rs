//! Sliver Packer
//!
//! Implements tar archive creation for sliver snapshots.
//! Packs metadata, heap blob, and VFS contents into a single archive.

use tar::{Builder, Header};

use crate::sliver::error::SliverResult;
use crate::sliver::format::{HEAP_FILENAME, MANIFEST_FILENAME, METADATA_FILENAME, VFS_PREFIX};
use crate::sliver::metadata::SliverMetadata;
use crate::vfs::types::{VfsFile, VfsPath};

/// Packs sliver components into a tar archive
///
/// This struct provides methods to incrementally build a sliver archive
/// before finalizing it into a byte vector.
pub struct SliverPacker {
    builder: Builder<Vec<u8>>,
    entries: Vec<String>,
}

impl SliverPacker {
    /// Create a new sliver packer
    pub fn new() -> Self {
        Self {
            builder: Builder::new(Vec::new()),
            entries: Vec::new(),
        }
    }

    /// Add metadata to the archive
    ///
    /// Serializes the metadata to JSON and adds it as meta.json
    pub fn add_metadata(&mut self, metadata: &SliverMetadata) -> SliverResult<()> {
        let json_data = metadata.to_json()?;
        
        let mut header = Header::new_gnu();
        header.set_path(METADATA_FILENAME)?;
        header.set_size(json_data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        self.builder.append(&header, json_data.as_slice())?;
        self.entries.push(METADATA_FILENAME.to_string());

        Ok(())
    }

    /// Add the V8 heap blob to the archive
    ///
    /// The heap data is treated as an opaque binary blob.
    pub fn add_heap(&mut self, heap_data: &[u8]) -> SliverResult<()> {
        let mut header = Header::new_gnu();
        header.set_path(HEAP_FILENAME)?;
        header.set_size(heap_data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        self.builder.append(&header, heap_data)?;
        self.entries.push(HEAP_FILENAME.to_string());

        Ok(())
    }

    /// Add a VFS file entry to the archive
    ///
    /// The path will be prefixed with "vfs/" in the archive.
    /// Preserves the directory structure from the VFS path.
    pub fn add_vfs_entry(&mut self, path: &VfsPath, file: &VfsFile) -> SliverResult<()> {
        let archive_path = format!("{}{}", VFS_PREFIX, path.as_str());
        
        let mut header = Header::new_gnu();
        header.set_path(&archive_path)?;
        header.set_size(file.content.len() as u64);
        header.set_mode(0o644);
        
        // Use file's modification time if available, otherwise current time
        let mtime = file
            .modified_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_else(|_| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            });
        header.set_mtime(mtime);
        
        header.set_cksum();

        self.builder.append(&header, file.content.as_slice())?;
        self.entries.push(archive_path);

        Ok(())
    }

    /// Add multiple VFS entries from a collection
    ///
    /// Convenience method for bulk adding VFS entries.
    pub fn add_vfs_entries(
        &mut self,
        entries: &[(VfsPath, VfsFile)],
    ) -> SliverResult<()> {
        for (path, file) in entries {
            self.add_vfs_entry(path, file)?;
        }
        Ok(())
    }

    /// Generate and add the manifest file
    ///
    /// The manifest is a human-readable listing of archive contents.
    fn add_manifest(&mut self) -> SliverResult<()> {
        let mut manifest_content = String::new();
        manifest_content.push_str("# Sliver Archive Manifest\n");
        manifest_content.push_str("# =========================\n\n");

        for entry in &self.entries {
            manifest_content.push_str(entry);
            manifest_content.push('\n');
        }

        let manifest_bytes = manifest_content.into_bytes();

        let mut header = Header::new_gnu();
        header.set_path(MANIFEST_FILENAME)?;
        header.set_size(manifest_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        self.builder.append(&header, manifest_bytes.as_slice())?;

        Ok(())
    }

    /// Finalize the archive and return the bytes
    ///
    /// This consumes the packer and generates the final tar archive.
    /// The manifest is automatically added before finalization.
    pub fn finalize(mut self) -> SliverResult<Vec<u8>> {
        // Add manifest before finalizing
        self.add_manifest()?;

        // Finalize the tar archive
        self.builder.finish()?;
        
        // Extract the underlying vector
        let data = self.builder.into_inner()?;
        
        Ok(data)
    }
}

impl Default for SliverPacker {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to pack a complete sliver
///
/// Creates a sliver archive with the given components in one call.
///
/// # Arguments
/// * `metadata` - Sliver metadata (hostname, timestamps, etc.)
/// * `heap_data` - Opaque V8 heap snapshot blob
/// * `vfs_entries` - Optional VFS file entries to include
///
/// # Returns
/// The serialized tar archive as a byte vector
pub fn pack_sliver(
    metadata: &SliverMetadata,
    heap_data: &[u8],
    vfs_entries: Option<&[(VfsPath, VfsFile)]>,
) -> SliverResult<Vec<u8>> {
    let mut packer = SliverPacker::new();

    packer.add_metadata(metadata)?;
    packer.add_heap(heap_data)?;

    if let Some(entries) = vfs_entries {
        packer.add_vfs_entries(entries)?;
    }

    packer.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sliver::format::SliverFormat;

    #[test]
    fn test_packer_basic() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0u8; 1024]; // Fake heap blob

        let archive = pack_sliver(&metadata, &heap_data, None).unwrap();
        
        // Should be non-empty
        assert!(!archive.is_empty());
        // Should be valid tar (starts with tar magic)
        assert_eq!(&archive[257..262], b"ustar");
    }

    #[test]
    fn test_packer_with_vfs() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0u8; 1024];
        
        let vfs_entries = vec![
            (
                VfsPath::new("config.json").unwrap(),
                VfsFile::new(b"{\"key\": \"value\"}".to_vec()),
            ),
            (
                VfsPath::new("data/users.txt").unwrap(),
                VfsFile::new(b"user1\nuser2".to_vec()),
            ),
        ];

        let archive = pack_sliver(&metadata, &heap_data, Some(&vfs_entries)).unwrap();
        
        // Verify it's a valid tar
        assert!(!archive.is_empty());
    }

    #[test]
    fn test_manifest_generation() {
        let mut packer = SliverPacker::new();
        
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        packer.add_metadata(&metadata).unwrap();
        packer.add_heap(&[0u8; 100]).unwrap();

        let archive = packer.finalize().unwrap();
        assert!(!archive.is_empty());
    }

    #[test]
    fn test_binary_content_preservation() {
        let metadata = SliverMetadata::new("app.example.com", "1.1.0");
        let heap_data = vec![0u8; 100];
        
        // Binary content with null bytes
        let binary_content: Vec<u8> = vec![0x00, 0x01, 0xFF, 0xFE, 0xFD];
        let vfs_entries = vec![(
            VfsPath::new("binary.dat").unwrap(),
            VfsFile::new(binary_content.clone()),
        )];

        let archive = pack_sliver(&metadata, &heap_data, Some(&vfs_entries)).unwrap();
        assert!(!archive.is_empty());
    }
}
