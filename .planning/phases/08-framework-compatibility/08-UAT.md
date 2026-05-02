---
status: complete
phase: 08-framework-compatibility
source: 08-01-SUMMARY.md, 08-02-SUMMARY.md
started: 2026-04-21T10:28:00Z
updated: 2026-04-21T10:35:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Hono.js App Serving
expected: Hono.js app with fetch handler serves requests correctly
result: pass
notes: 3/3 Hono tests passed. test_hono_cors_headers_on_all_routes, test_hono_middleware_chain_order verified.

### 2. Next.js Static Export
expected: Next.js static export app serves HTML/JS/CSS assets correctly
result: pass
notes: 6/6 Next.js tests passed after fix. Response headers now correctly extracted from both plain objects and Headers instances.

### 3. Astro Islands App
expected: Astro islands architecture app serves server-rendered HTML with hydration
result: pass
notes: 6/6 Astro tests passed. test_astro_island_hydration_strategy_markers, test_astro_server_rendered_content verified.

## Summary

total: 3
passed: 3
issues: 0
pending: 0
skipped: 0
blocked: 0

## Gaps

[none]

## Fix Applied

**Issue:** Response headers not extracted when Response created via `new Response()`
**Root Cause:** `extract_js_response()` didn't handle Headers class instances (which store data in `__headers__` internal property)
**Fix:** Updated `extract_js_response()` in both `src/runtime/handler.rs` and `src/worker/pool.rs` to:
1. First check for `__headers__` internal property (Headers class instances)
2. Fall back to direct property iteration (plain objects)
3. Skip function properties to avoid adding methods as headers
**Files Changed:** src/runtime/handler.rs, src/worker/pool.rs
