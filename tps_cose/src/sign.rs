/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use core::marker::PhantomData;

use tps_minicbor::{Array, ArrayIter, Decoder, Encoder, Item};

use crate::header::{
    HeaderParam, ParsedHeaders, Payload, encode_header_map, ensure_no_cross_duplicates,
    ensure_no_duplicate_labels, parse_headers, protected_bstr_for_structure,
    validate_builder_headers,
};
use crate::{Algorithm, CoseError, Signer, Verifier};

#[cfg(feature = "ring")]
use crate::crypto::ring::{EdDsaPublicKey, EdDsaVerifier, Es256PublicKey, Es256Verifier};

/// Typestate marker: signer list not set.
pub struct MissingSigners;
/// Typestate marker: signer list has been set.
pub struct WithSigners;

/// Describes one signer for a COSE_Sign message.
pub struct SignerEntry<'a> {
    /// Per-signer protected header parameters.
    pub protected: &'a [HeaderParam<'a>],
    /// Per-signer unprotected header parameters.
    pub unprotected: &'a [HeaderParam<'a>],
    /// The signing backend.
    pub signer: &'a dyn Signer,
    /// The algorithm this signer uses.
    pub algorithm: Algorithm,
    algorithm_in_protected: bool,
}

impl<'a> SignerEntry<'a> {
    /// Create a new signer entry with the given protected/unprotected headers, signer, and algorithm.
    pub const fn new(
        protected: &'a [HeaderParam<'a>],
        unprotected: &'a [HeaderParam<'a>],
        signer: &'a dyn Signer,
        algorithm: Algorithm,
    ) -> Self {
        Self {
            protected,
            unprotected,
            signer,
            algorithm,
            algorithm_in_protected: true,
        }
    }

    /// Move the algorithm label to the unprotected header for this signer entry.
    pub const fn algorithm_in_unprotected(mut self) -> Self {
        self.algorithm_in_protected = false;
        self
    }

    fn encode_protected(&self, out: &mut [u8]) -> Result<usize, CoseError> {
        validate_builder_headers(self.protected, self.unprotected)?;
        let mut enc = Encoder::<4>::new(out);
        let extra = usize::from(self.algorithm_in_protected);
        let mut map = enc.map_with_len((self.protected.len() + extra) as u64)?;
        if self.algorithm_in_protected {
            map.insert_key_value(1i64, self.algorithm.to_i64())?;
        }
        encode_header_map(&mut map, self.protected)?;
        map.end()?;
        Ok(enc.len())
    }
}

// Re-export state markers used by SignBuilder from sign1
use crate::sign1::{MissingPayload, WithPayload};

/// Typestate COSE_Sign builder.
pub struct SignBuilder<'a, P = MissingPayload, S = MissingSigners>(
    SignBuilderData<'a>,
    PhantomData<(P, S)>,
);

struct SignBuilderData<'a> {
    protected: &'a [HeaderParam<'a>],
    unprotected: &'a [HeaderParam<'a>],
    external_aad: &'a [u8],
    payload: Option<Payload<'a>>,
    signers: Option<&'a [SignerEntry<'a>]>,
}

impl<'a> SignBuilder<'a, MissingPayload, MissingSigners> {
    /// Create a new [`SignBuilder`] with the given protected and unprotected header parameters.
    pub const fn new(protected: &'a [HeaderParam<'a>], unprotected: &'a [HeaderParam<'a>]) -> Self {
        Self(
            SignBuilderData {
                protected,
                unprotected,
                external_aad: &[],
                payload: None,
                signers: None,
            },
            PhantomData,
        )
    }
}

impl<'a, P, S> SignBuilder<'a, P, S> {
    /// Set the external AAD to include in each Sig_structure.
    pub const fn external_aad(mut self, external_aad: &'a [u8]) -> Self {
        self.0.external_aad = external_aad;
        self
    }

    /// Set the payload (attached or detached).
    pub fn payload(self, payload: Payload<'a>) -> SignBuilder<'a, WithPayload, S> {
        SignBuilder(
            SignBuilderData {
                payload: Some(payload),
                ..self.0
            },
            PhantomData,
        )
    }

    /// Set the signer entries list.
    pub fn signers(self, signers: &'a [SignerEntry<'a>]) -> SignBuilder<'a, P, WithSigners> {
        SignBuilder(
            SignBuilderData {
                signers: Some(signers),
                ..self.0
            },
            PhantomData,
        )
    }
}

impl<'a, S> SignBuilder<'a, WithPayload, S> {
    /// Serialize the body protected header map into `out`, returning the byte count.
    #[must_use = "returns the encoded byte count"]
    pub fn encode_protected(&self, out: &mut [u8]) -> Result<usize, CoseError> {
        validate_builder_headers(self.0.protected, self.0.unprotected)?;
        let mut enc = Encoder::<4>::new(out);
        let mut map = enc.map_with_len(self.0.protected.len() as u64)?;
        encode_header_map(&mut map, self.0.protected)?;
        map.end()?;
        Ok(enc.len())
    }
}

impl<'a> SignBuilder<'a, WithPayload, WithSigners> {
    /// Sign and encode the complete COSE_Sign message with all signer entries.
    #[must_use = "returns the encoded byte count"]
    pub fn sign(
        &self,
        body_protected_buf: &mut [u8],
        signer_protected_buf: &mut [u8],
        sig_structure_buf: &mut [u8],
        signature_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let signers = self.0.signers.ok_or(CoseError::InvalidStructure)?;
        if signers.is_empty() {
            return Err(CoseError::InvalidStructure);
        }
        let payload_mode = self.0.payload.ok_or(CoseError::MissingPayload)?;
        let to_be_signed_payload = match payload_mode {
            Payload::Attached(payload) => payload,
            Payload::Detached(payload) => payload,
        };
        let body_protected_len = self.encode_protected(body_protected_buf)?;
        let body_protected_bytes = &body_protected_buf[..body_protected_len];
        let body_sig_protected = protected_bstr_for_structure(body_protected_bytes);

        let mut enc = Encoder::<4>::new(out);
        let mut top = enc.array_with_len(4)?;
        top.insert(body_protected_bytes)?;
        {
            let mut map = top.map_len(self.0.unprotected.len() as u64)?;
            encode_header_map(&mut map, self.0.unprotected)?;
            map.end()?;
        }
        match payload_mode {
            Payload::Attached(payload) => top.insert(payload)?,
            Payload::Detached(_) => top.insert(())?,
        };
        {
            let mut signatures_array = top.array_len(signers.len() as u64)?;
            for signer in signers {
                if signer.signer.algorithm() != signer.algorithm {
                    return Err(CoseError::AlgorithmMismatch);
                }
                let signer_protected_len = signer.encode_protected(signer_protected_buf)?;
                let signer_protected_bytes = &signer_protected_buf[..signer_protected_len];
                let signer_sig_protected = protected_bstr_for_structure(signer_protected_bytes);
                let sig_structure_len = encode_sig_structure_sign(
                    body_sig_protected,
                    signer_sig_protected,
                    self.0.external_aad,
                    to_be_signed_payload,
                    sig_structure_buf,
                )?;
                let signature_len = signer
                    .signer
                    .sign(&sig_structure_buf[..sig_structure_len], signature_buf)?;
                let mut signature_item = signatures_array.array_len(3)?;
                signature_item.insert(signer_protected_bytes)?;
                {
                    let extra = usize::from(!signer.algorithm_in_protected);
                    let mut map =
                        signature_item.map_len((signer.unprotected.len() + extra) as u64)?;
                    if !signer.algorithm_in_protected {
                        map.insert_key_value(1i64, signer.algorithm.to_i64())?;
                    }
                    encode_header_map(&mut map, signer.unprotected)?;
                    map.end()?;
                }
                signature_item.insert(&signature_buf[..signature_len])?;
                signature_item.end()?;
            }
            signatures_array.end()?;
        }
        top.end()?;
        Ok(enc.len())
    }
}

/// Decode-time view of a single COSE_Signature entry within a COSE_Sign message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignSignature<'a> {
    /// Serialized protected header bytes for this signature entry.
    pub protected_serialized: &'a [u8],
    /// Parsed protected header parameters.
    pub protected: ParsedHeaders<'a>,
    /// Parsed unprotected header parameters.
    pub unprotected: ParsedHeaders<'a>,
    /// Signature bytes.
    pub signature: &'a [u8],
}

/// Decoded COSE_Sign message with parsed body and signature list.
#[derive(Debug, Clone, PartialEq)]
pub struct Sign<'a> {
    /// Serialized protected header bytes of the body.
    pub protected_serialized: &'a [u8],
    /// Parsed body protected header parameters.
    pub protected: ParsedHeaders<'a>,
    /// Parsed body unprotected header parameters.
    pub unprotected: ParsedHeaders<'a>,
    /// Attached payload bytes, or `None` for a detached payload.
    pub payload: Option<&'a [u8]>,
    signatures: Array<'a>,
}

impl<'a> Sign<'a> {
    /// Iterate over the decoded signature entries.
    #[must_use]
    pub fn signatures(&self) -> SignSignatureIter<'a> {
        SignSignatureIter {
            inner: self.signatures.iter(),
        }
    }

    /// Return the number of signature entries.
    #[must_use]
    pub fn signature_count(&self) -> usize {
        self.signatures.len() as usize
    }
}

/// Iterator over [`SignSignature`] entries in a decoded COSE_Sign.
pub struct SignSignatureIter<'a> {
    inner: ArrayIter<'a>,
}

impl<'a> Iterator for SignSignatureIter<'a> {
    type Item = Result<SignSignature<'a>, CoseError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()? {
            Ok(item) => Some(parse_sign_signature_item(item)),
            Err(_) => Some(Err(CoseError::Decode)),
        }
    }
}

/// Encode the COSE Sig_structure for a COSE_Sign signature entry.
///
/// The Sig_structure carries the context string `"Signature"`, the serialized body
/// protected header (`body_protected`), the serialized per-signer protected header
/// (`signer_protected`), any external AAD, and the payload. Pass an empty slice for
/// `external_aad` when there is no external AAD.
///
/// Returns the number of bytes written to `out`.
///
/// # Errors
///
/// Returns [`CoseError::Encode`] if `out` is too small.
#[must_use = "returns the encoded byte count"]
pub fn encode_sig_structure_sign(
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

/// Decode and validate a COSE_Sign message.
///
/// Parses `bytes` as a COSE_Sign structure (RFC 9052 §4.1), validates header
/// semantics (no duplicate labels, algorithm present in each signature entry,
/// and unknown critical headers rejected), and returns a zero-copy [`Sign`]
/// view into `bytes`.
///
/// # Errors
///
/// Returns [`CoseError::InvalidStructure`] if the outer CBOR layout is wrong,
/// [`CoseError::DuplicateHeaderLabel`] if a label appears more than once,
/// [`CoseError::MissingAlgorithm`] if any signature entry lacks an algorithm,
/// or [`CoseError::Decode`] for any other CBOR parse failure.
#[must_use = "returns the decoded Sign message"]
pub fn decode_sign(bytes: &[u8]) -> Result<Sign<'_>, CoseError> {
    let mut dec = Decoder::new(bytes);
    let item = dec.decode()?;
    let Item::Array(array) = item else {
        return Err(CoseError::InvalidStructure);
    };
    if array.len() != 4 {
        return Err(CoseError::InvalidStructure);
    }
    let mut iter = array.iter();
    let protected_serialized = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => bytes,
        _ => return Err(CoseError::InvalidStructure),
    };
    let unprotected_map = match iter.next().transpose()? {
        Some(Item::Map(map)) => map,
        _ => return Err(CoseError::InvalidStructure),
    };
    let payload = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => Some(bytes),
        Some(Item::Null) => None,
        _ => return Err(CoseError::InvalidStructure),
    };
    let signatures = match iter.next().transpose()? {
        Some(Item::Array(array)) => array,
        _ => return Err(CoseError::InvalidStructure),
    };
    if signatures.is_empty() {
        return Err(CoseError::InvalidStructure);
    }

    let protected = parse_protected_headers_allow_empty(protected_serialized)?;
    ensure_no_duplicate_labels(&unprotected_map)?;
    if !protected_serialized.is_empty() {
        let mut protected_dec = Decoder::new(protected_serialized);
        let protected_map = protected_dec
            .next_map()
            .map_err(|_| CoseError::InvalidStructure)?;
        ensure_no_cross_duplicates(&protected_map, &unprotected_map)?;
    }
    let unprotected = parse_headers(&unprotected_map, false)?;

    for entry in signatures.iter() {
        let item = entry?;
        let parsed = parse_sign_signature_item(item)?;
        if parsed
            .protected
            .algorithm
            .or(parsed.unprotected.algorithm)
            .is_none()
        {
            return Err(CoseError::MissingAlgorithm);
        }
    }

    Ok(Sign {
        protected_serialized,
        protected,
        unprotected,
        payload,
        signatures,
    })
}

fn parse_sign_signature_item(item: Item<'_>) -> Result<SignSignature<'_>, CoseError> {
    let Item::Array(array) = item else {
        return Err(CoseError::InvalidStructure);
    };
    if array.len() != 3 {
        return Err(CoseError::InvalidStructure);
    }
    let mut iter = array.iter();
    let protected_serialized = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => bytes,
        _ => return Err(CoseError::InvalidStructure),
    };
    let unprotected_map = match iter.next().transpose()? {
        Some(Item::Map(map)) => map,
        _ => return Err(CoseError::InvalidStructure),
    };
    let signature = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => bytes,
        _ => return Err(CoseError::InvalidStructure),
    };
    let protected = parse_protected_headers_allow_empty(protected_serialized)?;
    ensure_no_duplicate_labels(&unprotected_map)?;
    if !protected_serialized.is_empty() {
        let mut protected_dec = Decoder::new(protected_serialized);
        let protected_map = protected_dec
            .next_map()
            .map_err(|_| CoseError::InvalidStructure)?;
        ensure_no_cross_duplicates(&protected_map, &unprotected_map)?;
    }
    let unprotected = parse_headers(&unprotected_map, false)?;

    Ok(SignSignature {
        protected_serialized,
        protected,
        unprotected,
        signature,
    })
}

fn parse_protected_headers_allow_empty(
    protected_serialized: &[u8],
) -> Result<ParsedHeaders<'_>, CoseError> {
    if protected_serialized.is_empty() {
        return Ok(ParsedHeaders::default());
    }
    let mut dec = Decoder::new(protected_serialized);
    let map = dec.next_map().map_err(|_| CoseError::InvalidStructure)?;
    if dec.pos() != protected_serialized.len() {
        return Err(CoseError::InvalidStructure);
    }
    ensure_no_duplicate_labels(&map)?;
    parse_headers(&map, true)
}

/// Verify one signature entry from a decoded COSE_Sign.
///
/// Constructs the Sig_structure from the body and per-signer protected headers, then
/// asks `verifier` to check `signature.signature` against that structure. The algorithm
/// recorded in `signature` must match `verifier.algorithm()`; a mismatch returns
/// [`CoseError::AlgorithmMismatch`].
///
/// If `sign` carries an attached payload, `detached_payload` is ignored. For a
/// detached payload, pass the raw payload bytes as `detached_payload`.
///
/// Pass an empty slice for `external_aad` when there is no external AAD.
///
/// # Errors
///
/// Returns [`CoseError::AlgorithmMismatch`] when the verifier algorithm does not
/// match the signature entry algorithm, [`CoseError::MissingPayload`] when neither
/// an attached nor a detached payload is available, or [`CoseError::InvalidSignature`]
/// if verification fails.
#[must_use = "returns an error if verification fails"]
pub fn verify_sign<V: Verifier>(
    sign: &Sign<'_>,
    signature: &SignSignature<'_>,
    verifier: &V,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let algorithm = signature
        .protected
        .algorithm
        .or(signature.unprotected.algorithm)
        .ok_or(CoseError::MissingAlgorithm)?;
    if algorithm != verifier.algorithm() {
        return Err(CoseError::AlgorithmMismatch);
    }
    let payload = match (sign.payload, detached_payload) {
        (Some(payload), _) => payload,
        (None, Some(payload)) => payload,
        (None, None) => return Err(CoseError::MissingPayload),
    };
    let body_protected = protected_bstr_for_structure(sign.protected_serialized);
    let signer_protected = protected_bstr_for_structure(signature.protected_serialized);
    let sig_len = encode_sig_structure_sign(
        body_protected,
        signer_protected,
        external_aad,
        payload,
        sig_structure_buf,
    )?;
    verifier.verify(&sig_structure_buf[..sig_len], signature.signature)
}

/// Verify a COSE_Sign signature entry with an ES256 public key (requires `ring` feature).
///
/// Convenience wrapper around [`verify_sign`] using [`Es256Verifier`].
///
/// # Errors
///
/// Propagates all errors from [`verify_sign`].
#[cfg(feature = "ring")]
#[must_use = "returns an error if verification fails"]
pub fn verify_sign_es256(
    sign: &Sign<'_>,
    signature: &SignSignature<'_>,
    key: &Es256PublicKey<'_>,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let verifier = Es256Verifier::new(key);
    verify_sign(
        sign,
        signature,
        &verifier,
        external_aad,
        detached_payload,
        sig_structure_buf,
    )
}

/// Verify a COSE_Sign signature entry with an EdDSA public key (requires `ring` feature).
///
/// Convenience wrapper around [`verify_sign`] using [`EdDsaVerifier`].
///
/// # Errors
///
/// Propagates all errors from [`verify_sign`].
#[cfg(feature = "ring")]
#[must_use = "returns an error if verification fails"]
pub fn verify_sign_eddsa(
    sign: &Sign<'_>,
    signature: &SignSignature<'_>,
    key: &EdDsaPublicKey<'_>,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let verifier = EdDsaVerifier::new(key);
    verify_sign(
        sign,
        signature,
        &verifier,
        external_aad,
        detached_payload,
        sig_structure_buf,
    )
}
