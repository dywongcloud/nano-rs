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

### Phase 17: Request/Response Fixes
**Goal:** Pass full WinterCG Request (URL, headers, body) to JS; add Promise support  
**Depends on:** v1.1 complete  
**Requirements:** REQ-17-01, REQ-17-02, REQ-17-03  
**Success Criteria**:
  1. Handler receives `{method, url, headers, body}` not just method
  2. Async handlers with `await` resolve correctly
  3. Request body readable as text/JSON
**Plans:** 
  - [ ] 17-01-PLAN.md — Request/Response fixes implementation
**UI hint:** no

### Phase 18: ESM Module System
**Goal:** Replace Script::compile with V8 Module API for ESM support  
**Depends on:** Phase 17  
**Requirements:** REQ-18-01, REQ-18-02  
**Success Criteria**:
  1. `export default { fetch }` compiles and runs
  2. Relative imports work within sliver VFS
  3. Hono.js and Next.js ESM bundles execute
**Plans:** TBD  
**UI hint:** no

### Phase 19: Config Mode Implementation
**Goal:** Actually implement `--config` workflow + port/host config  
**Depends on:** Phase 18  
**Requirements:** REQ-19-01, REQ-19-02  
**Success Criteria**:
  1. `nano-rs run --config config.json` loads and serves apps
  2. Port and host from config applied to server bind
  3. Multiple apps in config served with virtual host routing
**Plans:** TBD  
**UI hint:** no

### Phase 20: Sliver VFS Integration
**Goal:** Execute JS from packed sliver VFS, not OS filesystem  
**Depends on:** Phase 19  
**Requirements:** REQ-20-01, REQ-20-02  
**Success Criteria**:
  1. Sliver runs from any directory (portable)
  2. JS entrypoint read from vfs/ in sliver, not CWD
  3. No source files required to run sliver
**Plans:** TBD  
**UI hint:** no

### Phase 21: Documentation & Architecture
**Goal:** Fix cold start claims, document compatibility accurately  
**Depends on:** Phase 20  
**Requirements:** REQ-21-01, REQ-21-02, REQ-21-03, REQ-21-04  
**Success Criteria**:
  1. Cold start metrics distinguish process boot (~60ms) from request latency (~267µs)
  2. Node.js compatibility matrix published (~5% accurate)
  3. Per-worker state isolation documented with guidance
  4. Troubleshooting guide covers common issues
  5. All docs reviewed for accuracy
**Plans:**
  - [ ] 21-01-PLAN.md — Fix cold start metrics and performance documentation
  - [ ] 21-02-PLAN.md — Create Node.js compatibility matrix and migration guides
  - [ ] 21-03-PLAN.md — Document state architecture and troubleshooting guide
  - [ ] 21-04-PLAN.md — Finalize documentation index and cross-references
**UI hint:** no

**Full details:** [REMEDIATION-v1.2.md](./REMEDIATION-v1.2.md)

---

## v2.0 Advanced Features 📋

**Milestone Goal:** WebSockets, advanced crypto, compression, inter-isolate messaging

### Phase 22: WebSocket Server
**Goal:** RFC 6455 WebSocket upgrade handling  
**Depends on:** v1.2 complete  
**Requirements:** WS-01, WS-02, WS-03  
**Success Criteria** (what must be TRUE):
  1. HTTP server supports WebSocket upgrade
  2. JS can handle WebSocket connections
  3. Per-isolate connection limits enforced
**Plans**: TBD  
**UI hint**: no

### Phase 23: Advanced Crypto
**Goal:** RSA and ECDSA operations  
**Depends on:** Phase 22  
**Requirements:** CRYPT-05, CRYPT-06, CRYPT-07  
**Success Criteria** (what must be TRUE):
  1. RSA key generation and import/export
  2. ECDSA sign/verify operations
  3. RSA-OAEP encrypt/decrypt
**Plans**: TBD  
**UI hint**: no

### Phase 24: Compression Streams
**Goal:** CompressionStream and DecompressionStream  
**Depends on:** Phase 23  
**Requirements:** COMP-01, COMP-02  
**Success Criteria** (what must be TRUE):
  1. CompressionStream with deflate
  2. DecompressionStream with inflate
  3. Works with fetch() Response/Request bodies
**Plans**: TBD  
**UI hint**: no

### Phase 25: Inter-Isolate Messaging
**Goal:** PostMessage API between isolates  
**Depends on:** Phase 24  
**Requirements:** MSG-01, MSG-02  
**Success Criteria** (what must be TRUE):
  1. JS can postMessage to other isolates
  2. Broadcast channels for app groups
  3. Message serialization preserves types
**Plans**: TBD
**Goal:** RFC 6455 WebSocket upgrade handling  
**Depends on:** v1.1 complete  
**Requirements:** WS-01, WS-02, WS-03  
**Success Criteria** (what must be TRUE):
  1. HTTP server supports WebSocket upgrade
  2. JS can handle WebSocket connections
  3. Per-isolate connection limits enforced
**Plans**: TBD
**UI hint**: no

### Phase 18: Advanced Crypto
**Goal:** RSA and ECDSA operations  
**Depends on:** Phase 17  
**Requirements:** CRYPT-05, CRYPT-06, CRYPT-07  
**Success Criteria** (what must be TRUE):
  1. RSA key generation and import/export
  2. ECDSA sign/verify operations
  3. RSA-OAEP encrypt/decrypt
**Plans**: TBD
**UI hint**: no

### Phase 19: Compression Streams
**Goal:** CompressionStream and DecompressionStream  
**Depends on:** Phase 18  
**Requirements:** COMP-01, COMP-02  
**Success Criteria** (what must be TRUE):
  1. CompressionStream with deflate
  2. DecompressionStream with inflate
  3. Works with fetch() Response/Request bodies
**Plans**: TBD
**UI hint**: no

### Phase 20: Inter-Isolate Messaging
**Goal:** PostMessage API between isolates  
**Depends on:** Phase 19  
**Requirements:** MSG-01, MSG-02  
**Success Criteria** (what must be TRUE):
  1. JS can postMessage to other isolates
  2. Broadcast channels for app groups
  3. Message serialization preserves types
**Plans**: TBD
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
| 17. WebSocket Server | v2.0 | 0/TBD | Not started | - |
| 18. Advanced Crypto | v2.0 | 0/TBD | Not started | - |
| 19. Compression | v2.0 | 0/TBD | Not started | - |
| 20. Inter-Isolate | v2.0 | 0/TBD | Not started | - |

---

## What's Next

To start Phase 17 planning: `/gsd-plan-phase 17`

Or to start a new milestone: `/gsd-new-milestone`

---

*Roadmap updated: 2026-04-20 — v1.1 milestone shipped*
