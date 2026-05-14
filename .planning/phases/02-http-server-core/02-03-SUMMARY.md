# Phase 02 Plan 03: WinterTC Request/Response Types Summary

**Phase:** 02-http-server-core  
**Plan:** 03  
**Status:** ✅ COMPLETE  
**Completed:** 2026-04-19  

## One-Liner

Implemented WinterTC-compatible Request/Response/URL/Headers types that bridge Rust HTTP handling with JavaScript execution, including V8 serialization bridge.

## Deliverables

### Created Files

| File | Description | Exports |
|------|-------------|---------|
| `src/http/url.rs` | WinterTC URL and URLSearchParams | `NanoUrl`, `NanoUrlSearchParams` |
| `src/http/headers.rs` | WinterTC Headers API | `NanoHeaders` |
| `src/http/types.rs` | Request/Response types | `NanoRequest`, `NanoResponse` |
| `src/http/v8_bridge.rs` | V8 serialization bridge | `serialize_request_to_json`, `serialize_response_to_json` |
| `tests/http_wintertc_test.rs` | Integration tests | 9 compliance tests |

### Modified Files

| File | Changes |
|------|---------|
| `src/http/mod.rs` | Added new module exports |
| `src/http/router.rs` | Integrated WinterTC types into routing |
| `tests/http_routing_test.rs` | Updated for new handler type |
| `Cargo.toml` | Added url, percent-encoding, bytes, base64 dependencies |

## Specification Compliance

### WinterTC Requirements Met

- ✅ **Request**: method, url, headers, body properties
- ✅ **Response**: status, statusText, headers, body properties  
- ✅ **Headers**: get, getSetCookie, has, set, append, delete, forEach methods
- ✅ **URL**: href, origin, protocol, host, hostname, port, pathname, search, hash properties
- ✅ **URLSearchParams**: get, getAll, has, set, append, delete, toString methods

### Decisions Implemented

| Decision | Implementation |
|----------|------------------|
| **D-05** | Body buffering with 1MB limit (streaming in Phase 6) |
| **D-06** | JSON serialization for V8 bridge |
| **D-07** | Case-insensitive headers via lowercase HashMap keys |
| **D-08** | Set-Cookie separate handling (not comma-combined) |
| **D-09** | Full WinterTC URL compliance (all properties) |
| **D-10** | Lossy UTF-8 percent-decoding with U+FFFD replacement |

## Commits

| Hash | Message |
|------|---------|
| 11b34e1 | feat(02-03): Implement WinterTC HTTP types |
| 67ab6cf | feat(02-03): Integrate WinterTC types into router |
| 18efce8 | test(02-03): Add WinterTC compliance integration tests |

## Test Results

### Unit Tests (42 passed)

```
test http::url::tests::test_url_parsing ... ok
test http::url::tests::test_url_search_params ... ok
test http::url::tests::test_percent_decoding ... ok
test http::url::tests::test_lossy_percent_decoding ... ok
test http::headers::tests::test_case_insensitive_headers ... ok
test http::headers::tests::test_set_cookie_separate ... ok
test http::types::tests::test_request_creation ... ok
test http::types::tests::test_response_creation ... ok
test http::v8_bridge::tests::test_request_serialization ... ok
...
```

### Integration Tests (9 passed)

```
test test_full_request_response_cycle ... ok
test test_url_full_compliance ... ok
test test_headers_api_compliance ... ok
test test_url_search_params_compliance ... ok
test test_lossy_percent_decoding ... ok
test test_request_response_builder_pattern ... ok
test test_headers_set_cookie_handling ... ok
test test_url_various_protocols ... ok
test test_axum_conversion_roundtrip ... ok
```

### Total: 71 tests passing

## Key Design Decisions

### Headers Implementation
- Used `HashMap<String, Vec<String>>` with lowercase keys
- All header operations normalize names to lowercase
- Set-Cookie values stored separately in the Vec (D-08)
- `get()` returns comma-combined for non-Set-Cookie headers
- `get_set_cookie()` returns all cookie values as separate strings

### URL Implementation  
- Wraps the `url` crate for robust parsing
- `NanoUrlSearchParams` parses query strings with percent-decoding
- Invalid UTF-8 sequences become U+FFFD (replacement character) per D-10
- Full WinterTC property coverage including origin and hash

### Request/Response Body
- Small bodies (<1MB) buffered in memory as `Bytes` (D-05)
- `Option<Bytes>` distinguishes empty body from no body
- Builder pattern on NanoResponse for ergonomic construction

### V8 Bridge
- JSON serialization approach per D-06 (simpler than direct V8 API)
- Base64 encoding for binary body content
- JSON string escaping for safe serialization
- Full Request and Response JSON serialization functions

## Dependencies Added

```toml
url = "2.5"
percent-encoding = "2.3"
bytes = "1.6"
base64 = "0.22"
```

## Router Integration

The virtual host router now:

1. Extracts Host header from axum request
2. Constructs full URL from host + path for NanoUrl
3. Converts axum headers to NanoHeaders (case-insensitive)
4. Reads body with 1MB limit
5. Creates NanoRequest
6. Routes to handler (returns NanoResponse)
7. Converts NanoResponse back to axum response

## Threat Model Compliance

| Threat | Status | Mitigation |
|--------|--------|------------|
| T-02-07: Header injection | ✅ Mitigated | Header values validated when converting to axum HeaderValue |
| T-02-08: URL param leak | ✅ Accepted | Query params intentionally part of request |
| T-02-09: Large body DoS | ✅ Mitigated | 1MB body limit enforced (D-05) |
| T-02-10: JSON tampering | ✅ Accepted | JSON is intermediate format |

## Success Criteria Verification

| Criterion | Status |
|-----------|--------|
| NanoUrl implements all WinterTC URL properties | ✅ |
| NanoHeaders implements all WinterTC Headers methods | ✅ |
| Case-insensitive header names (RFC 7230) | ✅ |
| Set-Cookie values separate (browser behavior) | ✅ |
| NanoUrlSearchParams implements all methods | ✅ |
| Percent-decoding with lossy UTF-8 (D-10) | ✅ |
| Request/Response convert to/from axum types | ✅ |
| V8 bridge serializes for JavaScript integration | ✅ |
| All tests pass (71 total) | ✅ |
| No compiler errors | ✅ |

## Next Steps

Phase 2 is now complete. Phase 3 will:
- Execute JavaScript handlers using WinterTC types
- Implement Response parsing from V8 objects
- Add full V8 integration for the WinterTC handler

## Notes

- The `WinterTCHandler` variant replaces `JavaScriptEntrypoint` for clarity
- Actual JS execution happens in Phase 3 - current handler returns placeholder
- Base64 encoding implemented internally to avoid extra dependency
- All types follow Rust naming conventions (Nano* prefix to avoid conflicts)
