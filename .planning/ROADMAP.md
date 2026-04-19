# NANO Edge Runtime — Roadmap

**Current:** v1.0 SHIPPED ✅  
**Date:** 2026-04-19  
**Next:** v2.0 planning (run `/gsd-new-milestone` to start)

---

## Milestones

### v1.0 — Edge Runtime Foundation ✅

**Status:** SHIPPED  
**Date:** 2026-04-19  
**Scope:** Multi-tenant JavaScript edge runtime with WinterCG compliance

**9 phases, 42 requirements, 151 commits:**
- ✅ Phase 1: V8 Foundation (EPT fix, rusty_v8 integration)
- ✅ Phase 2: HTTP Server Core (axum, virtual host routing)
- ✅ Phase 3: Runtime APIs (console, timers, encoding, basic crypto)
- ✅ Phase 4: WorkerPool & Dispatch (multi-threading, context reset)
- ✅ Phase 5: Multi-App Hosting (config, limits, hot-reload)
- ✅ Phase 6: Outbound I/O (fetch, streaming)
- ✅ Phase 7: Production Features (logging, metrics, admin API)
- ✅ Phase 8: Framework Compatibility (Hono.js, Next.js, Astro)
- ✅ Phase 9: Crypto Core (AES-GCM, HMAC, JWK)

**Full details:** [v1.0-ROADMAP.md](./milestones/v1.0-ROADMAP.md)  
**Requirements:** [v1.0-REQUIREMENTS.md](./milestones/v1.0-REQUIREMENTS.md)

---

## Backlog

### Phase 999.1: Isolate Checkpoint/Restore
**Goal:** Enable serialization and migration of V8 isolates between NANO instances  
**Status:** Backlog — post-v2 exploration  
**Requirements:** TBD

### Phase 999.2: Virtual File System (VFS)
**Goal:** Per-isolate in-memory filesystem for static assets  
**Status:** Backlog — after checkpoint/restore  
**Requirements:** TBD

---

## What's Next

To start v2.0 planning: `/gsd-new-milestone`

Potential v2.0 focus areas:
- WebSocket server (RFC 6455)
- VFS for static asset hosting
- Advanced crypto (RSA, ECDSA)
- CompressionStream/DecompressionStream
- Inter-isolate messaging
- V8 startup snapshots for faster cold starts

---

*Roadmap archived: 2026-04-19 — v1.0 milestone complete*
