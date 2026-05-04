# NANO-RS Architecture Unification Plan

## Current State (Problematic)

```
┌─────────────────────────────────────────────────────────────┐
│                     TWO DIFFERENT ENGINES                      │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────────────┐    ┌──────────────────────┐         │
│  │   Config Mode      │    │   Sliver Mode        │         │
│  │   (queue.rs)       │    │   (pool.rs)          │         │
│  ├──────────────────────┤    ├──────────────────────┤         │
│  │ EntrypointWorkerPool │    │ WorkerPool           │         │
│  │ - Spawns threads   │    │ - Spawns threads     │         │
│  │ - Reads JS files   │    │ - Reads JS files     │         │
│  │ - No sliver support│    │ + Sliver support     │         │
│  │ - Different logs   │    │ + Eviction mgr       │         │
│  │                    │    │ + Memory monitoring  │         │
│  └──────────────────────┘    └──────────────────────┘         │
│           │                           │                       │
│           └───────────┬───────────────┘                       │
│                       │                                       │
│              SHOULD BE ONE                                    │
└─────────────────────────────────────────────────────────────┘
```

## Problems with Current Design

1. **Code Duplication**: Two separate worker pool implementations
   - `src/worker/queue.rs:EntrypointWorkerPool` - 1028 lines
   - `src/worker/pool.rs:WorkerPool` - 2274 lines
   - Similar logic duplicated: worker spawning, task dispatch, JS execution

2. **Feature Inconsistency**:
   - Sliver mode has: eviction, memory monitoring, CPU limits, isolate_id tracking
   - Config mode lacks: all of the above (only basic execution)

3. **Testing Complexity**: Two different test paths needed
   - Tests for queue.rs path
   - Tests for pool.rs path
   - Different behavior in each

4. **Logging Differences**: worker_id/isolate_id handling differs between paths

5. **Maintenance Burden**: Bug fixes need to be applied in two places

## Target Architecture (Unified)

```
┌─────────────────────────────────────────────────────────────┐
│                   SINGLE UNIFIED ENGINE                      │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              UnifiedWorkerPool                      │   │
│  │  (One implementation, all features)                  │   │
│  ├──────────────────────────────────────────────────────┤   │
│  │ Features (ALL app types get these):                  │   │
│  │ ✓ Worker threads with isolates                     │   │
│  │ ✓ Sliver support (can load from .sliver files)     │   │
│  │ ✓ Config support (can load from .js entrypoints) │   │
│  │ ✓ Memory monitoring & eviction                     │   │
│  │ ✓ CPU time limits                                  │   │
│  │ ✓ Tracing: request_id + worker_id + isolate_id     │   │
│  │ ✓ VFS for all (static + code artifacts)            │   │
│  └──────────────────────────────────────────────────────┘   │
│                          │                                    │
│         ┌────────────────┼────────────────┐                   │
│         │                │                │                   │
│    ┌────▼────┐     ┌────▼────┐     ┌────▼────┐              │
│    │ Static  │     │   JS    │     │  WASM   │              │
│    │ Sites   │     │  Apps   │     │  Apps   │              │
│    └─────────┘     └─────────┘     └─────────┘              │
│                                                               │
│  All three types:                                            │
│  - Run in isolates                                           │
│  - Can be slivered (packaged)                                │
│  - Get same tracing/logging                                  │
│  - Use same VFS for artifacts                                │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Consolidate Worker Pool (Critical)

**Goal**: Merge `EntrypointWorkerPool` (queue.rs) into `WorkerPool` (pool.rs)

**Steps**:
1. Extend `WorkerPool` to support non-sliver entrypoints
   - Add `entrypoint: Option<String>` to worker initialization
   - If no sliver, load JS from filesystem/VFS
   
2. Update `WorkQueue` to use `WorkerPool` internally
   - Instead of `EntrypointWorkerPool`, spawn `WorkerPool` with config
   
3. Remove `EntrypointWorkerPool` entirely
   - Move any unique logic to `WorkerPool`
   - Delete `queue.rs` or make it a thin wrapper

**Code Changes**:
```rust
// Current (duplicated):
// queue.rs: EntrypointWorkerPool::spawn_worker() - 150 lines
// pool.rs: WorkerPool::spawn_worker() - 200 lines

// Target (unified):
// pool.rs: WorkerPool::spawn_worker(entrypoint_or_sliver: Source)
```

### Phase 2: Unified VFS for All Artifacts

**Current**:
- Sliver mode: VFS for static files + code artifacts in sliver
- Config mode: Filesystem for code, separate static handling

**Target**:
- ALL modes: Unified VFS containing:
  - Code artifacts (JS, WASM files)
  - Static artifacts (HTML, CSS, images)
  - Both loaded from sliver OR filesystem at startup

**Implementation**:
```rust
pub struct AppPackage {
    pub code: HashMap<String, Vec<u8>>,     // JS, WASM files
    pub static_files: HashMap<String, Vec<u8>>, // HTML, CSS, images
    pub metadata: AppMetadata,
}

// Loaded from sliver file OR built from filesystem
```

### Phase 3: Unified Tracing

**Current**: 
- Sliver mode: worker_id + isolate_id + request_id logged
- Config mode: Only request_id (inconsistent)

**Target**:
- ALL modes: Full tracing combo
  - `request_id`: HTTP request correlation
  - `worker_id`: OS thread identifier  
  - `isolate_id`: V8 isolate instance hash

### Phase 4: Test Consolidation

**Delete redundant tests**:
- Tests specific to queue.rs path
- Tests that duplicate pool.rs functionality

**Keep unified tests**:
- One test suite per feature (not per path)
- All tests use unified WorkerPool

## Migration Path

### Step 1: Extend WorkerPool (Week 1)
- [ ] Add `load_from_entrypoint()` method to WorkerPool
- [ ] Test with existing sliver tests (should still pass)
- [ ] Add entrypoint tests (previously in queue.rs)

### Step 2: Update WorkQueue (Week 1-2)  
- [ ] Modify WorkQueue to spawn WorkerPool instead of EntrypointWorkerPool
- [ ] Pass entrypoint path to WorkerPool
- [ ] Test both sliver and config scenarios

### Step 3: Deprecate EntrypointWorkerPool (Week 2)
- [ ] Mark EntrypointWorkerPool as deprecated
- [ ] Add warnings if used
- [ ] Migrate all internal usage to WorkerPool

### Step 4: Remove queue.rs Worker Implementation (Week 3)
- [ ] Delete EntrypointWorkerPool struct and methods
- [ ] Keep queue.rs only for routing/dispatch logic
- [ ] Update all imports

### Step 5: Documentation Update (Week 3)
- [ ] Document unified architecture
- [ ] Update all examples to show single path
- [ ] Remove dual-path documentation

## Benefits

1. **50% Less Code**: ~1000 lines of duplication removed
2. **Single Source of Truth**: One place for worker logic
3. **Consistent Features**: All apps get memory/CPU limits
4. **Easier Testing**: One test suite covers all scenarios  
5. **Simpler Maintenance**: Bug fixes in one place
6. **Better Logging**: All requests have full trace context

## Files to Modify

### Core Unification
- `src/worker/pool.rs` - Extend to support entrypoints
- `src/worker/queue.rs` - Remove worker impl, keep dispatch
- `src/worker/mod.rs` - Clean up exports

### Tracing/Logging  
- `src/logging/` - Already unified (good!)

### Tests
- `tests/crud_operations_test.rs` - Rewrite to use unified API
- `tests/*_test.rs` - Remove queue.rs specific tests

### Documentation
- `docs/ARCHITECTURE.md` - Document unified design
- `EXAMPLES.md` - Show single path examples

## Verification Criteria

- [ ] `cargo test` passes with no queue.rs-specific tests
- [ ] Sliver tests still pass (backwards compatibility)
- [ ] Config tests pass (migrated to pool.rs path)
- [ ] All logs show worker_id + isolate_id + request_id
- [ ] Code coverage increases (less duplication)
- [ ] Binary size decreases (less code)

