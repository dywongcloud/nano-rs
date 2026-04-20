# NANO Edge Runtime — Roadmap

**Current:** v1.1 IN PROGRESS 🚧  
**Date:** 2026-04-19

---

## Milestones

- ✅ **v1.0 Foundation** — Phases 1-9 (shipped 2026-04-19)
- 🚧 **v1.1 Snapshots & VFS** — Phases 10-16 (in progress)
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

## v1.1 Snapshots & VFS 🚧

**Milestone Goal:** Container-image semantics for JavaScript isolates with ~1-2ms cold starts

### Phase 10: VFS Foundation
**Goal:** Core VFS module with in-memory storage  
**Depends on:** Phase 9 (v1.0 complete)  
**Requirements:** VFS-01, VFS-02
**Success Criteria** (what must be TRUE):
  1. VFS module exists at `src/vfs/` with clean API
  2. In-memory file storage works per-isolate
  3. Files are isolated between apps (no cross-access)
  4. Basic file operations (read, write, exists) work in Rust
**Plans**: TBD
**UI hint**: no

### Phase 11: VFS Storage Backends
**Goal:** Pluggable storage backends (disk, S3)  
**Depends on:** Phase 10  
**Requirements:** VFS-06, VFS-07
**Success Criteria** (what must be TRUE):
  1. Disk backend persists VFS to local filesystem
  2. S3-compatible backend stores files in object storage
  3. Backend selection is configurable per-app
  4. Storage backend abstraction is extensible
**Plans**: TBD
**UI hint**: no

### Phase 12: VFS JavaScript Bindings
**Goal:** `Nano.fs.*` API exposed to JavaScript  
**Depends on:** Phase 10  
**Requirements:** VFS-03, VFS-04, VFS-05
**Success Criteria** (what must be TRUE):
  1. JS can call `Nano.fs.readFile('/path')` and get contents
  2. JS can call `Nano.fs.writeFile('/path', data)` to store data
  3. JS can call `Nano.fs.exists('/path')` to check files
  4. Paths are resolved within isolate's namespace
  5. Errors are properly thrown as JS exceptions
**Plans**: TBD
**UI hint**: no

### Phase 13: Snapshot Format Design
**Goal:** Define tar-based snapshot structure  
**Depends on:** Phase 10  
**Requirements:** SNAP-05, SNAP-06, VFS-08
**Success Criteria** (what must be TRUE):
  1. Snapshot format specification documented
  2. Tar structure includes: metadata, V8 heap, VFS state
  3. Format is version-agnostic (opaque blob)
  4. Design allows future delta/differential extension
  5. Format is portable between systems
**Plans**: TBD
**UI hint**: no

### Phase 14: Snapshot Creation
**Goal:** CLI `snapshot create` with V8 SnapshotCreator  
**Depends on:** Phase 13  
**Requirements:** SNAP-01, SNAP-02, SNAP-07, CLI-01, CLI-02
**Success Criteria** (what must be TRUE):
  1. `nano-rs snapshot create <hostname>` produces tar file
  2. Snapshot captures V8 isolate heap via SnapshotCreator API
  3. Snapshot includes VFS state
  4. Multiple snapshots can be listed and managed
  5. Old snapshots can be deleted
**Plans**: TBD
**UI hint**: no

### Phase 15: Snapshot Restoration
**Goal:** Run isolates from snapshot with ~1-2ms cold start  
**Depends on:** Phase 14  
**Requirements:** SNAP-03, SNAP-04, PERF-01, MIGRATE-01
**Success Criteria** (what must be TRUE):
  1. `nano-rs run --snapshot app-v1.tar` restores isolate
  2. Restored isolate has preserved heap state
  3. Restored isolate has preserved VFS state
  4. Cold start from snapshot is ~1-2ms
  5. Snapshot can be moved to another NANO instance and work
**Plans**: TBD
**UI hint**: no

### Phase 16: CLI Integration & Polish
**Goal:** Complete CLI commands and integration tests  
**Depends on:** Phase 15  
**Requirements:** (integration of all v1.1 requirements)
**Success Criteria** (what must be TRUE):
  1. All CLI commands work end-to-end
  2. Snapshot roundtrip (create → move → restore) verified
  3. VFS JS API works with all storage backends
  4. Performance targets verified with benchmarks
  5. Documentation complete
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
| 10. VFS Foundation | v1.1 | 0/TBD | Not started | - |
| 11. VFS Backends | v1.1 | 0/TBD | Not started | - |
| 12. VFS JS Bindings | v1.1 | 0/TBD | Not started | - |
| 13. Snapshot Format | v1.1 | 0/TBD | Not started | - |
| 14. Snapshot Create | v1.1 | 0/TBD | Not started | - |
| 15. Snapshot Restore | v1.1 | 0/TBD | Not started | - |
| 16. CLI Integration | v1.1 | 0/TBD | Not started | - |

---

## What's Next

To start Phase 10 planning: `/gsd-plan-phase 10`

---

*Roadmap updated: 2026-04-19 — v1.1 milestone initialized*
