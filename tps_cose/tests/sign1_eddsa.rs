/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#[allow(dead_code)]
mod common;

#[cfg(feature = "ring")]
use common::{decode_hex, encode_tag, split_tag};
#[cfg(feature = "ring")]
use serde::Deserialize;
#[cfg(feature = "ring")]
use tps_cose::{
    Algorithm, ContentType, EdDsaPrivateKey, EdDsaPublicKey, HeaderParam, Payload, Sign1Builder,
    decode_sign1, verify_sign1_eddsa,
};

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaVector {
    input: EdDsaInput,
    output: EdDsaOutput,
}

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaInput {
    plaintext: String,
    sign0: EdDsaSign0,
}

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaSign0 {
    key: EdDsaKey,
    unprotected: EdDsaUnprotected,
}

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaKey {
    #[serde(rename = "x_hex")]
    x_hex: String,
    #[serde(rename = "d_hex")]
    d_hex: String,
}

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaUnprotected {
    kid: String,
}

#[cfg(feature = "ring")]
#[derive(Debug, Deserialize)]
struct EdDsaOutput {
    cbor: String,
}

#[cfg(feature = "ring")]
fn load_eddsa_sign1_vector() -> EdDsaVector {
    let root = env!("CARGO_MANIFEST_DIR");
    let path = format!("{root}/tests/Examples/eddsa-examples/eddsa-sig-01.json");
    let data = std::fs::read_to_string(path).expect("read eddsa sign1 vector");
    serde_json::from_str(&data).expect("parse eddsa sign1 vector")
}

#[cfg(feature = "ring")]
#[test]
fn eddsa_sign1_vector_round_trip_and_verify() {
    let vector = load_eddsa_sign1_vector();
    let payload = vector.input.plaintext.as_bytes();
    let kid = vector.input.sign0.unprotected.kid.into_bytes();
    let protected = [HeaderParam::ContentType(ContentType::Uint(0))];
    let unprotected = [HeaderParam::KeyId(kid.as_slice())];
    let mut protected_buf = [0u8; 64];
    let mut sig_structure_buf = [0u8; 128];
    let mut signature_buf = [0u8; 128];
    let mut message = [0u8; 256];
    let x = decode_hex(&vector.input.sign0.key.x_hex);
    let d = decode_hex(&vector.input.sign0.key.d_hex);
    let private = EdDsaPrivateKey { d: &d, x: &x };
    let public = EdDsaPublicKey { x: &x };

    let builder = Sign1Builder::new(&protected, &unprotected)
        .algorithm(Algorithm::EdDsa)
        .payload(Payload::Attached(payload));
    let msg_len = builder
        .sign_eddsa(
            &private,
            &mut protected_buf,
            &mut sig_structure_buf,
            &mut signature_buf,
            &mut message,
        )
        .expect("sign");

    let expected = decode_hex(&vector.output.cbor);
    let (tag, expected_untagged) = split_tag(&expected).expect("split expected tag");
    assert_eq!(tag, Some(18));
    assert_eq!(&message[..msg_len], expected_untagged);
    let tagged = encode_tag(18, &message[..msg_len]);
    assert_eq!(tagged, expected);

    let decoded = decode_sign1(expected_untagged).expect("decode sign1");
    assert_eq!(decoded.payload, Some(payload));
    assert_eq!(decoded.protected.algorithm, Some(Algorithm::EdDsa));
    verify_sign1_eddsa(&decoded, &public, b"", None, &mut sig_structure_buf).expect("verify");
}

#[cfg(feature = "ring")]
#[test]
fn eddsa_sign1_detached_empty_payload() {
    let x = decode_hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let d = decode_hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    let private = EdDsaPrivateKey { d: &d, x: &x };
    let public = EdDsaPublicKey { x: &x };
    let mut protected_buf = [0u8; 64];
    let mut sig_structure_buf = [0u8; 96];
    let mut signature_buf = [0u8; 96];
    let mut message = [0u8; 192];
    let builder = Sign1Builder::new(&[], &[])
        .algorithm(Algorithm::EdDsa)
        .payload(Payload::Detached(b""))
        .external_aad(b"aad");

    let msg_len = builder
        .sign_eddsa(
            &private,
            &mut protected_buf,
            &mut sig_structure_buf,
            &mut signature_buf,
            &mut message,
        )
        .expect("sign detached");
    let decoded = decode_sign1(&message[..msg_len]).expect("decode detached sign1");
    assert_eq!(decoded.payload, None);
    verify_sign1_eddsa(&decoded, &public, b"aad", Some(b""), &mut sig_structure_buf)
        .expect("verify detached empty payload");
}
