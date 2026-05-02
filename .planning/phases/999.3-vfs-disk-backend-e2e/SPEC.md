# Phase 999.3: VFS Disk Backend E2E Tests

## Status: BACKLOG

**Goal:** Fix WASM E2E tests that require disk VFS backend file access

**Context from Phase 27:**
Two E2E tests for WASM CPU timeout functionality were marked as `#[ignore]` because they require reading WASM files through the VFS disk backend, which is not properly wired for per-app configuration.

## Failing Tests

- `test_wasm_cpu_timeout` - WASM infinite loop with file read via Nano.fs
- `test_wasm_within_cpu_limit` - Normal WASM execution with file access

Both tests pass the JS tests (3/5 E2E tests passing) but fail for WASM because:
1. WASM modules need to be read via `Nano.fs.readFile()` from disk backend
2. Current WorkQueue creates pools with MemoryBackend by default
3. Per-app disk VFS backends require async pool creation refactoring

## Technical Details

### File Structure Expected
```
temp_dir/
├── handler.js          # Entrypoint (read directly by runtime)
└── {sanitized_hostname}/  # VFS-accessible directory
    └── add.wasm        # WASM module (read via Nano.fs)
```

### Hostname Sanitization
- `wasm-normal.local` → `wasm_normal_local`
- Dots and hyphens become underscores for filesystem compatibility

### Current Architecture Limitation

The `WorkQueue::get_or_create_pool()` method was originally synchronous, but disk VFS backend creation is async. Phase 27 refactored this to make pool creation async, but the full per-app backend configuration was not completed to avoid scope creep.

## Requirements

### REQ-999-03-01: Per-App Disk VFS Backends
- Each app should be able to specify its own VFS backend type
- Disk backend configuration from config.json must be honored
- Async pool creation must support backend initialization

### REQ-999-03-02: WASM File Access
- WASM files in `{base_path}/{hostname}/` must be accessible via Nano.fs
- File reads must work in E2E test environment
- Path resolution must handle hostname sanitization correctly

## Success Criteria

1. `test_wasm_cpu_timeout` passes without `#[ignore]`
2. `test_wasm_within_cpu_limit` passes without `#[ignore]`
3. All 5 E2E tests pass: 3 JS + 2 WASM
4. No workarounds - proper async architecture implemented

## Dependencies

- Phase 27 complete (identified the issue)
- Phase 999.2 may be prerequisite (WorkerPool architecture decision)

## Implementation Notes

**DO NOT use workarounds.** The proper fix requires:
1. Completing async pool creation architecture
2. Wiring per-app VFS backend configuration from AppRegistry
3. Ensuring worker threads initialize VFS before execution
4. Testing path resolution with sanitized hostnames

## Files Likely to Change

- `src/worker/queue.rs` - Pool creation logic
- `src/http/router.rs` - AppState backend configuration
- `src/http/server.rs` - Registry backend extraction
- `tests/cpu_timeout_e2e_test.rs` - Remove `#[ignore]` from WASM tests
