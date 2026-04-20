# NANO

A multi-tenant JavaScript edge runtime. One OS process hosts many isolated apps in separate V8 isolates, with millisecond cold starts and no container overhead.

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
      "entry_point": "./apps/api.js",
      "workers": 4,
      "memory_limit_mb": 128,
      "timeout_ms": 30000
    }
  ]
}
```

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

## Slivers (Optional)

Package apps as portable snapshots for faster cold starts (~1-2ms):

```bash
# Create from running app
nano-rs sliver create api.example.com --output api-v1.sliver

# Run from sliver
nano-rs run --sliver api-v1.sliver
```

See [SLIVER.md](SLIVER.md) for full documentation.

## Admin API

HTTP admin interface on port 8889 (configurable):

```bash
curl -H "X-Admin-Key: your-key" http://localhost:8889/admin/isolates
curl -H "X-Admin-Key: your-key" http://localhost:8889/admin/metrics
```

Unix socket (default `/var/run/nano/control.sock`) for local access.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — Internal design and decisions
- [SLIVER.md](SLIVER.md) — Edge snapshots and container-like deployments
- [examples/hello.js](examples/hello.js) — Minimal example app

## Requirements

- Rust 1.70+
- No V8 compilation needed (uses pre-built rusty_v8)

## License

MIT
