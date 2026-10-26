# tps_cose_tool

Provides `cose-tool`, a command line tool for COSE_Sign1 and COSE_Encrypt0 operations
([RFC 9052](https://www.rfc-editor.org/info/rfc9052)) together with basic COSE key inspection.
The cryptography and encoding are entirely `tps_cose`'s (built with its `ring` and `std`
features); this crate is a CLI over them.

It is useful both as a working example of the `tps_cose` API and as a way to produce and check
COSE artifacts from a shell or a test script — for instance to verify that an artifact signed
by an embedded target validates on a host.

## What is provided

- Binaries
  - `cose-tool`, with the subcommands described below.
  - `gen-testdata`, which regenerates the fixtures in `testdata/`. It takes no arguments and
    writes to a path derived from the crate's manifest directory.

This crate has no library target.

### `cose-tool sign1`

| Command | Effect |
|---|---|
| `sign --key <file> [--input <file>] [--output <file>] [--aad <hex>]` | Sign a payload, producing a COSE_Sign1 message |
| `verify --key <file> [--input <file>] [--output <file>] [--aad <hex>]` | Verify a COSE_Sign1 message and recover its payload |

Signing requires a private key; verification accepts either a private or a public key file.
Two algorithms are supported, selected by the key's own type rather than by a flag: EC2 P-256
(ES256) and OKP Ed25519 (EdDSA).

### `cose-tool encrypt0`

| Command | Effect |
|---|---|
| `encrypt --key <file> [--input <file>] [--output <file>] [--alg <alg>] [--nonce <hex>] [--aad <hex>]` | Encrypt plaintext, producing a COSE_Encrypt0 message |
| `decrypt --key <file> [--input <file>] [--output <file>] [--aad <hex>]` | Decrypt a COSE_Encrypt0 message |

The key file must be a COSE Symmetric key whose length matches the chosen algorithm.
`--nonce` is generated randomly (via `ring::rand::SystemRandom`) when omitted, which is what
you normally want — a nonce must never be reused with the same key. Accepted `--alg` values:

| `--alg` | Key | Nonce | Tag |
|---|---|---|---|
| `a128gcm` (default) | 128-bit | 12 bytes | 16 bytes |
| `a128ccm-16-64-128` | 128-bit | 13 bytes | 8 bytes |
| `a128ccm-16-64-256` | 256-bit | 13 bytes | 8 bytes |
| `a128ccm-64-64-128` | 128-bit | 7 bytes | 8 bytes |
| `a128ccm-64-64-256` | 256-bit | 7 bytes | 8 bytes |
| `a128ccm-16-128-128` | 128-bit | 13 bytes | 16 bytes |
| `a128ccm-16-128-256` | 256-bit | 13 bytes | 16 bytes |
| `a128ccm-64-128-128` | 128-bit | 7 bytes | 16 bytes |
| `a128ccm-64-128-256` | 256-bit | 7 bytes | 16 bytes |

The `a128ccm-` prefix is carried by every CCM variant, including the ones taking a 256-bit key;
read the key size off the last component (`AES-CCM-{L}-{tag}-{key}`, as named in RFC 9053 §4.2),
not off the prefix.

### `cose-tool key info <file>`

Prints a stored COSE key's type, and its curve and whether a private component is present
(asymmetric keys) or its length in bytes (symmetric keys), along with its key ID if it has one.
It does not print key material.

### Input and output conventions

`--input` defaults to stdin and `--output` to stdout throughout. Where a command's output is a
COSE message or a plaintext file, stdout receives raw binary; where the output is a recovered
payload and `--output` was omitted, it is hex-encoded instead so that it is safe to print to a
terminal. `--aad` is hex and defaults to empty, and must match between the two halves of a
sign/verify or encrypt/decrypt pair — if it does not, verification or decryption fails, which
is the point of it.

## Test data

`testdata/` holds IETF test vector keys and a sample payload; see `testdata/README.md` for
their provenance. They are published test vectors with published private keys, so they are
suitable for examples and tests and for nothing else.

```bash
# Sign with ES256, then verify with the matching public key
cose-tool sign1 sign --key testdata/es256_private.cbor \
    --input testdata/payload.txt --output signed.cbor
cose-tool sign1 verify --key testdata/es256_public.cbor --input signed.cbor
# Signature verified.
# 546869732069732074686520636f6e74656e742e

# Verify and recover the payload as a file rather than as hex
cose-tool sign1 verify --key testdata/es256_public.cbor \
    --input signed.cbor --output payload_out.txt

# Encrypt and decrypt, with external AAD
cose-tool encrypt0 encrypt --key testdata/aes128gcm_key.cbor --aad 00112233 \
    --input testdata/payload.txt --output enc.cbor
cose-tool encrypt0 decrypt --key testdata/aes128gcm_key.cbor --aad 00112233 \
    --input enc.cbor --output plain.txt

# Inspect a key
cose-tool key info testdata/es256_private.cbor
```

## License

`tps_cose_tool` is MIT licensed.
