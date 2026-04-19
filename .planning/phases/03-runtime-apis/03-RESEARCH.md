# Phase 03: Runtime APIs — Research

**Phase:** 03 — Runtime APIs  
**Researched:** 2026-04-19  
**Goal:** JavaScript code can use core WinterCG APIs for basic computation and async operations

## Domain Analysis

### What This Phase Delivers

Phase 3 implements the JavaScript runtime environment that executes within V8 isolates. This is where the HTTP server from Phase 2 meets the V8 engine from Phase 1 — enabling actual JavaScript execution on HTTP requests.

**Key integration point:** The router's `WinterCGHandler` variant (currently a placeholder) will execute real JavaScript code using the WinterCG Request/Response types.

### Scope Boundaries

**In scope:**
- fetch() handler interface (Cloudflare Workers style: `export default { fetch(request) { return new Response(...) } }`)
- console.log/warn/error with structured log output
- TextEncoder/TextDecoder for UTF-8 encoding/decoding
- setTimeout/setInterval with event loop integration
- AbortController/AbortSignal for async cancellation
- structuredClone() for deep object serialization
- crypto.getRandomValues() for random bytes
- performance.now() for high-resolution timing
- Blob and FormData for binary/form handling
- DOMException for standard error types

**Out of scope (future phases):**
- Full crypto.subtle (Phase 9)
- Outbound fetch() from JavaScript (Phase 6)
- ReadableStream/WritableStream (Phase 6)
- WebSocket (Phase 6/v2)

## Technical Research

### 1. V8 Function Binding Pattern

Based on Phase 1 implementation (`src/v8/script.rs`), the pattern for binding Rust functions to JavaScript:

```rust
// Create global object access
let global = context.global(scope);

// Create function from Rust callback
let log_fn = v8::Function::new(scope, console_log_callback);

// Attach to global.console
let console = v8::Object::new(scope);
console.set(scope, log_key.into(), log_fn.into());
global.set(scope, console_key.into(), console.into());
```

**Key insight:** Each API will need a similar binding pattern. We should create a reusable `RuntimeAPIs` struct that manages all bindings.

### 2. Handler Interface Design

Cloudflare Workers / WinterCG pattern:

```javascript
// Standard export pattern
export default {
  async fetch(request, env, ctx) {
    return new Response("Hello", { status: 200 });
  }
};

// Alternative: function export
export async function fetch(request) {
  return new Response("Hello");
}
```

**Decision needed:** Support both patterns or standardize on one?

### 3. Event Loop Integration

setTimeout/setInterval require an event loop that can:
1. Register callbacks with delays
2. Poll for expired timers
3. Execute callbacks in the V8 context

**Architecture options:**
- **Option A:** Use tokio's timer facilities, queue callbacks to V8
- **Option B:** Implement minimal event loop in Rust that integrates with V8

**Recommendation:** Option A (tokio) — already in dependency tree, battle-tested.

### 4. Request/Response in JavaScript

From Phase 2's v8_bridge.rs, we serialize Request to JSON:

```rust
pub fn serialize_request_to_json(request: &NanoRequest) -> String
```

In Phase 3, we need to:
1. Create actual JavaScript Request/Response constructors
2. Parse JavaScript Response objects back to NanoResponse
3. Handle body streaming (JSON serialization for now, streaming in Phase 6)

**Key challenge:** Converting V8 Object back to Rust NanoResponse requires extracting properties (status, headers, body) from the JavaScript object.

### 5. Error Handling

DOMException needs standard error names:
- `AbortError` — for AbortController cancellation
- `TypeError` — for invalid arguments
- `NotFoundError` — for missing resources
- `SecurityError` — for policy violations

Implementation approach:
```rust
fn create_dom_exception(scope: &mut v8::HandleScope, name: &str, message: &str) -> v8::Local<v8::Object> {
    let error = v8::Object::new(scope);
    let name_key = v8::String::new(scope, "name").unwrap();
    let name_val = v8::String::new(scope, name).unwrap();
    error.set(scope, name_key.into(), name_val.into());
    // ... message, stack
    error
}
```

### 6. Performance.now() Precision

Requires high-resolution monotonic clock. In Rust:
```rust
use std::time::Instant;
let start = Instant::now();
let elapsed_ms = start.elapsed().as_nanos() as f64 / 1_000_000.0;
```

**Critical:** Must be monotonic (never goes backward) and high resolution (microsecond precision).

### 7. Crypto.getRandomValues()

V8 has `v8::ArrayBuffer` and `v8::Uint8Array`. We need to:
1. Accept a TypedArray argument
2. Fill it with cryptographically random bytes from Rust's `getrandom` or `rand`
3. Return the same array

```rust
fn crypto_get_random_values_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    // Extract Uint8Array from args
    // Fill with random bytes
    // Return the array
}
```

### 8. Structured Clone

structuredClone() needs to:
1. Serialize JavaScript object to a format that preserves:
   - Primitive types
   - Objects and arrays
   - Date, RegExp, Error objects
   - ArrayBuffer, TypedArrays
   - Maps and Sets
2. Deserialize back to new object

**Approach:** JSON for simple cases, but need special handling for ArrayBuffer, Date, RegExp, etc.

**Simplification for v1:** Support primitives, objects, arrays, ArrayBuffer. Skip complex types initially.

### 9. TextEncoder/TextDecoder

WinterCG specifies UTF-8 encoding only (not full Encoding API).

Implementation:
```rust
struct TextEncoder;
impl TextEncoder {
    fn encode(&self, text: &str) -> Uint8Array { ... }
    fn encode_into(&self, text: &str, buffer: &mut [u8]) -> EncodeIntoResult { ... }
}
```

### 10. AbortController/AbortSignal

Event-driven API:
```javascript
const controller = new AbortController();
const signal = controller.signal;
signal.addEventListener('abort', () => { ... });
controller.abort();
```

**Implementation:** Need event emitter pattern in V8. Store abort state in Rust, trigger JS callbacks when aborted.

## Dependencies

**Required additions:**
```toml
[dependencies]
# Timer event loop integration
tokio = { version = "1", features = ["full", "time"] }  # Already present, ensure time feature

# Cryptographic randomness  
getrandom = "0.2"  # For crypto.getRandomValues

# High-resolution timing (if not already available)
# std::time::Instant is sufficient
```

**Already available:**
- rusty_v8 (V8 bindings)
- serde + serde_json (serialization)
- tokio (async runtime)
- tracing (structured logging)

## Common Pitfalls

### 1. Memory Safety with V8

**Pitfall:** Creating persistent handles without proper scope management.  
**Mitigation:** Always use HandleScope nesting pattern from Phase 1 (D-04).

### 2. Callback Context Lifetime

**Pitfall:** Storing V8 context references across await points.  
**Mitigation:** Use `v8::Global<Value>` for persistent references, reconstruct scope on each callback.

### 3. JSON Serialization Performance

**Pitfall:** Serializing large bodies to JSON is slow and memory-intensive.  
**Mitigation:** Accept this for Phase 3 (v1), plan streaming for Phase 6.

### 4. Error Stack Traces

**Pitfall:** JavaScript errors lose Rust stack context.  
**Mitigation:** Use anyhow's `context()` and tracing for full stack correlation.

## Architecture Patterns

### Runtime APIs Module Structure

```
src/runtime/
├── mod.rs           # Public exports, runtime initialization
├── apis.rs          # All API bindings (console, timers, etc.)
├── handler.rs       # JavaScript handler execution
├── event_loop.rs    # Timer event loop integration
└── types.rs         # Runtime-specific types (timers, abort signals)
```

### Integration Flow

```
HTTP Request (axum)
    ↓
router::virtual_host_handler (Phase 2)
    ↓
NanoRequest creation + serialization to JSON
    ↓
V8: Parse JSON → JavaScript Request object
    ↓
Call JS handler: export.fetch(request)
    ↓
JS returns Response object
    ↓
Extract status, headers, body from V8 object
    ↓
NanoResponse → axum response
```

## Reference Implementations

### Deno
- `ext/web` — AbortController, Event, TextEncoder
- `ext/timers` — setTimeout/setInterval
- `ext/crypto` — getRandomValues

### Cloudflare Workers Runtime
- `workerd` — Open source Workers runtime
- Pattern: Bind Rust functions to V8 global scope

### Node.js 
- `src/node_api.cc` — Similar binding patterns
- `lib/internal/per_context/` — Per-context primordials

## Success Criteria Mapping

| Success Criterion | Implementation Strategy |
|-------------------|------------------------|
| fetch() handler interface | WinterCGHandler executes JS, passes serialized Request, parses Response |
| console.log/warn/error | Bind to tracing crate with structured output |
| TextEncoder/TextDecoder | UTF-8 encode/decode bindings |
| setTimeout/setInterval | Tokio timers + V8 callback integration |
| AbortController/AbortSignal | Event emitter pattern + Rust state |
| structuredClone() | JSON-based serialization with ArrayBuffer support |
| crypto.getRandomValues() | getrandom crate + Uint8Array binding |
| performance.now() | std::time::Instant → millisecond float |
| Blob and FormData | Binary data containers with V8 bindings |
| DOMException | Error object with name/message properties |

## Validation Strategy

**Dimension 1 (Correctness):** Each API has unit tests verifying WinterCG compliance  
**Dimension 2 (Integration):** End-to-end test: HTTP request → JS handler → HTTP response  
**Dimension 3 (Performance):** Handler execution <50ms for simple responses  
**Dimension 4 (Error Handling):** Invalid JS returns 500 with structured error  
**Dimension 5 (Security):** Handler cannot access outside its isolate  
**Dimension 6 (State Management):** Each request gets fresh JS context  
**Dimension 7 (Observability):** Structured logs show handler execution time  
**Dimension 8 (Spec Compliance):** Tests against WinterCG specification examples

---

*Research completed: 2026-04-19*  
*Phase 3 ready for planning*
