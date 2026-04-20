# v1.2 Backlog

## V8 Runtime Heap Snapshots
- **Feature**: True V8 heap snapshot capture from running isolates
- **Trigger**: When rusty_v8 exposes runtime SnapshotCreator API
- **Status**: INVESTIGATED - v135 through v147 do not expose this API
- **Description**: 
  - **Current**: Placeholder marker returned (slivers work as packages only)
  - **Limitation**: rusty_v8's SnapshotCreator is `pub(crate)` (internal)
  - **What Works**: Build-time snapshots via `OwnedIsolate::create_blob()`
  - **What's Blocked**: Capturing heap state from EXISTING running isolate
  - **Alternative**: Context reset (~5ms) is production-ready
  - **Research**: v137 upgraded successfully, but API remains internal
  - **Recommendation**: Wait for upstream Deno/rusty_v8 to expose APIs, or implement custom serialization

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

