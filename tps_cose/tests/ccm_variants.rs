/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Round-trip coverage for all eight AES-CCM variants of COSE_Encrypt0.
//!
//! `tests/encrypt0.rs` exercises AES-CCM-16-64-128 only, and the RFC 9052 C.4 known-answer
//! vectors are also for that one algorithm. The remaining seven `Ccm<_, TagSize, NonceSize>`
//! instantiations were therefore only ever type-checked, never run. The tag length in particular
//! is read out of the cipher's own `TagSize` associated type, so a mistake there is invisible for
//! the 8-byte-tag variants that happen to be tested and wrong for the 16-byte-tag ones.
//!
//! Each case below asserts the round-trip through a *detached* payload, where the caller has to
//! state the ciphertext length itself: passing `plaintext.len() + tag_len` means an incorrect
//! tag length cannot round-trip successfully, so this covers the tag arithmetic as well as the
//! encrypt/decrypt agreement.

#[cfg(feature = "ring")]
use tps_cose::{
    CoseError, Encrypt0Builder, EncryptAlgorithm, Payload, decode_encrypt0,
    decrypt_encrypt0_with_raw_key,
};

/// `(algorithm, key_len, nonce_len, tag_len)` for every AES-CCM algorithm COSE defines.
///
/// The names read as `AesCcm{L}_{T}_{K}`: `L` is the width in bits of the CCM length field, which
/// fixes the nonce at 15 - L/8 bytes (13 for L=16, 7 for L=64); `T` is the tag width in bits; `K`
/// is the AES key width in bits.
#[cfg(feature = "ring")]
const CCM_VARIANTS: &[(EncryptAlgorithm, usize, usize, usize)] = &[
    (EncryptAlgorithm::AesCcm16_64_128, 16, 13, 8),
    (EncryptAlgorithm::AesCcm16_64_256, 32, 13, 8),
    (EncryptAlgorithm::AesCcm64_64_128, 16, 7, 8),
    (EncryptAlgorithm::AesCcm64_64_256, 32, 7, 8),
    (EncryptAlgorithm::AesCcm16_128_128, 16, 13, 16),
    (EncryptAlgorithm::AesCcm16_128_256, 32, 13, 16),
    (EncryptAlgorithm::AesCcm64_128_128, 16, 7, 16),
    (EncryptAlgorithm::AesCcm64_128_256, 32, 7, 16),
];

#[cfg(feature = "ring")]
fn round_trip(
    algorithm: EncryptAlgorithm,
    key_len: usize,
    nonce_len: usize,
    tag_len: usize,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(), CoseError> {
    let key = vec![0x11u8; key_len];
    let nonce = vec![0x22u8; nonce_len];
    let mut protected = [0u8; 64];
    let mut enc_structure = [0u8; 128];
    let mut ciphertext = [0u8; 256];
    let mut message = [0u8; 256];

    let msg_len = Encrypt0Builder::new(&[], &[])
        .algorithm(algorithm)
        .nonce(&nonce)
        .payload(Payload::Detached(plaintext))
        .external_aad(aad)
        .encrypt_with_raw_key(
            &key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )?;

    let decoded = decode_encrypt0(&message[..msg_len])?;
    assert!(
        decoded.ciphertext.is_none(),
        "{algorithm:?}: detached payload should leave no ciphertext in the message"
    );

    let mut plaintext_out = [0u8; 256];
    let plain_len = decrypt_encrypt0_with_raw_key(
        &decoded,
        &key,
        aad,
        Some(&ciphertext[..plaintext.len() + tag_len]),
        &mut enc_structure,
        &mut plaintext_out,
    )?;
    assert_eq!(
        &plaintext_out[..plain_len], plaintext,
        "{algorithm:?}: round-tripped plaintext differs"
    );
    Ok(())
}

#[cfg(feature = "ring")]
#[test]
fn every_ccm_variant_round_trips() {
    for &(algorithm, key_len, nonce_len, tag_len) in CCM_VARIANTS {
        for (plaintext, aad) in [
            (b"This is the content." as &[u8], b"" as &[u8]),
            (b"This is the content.", b"\x00\x11\x22\x33"),
            (b"", b"\xaa\xbb"),
        ] {
            round_trip(algorithm, key_len, nonce_len, tag_len, plaintext, aad)
                .unwrap_or_else(|e| panic!("{algorithm:?} failed on {plaintext:?}: {e:?}"));
        }
    }
}

/// A tag length one byte short of the cipher's own `TagSize` must fail authentication rather
/// than silently truncating. This is what pins the tag arithmetic: without it, a `TagSize`
/// misread that happened to be self-consistent between encrypt and decrypt would go unnoticed.
#[cfg(feature = "ring")]
#[test]
fn ccm_rejects_truncated_tag() {
    for &(algorithm, key_len, nonce_len, tag_len) in CCM_VARIANTS {
        let key = vec![0x11u8; key_len];
        let nonce = vec![0x22u8; nonce_len];
        let plaintext = b"This is the content.";
        let mut protected = [0u8; 64];
        let mut enc_structure = [0u8; 128];
        let mut ciphertext = [0u8; 256];
        let mut message = [0u8; 256];

        let msg_len = Encrypt0Builder::new(&[], &[])
            .algorithm(algorithm)
            .nonce(&nonce)
            .payload(Payload::Detached(plaintext))
            .external_aad(b"")
            .encrypt_with_raw_key(
                &key,
                &mut protected,
                &mut enc_structure,
                &mut ciphertext,
                &mut message,
            )
            .expect("encrypt");

        let decoded = decode_encrypt0(&message[..msg_len]).expect("decode");
        let mut plaintext_out = [0u8; 256];
        let result = decrypt_encrypt0_with_raw_key(
            &decoded,
            &key,
            b"",
            Some(&ciphertext[..plaintext.len() + tag_len - 1]),
            &mut enc_structure,
            &mut plaintext_out,
        );
        assert!(
            result.is_err(),
            "{algorithm:?}: a {}-byte tag was accepted where {tag_len} are required",
            tag_len - 1
        );
    }
}
