//! In-Memory VFS Backend
//!
//! Provides a fast, in-memory storage backend using DashMap for concurrent access.
//! This is the default backend for NANO's VFS and supports resource limiting.

use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::vfs::types::{ResourceLimits, VfsError, VfsFile, VfsPath, VfsResult};
use crate::vfs::VfsBackend;

/// In-memory storage backend
///
/// Uses DashMap for lock-free concurrent access and maintains
/// atomic counters for resource limit tracking.
#[derive(Debug)]
pub struct MemoryBackend {
    /// Storage map: path -> file metadata and content
    storage: DashMap<String, VfsFile>,
    /// Resource limits for this backend
    limits: ResourceLimits,
    /// Current total bytes stored
    total_bytes: AtomicUsize,
    /// Current file count
    file_count: AtomicUsize,
}

impl MemoryBackend {
    /// Create a new MemoryBackend with default limits
    pub fn new() -> Self {
        Self::with_limits(ResourceLimits::default())
    }

    /// Create a new MemoryBackend with custom limits
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            storage: DashMap::new(),
            limits,
            total_bytes: AtomicUsize::new(0),
            file_count: AtomicUsize::new(0),
        }
    }

    /// Clear all stored files
    pub fn clear(&self) {
        self.storage.clear();
        self.total_bytes.store(0, Ordering::SeqCst);
        self.file_count.store(0, Ordering::SeqCst);
    }

    /// Get the number of files stored
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Check if no files are stored
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Get current storage usage (file count, total bytes)
    pub fn current_usage(&self) -> (usize, usize) {
        (
            self.file_count.load(Ordering::SeqCst),
            self.total_bytes.load(Ordering::SeqCst),
        )
    }

    /// Get the resource limits
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Check if we can write a file of the given size
    fn check_write_limits(&self, _path: &VfsPath, content_len: usize, is_new: bool, old_size: usize) -> VfsResult<()> {
        // Check file size limit
        if content_len > self.limits.max_file_size {
            return Err(VfsError::QuotaExceeded {
                resource: "file_size".to_string(),
                limit: self.limits.max_file_size,
                current: content_len,
            });
        }

        if is_new {
            // Check file count limit
            let current_count = self.file_count.load(Ordering::SeqCst);
            if current_count >= self.limits.max_files {
                return Err(VfsError::QuotaExceeded {
                    resource: "file_count".to_string(),
                    limit: self.limits.max_files,
                    current: current_count,
                });
            }
        }

        // Calculate size delta
        let size_delta = content_len as i64 - old_size as i64;
        let current_total = self.total_bytes.load(Ordering::SeqCst) as i64;
        let new_total = (current_total + size_delta) as usize;

        // Check total storage limit
        if new_total > self.limits.max_total_storage {
            return Err(VfsError::QuotaExceeded {
                resource: "total_storage".to_string(),
                limit: self.limits.max_total_storage,
                current: current_total as usize,
            });
        }

        Ok(())
    }

}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend {
    /// Get all stored files as (path, file) pairs for snapshot serialization
    ///
    /// This method is used by the sliver packer to capture the complete
    /// VFS state for snapshot creation.
    pub fn snapshot_entries(&self) -> Vec<(VfsPath, VfsFile)> {
        self.storage
            .iter()
            .filter_map(|entry| {
                let path_str = entry.key();
                match VfsPath::new(path_str) {
                    Ok(path) => {
                        let file = entry.value().clone();
                        Some((path, file))
                    }
                    Err(_) => None, // Skip invalid paths
                }
            })
            .collect()
    }

    /// Restore entries from a snapshot
    ///
    /// Clears existing data and populates from the given entries.
    /// Used by the sliver unpacker to restore VFS state.
    pub fn restore_from_snapshot(&self, entries: &[(VfsPath, VfsFile)]) {
        self.clear();
        
        let mut total_bytes: usize = 0;
        for (path, file) in entries {
            total_bytes += file.content.len();
            self.storage.insert(path.as_str().to_string(), file.clone());
        }
        
        self.file_count.store(entries.len(), Ordering::SeqCst);
        self.total_bytes.store(total_bytes, Ordering::SeqCst);
    }
}

#[async_trait]
impl VfsBackend for MemoryBackend {
    async fn read(&self, path: &VfsPath) -> VfsResult<Vec<u8>> {
        match self.storage.get(path.as_str()) {
            Some(entry) => Ok(entry.content.clone()),
            None => Err(VfsError::NotFound {
                path: path.to_string(),
            }),
        }
    }

    async fn write(&self, path: &VfsPath, content: &[u8]) -> VfsResult<()> {
        let content_len = content.len();

        // Check if this is a new file and get old size BEFORE checking limits
        let is_new = !self.storage.contains_key(path.as_str());
        let old_size = if is_new {
            0
        } else {
            self.storage
                .get(path.as_str())
                .map(|entry| entry.content.len())
                .unwrap_or(0)
        };

        // Check limits
        self.check_write_limits(path, content_len, is_new, old_size)?;

        let now = std::time::SystemTime::now();

        let file = if is_new {
            VfsFile {
                content: content.to_vec(),
                created_at: now,
                modified_at: now,
                size: content_len,
            }
        } else {
            // Preserve creation time for existing files
            let existing = self.storage.get(path.as_str()).unwrap();
            VfsFile {
                content: content.to_vec(),
                created_at: existing.created_at,
                modified_at: now,
                size: content_len,
            }
        };

        // Store the file
        self.storage.insert(path.as_str().to_string(), file);

        // Update counters
        if is_new {
            self.file_count.fetch_add(1, Ordering::SeqCst);
        }
        let size_delta = content_len as i64 - old_size as i64;
        if size_delta > 0 {
            self.total_bytes.fetch_add(size_delta as usize, Ordering::SeqCst);
        } else if size_delta < 0 {
            self.total_bytes.fetch_sub((-size_delta) as usize, Ordering::SeqCst);
        }

        Ok(())
    }

    async fn exists(&self, path: &VfsPath) -> VfsResult<bool> {
        Ok(self.storage.contains_key(path.as_str()))
    }

    async fn delete(&self, path: &VfsPath) -> VfsResult<()> {
        match self.storage.remove(path.as_str()) {
            Some((_, file)) => {
                self.file_count.fetch_sub(1, Ordering::SeqCst);
                self.total_bytes.fetch_sub(file.size, Ordering::SeqCst);
                Ok(())
            }
            None => Err(VfsError::NotFound {
                path: path.to_string(),
            }),
        }
    }

    async fn metadata(&self, path: &VfsPath) -> VfsResult<VfsFile> {
        match self.storage.get(path.as_str()) {
            Some(entry) => Ok(entry.clone()),
            None => Err(VfsError::NotFound {
                path: path.to_string(),
            }),
        }
    }

    async fn list_dir(&self, path: &VfsPath) -> VfsResult<Vec<VfsPath>> {
        let prefix = path.as_str();
        let prefix_with_slash = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{}/", prefix)
        };

        let mut entries = std::collections::HashSet::new();

        for key in self.storage.iter() {
            let key_str = key.key();
            if key_str.starts_with(&prefix_with_slash) {
                // Get the remaining path after prefix
                let remaining = &key_str[prefix_with_slash.len()..];
                // Get first segment (immediate child)
                if let Some(slash_pos) = remaining.find('/') {
                    let child = &remaining[..slash_pos];
                    entries.insert(child.to_string());
                } else if !remaining.is_empty() {
                    // Direct file in this directory
                    entries.insert(remaining.to_string());
                }
            }
        }

        let paths: Vec<VfsPath> = entries
            .into_iter()
            .map(|name| {
                let full_path = format!("{}{}", prefix_with_slash, name);
                VfsPath::new(full_path).unwrap()
            })
            .collect();

        Ok(paths)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_backend_basic() {
        let backend = MemoryBackend::default();
        let path = VfsPath::new("test.txt").unwrap();

        // Write
        backend.write(&path, b"hello world").await.unwrap();

        // Read
        let content = backend.read(&path).await.unwrap();
        assert_eq!(content, b"hello world");

        // Exists
        assert!(backend.exists(&path).await.unwrap());

        // Delete
        backend.delete(&path).await.unwrap();
        assert!(!backend.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_memory_backend_empty_file() {
        let backend = MemoryBackend::default();
        let path = VfsPath::new("empty.txt").unwrap();

        // Write empty content
        backend.write(&path, b"").await.unwrap();

        // Read back
        let content = backend.read(&path).await.unwrap();
        assert!(content.is_empty());

        // Check metadata
        let meta = backend.metadata(&path).await.unwrap();
        assert_eq!(meta.size, 0);
    }

    #[tokio::test]
    async fn test_memory_backend_not_found() {
        let backend = MemoryBackend::default();
        let path = VfsPath::new("nonexistent.txt").unwrap();

        let result = backend.read(&path).await;
        assert!(matches!(result, Err(VfsError::NotFound { .. })));
        assert_eq!(result.unwrap_err().code(), "ENOENT");
    }

    #[tokio::test]
    async fn test_memory_backend_update() {
        let backend = MemoryBackend::default();
        let path = VfsPath::new("update.txt").unwrap();

        // Create file
        backend.write(&path, b"first").await.unwrap();
        let meta1 = backend.metadata(&path).await.unwrap();

        // Update file
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        backend.write(&path, b"second version").await.unwrap();
        let meta2 = backend.metadata(&path).await.unwrap();

        // Creation time preserved, modification time updated
        assert_eq!(meta1.created_at, meta2.created_at);
        assert!(meta2.modified_at > meta1.modified_at);
        assert_eq!(meta2.content, b"second version");
    }

    #[tokio::test]
    async fn test_memory_backend_quota_file_size() {
        let limits = ResourceLimits {
            max_file_size: 100,
            ..Default::default()
        };
        let backend = MemoryBackend::with_limits(limits);
        let path = VfsPath::new("large.txt").unwrap();

        // Small file OK
        backend.write(&path, &[0u8; 50]).await.unwrap();

        // Large file rejected
        let result = backend.write(&path, &[0u8; 101]).await;
        assert!(matches!(result, Err(VfsError::QuotaExceeded { ref resource, .. }) if resource == "file_size"));
        assert_eq!(result.unwrap_err().code(), "EQUOTA");
    }

    #[tokio::test]
    async fn test_memory_backend_quota_file_count() {
        let limits = ResourceLimits {
            max_files: 3,
            ..Default::default()
        };
        let backend = MemoryBackend::with_limits(limits);

        // Create 3 files
        for i in 0..3 {
            let path = VfsPath::new(&format!("file{}.txt", i)).unwrap();
            backend.write(&path, b"content").await.unwrap();
        }

        // 4th file rejected
        let path = VfsPath::new("file3.txt").unwrap();
        let result = backend.write(&path, b"content").await;
        assert!(matches!(result, Err(VfsError::QuotaExceeded { ref resource, .. }) if resource == "file_count"));
    }

    #[tokio::test]
    async fn test_memory_backend_quota_total_storage() {
        let limits = ResourceLimits {
            max_total_storage: 200,
            max_file_size: 100,
            max_files: 10,
        };
        let backend = MemoryBackend::with_limits(limits);

        // First file: 100 bytes
        let path1 = VfsPath::new("file1.txt").unwrap();
        backend.write(&path1, &[0u8; 100]).await.unwrap();

        // Second file: 100 bytes (total = 200)
        let path2 = VfsPath::new("file2.txt").unwrap();
        backend.write(&path2, &[0u8; 100]).await.unwrap();

        // Third file rejected (would exceed 200)
        let path3 = VfsPath::new("file3.txt").unwrap();
        let result = backend.write(&path3, &[0u8; 10]).await;
        assert!(matches!(result, Err(VfsError::QuotaExceeded { ref resource, .. }) if resource == "total_storage"));
    }

    #[tokio::test]
    async fn test_memory_backend_counters() {
        let backend = MemoryBackend::default();
        let path1 = VfsPath::new("file1.txt").unwrap();
        let path2 = VfsPath::new("file2.txt").unwrap();

        // Initial state
        assert_eq!(backend.len(), 0);
        assert!(backend.is_empty());
        assert_eq!(backend.current_usage(), (0, 0));

        // Create files
        backend.write(&path1, &[0u8; 100]).await.unwrap();
        assert_eq!(backend.len(), 1);
        assert_eq!(backend.current_usage(), (1, 100));

        backend.write(&path2, &[0u8; 50]).await.unwrap();
        assert_eq!(backend.len(), 2);
        assert_eq!(backend.current_usage(), (2, 150));

        // Update file (size change)
        backend.write(&path1, &[0u8; 80]).await.unwrap();
        assert_eq!(backend.current_usage(), (2, 130));

        // Delete file
        backend.delete(&path2).await.unwrap();
        assert_eq!(backend.len(), 1);
        assert_eq!(backend.current_usage(), (1, 80));

        // Clear all
        backend.clear();
        assert_eq!(backend.len(), 0);
        assert!(backend.is_empty());
        assert_eq!(backend.current_usage(), (0, 0));
    }

    #[tokio::test]
    async fn test_memory_backend_concurrent_writes() {
        use std::sync::Arc;

        let backend = Arc::new(MemoryBackend::default());
        let mut handles = vec![];

        // Spawn 10 concurrent writes
        for i in 0..10 {
            let backend = Arc::clone(&backend);
            let handle = tokio::spawn(async move {
                let path = VfsPath::new(&format!("file{}.txt", i)).unwrap();
                backend.write(&path, &[i as u8; 100]).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all files exist
        assert_eq!(backend.len(), 10);
        for i in 0..10 {
            let path = VfsPath::new(&format!("file{}.txt", i)).unwrap();
            assert!(backend.exists(&path).await.unwrap());
        }
    }

    #[tokio::test]
    async fn test_snapshot_entries() {
        let backend = MemoryBackend::default();
        
        // Create some files
        backend.write(&VfsPath::new("file1.txt").unwrap(), b"content1").await.unwrap();
        backend.write(&VfsPath::new("dir/file2.txt").unwrap(), b"content2").await.unwrap();
        backend.write(&VfsPath::new("empty.txt").unwrap(), b"").await.unwrap();

        // Get snapshot entries
        let entries = backend.snapshot_entries();
        
        assert_eq!(entries.len(), 3);
        
        // Check that all files are present
        let paths: Vec<_> = entries.iter().map(|(p, _)| p.as_str().to_string()).collect();
        assert!(paths.contains(&"file1.txt".to_string()));
        assert!(paths.contains(&"dir/file2.txt".to_string()));
        assert!(paths.contains(&"empty.txt".to_string()));
        
        // Verify content is preserved
        let file1 = entries.iter().find(|(p, _)| p.as_str() == "file1.txt").unwrap();
        assert_eq!(file1.1.content, b"content1");
    }

    #[tokio::test]
    async fn test_restore_from_snapshot() {
        let backend = MemoryBackend::default();
        
        // Create initial data
        backend.write(&VfsPath::new("old.txt").unwrap(), b"old content").await.unwrap();
        assert_eq!(backend.len(), 1);
        
        // Prepare snapshot entries
        let entries = vec![
            (VfsPath::new("new1.txt").unwrap(), VfsFile::new(b"new content 1".to_vec())),
            (VfsPath::new("new2.txt").unwrap(), VfsFile::new(b"new content 2".to_vec())),
        ];
        
        // Restore from snapshot
        backend.restore_from_snapshot(&entries);
        
        // Verify old data is gone
        assert!(!backend.exists(&VfsPath::new("old.txt").unwrap()).await.unwrap());
        
        // Verify new data is present
        assert_eq!(backend.len(), 2);
        assert!(backend.exists(&VfsPath::new("new1.txt").unwrap()).await.unwrap());
        assert!(backend.exists(&VfsPath::new("new2.txt").unwrap()).await.unwrap());
        
        let content1 = backend.read(&VfsPath::new("new1.txt").unwrap()).await.unwrap();
        assert_eq!(content1, b"new content 1");
        
        // Verify counters are correct
        assert_eq!(backend.current_usage(), (2, 26)); // 2 files, 13+13 bytes
    }

    #[tokio::test]
    async fn test_snapshot_roundtrip() {
        let backend = MemoryBackend::default();
        
        // Create files
        backend.write(&VfsPath::new("config.json").unwrap(), b"{\"key\": \"value\"}").await.unwrap();
        backend.write(&VfsPath::new("data/users.txt").unwrap(), b"user1\nuser2").await.unwrap();
        
        // Snapshot
        let entries = backend.snapshot_entries();
        
        // Create new backend and restore
        let new_backend = MemoryBackend::default();
        new_backend.restore_from_snapshot(&entries);
        
        // Verify all data restored
        assert_eq!(new_backend.len(), 2);
        
        let config = new_backend.read(&VfsPath::new("config.json").unwrap()).await.unwrap();
        assert_eq!(config, b"{\"key\": \"value\"}");
        
        let users = new_backend.read(&VfsPath::new("data/users.txt").unwrap()).await.unwrap();
        assert_eq!(users, b"user1\nuser2");
    }
}
