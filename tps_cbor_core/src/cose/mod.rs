/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! COSE operations and key generation.
//!
//! [`ops`] holds sign/verify/encrypt/decrypt; [`keygen`] mints keypairs. The split matters: all
//! cryptography in [`ops`] goes through `tps_cose` and thence `ring`, whereas [`keygen`] uses the
//! RustCrypto curve crates because `ring` will not hand back a freshly generated private scalar.
//! [`artefact`] is neither: it recognises what a byte string *is* and which stored key made it,
//! which is decoding plus calls into [`ops`].
//!
//! This module itself holds only the small amount of shared vocabulary: [`UnsupportedSign`], which
//! says *why* an otherwise valid key cannot sign, and the name helpers the UI uses to describe a
//! key.

pub mod artefact;
pub mod keygen;
pub mod ops;

// `artefact::eligible` stays qualified: a bare `eligible` says nothing about what for.
pub use artefact::{Artefact, Evidence, KeyIdentity, identify, sniff};

use tps_cose::{CoseKey, Curve};

/// Why a structurally valid key cannot be used for COSE signing.
///
/// Returned by [`unsupported_sign_reason`]. An enum rather than the `Option<String>` the old
/// backend used (`tps_cbor_gui/src-tauri/src/main.rs:551-582`), because the UI needs to
/// distinguish three different situations that all produce prose today:
///
/// * the key *could* sign with an algorithm `tps_cose` does not implement -- it is still useful,
///   and can be exported to an external tool;
/// * the key is for key agreement and will never sign, whatever the library supports;
/// * the key is symmetric, so the user wants the encrypt action instead.
///
/// The `Display` text is deliberately unchanged from the old backend's strings, since they were
/// reviewed prose and users may recognise them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnsupportedSign {
    /// ES384. The curve is fine; `tps_cose::Algorithm` has no ES384.
    #[error(
        "P-384 (ES384) signing is not supported: tps_cose implements only ES256 and EdDSA. \
         The key was generated and can be exported for use with external tools."
    )]
    Es384,

    /// ES512. `tps_cose::Algorithm` has no ES512 and the `ring` backend has no P-521 at all.
    #[error(
        "P-521 (ES512) signing is not supported: tps_cose implements only ES256 and EdDSA \
         (and the ring backend has no P-521). The key was generated and can be exported for \
         use with external tools."
    )]
    Es512,

    /// A key-agreement curve. No signature algorithm exists for it, in any library.
    #[error(
        "X25519 is a key-agreement curve and cannot be used for signing. \
         Use it for encryption/key agreement instead."
    )]
    KeyAgreement,

    /// A symmetric key. Signing needs an asymmetric key; the user probably wants encryption.
    #[error("Symmetric keys cannot be used for signing (use encryption).")]
    Symmetric,

    /// An EC2 curve this crate does not recognise.
    #[error("Unsupported EC2 curve for signing: {0}")]
    Ec2Curve(CurveName),

    /// An OKP curve this crate does not recognise.
    #[error("Unsupported OKP curve for signing: {0}")]
    OkpCurve(CurveName),

    /// The key is of a signable type but carries no private scalar.
    ///
    /// Signing needs `d`; a key exported or imported as public-only cannot do it. Reported
    /// separately because it is a key-*selection* mistake with an obvious remedy, not a limit of
    /// the library.
    #[error("this key has no private component, so it can verify but not sign")]
    NoPrivateKey,
}

/// Why a key cannot be used for COSE encryption.
///
/// Much simpler than [`UnsupportedSign`]: `tps_cose`'s COSE_Encrypt0 support is symmetric only,
/// so the only question is whether the key is symmetric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnsupportedEncrypt {
    /// An asymmetric key. COSE_Encrypt0 takes a shared symmetric key directly.
    #[error(
        "Asymmetric keys cannot be used for COSE_Encrypt0, which takes a shared symmetric key. \
         Generate or import a symmetric key instead."
    )]
    Asymmetric,

    /// A symmetric key of the wrong length for the chosen algorithm.
    ///
    /// Checked up front because the alternative is an opaque failure from `ring`. The old GUI
    /// could not hit this only because it hardcoded A128Gcm and sized every key at 16 bytes.
    #[error("this algorithm needs a {expected}-byte key, but the key is {actual} bytes")]
    WrongKeyLength {
        /// Length the algorithm requires.
        expected: usize,
        /// Length the key actually is.
        actual: usize,
    },
}

/// A [`Curve`] that can be named in a message.
///
/// `tps_cose::Curve` has no `Display`, and `{:?}` on `Curve::Other(9)` reads "Other(9)" rather
/// than anything a user would recognise. This wrapper exists so [`UnsupportedSign`] can be
/// `Copy` and still produce readable text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveName(pub Curve);

impl std::fmt::Display for CurveName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Curve::P256 => f.write_str("P-256"),
            Curve::P384 => f.write_str("P-384"),
            Curve::P521 => f.write_str("P-521"),
            Curve::X25519 => f.write_str("X25519"),
            Curve::X448 => f.write_str("X448"),
            Curve::Ed25519 => f.write_str("Ed25519"),
            Curve::Ed448 => f.write_str("Ed448"),
            Curve::Other(value) => write!(f, "curve {value}"),
        }
    }
}

/// The COSE key type of `key`, as the string recorded in the key store index.
pub fn key_type_name(key: &CoseKey<'_>) -> &'static str {
    match key {
        CoseKey::Ec2(_) => "EC2",
        CoseKey::Okp(_) => "OKP",
        CoseKey::Symmetric(_) => "Symmetric",
    }
}

/// The registry name of a COSE algorithm identifier, where this crate recognises one.
///
/// [`crate::keystore::KeyMeta::algorithm`] records the bare integer from the key's `alg` header,
/// which is what the COSE registry uses and what no user reads. Both frontends want the name, so
/// the table lives here rather than in either of them -- this crate's rule is one tested copy of
/// each algorithm.
///
/// Two halves, both delegating rather than restating. The signature algorithms are [`keygen`]'s
/// four tags, which is deliberately a *superset* of [`tps_cose::Algorithm`]: this crate generates
/// P-384 and P-521 keys it cannot sign with, and a key store holding one must still be able to say
/// what it is. The encryption algorithms come from [`ops::ENCRYPT_ALGORITHMS`] by way of
/// [`ops::encrypt_algorithm_name`], so a tenth algorithm added there is named here for free.
///
/// `None` for anything else, which the caller shows as the number. An algorithm this crate does not
/// implement is still worth displaying honestly, and a key imported from another tool may carry one.
pub fn algorithm_name(value: i64) -> Option<&'static str> {
    let signature = match value {
        -7 => Some("ES256"),
        -8 => Some("EdDSA"),
        -35 => Some("ES384"),
        -36 => Some("ES512"),
        _ => None,
    };
    signature.or_else(|| {
        ops::ENCRYPT_ALGORITHMS
            .iter()
            .copied()
            .find(|alg| alg.to_i64() == value)
            .map(ops::encrypt_algorithm_name)
    })
}

/// The curve of `key`, where it has one.
pub fn key_curve(key: &CoseKey<'_>) -> Option<Curve> {
    match key {
        CoseKey::Ec2(k) => Some(k.curve),
        CoseKey::Okp(k) => Some(k.curve),
        CoseKey::Symmetric(_) => None,
    }
}

/// The key identifier `key` carries, where it has one.
///
/// `tps_cose::CoseKey` is an enum over three key types with no accessor for the
/// [`tps_cose::KeyCommon`] all three share, so this is the one place that match is written.
/// [`artefact::identify`] needs it because [`crate::keystore::KeyMeta`] records no key id, and the
/// UI shows it as a detail row.
pub fn key_id<'a>(key: &CoseKey<'a>) -> Option<&'a [u8]> {
    match key {
        CoseKey::Ec2(k) => k.common.key_id,
        CoseKey::Okp(k) => k.common.key_id,
        CoseKey::Symmetric(k) => k.common.key_id,
    }
}

/// The private key material `key` carries: `d` for EC2 and OKP, `k` for Symmetric.
///
/// Borrowed from the decoded key, so it lives exactly as long as the buffer the key was decoded
/// from -- which for anything out of [`crate::keystore::KeyStore::load`] is a
/// [`zeroize::Zeroizing`]. Nothing here copies it; a caller that renders it is taking the exposure
/// on itself, which `UI_DESIGN.md:130`'s reveal button requires of the frontend.
pub fn key_private<'a>(key: &CoseKey<'a>) -> Option<&'a [u8]> {
    match key {
        CoseKey::Ec2(k) => k.d,
        CoseKey::Okp(k) => k.d,
        CoseKey::Symmetric(k) => Some(k.k),
    }
}

/// Whether `key` carries private key material.
pub fn key_has_private(key: &CoseKey<'_>) -> bool {
    match key {
        CoseKey::Ec2(k) => k.d.is_some(),
        CoseKey::Okp(k) => k.d.is_some(),
        // A symmetric key is *entirely* secret material; it has no public half to be without.
        CoseKey::Symmetric(_) => true,
    }
}

/// Return why `key` cannot be used for COSE signing, or `None` if it can.
///
/// Ported from `unsupported_sign_reason` (`tps_cbor_gui/src-tauri/src/main.rs:551-582`), returning
/// [`UnsupportedSign`] in place of a `String`.
pub fn unsupported_sign_reason(key: &CoseKey<'_>) -> Option<UnsupportedSign> {
    match key {
        CoseKey::Ec2(k) => match k.curve {
            Curve::P256 => None,
            Curve::P384 => Some(UnsupportedSign::Es384),
            Curve::P521 => Some(UnsupportedSign::Es512),
            other => Some(UnsupportedSign::Ec2Curve(CurveName(other))),
        },
        CoseKey::Okp(k) => match k.curve {
            Curve::Ed25519 => None,
            Curve::X25519 | Curve::X448 => Some(UnsupportedSign::KeyAgreement),
            other => Some(UnsupportedSign::OkpCurve(CurveName(other))),
        },
        CoseKey::Symmetric(_) => Some(UnsupportedSign::Symmetric),
    }
}

/// Return why `key` cannot be used for COSE_Encrypt0, or `None` if it can.
pub fn unsupported_encrypt_reason(key: &CoseKey<'_>) -> Option<UnsupportedEncrypt> {
    match key {
        CoseKey::Symmetric(_) => None,
        _ => Some(UnsupportedEncrypt::Asymmetric),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tps_cose::{Ec2Key, Ec2Y, KeyCommon, OkpKey, SymmetricKey};

    fn ec2(curve: Curve) -> CoseKey<'static> {
        CoseKey::Ec2(Ec2Key {
            common: KeyCommon::default(),
            curve,
            x: &[0u8; 32],
            y: Some(Ec2Y::Bytes(&[0u8; 32])),
            d: Some(&[0u8; 32]),
        })
    }

    fn okp(curve: Curve) -> CoseKey<'static> {
        CoseKey::Okp(OkpKey {
            common: KeyCommon::default(),
            curve,
            x: &[0u8; 32],
            d: Some(&[0u8; 32]),
        })
    }

    #[test]
    fn p256_and_ed25519_can_sign() {
        assert!(unsupported_sign_reason(&ec2(Curve::P256)).is_none());
        assert!(unsupported_sign_reason(&okp(Curve::Ed25519)).is_none());
    }

    #[test]
    fn the_three_known_refusals_are_distinguishable() {
        assert_eq!(
            unsupported_sign_reason(&ec2(Curve::P384)),
            Some(UnsupportedSign::Es384)
        );
        assert_eq!(
            unsupported_sign_reason(&ec2(Curve::P521)),
            Some(UnsupportedSign::Es512)
        );
        assert_eq!(
            unsupported_sign_reason(&okp(Curve::X25519)),
            Some(UnsupportedSign::KeyAgreement)
        );
    }

    #[test]
    fn a_symmetric_key_is_told_to_encrypt_instead() {
        let k = [0u8; 16];
        let key = CoseKey::Symmetric(SymmetricKey::new(&k));
        assert_eq!(
            unsupported_sign_reason(&key),
            Some(UnsupportedSign::Symmetric)
        );
        assert!(unsupported_encrypt_reason(&key).is_none());
    }

    #[test]
    fn an_asymmetric_key_cannot_encrypt() {
        assert_eq!(
            unsupported_encrypt_reason(&ec2(Curve::P256)),
            Some(UnsupportedEncrypt::Asymmetric)
        );
    }

    /// The old backend's `{:?}` on an unknown curve read "Other(9)"; this reads "curve 9".
    #[test]
    fn an_unknown_curve_is_named_readably() {
        let reason = unsupported_sign_reason(&ec2(Curve::Other(9))).expect("unsupported");
        assert_eq!(
            reason.to_string(),
            "Unsupported EC2 curve for signing: curve 9"
        );
    }

    #[test]
    fn curve_names_are_the_registry_spellings() {
        assert_eq!(CurveName(Curve::P256).to_string(), "P-256");
        assert_eq!(CurveName(Curve::Ed25519).to_string(), "Ed25519");
    }

    #[test]
    fn key_descriptions_report_type_curve_and_privacy() {
        let key = ec2(Curve::P384);
        assert_eq!(key_type_name(&key), "EC2");
        assert_eq!(key_curve(&key), Some(Curve::P384));
        assert!(key_has_private(&key));

        let k = [0u8; 16];
        let symmetric = CoseKey::Symmetric(SymmetricKey::new(&k));
        assert_eq!(key_type_name(&symmetric), "Symmetric");
        assert_eq!(key_curve(&symmetric), None);
        assert!(key_has_private(&symmetric));
    }

    /// Every algorithm the key store can *record* has a name, which is the property that matters:
    /// `GenKeyKind::algorithm` tags P-384 and P-521 keys, and those two values are outside
    /// `tps_cose::Algorithm` -- so a name table built from that enum would have shown a bare
    /// `-35` for a key this crate generates itself.
    #[test]
    fn every_algorithm_the_store_can_record_has_a_name() {
        for kind in crate::cose::keygen::GenKeyKind::ALL {
            if let Some(value) = kind.algorithm() {
                assert!(
                    algorithm_name(value).is_some(),
                    "{} is tagged {value} and has no name",
                    kind.label()
                );
            }
        }
        for alg in ops::ENCRYPT_ALGORITHMS {
            assert_eq!(
                algorithm_name(alg.to_i64()),
                Some(ops::encrypt_algorithm_name(alg))
            );
        }
    }

    /// `None` rather than a guess, so the caller can show the number instead of inventing a name.
    #[test]
    fn an_unregistered_algorithm_has_no_name() {
        assert_eq!(algorithm_name(-999), None);
        assert_eq!(algorithm_name(0), None);
    }

    #[test]
    fn a_public_only_key_reports_no_private_material() {
        let key = CoseKey::Ec2(Ec2Key {
            common: KeyCommon::default(),
            curve: Curve::P256,
            x: &[0u8; 32],
            y: Some(Ec2Y::Bytes(&[0u8; 32])),
            d: None,
        });
        assert!(!key_has_private(&key));
    }
}
