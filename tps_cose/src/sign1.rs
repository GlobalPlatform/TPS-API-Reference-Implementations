/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use core::marker::PhantomData;

use tps_minicbor::{Decoder, Encoder, Item};

use crate::header::{
    HeaderParam, ParsedHeaders, Payload, encode_header_map, ensure_no_cross_duplicates,
    ensure_no_duplicate_labels, parse_headers, protected_bstr_for_structure,
    validate_builder_headers,
};
use crate::{Algorithm, CoseError, Signer, Verifier};

#[cfg(feature = "ring")]
use crate::crypto::ring::{
    EdDsaPrivateKey, EdDsaPublicKey, EdDsaSigner, EdDsaVerifier, Es256PrivateKey, Es256PublicKey,
    Es256Signer, Es256Verifier,
};

/// Decoded and validated COSE_Sign1 message.
///
/// Obtained by calling [`decode_sign1`]. Fields carry zero-copy borrows
/// into the original byte slice.
///
/// # Examples
///
/// ```rust
/// use tps_cose::{Algorithm, Payload, Sign1Builder, decode_sign1};
///
/// let builder = Sign1Builder::new(&[], &[])
///     .algorithm(Algorithm::Es256)
///     .payload(Payload::Attached(b"hello"));
///
/// let mut protected_buf = [0u8; 32];
/// let mut out = [0u8; 128];
/// let signature = [0u8; 64]; // a real signature would come from a Signer
/// let len = builder.encode(&mut out, &mut protected_buf, &signature).unwrap();
///
/// let sign1 = decode_sign1(&out[..len]).unwrap();
/// assert_eq!(sign1.payload, Some(&b"hello"[..]));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Sign1<'a> {
    /// Serialized protected header bytes.
    pub protected_serialized: &'a [u8],
    /// Parsed protected header parameters.
    pub protected: ParsedHeaders<'a>,
    /// Parsed unprotected header parameters.
    pub unprotected: ParsedHeaders<'a>,
    /// Attached payload bytes, or `None` for a detached payload.
    pub payload: Option<&'a [u8]>,
    /// Signature bytes.
    pub signature: &'a [u8],
}

impl<'a> Sign1<'a> {
    /// Decode the protected header as a raw CBOR map.
    #[must_use = "returns the decoded protected header map"]
    pub fn protected_map(&self) -> Result<tps_minicbor::Map<'a>, CoseError> {
        let mut dec = Decoder::new(self.protected_serialized);
        dec.next_map().map_err(|_| CoseError::Decode)
    }
}

/// Typestate marker: algorithm not yet set.
pub struct MissingAlgorithm;
/// Typestate marker: algorithm has been set.
pub struct WithAlgorithm;
/// Typestate marker: payload not yet set.
pub struct MissingPayload;
/// Typestate marker: payload has been set.
pub struct WithPayload;

/// Typestate COSE_Sign1 builder.
pub struct Sign1Builder<'a, A = MissingAlgorithm, P = MissingPayload>(
    Sign1BuilderData<'a>,
    PhantomData<(A, P)>,
);

pub(crate) struct Sign1BuilderData<'a> {
    pub(crate) protected: &'a [HeaderParam<'a>],
    pub(crate) unprotected: &'a [HeaderParam<'a>],
    pub(crate) external_aad: &'a [u8],
    pub(crate) algorithm: Option<Algorithm>,
    pub(crate) algorithm_in_protected: bool,
    pub(crate) payload: Option<Payload<'a>>,
}

impl<'a> Sign1Builder<'a, MissingAlgorithm, MissingPayload> {
    /// Create a new builder with the given protected and unprotected header parameters.
    pub const fn new(protected: &'a [HeaderParam<'a>], unprotected: &'a [HeaderParam<'a>]) -> Self {
        Self(
            Sign1BuilderData {
                protected,
                unprotected,
                external_aad: &[],
                algorithm: None,
                algorithm_in_protected: true,
                payload: None,
            },
            PhantomData,
        )
    }
}

impl<'a, A, P> Sign1Builder<'a, A, P> {
    /// Set the external AAD to include in the Sig_structure.
    pub const fn external_aad(mut self, external_aad: &'a [u8]) -> Self {
        self.0.external_aad = external_aad;
        self
    }

    /// Set the signing algorithm.
    pub fn algorithm(self, algorithm: Algorithm) -> Sign1Builder<'a, WithAlgorithm, P> {
        Sign1Builder(
            Sign1BuilderData {
                algorithm: Some(algorithm),
                ..self.0
            },
            PhantomData,
        )
    }

    /// Set the payload (attached or detached).
    pub fn payload(self, payload: Payload<'a>) -> Sign1Builder<'a, A, WithPayload> {
        Sign1Builder(
            Sign1BuilderData {
                payload: Some(payload),
                ..self.0
            },
            PhantomData,
        )
    }
}

impl<'a, P> Sign1Builder<'a, WithAlgorithm, P> {
    /// Serialize the protected header map into `out`, returning the byte count.
    #[must_use = "returns the encoded byte count"]
    pub fn encode_protected(&self, out: &mut [u8]) -> Result<usize, CoseError> {
        let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
        validate_builder_headers(self.0.protected, self.0.unprotected)?;
        let mut enc = Encoder::<4>::new(out);
        let extra = usize::from(self.0.algorithm_in_protected);
        let mut map = enc.map_with_len((self.0.protected.len() + extra) as u64)?;
        if self.0.algorithm_in_protected {
            map.insert_key_value(1i64, algorithm.to_i64())?;
        }
        encode_header_map(&mut map, self.0.protected)?;
        map.end()?;
        Ok(enc.len())
    }

    /// Move the algorithm label to the unprotected header instead of the protected header.
    pub const fn algorithm_in_unprotected(mut self) -> Self {
        self.0.algorithm_in_protected = false;
        self
    }
}

impl<'a> Sign1Builder<'a, WithAlgorithm, WithPayload> {
    /// Encode the Sig_structure for this Sign1 into `out`, returning the byte count.
    #[must_use = "returns the encoded byte count"]
    pub fn encode_sig_structure(
        &self,
        protected: &[u8],
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        encode_sig_structure(protected, self.0.external_aad, payload, out)
    }

    /// Encode the complete COSE_Sign1 message using a pre-computed signature.
    #[must_use = "returns the encoded byte count"]
    pub fn encode(
        &self,
        out: &mut [u8],
        protected_buf: &mut [u8],
        signature: &[u8],
    ) -> Result<usize, CoseError> {
        let payload_mode = self.0.payload.ok_or(CoseError::MissingPayload)?;
        let protected_len = self.encode_protected(protected_buf)?;
        let protected_bytes = &protected_buf[..protected_len];
        let mut enc = Encoder::<4>::new(out);
        let mut array = enc.array_with_len(4)?;
        array.insert(protected_bytes)?;
        {
            let extra = usize::from(!self.0.algorithm_in_protected);
            let mut map = array.map_len((self.0.unprotected.len() + extra) as u64)?;
            if !self.0.algorithm_in_protected {
                let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
                map.insert_key_value(1i64, algorithm.to_i64())?;
            }
            encode_header_map(&mut map, self.0.unprotected)?;
            map.end()?;
        }
        match payload_mode {
            Payload::Attached(payload) => array.insert(payload)?,
            Payload::Detached(_) => array.insert(())?,
        };
        array.insert(signature)?;
        array.end()?;
        Ok(enc.len())
    }

    /// Sign and encode the complete COSE_Sign1 message.
    #[must_use = "returns the encoded byte count"]
    pub fn sign<S: Signer>(
        &self,
        signer: &S,
        protected_buf: &mut [u8],
        sig_structure_buf: &mut [u8],
        signature_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
        if signer.algorithm() != algorithm {
            return Err(CoseError::AlgorithmMismatch);
        }
        let payload_mode = self.0.payload.ok_or(CoseError::MissingPayload)?;
        let to_be_signed_payload = match payload_mode {
            Payload::Attached(payload) => payload,
            Payload::Detached(payload) => payload,
        };
        let protected_len = self.encode_protected(protected_buf)?;
        let protected_bytes = &protected_buf[..protected_len];
        let sig_protected = protected_bstr_for_structure(protected_bytes);
        let sig_len =
            self.encode_sig_structure(sig_protected, to_be_signed_payload, sig_structure_buf)?;
        let signature_len = signer.sign(&sig_structure_buf[..sig_len], signature_buf)?;
        self.encode(out, protected_buf, &signature_buf[..signature_len])
    }

    /// Sign using an ES256 (P-256) private key (requires `ring` feature).
    #[cfg(feature = "ring")]
    #[must_use = "returns the encoded byte count"]
    pub fn sign_es256(
        &self,
        key: &Es256PrivateKey<'_>,
        protected_buf: &mut [u8],
        sig_structure_buf: &mut [u8],
        signature_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let signer = Es256Signer::new(key);
        self.sign(
            &signer,
            protected_buf,
            sig_structure_buf,
            signature_buf,
            out,
        )
    }

    /// Sign using an EdDSA (Ed25519) private key (requires `ring` feature).
    #[cfg(feature = "ring")]
    #[must_use = "returns the encoded byte count"]
    pub fn sign_eddsa(
        &self,
        key: &EdDsaPrivateKey<'_>,
        protected_buf: &mut [u8],
        sig_structure_buf: &mut [u8],
        signature_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let signer = EdDsaSigner::new(key);
        self.sign(
            &signer,
            protected_buf,
            sig_structure_buf,
            signature_buf,
            out,
        )
    }
}

/// Encode the COSE Sig_structure for Sign1.
///
/// # Examples
///
/// ```rust
/// use tps_cose::encode_sig_structure;
///
/// let mut out = [0u8; 64];
/// let len = encode_sig_structure(&[0xa0], &[], b"hello", &mut out).unwrap();
/// assert!(len > 0);
/// ```
#[must_use = "returns the encoded byte count"]
pub fn encode_sig_structure(
    protected: &[u8],
    external_aad: &[u8],
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    let mut enc = Encoder::<4>::new(out);
    let mut array = enc.array_with_len(4)?;
    array.insert("Signature1")?;
    array.insert(protected)?;
    array.insert(external_aad)?;
    array.insert(payload)?;
    array.end()?;
    Ok(enc.len())
}

/// Decode a COSE_Sign1 message and validate header semantics.
///
/// Returns a zero-copy [`Sign1`] view into `bytes`.
#[must_use = "returns the decoded Sign1 message"]
pub fn decode_sign1(bytes: &[u8]) -> Result<Sign1<'_>, CoseError> {
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
    let signature = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => bytes,
        _ => return Err(CoseError::InvalidStructure),
    };

    let mut protected_dec = Decoder::new(protected_serialized);
    let protected = if protected_serialized.is_empty() {
        ParsedHeaders::default()
    } else {
        let protected_map = protected_dec
            .next_map()
            .map_err(|_| CoseError::InvalidStructure)?;
        if protected_dec.pos() != protected_serialized.len() {
            return Err(CoseError::InvalidStructure);
        }
        ensure_no_duplicate_labels(&protected_map)?;
        ensure_no_cross_duplicates(&protected_map, &unprotected_map)?;
        parse_headers(&protected_map, true)?
    };
    ensure_no_duplicate_labels(&unprotected_map)?;

    let unprotected = parse_headers(&unprotected_map, false)?;
    if protected.algorithm.or(unprotected.algorithm).is_none() {
        return Err(CoseError::MissingAlgorithm);
    }

    Ok(Sign1 {
        protected_serialized,
        protected,
        unprotected,
        payload,
        signature,
    })
}

/// Verify a decoded COSE_Sign1 with a crypto verifier backend.
#[must_use = "returns an error if verification fails"]
pub fn verify_sign1<V: Verifier>(
    sign1: &Sign1<'_>,
    verifier: &V,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let algorithm = sign1
        .protected
        .algorithm
        .or(sign1.unprotected.algorithm)
        .ok_or(CoseError::MissingAlgorithm)?;
    if algorithm != verifier.algorithm() {
        return Err(CoseError::AlgorithmMismatch);
    }
    let payload = match (sign1.payload, detached_payload) {
        (Some(payload), _) => payload,
        (None, Some(payload)) => payload,
        (None, None) => return Err(CoseError::MissingPayload),
    };
    let protected = protected_bstr_for_structure(sign1.protected_serialized);
    let sig_len = encode_sig_structure(protected, external_aad, payload, sig_structure_buf)?;
    verifier.verify(&sig_structure_buf[..sig_len], sign1.signature)
}

/// Verify a COSE_Sign1 with an ES256 public key (requires `ring` feature).
#[cfg(feature = "ring")]
#[must_use = "returns an error if verification fails"]
pub fn verify_sign1_es256(
    sign1: &Sign1<'_>,
    key: &Es256PublicKey<'_>,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let verifier = Es256Verifier::new(key);
    verify_sign1(
        sign1,
        &verifier,
        external_aad,
        detached_payload,
        sig_structure_buf,
    )
}

/// Verify a COSE_Sign1 with an EdDSA public key (requires `ring` feature).
#[cfg(feature = "ring")]
#[must_use = "returns an error if verification fails"]
pub fn verify_sign1_eddsa(
    sign1: &Sign1<'_>,
    key: &EdDsaPublicKey<'_>,
    external_aad: &[u8],
    detached_payload: Option<&[u8]>,
    sig_structure_buf: &mut [u8],
) -> Result<(), CoseError> {
    let verifier = EdDsaVerifier::new(key);
    verify_sign1(
        sign1,
        &verifier,
        external_aad,
        detached_payload,
        sig_structure_buf,
    )
}
