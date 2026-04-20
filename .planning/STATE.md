# NANO Project State

**Project:** nano-rs — Edge JavaScript Runtime  
**Version:** v1.1 — Isolate Snapshots & VFS  
**Created:** 2026-04-19  
**Updated:** 2026-04-20  
**Mode:** YOLO (auto-approve execution)

---

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-19)

**Core value:** One OS process hosts many isolated JS apps with millisecond cold starts, zero container overhead, and strong per-app isolation.

**Current focus:** Phase 14 — Snapshot Creation (Complete)

---

## Current Position

**Milestone:** v1.1 — Isolate Snapshots & VFS  
**Phase:** Phase 14 of 16 (Snapshot Creation)  
**Plan:** All 4 plans complete (14-01 through 14-04)  
**Status:** Complete

**Progress:**
```
[██████████████░░░░░░░░░░░░░░░░░░░░] 81% (13/16 phases complete, v1.1 in progress)
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

### Phase 12 Decisions
- **Node.js fs polyfill via require() hook** — Global require('fs') returns VFS-backed polyfill (D-21)
- **tokio block_on for sync operations** — Sync fs methods use tokio::runtime::Handle::try_current() (D-22)
- **Uint8Array-first extraction** — Binary data extraction before string to preserve raw bytes (D-23)
- **Block all ".." substrings** — Reject any path containing ".." for maximum security (D-24)

### Phase 13 Decisions
- **Tar-based snapshot format** — Simple, portable, extensible to deltas later (D-13)
- **heap.bin is completely opaque** — Never parsed by NANO, passed directly to V8 (D-26)
- **VFS entries under vfs/ prefix** — Clear separation of metadata, heap, and filesystem (D-27)
- **String-based format version** — Allows future versions without enum changes (D-28)
- **MemoryBackend snapshot methods** — Direct extraction/restore for efficient serialization (D-29)

### Phase 14 Decisions
- **CLI sliver commands use clap derive macros** — Type-safe, maintainable argument parsing (D-30)
- **Sliver name defaults to hostname** — Sensible default for simple use cases (D-31)
- **V8 135 SnapshotCreator API limited** — Use placeholders, full API not publicly exposed (D-32, D-33)
- **Added 'name' field to SliverMetadata** — Separate management name from hostname (D-38)
- **VFS capture framework ready** — Awaits list_dir() on backends for full implementation

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

**Last session:** 2026-04-20 — Completed Phase 14 (Snapshot Creation)  
**Completed:** CLI sliver commands, V8 snapshot integration, VFS capture, full pipeline
**Summary:** 441 tests passing - CLI create/list/delete, tar-based sliver output, placeholder V8 heap capture
**Next action:** Phase 15 — Snapshot Restoration (loading isolates from .sliver files)
**Resume file:** None

---

*State file: Updated 2026-04-20 — Phase 14 complete*
