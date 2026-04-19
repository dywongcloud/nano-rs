# Architecture Patterns

**Domain:** V8-based Edge JavaScript Runtime
**Researched:** 2026-04-19

## Overview

NANO is a single-process HTTP server hosting multiple isolated JavaScript applications in separate V8 isolates. Each application runs in its own isolate with strong isolation boundaries, context reset between requests, and WinterCG-compliant APIs. This architecture eliminates container overhead while maintaining security and performance.

## Recommended Architecture

### Core Architectural Pattern: WorkerPool + Isolate-per-Thread

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           HTTP Server (hyper/axum)                         │
│                     Virtual Host Router (Host header → app)                 │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ Request Dispatch
┌─────────────────────────────────────────────────────────────────────────────┐
│                         WorkQueue (tokio mpsc channels)                      │
│                        Per-app: (Request → Response) channel               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
                    ▼               ▼               ▼
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│ Worker  │    │ Worker  │    │ Worker  │    │ Worker  │
│ Thread  │    │ Thread  │    │ Thread  │    │ Thread  │
│ (App A) │    │ (App A) │    │ (App B) │    │ (App B) │
└────┬────┘    └────┬────┘    └────┬────┘    └────┬────┘
     │              │              │              │
     ▼              ▼              ▼              ▼
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│ V8      │    │ V8      │    │ V8      │    │ V8      │
│ Isolate │    │ Isolate │    │ Isolate │    │ Isolate │
│ (App A) │    │ (App A) │    │ (App B) │    │ (App B) │
└─────────┘    └─────────┘    └─────────┘    └─────────┘
     │              │              │              │
     └──────────────┴──────────────┴──────────────┘
                            │
                    ┌───────┴───────┐
                    │               │
                    ▼               ▼
            ┌──────────┐    ┌──────────┐
            │ Snapshots│    │ Platform │
            │ (cached) │    │ (V8 init)│
            └──────────┘    └──────────┘
```

## Component Boundaries

### 1. HTTP Server Layer

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| HTTP Server (hyper/axum) | Accept connections, parse requests, return responses | Virtual Host Router |
| Virtual Host Router | Map Host header → AppConfig | HTTP Server, WorkQueue |

**Data Flow:**
```
TCP Connection → HTTP Request → Host Header → App Lookup → WorkQueue Enqueue
```

**Key Design:**
- Stateless HTTP handling - all state lives in isolates
- Host header is routing key (SNI-style multi-tenancy)
- Keep-alive connections managed at TCP level

### 2. WorkQueue Layer

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| WorkQueue Manager | Route requests to appropriate worker pools | Virtual Host Router, WorkerPools |
| App WorkQueue | Per-app queue of pending requests | WorkQueue Manager, Worker Threads |

**Data Flow:**
```
HTTP Request → App WorkQueue → Worker Thread Pickup → V8 Context
```

**Key Design:**
- One WorkQueue per app (isolation at queue level)
- Backpressure via bounded channels
- Load balancing: round-robin or least-loaded worker selection

### 3. WorkerPool Layer

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| WorkerPool Manager | Spawn/manage worker threads per app | WorkQueue, Worker Threads |
| Worker Thread | Run V8 isolate, execute JS, return response | WorkerPool Manager, V8 Isolate |

**Data Flow:**
```
WorkQueue Pop → JS Handler Call → fetch() API Response → Serialize → HTTP Response
```

**Key Design:**
- One WorkerPool per app (configurable N workers)
- Worker thread owns exactly one V8 isolate
- Context reset between requests (5ms overhead vs 50-100ms isolate recreation)

### 4. V8 Isolate Layer

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| V8 Isolate | Sandboxed JS execution environment | Worker Thread, Extensions |
| Context | Per-request fresh V8 context | V8 Isolate |
| Extensions | WinterCG API bindings | V8 Isolate, Rust Ops |

**Data Flow:**
```
V8 Context → Load Handler Script → Create Request/Response → Execute fetch() → Capture Response
```

**Key Design:**
- Isolate = security boundary (separate heap, no shared state)
- Context = request boundary (fresh globals, no request bleed)
- Snapshots = fast startup (pre-compiled builtins)

### 5. Extension/Ops Layer

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| Extension Loader | Load WinterCG APIs into context | V8 Isolate |
| Ops (Rust functions) | Host functions callable from JS | Extension Loader, System Services |
| Resource Table | Track open resources (streams, sockets) | Ops, Resource Manager |

**Data Flow:**
```
JS: fetch() → Op: op_fetch() → tokio::spawn HTTP request → Promise Resolver → JS continuation
```

**Key Design:**
- Ops are sync or async (async ops return Promises)
- Resource IDs (rids) track external resources
- All I/O goes through tokio, never blocks V8 thread

## Data Flow Architecture

### Request Lifecycle

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│  Client  │────▶│   HTTP   │────▶│  Virtual │────▶│ WorkQueue│────▶│  Worker  │
│  Request │     │  Server  │     │  Host    │     │  (app)   │     │  Thread  │
└──────────┘     └──────────┘     └──────────┘     └──────────┘     └────┬─────┘
                                                                          │
                                                                          ▼
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│  Client  │◀────│   HTTP   │◀────│  Worker  │◀────│   V8     │◀────│  Context │
│ Response │     │  Server  │     │  Thread  │     │  Isolate │     │  (fresh) │
└──────────┘     └──────────┘     └──────────┘     └──────────┘     └──────────┘
                                                                          │
                                                                          ▼
                                                                    ┌──────────┐
                                                                    │  JS App  │
                                                                    │ Handler  │
                                                                    │  fetch() │
                                                                    └──────────┘
```

### Isolation Boundaries

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        PROCESS BOUNDARY (OS-level)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │
│  │   Isolate   │  │   Isolate   │  │   Isolate   │  │   Isolate   │      │
│  │    App A    │  │    App A    │  │    App B    │  │    App C    │      │
│  │   Worker 1  │  │   Worker 2  │  │   Worker 1  │  │   Worker 1  │      │
│  │             │  │             │  │             │  │             │      │
│  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │      │
│  │  │Context│  │  │  │Context│  │  │  │Context│  │  │  │Context│  │      │
│  │  │ Req 1 │  │  │  │ Req 2 │  │  │  │ Req 1 │  │  │  │ Req 1 │  │      │
│  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │      │
│  │             │  │             │  │             │  │             │      │
│  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │      │
│  │  │Context│  │  │  │Context│  │  │  │Context│  │  │  │Context│  │      │
│  │  │ Req 3 │  │  │  │ Req 4 │  │  │  │ Req 2 │  │  │  │ Req 2 │  │      │
│  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │      │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘      │
│                                                                             │
│  Each isolate has:                                                         │
│  - Separate V8 heap (no object sharing between isolates)                    │
│  - Separate context per request (reset = fresh globals)                       │
│  - Separate resource table (no file descriptor leakage)                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Patterns to Follow

### Pattern 1: Isolate-per-Thread with Context Reset

**What:** Each worker thread owns one V8 isolate permanently. Between requests, reset only the context (not the entire isolate).

**Why this works:**
- Isolate creation: 50-100ms (expensive)
- Context reset: ~5ms (cheap)
- Fresh context = no state leakage between requests
- Worker reuse = amortized isolate creation cost

**Implementation:**
```rust
// Pseudo-code for worker thread loop
loop {
    let request = work_queue.recv().await;
    let context = isolate.create_context();  // Fresh context
    
    // Bind WinterCG APIs to context
    bind_fetch_api(&context);
    bind_crypto_api(&context);
    
    // Execute user handler
    let response = execute_handler(&context, request).await;
    
    // Context dropped here - all request state cleaned up
    drop(context);
    
    send_response(response);
}
```

### Pattern 2: Async Op Pattern for I/O

**What:** All I/O operations are async ops that yield to tokio, never block the V8 thread.

**Why this works:**
- V8 is single-threaded within an isolate
- Blocking = wasted CPU, reduced throughput
- Async ops = concurrent I/O handling
- Rust futures integrate cleanly with V8 promises

**Implementation:**
```rust
// Rust side: async op
#[op2(async)]
async fn op_fetch(url: String) -> Result<Response, Error> {
    // tokio::spawn - runs on thread pool, doesn't block V8
    let response = tokio::spawn(async move {
        reqwest::get(&url).await
    }).await??;
    
    Ok(response)
}

// JS side: Promise-based
async function handler(request) {
    const response = await fetch("https://api.example.com");
    return new Response(response.body);
}
```

### Pattern 3: Resource Table Pattern

**What:** Track external resources (streams, sockets, files) via integer handles (rids), similar to file descriptors.

**Why this works:**
- Resources survive context reset (connection pooling)
- Explicit close semantics prevent leaks
- Async read/write through rids

**Implementation:**
```rust
// Resource trait
pub trait Resource: Any {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error>;
    fn close(&mut self);
}

// Resource table per isolate
pub struct ResourceTable {
    resources: HashMap<u32, Box<dyn Resource>>,
    next_rid: u32,
}
```

### Pattern 4: Snapshot-Based Startup

**What:** Pre-compile runtime code (WinterCG APIs) into V8 snapshots for instant context creation.

**Why this works:**
- Snapshot creation: Once at build time
- Context from snapshot: ~1ms vs ~50ms cold start
- APIs pre-loaded, pre-initialized
- Reduced per-request overhead

**Implementation:**
```rust
// Build time: create snapshot
let mut snapshot_creator = SnapshotCreator::new(None);
// ... load all WinterCG APIs ...
let snapshot = snapshot_creator.create_blob(SnapshotCreator::FunctionCodeHandling::Clear);

// Runtime: use snapshot
let params = CreateParams::default().snapshot_blob(&snapshot);
let isolate = Isolate::new(params);
// Context created from snapshot = instant API availability
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Shared State Between Isolates

**What:** Attempting to share V8 objects or heap memory between isolates.

**Why bad:** V8 isolates are completely separate heaps. Sharing causes crashes or undefined behavior.

**Instead:**
- Use message passing for inter-isolate communication
- Serialize data through channels
- Each isolate is self-contained

### Anti-Pattern 2: Isolate Creation Per Request

**What:** Creating a new V8 isolate for every incoming request.

**Why bad:** 50-100ms latency per request, unacceptable for edge workloads.

**Instead:**
- Pool isolates per worker thread
- Context reset for request isolation
- Keep isolates alive across many requests

### Anti-Pattern 3: Synchronous I/O in V8 Thread

**What:** Calling blocking I/O (file read, network request) directly from V8 ops.

**Why bad:** Blocks the V8 thread, prevents concurrent request handling within that isolate.

**Instead:**
- Always use async ops for I/O
- Delegate to tokio thread pool
- Return Promises to JS

### Anti-Pattern 4: Unbounded Resource Growth

**What:** Creating resources (streams, connections) without tracking or limits.

**Why bad:** Memory leaks, file descriptor exhaustion, connection pool overflow.

**Instead:**
- Resource table with explicit rids
- Resource limits per isolate
- Automatic cleanup on context disposal

## Scalability Considerations

| Concern | At 100 users | At 10K users | At 1M users |
|---------|--------------|--------------|-------------|
| **Worker Threads** | Fixed N per app (e.g., 4) | Fixed N per app | Fixed N per app |
| **V8 Isolates** | 1 per worker thread | 1 per worker thread | 1 per worker thread |
| **WorkQueue** | Bounded channel (1000) | Bounded channel (1000) | Bounded channel (1000) |
| **Context Reset** | Per request | Per request | Per request |
| **Resource Limits** | 100 rids per isolate | 1000 rids per isolate | 10000 rids per isolate |
| **Snapshot** | Build-time generated | Build-time generated | Build-time generated |
| **Horizontal Scale** | Single process | Single process | External LB + multiple processes |

**Key Insight:** NANO scales vertically within a process (more apps, more workers), but horizontal scaling requires external load balancing (by design - out of scope).

## Suggested Build Order

Based on component dependencies:

### Phase 1: Foundation
1. **V8 Platform initialization** (rusty_v8 setup)
2. **Single isolate proof-of-concept** (execute simple JS)
3. **HTTP server skeleton** (hyper/axum, no JS yet)

### Phase 2: Core Runtime
4. **WorkerPool scaffolding** (thread spawn, basic loop)
5. **WorkQueue implementation** (tokio channels, dispatch)
6. **Context lifecycle** (create, execute, reset, dispose)

### Phase 3: WinterCG APIs
7. **Request/Response/URL** (fetch interface types)
8. **Headers implementation** (Web-standard headers)
9. **fetch() handler routing** (HTTP → JS function call)
10. **console API** (basic logging)

### Phase 4: I/O and Utilities
11. **TextEncoder/TextDecoder** (encoding utilities)
12. **crypto.getRandomValues** (random bytes)
13. **Outbound fetch()** (HTTP client via tokio)

### Phase 5: Multi-tenancy
14. **Virtual host routing** (Host → App mapping)
15. **Per-app WorkerPools** (isolation between apps)
16. **Context reset optimization** (dispose/recycle)

### Phase 6: Advanced Features
17. **Streams APIs** (Readable/Writable/Transform)
18. **crypto.subtle** (Rust crypto crate integration)
19. **Compression streams** (flate2 integration)
20. **WebSocket server** (RFC 6455)
21. **VFS per isolate** (virtual filesystem)
22. **Inter-isolate messaging** (structured clone)
23. **V8 snapshots** (startup optimization)

**Dependency Graph:**
```
V8 Platform
    ↓
Single Isolate → WorkerPool → WorkQueue → HTTP Server
    ↓                   ↓           ↓
Context Lifecycle ←─────┴───────────┘
    ↓
WinterCG APIs (Request, Response, URL, Headers)
    ↓
fetch() Routing ←─── Outbound fetch() (tokio)
    ↓
Virtual Host Router
    ↓
Multi-tenant Workers
    ↓
Advanced Features (Streams, Crypto, WebSockets, etc.)
```

## Integration Points

### V8 ↔ Rust Integration

**Critical Points:**
1. **Platform initialization:** One-time, process-wide
2. **Isolate creation:** Per worker thread, heavy
3. **Context creation:** Per request, light with snapshots
4. **Op binding:** Startup time, static
5. **Promise resolution:** Event loop tick, ongoing

**Threading Model:**
```
┌─────────────────────────────────────────────────────────────┐
│                     Main Thread (Tokio)                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │   HTTP      │  │  WorkQueue   │  │   Async I/O  │         │
│  │   Server    │  │   Manager    │  │   (outbound) │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
└─────────────────────────────────────────────────────────────┘
                              │
            ┌─────────────────┼─────────────────┐
            │                 │                 │
            ▼                 ▼                 ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ Worker Thread │  │ Worker Thread │  │ Worker Thread │
│   (App A)     │  │   (App A)     │  │   (App B)     │
│               │  │               │  │               │
│ ┌───────────┐ │  │ ┌───────────┐ │  │ ┌───────────┐ │
│ │   V8      │ │  │ │   V8      │ │  │ │   V8      │ │
│ │  Isolate  │ │  │ │  Isolate  │ │  │ │  Isolate  │ │
│ │ (pinned)  │ │  │ │ (pinned)  │ │  │ │ (pinned)  │ │
│ └───────────┘ │  │ └───────────┘ │  │ └───────────┘ │
└───────────────┘  └───────────────┘  └───────────────┘
```

### External Dependencies

| Dependency | Role | Integration Point |
|------------|------|-------------------|
| rusty_v8 | V8 bindings | Core execution |
| tokio | Async runtime | I/O, timers, channels |
| hyper/axum | HTTP server | Request/response handling |
| ring | Crypto primitives | crypto.subtle implementation |
| flate2 | Compression | Compression streams |

## Sources

- Deno Architecture: https://docs.deno.com/runtime/contributing/architecture/ [HIGH confidence - official docs]
- deno_core ARCHITECTURE.md: https://github.com/denoland/deno_core/blob/main/ARCHITECTURE.md [HIGH confidence - official source]
- WinterCG Standards: https://wintercg.org/work [HIGH confidence - standards body]
- Cloudflare Workers Runtime APIs: https://developers.cloudflare.com/workers/runtime-apis/ [HIGH confidence - reference implementation]
- rusty_v8 Documentation: https://docs.rs/rusty_v8/latest/rusty_v8/ [HIGH confidence - API docs]
- V8 Isolates Documentation: https://v8.dev/docs/embed [MEDIUM confidence - V8 project docs]

## Research Confidence

| Area | Level | Notes |
|------|-------|-------|
| Component Structure | HIGH | Based on Deno/deno_core official architecture |
| Data Flow | HIGH | Standard pattern across V8 runtimes (Deno, CF Workers) |
| Threading Model | HIGH | rusty_v8 docs + deno_core implementation |
| Build Order | MEDIUM | Inferred from dependency analysis |
| Isolation Patterns | HIGH | Documented in V8 embedder guide + deno_core |
