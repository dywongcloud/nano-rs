//! Synchronous crypto host hooks for the node:crypto compatibility module.
//!
//! Contract: CONTRACT.md §4 (crypto section). All functions are synchronous,
//! accept/return `Uint8Array`, and throw coded JS errors on failure.

use super::helpers::*;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use digest::Digest;
use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{Oaep, Pkcs1v15Encrypt, Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};
use signature::hazmat::{PrehashSigner, PrehashVerifier};

/// Bind all crypto hooks onto the host object.
pub(super) fn bind(scope: &mut v8::PinnedRef<v8::HandleScope>, host: v8::Local<v8::Object>) {
    set_fn(scope, host, "cryptoDigest", crypto_digest);
    set_fn(scope, host, "cryptoHmac", crypto_hmac);
    set_fn(scope, host, "cryptoPbkdf2", crypto_pbkdf2);
    set_fn(scope, host, "cryptoScrypt", crypto_scrypt);
    set_fn(scope, host, "cryptoHkdf", crypto_hkdf);
    set_fn(scope, host, "cryptoRandomBytes", crypto_random_bytes);
    set_fn(scope, host, "cryptoTimingSafeEqual", crypto_timing_safe_equal);
    set_fn(scope, host, "cryptoCipher", crypto_cipher);
    set_fn(scope, host, "cryptoRsaGenerate", crypto_rsa_generate);
    set_fn(scope, host, "cryptoRsaSign", crypto_rsa_sign);
    set_fn(scope, host, "cryptoRsaVerify", crypto_rsa_verify);
    set_fn(scope, host, "cryptoRsaEncrypt", crypto_rsa_encrypt);
    set_fn(scope, host, "cryptoRsaDecrypt", crypto_rsa_decrypt);
    set_fn(scope, host, "cryptoEcGenerate", crypto_ec_generate);
    set_fn(scope, host, "cryptoEcSign", crypto_ec_sign);
    set_fn(scope, host, "cryptoEcVerify", crypto_ec_verify);
    set_fn(scope, host, "cryptoEd25519Generate", crypto_ed25519_generate);
    set_fn(scope, host, "cryptoEd25519Sign", crypto_ed25519_sign);
    set_fn(scope, host, "cryptoEd25519Verify", crypto_ed25519_verify);
}

// ---------------------------------------------------------------------------
// Digest / MAC / KDF
// ---------------------------------------------------------------------------

fn digest_bytes(alg: &str, data: &[u8]) -> Option<Vec<u8>> {
    Some(match alg {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha224" => sha2::Sha224::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        _ => return None,
    })
}

fn hmac_bytes(alg: &str, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use hmac::{Hmac, Mac};
    macro_rules! mac {
        ($d:ty) => {{
            let mut m = <Hmac<$d> as Mac>::new_from_slice(key).ok()?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    Some(match alg {
        "md5" => mac!(md5::Md5),
        "sha1" => mac!(sha1::Sha1),
        "sha224" => mac!(sha2::Sha224),
        "sha256" => mac!(sha2::Sha256),
        "sha384" => mac!(sha2::Sha384),
        "sha512" => mac!(sha2::Sha512),
        _ => return None,
    })
}

fn crypto_digest(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(alg), Some(data)) = (str_arg(scope, &args, 0), bytes_arg(scope, &args, 1)) else {
        return throw_bad_args(scope, "cryptoDigest");
    };
    match digest_bytes(&alg, &data) {
        Some(out) => retval.set(make_uint8array(scope, out).into()),
        None => throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", alg)),
    }
}

fn crypto_hmac(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(alg), Some(key), Some(data)) = (
        str_arg(scope, &args, 0),
        bytes_arg(scope, &args, 1),
        bytes_arg(scope, &args, 2),
    ) else {
        return throw_bad_args(scope, "cryptoHmac");
    };
    match hmac_bytes(&alg, &key, &data) {
        Some(out) => retval.set(make_uint8array(scope, out).into()),
        None => throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", alg)),
    }
}

fn crypto_pbkdf2(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(password), Some(salt), Some(iterations), Some(keylen), Some(alg)) = (
        bytes_arg(scope, &args, 0),
        bytes_arg(scope, &args, 1),
        num_arg(scope, &args, 2),
        num_arg(scope, &args, 3),
        str_arg(scope, &args, 4),
    ) else {
        return throw_bad_args(scope, "cryptoPbkdf2");
    };
    if iterations < 1.0 || iterations > u32::MAX as f64 || keylen < 0.0 || keylen > (1 << 30) as f64 {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "pbkdf2: iterations or keylen out of range");
    }
    let iterations = iterations as u32;
    let mut out = vec![0u8; keylen as usize];
    macro_rules! kdf {
        ($d:ty) => {
            pbkdf2::pbkdf2_hmac::<$d>(&password, &salt, iterations, &mut out)
        };
    }
    match alg.as_str() {
        "md5" => kdf!(md5::Md5),
        "sha1" => kdf!(sha1::Sha1),
        "sha224" => kdf!(sha2::Sha224),
        "sha256" => kdf!(sha2::Sha256),
        "sha384" => kdf!(sha2::Sha384),
        "sha512" => kdf!(sha2::Sha512),
        _ => {
            return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", alg));
        }
    }
    retval.set(make_uint8array(scope, out).into());
}

fn crypto_scrypt(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(password), Some(salt), Some(n), Some(r), Some(p), Some(keylen)) = (
        bytes_arg(scope, &args, 0),
        bytes_arg(scope, &args, 1),
        num_arg(scope, &args, 2),
        num_arg(scope, &args, 3),
        num_arg(scope, &args, 4),
        num_arg(scope, &args, 5),
    ) else {
        return throw_bad_args(scope, "cryptoScrypt");
    };
    let n = n as u64;
    if n < 2 || !n.is_power_of_two() || keylen < 0.0 || keylen > (1 << 26) as f64 {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "scrypt: invalid N (must be power of two > 1) or keylen");
    }
    let log_n = n.trailing_zeros() as u8;
    let params = match scrypt::Params::new(log_n, r as u32, p as u32, keylen as usize) {
        Ok(p) => p,
        Err(e) => {
            return throw_coded(scope, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", &format!("scrypt: {}", e));
        }
    };
    let mut out = vec![0u8; keylen as usize];
    if let Err(e) = scrypt::scrypt(&password, &salt, &params, &mut out) {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", &format!("scrypt: {}", e));
    }
    retval.set(make_uint8array(scope, out).into());
}

fn crypto_hkdf(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(alg), Some(ikm), Some(salt), Some(info), Some(keylen)) = (
        str_arg(scope, &args, 0),
        bytes_arg(scope, &args, 1),
        bytes_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
        num_arg(scope, &args, 4),
    ) else {
        return throw_bad_args(scope, "cryptoHkdf");
    };
    if keylen < 0.0 || keylen > (255 * 64) as f64 {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "hkdf: keylen out of range");
    }
    let mut out = vec![0u8; keylen as usize];
    let salt_opt = if salt.is_empty() { None } else { Some(salt.as_slice()) };
    macro_rules! kdf {
        ($d:ty) => {
            hkdf::Hkdf::<$d>::new(salt_opt, &ikm).expand(&info, &mut out).is_ok()
        };
    }
    let ok = match alg.as_str() {
        "md5" => kdf!(md5::Md5),
        "sha1" => kdf!(sha1::Sha1),
        "sha224" => kdf!(sha2::Sha224),
        "sha256" => kdf!(sha2::Sha256),
        "sha384" => kdf!(sha2::Sha384),
        "sha512" => kdf!(sha2::Sha512),
        _ => {
            return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", alg));
        }
    };
    if !ok {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "hkdf: derived key length too long for digest");
    }
    retval.set(make_uint8array(scope, out).into());
}

fn crypto_random_bytes(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(n) = num_arg(scope, &args, 0) else {
        return throw_bad_args(scope, "cryptoRandomBytes");
    };
    if n < 0.0 || n > (1 << 27) as f64 {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "randomBytes: size out of range (0..128MiB)");
    }
    let mut out = vec![0u8; n as usize];
    if getrandom::getrandom(&mut out).is_err() {
        return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", "randomBytes: entropy source failure");
    }
    retval.set(make_uint8array(scope, out).into());
}

fn crypto_timing_safe_equal(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(a), Some(b)) = (bytes_arg(scope, &args, 0), bytes_arg(scope, &args, 1)) else {
        return throw_bad_args(scope, "cryptoTimingSafeEqual");
    };
    if a.len() != b.len() {
        return throw_coded(
            scope,
            "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH",
            "Input buffers must have the same byte length",
        );
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    let equal = diff == 0;
    retval.set(v8::Boolean::new(scope, equal).into());
}

// ---------------------------------------------------------------------------
// AES ciphers (GCM / CBC / CTR)
// ---------------------------------------------------------------------------

enum AesMode {
    Gcm,
    Cbc,
    Ctr,
}

fn parse_aes_algo(algo: &str) -> Option<(usize, AesMode)> {
    let (bits, mode) = match algo {
        "aes-128-gcm" => (128, AesMode::Gcm),
        "aes-192-gcm" => (192, AesMode::Gcm),
        "aes-256-gcm" => (256, AesMode::Gcm),
        "aes-128-cbc" => (128, AesMode::Cbc),
        "aes-192-cbc" => (192, AesMode::Cbc),
        "aes-256-cbc" => (256, AesMode::Cbc),
        "aes-128-ctr" => (128, AesMode::Ctr),
        "aes-192-ctr" => (192, AesMode::Ctr),
        "aes-256-ctr" => (256, AesMode::Ctr),
        _ => return None,
    };
    Some((bits / 8, mode))
}

struct CipherOutput {
    data: Vec<u8>,
    tag: Option<Vec<u8>>,
}

fn aes_run(
    encrypt: bool,
    algo: &str,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
    aad: Option<&[u8]>,
    tag: Option<&[u8]>,
) -> Result<CipherOutput, (&'static str, String)> {
    let Some((key_len, mode)) = parse_aes_algo(algo) else {
        return Err(("ERR_CRYPTO_UNKNOWN_CIPHER", format!("Unknown cipher: {}", algo)));
    };
    if key.len() != key_len {
        return Err(("ERR_CRYPTO_INVALID_KEYLEN", format!("Invalid key length {} for {}", key.len(), algo)));
    }
    match mode {
        AesMode::Gcm => {
            if iv.len() != 12 {
                return Err((
                    "ERR_CRYPTO_INVALID_IV",
                    "GCM requires a 12-byte IV in the NANO runtime".to_string(),
                ));
            }
            let nonce: &aes_gcm::Nonce<aes_gcm::aead::consts::U12> = iv.try_into().map_err(|_| ("ERR_CRYPTO_INVALID_IV", "GCM requires a 12-byte IV in the NANO runtime".to_string()))?;
            let payload_aad = aad.unwrap_or(&[]);
            macro_rules! gcm {
                ($c:ty) => {{
                    let cipher = <$c>::new_from_slice(key)
                        .map_err(|e| ("ERR_CRYPTO_INVALID_KEYLEN", e.to_string()))?;
                    if encrypt {
                        let ct = cipher
                            .encrypt(nonce, Payload { msg: data, aad: payload_aad })
                            .map_err(|_| ("ERR_CRYPTO_OPERATION_FAILED", "GCM encrypt failed".to_string()))?;
                        let split = ct.len() - 16;
                        let mut ct = ct;
                        let tag_bytes = ct.split_off(split);
                        Ok(CipherOutput { data: ct, tag: Some(tag_bytes) })
                    } else {
                        let Some(tag) = tag else {
                            return Err((
                                "ERR_CRYPTO_INVALID_STATE",
                                "GCM decrypt requires an auth tag".to_string(),
                            ));
                        };
                        let mut combined = Vec::with_capacity(data.len() + tag.len());
                        combined.extend_from_slice(data);
                        combined.extend_from_slice(tag);
                        let pt = cipher
                            .decrypt(nonce, Payload { msg: &combined, aad: payload_aad })
                            .map_err(|_| (
                                "ERR_CRYPTO_INVALID_AUTH_TAG",
                                "Unsupported state or unable to authenticate data".to_string(),
                            ))?;
                        Ok(CipherOutput { data: pt, tag: None })
                    }
                }};
            }
            match key_len {
                16 => gcm!(aes_gcm::Aes128Gcm),
                24 => gcm!(aes_gcm::AesGcm<aes::Aes192, aes_gcm::aead::consts::U12>),
                _ => gcm!(aes_gcm::Aes256Gcm),
            }
        }
        AesMode::Cbc => {
            use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
            if iv.len() != 16 {
                return Err(("ERR_CRYPTO_INVALID_IV", "CBC requires a 16-byte IV".to_string()));
            }
            macro_rules! cbc_run {
                ($a:ty) => {{
                    if encrypt {
                        let enc = cbc::Encryptor::<$a>::new_from_slices(key, iv)
                            .map_err(|e| ("ERR_CRYPTO_INVALID_KEYLEN", e.to_string()))?;
                        Ok(CipherOutput { data: enc.encrypt_padded_vec_mut::<Pkcs7>(data), tag: None })
                    } else {
                        let dec = cbc::Decryptor::<$a>::new_from_slices(key, iv)
                            .map_err(|e| ("ERR_CRYPTO_INVALID_KEYLEN", e.to_string()))?;
                        let pt = dec.decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|_| (
                            "ERR_CRYPTO_OPERATION_FAILED",
                            "bad decrypt (invalid padding)".to_string(),
                        ))?;
                        Ok(CipherOutput { data: pt, tag: None })
                    }
                }};
            }
            match key_len {
                16 => cbc_run!(aes::Aes128),
                24 => cbc_run!(aes::Aes192),
                _ => cbc_run!(aes::Aes256),
            }
        }
        AesMode::Ctr => {
            use aes::cipher::{KeyIvInit, StreamCipher};
            if iv.len() != 16 {
                return Err(("ERR_CRYPTO_INVALID_IV", "CTR requires a 16-byte IV".to_string()));
            }
            let mut buf = data.to_vec();
            macro_rules! ctr_run {
                ($a:ty) => {{
                    let mut c = ctr::Ctr128BE::<$a>::new_from_slices(key, iv)
                        .map_err(|e| ("ERR_CRYPTO_INVALID_KEYLEN", e.to_string()))?;
                    c.apply_keystream(&mut buf);
                }};
            }
            match key_len {
                16 => ctr_run!(aes::Aes128),
                24 => ctr_run!(aes::Aes192),
                _ => ctr_run!(aes::Aes256),
            }
            Ok(CipherOutput { data: buf, tag: None })
        }
    }
}

fn crypto_cipher(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(op), Some(algo), Some(key), Some(iv), Some(data)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        bytes_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
        bytes_arg(scope, &args, 4),
    ) else {
        return throw_bad_args(scope, "cryptoCipher");
    };
    let Ok(aad) = opt_bytes_arg(scope, &args, 5) else {
        return throw_bad_args(scope, "cryptoCipher");
    };
    let Ok(tag) = opt_bytes_arg(scope, &args, 6) else {
        return throw_bad_args(scope, "cryptoCipher");
    };
    let encrypt = match op.as_str() {
        "encrypt" => true,
        "decrypt" => false,
        _ => return throw_bad_args(scope, "cryptoCipher"),
    };
    match aes_run(encrypt, &algo, &key, &iv, &data, aad.as_deref(), tag.as_deref()) {
        Ok(out) => {
            let result = v8::Object::new(scope);
            let data_key = v8::String::new(scope, "data").unwrap();
            let data_val = make_uint8array(scope, out.data);
            result.set(scope, data_key.into(), data_val.into());
            if let Some(t) = out.tag {
                let tag_key = v8::String::new(scope, "tag").unwrap();
                let tag_val = make_uint8array(scope, t);
                result.set(scope, tag_key.into(), tag_val.into());
            }
            retval.set(result.into());
        }
        Err((code, msg)) => throw_coded(scope, code, &msg),
    }
}

// ---------------------------------------------------------------------------
// RSA
// ---------------------------------------------------------------------------

fn parse_rsa_private(pem: &str) -> Result<RsaPrivateKey, String> {
    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|e| format!("invalid RSA private key: {}", e))
}

fn parse_rsa_public(pem: &str) -> Result<RsaPublicKey, String> {
    if let Ok(k) = RsaPublicKey::from_public_key_pem(pem) {
        return Ok(k);
    }
    if let Ok(k) = RsaPublicKey::from_pkcs1_pem(pem) {
        return Ok(k);
    }
    // Accept a private key where a public key is expected (Node does).
    parse_rsa_private(pem)
        .map(|k| k.to_public_key())
        .map_err(|e| format!("invalid RSA public key: {}", e))
}

fn crypto_rsa_generate(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(bits) = num_arg(scope, &args, 0) else {
        return throw_bad_args(scope, "cryptoRsaGenerate");
    };
    let bits = bits as usize;
    if !(512..=8192).contains(&bits) {
        return throw_coded(scope, "ERR_OUT_OF_RANGE", "RSA modulus must be 512..8192 bits");
    }
    let mut rng = rand::thread_rng();
    let private = match RsaPrivateKey::new(&mut rng, bits) {
        Ok(k) => k,
        Err(e) => {
            return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", &format!("RSA keygen failed: {}", e));
        }
    };
    let public = private.to_public_key();
    let (Ok(private_pem), Ok(public_pem)) = (
        private.to_pkcs8_pem(LineEnding::LF).map(|p| p.to_string()),
        public.to_public_key_pem(LineEnding::LF),
    ) else {
        return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", "RSA key encoding failed");
    };
    retval.set(pem_pair_object(scope, &private_pem, &public_pem).into());
}

fn pem_pair_object<'s>(
    scope: &v8::PinScope<'s, '_>,
    private_pem: &str,
    public_pem: &str,
) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);
    let k1 = v8::String::new(scope, "privatePem").unwrap();
    let v1 = v8::String::new(scope, private_pem).unwrap();
    obj.set(scope, k1.into(), v1.into());
    let k2 = v8::String::new(scope, "publicPem").unwrap();
    let v2 = v8::String::new(scope, public_pem).unwrap();
    obj.set(scope, k2.into(), v2.into());
    obj
}

/// Compute (digest bytes, pkcs1v15 scheme, pss scheme) for the requested hash.
fn rsa_schemes(hash: &str, data: &[u8], salt_len: usize) -> Option<(Vec<u8>, Pkcs1v15Sign, rsa::pss::Pss)> {
    let digest = digest_bytes(hash, data)?;
    let (pkcs1, pss) = match hash {
        "sha1" => (Pkcs1v15Sign::new::<sha1::Sha1>(), rsa::pss::Pss::new_with_salt::<sha1::Sha1>(salt_len)),
        "sha224" => (Pkcs1v15Sign::new::<sha2::Sha224>(), rsa::pss::Pss::new_with_salt::<sha2::Sha224>(salt_len)),
        "sha256" => (Pkcs1v15Sign::new::<sha2::Sha256>(), rsa::pss::Pss::new_with_salt::<sha2::Sha256>(salt_len)),
        "sha384" => (Pkcs1v15Sign::new::<sha2::Sha384>(), rsa::pss::Pss::new_with_salt::<sha2::Sha384>(salt_len)),
        "sha512" => (Pkcs1v15Sign::new::<sha2::Sha512>(), rsa::pss::Pss::new_with_salt::<sha2::Sha512>(salt_len)),
        _ => return None,
    };
    Some((digest, pkcs1, pss))
}

fn crypto_rsa_sign(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(padding), Some(hash), Some(pem), Some(data)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
    ) else {
        return throw_bad_args(scope, "cryptoRsaSign");
    };
    let salt_len = num_arg(scope, &args, 4).map(|n| n as usize).unwrap_or(0);
    let key = match parse_rsa_private(&pem) {
        Ok(k) => k,
        Err(e) => return throw_coded(scope, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", &e),
    };
    let Some((digest, pkcs1, pss)) = rsa_schemes(&hash, &data, salt_len) else {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
    };
    let result = match padding.as_str() {
        "pkcs1" => key.sign(pkcs1, &digest),
        "pss" => key.sign_with_rng(&mut rand::thread_rng(), pss, &digest),
        _ => return throw_bad_args(scope, "cryptoRsaSign"),
    };
    match result {
        Ok(sig) => retval.set(make_uint8array(scope, sig).into()),
        Err(e) => throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", &format!("RSA sign failed: {}", e)),
    }
}

fn crypto_rsa_verify(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(padding), Some(hash), Some(pem), Some(data), Some(sig)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
        bytes_arg(scope, &args, 4),
    ) else {
        return throw_bad_args(scope, "cryptoRsaVerify");
    };
    let salt_len = num_arg(scope, &args, 5).map(|n| n as usize).unwrap_or(0);
    let key = match parse_rsa_public(&pem) {
        Ok(k) => k,
        Err(e) => return throw_coded(scope, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", &e),
    };
    let Some((digest, pkcs1, pss)) = rsa_schemes(&hash, &data, salt_len) else {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
    };
    let ok = match padding.as_str() {
        "pkcs1" => key.verify(pkcs1, &digest, &sig).is_ok(),
        "pss" => key.verify(pss, &digest, &sig).is_ok(),
        _ => return throw_bad_args(scope, "cryptoRsaVerify"),
    };
    retval.set(v8::Boolean::new(scope, ok).into());
}

fn oaep_for(hash: &str) -> Option<Oaep> {
    Some(match hash {
        "sha1" => Oaep::new::<sha1::Sha1>(),
        "sha224" => Oaep::new::<sha2::Sha224>(),
        "sha256" => Oaep::new::<sha2::Sha256>(),
        "sha384" => Oaep::new::<sha2::Sha384>(),
        "sha512" => Oaep::new::<sha2::Sha512>(),
        _ => return None,
    })
}

fn crypto_rsa_encrypt(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(padding), Some(hash), Some(pem), Some(data)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
    ) else {
        return throw_bad_args(scope, "cryptoRsaEncrypt");
    };
    let key = match parse_rsa_public(&pem) {
        Ok(k) => k,
        Err(e) => return throw_coded(scope, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", &e),
    };
    let mut rng = rand::thread_rng();
    let result = match padding.as_str() {
        "oaep" => match oaep_for(&hash) {
            Some(oaep) => key.encrypt(&mut rng, oaep, &data),
            None => {
                return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
            }
        },
        "pkcs1" => key.encrypt(&mut rng, Pkcs1v15Encrypt, &data),
        _ => return throw_bad_args(scope, "cryptoRsaEncrypt"),
    };
    match result {
        Ok(ct) => retval.set(make_uint8array(scope, ct).into()),
        Err(e) => throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", &format!("RSA encrypt failed: {}", e)),
    }
}

fn crypto_rsa_decrypt(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(padding), Some(hash), Some(pem), Some(data)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
    ) else {
        return throw_bad_args(scope, "cryptoRsaDecrypt");
    };
    let key = match parse_rsa_private(&pem) {
        Ok(k) => k,
        Err(e) => return throw_coded(scope, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", &e),
    };
    let result = match padding.as_str() {
        "oaep" => match oaep_for(&hash) {
            Some(oaep) => key.decrypt(oaep, &data),
            None => {
                return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
            }
        },
        "pkcs1" => key.decrypt(Pkcs1v15Encrypt, &data),
        _ => return throw_bad_args(scope, "cryptoRsaDecrypt"),
    };
    match result {
        Ok(pt) => retval.set(make_uint8array(scope, pt).into()),
        Err(e) => throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", &format!("RSA decrypt failed: {}", e)),
    }
}

// ---------------------------------------------------------------------------
// ECDSA (P-256 / P-384)
// ---------------------------------------------------------------------------

fn crypto_ec_generate(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(curve) = str_arg(scope, &args, 0) else {
        return throw_bad_args(scope, "cryptoEcGenerate");
    };
    let mut rng = rand::thread_rng();
    let pair = match curve.as_str() {
        "p256" => {
            let sk = p256::SecretKey::random(&mut rng);
            let pk = sk.public_key();
            (
                sk.to_pkcs8_pem(LineEnding::LF).map(|p| p.to_string()),
                pk.to_public_key_pem(LineEnding::LF),
            )
        }
        "p384" => {
            let sk = p384::SecretKey::random(&mut rng);
            let pk = sk.public_key();
            (
                sk.to_pkcs8_pem(LineEnding::LF).map(|p| p.to_string()),
                pk.to_public_key_pem(LineEnding::LF),
            )
        }
        _ => {
            return throw_coded(scope, "ERR_INVALID_ARG_VALUE", &format!("Unsupported curve: {}", curve));
        }
    };
    let (Ok(private_pem), Ok(public_pem)) = pair else {
        return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", "EC key encoding failed");
    };
    retval.set(pem_pair_object(scope, &private_pem, &public_pem).into());
}

fn crypto_ec_sign(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(curve), Some(hash), Some(pem), Some(data)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
    ) else {
        return throw_bad_args(scope, "cryptoEcSign");
    };
    let Some(digest) = digest_bytes(&hash, &data) else {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
    };
    let der: Result<Vec<u8>, String> = match curve.as_str() {
        "p256" => p256::SecretKey::from_pkcs8_pem(&pem)
            .map_err(|e| e.to_string())
            .and_then(|sk| {
                let signing = p256::ecdsa::SigningKey::from(&sk);
                let sig: p256::ecdsa::Signature =
                    signing.sign_prehash(&digest).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }),
        "p384" => p384::SecretKey::from_pkcs8_pem(&pem)
            .map_err(|e| e.to_string())
            .and_then(|sk| {
                let signing = p384::ecdsa::SigningKey::from(&sk);
                let sig: p384::ecdsa::Signature =
                    signing.sign_prehash(&digest).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }),
        _ => Err(format!("Unsupported curve: {}", curve)),
    };
    match der {
        Ok(sig) => retval.set(make_uint8array(scope, sig).into()),
        Err(e) => throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", &format!("EC sign failed: {}", e)),
    }
}

fn crypto_ec_verify(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(curve), Some(hash), Some(pem), Some(data), Some(sig)) = (
        str_arg(scope, &args, 0),
        str_arg(scope, &args, 1),
        str_arg(scope, &args, 2),
        bytes_arg(scope, &args, 3),
        bytes_arg(scope, &args, 4),
    ) else {
        return throw_bad_args(scope, "cryptoEcVerify");
    };
    let Some(digest) = digest_bytes(&hash, &data) else {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_DIGEST", &format!("Invalid digest: {}", hash));
    };
    let ok = match curve.as_str() {
        "p256" => p256::PublicKey::from_public_key_pem(&pem)
            .ok()
            .and_then(|pk| {
                let verifying = p256::ecdsa::VerifyingKey::from(&pk);
                let sig = p256::ecdsa::Signature::from_der(&sig).ok()?;
                Some(verifying.verify_prehash(&digest, &sig).is_ok())
            })
            .unwrap_or(false),
        "p384" => p384::PublicKey::from_public_key_pem(&pem)
            .ok()
            .and_then(|pk| {
                let verifying = p384::ecdsa::VerifyingKey::from(&pk);
                let sig = p384::ecdsa::Signature::from_der(&sig).ok()?;
                Some(verifying.verify_prehash(&digest, &sig).is_ok())
            })
            .unwrap_or(false),
        _ => {
            return throw_coded(scope, "ERR_INVALID_ARG_VALUE", &format!("Unsupported curve: {}", curve));
        }
    };
    retval.set(v8::Boolean::new(scope, ok).into());
}

// ---------------------------------------------------------------------------
// Ed25519 (via ring)
// ---------------------------------------------------------------------------

fn crypto_ed25519_generate(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    use ring::signature::KeyPair;
    let rng = ring::rand::SystemRandom::new();
    let Ok(pkcs8) = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng) else {
        return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", "Ed25519 keygen failed");
    };
    let Ok(pair) = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()) else {
        return throw_coded(scope, "ERR_CRYPTO_OPERATION_FAILED", "Ed25519 keygen failed");
    };
    let obj = v8::Object::new(scope);
    let k1 = v8::String::new(scope, "privatePkcs8").unwrap();
    let v1 = make_uint8array(scope, pkcs8.as_ref().to_vec());
    obj.set(scope, k1.into(), v1.into());
    let k2 = v8::String::new(scope, "publicRaw").unwrap();
    let v2 = make_uint8array(scope, pair.public_key().as_ref().to_vec());
    obj.set(scope, k2.into(), v2.into());
    retval.set(obj.into());
}

fn crypto_ed25519_sign(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(pkcs8), Some(data)) = (bytes_arg(scope, &args, 0), bytes_arg(scope, &args, 1)) else {
        return throw_bad_args(scope, "cryptoEd25519Sign");
    };
    let Ok(pair) = ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8) else {
        return throw_coded(scope, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", "invalid Ed25519 PKCS#8 key");
    };
    let sig = pair.sign(&data);
    retval.set(make_uint8array(scope, sig.as_ref().to_vec()).into());
}

fn crypto_ed25519_verify(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(public_raw), Some(data), Some(sig)) = (
        bytes_arg(scope, &args, 0),
        bytes_arg(scope, &args, 1),
        bytes_arg(scope, &args, 2),
    ) else {
        return throw_bad_args(scope, "cryptoEd25519Verify");
    };
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &public_raw);
    let ok = key.verify(&data, &sig).is_ok();
    retval.set(v8::Boolean::new(scope, ok).into());
}
