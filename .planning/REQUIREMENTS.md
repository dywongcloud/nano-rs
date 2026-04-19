# Requirements: NANO Edge Runtime

**Defined:** 2026-04-19  
**Core Value:** One OS process hosts many isolated JS apps with millisecond cold starts, zero container overhead, and strong per-app isolation.

## v1 Requirements

### Foundation

- [ ] **FND-01**: Rust project skeleton with cargo workspace
- [ ] **FND-02**: rusty_v8 integration with platform initialization
- [ ] **FND-03**: EPT fix: strong v8::Global sentinel per isolate (prevents AP-02 SIGSEGV)
- [ ] **FND-04**: Basic "hello world" JS execution through V8

### HTTP Server Core

- [ ] **HTTP-01**: HTTP server (axum) binding to configurable port
- [ ] **HTTP-02**: Virtual host routing via Host header (exact hostname match)
- [ ] **HTTP-03**: Request/Response object mapping (WinterCG compatible)
- [ ] **HTTP-04**: Headers API implementation
- [ ] **HTTP-05**: URL/URLSearchParams implementation

### JavaScript Runtime APIs

- [x] **API-01**: fetch() handler interface (Cloudflare Workers style) ✅
- [x] **API-02**: console.log/warn/error output to structured logs ✅
- [x] **API-03**: TextEncoder/TextDecoder for UTF-8 ✅
- [x] **API-04**: setTimeout/setInterval/clearTimeout/clearInterval (event loop integration) ✅
- [x] **API-05**: AbortController/AbortSignal for cancellation ✅
- [x] **API-06**: structuredClone() for object serialization ✅
- [x] **API-07**: crypto.getRandomValues() for random bytes ✅
- [x] **API-08**: performance.now() for high-res timing ✅
- [x] **API-09**: Blob and FormData for binary/form handling ✅
- [x] **API-10**: DOMException for error types ✅

### WorkerPool & Dispatch

- [ ] **POOL-01**: WorkerPool per virtual host (N worker threads)
- [ ] **POOL-02**: WorkQueue: bounded MPSC channel (256-slot capacity)
- [ ] **POOL-03**: Affine dispatch: hostname → pool index → worker thread
- [ ] **POOL-04**: Context reset: dispose/recreate V8 context between requests (~5ms)
- [ ] **POOL-05**: Isolate-per-thread enforcement (never move isolates across threads)

### Multi-App Hosting

- [ ] **HOST-01**: JSON config file for app definitions (hostname → entry point)
- [ ] **HOST-02**: Per-app memory limits enforced
- [ ] **HOST-03**: Per-app timeout enforcement (watchdog timer)
- [ ] **HOST-04**: Per-app environment variables injection
- [ ] **HOST-05**: Hot-reload via config file watcher (2s poll interval)
- [ ] **HOST-06**: Graceful drain: complete in-flight requests before config swap

### Outbound I/O

- [ ] **IO-01**: Outbound fetch() via tokio (non-blocking)
- [ ] **IO-02**: ReadableStream for response body streaming
- [ ] **IO-03**: WritableStream for request body streaming

### Production Features

- [ ] **PROD-01**: Structured JSON logging (ts, level, event, hostname, request_id)
- [ ] **PROD-02**: Metrics endpoint (/_admin/metrics): request counts, latency histograms, error rates
- [ ] **PROD-03**: Graceful shutdown: SIGTERM/SIGINT handling with request drain
- [ ] **PROD-04**: OOM detection and heap limit enforcement
- [ ] **PROD-05**: HTTP Admin API on separate port (8889) with API key authentication
- [ ] **PROD-06**: Unix domain socket for local admin access (/var/run/nano/control.sock)
- [ ] **PROD-07**: Runtime app management: add/remove/disable/scale without restart
- [ ] **PROD-08**: Admin diagnostics endpoint (/admin/isolates) with ps-style output

### Framework Compatibility

- [ ] **FRAME-01**: Hono.js apps run without modification (test with hello-world + middleware)
- [ ] **FRAME-02**: Next.js static export apps serve correctly (HTML/CSS/JS assets)
- [ ] **FRAME-03**: Astro static build apps serve correctly (islands architecture preserved)
- [ ] **FRAME-04**: Generic WinterCG-compatible apps run (not framework-specific)

### Crypto Core

- [ ] **CRYPT-01**: crypto.subtle.generateKey (AES-GCM, HMAC)
- [ ] **CRYPT-02**: crypto.subtle.importKey/exportKey (JWK format)
- [ ] **CRYPT-03**: crypto.subtle.encrypt/decrypt (AES-GCM via ring crate)
- [ ] **CRYPT-04**: crypto.subtle.sign/verify (HMAC via ring crate)

## v2 Requirements

### Advanced APIs

- **ADV-01**: TransformStream for stream piping
- **ADV-02**: CompressionStream/DecompressionStream (gzip/deflate via flate2)
- **ADV-03**: Full WebSocket server (RFC 6455, Cloudflare Workers compatible WebSocketPair API)
- **ADV-04**: Full crypto.subtle: ECDSA (P-256), RSA-PSS (via p256/rsa crates)

### Platform Extensions

- **EXT-01**: VFS (Virtual Filesystem): per-isolate in-memory KV (Nano.fs.*)
- **EXT-02**: Inter-isolate messaging: sendMessage() / addEventListener('message')
- **EXT-03**: V8 startup snapshot for ~2ms cold starts (vs ~50-100ms)
- **EXT-04**: Agent tools API: structured tool definitions callable from JS

### Node.js Compatibility (Limited)

- **NODE-01**: Buffer global (for packages expecting Node.js)
- **NODE-02**: process.env passthrough
- **NODE-03**: Minimal path module polyfill

## Out of Scope

| Feature | Reason |
|---------|--------|
| npm package resolution | Apps are single-file; bundling is user responsibility |
| TypeScript/JSX transpilation | User must bundle with esbuild/tsc beforehand |
| Native module support (node-gyp) | Breaks isolate security model |
| Subprocess spawning | Security boundary violation |
| Real filesystem access | VFS only; no disk I/O from JS |
| Built-in horizontal clustering | Single-machine focus; use external LB for scale |
| Global edge network | Self-hosted; no CDN integration |
| Durable Objects/stateful coordination | Adds complexity; use external DB |
| HTML Rewriting API | Very high complexity; niche use case |
| TCP/UDP socket outbound | Security risk; use outbound fetch() instead |
| queueMicrotask | WinterTC gap; workaround: Promise.resolve() |
| atob/btoa | WinterTC gap; use Buffer or TextEncoder instead |

## Design Principles

**Firecracker VM for Edge:**
- Lightweight: <2MB memory overhead per isolate
- Fast: sub-5ms cold start with snapshots
- Isolated: V8 isolates are security boundaries (no shared heap)
- Simple: one binary, one config file, no orchestration

**Framework Focus:**
- Primary: Hono.js (lightweight, fast, WinterCG-native)
- Secondary: Next.js static export, Astro static build
- Compatibility: Any WinterCG-compliant framework

**Not a General Runtime:**
- No npm resolution (use bundlers)
- No Node.js API surface (except minimal v2 compat)
- Not competing with Deno/Node/Bun
- Specialized for high-density multi-tenant hosting

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| FND-01 | Phase 1 | Pending |
| FND-02 | Phase 1 | Pending |
| FND-03 | Phase 1 | Pending |
| FND-04 | Phase 1 | Pending |
| HTTP-01 | Phase 2 | Pending |
| HTTP-02 | Phase 2 | Pending |
| HTTP-03 | Phase 2 | Pending |
| HTTP-04 | Phase 2 | Pending |
| HTTP-05 | Phase 2 | Pending |
| API-01 | Phase 3 | Pending |
| API-02 | Phase 3 | Pending |
| API-03 | Phase 3 | Pending |
| API-04 | Phase 3 | Complete ✅ |
| API-05 | Phase 3 | Complete ✅ |
| API-06 | Phase 3 | Pending |
| API-07 | Phase 3 | Pending |
| API-08 | Phase 3 | Pending |
| API-09 | Phase 3 | Pending |
| API-10 | Phase 3 | Pending |
| POOL-01 | Phase 4 | Pending |
| POOL-02 | Phase 4 | Pending |
| POOL-03 | Phase 4 | Pending |
| POOL-04 | Phase 4 | Pending |
| POOL-05 | Phase 4 | Pending |
| HOST-01 | Phase 5 | Pending |
| HOST-02 | Phase 5 | Pending |
| HOST-03 | Phase 5 | Pending |
| HOST-04 | Phase 5 | Pending |
| HOST-05 | Phase 5 | Pending |
| HOST-06 | Phase 5 | Pending |
| IO-01 | Phase 6 | Pending |
| IO-02 | Phase 6 | Pending |
| IO-03 | Phase 6 | Pending |
| PROD-01 | Phase 7 | Pending |
| PROD-02 | Phase 7 | Pending |
| PROD-03 | Phase 7 | Pending |
| PROD-04 | Phase 7 | Pending |
| PROD-05 | Phase 7 | Pending |
| PROD-06 | Phase 7 | Pending |
| PROD-07 | Phase 7 | Pending |
| PROD-08 | Phase 7 | Pending |
| FRAME-01 | Phase 8 | Pending |
| FRAME-02 | Phase 8 | Pending |
| FRAME-03 | Phase 8 | Pending |
| FRAME-04 | Phase 8 | Pending |
| CRYPT-01 | Phase 9 | Pending |
| CRYPT-02 | Phase 9 | Pending |
| CRYPT-03 | Phase 9 | Pending |
| CRYPT-04 | Phase 9 | Pending |

**Coverage:**
- v1 requirements: 42 total
- Mapped to phases: 42
- Unmapped: 0 ✓

---
*Requirements defined: 2026-04-19 after research synthesis*
*Last updated: 2026-04-19 after initial definition*
