/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Phase D regression proof for the full artifact-movement chain: load CBOR
//! bytes -> convert to EDN -> validate against a CDDL schema -> COSE_Sign1
//! it -> decode/verify -> assert the recovered payload matches the
//! original bytes. This exercises the same library calls `cbor-tui`'s UI
//! layer wires up (`app.rs`/`cose_ops.rs`/`validate.rs`), bypassing the TUI
//! event loop entirely, since this crate has no lib target to import from
//! directly.

use tps_cddl::{Config, build_cddl, check_cbor, lex};
use tps_cose::{Algorithm, Es256PrivateKey, Es256PublicKey, Payload, Sign1Builder, decode_sign1, verify_sign1_es256};
use tps_minicbor::Decoder;

// Same fixed P-256 test vector used by `tps_cbor_tui/src/cose_ops.rs`'s own
// tests and by `tps_cose/tests/sign.rs`.
fn decode_base64url(input: &str) -> Vec<u8> {
    fn val(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => panic!("invalid base64url byte"),
        }
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let vals: Vec<u32> = chunk.iter().map(|&c| val(c)).collect();
        match vals.len() {
            4 => {
                let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6) | vals[3];
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
                out.push(n as u8);
            }
            3 => {
                let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6);
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
            }
            2 => {
                let n = (vals[0] << 18) | (vals[1] << 12);
                out.push((n >> 16) as u8);
            }
            _ => panic!("invalid base64url length"),
        }
    }
    out
}

#[test]
fn workflow6_load_validate_sign_verify_round_trip() {
    // 1. Load CBOR bytes: ["Bern", {longitude: 20, latitude: 46}]
    let original: Vec<u8> = vec![
        0x82, 0x64, 0x42, 0x65, 0x72, 0x6e, 0xa2, 0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74, 0x75,
        0x64, 0x65, 0x14, 0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, 0x18, 0x2e,
    ];

    // 2. Decode to EDN (as the CBOR<->EDN viewer would).
    let dec = Decoder::new(&original);
    let mut edn = String::new();
    dec.write_diagnostic_all_fmt(&mut edn).expect("decode to EDN");
    assert!(edn.contains("Bern"));

    // 3. Validate against a CDDL schema (RFC 8610 Appendix H, §3.3 Geography).
    let cddl_source = "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\nGpsCoordinates = { longitude: uint, latitude: uint }";
    let pairs = lex(cddl_source).expect("lex CDDL");
    let cddl = build_cddl(pairs).expect("build CDDL");
    let mismatches =
        check_cbor(&original, &cddl, Some("Geography"), &Config::default()).expect("check_cbor");
    assert!(mismatches.is_empty(), "expected no mismatches, got {mismatches:?}");

    // 4. Sign the validated artifact with an ES256 key.
    let d = decode_base64url("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM");
    let x = decode_base64url("usWxHK2PmfnHKwXPS54m0kTcGJ90UiglWiGahtagnv8");
    let y = decode_base64url("IBOL-C3BttVivg-lSreASjpkttcsz-1rb7btKLv8EX4");

    let payload_len = original.len();
    let mut protected_buf = vec![0u8; 256];
    let mut sig_structure_buf = vec![0u8; payload_len + 512];
    let mut signature_buf = vec![0u8; 128];
    let mut out = vec![0u8; payload_len + 1024];

    let priv_key = Es256PrivateKey { d: &d, x: &x, y: &y };
    let n = Sign1Builder::new(&[], &[])
        .algorithm(Algorithm::Es256)
        .payload(Payload::Attached(&original))
        .sign_es256(
            &priv_key,
            &mut protected_buf,
            &mut sig_structure_buf,
            &mut signature_buf,
            &mut out,
        )
        .expect("sign_es256");
    out.truncate(n);

    // 5. Decode and verify, recovering the payload.
    let sign1 = decode_sign1(&out).expect("decode_sign1");
    let payload = sign1.payload.expect("payload present");

    let pub_key = Es256PublicKey { x: &x, y: &y };
    let mut verify_buf = vec![0u8; payload.len() + 512];
    verify_sign1_es256(&sign1, &pub_key, &[], None, &mut verify_buf).expect("verify_sign1_es256");

    assert_eq!(payload, original.as_slice());
}
