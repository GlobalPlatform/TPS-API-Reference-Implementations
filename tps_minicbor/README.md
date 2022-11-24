# TPS-MINICBOR

An implementation of CBOR in Rust which is aimed at relatively constrained embedded
systems where the Serde implementation is not necessarily well suited.

## License

`tps_minicbor` is MIT licensed. See LICENSE.

## Features

- Designed to constrained embedded environments requiring `#[no_std]` support.
- High-level encoding and decoding APIs make serialization and deserialization
  flows easier to write correctly.
- Supports most CBOR constructions (see limitations)
  - All primitive types (positive and negative integers, tstr, bstr, simple types, tags,
    floats (including 16 bit floats).
- Arbitrary nesting of arrays and maps with automatic calculation of the correct
  number of items.
- Supports a subset of standard tags (Date/Time and Unix Epoch). Note that these require
  an allocator to be available.
- Conversions to/from Rust primitive types.
- Automatic preferred serialization for integers and floats.
- Iterators and indexing over arrays and maps when deserializing
- Extensive test cases, including test cases for all supported features from RFC8949
  - Note that floating point +Infinity, NaN and -Infinity are always serialized as f16 
    format because this is the preferred representation. Deserialisation works for all
    cases.
- Deserialization of non-preferred representations is supported.

## Current Limitations

- Does not support Canonical CBOR
- Does not support preferred serialization for arrays and maps
- Does not support indefinite length encoding
- Does not directly support Bignum, DecFrac or BigFloat

## Testing

You can run the test cases as follows:

`cargo test --features=full`

> Note that a current limitation is that tests cannot be executed by cargo test
> alone as the featurization does not allow this.

## A flavour of the APIs

### CBOR Encoding

Despite the small memory footprint, the CBOR serialization API is quite high-level,
supporting arbitrary nesting of arrays and maps. 

The example below is an implementation of [Simple TEE Attestation](https://www.ietf.org/archive/id/draft-ietf-rats-eat-14.html#name-eat-produced-by-attestation)
from draft 14 of the Entity Attestation Token specification under development at the IETF.

In CBOR diagnostic format, this is displayed as:

```
{
    / nonce /           10: h'948f8860d13a463e',
    / UEID /           256: h'0198f50a4ff6c05861c8860d13a638ea',
    / OEMID /          258: 64242, / Private Enterprise Number /
    / security-level / 261: 3, / hardware level security /
    / secure-boot /    262: true,
    / debug-status /   263: 3, / disabled-permanently /
    / HW version /     260: [ "3.1", 1 ] / Type is multipartnumeric /
}
```

This is encoded in tps_minicbor as:

```rust
fn encode_tee_eat() -> Result<(), CBORError> {
    // Encode-decode round trip test
    println!("<========================== encode_tee_eat =========================>");
    let mut bytes = [0u8; 1024];
    let nonce: &[u8] = &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e];
    let ueid: &[u8] = &[
        0x01, 0x98, 0xf5, 0x0a, 0x4f, 0xf6, 0xc0, 0x58, 0x61, 0xc8, 0x86, 0x0d, 0x13,
        0xa6, 0x38, 0xea,
    ];

    let mut encoded_cbor = CBORBuilder::new(&mut bytes);
    encoded_cbor.insert(&map(|buff| {
        buff.insert_key_value(&10, &nonce)?
            .insert_key_value(&256, &ueid)?
            .insert_key_value(&258, &64242)?
            .insert_key_value(&261, &3)?
            .insert_key_value(&262, &true)?
            .insert_key_value(&263, &3)?
            .insert_key_value(&260, &array(|buf| buf.insert(&"3.1")?.insert(&1)))
    }))?;

    // do_something_with(encoded_cbor.encoded()?);
    Ok(())
}
```

The only work to do 'by hand' is turning the `bstr` values into suitable references.

### CBOR Decoding

The example below shows one way to decode the payload generated above.

```rust
fn decode_tee_eat() -> Result<(), CBORError> {
    let mut input: &[u8] = &[
        167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245,
        10, 79, 246, 192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250,
        242, 25, 1, 5, 3, 25, 1, 6,  245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46,
        49, 1,
    ];
    let mut nonce = None;
    let mut ueid = None;
    let mut oemid = None;
    let mut sec_level = None;
    let mut sec_boot = None;
    let mut debug_state = None;
    let mut hw_ver_int = None;

    let mut decoder = CBORDecoder::from_slice(&mut input);
    decoder.decode_with(is_map(), |cbor| {
        if let CBOR::Map(map) = cbor {
            nonce = map.get_int(10);
            ueid = map.get_int(256);
            oemid = map.get_int(258);
            sec_level = map.get_int(261);
            sec_boot = map.get_int(262);
            debug_state = map.get_int(263);
            if let Some(CBOR::Array(ab)) = map.get_int(260) {
                hw_ver_int = match ab.index(1) {
                    None => None,
                    Some(CBOR::UInt(vi)) => Some(vi.clone()),
                    _ => None
                };
            }
        }
        Ok(())
    })?;
 Ok(())
}
```

### Examples

#### decode

The `decode` example is a very short sample of the use of the low-level decode API.

To run the example, from the top directory of the `tps_minicbor` crate:

```shell
cargo run --example decode --features=full
```

The expected output is:

```shell
v1 = Ok(1000), v2 = Ok(1000), v3 = Ok(1000), v4 = Err(OutOfRange)
r1 = UInt(1000), e = Some(Eof)
Value: UInt(1000)
```

#### trivial_cose

The `trivial_cose` example is an implementation of the `COSE_Sign1` single signer example in RFC9052
Appendix C.2.1. Keys, the message to be signed and other aspects of the cryptographic configuration
are hard-coded to the values in the Appendix.

While the example is called `trivial_cose` as it implements only the very simplest COSE example, it does
stand as a good example of how to encode and decode moderately complex CBOR structures. All of the inputs
and outputs are bit-exact against the example, thanks to the use of deterministic ECDSA in the signature.

The code also serves as a simplistic example of how to do ECDSA using the Rust crypto traits - something
for which there is a dearth of realistic examples.

> Note: The p256 crate used for ECDSA has not been audited. Please see the warning on the 
> [p256 crate](https://github.com/RustCrypto/elliptic-curves/tree/master/p256), and perform your own
> due diligence before use in production.

To run the example, from the top directory of the `tps_minicbor` crate:

```shell
cargo run --example trivial_cose --features=full
```

The expected output is:

```shell
To be signed 846a5369676e61747572653143a101264054546869732069732074686520636f6e74656e742e
Signature 8eb33e4ca31d1c465ab05aac34cc6b23d58fef5c083106c4d25a91aef0b0117e2af9a291aa32e14ab834dc56ed2a223444547e01f11d3b0916e5a4c345cacb36
Output d28443a10126a10242313154546869732069732074686520636f6e74656e742e58408eb33e4ca31d1c465ab05aac34cc6b23d58fef5c083106c4d25a91aef0b0117e2af9a291aa32e14ab834dc56ed2a223444547e01f11d3b0916e5a4c345cacb36
 18(     [
   h'a10126' ,
   {
      2 :  h'3131' ,
   }
,
   h'546869732069732074686520636f6e74656e742e' ,
   h'8eb33e4ca31d1c465ab05aac34cc6b23d58fef5c083106c4d25a91aef0b0117e2af9a291aa32e14ab834dc56ed2a223444547e01f11d3b0916e5a4c345cacb36' ,
 ],
 )
To be verified 846a5369676e61747572653143a101264054546869732069732074686520636f6e74656e742e
Verification succeeded: message content [84, 104, 105, 115, 32, 105, 115, 32, 116, 104, 101, 32, 99, 111, 110, 116, 101, 110, 116, 46]
```
