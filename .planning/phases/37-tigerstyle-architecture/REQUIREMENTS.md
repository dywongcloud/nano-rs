# Phase 37 Requirements: TigerStyle Architecture Adoption

**Phase:** 37  
**Goal:** Apply TigerBeetle's TigerStyle methodology for safety, performance, and correctness  
**Milestone:** v1.6 Architecture Hardening  

---

## Overview

This phase adopts TigerBeetle's TigerStyle engineering methodology to harden the nano-rs runtime for production use. TigerStyle emphasizes static memory allocation, comprehensive assertions, explicit resource limits, minimal dependencies, and clear control/data plane separation.

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
**Priority:** High  
**Description:** Address all 17 TODO/FIXME items in codebase  

**Acceptance Criteria:**
- All `TODO` comments replaced with real implementations or removed
- All `FIXME` comments resolved or converted to tracked issues
- No placeholder functions (functions that panic or return dummy values)
- No unimplemented!() macros in production code paths
- All stubs have clear paths to completion or are removed

**TODO Categories to Address:**
1. **Implement** - Add real implementation (most TODOs)
2. **Document** - Convert to documented intentional debt
3. **Remove** - Delete if no longer relevant
4. **Defer** - Move to GitHub issues for future work

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
| TIGER-08 | 37 | High | 0 TODOs remaining | TODO count verification |

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

1. All 8 TIGER requirements implemented and verified
2. Documentation updated with TigerStyle rationale
3. No regressions in existing test suite
4. Performance benchmarks show improvement or neutrality
5. Code review passed with TigerStyle checklist
6. Architecture Decision Record created for TigerStyle adoption

---

## References

- [TigerBeetle TigerStyle](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md)
- [TigerBeetle Code Review Guide](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/CODE_REVIEW.md)
- [TigerBeetle Design Principles](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/DESIGN.md)
