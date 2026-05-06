# WebAssembly (WASM) Issue Investigation Report

**Date:** 2026-05-06  
**Status:** ROOT CAUSE IDENTIFIED - Partial Fix Applied  
**Severity:** HIGH - WASM execution broken in production

---

## Problem Statement

WebAssembly compilation fails in nano-rs with error:
```
WebAssembly.compile(): section was shorter than expected 
size (9 bytes expected, 7 decoded) @+30
```

Same WASM binary works correctly in Node.js, confirming the binary is valid.

---

## Investigation Results

### ✅ What Works (Unit Tests)

1. **WASM Binary Preservation** - `tests/wasm_binary_debug_test.rs`
   - All 7 tests pass
   - Bytes preserved through all system layers
   - VFS correctly stores and retrieves binary data

2. **WASM Promise Resolution** - `tests/wasm_promise_resolution_test.rs`
   - All 3 tests pass
   - `WebAssembly.validate()` works correctly
   - V8 Promise resolution works

3. **WASM Integration** - `tests/wasm_integration_test.rs`
   - All 8 tests pass
   - Core WASM functionality works in isolation

### ❌ What Fails (Production/Blackbox Tests)

When running through the actual HTTP server:
```javascript
const response = await fetch('./add.wasm');
const bytes = await response.arrayBuffer();
const module = await WebAssembly.compile(bytes); // FAILS HERE
```

Error: `section was shorter than expected (9 bytes expected, 7 decoded)`

---

## Root Cause Analysis

### Finding 1: Binary Data Corruption in Response Path

**Location:** `src/runtime/fetch.rs:523-524`

```rust
// FALLBACK PATH (problematic)
if let Some(body_str) = body_val.to_string(scope) {
    Bytes::from(body_str.to_rust_string_lossy(scope).into_bytes())
}
```

When Response.arrayBuffer() falls back to reading the body property:
1. Binary bytes are converted to V8 string using `to_string()`
2. V8 strings are UTF-16, binary bytes become corrupted
3. Converting back to bytes produces truncated/corrupted data

**Impact:** WASM binaries > certain size get corrupted

### Finding 2: StaticFile Handler Corrupts Binaries

**Location:** `src/http/router.rs:314`

```rust
HandlerType::StaticFile { path, content_type } => {
    match tokio::fs::read_to_string(path).await {  // ❌ TEXT ONLY
```

`read_to_string()` reads as UTF-8 text, corrupting binary files.

**Impact:** WASM files served via StaticFile handler are corrupted

### Finding 3: External Data Storage Works Correctly

**Location:** `src/runtime/fetch.rs:515-517`

```rust
if let Some(data) = get_response_data(scope, this) {
    data.body.clone()  // ✅ BYTES PRESERVED
}
```

When using external Response data storage (primary path), bytes are preserved.

---

## Why Unit Tests Pass but Production Fails

| Scenario | Data Path | Result |
|----------|-----------|--------|
| Unit tests | Direct V8 execution | ✅ Works |
| Unit tests | External Response data | ✅ Works |
| Production | HTTP fetch + Response.body | ❌ Corrupted |
| Production | Static file serving | ❌ Corrupted |

The issue only manifests when:
1. HTTP layer is involved (fetch/Response)
2. Binary data goes through text conversion

---

## Recommended Fixes

### Fix 1: Update StaticFile Handler (HIGH PRIORITY)

**File:** `src/http/router.rs:310-323`

```rust
HandlerType::StaticFile { path, content_type } => {
    match tokio::fs::read(path).await {  // Use read() not read_to_string()
        Ok(bytes) => NanoResponse::ok()
            .with_header("Content-Type", content_type)
            .with_body_bytes(bytes),
        Err(e) => {
            tracing::warn!("Failed to read static file {}: {}", path, e);
            NanoResponse::not_found()
        }
    }
}
```

### Fix 2: Add Binary-Safe Fallback in Response.arrayBuffer()

**File:** `src/runtime/fetch.rs:508-542`

Options:
1. Remove the fallback path entirely (always require external data)
2. Store body as Uint8Array in JS instead of string
3. Add explicit binary marker to distinguish text vs bytes

### Fix 3: Add WASM Content-Type Detection

**File:** `src/http/router.rs`

For `.wasm` files, ensure:
- Content-Type: `application/wasm`
- Binary-safe read path used

---

## Workarounds (Until Fixed)

### Option 1: Use StaticDir Instead of StaticFile

```javascript
// Serve WASM from a directory, not as single static file
nano-rs run --dir ./public  // Use directory serving
```

`StaticDir` uses `read()` (binary-safe), `StaticFile` uses `read_to_string()` (text-only).

### Option 2: Inline WASM as Base64

```javascript
// Embed WASM directly in JS as base64
const wasmBase64 = "AGFzbQEAAAAB...";
const bytes = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));
const module = await WebAssembly.compile(bytes);
```

### Option 3: Use JavaScript Implementation

For simple WASM modules, implement the logic in JavaScript as fallback.

---

## Verification Checklist

After fixes are applied:

- [ ] `StaticFile` handler uses `read()` not `read_to_string()`
- [ ] `.wasm` files serve with correct `Content-Type: application/wasm`
- [ ] WASM binaries > 100 bytes compile successfully
- [ ] `response.arrayBuffer()` returns uncorrupted bytes
- [ ] Blackbox WASM tests pass (currently 1/4)
- [ ] `tests/wasm_*_test.rs` all pass (currently passing)

---

## Related Issues

- **Issue #1:** WASM compile fails (this issue)
- **Issue #2:** V8 v147 scope lifetime (FIXED in handler.rs)
- **Issue #3:** CRUD tests need Phase 35 engine unification

---

## V8 Version Information

**V8 Engine:** 14.7.173.20-rusty  
**V8 Crate:** 147.4.0 (rusty_v8)  
**Node.js Equivalent:** ~v20.x

The V8 version supports WASM fully. The issue is in nano-rs's data handling, not V8.

---

## Conclusion

**WASM is NOT fundamentally broken in V8.** The issue is in how nano-rs handles binary data through the HTTP → JavaScript → V8 pipeline.

**Immediate Action:** Apply Fix 1 (StaticFile handler) to resolve most WASM serving issues.

**Long-term:** Refactor Response body handling to never convert binary data through strings.
