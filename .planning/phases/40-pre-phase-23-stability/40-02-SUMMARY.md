---
phase: "40"
plan: "02"
status: complete
---

# Plan 40-02 Summary: Endurance Tests (ENDURE-01..03)

## What was done

Created `tests/isolate_endurance_test.rs` with three test functions proving end-to-end correctness through WorkerPool.

**ENDURE-01** (`endure_01_exception_recovery`): 30 requests to 1-worker pool. JS throws on every 3rd request. Verifies that the request immediately following a throw returns Ok(200). Proves TryCatch RAII correctly prevents exception state from leaking across requests.

**ENDURE-02** (`endure_02_module_state_persists`): 5 requests to 1-worker pool. JS counter increments each call. Assert values [1,2,3,4,5]. Documents CF-Workers persistent-scope semantics — module-level state is intentionally preserved within one isolate lifetime.

**ENDURE-03** (`endure_03_ten_plus_requests_no_degradation`): 15 requests to 1-worker pool, all stateless. All must return Ok(200). Proves STAB-04: no degradation after 10+ requests.

## Verification

- `cargo test --test isolate_endurance_test` → 3 passed, 0 failed
- `cargo test --test isolate_scope_test` → 9 passed, 0 failed (no regression)

## Commit

`test(endurance): ENDURE-01..03 -- exception recovery, state isolation, 10+ request endurance`
