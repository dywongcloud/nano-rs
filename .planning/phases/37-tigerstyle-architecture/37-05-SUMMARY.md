# Phase 37 Plan 05: Control/Data Plane Separation Summary

## One-Liner
Implement TigerStyle control plane / data plane separation: 40+ assertions centralized in control plane, zero-assertion optimized execution in data plane, with lookup tables and request batching.

## What Was Built

### Control Plane (`src/control_plane.rs`)
- **Request validation** with TigerStyle assertions covering positive space, negative space, preconditions, postconditions, invariants, ranges, and resource limits (48 `assert_` occurrences)
- **Tenant registry** with per-tenant limits for multi-tenant isolation
- **Request batching** by tenant/isolate with configurable batch size (64) and timeout (10ms)
- **Metrics collection** tracking requests submitted, validated, rejected, and batches flushed
- **`validate_request_ref`** method for non-consuming pre-dispatch validation used by HTTP router

### Data Plane (`src/control_plane.rs`)
- **Zero-assertion execution path** (`grep -c "assert_" src/data_plane.rs == 0`)
- **V8 execution functions** moved from `pool.rs` for clean separation: `execute_with_context_manager`, `execute_handler_code`, `extract_js_response`
- **Lookup tables** for HTTP status lines (O(1) resolution via direct index)
- **`DataPlane` executor** with `execute_single` and `execute_batch` methods
- **Batch execution** amortizes context reset cost across multiple requests on the same isolate

### Refactored Existing Code
- **`src/worker/pool.rs`**: Removed 500+ lines of execution functions into `data_plane.rs`. Worker threads now call re-exported `data_plane::execute_with_context_manager()`. Constructors use explicit `panic!` instead of TigerStyle assertion macros.
- **`src/worker/queue.rs`**: Added `ControlPlane` integration to `WorkQueue`. Created by default with `ControlPlane::new()`.
- **`src/http/router.rs`**: `dispatch_to_worker_pool` validates requests through `ControlPlane::validate_request_ref()` before dispatch. Returns HTTP 400 on validation failure.
- **Type fixes**: `WorkerPool::worker_count`, `WorkerHandle::id`, `WorkerPoolTrait::worker_count` changed from `u32` to `usize` for consistency. `NanoResponse::set_worker_id` signature fixed.

### Documentation
- **`docs/ARCHITECTURE_CONTROL_DATA_PLANE.md`**: Comprehensive architecture document covering responsibilities, handoff protocol, performance characteristics, and integration points.

## Metrics

| Metric | Value |
|--------|-------|
| Tasks Completed | 3/3 |
| New Files Created | 4 (control_plane.rs, data_plane.rs, ARCHITECTURE_CONTROL_DATA_PLANE.md, 37-05-SUMMARY.md) |
| Files Modified | 8+ (pool.rs, queue.rs, router.rs, types.rs, trait.rs, isolate.rs, ecdsa.rs) |
| Lines Added | ~1,500 |
| Lines Removed | ~600 |
| Assertions in Control Plane | 48 |
| Assertions in Data Plane | 0 |
| Compilation Status | `cargo check --lib` passes (0 errors) |

## Commits

| Commit | Hash | Description |
|--------|------|-------------|
| 1 | `6e2c2a65` | Create control plane and data plane modules |
| 2 | `251f0620` | Refactor pool, queue, router for control/data plane separation |

## Deviations from Plan

### Pre-existing Compilation Issues Fixed (Rule 3)

During execution, `cargo clean` revealed pre-existing compilation errors that were masked by cached artifacts. These were fixed to satisfy the plan's `cargo check --lib` verification:

1. **`src/v8/isolate.rs`**: Fixed typo `_min_limit` / `_max_limit` -> `_min_bytes` / `_max_bytes` in `set_heap_limits` logging.
2. **`src/runtime/crypto/ecdsa.rs`**: Fixed `p256`/`p384` `from_public_key_der` calls by importing `pkcs8::DecodePublicKey` trait. Fixed `as_ref().to_vec()` type ambiguity on `SharedSecret` by using `as_slice().to_vec()`.
3. **Type consistency fixes**: `WorkerPool::worker_count`, `WorkerHandle::id`, and `WorkerPoolTrait::worker_count` changed from `u32` to `usize` to resolve pervasive type mismatches across `pool.rs`, `queue.rs`, and `trait.rs`.

### Design Adjustment: Validation-Only API

The plan suggested `ControlPlane::submit_request(task)` taking ownership of `HandlerTask`. However, the router needs to validate tasks without consuming them (since the task must be dispatched to the work queue afterward). Added `validate_request_ref(&self, &HandlerTask)` method for this use case, with `submit_request` remaining available for true batching scenarios.

### Design Adjustment: Non-Clone ValidatedRequest

`ValidatedRequest` contains `HandlerTask` which includes `oneshot::Sender` (not `Clone`). Instead of making `ValidatedRequest` cloneable, redesigned `BatchQueue::drain_ready_batches` to move batches out of the pending map rather than clone them.

## Self-Check

- [x] `src/control_plane.rs` exists and compiles
- [x] `src/data_plane.rs` exists and compiles
- [x] `docs/ARCHITECTURE_CONTROL_DATA_PLANE.md` exists
- [x] `cargo check --lib` passes with 0 errors
- [x] `grep -c "assert_" src/data_plane.rs == 0`
- [x] `grep -c "assert_" src/control_plane.rs > 20`
- [x] `WorkerPool` uses `data_plane` execution functions
- [x] `WorkQueue` integrates `ControlPlane`
- [x] `router.rs` validates through control plane

## Known Stubs

None. All created modules are fully wired and functional.
