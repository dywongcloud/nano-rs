# Phase 07 Research: Production Features & Admin API

**Research Date:** 2026-04-19  
**Target Phase:** 07 — Production Features & Admin API

---

## 1. Structured JSON Logging with tracing-subscriber

### Technical Overview
The `tracing` crate combined with `tracing-subscriber` provides structured logging infrastructure. JSON format is enabled via the `json` feature flag.

### Key Configuration Patterns

```rust
// Basic JSON setup with custom fields
tracing_subscriber::fmt()
    .json()
    .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
    .flatten_event(true)  // Flatten event fields into root object
    .with_current_span(false)
    .init();
```

### Custom Layer for Rich Context
For per-request fields (hostname, request_id, worker_id), implement a custom `Layer`:

```rust
use tracing_subscriber::Layer;
use std::collections::BTreeMap;

pub struct NanoJsonLayer;

impl<S> Layer<S> for NanoJsonLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut fields);
        event.record(&mut visitor);
        
        // Capture span context for worker_id, isolate_id
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(storage) = span.extensions().get::<CustomFields>() {
                    // Merge span fields
                }
            }
        }
        
        let output = serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "level": event.metadata().level().to_string(),
            "event": event.metadata().name(),
            "fields": fields,
        });
        println!("{}", output);
    }
}
```

### Required Fields (per D-01)
- `ts`: ISO8601 timestamp
- `level`: INFO/WARN/ERROR
- `event`: Event type identifier
- `hostname`: Virtual host
- `request_id`: UUID per request
- `worker_id`: Worker thread identifier
- `isolate_id`: V8 isolate identifier
- `memory_bytes`: Current heap usage (optional)
- `duration_ms`: Request duration (optional)
- `message`: Human-readable description

### Best Practices
- Use `RUST_LOG` env filter for development: `RUST_LOG=info,nano=debug`
- Output to stdout only (container-friendly)
- Include contextual fields via span!() macros
- Use `flatten_event(true)` for cleaner JSON structure

---

## 2. Prometheus Metrics Exposition Format

### Format Specification
Prometheus text format uses line-oriented output:

```
# HELP http_requests_total Total HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",status="200"} 1027

# TYPE request_duration_seconds histogram
request_duration_seconds_bucket{le="0.1"} 33444
request_duration_seconds_bucket{le="0.5"} 129389
request_duration_seconds_bucket{le="+Inf"} 144320
request_duration_seconds_sum 53423
request_duration_seconds_count 144320
```

### Content Type Header
```
Content-Type: text/plain; version=0.0.4; charset=utf-8
```

### Recommended Histogram Buckets (per D-04)
For request latency (milliseconds):
- 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1000ms, +Inf

### Metric Naming Conventions
- `nano_requests_total`: Total requests (counter)
- `nano_request_duration_ms`: Request latency (histogram)
- `nano_errors_total`: Error count by code (counter)
- `nano_isolates_active`: Current isolates (gauge)
- `nano_memory_bytes`: Memory usage (gauge)
- `nano_worker_utilization`: Worker busy percentage (gauge)

### Implementation Strategy
Use `metrics` crate with `metrics-exporter-prometheus`:

```rust
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;

// In request handler
counter!("nano_requests_total", 1, "hostname" => host, "status" => status);
histogram!("nano_request_duration_ms", duration_ms);

// In isolate lifecycle
gauge!("nano_isolates_active", current_count as f64, "hostname" => host);
```

### Axum Handler
```rust
use axum::{extract::State, response::Response, http::StatusCode};
use metrics_exporter_prometheus::PrometheusHandle;

async fn metrics_handler(State(handle): State<PrometheusHandle>) -> Response {
    let metrics = handle.render();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(metrics.into())
        .unwrap()
}
```

---

## 3. Tokio Signal Handling for Graceful Shutdown

### Signal Sources
- `SIGTERM`: Container orchestration (Kubernetes, Docker)
- `SIGINT`: Ctrl+C in terminal

### Implementation Pattern

```rust
use tokio::signal;
use tokio::sync::broadcast;

pub fn shutdown_channel() -> broadcast::Sender<()> {
    let (tx, _rx) = broadcast::channel(1);
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
            _ = ctrl_c => info!("Received SIGINT"),
            _ = terminate => info!("Received SIGTERM"),
        }
        
        let _ = tx_clone.send(());
    });
    
    tx
}
```

### Axum Integration
```rust
let shutdown_tx = shutdown_channel();
let shutdown_rx = shutdown_tx.subscribe();

axum::serve(listener, app)
    .with_graceful_shutdown(async move {
        let _ = shutdown_rx.recv().await;
        info!("Starting graceful shutdown...");
        // Trigger drain logic
    })
    .await?;
```

### Request Drain Strategy
1. Stop accepting new connections (axum handles this)
2. Mark readiness probe as unhealthy
3. Wait for in-flight requests with timeout
4. Force close remaining connections after timeout

---

## 4. Unix Domain Sockets in Rust

### Creating Unix Listener
```rust
use tokio::net::UnixListener;
use std::path::Path;

async fn create_unix_socket(path: &Path) -> anyhow::Result<UnixListener> {
    // Remove stale socket
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    
    let listener = UnixListener::bind(path)?;
    
    // Set permissions (owner + group read/write)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o660);
        std::fs::set_permissions(path, perms)?;
    }
    
    Ok(listener)
}
```

### Dual Server Setup (HTTP + Unix Socket)
```rust
use axum::{Router, serve};
use std::sync::Arc;

async fn run_dual_server(
    tcp_app: Router,
    unix_app: Router,
    tcp_addr: SocketAddr,
    unix_path: &Path,
) -> anyhow::Result<()> {
    let shutdown = Arc::new(Notify::new());
    
    // TCP server
    let tcp_listener = tokio::net::TcpListener::bind(tcp_addr).await?;
    let tcp_shutdown = Arc::clone(&shutdown);
    let tcp_server = tokio::spawn(async move {
        serve(tcp_listener, tcp_app)
            .with_graceful_shutdown(async { tcp_shutdown.notified().await })
            .await
    });
    
    // Unix socket server
    let unix_listener = create_unix_socket(unix_path).await?;
    let unix_shutdown = Arc::clone(&shutdown);
    let unix_server = tokio::spawn(async move {
        serve(unix_listener, unix_app)
            .with_graceful_shutdown(async { unix_shutdown.notified().await })
            .await
    });
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    shutdown.notify_waiters();
    
    // Await both servers
    let (tcp_result, unix_result) = tokio::join!(tcp_server, unix_server);
    tcp_result??;
    unix_result??;
    
    Ok(())
}
```

### Socket Path Considerations
- Default: `/var/run/nano/control.sock`
- Requires directory exists with appropriate permissions
- Cleanup on shutdown (remove socket file)

---

## 5. Admin API Security Patterns

### API Key Authentication Middleware
```rust
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

impl AdminAuth {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
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
        _ => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    }
}
```

### Route Protection
```rust
let admin_routes = Router::new()
    .route("/admin/isolates", get(list_isolates))
    .route("/admin/apps", get(list_apps).post(create_app))
    .route("/admin/apps/:host", patch(update_app).delete(delete_app))
    .route("/admin/apps/:host/disable", post(disable_app))
    .route("/admin/apps/:host/enable", post(enable_app))
    .route("/admin/metrics", get(metrics_handler))
    .route_layer(middleware::from_fn_with_state(
        admin_auth.clone(),
        api_key_middleware,
    ));
```

### Unix Socket Security
- Filesystem permissions (0o660) control access
- Owner/group membership verification
- No API key required for Unix socket (trusted local access)

---

## 6. OOM Detection and Heap Monitoring

### V8 Heap Statistics
```rust
use rusty_v8::{Isolate, HeapStatistics};

pub fn get_heap_stats(isolate: &Isolate) -> HeapStatistics {
    let mut stats = HeapStatistics::default();
    isolate.get_heap_statistics(&mut stats);
    stats
}
```

### Memory Limit Enforcement
Already implemented in `worker/limits.rs` via `MemoryLimiter` trait:

```rust
impl MemoryLimiter for Isolate {
    fn is_oom(&self, limit_bytes: usize) -> bool {
        let mut stats = HeapStatistics::default();
        self.get_heap_statistics(&mut stats);
        stats.used_heap_size() > limit_bytes
    }
}
```

### OOM Response Strategy (per D-08, D-09)
1. Detect OOM during request execution
2. Log `oom_kill` event with context
3. Return 503 Service Unavailable to client
4. Dispose isolate immediately (no grace period)
5. Worker continues, fresh isolate created next request

### OOM Log Structure
```json
{
  "ts": "2026-04-19T17:57:00Z",
  "level": "ERROR",
  "event": "oom_kill",
  "hostname": "app.example.com",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "used_bytes": 104857600,
  "limit_bytes": 67108864,
  "isolate_id": "isolate_7_worker_3",
  "message": "Isolate terminated: heap limit exceeded"
}
```

### False Positive Prevention
- Set limit at 80% of available memory
- Monitor allocation rate trends
- Only trigger on sustained over-limit, not momentary spikes

---

## 7. Summary & Implementation Order

1. **Structured Logging** → Custom tracing layer, JSON output
2. **Metrics** → metrics crate + Prometheus exporter
3. **Graceful Shutdown** → Signal handling + drain logic
4. **Admin API HTTP** → Axum routes + API key auth
5. **Unix Socket** → tokio::net::UnixListener + permission handling
6. **OOM Detection** → Integrate with existing MemoryLimiter
7. **Runtime Management** → Admin endpoints for app CRUD

### Dependencies to Add
```toml
[dependencies]
# Already present: tracing, tokio, axum, metrics
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
metrics-exporter-prometheus = "0.16"
chrono = { version = "0.4", features = ["serde"] }
```

### Performance Considerations
- Metrics: Atomic counters, lock-free
- Logging: Async write via non-blocking layer
- Shutdown: Bounded drain timeout (default 30s)
- OOM: Immediate termination (no allocation during cleanup)
