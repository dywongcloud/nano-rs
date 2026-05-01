# Phase 27: Production Multi-Tenancy

**Status:** Planned  
**Milestone:** v2.0 Advanced Features  
**Goal:** Production-grade multi-tenancy features for high-density edge hosting

---

## Overview

This phase implements production multi-tenancy features based on Cloudflare's approach:

1. **CPU Time Limits** — Microsecond-precision tracking with timer-based termination
2. **Memory Monitoring** — Post-execution checks with soft eviction
3. **Per-Tenant Metrics** — Prometheus-compatible metrics and admin API
4. **WASM Support** — WebAssembly execution with sliver integration

---

## Plans

| Plan | Description | Wave | Dependencies |
|------|-------------|------|--------------|
| [27-01-PLAN.md](./27-01-PLAN.md) | CPU time tracking and timer-based termination | 1 | None |
| [27-02-PLAN.md](./27-02-PLAN.md) | Memory monitoring and soft/LRU eviction | 2 | 27-01 |
| [27-03-PLAN.md](./27-03-PLAN.md) | Per-tenant metrics and observability | 2 | 27-01, 27-02 |
| [27-04-PLAN.md](./27-04-PLAN.md) | WASM support and sliver integration | 3 | 27-01, 27-02, 27-03 |

---

## Execution Order

```
Wave 1: 27-01 (CPU limits)
Wave 2: 27-02, 27-03 (Memory + Metrics, parallel)
Wave 3: 27-04 (WASM)
```

---

## Key Features

### CPU Time Limits (Cloudflare-style)
- **Default:** 50ms per request (configurable)
- **Mechanism:** Linux `timer_create(CLOCK_THREAD_CPUTIME_ID)`
- **Termination:** V8 `Isolate::TerminateExecution()`
- **Fallback:** Wall-clock timeout for other platforms

### Memory Monitoring
- **Check frequency:** After every JS execution
- **Soft eviction:** At 85% of limit (finish current requests)
- **Hard eviction:** At 95% of limit (immediate termination)
- **LRU policy:** Least recently used stateless isolates first

### Per-Tenant Metrics
- **Collection:** Automatic on every request
- **Export:** Prometheus text format at `/admin/metrics`
- **JSON API:** `/admin/metrics/tenants`, `/admin/metrics/isolates`
- **Metrics:** Requests, CPU time, memory, latency histograms

### WASM Support
- **Loading:** From filesystem, VFS, or bytes
- **API:** WebAssembly.compile/instantiate/validate
- **Execution:** Same isolate as JS, shared memory
- **Slivers:** Pre-compiled modules cached for fast cold starts

---

## Requirements

See [REQUIREMENTS.md](./REQUIREMENTS.md) for detailed requirements:
- PROD-01 through PROD-03: CPU limits
- PROD-04 through PROD-06: Memory and eviction
- PROD-07 through PROD-09: Metrics
- PROD-10 through PROD-12: WASM

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Worker Thread                             │
│  ┌─────────────┐    ┌──────────────────┐    ┌────────────┐ │
│  │ CpuTracker  │───→│  HandlerTask     │───→│ Metrics    │ │
│  │ (27-01)     │    │  Execution       │    │ Collection │ │
│  └─────────────┘    └──────────────────┘    └────────────┘ │
│           │                   │                      │      │
│           ↓                   ↓                      ↓      │
│  ┌─────────────┐    ┌──────────────────┐    ┌────────────┐ │
│  │ Execution   │    │ MemoryMonitor    │    │ Tenant     │ │
│  │ Timer       │    │ (27-02)          │    │ Metrics    │ │
│  │ (timer_)    │    └──────────────────┘    └────────────┘ │
│  └─────────────┘           │                      │         │
│                            ↓                      ↓         │
│                   ┌──────────────────┐    ┌────────────┐   │
│                   │ EvictionManager  │    │ Prometheus │   │
│                   │ (LRU Cache)      │    │ Export     │   │
│                   └──────────────────┘    └────────────┘   │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    WASM Integration                          │
│  ┌─────────────┐    ┌──────────────────┐    ┌────────────┐ │
│  │ WasmLoader  │───→│ WasmRuntime      │───→│ V8 WASM    │ │
│  │ (27-04)     │    │ (compile/)       │    │ Engine     │ │
│  └─────────────┘    └──────────────────┘    └────────────┘ │
│           │                                         │       │
│           ↓                                         ↓       │
│  ┌─────────────┐                           ┌────────────┐  │
│  │ VFS/Sliver  │                           │ WebAssembly│  │
│  │ Integration │                           │ JS API     │  │
│  └─────────────┘                           └────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## New Modules

### CPU and Timeout
- `src/worker/cpu_tracker.rs` — Microsecond CPU tracking
- `src/worker/timeout.rs` — Timer-based termination

### Memory and Eviction
- `src/worker/memory_monitor.rs` — Post-execution memory checks
- `src/worker/eviction.rs` — LRU eviction manager

### Metrics
- `src/metrics/tenant.rs` — Per-tenant metrics collector
- `src/admin/handlers/isolates.rs` — Metrics endpoints

### WASM
- `src/wasm/mod.rs` — WASM module system
- `src/wasm/loader.rs` — WASM loading
- `src/wasm/runtime.rs` — V8 WASM integration
- `src/wasm/sliver.rs` — WASM sliver support
- `src/wasm/js_api.rs` — WebAssembly JS API

---

## Configuration

```json
{
  "hostname": "api.example.com",
  "entrypoint": "./app.js",
  "limits": {
    "memory_mb": 128,
    "timeout_secs": 30,
    "workers": 4,
    "cpu_time_ms": 50,
    "cpu_time_enabled": true
  }
}
```

---

## Testing

Each plan includes:
1. Unit tests for new modules
2. Integration tests for end-to-end flows
3. Manual verification steps

Key test scenarios:
- Infinite loop terminated at 50ms
- Memory pressure triggers soft eviction
- Per-tenant metrics collected correctly
- WASM module loads and executes

---

## Dependencies

### New Cargo Dependencies
```toml
[dependencies]
thread-time = "0.2"
signal-hook = "0.3"
bincode = "1.3"
sha2 = "0.10"
chrono = { version = "0.4", features = ["serde"] }

[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
nix = { version = "0.29", features = ["signal"] }
```

---

## Success Criteria

1. ✅ CPU time limits enforced (50ms default)
2. ✅ Timer-based termination on Linux
3. ✅ Memory monitoring after each execution
4. ✅ Soft eviction at 85%, hard at 95%
5. ✅ LRU eviction with stateless preference
6. ✅ Per-tenant metrics auto-collected
7. ✅ Prometheus endpoint with all metrics
8. ✅ JSON admin API for metrics
9. ✅ WASM modules loadable and executable
10. ✅ WASM in sliver snapshots
11. ✅ All tests pass

---

## Next Steps

1. Execute Wave 1: `/gsd-execute-phase 27` (starts with 27-01)
2. Monitor SUMMARY files after each plan
3. Run integration tests between waves
4. Update documentation as features land

---

*Created: 2026-05-01*
