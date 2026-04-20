# NANO — Edge JavaScript Runtime

## Current State

**Version:** v1.1 IN PROGRESS 🚧  
**Date:** 2026-04-19  
**Status:** Developing isolate snapshots and VFS for container-like semantics

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

## v1.1 Goals — Isolate Snapshots & VFS

**Theme:** Container-image semantics for JavaScript isolates

### Snapshot Features
- **Build:** `nano-rs snapshot create <hostname>` produces `app-v1.tar`
- **Run:** `nano-rs run --snapshot app-v1.tar` restores isolate state
- **Fast starts:** ~1-2ms cold start from snapshot (vs ~5ms context reset, ~50-100ms fresh isolate)
- **Migration:** Move isolates between NANO instances for load balancing
- **Checkpoint/Restore:** Save and resume isolate state

### VFS (Virtual File System)
- **JS API:** `Nano.fs.readFile('/data/session.json')` — makes isolates look like containers
- **Storage:** Filesystem or object storage (S3-compatible) backing
- **Per-isolate:** Each app has isolated filesystem namespace
- **In-memory:** Fast access with optional persistence

### Design Principles
- **Format:** Simple tar-based, extensible to deltas later
- **Opaque blobs:** Version-agnostic snapshots (no fancy versioning)
- **Container-inspired:** Familiar semantics for DevOps teams

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

### Active (v1.1)

- [ ] **SNAP-01:** CLI `nano-rs snapshot create <hostname>` produces tar archive
- [ ] **SNAP-02:** Snapshot tar contains V8 isolate heap + VFS state + metadata
- [ ] **SNAP-03:** CLI `nano-rs run --snapshot app-v1.tar` restores isolate from snapshot
- [ ] **SNAP-04:** Restored isolate resumes execution with preserved state
- [ ] **SNAP-05:** Snapshot format is tar-based with simple structure
- [ ] **SNAP-06:** Snapshots are opaque blobs (version-agnostic, no embedded versioning)
- [ ] **SNAP-07:** Multiple snapshots can coexist (versioned by filename)
- [ ] **VFS-01:** VFS module at `src/vfs/` with in-memory storage
- [ ] **VFS-02:** Per-isolate filesystem namespace (no cross-app access)
- [ ] **VFS-03:** JS API `Nano.fs.readFile(path)` reads file contents
- [ ] **VFS-04:** JS API `Nano.fs.writeFile(path, data)` writes file contents
- [ ] **VFS-05:** JS API `Nano.fs.exists(path)` checks file existence
- [ ] **VFS-06:** Optional disk backing for VFS persistence
- [ ] **VFS-07:** Optional S3-compatible object storage backend
- [ ] **VFS-08:** VFS state included in snapshot serialization
- [ ] **PERF-01:** Cold start from snapshot achieves ~1-2ms latency
- [ ] **MIGRATE-01:** Snapshot can be transferred between NANO instances
- [ ] **CLI-01:** `nano-rs snapshot list` shows available snapshots
- [ ] **CLI-02:** `nano-rs snapshot delete <name>` removes snapshot

### Out of Scope (v1.1)

| Feature | Reason |
|---------|--------|
| Delta/differential snapshots | Complex, defer to v1.2+ |
| Live migration (running isolates) | Requires freeze/thaw, significant complexity |
| npm package resolution | Apps remain single-file, bundling is user responsibility |
| TypeScript/JSX transpilation | User must bundle beforehand |
| Native module support | Only pure JS/WinterCG APIs |
| Subprocess spawning from JS | Security/scope constraint |
| Built-in horizontal clustering | External load balancer sufficient |

---

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust + rusty_v8 over Zig | Pre-built V8, type-safe bindings, stable ecosystem | ✅ v1.0 shipped |
| Context reset (not new isolate) | 5ms vs 50-100ms per request | ✅ Performance target met |
| ring over V8 crypto | Safer, avoids V8 internal complexity | ✅ Secure implementation |
| No npm resolution | Simplifies runtime, keeps isolates lightweight | ✅ Maintainable |
| WorkerPool per virtual host | Resource isolation between apps | ✅ Multi-tenant ready |
| Tar-based snapshot format | Simple, portable, extensible to deltas later | 🚧 v1.1 in progress |
| V8 SnapshotCreator API | Standard V8 approach for heap serialization | 🚧 v1.1 in progress |
| In-memory VFS with pluggable backends | Fast default, flexible persistence | 🚧 v1.1 in progress |

---

## Milestones

**v1.0 — Edge Runtime Foundation** ✅ SHIPPED 2026-04-19
- 9 phases, 42 requirements, 151 commits
- Multi-tenant JavaScript edge runtime with WinterCG compliance

**v1.1 — Isolate Snapshots & VFS** 🚧 IN PROGRESS
- Target: Container-image semantics for JS isolates
- Scope: Snapshot create/restore, VFS with JS API, fast cold starts

**v2.0 — Advanced Edge Features** 📋 PLANNED
- WebSocket support for real-time applications
- Advanced crypto (RSA signatures, ECDSA)
- Compression/Decompression streams
- Inter-isolate messaging

---

## Constraints

- **Tech stack**: Rust + rusty_v8 + tokio + axum
- **API surface**: WinterCG Minimum Common API compliance
- **V8 version**: Tracks Deno's rusty_v8 (auto-updates via crate)
- **Build time**: Uses pre-built V8 (no 2-hour compiles)
- **Snapshot format**: Tar-based, extensible, version-agnostic

---

## Evolution

**v1.0 (2026-04-19):** Foundation complete — multi-tenant edge runtime with WinterCG compliance, production observability, and crypto support.

**v1.1 (TBD):** Snapshot & VFS — container-image semantics for isolates with ~1-2ms cold starts and migration capabilities.

**v2.0 (TBD):** Advanced features — WebSockets, advanced crypto, compression streams, inter-isolate messaging.

---

*Last updated: 2026-04-19 — v1.1 milestone started*
