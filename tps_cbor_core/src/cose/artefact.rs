/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Recognising a COSE artefact, and naming the key behind one.
//!
//! Two questions the UI asks that no other module answers:
//!
//! 1. *What is this file?* `UI_DESIGN.md:119` lets File->Open accept a COSE key, and a COSE object
//!    is a `.cbor` file like any other, so the extension cannot decide. [`sniff`] decides by
//!    decoding.
//! 2. *Which key made this?* `UI_DESIGN.md:129` asks the right pane to show "the key used to create
//!    a loaded COSE item". [`identify`] answers it, or says honestly that it cannot.
//!
//! Neither is cryptography: everything here is decoding plus calls into [`ops`].

use tps_cose::{decode_cose_key, decode_encrypt0, decode_sign1};

use crate::cose::{key_id, ops};
use crate::keystore::{KeyMeta, KeyStore};

/// Which COSE artefact a byte string holds, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Artefact {
    /// A COSE_Key: key material, not a message.
    Key,
    /// A COSE_Sign1.
    Sign1,
    /// A COSE_Encrypt0.
    Encrypt0,
}

/// Recognise a COSE artefact by decoding it.
///
/// The three decoders are discriminating enough to sniff with, which was worth checking before
/// relying on it. All three are untagged, but all three are structurally strict:
///
/// * `decode_sign1` (`tps_cose/src/sign1.rs:310`) requires a **four**-element array of
///   `[bstr, map, bstr|nil, bstr]` *and* an algorithm header.
/// * `decode_encrypt0` (`tps_cose/src/encrypt0.rs:372`) requires a **three**-element array, an
///   encryption algorithm, and a nonce of the right length for that algorithm.
/// * `decode_cose_key` requires a **map** with a recognised `kty` at label 1, plus the curve and
///   coordinates that key type demands.
///
/// So an array cannot be mistaken for a key, and the two arrays are told apart by their length --
/// the order below is for determinism, not for correctness. The nearest miss is a CWT claims set,
/// which is a map whose label 1 is a `tstr` issuer rather than a key type; it has its own test,
/// because a false positive here would silently route a user's ordinary document to the COSE tab.
///
/// `None` is the common case and the one that must stay correct.
pub fn sniff(bytes: &[u8]) -> Option<Artefact> {
    if decode_sign1(bytes).is_ok() {
        return Some(Artefact::Sign1);
    }
    if decode_encrypt0(bytes).is_ok() {
        return Some(Artefact::Encrypt0);
    }
    if decode_cose_key(bytes).is_ok() {
        return Some(Artefact::Key);
    }
    None
}

/// Whether `meta` names a key that could possibly apply to `artefact`.
///
/// Signing needs an asymmetric key, so EC2 or OKP; `COSE_Encrypt0` takes a shared symmetric key
/// directly, so Symmetric. Nothing applies to a bare key, which is key material rather than
/// something a key produced.
///
/// Used to keep [`identify`]'s second pass from running an AEAD open against a signing key, which
/// would be a guaranteed failure reported as if it were evidence.
pub fn eligible(artefact: Artefact, meta: &KeyMeta) -> bool {
    match artefact {
        Artefact::Key => false,
        Artefact::Sign1 => meta.key_type == "EC2" || meta.key_type == "OKP",
        Artefact::Encrypt0 => meta.key_type == "Symmetric",
    }
}

/// How [`identify`] established which key made an artefact.
///
/// Worth distinguishing in the UI, because the two claims are not equally strong: a `kid` is an
/// assertion carried in the message and is not authenticated, whereas a successful verification or
/// decryption is proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The message's `kid` header matched the key's key identifier.
    Kid,
    /// The key verified the signature, or opened the ciphertext.
    TrialVerification,
}

/// Which stored key produced some bytes, or that none did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyIdentity {
    /// Not a COSE artefact at all, so there is no key to name: the right pane shows the key the
    /// user selected instead.
    NotCose,
    /// A COSE artefact that none of the stored keys accounts for -- `UI_DESIGN.md:129`'s
    /// "Unknown key".
    Unknown,
    /// The alias of the key that accounts for it, and how that was established.
    Known {
        /// The alias in the key store.
        alias: String,
        /// What makes this the answer.
        by: Evidence,
    },
}

impl Default for KeyIdentity {
    /// [`KeyIdentity::NotCose`], which is what an empty COSE document is.
    fn default() -> Self {
        Self::NotCose
    }
}

/// Name the key behind a COSE artefact.
///
/// Two passes, because one is not enough.
///
/// The `kid` header settles it when there is one. But `generate_cose_key` leaves `key_id: None`
/// (`keygen.rs:167`, `:188`), `KeyStore::generate_symmetric` does the same (`keystore.rs:275`), and
/// [`ops::sign`] writes no `kid` header -- so every artefact this tool produces itself is
/// unmatchable by kid, and a kid-only implementation would report "Unknown key" for the tool's own
/// output. Hence the second pass, which tries the operation: a `COSE_Sign1` a key verifies, or a
/// `COSE_Encrypt0` a key opens, *was* made by that key, which is a stronger answer than a kid
/// anyway.
///
/// [`eligible`] keeps the second pass to the keys that could apply. Within it a failure is just a
/// failure: an AEAD open under the wrong key is indistinguishable from the wrong key length, and
/// neither has any effect beyond returning an error.
///
/// Both passes load and decode each candidate key, because [`KeyMeta`] records no key identifier
/// (`keystore.rs:108-124`) and `ops` takes a borrowed `CoseKey`. That is why this is called when
/// the document changes and never from a render path.
pub fn identify(bytes: &[u8], store: &KeyStore) -> KeyIdentity {
    let Some(artefact) = sniff(bytes) else {
        return KeyIdentity::NotCose;
    };

    if let Some(kid) = message_key_id(bytes, artefact) {
        for meta in store.list() {
            if !eligible(artefact, meta) {
                continue;
            }
            let Ok(stored) = store.load(&meta.alias) else {
                continue;
            };
            let Ok(key) = decode_cose_key(&stored) else {
                continue;
            };
            if key_id(&key) == Some(kid) {
                return KeyIdentity::Known {
                    alias: meta.alias.clone(),
                    by: Evidence::Kid,
                };
            }
        }
    }

    for meta in store.list() {
        if !eligible(artefact, meta) {
            continue;
        }
        let Ok(stored) = store.load(&meta.alias) else {
            continue;
        };
        let Ok(key) = decode_cose_key(&stored) else {
            continue;
        };
        let accounts_for_it = match artefact {
            Artefact::Sign1 => ops::verify(bytes, &key).is_ok(),
            Artefact::Encrypt0 => ops::decrypt(bytes, &key).is_ok(),
            // Filtered out by `eligible` already; restated rather than left to a wildcard so that
            // a new `Artefact` variant has to be considered here.
            Artefact::Key => false,
        };
        if accounts_for_it {
            return KeyIdentity::Known {
                alias: meta.alias.clone(),
                by: Evidence::TrialVerification,
            };
        }
    }

    KeyIdentity::Unknown
}

/// The `kid` a message carries, from whichever header bucket holds it.
///
/// Protected first: it is the authenticated one, and a disagreement between the buckets should be
/// resolved in favour of the bytes the signature or tag covers.
fn message_key_id(bytes: &[u8], artefact: Artefact) -> Option<&[u8]> {
    match artefact {
        Artefact::Sign1 => {
            let message = decode_sign1(bytes).ok()?;
            message.protected.key_id.or(message.unprotected.key_id)
        }
        Artefact::Encrypt0 => {
            let message = decode_encrypt0(bytes).ok()?;
            message.protected.key_id.or(message.unprotected.key_id)
        }
        Artefact::Key => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cose::keygen::{GenKeyKind, generate_cose_key};
    use crate::keystore::Overwrite;
    use std::path::{Path, PathBuf};
    use tps_cose::{
        Algorithm, CoseKey, Ec2Key, EncryptAlgorithm, Es256PrivateKey, HeaderParam, Payload,
        Sign1Builder,
    };

    const PAYLOAD: &[u8] = &[0xa1, 0x01, 0x63, 0x61, 0x62, 0x63];

    fn examples() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("examples")
    }

    /// A fresh, empty directory unique to `name`, following `keystore.rs`'s own convention.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps_cbor_core_artefact_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn open_store(name: &str) -> (PathBuf, KeyStore) {
        let dir = temp_dir(name);
        let store = KeyStore::open(&dir).expect("open");
        (dir, store)
    }

    /// The most important test in the phase: an ordinary document must not be taken for COSE,
    /// because a false positive routes a user's file to the wrong tab with no way to tell why.
    #[test]
    fn an_ordinary_cbor_document_is_not_a_cose_artefact() {
        let dir = examples();
        for name in [
            "flat_tee_claims_good.cbor",
            "flat_tee_claims_bad.cbor",
            "trace_demo_good.cbor",
            "trace_demo_bad.cbor",
        ] {
            let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(sniff(&bytes), None, "{name}");
        }
    }

    /// The nearest miss, stated explicitly: a CWT claims set is a map, like a key, and its label 1
    /// is occupied -- by a `tstr` issuer rather than a key type.
    #[test]
    fn a_claims_set_is_not_mistaken_for_a_cose_key() {
        // {1: "issuer", 2: "subject"}
        let claims = [
            0xa2, 0x01, 0x66, b'i', b's', b's', b'u', b'e', b'r', 0x02, 0x67, b's', b'u', b'b',
            b'j', b'e', b'c', b't',
        ];
        assert_eq!(sniff(&claims), None);
    }

    #[test]
    fn the_sniffer_tells_the_three_artefacts_apart() {
        let key_bytes = generate_cose_key(GenKeyKind::Ec2P256).expect("generate");
        assert_eq!(sniff(&key_bytes), Some(Artefact::Key));

        let key = decode_cose_key(&key_bytes).expect("decode");
        let signed = ops::sign(PAYLOAD, &key).expect("sign");
        assert_eq!(sniff(&signed), Some(Artefact::Sign1));

        let k = [3u8; 16];
        let symmetric = CoseKey::Symmetric(tps_cose::SymmetricKey::new(&k));
        let sealed = ops::encrypt(PAYLOAD, &symmetric, EncryptAlgorithm::A128Gcm).expect("encrypt");
        assert_eq!(sniff(&sealed), Some(Artefact::Encrypt0));
    }

    /// The test the two-pass design exists for: a key this tool generated carries no key id, so
    /// kid matching alone would answer "Unknown key" for the tool's own signature.
    #[test]
    fn identify_names_the_signing_key_by_trial_verification() {
        let (dir, mut store) = open_store("trial");
        let meta = store
            .generate("signer", GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("generate");
        assert!(meta.has_private, "the generated key has a private part");

        let bytes = store.load("signer").expect("load");
        let key = decode_cose_key(&bytes).expect("decode");
        assert_eq!(key_id(&key), None, "generated keys carry no kid");
        let signed = ops::sign(PAYLOAD, &key).expect("sign");

        assert_eq!(
            identify(&signed, &store),
            KeyIdentity::Known {
                alias: "signer".to_owned(),
                by: Evidence::TrialVerification,
            }
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn identify_prefers_a_matching_kid() {
        let (dir, mut store) = open_store("kid");

        // A generated key, re-encoded with a key id, so the stored bytes carry one.
        let generated = generate_cose_key(GenKeyKind::Ec2P256).expect("generate");
        let decoded = decode_cose_key(&generated).expect("decode");
        let CoseKey::Ec2(ec2) = decoded else {
            panic!("P-256 is an EC2 key");
        };
        let mut common = ec2.common;
        common.key_id = Some(b"kid-1");
        let kidded = CoseKey::Ec2(Ec2Key { common, ..ec2 });
        let mut buf = [0u8; 256];
        let n = kidded.encode(&mut buf).expect("encode");
        store
            .import("kidded", &buf[..n], "now", Overwrite::Refuse)
            .expect("import");

        // A COSE_Sign1 that names the key in its unprotected header.
        let d = ec2.d.expect("the generated key has d");
        let tps_cose::Ec2Y::Bytes(y) = ec2.y.expect("uncompressed y") else {
            panic!("the generator writes y as coordinates");
        };
        let mut protected = [0u8; 256];
        let mut structure = [0u8; 1024];
        let mut signature = [0u8; 128];
        let mut out = [0u8; 1024];
        let n = Sign1Builder::new(&[], &[HeaderParam::KeyId(b"kid-1")])
            .algorithm(Algorithm::Es256)
            .payload(Payload::Attached(PAYLOAD))
            .sign_es256(
                &Es256PrivateKey { d, x: ec2.x, y },
                &mut protected,
                &mut structure,
                &mut signature,
                &mut out,
            )
            .expect("sign");

        assert_eq!(
            identify(&out[..n], &store),
            KeyIdentity::Known {
                alias: "kidded".to_owned(),
                by: Evidence::Kid,
            },
            "the kid pass runs first, so the evidence is the kid and not the verification"
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn identify_answers_unknown_when_no_key_fits() {
        let (dir, mut store) = open_store("unknown");
        let stranger = generate_cose_key(GenKeyKind::Ec2P256).expect("generate");
        let signed =
            ops::sign(PAYLOAD, &decode_cose_key(&stranger).expect("decode")).expect("sign");

        // An eligible key that did not make it, so the trial pass runs and still says no.
        store
            .generate("other", GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("generate");
        assert_eq!(identify(&signed, &store), KeyIdentity::Unknown);

        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// An empty store cannot name anything, and an ordinary document is not a COSE artefact at
    /// all -- two different answers, and the pane shows different things for each.
    #[test]
    fn identify_distinguishes_unknown_from_not_cose() {
        let (dir, store) = open_store("not_cose");
        assert_eq!(identify(PAYLOAD, &store), KeyIdentity::NotCose);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn identify_skips_keys_that_cannot_apply() {
        let (dir, mut store) = open_store("eligible");
        let signing = store
            .generate("signer", GenKeyKind::OkpEd25519, "now", Overwrite::Refuse)
            .expect("generate");
        let wrapping = store
            .generate_symmetric(
                "wrapper",
                EncryptAlgorithm::A128Gcm,
                "now",
                Overwrite::Refuse,
            )
            .expect("generate");

        assert!(eligible(Artefact::Sign1, &signing));
        assert!(!eligible(Artefact::Sign1, &wrapping));
        assert!(eligible(Artefact::Encrypt0, &wrapping));
        assert!(!eligible(Artefact::Encrypt0, &signing));
        assert!(!eligible(Artefact::Key, &signing));
        assert!(!eligible(Artefact::Key, &wrapping));

        // And the symmetric key is still the one found for an Encrypt0, with the signing key in
        // the store beside it.
        let bytes = store.load("wrapper").expect("load");
        let key = decode_cose_key(&bytes).expect("decode");
        let sealed = ops::encrypt(PAYLOAD, &key, EncryptAlgorithm::A128Gcm).expect("encrypt");
        assert_eq!(
            identify(&sealed, &store),
            KeyIdentity::Known {
                alias: "wrapper".to_owned(),
                by: Evidence::TrialVerification,
            }
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The round trip the `Verify` action's result destination rests on.
    #[test]
    fn sign_then_verify_returns_the_payload_unchanged() {
        let key_bytes = generate_cose_key(GenKeyKind::Ec2P256).expect("generate");
        let key = decode_cose_key(&key_bytes).expect("decode");
        let signed = ops::sign(PAYLOAD, &key).expect("sign");
        assert_eq!(ops::verify(&signed, &key).expect("verify"), PAYLOAD);
    }

    /// The round trip the `Decrypt` action's result destination rests on.
    #[test]
    fn encrypt_then_decrypt_returns_the_plaintext_unchanged() {
        let k = [5u8; 32];
        let key = CoseKey::Symmetric(tps_cose::SymmetricKey::new(&k));
        let sealed =
            ops::encrypt(PAYLOAD, &key, EncryptAlgorithm::AesCcm16_64_256).expect("encrypt");
        assert_eq!(ops::decrypt(&sealed, &key).expect("decrypt"), PAYLOAD);
    }
}
