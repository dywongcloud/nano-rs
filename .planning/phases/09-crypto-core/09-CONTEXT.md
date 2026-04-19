# Phase 9 Context: Crypto Core

**Phase:** 09-crypto-core  
**Goal:** Full WebCrypto implementation for encryption, signing, and key management  
**Planned:** 2026-04-19  
**Requirements:** CRYPT-01, CRYPT-02, CRYPT-03, CRYPT-04

## Decisions

### D-09-01: Use ring crate for all crypto operations
**Status:** LOCKED  
Per RESEARCH.md STACK section: "ring (^0.17) — Core crypto primitives — BoringSSL pedigree". Never use V8's crypto.subtle C++ implementation. This decision aligns with the broader architecture to bypass V8 complexity for critical operations.

### D-09-02: Implement only AES-GCM and HMAC for v1
**Status:** LOCKED  
Per REQUIREMENTS.md: CRYPT-01 through CRYPT-04 only require AES-GCM and HMAC. ECDSA (P-256) and RSA are v2 requirements (ADV-04). Do NOT implement asymmetric crypto in Phase 9.

### D-09-03: JWK format for key import/export
**Status:** LOCKED  
Per WebCrypto specification, JWK (JSON Web Key) is the standard format for symmetric key import/export. Implement kty: "oct" for symmetric keys with alg: "A128GCM"/"A192GCM"/"A256GCM" for AES and "HS256"/"HS384"/"HS512" for HMAC.

### D-09-04: Async API for all subtle operations
**Status:** LOCKED  
Per WebCrypto specification, all crypto.subtle methods return Promises. Even though ring operations are synchronous, wrap them in Promises for spec compliance and future async extension.

### D-09-05: Use pre-generated V8 bindings, never compile V8
**Status:** LOCKED  
Continue using pre-built rusty_v8 binaries as established in Phase 1. Do not add any V8 compilation steps.

## Deferred Ideas

- **RSA support (RSA-OAEP, RSASSA-PKCS1-v1_5):** Deferred to v2 (ADV-04)
- **ECDSA P-256 signing:** Deferred to v2 (ADV-04)  
- **SHA-1 support:** Not needed, SHA-256 is minimum
- **PBKDF2 key derivation:** Deferred to v2
- **HKDF key derivation:** Deferred to v2

## the agent's Discretion

### Key length defaults
- AES-GCM: Support 128, 192, 256 bits (all required)
- HMAC: Default to hash output length (256 bits for SHA-256)

### IV generation
- Generate random 12-byte IV per encryption using ring::rand
- Allow caller-provided IV but warn if not 12 bytes

### Tag length
- Default to 128 bits (16 bytes) for AES-GCM
- Support 32, 64, 96, 104, 112, 120, 128 bits per WebCrypto spec

### Error handling
- Map ring errors to WebCrypto error types
- Never expose internal error details or key material

## References

- RESEARCH.md STACK section (ring, p256, rsa crate selection)
- RESEARCH.md PITFALLS.md #12 (Crypto.subtle implementation security)
- REQUIREMENTS.md CRYPT-01 through CRYPT-04
- WebCrypto specification: https://www.w3.org/TR/WebCryptoAPI/
- RFC 7517 (JWK): https://tools.ietf.org/html/rfc7517
- RFC 7518 (JWA): https://tools.ietf.org/html/rfc7518

## Scope Confirmation

This phase delivers:
1. ✓ crypto.subtle.generateKey (AES-GCM, HMAC)
2. ✓ crypto.subtle.importKey/exportKey (JWK format)
3. ✓ crypto.subtle.encrypt/decrypt (AES-GCM)
4. ✓ crypto.subtle.sign/verify (HMAC)

This phase does NOT deliver:
- ✗ RSA operations (deferred)
- ✗ ECDSA operations (deferred)  
- ✗ Streaming crypto operations
- ✗ Non-extractable key enforcement (beyond basic flag checking)
- ✗ Hardware security module integration
