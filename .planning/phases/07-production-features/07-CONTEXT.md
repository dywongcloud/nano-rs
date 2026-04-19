# Phase 07: Production Features & Admin API — Context

**Gathered:** 2026-04-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Runtime has production-grade observability, metrics, operational stability, and remote management capabilities. This phase adds the operational layer that makes NANO production-ready.

**Success Criteria (from ROADMAP.md):**
1. Structured JSON logs include timestamp, level, event, hostname, request_id
2. GET /_admin/metrics returns Prometheus-compatible request/latency/error metrics
3. SIGTERM/SIGINT triggers graceful shutdown with in-flight request drain
4. Heap limit exceeded triggers OOM detection and isolate termination
5. HTTP Admin API on port 8889 (configurable) with API key authentication
6. Unix domain socket at /var/run/nano/control.sock for local access
7. Admin endpoints: /admin/isolates, /admin/apps, /admin/logs, /admin/drain, /admin/reload
8. Runtime app CRUD: add, remove, disable, enable, scale workers without restart

**In scope:**
- Structured logging infrastructure
- Prometheus-compatible metrics endpoint
- Graceful shutdown with SIGTERM/SIGINT handling
- OOM detection and isolate termination
- HTTP Admin API with authentication
- Unix domain socket for local admin access
- Runtime app management (hot-add/remove/scale)

**Out of scope:**
- Log aggregation/forwarding (use external tools like fluentd/vector)
- Metrics persistence/historical data (Prometheus scrape model only)
- Alerting rules (configure in Prometheus/Grafana)
- Multi-node cluster management (single-node focus)

</domain>

<decisions>
## Implementation Decisions

### Structured Logging (D-01 to D-03)
- **D-01:** Full JSON format with rich context
  - Fields: ts (ISO8601), level, event, hostname, request_id, worker_id, isolate_id, memory_bytes, duration_ms, message
  - Contextual fields enable deep debugging across worker threads and isolates
- **D-02:** stdout output only (container environment friendly)
  - No file logging — let container orchestration handle log collection
  - Structured format allows log aggregation tools to parse easily
- **D-03:** INFO level default, WARN for anomalies, ERROR for failures
  - RUST_LOG env filter supported for development debugging
  - Per-app log level configuration in future (not v1)

### Metrics Strategy (D-04 to D-05)
- **D-04:** Comprehensive runtime metrics exposed
  - Request metrics: count, duration histograms (p50/p95/p99), error rate by status code
  - Runtime metrics: isolate_count, memory_usage gauge, worker_utilization percentage
  - App metrics: app_restart_count, active_apps gauge
  - Format: Prometheus text format (application/prometheus)
- **D-05:** In-memory atomic counters only
  - No historical data persistence in NANO
  - Prometheus scrapes current values on its schedule (default 15s)
  - Gauges reflect instantaneous values

### Graceful Shutdown (D-06 to D-07)
- **D-06:** Configurable drain timeout (5-300s range)
  - Config key: `shutdown.drain_timeout_secs`
  - Validated at config load time
- **D-07:** Default 30 seconds for fast container-friendly shutdown
  - Stop accepting new requests immediately on SIGTERM
  - Wait for in-flight requests to complete up to timeout
  - Force terminate remaining requests after timeout with 503 status

### OOM Detection & Response (D-08 to D-09)
- **D-08:** Hard terminate isolate immediately when limit exceeded
  - Uses existing `MemoryLimiter` from `worker/limits.rs`
  - No grace period — immediate termination prevents cascade failures
  - Return 503 Service Unavailable to client
- **D-09:** Rich OOM logging via structured logs
  - Log event: `oom_kill` with used_bytes, limit_bytes, app_hostname
  - Memory stats captured at termination point
  - Isolate disposed and worker continues (next request creates fresh isolate)

### Admin API Design (D-10 to D-14)
- **D-10:** Dual interface: HTTP (port 8889) + Unix socket (/var/run/nano/control.sock)
  - HTTP for remote management and monitoring
  - Unix socket for local emergency access (bypasses network stack)
- **D-11:** Authentication: API key in X-Admin-Key header for HTTP
  - Config key: `admin.api_key` (256-bit random string recommended)
  - Unix socket uses filesystem permissions (owner/group access control)
- **D-12:** Full endpoint coverage
  - Read endpoints: /admin/isolates, /admin/apps, /admin/metrics, /admin/logs, /admin/health, /admin/ready
  - Write endpoints: POST /admin/apps, DELETE /admin/apps/:host, PATCH /admin/apps/:host
  - Action endpoints: POST /admin/apps/:host/disable, /enable, /reload, /scale, /drain
- **D-13:** Health and readiness probes separate
  - /admin/health: Returns 200 if server is running (LB health check)
  - /admin/ready: Returns 200 if all apps loaded and workers ready (k8s readiness)
- **D-14:** JSON request/response format consistent with existing error format
  - Errors: `{"error": "...", "message": "...", "code": N}`
  - Success: `{"status": "ok", "data": {...}}` or direct data for GET

### Runtime Management (D-15 to D-17)
- **D-15:** Full CRUD + lifecycle operations
  - Create: POST /admin/apps with full config (entrypoint, limits, env vars)
  - Read: GET /admin/apps (list all), GET /admin/apps/:host (specific)
  - Update: PATCH /admin/apps/:host (partial config updates)
  - Delete: DELETE /admin/apps/:host (remove app entirely)
  - Disable/Enable: POST /admin/apps/:host/disable (stop routing, preserve config)
  - Reload: POST /admin/apps/:host/reload (reload JS from disk)
  - Scale: POST /admin/apps/:host/scale with `{ "workers": N }` body
- **D-16:** Two-phase commit for app creation
  - POST /admin/apps creates "pending" app (validated but not activated)
  - POST /admin/apps/:host/activate promotes pending to active
  - Prevents bad configs from affecting running traffic
  - Validation includes: JS file exists and parses, hostname not duplicate
- **D-17:** Synchronous validation before response
  - Config validated immediately (schema, limits within bounds)
  - JS entry point test-loaded to verify no syntax errors
  - Returns 400 with detailed error if validation fails
  - Success returns 201 with created app config

### the agent's Discretion
- Exact Prometheus metric names and label conventions
- Histogram bucket boundaries for latency
- Log field ordering (aesthetic only)
- Unix socket permission bits (0600 vs 0660)
- Admin API rate limiting strategy (if any)
- Specific error code numbers (use HTTP status codes where applicable)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Prior Phase Context
- `.planning/phases/06-outbound-io/` — Outbound I/O implementation (Phase 6 must complete first)
- `.planning/phases/05-multi-app-hosting/` — App registry, limits, drain, reload infrastructure
- `.planning/phases/04-workerpool-dispatch/` — WorkerPool, isolate lifecycle

### Existing Code to Extend
- `src/admin/diagnostics.rs` — DiagnosticsCollector, SystemDiagnostics, IsolateInfo (extend for metrics/logging)
- `src/worker/limits.rs` — MemoryLimiter, OomError, HeapStatistics (OOM detection foundation)
- `src/app/drain.rs` — RequestDrain, DrainHandle (graceful shutdown foundation)
- `src/app/reload.rs` — reload_config, ConfigDiff (hot-reload foundation)
- `src/app/registry.rs` — AppRegistry (runtime management foundation)

### Requirements
- `.planning/REQUIREMENTS.md` §PROD-01 through PROD-08 — Phase 7 requirement definitions
- `.planning/REQUIREMENTS.md` §Design Principles — Firecracker VM philosophy for resource constraints

### Dependencies (Already Present)
- `tracing` = 0.1 — Structured logging foundation (from Phase 1 D-06)
- `tokio` = 1.52 — Signal handling, Unix sockets
- `axum` = 0.8 — Admin API HTTP server (same as main HTTP server)

### Technical References
- Prometheus exposition format: https://prometheus.io/docs/instrumenting/exposition_formats/
- tokio signal handling: https://docs.rs/tokio/latest/tokio/signal/
- tracing-subscriber JSON: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/admin/diagnostics.rs` — `DiagnosticsCollector` with `collect()` method, `SystemDiagnostics` formatting
  - Can extend for metrics collection
  - `format_json()` already produces JSON output
- `src/worker/limits.rs` — `MemoryLimiter::check_heap()` for OOM detection
  - `OomError` type for error handling
  - `HeapStatistics` for memory tracking
- `src/app/drain.rs` — `RequestDrain` for graceful shutdown request tracking
  - `DrainHandle` for completion notification
- `src/app/reload.rs` — `reload_config()` for config reloading
  - `ConfigDiff` for detecting changes
- `src/app/registry.rs` — `AppRegistry` for app storage
  - `Arc<RwLock<AppRegistry>>` pattern for thread-safe access

### Established Patterns
- Module structure: `src/admin/` for admin functionality
- Error handling: `anyhow::Result` + `thiserror` derive macros
- State management: `Arc<RwLock<T>>` for shared mutable state
- Configuration: Hierarchical config with serde deserialization
- Thread safety: `AtomicU64` for counters, `RwLock` for collections

### Integration Points
- Logging: Initialize `tracing_subscriber` in main.rs before server start
- Metrics: Add counters/gauges to request handling path in `http/router.rs`
- Signals: Use `tokio::signal` in main.rs alongside server startup
- Admin API: Mount on separate axum router, bind to separate port
- Unix socket: Create with `tokio::net::UnixListener`, serve with hyper

</code_context>

<specifics>
## Specific Ideas

### Structured Log Format Example
```json
{
  "ts": "2026-04-19T14:32:01.234Z",
  "level": "INFO",
  "event": "request_complete",
  "hostname": "api.example.com",
  "request_id": "req_abc123",
  "worker_id": 2,
  "isolate_id": "iso_7f8d9a",
  "memory_bytes": 16777216,
  "duration_ms": 45,
  "message": "GET /users 200 OK"
}
```

### Prometheus Metrics Example
```
# HELP nano_requests_total Total HTTP requests
# TYPE nano_requests_total counter
nano_requests_total{hostname="api.example.com",status="200"} 1423

# HELP nano_request_duration_seconds Request latency
# TYPE nano_request_duration_seconds histogram
nano_request_duration_seconds_bucket{hostname="api.example.com",le="0.1"} 892
nano_request_duration_seconds_bucket{hostname="api.example.com",le="0.5"} 1389
nano_request_duration_seconds_bucket{hostname="api.example.com",le="1.0"} 1420

# HELP nano_memory_bytes Current memory usage
# TYPE nano_memory_bytes gauge
nano_memory_bytes{hostname="api.example.com",isolate="worker-0"} 16777216
```

### Admin API Endpoint Summary
| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| /admin/health | GET | None | LB health check (200 = running) |
| /admin/ready | GET | None | Readiness probe (200 = all apps ready) |
| /admin/isolates | GET | API key | ps-style isolate listing |
| /admin/apps | GET | API key | List all apps |
| /admin/apps | POST | API key | Create new app (pending) |
| /admin/apps/:host | GET | API key | Get specific app |
| /admin/apps/:host | PATCH | API key | Update app config |
| /admin/apps/:host | DELETE | API key | Remove app |
| /admin/apps/:host/activate | POST | API key | Promote pending to active |
| /admin/apps/:host/disable | POST | API key | Stop routing to app |
| /admin/apps/:host/enable | POST | API key | Resume routing |
| /admin/apps/:host/reload | POST | API key | Reload JS from disk |
| /admin/apps/:host/scale | POST | API key | Adjust worker count |
| /admin/apps/:host/drain | POST | API key | Drain requests then disable |
| /admin/metrics | GET | API key | Prometheus metrics |
| /admin/logs | GET | API key | Recent log entries (buffered) |

### Unix Socket CLI Workflow
```bash
# Emergency local access when network down
echo '{"action": "list_apps"}' | nc -U /var/run/nano/control.sock

# Permission-based security (only nano user/group)
ls -la /var/run/nano/control.sock
# srwxrwx--- 1 nano nano 0 Apr 19 14:30 control.sock
```

</specifics>

<deferred>
## Deferred Ideas

- **Log shipping integration** — Native fluentd/vector integration for log forwarding (Phase 10+)
- **Metrics push gateway** — Push metrics to external collector instead of scrape (backlog)
- **Distributed tracing** — OpenTelemetry/Jaeger integration for cross-request tracing (backlog)
- **Admin Web UI** — Browser-based dashboard for visual management (Phase 10+)
- **GitOps integration** — Watch git repo for config changes instead of file (backlog)
- **Canary deployments** — Gradual traffic shifting between app versions (backlog)
- **Auto-scaling** — Dynamic worker adjustment based on queue depth (backlog)

</deferred>

---

*Phase: 07-production-features*
*Context gathered: 2026-04-19*