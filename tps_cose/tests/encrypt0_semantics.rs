/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_cose::{CoseError, EncryptAlgorithm, ParsedHeaders, decode_encrypt0};
use tps_minicbor::Encoder;

/// RFC 9052 sec. 3: "the protected bucket ... Senders SHOULD encode a
/// zero-length map as a zero-length byte string rather than as a zero-length
/// map (encoded as h'a0'). ... Recipients MUST accept both a zero-length byte
/// string and a zero-length map encoded in a byte string."
///
/// STAGE 1 FINDING (fixed): `decode_encrypt0` special-cases a zero-length
/// protected bstr by treating it as `ParsedHeaders::default()` instead of
/// calling `Decoder::next_map()` (which fails to decode an empty byte slice
/// as a map). This mirrors the identical fix in `decode_sign1` (see
/// `sign1_semantics.rs::decode_accepts_zero_length_protected_bstr`); the
/// correct reference pattern already existed in `decode_sign`'s
/// `parse_protected_headers_allow_empty`. Both the empty-bstr and `h'a0'`
/// forms are now accepted per RFC 9052.
#[test]
fn decode_accepts_zero_length_protected_bstr() {
    let mut enc0_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut enc0_buf);
    {
        let mut array = enc.array_with_len(3).expect("array");
        // Zero-length protected bstr: algorithm and IV are carried in the
        // unprotected bucket instead.
        array.insert(&[][..]).expect("zero-length protected bstr");
        let mut map = array.map_len(2).expect("unprotected");
        map.insert_key_value(1i64, 1i64).expect("alg (A128GCM) in unprotected");
        map.insert_key_value(5i64, [0x22u8; 12].as_slice())
            .expect("iv in unprotected");
        map.end().expect("unprotected end");
        array.insert(b"ciphertext".as_slice()).expect("ciphertext");
        array.end().expect("array end");
    }
    let enc0_len = enc.len();
    let result =
        decode_encrypt0(&enc0_buf[..enc0_len]).expect("zero-length protected bstr accepted");
    assert_eq!(result.protected, ParsedHeaders::default());
    assert_eq!(result.unprotected.encrypt_algorithm, Some(EncryptAlgorithm::A128Gcm));
}

/// RFC 9052 sec. 3.1: describing IV (label 5) and Partial IV (label 6):
/// "... Applications MUST NOT have both the IV and Partial IV header
/// parameters present in the same security layer."
///
/// STAGE 1 FINDING (fixed): `decode_encrypt0`'s IV/Partial-IV handling (see
/// `src/encrypt0.rs`, the `match (iv, partial_iv) { ... }` block) now rejects
/// a message carrying both an IV and a Partial IV in the same security
/// layer.
#[test]
fn decode_rejects_iv_and_partial_iv_together() {
    let mut protected_buf = [0u8; 16];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let map = protected_enc.map_with_len(0).expect("protected map");
        map.end().expect("protected map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut enc0_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut enc0_buf);
    {
        let mut array = enc.array_with_len(3).expect("array");
        array.insert(protected).expect("protected bstr");
        let mut unprotected = array.map_len(3).expect("unprotected");
        unprotected
            .insert_key_value(1i64, 1i64)
            .expect("alg (A128GCM)");
        // Both IV (label 5, full 12-byte nonce for A128GCM) and Partial IV
        // (label 6) present in the same (unprotected) bucket: spec says
        // this MUST NOT happen.
        unprotected
            .insert_key_value(5i64, [0x22u8; 12].as_slice())
            .expect("iv");
        unprotected
            .insert_key_value(6i64, [0x01u8; 4].as_slice())
            .expect("partial iv");
        unprotected.end().expect("unprotected end");
        array.insert(b"ciphertext".as_slice()).expect("ciphertext");
        array.end().expect("array end");
    }
    let enc0_len = enc.len();
    let result = decode_encrypt0(&enc0_buf[..enc0_len]);
    // Per RFC 9052 sec. 3.1 this MUST be rejected.
    assert_eq!(result, Err(CoseError::InvalidHeader));
}
