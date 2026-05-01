# Phase 27: Production Multi-Tenancy Requirements

**Phase:** 27-production-multi-tenancy  
**Goal:** Implement production-grade multi-tenancy features: WASM support, CPU limits with timer termination, memory monitoring with soft eviction, and comprehensive per-tenant metrics  
**Status:** Planned  

---

## Requirements

### PROD-01: CPU Time Tracking
**Priority:** P0 (Critical)  
**Description:** Track CPU time consumption per request with microsecond precision  
**Acceptance Criteria:**
- CPU time measured per thread using CLOCK_THREAD_CPUTIME_ID
- Measurement accurate to < 1ms
- Integration with WorkerPool execution flow
- Zero overhead when not queried

### PROD-02: Timer-Based Termination
**Priority:** P0 (Critical)  
**Description:** Implement Linux timer_create-based CPU time limit enforcement  
**Acceptance Criteria:**
- Uses timer_create(CLOCK_THREAD_CPUTIME_ID) on Linux
- SIGALRM handler sets atomic flag (no V8 calls from handler)
- V8 Isolate::TerminateExecution() called from main thread
- Configurable per-app limit (default 50ms like Cloudflare)
- Wall-clock timeout as backup (for platform compatibility)

### PROD-03: Per-App CPU Limits
**Priority:** P0 (Critical)  
**Description:** Allow per-application CPU time limits in configuration  
**Acceptance Criteria:**
- `cpu_time_ms` field in AppLimits (1-1000ms range)
- `cpu_time_enabled` toggle (default: true)
- Validation at config load time
- Default 50ms matches Cloudflare Workers

### PROD-04: Memory Monitoring
**Priority:** P0 (Critical)  
**Description:** Monitor memory usage after every JS execution  
**Acceptance Criteria:**
- Check heap statistics after each handler execution
- Track memory trend (growing, stable, shrinking)
- Maintain history of last 10 snapshots
- Pressure levels: Normal (<70%), Warning (70-85%), Critical (85-95%), Emergency (>95%)

### PROD-05: Soft Eviction
**Priority:** P0 (Critical)  
**Description:** Implement soft eviction for memory pressure handling  
**Acceptance Criteria:**
- Soft eviction allows current requests to complete
- New requests rejected during draining
- State transition: Active → Draining → Evicted
- Triggered at Critical pressure level
- Grace period for in-flight requests

### PROD-06: LRU Eviction
**Priority:** P0 (Critical)  
**Description:** LRU-based isolate eviction with stateless preference  
**Acceptance Criteria:**
- Track last used timestamp per isolate
- Evict least recently used isolates first
- Prefer stateless isolates for eviction
- EvictionPolicy enum: LRU, LFU, Random, LargestFirst
- Hard eviction at Emergency pressure level

### PROD-07: Per-Tenant Metrics Collection
**Priority:** P0 (Critical)  
**Description:** Automatic per-tenant metrics collection on every request  
**Acceptance Criteria:**
- Metrics collected per hostname automatically
- Request counts (total, success, error, timeout)
- CPU time per request and cumulative
- Memory usage (current, peak, external)
- Request latency histograms

### PROD-08: Metrics Export
**Priority:** P0 (Critical)  
**Description:** Prometheus-compatible metrics export  
**Acceptance Criteria:**
- GET /admin/metrics returns Prometheus text format
- Per-tenant metrics with hostname labels
- CPU time in seconds (Prometheus convention)
- Memory in bytes
- Histogram buckets: 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, +Inf

### PROD-09: Metrics Admin API
**Priority:** P1 (High)  
**Description:** JSON API endpoints for metrics access  
**Acceptance Criteria:**
- GET /admin/metrics/tenants - all tenant metrics
- GET /admin/metrics/isolates - per-isolate stats
- GET /admin/metrics/apps/:hostname - specific app
- GET /admin/metrics/summary - high-level overview
- Proper HTTP status codes (200, 404)

### PROD-10: WASM Module Loading
**Priority:** P1 (High)  
**Description:** Load and validate WebAssembly modules  
**Acceptance Criteria:**
- Load WASM from filesystem path
- Load WASM from VFS
- Validate magic number (\\0asm) and version (1.0 or 2.0)
- Error handling for invalid WASM
- Support up to 1MB WASM modules

### PROD-11: WASM Runtime Integration
**Priority:** P1 (High)  
**Description:** Execute WASM modules within V8 isolates  
**Acceptance Criteria:**
- WebAssembly.compile() returns Promise<Module>
- WebAssembly.instantiate() returns Promise<Instance>
- WebAssembly.validate() returns boolean
- WebAssembly.Memory/Table/Global constructors available
- Exports callable from JavaScript
- Same CPU/memory limits apply

### PROD-12: WASM Sliver Support
**Priority:** P2 (Medium)  
**Description:** Include WASM modules in sliver snapshots  
**Acceptance Criteria:**
- WASM files automatically discovered during sliver creation
- Compiled modules cached in sliver
- Cache serialization/deserialization
- Source hash verification
- Restoration from sliver with compiled modules

---

## Dependencies

- Phase 27-01 → Phase 27-02 (CPU tracking needed for eviction decisions)
- Phase 27-01 → Phase 27-03 (CPU tracking needed for metrics)
- Phase 27-02 → Phase 27-03 (Memory monitoring needed for metrics)
- All WASM tasks (27-04) can run in parallel with other plans

---

## Success Criteria

1. ✅ CPU time limits enforced with 50ms default
2. ✅ Timer-based termination on Linux
3. ✅ Memory monitoring after each execution
4. ✅ Soft eviction at 85% memory, hard at 95%
5. ✅ LRU eviction with stateless preference
6. ✅ Per-tenant metrics auto-collected
7. ✅ Prometheus endpoint with all metrics
8. ✅ JSON admin API for metrics
9. ✅ WASM modules loadable and executable
10. ✅ WASM in sliver snapshots
11. ✅ All tests pass

---

## Metrics to Track

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| nano_tenant_requests_total | Counter | hostname, status | Total requests per tenant |
| nano_tenant_cpu_seconds_total | Counter | hostname | Cumulative CPU time |
| nano_tenant_memory_bytes | Gauge | hostname, type | Current memory (heap/external) |
| nano_tenant_request_duration_seconds | Histogram | hostname | Request latency |
| nano_tenant_cpu_time_per_request_seconds | Histogram | hostname | CPU time per request |
| nano_isolate_context_resets_total | Counter | hostname, worker_id | Context reset count |
| nano_isolate_memory_bytes | Gauge | hostname, worker_id | Per-isolate memory |
| nano_evictions_total | Counter | hostname, reason | Eviction events |

---

*Requirements created: 2026-05-01*
