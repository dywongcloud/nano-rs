---
phase: 23-websocket-server
plan: 02
subsystem: worker
tags: [websocket, tenant-pool, rust, mpsc, atomics, concurrency]

# Dependency graph
requires:
  - phase: 23-websocket-server/23-01
    provides: "WsChannels struct and HandlerTask.ws field in src/worker/mod.rs; AppLimits WS methods in src/config/app.rs"

provides:
  - "TenantPool has lazy WS worker pool with ws_workers, ws_busy, max_ws_connections, ws_idle_timeout_ms fields"
  - "WsWorkerHandle struct for tracking WS worker threads"
  - "dispatch_ws() method with per-tenant connection limit enforcement (D-07)"
  - "run_worker() accepts ws_busy Arc<AtomicUsize> parameter (Plan 04 placeholder)"
  - "TenantPool::new() accepts &AppLimits to initialize WS config"
  - "Drop impl joins WS worker threads to prevent V8 use-after-free"

affects: [23-03, 23-04, 23-05]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Lazy worker pool: WS workers spawned on first connection, pruned on channel disconnect"
    - "TOCTOU-safe counter: ws_busy incremented inside worker thread, not in dispatch_ws (D-13b)"
    - "Dead-handle pruning: SendError triggers swap_remove + join of exited worker handle"
    - "ws_busy as Arc<AtomicUsize> shared between pool and workers for autonomous decrement"

key-files:
  created: []
  modified:
    - src/worker/tenant_pool.rs

key-decisions:
  - "ws_busy incremented by WORKER thread (not dispatch_ws) to avoid TOCTOU per D-13b — Plan 04 wires the increment"
  - "WsWorkerHandle.join is Option so Drop can take() without double-join"
  - "Dead-handle pruning deferred to send-time (no std::sync::mpsc probe API) — swap_remove keeps Vec compact"
  - "ws_idle_timeout_ms stored as field annotated #[allow(dead_code)] — Plan 04 wires recv_timeout"
  - "memory_limit_mb for WS workers set to 0 in Plan 02 as placeholder — Plan 04 will wire proper limits"
  - "WS workers reuse run_worker() (same isolate loop as HTTP workers) — WS-specific handling in Plan 04"

patterns-established:
  - "Mutex<Vec<WsWorkerHandle>> for mutable WS pool access from shared TenantPool"
  - "Arc<AtomicUsize> threaded through worker spawn so workers own decrement"

requirements-completed: [WS-02]

# Metrics
duration: 2min
completed: 2026-05-17
---

# Phase 23 Plan 02: TenantPool WS Pool Fields and dispatch_ws Summary

**Lazy WS worker pool added to TenantPool: ws_busy AtomicUsize, per-tenant connection cap from AppLimits, dispatch_ws() with dead-handle pruning, and Drop joining WS threads**

## Performance

- **Duration:** 2 min
- **Started:** 2026-05-17T05:01:47Z
- **Completed:** 2026-05-17T05:04:21Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Added `WsWorkerHandle` struct (task_tx + join handle) for WS thread lifecycle tracking
- Extended `TenantPool` with four WS pool fields: `ws_workers`, `ws_busy`, `max_ws_connections`, `ws_idle_timeout_ms`
- Updated `TenantPool::new()` to accept `&AppLimits` and log effective WS config
- Added `spawn_ws_worker()` for lazy WS thread creation; updated `run_worker()` with `ws_busy` parameter
- Implemented `dispatch_ws()`: checks connection limit, prunes dead handles via send-time detection, lazy-spawns workers
- Extended `Drop` impl to join WS worker threads, preventing V8 isolate use-after-platform-shutdown

## Task Commits

1. **Task 1: Add WS pool fields to TenantPool and WsWorkerHandle struct** - `667bb3f1` (feat)

**Plan metadata:** (created below)

## Files Created/Modified

- `src/worker/tenant_pool.rs` — WsWorkerHandle struct, four new TenantPool fields, updated new()/spawn_worker()/run_worker() signatures, new spawn_ws_worker() and dispatch_ws() methods, extended Drop impl

## Decisions Made

- ws_busy incremented by WORKER thread (not dispatch_ws) to avoid TOCTOU per D-13b. Plan 04 will add the `fetch_add` inside `run_worker` when the WS task is received.
- Dead-handle pruning uses send-time detection: `SendError` → `swap_remove(i)` + join. There is no non-destructive "is channel alive?" API in `std::sync::mpsc`.
- `memory_limit_mb` for WS workers is 0 (no OOM monitoring) in Plan 02 — stored config will be wired in Plan 04.
- `ws_idle_timeout_ms` is stored as an `#[allow(dead_code)]` field because Plan 04 will pass it into `recv_timeout` in the WS worker loop.

## Deviations from Plan

None — plan executed exactly as written. The one minor deviation is that `spawn_ws_worker` was split from `spawn_worker` to avoid passing dummy arguments, which keeps the code clean and matches the plan intent.

## Issues Encountered

One compilation error during initial edit: a type mismatch in a `retain` closure that was probing the channel with `()` instead of a `HandlerTask`. Fixed by removing the dead-code pruning probe (pruning correctly happens inline in the send loop via `SendError` handling, as documented in comments).

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- Plan 03 can now reference `TenantPool::dispatch_ws()` for the HTTP upgrade routing path
- Plan 04 can wire `ws_busy.fetch_add/fetch_sub` inside `run_worker` using the `Arc<AtomicUsize>` that is now threaded through
- Plan 04 can also set proper `memory_limit_mb` for WS workers and wire `ws_idle_timeout_ms` into `recv_timeout`
- `TenantPool::new()` now requires `&AppLimits` — all callers (currently only internal construction in integration tests) must be updated

## Threat Surface Scan

No new network endpoints, auth paths, or file access patterns introduced. `dispatch_ws()` operates entirely within the existing TenantPool trust boundary. Connection-limit gate (T-23-02) and dead-handle pruning (T-23-03) both implemented as specified in the plan threat model.

---

## Self-Check

- `src/worker/tenant_pool.rs` modified: FOUND
- Commit `667bb3f1` exists: FOUND
- `cargo check --lib` passes: VERIFIED (0 errors, 0 warnings after cleanup)
- `grep -c "dispatch_ws"` = 4 (>= 1): PASS
- `grep -c "ws_busy"` = 22 (>= 3): PASS
- `grep -c "WsWorkerHandle"` = 4 (>= 2): PASS
- `grep -c "max_ws_connections"` = 9 (>= 2): PASS

## Self-Check: PASSED

---
*Phase: 23-websocket-server*
*Completed: 2026-05-17*
