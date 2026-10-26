/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_minicbor::{Decoder, Encoder, Item, MapBuilder};

use crate::CoseError;
use crate::header::{Label, ensure_no_duplicate_labels, parse_label};

#[cfg(feature = "ring")]
use crate::crypto::ring::{EdDsaPrivateKey, EdDsaPublicKey, Es256PrivateKey, Es256PublicKey};

/// COSE key type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// Octet Key Pair (RFC 9053 §7.1) — e.g. Ed25519/X25519.
    Okp,
    /// Double-coordinate elliptic curve key (RFC 9053 §7.1) — e.g. P-256.
    Ec2,
    /// Symmetric key.
    Symmetric,
}

impl KeyType {
    /// Convert to the COSE `kty` (label 1) value defined by RFC 9053 §7.1.
    pub const fn to_i64(self) -> i64 {
        match self {
            KeyType::Okp => 1,
            KeyType::Ec2 => 2,
            KeyType::Symmetric => 4,
        }
    }

    /// Convert from a COSE `kty` (label 1) value, or `None` if unrecognized.
    pub const fn from_i64(value: i64) -> Option<Self> {
        match value {
            1 => Some(KeyType::Okp),
            2 => Some(KeyType::Ec2),
            4 => Some(KeyType::Symmetric),
            _ => None,
        }
    }
}

/// COSE curve identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// NIST P-256.
    P256,
    /// NIST P-384.
    P384,
    /// NIST P-521.
    P521,
    /// X25519 for use w/ ECDH only.
    X25519,
    /// X448 for use w/ ECDH only.
    X448,
    /// Ed25519 for use w/ EdDSA only.
    Ed25519,
    /// Ed448 for use w/ EdDSA only.
    Ed448,
    /// A curve identifier not recognized by this crate.
    Other(i64),
}

impl Curve {
    /// Convert to the COSE curve (`crv`, label -1) value defined by RFC 9053 §7.1.
    pub const fn to_i64(self) -> i64 {
        match self {
            Curve::P256 => 1,
            Curve::P384 => 2,
            Curve::P521 => 3,
            Curve::X25519 => 4,
            Curve::X448 => 5,
            Curve::Ed25519 => 6,
            Curve::Ed448 => 7,
            Curve::Other(v) => v,
        }
    }

    /// Convert from a COSE curve (`crv`, label -1) value.
    pub const fn from_i64(value: i64) -> Self {
        match value {
            1 => Curve::P256,
            2 => Curve::P384,
            3 => Curve::P521,
            4 => Curve::X25519,
            5 => Curve::X448,
            6 => Curve::Ed25519,
            7 => Curve::Ed448,
            v => Curve::Other(v),
        }
    }
}

/// Common COSE key parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyCommon<'a> {
    /// Key identifier (label 2).
    pub key_id: Option<&'a [u8]>,
    /// Algorithm this key is used with (label 3).
    pub algorithm: Option<i64>,
    /// Base IV to be XORed with partial IVs (label 5).
    pub base_iv: Option<&'a [u8]>,
}

/// EC2 y-coordinate representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ec2Y<'a> {
    /// Explicit y-coordinate bytes.
    Bytes(&'a [u8]),
    /// Compressed-point sign bit, per SEC1.
    Sign(bool),
}

/// EC2 COSE key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ec2Key<'a> {
    /// Parameters shared across all COSE key types.
    pub common: KeyCommon<'a>,
    /// Curve this key is defined over.
    pub curve: Curve,
    /// x-coordinate.
    pub x: &'a [u8],
    /// y-coordinate, if present.
    pub y: Option<Ec2Y<'a>>,
    /// Private key, if present.
    pub d: Option<&'a [u8]>,
}

impl<'a> Ec2Key<'a> {
    /// Create a new public EC2 key with no [`KeyCommon`] parameters set.
    pub const fn new(curve: Curve, x: &'a [u8], y: Option<Ec2Y<'a>>) -> Self {
        Self {
            common: KeyCommon {
                key_id: None,
                algorithm: None,
                base_iv: None,
            },
            curve,
            x,
            y,
            d: None,
        }
    }
}

/// OKP COSE key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OkpKey<'a> {
    /// Parameters shared across all COSE key types.
    pub common: KeyCommon<'a>,
    /// Curve this key is defined over.
    pub curve: Curve,
    /// Public key bytes.
    pub x: &'a [u8],
    /// Private key, if present.
    pub d: Option<&'a [u8]>,
}

impl<'a> OkpKey<'a> {
    /// Create a new public OKP key with no [`KeyCommon`] parameters set.
    pub const fn new(curve: Curve, x: &'a [u8]) -> Self {
        Self {
            common: KeyCommon {
                key_id: None,
                algorithm: None,
                base_iv: None,
            },
            curve,
            x,
            d: None,
        }
    }
}

/// Symmetric COSE key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymmetricKey<'a> {
    /// Parameters shared across all COSE key types.
    pub common: KeyCommon<'a>,
    /// Key bytes.
    pub k: &'a [u8],
}

impl<'a> SymmetricKey<'a> {
    /// Create a new symmetric key with no [`KeyCommon`] parameters set.
    pub const fn new(k: &'a [u8]) -> Self {
        Self {
            common: KeyCommon {
                key_id: None,
                algorithm: None,
                base_iv: None,
            },
            k,
        }
    }
}

/// Typed COSE_Key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoseKey<'a> {
    /// Double-coordinate elliptic curve key.
    Ec2(Ec2Key<'a>),
    /// Octet Key Pair key.
    Okp(OkpKey<'a>),
    /// Symmetric key.
    Symmetric(SymmetricKey<'a>),
}

impl<'a> CoseKey<'a> {
    /// Encode this key into `out` and return the number of bytes written.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CoseError> {
        encode_cose_key_impl(self, out)
    }

    /// Decode and validate a COSE_Key from `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<CoseKey<'_>, CoseError> {
        decode_cose_key_impl(bytes)
    }
}

/// Decode and validate a typed COSE_Key.
///
/// This is a free-function alias for [`CoseKey::decode`].
pub fn decode_cose_key(bytes: &[u8]) -> Result<CoseKey<'_>, CoseError> {
    CoseKey::decode(bytes)
}

fn encode_cose_key_impl(key: &CoseKey<'_>, out: &mut [u8]) -> Result<usize, CoseError> {
    let mut enc = Encoder::<4>::new(out);
    match key {
        CoseKey::Ec2(key) => {
            let mut len = 3u64; // kty, crv, x
            if key.y.is_some() {
                len += 1;
            }
            if key.d.is_some() {
                len += 1;
            }
            len += key_common_len(key.common);
            let mut map = enc.map_with_len(len)?;
            map.insert_key_value(1i64, KeyType::Ec2.to_i64())?;
            encode_key_common(&mut map, key.common)?;
            map.insert_key_value(-1i64, key.curve.to_i64())?;
            map.insert_key_value(-2i64, key.x)?;
            if let Some(y) = key.y {
                match y {
                    Ec2Y::Bytes(bytes) => map.insert_key_value(-3i64, bytes)?,
                    Ec2Y::Sign(sign) => map.insert_key_value(-3i64, sign)?,
                };
            }
            if let Some(d) = key.d {
                map.insert_key_value(-4i64, d)?;
            }
            map.end()?;
        }
        CoseKey::Okp(key) => {
            let mut len = 3u64; // kty, crv, x
            if key.d.is_some() {
                len += 1;
            }
            len += key_common_len(key.common);
            let mut map = enc.map_with_len(len)?;
            map.insert_key_value(1i64, KeyType::Okp.to_i64())?;
            encode_key_common(&mut map, key.common)?;
            map.insert_key_value(-1i64, key.curve.to_i64())?;
            map.insert_key_value(-2i64, key.x)?;
            if let Some(d) = key.d {
                map.insert_key_value(-4i64, d)?;
            }
            map.end()?;
        }
        CoseKey::Symmetric(key) => {
            let len = 2u64 + key_common_len(key.common); // kty, k
            let mut map = enc.map_with_len(len)?;
            map.insert_key_value(1i64, KeyType::Symmetric.to_i64())?;
            encode_key_common(&mut map, key.common)?;
            map.insert_key_value(-1i64, key.k)?;
            map.end()?;
        }
    }
    Ok(enc.len())
}

fn key_common_len(common: KeyCommon<'_>) -> u64 {
    let mut len = 0u64;
    if common.key_id.is_some() {
        len += 1;
    }
    if common.algorithm.is_some() {
        len += 1;
    }
    if common.base_iv.is_some() {
        len += 1;
    }
    len
}

fn encode_key_common(
    map: &mut MapBuilder<'_, '_, 4>,
    common: KeyCommon<'_>,
) -> Result<(), CoseError> {
    if let Some(kid) = common.key_id {
        map.insert_key_value(2i64, kid)?;
    }
    if let Some(alg) = common.algorithm {
        map.insert_key_value(3i64, alg)?;
    }
    if let Some(base_iv) = common.base_iv {
        map.insert_key_value(5i64, base_iv)?;
    }
    Ok(())
}

fn decode_cose_key_impl(bytes: &[u8]) -> Result<CoseKey<'_>, CoseError> {
    let mut dec = Decoder::new(bytes);
    let item = dec.decode()?;
    let Item::Map(map) = item else {
        return Err(CoseError::InvalidStructure);
    };
    ensure_no_duplicate_labels(&map)?;

    let mut common = KeyCommon::default();
    let mut kty: Option<KeyType> = None;
    let mut crv: Option<Curve> = None;
    let mut x: Option<&[u8]> = None;
    let mut y: Option<Ec2Y<'_>> = None;
    let mut d: Option<&[u8]> = None;
    let mut sym_k: Option<&[u8]> = None;

    for pair in map.iter() {
        let (label_item, value) = pair?;
        let label = parse_label(label_item)?;
        match label {
            Label::Int(1) => {
                let value = parse_i64_value(value)?;
                kty = KeyType::from_i64(value);
            }
            Label::Int(2) => {
                let Item::Bytes(kid) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                common.key_id = Some(kid);
            }
            Label::Int(3) => {
                common.algorithm = Some(parse_i64_value(value)?);
            }
            Label::Int(5) => {
                let Item::Bytes(base_iv) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                common.base_iv = Some(base_iv);
            }
            Label::Int(-1) => match value {
                Item::Bytes(k) => {
                    sym_k = Some(k);
                }
                _ => {
                    crv = Some(Curve::from_i64(parse_i64_value(value)?));
                }
            },
            Label::Int(-2) => {
                let Item::Bytes(bytes) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                x = Some(bytes);
            }
            Label::Int(-3) => match value {
                Item::Bytes(bytes) => y = Some(Ec2Y::Bytes(bytes)),
                Item::Bool(sign) => y = Some(Ec2Y::Sign(sign)),
                _ => return Err(CoseError::InvalidHeaderValue),
            },
            Label::Int(-4) => {
                let Item::Bytes(bytes) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                d = Some(bytes);
            }
            Label::Int(_) => {}
            Label::Text(_) => {}
        }
    }

    let kty = kty.ok_or(CoseError::InvalidStructure)?;
    match kty {
        KeyType::Ec2 => Ok(CoseKey::Ec2(Ec2Key {
            common,
            curve: crv.ok_or(CoseError::InvalidStructure)?,
            x: x.ok_or(CoseError::InvalidStructure)?,
            y,
            d,
        })),
        KeyType::Okp => Ok(CoseKey::Okp(OkpKey {
            common,
            curve: crv.ok_or(CoseError::InvalidStructure)?,
            x: x.ok_or(CoseError::InvalidStructure)?,
            d,
        })),
        KeyType::Symmetric => Ok(CoseKey::Symmetric(SymmetricKey {
            common,
            k: sym_k.ok_or(CoseError::InvalidStructure)?,
        })),
    }
}

pub(crate) fn parse_i64_value(item: Item<'_>) -> Result<i64, CoseError> {
    match item {
        Item::Negative(v) => Ok(v),
        Item::Unsigned(v) => i64::try_from(v).map_err(|_| CoseError::InvalidHeaderValue),
        _ => Err(CoseError::InvalidHeaderValue),
    }
}

#[cfg(feature = "ring")]
impl<'a> TryFrom<&Ec2Key<'a>> for Es256PublicKey<'a> {
    type Error = CoseError;

    fn try_from(value: &Ec2Key<'a>) -> Result<Self, Self::Error> {
        if value.curve != Curve::P256 {
            return Err(CoseError::InvalidKey);
        }
        if value.x.len() != Es256PublicKey::LEN {
            return Err(CoseError::InvalidKey);
        }
        let y = match value.y {
            Some(Ec2Y::Bytes(y)) => y,
            _ => return Err(CoseError::InvalidKey),
        };
        if y.len() != Es256PublicKey::LEN {
            return Err(CoseError::InvalidKey);
        }
        Ok(Self { x: value.x, y })
    }
}

#[cfg(feature = "ring")]
impl<'a> TryFrom<&Ec2Key<'a>> for Es256PrivateKey<'a> {
    type Error = CoseError;

    fn try_from(value: &Ec2Key<'a>) -> Result<Self, Self::Error> {
        let public = Es256PublicKey::try_from(value)?;
        let d = value.d.ok_or(CoseError::InvalidKey)?;
        if d.len() != Es256PrivateKey::LEN {
            return Err(CoseError::InvalidKey);
        }
        Ok(Self {
            d,
            x: public.x,
            y: public.y,
        })
    }
}

#[cfg(feature = "ring")]
impl<'a> From<Es256PublicKey<'a>> for Ec2Key<'a> {
    fn from(value: Es256PublicKey<'a>) -> Self {
        Self {
            common: KeyCommon::default(),
            curve: Curve::P256,
            x: value.x,
            y: Some(Ec2Y::Bytes(value.y)),
            d: None,
        }
    }
}

#[cfg(feature = "ring")]
impl<'a> From<Es256PrivateKey<'a>> for Ec2Key<'a> {
    fn from(value: Es256PrivateKey<'a>) -> Self {
        Self {
            common: KeyCommon::default(),
            curve: Curve::P256,
            x: value.x,
            y: Some(Ec2Y::Bytes(value.y)),
            d: Some(value.d),
        }
    }
}

#[cfg(feature = "ring")]
impl<'a> TryFrom<&OkpKey<'a>> for EdDsaPublicKey<'a> {
    type Error = CoseError;

    fn try_from(value: &OkpKey<'a>) -> Result<Self, Self::Error> {
        if value.curve != Curve::Ed25519 {
            return Err(CoseError::InvalidKey);
        }
        if value.x.len() != EdDsaPublicKey::LEN {
            return Err(CoseError::InvalidKey);
        }
        Ok(Self { x: value.x })
    }
}

#[cfg(feature = "ring")]
impl<'a> TryFrom<&OkpKey<'a>> for EdDsaPrivateKey<'a> {
    type Error = CoseError;

    fn try_from(value: &OkpKey<'a>) -> Result<Self, Self::Error> {
        let public = EdDsaPublicKey::try_from(value)?;
        let d = value.d.ok_or(CoseError::InvalidKey)?;
        if d.len() != EdDsaPrivateKey::LEN {
            return Err(CoseError::InvalidKey);
        }
        Ok(Self { d, x: public.x })
    }
}

#[cfg(feature = "ring")]
impl<'a> From<EdDsaPublicKey<'a>> for OkpKey<'a> {
    fn from(value: EdDsaPublicKey<'a>) -> Self {
        Self {
            common: KeyCommon::default(),
            curve: Curve::Ed25519,
            x: value.x,
            d: None,
        }
    }
}

#[cfg(feature = "ring")]
impl<'a> From<EdDsaPrivateKey<'a>> for OkpKey<'a> {
    fn from(value: EdDsaPrivateKey<'a>) -> Self {
        Self {
            common: KeyCommon::default(),
            curve: Curve::Ed25519,
            x: value.x,
            d: Some(value.d),
        }
    }
}
