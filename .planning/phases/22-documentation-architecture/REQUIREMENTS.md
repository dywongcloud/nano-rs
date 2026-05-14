# Phase 22: Documentation & Architecture Requirements

**Phase ID:** 22
**Name:** Documentation & Architecture
**Milestone:** v1.4.2 Code Cleanup
**Date:** 2026-05-02

---

## REQ-22-01: Cold Start Measurement and Documentation Correction

**Priority:** High  
**Category:** Documentation

### Description
The current README claims "~267µs cold starts via sliver snapshots" but the documentation is confusing about what "cold start" means. Need to:

1. Measure actual cold start times (isolate creation from sliver)
2. Measure warm start times (context reset between requests)
3. Distinguish between process boot time vs request handling time
4. Update all documentation to be precise about these metrics

### Acceptance Criteria
- [ ] Cold start from sliver measured and documented with methodology
- [ ] Context reset time measured and documented separately
- [ ] Process boot time measured separately
- [ ] All README claims updated with accurate numbers
- [ ] ARCHITECTURE.md updated with performance characteristics
- [ ] PERFORMANCE.md created with detailed benchmarks

### Notes
Current README says: "One OS process hosts multiple isolated apps with ~267µs cold starts via sliver snapshots"

This should clarify:
- Sliver restoration: ~267µs
- Context reset (warm): ~5ms
- Process boot: ~60ms (one-time)
- Fresh isolate creation: ~50-100ms

---

## REQ-22-02: README Feature Set Accuracy

**Priority:** High  
**Category:** Documentation

### Description
The current README has an "Executive Summary" but some feature claims may not match actual implementation. Need to audit and fix:

1. Verify all "100% Complete" claims against actual test results
2. Update Node.js compatibility table to reflect actual implemented APIs
3. Ensure version number matches actual releases
4. Remove or qualify claims about unimplemented features

### Acceptance Criteria
- [ ] Audit all feature status claims against actual tests
- [ ] Node.js compatibility matrix reflects reality (not just "100% Complete")
- [ ] WinterTC API documentation matches actual implementation
- [ ] WebCrypto documentation lists all supported algorithms
- [ ] README version number matches ROADMAP

### Notes
Current issues:
- README says "v1.5.0" but ROADMAP shows v1.1 shipped, v1.2 in remediation
- Some claims like "100% Complete" for all APIs may need qualification
- Need to document what "WinterTC compatibility" actually means (subset of APIs)

---

## REQ-22-03: Architecture Decision Records (ADRs)

**Priority:** Medium  
**Category:** Documentation

### Description
Create proper Architecture Decision Records for key technical choices:

1. EPT fix for SIGSEGV prevention (strong Global sentinel)
2. Context reset vs full isolate recreation
3. WorkerPool with thread-local isolates
4. VFS abstraction layer
5. Ring crate for crypto instead of V8
6. Sliver format (tar-based, opaque heap)
7. Transformation-based ESM vs full Module API

### Acceptance Criteria
- [ ] docs/ADR/ directory created
- [ ] ADR template established
- [ ] ADR-001: EPT Fix and V8 Integration
- [ ] ADR-002: Context Reset Architecture
- [ ] ADR-003: Thread-Local Isolate Model
- [ ] ADR-004: VFS Architecture and Backends
- [ ] ADR-005: Crypto Implementation Strategy
- [ ] ADR-006: Sliver Snapshot Format
- [ ] ADR-007: ESM Module Execution Strategy

### Notes
ADRs should follow standard format:
- Context (why we needed this decision)
- Decision (what we decided)
- Consequences (trade-offs, what we gave up)
- Status (accepted, superseded)

---

## REQ-22-04: Public API Documentation

**Priority:** Medium  
**Category:** Documentation

### Description
Generate and publish API documentation for:

1. Nano.* JavaScript globals (Nano.fs, etc.)
2. Runtime configuration options
3. Admin API endpoints
4. CLI commands and flags

### Acceptance Criteria
- [ ] docs/API.md with all JavaScript globals documented
- [ ] CLI documentation with all commands
- [ ] Admin API reference with endpoints, request/response formats
- [ ] Configuration schema documented
- [ ] Examples for each public API

### Notes
Currently API docs are scattered:
- Nano.fs mentioned in README
- Admin API partially documented in ARCHITECTURE.md
- CLI commands in SLIVER.md and EXAMPLES.md
- No single reference location

---

## REQ-22-05: Cold Start Metrics - Process Boot vs Request Latency

**Priority:** High  
**Category:** Documentation / Performance

### Description
Distinguish between different startup/latency metrics that are currently conflated:

1. **Process boot time**: Time from binary execution to HTTP server ready
2. **Cold start (sliver)**: Time from sliver file to first request served
3. **Warm start (context reset)**: Time between requests in same isolate
4. **Fresh isolate**: Time to create new isolate without sliver

### Acceptance Criteria
- [ ] All documentation uses precise terminology
- [ ] PROCESS_BOOT.md documenting ~60ms boot time
- [ ] COLD_START.md documenting sliver restoration (~267µs)
- [ ] All graphs/benchmarks labeled clearly
- [ ] No ambiguous "cold start" claims without context

### Notes
The confusion likely stems from:
- Process boot (~60ms) vs sliver cold start (~267µs) are very different
- "Cold start" in cloud context usually means process + runtime init
- Our "267µs" is just the sliver restoration part

---

## REQ-22-06: Node.js Compatibility Matrix

**Priority:** Medium  
**Category:** Documentation

### Description
Create accurate Node.js compatibility documentation:

1. Which Node.js APIs are polyfilled
2. Which work differently than Node.js
3. Migration guides for common patterns
4. Package compatibility notes

### Acceptance Criteria
- [ ] docs/NODEJS_COMPAT.md with accurate compatibility info
- [ ] Percentage-based compatibility score (not "100% Complete")
- [ ] Migration guide from Node.js to nano-rs
- [ ] Common gotchas and workarounds documented

### Notes
Current README says "Node.js Compatibility (100% Complete)" which is misleading.
We actually have:
- Buffer polyfill (partial)
- fs polyfill via VFS (partial, async only)
- setTimeout/setInterval (basic)
- Many Node.js APIs NOT implemented

---

## Dependencies

- Phase 21.2 (Critical Bug Fixes) - must be complete for accurate feature claims
- Phase 27 (Production Multi-Tenancy) - for CPU/memory metrics documentation

---

## Success Criteria

1. **REQ-22-01:** Documentation claims match actual measured cold start times with methodology documented
2. **REQ-22-02:** README accurately reflects current feature set with no "100%" claims without proof
3. **REQ-22-03:** 7 ADRs documenting key architecture decisions with trade-offs
4. **REQ-22-04:** Complete API documentation for JavaScript globals, CLI, Admin API
5. **REQ-22-05:** Cold start terminology standardized across all docs
6. **REQ-22-06:** Node.js compatibility matrix published with realistic percentages

---

## Plan Mapping

| Plan | Requirements | Description |
|------|--------------|-------------|
| 22-01 | REQ-22-01, REQ-22-05 | Cold start measurement and performance documentation correction |
| 22-02 | REQ-22-02 | README audit and feature set accuracy update |
| 22-03 | REQ-22-03 | Architecture Decision Records (ADRs) |
| 22-04 | REQ-22-04, REQ-22-06 | API documentation and Node.js compatibility |
