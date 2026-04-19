# NANO — Edge JavaScript Runtime

## Current State

**Version:** v1.0 SHIPPED ✅  
**Date:** 2026-04-19  
**Status:** Production-ready multi-tenant edge runtime

NANO is a single-process HTTP server that hosts multiple JavaScript applications in parallel, each in its own V8 isolate. It replaces container fleets running one Node.js app per pod—eliminating operational overhead, slow startup times, and resource waste. One binary, one config file, many isolated apps.

**Core Value:** Skip the container fleet entirely—one OS process hosts many isolated JS apps with millisecond cold starts, zero container overhead, and strong per-app isolation.

---

## What v1.0 Delivered

### Foundation
- ✅ Rust + rusty_v8 integration (pre-built V8 binaries)
- ✅ EPT fix: strong v8::Global sentinel prevents SIGSEGV
- ✅ V8 platform initialization and isolate management

### HTTP & Routing
- ✅ axum HTTP server with configurable port/host
- ✅ Virtual host routing (Host header → app mapping)
- ✅ WinterCG Request/Response/URL/Headers objects

### JavaScript Runtime
- ✅ fetch() handler interface (export default { fetch })
- ✅ console, TextEncoder/TextDecoder
- ✅ setTimeout/setInterval with AbortController
- ✅ crypto.getRandomValues, performance.now()
- ✅ structuredClone, Blob, FormData, DOMException

### Multi-Tenancy
- ✅ WorkerPool with N workers per app
- ✅ WorkQueue with bounded MPSC channel
- ✅ Context reset between requests (~5ms)
- ✅ JSON config loading with validation
- ✅ Per-app memory limits and timeout enforcement
- ✅ Hot-reload with graceful drain

### Production Features
- ✅ Structured JSON logging
- ✅ Prometheus metrics endpoint
- ✅ Graceful shutdown (SIGTERM/SIGINT)
- ✅ OOM detection and isolate termination
- ✅ HTTP Admin API (port 8889)
- ✅ Unix domain socket admin

### I/O & Crypto
- ✅ Outbound fetch() via tokio/hyper
- ✅ ReadableStream/WritableStream for streaming
- ✅ crypto.subtle (AES-GCM, HMAC, JWK)
- ✅ SSRF prevention and header filtering

### Framework Support
- ✅ Hono.js apps
- ✅ Next.js static export
- ✅ Astro static build
- ✅ Generic WinterCG compatibility

---

## Requirements

### Validated (v1.0)

- [x] Rust project skeleton with rusty_v8 integration
- [x] Platform initialization and single V8 isolate
- [x] HTTP server (axum) with fetch() handler interface
- [x] Core WinterCG APIs: Request/Response/Headers/URL
- [x] TextEncoder/TextDecoder and console APIs
- [x] crypto.getRandomValues() implementation
- [x] Outbound fetch() via tokio
- [x] WorkerPool with N worker threads per app
- [x] WorkQueue dispatch (tokio channels)
- [x] Virtual host routing (Host header → app mapping)
- [x] Context reset between requests (dispose/recreate V8 context)
- [x] Extended WinterCG: Streams (Readable/Writable)
- [x] crypto.subtle implementation using Rust crypto crates (ring)
- [x] EPT initialization fix (strong v8::Global sentinel)

### Active (v2.0 Candidates)

- [ ] CompressionStream/DecompressionStream (flate2)
- [ ] WebSocket server (RFC 6455)
- [ ] VFS (Virtual Filesystem) per isolate
- [ ] Inter-isolate messaging API
- [ ] V8 startup snapshot support
- [ ] Advanced crypto: RSA, ECDSA signatures

### Out of Scope (v1.0+)

- npm package resolution—apps are single-file, bundling is user responsibility
- TypeScript/JSX transpilation (user must bundle beforehand)
- Native module support (only pure JS/WinterCG APIs)
- Subprocess spawning from JS
- Built-in horizontal clustering (requires external load balancer)
- Global edge network (self-hosted only)
- queueMicrotask, atob/btoa (WinterCG gaps)

---

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust + rusty_v8 over Zig | Pre-built V8, type-safe bindings, stable ecosystem | ✅ v1.0 shipped |
| Context reset (not new isolate) | 5ms vs 50-100ms per request | ✅ Performance target met |
| ring over V8 crypto | Safer, avoids V8 internal complexity | ✅ Secure implementation |
| No npm resolution | Simplifies runtime, keeps isolates lightweight | ✅ Maintainable |
| WorkerPool per virtual host | Resource isolation between apps | ✅ Multi-tenant ready |

---

## Next Milestone Goals (v2.0)

**Target:** Advanced features for production edge workloads

**Potential scope:**
- WebSocket support for real-time applications
- VFS for static asset hosting and data persistence
- Advanced crypto (RSA signatures, ECDSA)
- Compression/Decompression streams
- Inter-isolate messaging
- V8 snapshots for ~2ms cold starts

**Start planning:** `/gsd-new-milestone`

---

## Constraints

- **Tech stack**: Rust + rusty_v8 + tokio + axum
- **API surface**: WinterCG Minimum Common API compliance
- **V8 version**: Tracks Deno's rusty_v8 (auto-updates via crate)
- **Build time**: Uses pre-built V8 (no 2-hour compiles)

---

## Evolution

**v1.0 (2026-04-19):** Foundation complete — multi-tenant edge runtime with WinterCG compliance, production observability, and crypto support.

**v2.0 (TBD):** Advanced features — WebSockets, VFS, advanced crypto, performance optimizations.

---

*Last updated: 2026-04-19 after v1.0 milestone completion*
