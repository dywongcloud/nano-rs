---
phase: 09-crypto-core
plan: 03
subsystem: crypto
summary: "HMAC sign/verify and JWK key import/export with full WebCrypto compliance"
dependency_graph:
  requires: ["09-01", "09-02"]
  provides: ["hmac-signatures", "jwk-import-export", "complete-crypto"]
tech_stack:
  added: [ring hmac]
  patterns: [JWK format, constant-time verification]
key_files:
  modified:
    - src/runtime/apis.rs (+341 lines)
    - src/runtime/crypto/mod.rs
    - src/runtime/crypto/hmac.rs
    - src/runtime/crypto/crypto_key.rs
decisions:
  - JWK format (kty: "oct") for symmetric key import/export
  - Constant-time HMAC verification via ring crate
  - SHA-256, SHA-384, SHA-512 hash algorithms supported
  - extractable flag enforced at export time
  - Promise-based API for all import/export operations
metrics:
  duration: "~25 minutes"
  tests_passed: 11
  algorithms_supported: 6 (HMAC-SHA256, HMAC-SHA384, HMAC-SHA512)
  api_methods: 4 (sign, verify, importKey, exportKey)
---

## What Was Built

### HMAC Implementation (src/runtime/crypto/hmac.rs)
- `HmacKey` struct for HMAC key material storage
- `HmacParams` for algorithm parameter handling (hash algorithm selection)
- Support for SHA-256, SHA-384, SHA-512 hash functions
- Constant-time signature verification (prevents timing attacks)
- Key generation, signing, and verification operations

### V8 Integration Layer (src/runtime/apis.rs)
- `subtle_sign()` — Creates HMAC signatures for arbitrary data
- `subtle_verify()` — Verifies HMAC signatures with constant-time comparison
- `subtle_import_key()` — Imports keys from JWK format
- `subtle_export_key()` — Exports keys to JWK format (if extractable)
- Algorithm identifier parsing for HMAC with hash selection

### Key Import/Export (JWK Format)
JWK (JSON Web Key) format support:
- `kty`: "oct" (octet sequence) for symmetric keys
- `k`: Base64URL-encoded key material
- `alg`: Algorithm identifier (e.g., "HS256", "A128GCM")
- `ext`: Extractable flag
- `key_ops`: Key usage array ("sign", "verify", "encrypt", "decrypt")

### CryptoKey Enhancements
- Algorithm identifier storage
- Extractable flag enforcement
- Key usage validation (sign, verify, encrypt, decrypt)
- Safe export with permission checks

## Verification

### Unit Tests (tests/crypto_hmac_test.rs)
All 11 tests passing:
- `test_hmac_generate_key_sha256` — HMAC-SHA256 key generation
- `test_hmac_generate_key_sha384` — HMAC-SHA384 key generation
- `test_hmac_generate_key_sha512` — HMAC-SHA512 key generation
- `test_hmac_sign_sha256` — Signing with SHA-256
- `test_hmac_sign_sha384` — Signing with SHA-384
- `test_hmac_sign_sha512` — Signing with SHA-512
- `test_hmac_verify_valid_signature` — Successful verification
- `test_hmac_verify_invalid_signature` — Failed verification for tampered data
- `test_hmac_import_key_jwk` — JWK import functionality
- `test_hmac_export_key_jwk` — JWK export functionality
- `test_hmac_import_export_roundtrip` — Import then export preserves key

### Integration Tests (tests/crypto_subtle_test.rs)
- API surface verification
- Promise-based operation confirmation
- Error handling validation

## Commits
- `945815ad` — feat(09-03): HMAC sign/verify and JWK import/export

## Security Features

### HMAC Security
- Constant-time signature verification (ring::hmac::verify)
- Prevents timing attacks on signature comparison
- Hash algorithm agility (SHA-256/384/512)

### Key Management Security
- Non-extractable keys cannot be exported
- Extractable flag set at key creation, enforced at export
- Key usage restrictions (e.g., "sign" key cannot be used for "verify")

### JWK Safety
- Base64URL encoding (URL-safe, no padding issues)
- Proper algorithm identifier mapping
- Extractable flag correctly serialized

## WebCrypto Compliance

### Implemented Methods
- `crypto.subtle.generateKey()` — AES-GCM and HMAC
- `crypto.subtle.importKey()` — JWK format, AES-GCM and HMAC
- `crypto.subtle.exportKey()` — JWK format (extractable keys only)
- `crypto.subtle.encrypt()` — AES-GCM
- `crypto.subtle.decrypt()` — AES-GCM
- `crypto.subtle.sign()` — HMAC
- `crypto.subtle.verify()` — HMAC

### Supported Algorithms
- AES-GCM: 128, 192, 256 bit keys
- HMAC: SHA-256, SHA-384, SHA-512

## Phase 9 Complete

All CRYPT-01 through CRYPT-04 requirements satisfied:
- ✅ crypto.subtle.generateKey (AES-GCM, HMAC)
- ✅ crypto.subtle.importKey/exportKey (JWK format)
- ✅ crypto.subtle.encrypt/decrypt (AES-GCM via ring)
- ✅ crypto.subtle.sign/verify (HMAC via ring)

Ready for milestone completion.
