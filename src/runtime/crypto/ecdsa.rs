//! ECDSA signing/verification implementation
//!
//! WebCrypto ECDSA using the `p256` and `p384` crates.

use p256::{
    ecdsa::{SigningKey as P256SigningKey, VerifyingKey as P256VerifyingKey, Signature as P256Signature},
    SecretKey as P256SecretKey,
};
use p384::{
    ecdsa::{SigningKey as P384SigningKey, VerifyingKey as P384VerifyingKey, Signature as P384Signature},
    SecretKey as P384SecretKey,
};
use signature::{Signer, Verifier};
use crate::runtime::crypto::{CryptoKey, CryptoError, KeyUsage, HashAlgorithm};

/// ECDSA algorithm parameters
#[derive(Debug, Clone)]
pub struct EcdsaParams {
    pub named_curve: String, // "P-256" or "P-384"
    pub hash: HashAlgorithm,
}

/// Generate a new ECDSA key pair
///
/// WebCrypto: generateKey with algorithm "ECDSA" or "ECDH"
pub fn generate_key(
    named_curve: &str,
    algorithm: &str,
    extractable: bool,
    usages: Vec<KeyUsage>,
) -> Result<CryptoKey, CryptoError> {
    match named_curve {
        "P-256" => {
            // Generate P-256 key pair
            let secret_key = P256SecretKey::random(&mut rand::rngs::OsRng);
            let signing_key = P256SigningKey::from(&secret_key);

            // Serialize private key to PKCS#8
            let private_key_bytes = secret_key.to_sec1_der()
                .map_err(|_| CryptoError::OperationFailed)?;

            // Create CryptoKey
            let alg = if algorithm == "ECDH" {
                crate::runtime::crypto::AlgorithmIdentifier::Ecdh {
                    named_curve: named_curve.to_string(),
                }
            } else {
                crate::runtime::crypto::AlgorithmIdentifier::Ecdsa {
                    named_curve: named_curve.to_string(),
                    hash: HashAlgorithm::Sha256, // Default hash
                }
            };

            Ok(CryptoKey::new_ecdsa_private(
                alg,
                private_key_bytes.to_vec(),
                extractable,
                usages,
            ))
        }
        "P-384" => {
            // Generate P-384 key pair
            let secret_key = P384SecretKey::random(&mut rand::rngs::OsRng);
            let signing_key = P384SigningKey::from(&secret_key);

            // Serialize private key to PKCS#8
            let private_key_bytes = secret_key.to_sec1_der()
                .map_err(|_| CryptoError::OperationFailed)?;

            let alg = if algorithm == "ECDH" {
                crate::runtime::crypto::AlgorithmIdentifier::Ecdh {
                    named_curve: named_curve.to_string(),
                }
            } else {
                crate::runtime::crypto::AlgorithmIdentifier::Ecdsa {
                    named_curve: named_curve.to_string(),
                    hash: HashAlgorithm::Sha384, // Default hash for P-384
                }
            };

            Ok(CryptoKey::new_ecdsa_private(
                alg,
                private_key_bytes.to_vec(),
                extractable,
                usages,
            ))
        }
        _ => Err(CryptoError::InvalidAlgorithm(format!(
            "Unsupported named curve: {}", named_curve
        ))),
    }
}

/// Import an ECDSA key from JWK or PKCS#8/SPKI format
///
/// WebCrypto: importKey with format "jwk", "pkcs8", "spki"
pub fn import_key(
    format: &str,
    key_data: &[u8],
    algorithm: &str,
    named_curve: &str,
    extractable: bool,
    usages: Vec<KeyUsage>,
) -> Result<CryptoKey, CryptoError> {
    match format {
        "pkcs8" => {
            // Import private key - try SEC1 format first (what we store internally)
            // Note: SEC1 is the standard format for EC private keys
            match named_curve {
                "P-256" => {
                    // Validate by trying to parse as SEC1
                    let _secret_key = P256SecretKey::from_sec1_der(key_data)
                        .map_err(|e| CryptoError::DataError(format!("Failed to parse SEC1 key: {}", e)))?;

                    let alg = if algorithm == "ECDH" {
                        crate::runtime::crypto::AlgorithmIdentifier::Ecdh {
                            named_curve: named_curve.to_string(),
                        }
                    } else {
                        crate::runtime::crypto::AlgorithmIdentifier::Ecdsa {
                            named_curve: named_curve.to_string(),
                            hash: HashAlgorithm::Sha256,
                        }
                    };

                    Ok(CryptoKey::new_ecdsa_private(
                        alg,
                        key_data.to_vec(),
                        extractable,
                        usages,
                    ))
                }
                "P-384" => {
                    // Validate by trying to parse as SEC1
                    let _secret_key = P384SecretKey::from_sec1_der(key_data)
                        .map_err(|e| CryptoError::DataError(format!("Failed to parse SEC1 key: {}", e)))?;

                    let alg = if algorithm == "ECDH" {
                        crate::runtime::crypto::AlgorithmIdentifier::Ecdh {
                            named_curve: named_curve.to_string(),
                        }
                    } else {
                        crate::runtime::crypto::AlgorithmIdentifier::Ecdsa {
                            named_curve: named_curve.to_string(),
                            hash: HashAlgorithm::Sha384,
                        }
                    };

                    Ok(CryptoKey::new_ecdsa_private(
                        alg,
                        key_data.to_vec(),
                        extractable,
                        usages,
                    ))
                }
                _ => Err(CryptoError::InvalidAlgorithm(format!(
                    "Unsupported named curve: {}", named_curve
                ))),
            }
        }
        "spki" => {
            // Public key import (not implemented for now)
            Err(CryptoError::NotSupported)
        }
        "jwk" => {
            // JWK import (not implemented for now)
            Err(CryptoError::NotSupported)
        }
        _ => Err(CryptoError::NotSupported),
    }
}

/// Sign data using ECDSA
///
/// WebCrypto: sign with algorithm "ECDSA"
pub fn sign(
    key: &CryptoKey,
    data: &[u8],
    params: &EcdsaParams,
) -> Result<Vec<u8>, CryptoError> {
    let private_key_bytes = match key.handle.as_ref() {
        crate::runtime::crypto::crypto_key::CryptoKeyHandle::EcdsaPrivateKey(bytes) => bytes,
        _ => return Err(CryptoError::InvalidKey),
    };

    match params.named_curve.as_str() {
        "P-256" => {
            // Parse private key
            let secret_key = P256SecretKey::from_sec1_der(private_key_bytes)
                .map_err(|_| CryptoError::InvalidKey)?;
            let signing_key = P256SigningKey::from(&secret_key);

            // Sign the data (pre-hashed)
            let signature: P256Signature = signing_key.sign(data);
            Ok(signature.to_bytes().to_vec())
        }
        "P-384" => {
            // Parse private key
            let secret_key = P384SecretKey::from_sec1_der(private_key_bytes)
                .map_err(|_| CryptoError::InvalidKey)?;
            let signing_key = P384SigningKey::from(&secret_key);

            // Sign the data (pre-hashed)
            let signature: P384Signature = signing_key.sign(data);
            Ok(signature.to_bytes().to_vec())
        }
        _ => Err(CryptoError::InvalidAlgorithm(params.named_curve.clone())),
    }
}

/// Verify an ECDSA signature
///
/// WebCrypto: verify with algorithm "ECDSA"
pub fn verify(
    key: &CryptoKey,
    signature: &[u8],
    data: &[u8],
    params: &EcdsaParams,
) -> Result<bool, CryptoError> {
    // For verification, we need the public key
    // Currently only supporting private key import which has the public component
    let private_key_bytes = match key.handle.as_ref() {
        crate::runtime::crypto::crypto_key::CryptoKeyHandle::EcdsaPrivateKey(bytes) => bytes,
        _ => return Err(CryptoError::InvalidKey),
    };

    match params.named_curve.as_str() {
        "P-256" => {
            // Parse private key and derive public key
            let secret_key = P256SecretKey::from_sec1_der(private_key_bytes)
                .map_err(|_| CryptoError::InvalidKey)?;
            let signing_key = P256SigningKey::from(&secret_key);
            let verifying_key = P256VerifyingKey::from(&signing_key);

            // Parse the signature
            let sig = P256Signature::try_from(signature)
                .map_err(|_| CryptoError::DataError("Invalid signature".to_string()))?;

            // Verify
            match verifying_key.verify(data, &sig) {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            }
        }
        "P-384" => {
            // Parse private key and derive public key
            let secret_key = P384SecretKey::from_sec1_der(private_key_bytes)
                .map_err(|_| CryptoError::InvalidKey)?;
            let signing_key = P384SigningKey::from(&secret_key);
            let verifying_key = P384VerifyingKey::from(&signing_key);

            // Parse the signature
            let sig = P384Signature::from_slice(signature)
                .map_err(|_| CryptoError::DataError("Invalid signature".to_string()))?;

            // Verify
            match verifying_key.verify(data, &sig) {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            }
        }
        _ => Err(CryptoError::InvalidAlgorithm(params.named_curve.clone())),
    }
}

/// Perform ECDH key agreement
///
/// WebCrypto: deriveKey with algorithm "ECDH"
pub fn derive_bits(
    private_key: &CryptoKey,
    public_key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>, CryptoError> {
    // ECDH implementation using p256/p384 ECDH
    // This is a simplified placeholder - full ECDH requires proper coordinate extraction
    Err(CryptoError::NotSupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecdsa_generate_key_p256() {
        let key = generate_key(
            "P-256",
            "ECDSA",
            true,
            vec![KeyUsage::Sign, KeyUsage::Verify],
        );
        assert!(key.is_ok());
        assert_eq!(key.unwrap().key_type(), "private");
    }

    #[test]
    fn test_ecdsa_generate_key_p384() {
        let key = generate_key(
            "P-384",
            "ECDSA",
            true,
            vec![KeyUsage::Sign, KeyUsage::Verify],
        );
        assert!(key.is_ok());
        assert_eq!(key.unwrap().key_type(), "private");
    }
}
