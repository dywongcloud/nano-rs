# NANO Edge Runtime — Phase Roadmap

**Version:** v1 (Rust Migration)  
**Created:** 2026-04-19  
**Granularity:** Coarse (9 phases, 42 requirements)  
**Mode:** YOLO (auto-approve execution)

## Context

This roadmap maps NANO's migration from Zig to Rust, delivering a multi-tenant edge JavaScript runtime. Each phase completes a verifiable capability, building toward full WinterCG compliance and framework compatibility.

**Critical Priority:** Phase 1 includes the EPT initialization fix (FND-03) to prevent the SIGSEGV crashes that affected the Zig implementation.

**Architecture preserved:** WorkerPool → WorkQueue → isolate-per-thread model from Zig version, with context reset (~5ms) between requests.

## Phases

- [x] **Phase 1: V8 Foundation** — rusty_v8 integration with EPT fix, single isolate proof-of-concept ✅
- [x] **Phase 2: HTTP Server Core** — axum server with virtual host routing and WinterCG request/response objects ✅
- [x] **Phase 3: Runtime APIs** — Core JavaScript APIs (fetch handler, console, timers, encoding, crypto basics) ✅
- [x] **Phase 4: WorkerPool & Dispatch** — Multi-threaded worker pools with context reset and affine dispatch ✅
- [ ] **Phase 5: Multi-App Hosting** — JSON config, per-app isolation, hot-reload, graceful drain
- [ ] **Phase 6: Outbound I/O** — Async fetch() from JavaScript with streaming request/response bodies
- [ ] **Phase 7: Production Features & Admin API** — Structured logging, metrics endpoint, graceful shutdown, OOM detection, HTTP/Unix socket admin API for runtime management
- [ ] **Phase 8: Framework Compatibility** — Hono.js, Next.js static export, Astro static build support
- [ ] **Phase 9: Crypto Core** — Full crypto.subtle implementation using Rust crypto crates

## Phase Details

### Phase 1: V8 Foundation
**Goal:** V8 platform initializes safely with EPT fix; can execute basic JavaScript
**Depends on:** Nothing (first phase)
**Requirements:** FND-01, FND-02, FND-03, FND-04
**Success Criteria** (what must be TRUE):
  1. `cargo build` produces binary using pre-built rusty_v8 (no V8 compilation)
  2. Platform initializes with strong v8::Global sentinel per isolate (EPT fix prevents SIGSEGV)
  3. JavaScript `console.log("hello")` executes and prints to stdout
  4. Isolate can be created and disposed without memory leaks or crashes
**Plans:** 3 plans

Plans:
- [x] 01-01-PLAN.md — Project skeleton with cargo config and rusty_v8 dependencies
- [x] 01-02-PLAN.md — V8 platform initialization with EPT fix sentinel
- [x] 01-03-PLAN.md — JavaScript execution with console.log binding

### Phase 2: HTTP Server Core
**Goal:** HTTP server accepts requests and routes by Host header with WinterCG-compatible objects
**Depends on:** Phase 1
**Requirements:** HTTP-01, HTTP-02, HTTP-03, HTTP-04, HTTP-05
**Success Criteria** (what must be TRUE):
  1. Server binds to configurable port and responds to HTTP requests
  2. Different Host headers route to different handler logic (virtual host routing)
  3. Request/Response/URL/Headers objects match WinterCG specification structure
  4. URL/URLSearchParams parse and construct URLs correctly
  5. Headers API supports append, set, get, delete, has, forEach operations
**Plans:** 3 plans

Plans:
- [x] 02-01-PLAN.md — HTTP server infrastructure with axum ✅
- [x] 02-02-PLAN.md — Virtual host routing with Host header matching ✅
- [x] 02-03-PLAN.md — WinterCG Request/Response/URL/Headers objects ✅

### Phase 3: Runtime APIs
**Goal:** JavaScript code can use core WinterCG APIs for basic computation and async operations
**Depends on:** Phase 2
**Requirements:** API-01, API-02, API-03, API-04, API-05, API-06, API-07, API-08, API-09, API-10
**Success Criteria** (what must be TRUE):
  1. fetch() handler interface registered in JS receives Request and returns Response
  2. console.log/warn/error output appears in structured log format
  3. TextEncoder/TextDecoder correctly handle UTF-8 encoding/decoding
  4. setTimeout/setInterval fire after specified delays; clear functions cancel them
  5. AbortController/AbortSignal enable cancellation of async operations
  6. structuredClone() deep-cycles objects including ArrayBuffers
  7. crypto.getRandomValues() fills Uint8Array with cryptographically random bytes
  8. performance.now() returns monotonic high-resolution timestamps
  9. Blob and FormData handle binary data and form field construction
  10. DOMException throws with standard error names and messages
**Plans:** 4 plans

Plans:
- [x] 03-01-PLAN.md — JavaScript handler interface with Request/Response flow ✅
- [x] 03-02-PLAN.md — Console API and TextEncoder/TextDecoder ✅
- [x] 03-03-PLAN.md — setTimeout/setInterval and AbortController/AbortSignal ✅
- [x] 03-04-PLAN.md — crypto, performance, structuredClone, Blob, FormData, DOMException ✅

### Phase 4: WorkerPool & Dispatch
**Goal:** Requests dispatch to isolated worker threads with proper context lifecycle management
**Depends on:** Phase 3
**Requirements:** POOL-01, POOL-02, POOL-03, POOL-04, POOL-05
**Success Criteria** (what must be TRUE):
  1. WorkerPool spawns N worker threads per app (configurable)
  2. WorkQueue uses bounded MPSC channel with 256-slot backpressure
  3. Same hostname always routes to same pool index (affine dispatch)
  4. Context reset between requests completes in <10ms (dispose + recreate)
  5. Isolates never move between threads (thread-local ownership enforced)
**Plans:** 3 plans

Plans:
- [x] 04-01-PLAN.md — WorkerPool infrastructure with thread-local isolate ownership ✅
- [x] 04-02-PLAN.md — WorkQueue with bounded MPSC channel and affine dispatch ✅
- [x] 04-03-PLAN.md — Context lifecycle management with sub-10ms context reset ✅

### Phase 5: Multi-App Hosting
**Goal:** Multiple isolated apps run in parallel with per-app configuration and resource limits
**Depends on:** Phase 4
**Requirements:** HOST-01, HOST-02, HOST-03, HOST-04, HOST-05, HOST-06
**Success Criteria** (what must be TRUE):
  1. JSON config file maps hostnames to entry point JS files
  2. Per-app memory limits trigger OOM when exceeded
  3. Per-app timeout watchdog terminates long-running requests
  4. Per-app environment variables inject into JS global scope
  5. Config file changes trigger hot-reload within 2 seconds
  6. In-flight requests complete before config swap (graceful drain)
**Plans:** 3 plans

Plans:
- [ ] 05-01-PLAN.md — JSON config loading and app registry with env vars
- [ ] 05-02-PLAN.md — Per-app memory limits and timeout enforcement
- [ ] 05-03-PLAN.md — Hot-reload with graceful drain and config watcher

### Phase 6: Outbound I/O
**Goal:** JavaScript can make non-blocking outbound HTTP requests with streaming support
**Depends on:** Phase 5
**Requirements:** IO-01, IO-02, IO-03
**Success Criteria** (what must be TRUE):
  1. fetch() from JavaScript makes outbound HTTP requests via tokio/hyper
  2. Response body streams via ReadableStream (backpressure handled)
  3. Request body streams via WritableStream for large uploads
**Plans:** 2 plans

Plans:
- [ ] 06-01-PLAN.md — Outbound fetch() with hyper client and ReadableStream response bodies
- [ ] 06-02-PLAN.md — WritableStream for streaming request body uploads

### Phase 7: Production Features & Admin API
**Goal:** Runtime has production-grade observability, metrics, operational stability, and remote management capabilities
**Depends on:** Phase 6
**Requirements:** PROD-01, PROD-02, PROD-03, PROD-04, PROD-05, PROD-06, PROD-07, PROD-08
**Success Criteria** (what must be TRUE):
  1. Structured JSON logs include timestamp, level, event, hostname, request_id
  2. GET /_admin/metrics returns Prometheus-compatible request/latency/error metrics
  3. SIGTERM/SIGINT triggers graceful shutdown with in-flight request drain
  4. Heap limit exceeded triggers OOM detection and isolate termination
  5. HTTP Admin API on port 8889 (configurable) with API key authentication
  6. Unix domain socket at /var/run/nano/control.sock for local access
  7. Admin endpoints: /admin/isolates, /admin/apps, /admin/logs, /admin/drain, /admin/reload
  8. Runtime app CRUD: add, remove, disable, enable, scale workers without restart
**Plans:** 6 plans — ✅ **COMPLETE** (31 commits, 2026-04-19)

Plans:
- [x] PLAN.md — Phase 7 master plan (6 executable plans)
- [x] 07-01 — Structured JSON Logging with rich context fields (7 commits)
- [x] 07-02 — Prometheus Metrics Endpoint with counters/gauges/histograms (4 commits)
- [x] 07-03 — Graceful Shutdown with SIGTERM/SIGINT handling (5 commits)
- [x] 07-04 — OOM Detection Integration with heap monitoring (5 commits)
- [x] 07-05 — Admin API HTTP Server with API key authentication (6 commits)
- [x] 07-06 — Unix Domain Socket Admin for local access (4 commits)

### Phase 8: Framework Compatibility
**Goal:** Popular JavaScript frameworks run without modification on NANO
**Depends on:** Phase 7
**Requirements:** FRAME-01, FRAME-02, FRAME-03, FRAME-04
**Success Criteria** (what must be TRUE):
  1. Hono.js hello-world app with middleware responds correctly
  2. Next.js static export (HTML/CSS/JS assets) serves all files correctly
  3. Astro static build (islands architecture) renders and hydrates correctly
  4. Generic WinterCG-compliant app (not framework-specific) runs correctly
**Plans:** TBD
**UI hint:** yes

### Phase 9: Crypto Core
**Goal:** Full WebCrypto implementation for encryption, signing, and key management
**Depends on:** Phase 8
**Requirements:** CRYPT-01, CRYPT-02, CRYPT-03, CRYPT-04
**Success Criteria** (what must be TRUE):
  1. crypto.subtle.generateKey creates AES-GCM and HMAC keys
  2. crypto.subtle.importKey/exportKey handle JWK format for supported algorithms
  3. crypto.subtle.encrypt/decrypt work with AES-GCM (via ring crate)
  4. crypto.subtle.sign/verify work with HMAC (via ring crate)
**Plans:** TBD

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. V8 Foundation | 3/3 | ✅ Complete | 2026-04-19 |
| 2. HTTP Server Core | 3/3 | ✅ Complete | 2026-04-19 |
| 3. Runtime APIs | 4/4 | ✅ Complete | 2026-04-19 |
| 4. WorkerPool & Dispatch | 3/3 | ✅ Complete | 2026-04-19 |
| 5. Multi-App Hosting | 0/3 | Not started | - |
| 6. Outbound I/O | 0/2 | Planned | - |
| 7. Production Features | 0/5 | Not started | - |
| 8. Framework Compatibility | 0/2 | Not started | - |
| 9. Crypto Core | 0/2 | Not started | - |

## Success Criteria by Phase

### Phase 1
- [x] Platform initializes without EPT crashes (100 isolate stress test passed)
- [x] Basic JS execution works (console.log("hello") prints to stdout)

### Phase 2
- [x] HTTP server responds to requests
- [x] Virtual host routing functional
- [x] WinterCG objects compatible

### Phase 3
- [x] fetch() handler interface works
- [x] Console, timers, encoding, basic crypto functional
- [x] All 10 core APIs verified

### Phase 4
- [x] Multi-threaded dispatch working
- [x] Context reset <10ms
- [x] Thread safety enforced

### Phase 5
- [ ] Multiple apps isolated
- [ ] Config hot-reload working
- [ ] Resource limits enforced

### Phase 6
- [ ] Outbound fetch() from JS works
- [ ] Streaming I/O functional

### Phase 7
- [ ] Logging and metrics visible
- [ ] Graceful shutdown works
- [ ] OOM detection functional
- [ ] Admin API for runtime management
- [ ] ps-style diagnostics remotely accessible
- [ ] Hot-add/remove apps without restart

### Phase 8
- [ ] Hono.js apps run
- [ ] Next.js static export serves
- [ ] Astro static build works

### Phase 9
- [ ] AES-GCM encrypt/decrypt works
- [ ] HMAC sign/verify works
- [ ] Key import/export functional

## Dependency Graph

```
Phase 1 (V8 Foundation)
    ↓
Phase 2 (HTTP Server Core)
    ↓
Phase 3 (Runtime APIs)
    ↓
Phase 4 (WorkerPool & Dispatch)
    ↓
Phase 5 (Multi-App Hosting)
    ↓
Phase 6 (Outbound I/O)
    ↓
Phase 7 (Production Features)
    ↓
Phase 8 (Framework Compatibility)
    ↓
Phase 9 (Crypto Core)
```

## Critical Path

The critical path for minimum viable product (MVP) is Phases 1-5:
- **Phase 1-2:** Foundation and HTTP (can receive requests)
- **Phase 3:** Runtime APIs (can execute JS)
- **Phase 4-5:** Multi-tenancy (can host multiple apps securely)

After Phase 5, NANO can host multiple isolated JavaScript applications. Phases 6-9 add production polish, framework compatibility, and advanced features.

## Backlog

### Phase 999.1: Isolate Checkpoint/Restore (Docker-like Image Portability) — BACKLOG

**Goal:** Enable serialization and migration of V8 isolates between NANO instances  
**Description:** Implement a mechanism to "snapshot" a running isolate's state (JavaScript heap, compiled code, global state) and serialize it for transport. This would allow:
- Saving isolate state to disk (isolate images)
- Loading saved isolates on different NANO instances
- Fast cold-start by restoring from pre-warmed snapshots
- Potential use cases: session persistence, function migration, backup/restore

**Key Technical Challenges:**
- V8 snapshot API limitations (v8::SnapshotCreator)
- Handling external references (bindings to Rust code)
- Version compatibility between NANO/V8 versions
- Security implications of serialized code execution

**Requirements:** TBD  
**Plans:** 0 plans  
**Priority:** Post-v1 exploration

Plans:
- [ ] TBD (promote with /gsd-review-backlog when ready)

### Phase 999.2: Virtual File System (VFS) for Isolates — BACKLOG

**Goal:** Provide a per-isolate in-memory filesystem for carrying data and files  
**Description:** Implement a VFS layer that allows isolates to have their own private filesystem namespace, similar to a container's filesystem. This enables:
- Self-contained "isolate images" with bundled assets
- File I/O operations (read/write) within the isolate
- Data persistence across requests (within the same isolate context)
- Isolation between apps (each sees only its own files)

**Use Cases:**
- Static assets bundled with the isolate image
- Temporary file storage during request processing
- Small data persistence without external storage
- Compatibility with libraries that expect filesystem access

**API Surface:**
```javascript
Nano.fs.readFile(path) -> Uint8Array
Nano.fs.writeFile(path, data)
Nano.fs.exists(path) -> boolean
Nano.fs.mkdir(path)
```

**Requirements:** TBD  
**Plans:** 0 plans  
**Priority:** Post-v1, after checkpoint/restore research

Plans:
- [ ] TBD (promote with /gsd-review-backlog when ready)

### Phase 999.3: Admin API Hybrid Design (HTTP + Unix Socket) — BACKLOG

**Goal:** Operational control plane for running NANO instances with both remote HTTP and local Unix socket access
**Description:** Building on the diagnostics foundation from multi-app testing, implement a hybrid admin interface:

**HTTP Admin Port (8889 by default):**
- `GET /admin/isolates` - ps-style diagnostics output
- `GET /admin/apps` - list all apps with config
- `POST /admin/apps` - add new app dynamically
- `DELETE /admin/apps/:hostname` - disable/remove app
- `POST /admin/apps/:hostname/reload` - reload specific app
- `POST /admin/apps/:hostname/scale` - adjust worker count
- `GET /admin/logs?app=X&follow=true` - streaming logs
- `POST /admin/drain` - graceful drain for deployment
- `GET /admin/metrics` - Prometheus-compatible metrics
- `GET /admin/health` - health check endpoint

**Unix Domain Socket (`/var/run/nano/control.sock`):**
- Local-only emergency access
- Bypasses network stack (faster)
- Filesystem permission-based security
- Used by CLI tool: `nano-cli isolates`, `nano-cli logs`, etc.

**Authentication:**
- HTTP: API key in `X-Admin-Key` header
- Socket: Unix filesystem permissions (owner/group)

**Use Cases:**
- Remote monitoring and alerting (HTTP)
- CI/CD deployment automation (HTTP)
- Local debugging and troubleshooting (socket)
- Emergency recovery when network down (socket)

**Requirements:** Phase 7 (PROD-05 through PROD-08)
**Plans:** Covered in Phase 7 plans
**Priority:** Phase 7 (after Phase 6 Outbound I/O)

## Revision History

| Date | Change |
|------|--------|
| 2026-04-19 | Initial roadmap created with 9 phases mapping 42 v1 requirements |
| 2026-04-19 | Added backlog items 999.1 (Isolate Checkpoint/Restore) and 999.2 (VFS) |
| 2026-04-19 | Updated Phase 7 with Admin API requirements (PROD-05 through PROD-08) |
| 2026-04-19 | Added backlog item 999.3 documenting hybrid HTTP/Unix socket design |

---
*Roadmap version: 1.0 | Last updated: 2026-04-19*
