# Technology Stack: NANO Edge JavaScript Runtime

**Project:** NANO — Edge JavaScript Runtime (Rust Migration)  
**Researched:** April 19, 2026  
**Confidence:** HIGH (based on official docs, Deno source patterns, and stable releases)

---

## Executive Summary

The 2025 standard stack for building a V8-based JavaScript edge runtime in Rust centers on **rusty_v8** (stable since Sept 2024) for V8 integration, **tokio + axum** for the async HTTP layer, and **deno_core-inspired patterns** for the JS-to-Rust bridge. The ecosystem has matured significantly—rusty_v8 now tracks Chrome versions (v147.x as of April 2026) with pre-built binaries, eliminating the multi-hour V8 compile times that plagued earlier attempts.

**Key architectural decision:** Use rusty_v8 directly (not deno_core) for maximum control over isolate lifecycle and memory management, implementing the extension/op pattern manually while leveraging tokio for async I/O.

---

## Recommended Stack

### Core Framework

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **v8** (rusty_v8) | ^147.0 | V8 JavaScript engine bindings | Zero-overhead C++ API bindings; pre-built binaries; stable Chrome-aligned versioning |
| **tokio** | ^1.52 | Async runtime and I/O | Industry standard; multi-thread scheduler; task spawning; channel primitives |
| **axum** | ^0.8 | HTTP server and routing | Ergonomic router; Tower middleware ecosystem; WebSocket support via tokio-tungstenite |
| **hyper** | ^1.0 (via axum) | HTTP protocol implementation | HTTP/1 + HTTP/2; battle-tested in production |

### Database / State

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **dashmap** | ^6.0 | Concurrent hash maps | Lock-free concurrent reads; good for virtual host routing tables |
| **tokio::sync** | Built-in | Work queues, channels | mpsc for WorkerPool dispatch; broadcast for inter-isolate messaging |

### Crypto (WebCrypto Implementation)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **ring** | ^0.17 | Core crypto primitives | Safe Rust wrapper around BoringSSL; SHA, HMAC, AES-GCM, ECDSA |
| **p256** | ^0.13 | ECDSA/P-256 operations | Pure Rust; WebCrypto ECDSA compliance |
| **rsa** | ^0.9 | RSA operations | WebCrypto RSA-OAEP/RSASSA-PKCS1-v1_5; Marvin Attack mitigation (check version) |
| **rand** | ^0.8 | CSPRNG | crypto.getRandomValues implementation |

### Virtual Filesystem

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **virtual-fs** | ^0.2 | In-memory filesystem per isolate | Multiple backend support (MemoryFS, SandboxedPhysicalFS); conforms to std::fs patterns |
| **vfs-kit** (alt) | ^0.2 | Alternative VFS | Lighter weight; MapFS for pure in-memory; actively developed 2026 |

### Compression (WinterTC CompressionStream)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **flate2** | ^1.1 | DEFLATE/gzip compression | Multiple backends (miniz_oxide, zlib-rs); streaming API; 321M+ downloads |
| **zlib-rs** feature | Via flate2 | Fastest Rust backend | Pure Rust zlib rewrite; outperforms C implementations |

### WebSocket Support

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **tokio-tungstenite** | ^0.29 | WebSocket protocol | RFC 6455 compliant; tokio integration; used by axum's ws feature |
| **axum::extract::ws** | ^0.8 | WebSocket handler | Native axum integration; upgrade handling |

### Serialization / Data

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **serde** | ^1.0 | Data serialization | Standard for Rust; op argument/return serialization |
| **serde_json** | ^1.0 | JSON handling | Request/Response body parsing |
| **bytes** | ^1.0 | Byte buffer handling | Zero-copy where possible; HTTP body handling |

### Supporting Infrastructure

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| **parking_lot** | ^0.12 | Synchronization primitives | Faster than std::sync; const constructors |
| **crossbeam** | ^0.8 | Lock-free data structures | Channels, queues for high-concurrency paths |
| **tracing** | ^0.1 | Structured logging | Industry standard; async-aware |
| **thiserror** | ^2.0 | Error handling | Ergonomic error definitions |
| **anyhow** | ^1.0 | Error propagation | Application-level error handling |

---

## Dependencies to Add

### Cargo.toml

```toml
[dependencies]
# Core runtime
v8 = "147"  # rusty_v8, tracks Chrome releases
tokio = { version = "1.52", features = ["full"] }
axum = { version = "0.8", features = ["ws"] }
tower = "0.5"
tower-http = "0.6"

# HTTP/WebSocket
hyper = "1.0"
hyper-util = "0.1"
tokio-tungstenite = "0.29"
http = "1.0"
http-body = "1.0"
http-body-util = "0.1"

# Crypto (WebCrypto implementation)
ring = "0.17"
p256 = "0.13"
rsa = "0.9"
rand = "0.8"

# Virtual filesystem
virtual-fs = "0.2"
# OR: vfs-kit = "0.2"

# Compression
flate2 = { version = "1.1", features = ["zlib-rs"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bytes = "1.0"

# Utilities
dashmap = "6.0"
parking_lot = "0.12"
crossbeam = "0.8"
tracing = "0.1"
thiserror = "2.0"
anyhow = "1.0"
once_cell = "1.20"

[dev-dependencies]
tokio-test = "0.4"
reqwest = "0.12"
```

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| **V8 Bindings** | v8 (rusty_v8) | deno_core | deno_core couples runtime decisions (event loop, module loading) that conflict with NANO's WorkerPool-per-app architecture |
| **V8 Bindings** | v8 | mozjs (SpiderMonkey) | SpiderMonkey bindings less mature; Deno's V8 ecosystem more battle-tested for edge runtimes |
| **HTTP Server** | axum | actix-web | actix-web has its own runtime (actix-rt); axum integrates better with custom tokio configurations |
| **HTTP Server** | axum | poem | poem is newer; axum has larger ecosystem, more examples, official tokio project |
| **HTTP Server** | axum | hyper directly | hyper is lower-level; axum provides routing, extractors, middleware without significant overhead |
| **Crypto** | ring + p256 + rsa | RustCrypto ecosystem (generic) | ring has broader audit history; BoringSSL pedigree; better side-channel resistance guarantees |
| **VFS** | virtual-fs | vfs (older crate) | vfs lacks active maintenance; virtual-fs has SandboxedPhysicalFS for security |
| **Compression** | flate2 | zstd | zstd not in WinterTC spec; flate2 covers gzip/deflate as required |
| **Compression** | flate2 | libz-sys | flate2 with zlib-rs backend is pure Rust, no C dependencies, faster |

---

## Critical Design Decisions

### 1. Use rusty_v8 Directly (Not deno_core)

**Rationale:** deno_core provides a high-level runtime abstraction with its own event loop, module loading, and op scheduling. NANO needs:
- Per-isolate WorkerPool with custom thread affinity
- Manual V8 context lifecycle (dispose/recreate between requests)
- Direct control over ArrayBuffer external storage (EPT fix)
- Custom virtual host routing outside deno_core's request flow

**Pattern:** Adopt deno_core's `op2` macro approach for JS-Rust binding ergonomics, but implement the runtime loop manually with tokio channels.

### 2. Native Rust Crypto (Not V8 crypto.subtle)

**Rationale:** V8's crypto.subtle is implemented in C++ with complex internal state. Using ring/p256/rsa:
- Avoids V8 internal complexity and potential SIGSEGV in crypto paths
- Allows Rust's memory safety guarantees for crypto operations
- Easier to audit and update independently of V8 version
- Better async integration with tokio (crypto.subtle in V8 uses its own task scheduler)

### 3. Pre-built V8 Binaries

**Rationale:** Setting `V8_FROM_SOURCE=0` (default) downloads pre-built static libs from GitHub releases.
- Build time: ~2 minutes vs ~30-120 minutes from source
- Binary size: ~80MB static lib per platform
- CI/CD: Must cache `RUSTY_V8_MIRROR` directory

**When to build from source:** Only when modifying rusty_v8 bindings themselves (rare).

### 4. Axum over Hyper Directly

**Rationale:** Axum adds minimal overhead while providing:
- Type-safe extractors (Path, Query, Json, State)
- Tower middleware compatibility (compression, tracing, auth)
- Built-in WebSocket upgrade handling
- Request routing with path parameters

**Trade-off:** Router compile times slightly longer than hyper directly, but negligible runtime cost.

---

## Version Alignment Notes

### V8 Versioning (rusty_v8)

- rusty_v8 **major version = Chrome major version** (e.g., v147.x = Chrome 147)
- New major version every ~4 weeks (Chrome release cycle)
- V8 engine version lags slightly (v147.x uses V8 14.7.x)
- Semver: Major bumps may have API changes; patch releases are safe

**Recommendation:** Pin to major version (`v8 = "147"`) and update quarterly after testing.

### Tokio Compatibility

- tokio 1.x has been stable since 2020; minor releases add features
- axum 0.8 requires tokio ^1.44
- Use `rt-multi-thread` for production (multi-threaded scheduler)

---

## Installation & Setup

### macOS / Linux

```bash
# Add dependencies
cargo add v8@147 tokio@1.52 axum@0.8 tower tower-http
cargo add ring@0.17 p256@0.13 rsa@0.9 rand@0.8
cargo add virtual-fs@0.2 flate2@1.1 --features zlib-rs
cargo add serde serde_json bytes dashmap parking_lot
cargo add tracing thiserror anyhow once_cell

# Build (uses pre-built V8 by default)
cargo build --release

# For development with debug V8:
V8_FORCE_DEBUG=true cargo build
```

### Windows

```powershell
# Requires Visual Studio 2019+ with C++ tools
# LLVM/Clang not required - downloaded automatically by rusty_v8 build script
cargo build --release
```

### CI/CD Caching

```yaml
# .github/workflows/ci.yml
- name: Cache V8 binaries
  uses: actions/cache@v4
  with:
    path: ~/.cache/rusty_v8
    key: v8-${{ runner.os }}-${{ hashFiles('Cargo.lock') }}
```

---

## Architecture Integration Notes

### rusty_v8 Key APIs

```rust
// Platform initialization (once per process)
let platform = v8::new_default_platform(0, false).make_shared();
v8::V8::initialize_platform(platform);
v8::V8::initialize();

// Isolate creation
let isolate = &mut v8::Isolate::new(Default::default());
let scope = &mut v8::HandleScope::new(isolate);
let context = v8::Context::new(scope);
let scope = &mut v8::ContextScope::new(scope, context);

// Script execution
let code = v8::String::new(scope, "1 + 1").unwrap();
let script = v8::Script::compile(scope, code, None).unwrap();
let result = script.run(scope).unwrap();
```

### Axum Integration with V8

```rust
use axum::{extract::State, routing::post, Router};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    worker_pool: WorkerPool, // Your V8 isolate pool
}

async fn handle_request(
    State(state): State<Arc<AppState>>,
    body: bytes::Bytes,
) -> axum::response::Response {
    // Dispatch to V8 isolate via WorkerPool
    let result = state.worker_pool.execute(body).await;
    axum::response::Json(result).into_response()
}

let app = Router::new()
    .route("/*path", post(handle_request))
    .with_state(Arc::new(AppState { worker_pool }));
```

### Tokio Channels for WorkQueue

```rust
use tokio::sync::mpsc;

// Multi-producer, single-consumer for worker dispatch
let (tx, mut rx) = mpsc::channel::<WorkItem>(1000);

// Spawn worker that receives from channel
tokio::spawn(async move {
    while let Some(work) = rx.recv().await {
        // Execute in V8 isolate
    }
});
```

---

## Anti-Patterns to Avoid

### ❌ Don't Use serde_v8

The `serde_v8` crate (used by older Deno versions) is deprecated and has soundness issues. Instead:
- Use `v8::Local<v8::Value>` direct conversion for hot paths
- Use `serde_json::Value` for structured data crossing the boundary
- Implement custom `FromV8`/`ToV8` traits for specific types

### ❌ Don't Build V8 From Source in CI

Unless you're modifying rusty_v8 itself:
- Always use pre-built binaries (default)
- Cache the `RUSTY_V8_ARCHIVE` path between builds
- Building from source adds 30-120 minutes to CI

### ❌ Don't Mix V8 and Tokio Thread Pools

- V8 isolates must stay on their assigned threads (V8 is not thread-safe across isolates)
- Use `tokio::task::spawn_local` or dedicated threads for isolate execution
- Use channels to communicate between HTTP async handlers and isolate threads

### ❌ Don't Use V8's crypto.subtle

- Implement `crypto.subtle` entirely in Rust via the op system
- V8's crypto uses internal C++ state that's hard to debug
- Rust crypto crates have better audit trails

---

## Confidence Assessment

| Component | Confidence | Evidence |
|-----------|----------|----------|
| **rusty_v8** | HIGH | Stable release (Sept 2024); Deno production use; 3M+ downloads; Chrome-aligned versioning |
| **tokio + axum** | HIGH | Industry standard; tokio 1.x stability guarantee; axum official tokio project |
| **ring crypto** | HIGH | BoringSSL pedigree; used by rustls; multiple security audits |
| **p256/rsa** | MEDIUM-HIGH | RustCrypto ecosystem; p256 audited; rsa has Marvin patches in recent versions |
| **virtual-fs** | MEDIUM | Less battle-tested than other components; viable alternatives (vfs-kit) available |
| **flate2** | HIGH | 321M+ downloads; standard compression in Rust; used by cargo itself |

---

## Sources

- **rusty_v8 releases:** https://github.com/denoland/rusty_v8/releases (v147.3.0, April 2026)
- **Deno blog (stable announcement):** https://deno.com/blog/rusty-v8-stabilized (Sept 2024)
- **deno_core extension system:** https://docs.rs/deno_core/latest/deno_core/ (v0.387)
- **deno_core V8 bridge article:** https://readoss.com/en/denoland/deno/v8-bridge-deno-extension-system-rust-to-javascript (April 2026)
- **axum docs:** https://docs.rs/axum/0.8.9/axum/ (April 2026)
- **tokio docs:** https://docs.rs/tokio/1.52.1/tokio/ (April 2026)
- **ring crypto:** https://docs.rs/ring/0.17/ring/
- **flate2:** https://docs.rs/flate2/1.1/flate2/
- **virtual-fs:** https://docs.rs/virtual-filesystem/0.2/

---

*Last updated: 2026-04-19*
