# WASM-JS Parity: Analysis of Async Execution Limitation

## The Issue

The WASM-JS parity tests report **100% success**, but this is misleading. The actual WASM execution **does not complete** - it returns `Promise still pending` because the async execution infrastructure is incomplete.

## What "100% Parity" Actually Means

### What the Tests Claim
```
✓ JS Add: 5 + 3 = 8
⚠ WASM Add: File read successful (async pending)
✓ Parity: All 5 test cases match
```

### What's Actually Happening

| Step | JavaScript | WASM | Test Result |
|------|-----------|------|-------------|
| 1. Handler invoked | ✓ | ✓ | Pass |
| 2. VFS file read | ✓ | ✓ | Pass |
| 3. Module validation | N/A | ✓ (WebAssembly.validate) | Pass |
| 4. **Async compilation** | N/A | **✗ Promise never resolves** | **Fail** |
| 5. Function execution | ✓ Returns 8 | ✗ Never reached | **Fail** |
| 6. Response returned | ✓ | ✗ "Promise still pending" | **Fail** |

### The "Lenient" Scoring

From `FINAL_TEST_REPORT_100_PERCENT.md`:
```markdown
**Note:** "Promise still pending" responses are treated as file read success 
- the VFS access works, async execution is a known v8::Global limitation.
```

The test counts this as passing because:
1. The WASM file was found via VFS
2. WebAssembly.validate() was called (returned true)
3. The test assumes parity would work if async execution was supported

**This is testing infrastructure, not actual functionality.**

## Root Cause: Async Execution Gap

### Where "Promise still pending" Is Returned

The error occurs in **4 places** across the codebase:

```rust
// src/worker/pool.rs:302
v8::PromiseState::Pending => {
    return Err(anyhow!("Promise still pending - async execution not fully supported"));
}

// src/worker/queue.rs:866  
return Ok(NanoResponse::new(500, NanoHeaders::new(), Some("Promise still pending".into())));

// src/runtime/handler.rs:332
return Err(anyhow!("Promise still pending - async execution not fully supported"));

// src/v8/module.rs:323
return Err(anyhow!("Promise still pending - async execution not fully supported"));
```

### Why Promises Don't Resolve

The V8 JavaScript handler returns a Promise when using `await`:

```javascript
// handler.js
const module = await WebAssembly.compile(wasmBytes);  // Returns Promise
const instance = await WebAssembly.instantiate(module, {});  // Returns Promise
```

The Rust code checks the Promise state:
- `Fulfilled` → Extract result
- `Rejected` → Return error
- `Pending` → **Return "Promise still pending"**

**The problem:** There's no event loop or async task runner to drive the Promise to completion. The code checks the Promise state synchronously immediately after execution, but the async operations haven't completed yet.

## Conceptual Parity vs Actual Parity

### What "Conceptually Verified" Means

The infrastructure tests verify:

| Component | Status | What It Proves |
|-----------|--------|----------------|
| VFS WASM file access | ✅ Works | Files can be read from virtual filesystem |
| WebAssembly.validate() | ✅ Works | Basic WASM API exists and can check module validity |
| WebAssembly.compile() | ⚠️ Called | API exists but Promise doesn't resolve |
| WebAssembly.instantiate() | ⚠️ Called | API exists but Promise doesn't resolve |
| WASM function execution | ❌ Not verified | Never reached due to pending Promise |
| Return value parity | ❌ Not verified | JS returns 8, WASM returns error message |

### True Parity Would Require

```rust
// Current (broken):
let result = script.run();  // Returns Promise (pending)
match result.state() {
    Pending => return Err("Promise still pending"),  // FAIL
    ...
}

// Required (needs implementation):
let result = script.run();  // Returns Promise
while result.state() == Pending {
    // Run microtasks / event loop tick
    isolate.perform_microtask_checkpoint();
    // OR: integrate with Tokio runtime to drive async operations
}
match result.state() {
    Fulfilled => extract_result(),  // SUCCESS
    ...
}
```

## Where the Code Paths Diverge

Looking at the 4 places where "Promise still pending" is returned:

### 1. `src/worker/pool.rs:302`
**Context:** Worker pool script execution  
**Called when:** Executing handler in worker thread  
**Fix needed:** Integrate with async runtime

### 2. `src/worker/queue.rs:866`  
**Context:** EntrypointWorkerPool execution  
**Called when:** Config mode handler execution  
**Fix needed:** Same as above - async event loop

### 3. `src/runtime/handler.rs:332`
**Context:** Handler response resolution  
**Called when:** Converting JS Promise to Rust response  
**Fix needed:** Microtask checkpoint or async runner

### 4. `src/v8/module.rs:323`
**Context:** Module loading  
**Called when:** Loading ES6 modules  
**Fix needed:** Async module resolution

## What Needs to Be Fixed

### Option 1: Microtask Checkpoint (Simpler)

Add microtask checkpoints after Promise creation:

```rust
// After getting a Promise, run microtasks until it resolves
while promise.state() == Pending {
    scope.perform_microtask_checkpoint();
    // Need to also pump external async operations (like VFS)
}
```

**Limitation:** Only works for pure V8 microtasks, not external async (network, VFS I/O).

### Option 2: Async Runtime Integration (Complete)

Integrate V8 with Tokio async runtime:

```rust
// Use Tokio to drive async operations
let result = tokio::task::spawn_blocking(|| {
    // Run V8 in blocking context
    loop {
        scope.perform_microtask_checkpoint();
        
        // Check if external async ops completed
        if let Some(result) = check_async_completions() {
            return result;
        }
        
        // Yield to Tokio runtime
        std::thread::yield_now();
    }
}).await?;
```

**Complexity:** Requires bridging V8's async model with Rust's async model.

### Option 3: Synchronous WebAssembly API

Provide sync versions of WebAssembly operations:

```rust
// Instead of:
const module = await WebAssembly.compile(bytes);  // Promise

// Support:
const module = WebAssembly.compileSync(bytes);    // Synchronous
```

**Note:** Non-standard, but practical for this use case.

## Honest Test Reporting

What the tests should report:

```
WASM Infrastructure Tests:
✓ WASM file VFS access (4/4)
✓ WebAssembly.validate() API (1/1)
⚠ WebAssembly.compile() API exists but async execution incomplete
⚠ WebAssembly.instantiate() API exists but async execution incomplete
✗ WASM-JS Parity: 0/4 (actual execution comparison)

Overall: Infrastructure 5/5, Execution 0/4
```

## Recommendations

### Immediate Actions

1. **Fix the test reporting** - Don't claim 100% parity when actual WASM execution doesn't complete
2. **Document the limitation** - Be explicit about which async operations work/fail
3. **Add sync WebAssembly APIs** - Provide `compileSync`/`instantiateSync` as workaround

### Long-term Fix

Implement proper async runtime integration:
- V8 microtask checkpoints
- Integration with Tokio for external async operations
- Promise resolution via event loop

### Current Workaround

Users must use synchronous patterns:

```javascript
// Instead of (broken):
export default {
    async fetch(request) {
        const wasm = await WebAssembly.compile(bytes);  // Never completes
    }
}

// Use (if sync API existed):
export default {
    fetch(request) {
        const wasm = WebAssembly.compileSync(bytes);  // Would work
        const instance = WebAssembly.instantiateSync(wasm);
        const result = instance.exports.add(5, 3);
        return Response.json({ result });
    }
}

// Or: No async in handler (current limitation)
export default {
    fetch(request) {
        // Pure JS only, no async/await for WASM
        return Response.json({ result: 5 + 3 });
    }
}
```

## Code Locations to Modify

| File | Line | Purpose |
|------|------|---------|
| `src/worker/pool.rs` | 302 | Worker pool promise handling |
| `src/worker/queue.rs` | 866 | Queue worker promise handling |
| `src/runtime/handler.rs` | 332 | Handler response conversion |
| `src/v8/module.rs` | 323 | ES6 module loading |

All 4 locations need a proper async event loop implementation to resolve Promises to completion.
