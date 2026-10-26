/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use ring::signature::{
    ECDSA_P256_SHA256_FIXED, ECDSA_P256_SHA256_FIXED_SIGNING, ED25519, EcdsaKeyPair,
    Ed25519KeyPair, UnparsedPublicKey,
};

use crate::{Algorithm, CoseError, Signer, Verifier};

/// ES256 public key data (P-256).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Es256PublicKey<'a> {
    /// x-coordinate, `LEN` bytes.
    pub x: &'a [u8],
    /// y-coordinate, `LEN` bytes.
    pub y: &'a [u8],
}

/// ES256 private key data (P-256).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Es256PrivateKey<'a> {
    /// Private scalar, `LEN` bytes.
    pub d: &'a [u8],
    /// Public x-coordinate, `LEN` bytes.
    pub x: &'a [u8],
    /// Public y-coordinate, `LEN` bytes.
    pub y: &'a [u8],
}

impl Es256PrivateKey<'_> {
    /// Length in bytes of the private scalar and each public coordinate.
    pub const LEN: usize = 32;
}

impl Es256PublicKey<'_> {
    /// Length in bytes of each public coordinate.
    pub const LEN: usize = 32;
}

pub(crate) fn fill_uncompressed_p256<'a>(
    x: &[u8],
    y: &[u8],
    out: &'a mut [u8; 65],
) -> Result<&'a [u8], CoseError> {
    if x.len() != Es256PublicKey::LEN || y.len() != Es256PublicKey::LEN {
        return Err(CoseError::InvalidKey);
    }
    out[0] = 0x04;
    out[1..33].copy_from_slice(x);
    out[33..65].copy_from_slice(y);
    Ok(&out[..])
}

/// Sign a Sig_structure using ES256 and return fixed-width signature (r||s).
pub fn sign_es256(
    key: &Es256PrivateKey<'_>,
    to_be_signed: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    if key.d.len() != Es256PrivateKey::LEN {
        return Err(CoseError::InvalidKey);
    }
    if out.len() < 64 {
        return Err(CoseError::Encode);
    }
    let mut uncompressed = [0u8; 65];
    let public_key = fill_uncompressed_p256(key.x, key.y, &mut uncompressed)?;
    let rng = ring::rand::SystemRandom::new();
    let key_pair = EcdsaKeyPair::from_private_key_and_public_key(
        &ECDSA_P256_SHA256_FIXED_SIGNING,
        key.d,
        public_key,
        &rng,
    )?;
    let sig = key_pair
        .sign(&rng, to_be_signed)
        .map_err(|_| CoseError::InvalidKey)?;
    let sig_bytes = sig.as_ref();
    out[..sig_bytes.len()].copy_from_slice(sig_bytes);
    Ok(sig_bytes.len())
}

/// Verify an ES256 Sig_structure signature.
pub fn verify_es256(
    key: &Es256PublicKey<'_>,
    to_be_signed: &[u8],
    signature: &[u8],
) -> Result<(), CoseError> {
    if signature.len() != 64 {
        return Err(CoseError::InvalidStructure);
    }
    let mut uncompressed = [0u8; 65];
    let public_key = fill_uncompressed_p256(key.x, key.y, &mut uncompressed)?;
    let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key);
    verifier
        .verify(to_be_signed, signature)
        .map_err(|_| CoseError::InvalidSignature)
}

/// ES256 signer adapter for ring backend.
pub struct Es256Signer<'a> {
    key: &'a Es256PrivateKey<'a>,
}

impl<'a> Es256Signer<'a> {
    /// Create a signer wrapping the given ES256 private key.
    pub const fn new(key: &'a Es256PrivateKey<'a>) -> Self {
        Self { key }
    }
}

impl Signer for Es256Signer<'_> {
    fn algorithm(&self) -> Algorithm {
        Algorithm::Es256
    }

    fn sign(&self, to_be_signed: &[u8], out: &mut [u8]) -> Result<usize, CoseError> {
        sign_es256(self.key, to_be_signed, out)
    }
}

/// ES256 verifier adapter for ring backend.
pub struct Es256Verifier<'a> {
    key: &'a Es256PublicKey<'a>,
}

impl<'a> Es256Verifier<'a> {
    /// Create a verifier wrapping the given ES256 public key.
    pub const fn new(key: &'a Es256PublicKey<'a>) -> Self {
        Self { key }
    }
}

impl Verifier for Es256Verifier<'_> {
    fn algorithm(&self) -> Algorithm {
        Algorithm::Es256
    }

    fn verify(&self, to_be_signed: &[u8], signature: &[u8]) -> Result<(), CoseError> {
        verify_es256(self.key, to_be_signed, signature)
    }
}

/// EdDSA public key data (Ed25519).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdDsaPublicKey<'a> {
    /// Public key bytes, `LEN` bytes.
    pub x: &'a [u8],
}

/// EdDSA private key data (Ed25519).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdDsaPrivateKey<'a> {
    /// Private seed, `LEN` bytes.
    pub d: &'a [u8],
    /// Public key bytes, `LEN` bytes.
    pub x: &'a [u8],
}

impl EdDsaPublicKey<'_> {
    /// Length in bytes of the public key.
    pub const LEN: usize = 32;
}

impl EdDsaPrivateKey<'_> {
    /// Length in bytes of the private seed.
    pub const LEN: usize = 32;
}

/// Sign a Sig_structure using EdDSA (Ed25519).
pub fn sign_eddsa(
    key: &EdDsaPrivateKey<'_>,
    to_be_signed: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    if key.d.len() != EdDsaPrivateKey::LEN || key.x.len() != EdDsaPublicKey::LEN {
        return Err(CoseError::InvalidKey);
    }
    if out.len() < 64 {
        return Err(CoseError::Encode);
    }
    let key_pair = Ed25519KeyPair::from_seed_and_public_key(key.d, key.x)
        .map_err(|_| CoseError::InvalidKey)?;
    let sig = key_pair.sign(to_be_signed);
    let sig_bytes = sig.as_ref();
    out[..sig_bytes.len()].copy_from_slice(sig_bytes);
    Ok(sig_bytes.len())
}

/// Verify an EdDSA Sig_structure signature (Ed25519).
pub fn verify_eddsa(
    key: &EdDsaPublicKey<'_>,
    to_be_signed: &[u8],
    signature: &[u8],
) -> Result<(), CoseError> {
    if key.x.len() != EdDsaPublicKey::LEN || signature.len() != 64 {
        return Err(CoseError::InvalidStructure);
    }
    let verifier = UnparsedPublicKey::new(&ED25519, key.x);
    verifier
        .verify(to_be_signed, signature)
        .map_err(|_| CoseError::InvalidSignature)
}

/// EdDSA signer adapter for ring backend.
pub struct EdDsaSigner<'a> {
    key: &'a EdDsaPrivateKey<'a>,
}

impl<'a> EdDsaSigner<'a> {
    /// Create a signer wrapping the given EdDSA private key.
    pub const fn new(key: &'a EdDsaPrivateKey<'a>) -> Self {
        Self { key }
    }
}

impl Signer for EdDsaSigner<'_> {
    fn algorithm(&self) -> Algorithm {
        Algorithm::EdDsa
    }

    fn sign(&self, to_be_signed: &[u8], out: &mut [u8]) -> Result<usize, CoseError> {
        sign_eddsa(self.key, to_be_signed, out)
    }
}

/// EdDSA verifier adapter for ring backend.
pub struct EdDsaVerifier<'a> {
    key: &'a EdDsaPublicKey<'a>,
}

impl<'a> EdDsaVerifier<'a> {
    /// Create a verifier wrapping the given EdDSA public key.
    pub const fn new(key: &'a EdDsaPublicKey<'a>) -> Self {
        Self { key }
    }
}

impl Verifier for EdDsaVerifier<'_> {
    fn algorithm(&self) -> Algorithm {
        Algorithm::EdDsa
    }

    fn verify(&self, to_be_signed: &[u8], signature: &[u8]) -> Result<(), CoseError> {
        verify_eddsa(self.key, to_be_signed, signature)
    }
}
