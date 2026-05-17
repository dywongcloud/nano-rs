# Phase 23: WebSocket Server - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-16
**Phase:** 23-WebSocket-Server
**Areas discussed:** Connection lifecycle model, JS API surface, Half-open on recycle, Connection limits

---

## Connection Lifecycle Model

| Option | Description | Selected |
|--------|-------------|----------|
| Pin-a-worker | One WS connection = one dedicated worker thread. JS context stays warm, messages sequential. Worker recycles when connection closes. | ✓ |
| Bridge/relay (async) | Async task holds WS socket, dispatches each message as a separate HandlerTask. Requires affinity routing. | |

**User's choice:** Pin-a-worker  
**Notes:** Fits TenantPool model exactly. No cross-thread shared mutable state. Worker enters WS message loop for connection duration. Prior discussion context: Cloudflare Workers WebSocket docs reviewed — this matches their non-hibernation model.

---

## JavaScript API Surface

| Option | Description | Selected |
|--------|-------------|----------|
| CF WebSocketPair | `new WebSocketPair()` → [client, server]. `server.accept()`, return client in 101 Response. CF-compatible. | ✓ |
| Event-based global | `__nano_ws` global injected per-connection. Simpler but not CF-compatible. | |

**User's choice:** CF WebSocketPair  
**Notes:** Full CF compatibility is the goal. `Object.values(new WebSocketPair())` destructuring, `server.accept()`, `new Response(null, { status: 101, webSocket: client })` pattern.

---

## Half-Open on Recycle

| Option | Description | Selected |
|--------|-------------|----------|
| Terminate connection | Send WS close 1001 (Going Away) when isolate recycles. Client must reconnect. | |
| Drain then recycle | Active WS connection extends isolate past request limit. Worker excluded from pool. Recycle after close. | ✓ |

**User's choice:** Drain then recycle  
**Notes:** WS messages do not increment the request counter. Isolate recycles only after the connection closes cleanly. OOM is an exception — send close 1011 (Internal Error) then recycle immediately.

---

## Connection Limits

| Option | Description | Selected |
|--------|-------------|----------|
| Per-worker (1 WS per worker) | Natural limit — concurrent WS = worker_count. No config. | |
| Per-tenant configurable | `max_ws_connections` in AppConfig. Enforced in TenantPool before upgrade. | ✓ |

**User's choice:** Per-tenant configurable  
**Notes:** Default value = `worker_count` (natural limit). Operators can set lower to reserve HTTP capacity.

---

## Claude's Discretion

- Wire protocol: use `tokio-tungstenite` (already in Cargo.toml)
- axum `ws::WebSocketUpgrade` extractor (cleaner than raw tungstenite)
- Internal channel: `tokio::sync::mpsc` unbounded per-connection
- Thread join: existing TenantWorker join handle pattern

## Deferred Ideas

- WebSocket Hibernation API (CF Durable Objects suspend-between-messages pattern)
- Outbound WebSocket client from JS handlers
- Multi-client broadcast (needs inter-isolate messaging — Phase 26)
- permessage-deflate compression (Phase 25)
- WSS/TLS (reverse proxy concern)
