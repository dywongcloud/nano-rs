# NANO Node.js Compatibility Layer — Architecture Contract

**Scope:** This document is the single normative reference for the Node.js
compatibility layer. Every JavaScript builtin module under `js/` and every
Rust host hook in `mod.rs` MUST conform to it exactly.

---

## 1. Execution environment

- V8 (rusty_v8 v147), ES2022. Classes, private fields, generators,
  async/await, optional chaining, `??`, spread, `BigInt` are all available.
- **`eval` and `new Function` are BANNED** (`set_allow_generation_from_strings(false)`
  plus hardening removes them). No dynamic code generation of any kind.
- Scripts run inside a per-request V8 context. Top-level code in module
  factories runs at most once per context (lazy, on first `require`).
- Strict mode everywhere: every file starts with `"use strict";`.
- No `Date.now` restrictions here (unlike workflow scripts) — wall clock is fine.

### Pre-existing globals (bound by the Rust runtime before this layer loads)

`console` (log/warn/error/info/debug), `TextEncoder`, `TextDecoder`,
`URL`, `URLSearchParams`, `Headers`, `Request`, `Response`, `fetch`,
`Blob`, `FormData`, `DOMException`, `structuredClone`, `performance.now()`,
`crypto.getRandomValues` + `crypto.subtle` (digest/generateKey/importKey/
exportKey/encrypt/decrypt/sign/verify/deriveBits/deriveKey),
`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval`,
`ReadableStream`/`WritableStream` (WinterTC), `WebAssembly`,
`WebSocketPair`, `Nano.fs` (readFile/writeFile/exists/deleteFile/listDir),
`_nano_fs` (legacy sync fs object: readFileSync/writeFileSync/existsSync/unlinkSync),
`Buffer` (a MINIMAL Rust stub — the JS `buffer` module REPLACES it; do not rely on it).

Missing (this layer provides them): `process`, `global`, `setImmediate`,
`clearImmediate`, `queueMicrotask`, `atob`/`btoa`, `EventTarget`/`Event`/
`CustomEvent`, `MessageChannel`/`MessagePort`, `BroadcastChannel`,
`CompressionStream`/`DecompressionStream`, `TextEncoderStream`/
`TextDecoderStream`, `navigator`, `AbortController`/`AbortSignal`
(present but re-verify; if absent, `internal/web` installs a spec-correct one).

---

## 2. Module registration protocol

Each file under `js/` registers one or more builtin modules with the loader
defined in `js/00_prelude.js`:

```js
"use strict";
__nanoNodeRegister("events", function (module, exports, require) {
  // CommonJS body. `module.exports = ...` or mutate `exports`.
});
```

Rules:

- `__nanoNodeRegister(id, factory)` — `id` is the bare name (`"events"`,
  `"fs/promises"`, `"internal/web"`). The loader aliases `node:<id>`
  automatically. Never register the `node:`-prefixed form.
- Factories are lazy: they run on first `require(id)`. Circular requires
  follow CommonJS semantics (partial exports visible).
- `require` inside a factory resolves builtin ids only (`"buffer"`,
  `"node:stream"`, `"internal/errors"` …). Unknown ids throw
  `ERR_MODULE_NOT_FOUND` (`code: "MODULE_NOT_FOUND"`).
- Files are concatenated in lexicographic filename order; registration order
  does not matter because instantiation is lazy. **A factory must not call
  `require` at registration time** (only inside the factory body).
- Do not touch `globalThis` from factories. Global installation happens only
  in `99_init.js` (owned by the integrator) or via an exported
  `__installGlobals(g)` function that `99_init.js` invokes explicitly.
- Prefix private cross-module helpers with `internal/` (e.g.
  `internal/errors`, `internal/streams-util`). They are hidden from
  `module.builtinModules` but requireable by this layer.

## 3. Error conventions (`internal/errors`)

`js/01_errors.js` registers `internal/errors` exporting:

```js
codes;                      // { ERR_INVALID_ARG_TYPE: class extends TypeError, ... }
makeError(Base, code, message) -> Error   // sets .code, fixes .name
uvError(code, syscall, path) -> Error     // ENOENT/EEXIST/... with errno, syscall, path
ERR_METHOD_NOT_IMPLEMENTED, ERR_INVALID_ARG_TYPE, ERR_INVALID_ARG_VALUE,
ERR_OUT_OF_RANGE, ERR_INVALID_CALLBACK, ERR_MISSING_ARGS,
ERR_STREAM_DESTROYED, ERR_STREAM_WRITE_AFTER_END, ERR_STREAM_ALREADY_FINISHED,
ERR_STREAM_PREMATURE_CLOSE, ERR_STREAM_PUSH_AFTER_EOF, ERR_STREAM_NULL_VALUES,
ERR_UNHANDLED_ERROR, ERR_BUFFER_OUT_OF_BOUNDS, ERR_UNKNOWN_ENCODING,
ERR_CRYPTO_INVALID_DIGEST, ERR_OPERATION_NOT_PERMITTED, ERR_UNSUPPORTED_OPERATION,
ERR_SOCKET_BAD_PORT, ERR_INVALID_PROTOCOL, ERR_INVALID_URL, ERR_IPC_CHANNEL_CLOSED
```

Every error a module throws MUST carry a Node-correct `.code`. Modules whose
operations are forbidden by the multi-tenant sandbox (see §6) throw/emit
`ERR_OPERATION_NOT_PERMITTED` with `errno: -1 (EPERM)`, `syscall` set, and a
message that names the NANO security policy — never a silent no-op, never a
fake success.

## 4. Rust host hooks — `globalThis.__nano_node_host`

All hooks are **synchronous**, throw JS `Error` with `.code` on failure, take
and return `Uint8Array` for binary data. The object is frozen after bind.
(Implemented in `src/runtime/node_compat/mod.rs`; JS must treat this table as
the exact, complete list.)

```text
-- crypto ------------------------------------------------------------------
cryptoDigest(alg, data) -> Uint8Array            alg: md5|sha1|sha224|sha256|sha384|sha512
cryptoHmac(alg, key, data) -> Uint8Array         same algs
cryptoPbkdf2(password, salt, iterations, keylen, alg) -> Uint8Array   alg: sha1|sha256|sha384|sha512
cryptoScrypt(password, salt, N, r, p, keylen) -> Uint8Array
cryptoHkdf(alg, ikm, salt, info, keylen) -> Uint8Array
cryptoRandomBytes(n) -> Uint8Array
cryptoTimingSafeEqual(a, b) -> boolean            throws ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH on length mismatch
cryptoCipher(op, algo, key, iv, data, aad, tag) -> { data: Uint8Array, tag: Uint8Array|undefined }
    op: "encrypt"|"decrypt"
    algo: aes-128-gcm|aes-192-gcm|aes-256-gcm|aes-128-cbc|aes-192-cbc|aes-256-cbc|aes-128-ctr|aes-192-ctr|aes-256-ctr
    aad/tag: Uint8Array or null (GCM only; tag required for GCM decrypt; encrypt returns 16-byte tag)
    CBC uses PKCS#7 padding.
cryptoRsaGenerate(modulusBits) -> { privatePem, publicPem }            PKCS#8 / SPKI PEM
cryptoRsaSign(padding, hash, privatePem, data, saltLength) -> Uint8Array   padding: pkcs1|pss
cryptoRsaVerify(padding, hash, publicPem, data, signature, saltLength) -> boolean
cryptoRsaEncrypt(padding, hash, publicPem, data) -> Uint8Array         padding: oaep|pkcs1
cryptoRsaDecrypt(padding, hash, privatePem, data) -> Uint8Array
cryptoEcGenerate(curve) -> { privatePem, publicPem }                   curve: p256|p384
cryptoEcSign(curve, hash, privatePem, data) -> Uint8Array              DER-encoded ECDSA signature
cryptoEcVerify(curve, hash, publicPem, data, signature) -> boolean
cryptoEd25519Generate() -> { privatePkcs8: Uint8Array, publicRaw: Uint8Array }
cryptoEd25519Sign(privatePkcs8, data) -> Uint8Array
cryptoEd25519Verify(publicRaw, data, signature) -> boolean

-- zlib --------------------------------------------------------------------
zlibSync(kind, data, level) -> Uint8Array
    kind: gzip|gunzip|deflate|inflate|deflateRaw|inflateRaw|unzip|brotliCompress|brotliDecompress
    level: 0-9 (or -1 default; ignored for decompressors/brotli)
zlibCreate(kind, level) -> number (handle)
zlibPush(handle, chunk, finish) -> Uint8Array     incremental output; finish=true flushes and invalidates handle
zlibFree(handle) -> undefined                     idempotent

-- fs (VFS-backed, sync) ----------------------------------------------------
fsReadFile(path) -> Uint8Array                    throws ENOENT
fsWriteFile(path, data) -> undefined
fsExists(path) -> boolean
fsUnlink(path) -> undefined                       throws ENOENT
fsMkdir(path, recursive) -> undefined             throws EEXIST (non-recursive, exists)
fsRmdir(path) -> undefined                        throws ENOTEMPTY if children exist
fsReaddir(path) -> string[]                       names only; throws ENOENT/ENOTDIR
fsStat(path) -> { size, mtimeMs, birthtimeMs, isFile, isDirectory }  throws ENOENT
fsRename(from, to) -> undefined
fsCopyFile(from, to) -> undefined

-- process / os -------------------------------------------------------------
hrtime() -> { sec: number, ns: number }           monotonic
memoryUsage() -> { rss, heapTotal, heapUsed, external }
hostname() -> string                              tenant hostname or "nano"
availableParallelism() -> number
getEnv() -> object                                per-app configured env vars (string->string)

-- dns ----------------------------------------------------------------------
dnsLookup(host, family) -> [{ address: string, family: 4|6 }]   family: 0|4|6; throws ENOTFOUND
```

## 5. Global installation order (`99_init.js`, integrator-owned)

1. `internal/web` — `__installGlobals`: EventTarget/Event/CustomEvent,
   AbortController/AbortSignal (if missing), atob/btoa, queueMicrotask,
   MessageChannel/MessagePort, BroadcastChannel, CompressionStream/
   DecompressionStream, TextEncoderStream/TextDecoderStream, navigator.
2. `buffer` — installs the full JS `Buffer`, `Blob` alias kept, `atob/btoa`
   consistency; **overwrites** the Rust Buffer stub global.
3. `timers` — installs `setImmediate`/`clearImmediate` (queueMicrotask-based
   macro-task emulation over `setTimeout(0)` ordering).
4. `process` — installs `globalThis.process`, `globalThis.global`.
5. `console` upgrades (`internal/console`) — extends the bound console with
   table/group/count/time/dir/assert/trace/countReset/timeEnd/timeLog.
6. Handler bridge — defines `globalThis.__nano_resolve_handler` (§7).
7. `require` — installs the full loader as `globalThis.require`
   (superseding the Rust `fs`-only `require`), plus `globalThis.module`,
   `globalThis.exports`, `globalThis.__dirname = "/"`,
   `globalThis.__filename = "/handler.js"`.

## 6. Sandbox policy (restricted operations)

The NANO isolate model forbids: raw sockets, spawning processes/threads/
workers, dynamic code evaluation, inspector attachment, host filesystem
outside the VFS. The corresponding APIs MUST exist with Node-correct
shapes and MUST fail loudly and predictably:

| Module / API | Behavior |
|---|---|
| `child_process` spawn/exec/fork family | async forms emit/callback `ERR_OPERATION_NOT_PERMITTED` (EPERM); sync forms throw it |
| `cluster.fork` | throws EPERM; `isPrimary=true`, `workers={}` |
| `worker_threads.Worker` | constructor throws EPERM; `isMainThread=true`; MessageChannel/MessagePort/markAsUntransferable fully functional in-isolate |
| `net.Socket.connect`, `net.createConnection`, `net.Server.listen` | EPERM (async: `error` event). `isIP/isIPv4/isIPv6`, address parsing fully functional |
| `tls.connect`, `tls.createServer` | EPERM; `getCiphers()`, constants functional |
| `dgram.createSocket` send/bind | EPERM on bind/send |
| `vm.Script`, `runInContext` etc. | throws EPERM (`ERR_OPERATION_NOT_PERMITTED`) citing the no-dynamic-code policy |
| `inspector.open/url/Session.connect` | `ERR_INSPECTOR_NOT_AVAILABLE` |
| `wasi` | `ERR_WASI_NOT_AVAILABLE`-style coded error on `new WASI()` |
| `trace_events.createTracing` | `ERR_TRACE_EVENTS_UNAVAILABLE` |
| `process.exit/kill/abort` | `exit` throws a catchable coded error the runtime maps to request termination; never kills the host |

`http`/`https` **server** APIs are NOT restricted: they bridge to the NANO
fetch-handler model (§7). `http`/`https`/`http2` **client** APIs run over the
bound `fetch`.

## 7. Handler bridge

`http.createServer(handler).listen(port)` registers the handler in
`internal/http-bridge`. `globalThis.__nano_resolve_handler()` (installed by
`99_init.js`) returns, in priority order:

1. `__nano_user_fetch` if defined (ESM transform output),
2. `module.exports.fetch` / `module.exports.default.fetch` (CJS bundles),
3. an adapter around the registered `http.Server` handler that converts the
   WinterTC request object into `IncomingMessage`, collects `ServerResponse`
   output, and resolves a `{ status, headers, body }` Response-shape,
4. `undefined` (runtime falls back to legacy behavior).

The Rust side (handler.rs / module.rs) calls `__nano_resolve_handler` after
user script evaluation when the legacy lookups fail.

## 8. Style invariants

- No `TODO`/`FIXME`/commented-out code. Total functions; every code path
  returns or throws typed errors.
- No `console.log` debugging residue.
- Deterministic iteration (no reliance on object key order beyond insertion
  order semantics guaranteed by ES).
- Zero dependence on host time zone (`Date` used only for epoch ms).
- Each file self-contained except `require` of other registered ids.
- Match Node v22 documented behavior; where NANO diverges (sandbox), the
  divergence is in §6 and documented in `docs/NODEJS_COMPAT.md`.
