# tps_cose Design

## Overview

`tps_cose` implements a subset of COSE (CBOR Object Signing and Encryption, RFC 9052)
targeting `no_std` embedded systems. It is built on top of `tps_minicbor` and focuses
on the three COSE structures most relevant to attestation and firmware update protocols:

- **COSE_Sign1** — single-signer signature
- **COSE_Sign** — multi-signer signature (behind the `cose_sign` feature)
- **COSE_Encrypt0** — single-recipient encryption

---

## Structure design

All three COSE structures share the same on-wire layout: a CBOR array of four
items `[protected, unprotected, payload/ciphertext, signature/recipients]`.
The design mirrors this layout in matching Rust types:

| Rust type | COSE structure |
|---|---|
| `Sign1<'a>` | COSE_Sign1 |
| `Sign<'a>` | COSE_Sign |
| `Encrypt0<'a>` | COSE_Encrypt0 |

Each decoded type borrows directly from the source byte slice (zero-copy).
`protected_serialized` holds the raw bytes of the protected header bstr so
that the Sig_Structure or Enc_Structure can be rebuilt from them without
re-encoding.

---

## Typestate builders

Construction is validated at compile time using the typestate pattern. Each
builder has phantom type parameters for mandatory fields. For example,
`Sign1Builder<'a, A, P>` has two phantom states:

- `A ∈ {MissingAlgorithm, WithAlgorithm}` — algorithm must be set before calling `build`.
- `P ∈ {MissingPayload, WithPayload}` — payload must be set before calling `build`.

Calling `.algorithm(alg)` on a `Sign1Builder<'_, MissingAlgorithm, _>` returns a
`Sign1Builder<'_, WithAlgorithm, _>`, and so on.  The `build()` method is only
available when both phantom types are `With*`, meaning the compiler rejects
incomplete builder usage without any runtime checks.

The same pattern applies to `Encrypt0Builder` (mandatory: algorithm + nonce).

---

## Header model

COSE headers are expressed as `&[HeaderParam<'a>]` slices passed to the builder.
`HeaderParam` is a small copy-efficient enum covering:

- `KeyId(&[u8])` — label 4
- `ContentType(ContentType)` — label 3
- `Algorithm(Algorithm)` / `EncryptAlgorithm(EncryptAlgorithm)` — label 1
- `IV(&[u8])` / `PartialIV(&[u8])` — labels 5/6
- `Critical(&[&[i64]])` — label 2
- `Custom(Label, HeaderValue)` — arbitrary application headers

The builder validates that:
- No label appears more than once within a single header map.
- No label appears in both protected and unprotected maps.
- The algorithm declared in `HeaderParam` matches the `Signer`/`Verifier` algorithm.

`ParsedHeaders<'a>` is the decoded counterpart: a slice-backed, linear-search
collection returned by `parse_headers()`, used in the decoded `Sign1` / `Encrypt0`
structs.

---

## Signing and verification

Two traits abstract the crypto backend:

```rust
pub trait Signer {
    fn algorithm(&self) -> Algorithm;
    fn sign(&self, to_be_signed: &[u8], out: &mut [u8]) -> Result<usize, CoseError>;
}

pub trait Verifier {
    fn algorithm(&self) -> Algorithm;
    fn verify(&self, to_be_signed: &[u8], signature: &[u8]) -> Result<(), CoseError>;
}
```

The Sig_Structure (RFC 9052 §4.4) is assembled by `encode_sig_structure()`:
`["Signature1", protected_bstr, external_aad, payload]`.

High-level helpers (`encode_sign1`, `decode_sign1`, `verify_sign1`) handle the
full sign/verify lifecycle.  Low-level helpers (`encode_sig_structure`,
`encode_enc_structure`) are also public so callers can implement custom flows.

---

## Encryption

`Encrypt0Builder` follows the same typestate pattern as `Sign1Builder`. The
ciphertext is produced by:

1. Encoding the Enc_Structure: `["Encrypt0", protected_bstr, external_aad]`.
2. Encrypting the plaintext payload with the algorithm from the protected header,
   using the Enc_Structure bytes as AEAD additional data.
3. Serializing the result as `[protected_bstr, {unprotected}, ciphertext]`.

Supported algorithms (from `EncryptAlgorithm`):

| Algorithm | COSE value | Backend |
|---|---|---|
| AES-GCM-128 | 1 | `ring` |
| AES-CCM-16-64-128 | 10 | `aes` + `ccm` |
| AES-CCM-16-64-256 | 11 | `aes` + `ccm` |
| AES-CCM-64-64-128 | 12 | `aes` + `ccm` |
| … (8 variants total) | | |

---

## Crypto backends (feature `ring`)

The `ring` feature provides concrete implementations of `Signer` and `Verifier`
for:

- **ES256** — ECDSA with P-256 and SHA-256
- **EdDSA** — Ed25519

Each algorithm has private key (`…PrivateKey`) and public key (`…PublicKey`)
wrappers that implement the appropriate trait. The key material is borrowed from
the caller's byte slice, so no heap allocation occurs.

COSE key decoding (`decode_cose_key`) reads a `COSE_Key` map and returns
a typed `CoseKey` enum covering OKP, EC2, and Symmetric key types.

---

## Error handling

A single `CoseError` enum covers all error cases, using `thiserror` for `Display`
formatting. Conversion impls `From<EncodeError>` and `From<DecodeError>` simplify
propagation from `tps_minicbor`.

---

## Embedded-system constraints

- `#![no_std]` — no heap allocation in the core sign/verify/encrypt/decrypt paths.
- The `ring` feature pulls in `ring` and `aes`/`ccm`; these are `no_std`-compatible.
- Buffer sizes for signatures and ciphertext must be provided by the caller.
- All zero-copy borrows (`'a` lifetime) avoid copying data unnecessarily.
