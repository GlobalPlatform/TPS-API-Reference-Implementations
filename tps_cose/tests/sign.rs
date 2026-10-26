/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#[allow(dead_code)]
mod common;

#[cfg(all(feature = "cose_sign", feature = "ring"))]
mod tests {
    #![allow(dead_code)]

    use super::common::{decode_base64url, decode_hex, split_tag};
    use serde::Deserialize;
    use tps_cose::{
        Algorithm, ContentType, CoseError, EdDsaPrivateKey, EdDsaPublicKey, EdDsaSigner,
        Es256PrivateKey, Es256PublicKey, Es256Signer, HeaderParam, Payload, SignBuilder,
        SignerEntry, decode_sign, verify_sign_eddsa, verify_sign_es256,
    };

    #[derive(Debug, Deserialize)]
    struct SignVector {
        #[serde(default)]
        fail: bool,
        input: SignInput,
        output: SignOutput,
    }

    #[derive(Debug, Deserialize)]
    struct SignInput {
        plaintext: String,
        sign: SignInputSign,
    }

    #[derive(Debug, Deserialize)]
    struct SignInputSign {
        #[serde(default)]
        protected: SignHeaders,
        signers: Vec<SignSigner>,
    }

    #[derive(Debug, Default, Deserialize)]
    struct SignHeaders {
        #[serde(default)]
        alg: Option<String>,
        #[serde(default)]
        ctyp: Option<u64>,
        #[serde(default)]
        kid: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct SignSigner {
        key: EcKey,
        #[serde(default)]
        protected: SignHeaders,
        #[serde(default)]
        unprotected: SignHeaders,
        #[serde(default)]
        external: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct EcKey {
        x: String,
        y: String,
    }

    #[derive(Debug, Deserialize)]
    struct SignOutput {
        cbor: String,
    }

    #[derive(Debug, Deserialize)]
    struct EdDsaSignVector {
        input: EdDsaSignInput,
        output: SignOutput,
    }

    #[derive(Debug, Deserialize)]
    struct EdDsaSignInput {
        plaintext: String,
        sign: EdDsaSignBody,
    }

    #[derive(Debug, Deserialize)]
    struct EdDsaSignBody {
        #[serde(default)]
        protected: SignHeaders,
        signers: Vec<EdDsaSignerInput>,
    }

    #[derive(Debug, Deserialize)]
    struct EdDsaSignerInput {
        key: EdDsaKey,
        #[serde(default)]
        protected: SignHeaders,
        #[serde(default)]
        unprotected: SignHeaders,
    }

    #[derive(Debug, Deserialize)]
    struct EdDsaKey {
        #[serde(rename = "x_hex")]
        x_hex: String,
        #[serde(rename = "d_hex")]
        d_hex: String,
    }

    fn load_sign_vector(name: &str) -> SignVector {
        let root = env!("CARGO_MANIFEST_DIR");
        let path = format!("{root}/tests/Examples/sign-tests/{name}");
        let data = std::fs::read_to_string(path).expect("read sign vector");
        serde_json::from_str(&data).expect("parse sign vector")
    }

    fn load_eddsa_sign_vector(name: &str) -> EdDsaSignVector {
        let root = env!("CARGO_MANIFEST_DIR");
        let path = format!("{root}/tests/Examples/eddsa-examples/{name}");
        let data = std::fs::read_to_string(path).expect("read eddsa sign vector");
        serde_json::from_str(&data).expect("parse eddsa sign vector")
    }

    fn verify_es256_sign_vector(name: &str) -> Result<(), CoseError> {
        let vector = load_sign_vector(name);
        let expected = decode_hex(&vector.output.cbor);
        let (tag, untagged) = split_tag(&expected).expect("split tag");
        if vector.fail && tag != Some(98) {
            return Ok(());
        }
        let sign = decode_sign(untagged)?;
        let payload = sign.payload.ok_or(CoseError::MissingPayload)?;
        assert_eq!(payload, vector.input.plaintext.as_bytes());
        let signer = vector.input.sign.signers.first().expect("signer");
        let external = signer
            .external
            .as_deref()
            .map(decode_hex)
            .unwrap_or_default();
        let x = decode_base64url(&signer.key.x);
        let y = decode_base64url(&signer.key.y);
        let key = Es256PublicKey { x: &x, y: &y };
        let signature = sign.signatures().next().expect("sig").expect("parse sig");
        let mut sig_structure = [0u8; 256];
        verify_sign_es256(&sign, &signature, &key, &external, None, &mut sig_structure)
    }

    #[test]
    fn cose_sign_es256_vectors() {
        let passes = [
            "sign-pass-01.json",
            "sign-pass-02.json",
            "sign-pass-03.json",
            "ecdsa-01.json",
        ];
        for path in passes {
            verify_es256_sign_vector(path).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        }

        let fails = [
            "sign-fail-01.json",
            "sign-fail-02.json",
            "sign-fail-03.json",
            "sign-fail-04.json",
            "sign-fail-06.json",
            "sign-fail-07.json",
        ];
        for path in fails {
            let vector = load_sign_vector(path);
            assert!(vector.fail);
            assert!(
                verify_es256_sign_vector(path).is_err() || path == "sign-fail-01.json",
                "{path}: expected failure"
            );
        }
    }

    #[test]
    fn cose_sign_eddsa_vector() {
        let vector = load_eddsa_sign_vector("eddsa-01.json");
        let expected = decode_hex(&vector.output.cbor);
        let (tag, untagged) = split_tag(&expected).expect("split tag");
        assert_eq!(tag, Some(98));
        let sign = decode_sign(untagged).expect("decode sign");
        assert_eq!(sign.payload, Some(vector.input.plaintext.as_bytes()));
        assert_eq!(sign.signature_count(), 1);
        let signer = vector.input.sign.signers.first().expect("signer");
        let x = decode_hex(&signer.key.x_hex);
        let key = EdDsaPublicKey { x: &x };
        let signature = sign.signatures().next().expect("sig").expect("parse sig");
        let mut sig_structure = [0u8; 256];
        verify_sign_eddsa(&sign, &signature, &key, b"", None, &mut sig_structure)
            .expect("verify eddsa");
    }

    #[test]
    fn cose_sign_multi_signature_round_trip() {
        let es256_d = decode_base64url("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM");
        let es256_x = decode_base64url("usWxHK2PmfnHKwXPS54m0kTcGJ90UiglWiGahtagnv8");
        let es256_y = decode_base64url("IBOL-C3BttVivg-lSreASjpkttcsz-1rb7btKLv8EX4");
        let ed_x = decode_hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let ed_d = decode_hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let es256_priv = Es256PrivateKey {
            d: &es256_d,
            x: &es256_x,
            y: &es256_y,
        };
        let es256_pub = Es256PublicKey {
            x: &es256_x,
            y: &es256_y,
        };
        let ed_priv = EdDsaPrivateKey { d: &ed_d, x: &ed_x };
        let ed_pub = EdDsaPublicKey { x: &ed_x };
        let es256_signer = Es256Signer::new(&es256_priv);
        let eddsa_signer = EdDsaSigner::new(&ed_priv);

        let body_protected = [HeaderParam::ContentType(ContentType::Uint(0))];
        let es256_unprotected = [HeaderParam::KeyId(b"11")];
        let eddsa_unprotected = [HeaderParam::KeyId(b"ed25519")];
        let es256_entry =
            SignerEntry::new(&[], &es256_unprotected, &es256_signer, Algorithm::Es256);
        let eddsa_entry =
            SignerEntry::new(&[], &eddsa_unprotected, &eddsa_signer, Algorithm::EdDsa);
        let signers = [es256_entry, eddsa_entry];

        let mut body_protected_buf = [0u8; 64];
        let mut signer_protected_buf = [0u8; 64];
        let mut sig_structure_buf = [0u8; 256];
        let mut signature_buf = [0u8; 128];
        let mut out = [0u8; 512];
        let msg_len = SignBuilder::new(&body_protected, &[])
            .payload(Payload::Attached(b"This is the content."))
            .signers(&signers)
            .sign(
                &mut body_protected_buf,
                &mut signer_protected_buf,
                &mut sig_structure_buf,
                &mut signature_buf,
                &mut out,
            )
            .expect("build sign");
        let decoded = decode_sign(&out[..msg_len]).expect("decode sign");
        assert_eq!(decoded.signature_count(), 2);
        let mut iter = decoded.signatures();
        let s0 = iter.next().expect("sig0").expect("parse sig0");
        let s1 = iter.next().expect("sig1").expect("parse sig1");

        verify_sign_es256(&decoded, &s0, &es256_pub, b"", None, &mut sig_structure_buf)
            .expect("verify es256");
        verify_sign_eddsa(&decoded, &s1, &ed_pub, b"", None, &mut sig_structure_buf)
            .expect("verify eddsa");
    }
}
