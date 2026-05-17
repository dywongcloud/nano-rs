# V8 Isolate Reuse Investigation: Failures and Findings

**Date:** 2026-05-16 (updated 2026-05-17)
**Status:** RESOLVED — Approach 6 works (see below)
**Current Working Solution:** Persistent HandleScope + ContextScope on the worker thread stack (commit 3f098832 + Phase 40 fixes)

---

## The Core Problem

**V8 function handles are strictly bound to their creation scope context**, not just their isolate. Creating fresh scopes per request breaks this binding irreversibly. The only way to call a V8 function is from the exact scope context that created/retrieved it.

Cloudflare/Deno achieve isolate reuse by using **persistent scopes** that span all requests - they don't create fresh scopes per request. But the v147 rusty_v8 API type system prevents implementing this pattern in safe Rust.

---

## Attempted Approaches

### Approach 1: Caching v8::Global<Function>
**Idea:** Cache handler as Global<Function> during initialization, convert to Local in fresh scopes per request.

**Implementation:**
- Execute script once during worker startup
- Store handler as `v8::Global<v8::Function>` in ContextManager
- Per request: create fresh scopes, convert Global to Local via `v8::Local::new()`

**Failure:**
```
Request 1: handler_fn.call() returns Some(response) ✓
Request 2: handler_fn.call() returns None ✗ (is_function=true but call fails)
```

**Root Cause:** Global<Function> survives across scope lifetimes, but calling it requires the **original scope context**. The v8::Local conversion creates a handle that's technically valid but functionally unusable outside the creation context.

---

### Approach 2: Persistent Scopes in Struct
**Idea:** Keep HandleScope + ContextScope alive across all requests by storing them in ContextManager struct with 'static lifetime.

**Implementation:**
```rust
pub struct ContextManager {
    isolate: NanoIsolate,
    scope_storage: Option<v8::HandleScope<'static, ()>>,  // transmuted
    context_scope: Option<v8::ContextScope<'static, 'static, v8::HandleScope<'static, ()>>>,
    cached_handler: Option<v8::Global<v8::Function>>,
}
```

**Failure:**
- v147 API requires `PinnedRef<HandleScope>` for ContextScope::new()
- Cannot store `PinnedRef` in struct (temporary reference)
- Cannot satisfy trait bounds: `HandleScope<'static>: NewContextScope` not implemented
- Type system fundamentally prevents persistent scope pattern

---

### Approach 3: Retrieve Handler Fresh Each Request
**Idea:** Don't cache function handle. Re-enter same context, retrieve handler from global scope each request.

**Implementation:**
- Store `v8::Global<v8::Context>` in ContextManager
- Per request: reopen context via `v8::Local::new()`, retrieve `__nano_user_fetch` from global object

**Failure:**
```
Global scope HAS __nano_user_fetch (verified via get_own_property_names)
Retrieved value: is_function=true ✓
handler_fn.call() returns None ✗
```

**Root Cause:** Even retrieving the function fresh from global scope creates a handle bound to the **retrieval scope context**, not the original definition context. The function exists but is unusable.

---

### Approach 4: Script Re-execution in Same Isolate
**Idea:** Keep isolate alive, but re-execute script each request to re-define handler in fresh context.

**Implementation:**
- Create isolate once (worker startup)
- Per request: create fresh context, execute script, call handler

**Failure:**
```
Request 1: Script executes, handler found, call succeeds ✓
Request 2: Script execution fails with "Script execution failed" ✗
```

**Root Cause:** V8 isolate internal state corrupted after first script execution. Cannot re-execute scripts in same isolate without full context reset (which defeats the purpose).

---

### Approach 5: Fresh Isolate Per Request (Current)
**Idea:** Accept isolate creation overhead, ensure correctness.

**Implementation:**
- Per request: create new isolate → execute script → call handler → drop isolate

**Status:** WORKING but INSUFFICIENT
- All requests pass correctly
- No V8 lifetime issues
- Clean, simple code

**Latency:** 50-100ms per request (FAILS requirements - must be <10ms)

---

### Approach 6: Persistent HandleScope + ContextScope (WORKING)

**Status:** WORKING as of commit 3f098832 + Phase 40 fixes (2026-05-17)

**Pattern:** Persistent HandleScope + ContextScope created ONCE on the worker thread stack at isolate startup. They are NEVER dropped between requests within one isolate lifetime. The handler function is cached as `Global<Function>` per entrypoint and converted to `Local<Function>` via `v8::Local::new(&mut ctx_scope, &global)` in the SAME persistent ContextScope.

**Key insight:** The `Global<Function>` -> `Local<Function>` conversion works ONLY when the ContextScope that created the Global is still entered. Exiting and re-entering the context (or creating a new ContextScope) invalidates the conversion. Since the HandleScope and ContextScope stay alive for the entire isolate lifetime (on the thread stack), all conversions succeed.

**Handler cache:** `HashMap<String, Global<Function>>` -- one entry per JS entrypoint file, cached on first request, reused for all subsequent requests within the same isolate lifetime.

**Required conditions for correctness:**
1. **TryCatch RAII** wrapping every `handler_local.call()` -- prevents exception state from leaking to the next request. Drop of `tc` clears the pending exception. Without this, a JS exception from request N poisons subsequent requests on the same isolate.
2. **`cancel_terminate_execution()`** called in `CpuTimeoutGuard::drop()` when `terminate_execution()` was fired -- prevents the V8 terminate flag from persisting on the isolate after a CPU timeout. Without this, every recycled worker silently returned `None` for all subsequent requests (fixed in Phase 40).
3. **`set_allow_generation_from_strings(false)`** at `Context::new()` -- security baseline, blocks dynamic code evaluation via string-based APIs.

**Proved by:**
- `tests/isolate_scope_test.rs` SCOPE-01..08 (1000 calls in persistent scope, exception isolation, async handler, ESM module transform)
- `tests/isolate_endurance_test.rs` ENDURE-01..03 (exception recovery across 30 requests, module state persistence, 15+ requests no degradation)

**Commits:**
- `3f098832` -- feat(worker): Persistent V8 scope lifecycle (initial implementation)
- Phase 40 -- fix(worker): TryCatch at handler call sites, block string generation, fix Buffer.from(Array)
- Phase 40 -- fix(data_plane): call cancel_terminate_execution() in CpuTimeoutGuard::drop

**Key lesson:** The fundamental blocker for Approaches 1-4 was creating fresh scopes per request. Approach 6 avoids this entirely by keeping the scope stack alive on the thread. The rusty_v8 type system DOES allow this -- it just requires the scopes to live on the stack of a long-running thread rather than in a struct.

---

## Technical Deep Dive

### V8 Scope Binding Semantics

V8 uses a stack-based handle system:

```
Isolate
  └── HandleScope (creates handle storage)
      └── ContextScope (enters execution context)
          └── Local<T> handles (point to V8 heap objects)
```

**Critical:** A `Local<Function>` created in ContextScope A **cannot be called** from ContextScope B, even in the same isolate. The handle is bound to the scope stack state at creation time.

### Cloudflare's Actual Architecture

Cloudflare Workers use **one persistent context per isolate**:

```javascript
// Worker script executes ONCE during isolate creation
export default {
  async fetch(request) {  // Handler defined here
    return new Response("Hello");
  }
};

// All subsequent requests enter SAME context, call SAME function
// No scope recreation between requests
```

The V8 embedder API allows this via `v8::Context::New()` followed by entering that context once and staying in it. But rusty_v7's type system (correctly for safety) prevents holding ContextScope across async/await points.

### Why rusty_v8 Makes This Hard

The rusty_v8 crate enforces at compile time:
1. Handles cannot outlive their creating scope
2. Scopes must be dropped in LIFO order
3. No "enter context forever" API exposed (would require `&'static mut`)

This is **correct for safety** - V8's C++ API is memory-unsafe if misused. But it prevents the Cloudflare pattern.

---

## Attempted Code Patterns (All Failed)

### Pattern A: Global<Function> Cache
```rust
// Initialization
let handler: v8::Local<v8::Function> = ...;
self.cached_handler = Some(v8::Global::new(&mut context_scope, handler));

// Per request
let handler_fn = v8::Local::new(&mut context_scope, &self.cached_handler.unwrap());
let result = handler_fn.call(&mut context_scope, ...);  // Returns None on 2nd+ request
```

### Pattern B: Persistent Scopes
```rust
// In struct
scope_storage: Option<v8::HandleScope<'static, ()>>,

// In new()
let scope = unsafe { std::mem::transmute(v8::HandleScope::new(isolate)) };
self.scope_storage = Some(scope);

// Per request - FAILS: cannot create ContextScope from stored HandleScope
let mut context_scope = v8::ContextScope::new(&mut self.scope_storage.as_mut().unwrap(), context);
// Error: trait bound `HandleScope<'static>: NewContextScope` not satisfied
```

### Pattern C: Transmute Everything to 'Static
```rust
unsafe {
    let mut scope_storage = v8::HandleScope::new(isolate);
    let scope_pin = std::pin::Pin::new_unchecked(&mut scope_storage);
    let mut handle_scope: v8::PinnedRef<'static, v8::HandleScope> =
        std::mem::transmute(scope_pin.init());
    // ... more transmutes for ContextScope, Context, Function
}
```
Fails: V8 internal state tracking still sees scope boundaries. Transmute bypasses Rust's type system but not V8's runtime checks.

---

## Potential Alternatives (Not Yet Explored)

### 1. WASM-Based Runtime
**Idea:** Replace V8 with Wasmtime or Wasmer

**Pros:**
- Properly supports module instance reuse
- Clear instantiation vs execution phases
- Rust-native API without C++ interop issues

**Cons:**
- Requires rewriting WinterTC APIs (fetch, Request, Response, etc.)
- JavaScript support requires JS→WASM compilation (SpiderMonkey or QuickJS)
- Large engineering effort

---

### 2. V8 Snapshot + Isolate Pool
**Idea:** Pre-warm isolates using V8 snapshots, pool them

**Approach:**
- Create snapshot with WinterTC APIs baked in (~1ms restore)
- Maintain pool of pre-warmed isolates
- Each request: checkout isolate, execute script (5ms?), return to pool

**Unknowns:**
- Can script execute in snapshot-restored isolate work correctly?
- Does V8 support "reset to snapshot" without full isolate recreation?

**Status:** Not tested. Snapshot infrastructure in place in `src/v8/snapshot.rs` but runtime creation not implemented (v147 API limitations).

---

### 3. Modify rusty_v8 Fork
**Idea:** Fork rusty_v8, add API for persistent contexts

**Approach:**
```rust
// Hypothetical API
let isolate = v8::Isolate::new(params);
let context = v8::Context::new(&isolate, Default::default());
isolate.enter_context_permanently(context);  // New API

// Now all operations implicitly use this context
// No ContextScope needed per request
```

**Cons:**
- Maintaining V8 fork is huge ongoing burden
- Safety guarantees would need careful design
- Unstable API surface

---

### 4. Single-Threaded Persistent Worker
**Idea:** Run worker in thread-local loop, never leave V8 context

**Approach:**
```rust
std::thread::spawn(|| {
    // Create isolate
    // Create context
    // Enter context
    // Execute script
    // Loop {
    //     recv request from channel
    //     call handler directly (no scope recreation)
    //     send response
    // }
})
```

**Blocker:** Current nano-rs architecture uses async/await worker tasks with tokio. Converting to thread-local message loop requires major architectural changes.

---

### 5. QuickJS Integration
**Idea:** Use QuickJS instead of V8

**Pros:**
- Pure C (easier to bind than V8 C++)
- Explicit context management
- Smaller footprint
- Designed for embedding

**Cons:**
- Rewriting all WinterTC bindings
- Performance unknown (likely slower than V8)
- Different JS engine quirks

---

## Decision Log

### Decision: Give Up on V8 Isolate Reuse (2026-05-16)
**Context:** 5+ approaches failed, fundamental V8 architecture limitations

**Decision:** Accept fresh isolate per request for now (~50-100ms latency)

**Rationale:**
- Correctness over performance
- User explicitly stated "50-100ms makes nano unusable"
- This decision VIOLATES user requirements
- But no viable alternative identified yet

**Regret:** Should have escalated to "impossible with current stack" 2 hours earlier instead of pursuing increasingly complex workarounds.

---

## Next Steps

1. **Evaluate WASM runtime** (Wasmtime/Wasmer) - Big engineering effort but potentially correct architecture
2. **Research V8 isolate pools** - May reduce cold start from 50ms to ~5ms
3. **Consider QuickJS** - Simpler C API, explicit context management
4. **Revisit if rusty_v8 adds persistent context API** - Upstream may solve this

---

## Key Files Modified During Investigation

- `src/worker/context.rs` - ContextManager architecture (persistent scope attempts)
- `src/data_plane.rs` - execute_with_context_manager, various handler caching attempts
- `src/v8/snapshot.rs` - V8 snapshot infrastructure (unused but ready)

---

## References

- V8 Embedder Guide: https://v8.dev/docs/embed
- rusty_v8 API docs: https://docs.rs/v8/147.4.0/v8/
- Cloudflare Workers architecture: https://developers.cloudflare.com/workers/learning/how-workers-works/
- Deno architecture: https://deno.land/manual/runtime/architecture

---

**Document Status:** Complete  
**Confidence in "impossible" assessment:** High (5+ failed approaches, clear V8 architecture constraints)  
**Recommendation:** Explore WASM or accept latency requirements must change
