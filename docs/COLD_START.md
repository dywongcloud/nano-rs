# Cold Start Terminology Guide

**Version:** 1.5.0  
**Last Updated:** 2026-05-02

---

## Overview

NANO uses precise terminology for timing measurements to avoid confusion. The term "cold start" is ambiguous — it could mean process boot time, isolate creation, sliver restoration, or context reset. This guide defines four distinct timing categories used throughout NANO documentation.

---

## Four Timing Categories

### 1. PROCESS BOOT (~60ms)

**Definition:** Time from binary execution to HTTP server ready. One-time per process start.

**What happens:**
- V8 platform initialization (v8::V8::Initialize)
- Configuration loading (JSON parsing, validation)
- HTTP server binding (TCP socket, axum setup)
- Worker pool creation (thread spawning, initial resource allocation)

**When it matters:**
- Every time the `nano-rs` binary starts
- Container/pod restarts
- Deployment rollouts
- Server reboots

**Not a per-request metric** — this is infrastructure startup time.

**Optimization strategies:**
- Minimize config file size
- Use pre-built V8 (avoid compilation)
- Tune worker pool sizing (not too large)

---

### 2. SLIVER RESTORATION (~267µs)

**Definition:** Time from sliver file to first request handling. Per-isolate from snapshot.

**What happens:**
1. Tar archive extraction (if compressed)
2. V8 isolate creation from snapshot blob
3. VFS state restoration (file metadata, contents)
4. Context initialization (global object, built-in APIs)

**When it matters:**
- New isolate needed (pool expansion)
- Isolate replacement (after eviction, crash)
- Multi-tenant cold start (new hostname)

**This is the "fast cold start" — 187x faster than fresh isolate creation.**

**Optimization strategies:**
- Keep sliver files small (compress large static assets separately)
- Use SSD/NVMe storage for sliver files
- Pre-warm worker pools during low-traffic periods

---

### 3. CONTEXT RESET (~5ms)

**Definition:** Time between requests in same isolate. Per-request isolation without full teardown.

**What happens:**
1. Context cleanup (previous request globals cleared)
2. Context creation (new V8 context)
3. Global object reset (clean slate)
4. API bindings re-application (console, fetch, timers)

**When it matters:**
- Every request after the first on a given isolate
- Request-to-request latency in high-throughput scenarios

**This provides isolation without the ~50-100ms cost of fresh isolate creation.**

**Optimization strategies:**
- Context reset is already optimized (~5ms is near-optimal for V8)
- Reduce global pollution in handlers (faster cleanup)
- Use sliver snapshots to skip initial compilation

---

### 4. FRESH ISOLATE CREATION (~50-100ms)

**Definition:** Time to create new isolate without sliver (from source/compiled code).

**What happens:**
1. V8 isolate creation from scratch (heap allocation)
2. Heap initialization (V8 internal setup)
3. Context creation (first context)
4. JavaScript compilation (parse + compile entrypoint)
5. API bindings setup (all WinterCG APIs)

**When it matters:**
- First request to a new hostname (no sliver)
- Sliver corrupted or incompatible
- Development/debugging scenarios

**This is the "slow cold start" — only use when slivers unavailable.**

**Optimization strategies:**
- Always use slivers in production
- Pre-create isolates during deployment
- Bundle/minimize JavaScript (faster compilation)

---

## Comparison Table

| Metric | Time | When It Happens | Relative Speed |
|--------|------|-----------------|----------------|
| Process Boot | ~60ms | Once per binary start | Baseline |
| Sliver Restore | ~267µs | New isolate from snapshot | 225x faster than boot |
| Context Reset | ~5ms | Between requests | 12x slower than sliver |
| Fresh Isolate | ~50-100ms | No sliver available | 187-375x slower than sliver |

---

## Usage Guidelines

### When Reporting Performance

**Correct:**
- "Sliver restoration: ~267µs from snapshot to ready"
- "Context reset overhead: ~5ms between requests"
- "Process boot time: ~60ms on startup"
- "Fresh isolate creation: ~50-100ms without snapshot"

**Incorrect (ambiguous):**
- "Cold start: ~267µs" (which kind?)
- "Startup time: ~60ms" (process or isolate?)
- "Request latency: ~5ms" (context reset or full handling?)

### When Designing Apps

**For lowest latency:**
1. Use sliver snapshots (267µs vs 50-100ms)
2. Pre-warm worker pools (amortize across requests)
3. Keep state in VFS, not globals (faster context reset)

**For predictable performance:**
1. Size worker pools appropriately (avoid cold starts under load)
2. Use memory limits to trigger eviction (controlled, not emergency)
3. Monitor context reset times (should stay ~5ms)

---

## Measurement Methodology

### Tools Used

- **std::time::Instant** — High-resolution monotonic clock
- **perf** — Linux perf events for detailed breakdown
- **Custom instrumentation** — Timing points in Rust code

### Test Environment

- **Hardware:** AMD EPYC 7B13 (2.8GHz), 64GB RAM
- **OS:** Linux 6.8.0, Ubuntu 24.04
- **V8 Version:** 12.4.0 (via rusty_v8 135)
- **Rust Version:** 1.88.0
- **Binary:** Release build (`cargo build --release`)

### Measurement Method

**Process Boot:**
```rust
let start = Instant::now();
// ... V8 init, config load, server bind ...
let boot_time = start.elapsed();
```

**Sliver Restoration:**
```rust
let start = Instant::now();
let isolate = sliver.restore()?;
let restore_time = start.elapsed();
```

**Context Reset:**
```rust
// Inside worker loop
let start = Instant::now();
context.reset()?;  // Clear + recreate context
let reset_time = start.elapsed();
```

**Fresh Isolate:**
```rust
let start = Instant::now();
let isolate = Isolate::new(params)?;
script.compile(&code)?;
let create_time = start.elapsed();
```

### Statistical Method

- **Sample size:** 1000 measurements per metric
- **Reported value:** 50th percentile (median)
- **Variance:** Standard deviation < 15% for all metrics
- **Outliers:** 95th percentile < 2x median (good consistency)

---

## Comparison to Other Runtimes

| Runtime | Cold Start | Notes |
|---------|------------|-------|
| **NANO (sliver)** | **~267µs** | Snapshot-based, per-isolate |
| **NANO (fresh)** | ~50-100ms | Full isolate creation |
| Cloudflare Workers | ~0-5ms | Edge deployment, pre-warmed |
| Deno Deploy | ~50-200ms | V8 isolate + TypeScript compile |
| Node.js | ~100-500ms | Process + module load + compile |
| AWS Lambda (Node) | ~200-1000ms | Container + runtime + handler |
| AWS Lambda (provisioned) | ~50-100ms | Pre-warmed container |

**NANO advantage:** Sub-millisecond cold starts with full V8 isolates, no container overhead.

---

## Related Documentation

- [Performance Benchmarks](PERFORMANCE.md) — Detailed benchmarks and tuning guide
- [Architecture Overview](../ARCHITECTURE.md) — How these timings fit into request flow
- [Sliver System](SLIVER_WORKFLOW.md) — Creating and managing snapshots
- [ROADMAP](../.planning/ROADMAP.md) — Upcoming performance improvements

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.5.0 | 2026-05-02 | Initial terminology standardization |

---

*Last updated: 2026-05-02*
