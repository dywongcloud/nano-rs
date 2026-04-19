# NANO — Edge JavaScript Runtime (Rust Migration)

## What This Is

NANO is a single-process HTTP server that hosts multiple JavaScript applications in parallel, each in its own V8 isolate. It replaces container fleets running one Node.js app per pod—eliminating operational overhead, slow startup times, and resource waste. One binary, one config file, many isolated apps.

Target users: Platform engineers and infrastructure teams running multi-tenant JavaScript workloads (API gateways, edge functions, webhook processors, serverless platforms) who need Cloudflare Workers-like performance without vendor lock-in.

## Core Value

Skip the container fleet entirely—one OS process hosts many isolated JS apps with millisecond cold starts, zero container overhead, and strong per-app isolation.

## Requirements

### Validated

(None yet—ship to validate)

### Active

- [ ] Rust project skeleton with rusty_v8 integration
- [ ] Platform initialization and single V8 isolate
- [ ] HTTP server (hyper/axum) with fetch() handler interface
- [ ] Core WinterCG APIs: Request/Response/Headers/URL
- [ ] TextEncoder/TextDecoder and console APIs
- [ ] crypto.getRandomValues() implementation
- [ ] Outbound fetch() via tokio
- [ ] WorkerPool with N worker threads per app
- [ ] WorkQueue dispatch (tokio channels)
- [ ] Virtual host routing (Host header → app mapping)
- [ ] Context reset between requests (dispose/recreate V8 context)
- [ ] Extended WinterCG: Streams (Readable/Writable/Transform)
- [ ] crypto.subtle implementation using Rust crypto crates (ring, p256, rsa)
- [ ] CompressionStream/DecompressionStream (flate2)
- [ ] WebSocket server (RFC 6455)
- [ ] VFS (Virtual Filesystem) per isolate
- [ ] Inter-isolate messaging API
- [ ] EPT initialization fix (strong v8::Global sentinel)
- [ ] V8 startup snapshot support
- [ ] Integration test parity with Zig version

### Out of Scope

- npm ecosystem support—apps are single-file, bundling is user responsibility
- TypeScript/JSX transpilation (user must bundle beforehand)
- Native module support (only pure JS/WinterCG APIs)
- Subprocess spawning from JS
- Built-in horizontal clustering (requires external load balancer)
- Global edge network (self-hosted only)
- queueMicrotask, atob/btoa (WinterCG gaps, can add later)

## Context

**Migration from Zig to Rust:** The current Zig implementation works but has significant maintenance burden—2-hour V8 rebuilds, hand-maintained C bindings, Zig stdlib instability. The Rust migration resolves these via pre-built rusty_v8 binaries and type-safe bindings.

**Architecture preserved:** WorkerPool → WorkQueue → isolate-per-thread model stays identical. V8 isolates remain security boundaries with context reset between requests.

**Critical technical debt:** AP-02 EPT SIGSEGV bug (ArrayBuffer allocation in serve path) must be addressed on day 1 in Rust. Same fix pattern applies: strong v8::Global<Value> sentinel per isolate.

**Crypto strategy:** Bypass V8's crypto.subtle C++ entirely. Implement all crypto in Rust using ring/rsa/p256 crates—safer and avoids V8 internal complexity.

## Constraints

- **Tech stack**: Rust + rusty_v8 + tokio + hyper/axum
- **Timeline**: ~4 months to functional parity (per migration analysis)
- **API surface**: WinterCG Minimum Common API compliance target
- **V8 version**: Tracks Deno's rusty_v8 (auto-updates via crate)
- **Build time**: Must use pre-built V8 (no 2-hour compiles)
- **Debuggability**: Richer Rust debug symbols than Zig

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust + rusty_v8 over Zig | Pre-built V8, type-safe bindings, stable ecosystem | — Pending |
| Rust crypto crates over V8 crypto | ring/rsa/p256 safer than V8 C++ crypto internals | — Pending |
| No npm/import resolution | Simplifies runtime, keeps isolates lightweight | — Pending |
| WorkerPool per virtual host | Resource isolation between apps | — Pending |
| Context reset (not new isolate per request) | 5ms vs 50-100ms context disposal cost | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-04-19 after initialization*
