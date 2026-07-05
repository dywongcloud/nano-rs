# Node.js Compatibility and Migration Guide

**Version:** 2.1.0-alpha
**Last Updated:** 2026-07-05

---

## Overview

NANO ships a from-scratch Node.js compatibility layer (`src/runtime/node_compat/`)
covering the full common built-in module surface — `http`, `net`, `os`, `path`,
`process`, `stream`, `crypto`, `events`, `util`, and more — in addition to its
native WinterTC (Web-interoperable Runtimes Community Group) APIs. WinterTC
`fetch(request)` handlers remain the recommended, zero-overhead calling
convention, but Node.js patterns now work directly rather than requiring a
rewrite.

**Key Differences from Node.js:**
- NANO is multi-tenant by design (one process, many isolated apps)
- No npm resolution at runtime (apps must be bundled — the bundle can use any
  of the modules below)
- OS-level operations that don't fit the isolation model fail loudly instead
  of silently no-oping: raw socket connect/listen, `child_process`, real OS
  thread spawn, `vm` script compilation, `cluster`, `dgram` (see
  [COMPATIBILITY.md](COMPATIBILITY.md#node-js-api-polyfills) for the exact list)
- Cold start optimized for edge deployment (~267µs)

**Target Use Case:**
- Edge functions, serverless workloads
- Static sites with light dynamic processing
- Microservices — whether written against WinterTC `fetch()` or Node's `http` module
- Cloudflare Workers migration

**NOT Suitable For:**
- Apps using native modules (C++ addons)
- Long-running processes with stateful raw-socket connections
- Multi-process architectures (`cluster`, `child_process`)

---

## Compatibility Matrix

### Core Modules

| Module | Status | Notes |
|--------|--------|-------|
| **assert** | ✅ Complete | Including `CallTracker` |
| **buffer** | ✅ Complete | Full `Buffer` API, all standard encodings |
| **console** | ✅ Complete | `table`, `group`, `dir`, `assert`, `count`, `time` |
| **crypto** | ✅ Complete | Sync `createHash`/`createHmac`/`createCipheriv`/`createSign`/`createVerify`, `randomUUID`, KDFs — *and* WebCrypto (`crypto.subtle`) |
| **dns** | ✅ Complete | `lookup`, `resolve*` |
| **events** | ✅ Complete | `EventEmitter` |
| **fs** | ✅ Complete | Full async/sync/promise API over the VFS |
| **http** | ✅ Complete | `createServer`, `request`/`get` — bridged into NANO's fetch-handler model |
| **http2** | ✅ Complete | Core streams/settings API |
| **net** | ✅ Complete | `isIP`/`BlockList`/`checkServerIdentity`; socket connect/listen sandboxed |
| **os** | ✅ Complete | `hostname()` (per-tenant), `platform`, `cpus`, memory info |
| **path** | ✅ Complete | POSIX and Windows semantics |
| **process** | ✅ Complete | `env` (per-tenant), `argv`, `nextTick`, `hrtime`, `memoryUsage` |
| **stream** | ✅ Complete | `Readable`/`Writable`/`Duplex`/`Transform`, `pipeline` |
| **url** | ✅ Complete | `URL`, `URLSearchParams`, legacy `url.parse`, `fileURLToPath` |
| **util** | ✅ Complete | `inspect`, `format`, `promisify`, `types.*` |
| **worker_threads** | ⚠️ Partial | API surface present; real OS thread spawn sandboxed |
| **vm** | ⚠️ Partial | API surface present; script compilation sandboxed (`eval` ban) |

**Overall Node.js module coverage: 29/29 (100%)** — see
[COMPATIBILITY.md](COMPATIBILITY.md#node-js-api-polyfills) for the complete
module-by-module list and the sandboxed-operation policy.

### Globals

| Global | Status | Notes |
|--------|--------|-------|
| `Buffer` | ✅ Full | Full implementation, supersedes the earlier Rust stub |
| `console` | ✅ Full | Same API, upgraded with Node's extra methods |
| `setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` | ✅ Full | Same API |
| `setImmediate`/`clearImmediate` | ✅ Full | Node-only; not part of WinterTC |
| `queueMicrotask` | ✅ Full | |
| `fetch` | ✅ Full | Native WinterTC implementation |
| `TextEncoder`/`TextDecoder` | ✅ Full | Same API |
| `crypto` | ✅ Full | WebCrypto (`crypto.subtle`) *and* `require('crypto')` both work |
| `URL`/`URLSearchParams` | ✅ Full | Same API |
| `atob`/`btoa` | ✅ Full | |
| `EventTarget`/`Event`/`CustomEvent` | ✅ Full | |
| `MessageChannel`/`MessagePort`/`BroadcastChannel` | ✅ Full | |
| `process` | ✅ Full | `process.env` populated from per-tenant `AppConfig.env_vars` |
| `global` | ✅ Full | Aliases `globalThis`, matching Node |
| `__dirname`/`__filename` | ✅ Full | Fixed values (`/`, `/handler.js`) — no real filesystem paths, since the entrypoint is a single bundled file |
| `require` | ✅ Full | Resolves any of the 29 built-in modules, bare or `node:`-prefixed |

---

## Migration Guide

The patterns below now work **without any changes** — `http`, `process.env`,
`fs`, `path`, and `crypto` (`node:crypto`) are all implemented directly. They're
kept here because the WinterTC-idiomatic alternative is still often a better
fit for the edge (no server bootstrap, structured config, async-first APIs) —
treat this as "options", not "required migrations".

### 1. `http` module — works directly, or use `fetch()`

**Works as-is:**
```javascript
const http = require('http');

const server = http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('Hello');
});

server.listen(3000);
```

NANO's handler-resolution bridge (CONTRACT.md §7) detects the registered
`http.Server` and dispatches requests to it automatically — no code changes
needed.

**WinterTC-idiomatic alternative:**
```javascript
export default {
  async fetch(request) {
    return new Response('Hello', {
      headers: { 'Content-Type': 'text/plain' }
    });
  }
};
```

The `fetch(request)` form is preferred for new code: it avoids the
IncomingMessage/ServerResponse translation overhead and maps 1:1 to how NANO
actually dispatches requests.

---

### 2. `process.env` — works directly

**Works as-is:**
```javascript
const dbUrl = process.env.DATABASE_URL;
```

`process.env` is populated per-tenant from that app's `env_vars` config (see
`AppConfig` / the admin API's app registration endpoint) — set them there
instead of in a `.env` file, since there is no filesystem-based process
environment in a multi-tenant isolate model.

**Alternatives, if you'd rather not rely on runtime config:**
```javascript
// Request headers
export default {
  async fetch(request) {
    const dbUrl = request.headers.get('X-Database-URL');
  }
};

// VFS config file
const config = JSON.parse(await Nano.fs.readFile('/data/config.json', { encoding: 'utf-8' }));

// Build-time bundling
import config from './config.json';
```

---

### 3. `fs` — works directly, or use `Nano.fs`

**Works as-is:**
```javascript
const fs = require('fs');
const data = fs.readFileSync('./data.json', 'utf8');
```

`node:fs` is backed by NANO's VFS — same data, same paths, both sync and
async/promise APIs available.

**WinterTC/NANO-idiomatic alternative:**
```javascript
const data = await Nano.fs.readFile('/data/config.json', { encoding: 'utf-8' });
```

---

### 4. `path` — works directly, or use `URL`

**Works as-is:**
```javascript
const path = require('path');
const fullPath = path.join(__dirname, 'config.json');
```

**WinterTC-idiomatic alternative:**
```javascript
const url = new URL('config.json', import.meta.url);
const response = await fetch(url);
const config = await response.json();
```

Or use relative paths with `Nano.fs`:
```javascript
const config = await Nano.fs.readFile('/data/config.json', { encoding: 'utf-8' });
```

---

### 5. `crypto` — works directly, or use WebCrypto

**Works as-is:**
```javascript
const crypto = require('crypto');
const hash = crypto.createHash('sha256').update(data).digest('hex');
```

**WebCrypto alternative** (portable to any WinterTC runtime, not NANO-specific):
```javascript
const encoder = new TextEncoder();
const data = encoder.encode('Hello');
const hashBuffer = await crypto.subtle.digest('SHA-256', data);
const hashArray = Array.from(new Uint8Array(hashBuffer));
const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
```

**Note:** WebCrypto is async and returns ArrayBuffers; `node:crypto`'s sync
API is often more convenient for one-shot digests. Both are fully implemented
— use whichever your dependencies expect.

See [API Reference](API.md) for full WebCrypto documentation.

---

### 6. `setTimeout` callback patterns with async/await

**Node.js (Old):**
```javascript
function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```

**NANO (New):**
```javascript
// Same pattern works in NANO
function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

// Usage
await delay(1000);
```

**Note:** Timer duration counts toward request CPU time limits.

---

## Common Patterns

### Express.js-style routing

A bundled Express app that only uses `http.createServer` + routing (no native
addons, no raw `net`/`cluster` access) now bridges into NANO's fetch-handler
model automatically via the `http.createServer` adapter — see
[COMPATIBILITY.md](COMPATIBILITY.md#handler-resolution-contractmd-7). This
hasn't been validated against the full Express test suite in this environment,
so treat it as "likely works" rather than a guarantee.

For a dependency that's designed for WinterTC from the ground up (smaller
bundle, no translation layer), use Hono.js (Express-like):

```javascript
import { Hono } from 'hono';

const app = new Hono();

app.get('/', (c) => c.text('Hello'));
app.get('/users/:id', (c) => {
  const id = c.req.param('id');
  return c.json({ id });
});

export default app;
```

**Bundle with Hono:**
```bash
npm install hono
npx esbuild src/index.js --bundle --outfile=dist/app.js --format=esm
```

---

### Environment-specific configuration

**Node.js (Old):**
```javascript
const config = require(`./config.${process.env.NODE_ENV}.json`);
```

**NANO (New) — Build-time bundling:**
```javascript
// Build-time environment substitution
import config from './config.json';
```

**With esbuild:**
```bash
NODE_ENV=production npx esbuild src/index.js --bundle --define:process.env.NODE_ENV=\"production\"
```

**Runtime lookup:**
```javascript
const env = request.headers.get('X-Environment') || 'production';
const config = await Nano.fs.readFile(`/data/config.${env}.json`, { encoding: 'utf-8' });
```

---

### Database connections

**Node.js (Old):**
```javascript
const { Client } = require('pg');
const client = new Client({ connectionString: process.env.DATABASE_URL });
await client.connect();
```

**NANO (New):**
```javascript
// Use fetch-based HTTP database client (if available)
// Or WebSocket-based client
// Or external database proxy

// For edge deployment, prefer:
// - Durable connection pools outside NANO
// - Connectionless protocols (HTTP-based DBs)
// - Short-lived connections per request

export default {
  async fetch(request) {
    // HTTP-based database query
    const response = await fetch('https://db-proxy.internal/query', {
      method: 'POST',
      headers: { 'Authorization': 'Bearer token' },
      body: JSON.stringify({ query: 'SELECT * FROM users' })
    });
    return response;
  }
};
```

---

## Package Compatibility

| Package Category | Compatibility | Notes |
|----------------|---------------|-------|
| **Pure ESM packages** | ✅ Excellent | Any package using standard Web APIs |
| **Node.js-specific packages (http, fs, path, crypto, util, stream, events)** | ✅ Good | These modules are now implemented directly |
| **Node.js packages needing raw sockets, native addons, or multi-process** | ❌ Poor | `net`/`tls` connect, C++ addons, `cluster`/`child_process` remain sandboxed |
| **Bundled applications** | ✅ Good | Webpack, Rollup, esbuild output |
| **Hono.js** | ✅ Excellent | Designed for WinterTC |
| **Express.js / Connect-style middleware stacks** | ⚠️ Likely Good | Bridges via `http.createServer`; not exhaustively tested against the npm package itself |
| **Next.js (static export)** | ✅ Good | Static HTML/JS output works |
| **Astro (static build)** | ✅ Good | Islands architecture preserved |
| **React/Vue/Svelte** | ✅ Good | Client-side bundles work |
| **Database ORMs** | ⚠️ Partial | Need HTTP-based drivers, or a driver that only uses `node:crypto`/`node:buffer`/`node:events` rather than raw `net` sockets |

---

## Testing Compatibility

**Node.js (Old):**
```bash
npm test  # Uses Jest, Mocha, etc.
```

**NANO (New):**
```bash
# Test with WinterTC-compatible test runner
# Or integration tests against running NANO

# Example: Use Vitest with happy-dom
npm install -D vitest
npx vitest

# Integration test
curl http://localhost:8080/test-endpoint
```

---

## Debugging

**Node.js (Old):**
```bash
node --inspect app.js
```

**NANO (New):**
```bash
# Use logging and admin API
nano-rs run --config config.json --verbose

# Monitor via admin API
curl -H "X-API-Key: secret" http://localhost:8889/isolates
```

---

## When NOT to Migrate

NANO is NOT suitable for:
- Apps requiring native modules (C++ addons)
- Long-running WebSocket servers (client + `WebSocketPair` supported; a full
  Node-style WS server library still needs validation)
- Apps needing raw TCP/UDP sockets, multi-process (`cluster`), or subprocess spawning
- Traditional server-side rendering that depends on Node.js internals beyond
  the modules listed in [COMPATIBILITY.md](COMPATIBILITY.md#node-js-api-polyfills)

**Consider staying with Node.js if:**
- You use native dependencies (C++ addons)
- You need raw socket / subprocess / multi-process access
- Your app doesn't fit either the WinterTC or bundled-Node-module execution model

---

## Framework Compatibility

| Framework | Compatibility | Notes |
|-----------|---------------|-------|
| **Next.js** | ⚠️ Static only | Static export (`next export`) works. SSR not supported. |
| **Nuxt** | ⚠️ Static only | Static generation works. Server features not supported. |
| **Gatsby** | ✅ Good | Static sites work perfectly |
| **Astro** | ✅ Excellent | Static and islands architecture fully supported |
| **SvelteKit** | ⚠️ Adapter needed | Use adapter-static or custom adapter |
| **Remix** | ⚠️ Limited | Edge adapter support needed |
| **Hono.js** | ✅ Excellent | Native WinterTC support |
| **Fresh** | ⚠️ Partial | Deno-specific, may need polyfills |
| **Express.js** | ⚠️ Likely Compatible | Bridges via `http.createServer`; not exhaustively tested against the npm package |
| **Fastify** | ⚠️ Likely Compatible | Same bridge applies; untested end-to-end against the npm package |

---

## Build Configuration

### esbuild

```bash
npx esbuild src/index.js \
  --bundle \
  --outfile=dist/app.js \
  --format=esm \
  --platform=neutral \
  --target=es2022
```

### Rollup

```javascript
// rollup.config.js
export default {
  input: 'src/index.js',
  output: {
    file: 'dist/app.js',
    format: 'esm'
  },
  external: [] // Bundle everything
};
```

### Webpack

```javascript
// webpack.config.js
module.exports = {
  entry: './src/index.js',
  output: {
    filename: 'app.js',
    library: { type: 'module' }
  },
  experiments: { outputModule: true }
};
```

---

## See Also

- [API Reference](API.md) — All available JavaScript APIs
- [WinterTC Spec](https://wintertc.org/) — Standard APIs NANO implements
- [Compatibility Matrix](COMPATIBILITY.md) — Full feature compatibility
- [CLI Reference](CLI.md) — Commands for running apps

---

*Last updated: 2026-07-05*
