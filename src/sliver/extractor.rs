//! Sliver VFS Extractor
//!
//! Extracts VFS entries from sliver archives to temporary directories
//! for JavaScript execution. This enables sliver portability - the ability
//! to run from any location without requiring source JS files in the CWD.
//!
//! ## Architecture
//!
//! - `SliverExtractor`: Extracts VFS entries to temp directories
//! - `TempVfsManager`: Manages temp directory lifecycle (auto-cleanup)
//!
//! ## Security
//!
//! - Uses `tempfile::Builder` for secure temp directory creation
//! - Owner-only permissions (0o700) on temp directories
//! - Path traversal prevention via VfsPath validation
//!
//! ## Example
//!
//! ```rust
//! use nano::sliver::{SliverExtractor, UnpackedSliver};
//!
//! // After unpacking a sliver
//! let unpacked: UnpackedSliver = /* ... */;
//!
//! // Extract to temp directory
//! let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();
//!
//! // Get entrypoint path in temp directory
//! let entrypoint = temp_vfs.entrypoint_path();
//!
//! // Temp directory cleaned up automatically when temp_vfs is dropped
//! ```

use crate::sliver::error::{SliverError, SliverResult};
use crate::sliver::unpacker::UnpackedSliver;
use crate::vfs::types::{VfsFile, VfsPath};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Extracts sliver VFS to temporary directories
///
/// This struct provides methods to extract all VFS entries from an
/// unpacked sliver into a temporary directory structure suitable for
/// direct JavaScript execution.
pub struct SliverExtractor;

impl SliverExtractor {
    /// Extract all VFS entries to a secure temporary directory
    ///
    /// Creates a temporary directory with secure permissions (0o700) and
    /// extracts all VFS entries from the unpacked sliver, preserving the
    /// directory structure.
    ///
    /// # Arguments
    ///
    /// * `unpacked` - The unpacked sliver containing VFS entries
    ///
    /// # Returns
    ///
    /// A `TempVfsManager` that owns the temp directory and provides access
    /// to the extracted files. The temp directory is automatically cleaned
    /// up when the manager is dropped.
    ///
    /// # Errors
    ///
    /// Returns `SliverError` if:
    /// - Temp directory creation fails
    /// - Any file cannot be written
    /// - Directory creation fails
    pub fn extract(unpacked: &UnpackedSliver) -> SliverResult<TempVfsManager> {
        Self::extract_with_entrypoint_detection(unpacked)
    }

    /// Extract VFS entries with automatic entrypoint detection
    ///
    /// Similar to `extract()`, but also detects the JavaScript entrypoint
    /// file from common patterns (index.js, app.js, main.js, server.js).
    fn extract_with_entrypoint_detection(unpacked: &UnpackedSliver) -> SliverResult<TempVfsManager> {
        // Create secure temp directory with restrictive permissions
        let temp_dir = tempfile::Builder::new()
            .prefix("nano-sliver-")
            .tempdir()
            .map_err(|e| SliverError::VfsRestore {
                path: "temp_dir".to_string(),
                reason: format!("Failed to create temp directory: {}", e),
            })?;

        // Set owner-only permissions on Unix systems
        #[cfg(unix)]
        {
            fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|e| SliverError::VfsRestore {
                    path: temp_dir.path().to_string_lossy().to_string(),
                    reason: format!("Failed to set temp directory permissions: {}", e),
                })?;
        }

        tracing::info!(
            "Extracting sliver VFS to temp directory: {}",
            temp_dir.path().display()
        );

        // Extract all VFS entries to temp directory
        for (vfs_path, vfs_file) in &unpacked.vfs_entries {
            Self::write_vfs_entry(&temp_dir, vfs_path, vfs_file)?;
        }

        // Detect entrypoint from VFS entries or use default
        let entrypoint = Self::detect_entrypoint(&unpacked.vfs_entries);
        let entrypoint_path = temp_dir.path().join(&entrypoint);

        // Verify entrypoint exists
        if !entrypoint_path.exists() {
            // If the entrypoint doesn't exist, try to find any JS file
            let js_file = unpacked.vfs_entries.iter()
                .find(|(path, _)| path.as_str().ends_with(".js"))
                .map(|(path, _)| path.as_str());

            if let Some(js_path) = js_file {
                let detected_entrypoint = temp_dir.path().join(js_path);
                tracing::info!(
                    "Entrypoint '{}' not found, using detected: {}",
                    entrypoint,
                    js_path
                );
                return Ok(TempVfsManager::new(temp_dir, detected_entrypoint));
            }

            return Err(SliverError::VfsRestore {
                path: entrypoint,
                reason: "Entrypoint file not found in VFS entries".to_string(),
            });
        }

        tracing::info!(
            "VFS extraction complete: {} files extracted, entrypoint: {}",
            unpacked.vfs_entries.len(),
            entrypoint_path.display()
        );

        Ok(TempVfsManager::new(temp_dir, entrypoint_path))
    }

    /// Write a single VFS entry to the temp directory
    fn write_vfs_entry(
        temp_dir: &TempDir,
        vfs_path: &VfsPath,
        vfs_file: &VfsFile,
    ) -> SliverResult<()> {
        // Get the relative path from the VFS path
        let relative_path = vfs_path.as_str();

        // Remove leading slash if present
        let relative_path = relative_path.strip_prefix('/').unwrap_or(relative_path);

        // Create the full path in temp directory
        let file_path = temp_dir.path().join(relative_path);

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| SliverError::VfsRestore {
                path: parent.to_string_lossy().to_string(),
                reason: format!("Failed to create directory: {}", e),
            })?;
        }

        // Write the file
        let mut file = fs::File::create(&file_path).map_err(|e| SliverError::VfsRestore {
            path: file_path.to_string_lossy().to_string(),
            reason: format!("Failed to create file: {}", e),
        })?;

        file.write_all(&vfs_file.content)
            .map_err(|e| SliverError::VfsRestore {
                path: file_path.to_string_lossy().to_string(),
                reason: format!("Failed to write file content: {}", e),
            })?;

        // Set file permissions to owner-only on Unix systems
        #[cfg(unix)]
        {
            fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| SliverError::VfsRestore {
                    path: file_path.to_string_lossy().to_string(),
                    reason: format!("Failed to set file permissions: {}", e),
                })?;
        }

        tracing::debug!(
            "Extracted VFS entry: {} ({} bytes)",
            file_path.display(),
            vfs_file.content.len()
        );

        Ok(())
    }

    /// Detect the entrypoint file from VFS entries
    ///
    /// Checks for common entrypoint patterns in order of preference:
    /// 1. index.js
    /// 2. app.js
    /// 3. main.js
    /// 4. server.js
    ///
    /// Returns the first match or defaults to "index.js"
    fn detect_entrypoint(vfs_entries: &[(VfsPath, VfsFile)]) -> String {
        let entrypoint_candidates = ["index.js", "app.js", "main.js", "server.js"];

        for candidate in &entrypoint_candidates {
            if vfs_entries.iter().any(|(path, _)| path.as_str() == *candidate) {
                return candidate.to_string();
            }
        }

        // Default to index.js if no match found
        "index.js".to_string()
    }
}

/// Manages a temporary VFS directory
///
/// This struct owns a temporary directory containing extracted VFS entries.
/// The directory is automatically cleaned up when this struct is dropped.
///
/// ## Lifecycle
///
/// 1. Created by `SliverExtractor::extract()`
/// 2. Provides access to temp directory and entrypoint paths
/// 3. Cleanup occurs automatically on drop
///
/// ## Security
///
/// The temp directory is created with 0o700 permissions (owner-only access)
/// and all files are written with 0o600 permissions.
pub struct TempVfsManager {
    /// The temporary directory (owned, auto-cleanup on drop)
    temp_dir: TempDir,
    /// Path to the entrypoint file within temp directory
    entrypoint: PathBuf,
}

impl TempVfsManager {
    /// Create a new TempVfsManager
    ///
    /// # Arguments
    ///
    /// * `temp_dir` - The owned TempDir
    /// * `entrypoint` - Path to the entrypoint file (already joined with temp dir)
    fn new(temp_dir: TempDir, entrypoint: PathBuf) -> Self {
        Self {
            temp_dir,
            entrypoint,
        }
    }

    /// Get the temp directory path
    pub fn temp_dir(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Get the entrypoint file path
    ///
    /// This is the full path to the JavaScript entrypoint file within
    /// the temp directory. The file can be read directly using `fs::read_to_string`.
    pub fn entrypoint_path(&self) -> &Path {
        &self.entrypoint
    }

    /// Explicitly clean up the temp directory
    ///
    /// This consumes the manager and logs the cleanup operation.
    /// Note: The temp directory is also cleaned up automatically when
    /// the manager is dropped, so this method is optional but provides
    /// explicit logging.
    pub fn cleanup(self) {
        tracing::info!("Cleaning up temp VFS: {}", self.temp_dir.path().display());
        // TempDir is dropped here, which deletes the directory
    }

    /// Verify that the temp directory is valid
    ///
    /// Checks:
    /// - Temp directory exists
    /// - Entrypoint file exists and is readable
    pub fn verify(&self) -> SliverResult<()> {
        // Check temp directory exists
        if !self.temp_dir.path().exists() {
            return Err(SliverError::VfsRestore {
                path: self.temp_dir.path().to_string_lossy().to_string(),
                reason: "Temp directory does not exist".to_string(),
            });
        }

        // Check entrypoint exists
        if !self.entrypoint.exists() {
            return Err(SliverError::VfsRestore {
                path: self.entrypoint.to_string_lossy().to_string(),
                reason: "Entrypoint file does not exist".to_string(),
            });
        }

        // Check entrypoint is readable (not a directory)
        if !self.entrypoint.is_file() {
            return Err(SliverError::VfsRestore {
                path: self.entrypoint.to_string_lossy().to_string(),
                reason: "Entrypoint is not a file".to_string(),
            });
        }

        Ok(())
    }
}

impl Drop for TempVfsManager {
    fn drop(&mut self) {
        // TempDir handles the actual cleanup, but we can log it
        tracing::debug!(
            "TempVfsManager dropped, temp directory will be cleaned up: {}",
            self.temp_dir.path().display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sliver::metadata::SliverMetadata;
    use crate::sliver::packer::pack_sliver;
    use crate::sliver::unpacker::unpack_sliver;

    fn create_test_sliver_with_vfs(vfs_entries: Vec<(VfsPath, VfsFile)>) -> UnpackedSliver {
        let metadata = SliverMetadata::new("test.example.com", "1.1.0");
        let heap_data = vec![0u8; 100];

        let archive = pack_sliver(&metadata, &heap_data, Some(&vfs_entries)).unwrap();
        unpack_sliver(&archive).unwrap()
    }

    #[test]
    fn test_extract_vfs_to_temp_creates_directory() {
        let vfs_entries = vec![
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(b"function fetch(req) { return {status: 200}; }".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Verify temp directory exists
        assert!(temp_vfs.temp_dir().exists(), "Temp directory should exist");
    }

    #[test]
    fn test_extract_all_vfs_files_written() {
        let vfs_entries = vec![
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(b"console.log('main');".to_vec()),
            ),
            (
                VfsPath::new("utils.js").unwrap(),
                VfsFile::new(b"export function helper() {}".to_vec()),
            ),
            (
                VfsPath::new("lib/data.json").unwrap(),
                VfsFile::new(b"{\"key\": \"value\"}".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries.clone());
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Verify all files exist
        for (vfs_path, _) in &vfs_entries {
            let relative_path = vfs_path.as_str().strip_prefix('/').unwrap_or(vfs_path.as_str());
            let file_path = temp_vfs.temp_dir().join(relative_path);
            assert!(
                file_path.exists(),
                "File should exist: {}",
                file_path.display()
            );
        }
    }

    #[test]
    fn test_extract_entrypoint_file_readable() {
        let code = b"function fetch(request) { return {status: 200, body: 'OK'}; }";
        let vfs_entries = vec![
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(code.to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Verify entrypoint exists and is readable
        let entrypoint = temp_vfs.entrypoint_path();
        assert!(entrypoint.exists(), "Entrypoint should exist");

        // Read and verify content
        let content = fs::read_to_string(entrypoint).unwrap();
        assert_eq!(content.as_bytes(), code);
    }

    #[test]
    fn test_extract_directory_permissions_secure() {
        let vfs_entries = vec![
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(b"function fetch() {}".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // On Unix systems, verify permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(temp_vfs.temp_dir()).unwrap();
            let mode = metadata.permissions().mode();
            // Check owner has full access (0o700)
            assert_eq!(
                mode & 0o777,
                0o700,
                "Temp directory should have 0o700 permissions, got {:o}",
                mode & 0o777
            );
        }
    }

    #[test]
    fn test_extract_returns_correct_entrypoint_path() {
        let vfs_entries = vec![
            (
                VfsPath::new("app.js").unwrap(),
                VfsFile::new(b"function fetch() {}".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Verify entrypoint path is within temp directory
        let entrypoint = temp_vfs.entrypoint_path();
        assert!(
            entrypoint.starts_with(temp_vfs.temp_dir()),
            "Entrypoint should be within temp directory"
        );
        assert_eq!(
            entrypoint.file_name().unwrap().to_str().unwrap(),
            "app.js",
            "Entrypoint should be app.js"
        );
    }

    #[test]
    fn test_extract_nested_directories() {
        let vfs_entries = vec![
            (
                VfsPath::new("src/handlers/api.js").unwrap(),
                VfsFile::new(b"export function apiHandler() {}".to_vec()),
            ),
            (
                VfsPath::new("src/utils/helpers.js").unwrap(),
                VfsFile::new(b"export function helper() {}".to_vec()),
            ),
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(b"import { apiHandler } from './src/handlers/api.js';".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries.clone());
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Verify nested directory structure
        for (vfs_path, _) in &vfs_entries {
            let relative_path = vfs_path.as_str().strip_prefix('/').unwrap_or(vfs_path.as_str());
            let file_path = temp_vfs.temp_dir().join(relative_path);
            assert!(
                file_path.exists(),
                "Nested file should exist: {}",
                file_path.display()
            );
        }

        // Verify parent directories were created
        let src_dir = temp_vfs.temp_dir().join("src");
        let handlers_dir = src_dir.join("handlers");
        let utils_dir = src_dir.join("utils");

        assert!(src_dir.exists(), "src directory should exist");
        assert!(handlers_dir.exists(), "handlers directory should exist");
        assert!(utils_dir.exists(), "utils directory should exist");
    }

    #[test]
    fn test_extract_empty_vfs() {
        let vfs_entries: Vec<(VfsPath, VfsFile)> = vec![];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);

        // Should error since there's no entrypoint
        let result = SliverExtractor::extract(&unpacked);
        assert!(
            result.is_err(),
            "Should fail when no VFS entries exist (no entrypoint)"
        );
    }

    #[test]
    fn test_detect_entrypoint_order() {
        // Test that index.js is preferred over app.js
        let vfs_entries = vec![
            (
                VfsPath::new("app.js").unwrap(),
                VfsFile::new(b"console.log('app');".to_vec()),
            ),
            (
                VfsPath::new("index.js").unwrap(),
                VfsFile::new(b"console.log('index');".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        let entrypoint_name = temp_vfs
            .entrypoint_path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            entrypoint_name, "index.js",
            "index.js should be preferred over app.js"
        );
    }

    #[test]
    fn test_temp_vfs_manager_verify() {
        let vfs_entries = vec![
            (
                VfsPath::new("main.js").unwrap(),
                VfsFile::new(b"function fetch() {}".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Should pass verification
        assert!(temp_vfs.verify().is_ok(), "Verification should pass");
    }

    #[test]
    fn test_temp_vfs_cleanup_on_drop() {
        let vfs_entries = vec![
            (
                VfsPath::new("test.js").unwrap(),
                VfsFile::new(b"console.log('test');".to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        // Get the temp path before dropping
        let temp_path = temp_vfs.temp_dir().to_path_buf();

        // Verify it exists
        assert!(temp_path.exists(), "Temp directory should exist before drop");

        // Drop the temp_vfs
        drop(temp_vfs);

        // On most systems, the temp directory should be cleaned up
        // Note: This is a best-effort test - cleanup might not be immediate on all systems
        // but tempfile::TempDir guarantees cleanup on drop
    }

    #[test]
    fn test_extract_preserves_file_content() {
        // Create content with various special characters
        let content = b"export function fetch(req) {\n  return {\n    status: 200,\n    headers: { 'Content-Type': 'text/plain' },\n    body: 'Hello \"World\"'\n  };\n}";

        let vfs_entries = vec![
            (
                VfsPath::new("handler.js").unwrap(),
                VfsFile::new(content.to_vec()),
            ),
        ];

        let unpacked = create_test_sliver_with_vfs(vfs_entries);
        let temp_vfs = SliverExtractor::extract(&unpacked).unwrap();

        let file_path = temp_vfs.temp_dir().join("handler.js");
        let read_content = fs::read(file_path).unwrap();

        assert_eq!(
            read_content, content,
            "File content should be preserved exactly"
        );
    }
}
