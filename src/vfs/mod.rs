//! Virtual File System (VFS) Module
//!
//! Provides a trait-based virtual file system with pluggable backends.
//! The VFS supports per-isolate namespaces for multi-tenant isolation.
//!
//! # Architecture
//!
//! - `VfsBackend`: Core trait for storage backends (async, object-safe)
//! - `FileSystem`: User-facing API that wraps a backend with path normalization
//! - `IsolateVfs`: Per-isolate VFS with namespace isolation
//!
//! # Backends
//!
//! - `MemoryBackend`: In-memory storage using DashMap (default, ephemeral)
//! - `DiskBackend`: Filesystem-backed persistent storage
//! - Future: S3Backend (Phase 11)

use async_trait::async_trait;
use std::sync::Arc;

// Re-export all types
pub mod disk;
pub mod isolate;
pub mod memory;
pub mod types;

pub use disk::DiskBackend;
pub use isolate::{IsolateVfs, VfsNamespace};
pub use memory::MemoryBackend;
pub use security::{PathValidator, ResourceLimiter};
pub use types::{ResourceLimits, VfsError, VfsFile, VfsPath, VfsResult};

/// Core trait for VFS storage backends
///
/// This trait is object-safe and supports async operations for future
/// backend implementations that may need async I/O (e.g., S3).
#[async_trait]
pub trait VfsBackend: Send + Sync {
    /// Read file content at the given path
    async fn read(&self, path: &VfsPath) -> VfsResult<Vec<u8>>;

    /// Write file content at the given path
    async fn write(&self, path: &VfsPath, content: &[u8]) -> VfsResult<()>;

    /// Check if a file exists at the given path
    async fn exists(&self, path: &VfsPath) -> VfsResult<bool>;

    /// Delete a file at the given path
    async fn delete(&self, path: &VfsPath) -> VfsResult<()>;

    /// Get file metadata
    async fn metadata(&self, path: &VfsPath) -> VfsResult<VfsFile>;
}

/// User-facing filesystem API
///
/// Wraps a backend and provides path normalization plus optional
/// namespace isolation. This is the primary interface for filesystem
/// operations within an isolate.
pub struct FileSystem {
    backend: Arc<dyn VfsBackend>,
    namespace: Option<String>,
    validator: PathValidator,
}

impl FileSystem {
    /// Create a new FileSystem with the given backend
    pub fn new(backend: Arc<dyn VfsBackend>) -> Self {
        Self {
            backend,
            namespace: None,
            validator: PathValidator::default(),
        }
    }

    /// Create a new FileSystem with a namespace prefix
    pub fn with_namespace(backend: Arc<dyn VfsBackend>, namespace: impl Into<String>) -> Self {
        Self {
            backend,
            namespace: Some(namespace.into()),
            validator: PathValidator::default(),
        }
    }

    /// Set a custom path validator
    pub fn with_validator(mut self, validator: PathValidator) -> Self {
        self.validator = validator;
        self
    }

    /// Read a file
    pub async fn read(&self, path: impl AsRef<str>) -> VfsResult<Vec<u8>> {
        let path = self.validate_and_normalize(path)?;
        let storage_path = self.prefix_namespace(path);
        self.backend.read(&storage_path).await
    }

    /// Write a file
    pub async fn write(&self, path: impl AsRef<str>, content: &[u8]) -> VfsResult<()> {
        let path = self.validate_and_normalize(path)?;
        let storage_path = self.prefix_namespace(path);
        self.backend.write(&storage_path, content).await
    }

    /// Check if a file exists
    pub async fn exists(&self, path: impl AsRef<str>) -> VfsResult<bool> {
        let path = self.validate_and_normalize(path)?;
        let storage_path = self.prefix_namespace(path);
        self.backend.exists(&storage_path).await
    }

    /// Delete a file
    pub async fn delete(&self, path: impl AsRef<str>) -> VfsResult<()> {
        let path = self.validate_and_normalize(path)?;
        let storage_path = self.prefix_namespace(path);
        self.backend.delete(&storage_path).await
    }

    /// Get file metadata
    pub async fn metadata(&self, path: impl AsRef<str>) -> VfsResult<VfsFile> {
        let path = self.validate_and_normalize(path)?;
        let storage_path = self.prefix_namespace(path);
        self.backend.metadata(&storage_path).await
    }

    /// Get the backend reference
    pub fn backend(&self) -> &Arc<dyn VfsBackend> {
        &self.backend
    }

    /// Get the namespace if set
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Validate and normalize a path string
    fn validate_and_normalize(&self, path: impl AsRef<str>) -> VfsResult<VfsPath> {
        let path_str = path.as_ref();

        // First pass: strict validation
        self.validator.validate(path_str)?;

        // Second pass: normalization (also validates)
        VfsPath::new(path_str)
    }

    /// Prefix path with namespace if set
    fn prefix_namespace(&self, path: VfsPath) -> VfsPath {
        match &self.namespace {
            Some(ns) => {
                // Format: "{namespace}::{path}"
                let prefixed = format!("{}::{}", ns, path.as_str());
                // This should always succeed since we already validated
                VfsPath::new(prefixed).unwrap_or(path)
            }
            None => path,
        }
    }
}

/// Security validation and resource limiting layer
///
/// This module provides defense-in-depth security for the VFS:
/// - PathValidator: Strict path validation (rejects traversal attempts)
/// - ResourceLimiter: Enforces file size and storage quotas
pub mod security {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Strict path validator
    ///
    /// Performs first-pass validation before path normalization.
    /// Rejects any suspicious patterns to prevent path traversal attacks.
    #[derive(Debug, Clone)]
    pub struct PathValidator {
        max_path_length: usize,
    }

    impl Default for PathValidator {
        fn default() -> Self {
            Self {
                max_path_length: crate::vfs::types::MAX_PATH_LENGTH,
            }
        }
    }

    impl PathValidator {
        /// Create a validator with custom max path length
        pub fn new(max_path_length: usize) -> Self {
            Self { max_path_length }
        }

        /// Strict validation - rejects anything suspicious
        ///
        /// This is the first line of defense against path traversal.
        /// Normalization happens after this validation.
        pub fn validate(&self, path: &str) -> VfsResult<()> {
            // Check length
            if path.len() > self.max_path_length {
                return Err(VfsError::InvalidPath {
                    path: path.to_string(),
                    reason: format!("Path exceeds max length: {}", self.max_path_length),
                });
            }

            // Check for null bytes
            if path.contains('\0') {
                return Err(VfsError::InvalidPath {
                    path: path.to_string(),
                    reason: "Path contains null bytes".to_string(),
                });
            }

            // Check for traversal attempts (before normalization)
            // We check for ".." as a path component
            for component in path.split('/') {
                if component == ".." {
                    return Err(VfsError::InvalidPath {
                        path: path.to_string(),
                        reason: "Path contains '..' which is not allowed".to_string(),
                    });
                }
            }

            Ok(())
        }
    }

    /// Resource limit tracker
    ///
    /// Maintains current usage statistics and enforces limits.
    /// Used by backends to implement quota enforcement.
    #[derive(Debug)]
    pub struct ResourceLimiter {
        limits: ResourceLimits,
        total_bytes: AtomicUsize,
        file_count: AtomicUsize,
    }

    impl ResourceLimiter {
        /// Create a new limiter with the given limits
        pub fn new(limits: ResourceLimits) -> Self {
            Self {
                limits,
                total_bytes: AtomicUsize::new(0),
                file_count: AtomicUsize::new(0),
            }
        }

        /// Check if a write operation is allowed
        pub fn check_write(&self, file_size: usize, is_new_file: bool) -> VfsResult<()> {
            // Check file size limit
            if file_size > self.limits.max_file_size {
                return Err(VfsError::QuotaExceeded {
                    resource: "file_size".to_string(),
                    limit: self.limits.max_file_size,
                    current: file_size,
                });
            }

            // Check file count limit for new files
            if is_new_file {
                let current_count = self.file_count.load(Ordering::SeqCst);
                if current_count >= self.limits.max_files {
                    return Err(VfsError::QuotaExceeded {
                        resource: "file_count".to_string(),
                        limit: self.limits.max_files,
                        current: current_count,
                    });
                }
            }

            Ok(())
        }

        /// Check total storage limit with a proposed delta
        pub fn check_total_storage(&self, size_delta: i64, current_total: usize) -> VfsResult<()> {
            let new_total = (current_total as i64 + size_delta) as usize;

            if new_total > self.limits.max_total_storage {
                return Err(VfsError::QuotaExceeded {
                    resource: "total_storage".to_string(),
                    limit: self.limits.max_total_storage,
                    current: current_total,
                });
            }

            Ok(())
        }

        /// Record a file creation
        pub fn record_create(&self, size: usize) {
            self.file_count.fetch_add(1, Ordering::SeqCst);
            self.total_bytes.fetch_add(size, Ordering::SeqCst);
        }

        /// Record a file deletion
        pub fn record_delete(&self, size: usize) {
            self.file_count.fetch_sub(1, Ordering::SeqCst);
            self.total_bytes.fetch_sub(size, Ordering::SeqCst);
        }

        /// Record a file update (size change)
        pub fn record_update(&self, old_size: usize, new_size: usize) {
            let delta = new_size as i64 - old_size as i64;
            if delta > 0 {
                self.total_bytes.fetch_add(delta as usize, Ordering::SeqCst);
            } else if delta < 0 {
                self.total_bytes.fetch_sub((-delta) as usize, Ordering::SeqCst);
            }
        }

        /// Get current usage statistics
        pub fn current_usage(&self) -> (usize, usize) {
            (
                self.file_count.load(Ordering::SeqCst),
                self.total_bytes.load(Ordering::SeqCst),
            )
        }

        /// Get the limits
        pub fn limits(&self) -> &ResourceLimits {
            &self.limits
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_filesystem_basic() {
        let backend = Arc::new(memory::MemoryBackend::default());
        let fs = FileSystem::new(backend);

        // Write and read back
        fs.write("test.txt", b"hello world").await.unwrap();
        let content = fs.read("test.txt").await.unwrap();
        assert_eq!(content, b"hello world");
    }

    #[tokio::test]
    async fn test_filesystem_with_namespace() {
        let backend = Arc::new(memory::MemoryBackend::default());
        let fs = FileSystem::with_namespace(backend, "app1");

        fs.write("test.txt", b"namespaced").await.unwrap();
        assert!(fs.exists("test.txt").await.unwrap());

        // Same backend, different namespace - should not see the file
        let fs2 = FileSystem::with_namespace(Arc::clone(fs.backend()), "app2");
        assert!(!fs2.exists("test.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_filesystem_traversal_blocked() {
        let backend = Arc::new(memory::MemoryBackend::default());
        let fs = FileSystem::new(backend);

        let result = fs.read("../etc/passwd").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "EINVAL");
    }
}
