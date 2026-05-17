# NANO Architecture

## Overview

NANO is a single-process HTTP server that hosts multiple JavaScript applications in parallel, each isolated in its own V8 isolate. It replaces container-per-app architectures with a single binary that manages multiple isolated execution contexts.

### Performance Characteristics

| Metric | Time | Description |
|--------|------|-------------|
| Process Boot | ~60ms | One-time on binary start |
| Sliver Restoration | ~267µs | New isolate from snapshot |
| Context Reset | ~5ms | Between requests (same isolate) |
| Fresh Isolate | ~50-100ms | New isolate without snapshot |

See [Performance Documentation](docs/PERFORMANCE.md) for detailed benchmarks and tuning guide.

## Core Components

### 1. HTTP Server (src/http/)

TCP accept loop on the main thread. Uses axum for HTTP handling.

- **server.rs**: Main HTTP server, binds to configured address
- **router.rs**: Virtual host routing — matches Host header to app
- **v8_bridge.rs**: Bridges HTTP requests into V8 isolates
- **client.rs**: Outbound HTTP client for `fetch()` from JavaScript
- **types.rs**: WinterTC Request/Response/URL/Headers implementations

Request flow: TCP accept → Host header lookup → app routing → work queue dispatch.

WebSocket upgrade path: HTTP GET with `Upgrade: websocket` → `detect_ws_upgrade()` → axum handshake (101) → tokio relay task bridges axum frames to `WsChannels` → `TenantPool::dispatch_ws()` → dedicated worker thread owns the `'ws_messages` loop for the connection lifetime.

### 2. V8 Integration (src/v8/)

Manages V8 platform and isolates.

- **platform.rs**: V8 platform initialization with EPT fix (strong Global sentinel prevents SIGSEGV)
- **isolate.rs**: Per-isolate management, thread-local ownership
- **context.rs**: V8 context creation and reset between requests
- **script.rs**: JavaScript execution and compilation

Critical: V8 isolates are NOT thread-safe. Each isolate is bound to one thread for its entire lifetime. Context reset (~5ms, not full isolate recreation) happens between requests instead of fresh isolate creation (~50-100ms). See [Cold Start Guide](docs/COLD_START.md) for performance terminology.

### 3. WorkerPool (src/worker/)

Manages worker threads and request dispatch.

- **pool.rs**: WorkerPool per app, spawns N threads
- **queue.rs**: WorkQueue with bounded MPSC channel (256 slots)
- **context.rs**: Context lifecycle — reset, not recreate
- **oom.rs**: OOM detection and isolate termination
- **limits.rs**: Per-app resource enforcement

Thread model: One worker thread per isolate. Affine dispatch — same hostname always routes to same worker index for cache locality. SliverWorkerPool enables ~267µs sliver restoration vs ~50-100ms fresh isolate creation. See [Performance Documentation](docs/PERFORMANCE.md) for benchmarks.

### 4. Runtime APIs (src/runtime/)

JavaScript APIs exposed to isolates.

- **handler.rs**: fetch() handler interface, Promise resolution
- **apis.rs**: API bindings — console, encoding, timers
- **crypto/**: WebCrypto implementation via ring crate
  - `crypto_key.rs`: CryptoKey with extractable flag
  - `subtle.rs`: SubtleCrypto API foundation
  - `aes_gcm.rs`: AES-GCM encrypt/decrypt
  - `hmac.rs`: HMAC sign/verify
- **fetch.rs**: Outbound fetch binding
- **stream.rs**: ReadableStream/WritableStream implementations

Design choice: All crypto operations use the ring crate, never V8's crypto.subtle C++ implementation. This avoids V8 internal complexity and provides safer Rust crypto.

### 5. App Management (src/app/)

Multi-app hosting infrastructure.

- **registry.rs**: App registry, hostname to config mapping
- **reload.rs**: Hot-reload on config changes
- **drain.rs**: Graceful drain during reload/shutdown
- **timeout.rs**: Per-request timeout watchdog

Config file maps hostnames to entry points, worker counts, memory limits.

### 6. Admin API (src/admin/)

Operational control plane.

- **server.rs**: HTTP admin server on port 8889
- **auth.rs**: API key authentication middleware
- **handlers/**: Admin endpoints (isolates, apps, health)
- **unix_socket.rs**: Unix domain socket for local access
- **diagnostics.rs**: ps-style diagnostics output
- **metrics.rs**: Prometheus metrics endpoint

Two access modes: HTTP for remote monitoring, Unix socket for local emergency access.

### 7. Config (src/config/)

Configuration loading and watching.

- **loader.rs**: JSON config parsing and validation
- **watcher.rs**: File watching for hot-reload
- **app.rs**: AppConfig struct with limits

### 8. Observability (src/logging/, src/metrics/, src/signal.rs)

- **logging/**: Structured JSON logs with context fields
- **metrics/**: Prometheus-compatible metrics (counter, gauge, histogram)
- **signal.rs**: SIGTERM/SIGINT graceful shutdown handling

## Request Lifecycle

```
HTTP Request:
1. TCP accept (main thread)
2. Parse Host header
3. Virtual host lookup → AppConfig
4. WorkQueue dispatch (bounded MPSC)
5. Worker thread dequeues request
6. Enter V8 isolate (thread-local)
7. Reset V8 context (~5ms)
8. Call JS fetch() handler
9. Serialize Response, write HTTP response
10. Context reset for next request

WebSocket Upgrade:
1. TCP accept (main thread)
2. Parse Host header + detect Upgrade: websocket
3. axum WebSocket handshake (101 Switching Protocols)
4. tokio relay task ↔ WsChannels (mpsc)
5. TenantPool::dispatch_ws() → dedicated worker thread
6. JS fetch() handler called (registers addEventListener callbacks)
7. 'ws_messages loop: recv_timeout(idle_timeout_ms) per frame
8. Per-frame: CpuTimeoutGuard + OOM check + JS event dispatch
9. Close/Disconnect: set_ws_readystate(3), break 'requests → isolate recycled
```

## Security Model

- V8 isolates provide memory and execution isolation between apps
- No npm/import resolution — apps are single-file, user bundles beforehand
- No subprocess spawning from JavaScript
- Per-app memory limits with OOM detection
- Per-app timeout enforcement
- SSRF prevention: private IP range blocking
- Dangerous header filtering (Host, Content-Length, Transfer-Encoding)

## Crypto Architecture

All WebCrypto operations use the ring crate:

- AES-GCM: ring::aead::Aes256Gcm
- HMAC: ring::hmac::HMAC
- Key material zeroized on Drop (zeroize crate)
- Non-extractable keys enforced at export time
- Constant-time comparison for HMAC verification

Async API: All subtle methods return Promises resolved via tokio.

## Threading Model

```
Main Thread:
  - TCP accept loop
  - HTTP request parsing
  - Virtual host routing
  - WorkQueue dispatch

Worker Threads (N per app):
  - WorkQueue consumer
  - V8 isolate owner (thread-local)
  - JavaScript execution
  - Context reset between requests
```

Isolates never move between threads. Thread-local storage enforces this.

## Memory Model

- Each isolate has its own V8 heap (configurable limit, default 128MB)
- Rust heap shared across runtime (logging, metrics, config)
- Context reset clears JavaScript global state without heap deallocation
- OOM detection monitors heap usage, terminates isolate at limit

## Configuration Hot-Reload

1. File watcher detects config.json change
2. Graceful drain: wait for in-flight requests (30s timeout)
3. Swap app registry atomically
4. Spawn new workers with updated config
5. Old workers drain and terminate

## Dependencies

Key crates:
- v8 (147): Pre-built V8 bindings
- tokio (1.52): Async runtime
- axum (0.8): HTTP server
- ring (0.17): Cryptographic operations
- hyper (1.4): HTTP client

See Cargo.toml for complete list.

## Build Profiles

Release profile optimized for size and speed:
- opt-level = 3
- lto = true
- codegen-units = 1
- panic = "abort"

## Design Decisions

1. **Rust over Zig**: Pre-built V8, type-safe bindings, stable ecosystem
2. **Context reset over new isolate**: 5ms vs 50-100ms per request
3. **ring over V8 crypto**: Safer, avoids V8 internal complexity
4. **No npm resolution**: Simplifies runtime, keeps isolates lightweight
5. **WorkerPool per app**: Resource isolation between tenants
6. **Bounded queues**: Backpressure prevents memory exhaustion
7. **Pre-built V8 only**: No 2-hour compilation, tracks Deno's rusty_v8

## WinterTC Compliance

NANO targets WinterTC Minimum Common API:

Implemented: Request, Response, Headers, URL, TextEncoder, TextDecoder, console, crypto.getRandomValues, crypto.subtle (AES-GCM, HMAC), ReadableStream, WritableStream, AbortController, performance.now(), structuredClone, Blob, FormData, DOMException

Not implemented: queueMicrotask, atob/btoa (can add later)
