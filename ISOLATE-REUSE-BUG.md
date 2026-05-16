# Isolate Reuse Bug Documentation

**Status:** Known Issue - Under Investigation  
**Severity:** Critical  
**Affected Versions:** v1.5.0  
**First Identified:** 2026-05-16

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

The issue is in `src/data_plane.rs` in the `execute_with_context_manager` function. The V8 context/scoping lifetime management uses `std::mem::transmute` to force `'static` lifetimes on V8 handles:

```rust
// v147 API: HandleScope requires pin! + init pattern
let result = unsafe {
    let mut scope_storage = v8::HandleScope::new(isolate);
    let scope_pin = Pin::new_unchecked(&mut scope_storage);

    let mut handle_scope: v8::PinnedRef<'static, v8::HandleScope> =
        std::mem::transmute(scope_pin.init());

    let v8_context: v8::Local<'static, v8::Context> = match global_ctx {
        Some(g) => std::mem::transmute(v8::Local::new(&mut handle_scope, &g)),
        None => return Err(anyhow!("No context available")),
    };

    let mut context_scope: v8::ContextScope<'static, 'static, v8::HandleScope<'static, v8::Context>> =
        std::mem::transmute(v8::ContextScope::new(&mut handle_scope, v8_context));

    let exec_result = execute_handler_code(
        std::mem::transmute(&mut context_scope),
        std::mem::transmute(v8_context),
        handler_ctx
    );
    // ...
};
```

### Problem

1. **Context Reset:** `ContextManager::reset_context()` creates a new V8 context and stores it as a `v8::Global<v8::Context>`
2. **Global Cloning:** `execute_with_context_manager` clones this Global before execution
3. **Local Creation:** Inside the unsafe block, `v8::Local::new()` creates a Local handle from the Global
4. **Lifetime Transmute:** The transmute to `'static` erases Rust's lifetime tracking

After context reset, the Global should still be valid (it's a persistent handle), but creating a Local from it in a new HandleScope appears to fail silently, causing script execution to throw an exception.

### What Works

- Fresh isolates: First request to each worker succeeds
- API binding: `RuntimeAPIs::bind_all()` succeeds ("Fetch state initialized successfully" is logged)
- Script compilation: `v8::Script::compile()` succeeds

### What Fails

- Script execution: `script.run(scope)` returns `None` (exception thrown)

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

### Option 1: Fix V8 Scope Lifetime Management

Eliminate the transmute to `'static` and properly manage V8 handle lifetimes:

```rust
// Instead of transmuting to 'static, properly nest scopes
let mut scope_storage = v8::HandleScope::new(isolate);
let scope_pin = Pin::new_unchecked(&mut scope_storage);
let handle_scope = scope_pin.init();

// Create Local without transmute - it borrows from handle_scope
let v8_context = v8::Local::new(handle_scope, &global)
    .ok_or_else(|| anyhow!("Failed to create context local"))?;

// Create ContextScope without transmute
let mut context_scope = v8::ContextScope::new(handle_scope, v8_context);

// Execute without transmute - pass references with correct lifetimes
execute_handler_code(&mut context_scope, v8_context, handler_ctx)?
```

**Challenges:**
- Requires significant refactoring of V8 integration code
- May need to restructure `execute_handler_code` signature
- Need to ensure no "active scope" V8 errors

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
- **Current:** Documented as known issue, awaiting fix

---

## References

- V8 API documentation: https://v8.github.io/api/head/
- rusty_v8 crate: https://docs.rs/v8/latest/v8/
- Original investigation report: See NANO-RS v1.5.0 Test Suite Investigation Report
