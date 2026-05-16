# Isolate Reuse Bug Documentation

**Status:** PERSISTENT - Refactoring completed but root cause remains  
**Severity:** Critical  
**Affected Versions:** v1.5.0  
**First Identified:** 2026-05-16  
**Last Update:** 2026-05-16 (refactoring completed)

---

## Summary

After a V8 isolate handles its first request, all subsequent requests on that same isolate fail with HTTP 500. The script execution throws an exception when the context is reset between requests.

**IMPORTANT:** Code refactoring was completed to improve structure and remove transmute across function boundaries, but the underlying V8 context reset issue persists.

---

## Update History

### 2026-05-16 - Refactoring Completed

**Changes Made:**
- Inlined `execute_handler_code()` and `extract_js_response()` into `execute_with_context()`
- Eliminated transmute across function boundaries
- Simplified V8 scope management with proper drop ordering
- Removed unused imports

**Result:** Code is cleaner and more maintainable, but isolate reuse bug still present.

---

## Symptoms

---

## Summary

After a V8 isolate handles its first request, all subsequent requests on that same isolate fail with HTTP 500. The script execution throws an exception when the context is reset between requests.

---

## Symptoms

1. **First 4 requests** (one per worker): HTTP 200, body contains expected content
2. **Request 5+** (worker reuse): HTTP 500, body contains "Script execution failed"
3. Server logs show:
   ```
   Script execution threw exception for entrypoint: /path/to/app.js
   ```

---

## Root Cause Analysis

### Current Understanding (Post-Refactoring)

After extensive refactoring (2026-05-16), the issue appears to be at the V8 isolate level, not just Rust lifetime management:

1. **Context Reset:** `ContextManager::reset_context()` creates a new V8 context and stores it as a `v8::Global<v8::Context>`
2. **Global Validity:** The Global is valid and can create Local handles successfully
3. **API Binding:** `RuntimeAPIs::bind_all()` succeeds (all APIs are bound to the new context)
4. **Script Compilation:** `v8::Script::compile()` succeeds in the new context
5. **Script Execution:** `script.run(scope)` returns `None` - **this is where it fails**

### What Works

- Fresh isolates: First request to each worker succeeds
- First context in each isolate: Script executes successfully
- API binding: All WinterTC APIs are properly bound to new contexts
- Script compilation: V8 accepts and compiles scripts in reset contexts

### What Fails

- Second context in same isolate: Script execution throws exception
- The exception occurs at the V8 level, not Rust

### Updated Theory

The issue is likely at the **V8 isolate level**, not Rust code:

1. When `ContextManager::reset_context()` disposes the old context and creates a new one, some V8 isolate internal state may become corrupted
2. This could be related to how V8 tracks active contexts, global handles, or internal isolate state
3. The transmute in Rust is a red herring - it's necessary for the V8 API and works fine for the first context

### Previous Code Structure (Now Fixed)

**OLD:** Multiple transmutes across function boundaries (`execute_handler_code`, `extract_js_response`)

**NEW:** Single transmute within one function, all execution logic inlined

The refactoring eliminated complex lifetime interactions but did not fix the underlying V8 issue.

---

## Test Evidence

Run the isolate reuse test to see the bug:

```bash
cargo test --test isolate_reuse_test -- --nocapture
```

Expected output:
```
Request 1: PASSED - Body: 'Hello from worker'
Request 2: PASSED - Body: 'Hello from worker'
Request 3: PASSED - Body: 'Hello from worker'
Request 4: PASSED - Body: 'Hello from worker'
Request 5: KNOWN BUG - Isolate reuse issue (script execution fails after context reset)
Request 6: KNOWN BUG - Isolate reuse issue (script execution fails after context reset)
...
```

---

## Workarounds

### For Testing

Restart the server every 3-4 requests to ensure fresh isolates:

```javascript
// In test code, restart server between test batches
for (const batch of testBatches) {
    await server.restart();  // Fresh isolates
    for (const test of batch) {
        await runTest(test);  // Max 3-4 requests per restart
    }
}
```

### For Production

**NOT RECOMMENDED FOR PRODUCTION USE** until this bug is fixed.

If necessary, you could:
1. Configure single-request worker lifecycle (worker handles 1 request then restarts)
2. Use external process isolation (each request in a new process)

---

## Potential Fixes

### Option 1: Fix V8 Scope Lifetime Management (ATTEMPTED - PARTIAL)

**Status:** Refactoring completed 2026-05-16, but isolate reuse bug persists.

**What Was Done:**
- Inlined all handler execution logic to eliminate transmute across function boundaries
- Simplified scope management with proper drop ordering
- Reduced complexity by keeping all V8 operations in a single function

**Result:** Code is cleaner and more maintainable, but the underlying V8 context reset issue remains. The transmute within the single function is still required due to V8 API lifetime constraints.

**Remaining Issue:** Script execution throws exception after context reset, suggesting deeper V8 state management problem beyond just lifetime handling.

---

### Option 2: Avoid Context Reset

Instead of resetting contexts, use fresh isolates for each request:

```rust
// In worker pool, create new isolate per request instead of resetting context
// Trade-off: 50-100ms overhead vs <10ms context reset
```

**Challenges:**
- Significant performance degradation (5-10x slower)
- Defeats the purpose of worker pool architecture

### Option 2: Avoid Context Reset

Instead of resetting contexts, use fresh isolates for each request:

```rust
// In worker pool, create new isolate per request instead of resetting context
// Trade-off: 50-100ms overhead vs <10ms context reset
```

**Challenges:**
- Significant performance degradation (5-10x slower)
- Defeats the purpose of worker pool architecture

### Option 3: Create Context Within Execution Scope

Instead of creating context in `reset_context()` and storing as Global, create it fresh in `execute_with_context_manager`:

```rust
pub fn execute_with_context_manager(...) -> Result<NanoResponse> {
    // Don't clone pre-created context
    // Instead, create fresh context here within the HandleScope
    let isolate = context_manager.isolate_mut().isolate();
    
    let mut scope_storage = v8::HandleScope::new(isolate);
    // ... create context within this scope
}
```

**Challenges:**
- Changes ContextManager architecture
- May affect performance metrics
- Need to verify context caching behavior

---

## Related Code

- `src/data_plane.rs:269-365` - `execute_with_context_manager` function
- `src/worker/context.rs:84-112` - `reset_context` function
- `src/worker/pool.rs:355-379` - Context reset call in worker
- `tests/isolate_reuse_test.rs` - Regression test

---

## Timeline

- **2026-05-16:** Bug identified during test suite investigation
- **2026-05-16:** Refactoring completed - code structure improved but bug persists
  - Inlined handler execution functions
  - Eliminated transmute across function boundaries
  - Improved code maintainability
- **Current:** Under investigation - suspected V8 isolate-level issue

---

## References

- V8 API documentation: https://v8.github.io/api/head/
- rusty_v8 crate: https://docs.rs/v8/latest/v8/
- Original investigation report: See NANO-RS v1.5.0 Test Suite Investigation Report
