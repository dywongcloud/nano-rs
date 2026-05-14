# v1.2 Backlog

## Hybrid Static File Serving (Sliver Mode)
- **Feature**: Fast-path static file serving from VFS without JS overhead
- **Status**: PLANNED
- **Description**:
  - **Current (v1.1)**: ALL requests dispatch to JS handler (pure WinterTC)
  - **Proposed**: Check VFS first for static files, serve directly from Rust
  - **Benefits**: 
    - ~10x faster for static assets (no JS isolate context switch)
    - Better for SPAs with many assets (CSS, JS, images)
  - **Implementation**:
    - Add routing manifest to sliver metadata (static paths vs dynamic routes)
    - Pattern matching: `/assets/*`, `*.css`, `*.js` → serve from VFS directly
    - Fallback to JS handler for non-matching paths
  - **Example**:
    ```yaml
    # In sliver metadata
    static_routes:
      - path: /assets/*
        serve_from_vfs: true
        cache_control: max-age=3600
      - path: /api/*
        serve_from_vfs: false  # Always to JS
    ```
  - **Tradeoffs**: 
    - Requires build-time routing analysis
    - Less pure WinterTC (but compatible)
    - Use Option 1 for now (full JS routing), add this later for performance

## V8 Runtime Heap Snapshots - IMPLEMENTED ✅
- **Feature**: True V8 heap snapshot capture for fast cold starts
- **Status**: **WORKING** - Implemented in v1.1 with rusty_v8 v139+
- **Description**: 
  - **Current**: Real V8 heap snapshots working (verified ~450KB blobs with V8 magic headers)
  - **Implementation**: `Isolate::snapshot_creator()` → `create_blob()` → pack into sliver
  - **What Works**: 
    - Build-time snapshots via `nano-rs sliver create` (creates isolate, captures heap)
    - Snapshot restoration via `SliverWorkerPool` (workers restore from snapshot blob)
    - ~267µs cold starts from snapshot (better than 1-2ms target)
  - **Limitation**: Cannot capture from EXISTING running isolate (snapshot_creator requires fresh isolate)
  - **Note**: Slivers created before v1.1 may have legacy placeholder markers - recreate them

## VFS S3 Backend
- **Feature**: S3-compatible object storage VFS backend
- **Trigger**: After v1.1 milestone completes
- **Description**: Implement the S3 VFS backend that was stubbed out in v1.1
  - Complete S3 VFS backend implementation in `src/vfs/s3.rs`
  - Add S3 backend tests with MinIO/localstack
  - Document S3 configuration options (endpoint, bucket, region, credentials)
  - Support both AWS S3 and MinIO (path-style URLs)
  - Implement proper error handling for S3 operations
  - Add integration tests for S3 VFS
  - Document backup/restore workflows for S3-backed apps
  - Consider adding S3 transfer acceleration support
  - Evaluate S3 consistency model implications for VFS operations

