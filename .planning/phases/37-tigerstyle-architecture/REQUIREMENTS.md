# Phase 37 Requirements: TigerStyle Architecture Adoption

**Phase:** 37  
**Goal:** Apply TigerBeetle's TigerStyle methodology for safety, performance, and correctness  
**Milestone:** v1.6 Architecture Hardening  

---

## Overview

This phase adopts TigerBeetle's TigerStyle engineering methodology to harden the nano-rs runtime for production use. TigerStyle emphasizes static memory allocation, comprehensive assertions, explicit resource limits, minimal dependencies, and clear control/data plane separation.

## Critical Policy: Zero Technical Debt

> **"Code, like steel, is easier to change while it's hot. Do it right the first time, the best you know how, because you may not get another chance, and because quality builds momentum."** — TigerStyle

**DECISION: This phase will NOT accumulate technical debt.**

- All placeholders MUST be fixed with production-ready implementations
- NO partial features — if it can't be completed now, it doesn't ship now
- NO "TODO: Implement in Phase X" — either implement it or remove it
- NO documentation-only decisions for broken code — fix it or delete it
- Every commit in this phase leaves the codebase cleaner, safer, and more complete

**Consequence of This Policy:**
- Some features may be removed temporarily until properly implemented
- Implementation time may be longer than stubbing
- The codebase will be in a known-good state at every commit
- Future work will build on solid foundations, not accumulated debt

---

## Requirements

### TIGER-01: Static Memory Allocation
**Priority:** Critical  
**Description:** Pre-allocate all isolate and buffer pools at startup with fixed sizes  

**Acceptance Criteria:**
- All isolate pools allocated once at runtime startup with fixed heap sizes
- Buffer pools (request/response bodies, VFS buffers) pre-allocated with configurable limits
- No runtime allocations in the hot path (request handling)
- Memory usage is predictable and bounded at startup

**Related Files:**
- `src/worker/pool.rs`
- `src/worker/queue.rs`
- `src/runtime/buffer_pool.rs` (new)

---

### TIGER-02: Comprehensive Assertions
**Priority:** High  
**Description:** Minimum 2 assertions per function in safety-critical code  

**Acceptance Criteria:**
- Every public function has at least 2 assertions (input validation, state checks)
- Safety-critical paths (isolate creation, request handling, VFS operations) have 3+ assertions
- Assertions use `debug_assert!` for development, `assert!` for critical invariants
- No `unwrap()` or `expect()` in production code paths (converted to assertions + error handling)

**Assertion Categories:**
1. Precondition assertions (input validation, state requirements)
2. Postcondition assertions (result validation, state changes)
3. Invariant assertions (internal consistency checks)

**Related Files:**
- All `src/` modules
- `src/sandbox/` (critical)
- `src/v8/` (critical)
- `src/vfs/` (critical)

---

### TIGER-03: Explicit Resource Limits
**Priority:** High  
**Description:** Fixed u32 limits for all resources with no dynamic growth  

**Acceptance Criteria:**
- All resource limits defined as `const` u32 values in configuration
- WorkQueue has hard size limit with bounded channels (crossbeam bounded)
- Per-app limits: max_isolates, max_memory_mb, max_cpu_ms, max_request_size_kb
- Global limits: max_total_isolates, max_total_memory_mb, max_concurrent_requests
- Limits enforced at startup (fail fast if config exceeds bounds)

**Resource Types:**
- Isolates (per-app pool + global pool)
- Memory (per-isolate heap + per-request body)
- CPU time (per-request execution limit)
- Request/response bodies (size limits)
- VFS operations (file count, size limits)

**Related Files:**
- `src/config.rs`
- `src/limits.rs` (new)
- `src/worker/pool.rs`

---

### TIGER-04: Zero Dependencies Audit
**Priority:** Medium  
**Description:** Review all 30+ Cargo dependencies and create reduction plan  

**Acceptance Criteria:**
- Complete audit of all Cargo.toml dependencies
- Document purpose, size, and alternatives for each dependency
- Identify candidates for removal or replacement with std library
- Create prioritization plan for dependency reduction
- Target: reduce production dependencies by 20-30%

**Audit Categories:**
1. **Essential** - Required for core functionality (v8, tokio, axum)
2. **Replaceable** - Can be replaced with std or smaller crates
3. **Optional** - Feature-gated, not required for core
4. **Remove** - Unused or redundant dependencies

**Related Files:**
- `Cargo.toml`
- `Cargo.lock`
- `docs/DEPENDENCY_AUDIT.md` (new)

---

### TIGER-05: Control/Data Plane Separation
**Priority:** High  
**Description:** Clear separation between control operations and data handling  

**Acceptance Criteria:**
- Control plane (config loading, pool management, admin API) uses aggressive assertions
- Data plane (request handling, JS execution) optimized for throughput
- No control operations in hot data paths
- Batching used for control operations affecting multiple isolates
- Clear documentation of which operations belong to which plane

**Control Plane Operations:**
- App registration/deregistration
- Isolate pool resize
- Configuration updates
- Metrics collection
- Admin API requests

**Data Plane Operations:**
- HTTP request handling
- JavaScript execution
- VFS read/write
- Response streaming

**Related Files:**
- `src/server/admin.rs`
- `src/app_registry.rs`
- `src/worker/pool.rs`
- `src/runtime/` (data plane)

---

### TIGER-06: Function Size Limits
**Priority:** Medium  
**Description:** Hard 70-line limit per function  

**Acceptance Criteria:**
- No function exceeds 70 lines (excluding comments and blank lines)
- Functions exceeding limit are refactored into smaller sub-functions
- Each function has single, clear responsibility
- Helper functions are private and well-named
- Complex logic split into logical steps with intermediate functions

**Exception Categories:**
- Match statements with many arms (allowed if each arm is 1-2 lines)
- Test functions (allowed to be longer if needed)
- Generated code (prost, macros)

**Related Files:**
- All `src/` modules

---

### TIGER-07: Naming Convention Alignment
**Priority:** Low  
**Description:** Follow TigerStyle naming conventions throughout codebase  

**Acceptance Criteria:**
- All identifiers use `snake_case`
- Big-endian fields explicitly named with `_be` suffix (e.g., `size_be`)
- Constants use `SCREAMING_SNAKE_CASE`
- No abbreviations in public APIs (except well-known: HTTP, URL, V8, VFS)
- Function names are verb phrases (e.g., `allocate_buffer`, not `buffer_alloc`)
- Type names are noun phrases (e.g., `IsolatePool`, not `PoolIsolates`)

**Naming Conventions:**
- Variables: `snake_case`
- Functions: `snake_case` (verb first)
- Types: `PascalCase` (noun phrases)
- Constants: `SCREAMING_SNAKE_CASE`
- Modules: `snake_case`

**Related Files:**
- All `src/` modules

---

### TIGER-08: Mock/Placeholder Removal
**Priority:** Critical  
**Description:** ELIMINATE all 17 TODO/FIXME items and 5 placeholder implementations. NO partial fixes. NO documentation-only decisions. NO growing technical debt.  

**⚠️ CRITICAL DECISION: All Placeholders Must Be Fixed**
Following TigerStyle's "zero technical debt" policy, every placeholder must be either:
1. **Fully implemented** with production-ready code
2. **Completely removed** with feature disabled until ready

There is NO option 3. No stubs, no mocks, no "Phase X" promises in code.

**Acceptance Criteria:**
- [ ] ZERO `TODO` comments in production code (src/, not tests/)
- [ ] ZERO `FIXME` comments in production code
- [ ] ZERO placeholder functions (panics, dummy returns, "Phase 3" strings)
- [ ] ZERO `unimplemented!()` macros in production paths
- [ ] ZERO stub WebAssembly API (real implementation or remove)
- [ ] ZERO placeholder heap in sliver packager (real heap capture or fail)
- [ ] ZERO router placeholder returning "JS handler (Phase 3)" (real routing or fail)
- [ ] ZERO mock timeout responses in HTTP client (real timeout or fail)

**Placeholders to Fix (MUST be production-ready or removed):**

| Location | Current State | Required Fix |
|----------|---------------|--------------|
| `src/wasm/js_api.rs:35-66` | Stub WebAssembly object | Real WAS API or remove WASM support |
| `src/sliver/packager.rs:166-180` | Placeholder heap data | Real V8 heap capture or error |
| `src/v8/module.rs:514-522` | VFS placeholder in loader | Real VFS reference passing |
| `src/http/router.rs` | Returns "JS handler (Phase 3)" | Real route dispatch or panic |
| `src/admin/unix_socket.rs:274` | Unix socket placeholder | Real Unix socket or remove feature |
| `src/http/client.rs:419` | Mock timeout response | Real timeout handling |

**TODOs to Resolve:**
- `src/runtime/fetch.rs:143` - Unused fields: either USE them or REMOVE them
- `src/worker/queue.rs:335` - CPU timeout future: either IMPLEMENT now or REMOVE comment
- `src/cli/error.rs:134` - Re-enable helpers: either FIX or DELETE the code
- `src/v8/isolate.rs:186` - Legacy placeholder: either SUPPORT legacy or ERROR on detection

**Related Files:**
- All `src/` files with TODO/FIXME comments
- `docs/TECHNICAL_DEBT.md`

---

## Traceability Matrix

| Requirement | Phase | Priority | Success Criteria | Verification |
|-------------|-------|----------|------------------|--------------|
| TIGER-01 | 37 | Critical | Static allocation working | Runtime startup tests |
| TIGER-02 | 37 | High | 2+ assertions per function | Assertion count analysis |
| TIGER-03 | 37 | High | Hard limits enforced | Limit enforcement tests |
| TIGER-04 | 37 | Medium | Audit document complete | DEPENDENCY_AUDIT.md |
| TIGER-05 | 37 | High | Separation documented | Architecture review |
| TIGER-06 | 37 | Medium | No functions >70 lines | Line count check |
| TIGER-07 | 37 | Low | Naming consistent | Naming audit |
| TIGER-08 | 37 | **Critical** | **ZERO placeholders remaining** | grep verification + test suite |

---

## Dependencies

- **Phase 36** (v1.5 Test Infrastructure Remediation) - Must be complete
- **All prior phases** - Runtime must be functional and tested

---

## Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Static allocation limits flexibility | Medium | Medium | Make limits configurable at startup |
| Assertion overhead in production | Low | Low | Use `debug_assert!` where possible |
| Function refactoring breaks logic | Medium | High | Comprehensive tests before refactoring |
| Dependency removal breaks features | Medium | Medium | Test each removal individually |
| TODO resolution reveals deep issues | Medium | High | Prioritize critical TODOs first |

---

## Definition of Done

### Hard Requirements (Must ALL Pass)

1. **All 8 TIGER requirements implemented and verified**
2. **ZERO placeholders remaining** — grep -r "TODO\|FIXME\|placeholder\|stub\|mock" src/ returns nothing
3. **ZERO unimplemented!() macros** in production code paths
4. **NO functions** that panic or return dummy values as "temporary" solutions
5. **All tests passing** — no regressions, no "expected failures"
6. **Documentation updated** with TigerStyle rationale and policy
7. **Performance benchmarks** show improvement or neutrality (no degradation accepted)
8. **Code review passed** with TigerStyle checklist
9. **Architecture Decision Record** created documenting the zero-debt policy

### Verification Commands

```bash
# Verify no TODOs in production code
grep -r "TODO\|FIXME" src/ --include="*.rs" | grep -v "test" | wc -l
# Expected output: 0

# Verify no unimplemented!() macros
grep -r "unimplemented!" src/ --include="*.rs" | wc -l
# Expected output: 0

# Verify no placeholder patterns
grep -r "placeholder\|stub\|mock" src/ --include="*.rs" | grep -v "// Mock" | wc -l
# Expected output: 0

# Verify all tests pass
cargo test --all 2>&1 | tail -1
# Expected output: test result: ok. NNN tests passed
```

---

## References

- [TigerBeetle TigerStyle](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md)
- [TigerBeetle Code Review Guide](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/CODE_REVIEW.md)
- [TigerBeetle Design Principles](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/DESIGN.md)
