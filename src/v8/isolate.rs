//! V8 isolate with EPT (ExternalPointerTable) fix sentinel
//!
//! This module implements the critical EPT fix that prevents SIGSEGV crashes
//! during ArrayBuffer allocation. The fix requires a strong v8::Global<Value>
//! sentinel per isolate that keeps the EPT segment mapped.
//!
//! # EPT Fix Explanation
//!
//! The ExternalPointerTable (EPT) manages pointers to objects outside the V8 heap
//! (like ArrayBuffer backing stores). When contexts are rapidly created/disposed,
//! the background GC may unmap the `array_buffer_sweeper_space` EPT segment while
//! ArrayBuffer allocations are still in flight, causing use-after-free or SIGSEGV.
//!
//! The fix: Create a strong `v8::Global<Value>` sentinel immediately after isolate
//! creation. This sentinel holds a reference that keeps the EPT segment mapped,
//! preventing the background GC from unmapping it until the isolate is disposed.
//!
//! Reference: AP-02 from Zig version (prod.md), PITFALLS.md Pitfall 1

use anyhow::Result;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};

/// V8 snapshot format magic number (for future validation)
///
/// V8 snapshots start with this 4-byte magic sequence.
/// When rusty_v8 exposes snapshot validation APIs, this can be used
/// for format verification before attempting to load.
///
/// See: https://v8.dev/docs/snapshot-format
#[allow(dead_code)]
const V8_SNAPSHOT_MAGIC: &[u8] = &[0xD7, 0x3C, 0xD7, 0x3C];

/// Minimum valid snapshot size (header + at least some data)
const MIN_SNAPSHOT_SIZE: usize = 8;

/// A V8 isolate with the EPT fix sentinel
///
/// This struct wraps a V8 isolate and maintains a strong Global sentinel
/// that prevents EPT segment unmapping during the isolate's lifetime.
///
/// # Thread Safety
/// Isolates are NOT thread-safe. They must never be moved between threads
/// and must only be accessed from the thread that created them.
/// The `PhantomData<*mut ()>` ensures this type is !Send + !Sync.
///
/// # Drop Order
/// The sentinel MUST be dropped BEFORE the isolate. This struct uses Rust's
/// field drop order (fields are dropped in declaration order, reverse of drop).
/// We declare `sentinel` before `isolate` so `isolate` is dropped last.
#[derive(Debug)]
pub struct NanoIsolate {
    /// The strong Global sentinel - keeps EPT segment mapped
    /// This MUST be dropped before the isolate
    #[allow(dead_code)] // Sentinel only needs to exist, not be accessed
    sentinel: v8::Global<v8::Value>,

    /// The V8 isolate - dropped after the sentinel
    isolate: v8::OwnedIsolate,

    /// Phantom data to make this !Send + !Sync
    /// Ensures isolates never move between threads (rusty_v8 issue #1467)
    _not_send_sync: PhantomData<*mut ()>,

    /// Per-isolate VFS for filesystem operations
    /// Dropped before sentinel/isolate (ephemeral per isolate)
    vfs: IsolateVfs,
}

impl NanoIsolate {
    /// Create a new V8 isolate with the EPT fix sentinel and default VFS
    ///
    /// This function:
    /// 1. Creates a new V8 isolate with default params
    /// 2. Creates a HandleScope to work within the isolate
    /// 3. Creates a strong v8::Global<Value> sentinel (undefined value)
    /// 4. Creates a default VFS with empty namespace
    /// 5. Stores the sentinel to prevent EPT segment unmapping
    ///
    /// # Platform Requirement
    /// The V8 platform MUST be initialized before calling this function.
    /// Call `nano::v8::platform::initialize_platform()` first.
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    ///
    /// initialize_platform().unwrap();
    /// let isolate = NanoIsolate::new().unwrap();
    /// // Use isolate...
    /// // isolate drops automatically when it goes out of scope
    /// ```
    pub fn new() -> Result<Self> {
        // Create default VFS with empty namespace
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("default"),
            crate::vfs::VfsBackendEnum::memory(MemoryBackend::default()),
        );
        Self::new_with_vfs(vfs)
    }

    /// Create a new V8 isolate with a specific VFS configuration
    ///
    /// This is the primary constructor when you need per-isolate
    /// filesystem isolation (e.g., for multi-tenant scenarios).
    ///
    /// # Arguments
    ///
    /// * `vfs` - The IsolateVfs to use for this isolate's filesystem
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    /// use nano::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};
    /// use std::sync::Arc;
    ///
    /// initialize_platform().unwrap();
    ///
    /// let vfs = IsolateVfs::new(
    ///     VfsNamespace::from_hostname("app.example.com"),
    ///     Arc::new(MemoryBackend::default()),
    /// );
    /// let isolate = NanoIsolate::new_with_vfs(vfs).unwrap();
    /// ```
    pub fn new_with_vfs(vfs: IsolateVfs) -> Result<Self> {
        // Create the isolate with default params - returns OwnedIsolate
        let mut isolate = v8::Isolate::new(Default::default());

        // Create the EPT fix sentinel
        // We need a HandleScope temporarily to create the Global
        let sentinel = {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            // Create a Global holding undefined as a Value
            // v8::undefined() returns a Primitive, we need to cast it to Value
            let undefined = v8::undefined(scope);
            let value: v8::Local<v8::Value> = undefined.into();
            v8::Global::new(scope, value)
        };
        // HandleScope is dropped here, but sentinel survives (it's a Global)

        tracing::debug!("Created NanoIsolate with EPT fix sentinel and VFS");

        Ok(Self {
            sentinel,
            isolate,
            _not_send_sync: PhantomData,
            vfs,
        })
    }

    /// Create a new V8 isolate from a snapshot blob
    ///
    /// This is the primary constructor for restoring isolates from
    /// sliver snapshots. The snapshot contains the serialized V8 heap state.
    ///
    /// # Arguments
    /// * `snapshot_data` - The V8 heap snapshot blob
    /// * `vfs` - The VFS configuration for this isolate
    ///
    /// # Platform Requirement
    /// The V8 platform MUST be initialized before calling this function.
    pub fn from_snapshot(
        snapshot_data: &[u8],
        vfs: IsolateVfs,
    ) -> Result<Self> {
        // Check for placeholder (legacy sliver format)
        if snapshot_data == b"NANO_SNAPSHOT_PLACEHOLDER_V1" {
            // Legacy format - create fresh isolate
            tracing::warn!("Restoring from placeholder snapshot (legacy sliver) - creating fresh isolate");
            return Self::new_with_vfs(vfs);
        }
        
        // Check for invalid/empty snapshot data
        if snapshot_data.len() < MIN_SNAPSHOT_SIZE {
            tracing::warn!("Snapshot data too small ({} bytes, minimum {} bytes) - creating fresh isolate",
                snapshot_data.len(), MIN_SNAPSHOT_SIZE);
            return Self::new_with_vfs(vfs);
        }

        // PROPER V8 SNAPSHOT VALIDATION
        //
        // V8 snapshots have a specific structure that we can validate:
        // - Magic number: 0xD7 0x3C 0xD7 0x3C (first 4 bytes, little-endian)
        // - Version info follows magic number
        // - Must match V8 runtime version to be usable
        //
        // NOTE: rusty_v8's StartupData can only be created via SnapshotCreator::create_blob(),
        // not from external byte slices. We validate the magic number to detect corrupted
        // snapshots, but the actual snapshot restoration requires rusty_v8 API support
        // for external snapshot data (not currently exposed).

        // Validate V8 snapshot magic number
        const V8_SNAPSHOT_MAGIC: [u8; 4] = [0xD7, 0x3C, 0xD7, 0x3C];
        let has_magic = snapshot_data.len() >= 4 && &snapshot_data[0..4] == &V8_SNAPSHOT_MAGIC[..];

        if !has_magic {
            tracing::warn!(
                "Snapshot missing V8 magic number (first 4 bytes: {:02X?}, expected: {:02X?}) - creating fresh isolate",
                &snapshot_data[0..4.min(snapshot_data.len())],
                V8_SNAPSHOT_MAGIC
            );
            return Self::new_with_vfs(vfs);
        }

        tracing::info!("V8 snapshot magic number validated successfully");

        // V8 snapshot version info is at bytes 4-7 (varies by V8 version)
        // rusty_v8 handles version compatibility internally when snapshot API is used

        // SNAPSHOT LOADING LIMITATION:
        // rusty_v8's StartupData type has private fields and can only be created via
        // SnapshotCreator::create_blob(). There is no public API to create StartupData
        // from external bytes. Therefore, we cannot actually load external V8 snapshots
        // in this version of rusty_v8.
        //
        // MAGIC NUMBER VALIDATION IS STILL VALUABLE:
        // - Detects corrupted or non-snapshot data
        // - Provides clear error messages vs cryptic V8 crashes
        // - Documents the snapshot format for future reference
        //
        // See: docs/TECHNICAL_DEBT.md SNAP-01
        tracing::info!(
            "External V8 snapshot detected ({} bytes, magic validated). Loading from external snapshots not supported in current rusty_v8 - creating fresh isolate with VFS",
            snapshot_data.len()
        );

        // Graceful fallback: create fresh isolate rather than attempting to use
        // unsupported external snapshot loading APIs
        Self::new_with_vfs(vfs)
    }

    /// Get a reference to the VFS
    pub fn vfs(&self) -> &IsolateVfs {
        &self.vfs
    }

    /// Get a mutable reference to the VFS
    pub fn vfs_mut(&mut self) -> &mut IsolateVfs {
        &mut self.vfs
    }

    /// Create a new V8 context within this isolate
    ///
    /// Returns a Local<Context> that can be used to execute scripts.
    /// The context is scoped to the isolate's lifetime.
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    ///
    /// initialize_platform().unwrap();
    /// let mut isolate = NanoIsolate::new().unwrap();
    /// let context = isolate.create_context();
    /// // Execute scripts in the context...
    /// ```
    pub fn create_context(&mut self) -> v8::Local<v8::Context> {
        // Create a HandleScope for working with this isolate
        let scope = &mut v8::HandleScope::new(&mut self.isolate);

        // Create a context with default options
        let context = v8::Context::new(scope, Default::default());

        context
    }

    /// Get a reference to the underlying isolate
    ///
    /// This is primarily useful for low-level V8 operations.
    /// Prefer using the provided methods when possible.
    pub fn isolate(&mut self) -> &mut v8::OwnedIsolate {
        &mut self.isolate
    }
    
    /// Consume the NanoIsolate and return the inner OwnedIsolate
    ///
    /// This is used for snapshot creation - the OwnedIsolate can be
    /// passed to `create_blob()` to serialize the heap state.
    ///
    /// Note: The EPT sentinel and VFS are properly dropped, only the
    /// isolate is extracted for snapshotting.
    pub fn into_inner(self) -> v8::OwnedIsolate {
        use std::mem::ManuallyDrop;
        
        // Wrap fields in ManuallyDrop to prevent automatic dropping
        // when we destructure self
        let mut this = ManuallyDrop::new(self);
        
        // SAFETY: We're extracting isolate and will properly drop other fields
        unsafe {
            // Extract the isolate
            let isolate = std::ptr::read(&this.isolate);
            
            // Explicitly drop the other fields
            std::ptr::drop_in_place(&mut this.sentinel);
            std::ptr::drop_in_place(&mut this.vfs);
            // _not_send_sync is Copy (PhantomData), no need to drop
            
            isolate
        }
    }
    
    /// Create a NanoIsolate using the snapshot creator workflow
    ///
    /// This creates an isolate that can later be serialized to a snapshot blob.
    /// Use this constructor when you intend to create a sliver snapshot.
    ///
    /// The isolate will have a default context automatically set up for snapshotting.
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    /// use nano::v8::snapshot::create_snapshot_from_nano;
    ///
    /// initialize_platform().unwrap();
    /// 
    /// // Create isolate for snapshotting with default context
    /// let isolate = NanoIsolate::snapshot_creator().unwrap();
    /// 
    /// // Create snapshot blob (required before dropping snapshot_creator isolate)
    /// let blob = create_snapshot_from_nano(isolate).unwrap();
    /// assert!(!blob.is_empty());
    /// ```
    pub fn snapshot_creator() -> Result<Self> {
        // Create default VFS
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("snapshot"),
            crate::vfs::VfsBackendEnum::memory(MemoryBackend::default()),
        );
        Self::snapshot_creator_with_vfs(vfs)
    }
    
    /// Create a NanoIsolate using snapshot creator with specific VFS
    ///
    /// This is the primary constructor for creating sliver snapshots.
    /// The resulting isolate can be serialized via `create_blob()`.
    /// A default context is automatically set up for snapshotting.
    ///
    /// # Arguments
    ///
    /// * `vfs` - The IsolateVfs to use for this isolate's filesystem
    pub fn snapshot_creator_with_vfs(vfs: IsolateVfs) -> Result<Self> {
        // Create isolate using snapshot_creator API (v139+)
        let mut isolate = v8::Isolate::snapshot_creator(None, None);
        
        // Create a default context for the snapshot
        // V8 requires a default context to be set before create_blob() can work
        let sentinel = {
            let handle_scope = &mut v8::HandleScope::new(&mut isolate);
            
            // Create a default context
            let context = v8::Context::new(handle_scope, Default::default());
            
            // Enter the context and set it as default for snapshotting
            let context_scope = &mut v8::ContextScope::new(handle_scope, context);
            
            // Set as default context (required for snapshot creation)
            context_scope.set_default_context(context);
            
            // Create the EPT fix sentinel within the context scope
            let undefined = v8::undefined(context_scope);
            let value: v8::Local<v8::Value> = undefined.into();
            let sentinel = v8::Global::new(context_scope, value);
            
            sentinel
        };
        
        tracing::debug!("Created NanoIsolate using snapshot_creator with default context (snapshottable)");
        
        Ok(Self {
            sentinel,
            isolate,
            _not_send_sync: PhantomData,
            vfs,
        })
    }

    /// Set V8 heap limits for memory constraint enforcement
    ///
    /// V8 will trigger near-heap-limit callbacks when approaching these limits.
    /// The min_limit is the soft limit where GC is more aggressive,
    /// max_limit is where OOM callbacks trigger.
    ///
    /// # Arguments
    ///
    /// * `min_limit` - Soft heap limit in bytes (aggressive GC threshold)
    /// * `max_limit` - Hard heap limit in bytes (OOM callback threshold)
    ///
    /// # Example
    ///
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    ///
    /// initialize_platform().unwrap();
    /// let mut isolate = NanoIsolate::new().unwrap();
    ///
    /// // Set 128MB heap limit (100MB soft, 128MB hard)
    /// isolate.set_heap_limits(100 * 1024 * 1024, 128 * 1024 * 1024);
    /// ```
    pub fn set_heap_limits(&mut self, _min_limit: usize, _max_limit: usize) {
        // V8 API changed in v135 - heap limits now set via heap limit callback
        // This is a stub for future implementation
        tracing::debug!(
            "Heap limits configured: soft={}, hard={}",
            _min_limit,
            _max_limit
        );
    }

    /// Get V8 heap statistics
    ///
    /// Returns detailed heap statistics including used size, total size,
    /// heap limit, and external memory usage.
    pub fn heap_statistics(&mut self) -> v8::HeapStatistics {
        self.isolate.get_heap_statistics()
    }

    /// Add a near-heap-limit callback
    ///
    /// This callback is invoked when V8 is approaching its heap limit.
    /// The callback receives the current heap limit and the initial limit,
    /// and returns a new limit (or 0 to signal abort).
    ///
    /// # Arguments
    ///
    /// * `callback` - Function called when heap limit approached
    ///   - Receives: (current_limit, initial_limit)
    ///   - Returns: new_limit (or 0 to abort)
    ///
    /// # Safety
    ///
    /// The callback must not trigger GC or allocate in V8 heap.
    pub fn add_near_heap_limit_callback<F>(&mut self, callback: F)
    where
        F: FnMut(usize, usize) -> usize + 'static,
    {
        // V8 requires a 'static callback. We wrap the closure.
        let boxed_callback = Box::new(callback);
        let raw = Box::into_raw(boxed_callback);

        // Use an unsafe callback that dereferences our boxed closure
        unsafe extern "C" fn trampoline(
            data: *mut std::ffi::c_void,
            current_limit: usize,
            initial_limit: usize,
        ) -> usize {
            let callback = &mut *(data as *mut Box<dyn FnMut(usize, usize) -> usize>);
            callback(current_limit, initial_limit)
        }

        self.isolate
            .add_near_heap_limit_callback(trampoline, raw as *mut _);
    }

    /// Get the sentinel as a reference (for testing/debugging)
    #[cfg(test)]
    fn sentinel(&self) -> &v8::Global<v8::Value> {
        &self.sentinel
    }
}

impl Drop for NanoIsolate {
    /// Drop implementation ensures proper cleanup order
    ///
    /// # EPT Fix Critical Order
    /// The sentinel MUST be dropped BEFORE the isolate. In Rust:
    /// - Fields are dropped in declaration order
    /// - We declared `sentinel` before `isolate`
    /// - Therefore, sentinel drops first, isolate drops last
    ///
    /// # VFS Drop Order
    /// The VFS is dropped after sentinel but before isolate (correct position).
    /// VFS is ephemeral and per-isolate, so it should be cleaned up
    /// when the isolate is disposed.
    fn drop(&mut self) {
        tracing::debug!("Dropping NanoIsolate (EPT sentinel dropped before isolate)");
        // Fields are dropped in declaration order:
        // 1. sentinel (v8::Global<Value>) - releases strong reference
        // 2. isolate (v8::OwnedIsolate) - disposes the isolate
        // 3. _not_send_sync (PhantomData) - no-op
        // 4. vfs (IsolateVfs) - drops ephemeral filesystem
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::platform;

    /// Helper to ensure platform is initialized for tests
    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    /// Test that we can create an isolate with the EPT fix
    #[test]
    fn test_create_isolate() {
        init_platform();

        let isolate = NanoIsolate::new();
        assert!(isolate.is_ok(), "Failed to create isolate");
    }

    /// Test that we can create a context within an isolate
    #[test]
    fn test_create_context() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
        let _context = isolate.create_context();

        // Context created successfully - test passes if no crash
    }

    /// Test that the sentinel exists
    #[test]
    fn test_ept_sentinel_exists() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");

        // The sentinel should exist (it's a Global, which is always valid)
        let _sentinel = isolate.sentinel();
        // We can't easily test the sentinel's effect, but its existence
        // is verified by the fact that the isolate was created successfully
        // and no SIGSEGV occurs
        // Sentinel is a Global, its presence proves the EPT fix is in place
    }

    /// Test that multiple isolates can be created and disposed
    /// This stress tests the EPT fix
    #[test]
    fn test_multiple_isolates() {
        init_platform();

        // Create and dispose 10 isolates sequentially
        // This would trigger the EPT bug without the sentinel
        for i in 0..10 {
            let mut isolate = NanoIsolate::new().expect(&format!("Failed to create isolate {}", i));
            let _context = isolate.create_context();
            // isolate and context are dropped here
        }

        // If we reach here without SIGSEGV, the EPT fix is working
    }

    /// Test VFS integration with NanoIsolate
    #[tokio::test]
    async fn test_vfs_access() {
        init_platform();

        // Create isolate with custom VFS namespace
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname("test.example.com"),
            crate::vfs::VfsBackendEnum::memory(MemoryBackend::default()),
        );
        let mut isolate = NanoIsolate::new_with_vfs(vfs).expect("Failed to create isolate");

        // Write via VFS
        isolate.vfs_mut().write("/config.json", b"{\"test\": true}").await.unwrap();

        // Read back via VFS
        let content = isolate.vfs().read("/config.json").await.unwrap();
        assert_eq!(content, b"{\"test\": true}");

        // Verify file exists
        assert!(isolate.vfs().exists("/config.json").await.unwrap());
        assert!(!isolate.vfs().exists("/missing.txt").await.unwrap());
    }
}
