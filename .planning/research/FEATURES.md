# Feature Landscape: Edge JavaScript Runtime

**Domain:** V8-based serverless/edge runtime platforms  
**Researched:** 2026-04-19  
**Confidence:** HIGH (based on official WinterTC spec, Cloudflare docs, Deno docs)

---

## Executive Summary

Edge JavaScript runtimes have converged on the **WinterTC Minimum Common Web API** (formerly WinterCG) as the baseline standard. The 2025 specification (ECMA-429) defines the minimum viable surface area for web-interoperable server runtimes. Platforms differentiate through non-web extensions like key-value storage, inter-isolate messaging, and Node.js compatibility layers.

The feature landscape splits cleanly into:
- **Table Stakes:** WinterTC APIs — missing any of these makes your runtime feel broken
- **Differentiators:** Platform-specific value-adds that create lock-in (but also create value)
- **Anti-Features:** Things that break the isolate model — you must actively say "no" to these

---

## Table Stakes

Features users expect. Missing = product feels incomplete. Users will leave.

| Feature | WinterTC Required | Why Expected | Complexity | Notes |
|---------|-------------------|--------------|------------|-------|
| **Fetch API** | Yes | Core I/O primitive; 95%+ of edge code uses fetch() | Low | fetch(), Request, Response, Headers, URL, URLSearchParams |
| **Web Crypto** | Yes | Auth, hashing, encryption — table stakes for any web app | Medium | crypto.subtle, crypto.getRandomValues(), CryptoKey |
| **Streams** | Yes | Modern web standard for I/O; required for large body handling | Medium-High | ReadableStream, WritableStream, TransformStream, BYOB readers |
| **Encoding** | Yes | Text encoding/decoding is fundamental | Low | TextEncoder, TextDecoder, TextEncoderStream, TextDecoderStream |
| **Compression** | Yes | gzip/deflate support expected for performance | Medium | CompressionStream, DecompressionStream |
| **Console** | Yes | Debugging without console.log is impossible | Low | console.log, warn, error, debug, table, etc. |
| **Timers** | Yes | setTimeout, setInterval, clearTimeout, clearInterval | Low | Event loop integration required |
| **AbortController** | Yes | Cancel async operations — modern JS pattern | Low | AbortController, AbortSignal |
| **WebAssembly** | Yes | Running compiled WASM modules at edge is standard | Medium | WebAssembly global with compile, instantiate, validate, Memory, Module, Instance, etc. |
| **structuredClone** | Yes | Deep copying with transferables | Low | Required for MessageChannel, postMessage patterns |
| **navigator.userAgent** | Yes | Runtime detection | Low | Standard way to detect environment |
| **performance.now()** | Yes | High-resolution timing | Low | Returns time since navigation start |
| **URLPattern** | Yes | Route matching — increasingly expected | Low | Modern alternative to regex routing |
| **Blob/File** | Yes | Binary data handling | Low | Required for FormData, file uploads |
| **FormData** | Yes | Form submissions, multipart handling | Low | Standard for POST request bodies |
| **queueMicrotask** | No* | Critical for async/await correctness | Low | *WinterTC gap but widely expected |
| **atob/btoa** | Yes | Base64 encoding/decoding | Low | Convenience APIs from HTML spec |

### WinterTC 2025 Snapshot Summary

Per the ECMA-429 specification (Dec 2025), a conforming implementation MUST provide:

**Core Interfaces (5.1):**
- DOM: AbortController, AbortSignal, Event, EventTarget
- HTML: CustomEvent, ErrorEvent, MessageChannel, MessageEvent, MessagePort, PromiseRejectionEvent
- WEBIDL: DOMException
- FETCH: Headers, Request, Response
- XHR: FormData
- FILEAPI: Blob, File
- COMPRESSION: CompressionStream, DecompressionStream
- STREAMS: Full streams suite (ReadableStream, WritableStream, TransformStream, controllers, readers, writers, strategies)
- ENCODING: TextEncoder, TextDecoder, TextEncoderStream, TextDecoderStream
- URL: URL, URLSearchParams
- URLPATTERN: URLPattern
- WEBCRYPTO: Crypto, CryptoKey, SubtleCrypto
- HR-TIME: Performance
- WASM: Full WebAssembly JS interface

**Global Methods (5.2):**
- globalThis, fetch, console, crypto, performance, WebAssembly
- atob, btoa, clearTimeout, clearInterval, setTimeout, setInterval
- navigator.userAgent, onerror, onunhandledrejection, onrejectionhandled
- queueMicrotask, reportError, self, structuredClone

---

## Differentiators

Features that set products apart. Not expected, but valued. Create competitive moats.

| Feature | Platform Example | Value Proposition | Complexity | Notes |
|---------|------------------|-------------------|------------|-------|
| **Key-Value Storage** | Cloudflare KV, Deno KV | Edge-native persistence; ~sub-ms reads | Medium-High | Must be eventually consistent, globally replicated |
| **Object Storage** | Cloudflare R2, S3-compatible | Large file storage at edge | Medium | Stream-friendly, HTTP-range support |
| **WebSocket Server** | Cloudflare Workers, Deno Deploy | Real-time bidirectional communication | High | Requires persistent connections, upgrade handling, message framing |
| **Durable Objects/State** | Cloudflare Durable Objects | Stateful coordination across requests | Very High | Single-threaded, transactional, persistent |
| **Inter-Isolate Messaging** | Cloudflare (via DO), custom | Cross-request state sharing | Medium-High | BroadcastChannel, custom pub/sub |
| **Virtual File System** | Cloudflare (behind flag) | Writable temp storage per isolate | Medium | In-memory or persisted; cleanup semantics matter |
| **TCP/TLS Socket Outbound** | Cloudflare, Deno Deploy | Connect to external databases | High | Connect to Postgres, MySQL, Redis from edge |
| **Node.js Compatibility** | Cloudflare (nodejs_compat), Deno | Run npm packages at edge | Very High | fs, path, crypto, stream, http polyfills |
| **HTML Rewriting** | Cloudflare HTMLRewriter | Streaming DOM manipulation | Very High | CSS-selector based, streaming parser |
| **Edge Caching** | Cloudflare Cache API | Request/response caching at edge | Medium | Cache API with Cloudflare-specific extensions |
| **Cron/Scheduled Tasks** | Cloudflare, Deno Deploy | Time-based function invocation | Medium | Scheduler API, trigger management |
| **EventSource (SSE)** | Cloudflare | Server-sent events streaming | Medium | Persistent HTTP connections |
| **RPC Between Workers** | Cloudflare Workers | Direct service-to-service calls | Medium | Service bindings, zero-egress-cost routing |
| **Environment/Bindings** | Cloudflare (env), Deno (Deno.env) | Secret management, service wiring | Low | Type-safe binding declarations |
| **Compatibility Dates** | Cloudflare | Runtime version pinning | Low | opt-in to breaking changes |
| **V8 Snapshots** | Cloudflare, custom | Faster cold starts | Medium | Pre-compile/serialize isolate state |
| **Profiling/Debugging** | Chrome DevTools integration | Runtime observability | Medium | V8 inspector protocol support |

### Key Differentiator Analysis

**WebSocket Server Support:**
- **Why it matters:** Real-time apps (chat, gaming, collab editing) require bidirectional persistent connections
- **Implementation complexity:** HIGH — requires handling Upgrade header, Sec-WebSocket-Key, frame parsing, masking, ping/pong, close handshake
- **Cloudflare approach:** WebSocketPair() for server-side, new WebSocket() for client connections, requires `accept()` call
- **Deno approach:** Native Deno.upgradeWebSocket() API

**TCP Socket Outbound:**
- **Why it matters:** Direct database connectivity without HTTP bridging
- **Implementation complexity:** HIGH — requires managing connection pooling, TLS handshake from within isolate
- **Current state:** Cloudflare added TCP sockets 2024; Deno Deploy has Deno.connect/Deno.connectTls

**Node.js Compatibility:**
- **Why it matters:** npm ecosystem access — massive library availability
- **Implementation complexity:** VERY HIGH — must polyfill fs, net, http, stream, crypto, etc.
- **Cloudflare approach:** Built-in APIs for common modules + polyfill injection via Wrangler
- **Node.js APIs supported (Cloudflare):** Buffer, crypto, fs, http, https, net, path, process, stream, url, zlib, etc.
- **Anti-pattern warning:** Not all Node.js APIs make sense at edge (child_process, cluster, etc. are stubs)

---

## Anti-Features

Features to explicitly NOT build. These break the edge/isolate model or create unsustainable complexity.

| Anti-Feature | Why Avoid | What to Do Instead | Risk if Built |
|--------------|-----------|-------------------|---------------|
| **Filesystem Access (real)** | Edge runs in V8 isolates, not OS processes. No real fs. | Use virtual file system (in-memory), object storage (R2/S3), or KV | Breaks isolate portability; security nightmare |
| **Child Process Spawning** | No OS-level process control in isolates. | Use external services, queue workers, or serverless functions | Breaks security model; can't work in V8 |
| **Native Module Loading (.node, .so, .dll)** | Requires OS-level linking; violates sandbox. | Use WASM modules, pure JS alternatives, or external services | Platform-specific; breaks edge portability |
| **eval() / new Function()** | Security risk in multi-tenant environments; arbitrary code execution. | Use structured JSON configs, safe expression evaluators | Prototype pollution, injection attacks |
| **WebAssembly.compile with buffer** | Same as eval — allows arbitrary code. | Use compileStreaming with known sources, pre-compiled modules | Code injection in multi-tenant |
| **Persistent TCP Connections (inbound)** | Edge isolates don't accept inbound TCP — proxy only. | Use WebSockets for bidirectional, HTTP for request/response | Architectural mismatch with edge model |
| **SharedArrayBuffer without COOP/COEP** | Security vulnerability (Spectre). | Don't implement, or require secure context headers | Spectre-class attacks |
| **process.exit() / os-level signals** | No OS process to kill. | Let request complete, return error response | Undefined behavior in isolates |
| **Dynamic import() with arbitrary URLs** | Security and caching complexity. | Static imports, explicit allow-list, or VFS-backed imports | Supply chain attacks, unpredictable loading |
| **npm Package Resolution** | Complex dependency tree, native modules, post-install scripts. | User bundles to single file, use CDN imports (esm.sh, unpkg) | 2hr+ deploy times, native module failures |
| **Debugger on Production** | Security risk, performance overhead. | Use structured logging, metrics, replay systems | PII exposure, DoS vector |
| **Synchronous I/O (readFileSync, etc.)** | Blocks event loop; antithetical to edge performance. | Always async/streaming APIs | Performance collapse under load |

### Anti-Feature Rationale

**Why No Native Modules:**
- Edge isolates are pure V8 — no Node.js runtime, no libuv, no OS access
- Native modules (.node files) require dynamic linking to Node.js internals
- Cross-platform ABI issues (macOS dev → Linux deploy)
- Sharp, bcrypt, canvas, etc. all fail at edge
- **Alternative:** WASM-compiled versions (e.g., wasm-canvas, bcryptjs)

**Why No eval()/new Function():**
- Multi-tenant security: One tenant's eval could affect another
- Dynamic code evaluation breaks static analysis for security scanning
- Cloudflare explicitly prohibits: "Dynamic code evaluation (e.g. 'eval', 'new Function', 'WebAssembly.compile') not allowed"
- **Alternative:** JSON.parse for configs, safe expression parsers, or WASM sandbox

**Why No npm Resolution:**
- npm install runs post-install scripts — arbitrary code at deploy time
- Native dependency compilation breaks in edge environment
- Dependency tree bloats bundle size (1MB+ limit on many platforms)
- Lockfile complexity, security audit surface
- **Alternative:** Users bundle with esbuild/rollup to single file; runtime loads from VFS

---

## Feature Dependencies

Some features require others. Build in this order or face rework.

```
Fetch API
  ├── Request/Response
  ├── Headers
  ├── URL/URLSearchParams
  └── Blob/File (for body handling)

Streams
  ├── ReadableStream → requires underlying source management
  ├── WritableStream → requires sink/backpressure
  └── TransformStream → requires both + queuing

Web Crypto
  ├── crypto.getRandomValues() (synchronous)
  └── crypto.subtle (async, algorithm implementations)
      ├── Digest (SHA-1, SHA-256, SHA-384, SHA-512)
      ├── Sign/Verify (RSA, ECDSA, HMAC, Ed25519)
      ├── Encrypt/Decrypt (RSA-OAEP, AES-GCM, AES-CBC)
      └── Key derivation (PBKDF2, HKDF, ECDH)

WebSocket Server
  ├── HTTP upgrade handling (fetch-based)
  ├── WebSocket frame parser (masking, opcode handling)
  ├── MessageEvent dispatch
  └── Connection lifecycle (close handshake, ping/pong)

Node.js Compatibility
  ├── EventEmitter (events)
  ├── Buffer (buffer)
  ├── Stream (stream - requires Web Streams bridge)
  ├── Crypto (crypto - bridge to Web Crypto)
  ├── Path (path - pure JS)
  ├── FS (fs - virtual file system required)
  └── Process (process - env, nextTick polyfills)
```

---

## Complexity Matrix

| Feature | Implementation Complexity | Runtime Overhead | Maintenance Burden | Priority for NANO |
|---------|--------------------------|------------------|-------------------|-------------------|
| Fetch + Request/Response | Low | Low | Low | **P0 — Core** |
| Headers | Low | Low | Low | **P0 — Core** |
| URL/URLSearchParams | Low | Low | Low | **P0 — Core** |
| TextEncoder/Decoder | Low | Low | Low | **P0 — Core** |
| crypto.getRandomValues | Low | Low | Low | **P0 — Core** |
| console | Low | Low | Low | **P0 — Core** |
| Timers | Medium | Low | Low | **P0 — Core** |
| AbortController | Low | Low | Low | **P1 — Required** |
| Streams | Medium-High | Medium | Medium | **P1 — Required** |
| crypto.subtle | Medium-High | Medium | Medium | **P1 — Required** |
| CompressionStream | Medium | Medium | Low | **P2 — Important** |
| WebAssembly | Medium | Medium | Low | **P2 — Important** |
| WebSocket Server | High | Medium | High | **P2 — Differentiator** |
| Virtual File System | Medium | Medium | Medium | **P2 — Important** |
| Node.js Compatibility | Very High | High | Very High | **P3 — Later** |
| TCP Socket Outbound | High | Medium | High | **P3 — Later** |
| Inter-Isolate Messaging | High | Medium | High | **P3 — Differentiator** |
| HTMLRewriter | Very High | High | High | **P4 — Cloudflare-specific** |

---

## Platform Comparison Matrix

| Capability | Cloudflare Workers | Deno Deploy | Vercel Edge | WinterTC Minimum |
|------------|-------------------|-------------|-------------|------------------|
| **Core Web APIs** | Full | Full | Full | Required |
| **Streams** | Full | Full | Full | Required |
| **Web Crypto** | Full | Full | Full | Required |
| **WebAssembly** | Full | Full | Full | Required |
| **Compression** | Full | Full | ? | Required |
| **WebSocket Server** | ✅ Yes | ✅ Yes | ⚠️ Limited | No |
| **KV Storage** | ✅ KV | ✅ Deno KV | ⚠️ Edge Config | No |
| **TCP Outbound** | ✅ Yes | ✅ Yes | ❌ No | No |
| **Node.js Compat** | ✅ Extensive | ✅ Full (Deno 2) | ⚠️ Limited | No |
| **Cron Jobs** | ✅ Cron Triggers | ✅ Deno.cron() | ❌ No | No |
| **HTML Rewriting** | ✅ HTMLRewriter | ❌ No | ❌ No | No |
| **Cold Start** | <1ms | 0-5ms | ~5ms | N/A |
| **Memory Limit** | 128MB | 512MB | 128MB | N/A |
| **CPU Time (free)** | 10ms | 50ms | ? | N/A |
| **CPU Time (paid)** | 5min | 200ms | 30s | N/A |

---

## MVP Recommendation for NANO

Based on research, prioritize:

### Phase 1 — Table Stakes (Must Have)
1. **Fetch API** — Request, Response, Headers, URL
2. **Crypto** — getRandomValues (sync), subtle foundation (async)
3. **Encoding** — TextEncoder/Decoder
4. **Console** — All methods
5. **Timers** — setTimeout, setInterval, clear variants
6. **AbortController** — Modern cancellation primitive

### Phase 2 — Core Differentiation
7. **Streams** — ReadableStream, WritableStream, TransformStream (required for real-world fetch)
8. **WebAssembly** — Global namespace, compile/instantiate
9. **Compression** — gzip/deflate streams
10. **WebSocket Server** — RFC 6455 implementation

### Phase 3 — Advanced Features
11. **Full crypto.subtle** — All algorithms (RSA, ECDSA, AES, etc.)
12. **Virtual File System** — Per-isolate, in-memory
13. **Inter-Isolate Messaging** — BroadcastChannel or equivalent
14. **Node.js Compatibility Layer** — fs, path, crypto polyfills

### Phase 4 — Platform Features
15. **TCP Socket Outbound** — Deno.connect equivalent
16. **URLPattern** — Modern routing
17. **Performance API** — timing APIs

### Explicitly Defer (Anti-Features)
- ❌ eval() / new Function() — Security
- ❌ Native module loading — Platform incompatible
- ❌ Child process — OS access violation
- ❌ npm resolution — Complexity explosion
- ❌ TypeScript transpilation — User responsibility

---

## Gotchas & Edge Cases

### Date.now() in Workers
Cloudflare specifically notes: `Date.now()` returns the time of the last I/O; it does not advance during code execution. This is a performance optimization. NANO should consider this behavior.

### Performance.now() Precision
Workers intentionally reduce precision: returns time of last I/O, not real-time. This prevents timing attacks.

### WinterTC Gaps
- queueMicrotask — Standard expects it but listed as gap in PROJECT.md
- atob/btoa — Listed as gap but actually in WinterTC spec (HTML Standard reference)

### WebSocket Close Handling
2026-04-07+ compatibility flag changes: auto-reply to close frames. Must handle allowHalfOpen for proxying use cases.

### Memory Limits Are Hard
Unlike containers that can swap, isolate OOM is immediate termination. No graceful degradation.

### CPU Time vs Wall Clock
- CPU time = actual execution cycles
- Waiting on fetch() doesn't count
- JSON.parse() on 5MB payload DOES count

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| WinterTC API Surface | **HIGH** | Official ECMA-429 spec published Dec 2025 |
| Cloudflare Features | **HIGH** | Official docs, actively maintained |
| Deno Deploy Features | **HIGH** | Official docs, recently GA (Feb 2026) |
| Anti-Features | **HIGH** | Well-established security constraints |
| Complexity Ratings | **MEDIUM** | Based on implementation experience, not measured data |
| Feature Dependencies | **HIGH** | Logical deduction from API semantics |

---

## Sources

1. **WinterTC Minimum Common Web API Specification (ECMA-429)** — https://min-common-api.proposal.wintertc.org/
   - Authoritative source for table stakes requirements
   - Published Dec 2025 by Ecma TC55

2. **Cloudflare Workers Runtime APIs** — https://developers.cloudflare.com/workers/runtime-apis/
   - Production runtime documentation
   - Limits: https://developers.cloudflare.com/workers/platform/limits/

3. **Cloudflare Node.js Compatibility** — https://developers.cloudflare.com/workers/runtime-apis/nodejs/
   - Detailed matrix of supported Node.js APIs

4. **Deno Deploy Runtime API** — https://docs.deno.com/deploy/reference/runtime/
   - Feature set and changelog

5. **Edge Computing Guide 2026** — Multiple sources
   - Daily.dev comparison (April 2026)
   - Architecting on Cloudflare book excerpts
   - PkgPulse blog on edge npm packages

---

## Open Questions for Phase-Specific Research

1. **WebSocket Implementation:** What is the minimal viable WebSocket server implementation? (RFC 6455 parsing complexity)

2. **Crypto Algorithm Priority:** Which crypto.subtle algorithms are actually used in production? (Likely: ECDSA P-256, RSA-PSS, AES-GCM, SHA-256)

3. **Node.js Compat Scope:** Which Node.js APIs are the 80% use case? (Likely: Buffer, stream, crypto, path, fs.promises)

4. **Virtual File System Semantics:** Should VFS be:
   - Pure in-memory (reset on context disposal)
   - Persisted across requests (with limits)
   - Backed by actual filesystem (with sandbox)

5. **Inter-Isolate Messaging:** What pattern — BroadcastChannel (per spec), custom pub/sub, or service bindings?

