# NANO Edge JavaScript Runtime — Research Summary

**Project:** NANO — Edge JavaScript Runtime (Rust Migration)  
**Domain:** V8-based serverless/edge runtime platforms  
**Researched:** 2026-04-19  
**Confidence:** HIGH (based on official WinterTC spec, Deno source patterns, stable releases)

---

## Executive Summary

NANO is a multi-tenant edge JavaScript runtime that hosts isolated applications in V8 isolates, eliminating container overhead while maintaining security boundaries. Based on research of Cloudflare Workers, Deno Deploy, and the WinterTC (formerly WinterCG) standard, the recommended approach centers on **rusty_v8** for V8 integration (not deno_core, which couples too many runtime decisions), **tokio + axum** for the async HTTP layer, and manual implementation of the extension/op pattern for JS-to-Rust bridging.

The 2025 ecosystem has matured significantly—rusty_v8 now tracks Chrome versions (v147.x as of April 2026) with pre-built binaries, eliminating the multi-hour V8 compile times. The core architectural pattern is WorkerPool-per-app with isolate-per-thread and context reset between requests (5ms overhead vs 50-100ms for full isolate recreation). All I/O must be async via tokio channels, never blocking the V8 thread.

Key risks include V8's External Pointer Table (EPT) SIGSEGV crashes when ArrayBuffers aren't properly managed, handle scope misuse causing memory leaks, and thread safety violations if isolates are moved between threads. These are all preventable with documented patterns: strong Global sentinel for EPT, nested HandleScopes for temporary operations, and strict thread-local isolate ownership.

---

## Key Findings

### Recommended Stack

The 2025 standard stack for V8-based edge runtimes centers on mature, production-tested components. **rusty_v8** (v147.x) provides stable V8 bindings with pre-built binaries. **tokio + axum** form the async HTTP layer—axum adds minimal overhead while providing type-safe extractors and Tower middleware compatibility. For crypto, the stack uses native Rust crates (**ring**, **p256**, **rsa**) rather than V8's crypto.subtle to avoid C++ complexity and gain Rust's memory safety guarantees.

**Core technologies:**
- **rusty_v8 (v8 = "147")** — V8 JavaScript engine bindings — Zero-overhead C++ API; pre-built binaries eliminate compile time
- **tokio (^1.52)** — Async runtime and I/O — Industry standard; multi-thread scheduler; channel primitives
- **axum (^0.8)** — HTTP server and routing — Ergonomic router; Tower middleware; WebSocket support via tokio-tungstenite
- **ring (^0.17) + p256 + rsa** — WebCrypto implementation — BoringSSL pedigree; avoids V8 crypto complexity
- **virtual-fs (^0.2)** — Virtual filesystem per isolate — SandboxedPhysicalFS for security; std::fs patterns
- **flate2 (^1.1)** — WinterCG CompressionStream — zlib-rs backend; pure Rust; 321M+ downloads
- **tokio-tungstenite (^0.29)** — WebSocket protocol — RFC 6455 compliant; native axum integration

### Expected Features

Edge runtimes converge on the **WinterTC Minimum Common Web API** (ECMA-429, Dec 2025) as the baseline. Missing any table stakes makes the runtime feel broken; differentiators create competitive moats but also lock-in.

**Must have (table stakes):**
- **Fetch API** — Request, Response, Headers, URL, URLSearchParams — 95%+ of edge code uses fetch()
- **Web Crypto** — crypto.subtle, crypto.getRandomValues(), CryptoKey — Auth, hashing, encryption
- **Encoding** — TextEncoder, TextDecoder — Fundamental for string handling
- **Console** — All methods — Debugging without console.log is impossible
- **Timers** — setTimeout, setInterval, clear variants — Event loop integration required
- **AbortController** — Modern cancellation primitive for async operations
- **Streams** — ReadableStream, WritableStream, TransformStream — Required for large body handling
- **WebAssembly** — Global namespace with compile/instantiate — Running WASM at edge is standard

**Should have (competitive):**
- **CompressionStream** — gzip/deflate support — Performance optimization
- **WebSocket Server** — RFC 6455 implementation — Real-time bidirectional communication
- **Virtual File System** — Per-isolate, in-memory — Writable temp storage
- **Full crypto.subtle** — All algorithms (RSA, ECDSA, AES, etc.) — Complete WebCrypto
- **Inter-Isolate Messaging** — BroadcastChannel or equivalent — Cross-request state sharing

**Defer (v2+):**
- **Node.js Compatibility** — fs, path, crypto, stream polyfills — Very high complexity; not essential for launch
- **TCP Socket Outbound** — Deno.connect equivalent — Direct database connectivity
- **URLPattern** — Modern routing — Nice-to-have routing alternative
- **Performance API** — timing APIs — Observability enhancement

### Architecture Approach

The recommended pattern is **WorkerPool + Isolate-per-Thread** with context reset between requests. Each worker thread owns exactly one V8 isolate permanently; between requests, only the context is reset (~5ms) rather than recreating the entire isolate (~50-100ms). This provides both performance (amortized isolate creation cost) and security (fresh context = no state leakage).

**Major components:**
1. **HTTP Server (axum)** — Accept connections, parse requests, virtual host routing by Host header
2. **WorkQueue (tokio mpsc)** — Per-app bounded channels for request dispatch with backpressure
3. **WorkerPool** — Spawn/manage worker threads per app; one thread owns one isolate
4. **V8 Isolate Layer** — Sandboxed JS execution; context per request; extensions bind WinterCG APIs
5. **Extension/Ops Layer** — Rust functions callable from JS; resource table tracks open resources

### Critical Pitfalls

Research identified 12 critical pitfalls, with these top 5 requiring immediate attention:

1. **EPT SIGSEGV from ArrayBuffer Allocation** — Maintain at least one strong `v8::Global<Value>` handle per isolate throughout its lifetime to prevent premature GC of objects with external pointers. Essential fix already identified in PROJECT.md.

2. **Handle Scope Misuse** — Create short-lived nested HandleScopes for temporary operations (compilation, object creation). Never hold temporary handles in long-lived scopes to prevent unbounded memory growth.

3. **Isolate Thread Safety Violations** — V8 isolates are `!Send + !Sync`. Never move isolates between threads; use thread-local isolate pattern. Use `std::sync::Once` to serialize first isolate instantiation.

4. **Blocking V8 Callbacks Deadlocking Tokio** — Never perform blocking I/O inside V8 FunctionCallbackArguments handlers. Use async bridge pattern: store JS callback as `v8::Global`, send work to scheduler via channel, let tokio perform async work, then call stored callback.

5. **Promise Resolution Without Microtask Checkpoint** — Always call `scope.perform_microtask_checkpoint()` after any JS execution that may create promises. Without this, async/await and Promise chains never make progress.

---

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: V8 Foundation
**Rationale:** Core infrastructure must be solid before building on top. EPT fix and platform initialization are prerequisites for everything else.
**Delivers:** rusty_v8 integration, EPT initialization fix, single isolate proof-of-concept
**Addresses:** Platform initialization (from ARCHITECTURE.md)
**Avoids:** EPT SIGSEGV, Handle scope misuse, Isolate thread safety violations
**Research Flag:** LOW — V8 integration is well-documented

### Phase 2: Core Runtime
**Rationale:** WorkerPool and WorkQueue are architectural foundations. Must get context lifecycle correct before adding APIs.
**Delivers:** WorkerPool scaffolding, WorkQueue implementation, context lifecycle (create/execute/reset/dispose)
**Uses:** tokio channels, parking_lot, crossbeam
**Implements:** WorkerPool pattern, Resource Table pattern
**Avoids:** Context reset vs isolate disposal cost miscalculation, External reference table leaks
**Research Flag:** MEDIUM — Multi-isolate memory management needs validation

### Phase 3: Basic WinterCG APIs
**Rationale:** Core web APIs are table stakes. Fetch is the primary interface—everything else extends from it.
**Delivers:** Request/Response/URL/Headers, console API, TextEncoder/Decoder, crypto.getRandomValues, timers, AbortController
**Addresses:** Table stakes features (Fetch, Encoding, Console, Timers from FEATURES.md)
**Avoids:** Blocking V8 callbacks, Promise resolution without microtask checkpoint
**Research Flag:** LOW — Well-documented WinterCG patterns

### Phase 4: I/O and Networking
**Rationale:** Outbound fetch() requires careful async integration with tokio. This is where most threading issues manifest.
**Delivers:** Outbound fetch() via tokio/hyper, basic streaming support
**Uses:** hyper, http, bytes, serde_json
**Implements:** Async Op Pattern
**Avoids:** Blocking V8 callbacks, Promise resolution issues
**Research Flag:** MEDIUM — Async bridge pattern needs careful implementation

### Phase 5: Multi-Tenancy
**Rationale:** Virtual host routing and per-app WorkerPools enable the core product value—hosting multiple isolated apps.
**Delivers:** Virtual host routing (Host → App mapping), per-app WorkerPools, dashmap for routing tables
**Addresses:** Multi-tenancy architecture
**Avoids:** Context reset issues, External reference leaks
**Research Flag:** LOW — Standard HTTP routing patterns

### Phase 6: Advanced APIs
**Rationale:** Differentiating features that add competitive value. These build on core infrastructure.
**Delivers:** Full Streams API, WebAssembly support, CompressionStream, WebSocket server (tokio-tungstenite)
**Addresses:** Differentiator features from FEATURES.md
**Uses:** flate2, tokio-tungstenite
**Avoids:** Stream backpressure mishandling, WebSocket RFC 6455 non-compliance, Snapshot version mismatches
**Research Flag:** HIGH — WebSocket implementation complexity; Stream backpressure handling

### Phase 7: Extended Features
**Rationale:** Nice-to-have features for completeness. Can be deferred if needed.
**Delivers:** Full crypto.subtle (ring integration), Virtual File System (virtual-fs), inter-isolate messaging
**Addresses:** Phase 3 features from FEATURES.md
**Uses:** ring, p256, rsa, virtual-fs
**Avoids:** Crypto timing attacks, VFS path traversal vulnerabilities
**Research Flag:** MEDIUM — Crypto implementation security review needed

### Phase 8: Platform Features (v2)
**Rationale:** Node.js compatibility and TCP sockets are major differentiators but very high complexity.
**Delivers:** Node.js compatibility layer (selective polyfills), TCP socket outbound support
**Addresses:** v2+ features from FEATURES.md
**Avoids:** npm resolution complexity (explicitly out of scope per anti-features)
**Research Flag:** HIGH — Node.js compat scope needs careful definition

### Phase Ordering Rationale

- **Foundation before features:** V8 integration and runtime core must be stable before adding APIs
- **Async I/O before multi-tenancy:** Single-tenant async patterns must work before scaling to multi-tenant
- **Core APIs before differentiators:** WinterCG table stakes are prerequisites for WebSocket, Streams
- **Crypto in phases:** Basic getRandomValues early, full subtle later due to algorithm complexity
- **WebSocket requires Streams:** Full duplex streaming prerequisite for WebSocket implementation

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Core Runtime):** Multi-isolate memory management, external reference cleanup patterns
- **Phase 4 (I/O):** Async bridge pattern, Promise microtask integration details
- **Phase 6 (Advanced APIs):** WebSocket RFC 6455 implementation complexity, stream backpressure handling
- **Phase 7 (Extended):** VFS semantics (pure in-memory vs persisted), crypto algorithm priority
- **Phase 8 (v2):** Node.js compat scope—which APIs are the 80% use case?

Phases with standard patterns (skip research-phase):
- **Phase 1 (Foundation):** V8 integration well-documented, EPT fix already identified
- **Phase 3 (Basic APIs):** WinterCG standard clear, fetch implementation patterns established
- **Phase 5 (Multi-tenancy):** HTTP routing, virtual host patterns are standard

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | rusty_v8 stable since Sept 2024; tokio 1.x stability guarantee; ring audited; all components production-tested |
| Features | HIGH | WinterTC ECMA-429 spec official; Cloudflare/Deno docs authoritative; anti-features well-established |
| Architecture | HIGH | Based on Deno/deno_core official architecture; patterns validated across multiple runtimes |
| Pitfalls | HIGH | All critical pitfalls documented with GitHub issues, ThreadSanitizer output, proven fixes |

**Overall confidence:** HIGH

### Gaps to Address

1. **WebSocket Implementation Details:** RFC 6455 parsing complexity; handshake validation; frame masking/unmasking. Address during Phase 6 planning—may need proof-of-concept.

2. **Crypto Algorithm Priority:** Which crypto.subtle algorithms are actually used in production? Likely ECDSA P-256, RSA-PSS, AES-GCM, SHA-256—but need validation during Phase 7.

3. **VFS Semantics Decision:** Should VFS be pure in-memory (reset on context disposal), persisted across requests, or backed by actual filesystem with sandbox? Decision needed in Phase 7.

4. **Node.js Compat Scope:** If pursuing Phase 8, must define which Node.js APIs are essential (likely: Buffer, stream, crypto, path, fs.promises). Scope definition needed before planning.

5. **Performance Benchmarks:** Context reset latency (5ms claim) should be validated in target environment during Phase 2.

---

## Sources

### Primary (HIGH confidence)
- **WinterTC Minimum Common Web API Specification (ECMA-429)** — https://min-common-api.proposal.wintertc.org/ — Authoritative table stakes requirements
- **rusty_v8 releases** — https://github.com/denoland/rusty_v8/releases — v147.3.0 API stability
- **deno_core ARCHITECTURE.md** — https://github.com/denoland/deno_core/blob/main/ARCHITECTURE.md — Component patterns
- **Deno Architecture** — https://docs.deno.com/runtime/contributing/architecture/ — Runtime design patterns
- **Cloudflare Workers Runtime APIs** — https://developers.cloudflare.com/workers/runtime-apis/ — Production reference
- **ring crypto** — https://docs.rs/ring/0.17/ring/ — Security-audited primitives

### Secondary (MEDIUM-HIGH confidence)
- **axum docs** — https://docs.rs/axum/0.8.9/axum/ — HTTP server patterns
- **tokio docs** — https://docs.rs/tokio/1.52.1/tokio/ — Async runtime patterns
- **rusty_v8 GitHub Issues** — #1467 (thread safety), #1348 (memory leaks), #481 (handle scopes) — Implementation pitfalls
- **Deno Deploy Runtime API** — https://docs.deno.com/deploy/reference/runtime/ — Feature comparison

### Tertiary (contextual)
- **Cloudflare Node.js Compatibility** — https://developers.cloudflare.com/workers/runtime-apis/nodejs/ — Node.js compat scope reference
- **flate2/zlib-rs** — https://docs.rs/flate2/1.1/flate2/ — Compression implementation
- **virtual-fs** — https://docs.rs/virtual-filesystem/0.2/ — VFS patterns

---

*Research completed: 2026-04-19*  
*Files synthesized: STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md*  
*Ready for roadmap: YES*
