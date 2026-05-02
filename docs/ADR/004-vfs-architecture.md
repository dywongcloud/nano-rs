# ADR-004: Virtual File System Abstraction

**Status:** Accepted  
**Date:** 2026-04-20  
**Deciders:** Core Team  
**Technical Story:** Need filesystem API for JS without direct OS filesystem access

---

## Context and Problem Statement

JavaScript applications need file I/O for:
1. **Reading configuration** — JSON config files, certificates
2. **Writing logs/state** — Application logs, user data
3. **Static file serving** — HTML, CSS, images
4. **Temporary storage** — Cache, uploads

However, direct filesystem access in a multi-tenant runtime is dangerous:
- **Path traversal attacks** — `../../../etc/passwd` access
- **Cross-tenant data access** — App A reading App B's files
- **Host filesystem pollution** — Apps writing to system directories
- **No isolation** — One app's files visible to all

We need a filesystem abstraction that provides:
1. **Security isolation** between tenants
2. **Snapshot support** (for slivers)
3. **Pluggable storage backends** (memory, disk, cloud)
4. **Per-isolate namespaces** (each app sees only its files)

---

## Decision Drivers

* **Security** — Prevent filesystem escape
* **Isolation** — Per-tenant filesystem namespaces
* **Portability** — Works with sliver snapshots
* **Flexibility** — Multiple storage backends
* **Standards compliance** — WinterCG-compatible
* **Performance** — Fast default, scalable options

---

## Considered Options

### Option 1: Direct OS Filesystem

Allow direct file access with sandboxing.

### Option 2: VFS Abstraction

Per-isolate namespaces with pluggable backends.

### Option 3: chroot per Isolate

Linux chroot jail per isolate.

### Option 4: FUSE Filesystem

User-space filesystem in external process.

---

## Decision Outcome

**Chosen option: "VFS Abstraction"**

VFS (`src/vfs/`) provides:
- **Per-isolate namespaces** — `{hostname}::/path/to/file`
- **Path validation** — Rejects `..`, null bytes, absolute paths
- **Pluggable backends** — Memory, Disk, S3
- **Snapshot support** — Can serialize entire VFS state
- **Consistent API** — Regardless of backend

**Rationale:**
- Strong security isolation (path validation + namespaces)
- Sliver snapshots capture full filesystem state
- Multiple storage options for different deployments
- Portable across OS (not Linux-specific like chroot)

---

## Implementation Details

### Architecture

```
┌─────────────────────────────────────────────┐
│           JavaScript Layer                  │
│  Nano.fs.readFile(), writeFile(), etc.     │
└──────────────┬──────────────────────────────┘
               │
┌──────────────▼──────────────────────────────┐
│           VFS Core Layer                    │
│  - Path validation                          │
│  - Namespace routing                        │
│  - File metadata                            │
└──────┬──────────────┬──────────────┬───────┘
       │              │              │
┌──────▼─────┐ ┌──────▼─────┐ ┌──────▼─────┐
│  Memory    │ │    Disk    │ │    S3      │
│  Backend   │ │  Backend   │ │  Backend   │
│  (default) │ │(persistent)│ │  (cloud)   │
└────────────┘ └────────────┘ └────────────┘
```

### Namespace Format

```rust
// Per-isolate namespace: {hostname}::{path}
let file_id = FileId::new("api.example.com::/data/config.json");
```

**Key properties:**
- Hostname isolates tenants
- Path is relative within tenant
- Cannot escape namespace (validated)

### Path Validation

Security rules:
```rust
fn validate_path(path: &str) -> Result<()> {
    // Reject path traversal
    if path.contains("..") {
        return Err(Error::InvalidPath);
    }
    
    // Reject null bytes
    if path.contains('\0') {
        return Err(Error::InvalidPath);
    }
    
    // Reject absolute paths
    if path.starts_with('/') {
        return Err(Error::InvalidPath);
    }
    
    Ok(())
}
```

### Backend Strategy

| Backend | Use Case | Persistence | Latency |
|---------|----------|-------------|---------|
| Memory | Fast, ephemeral, sliver snapshots | No | <1µs |
| Disk | Persistence across restarts | Yes | ~1ms |
| S3 | Cloud-native, scalable | Yes | ~50ms |

**Selection criteria:**
- **Development:** Memory (fast, no cleanup)
- **Production (single instance):** Disk (persistent)
- **Production (multi-instance):** S3 (shared)

---

## Positive Consequences

* **Strong security isolation** — Path validation + namespaces
* **Sliver snapshots work** — VFS state captured completely
* **Multiple storage options** — Memory/Disk/S3 for different deployments
* **Consistent API** — Same Nano.fs.* API regardless of backend
* **Portable** — Works on any OS (not Linux-specific)
* **Testable** — Easy to mock for unit tests

---

## Negative Consequences

* **More complex than direct filesystem** — Additional abstraction layer
* **Memory backend uses RAM** — Mitigated by quotas
* **Some Node.js fs APIs not supported** — Async only, limited methods
* **Additional dependency** — Tar library for snapshots
* **Performance overhead** — Namespace routing adds ~µs per call

---

## Backend Details

### Memory Backend (Default)

```rust
pub struct MemoryBackend {
    files: HashMap<FileId, Vec<u8>>,
    metadata: HashMap<FileId, Metadata>,
}
```

- Fast (<1µs per operation)
- Ephemeral (lost on restart)
- Best for sliver-based deployments

### Disk Backend

```rust
pub struct DiskBackend {
    base_path: PathBuf,
}
```

- Persistent across restarts
- File-based storage
- Good for development, single-instance production

### S3 Backend

```rust
pub struct S3Backend {
    bucket: String,
    prefix: String,
    client: S3Client,
}
```

- Cloud-native, multi-instance
- Eventual consistency
- Feature-gated (rust-s3 requires Rust 1.88)

---

## Alternatives Rejected

### Option 1: Direct OS Filesystem — Rejected

**Why:** Cannot provide per-tenant isolation. Path traversal attacks possible. Cross-tenant data access.

### Option 3: chroot per Isolate — Rejected

**Why:** Linux-specific (not portable). Complex setup. Overhead of chroot syscall. Still allows intra-tenant attacks.

### Option 4: FUSE Filesystem — Rejected

**Why:** External process overhead. Complex deployment. Not suitable for edge runtime simplicity goal.

---

## Security Model

### Trust Boundaries

```
┌─────────────────────────────────────┐
│  Untrusted JavaScript               │
│  Nano.fs.readFile("../../etc/passwd")│
└────────────────┬────────────────────┘
                 │ Path validation
                 ▼ REJECTED
┌─────────────────────────────────────┐
│  VFS Core                           │
│  Enforces per-isolate namespace     │
└────────────────┬────────────────────┘
                 │ Backend routing
                 ▼
┌─────────────────────────────────────┐
│  Backend (Memory/Disk/S3)            │
│  Physical storage                    │
└─────────────────────────────────────┘
```

### Attack Prevention

| Attack | Prevention |
|--------|------------|
| Path traversal (`../../`) | Rejected in validation |
| Null byte injection | Rejected in validation |
| Absolute paths (`/etc/`) | Rejected in validation |
| Cross-tenant access | Hostname namespace |
| DoS (large files) | Size limits enforced |
| DoS (many files) | Count limits enforced |

---

## Related Decisions

* [ADR-006: Sliver Format](006-sliver-format.md) — VFS state captured in sliver
* [ADR-007: ESM Strategy](007-esm-strategy.md) — VFS stores JS files
* `docs/VFS.md` — User-facing VFS documentation
* `src/vfs/` — Implementation

---

## Code References

- `src/vfs/mod.rs` — VFS trait and core
- `src/vfs/memory.rs` — Memory backend
- `src/vfs/disk.rs` — Disk backend
- `src/vfs/s3.rs` — S3 backend
- `src/runtime/apis.rs` — Nano.fs JavaScript bindings

---

## Migration from Node.js

**Node.js:**
```javascript
const fs = require('fs');
const data = fs.readFileSync('./config.json', 'utf8');
```

**NANO:**
```javascript
const data = await Nano.fs.readFile('/config.json', { encoding: 'utf-8' });
```

**Key differences:**
1. Async only (no sync methods in production)
2. Paths relative to VFS root (not OS filesystem)
3. Per-isolate isolation (no cross-app access)

---

*Last updated: 2026-04-20*
