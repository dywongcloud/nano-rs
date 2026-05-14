# Domain Pitfalls: V8-Based Edge Runtime

**Domain:** V8-based edge JavaScript runtime (Rust + rusty_v8 + tokio)
**Researched:** 2026-04-19
**Overall confidence:** HIGH

## Critical Pitfalls

Mistakes that cause rewrites, SIGSEGV crashes, or memory corruption.

### Pitfall 1: EPT SIGSEGV from ArrayBuffer Allocation in Request Path
**What goes wrong:**
ArrayBuffer allocations in the HTTP request serving path trigger SIGSEGV crashes due to External Pointer Table (EPT) lifecycle issues. When ArrayBuffers are allocated during request handling and the context is quickly disposed, the EPT entries may become dangling pointers, causing use-after-free or SIGSEGV when V8 attempts to mark/sweep the external pointer table during GC.

**Why it happens:**
- V8's External Pointer Table (EPT) manages pointers to objects outside the V8 heap (like ArrayBuffer backing stores)
- When the sandbox is disabled but pointer compression is enabled, ArrayBuffer extensions still get EPT entries
- If the external resource (ArrayBuffer backing store) is destroyed before the EPT entry is freed, the EPT entry contains a dangling pointer
- In request-serving paths where contexts are rapidly created and disposed, this race condition manifests as crashes

**Consequences:**
- Random SIGSEGV crashes in production under load
- Memory corruption that may not immediately crash but corrupts data
- Extremely difficult to debug due to async GC timing

**Prevention:**
- **Strong Global Sentinel Pattern:** Maintain at least one strong `v8::Global<Value>` handle per isolate that lives throughout the isolate's lifetime
- This prevents premature GC of objects with external pointers
- The PROJECT.md already identifies this fix: "EPT initialization fix (strong v8::Global sentinel)"
- Ensure ArrayBuffer allocators outlive any context that uses them

**Detection:**
- ThreadSanitizer warnings about data races in isolate creation/destruction
- Crash logs showing `v8::internal::ExternalPointerTable` or `v8::internal::Heap::AllocateRaw` in stack traces
- Intermittent crashes under load testing with ArrayBuffer-heavy workloads

**Phase to address:** Phase 1 (Platform + Single Isolate) — Must be fixed before any request handling

---

### Pitfall 2: Handle Scope Misuse Causing Memory Leaks
**What goes wrong:**
All Local handles allocated in a HandleScope are only freed when the HandleScope is destroyed. Creating long-lived HandleScopes (like per-isolate scopes) while repeatedly compiling scripts or creating objects causes unbounded memory growth until OOM.

**Why it happens:**
- HandleScopes are GC root containers
- Rooted values aren't garbage collected until the container goes out of scope
- V8 APIs require HandleScopes, but their lifetime determines GC visibility

**Consequences:**
- Memory grows unbounded (observed 2GB+ in minutes in rusty_v8 issue #481)
- OOM crashes in long-running services
- REPL and script compilation workflows leak memory

**Prevention:**
- **Nested HandleScope Pattern:** Create short-lived nested HandleScopes for temporary operations
```rust
{
    let scope = &mut v8::HandleScope::new(parent_scope);
    let script = v8::Script::compile(scope, code, None).unwrap();
    // script and other temporaries freed here when scope drops
}
```
- Never hold temporary handles in long-lived scopes
- Scope per compilation, per request, or per logical operation

**Detection:**
- Memory profiling showing growth in V8 heap or external memory
- DHAT/valgrind showing unfreed allocations from `v8::internal::HandleScope::CreateHandle`
- OOM in long-running services that create/destroy many scripts/contexts

**Phase to address:** Phase 1 (Core runtime setup)

---

### Pitfall 3: Isolate Thread Safety Violations
**What goes wrong:**
V8 isolates are not thread-safe (`!Send + !Sync`). Moving isolates between threads, creating isolates concurrently without proper synchronization, or accessing an isolate from multiple threads causes data races and crashes.

**Why it happens:**
- rusty_v8's `Isolate` is marked `Send` but this is unsound (issue #1467)
- ThreadSanitizer detects data races when isolates are created/dropped concurrently
- V8 isolates have thread-local state that must be entered/exited properly
- rusty_v8 note: "Unlike in the C++ API, the Isolate is entered when it is constructed and exited when dropped"

**Consequences:**
- ThreadSanitizer warnings (data races in `g_current_isolate`)
- SIGSEGV in `v8::internal::Isolate::Exit()` with "CurrentPerIsolateThreadData()->isolate_ == this" assertion failure
- Silent memory corruption

**Prevention:**
- **One Isolate Per Thread:** Never send isolates between threads
- **Sequential Isolate Creation:** Use `std::sync::Once` or `OnceLock` to serialize first isolate instantiation
- **Thread-Local Isolate Pattern:** Each thread manages its own isolate(s) independently
- **Use IsolateHandle for Termination:** `IsolateHandle` is thread-safe and designed for cross-thread termination signals only

**Detection:**
- `RUSTFLAGS=-Zsanitizer=thread cargo +nightly run` shows data races
- Crashes in isolate creation/destruction with multi-threaded test cases
- Assertion failures in `v8::internal::Isolate::Exit()`

**Phase to address:** Phase 2 (WorkerPool) — Critical for multi-worker architecture

---

### Pitfall 4: Blocking V8 Callbacks Deadlocking Tokio
**What goes wrong:**
Performing blocking I/O or long-running operations inside V8 FunctionCallbackArguments handlers blocks the V8 thread and can deadlock tokio's event loop.

**Why it happens:**
- V8 is single-threaded and synchronous
- Function callbacks execute synchronously on V8's thread
- Blocking operations prevent returning to the event loop
- tokio's `spawn_blocking` and async runtime expect non-blocking execution

**Consequences:**
- "Cannot drop a runtime in a context where blocking is not allowed" panics
- Deadlocks when async operations depend on V8 callbacks
- Event loop starvation

**Prevention:**
- **Async Bridge Pattern:** Use message passing between V8 and tokio
```rust
// 1. In callback: store JS callback as v8::Global, send work to scheduler
fn fetch_binding(scope: &mut v8::HandleScope, args: v8::FunctionCallbackArguments) {
    let callback = v8::Global::new(scope, args.get(0)); // Store for later
    scheduler_tx.send(SchedulerMessage::Fetch(callback_id, url)); // Non-blocking
}

// 2. Tokio performs async work

// 3. Event loop polls channel, calls stored JS callback, pumps microtasks
scope.perform_microtask_checkpoint(); // Essential!
```
- Never use `reqwest::blocking` or file I/O inside V8 callbacks
- Store callbacks as `v8::Global` to survive beyond HandleScope
- Always call `perform_microtask_checkpoint()` after invoking JS callbacks

**Detection:**
- "Cannot drop a runtime" panics at runtime
- Timeouts and hangs in async operations
- Event loop not making progress

**Phase to address:** Phase 3 (fetch() implementation)

---

### Pitfall 5: Promise Resolution Without Microtask Checkpoint
**What goes wrong:**
JavaScript Promises never resolve because microtasks aren't being pumped. `Promise.then()` chains hang indefinitely.

**Why it happens:**
- Promise resolution happens via microtasks
- V8's `perform_microtask_checkpoint()` must be called to execute microtasks
- Without this, async/await and Promise chains never make progress

**Consequences:**
- Hanging async operations that never complete
- Timeouts in what should be fast operations
- Event loop appears to stall

**Prevention:**
- **Always Pump Microtasks:** After any JS execution that may create promises:
```rust
scope.perform_microtask_checkpoint();
```
- Integrate microtask pumping into the event loop
- Consider using `MicrotasksPolicy::kExplicit` for explicit control

**Detection:**
- Promises that never resolve (even simple `Promise.resolve()`)
- Event loop iterations not progressing async work
- Works when manually calling `perform_microtask_checkpoint()`

**Phase to address:** Phase 2 (HTTP server integration)

---

### Pitfall 6: Persistent Handle Leaks (v8::Global Misuse)
**What goes wrong:**
`v8::Global` handles accumulate and pin objects on the V8 heap permanently, preventing GC and causing memory growth.

**Why it happens:**
- `v8::Global` is a persistent handle independent of HandleScope
- Globals remain valid until explicitly dropped
- Converting to raw with `into_raw()` without `from_raw()` loses the handle
- Used for callbacks, templates, and cached objects that outlive scopes

**Consequences:**
- V8 heap grows unbounded
- Objects never garbage collected
- Memory pressure leading to OOM

**Prevention:**
- **Explicit Global Cleanup:** Drop `v8::Global` handles when no longer needed
- **RAII Pattern:** Use Rust's Drop to manage global lifetimes
- **Never Leak Raw Pointers:** If using `into_raw()`, must use `from_raw()` to reconstruct
```rust
// Safe pattern
let global = v8::Global::new(scope, local_value);
// ... use global ...
drop(global); // Explicitly drop when done
```

**Detection:**
- V8 heap statistics showing growth despite low object count
- Heap snapshots showing many persistent handles
- Memory leak detectors (DHAT) showing unfreed allocations

**Phase to address:** Phase 2+ (All phases using persistent storage)

---

### Pitfall 7: Context Reset vs Isolate Disposal Cost Miscalculation
**What goes wrong:**
Choosing the wrong isolation strategy for request handling causes unacceptable latency (5ms vs 50-100ms) or insufficient isolation between requests.

**Why it happens:**
- Context reset: ~5ms (dispose context + recreate context in same isolate)
- New isolate per request: ~50-100ms (isolate creation + context creation)
- But context reset shares the isolate heap between requests
- May not provide sufficient security isolation for multi-tenant scenarios

**Consequences:**
- Unacceptable cold start latency if using new isolate per request
- Security breaches if context reset provides insufficient isolation
- Confusion about which approach to use

**Prevention:**
- **Context Reset for Warm Isolates:** Per PROJECT.md: "Context reset (not new isolate per request) - 5ms vs 50-100ms context disposal cost"
- Use context reset for same-tenant sequential requests
- Use new isolates only for true multi-tenant isolation or security boundaries
- Measure actual latency in your environment

**Detection:**
- Cold start latency measurements
- Security audits showing cross-request data leakage
- Performance profiling showing isolate creation overhead

**Phase to address:** Phase 2 (Virtual host routing) — Architecture decision

---

### Pitfall 8: External Reference Table Leaks in Multi-Isolate Scenarios
**What goes wrong:**
When creating and destroying many isolates (e.g., in a worker pool), memory leaks from external references and EPT entries accumulate.

**Why it happens:**
- deno_core's `JsRuntime` was originally designed for single-isolate use
- ExternalReferences and OpCtx leak when isolates are dropped (rusty_v8 issue #1348)
- EPT entries for ArrayBuffer backing stores not freed properly
- V8 internal resources not fully cleaned up on isolate disposal

**Consequences:**
- Long-running services leak memory over time
- V8's external memory grows unbounded
- Eventually OOM even with stable request load

**Prevention:**
- **Explicit Cleanup:** Manually clear external references and slots before isolate disposal
- **Use Isolate Slots:** Track external resources with `Isolate::get_slot`/`set_slot` for cleanup
- **Pool Isolates:** Reuse isolates instead of creating/destroying them
- **Monitor External Memory:** Check `HeapStatistics::external_memory()`

**Detection:**
- Valgrind/DHAT showing leaks in V8 allocation functions
- Growing external memory in heap statistics
- Memory growth over time in long-running tests

**Phase to address:** Phase 2 (WorkerPool)

---

### Pitfall 9: WinterTC fetch() Implementation Gaps
**What goes wrong:**
Implementing `fetch()` incorrectly for server-side runtimes causes compatibility issues with web standards, unexpected CORS behavior, or missing features like duplex streaming.

**Why it happens:**
- WinterTC documents divergence from browser fetch() for server-side
- Server runtimes don't have origins or cookie jars like browsers
- CORS is irrelevant on servers but browsers enforce it
- Full duplex HTTP streams are expected by modern frameworks

**Consequences:**
- Libraries fail on the edge runtime that work in browsers
- Unexpected security behavior (CORS errors in server code)
- Missing streaming support breaks large request/response handling

**Prevention:**
- **Follow WinterTC Fetch Subset:** Implement the documented server-side subset
- Skip CORS enforcement
- Support manual redirect handling
- Implement full duplex streaming
- Use `Headers`, `Request`, `Response` from WinterTC minimum common API

**Detection:**
- Compatibility test failures with isomorphic libraries
- fetch() behavior differences from Cloudflare Workers/Deno
- Issues with streaming requests/responses

**Phase to address:** Phase 3 (Core WinterTC APIs)

---

### Pitfall 10: Snapshot Version Mismatches and Corruption
**What goes wrong:**
V8 startup snapshots fail to load or cause crashes due to version mismatches, platform differences, or corrupted snapshot data.

**Why it happens:**
- Snapshots are tied to specific V8 versions (checksum verification)
- Version mismatch between snapshot and V8 binary causes FATAL error
- Snapshots may contain platform-specific data (timestamps, memory addresses)
- Reproducibility issues across builds

**Consequences:**
- "Version mismatch between V8 binary and snapshot" FATAL errors
- Deserialization crashes
- Startup failures in production

**Prevention:**
- **Version-Lock Snapshots:** Regenerate snapshots on every V8 version bump
- Use `Snapshot::VersionIsValid()` and `VerifyChecksum()` before use
- Build snapshots as part of the build process, not at runtime
- Consider reproducible snapshot generation with `--random_seed` and `--predictable`
- Externalize non-deterministic data from snapshots

**Detection:**
- FATAL errors at startup mentioning version mismatch
- Crash in `Snapshot::Initialize()` or deserializers
- Checksum verification failures

**Phase to address:** Phase 5 (Performance optimization)

---

### Pitfall 11: WebSocket Protocol Implementation Errors
**What goes wrong:**
WebSocket implementation fails to properly handle the RFC 6455 handshake, framing, or control frames, causing connection failures or security vulnerabilities.

**Why it happens:**
- WebSocket upgrade requires specific HTTP headers (`Upgrade: websocket`, `Connection: Upgrade`)
- Sec-WebSocket-Accept requires SHA1 hashing of key with magic string
- Masking is required client→server but not server→client
- Control frames (ping/pong/close) can appear mid-message
- Frame fragmentation must be handled correctly

**Consequences:**
- Connection failures with standard WebSocket clients
- Security vulnerabilities (not validating origin, improper masking)
- Protocol errors causing connection drops

**Prevention:**
- **Use Established Libraries:** Prefer `tokio-tungstenite` or similar for WebSocket handling
- **Validate Handshake:** Check all required headers, compute Sec-WebSocket-Accept correctly
- **Handle Control Frames:** Properly respond to ping with pong, handle close frames
- **Respect Masking:** Unmask client frames, don't mask server frames
- **Buffer Management:** Handle fragmented messages and large frames

**Detection:**
- WebSocket connection failures in browser dev tools
- Protocol errors in Wireshark/tcpdump traces
- Autobahn test suite failures

**Phase to address:** Phase 5 (WebSocket server)

---

### Pitfall 12: Crypto.subtle Implementation Security Issues
**What goes wrong:**
Implementing Web Crypto API incorrectly introduces timing attacks, improper key validation, or algorithm misuse vulnerabilities.

**Why it happens:**
- PROJECT.md correctly notes: "Bypass V8's crypto.subtle C++ entirely. Implement all crypto in Rust using ring/rsa/p256 crates"
- Custom crypto implementations are prone to timing side-channels
- Improper key validation leads to weak key attacks
- Algorithm parameter validation is complex

**Consequences:**
- Timing attacks leaking sensitive data
- Weak key vulnerabilities
- Non-compliance with FIPS/web standards
- Security audit failures

**Prevention:**
- **Use Well-Vetted Crates:** `ring` for symmetric crypto, `rsa`/`p256` for asymmetric
- **Avoid Custom Crypto:** Never implement primitives yourself
- **Timing-Safe Operations:** Use constant-time comparison functions
- **Proper Key Validation:** Validate all key parameters before use
- **Follow Web Crypto Spec:** Ensure algorithm names, parameters, and behaviors match spec

**Detection:**
- Security audit findings
- Timing analysis showing variable-time operations
- WPT (Web Platform Tests) crypto test failures

**Phase to address:** Phase 4 (Extended WinterTC)

---

## Moderate Pitfalls

### Pitfall 1: Tokio Runtime Flavor Mismatch
**What goes wrong:**
Using `tokio::runtime::Builder::new_multi_thread()` instead of `new_current_thread()` causes assertion failures when using deno_core or rusty_v8.

**Why it happens:**
- deno_core's `deno_unsync` assumes `RuntimeFlavor::CurrentThread`
- Multi-threaded tokio runtimes violate this assumption
- Assertion: `Handle::current().runtime_flavor() == RuntimeFlavor::CurrentThread`

**Consequences:**
- Panic: "assertion failed: Handle::current().runtime_flavor() == RuntimeFlavor::CurrentThread"
- Fatal runtime error when using setTimeout or async ops

**Prevention:**
- Use `tokio::runtime::Builder::new_current_thread()` for V8 integration
- Run V8 on dedicated threads, not tokio's thread pool

**Detection:**
- Panic in `deno_unsync::task` or similar locations
- Failure when spawning async operations

**Phase to address:** Phase 1 (Basic runtime setup)

---

### Pitfall 2: Dynamic Linking Issues with V8 Thread-Locals
**What goes wrong:**
Linking rusty_v8 into shared libraries fails with relocation errors against hidden V8 thread-local symbols.

**Why it happens:**
- V8 uses thread-local storage for `g_current_isolate`
- Recent V8 changes broke dynamic linking compatibility
- Error: "relocation R_X86_64_TPOFF32 against hidden symbol `g_current_isolate_E`"

**Consequences:**
- Link-time failures when building cdylib/so
- Cannot use rusty_v8 in dynamic library contexts

**Prevention:**
- Build with `v8_monolithic=true v8_monolithic_for_shared_library=true`
- Or set `V8_TLS_LIBRARY_MODE=1` during build
- Prefer static linking for V8-based applications

**Detection:**
- Link-time errors mentioning TLS and hidden symbols
- Only affects shared library builds

**Phase to address:** Build system setup

---

### Pitfall 3: VFS Path Traversal Vulnerabilities
**What goes wrong:**
Virtual filesystem implementation allows escaping the sandbox via path traversal (`../../etc/passwd`).

**Why it happens:**
- String-based path manipulation is error-prone
- Normalization may not properly handle all traversal sequences
- Edge cases with symlinks, relative paths, and absolute paths

**Consequences:**
- Sandbox escape vulnerabilities
- Read/write arbitrary files on host system
- Security breach in multi-tenant environments

**Prevention:**
- **Canonicalize All Paths:** Use `std::fs::canonicalize()` or equivalent
- **Validate Path Prefix:** Ensure resolved path starts with sandbox root
- **Use Path Types:** Leverage Rust's `Path` and `PathBuf` instead of strings
- **No Symlink Following:** Disable or carefully control symlink resolution

**Detection:**
- Security audit path traversal tests
- Fuzzing with path traversal payloads
- Code review of path handling

**Phase to address:** Phase 6 (VFS implementation)

---

### Pitfall 4: Stream Backpressure Handling
**What goes wrong:**
Web Streams API implementation doesn't properly handle backpressure, causing memory issues or data loss.

**Why it happens:**
- Readable/Writable streams need coordination between producer and consumer
- Backpressure signals when consumer can't keep up
- Improper handling causes unbounded buffering or dropped data

**Consequences:**
- Memory exhaustion with fast producers and slow consumers
- Data loss when buffers overflow
- Poor performance due to improper flow control

**Prevention:**
- **Implement Backpressure Signals:** Use desiredSize, queue strategies
- **Queue Management:** Limit internal queue sizes
- **Proper Await:** Await write promises before continuing
- **Follow Web Streams Spec:** Implement controller methods correctly

**Detection:**
- Memory growth during streaming operations
- Data corruption in streaming pipelines
- Performance degradation with large streams

**Phase to address:** Phase 4 (Streams implementation)

---

## Phase-Specific Warnings

| Phase | Topic | Likely Pitfall | Mitigation |
|-------|-------|----------------|------------|
| 1 | Platform init | Not using `std::sync::Once` for V8 init | Serialize first isolate creation |
| 1 | Handle scopes | Long-lived scopes with temporary handles | Nested HandleScope pattern |
| 2 | WorkerPool | Moving isolates between threads | Thread-local isolate pattern |
| 2 | Context reset | Memory leak from external references | Explicit cleanup before reset |
| 3 | fetch() | Blocking in callbacks | Async bridge pattern with channels |
| 3 | Promises | Not pumping microtasks | `perform_microtask_checkpoint()` |
| 4 | Crypto | Custom crypto implementations | Use ring/rsa/p256 crates only |
| 4 | Streams | Backpressure mishandling | Queue limits and flow control |
| 5 | WebSocket | RFC 6455 non-compliance | Use tokio-tungstenite |
| 5 | Snapshots | Version mismatches | Build-time snapshot generation |
| 6 | VFS | Path traversal | Canonicalize and validate paths |

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| EPT/SIGSEGV | HIGH | Well-documented in V8 issues and PROJECT.md |
| Handle scopes | HIGH | rusty_v8 docs and issue #481 confirm pattern |
| Thread safety | HIGH | Issue #1467 and ThreadSanitizer findings |
| Async integration | HIGH | StackOverflow answer and deno_core patterns |
| Memory leaks | HIGH | Issue #1348 and DHAT analysis |
| Context/isolate lifecycle | MEDIUM-HIGH | Based on V8 docs and deno_core behavior |
| WinterTC specifics | MEDIUM | Less documentation on edge cases |
| Snapshot issues | MEDIUM | V8 internals can vary by version |

---

## Sources

### rusty_v8 GitHub Issues
- Issue #1467: "Unsoundness when starting an isolate per thread" — Thread safety violations with ThreadSanitizer output
- Issue #1348: "Tracking Down A Memory Leak" — Multi-isolate memory leaks in deno_core
- Issue #481: "Is rusty_v8::Script::compile leaking memory?" — HandleScope lifetime explanation
- Issue #1706: TLS relocation errors with dynamic linking
- Issue #1259: "how to synchronously wait for a promise value" — perform_microtask_checkpoint requirement

### deno_core Issues
- Issue #1092: "What's the expected way to manage multiple JsRuntime within a single process?"
- Issue #708: "Multiple JSRuntime in a single tokio runtime" — Isolate creation/destruction crashes

### V8 Documentation
- docs.rs/rusty_v8 — HandleScope, EscapableHandleScope, Global, Isolate docs
- V8 External Pointer Table source (src/sandbox/external-pointer-table.h)
- V8 snapshot serialization docs

### Stack Overflow
- "How to handle asynchronous operations with rusty_v8" — Async bridge pattern
- "rusty_v8 TryCatch not catching heap limit" — Heap limit callback pattern

### Web Standards
- WinterTC Minimum Common API proposal
- WHATWG Fetch standard (server-side considerations)
- RFC 6455 WebSocket Protocol

### Blogs & Articles
- Joyee Cheung's Node.js snapshot reproducibility series
- Deno "Roll your own JavaScript runtime" tutorials
- Cloudflare Workers architecture documentation

---

*Last updated: 2026-04-19*
*Research completed for NANO edge runtime migration from Zig to Rust*
