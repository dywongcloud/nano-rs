# NANO-RS Blackbox Test Report

**Generated:** 2026-05-06T17:02:40.207Z  
**NANO Version:** nano-rs 1.5.0 (v8-crate: 147.4.0)  
**Test Duration:** 16543ms

## Summary

| Metric | Value |
|--------|-------|
| Total Tests | 27 |
| Passed | ✓ 27 |
| Failed | ✗ 0 |
| **Overall Score** | **100%** |

## Results by Category

### cli (3/3 - 100%)

| Test | Status |
|------|--------|
| nano-rs binary exists | ✓ passed |
| nano-rs --version works | ✓ passed |
| nano-rs --help works | ✓ passed |

### basic-http (3/3 - 100%)

| Test | Status |
|------|--------|
| Basic HTTP server starts | ✓ passed |
| Basic HTTP GET request | ✓ passed |
| HTTP POST with body | ✓ passed |

### wintercg (2/2 - 100%)

| Test | Status |
|------|--------|
| WinterCG: fetch() API available | ✓ passed |
| WinterCG: Request/Response objects | ✓ passed |

### nodejs (2/2 - 100%)

| Test | Status |
|------|--------|
| Node.js: console methods | ✓ passed |
| Node.js: Buffer | ✓ passed |

### webcrypto (2/2 - 100%)

| Test | Status |
|------|--------|
| WebCrypto: crypto.subtle available | ✓ passed |
| WebCrypto: AES-GCM encryption | ✓ passed |

### crud (6/6 - 100%)

| Test | Status |
|------|--------|
| CRUD: Server starts | ✓ passed |
| CRUD: CREATE (POST) | ✓ passed |
| CRUD: READ ALL (GET) | ✓ passed |
| CRUD: READ ONE (GET) | ✓ passed |
| CRUD: UPDATE (PUT) | ✓ passed |
| CRUD: DELETE (DELETE) | ✓ passed |

### http-verbs (7/7 - 100%)

| Test | Status |
|------|--------|
| HTTP: GET request | ✓ passed |
| HTTP: POST request | ✓ passed |
| HTTP: PUT request | ✓ passed |
| HTTP: PATCH request | ✓ passed |
| HTTP: DELETE request | ✓ passed |
| HTTP: HEAD request | ✓ passed |
| HTTP: OPTIONS request | ✓ passed |

### multi-tenancy (2/2 - 100%)

| Test | Status |
|------|--------|
| Multi-tenancy: Server with multiple apps starts | ✓ passed |
| Multi-tenancy: Routes to correct app by Host header | ✓ passed |

