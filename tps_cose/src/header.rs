/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_minicbor::{Item, Map, MapBuilder, MapValueBuilder, Simple, Undefined};

use crate::{Algorithm, CoseError, EncryptAlgorithm};

/// Cap on the number of entries a single protected/unprotected header map
/// may have, checked before [`ensure_no_duplicate_labels`]/
/// [`ensure_no_cross_duplicates`]'s pairwise scans run.
///
/// Both functions are O(n^2) in the number of map entries (a full nested
/// `map.iter() x map.iter()` comparison, re-decoding every value on every
/// pairwise check). CBOR map lengths are attacker-controlled and otherwise
/// unbounded, so a header map with tens of thousands of entries is a
/// CPU-exhaustion DoS via this quadratic cost alone, independent of any
/// nesting inside individual values. `Map::len()` is O(1) (a length
/// recorded at decode time), so this check is cheap regardless of map
/// size. No real COSE header map needs anywhere near this many distinct
/// labels (the whole IANA COSE Header Parameters registry is a few dozen
/// entries), so this cap is far above legitimate use while keeping
/// worst-case scan cost bounded.
const MAX_HEADER_ENTRIES: u64 = 256;

/// A COSE header map label (integer or text string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label<'a> {
    /// Integer label.
    Int(i64),
    /// Text label.
    Text(&'a str),
}

/// COSE header content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType<'a> {
    /// MIME type string.
    Text(&'a str),
    /// CoAP Content-Format number.
    Uint(u64),
}

/// Header values for unknown parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderValue<'a> {
    /// Unsigned integer value.
    Unsigned(u64),
    /// Negative integer value.
    Negative(i64),
    /// Byte string value.
    Bytes(&'a [u8]),
    /// Text string value.
    Text(&'a str),
    /// Boolean value.
    Bool(bool),
    /// CBOR null value.
    Null,
    /// CBOR undefined value.
    Undefined,
    /// CBOR simple value.
    Simple(u8),
    /// Floating-point value (requires feature `float`).
    #[cfg(feature = "float")]
    Float(f64),
}

/// Additional COSE header parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderParam<'a> {
    /// Key identifier (label 4).
    KeyId(&'a [u8]),
    /// Initialization vector (label 5).
    Iv(&'a [u8]),
    /// Partial initialization vector (label 6).
    PartialIv(&'a [u8]),
    /// Content type (label 3).
    ContentType(ContentType<'a>),
    /// Critical header labels (label 2).
    Critical(&'a [Label<'a>]),
    /// An unrecognized header parameter.
    Unknown(Label<'a>, HeaderValue<'a>),
}

/// Payload treatment for COSE_Sign1 messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload<'a> {
    /// Carry payload in the COSE_Sign1 payload field.
    Attached(&'a [u8]),
    /// Detach payload from message and sign provided detached bytes.
    Detached(&'a [u8]),
}

/// Parsed and validated header view.
///
/// Used for all COSE message types. For `COSE_Sign1` and `COSE_Sign` messages
/// the `algorithm` field is populated; for `COSE_Encrypt0` messages
/// `encrypt_algorithm` is populated instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParsedHeaders<'a> {
    /// Signing algorithm from the algorithm header parameter (label 1).
    pub algorithm: Option<Algorithm>,
    /// Encryption algorithm from the algorithm header parameter (label 1).
    pub encrypt_algorithm: Option<EncryptAlgorithm>,
    /// Key identifier from header parameter label 4.
    pub key_id: Option<&'a [u8]>,
    /// Initialization vector from header parameter label 5.
    pub iv: Option<&'a [u8]>,
    /// Partial initialization vector from header parameter label 6.
    pub partial_iv: Option<&'a [u8]>,
    /// Content type from header parameter label 3.
    pub content_type: Option<ContentType<'a>>,
}

pub(crate) fn parse_headers<'a>(
    map: &Map<'a>,
    is_protected: bool,
) -> Result<ParsedHeaders<'a>, CoseError> {
    let mut out = ParsedHeaders::default();
    for pair in map.iter() {
        let (key, value) = pair?;
        let label = parse_label(key)?;
        match label {
            Label::Int(1) => out.algorithm = Some(parse_algorithm(value)?),
            Label::Int(2) => {
                if !is_protected {
                    return Err(CoseError::InvalidHeader);
                }
                parse_crit(value, map)?;
            }
            Label::Int(3) => out.content_type = Some(parse_content_type(value)?),
            Label::Int(4) => {
                let Item::Bytes(kid) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.key_id = Some(kid);
            }
            Label::Int(5) => {
                let Item::Bytes(iv) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.iv = Some(iv);
            }
            Label::Int(6) => {
                let Item::Bytes(iv) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.partial_iv = Some(iv);
            }
            Label::Int(_) => {}
            Label::Text(_) => {}
        }
    }
    Ok(out)
}

pub(crate) fn parse_encrypt_headers<'a>(
    map: &Map<'a>,
    is_protected: bool,
) -> Result<ParsedHeaders<'a>, CoseError> {
    let mut out = ParsedHeaders::default();
    for pair in map.iter() {
        let (label_item, value) = pair?;
        let label = parse_label(label_item)?;
        match label {
            Label::Int(1) => {
                let raw = parse_i64_value(value)?;
                out.encrypt_algorithm = Some(
                    EncryptAlgorithm::try_from_i64(raw).ok_or(CoseError::UnsupportedAlgorithm)?,
                );
            }
            Label::Int(2) => {
                if !is_protected {
                    return Err(CoseError::InvalidHeader);
                }
                parse_crit(value, map)?;
            }
            Label::Int(3) => out.content_type = Some(parse_content_type(value)?),
            Label::Int(4) => {
                let Item::Bytes(kid) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.key_id = Some(kid);
            }
            Label::Int(5) => {
                let Item::Bytes(iv) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.iv = Some(iv);
            }
            Label::Int(6) => {
                let Item::Bytes(partial_iv) = value else {
                    return Err(CoseError::InvalidHeaderValue);
                };
                out.partial_iv = Some(partial_iv);
            }
            Label::Int(_) => {}
            Label::Text(_) => {}
        }
    }
    Ok(out)
}

pub(crate) fn parse_algorithm(item: Item<'_>) -> Result<Algorithm, CoseError> {
    let alg = match item {
        Item::Negative(v) => v,
        Item::Unsigned(v) => i64::try_from(v).map_err(|_| CoseError::UnsupportedAlgorithm)?,
        _ => return Err(CoseError::InvalidHeaderValue),
    };
    Algorithm::try_from_i64(alg).ok_or(CoseError::UnsupportedAlgorithm)
}

pub(crate) fn parse_content_type<'a>(item: Item<'a>) -> Result<ContentType<'a>, CoseError> {
    match item {
        Item::Text(v) => Ok(ContentType::Text(v)),
        Item::Unsigned(v) => Ok(ContentType::Uint(v)),
        _ => Err(CoseError::InvalidHeaderValue),
    }
}

/// Validate a decoded `crit` (label 2) array: every listed label must be understood by this
/// crate (see [`is_understood_label`]) and must actually be present in `protected_map` — RFC
/// 9052 §3.1 requires both, since a critical label the receiver doesn't recognize, or that
/// isn't actually there, means the message can't be safely processed.
pub(crate) fn parse_crit(item: Item<'_>, protected_map: &Map<'_>) -> Result<(), CoseError> {
    let Item::Array(array) = item else {
        return Err(CoseError::InvalidHeaderValue);
    };
    if array.is_empty() {
        return Err(CoseError::InvalidHeaderValue);
    }
    for entry in array.iter() {
        let entry = entry?;
        let label = parse_label(entry)?;
        if !is_understood_label(label) {
            return Err(CoseError::UnknownCriticalHeader);
        }
        if !map_contains_label(protected_map, label)? {
            return Err(CoseError::UnknownCriticalHeader);
        }
    }
    Ok(())
}

fn map_contains_label(map: &Map<'_>, label: Label<'_>) -> Result<bool, CoseError> {
    for pair in map.iter() {
        let (key, _) = pair?;
        if parse_label(key)? == label {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn ensure_no_duplicate_labels(map: &Map<'_>) -> Result<(), CoseError> {
    if map.len() > MAX_HEADER_ENTRIES {
        return Err(CoseError::TooManyHeaderParameters);
    }
    for (i, lhs) in map.iter().enumerate() {
        let (lhs_key, _) = lhs?;
        let lhs_label = parse_label(lhs_key)?;
        for (j, rhs) in map.iter().enumerate() {
            let (rhs_key, _) = rhs?;
            if j > i {
                let rhs_label = parse_label(rhs_key)?;
                if lhs_label == rhs_label {
                    return Err(CoseError::DuplicateHeaderLabel);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn ensure_no_cross_duplicates(
    protected: &Map<'_>,
    unprotected: &Map<'_>,
) -> Result<(), CoseError> {
    if protected.len() > MAX_HEADER_ENTRIES || unprotected.len() > MAX_HEADER_ENTRIES {
        return Err(CoseError::TooManyHeaderParameters);
    }
    for lhs in protected.iter() {
        let (lhs_key, _) = lhs?;
        let lhs_label = parse_label(lhs_key)?;
        for rhs in unprotected.iter() {
            let (rhs_key, _) = rhs?;
            let rhs_label = parse_label(rhs_key)?;
            if lhs_label == rhs_label {
                return Err(CoseError::DuplicateHeaderLabel);
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_builder_headers(
    protected: &[HeaderParam<'_>],
    unprotected: &[HeaderParam<'_>],
) -> Result<(), CoseError> {
    for i in 0..protected.len() {
        for j in (i + 1)..protected.len() {
            if header_param_label(protected[i]) == header_param_label(protected[j]) {
                return Err(CoseError::DuplicateHeaderLabel);
            }
        }
    }
    for i in 0..unprotected.len() {
        for j in (i + 1)..unprotected.len() {
            if header_param_label(unprotected[i]) == header_param_label(unprotected[j]) {
                return Err(CoseError::DuplicateHeaderLabel);
            }
        }
    }
    for p in protected {
        for u in unprotected {
            if header_param_label(*p) == header_param_label(*u) {
                return Err(CoseError::DuplicateHeaderLabel);
            }
        }
    }
    for param in unprotected {
        if matches!(param, HeaderParam::Critical(_)) {
            return Err(CoseError::InvalidHeader);
        }
    }
    for param in protected {
        validate_param_critical(param)?;
    }
    for param in unprotected {
        validate_param_critical(param)?;
    }
    Ok(())
}

/// Validate that a builder-supplied header parameter's `crit` (label 2) array, if any, only
/// lists understood labels — see [`is_understood_label`].
pub(crate) fn validate_param_critical(param: &HeaderParam<'_>) -> Result<(), CoseError> {
    if let HeaderParam::Critical(values) = param {
        for value in *values {
            if !is_understood_label(*value) {
                return Err(CoseError::UnknownCriticalHeader);
            }
        }
    }
    Ok(())
}

pub(crate) fn header_param_label(param: HeaderParam<'_>) -> Label<'_> {
    match param {
        HeaderParam::KeyId(_) => Label::Int(4),
        HeaderParam::Iv(_) => Label::Int(5),
        HeaderParam::PartialIv(_) => Label::Int(6),
        HeaderParam::ContentType(_) => Label::Int(3),
        HeaderParam::Critical(_) => Label::Int(2),
        HeaderParam::Unknown(label, _) => label,
    }
}

pub(crate) fn encode_header_map<'a>(
    map: &mut MapBuilder<'_, '_, 4>,
    params: &[HeaderParam<'a>],
) -> Result<(), CoseError> {
    for param in params {
        match *param {
            HeaderParam::KeyId(kid) => {
                map.insert_key_value(4i64, kid)?;
            }
            HeaderParam::Iv(iv) => {
                map.insert_key_value(5i64, iv)?;
            }
            HeaderParam::PartialIv(iv) => {
                map.insert_key_value(6i64, iv)?;
            }
            HeaderParam::ContentType(content_type) => match content_type {
                ContentType::Text(value) => {
                    map.insert_key_value(3i64, value)?;
                }
                ContentType::Uint(value) => {
                    map.insert_key_value(3i64, value)?;
                }
            },
            HeaderParam::Critical(values) => {
                let mut array = map.key(2i64)?.array_len(values.len() as u64)?;
                for value in values {
                    match *value {
                        Label::Int(v) => {
                            array.insert(v)?;
                        }
                        Label::Text(v) => {
                            array.insert(v)?;
                        }
                    }
                }
                array.end()?;
            }
            HeaderParam::Unknown(label, value) => {
                let value_builder = match label {
                    Label::Int(v) => map.key(v)?,
                    Label::Text(v) => map.key(v)?,
                };
                encode_header_value(value_builder, value)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn encode_header_value<'a>(
    value_builder: MapValueBuilder<'_, '_, 4>,
    value: HeaderValue<'a>,
) -> Result<(), CoseError> {
    match value {
        HeaderValue::Unsigned(v) => value_builder.insert(v)?,
        HeaderValue::Negative(v) => value_builder.insert(v)?,
        HeaderValue::Bytes(v) => value_builder.insert(v)?,
        HeaderValue::Text(v) => value_builder.insert(v)?,
        HeaderValue::Bool(v) => value_builder.insert(v)?,
        HeaderValue::Null => value_builder.insert(())?,
        HeaderValue::Undefined => value_builder.insert(Undefined)?,
        HeaderValue::Simple(v) => value_builder.insert(Simple::new(v)?)?,
        #[cfg(feature = "float")]
        HeaderValue::Float(v) => value_builder.insert(v)?,
    }
    Ok(())
}

pub(crate) fn parse_label<'a>(item: Item<'a>) -> Result<Label<'a>, CoseError> {
    match item {
        Item::Unsigned(v) => Ok(Label::Int(
            i64::try_from(v).map_err(|_| CoseError::UnsupportedLabelType)?,
        )),
        Item::Negative(v) => Ok(Label::Int(v)),
        Item::Text(v) => Ok(Label::Text(v)),
        _ => Err(CoseError::UnsupportedLabelType),
    }
}

/// Whether this crate understands the given header label well enough to validate a `crit`
/// (critical) entry for it. Covers labels 1-7 (`alg`, `crit`, `content type`, `kid`, `IV`,
/// `Partial IV`, `counter signature` — RFC 9052 §3.1); do not narrow this to `1..=6`, label 7
/// (counter signature) is understood too even though this crate doesn't implement it itself.
pub(crate) fn is_understood_label(label: Label<'_>) -> bool {
    match label {
        Label::Int(v) => (1..=7).contains(&v),
        Label::Text(_) => false,
    }
}

pub(crate) fn protected_bstr_for_structure(protected: &[u8]) -> &[u8] {
    if protected.len() == 1 && protected[0] == 0xa0 {
        &[]
    } else {
        protected
    }
}

pub(crate) fn parse_i64_value(item: Item<'_>) -> Result<i64, CoseError> {
    match item {
        Item::Negative(v) => Ok(v),
        Item::Unsigned(v) => i64::try_from(v).map_err(|_| CoseError::InvalidHeaderValue),
        _ => Err(CoseError::InvalidHeaderValue),
    }
}
