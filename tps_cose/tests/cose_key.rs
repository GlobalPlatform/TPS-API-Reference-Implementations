/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#[cfg(feature = "std")]
use tps_minicbor::Encoder;
use tps_cose::{CoseKey, Curve, Ec2Key, Ec2Y, KeyCommon, OkpKey, SymmetricKey, decode_cose_key};

#[test]
fn ec2_encode_decode_round_trip() {
    let key = Ec2Key {
        common: KeyCommon {
            key_id: Some(b"kid"),
            algorithm: Some(-7),
            base_iv: None,
        },
        curve: Curve::P256,
        x: &[0x11; 32],
        y: Some(Ec2Y::Bytes(&[0x22; 32])),
        d: Some(&[0x33; 32]),
    };
    let key = CoseKey::Ec2(key);
    let mut out = [0u8; 256];
    let len = key.encode(&mut out).expect("encode");
    let decoded = decode_cose_key(&out[..len]).expect("decode");
    assert_eq!(decoded, key);
}

#[test]
fn okp_encode_decode_round_trip() {
    let key = OkpKey {
        common: KeyCommon::default(),
        curve: Curve::Ed25519,
        x: &[0x44; 32],
        d: Some(&[0x55; 32]),
    };
    let key = CoseKey::Okp(key);
    let mut out = [0u8; 256];
    let len = key.encode(&mut out).expect("encode");
    let decoded = decode_cose_key(&out[..len]).expect("decode");
    assert_eq!(decoded, key);
}

#[test]
fn symmetric_encode_decode_round_trip() {
    let key = SymmetricKey {
        common: KeyCommon::default(),
        k: &[0x66; 16],
    };
    let key = CoseKey::Symmetric(key);
    let mut out = [0u8; 128];
    let len = key.encode(&mut out).expect("encode");
    let decoded = decode_cose_key(&out[..len]).expect("decode");
    assert_eq!(decoded, key);
}

#[cfg(feature = "std")]
#[test]
fn decode_allows_unknown_text_label_with_std() {
    let mut out = [0u8; 128];
    let mut enc = Encoder::<4>::new(&mut out);
    {
        let mut map = enc.map_with_len(4).expect("map");
        map.insert_key_value(1i64, 4i64).expect("kty");
        map.insert_key_value(-1i64, b"0123456789abcdef".as_slice())
            .expect("k");
        map.insert_key_value("meta", 7i64).expect("text label");
        map.insert_key_value(2i64, b"kid".as_slice()).expect("kid");
        map.end().expect("end");
    }
    let len = enc.len();
    let key = decode_cose_key(&out[..len]).expect("decode with text label");
    match key {
        CoseKey::Symmetric(sym) => {
            assert_eq!(sym.k, b"0123456789abcdef");
            assert_eq!(sym.common.key_id, Some(b"kid".as_slice()));
        }
        _ => panic!("expected symmetric key"),
    }
}

#[cfg(feature = "ring")]
#[test]
fn es256_conversion_helpers_round_trip() {
    use tps_cose::{EdDsaPrivateKey, EdDsaPublicKey, Es256PrivateKey, Es256PublicKey};

    let public = Es256PublicKey {
        x: &[0x01; 32],
        y: &[0x02; 32],
    };
    let ec2_public: Ec2Key<'_> = public.into();
    let public_back = Es256PublicKey::try_from(&ec2_public).expect("public conversion");
    assert_eq!(public_back, public);

    let private = Es256PrivateKey {
        d: &[0x03; 32],
        x: &[0x01; 32],
        y: &[0x02; 32],
    };
    let ec2_private: Ec2Key<'_> = private.into();
    let private_back = Es256PrivateKey::try_from(&ec2_private).expect("private conversion");
    assert_eq!(private_back, private);

    let ed_public = EdDsaPublicKey { x: &[0x09; 32] };
    let okp_public: OkpKey<'_> = ed_public.into();
    let ed_public_back = EdDsaPublicKey::try_from(&okp_public).expect("okp public conversion");
    assert_eq!(ed_public_back, ed_public);

    let ed_private = EdDsaPrivateKey {
        d: &[0x0a; 32],
        x: &[0x09; 32],
    };
    let okp_private: OkpKey<'_> = ed_private.into();
    let ed_private_back = EdDsaPrivateKey::try_from(&okp_private).expect("okp private conversion");
    assert_eq!(ed_private_back, ed_private);
}
