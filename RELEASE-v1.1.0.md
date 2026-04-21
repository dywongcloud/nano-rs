# NANO v1.1.0 — SLIVER Release

**Multi-tenant JavaScript edge runtime with isolate snapshots and VFS**

---

## What's New in v1.1

### Sliver Snapshots — Container-Image Semantics for Isolates

**~267µs cold starts** — 3.7x faster than the 1-2ms target

Create portable, pre-initialized JavaScript isolate snapshots:

```bash
# Create a sliver from a running app
nano-rs sliver create myapp.example.com --output myapp.sliver

# Run directly from sliver (no JS file needed)
nano-rs run --sliver myapp.sliver
```

**Sliver format:**

- Tar-based archive (inspectable with `tar -tf`)
- Contains: `meta.json`, `heap.bin`, `vfs/` directory
- Opaque heap blob (version-agnostic)
- Portable between systems

### Virtual File System (VFS)

JavaScript filesystem API with pluggable backends:

```javascript
// Native NANO API
await Nano.fs.writeFile('/data/config.json', '{"key": "value"}');
const data = await Nano.fs.readFile('/data/config.json');

// Node.js fs polyfill
const fs = require('fs');
fs.writeFileSync('/tmp/test.txt', 'Hello');
```

**Features:**

- In-memory backend (default, fast)
- Disk backend (persistence)
- S3 backend (feature-gated)
- Per-isolate namespace isolation
- Path traversal protection (blocks all "..")
- Resource quotas (file count, total storage, file size)

### CLI 

```bash
# Human-readable errors with suggestions
$ nano-rs sliver lst
Error: Unknown command 'lst'
Did you mean: sliver list?

# Progress indicators
$ nano-rs sliver create myapp --output app.sliver
Creating sliver... ████████████████████ 100% (2.4 MB)

# Colorized output (respects NO_COLOR)
$ nano-rs sliver list
NAME          SIZE    CREATED             HOSTNAME
myapp         2.4MB   2026-04-20 13:45    myapp.example.com
```

### Security Enhancements

- Path traversal blocked at all layers
- Namespace isolation between apps
- SSRF prevention in outbound fetch()
- Private IP blocking
- Dangerous header filtering

---

## Performance

| Metric                       | v1.0 | v1.1       | Improvement |
| ---------------------------- | ---- | ---------- | ----------- |
| Cold Start (fresh isolate)   | ~5ms | ~5ms       | —           |
| **Cold Start (from sliver)** | N/A  | **~267µs** | **18.7x**   |
| Context Reset                | ~5ms | ~5ms       | —           |

---

## Framework Compatibility

✅ **Hono.js** — Lightweight, WinterCG-native  
✅ **Next.js static export** — HTML/CSS/JS assets serve correctly  
✅ **Astro static build** — Islands architecture preserved  
✅ **Generic WinterCG** — Any spec-compliant framework

---

## Dependencies

- Rust 1.80+
- V8 139 (via rusty_v8)
- tokio 1.52
- axum 0.7

---

## Documentation

- [README.md](../README.md) — Quick start guide
- [SLIVER.md](../SLIVER.md) — Sliver CLI reference
- [VFS.md](../VFS.md) — VFS API documentation
- [CHANGELOG.md](../CHANGELOG.md) — Full changelog

---

**Full Changelog:** [CHANGELOG.md](CHANGELOG.md)

**License:** MIT
