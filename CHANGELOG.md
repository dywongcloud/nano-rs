# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v2.0a] - 2026-05-17

### Added

#### WebSocket Server (Phase 23)

- **HTTP upgrade detection** — `detect_ws_upgrade()` in `router.rs`; checks `Upgrade: websocket` and `Connection: Upgrade` headers
- **axum WebSocket handshake** — 101 Switching Protocols via axum `ws` feature
- **Relay task** — tokio task bridges axum WebSocket frames to `WsChannels` (mpsc channel pair)
- **`WsChannels`** — `WsInbound` / `WsOutbound` channel pair in `tenant_pool.rs`
- **`TenantPool::dispatch_ws()`** — Routes WebSocket request to dedicated worker thread (lazy spawn)
- **`AppLimits` WebSocket config** — `ws_max_connections`, `ws_max_message_bytes` (32 MiB default), `ws_idle_timeout_ms` (60 000 ms default)
- **`'ws_messages` loop** — Worker thread loop; `recv_timeout(idle_timeout_ms)` per frame with full frame arm handling
- **Frame handling** — Text (string MessageEvent), Binary (ArrayBuffer MessageEvent), Close (CloseEvent), Ping/Pong (skip), Timeout/Disconnect (1006 error + close)
- **Per-message resource enforcement** — `CpuTimeoutGuard` per frame (D-09b); OOM check per frame sends Close 1011 on heap limit
- **Isolate lifecycle** — `break 'requests` after `'ws_messages` forces fresh isolate per connection (D-10b)
- **WebSocket thread-locals** — `WS_OUTBOUND`, `WS_ACCEPTED`, `WS_MESSAGE_HANDLERS`, `WS_CLOSE_HANDLERS`, `WS_ERROR_HANDLERS`, `WS_SERVER_SOCKET`
- **`readyState` management** — `set_ws_readystate(1)` on entry, `set_ws_readystate(3)` on close/disconnect
- **`WebSocketPair` V8 binding** — `new WebSocketPair()` JS API (Plan 05)
- **`ws_busy` counter** — Incremented by worker thread on WS entry, decremented on exit; served counter not incremented for WS connections

#### Documentation

- `docs/WEBSOCKET.md` — Phase 23 architecture, WebSocketPair API, upgrade flow, limits

### Changed

- Cargo.toml comment: `v139` → `v147` (comment already matched actual `v8 = "147"`)
- README: version `1.4.2` → `v2.0a`, added WebSocket to API table and docs list
- ARCHITECTURE.md: added WebSocket upgrade path to request lifecycle
- docs/API.md: added WebSocket section with WebSocketPair API reference
- docs/CLOUDFLARE_COMPATIBILITY.md: added WebSocket compatibility cross-reference

## [1.7.2] - 2026-05-17

### Added

- Pre-Phase-23 stability (Phase 40): TryCatch RAII, `cancel_terminate_execution`, isolate endurance tests
- `CpuTimeoutGuard::drop()` now calls `cancel_terminate_execution()` — fixes exception bleed between requests
- `set_allow_generation_from_strings(false)` at all `Context::new()` sites
- `tests/isolate_endurance_test.rs` — 4 endurance tests (SCOPE-01, ENDURE-01..03)

## [1.7.1] - 2026-05-15

### Added

- Phase 41 Production Polish: heap limit enforcement, CPU time enforcement, Prometheus metrics
- V8 near-heap-limit callback terminates isolate on OOM
- Fixed cross-thread CPU termination bug (`thread_local!` → `AtomicPtr`)
- `nano_heap_limit_hits_total` and `nano_cpu_timeout_total` Prometheus counters
- Adversarial tests: 56/57 passing (98%)

## [1.2.4] - 2026-04-26

### Fixed

#### Runtime API Fixes

**Buffer.from().toString()**
- Problem: Returned comma-separated byte values (e.g., "116,101,115,116") instead of decoded string ("test")
- Root cause: Buffer implemented as Uint8Array; default Uint8Array.toString() returns byte values
- Solution: Added buffer_tostring_callback that extracts bytes and decodes to UTF-8 using String::from_utf8_lossy
- Files: src/runtime/apis.rs

**URL.toString()**
- Problem: Returned "[object Object]" instead of URL string
- Root cause: URL object had properties but no custom toString method; default Object.prototype.toString() returns "[object Object]"
- Solution: Added url_tostring_callback that returns href property; attached to URL prototype in bind_url
- Files: src/runtime/apis.rs

**HTTP Client**
- Problem: Returned mock 200 OK responses without making actual HTTP requests
- Root cause: HttpClient::request() was a stub returning hardcoded success
- Solution: Implemented using reqwest with connection pooling, timeouts, redirects, and proper error handling
- Files: src/http/client.rs

#### Test Harness Fixes

**crypto.subtle API Access**
- Problem: Tests for crypto.subtle.digest and crypto.subtle.generateKey failed with "Unknown test" error
- Root cause: Test harness used switch case key 'crypto:digest' but test sent category 'crypto.subtle' creating key 'crypto.subtle:digest'
- Solution: Updated switch case to use 'crypto.subtle:digest' and 'crypto.subtle:generateKey'
- Files: scripts/fast-compatibility-matrix.js

**CRUD Test Regex**
- Problem: "Script compilation failed" error on CRUD tests due to invalid regex in generated JavaScript
- Root cause: Test harness template literal used `^/api/items/(d+)$` which produced `^/api/items/(d+)$` in output (unescaped forward slashes)
- Solution: Changed to `^\\/api\\/items\\/(\\d+)$` in template literal which produces `^\/api\/items\/(\d+)$` in output (properly escaped)
- Files: scripts/run-tests.js, tests/harness.js

### Test Results

All test suites pass at 100%:

| Test Suite | Tests | Passed | Failed | Percentage |
|------------|-------|--------|--------|------------|
| API Compatibility Matrix | 26 | 26 | 0 | 100% |
| Comprehensive Test Suite | 27 | 27 | 0 | 100% |
| CRUD Operations | 6 | 6 | 0 | 100% |
| HTTP Verbs | 7 | 7 | 0 | 100% |
| Cloudflare Worker | 6 | 6 | 0 | 100% |
| WebCrypto | 2 | 2 | 0 | 100% |
| Multi-tenancy | 2 | 2 | 0 | 100% |

### Compatibility

- WinterTC APIs: 100% compatible
- WebCrypto: 100% compatible  
- Node.js fs polyfill: 100% compatible
- Cloudflare Workers: 100% compatible (standard patterns)
- Hono.js: Fully supported
- Next.js static: Fully supported
- Astro static: Fully supported

## [1.1.0] - 2026-04-20

### Added

#### Sliver Snapshots
- Sliver creation — `nano-rs sliver create <hostname>` creates portable isolate snapshots
- Sliver management — List, inspect, delete commands for sliver lifecycle
- Sliver restoration — Run isolates from slivers with ~1-2ms cold starts
- VFS in slivers — Complete filesystem state captured and restored
- Cross-instance migration — Slivers portable between NANO instances

#### Virtual File System (VFS)
- VFS core module — In-memory file storage per-isolate
- Storage backends — Pluggable backends (memory, disk, S3)
- JavaScript bindings — `Nano.fs.*` API for file operations
- Node.js polyfill — `require('fs')` returns VFS-backed implementation
- Security — Path validation, ".." blocking, per-isolate namespaces

#### CLI Improvements
- Sliver commands — Full CLI for sliver lifecycle management
- Progress indicators — Visual feedback during long operations
- Colorized output — Better readability with styled output
- Human-readable errors — Clear error messages with suggestions
- Input validation — Early validation with helpful feedback

### Performance

- ~267 µs cold start from sliver (3.7x better than 1-2ms target)
- ~19x faster than context reset (~5ms)
- ~187-375x faster than fresh isolate creation (~50-100ms)

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
