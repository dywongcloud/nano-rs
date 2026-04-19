# Phase 01: V8 Foundation - Context

**Gathered:** 2026-04-19  
**Status:** Ready for planning

<domain>
## Phase Boundary

 rusty_v8 integration with EPT fix, single isolate proof-of-concept. This phase establishes the V8 foundation that all subsequent phases build upon.

**Success Criteria (from ROADMAP.md):**
1. `cargo build` produces binary using pre-built rusty_v8 (no V8 compilation)
2. Platform initializes with strong v8::Global sentinel per isolate (EPT fix prevents SIGSEGV)
3. JavaScript `console.log("hello")` executes and prints to stdout
4. Isolate can be created and disposed without memory leaks or crashes

</domain>

<decisions>
## Implementation Decisions

### Project Structure
- **D-01:** Single crate with modules (not Cargo workspace)
  - Modules: `src/v8/` (V8 integration), `src/runtime/` (JS APIs), `src/http/` (HTTP server)
  - Rationale: Simpler for initial development, can split to workspace later if needed

### EPT Fix Strategy
- **D-02:** Strong v8::Global sentinel per isolate
  - Create one `v8::Global<Value>` per isolate at initialization
  - Hold sentinel until isolate disposal
  - Prevents `array_buffer_sweeper_space` EPT segment unmapping bug
  - This is the proven fix from the Zig version's AP-02 issue

### V8 Initialization Pattern
- **D-03:** Deno-style platform via rusty_v8
  - Use `v8::Platform` from rusty_v8 crate
  - Standard initialization sequence: `v8::V8::initialize_platform()` → `v8::Isolate::new()`
  - Follow patterns from deno_core for HandleScope nesting

### HandleScope Pattern
- **D-04:** Nested HandleScope pattern for memory safety
  - Create HandleScope before V8 operations
  - Drop HandleScope before long-lived operations
  - Prevents unbounded memory growth (reference: rusty_v8 issue #481)

### Thread Safety
- **D-05:** Thread-local isolate enforcement
  - Isolates are never moved between threads
  - Use `std::sync::Once` for platform initialization
  - Reference: rusty_v8 issue #1467 (Isolate Send is unsound)

### Dependencies
- **D-06:** Core dependencies locked
  - `v8` = rusty_v8 crate (pre-built V8)
  - `tokio` = async runtime (preparation for Phase 4)
  - `tracing` = structured logging (preparation for Phase 7)

### Testing
- **D-07:** Test strategy for this phase
  - Unit tests for V8 operations in `src/v8/`
  - Integration test: JavaScript execution and console output
  - Memory leak test: isolate create/dispose cycle

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### V8 and rusty_v8
- `.planning/research/STACK.md` — Technology stack recommendations
- `.planning/research/PITFALLS.md` — Critical pitfalls including EPT SIGSEGV and HandleScope
- `https://docs.rs/v8/latest/v8/` — rusty_v8 API documentation
- `https://github.com/denoland/rusty_v8` — rusty_v8 source and examples

### Project Context
- `.planning/PROJECT.md` — Core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` — FND-01 through FND-04 requirements
- `.planning/ROADMAP.md` — Phase 1 goal and success criteria
- `AGENTS.md` — Build commands and constraints

### EPT Fix Reference
- `prod.md` §AP-02 — Original EPT SIGSEGV bug description from Zig version
- `.planning/STATE.md` — Critical technical debt note on EPT fix

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- No existing code — greenfield project
- Can reference Zig version patterns from prod.md but not copy code

### Established Patterns
- From research: Deno/deno_core patterns for V8 integration
- From research: Nested HandleScope pattern is mandatory
- From research: Thread-local isolate ownership is enforced

### Integration Points
- Phase 2 (HTTP Server Core) will integrate with the isolate created here
- Phase 3 (Runtime APIs) will add JS bindings to this foundation
- Phase 4 (WorkerPool) will replicate this pattern across threads

</code_context>

<specifics>
## Specific Ideas

**EPT Fix Implementation Detail:**
Create a strong `v8::Global<v8::Value>` sentinel immediately after isolate creation. The sentinel holds a reference that keeps the `array_buffer_sweeper_space` EPT segment mapped, preventing the background GC from unmapping it between requests. This is the same fix pattern from the Zig version but implemented with rusty_v8's `v8::Global` type.

**Module Organization:**
```
src/
  main.rs          # Entry point, binary
  lib.rs           # Library exports (for testing)
  v8/
    mod.rs         # V8 module public API
    platform.rs    # Platform initialization
    isolate.rs     # Isolate creation/disposal with EPT fix
    context.rs     # Context creation/disposal
  runtime/
    mod.rs         # Runtime module (placeholder for Phase 3)
  http/
    mod.rs         # HTTP module (placeholder for Phase 2)
```

**First JavaScript to Execute:**
```javascript
console.log("hello from nano v8 isolate");
```

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within Phase 1 scope.

**Future phase notes:**
- Phase 2: HTTP server integration with isolate
- Phase 3: console.log implementation (actual binding)
- Phase 4: Multi-threaded WorkerPool (replicate this pattern)

</deferred>

---

*Phase: 01-v8-foundation*  
*Context gathered: 2026-04-19*
