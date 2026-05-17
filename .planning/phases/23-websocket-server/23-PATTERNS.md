# Phase 23: WebSocket Server - Pattern Map

**Mapped:** 2026-05-17
**Files analyzed:** 8
**Analogs found:** 8 / 8

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/worker/mod.rs` | model | request-response | `src/worker/mod.rs` (HandlerTask) | exact — add field to existing struct |
| `src/worker/tenant_pool.rs` | service | event-driven | `src/worker/tenant_pool.rs` (TenantPool/worker_loop) | exact — extend existing struct and loop |
| `src/worker/pool.rs` | service | event-driven | `src/worker/pool.rs` (pump_message_loop) | exact — reuse Promise resolution pattern |
| `src/runtime/apis.rs` | utility | request-response | `src/runtime/apis.rs` (bind_console, bind_crypto) | exact — same bind_* function signature |
| `src/http/router.rs` | middleware | request-response | `src/http/router.rs` (dispatch_to_worker_pool) | exact — add header branch at top |
| `src/http/server.rs` | config | request-response | `src/http/server.rs` (create_app_with_shutdown) | exact — add route before catch-all |
| `src/config/app.rs` | model | — | `src/config/app.rs` (AppLimits) | exact — add fields with serde defaults |
| `Cargo.toml` | config | — | `Cargo.toml` line 39 (`axum = "0.8"`) | exact — change to feature table |

---

## Pattern Assignments

### `src/worker/mod.rs` — add `ws: Option<WsChannels>` to HandlerTask

**Analog:** `src/worker/mod.rs` (existing HandlerTask struct)

**Existing HandlerTask struct** (lines 189–206):
```rust
#[derive(Debug)]
pub struct HandlerTask {
    pub entrypoint: String,
    pub request: NanoRequest,
    pub response_tx: oneshot::Sender<anyhow::Result<NanoResponse>>,
    pub hostname: String,
    pub start_time: std::time::Instant,
    pub cpu_time_limit_ms: u32,
    pub request_id: String,
    pub memory_limit_mb: u32,
    // ADD: pub ws: Option<WsChannels>,
}

// Safety annotation pattern (line 210):
unsafe impl Send for HandlerTask {}
```

**New WsChannels type to add above HandlerTask:**
```rust
/// Bridge between tokio relay task (async) and V8 worker thread (sync).
/// Uses std::sync::mpsc so the worker thread can call recv_timeout()
/// for idle shrink-to-zero (D-11b). tokio::sync::mpsc has no recv_timeout().
pub struct WsChannels {
    pub inbound_rx: std::sync::mpsc::Receiver<tungstenite::Message>,
    pub outbound_tx: std::sync::mpsc::SyncSender<tungstenite::Message>,
}

// WsChannels is intentionally NOT Send — it wraps mpsc types that hold
// raw V8 lifetimes on the worker side. Move it into the worker thread
// by passing through HandlerTask (which has unsafe Send impl).
```

**Copy import pattern** (lines 181–183):
```rust
use crate::http::{NanoRequest, NanoResponse};
use tokio::sync::oneshot;
// ADD: use tungstenite; (re-exported from tokio-tungstenite, already in Cargo.toml)
```

---

### `src/worker/tenant_pool.rs` — extend TenantPool + worker_loop

**Analog:** `src/worker/tenant_pool.rs` (TenantPool struct + run_worker function)

**Existing TenantPool struct** (lines 46–54) — copy and extend:
```rust
pub struct TenantPool {
    hostname: String,
    workers: Vec<TenantWorker>,
    next_worker: AtomicU64,
    // ADD these fields:
    ws_busy: Arc<std::sync::atomic::AtomicUsize>,
    max_ws_connections: u32,     // floor(worker_count / 2) by default (D-01b)
    ws_idle_timeout_ms: u64,     // 30_000 ms default (D-11b)
    #[allow(dead_code)]
    vfs_backend: VfsBackendEnum,
    #[allow(dead_code)]
    control_plane: Option<ControlPlane>,
}
```

**Existing channel creation pattern** (lines 103–104) — copy for WsChannels:
```rust
// HTTP worker channel (exact pattern to copy for ws_* variant):
let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
    mpsc::channel();

// WS relay → worker inbound:
// Use std::sync::mpsc::sync_channel so relay task (tokio) gets SyncSender
// and worker thread gets Receiver with recv_timeout().
let (ws_inbound_tx, ws_inbound_rx) = std::sync::mpsc::sync_channel::<tungstenite::Message>(128);
let (ws_outbound_tx, ws_outbound_rx) = std::sync::mpsc::sync_channel::<tungstenite::Message>(128);
```

**Existing 'isolate/'requests loop structure** (lines 165–215) — the WS 'ws_messages loop inserts INSIDE 'requests after `let task = ...` is received:
```rust
'isolate: loop {
    // ... create NanoIsolate, enter HandleScope, ContextScope ...

    'requests: loop {
        if served >= MAX_REQUESTS_PER_ISOLATE {
            break 'requests;  // D-03: if task.ws.is_some(), do NOT break here
        }

        let task = match task_rx.recv() {
            Ok(t) => t,
            Err(_) => break 'isolate,
        };

        // OOM check (lines 206–215) — copy into 'ws_messages too (D-13)
        if let Some(ref mon) = oom_monitor {
            let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
            if let Err(oom) = mon.check(iso_ref) {
                mon.log_oom_event(&oom, &task.request_id);
                // For WS: send close 1011 instead of sending OOM response
                break 'requests;
            }
        }

        // ADD: WS mode branch
        if let Some(ws_channels) = task.ws {
            ws_busy.fetch_add(1, Ordering::SeqCst);
            // ... set thread-locals, enter 'ws_messages loop ...
            ws_busy.fetch_sub(1, Ordering::SeqCst);
            break 'requests;  // D-10b: full context reset after WS connection
        }

        // ... existing HTTP handler path continues below ...
    }
}
```

**CpuTimeoutGuard pattern** (lines 270–276) — reuse per-message (D-09b):
```rust
// HTTP: one guard per request
let _timeout = if task.cpu_time_limit_ms > 0 {
    // SAFETY: iso_ptr is valid for this isolate's lifetime. CpuTimeoutGuard
    // stores the pointer and calls terminate_execution() from a timer thread,
    // which V8 documents as safe to call from any thread.
    let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
    Some(crate::data_plane::CpuTimeoutGuard::new(iso_ref, task.cpu_time_limit_ms))
} else { None };
// WS: identical pattern, created at start of each message, dropped before next recv_timeout()
```

**Existing imports** (lines 27–39) — copy and extend:
```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use anyhow::{anyhow, Result};
use tracing::{error, info};
// ADD:
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
```

**TenantWorker struct** (lines 56–59) — copy pattern for WS worker variant:
```rust
struct TenantWorker {
    task_tx: mpsc::Sender<HandlerTask>,
    thread: Option<thread::JoinHandle<()>>,
}
// WS workers have the same shape; they receive HandlerTask with ws.is_some()
// through the SAME task_tx channel (D-06b / D-13b).
```

---

### `src/worker/pool.rs` — pump_message_loop (reference, no changes needed)

**Analog:** `src/worker/pool.rs` lines 1097–1128

**pump_message_loop pattern** — copy verbatim into 'ws_messages for async WS handlers (D-11):
```rust
Some(v) if v.is_promise() => {
    let promise = v.cast::<v8::Promise>();
    let platform = v8::V8::get_current_platform();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        for _ in 0..5 {
            // SAFETY: pump_message_loop requires &Isolate.
            // iso_ptr is valid and pinned to this thread.
            let iso: &v8::Isolate = unsafe { &*iso_ptr };
            v8::Platform::pump_message_loop(&platform, iso, false);
        }
        tc.perform_microtask_checkpoint();
        match promise.state() {
            v8::PromiseState::Fulfilled => break promise.result(&tc),
            v8::PromiseState::Rejected => {
                let err = promise.result(&tc);
                let msg = err.to_string(&tc)
                    .map(|s| s.to_rust_string_lossy(&tc))
                    .unwrap_or_else(|| "Promise rejected".to_string());
                return Err(anyhow!("Promise rejected: {}", msg));
            }
            v8::PromiseState::Pending => {
                if std::time::Instant::now() > deadline {
                    return Err(anyhow!("Async handler timed out after 30s"));
                }
                std::thread::yield_now();
            }
        }
    }
}
```

---

### `src/runtime/apis.rs` — add `bind_websocket_pair()`

**Analog:** `src/runtime/apis.rs` (bind_console, bind_crypto patterns)

**bind_all() call site** (lines 35–59) — add ONE line at end of the call list:
```rust
pub fn bind_all(
    scope: &mut v8::PinnedRef<v8::HandleScope<'_, ()>>,
    context: v8::Local<v8::Context>,
) {
    // ... existing calls ...
    Self::bind_streams(scope, context);
    Self::bind_wasm(scope, context);
    Self::bind_websocket_pair(scope, context);  // ADD — must be last or after streams
}
```

**bind_websocket_pair() function signature** — copy from bind_console (lines 90–91):
```rust
fn bind_websocket_pair(
    scope: &mut v8::PinnedRef<v8::HandleScope<()>>,
    context: v8::Local<v8::Context>,
) {
    crate::runtime::websocket::bind_websocket_pair(scope, context);
}
```

**V8 object construction pattern** (lines 91–118 of apis.rs — bind_console):
```rust
// Pattern: enter ContextScope, create Object, attach Function::new callbacks, attach to global
let global = context.global(scope);
let mut ctx_scope = v8::ContextScope::new(scope, context);

let obj = v8::Object::new(&mut &mut ctx_scope);

if let Some(method_fn) = v8::Function::new(&mut ctx_scope, method_callback) {
    let key = v8::String::new(&mut ctx_scope, "methodName").unwrap();
    obj.set(&mut ctx_scope, key.into(), method_fn.into());
}

let global_key = v8::String::new(&mut ctx_scope, "GlobalName").unwrap();
global.set(&mut ctx_scope, global_key.into(), obj.into());
```

**FunctionTemplate constructor pattern** (lines 129–141 of apis.rs — bind_text_encoder):
```rust
// Pattern: FunctionTemplate for constructors (new WebSocketPair())
let ctor_template = v8::FunctionTemplate::new(&mut ctx_scope, websocket_pair_constructor);
let ctor = ctor_template.get_function(&mut &mut ctx_scope).unwrap();
let key = v8::String::new(&mut ctx_scope, "WebSocketPair").unwrap();
global.set(&mut ctx_scope, key.into(), ctor.into());
```

**Thread-local declaration pattern** (lines 15–21 of apis.rs — PERFORMANCE_BASELINE):
```rust
// Existing thread-local in apis.rs — copy this exact pattern for WS state:
use std::cell::Cell;
thread_local! {
    static PERFORMANCE_BASELINE: Cell<Option<Instant>> = Cell::new(None);
}

// WS thread-locals follow same pattern (add to websocket.rs):
use std::cell::{Cell, RefCell};
thread_local! {
    static WS_ACCEPTED: Cell<bool> = Cell::new(false);
    static WS_OUTBOUND: RefCell<Option<std::sync::mpsc::SyncSender<tungstenite::Message>>>
        = RefCell::new(None);
    static WS_MESSAGE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
    static WS_CLOSE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
    static WS_ERROR_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
    static WS_SERVER_SOCKET: RefCell<Option<v8::Global<v8::Object>>> = RefCell::new(None);
}
```

---

### `src/http/router.rs` — Upgrade header detection in dispatch_to_worker_pool

**Analog:** `src/http/router.rs` (dispatch_to_worker_pool, lines 791–1039)

**Upgrade detection branch** — insert at the TOP of dispatch_to_worker_pool, BEFORE `body = request.into_body()` on line 819 (body must NOT be consumed before upgrade):
```rust
pub async fn dispatch_to_worker_pool(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    // Extract host first (existing pattern lines 799–806)
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(':').next().unwrap_or(s).to_string())
        .unwrap_or_else(|| "default".to_string());

    // ADD: WebSocket upgrade detection BEFORE body is consumed
    if request
        .headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
    {
        return handle_ws_upgrade(state, request, host).await;
    }

    // ... existing: request_id, span, method, uri, headers, body = request.into_body() ...
}
```

**503 response pattern** (lines 958–966) — copy for WS connection limit exceeded:
```rust
// Existing 503 pattern for queue full:
Err(QueueError::ChannelFull) => {
    tracing::warn!("WorkQueue full for hostname: {}", host);
    let response = Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header("Retry-After", "1")
        .header("content-type", "text/plain")
        .body(Body::from("Service Unavailable - Queue Full"))
        .unwrap();
    (response, 503, None, None)
}
// WS limit exceeded returns same shape:
// StatusCode::SERVICE_UNAVAILABLE, "content-type: text/plain", "WebSocket limit reached"
```

**Existing imports** (lines 29–42) — add WS imports after existing:
```rust
use axum::{
    body::Body,
    extract::State,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
// ADD for WS:
// use axum::extract::ws::{WebSocketUpgrade, WebSocket, Message as WsMessage};
// Note: axum::extract::ws only available after Cargo.toml ws feature is enabled
```

---

### `src/http/server.rs` — add WS route before catch-all

**Analog:** `src/http/server.rs` create_app_with_shutdown (lines 197–227)

**Route insertion point** (lines 210–218) — add WS route BEFORE the `/{*path}` catch-all:
```rust
pub fn create_app_with_shutdown(state: Arc<AppStateWithShutdown>) -> Router {
    let app_state_clone = Arc::new(state.app_state.clone());

    Router::new()
        .route("/health", get(health_handler))
        .route("/_admin/health", get(admin_health_handler))
        .route("/_admin/ready", get(ready_handler))
        .route("/_admin/metrics", get(metrics_handler))
        // ADD: WebSocket upgrade route — registered BEFORE catch-all
        // axum matches routes in registration order; WS clients send GET with
        // Upgrade: websocket. The ws_upgrade_handler checks the header explicitly.
        // Alternative: detect in dispatch_to_worker_pool instead (see router.rs pattern).
        .route("/", any({
            let state = app_state_clone.clone();
            move |req| dispatch_to_worker_pool(AxumState(state), req)
        }))
        .route("/{*path}", any({
            let state = app_state_clone;
            move |req| dispatch_to_worker_pool(AxumState(state), req)
        }))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CompressionLayer::new())
        .with_state(state)
}
```

**Note on routing strategy:** Per Open Question 1 in RESEARCH.md, the cleanest path is to detect the `Upgrade: websocket` header at the TOP of `dispatch_to_worker_pool` (before body consumption) rather than adding a separate route. The existing `any()` catch-all already handles GET requests. No separate route entry is required if the branch-at-top approach is used in router.rs.

---

### `src/config/app.rs` — add WS fields to AppLimits

**Analog:** `src/config/app.rs` (AppLimits struct, lines 38–58)

**Existing struct and serde default pattern** (lines 38–70):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppLimits {
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u32,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
    #[serde(default = "default_workers")]
    pub workers: u32,
    #[serde(default = "default_cpu_time_ms")]
    pub cpu_time_ms: u32,
    #[serde(default = "default_cpu_time_enabled")]
    pub cpu_time_enabled: bool,
    // ADD:
    #[serde(default)]
    pub max_ws_connections: Option<u32>,   // default: floor(workers / 2) — enforced at runtime
    #[serde(default)]
    pub ws_idle_timeout_ms: Option<u64>,   // default: 30_000 — enforced at runtime
}
```

**Default function pattern** (lines 72–90) — add corresponding defaults:
```rust
// Existing pattern:
fn default_memory_mb() -> u32 { 128 }
fn default_workers() -> u32 { 4 }

// WS defaults: Option<T> with #[serde(default)] returns None automatically.
// Runtime default (floor(workers/2)) is computed in TenantPool::new(), not here.
```

**IMPORTANT:** `#[serde(deny_unknown_fields)]` on AppLimits means the new fields MUST be added to this struct or deserialization of existing configs will BREAK. Do not add to a separate struct unless deny_unknown_fields is removed.

---

### `Cargo.toml` — enable axum `ws` feature

**Analog:** `Cargo.toml` line 39

**Change required** (line 39):
```toml
# FROM:
axum = "0.8"

# TO:
axum = { version = "0.8", features = ["ws"] }
```

**tokio-tungstenite** (line 83 — already present, no change needed):
```toml
tokio-tungstenite = "0.24"
```

This is the **Wave 0 blocker** — axum `ws` feature must be enabled before any `axum::extract::ws::*` import compiles.

---

### Tests

**Analog:** Integration test pattern — no existing WS test to copy from.

**Existing test structure** (from `src/http/router.rs` lines 1215–1290):
```rust
// Pattern for tokio integration tests:
#[tokio::test]
async fn test_wintertc_handler_response() {
    crate::v8::platform::initialize_platform().expect("Failed to initialize V8 platform");

    let temp_dir = tempfile::tempdir().unwrap();
    let js_path = temp_dir.path().join("index.js");
    std::fs::write(&js_path, js_code).unwrap();

    let target = RouteTarget { ... };
    let request = NanoRequest::new("GET".to_string(), ...);
    let response = target.handle(request).await;
    assert_eq!(response.status(), 200);
}
```

**WS test shape** — see RESEARCH.md "No Analog Found" note. Use `tokio_tungstenite::connect_async` to open a client connection to a test server, then verify echo behavior. No existing pattern in codebase; implement from research.

---

## Shared Patterns

### std::sync::mpsc Channel Creation
**Source:** `src/worker/tenant_pool.rs` lines 103–104
**Apply to:** WsChannels inbound/outbound construction
```rust
let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
    mpsc::channel();
// WS variant uses sync_channel for bounded capacity:
let (inbound_tx, inbound_rx) = std::sync::mpsc::sync_channel::<tungstenite::Message>(128);
```

### V8 TryCatch + JS Exception Propagation
**Source:** `src/worker/tenant_pool.rs` lines 329–340
**Apply to:** All V8 callback invocations in 'ws_messages loop
```rust
let tc_storage = v8::TryCatch::new(&mut *ctx_scope);
let tc_pin = std::pin::pin!(tc_storage);
let mut tc = tc_pin.init();

let call_result = handler_local.call(&tc, global_obj.into(), &[js_req.into()]);
match call_result {
    None => {
        let msg = tc.exception()
            .and_then(|e| e.to_string(&tc))
            .map(|s| s.to_rust_string_lossy(&tc))
            .unwrap_or_else(|| "unknown JS exception".to_string());
        return Err(anyhow!("JS exception: {}", msg));
    }
    // ...
}
```

### SAFETY Comment Pattern for iso_ptr
**Source:** `src/worker/tenant_pool.rs` lines 207–210
**Apply to:** All `unsafe { &mut *iso_ptr }` usages in WS loop
```rust
// SAFETY: iso_ptr was captured from nano.isolate() before the HandleScope
// was created. The isolate is pinned to this thread and nano outlives scope.
// OomMonitor::check() only reads heap statistics via v8::HeapStatistics.
let iso_ref: &mut v8::Isolate = unsafe { &mut *iso_ptr };
```

### Handler Cache Lookup
**Source:** `src/worker/tenant_pool.rs` lines 278–283
**Apply to:** WS connection — reuse same handler_cache for entrypoint lookup
```rust
let handler_g = handler_cache.get(&entrypoint)
    .expect("handler must be cached: just inserted in block above");
let global_obj = context.global(&mut ctx_scope);
let handler_local = v8::Local::new(&mut ctx_scope, handler_g);
```

### Object Property Set (V8)
**Source:** `src/runtime/apis.rs` lines 100–102
**Apply to:** readyState, binaryType property setting on WebSocket objects
```rust
let key = v8::String::new(&mut ctx_scope, "propertyName").unwrap();
let val = v8::Integer::new_from_unsigned(&mut ctx_scope, value_u32);
obj.set(&mut ctx_scope, key.into(), val.into());
```

### HTTP 503 Response (Service Unavailable)
**Source:** `src/http/router.rs` lines 958–966
**Apply to:** WS connection limit exceeded response
```rust
Response::builder()
    .status(StatusCode::SERVICE_UNAVAILABLE)
    .header("Retry-After", "1")
    .header("content-type", "text/plain")
    .body(Body::from("Service Unavailable - WebSocket limit reached"))
    .unwrap()
```

### Thread Spawn + JoinHandle Storage
**Source:** `src/worker/tenant_pool.rs` lines 106–113
**Apply to:** Lazily-spawned WS worker threads stored in TenantPool
```rust
let thread = thread::spawn(move || {
    Self::worker_loop(id, hostname, memory_limit_mb, vfs_backend, task_rx);
});
Ok(TenantWorker {
    task_tx,
    thread: Some(thread),  // Option<JoinHandle> for clean Drop impl
})
```

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/runtime/websocket.rs` (new) | utility | event-driven | No WebSocketPair or event-driven V8 binding exists yet. Use RESEARCH.md Patterns 3–6 as primary reference. |
| WS integration tests (`tests/websocket_*.rs`) | test | event-driven | No existing WS tests in codebase. Use tokio_tungstenite::connect_async client pattern from library docs. |

---

## Implementation Wave Order

The planner MUST preserve this order — each wave depends on the previous compiling cleanly:

| Wave | Change | Blocking for |
|------|--------|-------------|
| 0 | `Cargo.toml`: `axum = { version = "0.8", features = ["ws"] }` | All other WS code |
| 1 | `src/worker/mod.rs`: add `WsChannels` struct + `ws: Option<WsChannels>` to HandlerTask | Waves 2, 3, 4 |
| 1 | `src/config/app.rs`: add `max_ws_connections`, `ws_idle_timeout_ms` to AppLimits | Wave 2 |
| 2 | `src/runtime/websocket.rs` (new): bind_websocket_pair, thread-locals, callbacks | Wave 4 |
| 2 | `src/runtime/apis.rs`: add `bind_websocket_pair()` call in `bind_all()` | Wave 4 |
| 3 | `src/worker/tenant_pool.rs`: extend TenantPool + add 'ws_messages loop | Wave 4 |
| 3 | `src/http/router.rs`: add Upgrade header branch in dispatch_to_worker_pool | Wave 4 |
| 4 | Integration tests | Final validation |

---

## Metadata

**Analog search scope:** `src/worker/`, `src/runtime/`, `src/http/`, `src/config/`, `Cargo.toml`
**Files scanned:** 8 source files read directly
**Pattern extraction date:** 2026-05-17
