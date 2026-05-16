# V8 Isolate Reuse Bug - Deep Dive Analysis

**Date:** 2026-05-16  
**Updated:** 2026-05-16  
**Status:** PERSISTENT - Context reset removed, bug still present  
**Severity:** Critical - Requires architecture change

---

## Executive Summary

**CRITICAL UPDATE:** Context reset has been removed from the worker loop, but the isolate reuse bug persists. This indicates the issue is deeper than just context reset - it appears to be a fundamental problem with V8 isolate state after multiple request executions.

The original analysis identified context reset as the problem. While context reset IS problematic (not supported by V8), removing it did not fix the bug. This suggests V8 isolate state corruption occurs from repeated script execution, not just context disposal.

### Key Finding

**Deno, Chrome, and Cloudflare Workers do NOT share isolates across requests within the same tenant.** They use:
1. One context per isolate for the isolate's entire lifetime (no request-to-request sharing)
2. Fresh isolates per request (complete isolation)
3. Isolate-per-tenant with context persistence (NOT request isolation via context reset)

### Key Finding

**Deno, Chrome, and Cloudflare Workers do NOT reset V8 contexts within the same isolate.** They either:
1. Keep one context per isolate for the isolate's entire lifetime
2. Create fresh isolates per tenant/request and dispose the entire isolate

---

## Update: Context Reset Removed - Bug Persists

### Test Results After Context Reset Removal

```
Request 1: PASSED - Body: 'Hello from worker'
Request 2: PASSED - Body: 'Hello from worker'
Request 3: PASSED - Body: 'Hello from worker'
Request 4: PASSED - Body: 'Hello from worker'
Request 5: FAILED - HTTP 500 (Script execution failed)
Request 6-10: FAILED - HTTP 500
```

**Finding:** Even with context reset removed, the V8 isolate fails after ~4 requests.

### What This Means

The issue is NOT just about context reset. The V8 isolate itself becomes corrupted after multiple script executions, even with:
- Same context throughout isolate lifetime
- No context disposal/recreation
- Fresh HandleScope for each request

**Hypothesis:** V8 isolate internal state (compilation cache, hidden classes, heap structures) becomes corrupted after repeated script executions, or there's a memory/GC issue that accumulates over requests.

---

## What nano-rs Did (WRONG - NOW REMOVED)

```rust
// OLD CODE - Context reset before each request (REMOVED)
// loop {
//     context_manager.reset_context();  // REMOVED - caused V8 issues
//     execute_with_context_manager(...);
// }

// NEW CODE - No context reset (STILL HAS BUG)
loop {
    // Context persists across requests
    execute_with_context_manager(...);  // Still fails after ~4 requests
}
```

**reset_context() implementation:**
```rust
pub fn reset_context(&mut self) -> Result<Duration> {
    // Dispose current context
    self.current_context = None;  // Drops the Global<Context>
    
    // Create new context with clean global scope
    let scope_storage = std::pin::pin!(v8::HandleScope::new(self.isolate.isolate()));
    let scope = scope_storage.init();
    let new_context = v8::Context::new(&scope, Default::default());
    let global_context = v8::Global::new(&scope, new_context);
    
    self.current_context = Some(global_context);
}
```

---

## What Deno Does (CORRECT)

### Pattern 1: One Context Per Isolate (Deno CLI/Deploy)

From Deno issue #17861 (Deno maintainer):
> "`JsRuntime` has a single 'global context' that lives as long as the runtime itself"

> "Currently there's no way to destroy a context from `JsRuntime` and create a new one, 
> as a lot of infrastructure depends on the assumption that there's a 'global context'"

```rust
// Deno pattern: Context is created once with the isolate
let global_context = v8::Global::new(scope, context);
// Global context lives for the ENTIRE lifetime of the isolate
// Never disposed and recreated
```

### Pattern 2: Separate Process Per Tenant (Deno Deploy)

From Deno Deploy architecture blog post:
> "Each deployment executes its own JavaScript code in its own V8 isolate in its own process"

```
Incoming Request
    ↓
Edge Proxy (routes by hostname)
    ↓
Runner Process (per VM)
    ↓
V8 Isolate (per deployment/tenant)
    ↓
Global Context (lives as long as isolate)
```

**Key insight:** Deno Deploy maintains separate isolates for each deployment, NOT separate contexts within the same isolate.

### Pattern 3: JsRealm for ShadowRealm (Experimental)

From Deno issue #17861:
> "`JsRealm` is still WIP and not really production-ready"
> "While isolates have separate memory heaps, so that dropping a `v8::Isolate` 
> will free all memory associated with it, realms/contexts don't really."

---

## Why V8 Context Reset Fails

### 1. Memory Model

From Deno issue #1067:
> "`v8::Global` handles in `ContextState` are preventing realms from being GC'd"

> "the `v8::Global` handles in `ContextState` point to `Deno.core.*` functions 
> inside the corresponding V8 context, which have a reference to the context's 
> `Function.prototype` built-in, which in turn has a reference to the context itself"

**Result:** Reference cycles that V8 GC cannot detect across context boundaries.

### 2. Script Source Retention

From Deno issue #17861:
> "Once you evaluate the code, V8 stores the source which IIRC will not be 
> freed until the context it belongs to is destroyed."

In nano-rs:
- Context 1 created → script loaded → context 1 disposed (but source retained in isolate)
- Context 2 created → script loaded → source from context 1 still in isolate memory
- Context N created → isolate memory keeps growing, internal state becomes corrupted

### 3. No True Isolation Between Contexts

From V8 documentation:
> "Isolate represents an isolated instance of the V8 engine. V8 isolates have 
> completely separate states. Objects from one isolate must not be used in other isolates."

**Critical:** V8 only guarantees isolation between ISOLATES, not between CONTEXTS within the same isolate.

Contexts within the same isolate:
- Share the same heap
- Share the same garbage collector
- Share internal V8 state (parser caches, hidden class transitions, etc.)
- Can leak references to each other through retained Global handles

### 4. Context Disposal Notification Missing

V8 provides `Isolate::ContextDisposedNotification()` which should be called when disposing contexts:

> "Optional notification that a context has been disposed. V8 uses these 
> notifications to guide the GC heuristic."

nano-rs does NOT call this, potentially leaving V8 in an inconsistent state.

---

## Chrome's Architecture

Chrome creates:
- **One isolate per renderer process**
- **One main context per isolate** (for the web page)
- **Additional contexts for iframes** (but these are managed carefully, not rapidly disposed)

Chrome does NOT:
- Dispose and recreate contexts for every user interaction
- Share isolates between different security origins
- Rapidly create/dispose contexts within the same isolate

---

## Cloudflare Workers Architecture

From Cloudflare blog posts and V8 isolate documentation:

```
Incoming Request (per tenant)
    ↓
V8 Isolate Pool
    ↓
Warm Isolate with Pre-warmed Context
    ↓
Execute Request
    ↓
Isolate Returned to Pool (NOT destroyed)
```

**Key difference from nano-rs:**
- Cloudflare uses the SAME context within the isolate for multiple requests
- They don't reset the context between requests
- State can persist between requests (by design)
- When isolate needs cleanup, the ENTIRE isolate is disposed, not just the context

---

## The Real Solutions

### Solution 1: No Context Reset (Recommended for Production)

**Pattern:** One context per isolate, isolate lives for N requests or until OOM

**Pros:**
- Fastest performance (no context creation overhead)
- Works with V8 architecture
- Used by Deno Deploy and Cloudflare Workers

**Cons:**
- State persists between requests (security concern for multi-tenant)
- Memory grows over time
- Need to periodically dispose entire isolate

**Implementation:**
```rust
// Remove reset_context() call from worker loop
loop {
    // NO context reset - use existing context
    execute_with_context_manager(&mut context_manager, ...);
    
    // Periodically dispose entire isolate, not just context
    if should_recycle_isolate(&isolate_id) {
        context_manager = ContextManager::new(create_fresh_isolate());
    }
}
```

### Solution 2: Fresh Isolate Per Request (Safest)

**Pattern:** Create new isolate for each request, execute, dispose entire isolate

**Pros:**
- True isolation between requests
- No state leakage
- Works reliably with V8

**Cons:**
- 50-100ms cold start per request
- High memory overhead
- Defeats worker pool purpose

**Implementation:**
```rust
// Worker creates fresh isolate per request
loop {
    let isolate = NanoIsolate::new();  // Fresh isolate
    let mut context_manager = ContextManager::new(isolate);
    context_manager.create_initial_context();
    
    execute_with_context_manager(&mut context_manager, ...);
    
    // Isolate automatically disposed when context_manager dropped
}
```

### Solution 3: Isolate Per Tenant (Deno Deploy Pattern)

**Pattern:** One isolate per tenant/hostname, context never reset

**Pros:**
- Good performance (isolate reuse)
- Security isolation between tenants
- State can persist (beneficial for some use cases)

**Cons:**
- More memory (one isolate per tenant)
- Need tenant-aware routing

**Implementation:**
```rust
// WorkQueue maintains isolate pool per hostname
struct TenantWorkerPool {
    hostname: String,
    isolates: Vec<ContextManager>,
}

// Each tenant gets dedicated isolates
// Context is never reset, isolate is recycled periodically
```

### Solution 4: Fix Context Reset (Hard, Maybe Impossible)

**Pattern:** Properly dispose context and notify V8

**Challenges:**
- Must call `isolate.ContextDisposedNotification()`
- Must ensure NO Global handles leak from disposed context
- Must wait for GC to free context memory
- May still hit V8 bugs (deno_core issue #1067)

**Why it may not work:**
> "This is not a cycle that V8's garbage collector can detect, because V8 
> has no way to inspect the contents of the slots associated to a `v8::Context`"

---

## Recommended Path Forward

### Immediate (For Testing)

Use **Solution 2** (fresh isolate per request) for testing to get reliable behavior:

```rust
// Quick fix for testing - disable context reset
// In worker/pool.rs, skip the reset_context() call
// The isolate will be created fresh for each worker spawn
```

### Short-term (Production v1.6)

Implement **Solution 1** with periodic isolate recycling:

1. Remove `reset_context()` call from worker loop
2. Add isolate request counter
3. After N requests (e.g., 100), dispose entire isolate and create fresh one
4. Monitor memory and recycle on OOM

```rust
const MAX_REQUESTS_PER_ISOLATE: u32 = 100;

loop {
    request_count += 1;
    
    if request_count >= MAX_REQUESTS_PER_ISOLATE {
        // Dispose entire isolate, not just context
        context_manager = create_fresh_context_manager();
        request_count = 0;
    }
    
    execute_with_context_manager(&mut context_manager, ...);
}
```

### Long-term (Production v2.0)

Implement **Solution 3** (isolate per tenant):

1. Create `TenantWorkerPool` struct
2. Maintain isolate pool per hostname
3. Route requests to tenant-specific isolates
4. Recycle isolates per-tenant based on memory/request count

This matches Deno Deploy and Cloudflare Workers architecture.

---

## Files to Modify

1. **`src/worker/pool.rs`** - Remove or make optional the `reset_context()` call
2. **`src/worker/context.rs`** - Add isolate recycling logic
3. **`src/worker/eviction.rs`** - Add per-isolate request counter
4. **`src/worker/queue.rs`** - Support tenant-specific worker pools

---

## References

- [Deno Issue #17861](https://github.com/denoland/deno/issues/17861) - "Reuse JsRuntime and MainWorker"
- [Deno Issue #1067](https://github.com/denoland/deno_core/issues/1067) - "Global handles preventing realms from being GC'd"
- [Deno Deploy Architecture](https://deno.com/blog/anatomy-isolate-cloud) - "The Anatomy of an Isolate Cloud"
- [V8 Isolate API](https://v8.github.io/api/head/classv8_1_1_isolate.html) - V8 Isolate documentation
- [V8 Context API](https://v8.github.io/api/head/classv8_1_1_context.html) - V8 Context documentation

---

## Summary

The isolate reuse bug is not a code bug that can be fixed with better Rust code - it's an **architectural mismatch** with V8's design. V8 contexts are not designed to be rapidly disposed and recreated within the same isolate.

**The only proper solutions are:**
1. Don't reset contexts (keep them for isolate lifetime)
2. Create fresh isolates per request
3. Use isolate-per-tenant model (Deno Deploy/Cloudflare Workers)

All three require architectural changes to nano-rs, not just bug fixes.
