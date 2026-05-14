⏺ NANO — Technical Product Document

  Edge Runtime: One Process, Many Isolated Apps

  ---
  What It Is

  NANO is a single-process HTTP server that hosts multiple JavaScript applications in parallel, each in its own V8 isolate, with zero container overhead. The motivating premise:
  a container fleet running one Node.js app per pod is operationally expensive, slow to start, and wasteful. NANO replaces the entire fleet with one binary and a config file.

  Core value: Skip the container fleet entirely — one OS process hosts many isolated JS apps, each with their own heap, globals, crypto keys, and VFS. Cold start is measured in
  milliseconds, not seconds.

  ---
  Architecture

  ┌────────────────────────────────────────────────────┐
  │  nano (single OS process)                          │
  │                                                    │
  │  TCP accept loop (main thread)                     │
  │    │                                               │
  │    ├── Host: app-a.example.com ──► WorkerPool A   │
  │    │                                 ├── Isolate 0 │
  │    │                                 ├── Isolate 1 │
  │    │                                 └── Isolate N │
  │    │                                               │
  │    └── Host: app-b.example.com ──► WorkerPool B   │
  │                                      ├── Isolate 0 │
  │                                      └── Isolate N │
  └────────────────────────────────────────────────────┘

  Request path: TCP accept → virtual host lookup (exact hostname match) → WorkQueue dispatch → worker thread enters isolate, calls JS fetch() handler → HTTP response written →
  context reset for next request.

  Thread model: One worker thread per isolate. Workers are persistent (not spawned per-request). Each worker owns its isolate for its lifetime. V8 isolates are NOT thread-safe —
  strict one-isolate-per-thread enforced.

  Context reset (PERF-03): After each request, the V8 context is disposed and recreated on the same isolate. All 20+ native API callbacks are re-registered. This resets JS global
   state without the overhead of a new isolate. Cost: ~5ms vs ~50-100ms for a cold isolate.

  ---
  Feature Inventory (Shipped)

  v1.0 — MVP

  - Single-app HTTP server, one V8 isolate
  - fetch() handler interface (Edge Workers style)
  - Request / Response objects (WinterTC)
  - console.log/warn/error
  - TextEncoder / TextDecoder
  - URL / URLSearchParams
  - Headers
  - setTimeout / setInterval / clearTimeout / clearInterval
  - crypto.getRandomValues()
  - crypto.subtle.generateKey, importKey, exportKey
  - crypto.subtle.encrypt / decrypt (AES-GCM)
  - crypto.subtle.sign / verify (HMAC, ECDSA)
  - FormData, Blob
  - structuredClone()
  - DOMException
  - AbortController / AbortSignal
  - fetch() (outbound HTTP, async via libxev + std.Thread pool)
  - performance.now()
  - ReadableStream / WritableStream / TransformStream
  - IIFE wrapper transforms export default for ESM compatibility

  v1.1 — Multi-App Hosting

  - Virtual host routing: Host header → app mapping
  - Multiple apps from single config JSON
  - Per-app timeout, memory limit, env vars, tool definitions
  - Hot-reload via config file watcher (poll-based, 2s interval)
  - Graceful drain: in-flight requests complete before swap

  v1.2 — Production Polish

  - Structured JSON logging (ts, level, event)
  - Metrics endpoint (/_admin/metrics): request counts, latency, error rates
  - Watchdog timer: terminates hung requests at configured timeout
  - Resource monitor: OOM detection, heap limit enforcement
  - Graceful shutdown: SIGTERM/SIGINT drain + stop

  v1.3 — Backlog Cleanup

  - VFS (Virtual Filesystem): per-isolate, in-memory KV store accessible from JS (Nano.fs.*)
  - Agent tools API: structured tool definitions callable from JS
  - Inter-isolate messaging: sendMessage() / addEventListener('message') across apps in same process
  - IsolateRegistry: named registry for cross-isolate addressing

  v1.4 — Performance & Agent Platform

  - WorkerPool: N worker threads per app, each with own isolate (replaces single-isolate mode)
  - WorkQueue: bounded MPSC queue, 256-slot capacity, blocking pop
  - structuredClone() for cross-isolate message serialization (JSON over isolate boundary)
  - Agent VFS: persistent KV accessible to AI agent tool calls
  - Resource governance: per-request memory accounting
  - performance.timeOrigin, performance.measure()

  v1.5-01 — Multi-App WorkerPool

  - WorkerPool per virtual host (not shared across apps)
  - Affine dispatch: hostname → pool index → queue
  - Isolate deinit serialization mutex (prevents concurrent Isolate::Deinit() EPT race)

  v1.5-02 — WebSocket & Events

  - WebSocket server: RFC 6455 framing, ping/pong, fragmentation
  - WebSocketPair API ([WinterTC](https://wintertc.org/) Workers compatible)
  - EventTarget / Event / CustomEvent polyfill (V8 global)
  - WS framing loop runs on isolate-owning worker thread (no cross-thread V8 access)
  - LowMemoryNotification() in deinit to complete incremental GC before isolate teardown

  v1.5-03 — Compression & RSA-PSS (in progress)

  - CompressionStream / DecompressionStream (gzip, deflate) via std.compress.flate
  - crypto.subtle.sign/verify RSA-PSS via std.crypto.ff.Modulus (constant-time)
  - RSA-PSS keygen using std.math.big.int for p*q, gcd, modInverse
  - Status: implementations complete in worktrees, blocked by AP-02 (EPT SIGSEGV)

  v1.5-04 — V8 Snapshot Hardening & CI (planned)

  - Embedded startup snapshot for fast isolate init (2-5ms vs 50-100ms)
  - External references audit (all native callback pointers registered for snapshot)
  - Integration test harness

  ---
  WinterTC Compliance

  NANO targets the WinterTC Minimum Common API:

  ┌───────────────────────────┬─────────┬─────────────────────────────────────────────┐
  │            API            │ Status  │                    Notes                    │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ fetch()                   │ ✅      │ Outbound HTTP, async                        │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ Request / Response        │ ✅      │ Body streaming partial                      │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ Headers                   │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ URL / URLSearchParams     │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ TextEncoder / TextDecoder │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ console                   │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ crypto.getRandomValues    │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ crypto.subtle             │ ✅      │ AES-GCM, HMAC, ECDSA, RSA-PSS (in progress) │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ AbortController           │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ performance               │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ ReadableStream            │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ WritableStream            │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ TransformStream           │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ CompressionStream         │ 🔧      │ In progress                                 │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ structuredClone           │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ FormData                  │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ Blob                      │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ DOMException              │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ EventTarget               │ ✅      │                                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ WebSocket (server)        │ ✅      │ Non-standard, [WinterTC](https://wintertc.org/) Workers style      │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ queueMicrotask            │ ❌      │ Not implemented                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ atob / btoa               │ ❌      │ Not implemented                             │
  ├───────────────────────────┼─────────┼─────────────────────────────────────────────┤
  │ Streams piping            │ Partial │ pipeTo works, tee missing                   │
  └───────────────────────────┴─────────┴─────────────────────────────────────────────┘

  ---
  Competitive Analysis

  vs. Common Edge Workers (production target)

  Advantages of NANO:
  - Self-hosted: no vendor lock-in, no per-request pricing
  - No 128MB isolate memory cap (configurable up to system RAM)
  - No CPU time limits
  - No cold start tax (workers are persistent, not per-request)
  - Local development is identical to production

  Disadvantages:
  - No global edge network
  - No KV/Durable Objects at platform level
  - No CDN integration
  - Single machine (no horizontal scale without external load balancer)

  vs. Deno / Node.js (single-app servers)

  Advantages of NANO:
  - Multiple isolated apps per process — one binary replaces a container fleet
  - Per-app resource limits (memory, timeout) enforced at runtime, not OS level
  - Zero container overhead: no Docker, no Kubernetes per-app
  - Millisecond cold start (no Node.js module graph resolution)
  - Strong isolation: each app has its own heap, globals, VFS — no shared state leaks
  - Predictable latency: no GC pauses shared across apps

  Disadvantages:
  - No npm ecosystem (no require/import resolution)
  - No native module support
  - No filesystem access (VFS is in-memory only)
  - No subprocess spawning
  - Smaller stdlib than Deno

  vs. Bun

  Advantages of NANO:
  - Multi-app isolation in a single process (Bun is single-app)
  - Lower memory per app (isolate is lighter than a full Bun process)
  - Deterministic resource limits per app

  Disadvantages:
  - No JSX/TypeScript transpilation
  - No package manager
  - Bun's V8-replacement (JavaScriptCore) is faster for single-app workloads
  - Bun has more complete WinterTC coverage

  vs. Vercel Edge Runtime / Next.js Edge

  Advantages of NANO:
  - Self-hosted, no framework dependency
  - Any JS that fits the WinterTC surface runs
  - No 4MB bundle size limit

  Disadvantages:
  - No framework integration
  - No streaming SSR helpers
  - No Next.js/React built-ins

  ---
  Pros (Strengths)

  1. Density: 10 apps × 10 worker threads = 100 isolates in one process. Each isolate ~10-20MB heap vs ~50-200MB per Docker container.
  2. Cold start: ~50-100ms cold (V8 init) → ~2-5ms with snapshot. 
  3. Isolation: V8 isolates are security boundaries. No shared heap. No shared globals. Cryptographic keys don't leak between apps.
  4. WinterTC portable: Apps written to WinterTC spec run on Edge Workers, Deno Deploy, and NANO without changes.
  5. Operational simplicity: One binary, one config file, no container orchestration.
  6. Resource governance: Per-app memory limits and timeouts enforced at the isolate level.
  7. WebSocket density: Many concurrent WS connections across many apps, all in one process.

  ---
  Cons (Weaknesses)

  1. Build time: Zig + V8 = ~2 hours for a clean build on macOS ARM. Incremental: ~10 seconds, but .zig-cache corruption requires full rebuild. Debugging V8 internals from Zig is
   painful (no LLDB V8 pretty-printers, no V8 source map).
  2. V8 binding fragility: Every V8 upgrade risks API breakage at the C binding layer. No automatic binding generation — hand-written C wrappers in binding.cpp. Memory layout
  assumptions (EPT space structure, segment sizes) are implicit.
  3. Debugging V8 crashes: EPT crashes, DCHECK failures, incremental GC races — all require deep V8 source knowledge to diagnose. LLDB shows raw V8 internals with no symbolic
  type information for template classes.
  4. Feature lag: WinterTC additions (e.g., atob, queueMicrotask, ReadableStream.tee) require manual implementation against V8 C API. No automatic platform updates.
  5. No npm: Apps are single-file index.js. Bundling is the user's responsibility. No import resolution.
  6. Zig instability: Zig is pre-1.0. API breaks between minor versions. std.math.big.int.Managed breaking changes (v1.5-03 spent significant time on API misuse from version
  drift).
  7. Single-machine: No built-in clustering or replication. Horizontal scale requires external load balancer + multiple NANO instances.
  8. V8 EPT complexity: The ExternalPointerTable (EPT) has two separate Spaces per isolate (external_pointer_space for v8::External, array_buffer_sweeper_space for
  JSArrayBuffer). Getting both right requires knowledge of V8 internals not documented publicly.

  ---
  Known Hard Problems (Engineering Debt)

  ┌────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────────────────┐
  │              Problem               │                                         Root cause                                          │                Status                 │
  ├────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────┤
  │ AP-02: SIGSEGV on ArrayBuffer      │ array_buffer_sweeper_space EPT segment unmapped by background GC between isolate.exit() and │ Root cause found, fix designed        │
  │ alloc in serve path                │  isolate.enter()                                                                            │ (strong Persistent handle)            │
  ├────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────┤
  │ Compression streams blocked by     │ Same serve path                                                                             │ Waiting on AP-02 fix                  │
  │ AP-02                              │                                                                                             │                                       │
  ├────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────┤
  │ No V8 startup snapshot             │ In-process snapshot requires all isolates share read-only heap; current architecture        │ Deferred to v1.5-04                   │
  │                                    │ creates isolates in worker threads post-platform-init                                       │                                       │
  ├────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────┤
  │ ReadableStream tee                 │ Not implemented                                                                             │ Backlog                               │
  ├────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────┤
  │ V8 rebuild on cache clear          │ .zig-cache clear = 2-hour V8 rebuild                                                        │ Mitigated: never clear unless         │
  │                                    │                                                                                             │ confirmed stale binary                │
  └────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────┴───────────────────────────────────────┘

  ---
  Migration Analysis: Zig → Rust + rusty_v8

  Why Migrate

  1. Build time: Rust + rusty_v8 uses pre-built V8 binaries from Deno's CI. No 2-hour V8 compile on clean build. cargo build downloads a pre-built librusty_v8.a.
  2. rusty_v8 is maintained by Deno: The binding layer is continuously updated with V8 upgrades. NANO's hand-written C bindings (binding.cpp) are a maintenance liability.
  3. Type safety: Rust's borrow checker enforces V8 handle scope semantics at compile time. V8 HandleScope is modeled as a lifetime in rusty_v8 — you cannot hold a v8::Local<T>
  past its scope without a compile error.
  4. Ecosystem: tokio for async, hyper or axum for HTTP, rustls for TLS. All production-grade, well-maintained.
  5. LLDB/gdb integration: Rust has richer debug symbols. V8 crashes are easier to map to Rust call sites.
  6. Stability: Rust 1.x is stable. No API breaks between minor versions.

  rusty_v8 Capability Map

  ┌──────────────────┬───────────────────────────────────────────┬─────────────────────────────────────────┐
  │   NANO feature   │            rusty_v8 equivalent            │                  Notes                  │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ Isolate::New     │ v8::Isolate::new()                        │ Same semantics                          │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ HandleScope      │ v8::HandleScope<'s>                       │ Lifetime-enforced                       │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ Context          │ v8::Context                               │ Same                                    │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ FunctionTemplate │ v8::FunctionTemplate                      │ Same                                    │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ Persistent<T>    │ v8::Global<T>                             │ Named differently but same concept      │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ External::New    │ v8::External::new()                       │ Same                                    │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ ArrayBuffer::New │ v8::ArrayBuffer::new_with_backing_store() │ Explicit backing store                  │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ Platform init    │ v8::Platform via rusty_v8                 │ Must call v8::V8::initialize_platform() │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ EPT dummy init   │ Same issue exists                         │ rusty_v8 exposes same underlying V8     │
  ├──────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────┤
  │ Custom allocator │ v8::Allocator trait                       │ Rust trait, cleaner than C vtable       │
  └──────────────────┴───────────────────────────────────────────┴─────────────────────────────────────────┘

  What rusty_v8 Does NOT Solve

  - EPT Space initialization bug (AP-02): This is a V8 internal issue, not a binding issue. rusty_v8 uses the same V8 underneath. The array_buffer_sweeper_space vs
  external_pointer_space distinction still exists. The fix (strong v8::Global<Value> for dummy ArrayBuffer) is the same concept in Rust.
  - V8 background GC: Same 9 DefaultWorker threads, same behavior.
  - WinterTC APIs: Must still be implemented by hand. rusty_v8 gives you V8 primitives, not WinterTC.
  - ArrayBuffer backing store semantics: Same V8 behavior, different Rust API.

  Migration Risk Map

  ┌─────────────────────────────────────────────────┬──────────┬────────────────────────────────────────────────────────────────────────────────────┐
  │                      Risk                       │ Severity │                                     Mitigation                                     │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ rusty_v8 API surface narrower than raw V8 C API │ Medium   │ Some C API calls not exposed; can add via raw unsafe FFI or contribute to rusty_v8 │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ WinterTC re-implementation effort               │ High     │ ~3-4 months. All 20+ APIs must be rewritten. Existing Zig code is not portable.    │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ WorkerPool thread model                         │ Low      │ std::thread + tokio::sync::mpsc maps directly to current design                    │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ Context reset pattern                           │ Low      │ Same approach: dispose context, create new, re-register callbacks                  │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ VFS / inter-isolate messaging                   │ Medium   │ Current Zig implementation is clean; Rust translation is straightforward           │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ EPT initialization (AP-02 class bugs)           │ Medium   │ Same underlying V8 behavior; same fix required                                     │
  ├─────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ V8 upgrade cadence                              │ Low      │ rusty_v8 tracks V8 automatically; Deno ships updates                               │
  └─────────────────────────────────────────────────┴──────────┴────────────────────────────────────────────────────────────────────────────────────┘

  Recommended Migration Path

  Phase 1 — Skeleton (2 weeks)
  - Rust project with rusty_v8
  - Platform init, single isolate, fetch() handler, HTTP server (hyper/axum)
  - No WinterTC APIs yet — just "hello world" from JS

  Phase 2 — Core WinterTC (6 weeks)
  - Request / Response / Headers / URL
  - TextEncoder / TextDecoder
  - console
  - crypto.getRandomValues()
  - fetch() outbound (tokio-based)

  Phase 3 — WorkerPool (2 weeks)
  - N worker threads, each owning one isolate
  - WorkQueue (tokio channel or crossbeam)
  - Virtual host routing
  - Context reset between requests

  Phase 4 — Extended WinterTC (4 weeks)
  - Streams (ReadableStream, WritableStream, TransformStream)
  - crypto.subtle (AES-GCM, ECDSA, RSA-PSS) — reuse Rust crypto crates (ring, p256, rsa)
  - CompressionStream / DecompressionStream (flate2 crate)
  - WebSocket server

  Phase 5 — Parity + EPT Fix (2 weeks)
  - VFS, inter-isolate messaging
  - EPT Space initialization (strong v8::Global<Value> sentinel per isolate)
  - V8 startup snapshot
  - Integration test parity

  Total estimate: ~4 months to functional parity. The biggest cost is re-implementing 20+ WinterTC APIs, not the V8 binding layer itself.

  Critical Decision: Rust Crypto vs V8 Crypto

  In Zig, RSA-PSS required implementing modular arithmetic from scratch because Zig's stdlib lacks RSA signing primitives. In Rust:

  - rsa crate: full RSA sign/verify with PSS padding, constant-time
  - ring crate: AES-GCM, ECDSA, HMAC, SHA — all hardware-accelerated
  - p256 crate: ECDSA over P-256

  Recommendation: bypass crypto.subtle V8 C++ entirely. Implement all crypto operations in Rust using ring + rsa + p256. Wire them to V8 callbacks. This avoids all the V8
  internal crypto complexity and is safer.

  ---
  Summary Judgment

  NANO as a concept is sound. The density proposition (one process, many isolated apps, millisecond cold start, WinterTC portable) has genuine value that no existing runtime
  delivers. Edge Workers delivers this, but only as a vendor-locked hosted service.

  The Zig implementation is a liability:
  - Build toolchain is fragile and slow
  - V8 binding layer is hand-maintained
  - Zig stdlib instability adds maintenance tax
  - V8 crash debugging from Zig is expensive (days per EPT bug, as evidenced)

  The Rust + rusty_v8 path resolves all three root causes (build time, binding maintenance, stdlib stability) while preserving the architecture entirely. The WinterTC
  re-implementation is the actual cost — 3-4 months of engineering — but it buys a platform that can be maintained sustainably.

  The EPT initialization complexity (AP-02 class bugs) will follow you to Rust. It is a V8 internal behavior, not a Zig problem. Document the fix pattern (strong
  v8::Global<Value> sentinel per isolate, one for external_pointer_space via v8::External, one for array_buffer_sweeper_space via v8::ArrayBuffer) and apply it on day 1 of the
  Rust implementation.
  

  ### old briefing, disconsider the zig specifics

  # NANO: Ultra-Dense JavaScript Isolate Runtime

## Product Description

NANO is a high-performance JavaScript runtime built for extreme density and sub-5ms cold start times. Written in Zig with embedded V8, NANO enables running thousands of lightweight JavaScript “nanoservices” in a single process—achieving 10x better resource efficiency than traditional Node.js deployments and outperforming Deno’s isolate model through manual memory control. The goal of this runtime is to optimize for hosting, to run more than one process and to be simple and nimble, not like a J2EE serv, way less bureaucractic and straight to the point. Think about a browser and its tabs.

**Target Market:** Platform engineers and infrastructure teams running multi-tenant JavaScript workloads (API gateways, edge functions, webhook processors, serverless platforms) who need Edge Workers-like performance without vendor lock-in.

**Key Differentiators:**

- **Sub-5ms cold starts** via V8 startup snapshots
- **<2MB memory overhead per isolate** vs 30MB+ for Node.js processes
- **Zero-copy I/O** using Zig’s direct epoll/io_uring integration
- **Arena-based memory management** eliminating GC pauses
- **Native Brazilian market support** (integrates with mcp-osv security scanning)


