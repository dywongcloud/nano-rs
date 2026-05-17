---
phase: 40-pre-phase-23-stability
plan: wasm-cache
subsystem: wasm
tags: [wasm, cache, performance, v8, sha256]
dependency_graph:
  requires: []
  provides: [global_wasm_cache, compute_hash_sha256, WasmModuleCache_len]
  affects: [src/wasm/engine.rs, src/wasm/js_api.rs, src/wasm/mod.rs]
tech_stack:
  added: [OnceLock, sha2::Sha256]
  patterns: [process-global singleton, stable hash keys, dead_code infra]
key_files:
  created:
    - tests/isolate_wasm_cache_test.rs
  modified:
    - src/wasm/engine.rs
    - src/wasm/js_api.rs
    - src/wasm/mod.rs
decisions:
  - "Do not override WebAssembly.compile/instantiate JS API: v8::WasmModuleObject::compile returns None in rusty_v8 v147 (confirmed by ignored test_7 in wasm_binary_debug_test)"
  - "Use SHA-256 instead of DefaultHasher for cross-process stable cache keys"
  - "Keep extract_wasm_bytes as dead_code infra for future JS-level interceptors"
metrics:
  duration: ~12min
  completed: "2026-05-17"
  tasks_completed: 5
  files_changed: 4
---

# Phase 40 Plan wasm-cache: Process-Global WASM Compilation Cache Summary

Process-global `WasmModuleCache` singleton with SHA-256 keyed entries and infrastructure for future JS-level WebAssembly.compile interception.

## What Was Built

### 1. `src/wasm/engine.rs`

- Added `OnceLock<WasmModuleCache>` static `GLOBAL_WASM_CACHE` initialized on first call
- Added `pub fn global_wasm_cache() -> &'static WasmModuleCache`
- Replaced `DefaultHasher`-based `compute_hash` with SHA-256 via `sha2` crate (already a dependency)
- Made `compute_hash` `pub` so js_api.rs and integration tests can call it directly
- Added `WasmModuleCache::len()` and `is_empty()` methods
- Added 5 new unit tests: `test_global_cache_singleton`, `test_hash_stability`, `test_hash_cross_process_stable`, `test_cache_len` (plus existing tests preserved)

### 2. `src/wasm/mod.rs`

- Re-exported `compute_hash` and `global_wasm_cache` from `engine` module

### 3. `src/wasm/js_api.rs`

- Added `extract_wasm_bytes()` helper (handles ArrayBuffer, Uint8Array, any ArrayBufferView) — marked `#[allow(dead_code)]` as infrastructure for future JS-level interceptors
- Added module-level documentation explaining V8 v147 compile API limitation
- WebAssembly.validate override preserved; compile/instantiate overrides not wired (see Deviations)

### 4. `tests/isolate_wasm_cache_test.rs`

- `[WASM-CACHE-01]`: WebAssembly.compile + instantiate works 10x in same worker — verifies no regression
- `[WASM-CACHE-02]`: WebAssembly.compile works across 4 workers — verifies cross-worker correctness
- `[WASM-CACHE-03]`: global_wasm_cache() accessible from integration context; SHA-256 hash stable and unique

## Test Results

- `cargo test --lib` → 667 passed
- `cargo test --test isolate_wasm_cache_test` → 3 passed
- `cargo test --test isolate_scope_test` → 9 passed (regression clean)
- `cargo test --test isolate_endurance_test` → 3 passed (regression clean)

## Deviations from Plan

### Auto-investigated Issue — V8 Compile API Limitation

**1. [Rule 4 — Architecture Constraint] v8::WasmModuleObject::compile returns None in rusty_v8 v147**

- **Found during:** Task 3 (wiring JS override callbacks)
- **Issue:** `v8::WasmModuleObject::compile(scope, bytes)` returns `None` when called from within a `FunctionCallbackArguments` context. This is a known limitation: `tests/wasm_binary_debug_test.rs` has `test_7_webassembly_compile` marked `#[ignore = "V8 internal WasmModuleObject::compile API may not be fully exposed - use JS WebAssembly API instead"]`.
- **Attempted:** `wasm_compile_cached` callback wired into `WebAssembly.compile` — all calls returned "WebAssembly.compile: compilation failed" (None from the Rust API).
- **Resolution:** Removed JS-level override for `WebAssembly.compile` and `WebAssembly.instantiate`. V8's native JS implementation handles those paths correctly. The `global_wasm_cache()` and `compile_module()` functions are wired for Rust-side compilation paths (sliver pre-compilation, direct Rust callers). The `extract_wasm_bytes()` helper is preserved as infrastructure for when a future V8 upgrade exposes a working synchronous compile path.
- **Files modified:** `src/wasm/js_api.rs`

**2. [Rule 1 — Bug] Wrong WASM export section size byte in test bytes**

- **Found during:** Task 4 (integration test run)
- **Issue:** Test WASM bytes used `0x07, 0x08` (export section size=8) instead of `0x07, 0x07` (size=7). V8 rejected: "section was shorter than expected size (8 bytes expected, 7 decoded)".
- **Fix:** Corrected to `0x07, 0x07` — verified against `wasm_vfs_compile_test.rs` WASM_BYTES constant which is known-good.
- **Files modified:** `tests/isolate_wasm_cache_test.rs`

## Known Limitations

- **JS-level caching not active:** `WebAssembly.compile()` called from JavaScript goes through V8's native async pipeline without hitting `global_wasm_cache`. Caching is available only for Rust-side callers of `compile_module()`.
- **Future work:** When rusty_v8 exposes a working synchronous compile API (or when a wasm-level JS shim pattern is available), wire `extract_wasm_bytes` + cache into `WebAssembly.compile`.

## Threat Flags

None — no new network endpoints, auth paths, or trust boundary changes introduced.

## Self-Check: PASSED

- `src/wasm/engine.rs`: exists, modified
- `src/wasm/js_api.rs`: exists, modified
- `src/wasm/mod.rs`: exists, modified
- `tests/isolate_wasm_cache_test.rs`: exists, created
- Commit `f1e5b6d8`: confirmed in git log
- 667 lib tests pass, 3 integration tests pass, 0 regressions
