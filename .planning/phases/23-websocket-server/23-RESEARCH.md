# Phase 23: WebSocket Server - Research

**Researched:** 2026-05-17
**Domain:** Rust async WebSocket upgrade, axum ws extractor, V8 rusty_v8 FunctionCallback, tokio mpsc channels
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Connection Lifecycle Model**
- D-01: Pin-a-worker. One WebSocket connection = one dedicated TenantPool worker thread for the connection's full duration. The worker enters a WS message loop instead of the normal request loop. No shared state between WS connection and other workers.
- D-02: Worker is marked `WsActive` (new enum variant on worker state). TenantPool skips WsActive workers during normal request dispatch. Worker returns to `Available` after connection closes.
- D-03: Drain-then-recycle on isolate limit. If the isolate hits MAX_REQUESTS_PER_ISOLATE while serving a WS connection, it continues serving the connection. Recycle happens after the WS connection closes cleanly. The 10k counter is paused during WS mode.

**JavaScript API**
- D-04: CF WebSocketPair pattern. `new WebSocketPair()` returns an object with keys `0` and `1`. JS destructures: `const [client, server] = Object.values(new WebSocketPair())`. Handler calls `server.accept()`, returns `new Response(null, { status: 101, webSocket: client })`.
- D-05: Response carries the socket. The `Response` constructor must accept a `webSocket` option. The axum handler detects this property on the returned NanoResponse and performs the actual HTTP upgrade.
- D-06: Events on server socket. `server.addEventListener('message', (evt) => ...)` — `evt.data` is string or ArrayBuffer. `server.addEventListener('close', (evt) => ...)` — `evt.code` and `evt.reason`. `server.addEventListener('error', ...)`.

**Connection Limits**
- D-07: Per-tenant configurable. Add `max_ws_connections: Option<u32>` to `AppConfig` (default: equal to `worker_count`). TenantPool enforces this before upgrading — returns 503 if at limit.
- D-08: Natural backpressure. WS pins a worker; HTTP requests queue behind WS if all workers are in WS mode.

**Message Handling**
- D-09: 32 MiB message limit. Incoming messages > 32 MiB trigger WS close frame with code 1009.
- D-10: Sequential per-connection. Messages on one connection processed one at a time.
- D-11: Async handlers. If JS fetch handler returns a Promise, resolve it using existing pump_message_loop pattern.

**Half-Open Behavior**
- D-12: Connection terminates with worker. If worker/thread panics, WS socket is dropped. tokio-tungstenite sends a close frame on drop.
- D-13: Graceful OOM. If OOM is detected during WS mode, send close frame code 1011 before recycling the isolate.

**Claude's Discretion**
- Wire protocol framing: use `tokio-tungstenite` (already in Cargo.toml)
- axum WebSocket extractor vs raw tungstenite upgrade — use axum's `ws::WebSocketUpgrade` for cleaner integration
- Internal channel type: `tokio::sync::mpsc` (unbounded, single-producer single-consumer per connection)
- Thread join strategy when WS worker exits: existing TenantWorker join handle pattern

### Additional Locked Decisions (from additional_context)
- D-01b: max_ws_connections default = floor(worker_count / 2)
- D-02b: WS pool is a FIELD on TenantPool (not a separate struct)
- D-03b: Lazy pool — workers spawn on demand, shrink-to-zero after ws_idle_timeout_ms (configurable, default 30_000)
- D-04b: Reusable WS worker pool (context reset between connections, not between messages)
- D-05b: Frame routing — tokio async task owns WebSocket stream, relays via mpsc channels
- D-06b: HandlerTask extended with `pub ws: Option<WsChannels>` — no new WsTask type
- D-07b: WsFrame type = tokio_tungstenite::tungstenite::Message directly (no rewrapping)
- D-08b: JS WebSocketPair — FunctionCallback on send(), stored Global<Function> callbacks for addEventListener
- D-09b: Per-message CpuTimeoutGuard
- D-10b: Full context reset between WS connections
- D-11b: Pool shrink — shrink to zero after ws_idle_timeout_ms (configurable, default 30s)
- D-12b: 32 MiB message limit enforced on tokio relay side, close 1009
- D-13b: Worker availability — loop-back on task_rx + AtomicUsize ws_busy counter
- D-14b: accept() guard — TypeError thrown if send() called before accept()
- D-15b: Ping/pong transparent — tungstenite handles, never reaches JS
- D-16b: readyState JS-owned — worker sets integer property at transitions
- D-17b: Stream error → drop inbound_tx → worker sees None from blocking_recv → abnormal close (1006, wasClean: false)
- D-18b: binaryType fixed "arraybuffer", setter throws TypeError

**Config additions (locked):**
```rust
max_ws_connections: Option<u32>,   // default: floor(worker_count / 2)
ws_idle_timeout_ms: Option<u64>,   // default: 30_000
```

**WsChannels type (locked):**
```rust
pub struct WsChannels {
    pub inbound_rx: mpsc::Receiver<tokio_tungstenite::tungstenite::Message>,
    pub outbound_tx: mpsc::Sender<tokio_tungstenite::tungstenite::Message>,
}
```

### Deferred Ideas (OUT OF SCOPE)
- WebSocket Hibernation API (CF Durable Objects pattern) — Phase 24+
- Outbound WebSocket from JS — after server WS is stable
- Multi-client broadcast — requires inter-isolate messaging, Phase 26
- permessage-deflate compression — Phase 25
- WSS/TLS — handled at reverse proxy layer
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| WS-01 | HTTP 101 upgrade via axum extractor; WebSocketPair JS global | axum `ws` feature must be added to Cargo.toml; WebSocketPair bound in `RuntimeAPIs::bind_all()` |
| WS-02 | Pin-a-worker lifecycle with drain-then-recycle on isolate limit | TenantPool extended with ws_workers field + AtomicUsize ws_busy counter; worker state enum gains WsActive variant |
| WS-03 | Message loop: 32 MiB limit, close codes 1009/1011/1006, readyState, binaryType, addEventListener | tokio async relay task + blocking worker loop; per-message CpuTimeoutGuard; V8 integer property set for readyState |
</phase_requirements>

---

## Summary

Phase 23 adds server-side WebSocket support to nano-rs using the Cloudflare Workers `WebSocketPair` API as the JavaScript surface. The architecture follows the "pin-a-worker" model: one dedicated worker thread per WebSocket connection for its full duration. The primary challenge is bridging three concurrency worlds — axum's async HTTP upgrade, tokio's async WebSocket frame relay, and V8's single-threaded synchronous execution model.

The codebase is well-prepared. `tokio-tungstenite = "0.24"` is already in Cargo.toml. The `TenantPool` and its `worker_loop` provide the exact pattern to extend with a `'ws_messages` inner loop. The `CpuTimeoutGuard` and OOM monitor are reusable per-message. The `RuntimeAPIs::bind_all()` function is the single add point for `bind_websocket_pair()`.

The single highest-risk item discovered in codebase inspection: **axum's `ws` feature is NOT currently enabled**. The Cargo.toml declares `axum = "0.8"` without features, and the resolved feature set (verified via cargo metadata) confirms `ws` is absent. This must be corrected in Wave 0 before any axum WebSocket extractor code can compile. The feature is available in axum 0.8.9 (confirmed from cargo metadata).

A second structural finding: the decisions describe a "lazy WS pool on TenantPool" model where WS workers are spawned on demand and shrink to zero after idle timeout. The actual `TenantPool` struct uses standard `std::sync::mpsc` channels (not tokio), and HTTP workers are pre-spawned. The WS extension adds a separate `ws_workers` field with a different lifecycle (lazy/shrink) from the pre-spawned HTTP workers — this is a meaningful structural difference requiring careful implementation.

**Primary recommendation:** Implement in strict wave order: (1) cargo feature flag + data types, (2) axum upgrade route + tokio relay task, (3) WS worker loop extension in TenantPool, (4) V8 WebSocketPair binding. Each wave depends on the previous.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| HTTP 101 Upgrade | API / Backend (axum) | — | axum handles the HTTP handshake and protocol switch |
| WebSocket frame I/O | Async relay (tokio task) | — | owns the WebSocket stream, bridges to sync worker via mpsc |
| JS handler execution | Worker thread (V8) | — | V8 is single-threaded; blocking_recv() drives message loop |
| WebSocketPair JS API | Worker thread (V8 binding) | — | runtime API binding in RuntimeAPIs::bind_all() |
| Connection limit enforcement | TenantPool | — | AtomicUsize ws_busy counter checked before accepting upgrade |
| Config (max_ws_connections, ws_idle_timeout_ms) | AppLimits struct | AppConfig | matches existing limits field pattern |

---

## Standard Stack

### Core (all already in Cargo.toml)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio-tungstenite | 0.24 | WebSocket wire protocol over tokio | Already declared in Cargo.toml line 83; standard Rust WS library |
| tungstenite | 0.24 | Message types (Text, Binary, Close, Ping, Pong) | Transitive dep of tokio-tungstenite; D-07b uses Message directly |
| axum (ws feature) | 0.8 | `WebSocketUpgrade` extractor for HTTP 101 | Clean integration with existing axum router; D-04/D-05 decision |
| tokio::sync::mpsc | (tokio 1.52) | inbound/outbound frame channels | D-05b decision; matches existing oneshot usage in HandlerTask |

[VERIFIED: cargo metadata] — all packages confirmed in resolved dependency graph.

### Feature Flag Addition Required

```toml
# Cargo.toml — change from:
axum = "0.8"
# to:
axum = { version = "0.8", features = ["ws"] }
```

[VERIFIED: cargo metadata] — axum 0.8.9 has `ws` feature available but it is NOT currently enabled.

### No New Dependencies

No new crates needed. Only the axum feature flag addition is required.

---

## Package Legitimacy Audit

All packages are already in the project's Cargo.toml and dependency graph — no new package installs.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| tokio-tungstenite | crates.io | ~5 yrs | millions | github.com/snapview/tokio-tungstenite | N/A (pre-existing) | Already in project |
| axum (ws feature) | crates.io | ~3 yrs | millions | github.com/tokio-rs/axum | N/A (feature only) | Already in project |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*slopcheck was unavailable at research time. No new packages are being added — all are pre-existing verified dependencies.*

---

## Architecture Patterns

### System Architecture Diagram

```
HTTP Client (GET /path, Upgrade: websocket)
    |
    v
axum router (dispatch_to_worker_pool)
    |  detect Upgrade header
    v
TenantPool::try_claim_ws_worker()
    |  checks ws_busy < max_ws_connections
    |  returns WsChannels pair
    v
axum WebSocketUpgrade.on_upgrade(async move |ws| {
    // tokio async relay task
    loop {
        tokio::select! {
            frame = ws.recv() => {
                // enforce 32 MiB limit → close 1009
                inbound_tx.send(frame)
            }
            msg = outbound_rx.recv() => {
                ws.send(msg)
            }
        }
    }
})
    |
    v  (mpsc channels)
TenantWorker 'ws_messages loop (blocking on sync receiver)
    |
    blocking_recv() with recv_timeout for idle shrink
    |
    v
V8 isolate (JS message handler)
    |  call message_handler(evt)
    |  resolve Promise if async
    |  CpuTimeoutGuard per message
    |
    for each outbound frame from server.send():
        outbound_tx.send(msg)
    |
    v
[loop back to blocking_recv]

Connection close:
    relay task drops inbound_tx
    → worker sees Disconnected from recv_timeout
    → sends close frame via outbound_tx (code 1006 if abnormal)
    → resets context (D-10b)
    → ws_busy.fetch_sub(1)
    → if idle timeout: worker thread exits (shrink-to-zero)
```

### Recommended Project Structure

```
src/
├── worker/
│   ├── mod.rs          # HandlerTask: add pub ws: Option<WsChannels>
│   │                   # WsChannels struct definition here
│   ├── tenant_pool.rs  # TenantPool: add ws_busy counter + config fields
│   │                   # TenantPool::try_claim_ws_worker()
│   │                   # worker_loop: add 'ws_messages inner loop
│   └── pool.rs         # (no changes needed)
├── http/
│   ├── server.rs       # (no changes needed)
│   ├── router.rs       # dispatch_to_worker_pool: detect Upgrade header,
│   │                   # route to WS upgrade path before HandlerTask dispatch
│   └── config.rs       # (no changes needed — WS config goes in AppLimits)
├── config/
│   └── app.rs          # AppLimits: add max_ws_connections, ws_idle_timeout_ms
├── runtime/
│   └── apis.rs         # RuntimeAPIs::bind_all(): add bind_websocket_pair()
│                       # bind_websocket_pair() — new function in this file
└── Cargo.toml          # axum ws feature flag
```

### Pattern 1: axum WebSocketUpgrade Extractor Integration

The `axum::extract::ws::WebSocketUpgrade` extractor matches HTTP requests with `Upgrade: websocket` header. It DOES NOT perform the upgrade immediately — it returns a value consumed with `.on_upgrade(|socket| async move { ... })`. The returned `impl IntoResponse` sends the 101 response.

**Key constraint for this codebase:** `dispatch_to_worker_pool` receives a raw `Request<Body>`. The cleanest integration is to detect the `Upgrade: websocket` header at the TOP of `dispatch_to_worker_pool`, then manually extract `WebSocketUpgrade` from the request parts:

```rust
// Source: axum 0.8 — FromRequestParts trait
// In dispatch_to_worker_pool(), before URL parsing:
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::FromRequestParts;

if request.headers()
    .get("upgrade")
    .and_then(|v| v.to_str().ok())
    .map(|v| v.eq_ignore_ascii_case("websocket"))
    .unwrap_or(false)
{
    return handle_ws_upgrade(state, request).await;
}
// ... rest of HTTP handling
```

The `handle_ws_upgrade` function uses the axum `WebSocketUpgrade` type properly.

[ASSUMED] — the exact way to extract WebSocketUpgrade from a raw Request inside a non-axum-extractor function needs verification against axum 0.8 docs. Alternative: add a separate route in `create_app_with_shutdown()` with the `WebSocketUpgrade` extractor in the function signature, placed BEFORE the catch-all `/{*path}` route.

### Pattern 2: tokio-tungstenite Message Variants

```rust
// Source: tungstenite 0.24 (transitive dep) — verified in cargo tree
use tungstenite::Message;

match msg {
    Message::Text(s) => {
        // JS: evt.data = string value
    }
    Message::Binary(b) => {
        // JS: evt.data = ArrayBuffer
        // Reuse existing ArrayBuffer handling from runtime/fetch.rs:389
    }
    Message::Close(frame) => {
        // JS: close event with code/reason
        // frame: Option<CloseFrame<'static>>
        // code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(1000)
        // reason = frame.as_ref().map(|f| f.reason.as_str()).unwrap_or("")
    }
    Message::Ping(_) | Message::Pong(_) => {
        // D-15b: transparent — tungstenite handles pong reply automatically
        // never dispatched to JS
    }
}
```

[VERIFIED: codebase] — `tokio-tungstenite = "0.24"` confirmed in Cargo.toml line 83.

### Pattern 3: V8 FunctionCallback Capturing Rust State (Outbound Channel)

The existing codebase does NOT use V8 external data slots for callbacks. The approach for stateful WS callbacks that need to send to `outbound_tx` is thread-local storage on the worker thread:

```rust
// In worker thread, set before entering 'ws_messages loop:
thread_local! {
    static WS_OUTBOUND: RefCell<Option<std::sync::mpsc::SyncSender<tungstenite::Message>>>
        = RefCell::new(None);
    static WS_ACCEPTED: Cell<bool> = Cell::new(false);
}

// Set before 'ws_messages:
WS_OUTBOUND.with(|tx| *tx.borrow_mut() = Some(outbound_tx.clone()));
WS_ACCEPTED.with(|a| a.set(false));

// In send() FunctionCallback (same worker thread):
fn ws_send_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    // D-14b: TypeError if not accepted yet
    let accepted = WS_ACCEPTED.with(|a| a.get());
    if !accepted {
        let msg = v8::String::new(scope, "WebSocket is not accepted").unwrap();
        let err = v8::Exception::type_error(scope, msg);
        scope.throw_exception(err);
        return;
    }
    // build message, send via thread-local
    WS_OUTBOUND.with(|tx| {
        if let Some(tx) = tx.borrow().as_ref() {
            let _ = tx.try_send(/* message */);
        }
    });
}
```

[ASSUMED] — thread-local approach for stateful V8 callbacks is inferred from codebase patterns. The existing fetch() binding uses a different mechanism (it calls block_on on the worker's tokio runtime). Thread-local is the lowest-friction approach.

### Pattern 4: Global<Function> for addEventListener Handlers

The codebase already uses `handler_cache: HashMap<String, v8::Global<v8::Function>>` in the worker loop. The same pattern applies for WS event handlers stored during the fetch() call and used in the `'ws_messages` loop:

```rust
// Thread-local per-connection handler lists:
thread_local! {
    static WS_MESSAGE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
    static WS_CLOSE_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
    static WS_ERROR_HANDLERS: RefCell<Vec<v8::Global<v8::Function>>> = RefCell::new(Vec::new());
}

// addEventListener FunctionCallback adds to the appropriate list:
fn ws_add_event_listener(scope: &mut v8::HandleScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let event_type = args.get(0).to_string(scope).map(|s| s.to_rust_string_lossy(scope)).unwrap_or_default();
    let handler_val = args.get(1);
    if handler_val.is_function() {
        let handler_fn = handler_val.cast::<v8::Function>();
        let global = v8::Global::new(scope, handler_fn);
        match event_type.as_str() {
            "message" => WS_MESSAGE_HANDLERS.with(|h| h.borrow_mut().push(global)),
            "close"   => WS_CLOSE_HANDLERS.with(|h| h.borrow_mut().push(global)),
            "error"   => WS_ERROR_HANDLERS.with(|h| h.borrow_mut().push(global)),
            _ => {}
        }
    }
}
```

All `v8::Global<T>` values live on the worker thread only — they are `!Send`.

### Pattern 5: Worker Loop Extension ('ws_messages loop)

The existing `tenant_pool.rs` worker loop structure:

```
'isolate: loop {
    // create NanoIsolate
    'requests: loop {
        task = task_rx.recv()
        // execute handler
        served += 1
    }
    // recycle isolate
}
```

WS extension adds a third inner loop triggered by `task.ws.is_some()`:

```rust
// Inside 'requests: loop, after receiving task:
if let Some(ws_channels) = task.ws {
    // Mark worker busy (prevents further WS dispatch to this worker)
    ws_busy.fetch_add(1, Ordering::SeqCst);
    
    // Initialize WS thread-locals
    WS_OUTBOUND.with(|tx| *tx.borrow_mut() = Some(ws_channels.outbound_tx));
    WS_ACCEPTED.with(|a| a.set(false));
    // clear handler lists from previous connection
    WS_MESSAGE_HANDLERS.with(|h| h.borrow_mut().clear());
    WS_CLOSE_HANDLERS.with(|h| h.borrow_mut().clear());
    WS_ERROR_HANDLERS.with(|h| h.borrow_mut().clear());
    
    // Set readyState = OPEN (1) on socket V8 object
    
    'ws_messages: loop {
        use std::sync::mpsc::RecvTimeoutError;
        let idle_dur = std::time::Duration::from_millis(ws_idle_timeout_ms);
        
        match ws_channels.inbound_rx.recv_timeout(idle_dur) {
            Ok(tungstenite::Message::Text(_)) | Ok(tungstenite::Message::Binary(_)) => {
                // OOM check
                if let Err(oom) = mon.check(iso_ref) {
                    // send close 1011 via outbound channel
                    break 'ws_messages;
                }
                // Per-message CpuTimeoutGuard
                let _guard = if cpu_time_limit_ms > 0 {
                    Some(CpuTimeoutGuard::new(iso_ref, cpu_time_limit_ms))
                } else { None };
                // call JS message handlers
                // D-10b: served counter NOT incremented (paused during WS)
            }
            Ok(tungstenite::Message::Close(frame)) => {
                // set readyState = CLOSED (3)
                // call JS close handlers
                break 'ws_messages;
            }
            Ok(tungstenite::Message::Ping(_)) | Ok(tungstenite::Message::Pong(_)) => {
                // D-15b: transparent, skip
            }
            Err(RecvTimeoutError::Timeout) => {
                // D-11b: idle shrink — exit worker thread entirely
                break 'isolate;
            }
            Err(RecvTimeoutError::Disconnected) => {
                // D-17b: relay dropped inbound_tx → abnormal close code 1006
                break 'ws_messages;
            }
        }
    }
    
    // Clear WS thread-locals
    WS_OUTBOUND.with(|tx| *tx.borrow_mut() = None);
    ws_busy.fetch_sub(1, Ordering::SeqCst);
    
    // D-10b: full context reset → break 'requests forces isolate recycle
    break 'requests;
}
// else: normal HTTP task handling continues below...
```

[VERIFIED: codebase read] — `std::sync::mpsc::Receiver::recv_timeout()` confirmed available; tenant_pool.rs uses std::sync::mpsc (lines 103-104 verified).

### Pattern 6: Setting V8 Integer Properties (readyState)

```rust
// Store server socket as Global<Object> in a thread-local:
thread_local! {
    static WS_SERVER_SOCKET: RefCell<Option<v8::Global<v8::Object>>> = RefCell::new(None);
}

// Helper to update readyState — called from 'ws_messages loop:
fn set_readystate(ctx_scope: &mut v8::ContextScope<v8::HandleScope>, state: u32) {
    WS_SERVER_SOCKET.with(|g| {
        if let Some(global) = g.borrow().as_ref() {
            let socket = v8::Local::new(ctx_scope, global);
            let key = v8::String::new(ctx_scope, "readyState").unwrap();
            let val = v8::Integer::new_from_unsigned(ctx_scope, state);
            socket.set(ctx_scope, key.into(), val.into());
        }
    });
}
```

Constants: CONNECTING=0, OPEN=1, CLOSING=2, CLOSED=3 (matches browser spec per D-16b).

[ASSUMED] — the exact scope type required for `v8::Object::set()` in rusty_v8 v147 (ContextScope vs HandleScope) should be verified against the existing property set patterns in apis.rs before implementation.

### Anti-Patterns to Avoid

- **Sending V8 Global across threads:** `v8::Global<T>` is `!Send`. Never attempt to move WS event handler Globals across thread boundaries. Use thread-locals on the worker thread exclusively.
- **Forgetting axum `ws` feature:** Without `features = ["ws"]` on axum, `axum::extract::ws` module does not exist. This is the most likely compilation blocker in Wave 0.
- **Multiple concurrent CpuTimeoutGuard instances:** CpuTimeoutGuard uses global `AtomicPtr`/`AtomicBool`. Only one guard should be active per isolate at a time. Identical to HTTP pattern: create guard, execute one JS call, drop guard.
- **Counting WS messages against served:** D-03 says the 10k counter is paused during WS mode. Do NOT increment `served` inside the `'ws_messages` loop.
- **Using tokio mpsc receiver on the worker side:** Worker threads in `tenant_pool.rs` are `std::thread::spawn`. The WS inbound receiver must use `std::sync::mpsc::Receiver` so that `recv_timeout()` is available synchronously.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| WebSocket framing, masking, ping/pong | Custom frame parser | tokio-tungstenite (already in project) | Frame masking is error-prone; standard handles RFC 6455 edge cases |
| HTTP 101 handshake | Raw Sec-WebSocket-Accept header | axum `ws` feature WebSocketUpgrade | Header calculation has subtle requirements |
| Cross-thread async/sync bridge | Custom channel abstraction | std::sync::mpsc (worker side) + tokio task (async side) | Well-understood pattern; Receiver has recv_timeout() for idle shrink |
| JS event dispatching | Custom event system | Vec<Global<Function>> iterated at each event | addEventListener is one-way accumulation; removeEventListener is out of scope |

**Key insight:** The hardest problem is the async/sync boundary. The tokio relay task (async) and the V8 worker thread (sync/blocking) are in different execution models. The chosen design (std mpsc channels bridging them) is exactly right.

---

## Common Pitfalls

### Pitfall 1: axum `ws` Feature Not Enabled

**What goes wrong:** `axum::extract::ws` module does not exist at compile time. Error: "unresolved import `axum::extract::ws`".
**Why it happens:** axum 0.8 gates WebSocket support behind the `ws` feature flag. This project does NOT have it enabled (verified via cargo metadata).
**How to avoid:** Wave 0 task must update `Cargo.toml`: `axum = { version = "0.8", features = ["ws"] }`.
**Warning signs:** First cargo check after implementing imports fails with unresolved module.

### Pitfall 2: tokio mpsc Receiver on the Worker Thread Side

**What goes wrong:** `tokio::sync::mpsc::Receiver` has no `blocking_recv_timeout()`. Implementing idle shrink (D-11b) requires a timed blocking receive. Using `.blocking_recv()` blocks forever; using `.try_recv()` in a spin-loop burns CPU.
**Why it happens:** tokio mpsc is designed for async contexts. Timeout requires `tokio::time::timeout` inside an async block.
**How to avoid:** Use `std::sync::mpsc::Receiver::recv_timeout(Duration)` for `WsChannels.inbound_rx`. The tokio relay task sends via a `std::sync::mpsc::SyncSender`.
**Warning signs:** Shrink-to-zero never triggers; idle workers don't exit.

### Pitfall 3: accept() State Must Survive Across FunctionCallback Calls

**What goes wrong:** D-14b requires TypeError if `send()` is called before `accept()`. The `accepted` boolean must persist on the socket between multiple JS calls.
**Why it happens:** V8 FunctionCallbacks are stateless by default.
**How to avoid:** Store `accepted` state in the thread-local `WS_ACCEPTED: Cell<bool>`. The send() callback reads it on every call. Set to `true` in the accept() callback.
**Warning signs:** send() works before accept() (security regression); or TypeError is always thrown.

### Pitfall 4: CpuTimeoutGuard Global State Conflict

**What goes wrong:** CpuTimeoutGuard uses global `TERMINATION_ISOLATE_PTR: AtomicPtr<v8::Isolate>`. If a second guard is created while the first is active, the pointer is overwritten — the timer thread terminates the wrong (or already-terminated) isolate.
**Why it happens:** The global state design is correct for the HTTP model where exactly one guard exists per request per worker thread.
**How to avoid:** In the WS message loop, follow exactly the same pattern as HTTP: create guard, execute ONE JS handler call (with pump_message_loop for async if needed), drop guard before the next `recv_timeout`. Never nest guards.
**Warning signs:** CPU timeout fires at wrong time; messages stop executing after a timeout.

### Pitfall 5: V8 Global<Function> is !Send

**What goes wrong:** `v8::Global<v8::Function>` cannot be sent across threads. Any attempt to move WS event handler Globals into a shared structure across threads fails to compile.
**Why it happens:** V8 objects are tied to a specific isolate; isolates are `!Send`.
**How to avoid:** All WS event handler Globals must live exclusively in the worker thread that owns the isolate. Use thread-local storage (same thread as V8). The `'ws_messages` loop pattern is correct by construction.
**Warning signs:** Compile errors about `Send` not satisfied on `v8::Global`.

### Pitfall 6: axum WebSocketUpgrade in Catch-All Handler

**What goes wrong:** `dispatch_to_worker_pool` receives `Request<Body>` after the body has been consumed. `WebSocketUpgrade` needs to see the request before body consumption.
**Why it happens:** axum extractors are designed for handler signatures; manual extraction from raw Request may require `request.into_parts()` before body reading.
**How to avoid:** Check the `Upgrade` header BEFORE calling `axum::body::to_bytes()`. If WS upgrade is detected, do NOT read the body — pass the raw request to the upgrade extractor. Branch the function at the top based on the header check.
**Warning signs:** WebSocket handshake fails with 400; body read errors in upgrade path.

### Pitfall 7: TenantPool Drop Must Join WS Worker Threads

**What goes wrong:** WS worker threads outlive the TenantPool if they are not joined. The V8 isolate they own may be destroyed after the V8 platform shuts down.
**Why it happens:** Tokio tasks spawned for relay are independent of the spawning scope; similarly, WS worker threads spawned lazily must be tracked.
**How to avoid:** Store lazily-spawned WS worker thread handles in a `Vec<Option<JoinHandle<()>>>` field on `TenantPool`. The existing `Drop` implementation pattern (joining all thread handles) must be extended to include WS workers.

---

## Code Examples

### WebSocketPair Binding Structure (V8)

```rust
// Source: CF Workers WebSocketPair pattern + existing bind_streams() in stream.rs

fn bind_websocket_pair(scope: &mut v8::PinnedRef<v8::HandleScope<()>>, context: v8::Local<v8::Context>) {
    let global = context.global(scope);
    let mut ctx_scope = v8::ContextScope::new(scope, context);

    // Constructor — returns {0: serverSocket, 1: clientSocket}
    let ws_pair_template = v8::FunctionTemplate::new(&mut ctx_scope, websocket_pair_constructor);
    let ws_pair_ctor = ws_pair_template.get_function(&mut ctx_scope).unwrap();
    let ws_pair_key = v8::String::new(&mut ctx_scope, "WebSocketPair").unwrap();
    global.set(&mut ctx_scope, ws_pair_key.into(), ws_pair_ctor.into());
}

fn websocket_pair_constructor(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let server_socket = create_websocket_object(scope);
    let client_socket = create_websocket_object(scope);

    let pair = v8::Object::new(scope);
    let k0 = v8::String::new(scope, "0").unwrap();
    let k1 = v8::String::new(scope, "1").unwrap();
    pair.set(scope, k0.into(), server_socket.into());
    pair.set(scope, k1.into(), client_socket.into());

    retval.set(pair.into());
}

fn create_websocket_object(scope: &mut v8::HandleScope) -> v8::Local<v8::Object> {
    let socket = v8::Object::new(scope);

    // Methods: accept, send, close, addEventListener
    for (name, cb) in &[
        ("accept",           ws_accept_callback as v8::FunctionCallback),
        ("send",             ws_send_callback),
        ("close",            ws_close_callback),
        ("addEventListener", ws_add_event_listener),
    ] {
        let fn_val = v8::Function::new(scope, *cb).unwrap();
        let key = v8::String::new(scope, name).unwrap();
        socket.set(scope, key.into(), fn_val.into());
    }

    // Properties: readyState = 0 (CONNECTING), binaryType = "arraybuffer"
    let rs_key = v8::String::new(scope, "readyState").unwrap();
    let rs_val = v8::Integer::new_from_unsigned(scope, 0);
    socket.set(scope, rs_key.into(), rs_val.into());

    let bt_key = v8::String::new(scope, "binaryType").unwrap();
    let bt_val = v8::String::new(scope, "arraybuffer").unwrap();
    socket.set(scope, bt_key.into(), bt_val.into());

    socket
}
```

### HandlerTask Extension

```rust
// In src/worker/mod.rs:

// New type — bridge between tokio relay task and V8 worker thread
pub struct WsChannels {
    pub inbound_rx: std::sync::mpsc::Receiver<tungstenite::Message>,
    pub outbound_tx: std::sync::mpsc::SyncSender<tungstenite::Message>,
}

// HandlerTask gains one field:
pub struct HandlerTask {
    pub entrypoint: String,
    pub request: NanoRequest,
    pub response_tx: tokio::sync::oneshot::Sender<anyhow::Result<NanoResponse>>,
    pub hostname: String,
    pub start_time: std::time::Instant,
    pub cpu_time_limit_ms: u32,
    pub request_id: String,
    pub memory_limit_mb: u32,
    pub ws: Option<WsChannels>,  // NEW: Some(_) = WS mode; None = HTTP mode
}
```

### TenantPool WS State Extension

```rust
// In src/worker/tenant_pool.rs:
pub struct TenantPool {
    hostname: String,
    workers: Vec<TenantWorker>,
    next_worker: AtomicU64,
    // NEW fields:
    ws_busy: Arc<std::sync::atomic::AtomicUsize>,
    max_ws_connections: u32,     // floor(worker_count / 2) by default
    ws_idle_timeout_ms: u64,     // 30_000 ms default
    #[allow(dead_code)]
    vfs_backend: VfsBackendEnum,
    #[allow(dead_code)]
    control_plane: Option<ControlPlane>,
}

// New method on TenantPool:
pub fn try_claim_ws_connection(&self) -> Option</* channel pair */> {
    let current = self.ws_busy.load(Ordering::SeqCst);
    if current >= self.max_ws_connections as usize {
        return None; // at limit — 503
    }
    // ws_busy is incremented by the WORKER after receiving the WS task,
    // not here — avoids TOCTOU with AtomicUsize compare-and-swap.
    // (Simple approach: rely on max_ws_connections as a soft limit;
    //  exact enforcement is in the worker task dispatch path.)
    Some(/* WsChannels */)
}
```

### AppLimits Extension

```rust
// In src/config/app.rs — AppLimits:
pub struct AppLimits {
    pub memory_mb: u32,
    pub timeout_secs: u32,
    pub workers: u32,
    pub cpu_time_ms: u32,
    pub cpu_time_enabled: bool,
    // NEW:
    /// Maximum concurrent WebSocket connections (default: floor(workers / 2))
    #[serde(default)]
    pub max_ws_connections: Option<u32>,
    /// Idle timeout for WS workers in ms before shrink-to-zero (default: 30000)
    #[serde(default)]
    pub ws_idle_timeout_ms: Option<u64>,
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| axum 0.7 `ws::WebSocketUpgrade` | axum 0.8 `ws::WebSocketUpgrade` | axum 0.8 release | API same; feature flag name unchanged (`ws`) |
| tungstenite `CloseFrame<'_>` (borrowed) | tungstenite 0.24 `CloseFrame<'static>` | 0.23+ | Close frame reason is now `String` not `&str`; no lifetime issues in 0.24 |

**Deprecated/outdated:**
- `axum::TypedHeader` for Upgrade detection: deprecated in axum 0.8. Use `headers.get("upgrade")` directly or the `WebSocketUpgrade` extractor.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | axum 0.8 `WebSocketUpgrade` can be used inside `dispatch_to_worker_pool` by branching at top before body read | Pattern 1, Pitfall 6 | May require a separate dedicated WS route added to `create_app_with_shutdown()` BEFORE the catch-all |
| A2 | Thread-local storage is the correct approach for capturing outbound_tx in V8 FunctionCallback | Pattern 3 | If wrong, alternative is V8 object internal field or external data pointer (set_slot) — more complex but valid |
| A3 | std::sync::mpsc (not tokio::sync::mpsc) for WsChannels.inbound_rx | Pitfall 2 | If tokio mpsc is used, idle shrink requires block_on(timeout(rx.recv())) — more complex but feasible |
| A4 | Thread-local `Cell<bool>` for accepted state is sufficient for accept() guard | Pattern 3, Pitfall 3 | Correct by construction since each worker thread has its own thread-locals |
| A5 | tungstenite 0.24 CloseFrame.code is `CloseCode` type (not raw u16) | Code Examples | Planner must check tungstenite docs for exact type; `u16::from(close_code)` converts it |

---

## Open Questions

1. **axum WebSocketUpgrade integration point in dispatch_to_worker_pool**
   - What we know: The function receives `Request<Body>`; WebSocketUpgrade is an extractor. Body must NOT be consumed before upgrade extraction.
   - What's unclear: Whether `WebSocketUpgrade::from_request_parts(&mut req.parts, &state)` is callable inside an arbitrary async fn or only in axum handler signatures.
   - Recommendation: Planner should add a separate async fn `ws_upgrade_handler` with `WebSocketUpgrade` in its signature, registered as a separate route in `create_app_with_shutdown()` BEFORE `/{*path}`. This is cleaner than manual extraction.

2. **WsChannels channel type: std::sync::mpsc vs tokio::sync::mpsc**
   - What we know: Worker thread uses std::sync::mpsc for task_rx. It creates its own tokio::runtime for fetch(). Idle shrink requires recv_timeout.
   - What's unclear: Whether using the worker's internal tokio runtime to do `block_on(timeout(tokio_rx.recv()))` is safe alongside concurrent fetch() calls.
   - Recommendation: Use std::sync::mpsc for both inbound and outbound channels. Relay task uses std::sync::mpsc::SyncSender. Avoids any tokio runtime interaction complexity.

3. **WS worker claiming: dedicated WS worker threads vs reuse of HTTP workers**
   - What we know: D-13b says "loop-back on task_rx + AtomicUsize ws_busy counter" — implying HTTP workers receive WS tasks through the same channel.
   - What's unclear: The CONTEXT.md also mentions "lazy pool" and "shrink-to-zero" for WS workers, implying WS workers may be separate from HTTP workers.
   - Recommendation: Planner should clarify. The simplest implementation is same-channel (HTTP worker receives task.ws.is_some() task and enters WS mode). This satisfies D-06b (HandlerTask extended with ws field) and D-13b.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| axum `ws` feature | WebSocket upgrade extractor | NOT ENABLED | 0.8.9 (feature present) | Enable in Cargo.toml — no alternative |
| tokio-tungstenite | WebSocket framing | Available | 0.24.0 | — |
| tungstenite | Message types | Available | 0.24.0 (transitive dep) | — |
| std::sync::mpsc | Sync channel for worker side | Available | std | — |
| tokio::sync::mpsc or std::sync::mpsc | Relay task outbound | Available | tokio 1.52 / std | Either works |

**Missing dependencies with no fallback:**
- axum `ws` feature — must be enabled before any WS code compiles. This is a blocking Wave 0 item.

**Missing dependencies with fallback:**
- None.

---

## Security Domain

> `security_enforcement` not set in config.json — treated as enabled.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | WS upgrades inherit HTTP auth from request headers |
| V3 Session Management | yes | Pin-a-worker model; connection drops on worker exit |
| V4 Access Control | no | Tenant isolation is structural (per-hostname TenantPool) |
| V5 Input Validation | yes | 32 MiB message size limit enforced before frame enters channel (D-12b) |
| V6 Cryptography | no | WSS/TLS handled at reverse proxy layer (out of scope) |

### Known Threat Patterns for WebSocket + V8 Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Oversized WebSocket frame (message bomb) | DoS | 32 MiB limit on relay side before frame enters mpsc channel; close 1009 |
| Worker starvation via WS connection flooding | DoS | max_ws_connections enforced at TenantPool; 503 at limit |
| Dynamic code generation in WS message handler | Elevation | `set_allow_generation_from_strings(false)` already set at Context::new() in tenant_pool.rs line 184 |
| CPU-bound message handler (infinite loop) | DoS | Per-message CpuTimeoutGuard (D-09b) terminates V8 on timeout |
| send() before accept() bypass | Spoofing | D-14b: TypeError thrown if send() called before accept(); enforced in FunctionCallback |

---

## Sources

### Primary (HIGH confidence)
- Direct codebase read: `src/worker/tenant_pool.rs` — worker loop structure, std::sync::mpsc usage (lines 103-104), OOM monitor integration, `'isolate`/`'requests` loop pattern
- Direct codebase read: `src/worker/mod.rs` — HandlerTask struct definition (no existing ws field confirmed)
- Direct codebase read: `src/data_plane.rs` — CpuTimeoutGuard implementation, global AtomicPtr pattern, cancel_terminate_execution in Drop
- Direct codebase read: `Cargo.toml` — tokio-tungstenite = "0.24" line 83
- `cargo metadata --format-version 1` — axum 0.8.9 `ws` feature available, NOT enabled in resolved deps
- Direct codebase read: `src/runtime/apis.rs` — RuntimeAPIs::bind_all() signature, bind_streams() pattern
- Direct codebase read: `src/http/router.rs` — dispatch_to_worker_pool() full implementation
- Direct codebase read: `src/config/app.rs` — AppLimits struct fields
- Direct codebase read: `src/worker/tenant_pool.rs` lines 165-200 — `'isolate`/`'requests` loops, context::set_allow_generation_from_strings(false) at line 184

### Secondary (MEDIUM confidence)
- Direct codebase read: `src/runtime/stream.rs` — WritableStream binding pattern as structural reference for WebSocketPair V8 binding
- Direct codebase read: `src/worker/pool.rs` lines 1100-1128 — pump_message_loop pattern for Promise resolution (reuse for async WS handlers)
- Direct codebase read: `src/http/server.rs` — create_app_with_shutdown() route registration for WS route insertion point

### Tertiary (LOW confidence, marked ASSUMED)
- A1: axum WebSocketUpgrade extraction in arbitrary async fn (not confirmed vs axum 0.8 docs)
- A2: Thread-local as preferred pattern for outbound_tx capture in V8 FunctionCallback
- A3: std::sync::mpsc preferred over tokio::sync::mpsc for WsChannels

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all deps verified via cargo metadata and Cargo.toml direct read
- Architecture: HIGH — based on direct codebase analysis of existing patterns
- Pitfalls: HIGH for items derived from code; MEDIUM for V8 binding specifics
- axum ws integration point: MEDIUM — feature availability confirmed, exact extraction API [ASSUMED]

**Research date:** 2026-05-17
**Valid until:** 2026-06-17 (stable crates — 30 day window)
