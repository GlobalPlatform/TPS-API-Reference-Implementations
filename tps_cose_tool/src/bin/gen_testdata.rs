/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/// Generates IETF test vector key files in the cose_tool/testdata/ directory.
///
/// Run from the workspace root: cargo run -p cose_tool --bin gen_testdata
use std::{fs, path::Path};

use tps_cose::{CoseKey, Curve, Ec2Key, Ec2Y, OkpKey, SymmetricKey};

fn main() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    fs::create_dir_all(&out_dir).expect("create testdata dir");

    write_es256_private(&out_dir);
    write_es256_public(&out_dir);
    write_eddsa_private(&out_dir);
    write_eddsa_public(&out_dir);
    write_aes128gcm_key(&out_dir);
    write_payload(&out_dir);

    println!("Test data written to {}", out_dir.display());
}

// ES256 test vector from RFC 9052 Appendix C.7.1 / IETF sign-pass-01.json
const ES256_X: &[u8] = &[
    0xba, 0xc5, 0xb1, 0x1c, 0xad, 0x8f, 0x99, 0xf9, 0xc7, 0x2b, 0x05, 0xcf, 0x4b, 0x9e, 0x26, 0xd2,
    0x44, 0xdc, 0x18, 0x9f, 0x74, 0x52, 0x28, 0x25, 0x5a, 0x21, 0x9a, 0x86, 0xd6, 0xa0, 0x9e, 0xff,
];
const ES256_Y: &[u8] = &[
    0x20, 0x13, 0x8b, 0xf8, 0x2d, 0xc1, 0xb6, 0xd5, 0x62, 0xbe, 0x0f, 0xa5, 0x4a, 0xb7, 0x80, 0x4a,
    0x3a, 0x64, 0xb6, 0xd7, 0x2c, 0xcf, 0xed, 0x6b, 0x6f, 0xb6, 0xed, 0x28, 0xbb, 0xfc, 0x11, 0x7e,
];
const ES256_D: &[u8] = &[
    0x57, 0xc9, 0x20, 0x77, 0x66, 0x41, 0x46, 0xe8, 0x76, 0x76, 0x0c, 0x95, 0x20, 0xd0, 0x54, 0xaa,
    0x93, 0xc3, 0xaf, 0xb0, 0x4e, 0x30, 0x67, 0x05, 0xdb, 0x60, 0x90, 0x30, 0x85, 0x07, 0xb4, 0xd3,
];

// EdDSA test vector from RFC 8037 / IETF eddsa-01.json
const EDDSA_X: &[u8] = &[
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];
const EDDSA_D: &[u8] = &[
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

// AES-128-GCM key — RFC 9053 Appendix B.2
const AES128GCM_K: &[u8] = &[
    0x84, 0x9b, 0x57, 0x21, 0x9d, 0xae, 0x48, 0xde, 0x64, 0x6d, 0x07, 0xdb, 0xb5, 0x33, 0x56, 0x6e,
];

fn encode_key(key: &CoseKey<'_>, path: &Path) {
    let mut buf = vec![0u8; 512];
    let n = key.encode(&mut buf).expect("encode key");
    fs::write(path, &buf[..n]).expect("write key file");
    println!("  wrote {}", path.display());
}

fn write_es256_private(dir: &Path) {
    let mut k = Ec2Key::new(Curve::P256, ES256_X, Some(Ec2Y::Bytes(ES256_Y)));
    k.d = Some(ES256_D);
    encode_key(&CoseKey::Ec2(k), &dir.join("es256_private.cbor"));
}

fn write_es256_public(dir: &Path) {
    let k = Ec2Key::new(Curve::P256, ES256_X, Some(Ec2Y::Bytes(ES256_Y)));
    encode_key(&CoseKey::Ec2(k), &dir.join("es256_public.cbor"));
}

fn write_eddsa_private(dir: &Path) {
    let mut k = OkpKey::new(Curve::Ed25519, EDDSA_X);
    k.d = Some(EDDSA_D);
    encode_key(&CoseKey::Okp(k), &dir.join("eddsa_private.cbor"));
}

fn write_eddsa_public(dir: &Path) {
    let k = OkpKey::new(Curve::Ed25519, EDDSA_X);
    encode_key(&CoseKey::Okp(k), &dir.join("eddsa_public.cbor"));
}

fn write_aes128gcm_key(dir: &Path) {
    let k = SymmetricKey::new(AES128GCM_K);
    encode_key(&CoseKey::Symmetric(k), &dir.join("aes128gcm_key.cbor"));
}

fn write_payload(dir: &Path) {
    let path = dir.join("payload.txt");
    fs::write(&path, b"This is the content.").expect("write payload");
    println!("  wrote {}", path.display());
}
