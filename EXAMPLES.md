# NANO Examples

Complete, copy-paste ready examples for common NANO use cases.

---

## 1. Simple JavaScript App (5 minutes)

A minimal HTTP handler using the WinterTC fetch API.

### Step 1: Create the JavaScript file

Create `app.js`:

```javascript
export default {
  async fetch(request) {
    const url = new URL(request.url);
    
    if (url.pathname === '/') {
      return new Response('Hello from NANO!', {
        headers: { 'Content-Type': 'text/plain' }
      });
    }
    
    if (url.pathname === '/json') {
      return Response.json({ 
        message: 'Hello from NANO!',
        runtime: 'nano-rs',
        time: Date.now()
      });
    }
    
    return new Response('Not Found', { status: 404 });
  }
};
```

### Step 2: Create the config

Create `nano.json`:

```json
{
  "server": {
    "port": 8080
  },
  "apps": [
    {
      "hostname": "localhost",
      "entrypoint": "./app.js",
      "limits": {
        "workers": 2,
        "memory_mb": 64
      }
    }
  ]
}
```

### Step 3: Run

```bash
# Terminal 1: Start the server
nano-rs run --config nano.json

# Terminal 2: Test
 curl http://localhost:8080/
# Output: Hello from NANO!

curl http://localhost:8080/json
# Output: {"message":"Hello from NANO!","runtime":"nano-rs","time":1234567890}
```

---

## 2. WebAssembly App (10 minutes)

Compile Rust to WASM and call it from JavaScript.

### Step 1: Create the Rust WASM module

Create `wasm/Cargo.toml`:

```toml
[package]
name = "calc"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wasm-bindgen = "0.2"

[profile.release]
opt-level = 3
lto = true
```

Create `wasm/src/lib.rs`:

```rust
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn fibonacci(n: u32) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

#[wasm_bindgen]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

### Step 2: Compile to WASM

```bash
cd wasm
wasm-pack build --target web --out-dir ../pkg
```

### Step 3: Create the JavaScript handler

Create `wasm-app.js`:

```javascript
import wasmModule from './pkg/calc.js';

let wasm;

async function initWasm() {
  wasm = await wasmModule();
}

// Initialize on first request
let initPromise = initWasm();

export default {
  async fetch(request) {
    await initPromise;
    
    const url = new URL(request.url);
    
    if (url.pathname === '/add') {
      const a = parseInt(url.searchParams.get('a') || '0');
      const b = parseInt(url.searchParams.get('b') || '0');
      const result = wasm.add(a, b);
      return Response.json({ a, b, result });
    }
    
    if (url.pathname === '/fib') {
      const n = parseInt(url.searchParams.get('n') || '10');
      const result = wasm.fibonacci(n);
      return Response.json({ n, result });
    }
    
    return new Response('Usage: /add?a=5&b=3 or /fib?n=20', {
      headers: { 'Content-Type': 'text/plain' }
    });
  }
};
```

### Step 4: Create config

Create `nano-wasm.json`:

```json
{
  "server": {
    "port": 8080
  },
  "apps": [
    {
      "hostname": "localhost",
      "entrypoint": "./wasm-app.js",
      "limits": {
        "workers": 2,
        "memory_mb": 128
      }
    }
  ]
}
```

### Step 5: Run

```bash
# Terminal 1: Start
nano-rs run --config nano-wasm.json

# Terminal 2: Test WASM calls
curl "http://localhost:8080/add?a=5&b=3"
# Output: {"a":5,"b":3,"result":8}

curl "http://localhost:8080/fib?n=20"
# Output: {"n":20,"result":6765}
```

---

## 3. Slivers: Create and Deploy (5 minutes)

Slivers are portable snapshots with ~267µs cold starts.

### Step 1: Create your app

Create `sliver-demo.js`:

```javascript
export default {
  async fetch(request) {
    const url = new URL(request.url);
    
    if (url.pathname === '/') {
      return new Response('Hello from Sliver!', {
        headers: { 'Content-Type': 'text/plain' }
      });
    }
    
    if (url.pathname === '/version') {
      return Response.json({ 
        version: '1.0.0',
        deployed: new Date().toISOString()
      });
    }
    
    return new Response('Not Found', { status: 404 });
  }
};
```

### Step 2: Run the app (for testing)

Create `dev.json`:

```json
{
  "server": { "port": 8080 },
  "apps": [{
    "hostname": "localhost",
    "entrypoint": "./sliver-demo.js",
    "limits": { "workers": 2 }
  }]
}
```

```bash
# Terminal 1: Run for testing
nano-rs run --config dev.json

# Terminal 2: Verify it works
curl http://localhost:8080/
# Output: Hello from Sliver!
```

### Step 3: Create a sliver

```bash
# In another terminal - create from the running app
nano-rs sliver create localhost --output my-app-v1.sliver

# Verify it was created
ls -lh my-app-v1.sliver
```

### Step 4: Stop the dev server

Press `Ctrl+C` in Terminal 1 to stop the dev server.

### Step 5: Run from sliver

```bash
# Terminal 1: Run directly from sliver (no config needed!)
nano-rs run --sliver my-app-v1.sliver --port 8080

# Terminal 2: Test - should have identical behavior
curl http://localhost:8080/
# Output: Hello from Sliver!

curl http://localhost:8080/version
# Output: {"version":"1.0.0","deployed":"2026-01-01T12:00:00Z"}
```

### Production: Use sliver in config

Create `production.json`:

```json
{
  "server": { "port": 80 },
  "apps": [{
    "hostname": "api.example.com",
    "sliver": "./my-app-v1.sliver",
    "limits": {
      "workers": 8,
      "memory_mb": 128
    }
  }]
}
```

```bash
# Production run
nano-rs run --config production.json
```

---

## Quick Reference

### Commands

```bash
# Run with config
nano-rs run --config nano.json

# Run directly from JS
nano-rs run --sliver app.sliver

# Create sliver from running app
nano-rs sliver create <hostname> --output <name>.sliver

# List all slivers
nano-rs sliver list
```

### Config Structure

```json
{
  "server": {
    "port": 8080,
    "host": "0.0.0.0"
  },
  "apps": [{
    "hostname": "localhost",
    "entrypoint": "./app.js",
    "sliver": null,
    "limits": {
      "workers": 4,
      "memory_mb": 128,
      "timeout_secs": 30,
      "cpu_time_ms": 50
    }
  }]
}
```

### Testing

```bash
# Basic test
curl http://localhost:8080/

# With Host header (for multi-tenant)
curl -H "Host: api.example.com" http://localhost:8080/

# JSON endpoint
curl http://localhost:8080/json
```

---

## Troubleshooting

### "Failed to parse config JSON: unknown field `port`"

Your config has `port` at the wrong level. Put it in `server`:

```json
{
  "server": { "port": 8080 },  // ✓ Correct
  "apps": [...]
}
```

NOT:

```json
{
  "port": 8080,  // ✗ Wrong
  "apps": [...]
}
```

### "Cannot find module"

Make sure your `entrypoint` path is correct. Use `./` for relative paths:

```json
"entrypoint": "./app.js"  // ✓ Correct
```

### Sliver not found

If using `--sliver`, check the file exists:

```bash
ls -lh my-app.sliver
nano-rs run --sliver ./my-app.sliver
```

### Port already in use

```bash
# Find and kill the process
lsof -ti:8080 | xargs kill -9
```

---

## See Also

- [API Reference](docs/API.md) - Complete WinterTC API documentation
- [CLI Reference](docs/CLI.md) - All CLI commands
- [Config Reference](docs/CONFIG.md) - Full configuration schema
