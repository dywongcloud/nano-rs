# NANO Runtime Compatibility Matrix

**Version:** 2.1.0-alpha
**Last Updated:** 2026-07-05

---

## WinterTC Minimum Common APIs

| API | Status | Notes |
|-----|--------|-------|
| fetch() | ✅ Complete | Full implementation with Request/Response |
| Request | ✅ Complete | Constructor with all standard properties |
| Response | ✅ Complete | Constructor with status, headers, body |
| Headers | ✅ Complete | Map-like interface, case-insensitive |
| URL | ✅ Complete | Full URL parsing |
| URLSearchParams | ✅ Complete | Query string manipulation |
| TextEncoder | ✅ Complete | UTF-8 encoding |
| TextDecoder | ✅ Complete | UTF-8 decoding |
| console | ✅ Complete | log, error, warn methods |
| ReadableStream | ✅ Complete | WinterTC streams |
| WritableStream | ✅ Complete | WinterTC streams |
| AbortController | ✅ Complete | Signal-based cancellation |
| Blob | ✅ Complete | Binary data wrapper |
| FormData | ✅ Complete | Multipart form data |
| DOMException | ✅ Complete | Standard error types |
| structuredClone | ✅ Complete | Deep object cloning |
| performance.now() | ✅ Complete | High-res timer |
| CompressionStream | ✅ Complete | gzip/deflate/deflate-raw, via `internal/web` |
| DecompressionStream | ✅ Complete | gzip/deflate/deflate-raw, via `internal/web` |
| TextEncoderStream / TextDecoderStream | ✅ Complete | Streaming text codec |
| EventTarget / Event / CustomEvent | ✅ Complete | Spec-correct fallback when the host lacks one |
| MessageChannel / MessagePort | ✅ Complete | `postMessage`/`onmessage` |
| BroadcastChannel | ✅ Complete | Same-isolate channel |

**Coverage:** 23/23 core WinterTC APIs (100%)

---

## WebCrypto APIs

| API | Status | Algorithms | Notes |
|-----|--------|------------|-------|
| crypto.getRandomValues | ✅ Complete | All TypedArray types | |
| crypto.subtle.digest | ✅ Complete | SHA-256, SHA-384, SHA-512 | |
| crypto.subtle.generateKey | ✅ Complete | AES-GCM, HMAC, RSA-OAEP, RSA-PSS, RSASSA-PKCS1-v1_5, ECDSA, ECDH | |
| crypto.subtle.importKey | ✅ Complete | JWK, PKCS8, SPKI, raw | All algorithms above |
| crypto.subtle.exportKey | ✅ Complete | JWK, PKCS8, SPKI, raw | All algorithms above |
| crypto.subtle.encrypt | ✅ Complete | AES-GCM, RSA-OAEP | |
| crypto.subtle.decrypt | ✅ Complete | AES-GCM, RSA-OAEP | |
| crypto.subtle.sign | ✅ Complete | HMAC, RSA-PSS, RSASSA-PKCS1-v1_5, ECDSA | |
| crypto.subtle.verify | ✅ Complete | HMAC, RSA-PSS, RSASSA-PKCS1-v1_5, ECDSA | |
| crypto.subtle.deriveBits | ✅ Complete | ECDH (P-256, P-384) | |
| crypto.subtle.deriveKey | ✅ Complete | ECDH (P-256, P-384) | Derives AES-GCM/HMAC keys |
| crypto.subtle.wrapKey / unwrapKey | ✅ Complete | AES-GCM wrapping | |

**Coverage:** 12/12 implemented (100%)

---

## Node.js API Polyfills

NANO ships a from-scratch, JavaScript-implemented Node.js compatibility layer
(`src/runtime/node_compat/`) — 29 modules, ~16,000 lines, each differentially
tested against real Node.js v22 behavior. Every module is reachable both via
bare specifiers (`require('crypto')`) and `node:`-prefixed specifiers
(`require('node:crypto')`), and via ESM `import` statements (see "ESM Import
Support" below).

| Module | Status | Notes |
|--------|--------|-------|
| **assert** | ✅ Complete | Including `CallTracker`, strict mode |
| **buffer** | ✅ Complete | Full `Buffer` (supersedes the Rust stub); all standard encodings |
| **console** | ✅ Complete | Upgrades the global console: `table`, `group`, `dir`, `assert`, `count`, `time` |
| **crypto** | ✅ Complete | Sync `createHash`/`createHmac`/`createCipheriv`/`createSign`/`createVerify`, `randomUUID`, `randomBytes`, `pbkdf2`/`scrypt`/`hkdf`, RSA/EC/Ed25519 key detection via DER parsing |
| **diagnostics_channel** | ✅ Complete | `channel`, `subscribe`, `unsubscribe`, `hasSubscribers` |
| **dns** / **dns/promises** | ✅ Complete | `lookup`, `resolve*` — backed by the Rust host's resolver |
| **events** | ✅ Complete | `EventEmitter`, `once`, `on`, error-event semantics |
| **fs** / **fs/promises** | ✅ Complete | Full API surface over the VFS: read/write/mkdir/rmdir/readdir/stat/rename/copyFile, sync and async/promise variants |
| **http** | ✅ Complete | `http.createServer`, `http.request`/`.get`, `IncomingMessage`/`ServerResponse`, bridged into NANO's fetch-handler model (see below) |
| **http2** | ✅ Complete | Core streams/settings API, including `getPackedSettings`/`getUnpackedSettings` |
| **module** | ✅ Complete | `require`, `require.resolve`, CommonJS shape (`module.exports`, `__dirname`, `__filename`) |
| **net** | ✅ Complete | `isIP`/`isIPv4`/`isIPv6`, `BlockList`, `checkServerIdentity`; **socket connect/listen are sandboxed (`ERR_OPERATION_NOT_PERMITTED`)** — see Sandbox Policy |
| **os** | ✅ Complete | `hostname()` (tenant-aware), `platform`, `arch`, `cpus`, `totalmem`/`freemem`, `availableParallelism` |
| **path** / **path/posix** / **path/win32** | ✅ Complete | Full API, both POSIX and Windows semantics |
| **perf_hooks** | ✅ Complete | `performance`, `PerformanceObserver` |
| **process** | ✅ Complete | `process.env` (per-tenant, from `AppConfig.env_vars`), `argv`, `platform`, `nextTick`, `hrtime`, `memoryUsage`, `exit` (sandboxed no-op — see below) |
| **punycode** | ✅ Complete | Legacy but fully ported |
| **querystring** | ✅ Complete | `parse`/`stringify`, matches Node's empty-segment handling |
| **stream** | ✅ Complete | `Readable`/`Writable`/`Duplex`/`Transform`/`PassThrough`, `pipeline`, async iteration |
| **string_decoder** | ✅ Complete | Including UTF-16 surrogate-pair boundary handling |
| **timers** / **timers/promises** | ✅ Complete | `setImmediate`/`clearImmediate` (WinterTC only provides `setTimeout`/`setInterval`) |
| **tty** | ✅ Complete | `isatty` (always false — no real TTY in an isolate) |
| **url** | ✅ Complete | `fileURLToPath`, `pathToFileURL`, legacy `url.parse`, exact percent-escape table |
| **util** | ✅ Complete | `inspect` (with Node's array/object column-grouping algorithm), `format`, `promisify`, `callbackify`, `types.*` |
| **util/types** | ✅ Complete | `isArrayBuffer`, `isTypedArray`, etc. |
| **vm** | ⚠️ Partial | Sandboxed: `vm.Script`/`runInContext` reject (`ERR_OPERATION_NOT_PERMITTED`) since NANO already bans dynamic code generation |
| **worker_threads** | ⚠️ Partial | `MessageChannel`-based API surface; **actual thread spawn is sandboxed** (one JS thread per isolate — see below) |
| **zlib** | ✅ Complete | `gzip`/`gunzip`/`deflate`/`inflate`/`deflateRaw`/`inflateRaw`/`brotli*`, sync and stream variants |

**Sandboxed by design (CONTRACT.md §6)** — present as real modules so `require()`
never throws `MODULE_NOT_FOUND`, but the operations that would need OS-level
resources fail loudly with a typed `ERR_OPERATION_NOT_PERMITTED`/EPERM error
instead of silently no-oping, per NANO's isolation model:
- `child_process` — spawn/exec/fork
- `cluster` — worker process management
- `dgram` — UDP sockets
- `net`/`tls` — raw socket connect/listen (client HTTP still works via `fetch`/`http.request`)
- `worker_threads` — actual OS thread spawn
- `vm` — `eval`-equivalent script compilation
- `inspector` — debugger protocol

**Coverage:** 29/29 modules implemented, all reachable via `require()`/`import` (100%)

### ESM Import Support

ADR-007's "Transformation" strategy now covers `import` statements, not just
`export default`. All seven ESM import forms are regex-transformed into
`require()`-based classic-script code before compilation:

```javascript
import crypto from 'node:crypto';           // default import
import { randomUUID } from 'crypto';        // named import (unprefixed works too)
import * as qs from 'node:querystring';     // namespace import
import Default, { a, b } from 'node:util';  // mixed default + named
import Default, * as ns from 'node:util';   // mixed default + namespace
import 'node:buffer';                       // side-effect only
import type { Foo } from 'some-types';      // type-only — dropped entirely
```

Both syntactic forms of a default export are handled: the literal
`export default app;` and the export-list form esbuild emits when bundling to
ESM (`export { app_default as default };`) — validated against a real esbuild
bundle of a Hono app.

Relative imports (`import { helper } from './utils.js'`) are not resolvable —
NANO's philosophy is that applications are bundled before deployment (ADR-007)
— but now fail with a clear `MODULE_NOT_FOUND` at runtime instead of an opaque
`SyntaxError` at compile time.

### Handler Resolution (CONTRACT.md §7)

The Rust runtime resolves a request handler in priority order:

1. `export default { fetch }` / `export default { async fetch() {} }` (ESM, via transform),
2. `module.exports.fetch` / `module.exports.default.fetch` (CommonJS bundles),
3. a bridge adapter around a registered `http.createServer(handler).listen()`
   listener — converts the WinterTC `Request` into an `IncomingMessage`,
   collects the `ServerResponse` output, and resolves the same
   `{ status, headers, body }` shape a `fetch(request)` handler would return,
4. a static "no fetch handler" response if none of the above apply.

This means Node-style `http.createServer` applications (including many
Express-like frameworks that only use `http.createServer` + routing, without
native addons or raw socket access) now execute without modification, in
addition to the native `fetch(request)` handler style NANO has always supported.

---

## NANO-Specific APIs

| API | Status | Notes |
|-----|--------|-------|
| Nano.fs.readFile | ✅ Complete | Async file read from VFS |
| Nano.fs.writeFile | ✅ Complete | Async file write to VFS |
| Nano.fs.exists | ✅ Complete | Check file existence |
| Nano.fs.deleteFile | ✅ Complete | Remove files |
| Nano.fs.listDir | ✅ Complete | Directory listing (also available as `node:fs` `readdir`) |
| Nano.fs.mkdir | ✅ Complete | Also available as `node:fs` `mkdir` |

---

## Framework Compatibility

| Framework | Status | Notes |
|-----------|--------|-------|
| Hono.js | ✅ Validated | Real `hono@4` bundle (esbuild) tested through the ESM transform path: routing, params, JSON round-trip |
| Next.js (static export) | ✅ Supported | Static assets + JS execution |
| Astro (static build) | ✅ Supported | Islands architecture |
| Cloudflare Workers | ⚠️ Mostly Compatible | Standard patterns work; KV, Durable Objects not available |
| Express.js | ✅ Validated | Real `express@5.2.1` bundle tested through the `http.createServer` bridge: routing, route params, query parsing, `express.json()` body parsing, 404 chain, custom headers (`framework_bundle.test.mjs`) |
| Fastify | ❌ Not Compatible | Fastify's router (find-my-way) compiles route handlers with `new Function` at startup; NANO bans dynamic code generation as a security invariant (same restriction as Cloudflare Workers) — fails at `fastify.get(...)`, even for parameterless routes |
| Nuxt (static) | ⚠️ Static only | Static generation works |
| Gatsby | ✅ Good | Static sites work perfectly |
| SvelteKit | ⚠️ Adapter needed | Use adapter-static or custom adapter |
| Remix | ⚠️ Limited | Edge adapter support needed |
| Fresh | ⚠️ Partial | Deno-specific, may need polyfills |

Express and Hono statuses are backed by executable evidence:
`src/runtime/node_compat/testing/framework_bundle.test.mjs` runs the real npm
package, bundled with esbuild exactly as a NANO user would deploy it, inside
the strict harness (eval banned, any module the compat layer doesn't provide
throws `MODULE_NOT_FOUND`), resolved through the same handler-resolution
tiers as the Rust runtime. Fastify's incompatibility is a deliberate policy
outcome, not a gap: weakening the eval ban to accommodate it would break
CONTRACT.md §1's security model.

---

## Production Multi-Tenancy (v2.1.0-alpha)

| Feature | Status | Notes |
|---------|--------|-------|
| CPU Time Tracking | ✅ Implemented | Microsecond precision per request |
| CPU Time Limits | ✅ Implemented | 50ms default (Cloudflare-style) |
| Timer-based Termination | ✅ Implemented | Linux timer_create + V8 terminate |
| Memory Monitoring | ✅ Implemented | 4-tier pressure levels |
| Soft Eviction | ✅ Implemented | Graceful isolate draining |
| LRU Eviction | ✅ Implemented | Least Recently Used policy |
| Per-Tenant Metrics | ✅ Implemented | Auto-collected per hostname |
| Prometheus Export | ✅ Implemented | /admin/metrics endpoint |
| WASM Support | ✅ Implemented | Load, compile, execute |
| WASM JS API | ✅ Implemented | WebAssembly.* full API |
| WASM Sliver Support | ✅ Implemented | Cached compiled modules |
| Per-Tenant `process.env` | ✅ Implemented | From `AppConfig.env_vars`, registered per hostname and picked up by each worker thread |
| Per-Tenant `os.hostname()` | ✅ Implemented | Set once per worker thread from the tenant hostname |

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ Complete | Fully implemented and tested |
| ⚠️ Partial | Works for common cases, limitations documented |
| 🚧 In Progress | Implementation underway |
| ❌ Not Implemented | Not available (may be planned or out of scope) |

---

## Verification Methodology

The `node_compat` layer is pure JavaScript (`src/runtime/node_compat/js/*.js`),
loaded into V8 by a small set of Rust host hooks. This lets it be tested two
ways:

1. **Differential testing against real Node.js v22** — every module has (or
   is exercised by) a test under `src/runtime/node_compat/testing/` that runs
   the *exact same JavaScript source* inside a real Node.js `vm.createContext`
   sandbox (with `codeGeneration: {strings: false}`, matching NANO's `eval`
   ban) backed by real Node builtins for any not-yet-authored peer. This does
   not require V8/rusty_v8 at all and is the primary correctness gate for the
   JS layer — run it with `node src/runtime/node_compat/testing/<name>.test.mjs`.
2. **Rust-level integration tests** (`tests/node_compat_integration_test.rs`)
   exercise the real `execute_handler` → `Script::compile` path with ESM
   `import` syntax against Node builtins, end to end.

**Note on this update:** the JavaScript-level differential tests (method 1)
were run and pass in this environment. The Rust-level integration tests
(method 2) compile cleanly (`cargo check --all-targets`) but could not be
*linked and executed* in this particular sandbox, because `rusty_v8`'s only
distribution channel (prebuilt archives on GitHub Releases) was unreachable
under this session's network policy. Run `cargo test` in a normal development
environment to confirm the Rust-level tests pass before relying on this in
production.

---

## Compatibility Claims vs Reality

### What "100%" Means

When we say "100% Complete" for WinterTC or WebCrypto APIs, we mean:
- All core APIs are implemented
- Differential/unit tests pass
- Full specification compliance for the algorithms/features listed

### What "100% Node.js Module Coverage" Means

Every module in Node's common built-in surface (`assert`, `buffer`, `console`,
`crypto`, `dns`, `events`, `fs`, `http`, `http2`, `net`, `os`, `path`,
`process`, `querystring`, `stream`, `string_decoder`, `timers`, `url`,
`util`, `zlib`, and more) is implemented and reachable via `require()`. It
does **not** mean every function of every module matches Node byte-for-byte
in every edge case, nor that native addons or raw OS access work — see the
Sandbox Policy list above for operations that intentionally fail loudly
instead of silently no-oping.

### Design Philosophy

NANO now targets **WinterTC and Node.js as co-equal, first-class surfaces**:

1. `fetch()`-style handlers remain the native, zero-overhead calling convention.
2. `http.createServer()`-style Node handlers are bridged automatically.
3. WebCrypto and `node:crypto` are both fully implemented; use whichever your
   dependencies expect.
4. Apps still must be bundled (no npm resolution at runtime) — but the bundle
   itself can now use the full Node built-in surface.

---

## Migration from Node.js

See [Node.js Compatibility and Migration Guide](NODEJS_COMPAT.md) for detailed migration patterns — most of which are now optional, since the underlying Node modules work directly.

Quick reference:

| Node.js Pattern | NANO Support |
|-----------------|----------------|
| `http.createServer()` | Works directly (bridged), or use `export default { fetch }` |
| `process.env.VAR` | Works directly (per-tenant `AppConfig.env_vars`) |
| `fs.readFileSync()` | Works directly (`node:fs`, backed by the VFS) — or `await Nano.fs.readFile()` |
| `crypto.createHash()` | Works directly (`node:crypto`) — or `crypto.subtle.digest()` |
| `path.join()` | Works directly (`node:path`) — or `new URL()` |

---

## See Also

- [API Reference](API.md) — All JavaScript APIs with examples
- [Node.js Migration Guide](NODEJS_COMPAT.md) — Detailed migration patterns
- [Node.js Compat Layer Contract](../src/runtime/node_compat/CONTRACT.md) — Normative architecture spec for the compatibility layer
- [WinterTC Spec](https://wintertc.org/) — Standard APIs NANO implements
- [Architecture Decision Records](ADR/) — Design decisions behind compatibility choices

---

*Last updated: 2026-07-05*
