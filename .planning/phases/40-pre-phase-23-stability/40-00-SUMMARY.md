---
phase: "40"
plan: "00"
status: complete
---

# Plan 40-00 Summary: Commit In-Session Fixes

## What was done

Verified and committed three in-session fixes that were applied 2026-05-17 but not yet in version control.

**pool.rs:** TryCatch at handler_local.call() sites (2 sites) + set_allow_generation_from_strings(false)

**tenant_pool.rs:** TryCatch at handler_local.call() site + set_allow_generation_from_strings(false)

**apis.rs:** Buffer.from(Array) fix — is_array() guard before string coercion; also ArrayBuffer and ArrayBufferView handling

## Verification

- `grep -c "TryCatch::new" pool.rs` → 1+
- `grep -c "TryCatch::new" tenant_pool.rs` → 1+
- `grep -c "set_allow_generation_from_strings" pool.rs` → 2
- `cargo build --lib` → 0 errors
- `cargo test --test isolate_scope_test` → 9 passed, 0 failed

## Commit

`fix(worker): TryCatch at handler call sites, block string generation, fix Buffer.from(Array)`
