# NANO Project State

**Project:** nano-rs — Edge JavaScript Runtime  
**Version:** v1.1 — Isolate Snapshots & VFS  
**Created:** 2026-04-19  
**Updated:** 2026-04-19  
**Mode:** YOLO (auto-approve execution)

---

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-19)

**Core value:** One OS process hosts many isolated JS apps with millisecond cold starts, zero container overhead, and strong per-app isolation.

**Current focus:** Phase 11 — VFS Storage Backends

---

## Current Position

**Milestone:** v1.1 — Isolate Snapshots & VFS  
**Phase:** Phase 11 of 16 (VFS Storage Backends)  
**Plan:** 11-01 (Storage Backends Implementation)  
**Status:** Complete

**Progress:**
```
[███████████░░░░░░░░░░░░░░░░░░░░░░░] 62% (10/16 phases complete, v1.1 in progress)
```

---

## Performance Metrics

**v1.0 Completed:**
- Total phases completed: 9
- Total plans completed: 29
- Requirements satisfied: 42

**v1.1 Target:**
- Cold start from snapshot: ~1-2ms
- VFS read latency: <1ms (in-memory)

---

## Accumulated Context

### Key Decisions from v1.0
- Rust + rusty_v8 over Zig (pre-built V8, type-safe bindings)
- Rust crypto crates over V8 crypto (ring/rsa/p256 safer)
- No npm/import resolution (keeps isolates lightweight)
- WorkerPool per virtual host (resource isolation)
- Context reset (not new isolate per request) for 5ms vs 50-100ms cost

### New v1.1 Decisions
- **Tar-based snapshot format** — Simple, portable, extensible to deltas later (D-13)
- **V8 SnapshotCreator API** — Standard V8 approach for heap serialization (D-14)
- **In-memory VFS with pluggable backends** — Fast default, flexible persistence (D-15)
- **Opaque snapshot blobs** — Version-agnostic, no embedded versioning complexity (D-16)
- **Per-isolate filesystem namespace** — Security isolation between apps (D-17)

### Critical Technical Context
- **EPT SIGSEGV bug:** ✅ RESOLVED — strong v8::Global sentinel implemented and verified
- **V8 SnapshotCreator:** rusty_v8 exposes `v8::SnapshotCreator` for heap serialization
- **VFS design:** Layered approach: API → Core → Backend (memory/disk/S3)

### Phase 11 Decisions
- **S3 backend feature-gated** — rust-s3 requires Rust 1.88, made optional via `vfs-s3` feature (D-18)
- **Atomic file writes** — DiskBackend uses write-to-temp-rename pattern for data integrity (D-19)
- **BackendFactory pattern** — Runtime backend selection via factory (D-20)

---

## Deferred Items from v1.0

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Feature | WebSocket support | v2.0 | v1.0 completion |
| Feature | Compression streams | v2.0 | v1.0 completion |
| Feature | Advanced crypto (RSA, ECDSA) | v2.0 | v1.0 completion |
| Feature | Inter-isolate messaging | v2.0 | v1.0 completion |

---

## Session Continuity

**Last session:** 2026-04-19 — Completed Phase 11 Plan 01 (VFS Storage Backends)  
**Completed:** DiskBackend, S3Backend (feature-gated), BackendFactory, config integration, WorkerPool integration
**Next action:** Phase 12 — VFS JavaScript Bindings (Nano.fs.* API)
**Resume file:** None

---

*State file: Updated at milestone transition*
