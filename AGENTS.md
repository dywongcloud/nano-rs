# NANO Edge Runtime — Agent Instructions

**Project:** Multi-tenant JavaScript edge runtime (Rust + rusty_v8)  
**Current Phase:** 1 — V8 Foundation  
**Core Value:** One OS process hosts many isolated JS apps with millisecond cold starts

## Critical Technical Context

**EPT Fix Required (Day 1):** The ArrayBuffer allocation SIGSEGV (AP-02 from Zig version) requires a strong `v8::Global<Value>` sentinel per isolate. This prevents the ExternalPointerTable segment unmapping bug. Implement this in Phase 1 before any isolate operations.

**Architecture:** WorkerPool → WorkQueue → isolate-per-thread. Context reset between requests (~5ms), not full isolate recreation (~50-100ms).

**Not a General Runtime:** Specialized for high-density hosting. No npm resolution, no Node.js API surface (except minimal v2 compat). Users bundle apps beforehand.

## Framework Compatibility Targets

- **Hono.js:** Primary target — lightweight, WinterTC-native
- **Next.js static export:** HTML/CSS/JS assets serve correctly
- **Astro static build:** Islands architecture preserved
- **Generic WinterTC:** Any spec-compliant framework

## Build Commands

```bash
# Standard build
cargo build --release

# With all tests
cargo test --all

# Check without building
cargo check
```

## Testing

Each phase should include:
1. Unit tests for Rust modules
2. Integration tests with actual JS execution
3. Framework compatibility tests (Hono/Next.js/Astro examples)

## Documentation

- `.planning/PROJECT.md` — Project context and decisions
- `.planning/REQUIREMENTS.md` — v1/v2 requirements with traceability
- `.planning/ROADMAP.md` — 9-phase roadmap with success criteria
- `.planning/STATE.md` — Current state and accumulated context
- `.planning/research/` — Stack, features, architecture, pitfalls research

## Workflow

This project uses GSD (Get Shit Done) workflow:
- `/gsd-plan-phase N` — Create detailed plan for phase N
- `/gsd-discuss-phase N` — Gather context before planning
- `/gsd-execute-phase N` — Execute all plans in phase N
- `/gsd-progress` — Check overall project status

## Constraints

- Use pre-built rusty_v8 (never compile V8 from source)
- Rust crypto crates (ring, p256, rsa) — bypass V8 crypto.subtle C++
- Thread-local isolates — never move between threads
- Nested HandleScope pattern — prevent memory leaks
