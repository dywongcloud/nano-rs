# Phase 07: Production Features & Admin API — Plan

**Phase Goal:** Add production-grade observability, metrics, operational stability, and remote management capabilities to NANO runtime.

**Requirements Coverage:** PROD-01 through PROD-08  
**Decision Alignment:** D-01 through D-17 from 07-CONTEXT.md

---

## Plan Summary

| Plan | Name | Purpose | Requirements |
|------|------|---------|--------------|
| 07-01 | Structured JSON Logging | Rich contextual logging with hostname, request_id, worker_id | PROD-01 |
| 07-02 | Prometheus Metrics Endpoint | Request/latency/error metrics in Prometheus format | PROD-02 |
| 07-03 | Graceful Shutdown | SIGTERM/SIGINT handling with request draining | PROD-03 |
| 07-04 | OOM Detection Integration | Heap limit monitoring and isolate termination | PROD-04 |
| 07-05 | Admin API HTTP Server | API key auth, app CRUD, diagnostics endpoints | PROD-05, PROD-07, PROD-08 |
| 07-06 | Unix Domain Socket Admin | Local admin access via Unix socket | PROD-06 |

---

## Plan 07-01: Structured JSON Logging

### Goal
Implement rich structured JSON logging with contextual fields per request including timestamp, level, event type, hostname, request_id, worker_id, and isolate_id.

### Implementation Steps

1. **Create logging module structure**
   - Create `src/logging/` directory
   - Create `src/logging/json_layer.rs` - Custom tracing subscriber layer
   - Create `src/logging/fields.rs` - Field extraction helpers
   - Create `src/logging/mod.rs` - Module exports

2. **Implement custom JSON layer**
   - Define `NanoJsonLayer` struct implementing `tracing_subscriber::Layer<S>`
   - Extract fields: ts (RFC3339), level, event, hostname, request_id, worker_id, isolate_id
   - Use `tracing::Span` context to carry per-request fields
   - Implement `JsonVisitor` for field value extraction
   - Output to stdout with `serde_json::to_string()`

3. **Integrate with request handling**
   - In `src/http/router.rs`, create spans for each request with context fields
   - Use `tracing::info_span!("request", hostname = %host, request_id = %uuid, worker_id = %id)`
   - Log completion with duration_ms and memory_bytes

4. **Update main.rs initialization**
   - Replace `tracing_subscriber::fmt::init()` with custom subscriber
   - Add env-filter support: `RUST_LOG=info,nano=debug`
   - Enable JSON formatting with proper timestamp

### Technical Details

**Dependencies (update Cargo.toml):**
```toml
tracing-subscriber = { version = "0.3", features = ["json", "env-filter", "time"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.8", features = ["v4", "serde"] }
```

**Key Code Pattern:**
```rust
// src/logging/json_layer.rs
impl<S> Layer<S> for NanoJsonLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut JsonVisitor(&mut fields));
        
        // Extract span context
        let mut hostname = None;
        let mut request_id = None;
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(ext) = span.extensions().get::<NanoSpanExt>() {
                    hostname = hostname.or_else(|| ext.hostname.clone());
                    request_id = request_id.or_else(|| ext.request_id.clone());
                }
            }
        }
        
        let output = serde_json::json!({
            "ts": Utc::now().to_rfc3339(),
            "level": event.metadata().level().to_string(),
            "event": event.metadata().name(),
            "hostname": hostname,
            "request_id": request_id,
            "fields": fields,
        });
        println!("{}", output);
    }
}
```

### Testing

- [ ] Unit tests for JSON layer field extraction
- [ ] Integration test: verify log output contains required fields
- [ ] Test env-filter level configuration
- [ ] Verify request span context propagation

### Success Criteria
- ✅ JSON logs include: ts, level, event, hostname, request_id, worker_id, isolate_id
- ✅ RUST_LOG env filter works for level control
- ✅ Logs output to stdout in JSON format
- ✅ Request context carries through worker thread boundaries

---

## Plan 07-02: Prometheus Metrics Endpoint

### Goal
Expose Prometheus-compatible metrics at `/_admin/metrics` with request counts, latency histograms, error rates, and runtime statistics.

### Implementation Steps

1. **Create metrics module**
   - Create `src/metrics/` directory
   - Create `src/metrics/collector.rs` - Metrics collection interface
   - Create `src/metrics/types.rs` - Counter, Gauge, Histogram types
   - Create `src/metrics/exporter.rs` - Prometheus text format rendering
   - Create `src/metrics/mod.rs` - Module exports

2. **Implement metric types**
   - `Counter`: AtomicU64, monotonically increasing
   - `Gauge`: AtomicU64/Arc<dyn Fn() -> u64>, instantaneous value
   - `Histogram`: Atomic bucket counters with predefined bounds
   - All types must be thread-safe (Send + Sync)

3. **Implement Prometheus text format**
   - Format: `# HELP`, `# TYPE`, metric lines
   - Counter: `nano_requests_total{hostname="...",status="200"} 1423`
   - Histogram: buckets with `le` labels, `_sum`, `_count`
   - Gauge: `nano_memory_bytes{hostname="..."} 16777216`

4. **Integrate with request handling**
   - Add metrics collection to `src/http/router.rs`
   - Increment `nano_requests_total` counter per request
   - Record `nano_request_duration_ms` histogram with timing
   - Track error counts by status code

5. **Add metrics endpoint**
   - Create `/_admin/metrics` handler in `src/admin/metrics.rs`
   - Return Prometheus text format with correct Content-Type
   - Handler fetches from global metrics registry

### Technical Details

**Histogram Buckets (per D-04):**
```rust
const REQUEST_DURATION_BUCKETS: &[f64] = &[
    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, f64::INFINITY
];
```

**Metric Definitions:**
```rust
// src/metrics/registry.rs
lazy_static! {
    static ref METRICS: MetricsRegistry = MetricsRegistry::new();
}

pub struct MetricsRegistry {
    requests_total: CounterVec<["hostname", "status"]>,
    request_duration: HistogramVec<["hostname"], 10>,
    errors_total: CounterVec<["hostname", "code"]>,
    isolates_active: GaugeVec<["hostname", "worker_id"]>,
    memory_bytes: GaugeVec<["hostname", "isolate_id"]>,
    worker_utilization: GaugeVec<["hostname", "worker_id"]>,
}
```

**Prometheus Output Example:**
```
# HELP nano_requests_total Total HTTP requests
# TYPE nano_requests_total counter
nano_requests_total{hostname="api.example.com",status="200"} 1423
nano_requests_total{hostname="api.example.com",status="500"} 12

# HELP nano_request_duration_ms Request latency in milliseconds
# TYPE nano_request_duration_ms histogram
nano_request_duration_ms_bucket{hostname="api.example.com",le="10"} 892
nano_request_duration_ms_bucket{hostname="api.example.com",le="100"} 1389
nano_request_duration_ms_sum{hostname="api.example.com"} 45234
nano_request_duration_ms_count{hostname="api.example.com"} 1435
```

### Testing

- [ ] Unit tests for each metric type (counter, gauge, histogram)
- [ ] Test Prometheus text format output matches spec
- [ ] Integration test: request increments counter
- [ ] Integration test: latency histogram records values
- [ ] Test metrics endpoint returns 200 with correct content-type

### Success Criteria
- ✅ `/_admin/metrics` returns Prometheus-compatible output
- ✅ Metrics include: request counts, latency histograms, error rates
- ✅ Content-Type header: `text/plain; version=0.0.4; charset=utf-8`
- ✅ Histogram buckets cover 1ms to 1s range
- ✅ All metric labels properly formatted

---

## Plan 07-03: Graceful Shutdown

### Goal
Implement SIGTERM/SIGINT signal handling with graceful shutdown including in-flight request draining with configurable timeout.

### Implementation Steps

1. **Create signal handling module**
   - Create `src/signal.rs` - Signal handling and shutdown coordination
   - Implement `shutdown_channel()` returning broadcast::Sender<()>
   - Handle SIGTERM (Unix) and SIGINT (Ctrl+C on all platforms)

2. **Implement graceful shutdown logic**
   - Add `GracefulShutdown` struct with drain tracking
   - Integrate with existing `RequestDrain` from `src/app/drain.rs`
   - Add timeout enforcement (default 30s, configurable)

3. **Integrate with HTTP server**
   - Modify `src/http/server.rs` to accept shutdown signal
   - Use `axum::serve(...).with_graceful_shutdown(signal)`
   - Stop accepting new connections immediately on signal

4. **Add health/readiness endpoints**
   - `/_admin/health` - Always returns 200 (LB health check)
   - `/_admin/ready` - Returns 200 if not shutting down, 503 if draining
   - Readiness fails during shutdown to stop new traffic

5. **Update main.rs server startup**
   - Initialize shutdown channel before server start
   - Pass shutdown receiver to server
   - Await shutdown completion before exit

### Technical Details

**Signal Handler Implementation:**
```rust
// src/signal.rs
use tokio::signal;
use tokio::sync::broadcast;

pub fn shutdown_channel() -> broadcast::Sender<()> {
    let (tx, _) = broadcast::channel(1);
    let tx_clone = tx.clone();
    
    tokio::spawn(async move {
        let ctrl_c = async {
            signal::ctrl_c().await.expect("Ctrl+C handler failed");
        };
        
        #[cfg(unix)]
        let terminate = async {
            let mut sigterm = signal::unix::signal(
                signal::unix::SignalKind::terminate()
            ).expect("SIGTERM handler failed");
            sigterm.recv().await;
        };
        
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        
        tokio::select! {
            _ = ctrl_c => tracing::info!("Received SIGINT"),
            _ = terminate => tracing::info!("Received SIGTERM"),
        }
        
        let _ = tx_clone.send(());
    });
    
    tx
}
```

**Server Integration:**
```rust
// In main.rs or server.rs
let shutdown_tx = shutdown_channel();
let mut shutdown_rx = shutdown_tx.subscribe();

let server = axum::serve(listener, app)
    .with_graceful_shutdown(async move {
        let _ = shutdown_rx.recv().await;
        tracing::info!("Starting graceful shutdown...");
        
        // Mark as not ready
        state.mark_shutting_down();
        
        // Wait for drain
        let drained = drain.await_complete(Duration::from_secs(30)).await;
        if !drained {
            tracing::warn!("Drain timeout exceeded, forcing shutdown");
        }
    });
```

**Configuration (D-06, D-07):**
```rust
pub struct ShutdownConfig {
    pub drain_timeout_secs: u64, // Default: 30, Range: 5-300
}
```

### Testing

- [ ] Test SIGINT triggers graceful shutdown
- [ ] Test SIGTERM triggers graceful shutdown (Unix)
- [ ] Test in-flight requests complete before shutdown
- [ ] Test timeout forces shutdown after deadline
- [ ] Test readiness endpoint returns 503 during shutdown
- [ ] Test new connections rejected during shutdown

### Success Criteria
- ✅ SIGTERM/SIGINT triggers graceful shutdown
- ✅ In-flight requests complete before termination (up to timeout)
- ✅ Readiness probe returns 503 during shutdown
- ✅ Configurable drain timeout (5-300s range)
- ✅ Timeout forces shutdown after deadline

---

## Plan 07-04: OOM Detection Integration

### Goal
Integrate heap limit monitoring with the existing `MemoryLimiter` to detect OOM conditions and terminate isolates while logging structured OOM events.

### Implementation Steps

1. **Extend MemoryLimiter trait**
   - Add `check_oom()` method returning `Result<(), OomError>`
   - Add `oom_threshold()` for configurable limit threshold
   - Integrate with V8 `HeapStatistics` (already available)

2. **Create OOM monitor module**
   - Create `src/worker/oom.rs` - OOM detection and response
   - Implement `OomMonitor` with periodic heap checks
   - Add OOM event logging with structured format

3. **Integrate with worker request handling**
   - Check heap before each request execution
   - Check heap periodically during long-running requests
   - On OOM: log event, return 503, dispose isolate

4. **Add OOM logging**
   - Log event: `oom_kill` with used_bytes, limit_bytes, app_hostname
   - Include request_id and isolate_id from span context
   - Level: ERROR with full context

### Technical Details

**OOM Check Implementation:**
```rust
// src/worker/oom.rs
use crate::worker::limits::{HeapStatistics, OomError};

pub struct OomMonitor {
    limit_bytes: usize,
    app_hostname: String,
}

impl OomMonitor {
    pub fn check(&self, stats: &HeapStatistics) -> Result<(), OomError> {
        if stats.used_heap_size > self.limit_bytes {
            Err(OomError::LimitExceeded {
                used_bytes: stats.used_heap_size,
                limit_bytes: self.limit_bytes,
                app_hostname: self.app_hostname.clone(),
            })
        } else {
            Ok(())
        }
    }
    
    pub fn log_oom(&self, error: &OomError, request_id: &str, isolate_id: &str) {
        tracing::error!(
            event = "oom_kill",
            used_bytes = error.used_bytes(),
            limit_bytes = error.limit_bytes(),
            app_hostname = error.app_hostname(),
            request_id = request_id,
            isolate_id = isolate_id,
            "Isolate terminated: heap limit exceeded"
        );
    }
}
```

**Integration with Request Execution:**
```rust
// In worker request handling
async fn execute_request(&mut self, request: Request) -> Response {
    // Pre-request OOM check
    if let Err(oom) = self.oom_monitor.check(&self.heap_stats) {
        self.oom_monitor.log_oom(&oom, &request.id, &self.isolate_id);
        self.dispose_isolate();
        return Response::builder()
            .status(503)
            .body("Service Unavailable: Resource limit exceeded")
            .unwrap();
    }
    
    // Execute request...
}
```

**OOM Log Format (D-09):**
```json
{
  "ts": "2026-04-19T17:57:00Z",
  "level": "ERROR",
  "event": "oom_kill",
  "hostname": "app.example.com",
  "request_id": "req_abc123",
  "used_bytes": 104857600,
  "limit_bytes": 67108864,
  "isolate_id": "iso_7f8d9a",
  "message": "Isolate terminated: heap limit exceeded"
}
```

### Testing

- [ ] Unit test: OOM detection triggers at limit
- [ ] Unit test: OOM logging contains all required fields
- [ ] Integration test: Isolate disposed on OOM
- [ ] Integration test: 503 returned to client on OOM
- [ ] Test OOM event includes correct memory stats

### Success Criteria
- ✅ OOM detected when heap exceeds configured limit
- ✅ Structured log emitted with used_bytes, limit_bytes, hostname
- ✅ 503 Service Unavailable returned to client
- ✅ Isolate disposed immediately (no grace period)
- ✅ Worker continues with fresh isolate next request

---

## Plan 07-05: Admin API HTTP Server

### Goal
Create HTTP Admin API on separate port (8889) with API key authentication, providing endpoints for isolate diagnostics, app management, and runtime control.

### Implementation Steps

1. **Create Admin API module structure**
   - Create `src/admin/server.rs` - Admin HTTP server
   - Create `src/admin/auth.rs` - API key authentication middleware
   - Create `src/admin/handlers/` - Endpoint handlers
   - Update `src/admin/mod.rs` - Module exports

2. **Implement authentication middleware**
   - Create `api_key_middleware()` function
   - Read `X-Admin-Key` header
   - Compare against configured API key
   - Return 401 for missing/invalid keys
   - Skip auth for health/ready endpoints

3. **Implement admin endpoints**
   - GET `/admin/health` - Health check (no auth)
   - GET `/admin/ready` - Readiness probe (no auth)
   - GET `/admin/isolates` - ps-style isolate listing
   - GET `/admin/apps` - List all apps
   - POST `/admin/apps` - Create new app (pending)
   - GET `/admin/apps/:host` - Get specific app
   - PATCH `/admin/apps/:host` - Update app config
   - DELETE `/admin/apps/:host` - Remove app
   - POST `/admin/apps/:host/activate` - Promote pending to active
   - POST `/admin/apps/:host/disable` - Stop routing
   - POST `/admin/apps/:host/enable` - Resume routing
   - POST `/admin/apps/:host/reload` - Reload JS from disk
   - POST `/admin/apps/:host/scale` - Adjust worker count
   - POST `/admin/apps/:host/drain` - Drain then disable
   - GET `/admin/metrics` - Prometheus metrics

4. **Implement app CRUD handlers**
   - Create: Validate config, test-load JS, create pending
   - Read: Return app config and status
   - Update: PATCH partial updates, validate, apply
   - Delete: Remove from registry, drain first if active
   - Lifecycle: Disable/Enable/Activate/Scale operations

5. **Add admin server configuration**
   - Config struct: port (default 8889), api_key (required)
   - Validation: API key minimum length (32 chars recommended)
   - Bind to separate port, not the main HTTP port

6. **Integrate with main.rs**
   - Spawn admin server alongside main HTTP server
   - Share AppRegistry and WorkerPool state
   - Both servers use same shutdown signal

### Technical Details

**Authentication Middleware:**
```rust
// src/admin/auth.rs
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    http::StatusCode,
};

#[derive(Clone)]
pub struct AdminAuth {
    api_key: String,
}

pub async fn api_key_middleware(
    State(auth): State<AdminAuth>,
    req: Request,
    next: Next,
) -> Response {
    let key = req
        .headers()
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok());
    
    match key {
        Some(k) if k == auth.api_key => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, json!({"error": "Unauthorized"})).into_response(),
    }
}
```

**Admin Router Setup:**
```rust
// src/admin/server.rs
pub fn create_admin_router(
    auth: AdminAuth,
    registry: Arc<RwLock<AppRegistry>>,
    pools: Arc<HashMap<String, WorkerPool>>,
) -> Router {
    let public_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler));
    
    let protected_routes = Router::new()
        .route("/isolates", get(list_isolates))
        .route("/apps", get(list_apps).post(create_app))
        .route("/apps/:host", get(get_app).patch(update_app).delete(delete_app))
        .route("/apps/:host/activate", post(activate_app))
        .route("/apps/:host/disable", post(disable_app))
        .route("/apps/:host/enable", post(enable_app))
        .route("/apps/:host/reload", post(reload_app))
        .route("/apps/:host/scale", post(scale_app))
        .route("/apps/:host/drain", post(drain_app))
        .route("/metrics", get(metrics_handler))
        .route_layer(middleware::from_fn_with_state(auth.clone(), api_key_middleware));
    
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
}
```

**Two-Phase App Creation (D-16):**
```rust
async fn create_app(
    State(registry): State<Arc<RwLock<AppRegistry>>>,
    Json(config): Json<AppConfig>,
) -> Result<impl IntoResponse, AdminError> {
    // Validate config
    validate_app_config(&config)?;
    
    // Test-load JS to verify no syntax errors
    test_load_js(&config.entrypoint).await?;
    
    // Create as pending
    let pending = PendingApp::new(config);
    registry.write().await.add_pending(pending.clone());
    
    Ok((StatusCode::CREATED, Json(pending)))
}

async fn activate_app(
    State(registry): State<Arc<RwLock<AppRegistry>>>,
    Path(hostname): Path<String>,
) -> Result<impl IntoResponse, AdminError> {
    let mut reg = registry.write().await;
    let pending = reg.remove_pending(&hostname)
        .ok_or(AdminError::NotFound)?;
    
    // Activate: add to active apps, start workers
    reg.activate(pending).await?;
    
    Ok(Json(json!({"status": "activated"})))
}
```

### Testing

- [ ] Test health endpoint returns 200 without auth
- [ ] Test protected endpoints require X-Admin-Key header
- [ ] Test invalid API key returns 401
- [ ] Test GET /admin/apps lists all apps
- [ ] Test POST /admin/apps creates pending app
- [ ] Test activate endpoint promotes pending to active
- [ ] Test DELETE /admin/apps/:host removes app
- [ ] Test PATCH /admin/apps/:host updates config
- [ ] Test scale endpoint adjusts worker count
- [ ] Test disable/enable toggle routing

### Success Criteria
- ✅ Admin API available on separate port (8889 default)
- ✅ API key authentication on X-Admin-Key header
- ✅ All endpoints per table in 07-CONTEXT.md
- ✅ Two-phase app creation (pending → activate)
- ✅ Synchronous validation before response
- ✅ JSON error format: `{"error": "...", "message": "...", "code": N}`
- ✅ Health/ready endpoints accessible without auth

---

## Plan 07-06: Unix Domain Socket Admin

### Goal
Provide Unix domain socket for local admin access at `/var/run/nano/control.sock`, bypassing network stack and using filesystem permissions for security.

### Implementation Steps

1. **Create Unix socket module**
   - Create `src/admin/unix_socket.rs` - Unix socket server
   - Implement `create_unix_socket(path)` function
   - Handle socket file permissions (0o660 owner+group)
   - Clean up stale socket files on startup

2. **Implement dual server setup**
   - Reuse admin router from Plan 07-05
   - Serve same routes over Unix socket
   - No API key required for Unix socket access
   - Add middleware to detect Unix socket requests

3. **Handle socket lifecycle**
   - Remove existing socket file on bind (if stale)
   - Set permissions after bind (0o660)
   - Delete socket file on graceful shutdown
   - Handle errors: permission denied, path not found

4. **Add configuration**
   - Config field: `admin.unix_socket_path` (optional)
   - Default: `/var/run/nano/control.sock`
   - Enable only if path configured (disabled by default for dev)

5. **Integrate with main.rs**
   - Spawn Unix socket server alongside TCP admin server
   - Both share same router, different listeners
   - Both use same graceful shutdown signal

### Technical Details

**Unix Socket Creation:**
```rust
// src/admin/unix_socket.rs
use tokio::net::UnixListener;
use std::path::Path;
use std::os::unix::fs::PermissionsExt;

pub async fn create_unix_socket(path: &Path) -> anyhow::Result<UnixListener> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    // Remove stale socket
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    
    // Bind socket
    let listener = UnixListener::bind(path)?;
    
    // Set permissions (owner + group read/write)
    let perms = std::fs::Permissions::from_mode(0o660);
    std::fs::set_permissions(path, perms)?;
    
    tracing::info!("Unix socket listening at {}", path.display());
    Ok(listener)
}
```

**Dual Server Spawn:**
```rust
// In main.rs or admin module
pub async fn start_admin_servers(
    config: AdminConfig,
    state: AdminState,
) -> anyhow::Result<()> {
    let shutdown = Arc::new(Notify::new());
    
    // TCP server
    if let Some(tcp_addr) = config.tcp_addr {
        let tcp_listener = tokio::net::TcpListener::bind(tcp_addr).await?;
        let tcp_shutdown = Arc::clone(&shutdown);
        let tcp_router = create_admin_router(state.clone());
        
        tokio::spawn(async move {
            axum::serve(tcp_listener, tcp_router)
                .with_graceful_shutdown(async { tcp_shutdown.notified().await })
                .await
        });
    }
    
    // Unix socket server
    if let Some(unix_path) = config.unix_socket_path {
        let unix_listener = create_unix_socket(&unix_path).await?;
        let unix_shutdown = Arc::clone(&shutdown);
        let unix_router = create_admin_router(state.clone())
            .layer(middleware::from_fn(skip_auth_for_unix_socket));
        
        tokio::spawn(async move {
            axum::serve(unix_listener, unix_router)
                .with_graceful_shutdown(async { 
                    unix_shutdown.notified().await;
                    // Cleanup socket
                    let _ = tokio::fs::remove_file(&unix_path).await;
                })
                .await
        });
    }
    
    // Wait for shutdown
    tokio::signal::ctrl_c().await?;
    shutdown.notify_waiters();
    
    Ok(())
}
```

**Permission-Based Security:**
```bash
# Unix socket accessible only to nano group
$ ls -la /var/run/nano/control.sock
srwxrwx--- 1 nano nano 0 Apr 19 14:30 control.sock

# Users must be in nano group to access
$ groups user
user : user nano

# Access via socat/nc
$ echo '{"action":"list_apps"}' | socat - UNIX-CONNECT:/var/run/nano/control.sock
```

### Testing

- [ ] Test Unix socket created at configured path
- [ ] Test socket has correct permissions (0o660)
- [ ] Test stale socket removed on startup
- [ ] Test requests work without API key over Unix socket
- [ ] Test same endpoints available as TCP
- [ ] Test socket cleaned up on shutdown
- [ ] Test error handling for permission denied

### Success Criteria
- ✅ Unix socket available at `/var/run/nano/control.sock` (configurable)
- ✅ Filesystem permissions control access (0o660)
- ✅ No API key required for Unix socket access
- ✅ Same admin endpoints available as HTTP
- ✅ Stale socket files cleaned up on startup
- ✅ Socket removed on graceful shutdown

---

## Integration & Coordination

### Module Dependencies

```
Plan 07-01 (Logging)
  └─> Used by: All other plans

Plan 07-02 (Metrics)
  └─> Used by: 07-05 (metrics endpoint)
  └─> Used by: Request handling (main HTTP)

Plan 07-03 (Graceful Shutdown)
  └─> Used by: 07-05, 07-06 (admin servers)
  └─> Integrates with: Existing RequestDrain

Plan 07-04 (OOM Detection)
  └─> Integrates with: Existing MemoryLimiter
  └─> Uses: 07-01 (logging)

Plan 07-05 (Admin API HTTP)
  └─> Uses: 07-02 (metrics endpoint)
  └─> Uses: 07-01 (logging)
  └─> Integrates with: AppRegistry, WorkerPool

Plan 07-06 (Unix Socket)
  └─> Reuses: 07-05 router
  └─> Uses: 07-03 (shutdown)
```

### Execution Order

1. **07-01 Structured JSON Logging** - Foundation for all other logging
2. **07-03 Graceful Shutdown** - Infrastructure needed by servers
3. **07-02 Prometheus Metrics** - Metrics collection before endpoint
4. **07-04 OOM Detection** - Safety feature, can be parallel with 02
5. **07-05 Admin API HTTP** - Requires 02, 03
6. **07-06 Unix Socket** - Requires 05, 03

### State Sharing

All plans share these components via `Arc`:
- `Arc<RwLock<AppRegistry>>` - App configurations
- `Arc<HashMap<String, WorkerPool>>` - Worker pools per app
- `Arc<MetricsRegistry>` - Prometheus metrics
- `broadcast::Sender<()>` - Shutdown signal

### Configuration Schema Additions

```rust
// Add to config/app.rs or new config/admin.rs
pub struct AdminConfig {
    pub http_port: u16,           // Default: 8889
    pub api_key: String,          // Required, 32+ chars recommended
    pub unix_socket_path: Option<PathBuf>, // Default: /var/run/nano/control.sock
}

pub struct ShutdownConfig {
    pub drain_timeout_secs: u64,  // Default: 30, Range: 5-300
}

pub struct LoggingConfig {
    pub format: LogFormat,        // Json | Pretty
    pub level: String,            // Default: "info"
}
```

### Error Handling

All admin errors use consistent JSON format:
```json
{
  "error": "NotFound",
  "message": "App 'unknown.example.com' not found",
  "code": 404
}
```

---

## Success Criteria Summary

### Phase Completion Checklist

- [ ] **PROD-01**: Structured JSON logs with ts, level, event, hostname, request_id, worker_id, isolate_id
- [ ] **PROD-02**: Prometheus metrics at `/_admin/metrics` with request counts, latency histograms, error rates
- [ ] **PROD-03**: SIGTERM/SIGINT handling with graceful shutdown and request drain (default 30s timeout)
- [ ] **PROD-04**: OOM detection triggering isolate termination with structured `oom_kill` log event
- [ ] **PROD-05**: HTTP Admin API on port 8889 with API key authentication on X-Admin-Key header
- [ ] **PROD-06**: Unix domain socket at `/var/run/nano/control.sock` with filesystem permission security
- [ ] **PROD-07**: Runtime app CRUD (create, read, update, delete, disable, enable, reload, scale) without restart
- [ ] **PROD-08**: Admin diagnostics endpoint `/admin/isolates` with ps-style output

### Verification Tests

- [ ] All unit tests pass (`cargo test`)
- [ ] Integration tests verify each requirement
- [ ] Prometheus scrape returns valid format
- [ ] Graceful shutdown completes within timeout
- [ ] OOM detection logs correct fields
- [ ] Admin API endpoints return correct status codes
- [ ] Unix socket accessible with correct permissions

---

**Plan Version:** 1.0  
**Created:** 2026-04-19  
**Based on:** 07-CONTEXT.md decisions D-01 through D-17
