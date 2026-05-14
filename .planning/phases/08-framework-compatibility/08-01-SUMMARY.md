---
phase: 08-framework-compatibility
plan: 01
subsystem: test
summary: "Hono.js and Generic WinterTC test applications with integration tests verifying framework compatibility"
dependency_graph:
  requires: ["07-complete"]
  provides: ["framework-compat-tests"]
tech_stack:
  added: [rusty_v8, JavaScript fixtures]
  patterns: [test fixtures, integration tests, ES6 module transformation]
key_files:
  created:
    - tests/fixtures/frameworks/hono-app.js
    - tests/fixtures/frameworks/generic-wintertc-app.js
    - tests/framework_compat_test.rs
    - tests/hono_integration_test.rs
  modified:
    - src/runtime/handler.rs (ES6 module transformation, Promise resolution)
    - src/runtime/apis.rs (Response, URL, Headers constructors)
decisions:
  - ES6 export default syntax must be transformed for V8 Script execution
  - RuntimeAPIs must be bound before handler execution
  - Promise resolution required for async fetch handlers
  - Headers object must be converted from plain object to Headers instance
metrics:
  duration: "~45 minutes"
  tests_passed: 10
  fixtures_created: 2
  apis_added: 3
---

# Phase 8 Plan 1: Hono.js & Generic WinterTC Test Apps Summary

## Overview

Created test applications and integration tests verifying Hono.js-style and generic WinterTC app compatibility with NANO's runtime. Successfully tested the `export default { fetch }` pattern with middleware, routing, and WinterTC APIs.

## What Was Built

### Test Fixtures

1. **hono-app.js** - Hono.js-style app with:
   - Middleware chain (logger + CORS)
   - Router with / and /about routes
   - 404 handling for unknown paths
   - Uses performance.now() and console.log()

2. **generic-wintertc-app.js** - Plain WinterTC app with:
   - Direct WinterTC API usage (crypto.getRandomValues, performance.now)
   - Multiple routes: /, /api/data, /health
   - Headers iteration with forEach
   - 404 handling

### Integration Tests

1. **framework_compat_test.rs** (7 tests):
   - Generic WinterTC app: root route, health route, api data route, 404
   - Hono-style app: root route, about route, 404
   - Verifies status codes, Content-Type headers, CORS headers

2. **hono_integration_test.rs** (3 tests):
   - Middleware chain order verification
   - POST request handling
   - CORS headers on all routes

## Key Implementation Details

### ES6 Module Transformation

V8's Script API doesn't natively support ES6 module syntax (`export default`). Added `transform_module_code()` function that converts:

```javascript
// Before
export default { async fetch(request) { ... } }

// After (V8-compatible)
var __nano_handler = { async fetch(request) { ... } }
if (typeof __nano_handler === 'object' && __nano_handler.fetch) {
    var fetch = __nano_handler.fetch;
}
```

### Promise Resolution

Async fetch handlers return Promises. Added inline Promise resolution:

```rust
if response.is_promise() {
    let promise = response.cast::<v8::Promise>();
    match promise.state() {
        v8::PromiseState::Fulfilled => Some(promise.result(scope)),
        v8::PromiseState::Rejected => Err(...),
        v8::PromiseState::Pending => Err(...),
    }
}
```

### API Bindings Added

1. **Response constructor** - Creates Response objects with status, headers, body
2. **URL constructor** - Parses URLs with href, protocol, host, hostname, port, pathname, search, hash properties
3. **Headers constructor** - Headers object with get, set, forEach methods
4. **Headers conversion** - Plain object headers are wrapped in Headers instance for forEach support

## Tests Status

| Test File | Tests | Status |
|-----------|-------|--------|
| framework_compat_test.rs | 7 | ✅ All passing |
| hono_integration_test.rs | 3 | ✅ All passing |
| **Total** | **10** | **✅ 100%** |

## Success Criteria

- ✅ tests/fixtures/frameworks/hono-app.js created with middleware pattern
- ✅ tests/fixtures/frameworks/generic-wintertc-app.js created with raw WinterTC APIs
- ✅ tests/framework_compat_test.rs created with 7 test functions
- ✅ tests/hono_integration_test.rs created with extended middleware tests
- ✅ All tests pass with `cargo test`
- ✅ FRAME-01 satisfied: Hono.js compatibility verified
- ✅ FRAME-04 satisfied: Generic WinterTC compatibility verified

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] ES6 module syntax not supported by V8 Script**
- **Found during:** Task 3 (writing integration tests)
- **Issue:** V8's Script::compile() doesn't understand `export default` syntax
- **Fix:** Added `transform_module_code()` function to convert ES6 modules to V8-compatible code
- **Commit:** `fix(08-01): transform ES6 export default syntax for V8 compatibility`

**2. [Rule 2 - Missing] URL constructor not bound**
- **Found during:** Task 3 (test execution)
- **Issue:** JavaScript error: `ReferenceError: URL is not defined`
- **Fix:** Added URL API binding with href, protocol, host, hostname, port, pathname, search, hash properties
- **Commit:** `feat(08-01): add URL constructor for WinterTC compatibility`

**3. [Rule 2 - Missing] Headers constructor with forEach not available**
- **Found during:** Task 3 (test execution)
- **Issue:** `TypeError: request.headers.forEach is not a function`
- **Fix:** Added Headers API with get, set, forEach methods; converted plain object headers to Headers instance
- **Commit:** `feat(08-01): add Headers constructor with forEach support`

**4. [Rule 1 - Bug] Async fetch handlers return Promises**
- **Found during:** Task 3 (test execution)
- **Issue:** Response was a Promise, not a resolved Response object
- **Fix:** Added inline Promise resolution in `execute_in_v8()`
- **Commit:** `fix(08-01): inline Promise resolution to avoid lifetime issues`

**5. [Rule 1 - Bug] Plain headers object lacks forEach method**
- **Found during:** Task 3 (test execution)
- **Issue:** Request headers from JSON parsing is plain object, not Headers instance
- **Fix:** After parsing request JSON, wrap headers in Headers constructor
- **Commit:** `fix(08-01): convert plain headers to Headers instance in request`

## Self-Check

- ✅ All 10 tests pass
- ✅ 2 test fixtures created
- ✅ 4 API bindings added (Response, URL, Headers, plus Promise handling)
- ✅ ES6 module transformation working
- ✅ Handler execution fully functional

## Commits

```
1. test(08-01): create Hono.js and generic WinterTC test fixtures
2. test(08-01): add framework compatibility integration tests
3. fix(08-01): transform ES6 export default syntax for V8 compatibility
4. feat(08-01): add Response constructor and bind RuntimeAPIs in handler
5. fix(08-01): reorder scope creation for RuntimeAPIs binding
6. fix(08-01): bind RuntimeAPIs after ContextScope creation
7. fix(08-01): add Promise resolution in response extraction
8. fix(08-01): add lifetime specifiers to resolve_promise
9. fix(08-01): add lifetime specifiers to extract_js_response
10. fix(08-01): remove explicit lifetimes from promise functions
11. fix(08-01): use 's lifetime for promise functions
12. fix(08-01): inline Promise resolution to avoid lifetime issues
13. debug(08-01): add error message to Promise rejection
14. feat(08-01): add URL constructor for WinterTC compatibility
15. feat(08-01): add Headers constructor with forEach support
16. fix(08-01): convert plain headers to Headers instance in request
17. fix(08-01): use correct variable name js_request
18. chore(08-01): remove debug output from status extraction
```

## Next Steps

Plan 08-01 is complete. Proceed to Plan 08-02 (Next.js static export & Astro islands test apps).
