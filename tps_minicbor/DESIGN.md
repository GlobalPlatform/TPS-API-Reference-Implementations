# tps_minicbor Design

## Overview

`tps_minicbor` is a `no_std` CBOR encoder/decoder compliant with RFC 8949. It targets embedded systems where neither heap allocation nor the standard library may be available.
Its primary goals are correctness, compact code size, and portability.

---

## Encoding

### Preferred serialization

All values are encoded in the shortest valid CBOR representation (RFC 8610 §4.1):

- Integers use the smallest argument encoding that holds the value (1-byte for 0–23, etc.).
- Byte strings, text strings, arrays, and maps always use definite-length encoding.
- Floats (when the `float` feature is enabled) are downcast to the smallest width
  (`f16` → `f32` → `f64`) that round-trips the value without loss.
- NaN is canonicalized to the preferred half-precision encoding `0xf97e00`.

### Nesting depth via const generic

The `Encoder<'a, const N: usize>` struct carries a fixed-size frame stack of
`N` `ContainerFrame` entries on the call stack — no heap allocation is needed.
`EncoderStd` is a typedef for `Encoder<'a, 4>` (depth 4), which suits most
real-world CBOR structures.

When a container (array or map) is opened, the encoder writes a placeholder
header and pushes a `ContainerFrame` recording the start position and item count.
On `.end()`, the placeholder is overwritten with the actual length, using a
fixed-size header reservation (`MAX_HEADER_LEN = 9` bytes) to avoid shifting
bytes.

### Deterministic encoding (feature `deterministic_encoding`)

Enabling the feature activates pure bytewise-lexicographic map key sorting
on the encoded key bytes (RFC 8949 §4.2.1 — not RFC 7049 "Canonical CBOR"'s
length-first ordering), so it applies uniformly regardless of key type
(integer, `tstr`, `bstr`, ...).
A caller-supplied scratch buffer holds the unordered key-value pairs; they are
sorted in-place on `.end()` and written back to the output buffer. The scratch
buffer must be at least as large as the output buffer.

Duplicate key detection runs during the sort pass; `EncodeError::DuplicateKey`
is returned if any key appears more than once.

### `EncodeItem` trait

Any type that implements `EncodeItem` can be passed to `Encoder::insert()`.
Implementations are provided for all primitive Rust integer types (`u8`–`u128`,
`i8`–`i64`), `bool`, `&str`, `&[u8]`, `f32`/`f64` (under `float`), and the
crate's own `Simple`, `Undefined`, and `Tag` wrapper types.

---

## Decoding

### Zero-copy, streaming

`Decoder` takes an immutable byte slice and advances a position cursor with each
call to `decode()`. All string and byte-slice variants of `Item<'a>` borrow
directly from the source slice, avoiding allocation.

### `Item<'a>` enum

The decoded value is returned as an `Item<'a>` variant:

| Variant | CBOR major type |
|---|---|
| `Unsigned(u64)` | 0 |
| `Negative(i64)` | 1 |
| `Bytes(&[u8])` | 2 |
| `Text(&str)` | 3 |
| `Array(Array)` | 4 |
| `Map(Map)` | 5 |
| `Tag(TaggedItem)` | 6 |
| `Bool(bool)` | 7 simple 20/21 |
| `Null` | 7 simple 22 |
| `Undefined` | 7 simple 23 |
| `Simple(u8)` | 7 other simples |
| `Float(Float)` | 7 float (feature) |

`TryFrom<Item>` conversions are provided for all Rust integer types and `bool`,
returning `DecodeError::OutOfRange` when the value doesn't fit.

### Array and Map lazy iteration

`Array<'a>` and `Map<'a>` do not decode their contents eagerly. Instead, they
expose iterators (`ArrayIter`, `MapIter`) that parse one item per `next()` call,
keeping memory use bounded.

### Error handling

Both `EncodeError` and `DecodeError` are `#[derive(Copy)]` enums. This avoids
any allocation in error paths. `no_std` code can pattern-match directly on these;
`std` code can use them with `thiserror`-derived wrappers in higher layers.

---

## Feature flags

| Flag | Description |
|---|---|
| `std` | Enables `std::error::Error` impls and `std::` test utilities. Not required for string or byte-string map keys, which work on `no_std` |
| `float` | Adds `f16`/`f32`/`f64` support via the `half` crate |
| `deterministic_encoding` | Length-first, then lexicographic map key sorting (RFC 7049 Canonical CBOR ordering) |
| `standard_tags` | Named constants for IANA-assigned tag numbers |
| `diagnostic` | CBOR diagnostic notation output (RFC 8949 §8.1) |

---

## Embedded-system constraints

- `#![no_std]` at crate root; `extern crate alloc` is never imported in the core encoder/decoder.
- The `std` and `diagnostic` features bring in `std::` only for test paths and diagnostic output respectively.
- The frame stack is sized by a const generic at the call site, so stack usage is predictable at compile time.
- All `unsafe` code is confined to byte-level buffer manipulation with bounds checks enforced by the `Sizer` abstraction.
