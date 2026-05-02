# Phase 21: v1.2.0 Remediation Completion - Context

**Phase:** 21  
**Goal:** Fix remaining 8 failing tests to reach 90%+ score for v1.2.0 production release  
**Current Score:** 84% (42/50 tests passing)  
**Target Score:** 90%+ (45+/50 tests passing)  
**Date:** 2026-04-21  

---

## Remaining Failures (8 Tests)

### VFS Issues (3 tests) - Priority: HIGH
These are the biggest blocker - 6% of the test score

1. **VFS: Nano.fs.writeFile** - File write operations not working
2. **VFS: Nano.fs.readFile** - File read operations not working  
3. **VFS: Node.js fs module compatibility** - Node.js fs polyfill not available

**Root Cause Analysis:**
- VFS bindings exist in `src/runtime/vfs_js.rs` but aren't properly wired to worker threads
- The thread-local VFS context is not set in the WorkQueue execution path
- Need to inject VFS into JS context and set up proper bindings

**Impact:** +6% score (42% → 48% of remaining failures)

### WinterCG API Issues (3 tests) - Priority: MEDIUM

4. **WinterCG: Headers API** - Headers class methods not accessible in handlers
5. **WinterCG: URL API** - URL search params and methods not working
6. **WinterCG: ReadableStream/WritableStream** - Stream APIs not implemented

**Root Cause Analysis:**
- Headers API: Need to implement proper Headers prototype with methods like `get()`, `set()`, `has()`
- URL API: URL searchParams and some methods not accessible
- Streams: ReadableStream/WritableStream not implemented at all

**Impact:** +6% score

### Node.js Timer Issue (1 test) - Priority: LOW

7. **Node.js: setTimeout/setInterval** - Timer functions not available

**Root Cause Analysis:**
- Timer functions not injected into the JS global scope
- Need to implement timer API using V8 bindings

**Impact:** +2% score

### WebCrypto Issue (1 test) - Priority: LOW

8. **WebCrypto: SHA-256 hashing** - SHA-256 digest not working

**Root Cause Analysis:**
- May be a test issue or actual missing implementation
- Need to verify if SHA-256 is in the existing crypto.subtle implementation

**Impact:** +2% score

---

## Implementation Strategy

### Order of Execution

**Wave 1: VFS (Highest Impact)**
- Plan 21-01: VFS JavaScript bindings and integration
- This fixes 3 tests = +6% score

**Wave 2: WinterCG Headers & URL**
- Plan 21-02: Headers API implementation
- Plan 21-03: URL API fixes
- This fixes 2 tests = +4% score

**Wave 3: Streams**
- Plan 21-04: ReadableStream/WritableStream
- This fixes 1 test = +2% score

**Wave 4: Timers & SHA-256**
- Plan 21-05: Timer functions (setTimeout/setInterval)
- Plan 21-06: SHA-256 and final verification
- This fixes 2 tests = +4% score

---

## Success Criteria

**Must be TRUE for Phase 21 completion:**

1. ✅ Score reaches 90%+ (45+/50 tests passing)
2. ✅ All VFS operations work (3/3 tests passing)
3. ✅ WinterCG Headers API works (1/1 test passing)
4. ✅ WinterCG URL API works (1/1 test passing)
5. ✅ Streams API functional (1/1 test passing)
6. ✅ Timer functions available (1/1 test passing)
7. ✅ SHA-256 hashing works (1/1 test passing)
8. ✅ No regressions in existing passing tests

---

## Key Files

### VFS Integration
- `src/runtime/vfs_js.rs` - VFS JavaScript bindings
- `src/worker/queue.rs` - Worker execution, needs VFS context setup
- `src/runtime/apis.rs` - Runtime API injection point

### WinterCG APIs
- `src/runtime/headers.rs` - Headers implementation (may need creation)
- `src/runtime/url.rs` - URL implementation fixes
- `src/runtime/streams.rs` - Streams implementation (needs creation)

### Timers
- `src/runtime/timers.rs` - Timer implementation (needs creation)

### WebCrypto
- `src/crypto/subtle.rs` - Check SHA-256 implementation

---

## Dependencies

- Phase 20 (Sliver VFS Integration) ✅ COMPLETE
- Existing VFS infrastructure in `src/vfs/`
- Existing WinterCG Request/Response in `src/runtime/`

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| VFS thread-safety issues | Medium | High | Use existing VFS patterns, test thoroughly |
| Streams implementation complexity | Medium | Medium | Start with basic implementation, expand later |
| Timer integration with V8 event loop | Low | Medium | Use existing tokio runtime |
| SHA-256 already implemented, test issue | High | Low | Verify existing implementation first |

---

## Notes

- VFS is the biggest win - 3 tests for likely similar effort to 1 test for other features
- Current VFS exists but isn't wired to JS execution context
- Streams are complex but can be basic implementation for v1.2.0
- Timers are straightforward but less critical
- Focus on VFS first for maximum impact

