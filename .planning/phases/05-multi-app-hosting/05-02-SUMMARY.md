---
phase: 05-multi-app-hosting
plan: 02
subsystem: limits
summary: "Per-app memory limits with V8 heap integration and timeout watchdog"
dependency_graph:
  requires: ["05-01"]
  provides: ["resource-limits", "oom-protection"]
tech_stack:
  added: [v8 heap APIs]
  patterns: [Heap limit callbacks, Watchdog timers]
key_files:
  created:
    - src/worker/limits.rs
    - src/worker/oom.rs
    - src/app/timeout.rs
  modified:
    - src/worker/pool.rs
    - src/worker/context.rs
decisions:
  - V8 heap limit callback triggers before OOM (soft limit 80%, hard limit 100%)
  - Timeout watchdog uses tokio::time for async cancellation
  - OOM detection terminates isolate, returns 503 to client
  - Limits enforced per-isolate, not global
metrics:
  duration: "~35 minutes"
  oom_tests: 3
  timeout_tests: 4
  limit_types: 2 (memory, timeout)
---

## What Was Built

### Memory Limits (src/worker/limits.rs)
- `IsolateLimits` — Per-isolate memory tracking
- `MemoryLimiter` — Heap limit callback registration
- Soft limit at 80% (warning log)
- Hard limit at 100% (OOM kill)
- Integration with V8 heap statistics APIs

### OOM Detection (src/worker/oom.rs)
- `OOMDetector` — Monitor heap usage during request execution
- `oom_kill()` — Terminate isolate, log structured event
- 503 Service Unavailable response to client
- Structured log: `{"event": "oom_kill", "isolate_id": "...", "memory_mb": 128}`
- Metrics tracking: `nano_oom_kills_total`

### Timeout Watchdog (src/app/timeout.rs)
- `TimeoutWatchdog` — Per-request timeout enforcement
- Configurable per-app (default 30s, max 300s)
- `watchdog.start()` — Begin monitoring when request starts
- `watchdog.check()` — Check if timeout exceeded
- `watchdog.stop()` — Stop monitoring on completion
- Returns 408 Request Timeout if exceeded

### Worker Pool Integration
- Limits applied when creating new isolates
- Heap limit callbacks registered during context setup
- Timeout watchdog injected into request handler
- Graceful cleanup on limit exceeded

## Verification

### OOM Tests
- `test_oom_at_128mb_limit` — Triggers at exact limit
- `test_oom_warning_at_80_percent` — Soft limit warning
- `test_oom_structured_logging` — Log format verification

### Timeout Tests
- `test_timeout_30s_default` — Default timeout works
- `test_timeout_custom_per_app` — Per-app override
- `test_timeout_408_response` — HTTP status code
- `test_timeout_cancellation` — Request cancelled

## Security

- Per-app isolation prevents one app from exhausting others
- OOM kills only affect offending isolate
- Timeouts prevent indefinite resource holding
- Structured logging for audit trail

## Commits
- `5d5baf84` — feat(05-02): Memory limiter with V8 heap integration

## Next Steps
- Phase 05-03: Hot-reload with graceful drain
