/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]

//! COSE support for no_std targets.
//!
//! This crate focuses on COSE_Sign1 with strict header semantics and
//! crypto-provider agnostic signing/verification traits.

use thiserror::Error;

use tps_minicbor::{DecodeError, EncodeError};

#[cfg(feature = "ring")]
pub mod crypto;
pub mod encrypt0;
pub mod header;
pub mod key;
#[cfg(feature = "cose_sign")]
pub mod sign;
pub mod sign1;

// Re-export header types
pub use header::{ContentType, HeaderParam, HeaderValue, Label, ParsedHeaders, Payload};

// Re-export sign1 types and functions
pub use sign1::{
    MissingAlgorithm, MissingPayload, Sign1, Sign1Builder, WithAlgorithm, WithPayload,
    decode_sign1, encode_sig_structure, verify_sign1,
};

#[cfg(feature = "ring")]
pub use sign1::{verify_sign1_eddsa, verify_sign1_es256};

// Re-export sign types and functions (cose_sign feature)
#[cfg(feature = "cose_sign")]
pub use sign::{
    MissingSigners, Sign, SignBuilder, SignSignature, SignSignatureIter, SignerEntry, WithSigners,
    decode_sign, encode_sig_structure_sign, verify_sign,
};

#[cfg(all(feature = "cose_sign", feature = "ring"))]
pub use sign::{verify_sign_eddsa, verify_sign_es256};

// Re-export encrypt0 types and functions
pub use encrypt0::{
    Encrypt0, Encrypt0Builder, MissingNonce, WithNonce, decode_encrypt0, encode_enc_structure,
};

#[cfg(feature = "ring")]
pub use encrypt0::{decrypt_encrypt0_with_cose_key, decrypt_encrypt0_with_raw_key};

// Re-export key types and functions
pub use key::{
    CoseKey, Curve, Ec2Key, Ec2Y, KeyCommon, KeyType, OkpKey, SymmetricKey, decode_cose_key,
};

// Re-export crypto types (ring feature)
#[cfg(feature = "ring")]
pub use crypto::ring::{
    EdDsaPrivateKey, EdDsaPublicKey, EdDsaSigner, EdDsaVerifier, Es256PrivateKey, Es256PublicKey,
    Es256Signer, Es256Verifier, sign_eddsa, sign_es256, verify_eddsa, verify_es256,
};

/// Errors returned by COSE operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoseError {
    /// COSE structure did not match the expected layout.
    #[error("invalid COSE structure")]
    InvalidStructure,
    /// A header label appeared more than once.
    #[error("duplicate header label")]
    DuplicateHeaderLabel,
    /// No algorithm header was present.
    #[error("missing algorithm")]
    MissingAlgorithm,
    /// A required payload was not provided.
    #[error("missing payload")]
    MissingPayload,
    /// An IV or nonce was required but not present.
    #[error("missing nonce")]
    MissingNonce,
    /// Ciphertext was required but not present.
    #[error("missing ciphertext")]
    MissingCiphertext,
    /// The algorithm identifier is not supported.
    #[error("unsupported algorithm")]
    UnsupportedAlgorithm,
    /// A header label had an unsupported type.
    #[error("unsupported label type")]
    UnsupportedLabelType,
    /// A header parameter value is invalid or contradictory.
    #[error("invalid COSE header")]
    InvalidHeader,
    /// A header parameter value had the wrong CBOR type.
    #[error("invalid COSE header value")]
    InvalidHeaderValue,
    /// Signature verification failed.
    #[error("invalid signature")]
    InvalidSignature,
    /// Key data is invalid or incompatible with the algorithm.
    #[error("invalid key")]
    InvalidKey,
    /// Signer/verifier algorithm does not match the header.
    #[error("algorithm mismatch")]
    AlgorithmMismatch,
    /// A critical header extension is not understood.
    #[error("unknown critical header")]
    UnknownCriticalHeader,
    /// CBOR encoding failed.
    #[error("CBOR encoding failed")]
    Encode,
    /// CBOR decoding failed.
    #[error("CBOR decoding failed")]
    Decode,
    /// A header map had more entries than the crate's internal limit
    /// (`MAX_HEADER_ENTRIES`, currently 256) allows.
    #[error("too many header parameters")]
    TooManyHeaderParameters,
}

impl From<EncodeError> for CoseError {
    fn from(_: EncodeError) -> Self {
        Self::Encode
    }
}

impl From<DecodeError> for CoseError {
    fn from(_: DecodeError) -> Self {
        Self::Decode
    }
}

#[cfg(feature = "ring")]
impl From<ring::error::KeyRejected> for CoseError {
    fn from(_: ring::error::KeyRejected) -> Self {
        Self::InvalidKey
    }
}

#[cfg(feature = "ring")]
impl From<ring::error::Unspecified> for CoseError {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::InvalidSignature
    }
}

/// COSE signing algorithm identifier (RFC 9053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// ECDSA with P-256 and SHA-256 (algorithm value -7).
    Es256,
    /// EdDSA with Ed25519 (algorithm value -8).
    EdDsa,
}

impl Algorithm {
    /// Return the COSE integer algorithm identifier.
    #[must_use]
    pub const fn to_i64(self) -> i64 {
        match self {
            Algorithm::Es256 => -7,
            Algorithm::EdDsa => -8,
        }
    }

    /// Parse an algorithm from its COSE integer identifier, returning `None` for unknown values.
    #[must_use]
    pub const fn try_from_i64(alg: i64) -> Option<Self> {
        match alg {
            -7 => Some(Algorithm::Es256),
            -8 => Some(Algorithm::EdDsa),
            _ => None,
        }
    }
}

/// COSE encryption algorithm identifier (RFC 9053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptAlgorithm {
    /// AES-GCM with 128-bit key (algorithm value 1).
    A128Gcm,
    /// AES-CCM, 16-byte nonce, 64-bit tag, 128-bit key (algorithm value 10).
    AesCcm16_64_128,
    /// AES-CCM, 16-byte nonce, 64-bit tag, 256-bit key (algorithm value 11).
    AesCcm16_64_256,
    /// AES-CCM, 7-byte nonce, 64-bit tag, 128-bit key (algorithm value 12).
    AesCcm64_64_128,
    /// AES-CCM, 7-byte nonce, 64-bit tag, 256-bit key (algorithm value 13).
    AesCcm64_64_256,
    /// AES-CCM, 16-byte nonce, 128-bit tag, 128-bit key (algorithm value 30).
    AesCcm16_128_128,
    /// AES-CCM, 16-byte nonce, 128-bit tag, 256-bit key (algorithm value 31).
    AesCcm16_128_256,
    /// AES-CCM, 7-byte nonce, 128-bit tag, 128-bit key (algorithm value 32).
    AesCcm64_128_128,
    /// AES-CCM, 7-byte nonce, 128-bit tag, 256-bit key (algorithm value 33).
    AesCcm64_128_256,
}

impl EncryptAlgorithm {
    /// Return the COSE integer algorithm identifier.
    #[must_use]
    pub const fn to_i64(self) -> i64 {
        match self {
            EncryptAlgorithm::A128Gcm => 1,
            EncryptAlgorithm::AesCcm16_64_128 => 10,
            EncryptAlgorithm::AesCcm16_64_256 => 11,
            EncryptAlgorithm::AesCcm64_64_128 => 12,
            EncryptAlgorithm::AesCcm64_64_256 => 13,
            EncryptAlgorithm::AesCcm16_128_128 => 30,
            EncryptAlgorithm::AesCcm16_128_256 => 31,
            EncryptAlgorithm::AesCcm64_128_128 => 32,
            EncryptAlgorithm::AesCcm64_128_256 => 33,
        }
    }

    /// Convert a COSE algorithm integer identifier into an [`EncryptAlgorithm`], if recognised.
    #[must_use]
    pub const fn try_from_i64(alg: i64) -> Option<Self> {
        match alg {
            1 => Some(EncryptAlgorithm::A128Gcm),
            10 => Some(EncryptAlgorithm::AesCcm16_64_128),
            11 => Some(EncryptAlgorithm::AesCcm16_64_256),
            12 => Some(EncryptAlgorithm::AesCcm64_64_128),
            13 => Some(EncryptAlgorithm::AesCcm64_64_256),
            30 => Some(EncryptAlgorithm::AesCcm16_128_128),
            31 => Some(EncryptAlgorithm::AesCcm16_128_256),
            32 => Some(EncryptAlgorithm::AesCcm64_128_128),
            33 => Some(EncryptAlgorithm::AesCcm64_128_256),
            _ => None,
        }
    }
}

/// Abstraction over a COSE signing backend.
///
/// Implement this trait to plug in a custom signing key. The `sign` method
/// takes the pre-hashed Sig_structure bytes and writes a signature into `out`.
///
/// # Examples
///
/// See [`crate::crypto::ring::Es256Signer`] for a concrete implementation.
pub trait Signer {
    /// Return the algorithm this signer uses.
    #[must_use]
    fn algorithm(&self) -> Algorithm;
    /// Sign `to_be_signed` and write the signature into `out`, returning the number of bytes written.
    #[must_use = "returns the signature length"]
    fn sign(&self, to_be_signed: &[u8], out: &mut [u8]) -> Result<usize, CoseError>;
}

/// Abstraction over a COSE signature verification backend.
///
/// Implement this trait to plug in a custom verification key.
pub trait Verifier {
    /// Return the algorithm this verifier uses.
    #[must_use]
    fn algorithm(&self) -> Algorithm;
    /// Verify that `signature` is a valid signature over `to_be_signed`.
    #[must_use = "returns an error if verification fails"]
    fn verify(&self, to_be_signed: &[u8], signature: &[u8]) -> Result<(), CoseError>;
}
