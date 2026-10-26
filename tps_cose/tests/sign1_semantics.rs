/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_minicbor::Encoder;
use tps_cose::{Algorithm, CoseError, HeaderParam, Label, ParsedHeaders, Sign1Builder, decode_sign1};

#[test]
fn reject_duplicate_labels_in_protected_map() {
    // Hand-craft a protected header with duplicate label 4 (kid).
    // Encoder::end_map() rejects duplicates, so we build the bytes directly.
    // CBOR: {4: h'61', 4: h'62'}  →  a2 04 41 61 04 41 62
    let protected: &[u8] = &[0xa2, 0x04, 0x41, 0x61, 0x04, 0x41, 0x62];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let err =
        decode_sign1(&sign1_buf[..sign1_len]).expect_err("duplicate protected labels must fail");
    assert_eq!(err, CoseError::DuplicateHeaderLabel);
}

#[test]
fn reject_duplicate_labels_across_maps() {
    let mut protected_buf = [0u8; 32];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(2).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        map.insert_key_value(4i64, b"a".as_slice()).expect("kid");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let mut map = array.map_len(1).expect("unprotected");
        map.insert_key_value(4i64, b"b".as_slice())
            .expect("duplicate kid");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let err =
        decode_sign1(&sign1_buf[..sign1_len]).expect_err("cross-map duplicate labels must fail");
    assert_eq!(err, CoseError::DuplicateHeaderLabel);
}

#[test]
fn reject_unknown_critical_label() {
    let mut protected_buf = [0u8; 48];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(2).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        let mut crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(1)
            .expect("crit array");
        crit.insert(99i64).expect("unknown crit label");
        crit.end().expect("crit end");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let err = decode_sign1(&sign1_buf[..sign1_len]).expect_err("unknown critical label must fail");
    assert_eq!(err, CoseError::UnknownCriticalHeader);
}

#[cfg(feature = "std")]
#[test]
fn reject_unknown_text_critical_label() {
    let mut protected_buf = [0u8; 48];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(2).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        let mut crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(1)
            .expect("crit array");
        crit.insert("example-label")
            .expect("unknown crit text label");
        crit.end().expect("crit end");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let err =
        decode_sign1(&sign1_buf[..sign1_len]).expect_err("unknown text critical label must fail");
    assert_eq!(err, CoseError::UnknownCriticalHeader);
}

/// RFC 9052 sec. 3: "the protected bucket ... Senders SHOULD encode a
/// zero-length map as a zero-length byte string rather than as a zero-length
/// map (encoded as h'a0'). ... Recipients MUST accept both a zero-length byte
/// string and a zero-length map encoded in a byte string."
///
/// STAGE 1 FINDING (fixed): `decode_sign1` special-cases a zero-length
/// protected bstr (`empty_or_serialized_map` alternative `bstr .size 0`) by
/// treating it as `ParsedHeaders::default()` instead of calling
/// `Decoder::next_map()` (which fails to decode an empty byte slice as a
/// map). The `h'a0'` (CBOR-encoded empty map wrapped in a bstr) form was
/// already accepted; both forms are now accepted per RFC 9052.
#[test]
fn decode_accepts_zero_length_protected_bstr() {
    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        // Zero-length protected bstr: no algorithm carried in protected bucket,
        // so it must be supplied via the unprotected bucket instead.
        array.insert(&[][..]).expect("zero-length protected bstr");
        let mut map = array.map_len(1).expect("unprotected");
        map.insert_key_value(1i64, -7i64).expect("alg in unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]).expect("zero-length protected bstr accepted");
    assert_eq!(result.protected, ParsedHeaders::default());
    assert_eq!(result.unprotected.algorithm, Some(Algorithm::Es256));
}

/// RFC 9052 sec. 3.1: "When present, the 'crit' header parameter MUST be
/// placed in the protected-header-parameters bucket." and: "If the 'crit'
/// value list includes a label for which the header parameter is not in the
/// protected-header-parameters bucket, this is a fatal error in processing
/// the message."
///
/// STAGE 1 FINDING (fixed): `parse_headers` now rejects a `crit` (label 2)
/// header parameter that appears in the unprotected bucket with
/// `CoseError::InvalidHeader`.
#[test]
fn decode_rejects_crit_in_unprotected_bucket() {
    let mut protected_buf = [0u8; 16];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(1).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let mut map = array.map_len(1).expect("unprotected");
        // crit (label 2) placed in the UNPROTECTED bucket: spec says this
        // MUST be a fatal error.
        let mut crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(1)
            .expect("crit array");
        crit.insert(4i64).expect("understood label (kid)");
        crit.end().expect("crit end");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]);
    // Per RFC 9052 sec. 3.1 this MUST be rejected.
    assert_eq!(result, Err(CoseError::InvalidHeader));
}

/// RFC 9052 sec. 3.1: "If the 'crit' value list includes a label for which
/// the header parameter is not in the protected-header-parameters bucket,
/// this is a fatal error in processing the message." This applies both to
/// labels absent entirely and to labels present only in the unprotected
/// bucket -- the label named by `crit` must itself be present in the
/// *protected* bucket.
///
/// STAGE 1 FINDING (fixed): `parse_crit` now cross-checks each label named by
/// `crit` against the protected map's actual contents and rejects the
/// message with `CoseError::UnknownCriticalHeader` when the named header
/// parameter is absent: `crit` names label 4 (kid), but no kid is present
/// anywhere in the message.
#[test]
fn decode_rejects_crit_referencing_absent_header() {
    let mut protected_buf = [0u8; 48];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(2).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        let mut crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(1)
            .expect("crit array");
        crit.insert(4i64).expect("kid label declared critical");
        crit.end().expect("crit end");
        map.end().expect("map end"); // kid (label 4) itself is NOT present
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]);
    // Per RFC 9052 sec. 3.1 this MUST be rejected.
    assert_eq!(result, Err(CoseError::UnknownCriticalHeader));
}

/// RFC 9052 sec. 3.1 Table 3 and RFC 9053 Tables 1/2: pin the exact COSE
/// algorithm integer identifiers this crate dispatches on, so a future
/// refactor cannot silently swap ES256/EdDSA (or any AES-CCM/GCM variant)
/// values.
#[test]
fn algorithm_identifiers_match_rfc9053_tables() {
    use tps_cose::{Algorithm, EncryptAlgorithm};

    // RFC 9053 Table 1 (ECDSA) / Table 2 (EdDSA).
    assert_eq!(Algorithm::Es256.to_i64(), -7);
    assert_eq!(Algorithm::EdDsa.to_i64(), -8);
    assert_eq!(Algorithm::try_from_i64(-7), Some(Algorithm::Es256));
    assert_eq!(Algorithm::try_from_i64(-8), Some(Algorithm::EdDsa));
    // ES384 (-35) and ES512 (-36) are defined by RFC 9053 but not
    // implemented by this crate; confirm they are correctly rejected rather
    // than silently mismapped to a supported variant.
    assert_eq!(Algorithm::try_from_i64(-35), None);
    assert_eq!(Algorithm::try_from_i64(-36), None);

    // RFC 9053 Table 5 (AES-GCM) / Table 6 (AES-CCM).
    assert_eq!(EncryptAlgorithm::A128Gcm.to_i64(), 1);
    assert_eq!(EncryptAlgorithm::AesCcm16_64_128.to_i64(), 10);
    assert_eq!(EncryptAlgorithm::AesCcm16_64_256.to_i64(), 11);
    assert_eq!(EncryptAlgorithm::AesCcm64_64_128.to_i64(), 12);
    assert_eq!(EncryptAlgorithm::AesCcm64_64_256.to_i64(), 13);
    assert_eq!(EncryptAlgorithm::AesCcm16_128_128.to_i64(), 30);
    assert_eq!(EncryptAlgorithm::AesCcm16_128_256.to_i64(), 31);
    assert_eq!(EncryptAlgorithm::AesCcm64_128_128.to_i64(), 32);
    assert_eq!(EncryptAlgorithm::AesCcm64_128_256.to_i64(), 33);
    // A192GCM (2) and A256GCM (3) are defined by RFC 9053 but not
    // implemented by this crate.
    assert_eq!(EncryptAlgorithm::try_from_i64(2), None);
    assert_eq!(EncryptAlgorithm::try_from_i64(3), None);
}

/// RFC 9052 sec. 3.1: "the header parameter 'counter signature' (label 7)
/// defined by [RFC8152] must be understood by new implementations, to remain
/// compatible with senders that adhere to that document and assume all
/// implementations will understand it."
///
/// STAGE 1 FINDING (fixed): `is_understood_label`'s range is now widened to
/// `1..=7`, so a `crit` array naming label 7 (counter signature) is no
/// longer rejected as unknown -- provided the label is actually present in
/// the protected bucket (see the Finding 3 fix in
/// `decode_rejects_crit_referencing_absent_header`).
#[test]
fn decode_accepts_crit_label_seven_counter_signature() {
    let mut protected_buf = [0u8; 48];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(3).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        let mut crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(1)
            .expect("crit array");
        crit.insert(7i64).expect("counter signature label");
        crit.end().expect("crit end");
        // The counter signature label (7) itself, present as required by the
        // Finding 3 fix. This crate does not implement counter signatures,
        // so an arbitrary placeholder value is used.
        map.insert_key_value(7i64, b"placeholder".as_slice())
            .expect("counter signature placeholder");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]);
    // Per RFC 9052 sec. 3.1, label 7 must be understood.
    result.expect("crit naming label 7 (counter signature) must be accepted");
}

/// RFC 9052 sec. 3.1: "crit: ... The array MUST have at least one value in
/// it."
///
/// STAGE 1 FINDING (fixed): `parse_crit` now rejects a zero-length `crit`
/// array with `CoseError::InvalidHeaderValue`.
#[test]
fn decode_rejects_empty_crit_array() {
    let mut protected_buf = [0u8; 48];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc.map_with_len(2).expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        let crit = map
            .key(2i64)
            .expect("crit key")
            .array_len(0)
            .expect("empty crit array");
        crit.end().expect("crit end");
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]);
    // Per RFC 9052 sec. 3.1 this MUST be rejected (crit array must have at
    // least one value).
    assert_eq!(result, Err(CoseError::InvalidHeaderValue));
}

/// RFC 9052 sec. 3.1: "When present, the 'crit' header parameter MUST be
/// placed in the protected-header-parameters bucket." Same requirement as
/// the decode-side Finding 2 (`decode_rejects_crit_in_unprotected_bucket`
/// above), but for the *encode* path: a builder must not silently accept a
/// `HeaderParam::Critical` placed in the unprotected header slice.
///
/// STAGE 1 FINDING 7 (fixed): `validate_builder_headers` now rejects a
/// `HeaderParam::Critical` entry found in the unprotected slice with
/// `CoseError::InvalidHeader`, before any bytes are encoded.
#[test]
fn encode_rejects_crit_in_unprotected_bucket() {
    let protected: [HeaderParam<'_>; 0] = [];
    let unprotected = [HeaderParam::Critical(&[Label::Int(4)])];
    let builder = Sign1Builder::new(&protected, &unprotected).algorithm(Algorithm::Es256);
    let mut protected_buf = [0u8; 64];
    let result = builder.encode_protected(&mut protected_buf);
    assert_eq!(result, Err(CoseError::InvalidHeader));
}

/// STAGE 3 FINDING (tps_cose #2): `ensure_no_duplicate_labels`/
/// `ensure_no_cross_duplicates` in `header.rs` are O(n^2) in the number of
/// map entries (a full nested scan, re-decoding every value on every
/// pairwise comparison). CBOR map length is attacker-controlled and
/// otherwise unbounded, so a header map with many entries is a CPU-
/// exhaustion DoS via this quadratic cost alone. Fixed by a cheap
/// `Map::len()` pre-check against `header::MAX_HEADER_ENTRIES` (256) before
/// either function's O(n^2) scan runs.
#[test]
fn decode_rejects_protected_header_map_exceeding_max_entries() {
    // 1 (algorithm) + 256 filler labels = 257 entries, one past the cap.
    const EXTRA: usize = 256;
    let mut protected_buf = [0u8; 4096];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc
            .map_with_len((EXTRA + 1) as u64)
            .expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        for i in 0..EXTRA {
            map.insert_key_value(1000i64 + i as i64, i as i64)
                .expect("filler label");
        }
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 8192];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let err = decode_sign1(&sign1_buf[..sign1_len])
        .expect_err("oversized header map must be rejected");
    assert_eq!(err, CoseError::TooManyHeaderParameters);
}

/// Boundary companion to `decode_rejects_protected_header_map_exceeding_max_entries`:
/// a header map with exactly `MAX_HEADER_ENTRIES` entries must not be
/// rejected for size (it may still fail decoding for other reasons, but
/// never with `TooManyHeaderParameters`).
#[test]
fn decode_accepts_protected_header_map_at_max_entries() {
    // 1 (algorithm) + 255 filler labels = 256 entries, exactly at the cap.
    const EXTRA: usize = 255;
    let mut protected_buf = [0u8; 4096];
    let mut protected_enc = Encoder::<4>::new(&mut protected_buf);
    {
        let mut map = protected_enc
            .map_with_len((EXTRA + 1) as u64)
            .expect("protected map");
        map.insert_key_value(1i64, -7i64).expect("alg");
        for i in 0..EXTRA {
            map.insert_key_value(1000i64 + i as i64, i as i64)
                .expect("filler label");
        }
        map.end().expect("map end");
    }
    let protected_len = protected_enc.len();
    let protected = &protected_buf[..protected_len];

    let mut sign1_buf = [0u8; 8192];
    let mut enc = Encoder::<4>::new(&mut sign1_buf);
    {
        let mut array = enc.array_with_len(4).expect("array");
        array.insert(protected).expect("protected bstr");
        let map = array.map_len(0).expect("unprotected");
        map.end().expect("unprotected end");
        array.insert(b"payload".as_slice()).expect("payload");
        array.insert(b"sig".as_slice()).expect("signature");
        array.end().expect("array end");
    }
    let sign1_len = enc.len();
    let result = decode_sign1(&sign1_buf[..sign1_len]);
    assert!(
        !matches!(result, Err(CoseError::TooManyHeaderParameters)),
        "at-cap header map must not be rejected for size: {result:?}"
    );
}

