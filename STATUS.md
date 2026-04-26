# NANO Runtime v1.2.4 Technical Summary

Date: 2026-04-26  
Status: All tests passing (100%)

## Changes in This Release

### Runtime API Fixes (Rust Implementation)

#### 1. Buffer.from().toString() Fix
**File:** src/runtime/apis.rs

**Problem:** Buffer.from('test').toString() returned "116,101,115,116" instead of "test"

**Solution:** 
- Implemented buffer_tostring_callback that decodes Uint8Array bytes to UTF-8 string
- Added add_buffer_tostring_to_instance helper to attach toString as own property on Buffer instances
- Updated buffer_constructor, buffer_from_callback, and buffer_alloc_callback to attach toString method

**Technical Details:**
- Uses String::from_utf8_lossy for UTF-8 decoding with replacement for invalid sequences
- Attaches toString as own property (not prototype) to override Uint8Array default
- Preserves all byte data while providing proper string representation

#### 2. URL.toString() Fix
**File:** src/runtime/apis.rs

**Problem:** new URL('http://example.com/').toString() returned "[object Object]"

**Solution:**
- Implemented url_tostring_callback that returns the href property value
- Added url_href_callback as property getter
- Updated bind_url to attach toString method to URL prototype

**Technical Details:**
- Retrieves href property from URL instance at runtime
- Returns empty string if href property not found (fallback)
- Uses v8::Function::new for proper method binding

#### 3. HTTP Client Implementation
**File:** src/http/client.rs

**Problem:** HTTP client returned mock 200 OK responses without making actual HTTP requests

**Solution:**
- Implemented real HTTP client using reqwest crate
- Added reqwest::Client field to HttpClient struct for connection pooling
- Implemented full request/response lifecycle with proper error handling

**Technical Details:**
- Uses reqwest::Client::builder() with timeout (30s) and redirect policy (10 max)
- Supports HTTP/1.1 and HTTP/2 via reqwest's default features
- Handles request body streaming and response body accumulation
- Enforces response size limits (100MB default) with HttpClientError::ResponseTooLarge
- Implements comprehensive error types: InvalidUrl, PrivateIpBlocked, Network, Timeout, TooManyRedirects, BlockedHeader, ResponseTooLarge, Tls

### Test Harness Fixes (JavaScript/Test Code)

#### 4. crypto.subtle API Access Fix
**File:** scripts/fast-compatibility-matrix.js

**Problem:** Tests for crypto.subtle.digest and crypto.subtle.generateKey failed with "Unknown test" error

**Root Cause:** Test harness used switch case key pattern `category + ':' + test`. Test definitions used `api: 'crypto.subtle'` and `name: 'digest'`, creating lookup key `crypto.subtle:digest`. The TEST_APP switch case expected `crypto:digest`.

**Solution:** Updated switch cases in TEST_APP from `crypto:digest` and `crypto:generateKey` to `crypto.subtle:digest` and `crypto.subtle:generateKey`

#### 5. CRUD Test Regex Fix
**Files:** scripts/run-tests.js, tests/harness.js

**Problem:** "Script compilation failed" errors on CRUD tests due to invalid regex in generated JavaScript

**Root Cause:** Template literals in JavaScript treat `/` as just `/` in the output (not an escape sequence). The regex pattern `^/api/items/(
+)$` was being written with `^/api/items/(d+)$` which has unescaped forward slashes, causing a JavaScript syntax error.

**Solution:** 
- In scripts/run-tests.js: Changed from `^\/api\/items\/(\d+)$` to `^\\/api\\/items\\/(\\d+)$`
- In tests/harness.js: Changed from `^\\/api\\/items(?:\\/(\\d+))?\$` to `^\\/api\\/items(?:\\/(\\d+))?$`

The double backslash in template literal source produces a single backslash in output, which then escapes the forward slash in the regex.

## Test Results Summary

### API Compatibility Matrix
- Total Tests: 26
- Passed: 26 (100%)
- Failed: 0

### Comprehensive Test Suite  
- Total Tests: 27
- Passed: 27 (100%)
- Failed: 0

### Test Category Breakdown

| Category | Tests | Passed | Status |
|----------|-------|--------|--------|
| CLI | 3 | 3 | 100% |
| Basic HTTP | 3 | 3 | 100% |
| WinterCG APIs | 2 | 2 | 100% |
| Node.js APIs | 2 | 2 | 100% |
| WebCrypto | 2 | 2 | 100% |
| CRUD Operations | 6 | 6 | 100% |
| HTTP Verbs | 7 | 7 | 100% |
| Multi-tenancy | 2 | 2 | 100% |

## What Works

All documented APIs are implemented and tested:

- Multi-tenant JavaScript isolation with V8 isolates
- HTTP server with virtual host routing  
- WinterCG-compatible fetch(), Request, Response, Headers
- URL and URLSearchParams API
- TextEncoder/TextDecoder for UTF-8
- WebCrypto: AES-GCM, HMAC, SHA-256, PBKDF2
- Node.js Buffer with proper toString()
- Node.js fs polyfill via require('fs')
- VFS with memory/disk/S3 backends
- Console API (log, error, warn)
- Timers (setTimeout, setInterval, clearTimeout, clearInterval)
- Sliver snapshots with ~267µs cold starts
- Multi-tenancy with per-app worker pools

## What Does Not Work (By Design)

The following are intentionally not supported:

- Node.js http module - Use WinterCG fetch() instead
- Node.js net module - Raw sockets not supported
- process.env global - Use request headers or config
- Node.js path module - Use URL API instead
- Cloudflare KV API - Not supported (use VFS or external DB)
- Cloudflare Durable Objects - Not supported

These limitations are architectural decisions for WinterCG compatibility and security.

## Performance Characteristics

Measured on Darwin arm64, Rust 1.75, V8 12.0:

- Cold start from sliver: 267µs
- Context reset: 5ms
- Fresh isolate creation: 50-100ms
- HTTP request handling: <1ms (excluding JS execution)
- Max response body size: 100MB (configurable)
- Default timeout: 30 seconds (configurable)
- Max redirects: 10 (configurable)

## Architecture Summary

- One OS process hosts many isolated JavaScript apps
- Each app runs in a separate V8 isolate with 128MB default memory limit
- Worker pool handles requests with 4 workers per app (configurable)
- Context reset between requests provides isolation (~5ms overhead)
- VFS provides per-isolate filesystem namespaces
- Sliver snapshots enable sub-millisecond cold starts (~267µs)
- HTTP server with virtual host routing by Host header
- Outbound HTTP via reqwest with connection pooling and timeout handling

## Security Model

- Per-isolate VFS namespaces prevent filesystem escape
- Path traversal blocked (".." sequences rejected in all VFS operations)
- SSRF prevention blocks private IP ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8)
- Dangerous headers filtered (Content-Length, Host, Transfer-Encoding, Connection)
- URL scheme restricted to http/https only (blocks file://, ftp://, javascript://, data://)
- Request timeouts enforced per-isolate (default: 30s, configurable)
- Memory limits enforced per-isolate (default: 128MB, configurable)
- Worker pool limits prevent resource exhaustion

## Building and Testing

Requirements:
- Rust 1.70+
- No V8 compilation needed (uses pre-built rusty_v8)

Build:
```bash
cargo build --release
```

The binary is at `target/release/nano-rs`.

Run tests:
```bash
# API compatibility tests
cd /path/to/test-suite
NANO_BINARY=/path/to/nano-rs node scripts/fast-compatibility-matrix.js

# Comprehensive test suite
NANO_BINARY=/path/to/nano-rs node scripts/run-tests.js
```

All tests pass at 100%.

## License

MIT License - See LICENSE file for details.
