# Phase 08: Framework Compatibility — Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-19
**Phase:** 08-framework-compatibility
**Areas discussed:** Framework detection & routing, Static asset serving, Entry point patterns, Framework test strategy

---

## Framework Detection & Routing

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-detect by entrypoint analysis | Parse JS to detect imports/patterns. Zero-config but complex. | |
| Explicit config flag per app | Add 'framework' field to config. Simple but requires user input. | |
| Standardize on single pattern | Require all apps to export WinterCG handler. Reduces compatibility. | |
| **Standard WinterCG only** | **No framework detection — frameworks adapt to runtime like CF Workers/Deno** | ✓ |

**User's choice:** Standard WinterCG only
**Notes:** User insight — "how does other runtimes do that? they don't know the frameworks, we should be the same." This led to the key realization that edge runtimes don't detect frameworks; frameworks export WinterCG handlers when configured for edge deployment.

---

## Static Asset Serving

| Option | Description | Selected |
|--------|-------------|----------|
| **VFS bundle approach** | **All assets bundled into JS entrypoint. No filesystem access.** | ✓ |
| Static directory serving | Add 'static_dir' config. Simple but violates no-filesystem constraint. | |
| JS handler serves all | Require router in JS to serve files. Maximum flexibility. | |

**User's choice:** VFS bundle approach
**Notes:** Keeps NANO pure with no filesystem access, consistent with PROJECT.md constraints. Frameworks must bundle static assets.

---

## Entry Point Patterns

| Option | Description | Selected |
|--------|-------------|----------|
| **Cloudflare Workers style only** | **Support only `export default { fetch(request) {...} }`** | ✓ |
| Multiple common patterns | Support various export styles. More flexible but more code. | |
| Configurable entry function | Add config to specify which export to use. Requires user config. | |

**User's choice:** Cloudflare Workers style only
**Notes:** Consistent with existing Phase 3 D-01 decision. Single standard pattern.

---

## Framework Test Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| **Minimal test apps** | **Hand-written test apps mimicking framework patterns. Fast CI.** | ✓ |
| Real framework builds | Actually install/build frameworks. Realistic but slow. | |
| Hybrid approach | Minimal in CI, real builds in nightly. Balanced. | |

**User's choice:** Minimal test apps
**Notes:** Fast tests with clear failure diagnosis. No external npm dependencies in CI.

---

## the agent's Discretion

No areas deferred to the agent — all decisions were explicit.

## Deferred Ideas

- Real framework npm installs in CI (slow, can add later as extended tests)
- Framework-specific optimization hints (future phase)
- VFS implementation for file serving (Phase 999.2 backlog)
- Framework adapter SDKs (if demand emerges)
