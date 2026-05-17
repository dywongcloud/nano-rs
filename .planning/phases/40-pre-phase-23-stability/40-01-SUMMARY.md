---
phase: "40"
plan: "01"
status: complete
---

# Plan 40-01 Summary: cancel_terminate_execution in CpuTimeoutGuard::drop

## What was done

Added `cancel_terminate_execution()` call to `CpuTimeoutGuard::drop()` in `src/data_plane.rs`.

**Root cause fixed:** V8's terminate flag was sticky. After `terminate_execution()` fired on timeout, `drop()` joined the timer thread and cleared the pointer but never cancelled the flag. Every subsequent handler call on the same isolate returned `None` immediately.

**Fix:** Before zeroing `TERMINATION_ISOLATE_PTR`, check `TERMINATION_REQUESTED`. If true, load the pointer and call `isolate.cancel_terminate_execution()`. This clears the V8 terminate flag so the isolate can serve future requests.

## Verification

- `grep -n "cancel_terminate_execution" src/data_plane.rs` → found in `drop()` impl
- `cargo build --lib` → 0 errors
- `cargo test --lib` → 663 passed, 0 failed
- `cargo test --test isolate_scope_test` → 9 passed, 0 failed

## Commit

`fix(data_plane): call cancel_terminate_execution() in CpuTimeoutGuard::drop`
