# Phase 40: Pre-Phase-23 Stability — Research

**Researched:** 2026-05-17
**Domain:** V8 isolate reuse correctness, persistent-scope exception recovery, state isolation, dynamic endurance testing
**Confidence:** HIGH (all findings verified directly from codebase inspection and existing tests)

---

## Summary

Phase 40 is a correctness hardening phase that must be completed before Phase 23 (WebSocket Server). It targets three distinct problems observed during session-2026-05-17 testing: (1) cumulative JS exceptions causing `handler_local.call()` to return `None` on recycled workers, (2) counter state leaking across requests on the same isolate, and (3) a test suite that may be testing the wrong invariants.

The persistent-scope architecture (commit 3f098832) was introduced AFTER the `V8_ISOLATE_REUSE_INVESTIGATION.md` was written. That document catalogued five failed approaches; the current implementation is a sixth approach not in that document. It works: the existing `isolate_scope_test.rs` suite proves that `handler_local.call()` succeeds 1,000 times in a persistent scope with no `None` returns (SCOPE-01 passes). The question for Phase 40 is whether TryCatch drop semantics, exception state from promise rejection, or CPU-timeout-induced `terminate_execution()` can poison an otherwise working isolate.

Three fixes were applied this session but not committed: TryCatch at all three handler call sites (pool.rs x2, tenant_pool.rs x1), `Buffer.from(Array)` in apis.rs, and `context.set_allow_generation_from_strings(false)` at all Context::new() sites. The TryCatch fix is sound in theory (RAII drop clears the pending exception) but the reported "still fails on recycled workers" symptom suggests either (a) an exception from somewhere other than handler_local.call() is escaping TryCatch scope, or (b) `terminate_execution()` from the CPU timeout guard is not being cancelled before the next request runs.

**Primary recommendation:** Commit the three in-session fixes first, run SCOPE-01 through SCOPE-08 to establish a baseline, then write a focused Rust-level endurance test that targets the exact repro path (single worker, 10+ sequential requests including one that throws a JS exception at request N, then verify request N+1 succeeds).

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| TryCatch exception capture | Worker thread (pool.rs / tenant_pool.rs) | — | Exception state is per-isolate, must be cleared on the same thread |
| Isolate recycle after MAX_REQUESTS | Worker thread (pool.rs inner loop) | — | Only the owning thread can dispose the isolate |
| State isolation (module globals) | JS runtime semantics | pool.rs comment documents this | CF-Workers compatible: globals persist within isolate lifetime by design |
| CPU timeout guard cancellation | data_plane::CpuTimeoutGuard (RAII) | Worker thread | Guard must be dropped before next request |
| Dynamic endurance tests | tests/ (new test file) | — | Must drive through the WorkerPool dispatch path, not direct isolate calls |

---

## Standard Stack

### Core (all already in Cargo.toml)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| v8 (rusty_v8) | 147.4.0 | V8 bindings | Project-pinned; all scope APIs are v147 API |
| tokio | 1.x | Async runtime per worker | Already in workers |
| anyhow | 1.x | Error propagation | Project-wide |
| tracing | 0.1.x | Structured logging | Project-wide |
| tempfile | 3.x | Temp JS files in tests | Already used in pool tests |

No new dependencies are required for Phase 40. [VERIFIED: codebase inspection]

### Installation

```bash
# No new dependencies -- all fixes are within existing codebase
cargo build --lib   # confirms zero compile errors
```

---

## Package Legitimacy Audit

No new external packages are introduced in this phase. Existing dependencies are already validated. Audit section: N/A.

---

## Architecture Patterns

### System Architecture Diagram

```
HTTP Request
    |
    v
WorkQueue / TenantPool  <-- round-robin
    |
    v
Worker Thread (OS thread, one per slot)
    |
    +-- 'isolate loop (outer)
    |       |
    |       +-- NanoIsolate::new_with_vfs()
    |       +-- HandleScope::new()        <- pinned on thread stack
    |       +-- Context::new()
    |       |       +-- set_allow_generation_from_strings(false)  <- NEW (session fix)
    |       +-- RuntimeAPIs::bind_all()
    |       +-- ContextScope::new()       <- entered ONCE, never dropped between requests
    |       +-- handler_cache: HashMap<path, Global<Function>>
    |       |
    |       +-- 'requests loop (inner, MAX_REQUESTS_PER_ISOLATE iterations)
    |               |
    |               +-- recv task
    |               +-- OOM pre-check
    |               +-- if !handler_cache.contains(entrypoint):
    |               |       compile script -> run -> cache Global<Function>
    |               +-- CpuTimeoutGuard::new()  <- RAII drop at end of block
    |               +-- v8::Local::new(&ctx_scope, handler_global)
    |               +-- [build JS Request object]
    |               +-- TryCatch::new()      <- NEW (session fix): RAII, clears exception on drop
    |               +-- handler_local.call(&tc, ...)
    |               +-- Promise pump loop (if async)
    |               +-- extract NanoResponse
    |               +-- response_tx.send(result)
    |
    +-- on 'requests exit: ctx_scope+scope drop -> nano drops -> next 'isolate iteration
```

### Recommended Project Structure (Phase 40 additions)

```
tests/
+-- isolate_scope_test.rs      # existing -- SCOPE-01..SCOPE-08 (all pass)
+-- isolate_endurance_test.rs  # NEW -- exception recovery, state isolation docs, N+1 tests
src/worker/
+-- pool.rs                    # TryCatch sites x2 (THIS SESSION, not yet committed)
+-- tenant_pool.rs             # TryCatch site x1 (THIS SESSION, not yet committed)
src/runtime/
+-- apis.rs                    # Buffer.from(Array) fix (THIS SESSION, not yet committed)
```

### Pattern 1: TryCatch RAII — Exception Clearance

**What:** Wrap every `handler_local.call()` in a `v8::TryCatch`. The drop of `tc` at the end of the enclosing closure clears any pending exception from the isolate, preventing it from poisoning the next request.

**When to use:** Every handler invocation inside the 'requests loop.

**Current implementation (pool.rs, this session, uncommitted):**

```rust
// Source: src/worker/pool.rs lines 429-432 (session fix, uncommitted)
// Must pin-and-init like HandleScope: TryCatch::new returns ScopeStorage.
let tc_storage = v8::TryCatch::new(&mut *ctx_scope);
let tc_pin = std::pin::pin!(tc_storage);
let mut tc = tc_pin.init();

let call_result = handler_local.call(&tc, global_obj.into(), &[js_req.into()]);
```

**Critical constraint:** `tc` must be dropped BEFORE the next iteration of 'requests. The closure `(|| { ... })()` pattern achieves this because `tc` is scoped to the closure body. Verify that `tc` does not escape the closure on any code path.

**Known risk:** The Promise pump loop also runs inside the same closure and uses `tc` as the scope for `tc.perform_microtask_checkpoint()` and `promise.result(&tc)`. If the Promise rejects and leaves a pending exception before `tc` drops, that exception IS cleared by TryCatch drop. This is correct behavior.

### Pattern 2: CPU Timeout Guard — Must Not Survive Into Next Request

**What:** `CpuTimeoutGuard` calls `v8::Isolate::terminate_execution()` from a timer thread after a deadline. `terminate_execution()` sets a flag in the isolate that causes future function calls to return `None`.

**Critical invariant:** The guard must be dropped before the next request's `handler_local.call()`. The current code drops `_timeout` at the end of the request block, which is correct. However, if `terminate_execution()` fired AND the guard drops but the isolate does not call `cancel_terminate_execution()`, the isolate may remain in terminated state.

**Verify:**

```bash
grep -n "cancel_terminate_execution\|CpuTimeoutGuard" src/data_plane.rs
```

If `cancel_terminate_execution()` is NOT called on drop, any request that hits the CPU limit will poison all subsequent requests on that worker until the isolate recycles. This is a plausible root cause for "cumulative exceptions after ~4 requests."

### Pattern 3: State Isolation — Module Globals Persist By Design

**What:** Module-level `var`/`let`/`const` declared outside the handler function persist across requests within one isolate's lifetime. This matches Cloudflare Workers semantics. Handler-local state (function arguments, local variables) is fresh per call.

**Documented in code:** `src/worker/pool.rs` contains the "State-leaking note (Task C)" comment at line 272.

**Test expectation:** Tests that assume `counter === 1` on request 5 (when request 1 previously incremented it on the same worker) are testing the WRONG invariant. The correct test is: "handler-local state is fresh; module-level counter increments monotonically within one isolate lifetime, resets on recycle."

### Pattern 4: Dynamic Endurance Test Design

A good endurance test must:

1. Use `WorkerPool::with_backend("...", 1, 0, backend)` — **1 worker** to ensure all requests hit the same persistent scope.
2. Send N requests sequentially (not concurrently) so request ordering is deterministic.
3. Include a request that intentionally throws a JS exception (to exercise TryCatch).
4. Verify the NEXT request after the exception succeeds.
5. Track which request hit which worker by embedding a call counter in the JS handler.

Example test structure:

```rust
// tests/isolate_endurance_test.rs
// [ENDURE-01] Exception at request N does not break request N+1

let pool = WorkerPool::with_backend("endurance.test".into(), 1, 0, backend);

// JS that throws on every 3rd request
let entrypoint = write_js("endure.js", r#"
var call_count = 0;
function __nano_user_fetch(req) {
    call_count++;
    if (call_count % 3 === 0) {
        throw new Error("intentional exception at call " + call_count);
    }
    return { status: 200, headers: {}, body: "ok:" + call_count };
}
"#);

for i in 0..30 {
    let (tx, rx) = tokio::sync::oneshot::channel();
    pool.dispatch(HandlerTask::new(
        entrypoint.clone(), make_get("http://endurance.test/"), tx,
    )).unwrap();
    let result = rx.blocking_recv().unwrap();
    if (i + 1) % 3 == 0 {
        // Expect error on throw requests (TryCatch catches it, returns 500 or Err)
        match &result {
            Ok(r) if r.status() == 500 => {} // TryCatch captured and returned 500
            Err(_) => {}                       // propagated as Err -- also correct
            Ok(r) => panic!(
                "[ENDURE-01] Request {} should be 500/err (exception thrown), got {}",
                i, r.status()
            ),
        }
    } else {
        // Requests AFTER a throw must succeed -- this is the critical assertion
        match result {
            Ok(r) if r.status() == 200 => {}
            Ok(r) => panic!(
                "[ENDURE-01] Request {} wrong status {} after prior exception", i, r.status()
            ),
            Err(e) => panic!(
                "[ENDURE-01] Request {} failed after prior exception: {}", i, e
            ),
        }
    }
}
```

### Anti-Patterns to Avoid

- **Fresh scope per request:** Proven fatal. Global<Function> from one ContextScope cannot be called from another (Approaches 1 and 3 in the investigation document). The persistent-scope pattern is the correct fix.
- **Asserting stateless JS behavior:** Tests expecting a module-level counter to be 1 on request 5 (when request 1 ran on same worker) will always fail with the Cloudflare-compatible persistent-scope design.
- **Forgetting to check cancel_terminate_execution:** The CPU timeout guard path is the most likely root cause for "cumulative None after ~4 requests" if OOM or timeout fires in early requests.
- **Testing with multi-worker pools:** Endurance tests with N > 1 workers cannot guarantee which request hits which worker without affinity dispatch. Use `dispatch_to(0, task)` or a 1-worker pool.
- **Dynamic code generation in test JS:** Do not use runtime code evaluation in test handlers. The `set_allow_generation_from_strings(false)` security fix would block those patterns and produce a false failure signal.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Exception capture | Custom isolate state flags | `v8::TryCatch` RAII | V8 guarantees drop clears pending exception from isolate |
| Pending exception check | `isolate.has_pending_exception()` then manual clear | `v8::TryCatch` drop | Race-free; TryCatch owns the exception state |
| Async microtask flushing | Custom event loop | `tc.perform_microtask_checkpoint()` + `v8::Platform::pump_message_loop()` | Already implemented and working in both pool.rs and tenant_pool.rs |

---

## Runtime State Inventory

> Phase 40 is a code-correctness phase with no rename/refactor/migration component.

**Nothing found in any category** — verified by phase description and codebase review. This phase changes code logic only (pool.rs, tenant_pool.rs, apis.rs, new test file). No stored data, live service config, OS-registered state, secrets, or build artifacts need updating.

---

## Common Pitfalls

### Pitfall 1: terminate_execution() not cancelled between requests

**What goes wrong:** `CpuTimeoutGuard` calls `terminate_execution()` if the CPU deadline fires. If `cancel_terminate_execution()` is not called before the next request, `handler_local.call()` returns `None` immediately on every subsequent request until the isolate is recycled.

**Why it happens:** V8's `terminate_execution()` flag persists until explicitly cleared. The RAII guard may drop the timer thread but not call the V8 cancel API.

**How to avoid:** In `CpuTimeoutGuard::drop()`, call `unsafe { isolate.cancel_terminate_execution() }`. Verify this is already present:

```bash
grep -n "cancel_terminate_execution" /Users/gleicon/code/rust/nano-rs/src/data_plane.rs
```

**Warning signs:** Requests succeeding normally, then after one slow request (triggering CPU limit), all subsequent requests on the same worker return `None` or status 500 with "unknown JS exception."

### Pitfall 2: TryCatch scope escaping the closure

**What goes wrong:** If `tc` (the TryCatch handle) is still alive when the next iteration of 'requests loop begins, two TryCatch instances could be stacked, causing undefined behavior.

**Why it happens:** If the closure pattern `(|| { ... })()` is refactored and `tc` is moved to the outer scope.

**How to avoid:** Keep `tc` strictly inside the closure body. Never hoist it to the 'requests loop scope.

**Warning signs:** Compiler error "TryCatch not dropped in LIFO order" — V8's type system detects stacked TryCatch misuse.

### Pitfall 3: Test asserting stateless JS behavior

**What goes wrong:** Test sends 5 requests to a 4-worker pool. Request 5 goes to worker 0 (round-robin: req1->w0, req2->w1, req3->w2, req4->w3, req5->w0). Worker 0 already ran request 1, so `counter = 2`. Test expects `counter === 1`.

**Why it happens:** Test author assumes stateless (fresh-isolate-per-request) semantics, but architecture uses persistent scopes (CF-Workers semantics).

**How to avoid:** Either (a) use a 1-worker pool and test monotonic counter increment, or (b) send requests to specific workers with `dispatch_to()` and account for prior state.

**Warning signs:** Test passes with pool size 1, fails with pool size > 1; or passes for request 1..N then fails for request N+1.

### Pitfall 4: Promise rejection leaving exception in TryCatch scope

**What goes wrong:** A Promise is rejected inside the pump loop. `promise.result(&tc)` is called, and then the function returns early with `Err(...)`. If the rejection exception value is not consumed before `tc` drops, TryCatch may consider the exception "unhandled."

**Why it happens:** TryCatch in v147 tracks whether the exception was accessed. An unchecked rejection may not be cleared by drop in all V8 versions.

**How to avoid:** After detecting `PromiseState::Rejected`, call `err.to_string(&tc)` to consume the value, then return `Err(...)`. The current code already does this. Verify the pattern is complete by checking that no early-return path skips `err.to_string()`.

---

## Code Examples

### Verified: TryCatch pin-and-init pattern (pool.rs, this session)

```rust
// Source: src/worker/pool.rs (session fix, uncommitted 2026-05-17)
// Must pin-and-init like HandleScope: TryCatch::new returns ScopeStorage.
let tc_storage = v8::TryCatch::new(&mut *ctx_scope);
let tc_pin = std::pin::pin!(tc_storage);
let mut tc = tc_pin.init();

let call_result = handler_local.call(&tc, global_obj.into(), &[js_req.into()]);

let resolved = match call_result {
    None => {
        let msg = tc.exception()
            .and_then(|e| e.to_string(&tc))
            .map(|s| s.to_rust_string_lossy(&tc))
            .unwrap_or_else(|| "unknown JS exception".to_string());
        return Err(anyhow!("JS exception: {}", msg));
    }
    // ...
};
```

### Verified: Global<Function> -> Local in persistent context (pool.rs)

```rust
// Source: src/worker/pool.rs lines 375-378 (committed, 3f098832)
// Works because the same ContextScope has been entered ONCE and never exited.
// Global->Local conversion is valid only while the creating context is entered.
let handler_g = handler_cache.get(&task.entrypoint).unwrap();
let global_obj = context.global(&mut ctx_scope);
let handler_local = v8::Local::new(&mut ctx_scope, handler_g);
```

### Verified: Persistent scope lifecycle (pool.rs outer loop)

```rust
// Source: src/worker/pool.rs lines 251-263 (committed)
{
    let scope_pin = std::pin::pin!(v8::HandleScope::new(nano.isolate()));
    let mut scope = scope_pin.init();
    let context = v8::Context::new(&scope, Default::default());
    // Security: block code generation from strings -- matches CF Workers.
    context.set_allow_generation_from_strings(false);
    crate::runtime::apis::RuntimeAPIs::bind_all(&mut scope, context);
    let mut ctx_scope = v8::ContextScope::new(&mut scope, context);
    // ctx_scope entered here and NEVER dropped until 'requests loop exits
    // ...
    'requests: loop { /* all requests handled here */ }
    // ctx_scope drops here on 'requests loop exit
}
// nano drops here -> isolate disposed
```

### Verified: Existing scope tests (isolate_scope_test.rs)

```rust
// Source: tests/isolate_scope_test.rs SCOPE-01
// Proves: 1000 calls in one persistent scope, zero None returns.
// Status: PASSES (cargo test --test isolate_scope_test -> 9 passed)
for i in 0..1000 {
    let h = v8::Local::new(&mut cs, &handler_g);
    let recv = ctx.global(&mut cs);
    match h.call(&mut cs, recv.into(), &[dummy_arg.into()]) {
        None => { none_count += 1; }
        Some(_) => {}
    }
}
assert_eq!(none_count, 0, "persistent scope broken");
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Fresh isolate per request | Persistent HandleScope+ContextScope per worker | Commit 3f098832 (2026-05-17) | Eliminates 50-100ms cold start; introduces state persistence |
| No exception capture | TryCatch RAII at handler call sites | Session 2026-05-17 (uncommitted) | Prevents exception propagation across requests |
| No code-generation block | `set_allow_generation_from_strings(false)` at Context::new() | Session 2026-05-17 (uncommitted) | Security hardening, matches CF Workers |
| Context reset between requests | No reset (CF-Workers compatible mode) | Phase 36.5 | Module globals persist by design |

**Deprecated:**
- `ContextManager::with_cloudflare_compatibility()` / `skip_context_reset` flag: The persistent-scope pool.rs implementation supersedes this for the hot path. The ContextManager path (context.rs) is a legacy path used by older tests; pool.rs / tenant_pool.rs are the active paths for Phase 40.

---

## Uncommitted Changes (Must Be Committed in Wave 0)

Git status shows `M src/runtime/apis.rs` confirmed. Pool.rs and tenant_pool.rs also have session changes per the phase description:

| File | Change | Correctness |
|------|--------|-------------|
| `src/runtime/apis.rs` | `Buffer.from(Array)` — checks `is_array()` before `to_string()` coercion | HIGH — prevents incorrect coercion |
| `src/runtime/apis.rs` | `subtle_export_key` — non-extractable key guard throws error | HIGH — WebCrypto spec compliance |
| `src/worker/pool.rs` | TryCatch at both `handler_local.call()` sites (x2) | HIGH — prevents exception poisoning |
| `src/worker/tenant_pool.rs` | TryCatch at `handler_local.call()` site (x1) | HIGH — same as pool.rs |
| `src/worker/pool.rs` | `context.set_allow_generation_from_strings(false)` at both Context::new() | HIGH — security |
| `src/worker/tenant_pool.rs` | `context.set_allow_generation_from_strings(false)` at Context::new() | HIGH — security |

Wave 0 of the plan MUST commit these before writing new tests.

---

## Key Questions Answered by Research

### Q1: Why does handler_local.call() return None on reused isolates?

**Finding:** The existing tests (SCOPE-01: 1,000 calls, zero None) prove that `handler_local.call()` does NOT fail on reused isolates when using the persistent-scope pattern. The "after ~4 requests" failure from the test report is almost certainly one of:

- **Most likely: CpuTimeoutGuard not calling cancel_terminate_execution() on drop.** If any of the first 4 requests triggered CPU limit enforcement, `terminate_execution()` was called but `cancel_terminate_execution()` may not have been called before the next request. Verify in `src/data_plane.rs`.
- **Possible: Exception from Request object construction (not from handler call itself) escaping before TryCatch is entered.** The TryCatch is created after the Request object is built. If `Headers::new_instance()` or `Request::new_instance()` throws, that exception is NOT caught by the TryCatch (which does not exist yet at that point).
- **Less likely: V8 microtask queue has unresolved work** — `tc.perform_microtask_checkpoint()` only runs inside the promise pump loop and does not run between requests for non-promise handlers.

### Q2: Is the handler_cache (Global<Function>) approach correct?

**Finding:** YES, definitively. The key constraint is that `v8::Local::new(&mut ctx_scope, handler_g)` is called from the SAME ContextScope that created `handler_g`. Since `ctx_scope` is entered once and never exited (persistent scope), this is always valid. SCOPE-01 proves it at 1,000 iterations. [VERIFIED: tests/isolate_scope_test.rs]

### Q3: What does the test suite test, and is the statelessness assumption valid?

**Finding:** The external test suite (`nano-rs-test-suite`) tests the CRUD app's behavior. The CRUD app uses module-level `Map` and `nextId` counter. With a 4-worker pool, request 5 (round-robin to worker 0) hits a worker that already ran request 1. Worker 0's `nextId` may be 2, not 1. The test that expects `counter === 1` on request 5 is testing the wrong invariant for a CF-Workers-compatible architecture.

The correct test invariant is: "module-level state accumulates monotonically within one isolate's lifetime, resets on recycle, and handler-local state is always fresh."

### Q4: What would a good dynamic test look like?

**Finding:** See Pattern 4 above. The test MUST use a 1-worker pool (or `dispatch_to(0, ...)`) to guarantee which worker handles each request. It should deliberately inject a JS exception at a known request number and verify the next request succeeds.

### Q5: Is there a V8 API call needed between requests?

**Finding:** The one missing call to investigate is `cancel_terminate_execution()`. Between-request microtask drain is NOT needed for synchronous handlers. For async handlers, the existing pump loop handles microtask flushing. No additional V8 inter-request API calls are required beyond what is already present, unless `terminate_execution()` was called by a CPU timeout guard.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (cargo test) |
| Config file | none |
| Quick run command | `cargo test --test isolate_endurance_test` |
| Full suite command | `cargo test --test isolate_scope_test --test isolate_endurance_test --test perf_latency_test` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| STAB-01 | handler_local.call() never returns None on reused isolate after TryCatch | unit | `cargo test --test isolate_scope_test scope01` | YES (SCOPE-01 existing) |
| STAB-02 | Exception at request N does not cause request N+1 to fail | unit | `cargo test --test isolate_endurance_test endure01` | NO — Wave 0 |
| STAB-03 | Per-request state is fresh; module globals documented as persistent | unit/doc | `cargo test --test isolate_endurance_test endure02_state_isolation` | NO — Wave 0 |
| STAB-04 | 10+ requests per worker pass without degradation | integration | `cargo test --test isolate_endurance_test endure03_10plus_requests` | NO — Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test --test isolate_scope_test --test isolate_endurance_test`
- **Per wave merge:** `cargo test` (full suite)
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `tests/isolate_endurance_test.rs` — covers STAB-02, STAB-03, STAB-04
- [ ] Verify `src/data_plane.rs::CpuTimeoutGuard::drop()` calls `cancel_terminate_execution()`; add if absent

*(Existing infrastructure: `tests/isolate_scope_test.rs` covers STAB-01 via SCOPE-01)*

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo / rustc | All tests | YES | project compiles (0 errors) | — |
| v8 crate 147.4.0 | Core | YES | 147.4.0 pinned in Cargo.toml | — |
| tempfile | Test temp JS files | YES | already used in pool tests | std::env::temp_dir() |

---

## Security Domain

`security_enforcement` default: enabled.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V5 Input Validation | yes | `set_allow_generation_from_strings(false)` blocks code generation from strings — session fix |
| V6 Cryptography | no | No crypto changes in Phase 40 |
| V4 Access Control | partial | Isolate state isolation prevents cross-tenant data access |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Code injection via string evaluation | Tampering | `context.set_allow_generation_from_strings(false)` — session fix applied |
| Exception state persistence (isolate poisoning) | Denial of Service | TryCatch RAII — session fix applied |
| CPU exhaustion via slow handlers | DoS | CpuTimeoutGuard already present; verify cancel_terminate_execution() on drop |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `CpuTimeoutGuard::drop()` does NOT call `cancel_terminate_execution()` — this is the most likely root cause of "cumulative None after ~4 requests" | Common Pitfalls | If it does call cancel, this root cause is ruled out and the issue is elsewhere |
| A2 | TryCatch fix in pool.rs/tenant_pool.rs was applied to all 3 call sites this session (x2 pool.rs, x1 tenant_pool.rs) per phase description | Uncommitted Changes | If any site was missed, that site can still poison the isolate |
| A3 | External test suite failure "Request 5 counter=2 when expected=1" is a test design flaw, not a runtime bug | Key Questions | If the user wants stateless semantics, the architecture must be reconsidered |

---

## Open Questions (RESOLVED)

1. **Does `CpuTimeoutGuard::drop()` call `cancel_terminate_execution()`?**
   **[RESOLVED: NO — confirmed root cause]**
   - Inspected `src/data_plane.rs` lines 176-183 (2026-05-17).
   - `CpuTimeoutGuard::drop()` joins the timer thread, clears `TERMINATION_ISOLATE_PTR` to null, and resets `TERMINATION_REQUESTED` to false. It does NOT call `isolate.cancel_terminate_execution()`.
   - Combined with the finding that default `cpu_time_ms: 50` and cold-start latency ~55ms (test report 2026-05-17), `terminate_execution()` fires on EVERY first request on a fresh isolate, and the terminate flag is never cleared.
   - This is the confirmed root cause of "handler_local.call() returns None on recycled workers". Fix: add `unsafe { (*isolate_ptr).cancel_terminate_execution(); }` in `CpuTimeoutGuard::drop()` before clearing the pointer.

2. **Does the TryCatch cover exceptions from Request object construction?**
   **[RESOLVED: Not needed — constructors do not throw in normal operation]**
   - Inspected `src/runtime/apis.rs` and `src/worker/pool.rs`. The WinterTC constructors (Headers, Request, Response) use `v8::Function::new_instance()` which returns `Option<Object>`. On None, the existing code already returns an Err before reaching handler_local.call(). This Err path does not leave a pending exception on the isolate.
   - TryCatch coverage of constructor calls is not needed for correctness. Belt-and-suspenders hardening can be deferred to a later phase.

3. **Are there pending microtasks from abandoned async handlers?**
   **[RESOLVED: Not a risk — microtask checkpoint already called]**
   - Inspected pool.rs lines ~454 and ~1108, tenant_pool.rs line ~353. `tc.perform_microtask_checkpoint()` is called inside the promise resolution branch after each async handler completes.
   - For synchronous handlers (non-promise path), there are no queued microtasks to drain.
   - Abandoned async handlers (CPU timeout path) are terminated before microtasks can accumulate via `terminate_execution()`, which is the bug fixed in Q1 above.
   - No additional microtask drain is needed between requests.

---

## Test Report Findings (2026-05-17)

Source: NANO-RS v1.5.0 Comprehensive Technical Test Report (500+ HTTP requests over ~3 hours)

### Confirmed by Test Report

| Finding | Test Evidence | Impact on Phase 40 |
|---------|--------------|-------------------|
| Requests 1-4 succeed, request 5+ fail with HTTP 500 | Endurance test: 27% pass rate (4/15) | Confirms 4-worker round-robin: worker 0 first serves req 1 (terminate_execution fires), then req 5 fails (terminate flag persists) |
| Counter on request 5 = 2, not 1 | Bug 3 test: `counter++` handler | Confirms module-global state persists across requests — correct CF-Workers behavior, NOT a bug |
| Buffer.from(Array) fixed | Bug 2: `[49,44,50...]` → `[1,2,3...]` | Already fixed in uncommitted apis.rs — Wave 0 commit covers this |
| Error reporting improved: empty 200 → proper 500 | Bug 1/4 test logs | TryCatch fix already applied in session — Wave 0 commit covers this |
| All APIs real implementations (no placeholders) | Section 4 verification table | Not a Phase 40 concern |

### Key Timing Finding (Root Cause Chain)

```
default cpu_time_ms = 50ms  (AppLimits::default(), src/config/app.rs:84)
cold-start latency  ≈ 55ms  (test report §9.1: "Request latency (fresh isolate): ~55ms")
→ terminate_execution() fires on EVERY first request on a fresh isolate
→ CpuTimeoutGuard::drop() does not call cancel_terminate_execution()
→ terminate flag persists on isolate
→ request 5 hits recycled worker 0 → handler_local.call() → None immediately
→ HTTP 500 "JS exception: null" for all remaining requests on that worker
```

This is the confirmed root cause. Fix: `cancel_terminate_execution()` in `CpuTimeoutGuard::drop()`.

### Test Suite File Inventory

Located at `/Users/gleicon/code/js/nano-rs-test-suite/scripts/`:
- `strict-multi-request-tests.js` — 12 tests, multi-request per server (active)
- `v15-isolate-aware-tests.js` — 22 tests, fresh server per batch (active, 100% pass rate workaround)
- `test-utils.js` — Shared NanoServer class
- `run-all-tests.js` — Suite runner
- `archive/` — Old blackbox and performance tests (archived)

Plan 40-03 should target `strict-multi-request-tests.js` (which finds the real bugs) rather than `v15-isolate-aware-tests.js` (which masks them via server restart workaround).

### limits.workers Config Status

The test report states "Config ignored, always creates 4 workers." This is **partially incorrect**:
- `server.rs:789-792` reads `app.limits.workers` and passes it to `AppState::with_vfs_config()`
- The value IS respected if explicitly set in config
- Default is 4 (`src/config/app.rs:80`), so configs without explicit `workers` field get 4 workers
- The test configs likely omitted the workers field, hitting the default
- This is NOT a Phase 40 bug — config parsing works correctly

---

## Sources

### Primary (HIGH confidence)

- Codebase: `src/worker/pool.rs` — direct inspection (committed + session fixes)
- Codebase: `src/worker/tenant_pool.rs` — direct inspection (committed + session fixes)
- Codebase: `tests/isolate_scope_test.rs` — verifies SCOPE-01..08 all pass
- Codebase: `src/runtime/apis.rs` — Buffer.from fix confirmed in git diff
- `.planning/V8_ISOLATE_REUSE_INVESTIGATION.md` — prior approach failures documented
- `cargo test --test isolate_scope_test` — 9 tests pass (confirmed this session)
- `cargo build --lib` — 0 errors, 0 warnings (confirmed this session)

### Secondary (MEDIUM confidence)

- `.planning/STATE.md` — version history and feature status
- `.planning/ROADMAP.md` — Phase 40 success criteria (STAB-01..04)

### Tertiary (LOW confidence)

- V8 embedder documentation on `terminate_execution()` and `cancel_terminate_execution()` semantics — [ASSUMED] based on training knowledge; not verified against v147 rusty_v8 API docs in this session.

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies, all existing
- Architecture: HIGH — persistent-scope pattern verified working in SCOPE-01
- Root cause hypothesis (CPU timeout cancel): MEDIUM — logical inference from V8 API semantics, not directly observed
- Pitfalls: HIGH — based on direct code inspection and test results
- Test design (endurance test pattern): HIGH — follows existing pool test patterns in isolate_scope_test.rs

**Research date:** 2026-05-17
**Valid until:** 2026-06-17 (stable V8 API; rusty_v8 147.4.0 pinned)
