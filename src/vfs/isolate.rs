//! Per-Isolate VFS Integration
//!
//! Provides the IsolateVfs wrapper that attaches a VFS namespace to each isolate.
//! This module implements the per-isolate filesystem isolation required for
//! multi-tenant security.

use std::sync::Arc;

use crate::vfs::types::{VfsError, VfsPath, VfsResult};
use crate::vfs::VfsBackend;

/// A namespace for VFS isolation
///
/// Derived from the application hostname, this ensures each app
/// has an isolated filesystem that cannot access other apps' files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VfsNamespace(String);

impl VfsNamespace {
    /// Create a namespace from an application hostname
    ///
    /// Sanitizes the hostname by:
    /// - Converting to lowercase
    /// - Replacing '.' with '_'
    /// - Replacing '-' with '_'
    pub fn from_hostname(hostname: &str) -> Self {
        let sanitized = hostname
            .to_lowercase()
            .replace('.', "_")
            .replace('-', "_");
        Self(sanitized)
    }

    /// Get the namespace as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VfsNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

use std::fmt;

/// Per-isolate VFS wrapper
///
/// Combines a namespace with a backend to provide isolated filesystem
/// access for a single isolate. This is owned by NanoIsolate.
pub struct IsolateVfs {
    namespace: VfsNamespace,
    backend: Arc<dyn VfsBackend>,
}

impl IsolateVfs {
    /// Create a new IsolateVfs with the given namespace and backend
    pub fn new(namespace: VfsNamespace, backend: Arc<dyn VfsBackend>) -> Self {
        Self {
            namespace,
            backend,
        }
    }

    /// Get the namespace
    pub fn namespace(&self) -> &VfsNamespace {
        &self.namespace
    }

    /// Get the backend reference
    pub fn backend(&self) -> &Arc<dyn VfsBackend> {
        &self.backend
    }

    /// Read a file from the isolate's namespace
    pub async fn read(&self, path: impl AsRef<str>) -> VfsResult<Vec<u8>> {
        let storage_path = self.prefix_namespace(path.as_ref())?;
        self.backend.read(&storage_path).await
    }

    /// Write a file to the isolate's namespace
    pub async fn write(&self, path: impl AsRef<str>, content: &[u8]) -> VfsResult<()> {
        let storage_path = self.prefix_namespace(path.as_ref())?;
        self.backend.write(&storage_path, content).await
    }

    /// Check if a file exists in the isolate's namespace
    pub async fn exists(&self, path: impl AsRef<str>) -> VfsResult<bool> {
        let storage_path = self.prefix_namespace(path.as_ref())?;
        self.backend.exists(&storage_path).await
    }

    /// Delete a file from the isolate's namespace
    pub async fn delete(&self, path: impl AsRef<str>) -> VfsResult<()> {
        let storage_path = self.prefix_namespace(path.as_ref())?;
        self.backend.delete(&storage_path).await
    }

    /// Get file metadata from the isolate's namespace
    pub async fn metadata(&self, path: impl AsRef<str>) -> VfsResult<crate::vfs::types::VfsFile> {
        let storage_path = self.prefix_namespace(path.as_ref())?;
        self.backend.metadata(&storage_path).await
    }

    /// Prefix a path with the namespace for storage
    ///
    /// Format: "{namespace}::{normalized_path}"
    fn prefix_namespace(&self, path: &str) -> VfsResult<VfsPath> {
        // Validate and normalize the user path
        let normalized = VfsPath::new(path)?;

        // Prefix with namespace for storage isolation
        let storage_key = format!("{}::{}", self.namespace.as_str(), normalized.as_str());

        // This should always succeed since both parts are validated
        Ok(VfsPath::new(storage_key)?)
    }
}

impl fmt::Debug for IsolateVfs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IsolateVfs")
            .field("namespace", &self.namespace)
            .field("backend", &"<dyn VfsBackend>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::MemoryBackend;

    #[test]
    fn test_vfs_namespace_from_hostname() {
        assert_eq!(
            VfsNamespace::from_hostname("api.example.com").as_str(),
            "api_example_com"
        );
        assert_eq!(
            VfsNamespace::from_hostname("APP.EXAMPLE.COM").as_str(),
            "app_example_com"
        );
        assert_eq!(
            VfsNamespace::from_hostname("my-app.example.com").as_str(),
            "my_app_example_com"
        );
    }

    #[tokio::test]
    async fn test_isolate_vfs_basic() {
        let backend = Arc::new(MemoryBackend::default());
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("test.example.com"),
            backend
        );

        // Write
        vfs.write("/config.json", b"{\"key\": \"value\"}").await.unwrap();

        // Read
        let content = vfs.read("/config.json").await.unwrap();
        assert_eq!(content, b"{\"key\": \"value\"}");

        // Exists
        assert!(vfs.exists("/config.json").await.unwrap());
        assert!(!vfs.exists("/missing.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_isolate_vfs_namespace_isolation() {
        let shared_backend: Arc<dyn crate::vfs::VfsBackend> = Arc::new(MemoryBackend::default());

        // Two isolates with different namespaces sharing the same backend
        let vfs_a = IsolateVfs::new(
            VfsNamespace::from_hostname("app-a.example.com"),
            Arc::clone(&shared_backend)
        );

        let vfs_b = IsolateVfs::new(
            VfsNamespace::from_hostname("app-b.example.com"),
            Arc::clone(&shared_backend)
        );

        // Write in app A
        vfs_a.write("/secret.txt", b"app-a-secret").await.unwrap();

        // App B cannot read
        let result = vfs_b.read("/secret.txt").await;
        assert!(matches!(result, Err(VfsError::NotFound { .. })));

        // App A can read
        let content = vfs_a.read("/secret.txt").await.unwrap();
        assert_eq!(content, b"app-a-secret");
    }

    #[tokio::test]
    async fn test_isolate_vfs_path_traversal_blocked() {
        let backend = Arc::new(MemoryBackend::default());
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("test.example.com"),
            backend
        );

        // Create a file
        vfs.write("/data/file.txt", b"content").await.unwrap();

        // Try traversal - should be blocked
        let result = vfs.read("../data/file.txt").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "EINVAL");

        let result = vfs.read("data/../../etc/passwd").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "EINVAL");
    }

    #[tokio::test]
    async fn test_isolate_vfs_unicode_paths() {
        let backend = Arc::new(MemoryBackend::default());
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("test.example.com"),
            backend
        );

        // Unicode paths should work
        vfs.write("/文件.txt", b"content").await.unwrap();
        let content = vfs.read("/文件.txt").await.unwrap();
        assert_eq!(content, b"content");
    }
}
