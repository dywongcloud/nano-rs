---
phase: 10-vfs-foundation
discussed: 2026-04-19
participants: gleicon
---

## Decisions Locked

### D-10-01: VFS as Ephemeral localStorage (Per-Isolate, Not Shared)

**Decision:** Each isolate gets its own isolated VFS. No shared backend between workers.

**Rationale:**
- Mental model matches browser localStorage — familiar to JS developers
- Simpler implementation — no coordination needed between workers
- Aligns with container-like semantics (ephemeral by default)
- Shared backend can be added later as a plugin (v1.2+)

**Behavior:**
- Workers for same app have **separate** filesystems
- Files created at runtime are **ephemeral** (lost on isolate termination)
- Only files **bundled in snapshot** persist (snapshot = seed data)
- Each isolate has its own namespace (hostname-derived)

### D-10-02: Namespace Approach

**Decision:** Hostname-based namespaces with sanitization.

**Format:** `{sanitized_hostname}::/path/to/file`

**Example:** `api_example_com::/data/config.json`

**Rationale:** Human-readable, matches app identity, simple to implement.

### D-10-03: Resource Limits (Per-Isolate)

**Decision:** Hardcoded limits per isolate (not per-app, not global).

**Limits:**
- Max file size: 10MB
- Max total storage: 100MB
- Max files: 1000
- Max path length: 4096 bytes

**Rationale:** Per-isolate isolation means each isolate gets its own budget.

### D-10-04: Security Layers

**Decision:** Keep all 4 defense-in-depth layers.

**Layers:**
1. PathValidator — rejects `..` before normalization
2. VfsPath — path normalization, basic validation
3. IsolateVfs — namespace prefixing
4. Backend — storage-level isolation

**Rationale:** Defense-in-depth is appropriate for filesystem security.

### D-10-05: Integration with WorkerPool/Isolate

**Decision:** VFS lives in NanoIsolate, survives context reset.

**Architecture:**
- Each `NanoIsolate` owns a `VfsNamespace`
- `IsolateVfs` wraps the namespace + backend
- Context reset preserves VFS (only JS context reset, not VFS reset)
- VFS backend is created with isolate, destroyed with isolate

### D-10-06: VfsBackend Trait

**Decision:** Use `async-trait` for v1.1 (stable, object-safe).

**Methods for v1.1:**
- `read(path) -> Result<Vec<u8>>`
- `write(path, data) -> Result<()>`
- `exists(path) -> Result<bool>`
- `delete(path) -> Result<()>`
- `list_dir(path) -> Result<Vec<String>>` (optional for v1.1)

**Rationale:** Async trait stable, object-safe for dyn dispatch, extensible for future backends.

### D-10-07: Error Codes (Node.js Compatibility)

**Decision:** Match Node.js fs error codes.

**Codes:**
- `ENOENT` — file not found
- `EACCES` — permission denied
- `EINVAL` — invalid argument
- `EQUOTA` — quota exceeded (NANO-specific)

### D-10-08: In-Memory Backend Default

**Decision:** In-memory backend is the only backend for Phase 10.

**Phase 11:** Will add disk and S3 backends.

**Rationale:** Focus on core VFS first, then add persistence options.

---

## Deferred Ideas (v1.2+)

- Shared backend per app (distributed filesystem)
- Directory operations (mkdir, rmdir)
- File watching (inotify-like)
- Symbolic links
- File permissions/ACLs

---

## Technical Approach

### Data Structures

```rust
// VfsBackend trait — async, object-safe
trait VfsBackend {
    async fn read(&self, path: &VfsPath) -> Result<Vec<u8>>;
    async fn write(&self, path: &VfsPath, data: &[u8]) -> Result<()>;
    async fn exists(&self, path: &VfsPath) -> Result<bool>;
    async fn delete(&self, path: &VfsPath) -> Result<()>;
}

// InMemoryBackend — HashMap<String, Vec<u8>> + total_size tracking
struct InMemoryBackend {
    files: HashMap<String, Vec<u8>>,
    total_size: usize,
    file_count: usize,
}

// VfsNamespace — identifies isolate's filesystem
struct VfsNamespace {
    hostname: String, // sanitized
    isolate_id: Uuid, // for uniqueness
}

// IsolateVfs — per-isolate VFS interface
struct IsolateVfs {
    namespace: VfsNamespace,
    backend: Arc<dyn VfsBackend>,
    limits: VfsLimits,
}
```

### Security Flow

```
JS: Nano.fs.readFile('/data/config.json')
  ↓
1. PathValidator: rejects if contains '..'
  ↓
2. VfsPath::new('/data/config.json'): normalize
  ↓
3. IsolateVfs: prefix with namespace
     → 'api_example_com__{uuid}::/data/config.json'
  ↓
4. Backend: lookup in HashMap
```

### Integration Points

1. **NanoIsolate** — add `vfs: IsolateVfs` field
2. **WorkerPool** — create VFS when creating isolate
3. **Context reset** — preserve VFS (don't reset)
4. **Isolate drop** — drop VFS (ephemeral)

---

## Success Criteria

- [ ] VFS module compiles and passes unit tests
- [ ] In-memory backend stores/retrieves files
- [ ] Path validation rejects traversal attempts
- [ ] Resource limits enforced (file size, count, total)
- [ ] Per-isolate isolation verified (cross-isolate access blocked)
- [ ] Error codes match Node.js semantics
- [ ] VFS survives context reset
- [ ] VFS dropped with isolate (ephemeral)
