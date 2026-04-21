# Sliver Functional Workflow Documentation

## Overview

This document describes the complete sliver workflow for creating, running, snapshotting, and restoring JavaScript applications in the NANO runtime.

## What Are Slivers?

Slivers are **application snapshots** that capture:
- **V8 Heap State**: Compiled JavaScript code, global variables, heap objects
- **Virtual Filesystem**: All files written during the app session
- **Metadata**: App hostname, version, timestamps

Slivers enable **fast warm-starts** (~5-10ms) compared to cold starts (~50-100ms).

## Sliver Format

```
app-v1.sliver (tar archive)
├── meta.json          # App metadata (hostname, version, timestamps)
├── heap.bin           # V8 heap snapshot (~100KB - 1MB)
├── vfs/               # Virtual filesystem contents
│   ├── index.js
│   ├── package.json
│   ├── data/
│   │   ├── user-settings.json
│   │   └── cache.db
│   └── assets/
│       ├── image-0.png
│       └── logo.svg
└── manifest.txt       # Human-readable file listing
```

## Complete Workflow

### 1. Create App with VFS

```rust
use nano::v8::{initialize_platform, NanoIsolate};
use nano::vfs::{IsolateVfs, MemoryBackend, VfsNamespace};

// Initialize V8 (required once)
initialize_platform()?;

// Create VFS
let vfs = IsolateVfs::new(
    VfsNamespace::from_hostname("api.example.com"),
    Arc::new(MemoryBackend::default()),
);

// Create snapshottable isolate (REQUIRED for sliver creation)
let mut isolate = NanoIsolate::snapshot_creator_with_vfs(vfs)?;

// Write app files
isolate.vfs().write("/index.js", app_code.as_bytes()).await?;
isolate.vfs().write("/package.json", package_json).await?;
```

### 2. Execute JavaScript to Set State

```rust
// Execute initialization code
{
    let scope = &mut v8::HandleScope::new(isolate.isolate());
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    
    let code = v8::String::new(scope, js_code).unwrap();
    let script = v8::Script::compile(scope, code, None)?;
    script.run(scope)?;
}
// Global state is now set: globalThis.appState = { counter: 42, ... }
```

### 3. Create Runtime Files

```rust
// Write files during app execution
isolate.vfs().write("/data/user-settings.json", settings_json).await?;
isolate.vfs().write("/data/cache.db", cache_data).await?;
isolate.vfs().write("/assets/image.png", image_bytes).await?;
```

### 4. Create Sliver (Snapshot + VFS)

```rust
use nano::v8::snapshot::create_snapshot_from_nano;
use nano::sliver::{pack_sliver, SliverMetadata};

// Capture VFS state (collect all files)
let vfs_entries = vec![
    (VfsPath::new("index.js")?, VfsFile { ... }),
    (VfsPath::new("data/settings.json")?, VfsFile { ... }),
    // ... all files
];

// Create heap snapshot (THIS IS THE MAGIC!)
let heap_data = create_snapshot_from_nano(isolate)?;
// heap_data now contains ~452KB of real V8 heap state

// Create metadata
let metadata = SliverMetadata::new("api.example.com", "1.1.0");

// Pack into sliver archive
let sliver_data = pack_sliver(&metadata, &heap_data, Some(&vfs_entries))?;
std::fs::write("api-v1.sliver", &sliver_data)?;
```

### 5. Destroy Original Isolate

```rust
// The isolate was consumed by create_snapshot_from_nano()
// This simulates a full application restart
println!("Original isolate destroyed - simulating restart...");
```

### 6. Restore from Sliver

```rust
use nano::sliver::unpack_sliver;

// Read sliver
let sliver_data = std::fs::read("api-v1.sliver")?;

// Unpack
let unpacked = unpack_sliver(&sliver_data)?;
println!("Sliver unpacked: {} bytes heap, {} VFS files",
    unpacked.heap_data.len(),
    unpacked.vfs_entries.len()
);

// Create new isolate from snapshot
let vfs = IsolateVfs::new(
    VfsNamespace::from_hostname("api.example.com"),
    Arc::new(MemoryBackend::default()),
);

let mut restored_isolate = NanoIsolate::from_snapshot(&unpacked.heap_data, vfs)?;

// Restore VFS contents
for (path, file) in &unpacked.vfs_entries {
    let path_str = format!("/{}", path.as_str());
    restored_isolate.vfs().write(&path_str, &file.content).await?;
}
```

### 7. Verify Restoration

```rust
// Verify files exist
let settings = restored_isolate.vfs().read("/data/user-settings.json").await?;
assert!(settings.contains("dark"));

// Verify global state (if heap was restored successfully)
// Note: Requires valid V8 snapshot, otherwise falls back to fresh isolate
```

## CLI Commands

### Create Sliver

```bash
# Create sliver from running app
nano-rs sliver create api.example.com --name api-prod --tag v1.0

# Creates: api-prod-v1.0.sliver
```

### List Slivers

```bash
nano-rs sliver list --verbose
```

### Run from Sliver

```bash
# Start server from sliver
nano-rs run --sliver ./api-prod-v1.0.sliver --workers 4
```

### Delete Sliver

```bash
nano-rs sliver delete api-prod --force
```

## Performance Comparison

| Startup Type | Time | Description |
|--------------|------|---------------|
| Cold Start | ~50-100ms | Create isolate + compile JS |
| Context Reset | ~5ms | Keep isolate, reset context |
| **Snapshot Restore** | **~5-10ms** | **Load pre-serialized heap** |

## Technical Details

### V8 Snapshot API (v139+)

The snapshot functionality is unlocked by V8 v139's public API:

```rust
// Create isolate that can be snapshotted
let isolate = v8::Isolate::snapshot_creator(None, None);

// Set default context (required for snapshotting)
let context = v8::Context::new(scope, Default::default());
scope.set_default_context(context);

// Later: Create snapshot blob
let startup_data = isolate.create_blob(FunctionCodeHandling::Clear)
    .expect("Failed to create snapshot");
```

### Fallback Behavior

If snapshot restoration fails (invalid format, version mismatch), the system gracefully falls back to creating a fresh isolate:

```rust
pub fn from_snapshot(snapshot_data: &[u8], vfs: IsolateVfs) -> Result<Self> {
    // Check for invalid/corrupted data
    if snapshot_data.len() < 8 || is_invalid_format(snapshot_data) {
        tracing::warn!("Invalid snapshot - creating fresh isolate");
        return Self::new_with_vfs(vfs);  // Fallback
    }
    
    // Attempt restoration...
}
```

## Testing

Run the functional test:

```bash
cargo test --test sliver_functional_test test_sliver_full_workflow -- --nocapture
```

Expected output:
```
[STEP 1] Creating app with VFS...
  ✓ App files written to VFS
[STEP 2] Executing JavaScript to set state...
  ✓ JavaScript executed - global state set
[STEP 3] Creating additional VFS files...
  ✓ Runtime VFS files created
[STEP 4] Creating sliver (snapshot + VFS capture)...
  ✓ Heap snapshot created: 452988 bytes
  ✓ Sliver created: 465920 bytes total
[STEP 5] Destroying original isolate (simulating restart)...
  ✓ Original isolate destroyed
[STEP 6] Restoring from sliver...
  ✓ Isolate restored from snapshot
  ✓ VFS contents restored: 9 files
[STEP 7] Verifying VFS restoration...
  ✓ /index.js: 178 bytes
  ✓ /package.json: 40 bytes
  ✓ /data/user-settings.json: 35 bytes
  ✓ Settings file contents verified
  ✓ Cache file contents verified

==================================================
SLIVER WORKFLOW TEST COMPLETE
==================================================
```

## Limitations

1. **Placeholder Snapshots**: Legacy slivers (v135-v137) used placeholder markers. New slivers (v139+) contain real heap data.

2. **V8 Version Compatibility**: Snapshots are tied to V8 version. Newer V8 versions can't load snapshots from older versions.

3. **Context Required**: Snapshot creator isolates must have a default context set before `create_blob()` can work.

4. **S3 VFS Backend**: Not yet implemented (deferred to v1.2).

## Future Enhancements

- Delta/differential snapshots
- Compression (gzip/brotli)
- Cross-version snapshot migration
- Streaming snapshot creation for large heaps

## See Also

- [Sliver Format Specification](../src/sliver/format.rs)
- [V8 Snapshot API](../src/v8/snapshot.rs)
- [VFS Integration](../src/vfs/isolate.rs)
- [CLI Commands](../src/cli/sliver.rs)
