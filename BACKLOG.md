# v1.2 Backlog

## VFS S3 Backend
- **Feature**: S3-compatible object storage VFS backend
- **Trigger**: After v1.1 milestone completes
- **Description**: Implement the S3 VFS backend that was stubbed out in v1.1
  - Complete S3 VFS backend implementation in 
  - Add S3 backend tests with MinIO/localstack
  - Document S3 configuration options (endpoint, bucket, region, credentials)
  - Support both AWS S3 and MinIO (path-style URLs)
  - Implement proper error handling for S3 operations
  - Add integration tests for S3 VFS
  - Document backup/restore workflows for S3-backed apps
  - Consider adding S3 transfer acceleration support
  - Evaluate S3 consistency model implications for VFS operations

