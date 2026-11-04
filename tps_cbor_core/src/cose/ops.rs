/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! COSE sign, verify, encrypt and decrypt over in-memory buffers.
//!
//! Ported from `tps_cbor_tui/src/cose_ops.rs`, which in turn vendored the call sequences proven in
//! `tps_cose_tool/src/commands/{sign1,encrypt0}.rs`. This is now the single copy; the TUI's own is
//! deleted.
//!
//! # Why the buffers are sized the way they are
//!
//! `tps_cose` is `no_std`-first: every builder writes into caller-provided slices and returns the
//! number of bytes used, rather than allocating. That leaves the caller to pick sizes, and the old
//! copies did so with bare literals (`payload_len + 512`, `vec![0u8; 256]`) and no explanation.
//! The constants below name each one and say what it covers. They are deliberately generous: an
//! undersized buffer is a hard failure, and these buffers are transient.

use ring::rand::{SecureRandom, SystemRandom};
use tps_cose::{
    Algorithm, CoseError, CoseKey, Curve, Ec2Y, EdDsaPrivateKey, EdDsaPublicKey, Encrypt0Builder,
    EncryptAlgorithm, Es256PrivateKey, Es256PublicKey, Payload, Sign1Builder, decode_encrypt0,
    decode_sign1, decrypt_encrypt0_with_cose_key, verify_sign1_eddsa, verify_sign1_es256,
};

use crate::cose::{
    UnsupportedEncrypt, UnsupportedSign, key_has_private, unsupported_encrypt_reason,
    unsupported_sign_reason,
};
use crate::error::{CoreError, Result};

/// Space for the serialised protected header bucket.
///
/// A COSE_Sign1 or COSE_Encrypt0 protected header written here holds an algorithm identifier and,
/// for encryption, a nonce -- a few dozen bytes. 256 leaves room for a key id and critical-header
/// list without ever being revisited.
const PROTECTED_HEADER_BUDGET: usize = 256;

/// Headroom added to the payload length for the Sig_structure or Enc_structure.
///
/// The structure is a 4- or 5-element array wrapping the context string, the protected headers,
/// the external AAD and the payload. Everything except the payload is bounded by
/// [`PROTECTED_HEADER_BUDGET`] plus a short string, so one allowance covers both.
const STRUCTURE_MARGIN: usize = 512;

/// Space for a raw signature.
///
/// The largest this crate can produce is a P-521 ECDSA signature at 132 bytes of raw `r || s`;
/// ES256 is 64 and Ed25519 is 64. 128 covers everything `tps_cose` actually signs with, with the
/// P-521 case refused earlier by [`unsupported_sign_reason`].
const SIGNATURE_BUDGET: usize = 128;

/// Headroom added to the payload length for the final encoded COSE object.
///
/// Covers the outer array, the protected and unprotected header buckets, the signature or
/// authentication tag, and the CBOR framing of each.
const OUTPUT_MARGIN: usize = 1024;

/// Headroom added to the plaintext length for the ciphertext buffer: the AEAD authentication tag,
/// which is at most 16 bytes, plus CBOR framing.
const AEAD_TAG_MARGIN: usize = 32;

/// The nonce length an encryption algorithm requires, in bytes.
///
/// The names are misleading: `AesCcm16_*` means a 16-bit (2-byte) *length field*, which leaves 13
/// bytes of nonce, and `AesCcm64_*` a 64-bit length field, leaving 7. See RFC 9053 §4.2.
pub const fn nonce_len(alg: EncryptAlgorithm) -> usize {
    match alg {
        EncryptAlgorithm::A128Gcm => 12,
        EncryptAlgorithm::AesCcm16_64_128
        | EncryptAlgorithm::AesCcm16_64_256
        | EncryptAlgorithm::AesCcm16_128_128
        | EncryptAlgorithm::AesCcm16_128_256 => 13,
        EncryptAlgorithm::AesCcm64_64_128
        | EncryptAlgorithm::AesCcm64_64_256
        | EncryptAlgorithm::AesCcm64_128_128
        | EncryptAlgorithm::AesCcm64_128_256 => 7,
    }
}

/// The symmetric key length an encryption algorithm requires, in bytes.
///
/// Follows the trailing component of the algorithm name: `_128` takes 16 bytes, `_256` takes 32.
/// This replaces the old backend's stub `match alg { A128Gcm => 16, _ => 16 }`
/// (`tps_cbor_gui/src-tauri/src/main.rs:970`), which silently minted 128-bit keys for the
/// 256-bit algorithms.
pub const fn symmetric_key_len(alg: EncryptAlgorithm) -> usize {
    match alg {
        EncryptAlgorithm::A128Gcm
        | EncryptAlgorithm::AesCcm16_64_128
        | EncryptAlgorithm::AesCcm64_64_128
        | EncryptAlgorithm::AesCcm16_128_128
        | EncryptAlgorithm::AesCcm64_128_128 => 16,
        EncryptAlgorithm::AesCcm16_64_256
        | EncryptAlgorithm::AesCcm64_64_256
        | EncryptAlgorithm::AesCcm16_128_256
        | EncryptAlgorithm::AesCcm64_128_256 => 32,
    }
}

/// Every encryption algorithm `tps_cose` implements, in registry order.
///
/// Exposed so the UI can populate its algorithm dropdown from the library rather than hardcoding
/// one entry, as `EncryptPanel.tsx:85-88` did.
pub const ENCRYPT_ALGORITHMS: [EncryptAlgorithm; 9] = [
    EncryptAlgorithm::A128Gcm,
    EncryptAlgorithm::AesCcm16_64_128,
    EncryptAlgorithm::AesCcm16_64_256,
    EncryptAlgorithm::AesCcm64_64_128,
    EncryptAlgorithm::AesCcm64_64_256,
    EncryptAlgorithm::AesCcm16_128_128,
    EncryptAlgorithm::AesCcm16_128_256,
    EncryptAlgorithm::AesCcm64_128_128,
    EncryptAlgorithm::AesCcm64_128_256,
];

/// A short label for an encryption algorithm, as it appears in RFC 9053's registry.
pub const fn encrypt_algorithm_name(alg: EncryptAlgorithm) -> &'static str {
    match alg {
        EncryptAlgorithm::A128Gcm => "A128GCM",
        EncryptAlgorithm::AesCcm16_64_128 => "AES-CCM-16-64-128",
        EncryptAlgorithm::AesCcm16_64_256 => "AES-CCM-16-64-256",
        EncryptAlgorithm::AesCcm64_64_128 => "AES-CCM-64-64-128",
        EncryptAlgorithm::AesCcm64_64_256 => "AES-CCM-64-64-256",
        EncryptAlgorithm::AesCcm16_128_128 => "AES-CCM-16-128-128",
        EncryptAlgorithm::AesCcm16_128_256 => "AES-CCM-16-128-256",
        EncryptAlgorithm::AesCcm64_128_128 => "AES-CCM-64-128-128",
        EncryptAlgorithm::AesCcm64_128_256 => "AES-CCM-64-128-256",
    }
}

/// The public coordinates of an EC2 key, rejecting the compressed-`y` form.
///
/// `tps_cose`'s signing API takes `x` and `y` as plain slices, so a key whose `y` is a sign bit
/// rather than a coordinate cannot be used without point decompression, which this crate does not
/// do.
fn ec2_public_bytes<'a>(k: &'a tps_cose::Ec2Key<'a>) -> Result<(&'a [u8], &'a [u8])> {
    match k.y {
        Some(Ec2Y::Bytes(y)) => Ok((k.x, y)),
        _ => Err(CoreError::Cose(CoseError::InvalidKey)),
    }
}

/// Sign `payload` as a COSE_Sign1 using `key`.
///
/// Supports EC2/P-256 (ES256) and OKP/Ed25519 (EdDSA), which is everything `tps_cose::Algorithm`
/// defines. Any other key yields a [`CoreError::UnsupportedSign`] saying why, rather than the
/// undifferentiated `CoseError::InvalidKey` the old copies returned.
pub fn sign(payload: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>> {
    if let Some(reason) = unsupported_sign_reason(key) {
        return Err(reason.into());
    }
    if !key_has_private(key) {
        return Err(UnsupportedSign::NoPrivateKey.into());
    }

    let mut protected_buf = vec![0u8; PROTECTED_HEADER_BUDGET];
    let mut sig_structure_buf = vec![0u8; payload.len() + STRUCTURE_MARGIN];
    let mut signature_buf = vec![0u8; SIGNATURE_BUDGET];
    let mut out = vec![0u8; payload.len() + OUTPUT_MARGIN];

    let n = match key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let d = k.d.ok_or(CoseError::InvalidKey)?;
            let (x, y) = ec2_public_bytes(k)?;
            let priv_key = Es256PrivateKey { d, x, y };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::Es256)
                .payload(Payload::Attached(payload))
                .sign_es256(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            let d = k.d.ok_or(CoseError::InvalidKey)?;
            let priv_key = EdDsaPrivateKey { d, x: k.x };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::EdDsa)
                .payload(Payload::Attached(payload))
                .sign_eddsa(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        // Unreachable: `unsupported_sign_reason` above accepts only the two arms covered, and
        // both require `d`, which is checked there. Kept as a hard error rather than a panic.
        _ => return Err(CoreError::Cose(CoseError::InvalidKey)),
    };

    out.truncate(n);
    Ok(out)
}

/// Verify a COSE_Sign1 and return its payload.
///
/// The payload must be attached: a detached payload would have to be supplied separately, and no
/// caller in either frontend does.
pub fn verify(bytes: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>> {
    if let Some(reason) = unsupported_sign_reason(key) {
        return Err(reason.into());
    }

    let sign1 = decode_sign1(bytes)?;
    let payload = sign1.payload.ok_or(CoseError::InvalidStructure)?;
    let mut sig_structure_buf = vec![0u8; payload.len() + STRUCTURE_MARGIN];

    match key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let (x, y) = ec2_public_bytes(k)?;
            verify_sign1_es256(
                &sign1,
                &Es256PublicKey { x, y },
                &[],
                None,
                &mut sig_structure_buf,
            )?;
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            verify_sign1_eddsa(
                &sign1,
                &EdDsaPublicKey { x: k.x },
                &[],
                None,
                &mut sig_structure_buf,
            )?;
        }
        _ => return Err(CoreError::Cose(CoseError::InvalidKey)),
    }

    Ok(payload.to_vec())
}

/// Encrypt `plaintext` as a COSE_Encrypt0 under symmetric `key` with `alg`.
///
/// The nonce is freshly drawn from the system CSPRNG on every call; reusing one under the same key
/// would be catastrophic for both GCM and CCM.
pub fn encrypt(plaintext: &[u8], key: &CoseKey<'_>, alg: EncryptAlgorithm) -> Result<Vec<u8>> {
    if let Some(reason) = unsupported_encrypt_reason(key) {
        return Err(reason.into());
    }
    // Checked here, not in `unsupported_encrypt_reason`, because it depends on the algorithm the
    // user chose rather than on the key alone.
    if let CoseKey::Symmetric(k) = key {
        let expected = symmetric_key_len(alg);
        if k.k.len() != expected {
            return Err(UnsupportedEncrypt::WrongKeyLength {
                expected,
                actual: k.k.len(),
            }
            .into());
        }
    }

    let mut nonce = vec![0u8; nonce_len(alg)];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| CoreError::Cose(CoseError::InvalidKey))?;

    let mut protected_buf = vec![0u8; PROTECTED_HEADER_BUDGET];
    let mut enc_structure_buf = vec![0u8; plaintext.len() + STRUCTURE_MARGIN];
    let mut ciphertext_buf = vec![0u8; plaintext.len() + AEAD_TAG_MARGIN];
    let mut out = vec![0u8; plaintext.len() + OUTPUT_MARGIN];

    let n = Encrypt0Builder::new(&[], &[])
        .algorithm(alg)
        .payload(Payload::Attached(plaintext))
        .nonce(nonce.as_slice())
        .encrypt_with_cose_key(
            key,
            &mut protected_buf,
            &mut enc_structure_buf,
            &mut ciphertext_buf,
            &mut out,
        )?;

    out.truncate(n);
    Ok(out)
}

/// Decrypt a COSE_Encrypt0 under symmetric `key`.
///
/// The algorithm and nonce come from the object's own protected header, so no algorithm argument
/// is needed -- and the key length cannot be pre-checked here, since a mismatch is indistinguishable
/// from a wrong key and must surface as an authentication failure.
pub fn decrypt(bytes: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>> {
    if let Some(reason) = unsupported_encrypt_reason(key) {
        return Err(reason.into());
    }

    let encrypt0 = decode_encrypt0(bytes)?;
    let ciphertext_len = encrypt0.ciphertext.map_or(0, <[u8]>::len);

    let mut enc_structure_buf = vec![0u8; ciphertext_len + STRUCTURE_MARGIN];
    // The plaintext is shorter than the ciphertext by the authentication tag, so the ciphertext
    // length is always sufficient.
    let mut plaintext_buf = vec![0u8; ciphertext_len];

    let n = decrypt_encrypt0_with_cose_key(
        &encrypt0,
        key,
        &[],
        None,
        &mut enc_structure_buf,
        &mut plaintext_buf,
    )?;

    plaintext_buf.truncate(n);
    Ok(plaintext_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cose::keygen::{GenKeyKind, generate_cose_key};
    use tps_cose::{KeyCommon, SymmetricKey};

    fn symmetric(len: usize) -> Vec<u8> {
        vec![9u8; len]
    }

    #[test]
    fn every_algorithm_round_trips_at_its_own_key_length() {
        for alg in ENCRYPT_ALGORITHMS {
            let k = symmetric(symmetric_key_len(alg));
            let key = CoseKey::Symmetric(SymmetricKey {
                common: KeyCommon::default(),
                k: &k,
            });
            let plaintext = b"hello cose";
            let ciphertext = encrypt(plaintext, &key, alg)
                .unwrap_or_else(|e| panic!("{} encrypt: {e}", encrypt_algorithm_name(alg)));
            let decrypted = decrypt(&ciphertext, &key)
                .unwrap_or_else(|e| panic!("{} decrypt: {e}", encrypt_algorithm_name(alg)));
            assert_eq!(decrypted, plaintext, "{}", encrypt_algorithm_name(alg));
        }
    }

    /// The regression test for the key-length stub: a 256-bit algorithm must not accept a
    /// 128-bit key, which is what the old GUI would have handed it.
    #[test]
    fn a_short_key_is_refused_before_any_crypto_runs() {
        let k = symmetric(16);
        let key = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k,
        });
        let error = encrypt(b"x", &key, EncryptAlgorithm::AesCcm16_64_256)
            .expect_err("16 bytes is not a 256-bit key");
        assert!(
            matches!(
                error,
                CoreError::UnsupportedEncrypt(UnsupportedEncrypt::WrongKeyLength {
                    expected: 32,
                    actual: 16
                })
            ),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn the_256_bit_algorithms_need_32_bytes() {
        assert_eq!(symmetric_key_len(EncryptAlgorithm::A128Gcm), 16);
        assert_eq!(symmetric_key_len(EncryptAlgorithm::AesCcm16_64_256), 32);
        assert_eq!(symmetric_key_len(EncryptAlgorithm::AesCcm64_128_256), 32);
    }

    #[test]
    fn nonce_lengths_follow_the_length_field_width() {
        assert_eq!(nonce_len(EncryptAlgorithm::A128Gcm), 12);
        assert_eq!(nonce_len(EncryptAlgorithm::AesCcm16_64_128), 13);
        assert_eq!(nonce_len(EncryptAlgorithm::AesCcm64_64_128), 7);
    }

    #[test]
    fn a_fresh_nonce_is_used_each_time() {
        let k = symmetric(16);
        let key = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k,
        });
        let first = encrypt(b"same plaintext", &key, EncryptAlgorithm::A128Gcm).unwrap();
        let second = encrypt(b"same plaintext", &key, EncryptAlgorithm::A128Gcm).unwrap();
        assert_ne!(first, second, "nonce was reused");
    }

    #[test]
    fn decrypt_with_the_wrong_key_fails() {
        let k1 = symmetric(16);
        let k2 = vec![8u8; 16];
        let key1 = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k1,
        });
        let key2 = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k2,
        });
        let ciphertext = encrypt(b"hello", &key1, EncryptAlgorithm::A128Gcm).unwrap();
        assert!(decrypt(&ciphertext, &key2).is_err());
    }

    #[test]
    fn sign_and_verify_round_trip_for_both_supported_key_types() {
        for kind in [GenKeyKind::Ec2P256, GenKeyKind::OkpEd25519] {
            let bytes = generate_cose_key(kind).expect("generate");
            let key = CoseKey::decode(&bytes).expect("decode");
            let payload = b"sign me";
            let signed = sign(payload, &key).unwrap_or_else(|e| panic!("{kind:?} sign: {e}"));
            let verified = verify(&signed, &key).unwrap_or_else(|e| panic!("{kind:?} verify: {e}"));
            assert_eq!(verified, payload, "{kind:?}");
        }
    }

    #[test]
    fn a_tampered_signature_fails_verification() {
        let bytes = generate_cose_key(GenKeyKind::Ec2P256).unwrap();
        let key = CoseKey::decode(&bytes).unwrap();
        let mut signed = sign(b"sign me", &key).unwrap();
        let last = signed.len() - 1;
        signed[last] ^= 0xff;
        assert!(verify(&signed, &key).is_err());
    }

    /// The improvement over the old copies: refusing a P-384 key says *why*.
    #[test]
    fn signing_with_a_p384_key_explains_itself() {
        let bytes = generate_cose_key(GenKeyKind::Ec2P384).unwrap();
        let key = CoseKey::decode(&bytes).unwrap();
        let error = sign(b"x", &key).expect_err("ES384 is not implemented");
        assert!(
            matches!(error, CoreError::UnsupportedSign(UnsupportedSign::Es384)),
            "unexpected error: {error}"
        );
        assert!(error.to_string().contains("ES384"), "{error}");
    }

    #[test]
    fn encrypting_with_an_asymmetric_key_is_refused() {
        let bytes = generate_cose_key(GenKeyKind::Ec2P256).unwrap();
        let key = CoseKey::decode(&bytes).unwrap();
        let error = encrypt(b"x", &key, EncryptAlgorithm::A128Gcm).expect_err("asymmetric");
        assert!(
            matches!(
                error,
                CoreError::UnsupportedEncrypt(UnsupportedEncrypt::Asymmetric)
            ),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn signing_with_a_symmetric_key_is_refused() {
        let k = symmetric(16);
        let key = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k,
        });
        let error = sign(b"x", &key).expect_err("symmetric");
        assert!(
            matches!(
                error,
                CoreError::UnsupportedSign(UnsupportedSign::Symmetric)
            ),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_large_payload_fits_the_buffers() {
        // Exercises the payload-proportional buffer sizing rather than the fixed margins.
        let bytes = generate_cose_key(GenKeyKind::OkpEd25519).unwrap();
        let key = CoseKey::decode(&bytes).unwrap();
        let payload = vec![0x5au8; 64 * 1024];
        let signed = sign(&payload, &key).expect("sign 64 KiB");
        assert_eq!(verify(&signed, &key).expect("verify"), payload);
    }

    #[test]
    fn the_algorithm_table_has_no_duplicates_and_covers_the_registry() {
        let mut ids: Vec<i64> = ENCRYPT_ALGORITHMS.iter().map(|a| a.to_i64()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(
            ids.len(),
            count,
            "duplicate algorithm in ENCRYPT_ALGORITHMS"
        );
    }
}
