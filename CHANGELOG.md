# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-04-20

### Added

#### Sliver Snapshots
- **Sliver creation** — `nano-rs sliver create <hostname>` creates portable isolate snapshots
- **Sliver management** — List, inspect, delete commands for sliver lifecycle
- **Sliver restoration** — Run isolates from slivers with ~1-2ms cold starts
- **VFS in slivers** — Complete filesystem state captured and restored
- **Cross-instance migration** — Slivers portable between NANO instances

#### Virtual File System (VFS)
- **VFS core module** — In-memory file storage per-isolate
- **Storage backends** — Pluggable backends (memory, disk, S3)
- **JavaScript bindings** — `Nano.fs.*` API for file operations
- **Node.js polyfill** — `require('fs')` returns VFS-backed implementation
- **Security** — Path validation, ".." blocking, per-isolate namespaces

#### CLI Improvements
- **Sliver commands** — Full CLI for sliver lifecycle management
- **Progress indicators** — Visual feedback during long operations
- **Colorized output** — Better readability with styled output
- **Human-readable errors** — Clear error messages with suggestions
- **Input validation** — Early validation with helpful feedback

### Performance

- **~267 µs cold start** from sliver (3.7x better than 1-2ms target)
- **~19x faster** than context reset (~5ms)
- **~187-375x faster** than fresh isolate creation (~50-100ms)

### Technical

- V8 SnapshotCreator integration (placeholder in v135, full in future)
- Tar-based snapshot format for portability
- Per-isolate filesystem namespaces for security
- Atomic file writes in disk backend
- S3 backend (feature-gated: `vfs-s3`)

### Documentation

- SLIVER.md — Complete sliver documentation
- VFS.md — Virtual File System documentation
- README.md — Quick start with slivers

## [1.0.0] - 2026-04-19

### Added

- Multi-tenant JavaScript isolation with V8 isolates
- HTTP server with virtual host routing
- WorkerPool with context reset for request handling
- Runtime APIs: console, encoding, timers, crypto (AES-GCM, HMAC, PBKDF2)
- Fetch API with streaming support
- Hono.js, Next.js static, Astro framework compatibility
- Production features: logging, metrics, admin API
