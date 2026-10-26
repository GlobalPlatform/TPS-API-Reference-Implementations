/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
mod common;

#[cfg(feature = "ring")]
use common::decode_base64url;
use common::{HeaderMap, decode_hex, encode_tag, load_sign_vector, load_sign1_vector, split_tag};
use tps_minicbor::Encoder;
use tps_cose::{
    Algorithm, ContentType, CoseError, HeaderParam, Payload, Sign1Builder, decode_sign1,
};

struct ParamStorage {
    bytes: Vec<Vec<u8>>,
}

impl ParamStorage {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn push_bytes(&mut self, bytes: Vec<u8>) -> &[u8] {
        self.bytes.push(bytes);
        self.bytes.last().expect("bytes").as_slice()
    }
}

fn header_params_from_map<'a>(
    map: &'a HeaderMap,
    storage: &'a mut ParamStorage,
) -> Vec<HeaderParam<'a>> {
    let mut params: Vec<(u8, HeaderParam<'a>)> = Vec::new();
    if let Some(ref kid) = map.kid {
        let bytes = storage.push_bytes(kid.as_bytes().to_vec());
        params.push((4, HeaderParam::KeyId(bytes)));
    }
    if let Some(ctyp) = map.ctyp {
        params.push((3, HeaderParam::ContentType(ContentType::Uint(ctyp))));
    }
    params.sort_by_key(|(key, _)| *key);
    params.into_iter().map(|(_, param)| param).collect()
}

fn parse_alg(value: &str) -> Algorithm {
    match value {
        "ES256" => Algorithm::Es256,
        "EdDSA" => Algorithm::EdDsa,
        _ => panic!("unsupported alg: {value}"),
    }
}

fn alg_from_map(map: &HeaderMap) -> Option<Algorithm> {
    map.alg.as_deref().map(parse_alg)
}

fn alg_in_protected(map: &HeaderMap) -> bool {
    map.alg.is_some()
}

fn encode_header_map<'a>(
    map: &mut tps_minicbor::MapBuilder<'_, '_, 4>,
    algorithm: Option<Algorithm>,
    params: &[HeaderParam<'a>],
) -> Result<(), CoseError> {
    if let Some(algorithm) = algorithm {
        map.insert_key_value(1i64, algorithm.to_i64())?;
    }
    for param in params {
        match *param {
            HeaderParam::KeyId(kid) => {
                map.insert_key_value(4i64, kid)?;
            }
            HeaderParam::ContentType(content_type) => match content_type {
                ContentType::Text(value) => {
                    map.insert_key_value(3i64, value)?;
                }
                ContentType::Uint(value) => {
                    map.insert_key_value(3i64, value)?;
                }
            },
            _ => {}
        }
    }
    Ok(())
}

fn encode_sig_structure_sign(
    body_protected: &[u8],
    signer_protected: &[u8],
    external_aad: &[u8],
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    let mut enc = Encoder::<4>::new(out);
    let mut array = enc.array_with_len(5)?;
    array.insert("Signature")?;
    array.insert(body_protected)?;
    array.insert(signer_protected)?;
    array.insert(external_aad)?;
    array.insert(payload)?;
    array.end()?;
    Ok(enc.len())
}

fn protected_bstr_for_structure(protected: &[u8]) -> &[u8] {
    if protected.len() == 1 && protected[0] == 0xa0 {
        &[]
    } else {
        protected
    }
}

fn load_sign1_paths() -> [&'static str; 3] {
    [
        "sign-pass-01.json",
        "sign-pass-02.json",
        "sign-pass-03.json",
    ]
}

#[test]
fn sign1_es256_vectors() {
    for path in load_sign1_paths() {
        let vector = load_sign1_vector(path);
        assert!(!vector.title.is_empty());
        let mut protected_storage = ParamStorage::new();
        let mut unprotected_storage = ParamStorage::new();
        let protected =
            header_params_from_map(&vector.input.sign0.protected, &mut protected_storage);
        let unprotected =
            header_params_from_map(&vector.input.sign0.unprotected, &mut unprotected_storage);
        let alg = alg_from_map(&vector.input.sign0.protected)
            .or_else(|| alg_from_map(&vector.input.sign0.unprotected))
            .expect("missing algorithm in vector");
        let payload = vector.input.plaintext.as_bytes();
        let external = vector
            .input
            .sign0
            .external
            .as_deref()
            .map(decode_hex)
            .unwrap_or_default();

        let mut builder = Sign1Builder::new(&protected, &unprotected)
            .algorithm(alg)
            .payload(Payload::Attached(payload))
            .external_aad(&external);
        if !alg_in_protected(&vector.input.sign0.protected) {
            builder = builder.algorithm_in_unprotected();
        }

        let mut protected_buf = [0u8; 64];
        let protected_len = builder.encode_protected(&mut protected_buf).unwrap();
        let protected_bytes = &protected_buf[..protected_len];
        let expected_sig = vector
            .intermediates
            .as_ref()
            .expect("missing intermediates")
            .to_be_sign_hex
            .as_str();
        let expected_sig = decode_hex(expected_sig);
        let mut sig_out = vec![0u8; expected_sig.len() + 32];
        let sig_protected = match path {
            // sign-pass-01 uses an empty protected bstr in the Sig_structure.
            "sign-pass-01.json" => &[][..],
            _ => protected_bytes,
        };
        let sig_len = builder
            .encode_sig_structure(sig_protected, payload, &mut sig_out)
            .unwrap();
        assert_eq!(&sig_out[..sig_len], expected_sig.as_slice());

        let expected_cbor = decode_hex(&vector.output.cbor);
        let (tag, untagged) = split_tag(&expected_cbor).expect("parse tag");
        let expected_tag = match path {
            "sign-pass-01.json" => Some(18),
            "sign-pass-02.json" => Some(18),
            "sign-pass-03.json" => None,
            _ => None,
        };
        assert_eq!(tag, expected_tag, "{path}: unexpected tag");

        let sign1 = decode_sign1(untagged).expect("decode sign1");
        assert_eq!(sign1.payload, Some(payload));

        let mut encode_buf = [0u8; 64];
        let mut out = vec![0u8; untagged.len()];
        let len = builder
            .encode(&mut out, &mut encode_buf, sign1.signature)
            .unwrap();
        assert_eq!(&out[..len], untagged);
        assert_eq!(sign1.protected_serialized, protected_bytes);
        let tagged = match tag {
            Some(tag) => encode_tag(tag, untagged),
            None => untagged.to_vec(),
        };
        assert_eq!(expected_cbor, tagged);

        #[cfg(feature = "ring")]
        {
            let key = &vector.input.sign0.key;
            let x = decode_base64url(&key.x);
            let y = decode_base64url(&key.y);
            let public_key = tps_cose::Es256PublicKey { x: &x, y: &y };
            let mut sig_structure = vec![0u8; expected_sig.len() + 32];
            tps_cose::verify_sign1_es256(&sign1, &public_key, &external, None, &mut sig_structure)
                .expect("verify ES256");

            let d = decode_base64url(&key.d);
            let private_key = tps_cose::Es256PrivateKey {
                d: &d,
                x: &x,
                y: &y,
            };
            let mut signature = [0u8; 64];
            let sig_len =
                tps_cose::sign_es256(&private_key, expected_sig.as_slice(), &mut signature)
                    .expect("sign ES256");
            assert_eq!(sig_len, 64);
            tps_cose::verify_es256(&public_key, expected_sig.as_slice(), &signature)
                .expect("verify ES256 signature");
        }
    }
}

#[test]
fn sign1_wrong_tag_fails() {
    let vector = load_sign1_vector("sign-fail-01.json");
    assert!(vector.fail);
    assert!(!vector.title.is_empty());
    let mut protected_storage = ParamStorage::new();
    let mut unprotected_storage = ParamStorage::new();
    let protected = header_params_from_map(&vector.input.sign0.protected, &mut protected_storage);
    let unprotected =
        header_params_from_map(&vector.input.sign0.unprotected, &mut unprotected_storage);
    let alg = alg_from_map(&vector.input.sign0.protected)
        .or_else(|| alg_from_map(&vector.input.sign0.unprotected))
        .expect("missing algorithm in vector");
    let payload = vector.input.plaintext.as_bytes();
    let external = vector
        .input
        .sign0
        .external
        .as_deref()
        .map(decode_hex)
        .unwrap_or_default();
    let mut builder = Sign1Builder::new(&protected, &unprotected)
        .algorithm(alg)
        .payload(Payload::Attached(payload))
        .external_aad(&external);
    if !alg_in_protected(&vector.input.sign0.protected) {
        builder = builder.algorithm_in_unprotected();
    }
    let mut protected_buf = [0u8; 64];
    let protected_len = builder.encode_protected(&mut protected_buf).unwrap();
    let protected_bytes = &protected_buf[..protected_len];
    let expected_sig = vector
        .intermediates
        .as_ref()
        .expect("missing intermediates")
        .to_be_sign_hex
        .as_str();
    let expected_sig = decode_hex(expected_sig);
    let mut sig_out = vec![0u8; expected_sig.len() + 32];
    let sig_len = builder
        .encode_sig_structure(protected_bytes, payload, &mut sig_out)
        .unwrap();
    assert_eq!(&sig_out[..sig_len], expected_sig.as_slice());
    let expected_cbor = decode_hex(&vector.output.cbor);
    let (tag, untagged) = split_tag(&expected_cbor).expect("parse tag");
    assert_eq!(tag, Some(998));
    let sign1 = decode_sign1(untagged).expect("decode sign1");
    let mut encode_buf = [0u8; 64];
    let mut out = vec![0u8; untagged.len()];
    let len = builder
        .encode(&mut out, &mut encode_buf, sign1.signature)
        .unwrap();
    let tagged = encode_tag(998, &out[..len]);
    assert_eq!(expected_cbor, tagged);
}

#[test]
fn eddsa_sign_vectors_are_not_sign1() {
    let paths = ["eddsa-01.json", "eddsa-02.json"];
    for path in paths {
        let vector = load_sign_vector(path);
        assert!(!vector.title.is_empty());
        assert_eq!(vector.input.plaintext, "This is the content.");
        assert_eq!(vector.input.sign.signers.len(), 1);
        let signer = &vector.input.sign.signers[0];
        let _ = signer.protected.alg.as_deref();
        let _ = signer.unprotected.kid.as_deref();
        let _ = vector.input.sign.protected.ctyp;
        assert_eq!(vector.intermediates.signers.len(), 1);
        let expected_sig = decode_hex(&vector.intermediates.signers[0].to_be_sign_hex);

        let mut body_storage = ParamStorage::new();
        let mut signer_storage = ParamStorage::new();
        let body_protected =
            header_params_from_map(&vector.input.sign.protected, &mut body_storage);
        let signer_protected = header_params_from_map(&signer.protected, &mut signer_storage);

        let mut body_buf = [0u8; 32];
        let mut enc = Encoder::<4>::new(&mut body_buf);
        {
            let mut map = enc
                .map_with_len(
                    (body_protected.len()
                        + usize::from(alg_from_map(&vector.input.sign.protected).is_some()))
                        as u64,
                )
                .unwrap();
            encode_header_map(
                &mut map,
                alg_from_map(&vector.input.sign.protected),
                &body_protected,
            )
            .unwrap();
            map.end().unwrap();
        }
        let body_len = enc.len();
        let body_protected_bytes = protected_bstr_for_structure(&body_buf[..body_len]);

        let mut signer_buf = [0u8; 32];
        let mut enc = Encoder::<4>::new(&mut signer_buf);
        {
            let mut map = enc
                .map_with_len(
                    (signer_protected.len()
                        + usize::from(alg_from_map(&signer.protected).is_some()))
                        as u64,
                )
                .unwrap();
            encode_header_map(&mut map, alg_from_map(&signer.protected), &signer_protected)
                .unwrap();
            map.end().unwrap();
        }
        let signer_len = enc.len();
        let signer_protected_bytes = protected_bstr_for_structure(&signer_buf[..signer_len]);

        let mut sig_structure = vec![0u8; expected_sig.len() + 32];
        let sig_len = encode_sig_structure_sign(
            body_protected_bytes,
            signer_protected_bytes,
            b"",
            vector.input.plaintext.as_bytes(),
            &mut sig_structure,
        )
        .unwrap();
        assert_eq!(&sig_structure[..sig_len], expected_sig.as_slice());

        let expected_cbor = decode_hex(&vector.output.cbor);
        let (tag, untagged) = split_tag(&expected_cbor).expect("parse tag");
        assert_eq!(tag, Some(98));
        let err = decode_sign1(untagged).expect_err("eddsa vector is not Sign1");
        assert_eq!(err, CoseError::InvalidStructure);
        let tagged = encode_tag(98, untagged);
        assert_eq!(expected_cbor, tagged);
    }
}
