# Phase 02: HTTP Server Core — Context

**Gathered:** 2026-04-19  
**Status:** Ready for planning

<domain>
## Phase Boundary

HTTP server accepts requests and routes by Host header with WinterCG-compatible objects. This phase establishes the HTTP layer that sits between incoming TCP connections and V8 isolate execution.

**In scope:**
- axum HTTP server with configurable port binding
- Virtual host routing via Host header (exact match)
- Request/Response object mapping to WinterCG specification
- Headers API implementation (case-insensitive, multi-value handling)
- URL/URLSearchParams implementation
- Error handling for routing and execution failures

**Out of scope:**
- WebSocket support (Phase 6/v2)
- HTTP/3 or QUIC (future)
- Custom TLS termination (assumes reverse proxy)
- Load balancing across multiple processes

</domain>

<decisions>
## Implementation Decisions

### Server Architecture (D-01 to D-02)
- **D-01:** Full middleware stack — tracing + compression + timeout layers (not minimal)
- **D-02:** State management via `Arc<State>` in axum layer (not global static) for testability

### Virtual Host Routing (D-03 to D-04)
- **D-03:** Exact hostname match only (no wildcards or regex patterns for v1)
- **D-04:** Fallback to default/catch-all handler when no hostname matches (not 404)

### Request/Response Mapping (D-05 to D-06)
- **D-05:** Hybrid body handling — buffer small bodies (<1MB) in memory, stream large bodies
- **D-06:** Response objects via JSON serialization → V8 parse (not direct V8 API creation)

### Headers API Details (D-07 to D-08)
- **D-07:** Case-insensitive header names, normalized to lowercase per RFC 7230
- **D-08:** Multiple values combine with commas, except Set-Cookie which stays separate (browser behavior)

### URL/URLSearchParams (D-09 to D-10)
- **D-09:** Full WinterCG URL compliance (not basic parsing only)
- **D-10:** Percent-decode with lossy UTF-8 replacement (U+FFFD for invalid sequences)

### Error Handling (D-11 to D-12)
- **D-11:** JSON error response format: `{"error": "...", "message": "...", "code": N}`
- **D-12:** JS execution failures: generic 500 to client, detailed error in server logs (no leak)

### the agent's Discretion
- Exact byte thresholds for "small" vs "large" body streaming
- Specific middleware ordering (tracing before/after compression)
- Default/catch-all handler implementation details
- Error code numbering scheme (500 for JS errors, 404 for no route, etc.)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase 1 Context
- `.planning/phases/01-v8-foundation/01-CONTEXT.md` — V8 initialization patterns and EPT fix
- `.planning/phases/01-v8-foundation/01-02-SUMMARY.md` — NanoIsolate API for context creation

### Requirements
- `.planning/REQUIREMENTS.md` §HTTP-01 through HTTP-05 — Phase 2 requirement definitions
- `.planning/REQUIREMENTS.md` §Design Principles — Firecracker VM philosophy

### Project Context
- `.planning/PROJECT.md` — Core value and constraints
- `.planning/ROADMAP.md` §Phase 2 — Goal and success criteria

### WinterCG Specification
- https://wintercg.org/working-group/ — WinterCG working group specs (URL, Headers, Request, Response)

### axum Documentation
- https://docs.rs/axum/latest/axum/ — Router, middleware, State extractor patterns

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/http/mod.rs` — Module skeleton with placeholder documentation (Phase 2 scope already documented)
- `src/v8/isolate.rs` — NanoIsolate with context creation (use for JS execution)
- `src/v8/script.rs` — execute_script() pattern (adapt for fetch() handler invocation)

### Established Patterns
- Module structure: `src/http/` following `src/v8/` pattern
- Error handling: `anyhow::Result` throughout codebase
- Tracing: `tracing` crate for structured logging (already in dependencies)
- Thread safety: `PhantomData<*mut ()>` for !Send + !Sync isolates

### Integration Points
- HTTP server needs to call `v8::platform::initialize_platform()` on startup
- Request handling will create V8 context → call JS fetch() handler → return Response
- Error propagation from JS to HTTP layer needs careful mapping

</code_context>

<specifics>
## Specific Ideas

### Middleware Ordering
```rust
// Intended pattern: Tracing → Timeout → Compression → Routing
let app = Router::new()
    .layer(TraceLayer::new_for_http())
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(CompressionLayer::new())
    .route("/*", any(handler));
```

### Virtual Host Routing Table
```rust
// Config structure (JSON file in Phase 5)
{
  "apps": [
    {"hostname": "api.example.com", "entrypoint": "./api.js"},
    {"hostname": "blog.example.com", "entrypoint": "./blog.js"}
  ],
  "default": {"entrypoint": "./default.js"}
}
```

### Error Response Format
```json
{
  "error": "NotFound",
  "message": "No application configured for hostname 'unknown.example.com'",
  "code": 404
}
```

### URL Lossy Decoding Example
- Input: `https://example.com/path?q=%FF%FE` (invalid UTF-8)
- Output: `https://example.com/path?q=` (replacement characters)

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

### Noted for Future Phases
- WebSocket upgrade support (Phase 6/v2: ADV-03)
- HTTP/3 or QUIC (not on current roadmap)
- Custom TLS/HTTPS termination (assumes reverse proxy like nginx/traefik)
- Header compression (HPACK/QPACK for HTTP/2-3 — future)

</deferred>

---

*Phase: 02-http-server-core*  
*Context gathered: 2026-04-19*
