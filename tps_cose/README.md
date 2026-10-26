# tps_cose

## Overview

A library implementing COSE (IETF RFCs 9052, 9053, 9054) on top of the `tps_cbor` crate.

It currently supports `COSE_Sign1`, `COSE_Sign`, `COSE_Encrypt0` operations. `COSE_Key` can represent EC2 keys, OKP keys and symmetric keys for any curve identifier in `Curve`, but the `ring`-backed signing/verification implementation only covers:

- Asymmetric Elliptic Curve-based cryptography
  - ES256 (NIST P-256)
  - EdDSA (Ed25519)
- Symmetric cryptography
  - AES-GCM with 128 bit key
  - AES-CCM variants (16-64-128, 16-64-256, 64-64-128, 64-64-256, 16-128-128, 16-128-256, 64-128-128, 64-128-256).

`Curve` also has identifiers for P384, P521, X25519 and X448, but no signer/verifier implements them yet. In the current implementation the `ring` crate provides cryptographic operations, so it is fairly straightforward to add support for anything ring and the COSE standards support.

Here's an outline of how to perform a COSE_Sign1 operation using EdDSA:

```rust
let payload = [1,2,3]; // Replace with real CBOR-encoded payload
let kid = [1]; // Put a real key ID
let protected = [HeaderParam::ContentType(ContentType::Uint(0))];
let unprotected = [HeaderParam::KeyId(kid.as_slice())];

// Buffers used during serialization
let mut protected_buf = [0u8; 64];      // Protected headers serialization buffer
let mut sig_structure_buf = [0u8; 128]; // Signature structure buffer
let mut signature_buf = [0u8; 128];     // Output signature buffer
let mut message = [0u8; 256];           // Signed message output

let x = 0; // Set a real value for signing key X parameter here
let d = 0; // Set a real value for signing key D parameter here
let private = EdDsaPrivateKey { d: &d, x: &x };

let builder = Sign1Builder::new(&protected, &unprotected)
    .algorithm(Algorithm::EdDsa)
    .payload(Payload::Attached(payload));
let msg_len = builder
    .sign_eddsa(
        &private,
        &mut protected_buf,
        &mut sig_structure_buf,
        &mut signature_buf,
        &mut message,
    )
    .expect("sign1 failure");
```

