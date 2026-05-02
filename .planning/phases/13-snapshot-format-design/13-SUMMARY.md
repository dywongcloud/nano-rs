---
phase: 13
plan: snapshot-format-design
subsystem: sliver
status: complete
date: 2026-04-20
tags: [snapshots, tar, vfs, serialization]
requires: [Phase 10-12 VFS]
provides: [Phase 14 Snapshot Creation]
tech-stack:
  added: [tar crate, serde]
patterns: [tar archive, opaque blobs, version-agnostic]
key-files:
  created:
    - src/sliver/mod.rs
    - src/sliver/error.rs
    - src/sliver/format.rs
    - src/sliver/metadata.rs
    - src/sliver/packer.rs
    - src/sliver/unpacker.rs
    - src/sliver/FORMAT.md
  modified:
    - src/lib.rs
    - src/vfs/memory.rs
    - Cargo.toml
tests: 27 passing
deviations: None
---

# Phase 13: Snapshot Format Design — Summary

## What Was Built

A tar-based sliver format for JavaScript isolate snapshots. The sliver format captures the complete state of a V8 isolate including heap snapshot and virtual filesystem contents, enabling fast cold starts (~1-2ms) and migration between NANO instances.

### Archive Structure

```
app-v1.sliver (tar archive)
├── meta.json          # Metadata: hostname, created_at, version
├── heap.bin           # V8 heap snapshot (opaque blob)
├── vfs/               # Virtual filesystem contents
│   ├── data/
│   │   └── config.json
│   └── assets/
│       └── logo.png
└── manifest.txt       # Human-readable manifest
```

### Core Components

| Component | Purpose | Lines |
|-----------|---------|-------|
| `error.rs` | SliverError enum with 8 error variants | 75 |
| `format.rs` | Format constants, version checking | 120 |
| `metadata.rs` | SliverMetadata struct with serde | 180 |
| `packer.rs` | Tar archive creation | 260 |
| `unpacker.rs` | Tar archive extraction | 335 |
| `FORMAT.md` | Format specification document | 280 |

### Design Decisions

1. **Tar Format**: Standard tar (inspectable with `tar -tf`)
2. **Opaque Heap**: heap.bin is opaque, never parsed by NANO
3. **Version Agnostic**: Format uses string versions, not enums
4. **Portable**: No host-specific paths or IDs
5. **Evolvable**: Extension points for deltas and compression

### VFS Integration

- `MemoryBackend.snapshot_entries()` - Extract all files for serialization
- `MemoryBackend.restore_from_snapshot()` - Populate from snapshot data
- Binary-safe content handling (preserves null bytes, emoji, etc.)
- Directory structure preserved in tar entries

## Test Results

### Sliver Module Tests: 27 passing
- Format version validation ✓
- Metadata JSON roundtrip ✓
- Pack/unpack with VFS entries ✓
- Binary content preservation ✓
- Error handling (missing files, invalid version) ✓
- Tar structure validation ✓

### VFS Snapshot Tests: 4 passing
- Snapshot entries extraction ✓
- Restore from snapshot ✓
- Roundtrip preservation ✓
- Counter synchronization ✓

## Requirements Satisfied

| Requirement | Status | Notes |
|-------------|--------|-------|
| SNAP-05 | ✅ | Tar-based format with meta.json, heap.bin, vfs/ structure |
| SNAP-06 | ✅ | heap.bin is opaque blob, version-agnostic |
| VFS-08 | ✅ | VFS state serialized as tar entries under vfs/ prefix |

## Commits

```
7e34deba feat(13-01): Implement sliver format core module
- 7 files, 1523 lines added
- SliverMetadata, SliverPacker, SliverUnpacker
- FORMAT.md specification
- 27 unit tests

e03bc105 feat(13-02): Add VFS snapshot serialization support  
- MemoryBackend snapshot/restore methods
- 4 unit tests

49cffb11 chore(13): Add tar crate dependency and integrate sliver module
- tar = "0.4" dependency
- Module exports
```

## Integration Points

### For Phase 14 (Snapshot Creation)
- Use `SliverPacker` with V8 SnapshotCreator output
- Call `MemoryBackend.snapshot_entries()` to capture VFS
- Write archive to `.sliver` file

### For Phase 15 (Snapshot Restoration)
- Use `SliverUnpacker` to read archive
- Pass heap blob to V8 isolate creation
- Call `MemoryBackend.restore_from_snapshot()` for VFS

## Format Specification Highlights

**Key files required:**
- `meta.json` - JSON metadata
- `heap.bin` - Opaque V8 heap blob

**VFS storage:**
- All files under `vfs/` prefix
- Paths preserved (forward slashes)
- Binary content stored as-is

**Future extensions:**
- Delta snapshots (additive format)
- Compression layer (gzip/brotli)
- Checksum verification

## Self-Check

✅ All 27 sliver tests passing
✅ All 4 VFS snapshot tests passing  
✅ All 413 total library tests passing
✅ No compiler errors
✅ FORMAT.md documents complete specification
✅ Requirements SNAP-05, SNAP-06, VFS-08 satisfied

## Next Steps

Phase 14 (Snapshot Creation) can now proceed:
1. CLI `snapshot create <hostname>` command
2. V8 SnapshotCreator API integration
3. File I/O for `.sliver` output

---
*Completed: 2026-04-20*
