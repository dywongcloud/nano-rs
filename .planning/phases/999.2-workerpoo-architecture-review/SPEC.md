# Phase 999.2: WorkerPool Architecture Consolidation

## Status: BACKLOG

**Goal:** Merge or separate duplicate WorkerPool implementations, unify VFS backend lifecycle

**Context from Phase 27:**
The codebase contains two separate WorkerPool implementations that were identified during production multi-tenancy work. This architectural debt complicates VFS backend configuration and creates maintenance burden.

## Current State

### Two WorkerPool Implementations

1. **src/worker/pool.rs** - Complex WorkerPool
   - Full sliver support (snapshot restore)
   - CPU timeout integration
   - Thread-local isolate management
   - Has `with_backend()` method for custom VFS
   - Complex lifecycle management

2. **src/worker/queue.rs** - Simple WorkerPool  
   - Reimplements WorkerPool from scratch
   - Used for entrypoint dispatch
   - Added `with_vfs_config()` in Phase 27
   - Async pool creation support
   - No sliver support

### Problems Identified

1. **Code Duplication:** Two pools don't share code or communicate
2. **VFS Configuration Duplication:** Backend config must be implemented twice
3. **Unclear Usage:** No documentation on which pool to use when
4. **Per-App VFS Difficulty:** Architecture makes app-specific backends complex

## Success Criteria

1. **Decision Made:** Either merged into single pool OR clearly separated with documented responsibilities
2. **Common Trait:** Extract shared WorkerPool trait for interchangeable use
3. **VFS Unification:** Single VFS backend creation path
4. **Documentation:** Clear guidance on pool selection for future development

## Options to Evaluate

### Option A: Merge Pools
- Create unified WorkerPool with all capabilities
- Feature flags for sliver support if needed
- Single VFS configuration path

### Option B: Separate Clearly
- pool.rs becomes "SliverWorkerPool" - only for sliver execution
- queue.rs becomes "EntrypointWorkerPool" - only for entrypoint dispatch
- Extract common WorkerPool trait
- Document when to use each

### Option C: Trait-Based Architecture
- WorkerPool trait with common interface
- Different implementations for different use cases
- Dependency injection for VFS backends

## Dependencies

- Phase 27 complete (identified the issue)
- Architecture review meeting required
- Decision on async pool creation pattern

## Do Not Start Without

- Architecture review meeting
- Decision on merge vs separate
- Approval on chosen approach
