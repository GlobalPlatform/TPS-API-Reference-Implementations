/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! COSE keypair generation.
//!
//! `ring` -- used everywhere else in this crate for sign/verify/encrypt/decrypt -- does not expose
//! the raw private scalar of a freshly generated key, and has no P-521 or extractable X25519. So
//! generation uses the RustCrypto elliptic-curve crates, which hand back the scalar and affine
//! coordinates directly, and the resulting bytes are assembled into a `tps_cose::CoseKey`.
//!
//! This module only *mints* keys. All cryptographic operations still go through `tps_cose` and
//! `ring`; see [`crate::cose::ops`].
//!
//! Ported from `tps_cbor_gui/src-tauri/src/keygen.rs` with one substantive change: every buffer
//! that holds a private scalar, and the encoded COSE_Key itself, is wrapped in
//! [`Zeroizing`](zeroize::Zeroizing) so it is wiped on drop. The old version left the scalar in
//! three separate heap buffers -- `to_bytes()`'s output, the encode scratch buffer, and the
//! returned `Vec` -- none of them cleared.
//!
//! One residual exposure is worth naming: the `to_bytes()` temporaries returned by the RustCrypto
//! secret-key types are plain arrays, not zeroizing ones, so the scalar exists unwiped for the
//! duration of one expression before being copied into a `Zeroizing` buffer. Closing that would
//! mean reaching past the public API of those crates.
//!
//! # A caveat the UI must repeat
//!
//! Key material here comes from `rand_core::OsRng`, i.e. the operating system CSPRNG. That is
//! sound on a desktop, but this is a developer tool and the keys it mints are for testing. The
//! IDE shows a warning to that effect on every generation; see `UI_DESIGN.md`.

use p256::elliptic_curve::sec1::ToEncodedPoint;
use tps_cose::{CoseError, CoseKey, Curve, Ec2Key, Ec2Y, KeyCommon, OkpKey};
use zeroize::Zeroizing;

/// COSE algorithm identifiers from the RFC 9053 registry, used to tag generated keys.
///
/// `tps_cose::Algorithm` models only ES256 and EdDSA, so the wider ECDSA values are spelled out
/// here. A P-384 or P-521 key is still worth generating and exporting even though this crate
/// cannot sign with it; see [`crate::cose::UnsupportedSign`].
const ALG_ES256: i64 = -7;
const ALG_ES384: i64 = -35;
const ALG_ES512: i64 = -36;
const ALG_EDDSA: i64 = -8;

/// Headroom over the field size when sizing the COSE_Key encode buffer.
///
/// A COSE_Key holds at most four field-sized values (`d`, `x`, `y`, and for OKP just `d` and `x`)
/// plus a handful of small integer labels, so four times the largest field plus a fixed allowance
/// is comfortably sufficient.
const KEY_ENCODE_OVERHEAD: usize = 128;

/// The kinds of keypair this crate can generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenKeyKind {
    /// EC2 on NIST P-256, tagged ES256. The only EC2 kind this crate can sign with.
    Ec2P256,
    /// EC2 on NIST P-384, tagged ES384. Generated for export; cannot sign here.
    Ec2P384,
    /// EC2 on NIST P-521, tagged ES512. Generated for export; cannot sign here.
    Ec2P521,
    /// OKP on Ed25519, tagged EdDSA.
    OkpEd25519,
    /// OKP on X25519, untagged. Key agreement only.
    OkpX25519,
}

impl GenKeyKind {
    /// Every kind, in menu order.
    pub const ALL: [GenKeyKind; 5] = [
        GenKeyKind::Ec2P256,
        GenKeyKind::Ec2P384,
        GenKeyKind::Ec2P521,
        GenKeyKind::OkpEd25519,
        GenKeyKind::OkpX25519,
    ];

    /// The stable identifier used in settings files and on the command line.
    pub const fn id(self) -> &'static str {
        match self {
            GenKeyKind::Ec2P256 => "ec2-p256",
            GenKeyKind::Ec2P384 => "ec2-p384",
            GenKeyKind::Ec2P521 => "ec2-p521",
            GenKeyKind::OkpEd25519 => "okp-ed25519",
            GenKeyKind::OkpX25519 => "okp-x25519",
        }
    }

    /// A label for a menu or dropdown.
    pub const fn label(self) -> &'static str {
        match self {
            GenKeyKind::Ec2P256 => "EC2 / P-256 (ES256)",
            GenKeyKind::Ec2P384 => "EC2 / P-384 (ES384)",
            GenKeyKind::Ec2P521 => "EC2 / P-521 (ES512)",
            GenKeyKind::OkpEd25519 => "OKP / Ed25519 (EdDSA)",
            GenKeyKind::OkpX25519 => "OKP / X25519 (key agreement)",
        }
    }

    /// The curve the generated key will be on.
    pub const fn curve(self) -> Curve {
        match self {
            GenKeyKind::Ec2P256 => Curve::P256,
            GenKeyKind::Ec2P384 => Curve::P384,
            GenKeyKind::Ec2P521 => Curve::P521,
            GenKeyKind::OkpEd25519 => Curve::Ed25519,
            GenKeyKind::OkpX25519 => Curve::X25519,
        }
    }

    /// The COSE algorithm the generated key will be tagged with, if any.
    ///
    /// `None` for X25519: it is a key-agreement curve with no signature or AEAD algorithm of its
    /// own, and tagging it would be wrong.
    pub const fn algorithm(self) -> Option<i64> {
        match self {
            GenKeyKind::Ec2P256 => Some(ALG_ES256),
            GenKeyKind::Ec2P384 => Some(ALG_ES384),
            GenKeyKind::Ec2P521 => Some(ALG_ES512),
            GenKeyKind::OkpEd25519 => Some(ALG_EDDSA),
            GenKeyKind::OkpX25519 => None,
        }
    }
}

impl std::str::FromStr for GenKeyKind {
    type Err = UnknownKeyKind;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        GenKeyKind::ALL
            .into_iter()
            .find(|kind| kind.id() == s)
            .ok_or_else(|| UnknownKeyKind(s.to_owned()))
    }
}

/// A key kind identifier that does not name any [`GenKeyKind`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown key kind '{0}'")]
pub struct UnknownKeyKind(pub String);

/// Generate a fresh keypair of `kind`, returning the encoded COSE_Key CBOR bytes.
///
/// The returned bytes include the private key, and are [`Zeroizing`] so the caller cannot
/// accidentally leave a copy on the heap. Writing them to disk is the key store's job; see
/// [`crate::keystore`], which also sets the file permissions.
pub fn generate_cose_key(kind: GenKeyKind) -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    match kind {
        GenKeyKind::Ec2P256 => gen_p256(),
        GenKeyKind::Ec2P384 => gen_p384(),
        GenKeyKind::Ec2P521 => gen_p521(),
        GenKeyKind::OkpEd25519 => gen_ed25519(),
        GenKeyKind::OkpX25519 => gen_x25519(),
    }
}

/// Encode an EC2 private key (`d` + `x` + `y`) as COSE_Key bytes.
fn encode_ec2(
    curve: Curve,
    alg: i64,
    d: &[u8],
    x: &[u8],
    y: &[u8],
) -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let key = CoseKey::Ec2(Ec2Key {
        common: KeyCommon {
            key_id: None,
            algorithm: Some(alg),
            base_iv: None,
        },
        curve,
        x,
        y: Some(Ec2Y::Bytes(y)),
        d: Some(d),
    });
    encode(&key, d.len())
}

/// Encode an OKP key (`x` + `d`) as COSE_Key bytes.
fn encode_okp(
    curve: Curve,
    alg: Option<i64>,
    d: &[u8],
    x: &[u8],
) -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let key = CoseKey::Okp(OkpKey {
        common: KeyCommon {
            key_id: None,
            algorithm: alg,
            base_iv: None,
        },
        curve,
        x,
        d: Some(d),
    });
    encode(&key, d.len().max(x.len()))
}

/// Encode a `CoseKey` into a right-sized, zeroizing buffer.
///
/// `field_len` is the size of the largest field. The buffer is allocated at
/// `field_len * 4 + `[`KEY_ENCODE_OVERHEAD`] and truncated to the bytes actually written; because
/// it is [`Zeroizing`], the slack is wiped along with the rest on drop.
fn encode(
    key: &CoseKey<'_>,
    field_len: usize,
) -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let mut out = Zeroizing::new(vec![0u8; field_len * 4 + KEY_ENCODE_OVERHEAD]);
    let n = key.encode(&mut out)?;
    out.truncate(n);
    Ok(out)
}

fn gen_p256() -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let sk = p256::SecretKey::random(&mut rand_core::OsRng);
    let d = Zeroizing::new(sk.to_bytes().to_vec());
    let point = sk.public_key().to_encoded_point(false);
    let x = point.x().ok_or(CoseError::InvalidKey)?;
    let y = point.y().ok_or(CoseError::InvalidKey)?;
    encode_ec2(Curve::P256, ALG_ES256, d.as_slice(), x, y)
}

fn gen_p384() -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let sk = p384::SecretKey::random(&mut rand_core::OsRng);
    let d = Zeroizing::new(sk.to_bytes().to_vec());
    let point = sk.public_key().to_encoded_point(false);
    let x = point.x().ok_or(CoseError::InvalidKey)?;
    let y = point.y().ok_or(CoseError::InvalidKey)?;
    encode_ec2(Curve::P384, ALG_ES384, d.as_slice(), x, y)
}

fn gen_p521() -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    let sk = p521::SecretKey::random(&mut rand_core::OsRng);
    let d = Zeroizing::new(sk.to_bytes().to_vec());
    let point = sk.public_key().to_encoded_point(false);
    let x = point.x().ok_or(CoseError::InvalidKey)?;
    let y = point.y().ok_or(CoseError::InvalidKey)?;
    encode_ec2(Curve::P521, ALG_ES512, d.as_slice(), x, y)
}

fn gen_ed25519() -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    use ed25519_dalek::SigningKey;
    let sk = SigningKey::generate(&mut rand_core::OsRng);
    // The 32-byte seed, which is what COSE calls `d` for an OKP key.
    let d = Zeroizing::new(sk.to_bytes().to_vec());
    let x = sk.verifying_key().to_bytes();
    encode_okp(Curve::Ed25519, Some(ALG_EDDSA), d.as_slice(), &x)
}

fn gen_x25519() -> std::result::Result<Zeroizing<Vec<u8>>, CoseError> {
    use x25519_dalek::{PublicKey, StaticSecret};
    let secret = StaticSecret::random_from_rng(rand_core::OsRng);
    let d = Zeroizing::new(secret.to_bytes().to_vec());
    let x = PublicKey::from(&secret).to_bytes();
    // X25519 is a key-agreement curve, not a signing algorithm: leave `alg` unset.
    encode_okp(Curve::X25519, None, d.as_slice(), &x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cose::ops::{sign, verify};
    use crate::cose::{key_curve, key_has_private, unsupported_sign_reason};

    /// Every generated key must decode back to the expected type, curve and field sizes, and
    /// carry a private scalar.
    #[test]
    fn generated_keys_decode_with_the_expected_shape() {
        let cases = [
            (GenKeyKind::Ec2P256, Curve::P256, 32),
            (GenKeyKind::Ec2P384, Curve::P384, 48),
            (GenKeyKind::Ec2P521, Curve::P521, 66),
            (GenKeyKind::OkpEd25519, Curve::Ed25519, 32),
            (GenKeyKind::OkpX25519, Curve::X25519, 32),
        ];
        for (kind, expected_curve, field_len) in cases {
            let bytes = generate_cose_key(kind).expect("generate");
            let key = CoseKey::decode(&bytes).expect("decode");
            assert_eq!(key_curve(&key), Some(expected_curve), "{kind:?} curve");
            assert_eq!(
                key_curve(&key),
                Some(kind.curve()),
                "{kind:?} declared curve"
            );
            assert!(key_has_private(&key), "{kind:?} missing d");
            match key {
                CoseKey::Ec2(k) => {
                    assert_eq!(k.d.expect("d").len(), field_len, "{kind:?} d len");
                    assert_eq!(k.x.len(), field_len, "{kind:?} x len");
                    assert_eq!(k.common.algorithm, kind.algorithm(), "{kind:?} alg");
                }
                CoseKey::Okp(k) => {
                    assert_eq!(k.d.expect("d").len(), field_len, "{kind:?} d len");
                    assert_eq!(k.x.len(), 32, "{kind:?} x len");
                    assert_eq!(k.common.algorithm, kind.algorithm(), "{kind:?} alg");
                }
                CoseKey::Symmetric(_) => panic!("{kind:?} decoded as symmetric"),
            }
        }
    }

    #[test]
    fn x25519_is_not_tagged_with_an_algorithm() {
        let bytes = generate_cose_key(GenKeyKind::OkpX25519).expect("generate");
        let key = CoseKey::decode(&bytes).expect("decode");
        let CoseKey::Okp(k) = key else {
            panic!("not OKP")
        };
        assert_eq!(k.common.algorithm, None);
    }

    #[test]
    fn a_generated_p256_key_signs_and_verifies() {
        let bytes = generate_cose_key(GenKeyKind::Ec2P256).expect("generate");
        let key = CoseKey::decode(&bytes).expect("decode");
        let signed = sign(b"hello p256", &key).expect("sign");
        assert_eq!(verify(&signed, &key).expect("verify"), b"hello p256");
    }

    #[test]
    fn a_generated_ed25519_key_signs_and_verifies() {
        let bytes = generate_cose_key(GenKeyKind::OkpEd25519).expect("generate");
        let key = CoseKey::decode(&bytes).expect("decode");
        let signed = sign(b"hello ed25519", &key).expect("sign");
        assert_eq!(verify(&signed, &key).expect("verify"), b"hello ed25519");
    }

    /// The keys this crate can generate but not sign with are generated anyway, and say why.
    #[test]
    fn the_export_only_kinds_generate_and_explain_themselves() {
        for kind in [
            GenKeyKind::Ec2P384,
            GenKeyKind::Ec2P521,
            GenKeyKind::OkpX25519,
        ] {
            let bytes = generate_cose_key(kind).unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
            let key = CoseKey::decode(&bytes).expect("decode");
            assert!(
                unsupported_sign_reason(&key).is_some(),
                "{kind:?} should not be signable"
            );
        }
    }

    #[test]
    fn two_generations_differ() {
        let first = generate_cose_key(GenKeyKind::Ec2P256).unwrap();
        let second = generate_cose_key(GenKeyKind::Ec2P256).unwrap();
        assert_ne!(*first, *second, "generated the same key twice");
    }

    #[test]
    fn ids_round_trip_through_from_str() {
        for kind in GenKeyKind::ALL {
            assert_eq!(kind.id().parse::<GenKeyKind>(), Ok(kind));
        }
        assert_eq!(
            "nope".parse::<GenKeyKind>(),
            Err(UnknownKeyKind("nope".to_owned()))
        );
    }

    #[test]
    fn every_kind_has_a_distinct_id_and_label() {
        let mut ids: Vec<&str> = GenKeyKind::ALL.iter().map(|k| k.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), GenKeyKind::ALL.len());

        let mut labels: Vec<&str> = GenKeyKind::ALL.iter().map(|k| k.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), GenKeyKind::ALL.len());
    }
}
