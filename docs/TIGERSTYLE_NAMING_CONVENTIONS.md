# TigerStyle Naming Conventions for nano-rs

Based on TigerBeetle's TigerStyle methodology.

## Core Rules

### 1. Use snake_case

All identifiers use snake_case (lowercase with underscores).

```rust
// Correct
fn handle_request(request: Request) -> Response

// Incorrect
fn handleRequest(request: Request) -> Response
fn handle_request(request: request) -> response
```

### 2. No Abbreviations

Never abbreviate. Write it out.

| Don't Use | Use Instead |
|-----------|-------------|
| ctx | context |
| req | request |
| res | response |
| resp | response |
| buf | buffer |
| len | length |
| idx | index |
| cnt | count |
| num | number |
| val | value |
| ptr | pointer |
| ref | reference |
| opts | options |
| cfg | configuration |
| err | error |
| msg | message |
| str | string |
| int | integer |
| func | function |
| cb | callback |
| src | source |
| dst | destination |
| addr | address |
| conn | connection |
| stmt | statement |
| expr | expression |
| init | initialization |
| exec | execution |
| eval | evaluation |
| gen | generation |
| calc | calculation |
| env | environment |
| tmp/temp | temporary |
| curr | current |
| prev | previous |
| min | minimum |
| max | maximum |

### 3. Big-Endian Naming

Put qualifiers at the end (big-endian), not the beginning.

| Don't Use | Use Instead |
|-----------|-------------|
| max_file_size | file_size_bytes_max |
| max_total_storage | total_storage_bytes_max |
| max_files | files_count_max |
| max_heap_size | heap_size_bytes_max |
| min_timeout | timeout_min |
| max_buffer_length | buffer_length_max |
| cached_script | script_cached |
| enabled_feature | feature_enabled |
| active_connection | connection_active |
| total_count | count_total |
| current_index | index_current |

### 4. Same Character Count

Related names should have the same length when possible.

| Don't Use | Use Instead |
|-----------|-------------|
| src / dst | source / target |
| from / to | source / target |
| old / new | previous / current |
| min / max | minimum / maximum |

### 5. Type Names

Types are nouns or noun phrases.

```rust
// Correct
struct Request
struct Response
struct IsolatePool
struct WorkQueue

// Incorrect
struct RequestData
struct HandleRequest
struct Pool
```

### 6. Function Names

Functions are verbs or verb phrases.

```rust
// Correct
fn request_handle(request: Request)
fn isolate_acquire(isolate: &Isolate)
fn context_reset(context: &Context)
fn response_build(result: Result)

// Incorrect
fn handle_request()  // TigerStyle prefers verb first
fn acquireIsolate()
fn resetCtx()
fn build_response()
```

Note: TigerStyle differs from Rust conventions here. TigerStyle puts the verb first.

### 7. Constants

Constants are SCREAMING_SNAKE_CASE but still big-endian.

```rust
// Correct
const HEAP_SIZE_BYTES_MAX: u32 = 128 * 1024 * 1024;
const REQUEST_TIMEOUT_MS: u32 = 30_000;
const ISOLATE_POOL_SIZE_MAX: u32 = 10_000;

// Incorrect
const MAX_HEAP_SIZE: u32 = 128 * 1024 * 1024;
const REQUEST_TIMEOUT: u32 = 30_000;
```

### 8. Units in Names

Always include units.

| Don't Use | Use Instead |
|-----------|-------------|
| timeout | timeout_ms |
| delay | delay_us |
| size | size_bytes |
| count | count_items |
| limit | limit_bytes |

### 9. Struct Field Ordering

Order: Fields → Types → Methods

```rust
pub struct Isolate {
    // Fields first (nouns)
    heap_size_bytes_max: u32,
    heap_size_bytes_used: u32,
    context_reset_count: u64,
    
    // Types second
    pub const HeapLimit: u32 = 128 * 1024 * 1024;
    
    // Methods third
    pub fn context_reset(&mut self) { }
}
```

## Common nano-rs Examples

### Before (Non-TigerStyle)

```rust
fn handle_req(req: Request, ctx: &mut Context) -> Result<Response, Error> {
    let max_heap = ctx.max_heap;
    let buf = Vec::with_capacity(req.len);
    // ...
}
```

### After (TigerStyle)

```rust
fn request_handle(request: Request, context: &mut Context) -> Result<Response, Error> {
    let heap_size_bytes_max = context.heap_size_bytes_max;
    let buffer = Vec::with_capacity(request.length);
    // ...
}
```

## Migration Priority

1. **P0 (Critical):** Public API names, function names, constants in limits.rs
2. **P1 (High):** Configuration fields, public struct fields
3. **P2 (Medium):** Internal variable names, private functions
4. **P3 (Low):** Local variables in small functions

## Review Checklist

- [ ] All identifiers snake_case
- [ ] No abbreviations (ctx, req, res, buf, len, idx)
- [ ] Big-endian naming (*_max, *_min at end)
- [ ] Units included (ms, us, bytes)
- [ ] Related names same length
- [ ] Functions verb-first
- [ ] Types are nouns
