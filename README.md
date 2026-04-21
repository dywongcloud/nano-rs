# NANO

A multi-tenant JavaScript edge runtime. One OS process hosts many isolated apps in separate V8 isolates, with **~1-2ms cold starts** using sliver snapshots and no container overhead.

**Features:**
- ✅ Multi-tenant JavaScript isolation (one process, many apps)
- ✅ Sub-2ms cold starts with sliver snapshots  
- ✅ WinterCG-compatible `fetch()` and streams
- ✅ WebCrypto AES-GCM, HMAC, PBKDF2
- ✅ VFS with memory/disk/S3 backends
- ✅ Hono.js, Next.js static, Astro support

## Quick Start

### Build

```bash
make build
```

Or with cargo directly:

```bash
cargo build --release
```

The binary is at `target/release/nano-rs`.

### Run

```bash
./target/release/nano-rs --config config.json
```

### Test

```bash
make test
```

## Configuration

Create a `config.json`:

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8080
  },
  "apps": [
    {
      "hostname": "api.example.com",
      "entrypoint": "./apps/api.js",
      "limits": {
        "workers": 4,
        "memory_mb": 128,
        "timeout_secs": 30
      }
    }
  ]
}
```

See [docs/config-mode.md](docs/config-mode.md) for complete configuration reference.

### Config Mode (Multi-App Hosting)

Run multiple isolated apps from a single configuration:

```bash
nano-rs run --config apps.json
```

Example `apps.json`:

```json
{
  "apps": [
    {
      "hostname": "api.example.com",
      "sliver": "./api.sliver",
      "limits": {"memory_mb": 256, "workers": 8}
    },
    {
      "hostname": "blog.example.com",
      "entrypoint": "./blog.js",
      "limits": {"memory_mb": 128, "workers": 4}
    }
  ],
  "server": {"port": 8080, "host": "0.0.0.0"}
}
```

Features:
- Virtual host routing (Host header → app)
- Per-app worker pools with isolated memory limits
- Mix of sliver-based (~267µs cold start) and entrypoint-based apps

## JavaScript App

Apps must export a fetch handler:

```javascript
export default {
  async fetch(request) {
    return new Response("Hello from NANO");
  }
};
```

NANO provides WinterCG-compatible APIs: `Request`, `Response`, `Headers`, `URL`, `TextEncoder`, `TextDecoder`, `console`, `crypto.subtle`.

### Filesystem (VFS)

Each isolate has its own ephemeral filesystem:

```javascript
// Explicit API
const data = await Nano.fs.readFile('/data/config.json');
await Nano.fs.writeFile('/data/output.txt', 'Hello');

// Or use Node.js compatible API
const fs = require('fs');
fs.writeFileSync('/data/output.txt', 'Hello');
```

## Quick Start with Slivers (v1.1)

Slivers are portable snapshots of JavaScript isolates. They enable **~1-2ms cold starts** (measured: ~267µs) vs ~50-100ms for fresh isolates.

### 1. Create a Sliver

```bash
# From a configured app
nano-rs sliver create api.example.com --name api-prod --tag v1.0
```

### 2. Run from Sliver

```bash
# Start server using the sliver
nano-rs run --sliver api-prod.sliver

# Or use config which references the sliver
nano-rs run --config production.json
```

### 3. Manage Slivers

```bash
# List all slivers
nano-rs sliver list --verbose

# Inspect a sliver
nano-rs sliver inspect api-prod.sliver

# Delete old versions
nano-rs sliver delete api-prod --force
```

### Performance Comparison

| Startup Method | Time | Use Case |
|---------------|------|----------|
| Fresh isolate | ~50-100ms | Development |
| Context reset | ~5ms | Hot code reload |
| **Sliver restore** | **~267µs** | **Production** |

See [SLIVER.md](SLIVER.md) for complete documentation.

## Admin API

HTTP admin interface on port 8889 (configurable):

```bash
curl -H "X-Admin-Key: your-key" http://localhost:8889/admin/isolates
curl -H "X-Admin-Key: your-key" http://localhost:8889/admin/metrics
```

Unix socket (default `/var/run/nano/control.sock`) for local access.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — Internal design and decisions
- [VFS.md](VFS.md) — Virtual File System API and configuration
- [SLIVER.md](SLIVER.md) — Edge snapshots and container-like deployments
- [EXAMPLES.md](EXAMPLES.md) — Comprehensive usage examples
- [examples/hello.js](examples/hello.js) — Minimal example app

## Requirements

- Rust 1.70+
- No V8 compilation needed (uses pre-built rusty_v8)

## License

MIT
