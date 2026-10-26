/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use core::marker::PhantomData;

use tps_minicbor::{Decoder, Encoder, Item};

#[cfg(feature = "ring")]
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};

#[cfg(feature = "ring")]
use aes::{Aes128, Aes256};

// ccm 0.6 / aead 0.6 replaced `generic-array` with `hybrid-array` and split the in-place AEAD
// methods out into `AeadInOut`, which operates on `InOutBuf` rather than on a `&mut [u8]`.
// `Nonce<C>`/`Tag<C>` are the hybrid-array aliases keyed on the cipher's own associated sizes.
#[cfg(feature = "ring")]
// Aliased: `Nonce` and `Tag` would otherwise collide with `ring::aead`'s non-generic types of
// the same name, which are imported above for the AES-GCM paths.
use ccm::aead::{AeadInOut, KeyInit, Nonce as CcmNonce, Tag as CcmTag};

#[cfg(feature = "ring")]
use ccm::consts::{U7, U8, U13, U16};

#[cfg(feature = "ring")]
use ccm::Ccm;

// Provides the `USIZE` associated constant on the ciphers' `TagSize`, which replaces the
// `Unsigned::to_usize()` method that generic-array's typenum re-export used to supply.
#[cfg(feature = "ring")]
use ccm::aead::array::typenum::Unsigned;

use crate::header::{
    HeaderParam, Label, ParsedHeaders, Payload, encode_header_map, ensure_no_cross_duplicates,
    ensure_no_duplicate_labels, header_param_label, parse_encrypt_headers,
    validate_builder_headers,
};
use crate::sign1::{MissingAlgorithm, MissingPayload, WithAlgorithm, WithPayload};
use crate::{CoseError, EncryptAlgorithm};

#[cfg(feature = "ring")]
use crate::header::protected_bstr_for_structure;

#[cfg(feature = "ring")]
use crate::key::CoseKey;

pub(crate) const ALG_A128GCM: i64 = 1;
pub(crate) const ALG_A128CCM_16_64_128: i64 = 10;
pub(crate) const ALG_A256CCM_16_64_256: i64 = 11;
pub(crate) const ALG_A128CCM_64_64_128: i64 = 12;
pub(crate) const ALG_A256CCM_64_64_256: i64 = 13;
pub(crate) const ALG_A128CCM_16_128_128: i64 = 30;
pub(crate) const ALG_A256CCM_16_128_256: i64 = 31;
pub(crate) const ALG_A128CCM_64_128_128: i64 = 32;
pub(crate) const ALG_A256CCM_64_128_256: i64 = 33;
pub(crate) const AES_GCM_NONCE_LEN: usize = 12;
#[cfg(feature = "ring")]
pub(crate) const AES_GCM_KEY_LEN: usize = 16;
#[cfg(feature = "ring")]
pub(crate) const AES_GCM_TAG_LEN: usize = 16;

/// Decoded COSE_Encrypt0 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encrypt0<'a> {
    /// The raw, still-serialized protected header bytes (needed verbatim to rebuild the Enc_structure).
    pub protected_serialized: &'a [u8],
    /// Headers parsed from the protected header map.
    pub protected: ParsedHeaders<'a>,
    /// Headers parsed from the unprotected header map.
    pub unprotected: ParsedHeaders<'a>,
    /// The ciphertext, or `None` if it was detached.
    pub ciphertext: Option<&'a [u8]>,
}

impl<'a> Encrypt0<'a> {
    /// The encryption algorithm, from whichever header bucket (protected or unprotected) carries it.
    pub fn effective_algorithm(&self) -> Option<EncryptAlgorithm> {
        self.protected
            .encrypt_algorithm
            .or(self.unprotected.encrypt_algorithm)
    }

    /// The full IV, from whichever header bucket (protected or unprotected) carries it.
    pub fn effective_iv(&self) -> Option<&'a [u8]> {
        self.protected.iv.or(self.unprotected.iv)
    }

    /// The partial IV, from whichever header bucket (protected or unprotected) carries it.
    pub fn effective_partial_iv(&self) -> Option<&'a [u8]> {
        self.protected.partial_iv.or(self.unprotected.partial_iv)
    }
}

pub(crate) const fn nonce_len_for_algorithm(algorithm: i64) -> Option<usize> {
    match algorithm {
        ALG_A128GCM => Some(AES_GCM_NONCE_LEN),
        ALG_A128CCM_16_64_128
        | ALG_A256CCM_16_64_256
        | ALG_A128CCM_16_128_128
        | ALG_A256CCM_16_128_256 => Some(13),
        ALG_A128CCM_64_64_128
        | ALG_A256CCM_64_64_256
        | ALG_A128CCM_64_128_128
        | ALG_A256CCM_64_128_256 => Some(7),
        _ => None,
    }
}

#[cfg(feature = "ring")]
pub(crate) const fn key_len_for_algorithm(algorithm: i64) -> Option<usize> {
    match algorithm {
        ALG_A128GCM
        | ALG_A128CCM_16_64_128
        | ALG_A128CCM_64_64_128
        | ALG_A128CCM_16_128_128
        | ALG_A128CCM_64_128_128 => Some(16),
        ALG_A256CCM_16_64_256
        | ALG_A256CCM_64_64_256
        | ALG_A256CCM_16_128_256
        | ALG_A256CCM_64_128_256 => Some(32),
        _ => None,
    }
}

/// Builder state marker for missing nonce.
pub struct MissingNonce;
/// Builder state marker for set nonce.
pub struct WithNonce;

/// Typestate COSE_Encrypt0 builder.
pub struct Encrypt0Builder<'a, A = MissingAlgorithm, P = MissingPayload, N = MissingNonce>(
    Encrypt0BuilderData<'a>,
    PhantomData<(A, P, N)>,
);

struct Encrypt0BuilderData<'a> {
    protected: &'a [HeaderParam<'a>],
    unprotected: &'a [HeaderParam<'a>],
    external_aad: &'a [u8],
    payload: Option<Payload<'a>>,
    algorithm: Option<EncryptAlgorithm>,
    algorithm_in_protected: bool,
    nonce: Option<&'a [u8]>,
    nonce_in_protected: bool,
}

impl<'a> Encrypt0Builder<'a, MissingAlgorithm, MissingPayload, MissingNonce> {
    /// Create a new builder over the given protected/unprotected header parameter slices.
    pub const fn new(protected: &'a [HeaderParam<'a>], unprotected: &'a [HeaderParam<'a>]) -> Self {
        Self(
            Encrypt0BuilderData {
                protected,
                unprotected,
                external_aad: &[],
                payload: None,
                algorithm: None,
                algorithm_in_protected: true,
                nonce: None,
                nonce_in_protected: false,
            },
            PhantomData,
        )
    }
}

impl<'a, A, P, N> Encrypt0Builder<'a, A, P, N> {
    /// Set the external additional authenticated data (AAD) supplied by the application.
    pub const fn external_aad(mut self, external_aad: &'a [u8]) -> Self {
        self.0.external_aad = external_aad;
        self
    }

    /// Set the encryption algorithm.
    pub fn algorithm(
        self,
        algorithm: EncryptAlgorithm,
    ) -> Encrypt0Builder<'a, WithAlgorithm, P, N> {
        Encrypt0Builder(
            Encrypt0BuilderData {
                algorithm: Some(algorithm),
                ..self.0
            },
            PhantomData,
        )
    }

    /// Set the payload to be encrypted (attached or detached).
    pub fn payload(self, payload: Payload<'a>) -> Encrypt0Builder<'a, A, WithPayload, N> {
        Encrypt0Builder(
            Encrypt0BuilderData {
                payload: Some(payload),
                ..self.0
            },
            PhantomData,
        )
    }

    /// Set the nonce (IV) to use for encryption.
    pub fn nonce(self, nonce: &'a [u8]) -> Encrypt0Builder<'a, A, P, WithNonce> {
        Encrypt0Builder(
            Encrypt0BuilderData {
                nonce: Some(nonce),
                ..self.0
            },
            PhantomData,
        )
    }
}

impl<'a, P, N> Encrypt0Builder<'a, WithAlgorithm, P, N> {
    /// Place the algorithm header in the unprotected bucket instead of the (default) protected one.
    pub const fn algorithm_in_unprotected(mut self) -> Self {
        self.0.algorithm_in_protected = false;
        self
    }
}

impl<'a, A, P> Encrypt0Builder<'a, A, P, WithNonce> {
    /// Place the nonce header in the protected bucket instead of the (default) unprotected one.
    pub const fn nonce_in_protected(mut self) -> Self {
        self.0.nonce_in_protected = true;
        self
    }
}

impl<'a, P, N> Encrypt0Builder<'a, WithAlgorithm, P, N> {
    /// Encode the protected header map to `out`, returning the number of bytes written.
    pub fn encode_protected(&self, out: &mut [u8]) -> Result<usize, CoseError> {
        validate_encrypt0_builder_headers(self.0.protected, self.0.unprotected)?;
        let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
        let mut extra = usize::from(self.0.algorithm_in_protected);
        if self.0.nonce_in_protected && self.0.nonce.is_some() {
            extra += 1;
        }
        let mut enc = Encoder::<4>::new(out);
        let mut map = enc.map_with_len((self.0.protected.len() + extra) as u64)?;
        if self.0.algorithm_in_protected {
            map.insert_key_value(1i64, algorithm.to_i64())?;
        }
        if self.0.nonce_in_protected
            && let Some(nonce) = self.0.nonce
        {
            map.insert_key_value(5i64, nonce)?;
        }
        encode_header_map(&mut map, self.0.protected)?;
        map.end()?;
        Ok(enc.len())
    }
}

impl<'a> Encrypt0Builder<'a, WithAlgorithm, WithPayload, WithNonce> {
    /// Encode the COSE Enc_structure (RFC 9052 §5.3) to `out`, for use as AEAD additional data.
    pub fn encode_enc_structure(
        &self,
        protected: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        encode_enc_structure(protected, self.0.external_aad, out)
    }

    /// Encode the full COSE_Encrypt0 message from an already-computed `ciphertext`.
    pub fn encode(
        &self,
        out: &mut [u8],
        protected_buf: &mut [u8],
        ciphertext: &[u8],
    ) -> Result<usize, CoseError> {
        let _ = self.0.payload.ok_or(CoseError::MissingPayload)?;
        let protected_len = self.encode_protected(protected_buf)?;
        let protected_bytes = &protected_buf[..protected_len];
        let mut extra = usize::from(!self.0.algorithm_in_protected);
        if !self.0.nonce_in_protected {
            extra += 1;
        }
        let mut enc = Encoder::<4>::new(out);
        let mut array = enc.array_with_len(3)?;
        array.insert(protected_bytes)?;
        {
            let mut map = array.map_len((self.0.unprotected.len() + extra) as u64)?;
            if !self.0.algorithm_in_protected {
                let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
                map.insert_key_value(1i64, algorithm.to_i64())?;
            }
            if !self.0.nonce_in_protected {
                let nonce = self.0.nonce.ok_or(CoseError::MissingNonce)?;
                map.insert_key_value(5i64, nonce)?;
            }
            encode_header_map(&mut map, self.0.unprotected)?;
            map.end()?;
        }
        match self.0.payload.ok_or(CoseError::MissingPayload)? {
            Payload::Attached(_) => array.insert(ciphertext)?,
            Payload::Detached(_) => array.insert(())?,
        };
        array.end()?;
        Ok(enc.len())
    }

    #[cfg(feature = "ring")]
    /// Encrypt the payload with a raw symmetric key and encode the resulting COSE_Encrypt0 message.
    pub fn encrypt_with_raw_key(
        &self,
        key: &[u8],
        protected_buf: &mut [u8],
        enc_structure_buf: &mut [u8],
        ciphertext_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let payload = self.0.payload.ok_or(CoseError::MissingPayload)?;
        let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
        let plaintext = match payload {
            Payload::Attached(p) => p,
            Payload::Detached(p) => p,
        };
        let nonce = self.0.nonce.ok_or(CoseError::MissingNonce)?;
        let expected_nonce_len =
            nonce_len_for_algorithm(algorithm.to_i64()).ok_or(CoseError::UnsupportedAlgorithm)?;
        if nonce.len() != expected_nonce_len {
            return Err(CoseError::InvalidStructure);
        }
        let protected_len = self.encode_protected(protected_buf)?;
        let protected = protected_bstr_for_structure(&protected_buf[..protected_len]);
        let aad_len = self.encode_enc_structure(protected, enc_structure_buf)?;
        let ct_len = encrypt_with_algorithm_raw(
            algorithm.to_i64(),
            key,
            nonce,
            plaintext,
            &enc_structure_buf[..aad_len],
            ciphertext_buf,
        )?;
        self.encode(out, protected_buf, &ciphertext_buf[..ct_len])
    }

    #[cfg(feature = "ring")]
    /// Encrypt the payload with a typed symmetric [`CoseKey`] and encode the resulting COSE_Encrypt0 message.
    pub fn encrypt_with_cose_key(
        &self,
        key: &CoseKey<'_>,
        protected_buf: &mut [u8],
        enc_structure_buf: &mut [u8],
        ciphertext_buf: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, CoseError> {
        let algorithm = self.0.algorithm.ok_or(CoseError::MissingAlgorithm)?;
        let (raw, _) = extract_symmetric_key(key, algorithm.to_i64())?;
        self.encrypt_with_raw_key(raw, protected_buf, enc_structure_buf, ciphertext_buf, out)
    }
}

/// Encode the COSE Enc_structure for Encrypt0.
pub fn encode_enc_structure(
    protected: &[u8],
    external_aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    let mut enc = Encoder::<4>::new(out);
    let mut array = enc.array_with_len(3)?;
    array.insert("Encrypt0")?;
    array.insert(protected)?;
    array.insert(external_aad)?;
    array.end()?;
    Ok(enc.len())
}

/// Decode and validate a COSE_Encrypt0 message.
///
/// Exactly one of IV or Partial IV must be present across the protected/unprotected
/// headers combined; presence of both, or neither, is rejected with
/// [`CoseError::InvalidHeader`]/[`CoseError::MissingNonce`] respectively.
pub fn decode_encrypt0(bytes: &[u8]) -> Result<Encrypt0<'_>, CoseError> {
    let mut dec = Decoder::new(bytes);
    let item = dec.decode()?;
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
    let ciphertext = match iter.next().transpose()? {
        Some(Item::Bytes(bytes)) => Some(bytes),
        Some(Item::Null) => None,
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
        parse_encrypt_headers(&protected_map, true)?
    };
    ensure_no_duplicate_labels(&unprotected_map)?;

    let unprotected = parse_encrypt_headers(&unprotected_map, false)?;
    let algorithm = protected
        .encrypt_algorithm
        .or(unprotected.encrypt_algorithm)
        .ok_or(CoseError::MissingAlgorithm)?;
    let nonce_len =
        nonce_len_for_algorithm(algorithm.to_i64()).ok_or(CoseError::UnsupportedAlgorithm)?;
    let iv = protected.iv.or(unprotected.iv);
    let partial_iv = protected.partial_iv.or(unprotected.partial_iv);
    match (iv, partial_iv) {
        (Some(_), Some(_)) => return Err(CoseError::InvalidHeader),
        (Some(iv), None) => {
            if iv.len() != nonce_len {
                return Err(CoseError::InvalidStructure);
            }
        }
        (None, Some(partial)) => {
            if partial.is_empty() || partial.len() > nonce_len {
                return Err(CoseError::InvalidStructure);
            }
        }
        (None, None) => return Err(CoseError::MissingNonce),
    }

    Ok(Encrypt0 {
        protected_serialized,
        protected,
        unprotected,
        ciphertext,
    })
}

fn validate_encrypt0_builder_headers(
    protected: &[HeaderParam<'_>],
    unprotected: &[HeaderParam<'_>],
) -> Result<(), CoseError> {
    validate_builder_headers(protected, unprotected)?;
    for param in protected {
        let label = header_param_label(*param);
        if matches!(label, Label::Int(1) | Label::Int(5)) {
            return Err(CoseError::InvalidHeader);
        }
    }
    for param in unprotected {
        let label = header_param_label(*param);
        if matches!(label, Label::Int(1) | Label::Int(5)) {
            return Err(CoseError::InvalidHeader);
        }
    }
    Ok(())
}

#[cfg(feature = "ring")]
fn encrypt_a128gcm_raw(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    if key.len() != AES_GCM_KEY_LEN || nonce.len() != AES_GCM_NONCE_LEN {
        return Err(CoseError::InvalidKey);
    }
    if out.len() < plaintext.len() + AES_GCM_TAG_LEN {
        return Err(CoseError::Encode);
    }
    let nonce_bytes: [u8; AES_GCM_NONCE_LEN] =
        nonce.try_into().map_err(|_| CoseError::InvalidKey)?;
    let unbound = UnboundKey::new(&aead::AES_128_GCM, key).map_err(|_| CoseError::InvalidKey)?;
    let key = LessSafeKey::new(unbound);
    out[..plaintext.len()].copy_from_slice(plaintext);
    let tag = key
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(aad),
            &mut out[..plaintext.len()],
        )
        .map_err(|_| CoseError::InvalidKey)?;
    out[plaintext.len()..plaintext.len() + AES_GCM_TAG_LEN].copy_from_slice(tag.as_ref());
    Ok(plaintext.len() + AES_GCM_TAG_LEN)
}

#[cfg(feature = "ring")]
fn decrypt_a128gcm_raw(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    if key.len() != AES_GCM_KEY_LEN || nonce.len() != AES_GCM_NONCE_LEN {
        return Err(CoseError::InvalidKey);
    }
    if ciphertext.len() < AES_GCM_TAG_LEN || out.len() < ciphertext.len() - AES_GCM_TAG_LEN {
        return Err(CoseError::Decode);
    }
    let nonce_bytes: [u8; AES_GCM_NONCE_LEN] =
        nonce.try_into().map_err(|_| CoseError::InvalidKey)?;
    let unbound = UnboundKey::new(&aead::AES_128_GCM, key).map_err(|_| CoseError::InvalidKey)?;
    let key = LessSafeKey::new(unbound);
    let body_len = ciphertext.len() - AES_GCM_TAG_LEN;
    out[..body_len].copy_from_slice(&ciphertext[..body_len]);
    let tag =
        aead::Tag::try_from(&ciphertext[body_len..]).map_err(|_| CoseError::InvalidStructure)?;
    let plain = key
        .open_in_place_separate_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(aad),
            tag,
            &mut out[..body_len],
            0..,
        )
        .map_err(|_| CoseError::InvalidSignature)?;
    Ok(plain.len())
}

#[cfg(feature = "ring")]
type AesCcm16_64_128 = Ccm<Aes128, U8, U13>;
#[cfg(feature = "ring")]
type AesCcm16_64_256 = Ccm<Aes256, U8, U13>;
#[cfg(feature = "ring")]
type AesCcm64_64_128 = Ccm<Aes128, U8, U7>;
#[cfg(feature = "ring")]
type AesCcm64_64_256 = Ccm<Aes256, U8, U7>;
#[cfg(feature = "ring")]
type AesCcm16_128_128 = Ccm<Aes128, U16, U13>;
#[cfg(feature = "ring")]
type AesCcm16_128_256 = Ccm<Aes256, U16, U13>;
#[cfg(feature = "ring")]
type AesCcm64_128_128 = Ccm<Aes128, U16, U7>;
#[cfg(feature = "ring")]
type AesCcm64_128_256 = Ccm<Aes256, U16, U7>;

#[cfg(feature = "ring")]
fn encrypt_ccm<C>(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError>
where
    C: KeyInit + AeadInOut,
{
    let tag_len = C::TagSize::USIZE;
    if out.len() < plaintext.len() + tag_len {
        return Err(CoseError::Encode);
    }
    let cipher = C::new_from_slice(key).map_err(|_| CoseError::InvalidKey)?;
    // Callers validate the nonce length against `nonce_len_for_algorithm` before reaching here,
    // so this conversion does not fail in practice. It is fallible rather than panicking (the old
    // `GenericArray::from_slice` panicked on a length mismatch) so that a future caller which
    // forgets that check gets an error instead of aborting the process on attacker-supplied input.
    let nonce = CcmNonce::<C>::try_from(nonce).map_err(|_| CoseError::InvalidKey)?;
    out[..plaintext.len()].copy_from_slice(plaintext);
    let tag = cipher
        .encrypt_inout_detached(&nonce, aad, (&mut out[..plaintext.len()]).into())
        .map_err(|_| CoseError::InvalidKey)?;
    out[plaintext.len()..plaintext.len() + tag_len].copy_from_slice(tag.as_slice());
    Ok(plaintext.len() + tag_len)
}

#[cfg(feature = "ring")]
fn decrypt_ccm<C>(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError>
where
    C: KeyInit + AeadInOut,
{
    let tag_len = C::TagSize::USIZE;
    if ciphertext.len() < tag_len || out.len() < ciphertext.len() - tag_len {
        return Err(CoseError::Decode);
    }
    let cipher = C::new_from_slice(key).map_err(|_| CoseError::InvalidKey)?;
    // See the note in `encrypt_ccm`: fallible rather than panicking on a bad nonce length.
    let nonce = CcmNonce::<C>::try_from(nonce).map_err(|_| CoseError::InvalidKey)?;
    let body_len = ciphertext.len() - tag_len;
    out[..body_len].copy_from_slice(&ciphertext[..body_len]);
    // The `tag_len` check above guarantees this split is exactly `TagSize` bytes long.
    let tag = CcmTag::<C>::try_from(&ciphertext[body_len..]).map_err(|_| CoseError::Decode)?;
    cipher
        .decrypt_inout_detached(&nonce, aad, (&mut out[..body_len]).into(), &tag)
        .map_err(|_| CoseError::InvalidSignature)?;
    Ok(body_len)
}

#[cfg(feature = "ring")]
fn encrypt_with_algorithm_raw(
    algorithm: i64,
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    let expected_key_len =
        key_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    let expected_nonce_len =
        nonce_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    if key.len() != expected_key_len || nonce.len() != expected_nonce_len {
        return Err(CoseError::InvalidKey);
    }
    match algorithm {
        ALG_A128GCM => encrypt_a128gcm_raw(key, nonce, plaintext, aad, out),
        ALG_A128CCM_16_64_128 => encrypt_ccm::<AesCcm16_64_128>(key, nonce, plaintext, aad, out),
        ALG_A256CCM_16_64_256 => encrypt_ccm::<AesCcm16_64_256>(key, nonce, plaintext, aad, out),
        ALG_A128CCM_64_64_128 => encrypt_ccm::<AesCcm64_64_128>(key, nonce, plaintext, aad, out),
        ALG_A256CCM_64_64_256 => encrypt_ccm::<AesCcm64_64_256>(key, nonce, plaintext, aad, out),
        ALG_A128CCM_16_128_128 => encrypt_ccm::<AesCcm16_128_128>(key, nonce, plaintext, aad, out),
        ALG_A256CCM_16_128_256 => encrypt_ccm::<AesCcm16_128_256>(key, nonce, plaintext, aad, out),
        ALG_A128CCM_64_128_128 => encrypt_ccm::<AesCcm64_128_128>(key, nonce, plaintext, aad, out),
        ALG_A256CCM_64_128_256 => encrypt_ccm::<AesCcm64_128_256>(key, nonce, plaintext, aad, out),
        _ => Err(CoseError::UnsupportedAlgorithm),
    }
}

#[cfg(feature = "ring")]
fn decrypt_with_algorithm_raw(
    algorithm: i64,
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
    out: &mut [u8],
) -> Result<usize, CoseError> {
    let expected_key_len =
        key_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    let expected_nonce_len =
        nonce_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    if key.len() != expected_key_len || nonce.len() != expected_nonce_len {
        return Err(CoseError::InvalidKey);
    }
    match algorithm {
        ALG_A128GCM => decrypt_a128gcm_raw(key, nonce, ciphertext, aad, out),
        ALG_A128CCM_16_64_128 => decrypt_ccm::<AesCcm16_64_128>(key, nonce, ciphertext, aad, out),
        ALG_A256CCM_16_64_256 => decrypt_ccm::<AesCcm16_64_256>(key, nonce, ciphertext, aad, out),
        ALG_A128CCM_64_64_128 => decrypt_ccm::<AesCcm64_64_128>(key, nonce, ciphertext, aad, out),
        ALG_A256CCM_64_64_256 => decrypt_ccm::<AesCcm64_64_256>(key, nonce, ciphertext, aad, out),
        ALG_A128CCM_16_128_128 => decrypt_ccm::<AesCcm16_128_128>(key, nonce, ciphertext, aad, out),
        ALG_A256CCM_16_128_256 => decrypt_ccm::<AesCcm16_128_256>(key, nonce, ciphertext, aad, out),
        ALG_A128CCM_64_128_128 => decrypt_ccm::<AesCcm64_128_128>(key, nonce, ciphertext, aad, out),
        ALG_A256CCM_64_128_256 => decrypt_ccm::<AesCcm64_128_256>(key, nonce, ciphertext, aad, out),
        _ => Err(CoseError::UnsupportedAlgorithm),
    }
}

#[cfg(feature = "ring")]
fn extract_symmetric_key<'a>(
    key: &'a CoseKey<'a>,
    algorithm: i64,
) -> Result<(&'a [u8], Option<&'a [u8]>), CoseError> {
    let expected_key_len =
        key_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    match key {
        CoseKey::Symmetric(sym) => {
            if sym.k.len() != expected_key_len {
                return Err(CoseError::InvalidKey);
            }
            Ok((sym.k, sym.common.base_iv))
        }
        _ => Err(CoseError::InvalidKey),
    }
}

#[cfg(feature = "ring")]
fn derive_nonce<'a>(
    encrypt0: &'a Encrypt0<'a>,
    algorithm: i64,
    base_iv: Option<&'a [u8]>,
    nonce_buf: &'a mut [u8; 13],
) -> Result<&'a [u8], CoseError> {
    let expected_nonce_len =
        nonce_len_for_algorithm(algorithm).ok_or(CoseError::UnsupportedAlgorithm)?;
    if let Some(iv) = encrypt0.effective_iv() {
        if iv.len() != expected_nonce_len {
            return Err(CoseError::InvalidStructure);
        }
        return Ok(iv);
    }
    let partial_iv = encrypt0
        .effective_partial_iv()
        .ok_or(CoseError::MissingNonce)?;
    if partial_iv.is_empty() || partial_iv.len() > expected_nonce_len {
        return Err(CoseError::InvalidStructure);
    }
    let base_iv = base_iv.ok_or(CoseError::MissingNonce)?;
    if base_iv.len() != expected_nonce_len {
        return Err(CoseError::InvalidKey);
    }
    nonce_buf[..expected_nonce_len].copy_from_slice(base_iv);
    let start = expected_nonce_len - partial_iv.len();
    for (dst, src) in nonce_buf[start..expected_nonce_len]
        .iter_mut()
        .zip(partial_iv.iter().copied())
    {
        *dst ^= src;
    }
    Ok(&nonce_buf[..expected_nonce_len])
}

/// Decrypt a decoded COSE_Encrypt0 with raw key bytes.
#[cfg(feature = "ring")]
pub fn decrypt_encrypt0_with_raw_key(
    encrypt0: &Encrypt0<'_>,
    key: &[u8],
    external_aad: &[u8],
    detached_ciphertext: Option<&[u8]>,
    enc_structure_buf: &mut [u8],
    plaintext_out: &mut [u8],
) -> Result<usize, CoseError> {
    let algorithm = encrypt0
        .effective_algorithm()
        .ok_or(CoseError::MissingAlgorithm)?;
    let ciphertext = match (encrypt0.ciphertext, detached_ciphertext) {
        (Some(ct), _) => ct,
        (None, Some(ct)) => ct,
        (None, None) => return Err(CoseError::MissingCiphertext),
    };
    let mut nonce_buf = [0u8; 13];
    let nonce = derive_nonce(encrypt0, algorithm.to_i64(), None, &mut nonce_buf)?;
    let protected = protected_bstr_for_structure(encrypt0.protected_serialized);
    let aad_len = encode_enc_structure(protected, external_aad, enc_structure_buf)?;
    decrypt_with_algorithm_raw(
        algorithm.to_i64(),
        key,
        nonce,
        ciphertext,
        &enc_structure_buf[..aad_len],
        plaintext_out,
    )
}

/// Decrypt a decoded COSE_Encrypt0 with a typed symmetric COSE key.
#[cfg(feature = "ring")]
pub fn decrypt_encrypt0_with_cose_key(
    encrypt0: &Encrypt0<'_>,
    key: &CoseKey<'_>,
    external_aad: &[u8],
    detached_ciphertext: Option<&[u8]>,
    enc_structure_buf: &mut [u8],
    plaintext_out: &mut [u8],
) -> Result<usize, CoseError> {
    let algorithm = encrypt0
        .effective_algorithm()
        .ok_or(CoseError::MissingAlgorithm)?;
    let (raw, base_iv) = extract_symmetric_key(key, algorithm.to_i64())?;
    let ciphertext = match (encrypt0.ciphertext, detached_ciphertext) {
        (Some(ct), _) => ct,
        (None, Some(ct)) => ct,
        (None, None) => return Err(CoseError::MissingCiphertext),
    };
    let mut nonce_buf = [0u8; 13];
    let nonce = derive_nonce(encrypt0, algorithm.to_i64(), base_iv, &mut nonce_buf)?;
    let protected = protected_bstr_for_structure(encrypt0.protected_serialized);
    let aad_len = encode_enc_structure(protected, external_aad, enc_structure_buf)?;
    decrypt_with_algorithm_raw(
        algorithm.to_i64(),
        raw,
        nonce,
        ciphertext,
        &enc_structure_buf[..aad_len],
        plaintext_out,
    )
}
