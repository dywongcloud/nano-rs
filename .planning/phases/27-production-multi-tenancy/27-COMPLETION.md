# Phase 27 Completion Report

## Summary
Phase 27 (Production Multi-Tenancy) delivered CPU timeout enforcement, WebAssembly support, memory monitoring, and E2E testing infrastructure. All compilation errors resolved, 622 library tests passing, 3/5 E2E tests passing. Final fix: CPU time limits now properly wired from app config to execution.

## Post-Session Fix (2025-05-02)
**Issue:** `cpu_time_limit_ms: 0` was hardcoded in router.rs with TODO comment  
**Solution:** Added `AppRegistry` to `AppState`, implemented `get_cpu_time_limit_ms()` to lookup limits from config  
**Files:** `src/http/router.rs`, `src/http/server.rs`  
**Status:** Fixed, tested, committed

## Files Changed

### Configuration & Build
| File | Changes |
|------|---------|
| `Cargo.toml` | Added `futures = "0.3"` dev-dependency for E2E test parallel requests |
| `src/config/mod.rs` | Fixed dual AppLimits struct - added `cpu_time_ms` and `cpu_time_enabled` fields |

### Worker Pool & VFS Architecture
| File | Changes |
|------|---------|
| `src/worker/queue.rs` | Added VFS backend configuration support: `WorkQueue::with_vfs_config()`, `WorkerPool::with_backend()`, imports for `VfsDiskConfig`, `VfsBackendType` |
| `src/http/router.rs` | Added `AppState::with_vfs_config()` for disk backend configuration; Added `AppRegistry` integration and `get_cpu_time_limit_ms()` |
| `src/http/server.rs` | Updated `start_server_with_config()` to extract and pass disk VFS config and AppRegistry |

### Test Infrastructure
| File | Changes |
|------|---------|
| `src/worker/queue.rs` | Fixed HandlerTask test helpers - added `cpu_time_limit_ms: 0` |
| `src/app/registry.rs` | Fixed test - added CPU fields to AppLimits |
| `tests/config_mode_test.rs` | Fixed tests - added CPU fields to two AppLimits instances |
| `tests/cpu_timeout_e2e_test.rs` | Complete rewrite with NanoProcess helper, dynamic ports, proper file structure |

### Planning
| File | Changes |
|------|---------|
| `.planning/ROADMAP.md` | Added backlog phases 999.1 and 999.2 |
| `.planning/phases/999.1-adversarial-security-tests/` | Created backlog phase directory |
| `.planning/phases/999.2-workerpoo-architecture-review/` | Created backlog phase directory |

## Test Results

```
Library Tests:     622 passed
Integration Tests: 379 passed
E2E Tests:          3/5 passed
```

### Passing E2E Tests
- `test_js_cpu_timeout` - JavaScript infinite loop terminated within 500ms
- `test_js_within_cpu_limit` - Normal JS execution completes successfully
- `test_cpu_limit_per_isolate` - Per-isolate CPU limits enforced correctly

### Pending E2E Tests (Architecture Limitation)
- `test_wasm_cpu_timeout` - WASM file access through VFS (needs lazy pool creation with app-specific backends)
- `test_wasm_within_cpu_limit` - WASM execution with file access (same limitation)

## Architecture Debt Identified

### Issue 1: Duplicate WorkerPool Implementations
**Location:** `src/worker/pool.rs` vs `src/worker/queue.rs`
- Two separate WorkerPool structs with different capabilities
- No shared trait or common interface
- pool.rs has sliver support, CPU timeout, more complex lifecycle
- queue.rs reimplements simpler version for entrypoint dispatch

**Impact:** VFS backend configuration must be implemented twice, maintenance burden

**Resolution:** Backlogged as Phase 999.2

### Issue 2: Per-App VFS Backend Configuration
**Current State:** WorkQueue uses single VFS backend for all pools
**Required:** Lazy pool creation with app-specific VFS configurations
**Blocker:** WorkQueue::get_or_create_pool() is synchronous, disk backend creation is async

**Resolution:** Backlogged as Phase 999.3

### Issue 3: Pre-existing Technical Debt
**Location:** Various TODOs in codebase (not from Phase 27)
- src/runtime/apis.rs:1821 - RSA/ECDSA algorithm properties
- src/v8/module.rs:522 - Proper ESM execution with lifetimes  
- src/sliver/mod.rs:90 - VFS list_dir() method
- src/v8/isolate.rs:176 - V8 snapshot validation

**Impact:** Low to Medium - workarounds exist for all items

**Resolution:** Backlogged as Phase 999.4 for individual evaluation

## Key Design Decisions

1. **CPU Timeout Architecture:** Timer thread signals termination, main thread calls `v8::Isolate::terminate_execution()` - required because V8 isolates cannot be shared between threads

2. **E2E Test Port Allocation:** Dynamic port finding with `TcpListener::bind("127.0.0.1:0")` to avoid TIME_WAIT conflicts

3. **VFS File Structure:** Entrypoints at temp directory root (read directly by runtime), VFS-accessible files in `{sanitized_hostname}/` subdirectory

4. **Hostname Sanitization:** Dots and hyphens become underscores for filesystem compatibility (e.g., `wasm-normal.local` → `wasm_normal_local`)

## Backlog Items (GSD Format)

### Phase 999.1: Adversarial Security Testing Suite
**Goal:** Security gateway test suite for adversarial attacks and CVE monitoring  
**Requirements:** Research CVE databases, design attack scenarios, implement test harness  
**Plans:** 0 plans

**Scope:**
- CPU exhaustion attacks (infinite loops, pathological regex)
- Memory exhaustion attacks (large allocations, leaks)
- VFS escape attempts (path traversal, symlink attacks)
- Network-based attacks (DNS rebinding, request flooding)
- JavaScript injection via input validation bypasses
- WebAssembly validation bypasses and malicious modules
- Multi-tenant isolation breaches
- Cryptographic attacks (weak keys, timing attacks)

**CVE Research:**
- V8 engine vulnerabilities
- Rust async runtime issues
- HTTP parsing libraries (axum, hyper)
- VFS path sanitization bypasses
- WebAssembly runtime exploits

**Makefile Targets:**
- `make test-security` - Run adversarial tests
- `make test-cve-check` - Scan dependencies against CVE databases
- `make test-all` - Run all test suites

### Phase 999.2: WorkerPool Architecture Consolidation
**Goal:** Merge or separate duplicate WorkerPool implementations, unify VFS backend lifecycle  
**Requirements:** Architecture review of pool.rs vs queue.rs, trait extraction, VFS unification  
**Plans:** 0 plans

**Proposed Actions:**
- Extract common WorkerPool trait
- Merge or clearly separate responsibilities
- Unify VFS backend creation and lifecycle
- Implement lazy pool creation with app-specific configs
- Document pool selection criteria for each scenario

### Phase 999.3: VFS Disk Backend E2E Tests
**Goal:** Fix WASM E2E tests requiring disk VFS backend file access  
**Requirements:** REQ-999-03-01, REQ-999-03-02  
**Failing Tests:** `test_wasm_cpu_timeout`, `test_wasm_within_cpu_limit`  
**Root Cause:** Per-app disk VFS backends not fully wired for async pool creation  
**Plans:** 0 plans

**Success Criteria:**
- Both WASM E2E tests pass without `#[ignore]`
- All 5 E2E tests passing (3 JS + 2 WASM)
- No workarounds - proper async architecture

### Phase 999.4: Pre-existing Technical Debt
**Goal:** Address TODOs from previous phases identified during Phase 27  
**Items:** 4 TODOs in runtime/apis.rs, v8/module.rs, sliver/mod.rs, v8/isolate.rs  
**Impact:** Low to Medium (workarounds exist)  
**Plans:** 0 plans

**Note:** These TODOs were NOT introduced in Phase 27. They are pre-existing technical debt from earlier phases.

## Commits

1. `Phase 27 completion: CPU timeout enforcement, WASM support, and production multi-tenancy` - Initial completion
2. `Phase 27: Complete E2E test framework with NanoProcess helper` - E2E improvements
3. `Phase 27: VFS disk backend configuration plumbing` - Architecture fixes
4. `Add backlog items: adversarial security tests and WorkerPool architecture review` - Backlog setup
5. `Add GSD backlog phases: 999.1 adversarial security tests, 999.2 WorkerPool architecture consolidation` - GSD format

## Verification

```bash
# All library tests pass
cargo test --lib

# E2E tests available (3/5 pass)
cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1

# Security tests ready for Phase 999.1
# Architecture work queued for Phase 999.2
# VFS disk backend E2E tests in Phase 999.3
# Pre-existing technical debt in Phase 999.4
```
