# SLIVER — Edge Snapshots for NANO

> *"A sliver of state, frozen in time, ready to materialize anywhere on the edge."*

**SLIVER** is NANO's container-image system for JavaScript isolates. Like a sliver of time from sci-fi lore (think technocore from *Rise of Endymion*), a sliver encapsulates an entire isolate—its heap, its VFS, its state—into a portable, opaque blob that can materialize instantly on any NANO instance.

---

## What is a Sliver?

A **sliver** is a snapshot of a running isolate:
- **V8 heap state** — frozen JavaScript execution context
- **VFS contents** — bundled files from directory or captured from running app
- **Metadata** — app identity, creation timestamp, dependencies

**Format:** Simple tar archive (opaque, version-agnostic, evolvable)

**Use cases:**
- **Fast cold starts** — ~1-2ms from sliver vs ~5ms context reset
- **Migration** — move running apps between NANO instances
- **Checkpoint/restore** — save state, resume later
- **Distribution** — package apps as slivers, deploy anywhere
- **Load balancing** — replicate slivers across edge nodes

---

## CLI Semantics

```bash
# Create from running app
nano-rs sliver create api.example.com --output api-v1.sliver

# Create from directory (pre-bundled app)
nano-rs sliver pack ./my-app --output my-app-v1.sliver

# Inspect sliver contents
nano-rs sliver inspect api-v1.sliver

# Unpack sliver to directory
nano-rs sliver unpack api-v1.sliver --output ./unpacked

# List local slivers
nano-rs sliver list

# Remove sliver
nano-rs sliver remove api-v1.sliver

# Run directly from sliver (bypass config)
nano-rs run --sliver api-v1.sliver

# Or via config
nano-rs run --config apps.json
# apps.json:
# {
#   "apps": [{
#     "hostname": "api.example.com",
#     "sliver": "./api-v1.sliver"
#   }]
# }
```

---

## Sliver vs Config

**Slivers are optional.** You don't need them to run apps on NANO.

| Method | Use When | Cold Start |
|--------|----------|------------|
| **Config** (traditional) | Development, dynamic apps | ~5ms |
| **Sliver** (snapshot) | Production, fast scale, migration | ~1-2ms |

**Config approach:**
```json
{
  "apps": [{
    "hostname": "api.example.com",
    "entrypoint": "./apps/api.js",
    "workers": 4
  }]
}
```

**Sliver approach:**
```json
{
  "apps": [{
    "hostname": "api.example.com",
    "sliver": "./api-v1.sliver",
    "workers": 4
  }]
}
```

**Hybrid:** Use config during development, create sliver for production deployment.

---

## Sliver Architecture

### Creation Paths

```
Path A: From Running App
========================
Running Isolate
    ↓
V8 SnapshotCreator → heap blob
    ↓
VFS state → tar entries
    ↓
Metadata (hostname, created_at, version)
    ↓
Pack into sliver.tar

Path B: From Directory
========================
Directory (./my-app/)
    ↓
VFS initialization (files → VFS)
    ↓
Optional: Run init script
    ↓
V8 SnapshotCreator → heap blob (if JS executed)
    ↓
Pack into sliver.tar
```

### Sliver Format (tar)

```
api-v1.sliver (tar archive)
├── meta.json          # Metadata: hostname, created_at, version
├── heap.bin           # V8 heap snapshot (opaque blob)
├── vfs/               # Virtual filesystem contents
│   ├── data/
│   │   └── config.json
│   └── assets/
│       └── logo.png
└── manifest.txt       # Human-readable manifest
```

**Design principles:**
- **Opaque:** Don't parse heap.bin, treat as blob
- **Portable:** No host-specific paths or IDs
- **Evolvable:** Format allows future extensions (deltas, compression)
- **Simple:** Just a tar file, can inspect with `tar -tf`

---

## The Vision

### Phase 1: Foundation (Current)
- ✅ VFS — per-isolate filesystem
- ✅ Basic sliver format — tar with heap + VFS
- CLI: `sliver create`, `run --sliver`

### Phase 2: Distribution (v1.2)
- Sliver registry (S3-compatible storage)
- `nano-rs sliver push api-v1.sliver s3://my-registry`
- `nano-rs sliver pull s3://my-registry/api-v1.sliver`

### Phase 3: Orchestration (v1.3)
- Sliver replication across edge nodes
- Automatic load balancing
- Checkpoint/restore for long-running apps

### Phase 4: Advanced (Future)
- **Delta slivers** — only changed files
- **Layered slivers** — base image + app layer (Docker-like)
- **Encrypted slivers** — at-rest encryption
- **Signed slivers** — provenance verification

---

## Why "Sliver"?

> *"The technocore had slivered itself across the galaxy, fragments of intelligence waiting to coalesce."*
> — *Rise of Endymion*, Dan Simmons

A **sliver** is:
- Small but complete — contains everything needed to run
- Portable — can exist anywhere, materialize on demand
- Ephemeral or persistent — use and discard, or keep forever
- Self-contained — no external dependencies

Just as the technocore distributed itself as slivers across worlds, NANO distributes JavaScript apps as slivers across the edge.

---

## Quick Start

```bash
# 1. Have a running app
nano-rs run --config dev.json

# 2. Create sliver from it
nano-rs sliver create api.example.com --output api-prod-v1.sliver

# 3. Run from sliver (faster cold start)
nano-rs run --sliver api-prod-v1.sliver

# 4. Or use in production config
cat > prod.json << 'EOF'
{
  "apps": [{
    "hostname": "api.example.com",
    "sliver": "./api-prod-v1.sliver",
    "workers": 8
  }]
}
EOF
nano-rs run --config prod.json
```

---

## Implementation Status

| Component | Phase | Status |
|-----------|-------|--------|
| VFS Foundation | 10 | ✅ Complete |
| Storage Backends | 11 | ✅ Complete |
| JS Bindings (`Nano.fs`) | 12 | 🚧 In Progress |
| Node.js fs polyfill | 12 | 🚧 In Progress |
| Snapshot Format | 13 | 📋 Planned |
| Snapshot Creation | 14 | 📋 Planned |
| Snapshot Restoration | 15 | 📋 Planned |
| CLI Integration | 16 | 📋 Planned |

---

## Philosophy

**Slivers are containers for the edge era.**

- Docker packages OS-level dependencies
- Slivers package runtime state

Both are:
- Portable artifacts
- Fast to instantiate
- Isolated execution environments
- Distributed via registries

But slivers are:
- **Lighter** — no OS, just V8 isolate
- **Faster** — ~1-2ms cold start
- **Ephemeral by default** — but can persist
- **JavaScript-native** — no translation layer

---

*A sliver of your app, anywhere, instantly.*
