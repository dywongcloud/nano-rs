# Requirements: NANO v1.1 — Isolate Snapshots & VFS

**Defined:** 2026-04-19  
**Core Value:** One OS process hosts many isolated JS apps with millisecond cold starts  
**Milestone Goal:** Container-image semantics for JavaScript isolates

---

## v1.1 Requirements

### Snapshot Core

- [ ] **SNAP-01:** CLI `nano-rs snapshot create <hostname>` produces tar archive
- [ ] **SNAP-02:** Snapshot tar contains V8 isolate heap + VFS state + metadata
- [ ] **SNAP-03:** CLI `nano-rs run --snapshot app-v1.tar` restores isolate from snapshot
- [ ] **SNAP-04:** Restored isolate resumes execution with preserved state
- [ ] **SNAP-05:** Snapshot format is tar-based with simple structure
- [ ] **SNAP-06:** Snapshots are opaque blobs (version-agnostic, no embedded versioning)
- [ ] **SNAP-07:** Multiple snapshots can coexist (versioned by filename)

### VFS Core

- [ ] **VFS-01:** VFS module at `src/vfs/` with in-memory storage
- [ ] **VFS-02:** Per-isolate filesystem namespace (no cross-app access)
- [ ] **VFS-03:** JS API `Nano.fs.readFile(path)` reads file contents
- [ ] **VFS-04:** JS API `Nano.fs.writeFile(path, data)` writes file contents
- [ ] **VFS-05:** JS API `Nano.fs.exists(path)` checks file existence
- [ ] **VFS-06:** Optional disk backing for VFS persistence
- [ ] **VFS-07:** Optional S3-compatible object storage backend
- [ ] **VFS-08:** VFS state included in snapshot serialization

### Performance & Migration

- [ ] **PERF-01:** Cold start from snapshot achieves ~1-2ms latency
- [ ] **MIGRATE-01:** Snapshot can be transferred between NANO instances

### CLI & Management

- [ ] **CLI-01:** `nano-rs snapshot list` shows available snapshots
- [ ] **CLI-02:** `nano-rs snapshot delete <name>` removes snapshot

---

## v2.0 Requirements (Deferred)

### WebSocket Support

- **WS-01:** WebSocket server upgrade handling (RFC 6455)
- **WS-02:** JS WebSocket API for real-time communication
- **WS-03:** Per-isolate WebSocket connection limits

### Advanced Crypto

- **CRYPT-05:** RSA key generation and import/export
- **CRYPT-06:** ECDSA sign/verify operations
- **CRYPT-07:** RSA-OAEP encrypt/decrypt

### Compression

- **COMP-01:** CompressionStream with deflate
- **COMP-02:** DecompressionStream with inflate

### Inter-Isolate Messaging

- **MSG-01:** PostMessage API between isolates
- **MSG-02:** Broadcast channels for app groups

---

## Out of Scope

| Feature | Reason |
|---------|--------|
| Delta/differential snapshots | Complex, defer to v1.2+ |
| Live migration (running isolates) | Requires freeze/thaw, significant complexity |
| npm package resolution | Apps remain single-file, bundling is user responsibility |
| TypeScript/JSX transpilation | User must bundle beforehand |
| Native module support | Only pure JS/WinterCG APIs |
| Subprocess spawning from JS | Security/scope constraint |
| Built-in horizontal clustering | External load balancer sufficient |
| VFS directory operations (mkdir, readdir) | Defer to v1.2, focus on file I/O first |
| VFS file watching | Complex, defer to v2.0 |

---

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| SNAP-01 | Phase 14 | Pending |
| SNAP-02 | Phase 14 | Pending |
| SNAP-03 | Phase 15 | Pending |
| SNAP-04 | Phase 15 | Pending |
| SNAP-05 | Phase 13 | Pending |
| SNAP-06 | Phase 13 | Pending |
| SNAP-07 | Phase 14 | Pending |
| VFS-01 | Phase 10 | Pending |
| VFS-02 | Phase 10 | Pending |
| VFS-03 | Phase 12 | Pending |
| VFS-04 | Phase 12 | Pending |
| VFS-05 | Phase 12 | Pending |
| VFS-06 | Phase 11 | Pending |
| VFS-07 | Phase 11 | Pending |
| VFS-08 | Phase 13 | Pending |
| PERF-01 | Phase 15 | Pending |
| MIGRATE-01 | Phase 15 | Pending |
| CLI-01 | Phase 16 | Pending |
| CLI-02 | Phase 16 | Pending |

**Coverage:**
- v1.1 requirements: 20 total
- Mapped to phases: 20
- Unmapped: 0 ✓

---

*Requirements defined: 2026-04-19*  
*Last updated: 2026-04-19 — v1.1 milestone initialization*
