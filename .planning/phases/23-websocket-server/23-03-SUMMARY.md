---
phase: 23-websocket-server
plan: 03
subsystem: api
tags: [websocket, axum, tungstenite, relay, channels, tokio]

requires:
  - phase: 23-01
    provides: WsChannels and HandlerTask.ws field in src/worker/mod.rs
  - phase: 23-02
    provides: TenantPool::dispatch_ws and ws_busy/max_ws_connections in tenant_pool.rs

provides:
  - Upgrade:websocket header detection before body consumption in dispatch_to_worker_pool
  - handle_ws_upgrade: WebSocketUpgrade extractor + sync_channel pair + HandlerTask dispatch
  - ws_relay_task: tokio::select! loop bridging async axum WebSocket to std::sync::mpsc channels
  - 32 MiB inbound message limit with close 1009
  - axum_to_tungstenite and tungstenite_to_axum conversion helpers

affects: [23-websocket-server, http-router, worker-dispatch]

tech-stack:
  added:
    - futures-util = "0.3" (direct dependency, previously transitive via axum)
  patterns:
    - WS upgrade detection before body consumption via Upgrade header check
    - Request split into parts (into_parts) before WebSocketUpgrade::from_request_parts
    - spawn_blocking bridge for blocking std::sync::mpsc::Receiver in async context
    - tokio::select! dual-direction relay for async WS stream and sync channel

key-files:
  created: []
  modified:
    - src/http/router.rs
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "WS detection via Upgrade header before body consumption — checked at top of dispatch_to_worker_pool to avoid consuming the body before branching"
  - "WebSocketUpgrade::from_request_parts used after splitting request into parts — avoids consuming the request before capturing method/uri/headers"
  - "Dispatch WS tasks via WorkQueue.dispatch() (same as HTTP) since AppState only exposes WorkQueue, not TenantPool directly — ws field on HandlerTask distinguishes WS from HTTP"
  - "spawn_blocking bridges blocking outbound_rx.recv() to tokio channel — avoids blocking the async executor"
  - "Ping/Pong skipped in axum_to_tungstenite — axum handles pong replies automatically per D-15b"

patterns-established:
  - "Pattern: WS upgrade check at top of dispatch handler before any body reads"
  - "Pattern: Request.into_parts() + FromRequestParts for WS extraction while preserving metadata"
  - "Pattern: spawn_blocking + tokio::sync::mpsc as bridge for blocking Receiver in async context"

requirements-completed: [WS-01]

duration: 12min
completed: 2026-05-17
---

# Phase 23 Plan 03: HTTP Router WS Upgrade Detection, Relay Task, and Dispatch Summary

**Upgrade:websocket detection added to dispatch_to_worker_pool with axum WebSocketUpgrade extractor, sync_channel(128) bridging, 32 MiB inbound limit, and tokio::select! relay connecting async WS frames to std::sync::mpsc channels for worker threads.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-05-17T12:00:00Z
- **Completed:** 2026-05-17T12:12:00Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments

- WebSocket upgrade detection inserted before body consumption in `dispatch_to_worker_pool` — prevents axum from consuming the request body before the WS upgrade path
- `handle_ws_upgrade` function: splits request into parts, calls `WebSocketUpgrade::from_request_parts`, builds `WsChannels` with `sync_channel(128)` pairs, constructs `HandlerTask` with `ws: Some(WsChannels)`, dispatches via `WorkQueue`, returns 503 on queue full
- `ws_relay_task`: `tokio::select!` loop with inbound (client → worker via `inbound_tx`) and outbound (worker → client via `outbound_notify_rx`) paths, 32 MiB payload limit enforced with close code 1009, `spawn_blocking` bridges blocking `outbound_rx` to async channel
- Conversion helpers `axum_to_tungstenite` and `tungstenite_to_axum` handle the tungstenite 0.24 (direct dep) ↔ axum `Message` translation without touching axum's private `into_tungstenite()` methods

## Task Commits

1. **Task 1: WS upgrade detection, handle_ws_upgrade, ws_relay_task** - `48c68691` (feat)

## Files Created/Modified

- `src/http/router.rs` - Added WS detection branch, handle_ws_upgrade, ws_relay_task, conversion helpers; removed unused AtomicUsize/Ordering/TungsteniteMessage/StreamExt/SinkExt imports
- `Cargo.toml` - Added `futures-util = "0.3"` as direct dependency
- `Cargo.lock` - Updated lockfile

## Decisions Made

- Dispatched WS HandlerTask via `WorkQueue::dispatch()` (same mechanism as HTTP) rather than `TenantPool::dispatch_ws()` because `AppState` exposes only `WorkQueue`. The `ws: Some(WsChannels)` field in `HandlerTask` distinguishes WS tasks from HTTP tasks for workers. `TenantPool::dispatch_ws()` can be wired in a future plan when `AppState` grows a tenant registry.
- Used `Request::into_parts()` + `WebSocketUpgrade::from_request_parts` instead of `WebSocketUpgrade::from_request` because we need to capture `method`/`uri`/`headers` before the extractor consumes the parts.
- The `_response_rx` oneshot receiver is intentionally dropped immediately for WS connections — WS workers respond via the outbound channel, not the oneshot.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Removed unused imports to achieve zero warnings**
- **Found during:** Task 1 (post-implementation cargo check)
- **Issue:** Unstaged imports from previous attempt (AtomicUsize, Ordering, TungsteniteMessage, StreamExt, SinkExt) were unused after the final implementation — cargo check produced 4 warnings
- **Fix:** Removed the 5 unused imports from the use statements at the top of router.rs
- **Files modified:** src/http/router.rs
- **Verification:** `cargo check --lib` passes with 0 warnings, 0 errors
- **Committed in:** 48c68691 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical — unused import cleanup)
**Impact on plan:** Zero scope creep; cleanup was required for a clean build.

## Issues Encountered

- **Tungstenite version split**: `tungstenite = "0.24"` is the direct dep (used by `WsChannels`), but axum 0.8 uses `tungstenite = "0.29"` internally. Axum's `Message::into_tungstenite()` / `Message::from_tungstenite()` are private, so conversion is done manually via `.as_str()` / `bytes::Bytes::from(vec)` / `CloseFrame` field-by-field conversion. Both versions have identical structure for Text/Binary/Close/Ping/Pong.

## Known Stubs

- `_response_rx` oneshot is created and immediately dropped for WS HandlerTasks. Workers that attempt to send via `response_tx` will get a send error. This is intentional for Plan 03 — WS worker response handling via the oneshot path is a no-op until Plan 04 wires proper WS worker execution. The `inbound_tx`/`outbound_rx` channels are the actual data path.

## Next Phase Readiness

- HTTP entry point for WS connections is complete: 101 handshake, frame bridging, backpressure
- Plan 04 can wire WS worker execution using `WsChannels` from `HandlerTask.ws`
- Connection limit (503 at queue full) and 32 MiB frame limit (close 1009) are both enforced

---
*Phase: 23-websocket-server*
*Completed: 2026-05-17*
