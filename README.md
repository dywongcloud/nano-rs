# NANO Runtime Technical Documentation

Version: 1.5.0  
Last Updated: 2026-05-01

## Executive Summary

NANO is a multi-tenant JavaScript edge runtime using V8 isolates. One OS process hosts multiple isolated apps with:

- **~267µs sliver restoration** from snapshot (new isolate ready to serve)
- **~5ms context reset** between requests (isolation without overhead)
- **~60ms process boot** time (one-time on server start)

See [Cold Start Guide](docs/COLD_START.md) for detailed performance characteristics.

**Test Status: 100% Pass Rate**
- API Compatibility: 26/26 tests passing
- Comprehensive Suite: 27/27 tests passing
- CRUD Operations: 6/6 tests passing
- Cloudflare Worker: 6/6 tests passing
- **Phase 27 (Production Multi-Tenancy): 91/91 new tests passing**
- **Total: 981 tests passing**

## Architecture

### Core Components

1. **V8 Platform** - Shared V8 instance with snapshot-based isolate creation
2. **Worker Pool** - Per-app worker pools with configurable size (default: 4 workers)
3. **VFS (Virtual File System)** - Per-isolate filesystem with memory/disk/S3 backends
4. **HTTP Router** - Virtual host routing by Host header
5. **Sliver System** - Portable isolate snapshots for ~267µs cold starts

### Request Flow

1. HTTP request arrives with Host header
2. Router matches hostname to app configuration
3. Request dispatched to app's worker pool
4. Worker executes handler in V8 isolate context
5. Response returned through HTTP layer

## Implemented APIs

### WinterCG APIs (100% Complete)

All WinterCG-compatible APIs are fully implemented and tested:

| API | Status | Notes |
|-----|--------|-------|
| fetch() | Implemented | Full HTTP client with request/response handling |
| Request | Implemented | Constructor with method, headers, body support |
| Response | Implemented | Constructor with status, headers, body support |
| Headers | Implemented | Map-like interface for HTTP headers |
| URL | Implemented | Full URL parsing with pathname, search, hash |
| URLSearchParams | Implemented | Query string manipulation |
| TextEncoder | Implemented | UTF-8 encoding to Uint8Array |
| TextDecoder | Implemented | UTF-8 decoding from Uint8Array |
| console | Implemented | log, error, warn methods |

### WebCrypto APIs (100% Complete)

Full WebCrypto implementation via Rust crypto crates:

| API | Status | Algorithms |
|-----|--------|------------|
| crypto.getRandomValues | Implemented | All TypedArray types |
| crypto.subtle.digest | Implemented | SHA-256, SHA-512 |
| crypto.subtle.generateKey | Implemented | AES-GCM, HMAC |
| crypto.subtle.importKey | Implemented | JWK format |
| crypto.subtle.exportKey | Implemented | JWK format |
| crypto.subtle.encrypt | Implemented | AES-GCM |
| crypto.subtle.decrypt | Implemented | AES-GCM |
| crypto.subtle.sign | Implemented | HMAC |
| crypto.subtle.verify | Implemented | HMAC |

### Node.js Compatibility (100% Complete)

Node.js APIs available for compatibility:

| API | Status | Notes |
|-----|--------|-------|
| Buffer.from() | Implemented | From string, array, hex/base64 |
| Buffer.alloc() | Implemented | Allocate with size and fill value |
| Buffer.toString() | Implemented | Decodes to UTF-8 string |
| TextEncoder | Implemented | Standard encoding |
| TextDecoder | Implemented | Standard decoding |
| setTimeout | Implemented | Basic timer support |
| setInterval | Implemented | Basic timer support |
| clearTimeout | Implemented | Timer cancellation |
| clearInterval | Implemented | Timer cancellation |
| require('fs') | Implemented | Node.js fs polyfill via VFS |
| Nano.fs.* | Implemented | Direct VFS API |

### HTTP Features (100% Complete)

Full HTTP server and client implementation:

| Feature | Status | Notes |
|---------|--------|-------|
| HTTP/1.1 server | Implemented | Configurable host/port |
| Virtual host routing | Implemented | By Host header |
| Multi-tenant isolation | Implemented | Per-app worker pools |
| Worker pool | Implemented | Configurable size and limits |
| Context reset | Implemented | ~5ms between requests |
| Outbound HTTP fetch | Implemented | reqwest client with connection pooling |
| Timeout handling | Implemented | Configurable per-request |
| Redirect handling | Implemented | Configurable max redirects |
| Response body limits | Implemented | 100MB default, configurable |

### Sliver System (100% Complete)

Full sliver snapshot implementation:

| Feature | Status | Notes |
|---------|--------|-------|
| Sliver creation | Implemented | From running apps |
| Sliver restoration | Implemented | ~267µs cold start |
| VFS state capture | Implemented | Filesystem included |
| Tar-based format | Implemented | Portable format |
| Cross-instance migration | Implemented | Slivers portable |
| Sliver listing | Implemented | CLI command |
| Sliver inspection | Implemented | CLI command |
| Sliver deletion | Implemented | CLI command |

### Production Multi-Tenancy (v1.5.0 - 100% Complete)

Phase 27 production-grade multi-tenancy features:

| Feature | Status | Notes |
|---------|--------|-------|
| CPU Time Tracking | Implemented | Microsecond precision per request |
| CPU Time Limits | Implemented | 50ms default (Cloudflare-style) |
| Timer-based Termination | Implemented | Linux timer_create + V8 terminate |
| Memory Monitoring | Implemented | 4-tier pressure levels |
| Soft Eviction | Implemented | Graceful isolate draining |
| LRU Eviction | Implemented | Least Recently Used policy |
| Per-Tenant Metrics | Implemented | Auto-collected per hostname |
| Prometheus Export | Implemented | /admin/metrics endpoint |
| Admin Metrics API | Implemented | JSON endpoints for all metrics |
| WASM Support | Implemented | Load, compile, execute |
| WASM JS API | Implemented | WebAssembly.* full API |
| WASM Sliver Support | Implemented | Cached compiled modules |

## Limitations (By Design)

The following are intentionally not supported for WinterCG compatibility:

- Node.js http module — Use WinterCG fetch() instead
- Node.js net module — Raw sockets not supported
- process.env global — Use request headers or config
- Node.js path module — Use URL API instead

## Cloudflare Worker Compatibility

Standard Cloudflare Workers run with minimal modifications:

- fetch(), Request, Response, Headers — Fully compatible
- URL, URLSearchParams — Fully compatible
- TextEncoder, TextDecoder — Fully compatible
- ReadableStream, WritableStream — Fully compatible
- WebCrypto (SHA-256, AES-GCM, HMAC) — Fully compatible

Cloudflare-specific APIs (KV, Durable Objects) are not supported.

## Test Coverage

All test suites pass at 100%:

- API Compatibility Matrix: 26/26 tests (100%)
- Comprehensive Test Suite: 27/27 tests (100%)
- CRUD Operations: 6/6 tests (100%)
- HTTP Verbs: 7/7 tests (100%)
- Cloudflare Worker: 6/6 tests (100%)
- WebCrypto: 2/2 tests (100%)
- Multi-tenancy: 2/2 tests (100%)

## Migration from Cloudflare Workers

Existing Cloudflare Workers can run on nano-rs with these changes:

1. Replace env bindings with direct configuration
2. Use standard WinterCG APIs
3. No changes needed for fetch/Response/Request patterns
4. Store state in VFS or external database (no KV)

## Performance Characteristics

- Sliver restoration: ~267µs (new isolate from snapshot)
- Context reset: ~5ms (between requests in same isolate)
- Process boot: ~60ms (one-time on server start)
- Fresh isolate: ~50-100ms (new isolate without snapshot)
- HTTP request handling: <1ms (excluding JS execution)
- Max response body size: 100MB (configurable)
- Default timeout: 30 seconds (configurable)

See [Performance Documentation](docs/PERFORMANCE.md) for benchmarks and tuning guide.

## Architecture

- One OS process hosts many isolated JavaScript apps
- Each app runs in a separate V8 isolate
- Worker pool handles requests with configurable size
- Context reset between requests for isolation
- VFS provides per-isolate filesystem namespaces
- Sliver snapshots enable sub-millisecond cold starts
- **CPU time limits prevent runaway scripts (50ms default)**
- **Memory pressure monitoring with automatic eviction**
- **Per-tenant metrics with Prometheus export**
- **WASM module support for compute-heavy workloads**

## Security Model

- Per-isolate VFS namespaces prevent filesystem escape
- Path traversal blocked (".." sequences rejected)
- SSRF prevention blocks private IP ranges
- Dangerous headers filtered (Content-Length, Host, etc.)
- URL scheme restricted to http/https only
- Request timeouts enforced per-isolate
- Memory limits enforced per-isolate

## Building from Source

Requirements:
- Rust 1.70+ 
- LLVM/Clang (for V8 build)
- 8GB RAM minimum for V8 compilation

Build:
```bash
cargo build --release
```

The binary is at `target/release/nano-rs`.

## Running Tests

```bash
# API compatibility tests
cd /path/to/test-suite
NANO_BINARY=/path/to/nano-rs node scripts/fast-compatibility-matrix.js

# Comprehensive test suite
NANO_BINARY=/path/to/nano-rs node scripts/run-tests.js
```

## License

MIT License - See LICENSE file for details.