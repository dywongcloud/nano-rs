# Technical Debt Register

## ESM-01: Module API Execution with Lifetime Management

**Status:** Accepted (Intentional)  
**Created:** 2026-05-02  
**Phase:** 999.4 Pre-existing Technical Debt

### Description

The ESM module execution in `src/v8/module.rs` uses V8's Module API to compile and evaluate modules, but then falls back to classic script execution via code transformation rather than using the evaluated module's exports directly.

### Location
- `src/v8/module.rs:518-542` - `execute_esm_module()` function
- Comment: "ARCHITECTURAL NOTE: Proper ESM execution vs Transformation approach"

### Current Behavior

1. ESM code is parsed and compiled using `v8::script_compiler::compile_module()`
2. Module is instantiated with import resolution callback
3. Module is evaluated with `module.evaluate()`
4. **Fallback:** Code is transformed and executed as classic script
5. Export extraction happens from the re-executed script context

### Why This Is Technical Debt

The Module API evaluation result is essentially discarded. We pay the compilation cost twice:
1. Once for Module API (validates syntax, resolves imports)
2. Once for classic script execution (actual runtime)

### Why It's Accepted

**Lifetime Complexity:** Extracting exports from the evaluated module requires holding references across scope boundaries that violate Rust's borrow checker rules when combined with V8's Local/Global handle system.

**No User Impact:** The transformation approach works correctly for all current use cases:
- Hono.js `export default { fetch }` ✅
- Next.js static exports ✅
- Astro static builds ✅
- Generic WinterCG patterns ✅

**Performance:** The double compilation cost is negligible for the runtime's use case (ms-scale, not µs-scale operations).

### Path to Resolution

**v2.0 Advanced ESM Features (Phase 28)** will implement proper Module API execution:
- Store module namespace as `v8::Global<v8::Object>` to persist across scopes
- Access exports via the global handle instead of re-execution
- Enable advanced features like top-level await, circular imports, dynamic imports

### Decision Record

**DECISION:** Accept transformation approach for v1.x, defer proper Module API execution to v2.0.

**Rationale:**
1. Current implementation satisfies all requirements
2. User-visible behavior is identical
3. Lifetime issues require significant refactoring
4. Risk of introducing bugs exceeds benefit for v1.x
5. Framework compatibility already achieved

**Revisit When:**
- v2.0 planning begins (advanced ESM features)
- Performance profiling shows ESM compilation as bottleneck
- Need for top-level await or circular import support
- Rust/V8 binding improvements simplify lifetime management

### Related Code

```rust
// In src/v8/module.rs:execute_esm_module()
// This function exists but cannot be used due to scope issues:
fn extract_default_export<'s>(
    scope: &'s mut v8::ContextScope<'s, v8::HandleScope<'s>>,
    module: v8::Local<'s, v8::Module>,
) -> Result<(v8::Local<'s, v8::Function>, Option<v8::Local<'s, v8::Object>>)> {
    // ... implementation that works in theory but not across scope boundaries
}
```

---

## SNAP-01: V8 Snapshot Validation

**Status:** Accepted (Limited by upstream API)  
**Created:** 2026-05-02  
**Phase:** 999.4 Pre-existing Technical Debt

### Description

V8 snapshot loading in `src/v8/isolate.rs` has minimal validation (size checks, placeholder detection) rather than comprehensive format validation.

### Location
- `src/v8/isolate.rs:172-182` - `restore_from_snapshot()` function
- Comment: "SAFETY NOTE: Current validation is INTENTIONALLY LIMITED"

### Current Validation

**Implemented:**
1. Size check (< 8 bytes rejected as obviously invalid)
2. Placeholder detection ("NANO_SNAPSHOT_PLACEHOLDER_V1" legacy format)
3. V8 internal validation (rusty_v8's SnapshotBlob validates on load)
4. Graceful fallback (any issue → fresh isolate)

**Not Implemented:**
- Magic number verification
- V8 version compatibility check  
- Checksum/hash validation
- Format structure validation

### Why This Is Technical Debt

Reliance on V8 internal validation and graceful fallback instead of explicit pre-flight validation.

### Why It's Accepted

**API Limitations:** The rusty_v8 crate doesn't expose snapshot validation functions:
- No `SnapshotBlob::IsValid()` 
- No version extraction from raw bytes
- No structured metadata access

**Risk Profile: Very Low**
- Snapshots are internal data, not user input
- Corruption leads to fresh isolate (not crash or undefined behavior)
- Production uses placeholder format (no binary snapshots yet)
- V8's internal validation catches most corruption

**Cost/Benefit:**
- Cost: Medium (requires V8 format knowledge, testing with corrupted data)
- Benefit: Low (marginally earlier error detection, same end result)

### Path to Resolution

**When rusty_v8 exposes validation APIs:**
1. Magic number check (V8 snapshots start with 0xD7 0x3C 0xD7 0x3C)
2. Version compatibility (snapshot V8 vs current V8)
3. Optional: Add SHA-256 checksum to sliver metadata

**No current timeline** — rusty_v8 prioritizes stability over feature coverage.

### Decision Record

**DECISION:** Accept limited validation, rely on V8 internal checks + graceful fallback.

**Rationale:**
1. rusty_v8 API doesn't support explicit validation
2. Current approach is safe (fallback on any issue)
3. Production use case uses placeholder format
4. Real V8 snapshots blocked by rusty_v8 SnapshotCreator API limitations anyway
5. Engineering effort better spent on user-visible features

**Revisit When:**
- rusty_v8 exposes SnapshotBlob::IsValid() or similar
- Production use case shifts to real V8 snapshots
- Security audit identifies snapshot loading as risk area
- Need for cross-version snapshot compatibility arises

### Monitoring

Current metrics:
- Warning logged when non-placeholder snapshot detected
- Fallback to fresh isolate is observable in logs
- No crashes reported from snapshot loading in testing

Watch for:
- Bug reports mentioning "non-placeholder snapshot" warnings
- Issues with isolate restoration from slivers
- rusty_v8 changelog mentioning snapshot validation APIs

### Related Issues

- rusty_v8 Issue #XXX: Snapshot validation API (link when available)
- V8 Snapshot Format: https://v8.dev/docs/snapshot-format
- Phase 14: Snapshot Creation (used placeholder due to API limits)
