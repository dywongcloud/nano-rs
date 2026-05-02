# NANO Performance Characteristics

**Version:** 1.5.0  
**Last Updated:** 2026-05-02

---

## Executive Summary

| Metric | Time | When It Matters |
|--------|------|----------------|
| Process Boot | ~60ms | Every process restart |
| Sliver Restore | ~267µs | New isolate from snapshot |
| Context Reset | ~5ms | Every request (same isolate) |
| Fresh Isolate | ~50-100ms | New isolate without snapshot |
| Request Handling | <1ms | HTTP overhead (excluding JS execution) |

**Key Takeaway:** NANO delivers sub-millisecond cold starts via sliver snapshots (267µs), 187x faster than fresh isolate creation (~50ms). Context reset between requests adds only ~5ms overhead.

---

## Detailed Benchmarks

### Boot and Restoration Times

| Operation | Median | P95 | P99 | Unit |
|-----------|--------|-----|-----|------|
| Process Boot | 60 | 72 | 85 | ms |
| Sliver Restoration | 267 | 312 | 385 | µs |
| Context Reset | 5.2 | 6.8 | 8.1 | ms |
| Fresh Isolate Creation | 75 | 95 | 120 | ms |

*Measured on AMD EPYC 7B13 (2.8GHz), Linux 6.8.0, V8 12.4.0*

### Memory Overhead

| Component | Per-Isolate | Notes |
|-----------|-------------|-------|
| V8 Heap (default) | 128 MB | Configurable per-app |
| V8 Heap (minimum) | 16 MB | Lower for simple apps |
| Rust Runtime | ~50 MB | Shared across all isolates |
| HTTP Server | ~20 MB | Connection buffers, routing |
| **Total per app** | **4-8 GB** | With 32 workers @ 128MB |

### Throughput Limits

| Metric | Value | Configuration |
|--------|-------|---------------|
| Max Concurrent Isolates | 1000+ | Memory-limited |
| Requests Per Second (RPS) | 50,000+ | Per core, simple handlers |
| Max Request Body Size | 100 MB | Configurable |
| Max Response Body Size | 100 MB | Configurable |

### Latency Distribution

End-to-end request latency (simple handler returning "Hello"):

| Percentile | Latency | Breakdown |
|------------|---------|-----------|
| P50 | 2.1 ms | Routing + context reset + handler |
| P95 | 4.5 ms | With occasional GC |
| P99 | 8.2 ms | With cold context |

---

## Comparison to Other Runtimes

### Cold Start Performance

| Runtime | Cold Start | Warm Request | Notes |
|---------|------------|--------------|-------|
| **NANO (sliver)** | **267 µs** | **<1 ms** | Snapshot-based isolate |
| **NANO (fresh)** | 50-100 ms | <1 ms | Full isolate creation |
| Cloudflare Workers | 0-5 ms | <1 ms | Pre-warmed at edge |
| Deno Deploy | 50-200 ms | 1-3 ms | TypeScript compilation |
| Node.js (container) | 100-500 ms | 5-20 ms | Process + module load |
| AWS Lambda | 200-1000 ms | 10-50 ms | Container cold start |
| AWS Lambda (provisioned) | 50-100 ms | 10-50 ms | Pre-warmed container |

### Memory Efficiency

| Runtime | Per-Request Memory | Isolation Model |
|---------|-------------------|-----------------|
| **NANO** | 128 MB (isolate) | V8 isolate |
| Cloudflare Workers | 128 MB | V8 isolate |
| Deno Deploy | 512 MB | Deno process |
| Node.js (per container) | 512 MB+ | Process |
| AWS Lambda | 128-1024 MB | Container |

---

## Tuning Guide

### Worker Pool Sizing

**Default:** 4 workers per app

**For low traffic (< 10 RPS):**
- Workers: 1-2
- Memory: 64 MB per isolate
- Cold starts acceptable

**For medium traffic (10-1000 RPS):**
- Workers: 4-8 (default good)
- Memory: 128 MB per isolate
- Pre-warm with synthetic traffic

**For high traffic (> 1000 RPS):**
- Workers: 8-16 (match CPU cores)
- Memory: 256 MB per isolate
- Use slivers exclusively
- Monitor context reset times

**Formula:**
```
Workers = min(CPU_cores * 2, max_expected_concurrent_requests / 10)
```

### Sliver vs Fresh Isolate Trade-offs

| Factor | Sliver | Fresh Isolate |
|--------|--------|---------------|
| **Cold start** | 267 µs | 50-100 ms |
| **File size** | 10-500 KB | N/A |
| **Build step** | Required (create sliver) | None |
| **Updates** | Need new sliver | Instant |
| **Use case** | Production | Development |

**Recommendation:**
- **Development:** Use `--entrypoint` (faster iteration)
- **Production:** Always use `--sliver` (faster cold starts)
- **CI/CD:** Build sliver as artifact, deploy sliver

### Memory vs Latency Trade-offs

**Lower memory (64 MB):**
- More isolates per server
- Higher eviction rate
- More cold starts (if not using slivers)

**Higher memory (256 MB):**
- Fewer isolates per server
- Lower eviction rate
- Better cache hit rates

**CPU Time Limits (v1.5.0+):**
- Default: 50ms per request (Cloudflare-style)
- CPU-intensive tasks: Increase to 100-500ms
- Background tasks: Not supported (use separate service)

---

## Measurement Methodology

### Test Environment

- **Hardware:** AMD EPYC 7B13 (2.8GHz), 64GB RAM, NVMe SSD
- **OS:** Ubuntu 24.04 LTS, Linux 6.8.0
- **V8:** 12.4.0 (via rusty_v8 135)
- **Rust:** 1.88.0
- **Build:** Release profile (`cargo build --release`)

### Tools

- **std::time::Instant** — Monotonic high-res clock
- **perf stat** — Hardware counters
- **wrk** — HTTP load testing
- **custom harness** — V8 timing instrumentation

### Methodology

1. **Warm-up:** 100 requests to warm caches
2. **Measurement:** 1000 requests, record timing
3. **Statistics:** Report median (P50), P95, P99
4. **Iterations:** 10 runs, report average of medians

### Reproducing Results

```bash
# Build release binary
cargo build --release

# Test sliver restoration
time ./target/release/nano-rs run --sliver app.sliver &
curl http://localhost:8080/  # First request after boot

# Benchmark with wrk
wrk -t4 -c100 -d30s http://localhost:8080/

# Check metrics
curl -H "X-API-Key: secret" http://localhost:8889/metrics
```

---

## Configuration for Performance

### Fastest Cold Starts

```json
{
  "apps": [
    {
      "hostname": "api.example.com",
      "sliver": "./app.sliver",
      "workers": 4,
      "memory_limit_mb": 128
    }
  ]
}
```

### Maximum Throughput

```json
{
  "apps": [
    {
      "hostname": "high-traffic.example.com",
      "sliver": "./app.sliver",
      "workers": 16,
      "memory_limit_mb": 256,
      "cpu_limit_ms": 100
    }
  ]
}
```

### Memory-Constrained

```json
{
  "apps": [
    {
      "hostname": "lightweight.example.com",
      "sliver": "./small-app.sliver",
      "workers": 2,
      "memory_limit_mb": 64
    }
  ]
}
```

---

## Performance Monitoring

### Key Metrics to Track

| Metric | Target | Alert If |
|--------|--------|----------|
| Context reset time | < 10 ms | > 15 ms |
| Sliver restore time | < 500 µs | > 1 ms |
| Memory usage | < 80% limit | > 95% limit |
| Request latency (P95) | < 10 ms | > 50 ms |
| Error rate | < 0.1% | > 1% |

### Admin API Endpoints

```bash
# Get latency metrics
curl -s -H "X-API-Key: secret" http://localhost:8889/metrics | \
  grep "nano_request_duration_seconds"

# Get memory usage
curl -s -H "X-API-Key: secret" http://localhost:8889/isolates | \
  jq '.isolates[] | {id: .id, memory_mb: .memory_mb}'

# Prometheus scraping
curl -H "X-API-Key: secret" http://localhost:8889/metrics
```

---

## Bottlenecks and Solutions

### Slow Context Reset (> 10ms)

**Symptoms:** High P95 latency, increasing over time

**Causes:**
- Global object pollution (accumulated state)
- Large VFS directories (cleanup overhead)
- Memory pressure (GC during reset)

**Solutions:**
1. Avoid global variables (use local scope)
2. Clean up VFS in handler (don't rely on reset)
3. Increase memory limits or add workers

### High Cold Start Rate

**Symptoms:** Many sliver restores under load

**Causes:**
- Worker pool too small
- Aggressive memory eviction
- Traffic spikes

**Solutions:**
1. Increase `workers` count
2. Increase `memory_limit_mb`
3. Pre-warm pools with synthetic traffic

### Memory Exhaustion

**Symptoms:** OOM kills, 503 errors, high eviction

**Causes:**
- Too many apps with high memory limits
- Memory leaks in handlers
- Insufficient total RAM

**Solutions:**
1. Reduce `memory_limit_mb` per app
2. Reduce `workers` count
3. Add more servers (horizontal scaling)

---

## Future Improvements

### v2.0 Planned

- **Incremental sliver updates** — Delta snapshots for faster updates
- **Cross-isolate caching** — Shared compiled code cache
- **Compression** — Smaller sliver files (trade CPU for I/O)
- **Predictive pre-warming** — ML-based worker pool sizing

### Research Areas

- **V8 pointer compression** — Reduce heap overhead
- **Snapshot precompilation** — AOT compile to bytecode
- **NUMA awareness** — Optimize for multi-socket servers

---

## References

- [Cold Start Terminology](COLD_START.md) — Definitions and usage guidelines
- [Architecture Overview](../ARCHITECTURE.md) — How components interact
- [Configuration Reference](CONFIG.md) — All tunable parameters
- [CLI Reference](CLI.md) — Commands for performance testing

---

*Last updated: 2026-05-02*
