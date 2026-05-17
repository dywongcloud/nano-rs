# Phase 23: WebSocket Server - Context

**Gathered:** 2026-05-16
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement server-side WebSocket upgrade handling for nano-rs, using the
Cloudflare Workers WebSocketPair API as the JS surface. A connecting HTTP
client that sends `Upgrade: websocket` gets routed to a dedicated worker
thread (pin-a-worker model). That worker enters a message loop, calling the
JS handler for each incoming message, and exits the loop when the connection
closes. The isolate serving a WS connection is excluded from request routing
for the connection's duration and drains fully before recycling.

**In scope:**
- HTTP 101 upgrade response via axum extractor
- `WebSocketPair` JS global: `new WebSocketPair()`, `.accept()`, `.send()`, `.close()`
- `addEventListener('message' | 'close' | 'error')` on WS objects
- Pin-a-worker lifecycle: TenantPool marks one worker "WS mode" for duration
- Drain-then-recycle on isolate limit: active WS connection extends isolate past 10k req limit
- Per-tenant `max_ws_connections` config field with enforcement in TenantPool
- Message size limit: 32 MiB (matching CF Workers), disconnect with code 1009 on excess
- `readyState` property: CONNECTING(0), OPEN(1), CLOSING(2), CLOSED(3)
- `binaryType` property: "arraybuffer" default (consistent with existing ArrayBuffer support)

**Out of scope (future phases):**
- WebSocket Hibernation API (CF Durable Objects pattern — Phase 24+)
- WSS/TLS termination (handled at reverse proxy layer)
- Multi-client broadcast / pub-sub (requires inter-isolate messaging — Phase 26)
- WebSocket client from within JS (outbound WS connections from handler)
- Compression (permessage-deflate) — Phase 25

</domain>

<decisions>
## Implementation Decisions

### Connection Lifecycle Model
- **D-01: Pin-a-worker.** One WebSocket connection = one dedicated TenantPool worker thread for the connection's full duration. The worker enters a WS message loop instead of the normal request loop. No shared state between WS connection and other workers.
- **D-02: Worker state during WS.** Worker is marked `WsActive` (new enum variant on worker state). TenantPool skips WsActive workers during normal request dispatch. Worker returns to `Available` after connection closes.
- **D-03: Drain then recycle on isolate limit.** If the isolate hits MAX_REQUESTS_PER_ISOLATE while serving a WS connection, it continues serving the connection (does NOT terminate). Recycle happens after the WS connection closes cleanly. The 10k counter is paused during WS mode (WS messages don't increment it).

### JavaScript API
- **D-04: CF WebSocketPair pattern.** Implement `WebSocketPair` as a V8 global. `new WebSocketPair()` returns an object with keys `0` and `1` (two linked sockets). JS destructures: `const [client, server] = Object.values(new WebSocketPair())`. Handler calls `server.accept()`, returns `new Response(null, { status: 101, webSocket: client })`.
- **D-05: Response carries the socket.** The `Response` constructor must accept a `webSocket` option. The axum handler detects this property on the returned NanoResponse and performs the actual HTTP upgrade.
- **D-06: Events on server socket.** `server.addEventListener('message', (evt) => ...)` — `evt.data` is string or ArrayBuffer. `server.addEventListener('close', (evt) => ...)` — `evt.code` and `evt.reason`. `server.addEventListener('error', ...)`.

### Connection Limits
- **D-07: Per-tenant configurable.** Add `max_ws_connections: Option<u32>` to `AppConfig` (default: equal to `worker_count` since each WS pins one worker). TenantPool enforces this before upgrading — returns 503 if at limit.
- **D-08: Natural backpressure.** Since WS pins a worker, normal HTTP requests queue behind WS if all workers are in WS mode. This is acceptable — operators configure `max_ws_connections` to leave headroom for HTTP.

### Message Handling
- **D-09: 32 MiB message limit.** Incoming messages > 32 MiB trigger WS close frame with code 1009 (Message Too Big). Matches CF Workers limit.
- **D-10: Sequential per-connection.** Messages on one connection processed one at a time (worker is single-threaded). No message parallelism within a connection — correct by construction.
- **D-11: Async handlers.** If the JS fetch handler returns a Promise for the 101 response (or message handler returns a Promise), resolve it using the existing pump_message_loop pattern before sending/continuing.

### Half-Open Behavior
- **D-12: Connection terminates with worker.** If the worker/thread panics or the isolate crashes mid-connection, the WS socket is dropped. tokio-tungstenite will send a close frame on drop. Client must reconnect.
- **D-13: Graceful OOM.** If OOM is detected during WS mode, send close frame code 1011 (Internal Error) before recycling the isolate.

### Claude's Discretion
- Wire protocol framing: use `tokio-tungstenite` (already in Cargo.toml)
- axum WebSocket extractor vs raw tungstenite upgrade — use axum's `ws::WebSocketUpgrade` for cleaner integration with existing axum routing
- Internal channel type for routing WS frames to/from JS: `std::sync::mpsc` (SyncSender/Receiver — REQUIRED for `recv_timeout()` on blocking worker threads; `tokio::sync::mpsc` has no `recv_timeout()` so idle shrink-to-zero is impossible with it; this supersedes the initial tokio mpsc choice)
- Thread join strategy when WS worker exits: existing TenantWorker join handle pattern

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Architecture
- `src/worker/tenant_pool.rs` — TenantPool and TenantWorker implementation; WS worker state must extend this
- `src/worker/mod.rs` — HandlerTask struct; WS needs a parallel WsTask or extension
- `src/worker/pool.rs` — WorkerPool; same pattern as TenantPool for the standard pool path
- `src/http/server.rs` — axum server setup; WebSocket upgrade route hooks in here
- `src/http/router.rs` — request dispatch; upgrade detection before HandlerTask dispatch

### JS Runtime Bindings Pattern
- `src/runtime/apis.rs` — `RuntimeAPIs::bind_all()` — add `bind_websocket_pair()` here
- `src/runtime/stream.rs` — reference for how WritableStream JS class is bound in V8
- `src/runtime/fetch.rs` — reference for ArrayBuffer data handling (binaryType support)

### Config Schema
- `src/http/config.rs` — AppConfig struct; add `max_ws_connections: Option<u32>` here

### External Reference
- CF Workers WebSocket API: https://developers.cloudflare.com/workers/runtime-apis/websockets/
  Key pattern: WebSocketPair, accept(), Response with webSocket property, 32 MiB limit

### Existing Dependency
- `Cargo.toml:83` — `tokio-tungstenite = "0.24"` already present, unused

No external ADRs — requirements fully captured in decisions above.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `tokio-tungstenite = "0.24"`: dependency already declared, zero setup cost
- `pump_message_loop` pattern in `pool.rs:1060-1065` and `tenant_pool.rs`: reuse for resolving Promises returned from WS message handlers
- `OomMonitor::check()` in TenantPool `'requests` loop: extend to WS message loop with 1011 close on OOM
- `CpuTimeoutGuard`: reuse per-message if cpu_time_limit_ms configured (terminate execution on single slow message)
- `ArrayBuffer` handling in `runtime/fetch.rs:389` and `apis.rs:763`: reuse for `binaryType = "arraybuffer"` message delivery

### Established Patterns
- Worker thread lifecycle: `'isolate` outer loop (create isolate), `'requests` inner loop (serve requests). WS adds a third inner loop `'ws_messages` that replaces `'requests` while connection is active.
- Handler caching: `handler_cache: HashMap<String, Global<Function>>` — reuse same cache for WS connections (same entrypoint, same handler function)
- MPSC task dispatch: `task_tx: mpsc::Sender<HandlerTask>` per worker — add parallel `ws_tx: mpsc::Sender<WsTask>` or multiplex via enum wrapper
- Error propagation back to caller: `oneshot::Sender<Result<NanoResponse>>` — for WS, the 101 response is the "first response"; subsequent messages use a different channel

### Integration Points
- `src/http/server.rs` — add `GET /{*path}` route with `axum::extract::ws::WebSocketUpgrade` extractor, checked after verifying the Upgrade header
- `src/http/router.rs` — detect `Upgrade: websocket` header before creating HandlerTask; route to TenantPool's WS upgrade path instead
- `src/http/config.rs` — `max_ws_connections` field added to `AppConfig` / per-hostname config
- `TenantPool::add_ws_connection()` — new method that claims a worker, transitions it to WS mode, returns channel pair for frame I/O

</code_context>

<specifics>
## Specific Requirements

- CF Workers WebSocketPair API exactly: `new WebSocketPair()` → object with keys `0` and `1`
- `Object.values(new WebSocketPair())` destructuring must work (values in insertion order)
- `server.accept()` required before send/receive — matches CF contract
- `new Response(null, { status: 101, webSocket: client })` — Response constructor must accept webSocket option
- 32 MiB message limit, disconnect with code 1009 (matches CF)
- `readyState` values match browser spec: 0/1/2/3
- WS connections count against `worker_count` (no over-provisioning by default)
- Drain-then-recycle: WS connection extends isolate life; request counter paused during WS mode

</specifics>

<deferred>
## Deferred Ideas

- **WebSocket Hibernation API** — CF Durable Objects pattern where worker suspends between messages. Requires fundamentally different lifecycle (worker released to pool between messages, state serialized). Phase 24+ if needed.
- **Outbound WebSocket from JS** — `new WebSocket('ws://...')` from within a handler (WS client). Different from server-side. Phase after server WS is stable.
- **Multi-client broadcast** — coordinating messages across multiple WS connections. Requires inter-isolate messaging (Phase 26).
- **permessage-deflate compression** — Phase 25 (CompressionStream phase).
- **WSS/TLS** — handled at reverse proxy (nginx/caddy) layer; not a nano concern.

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 23-WebSocket-Server*
*Context gathered: 2026-05-16*
