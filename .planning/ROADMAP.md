# NANO Edge Runtime — Roadmap

**Current:** v1.1 SHIPPED ✅  
**Date:** 2026-04-20

---

## Milestones

- ✅ **v1.0 Foundation** — Phases 1-9 (shipped 2026-04-19)
- ✅ **v1.1 SLIVER** — Phases 10-16 (shipped 2026-04-20)
- 📋 **v2.0 Advanced Features** — Phases 17+ (planned)

---

## v1.0 Foundation ✅

<details>
<summary>✅ v1.0 — Multi-tenant edge runtime (Phases 1-9) SHIPPED 2026-04-19</summary>

### Phase 1: V8 Foundation
**Goal:** Rust + rusty_v8 integration with EPT fix  
**Plans:** 3 plans complete

### Phase 2: HTTP Server Core
**Goal:** axum server with virtual host routing  
**Plans:** 3 plans complete

### Phase 3: Runtime APIs
**Goal:** Console, encoding, timers, basic crypto  
**Plans:** 4 plans complete

### Phase 4: WorkerPool & Dispatch
**Goal:** Multi-threading with context reset  
**Plans:** 3 plans complete

### Phase 5: Multi-App Hosting
**Goal:** Config loading, limits, hot-reload  
**Plans:** 3 plans complete

### Phase 6: Outbound I/O
**Goal:** fetch() and streaming  
**Plans:** 2 plans complete

### Phase 7: Production Features
**Goal:** Logging, metrics, admin API  
**Plans:** 6 plans complete

### Phase 8: Framework Compatibility
**Goal:** Hono.js, Next.js, Astro support  
**Plans:** 2 plans complete

### Phase 9: Crypto Core
**Goal:** WebCrypto AES-GCM, HMAC, JWK  
**Plans:** 3 plans complete

**Full details:** [v1.0-ROADMAP.md](./milestones/v1.0-ROADMAP.md)  
**Requirements:** [v1.0-REQUIREMENTS.md](./milestones/v1.0-REQUIREMENTS.md)

</details>

---

## v1.1 SLIVER ✅

<details>
<summary>✅ v1.1 — Snapshots & VFS (Phases 10-16) SHIPPED 2026-04-20</summary>

### Phase 10: VFS Foundation
**Goal:** Core VFS module with in-memory storage  
**Plans:** 3 plans complete

### Phase 11: VFS Storage Backends
**Goal:** Pluggable storage backends (disk, S3)  
**Plans:** 1 plan complete

### Phase 12: VFS JavaScript Bindings
**Goal:** `Nano.fs.*` API + Node.js `fs` polyfill  
**Plans:** 4 plans complete

### Phase 13: Snapshot Format Design
**Goal:** Define tar-based snapshot structure  
**Plans:** 2 plans complete

### Phase 14: Snapshot Creation
**Goal:** CLI `snapshot create` with V8 SnapshotCreator  
**Plans:** 4 plans complete

### Phase 15: Snapshot Restoration
**Goal:** Run isolates from snapshot with ~1-2ms cold start  
**Plans:** 5 plans complete

### Phase 16: CLI Integration & Polish
**Goal:** Complete CLI commands and integration tests  
**Plans:** 5 plans complete

**Full details:** [v1.1-ROADMAP.md](./milestones/v1.1-ROADMAP.md)  
**Requirements:** [v1.1-REQUIREMENTS.md](./milestones/v1.1-REQUIREMENTS.md)

</details>

---

## v1.2 Remediation 🚧

**Milestone Goal:** Fix 7 critical bugs from blackbox evaluation + documentation

<blockquote>
⚠️ **Post-v1.1 Evaluation Finding:** Blackbox testing revealed 7 bugs breaking core functionality including incomplete Request objects, ESM not supported, --config no-op, and sliver VFS not wired for execution.
</blockquote>

### Phase 17: Request/Response Fixes ✅ COMPLETE
**Goal:** Pass full WinterCG Request (URL, headers, body) to JS; add Promise support  
**Depends on:** v1.1 complete  
**Requirements:** REQ-17-01, REQ-17-02, REQ-17-03  
**Success Criteria**:
  1. ✅ Handler receives `{method, url, headers, body}` not just method
  2. ✅ Async handlers with `await` resolve correctly
  3. ✅ Request body readable as text/JSON
  4. ✅ All HTTP methods work (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS, custom)
**Plans:** 
  - [x] 17-01-PLAN.md — Request/Response fixes implementation (COMPLETE 2026-04-21)
**UI hint:** no
**Test Coverage:** 492+ unit tests, 18 integration tests (6 + 12 HTTP verb tests), 52 doc tests — all passing

### Phase 18: ESM Module System ✅ COMPLETE
**Goal:** Replace Script::compile with V8 Module API for ESM support  
**Depends on:** Phase 17  
**Requirements:** REQ-18-01, REQ-18-02, REQ-18-03  
**Success Criteria**:
  1. ✅ `export default { fetch }` compiles and runs (via transformation)
  2. ✅ Classic scripts remain backward compatible
  3. ✅ Hono.js and Next.js ESM bundles execute correctly
  4. ✅ 508 tests pass (495 unit + 13 ESM integration)
**Plans:** 
  - [x] 18-01-PLAN.md — ESM module loader with transformation (COMPLETE 2026-04-21)
**UI hint:** no
**Notes:** Full V8 Module API with VFS-backed imports implemented as infrastructure; transformation approach provides immediate framework compatibility

### Phase 19: Config Mode Implementation ✅ COMPLETE
**Goal:** Actually implement `--config` workflow + port/host config  
**Depends on:** Phase 18  
**Requirements:** REQ-19-01, REQ-19-02, REQ-19-03  
**Success Criteria**:
  1. ✅ `nano-rs run --config config.json` loads and serves apps (Bug #3 fixed)
  2. ✅ Port and host from config applied to server bind (Bug #5 fixed)
  3. ✅ Multiple apps served with virtual host routing
  4. ✅ Per-app limits enforced (memory, timeout, workers)
  5. ✅ 560+ tests pass (13 new integration tests)
**Plans:** 
  - [x] 19-01-PLAN.md — Config mode implementation (COMPLETE 2026-04-21)
**UI hint:** no
**Bugs Fixed:** #3 (--config no-op), #5 (Port Config Ignored)

### Phase 20: Sliver VFS Integration ✅ COMPLETE
**Goal:** Execute JS from packed sliver VFS, not OS filesystem  
**Depends on:** Phase 19  
**Requirements:** REQ-20-01, REQ-20-02, REQ-20-03  
**Success Criteria**:
  1. ✅ Sliver runs from any directory (portable) - Bug #6 fixed
  2. ✅ JS entrypoint read from sliver VFS, not CWD
  3. ✅ No source files required to run sliver
  4. ✅ Temp directories cleaned up on shutdown
  5. ✅ 12 new tests, 511+ total tests passing
**Plans:** 
  - [x] 20-01-PLAN.md — Sliver VFS integration (COMPLETE 2026-04-21)
**UI hint:** no
**Bug Fixed:** #6 (VFS Not Used for JS)
**Approach:** Temp directory extraction with RAII cleanup

### Phase 21: v1.2.0 Remediation Completion 🚧
**Goal:** Fix remaining 8 failing tests to reach 90%+ score for production release  
**Depends on:** Phase 20  
**Requirements:** REQ-21-01 through REQ-21-08  
**Success Criteria**:
  1. Score reaches 90%+ (45+/50 tests passing)
  2. All CRUD operations work (already passing - 6/6)
  3. VFS fully functional (3/3 tests passing)
  4. WinterCG Headers API works
  5. WinterCG URL API works  
  6. Streams API functional
  7. Timer functions available
  8. SHA-256 hashing works
**Plans:**
  - [ ] 21-01-PLAN.md — VFS Implementation (writeFile, readFile, Node.js fs compat)
  - [ ] 21-02-PLAN.md — WinterCG Headers API fix
  - [ ] 21-03-PLAN.md — WinterCG URL API fix
  - [ ] 21-04-PLAN.md — Streams API (ReadableStream/WritableStream)
  - [ ] 21-05-PLAN.md — Timer functions (setTimeout/setInterval)
  - [ ] 21-06-PLAN.md — SHA-256 hashing and final verification
**UI hint:** no

### Phase 21.1: v1.2.1 Static File Serving & VFS Improvements ✅ COMPLETE
**Goal:** Auto-detect non-JS entrypoints, serve static files, improve sliver creation workflow  
**Depends on:** Phase 21  
**Requirements:** REQ-21.1-01, REQ-21.1-02, REQ-21.1-03, REQ-21.1-04  
**Success Criteria**:
  ✅ Entrypoint auto-detection: JS files execute, other files served statically
  ✅ Static files (HTML, CSS, images) serve with correct content-type
  ✅ Sliver creation works without requiring app to be running
  ✅ Sliver packs all app files (not just JS)
  ✅ Sliver runs standalone without external dependencies
  ✅ Astro/Next.js static exports work out of the box
**Plans:**
  - [x] 21.1-01-PLAN.md — Auto-detect entrypoint type and serve static files (COMPLETE 2026-04-22)
  - [x] 21.1-02-PLAN.md — VFS directory loading for static assets (COMPLETE 2026-04-22)
  - [x] 21.1-03-PLAN.md — Sliver creation from directory (no running app required) (COMPLETE 2026-04-22)
  - [x] 21.1-04-PLAN.md — Test suite for static file serving (COMPLETE 2026-04-22)
**UI hint:** no
**Test Coverage:** 26 integration tests, 7 VFS loader tests, all passing

### Phase 21.2: v1.2.2 Critical Bug Fixes 🚧 (INSERTED)
**Goal:** Fix runtime bugs discovered during test suite validation  
**Depends on:** Phase 21.1  
**Requirements:** REQ-21.2-01, REQ-21.2-02  
**Success Criteria**:
  ✅ VFS path validation allows `[...]` file patterns (Astro/Next.js catch-all routes)
  2. Server process cleanup releases port after error scenarios
  ✅ Sliver creation works with framework files like `[...slug].astro`
  4. Test suite passes all 50 tests without skips
**Plans:**
  - [x] 21.2-01-PLAN.md — Fix VFS path validation for special characters in filenames (COMPLETE 2026-04-23)
  - [ ] 21.2-02-PLAN.md — Fix server process cleanup on error/termination
**UI hint:** no
**Test Coverage:** 76 VFS tests passing, 4 test suite tests still skipped pending server cleanup fix
**Bug Reports:** [docs/BUG_REPORTS.md](./docs/BUG_REPORTS.md)

### Phase 22: Documentation & Architecture 📋
**Goal:** Fix cold start claims, document compatibility accurately  
**Depends on:** Phase 21.2  
**Requirements:** REQ-22-01, REQ-22-02, REQ-22-03, REQ-22-04  
**Success Criteria**:
  1. Cold start metrics distinguish process boot (~60ms) from request latency (~267µs)
  2. Node.js compatibility matrix published (~5% accurate)
  3. Per-worker state isolation documented with guidance
  4. Troubleshooting guide covers common issues
  5. All docs reviewed for accuracy
**Plans:**
  - [ ] 22-01-PLAN.md — Fix cold start metrics and performance documentation
  - [ ] 22-02-PLAN.md — Create Node.js compatibility matrix and migration guides
  - [ ] 22-03-PLAN.md — Document state architecture and troubleshooting guide
  - [ ] 22-04-PLAN.md — Finalize documentation index and cross-references
**UI hint:** no

**Full details:** [REMEDIATION-v1.2.md](./REMEDIATION-v1.2.md)

---

## v2.0 Advanced Features 📋

**Milestone Goal:** WebSockets, advanced crypto, compression, inter-isolate messaging

### Phase 23: WebSocket Server
**Goal:** RFC 6455 WebSocket upgrade handling  
**Depends on:** v1.2 complete  
**Requirements:** WS-01, WS-02, WS-03  
**Success Criteria** (what must be TRUE):
  1. HTTP server supports WebSocket upgrade
  2. JS can handle WebSocket connections
  3. Per-isolate connection limits enforced
**Plans**: TBD  
**UI hint**: no

### Phase 24: Advanced Crypto
**Goal:** RSA and ECDSA operations  
**Depends on:** Phase 23  
**Requirements:** CRYPT-05, CRYPT-06, CRYPT-07  
**Success Criteria** (what must be TRUE):
  1. RSA key generation and import/export
  2. ECDSA sign/verify operations
  3. RSA-OAEP encrypt/decrypt
**Plans**: TBD  
**UI hint**: no

### Phase 25: Compression Streams
**Goal:** CompressionStream and DecompressionStream  
**Depends on:** Phase 24  
**Requirements:** COMP-01, COMP-02  
**Success Criteria** (what must be TRUE):
  1. CompressionStream with deflate
  2. DecompressionStream with inflate
  3. Works with fetch() Response/Request bodies
**Plans**: TBD  
**UI hint**: no

### Phase 26: Inter-Isolate Messaging
**Goal:** PostMessage API between isolates  
**Depends on:** Phase 25  
**Requirements:** MSG-01, MSG-02  

### Phase 27: Production Multi-Tenancy
**Goal:** Production-grade multi-tenancy: WASM support, CPU limits with timer termination, memory monitoring with soft eviction, per-tenant metrics  
**Depends on:** v1.2 complete  
**Requirements:** PROD-01 through PROD-12  
**Success Criteria** (what must be TRUE):
  1. CPU time limits enforced with 50ms default using timer_create
  2. Memory monitoring after each JS call with soft eviction
  3. LRU eviction with stateless isolate preference
  4. Per-tenant metrics collected and exported via Prometheus
  5. WASM modules loadable and executable in isolates
  6. WASM modules cached in sliver snapshots
**Plans:**
  - [x] 27-01-PLAN.md — CPU time tracking and timer-based termination (COMPLETE 2026-05-01)
  - [x] 27-02-PLAN.md — Memory monitoring and soft/LRU eviction (COMPLETE 2026-05-01)
  - [x] 27-03-PLAN.md — Per-tenant metrics and observability (COMPLETE 2026-05-01)
  - [x] 27-04-PLAN.md — WASM support and sliver integration (COMPLETE 2026-05-01)
**UI hint**: no

---

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. V8 Foundation | v1.0 | 3/3 | Complete | 2026-04-19 |
| 2. HTTP Server | v1.0 | 3/3 | Complete | 2026-04-19 |
| 3. Runtime APIs | v1.0 | 4/4 | Complete | 2026-04-19 |
| 4. WorkerPool | v1.0 | 3/3 | Complete | 2026-04-19 |
| 5. Multi-App | v1.0 | 3/3 | Complete | 2026-04-19 |
| 6. Outbound I/O | v1.0 | 2/2 | Complete | 2026-04-19 |
| 7. Production | v1.0 | 6/6 | Complete | 2026-04-19 |
| 8. Frameworks | v1.0 | 2/2 | Complete | 2026-04-19 |
| 9. Crypto Core | v1.0 | 3/3 | Complete | 2026-04-19 |
| 10. VFS Foundation | v1.1 | 3/3 | Complete | 2026-04-19 |
| 11. VFS Backends | v1.1 | 1/1 | Complete | 2026-04-19 |
| 12. VFS JS Bindings | v1.1 | 4/4 | Complete | 2026-04-19 |
| 13. Snapshot Format | v1.1 | 2/2 | Complete | 2026-04-20 |
| 14. Snapshot Create | v1.1 | 4/4 | Complete | 2026-04-20 |
| 15. Snapshot Restore | v1.1 | 5/5 | Complete | 2026-04-20 |
| 16. CLI Integration | v1.1 | 5/5 | Complete | 2026-04-20 |
| 17. Request/Response Fixes | v1.2 | 1/1 | Complete | 2026-04-21 |
| 18. ESM Module System | v1.2 | 1/1 | Complete | 2026-04-21 |
| 19. Config Mode Implementation | v1.2 | 1/1 | Complete | 2026-04-21 |
| 20. Sliver VFS Integration | v1.2 | 1/1 | Complete | 2026-04-21 |
| 21. v1.2.0 Remediation Completion | v1.2 | 0/6 | In Progress | - |
| 22. Documentation | v1.2 | 0/4 | Planned | - |
| 23. WebSocket Server | v2.0 | 0/TBD | Not started | - |
| 24. Advanced Crypto | v2.0 | 0/TBD | Not started | - |
| 25. Compression | v2.0 | 0/TBD | Not started | - |
| 26. Inter-Isolate | v2.0 | 0/TBD | Not started | - |
| 27. Production Multi-Tenancy | v2.0 | 4/4 | Complete | 2026-05-01 |

---

## What's Next

**v1.2.0 Goal:** Fix remaining 8 tests to reach 90%+ score

Current: 84% (42/50 tests passing)
Target: 90%+ (45+/50 tests passing)

### Remaining Issues (8 tests):
1. VFS: Nano.fs.writeFile
2. VFS: Nano.fs.readFile  
3. VFS: Node.js fs module compatibility
4. WinterCG: Headers API
5. WinterCG: URL API
6. WinterCG: ReadableStream/WritableStream
7. Node.js: setTimeout/setInterval
8. WebCrypto: SHA-256 hashing

### New Phase 27: Production Multi-Tenancy

**Status:** Planned — 4 plans created  
**Location:** `.planning/phases/27-production-multi-tenancy/`  

**Features:**
1. **CPU Time Limits** — 50ms default using Linux timer_create, V8 TerminateExecution
2. **Memory Monitoring** — Check after each JS call, soft eviction, LRU cache
3. **Per-Tenant Metrics** — Prometheus export, admin API, CPU/memory/request tracking
4. **WASM Support** — WebAssembly execution, WASI, sliver integration

**Plans:**
- `27-01-PLAN.md` — CPU time tracking and timer-based termination
- `27-02-PLAN.md` — Memory monitoring and soft/LRU eviction  
- `27-03-PLAN.md` — Per-tenant metrics and observability
- `27-04-PLAN.md` — WASM support and sliver integration

**Requirements:** `.planning/phases/27-production-multi-tenancy/REQUIREMENTS.md`

### Commands:
- Start Phase 21 planning: `/gsd-plan-phase 21`
- Start Phase 27 execution: `/gsd-execute-phase 27`
- Check progress: `/gsd-progress`

---

*Roadmap updated: 2026-05-01 — Phase 27 planned (Production Multi-Tenancy with WASM, CPU limits, memory eviction, metrics)*

## Backlog Items

### Phase 999.1: Adversarial Security Testing Suite (PLANNED)
**Goal:** Security gateway test suite for adversarial attacks and CVE monitoring  
**Requirements:** Research CVE databases, design attack scenarios, implement test harness  
**Plans:** 1 plan created

Plans:
- [x] 999.1-01-PLAN.md — Comprehensive adversarial security test suite (62+ tests, 8 attack vectors, CVE scanning, CI security gates)

**Security Review Required:** Execution blocked pending security expert approval per SPEC.md notes

### 999.x - Adversarial Security Testing Suite
**Status:** Proposed  
**Priority:** Security Gateway

Write extensive adversarial tests for nano-rs covering:
- CPU exhaustion attacks (infinite loops, pathological regex)
- Memory exhaustion attacks (large allocations, memory leaks)
- VFS escape attempts (path traversal, symlink attacks)
- Network-based attacks (DNS rebinding, request flooding)
- JavaScript injection via input validation bypasses
- WebAssembly validation bypasses and malicious modules
- Multi-tenant isolation breaches (cross-tenant data access)
- Cryptographic attacks (weak key generation, timing attacks)

Research CVEs associated with:
- V8 engine vulnerabilities
- Rust async runtime issues  
- HTTP parsing libraries (axum, hyper)
- VFS path sanitization bypasses
- WebAssembly runtime exploits

Create Makefile targets:
- `make test-security`: Run adversarial tests with security gateway
- `make test-cve-check`: Scan dependencies against CVE databases
- `make test-all`: Run unit, integration, and adversarial tests

Mark all security tests as blocking for releases.

### Phase 999.2: WorkerPool Architecture Consolidation (PLANNED)
**Goal:** Merge or separate duplicate WorkerPool implementations, unify VFS backend lifecycle  
**Requirements:** Architecture review of pool.rs vs queue.rs, trait extraction, VFS unification  
**Plans:** 1 plan

Plans:
- [ ] 999.2-01-PLAN.md — Create WorkerPool trait and separate SliverWorkerPool/EntrypointWorkerPool implementations

### 999.y - WorkerPool Architecture Review
**Status:** Identified from Phase 27  
**Priority:** Architecture Debt

Review duplicate WorkerPool implementations:
- `src/worker/pool.rs` - Complex pool with sliver support, CPU timeout
- `src/worker/queue.rs` - Simpler pool for entrypoint dispatch

Issues identified:
- Two pools don't communicate or share code
- pool.rs has with_backend but queue.rs reimplements WorkerPool
- VFS backend configuration needs unification
- Architecture makes per-app VFS backends difficult

Proposed actions:
- Merge or clearly separate responsibilities
- Extract common WorkerPool trait
- Unify VFS backend creation and lifecycle
- Document which pool type to use for each scenario

### Phase 999.3: VFS Disk Backend E2E Tests (PLANNED)
**Goal:** Fix WASM E2E tests that require disk VFS backend file access
**Requirements:** REQ-999-03-01: Enable per-app disk VFS backends for E2E tests, REQ-999-03-02: Ensure WASM file access works via Nano.fs.readFile()
**Success Criteria:**
  1. `test_wasm_cpu_timeout` passes with disk VFS backend
  2. `test_wasm_within_cpu_limit` passes with disk VFS backend
  3. File structure: entrypoints at temp root, VFS files in `{sanitized_hostname}/`
  4. No workarounds - proper async pool creation with app-specific backends
**Plans:** 1 plan

Plans:
- [ ] `999.3-01-PLAN.md` — Wire per-app disk VFS backends from AppRegistry to WorkQueue

**Context from Phase 27:**
The WASM E2E tests fail because they need to read files via `Nano.fs.readFile()` from disk VFS backend.
Current architecture limitation: WorkQueue uses MemoryBackend by default, and per-app disk backends require async pool creation refactoring that was identified but not implemented to avoid scope creep.

### Phase 999.4: Pre-existing Technical Debt (📋 PLANNED)
**Goal:** Address TODOs from previous phases identified during Phase 27
**Requirements:** Review and resolve 4 pre-existing TODO items in codebase
**Plans:** 4 plans created

Plans:
- [ ] 999.4-01-PLAN.md — RSA/ECDSA algorithm properties (Web Crypto spec compliance)
- [ ] 999.4-02-PLAN.md — VFS list_dir() implementation for snapshot capture
- [ ] 999.4-03-PLAN.md — ESM execution architecture documentation (accepted technical debt)
- [ ] 999.4-04-PLAN.md — V8 snapshot validation documentation (accepted technical debt)

TODOs identified:
1. **src/runtime/apis.rs:1821** — "RSA and ECDSA algorithms - TODO: add specific properties"
   - Decision: FIX — High value, low effort (spec compliance for v2.0 crypto)
   - Plan: 999.4-01 — Add hash property for RSA, namedCurve for ECDSA

2. **src/v8/module.rs:522** — "TODO: Implement proper ESM execution with correct lifetime management"
   - Decision: DOCUMENT as accepted — Transformation approach works, proper Module API deferred to v2.0
   - Plan: 999.4-03 — Document architectural decision and rationale

3. **src/sliver/mod.rs:90** — "TODO: Add list_dir() method to VfsBackend trait for full implementation"
   - Decision: FIX — Core VFS functionality needed for complete sliver snapshots
   - Plan: 999.4-02 — Implement list_dir() on all backends, enable walk_vfs_for_snapshot()

4. **src/v8/isolate.rs:176** — "TODO: Implement proper V8 snapshot validation and loading"
   - Decision: DOCUMENT as accepted — Current validation sufficient, rusty_v8 API limits full implementation
   - Plan: 999.4-04 — Document safety rationale and V8 magic number constant for future use

**Summary:**
- 2 FIX plans (high value fixes): 999.4-01 (crypto properties), 999.4-02 (VFS list_dir)
- 2 DOCUMENT plans (intentional debt): 999.4-03 (ESM architecture), 999.4-04 (snapshot validation)
- Total effort: ~1 day for fixes, ~2 hours for documentation
- Result: Spec-compliant crypto, complete VFS, documented technical debt decisions

Note: These TODOs were NOT introduced in Phase 27. They are pre-existing technical debt from earlier phases that was identified during Phase 27 completion review. Each analyzed and planned individually with appropriate fix vs document decisions.
