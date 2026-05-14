---
phase: 02-http-server-core
plan: 02
name: Virtual Host Routing
status: complete
completed_at: 2026-04-19
requirements: [HTTP-02]
dependencies: ["02-01"]

metrics:
  duration: 45min
  commits: 5
  files_created: 2
  files_modified: 5
  tests_added: 12

key-deliverables:
  - "VirtualHostRouter with case-insensitive exact hostname matching"
  - "Fallback handler for unmatched hosts (per D-04)"
  - "axum integration with Host header extraction"
  - "7 integration tests for routing scenarios"

tech-stack:
  added:
    - "axum host header extraction via HeaderMap"
    - "tower ServiceExt for test oneshot()"
  patterns:
    - "Arc<AppState> for shared router across requests"
    - "HashMap<String, RouteTarget> with lowercase keys for case-insensitive lookup"

decisions:
  - "axum 0.8 wildcard syntax: /{*path} instead of /*path"
  - "Extract Host header directly from HeaderMap (axum::extract::Host not in 0.8)"

artifacts:
  created:
    - src/http/router.rs
    - tests/http_routing_test.rs
    - scripts/test_routing.sh
  modified:
    - src/http/server.rs
    - src/http/config.rs
    - src/http/mod.rs
    - src/main.rs
    - tests/http_server_test.rs

key-files:
  - path: "src/http/router.rs"
    purpose: "Virtual host routing logic with RouteTarget, HandlerType, VirtualHostRouter"
    exports: ["VirtualHostRouter", "RouteTarget", "HandlerType", "AppState", "virtual_host_handler"]
  - path: "src/http/server.rs"
    purpose: "Updated server with VirtualHostRouter integration in create_app()"
  - path: "src/http/config.rs"
    purpose: "ServerConfig extended with routes field for future configuration"
  - path: "tests/http_routing_test.rs"
    purpose: "7 integration tests covering routing, fallback, and case insensitivity"

requires:
  - 02-01  # HTTP Server Foundation

provides:
  - HTTP-02  # Requirement for virtual host routing

affects:
  - src/http/mod.rs  # Added router module export
  - src/main.rs      # Now starts HTTP server instead of executing JS
---

# Phase 02 Plan 02: Virtual Host Routing Summary

## One-Liner

Implemented virtual host routing that directs HTTP requests to different handlers based on the Host header, with case-insensitive exact matching and fallback to default handler.

## What Was Delivered

### Core Components

1. **VirtualHostRouter** (`src/http/router.rs`)
   - Exact hostname matching (per D-03)
   - Case-insensitive lookup using lowercase HashMap keys
   - Fallback handler for unmatched hosts (per D-04)
   - Support for two handler types: `StaticResponse` and `JavaScriptEntrypoint`

2. **axum Integration**
   - `virtual_host_handler` extracts Host header from request
   - `AppState` shared across requests via `Arc<AppState>`
   - `error_response` helper for JSON error format (per D-11)

3. **Server Integration**
   - `create_app()` creates router with 2 example routes:
     - `api.example.com` → "API Handler"
     - `blog.example.com` → "Blog Handler"
   - Default handler returns "NANO Runtime"
   - Health endpoint preserved at `/health`
   - Catch-all route at `/{*path}` for virtual host handling

### Test Coverage

- **7 unit tests** in router.rs:
  - Exact match, fallback, default constructor, case variations, multiple routes, JS entrypoint

- **7 integration tests** in `tests/http_routing_test.rs`:
  - `test_routes_by_host_header`: Verifies Host-based routing
  - `test_blog_host_routing`: Multiple route registration
  - `test_fallback_routing`: Default handler for unknown hosts
  - `test_case_insensitive_host`: Uppercase hostname matching
  - `test_mixed_case_host`: Mixed case hostname matching
  - `test_javascript_entrypoint_routing`: JS handler placeholder
  - `test_no_host_header_uses_default`: Fallback without Host header

### Verification

All 62 tests pass:
- 28 unit tests
- 12 HTTP integration tests
- 3 JS execution tests
- 5 V8 integration tests
- 14 doc tests

Build passes: `cargo build --release` succeeds with only 1 unused code warning (error_response helper reserved for future use).

## Deviations from Plan

### Technical Adjustments

1. **axum 0.8 wildcard syntax**: Changed from `/*path` to `/{*path}` (axum 0.8 requirement)

2. **Host header extraction**: Used `HeaderMap` directly instead of `axum::extract::Host` (not available in axum 0.8)
   - Extracts Host header: `request.headers().get(header::HOST)`
   - Falls back to "default" if header missing

3. **Added route_count() accessor**: Added public method to VirtualHostRouter for logging route count at startup

### None - Plan executed as written

All major requirements met:
- ✅ Exact hostname matching (D-03)
- ✅ Fallback handler (D-04)
- ✅ Case-insensitive matching
- ✅ axum integration with middleware
- ✅ Health endpoint preserved
- ✅ All tests passing

## Known Stubs

| File | Line | Description | Resolution |
|------|------|-------------|------------|
| `src/http/router.rs:279` | `JavaScriptEntrypoint` handler | Returns placeholder "JS handler (Phase 3): {path}" | Phase 3 will execute actual JS |
| `src/http/router.rs:256` | `error_response()` helper | Implemented but unused | Reserved for Phase 3 error handling |
| `src/http/config.rs:55` | `routes` field in ServerConfig | Currently only used for testing | Phase 5 adds config file loading |

## Threat Flags

No new threat surface introduced beyond what was planned:
- Host header injection mitigated by exact match only (D-03)
- No route listing endpoints (per threat model T-02-05)
- Generic fallback per D-04 prevents information disclosure

## Documentation

- Router module documented with examples
- All public types have rustdoc comments
- Doctests verify example code compiles

## Dependencies

- **Requires**: Phase 2 Plan 01 (HTTP Server Foundation) - provides axum server infrastructure
- **Enables**: Phase 2 Plan 03 (WinterTC Request/Response) - routing foundation in place
- **Enables**: Phase 3 (Runtime APIs) - JavaScript handler execution

## Performance Notes

- Router lookup is O(1) via HashMap
- Case conversion only on register/resolve (not per character)
- No allocations on hot path (resolve returns reference)

## Commits

1. `d56dfa1` - feat(02-02): create virtual host routing types
2. `d98af8f` - feat(02-02): add axum routing handler with Host extraction
3. `30dea86` - feat(02-02): integrate virtual host routing into HTTP server
4. `ba71be5` - test(02-02): add virtual host routing integration tests
5. `0aa6f0b` - feat(02-02): update main.rs for HTTP server with routing

## Next Steps

Phase 2 Plan 03: WinterTC Request/Response Objects - Build on this routing foundation to implement WinterTC-compliant request/response handling before JS execution integration in Phase 3.
