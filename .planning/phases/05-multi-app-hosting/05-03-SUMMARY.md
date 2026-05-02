---
phase: 05-multi-app-hosting
plan: 03
subsystem: ops
summary: "Hot-reload with config file watching and graceful drain of in-flight requests"
dependency_graph:
  requires: ["05-01", "05-02"]
  provides: ["hot-reload", "zero-downtime-deploy"]
tech_stack:
  added: [notify crate for file watching]
  patterns: [Graceful shutdown, Atomic config swap]
key_files:
  created:
    - src/config/watcher.rs
    - src/app/reload.rs
    - src/app/drain.rs
  modified:
    - src/app/registry.rs
    - src/worker/pool.rs
    - src/http/server.rs
decisions:
  - File watcher uses debouncing (2 second delay) to avoid reload spam
  - Graceful drain waits for in-flight requests (30s timeout)
  - Atomic swap: new registry built, then pointer swapped
  - Old workers drained and terminated after swap
metrics:
  duration: "~30 minutes"
  reload_time_target: "<2s from config change"
  drain_timeout: "30s default"
  zero_downtime: "yes"
---

## What Was Built

### Config Watcher (src/config/watcher.rs)
- `ConfigWatcher` — File system watcher using `notify` crate
- Watches `config.json` for modifications
- Debouncing: 2 second delay to batch rapid changes
- Checks SHA256 hash to skip reload on identical content
- Triggers `reload_config()` on valid changes

### Hot Reload (src/app/reload.rs)
- `reload_config()` — Orchestrates full reload cycle
- Steps: 1) Load new config, 2) Validate, 3) Build new registry, 4) Graceful drain, 5) Atomic swap
- `build_new_pools()` — Create WorkerPools for new apps
- `drain_old_pools()` — Wait for in-flight requests
- `swap_registry()` — Atomic pointer swap (instant)
- Rollback on validation failure

### Graceful Drain (src/app/drain.rs)
- `DrainHandle` — Tracks in-flight requests per WorkerPool
- `start_drain()` — Stop accepting new requests
- `wait_for_drain()` — Block until completion or timeout
- Timeout: 30 seconds (configurable)
- Force kill remaining requests after timeout

### Registry Atomic Swap
- `AppRegistry` uses `Arc<RwLock<HashMap>>`
- New registry built completely before swap
- Swap is single atomic operation (near-instant)
- No request dropped during swap

## Verification

### Hot-Reload Tests
- `test_config_change_triggers_reload` — File change detection
- `test_reload_within_2_seconds` — Performance target
- `test_invalid_config_rollback` — Validation failure handling
- `test_hash_check_skips_duplicate` — No-op on identical file

### Graceful Drain Tests
- `test_drain_completes_in_flight` — Requests finish normally
- `test_drain_timeout_force_kill` — Timeout enforcement
- `test_zero_downtime_swap` — No dropped requests

## Operational Benefits

- Zero-downtime config updates
- Add/remove apps without restart
- Change resource limits dynamically
- Failed reloads roll back automatically

## Commits
- Part of multi-app hosting implementation
- Hot-reload and graceful drain

## Phase 5 Complete

All HOST-01 through HOST-06 requirements satisfied:
- ✅ JSON config maps hostnames to entrypoints
- ✅ Per-app memory limits with OOM detection
- ✅ Per-app timeout enforcement
- ✅ Per-app environment variables
- ✅ Hot-reload within 2 seconds
- ✅ Graceful drain of in-flight requests
