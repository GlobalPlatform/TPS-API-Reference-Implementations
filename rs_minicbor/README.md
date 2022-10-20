# RS-MINICBOR

An implementation of CBOR in Rust which is aimed at relatively constrained embedded
systems where the Serde implementation is not necessarily well suited.

## License

`rs_minicbor` is MIT licensed. See LICENSE.

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
- Does not support Bignum, DecFrac or BigFloat

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

This is encoded in rs_minicbor as:

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
