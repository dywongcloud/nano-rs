# Phase 21: v1.2.0 Remediation Completion - Master Plan

**Phase:** 21  
**Milestone:** v1.2 Remediation  
**Goal:** Fix remaining 8 failing tests to reach 90%+ score for production release  
**Current Score:** 84% (42/50 tests passing)  
**Target Score:** 90%+ (45+/50 tests passing)  
**Date:** 2026-04-21  

---

## Executive Summary

### The Problem

We have 8 remaining failing tests preventing v1.2.0 from reaching production-ready 90%+ score:
- 3 VFS tests (6% impact)
- 3 WinterCG tests (6% impact)  
- 1 Timer test (2% impact)
- 1 SHA-256 test (2% impact)

### The Solution

Execute 6 focused plans to fix all 8 issues:

| Plan | Goal | Impact | Tests Fixed |
|------|------|--------|-------------|
| 21-01 | VFS JavaScript bindings | +6% | 3 tests |
| 21-02 | Headers API | +2% | 1 test |
| 21-03 | URL API | +2% | 1 test |
| 21-04 | Streams API | +2% | 1 test |
| 21-05 | Timer functions | +2% | 1 test |
| 21-06 | SHA-256 + Verification | +2% | 1 test |

**Total Impact:** 84% → 90%+

---

## Execution Strategy

### Wave 1: High Impact (VFS)
**Execute First:** Plan 21-01
- **Why:** Fixes 3 tests at once (6% score)
- **Effort:** 2-3 hours
- **Result:** Score jumps to 90%

### Wave 2: WinterCG APIs
**Execute Second:** Plans 21-02, 21-03 (can be parallel)
- **Why:** Fix Headers and URL APIs
- **Effort:** 1-2 hours each
- **Result:** Score reaches 92-94%

### Wave 3: Advanced APIs
**Execute Third:** Plan 21-04
- **Why:** Streams are more complex
- **Effort:** 3-4 hours
- **Result:** Score reaches 96%

### Wave 4: Final Fixes
**Execute Last:** Plans 21-05, 21-06
- **Why:** Lower impact but needed for completeness
- **Effort:** 1-2 hours each
- **Result:** Score reaches 98%+

---

## Sub-Plans

### Plan 21-01: VFS JavaScript Bindings
**File:** `21-01-PLAN.md`

**Goal:** Implement `Nano.fs.writeFile`, `Nano.fs.readFile`, and `require('fs')`

**Key Changes:**
- Wire VFS context into worker thread execution
- Add Node.js fs module polyfill
- Test all 3 VFS operations

**Files Modified:**
- `src/worker/queue.rs` - Add VFS context setup
- `src/runtime/vfs_js.rs` - Add fs polyfill
- `src/runtime/apis.rs` - Verify binding call

**Success Criteria:**
- [ ] `Nano.fs.writeFile()` works
- [ ] `Nano.fs.readFile()` works
- [ ] `require('fs')` returns polyfill
- [ ] 3 VFS tests pass

---

### Plan 21-02: WinterCG Headers API
**File:** `21-02-PLAN.md`

**Goal:** Fix Headers class methods (`get`, `set`, `has`, `delete`, `entries`)

**Key Changes:**
- Create Headers constructor with prototype
- Implement all Headers methods
- Update Request creation to use Headers instance

**Files Modified:**
- `src/runtime/headers.rs` (new) - Headers implementation
- `src/runtime/apis.rs` - Add binding
- `src/worker/queue.rs` - Use Headers instance

**Success Criteria:**
- [ ] `new Headers()` works
- [ ] `headers.get()` works
- [ ] `headers.set()` works
- [ ] Headers test passes

---

### Plan 21-03: WinterCG URL API
**File:** `21-03-PLAN.md`

**Goal:** Fix URL `searchParams` and methods

**Key Changes:**
- Create URLSearchParams class
- Implement URLSearchParams methods
- Update URL constructor to create searchParams

**Files Modified:**
- `src/runtime/url.rs` - Add URLSearchParams
- `src/runtime/apis.rs` - Add binding

**Success Criteria:**
- [ ] `url.searchParams` is URLSearchParams instance
- [ ] `searchParams.get()` works
- [ ] `searchParams.set()` works
- [ ] URL test passes

---

### Plan 21-04: Streams API
**File:** `21-04-PLAN.md`

**Goal:** Implement ReadableStream and WritableStream (basic)

**Key Changes:**
- Create ReadableStream with getReader()
- Create ReadableStreamDefaultReader with read()
- Create WritableStream with getWriter()
- Create WritableStreamDefaultWriter with write()/close()

**Files Modified:**
- `src/runtime/streams.rs` (new) - All stream classes
- `src/runtime/apis.rs` - Add binding

**Success Criteria:**
- [ ] `new ReadableStream()` works
- [ ] `readable.getReader()` works
- [ ] `reader.read()` returns {value, done}
- [ ] Streams test passes

---

### Plan 21-05: Timer Functions
**File:** `21-05-PLAN.md`

**Goal:** Implement `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`

**Key Changes:**
- Create timer state management
- Implement setTimeout with delay
- Implement setInterval with repeat
- Implement clear functions

**Files Modified:**
- `src/runtime/timers.rs` (new) - Timer implementation
- `src/runtime/apis.rs` - Add binding

**Success Criteria:**
- [ ] `setTimeout()` returns timer ID
- [ ] `setInterval()` returns interval ID
- [ ] `clearTimeout()` stops timer
- [ ] Timers test passes

---

### Plan 21-06: SHA-256 and Final Verification
**File:** `21-06-PLAN.md`

**Goal:** Fix SHA-256 and verify 90%+ score

**Key Changes:**
- Check if SHA-256 is already implemented
- Add SHA-256 if missing
- Run full test suite
- Verify 90%+ score achieved

**Files Modified:**
- `src/crypto/subtle.rs` - Add/fix SHA-256

**Success Criteria:**
- [ ] SHA-256 digest works
- [ ] Score reaches 90%+
- [ ] All 8 targeted tests pass
- [ ] Ready for v1.2.0 release

---

## Execution Commands

### Sequential Execution (Recommended)

```bash
# Execute each plan in order
cd /Users/gleicon/code/rust/nano-rs

# Wave 1: VFS (Highest impact)
# Follow 21-01-PLAN.md
cargo build --release
# Test: Score should reach 90%

# Wave 2: WinterCG APIs
# Follow 21-02-PLAN.md and 21-03-PLAN.md
cargo build --release
# Test: Score should reach 92-94%

# Wave 3: Streams
# Follow 21-04-PLAN.md
cargo build --release
# Test: Score should reach 96%

# Wave 4: Final fixes
# Follow 21-05-PLAN.md and 21-06-PLAN.md
cargo build --release
# Test: Score should reach 98%+
```

### Verification After Each Plan

```bash
cd /Users/gleicon/code/js/nano-rs-test-suite
cp /Users/gleicon/code/rust/nano-rs/target/release/nano-rs bin/nano-rs
rm -f *.sliver
node tests/harness.js 2>&1 | grep -E "(Score|Total|Passed)"
```

---

## Dependencies

**Internal Dependencies:**
- Phase 20 (Sliver VFS Integration) ✅ COMPLETE
- Existing VFS in `src/vfs/`
- Existing runtime APIs in `src/runtime/`

**External Dependencies:**
- `sha2` crate (likely already in Cargo.toml)
- `tokio` for timers (already used)

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| VFS thread-safety issues | Medium | High | Use existing patterns |
| Streams complexity | Medium | Medium | Basic implementation only |
| Timer callback execution | Low | Low | API existence is priority |
| Score doesn't reach 90% | Low | High | VFS alone gives +6% |

---

## Success Criteria (Phase 21 Completion)

**Must be TRUE:**

1. ✅ Score reaches 90%+ (45+/50 tests passing)
2. ✅ All VFS operations work (3/3 tests passing)
3. ✅ WinterCG Headers API works (1/1 test passing)
4. ✅ WinterCG URL API works (1/1 test passing)
5. ✅ Streams API functional (1/1 test passing)
6. ✅ Timer functions available (1/1 test passing)
7. ✅ SHA-256 hashing works (1/1 test passing)
8. ✅ No regressions in existing passing tests
9. ✅ Binary builds successfully
10. ✅ v1.2.0 production ready

---

## Post-Phase 21

After completing Phase 21:

### Immediate Actions
1. Update documentation (Phase 22)
2. Create v1.2.0 release notes
3. Tag release
4. Archive v1.2 milestone

### What's Next (v2.0)
- Phase 23: WebSocket Server
- Phase 24: Advanced Crypto (RSA, ECDSA)
- Phase 25: Compression Streams
- Phase 26: Inter-Isolate Messaging

---

## Summary

**Phase 21 Goal:** Fix 8 remaining tests, reach 90%+ score

**Strategy:** 6 focused plans, highest impact first

**Expected Result:** 84% → 90%+ (production ready)

**Timeline:** 1-2 days of focused implementation

**Outcome:** v1.2.0 production release

---

**Let's execute and ship v1.2.0!** 🚀

