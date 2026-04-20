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

## v2.0 Advanced Features 📋

**Milestone Goal:** WebSockets, advanced crypto, compression, inter-isolate messaging

### Phase 17: WebSocket Server
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
