# Phase 37: Zero Technical Debt Policy — DECISION REGISTER

**Date:** 2026-05-11  
**Phase:** 37 TigerStyle Architecture Adoption  
**Decision ID:** PHASE37-DEC-001  
**Status:** ✅ REGISTERED

---

## The Decision

**ALL placeholders and incomplete features MUST be fully fixed in Phase 37.**

There will be:
- ✅ NO partial implementations
- ✅ NO "temporary" solutions  
- ✅ NO documentation-only decisions
- ✅ NO "TODO: Fix in Phase X" comments
- ✅ NO stubs, mocks, or placeholders left in production code

Each placeholder will be either:
1. **Fully implemented** with production-ready, tested code
2. **Completely removed** with the feature disabled until properly implemented

There is no option 3.

---

## Why This Decision

Following TigerBeetle's TigerStyle methodology:

> **"Code, like steel, is easier to change while it's hot. Do it right the first time, the best you know how, because you may not get another chance, and because quality builds momentum."**

### The Broken Windows Theory Applied to Code

Placeholders signal that broken code is acceptable. This erodes quality standards across the entire codebase. Every placeholder we leave behind:
- Accepts technical debt as normal
- Encourages future shortcuts
- Breaks user trust when features don't work
- Creates a culture of "ship now, fix later"

**Phase 37 draws a hard line: Zero Technical Debt.**

---

## Placeholders Targeted for Elimination

| # | Location | Current State | Required Fix |
|---|----------|---------------|--------------|
| 1 | `src/http/router.rs` | Returns "JS handler (Phase 3)" text | **FIX:** Wire to WorkerPool for real JS execution |
| 2 | `src/v8/module.rs:514` | Placeholder VFS in module loader | **FIX:** Pass real VFS reference |
| 3 | `src/wasm/js_api.rs:35-66` | Stub WebAssembly object | **FIX:** Real WASM API or **REMOVE** feature |
| 4 | `src/sliver/packager.rs:126-171` | Placeholder heap data | **FIX:** Real heap capture or **REMOVE** cold sliver support |
| 5 | `src/http/client.rs:419` | Mock timeout responses | **FIX:** Real timeout handling |
| 6 | `src/admin/unix_socket.rs:274` | Unix socket placeholder | **FIX:** Real implementation or **REMOVE** admin socket |

Plus all TODO/FIXME comments in production code (`src/` directory).

---

## Verification Criteria

Phase 37 will only be marked complete when ALL of the following return **ZERO**:

```bash
# Placeholder check
grep -r "placeholder\|Placeholder\|PLACEHOLDER" src/ --include="*.rs" | wc -l
# Expected: 0

# TODO check  
grep -r "TODO\|FIXME\|XXX\|HACK" src/ --include="*.rs" | wc -l
# Expected: 0

# Macro check
grep -r "todo!\|unimplemented!" src/ --include="*.rs" | wc -l
# Expected: 0

# Mock check
grep -r "mock\|Mock\|MOCK" src/ --include="*.rs" | grep -v "test" | wc -l
# Expected: 0

# Test result
cargo test --all 2>&1 | grep "test result"
# Expected: test result: ok. ALL tests passed
```

---

## Files Updated with This Decision

1. `.planning/phases/37-tigerstyle-architecture/REQUIREMENTS.md` — Added Critical Policy section
2. `.planning/phases/37-tigerstyle-architecture/37-08-PLAN.md` — Mandated elimination, removed "document" option
3. `.planning/ROADMAP.md` — Added zero-debt policy banners throughout
4. This file — `PHASE37-ZERO-DEBT-POLICY.md` — Decision register

---

## What This Means for Contributors

### ✅ ACCEPTABLE
- Fully implemented, tested features
- Clean code with no TODO markers
- Honest documentation about what works
- Features disabled until properly implemented

### ❌ UNACCEPTABLE (Will Block Commits)
- Placeholder implementations that return dummy values
- TODO comments marking "future work"
- Mock implementations in production code
- Features advertised but non-functional
- Partial implementations with "Phase X" promises

---

## Success State

When Phase 37 completes:
- Every feature advertised in README actually works
- Every function has a complete, tested implementation
- grep for TODOs returns nothing
- All tests pass (no skipped, no ignored, no "expected failures")
- Users never see placeholder text like "JS handler (Phase 3)"

**This is the TigerStyle way. This is the nano-rs way from Phase 37 onward.**

---

*Registered by: AI Assistant*  
*Policy Authority: TigerStyle Methodology*  
*Enforcement: Phase 37 success criteria verification*
