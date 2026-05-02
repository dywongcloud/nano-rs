# Phase 999.4: Pre-existing Technical Debt

## Status: BACKLOG

**Goal:** Address TODOs from previous phases identified during Phase 27 completion

**Important:** These TODOs were NOT introduced in Phase 27. They are pre-existing technical debt from earlier phases that was identified during Phase 27 code review. Each requires its own analysis and phase planning.

## TODO Items

### TODO 1: RSA and ECDSA Algorithm Properties

**Location:** `src/runtime/apis.rs:1821`
**Text:** "RSA and ECDSA algorithms - TODO: add specific properties"

**Context:**
- Part of the Web Crypto API implementation
- Asymmetric crypto algorithms (RSA, ECDSA) work but lack complete property handling
- Found in crypto key generation and operation code

**Impact:** Low
- Algorithms function correctly
- Properties are incomplete but not blocking

**Analysis Needed:**
- What specific properties are missing?
- Are they required for spec compliance?
- Which algorithms affected (RSA-PSS, RSA-OAEP, ECDSA P-256, P-384)?

---

### TODO 2: Proper ESM Execution with Lifetime Management

**Location:** `src/v8/module.rs:522`
**Text:** "TODO: Implement proper ESM execution with correct lifetime management"

**Context:**
- ESM module system uses V8 Module API infrastructure
- Current implementation uses transformation approach for immediate framework compatibility
- Full Module API execution with proper lifetimes not yet implemented

**Impact:** Medium
- Current transformation approach works (Hono.js, Next.js run)
- Proper Module API would be cleaner architecture
- May be needed for advanced ESM features (top-level await, etc.)

**Analysis Needed:**
- What lifetime issues block proper implementation?
- Is transformation approach sufficient for v1?
- What features require full Module API?

---

### TODO 3: VFS list_dir() Method

**Location:** `src/sliver/mod.rs:90`
**Text:** "TODO: Add list_dir() method to VfsBackend trait for full implementation"

**Context:**
- VFS trait has read_file, write_file, exists, etc.
- Directory listing method missing
- Sliver operations use walk_vfs() as workaround

**Impact:** Low
- walk_vfs() works for directory traversal
- list_dir() would be cleaner for certain operations
- Not blocking any current functionality

**Analysis Needed:**
- Which operations need list_dir() vs walk_vfs()?
- Should return iterator or Vec?
- Error handling for permission denied?

---

### TODO 4: V8 Snapshot Validation

**Location:** `src/v8/isolate.rs:176`
**Text:** "TODO: Implement proper V8 snapshot validation and loading"

**Context:**
- V8 snapshots are used for fast context creation
- Currently loaded without validation
- Could be security/stability issue with corrupted snapshots

**Impact:** Low
- Snapshots work correctly in practice
- Validation would add safety margin
- Corrupted snapshots would likely crash anyway

**Analysis Needed:**
- What validation does V8 provide?
- Checksum, version, or format validation?
- Performance impact of validation?

## Recommendation

These TODOs should be evaluated individually and either:
1. **Promoted to active phases** - If they block features or represent significant technical debt
2. **Resolved as quick fixes** - If they're small and can be done in <1 day
3. **Left as low-priority** - If impact is truly low and workarounds suffice

## Do Not Start Without

- Individual analysis of each TODO
- Prioritization against other roadmap items
- Decision on whether to fix or accept as permanent workarounds
