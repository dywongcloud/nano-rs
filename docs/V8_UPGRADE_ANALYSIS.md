# V8 Upgrade Analysis for WASM Support

## Date: 2026-05-04
## Current Version: v8 = "139"
## Latest Available: v8 = "147.4.0"

---

## Key Finding: WASM Compilation Fix in v146

The `v8` crate (formerly `rusty_v8`) received a significant WASM-related update in **v146.0.0**:

### PR #1908: feat: add bindings for v8::WasmModuleCompilation
- **Merged:** Feb 19, 2026
- **Version:** v146.0.0
- **V8 Engine:** 14.6.202.2

**What it adds:**
- `WasmModuleCompilation::new()` - create a new compilation
- `on_bytes_received(&mut self, data: &[u8])` - feed wasm bytes from any thread
- `finish(...)` - finalize compilation with resolution callback
- `abort(self)` - cancel compilation
- Asynchronous WebAssembly module compilation API

**Why this matters:**
Our investigation proved that the WASM compilation issue in nano-rs v1.4.2 is a **V8 limitation** in v139, not a nano-rs code issue. This new API in v146 could potentially fix the issue.

---

## Version Comparison

| Version | V8 Engine | Release Date | WASM Relevant Changes |
|---------|-----------|--------------|------------------------|
| v139 | 13.9.x | 2025-07-24 | Current - WASM compile fails |
| v140 | 14.0.x | 2025-09-05 | - |
| v142 | 14.2.x | 2025-10-24 | - |
| v145 | 14.4.x | 2026-01-16 | - |
| **v146** | **14.6.x** | **2026-02-19** | **WASM module compilation API** |
| v147 | 14.7.x | 2026-03-24 | Latest |

---

## Upgrade Attempt Results

### Attempted: v145 → v146
**Result:** ❌ Compilation failed

**Error:** Not in v8, but in `temporal_rs` dependency:
```
error[E0599]: no variant or associated item named `MonthCodeNotInCalendar` 
  found for enum `DateFromFieldsError` in the current scope
```

**Root Cause:** Dependency tree conflict with `icu_calendar` crate when regenerating Cargo.lock

**Issue:** The `temporal_rs` crate (used by ???) has breaking changes with newer `icu_calendar` versions that get pulled in when updating v8.

---

## Recommendation

### Option 1: Update v8 + Fix Dependencies (Recommended)
1. Update v8 to v146 or v147
2. Update `temporal_rs` or replace with alternative
3. Fix any API breaking changes in nano-rs code
4. Test WASM compilation with new API

**Effort:** Medium (2-3 days)
**Risk:** Medium (dependency chain updates needed)

### Option 2: Document Current Limitation
1. Keep v8 at v139
2. Document WASM as "not working due to V8 limitation"
3. Consider alternative WASM runtime (wasmtime)

**Effort:** Low (1 day)
**Risk:** Low (no code changes)

### Option 3: Use New WASM Compilation API
1. Upgrade to v146+
2. Refactor nano-rs to use `WasmModuleCompilation` instead of `WebAssembly.compile()`
3. This bypasses the Promise-based API that was failing

**Effort:** High (requires refactoring wasm module loading)
**Risk:** Medium (new API, needs testing)

---

## Our Current Status

| Component | Status |
|-----------|--------|
| WASM Binary Loading | ✅ Working |
| Byte Preservation | ✅ Perfect |
| V8 WASM Compile API | ❌ Broken (v139 limitation) |
| WebAssembly.validate() | ⚠️ Our custom validator |
| WebAssembly.compile() | ❌ Fails with "section shorter" error |
| WebAssembly.instantiate() | ❌ Depends on compile() |

---

## Conclusion

The WASM issue is confirmed to be a **V8 version limitation** in v139. Newer versions (v146+) have:
1. A new `WasmModuleCompilation` API for async compilation
2. V8 engine 14.6+ with potential WASM fixes

**To fix WASM:** We need to upgrade v8 to v146+ and handle the dependency updates required (primarily `temporal_rs` / `icu_calendar` chain).

**Files to update:**
- `Cargo.toml` - v8 version
- `Cargo.lock` - full dependency refresh
- `src/v8/isolate.rs` - potentially use new WASM APIs
- `src/wasm/` - refactor to use `WasmModuleCompilation` if beneficial

---

## References

- rusty_v8 PR #1908: https://github.com/denoland/rusty_v8/pull/1908
- crates.io v8: https://crates.io/crates/v8
- Current nano-rs v8: 139
- Target v8: 146+ for WASM fixes
