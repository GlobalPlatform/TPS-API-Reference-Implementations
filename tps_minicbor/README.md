# tps_minicbor

CBOR encoder/decoder optimized for no_std targets with minimal dependencies.

## Overview

- `no_std` core with optional features for floats, deterministic encoding, and standard tags.
  - Deterministic encoding requires a `std` environment
- Supports most CBOR constructions (see limitations).
  - All primitive types (positive and negative integers, tstr, bstr, simple types, tags, floats including 16 bit floats)
  - Supports most tags in RFC8949 (values unchecked): Date/Time, Epoch, Bignums, Decimal Fractions, Base16 and 64, embedded CBOR, URI, Regex, MIMEtype)
- Configurable nesting depth for encoding arrays and maps with automatic calculation of the correct number of items.
  - `EncoderStd` is a convenience wrapper with the nesting depth set to 4.
- Defaults to CBOR preferred encoding.
  - Deterministic encoding is an optional feature (requires `std`)
  - Numeric types, including floats, are always encoded at minimal size.
  - Non-trivial `NaN` is not supported. `NaN` is always encoded as `0x7e00`.
- Extensive test cases implemented including all RFC8949 tests.
- COSE and basic CDDL support available in related `tps_cose` and `tps_cddl` crates.

## Encoding

### Basic usage

```rust
use tps_minicbor::{EncoderStd, Encoder};

let mut buf = [0u8; 16];
let mut enc = EncoderStd::new(&mut buf);
enc.insert(1u8).unwrap();
assert_eq!(enc.encoded(), &[0x01]);

let mut buf = [0u8; 16];
let mut enc = Encoder::<8>::new(&mut buf);
enc.insert(2u8).unwrap();
assert_eq!(enc.encoded(), &[0x02]);
```

### Arrays and maps

```rust
use tps_minicbor::Encoder;

let mut buf = [0u8; 16];
let mut enc = Encoder::<2>::new(&mut buf);
let mut array = enc.array().unwrap();
array.insert(1u8).unwrap();
array.insert(2u8).unwrap();
array.insert(3u8).unwrap();
array.end().unwrap();
assert_eq!(enc.encoded(), &[0x83, 0x01, 0x02, 0x03]);
```

### Deterministic encoding

When `deterministic_encoding` is enabled, map entries are sorted into pure
bytewise-lexicographic order of their encoded key bytes (RFC 8949 §4.2.1)
before the map is finalized. This requires a scratch buffer supplied by the
caller so that the encoder can reorder entries without allocations.

The scratch buffer must not alias the output buffer and should be at least as
large as the output buffer to handle worst-case maps that fill the buffer and
require complete reordering.

```rust
use tps_minicbor::Encoder;

let mut buf = [0u8; 32];
let mut enc = Encoder::<4>::new(&mut buf);
let mut map = enc.map().unwrap();
map.insert_key_value("b", 2u8).unwrap();
map.insert_key_value("aa", 3u8).unwrap();
map.insert_key_value("a", 1u8).unwrap();
map.end().unwrap();
assert_eq!(
    enc.encoded(),
    &[0xa3, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03, 0x61, 0x61, 0x01]
);
```

```rust
# #[cfg(feature = "deterministic_encoding")]
# {
use tps_minicbor::EncoderStd;

let mut buf = [0u8; 32];
let mut scratch = [0u8; 32];
let mut enc = EncoderStd::new_deterministic(&mut buf, &mut scratch).unwrap();
let mut map = enc.map().unwrap();
map.insert_key_value("b", 2u8).unwrap();
map.insert_key_value("aa", 3u8).unwrap();
map.insert_key_value("a", 1u8).unwrap();
map.end().unwrap();
assert_eq!(
    enc.encoded(),
    &[0xa3, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03]
);
# }
```

### Floating point

When `float` is enabled, CBOR support for encoding and decoding `f64`, `f32` and `f16`
types is present. These do not depend on the allocator, so are suitable for `no_std`
targets, but there will be additional code, which may be an issue.

Where a value is `NaN`, the encoding is always equivalent to `f16::NAN`, which
corresponds to an encoded value of `0x7e00`. This value was selected as the shortest
encoding of `NaN`. If you require more sophisticated `NaN` handling along with
interoperability, you will likely need to handle this at the application level with
additional tags. Handling of non-trivial `NaN` values has been
[discussed extensively on the CBOR mailing list](https://mailarchive.ietf.org/arch/browse/cbor/?gbt=1&q=NaN),
and anyone needing this support is advised to read these discussions first.

## Decoding

`Decoder` provides two API families:

- **Typed direct methods** — `uint()`, `neg_int()`, `bytes()`, `text()`, `bool()`,
  `null()`, `array_len()`, `map_len()`: decode a single item directly into a Rust type.
  These are the preferred methods for use inside `Decode` implementations where the
  expected type is known.

- **`next_*` convenience methods** — `next()`, `next_uint()`, `next_bytes()`, etc.:
  decode the next item into the `Item` enum (or an extracted field thereof). Use these
  when the item type must be determined at runtime, for example when iterating a
  heterogeneous array.

### Basic decoding

```rust
use tps_minicbor::{Decoder, Item};

let data = [0x01];
let mut dec = Decoder::new(&data);
let item = dec.decode().unwrap();
assert!(matches!(item, Item::Unsigned(1)));
```

### Implementing `Decode` for custom types

```rust
use tps_minicbor::{Decode, Decoder, DecodeError};

struct Point { x: i32, y: i32 }

impl<'a> Decode<'a> for Point {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let len = d.array_len()?;
        if len != 2 { return Err(DecodeError::InvalidAdditionalInfo); }
        let x = i32::decode(d)?;
        let y = i32::decode(d)?;
        Ok(Point { x, y })
    }
}
```

### Decoding arrays and maps

```rust
use tps_minicbor::{Decoder, Item};

let data = [0x82, 0x01, 0x02];
let mut dec = Decoder::new(&data);
let array = dec.next_array().unwrap();
let items: Vec<u64> = array.iter()
    .map(|r| match r.unwrap() { Item::Unsigned(v) => v, _ => 0 })
    .collect();
assert_eq!(items, vec![1, 2]);
```

## Features

- `float`: encode/decode floating-point values with smallest-width encoding and canonical NaN.
- `deterministic_encoding`: canonical map ordering using a caller-provided scratch buffer.
- `standard_tags`: support for RFC 8949 standard tags.
- `std`: enables `std::error::Error` impls and I/O sink support for diagnostic output. Not required for string or byte-string map keys, which are supported on `no_std`.
- `diagnostic`: CBOR diagnostic notation output (requires `std`).

## Interoperability

> No configuration of tps_minicbor supports indefinite length encoding. This is a design
> decision based on the intended use in constrained embedded devices.

When compiled with default features:

- Encoding and decoding of Rust integer types is supported for values, `v` in the range `i64::MIN` <= v <= `u64::MAX`.
  - Traits provide conversion to and from all Rust builtin integer types in the value range above.
  - Outside of this range, an error is generated.
  - Integers are always encoded using the smallest binary representation, regardless of the original Rust type.
- Map items are encoded in the order of insertion. There are no checks for
  duplicate keys unless `deterministic_encoding` is enabled. Integer, text
  string (`tstr`), and byte string (`bstr`) keys are all supported, on `no_std`
  as well as `std` targets. It is possible to access all instances of values
  where keys are duplicated, although some handling by the caller may be required.
- Float values cannot be encoded or decoded.
- Simple values can be encoded and decoded. On decode, simple integer values are
  distinguished from the main integer type.
- Arrays and maps can be encoded to a maximum nesting level determined when the
  encoder is constructed. This design has chosen (rather than arbitrary nesting)
  to allow an implementation without an allocator.

When compiled with `float` support, the following additional interoperability
guarantees are made:

- Encoding and decoding of Rust `f64`, `f32` and `f16` floating point values in the
  smallest CBOR float encoding that accurately preserves the value (f16, f32, f64),
  regardless of the original Rust type.
- `NaN` always encoded as `0x7e00`.
- Infinities may not be minimized. Infinities and subnormal values are handled per
  the implementation of Rust floating point on the target architecture (Rust makes
  no guarantees on such points as underlying platform hardware behaviour can differ).
  - Interoperability of encoded and decoded infinities is thus only guaranteed
    between Rust implementations on platforms known to behave identically.
- Floating point values which have accurate integer representations are encoded as
  floats. In other words, float and integer values are kept entirely separate and
  there is no _unified_ numeric stack. This behaviour is most compatible with the
  expectations of constrained embedded targets which generally separate integer and
  float types.
  - This behaviour is not compatible with dCBOR.

When compiled with `std` support, the following additional interoperability guarantees
are made:

- `std::error::Error` is implemented for [`EncodeError`] and [`DecodeError`].
  (Integer, text string, and byte string map keys are supported without this
  feature; see [Interoperability](#interoperability) above.)

When compiled with `deterministic_encoding` support, the following additional
interoperability guarantees are made:

- Map items are ordered by pure bytewise-lexicographic comparison of the
  encoded key bytes, per the core deterministic encoding requirements of
  RFC 8949 section 4.2.1 (not RFC 7049 "Canonical CBOR"'s length-first
  ordering).
- Duplicate map keys are detected on encoding. The behaviour of decoding in the
  presence of duplicate map keys is undefined and may change between versions of this crate.
- This encoding is not compatible with RFC 7049 "Canonical CBOR" encoding.

When compiled with `standard_tags` support, the following additional interoperability guarantees are made:

- Epoch time (Tag 1) is encoded in shortest representation of its underlying type (i.e. integers as integers and floats as floats).
- Epoch time float values are only encoded and decoded when `float` support is enabled.

### Further interoperability considerations

- Map keys are not restricted by CBOR major type: anything implementing
  [`EncodeItem`] can be used as a key on encoding, and decoding returns the raw
  key `Item` with no filtering. Integer, text string (`tstr`), and byte string
  (`bstr`) keys are the practical common case and the ones exercised by this
  crate's test suite; other `EncodeItem` types (e.g. `bool`, tagged items) are
  accepted but not a primary design target.
- There is no support for equivalence models for the standard tags defined in RFC 8949. Values are not validated on encode or decode; upper layers are expected to manage this.

## Design

There are some brief [notes on the design](DESIGN.md).