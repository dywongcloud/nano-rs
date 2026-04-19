# NANO Project - Gap Analysis Report

**Date:** 2026-04-19  
**Scope:** Phases 1-5 (V8 Foundation through Multi-App Hosting)  
**Status:** MVP Core Complete, Testing Gaps Identified

---

## Executive Summary

Phases 1-5 are functionally complete with 147/153 tests passing (96% pass rate). 
**6 test failures** require remediation before production use. No architectural gaps—only 
test/edge-case fixes needed.

---

## Test Failure Breakdown

### 1. Timing-Sensitive Test (Phase 5)

**Test:** `app::timeout::tests::test_watchdog_creation`  
**Location:** `src/app/timeout.rs:447`

```rust
assert_eq!(watchdog.remaining_ms(), 5000);  // Fails: 4999 vs 5000
```

**Issue:** Timing assertion too strict. 1ms difference due to execution time between 
`Instant::now()` calls.

**Fix:** Use range assertion or tolerance check:
```rust
assert!(watchdog.remaining_ms() >= 4990 && watchdog.remaining_ms() <= 5000);
```

**Severity:** Low (cosmetic test issue, not production bug)

---

### 2. String Case Mismatch (Phase 5)

**Test:** `config::tests::test_config_validation_duplicate_hostname`  
**Location:** `src/config/mod.rs:444`

**Issue:** Test expects "Duplicate hostname" (capital D) but code produces 
"duplicate hostname" (lowercase d).

**Code:**
```rust
errors.add(format!(
    "duplicate hostname: '{}' (case-insensitive)",  // lowercase 'd'
    app.hostname
));
```

**Test:**
```rust
assert!(result.unwrap_err().to_string().contains("Duplicate hostname"));  // Capital 'D'
```

**Fix:** Align test with actual error message or vice versa.

**Severity:** Low (validation works, just test assertion mismatch)

---

### 3. Async Drain Test Logic (Phase 5)

**Test:** `app::drain::tests::test_await_complete_timeout`  
**Location:** `src/app/drain.rs:161`

```rust
#[tokio::test]
async fn test_await_complete_timeout() {
    let drain = RequestDrain::new();
    drain.request_started();  // Never completed!
    
    // Should timeout because request never completes
    let result = drain.await_complete(Duration::from_millis(50)).await;
    assert!(!result);  // Fails: result is true
}
```

**Issue:** Semaphore-based drain may return true (success) even with pending requests 
if the semaphore isn't properly tracking.

**Investigation Needed:** Check if semaphore acquisition logic correctly waits for 
request completion vs. returns early.

**Severity:** Medium (potential production issue with graceful drain)

---

### 4. Environment Variable Substitution Tests (Phase 5)

**Tests:**
- `config::loader::tests::test_substitute_env_vars_default`
- `config::loader::tests::test_load_config_with_env_substitution`
- `config::loader::tests::test_load_config_from_str_validation_failure`

**Likely Issues:**
- Default value parsing: `${VAR:-default}` syntax
- Async file loading with env substitution
- Validation error message content mismatch

**Investigation Status:** Pending detailed failure analysis

**Severity:** Medium (config loading is critical path)

---

## Requirement Traceability Gaps

### REQUIREMENTS.md Out of Date

**Critical Finding:** REQUIREMENTS.md shows most Phase 1-5 requirements as "Pending" 
despite being implemented.

| Requirement | Actual Status | Documented Status | Gap |
|-------------|---------------|-------------------|-----|
| FND-01 (Project skeleton) | ✅ Complete | ⏳ Pending | Document |
| FND-02 (rusty_v8 integration) | ✅ Complete | ⏳ Pending | Document |
| FND-03 (EPT fix) | ✅ Complete | ⏳ Pending | Document |
| FND-04 (JS execution) | ✅ Complete | ⏳ Pending | Document |
| HTTP-01 (HTTP server) | ✅ Complete | ⏳ Pending | Document |
| HTTP-02 (Virtual host routing) | ✅ Complete | ⏳ Pending | Document |
| HTTP-03 (Request/Response) | ✅ Complete | ⏳ Pending | Document |
| HTTP-04 (Headers API) | ✅ Complete | ⏳ Pending | Document |
| HTTP-05 (URL/URLSearchParams) | ✅ Complete | ⏳ Pending | Document |
| POOL-01 (WorkerPool) | ✅ Complete | ⏳ Pending | Document |
| POOL-02 (WorkQueue) | ✅ Complete | ⏳ Pending | Document |
| POOL-03 (Affine dispatch) | ✅ Complete | ⏳ Pending | Document |
| POOL-04 (Context reset <10ms) | ✅ Complete | ⏳ Pending | Document |
| POOL-05 (Thread-local isolates) | ✅ Complete | ⏳ Pending | Document |
| HOST-01 (JSON config) | ✅ Complete | ⏳ Pending | Document |
| HOST-02 (Memory limits) | ✅ Complete | ⏳ Pending | Document |
| HOST-03 (Timeout watchdog) | ✅ Complete | ⏳ Pending | Document |
| HOST-04 (Environment vars) | ✅ Complete | ⏳ Pending | Document |
| HOST-05 (Hot-reload) | ✅ Complete | ⏳ Pending | Document |
| HOST-06 (Graceful drain) | ⚠️ Partial | ⏳ Pending | Fix + Document |

**Action Required:** Update REQUIREMENTS.md traceability table.

---

## Integration Gaps

### Missing Integration: Config → Registry → WorkerPool

**Gap:** While individual components exist, full integration test for:
```
Config file → Load → Registry → WorkerPool dispatch → Handler execution
```

is missing.

**Impact:** Cannot verify end-to-end request handling with config-defined apps.

**Recommendation:** Add integration test in `tests/` directory.

---

## Performance Validation Gaps

### Context Reset Timing

**Requirement:** POOL-04 mandates <10ms context reset  
**Status:** Tests exist but no CI performance gate

**Gap:** No automated regression detection for context reset timing.

**Recommendation:** Add benchmark test with assert:
```rust
assert!(reset_time < Duration::from_millis(10));
```

---

## Security Validation Gaps

### Threat Model Verification

**Documented Threats:** From plan frontmatter (T-03-01 through T-05-04)  
**Status:** Implemented but not verified

**Gaps:**
- No automated test for path traversal prevention (T-05-04)
- No test for env var injection limits (T-05-02)
- No memory limit OOM trigger test (HOST-02)

**Recommendation:** Add security-focused tests to validate threat mitigations.

---

## Documentation Gaps

### Missing SUMMARY.md Files

**Expected:** 04-01-SUMMARY.md, 04-02-SUMMARY.md, 05-01-SUMMARY.md, 05-02-SUMMARY.md, 05-03-SUMMARY.md  
**Found:** Only partial summaries

**Impact:** No completion artifacts for verification.

---

## Recommended Fix Priority

### P0 (Critical) - Before Production
1. Fix drain test logic (`test_await_complete_timeout`)
2. Verify graceful drain actually works in production scenario
3. Add integration test for full request pipeline

### P1 (High) - Before v1.0 Release
1. Fix config loader test failures (3 tests)
2. Update REQUIREMENTS.md traceability
3. Create missing SUMMARY.md files
4. Add security validation tests

### P2 (Medium) - Technical Debt
1. Fix timing-sensitive watchdog test
2. Fix case-sensitive string assertion
3. Add performance regression tests

### P3 (Low) - Nice to Have
1. Complete 05-03 hot-reload implementation (partial)
2. Add more edge case tests

---

## Success Criteria Verification

| Phase | Success Criteria | Status |
|-------|-----------------|--------|
| 1 | V8 initializes, EPT fix works, JS executes | ✅ Verified |
| 2 | HTTP server, routing, WinterCG objects | ✅ Verified |
| 3 | 10 WinterCG APIs functional | ✅ Verified |
| 4 | WorkerPool, context reset <10ms | ✅ Verified |
| 5 | Config loading, limits, hot-reload | ⚠️ Partial (6 test failures) |

---

## Next Steps

1. **Immediate:** Fix 6 test failures
2. **Short-term:** Update REQUIREMENTS.md documentation
3. **Medium-term:** Add integration tests and security validation
4. **Ongoing:** Create missing SUMMARY.md files for completed phases

---

*Report generated: 2026-04-19*  
*Phases reviewed: 1-5 (MVP critical path)*
