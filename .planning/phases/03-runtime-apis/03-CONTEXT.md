# Phase 03: Runtime APIs — Context

**Gathered:** 2026-04-19  
**Status:** Ready for planning

<domain>
## Phase Boundary

JavaScript runtime environment executing within V8 isolates. This phase enables the HTTP server from Phase 2 to actually execute JavaScript code and return dynamic responses.

**In scope:**
- fetch() handler interface for Cloudflare Workers-style request handling
- Core JavaScript APIs: console, timers, encoding, basic crypto
- JavaScript → Rust integration for Request/Response flow
- Error handling with DOMException

**Out of scope:**
- Full crypto.subtle (Phase 9)
- Outbound fetch() from JavaScript (Phase 6)
- Streaming bodies (Phase 6)

</domain>

<decisions>
## Implementation Decisions

### Handler Interface (D-01)
- **D-01:** Support Cloudflare Workers export pattern: `export default { fetch(request) { ... } }`
- Standard WinterTC Request/Response objects passed to handler

### API Implementation (D-02 to D-06)
- **D-02:** console.log binds to Rust tracing crate (structured logging)
- **D-03:** setTimeout/setInterval use tokio timers (not custom event loop)
- **D-04:** TextEncoder/TextDecoder support UTF-8 only (per WinterTC)
- **D-05:** crypto.getRandomValues uses getrandom crate (not V8 entropy)
- **D-06:** performance.now() uses std::time::Instant (monotonic high-res)

### Handler Execution (D-07 to D-08)
- **D-07:** Request serialization via JSON → V8 parse (existing v8_bridge pattern)
- **D-08:** Response extraction via V8 object property access

### the agent's Discretion
- Exact module structure for runtime APIs
- Timer callback queue implementation details
- structuredClone complexity level (full vs simplified)
- Blob and FormData implementation depth

</decisions>

<canonical_refs>
## Canonical References

### Prior Phase Context
- `.planning/phases/02-http-server-core/02-CONTEXT.md` — WinterTC types and routing
- `.planning/phases/02-http-server-core/02-03-SUMMARY.md` — v8_bridge serialization pattern
- `.planning/phases/01-v8-foundation/01-03-SUMMARY.md` — console.log binding pattern

### Requirements
- `.planning/REQUIREMENTS.md` §API-01 through API-10 — Phase 3 requirement definitions

### Project Context
- `.planning/ROADMAP.md` §Phase 3 — Goal and success criteria
- `.planning/PROJECT.md` — Core value and constraints

### Technical References
- WinterTC specification: https://wintertc.org/working-group/
- Cloudflare Workers docs: https://developers.cloudflare.com/workers/

</canonical_refs>

<deferred>
## Deferred Ideas

- WebSocket support (Phase 6/v2)
- Advanced Encoding API (only UTF-8 for v1)
- Full Web Streams API (Phase 6)

</deferred>

---

*Phase: 03-runtime-apis*
