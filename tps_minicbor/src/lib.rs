/***************************************************************************************************
 * Copyright (c) 2020-2022, Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice (including the next
 * paragraph) shall be included in all copies or substantial portions of the
 * Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/***************************************************************************************************
 * rs_minicbor module definition
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC8949).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/

// Default configuration
#![no_std]
#![warn(missing_docs)]

//! # TPS_MINICBOR
//!
//! The `tps_minicbor` crate provides a CBOR implementation aimed at embedded targets where the
//! programmer wants to maintain low-level control over serialization and deserialization. The
//! typical use-case is implementation of a standardized CBOR-based container or protocol such as
//! COSE [RFC 9052](https://datatracker.ietf.org/doc/rfc9052/),
//! [C509 certificates](https://datatracker.ietf.org/doc/draft-ietf-cose-cbor-encoded-cert/) or
//! [Entity Attestation Token](https://www.ietf.org/archive/id/draft-ietf-rats-eat-18.txt).
//!
//! The default configuration of `tps_minicbor` does not require an allocator, and simply serializes
//! or deserializes on a byte buffer of your choice. An allocator is required to run tests and some
//! of the examples, but this is only to allow string-based I/O, and is not used by the
//! implementation.
//!
//! The implementation provides a balance of flexibility and small size appropriate for many
//! embedded targets.
//!
//! ## Features
//!
//! The main `tps_minicbor` APIs have been designed to be fairly close to the equivalent
//! constructions in [CDDL](https://www.rfc-editor.org/rfc/rfc8610.txt), which is being used
//! increasingly in IETF specifications to define permitted data structures.
//!
//! - Encoder APIs calculate the correct sizes of arrays and maps and encode them using the
//!   smallest available representation. This helps to avoid errors when changing array and map
//!   structure definitions.
//! - Supports all CBOR primitive types. Positive and negative integers, `tstr`, `bstr` and floats
//!   including `f16`. Floating point support is optional is not needed.
//! - Arrays can be accessed by iterator or by index.
//! - Maps are accessed by key, with integer and `tstr` types being supported.
//! - Conversions to/from Rust primitive types.
//! - Preferred serialization used for integers and floats.
//!
//! ## Examples
//!
//! In the examples below we demonstrate how to encode and decode a simple example of a HW block
//! attestation claims set - this example comes from the Entity Attestation Token, Appendix
//! A.1.3. The example is expressed in CBOR diagnostic format as:
//!
//! > {
//! >      / eat_nonce /       10: h'948f8860d13a463e',
//! >      / ueid /           256: h'0198f50a4ff6c05861c8860d13a638ea',
//! >      / oemid /          258: 64242, / Private Enterprise Number /
//! >      / oemboot /        262: true,
//! >      / dbgstat /        263: 3, / disabled-permanently /
//! >      / hwversion /      260: [ "3.1", 1 ] / Type is multipartnumeric /
//! >  }
//!
//! This is a simple map without optional claims where one claim is an array. The map keys are all
//! integers and the claim values are a mic of integers, `tstr` and `bstr`.
//!
//! > If you want a more complex example, `examples/trivial_cose` demonstrates how to construct,
//! > sign and verify a COSE_Sign1 structure using a deterministic ECDSA P256/SHA256 signature.
//!
//! ### Encoding
//!
//! The example below encodes the above attestation claims on the buffer `bytes`.
//!
//! A few points to note:
//!
//! - The [`encoder::CBORBuilder`] is a wrapper around `bytes` which keeps track of insert position in the
//!   buffer and supports operations like fixing up array and map sizes.
//! -  Values are inserted by reference, even if they are constants
//! - [`types::map`] takes a closure which inserts items into a map. It then fixes up the
//!   number of items in the map for you.
//!     - It is a runtime error if the number of items in the map is not even.
//!     - There is no real check to ensure that your keys and values make sense. The
//!       [`encoder::EncodeBuffer::insert_key_value`] function can help to avoid errors here, but it is possible
//!       to create something that is almost impossible to decode if you try hard enough.
//! - [`types::array`] similarly takes a closure which inserts items into a CBOR array and
//!   then fixes up the number of items for you.
//!
//! ```
//! use tps_minicbor::encoder::CBORBuilder;
//! use tps_minicbor::error::CBORError;
//! use tps_minicbor::types::{array, map};
//!
//! fn main() -> Result<(), CBORError> {
//!     // Encode-decode round trip test
//!     println!("<========================== encode_tee_eat =========================>");
//!     let mut bytes = [0u8; 1024];
//!     let expected: &[u8] = &[
//!         167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245,
//!         10, 79, 246, 192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250,
//!         242, 25, 1, 5, 3, 25, 1, 6, 245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46, 49, 1,
//!     ];
//!
//!     let mut encoded_cbor = CBORBuilder::new(&mut bytes);
//!     encoded_cbor.insert(&map(|buff| {
//!         buff.insert_key_value(&10, &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e].as_slice())?
//!             .insert_key_value(&256, &[0x01, 0x98, 0xf5, 0x0a, 0x4f, 0xf6, 0xc0, 0x58,
//!                                       0x61, 0xc8, 0x86, 0x0d, 0x13, 0xa6, 0x38, 0xea].as_slice())?
//!             .insert_key_value(&258, &64242)?
//!             .insert_key_value(&261, &3)?
//!             .insert_key_value(&262, &true)?
//!             .insert_key_value(&263, &3)?
//!             .insert_key_value(&260, &array(|buf| buf.insert(&"3.1")?.insert(&1)))
//!     }))?;
//!
//!     assert_eq!(encoded_cbor.encoded()?, expected);
//!     Ok(())
//! }
//! ```
//! ### Decoding
//!
//! The example below decodes the claims set that was encoded in the previous example. The
//! implementation decodes into a structure called `TeeEat`.
//!
//! > The structure is defined outside of the closure body, so borrowed values need to be
//! > cloned to be assigned there, because the Borrow Checker cannot determine that `input`
//! > outlives the closure.
//!
//! Some things to note:
//!
//! - [`decoder::CBORDecoder`] is a wrapper over a byte slice that keeps track of decoding state. It
//!   supports a number of methods that help to decode different types of CBOR structure.
//! - [`decoder::CBORDecoder::map`] takes a closure that is passed a [`decoder::MapBuf`], which is a buffer
//!   supporting useful operations such as [`decoder::MapBuf::lookup`] which looks up a value by key
//!   and attempts to turn it into anything with an instance of `TryFrom<CBOR>`.
//! - If you look up a key which holds a map or array, you will get a [`decoder::MapBuf`] or [`decoder::ArrayBuf`],
//!   respectively.
//!
//! ```
//! use tps_minicbor::decoder::{ArrayBuf, CBORDecoder};
//! use tps_minicbor::error::CBORError;
//! use tps_minicbor::types::{array, map};
//!
//! #[derive(Debug, Clone)]
//! struct TeeEat<'t> {
//!     pub nonce: &'t [u8],
//!     pub ueid: &'t [u8],
//!     pub oemid: u64,
//!     pub sec_level: u64,
//!     pub sec_boot: bool,
//!     pub debug_status: u64,
//!     pub hw_version: HwVersion<'t>,
//! }
//!
//! #[derive(Debug, Clone)]
//! struct HwVersion<'t> {
//!     s: &'t str,
//!     v: u64,
//! }
//! 
//! fn decode_tee_eat() -> Result<(), CBORError> {
//!     let mut input: &[u8] = &[
//!         167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245,
//!         10, 79, 246, 192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250,
//!         242, 25, 1, 5, 3, 25, 1, 6, 245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46, 49, 1,
//!     ];
//!     let mut vsn_string = String::new();
//!     let mut token = TeeEat {
//!         nonce: &[],
//!         ueid: &[],
//!         oemid: 0,
//!         sec_level: 0,
//!         sec_boot: false,
//!         debug_status: 0,
//!         hw_version: HwVersion { s: &"", v: 0},
//!     };
//!
//!     let decoder = CBORDecoder::from_slice(&mut input);
//!     {
//!         let _d = decoder.map(|mb| {
//!             token.nonce = mb.lookup(10)?;
//!             token.ueid = mb.lookup(256)?;
//!             token.oemid = mb.lookup(258)?;
//!             token.sec_level = mb.lookup(261)?;
//!             token.sec_boot = mb.lookup(262)?;
//!             token.debug_status = mb.lookup(263)?;
//!             let ab: ArrayBuf = mb.lookup(260)?;
//!             // Any bstr or tstr needs to be deep copied somewhere that
//!             // definitely lives longer than the closure if we wish to use it
//!             // outside. Here we copy into a string which is inferred to outlive
//!             // the closure.
//!             vsn_string = String::from(ab.item::<&str>(0)?);
//!             token.hw_version.s = &vsn_string;
//!             token.hw_version.v = ab.item(1)?;
//!             Ok(())
//!         })?;
//!     }
//!
//!     assert_eq!(
//!         token.nonce,
//!         &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e]
//!     );
//!     assert_eq!(
//!         token.ueid,
//!         &[
//!             0x01, 0x98, 0xf5, 0x0a, 0x4f, 0xf6, 0xc0, 0x58, 0x61, 0xc8, 0x86, 0x0d, 0x13, 0xa6,
//!             0x38, 0xea
//!         ]
//!     );
//!     assert_eq!(token.oemid, 64242);
//!     assert_eq!(token.sec_level, 3);
//!     assert_eq!(token.sec_boot, true);
//!     assert_eq!(token.debug_status, 3);
//!     assert_eq!(token.hw_version.s, "3.1");
//!     assert_eq!(token.hw_version.v, 1);
//!     Ok(())
//! }
//! ```

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "full", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

#[cfg(any(feature = "float", test))]
extern crate half;

#[cfg(any(feature = "full", test))]
extern crate chrono;

pub(crate) mod array;
pub(crate) mod ast;
mod cbor_diag;
pub(crate) mod constants;
pub(crate) mod decode;
pub(crate) mod decode_combinators;
pub(crate) mod encode;
pub(crate) mod map;
pub(crate) mod tag;
pub(crate) mod utils;

/// The `error` module contains error definitions used throughout `tps_minicbor`.
pub mod error;

/// The `types` module exports the main [`types::CBOR`] structure which represents a single
/// CBOR item, and the [`types::array`], [`types::map`] and [`types::tag`] which simplify
/// encoding of maps, arrays and tags, respectively.
pub mod types {
    pub use super::array::array;
    pub use super::ast::CBOR;
    pub use super::map::map;
    pub use super::tag::tag;
}

/// The `decoder` module exports types, functions and traits for decoding CBOR items from a buffer
pub mod decoder {
    // Low-level API
    pub use super::array::ArrayBuf;
    pub use super::decode::{DecodeBufIterator, SequenceBuffer};
    pub use super::map::MapBuf;
    pub use super::tag::TagBuf;

    // Decode Combinators API
    #[cfg(any(feature = "combinators", test))]
    pub use super::decode_combinators::{
        apply, cond, decode_bool, decode_bstr, decode_int, decode_nint, decode_null,
        decode_simple, decode_tstr, decode_uint, decode_undefined, is_any, is_array, is_bool,
        is_bstr, is_eof, is_false, is_int, is_map, is_nint, is_null, is_simple, is_tag,
        is_tag_with_value, is_true, is_tstr, is_uint, is_undefined, opt, or, with_pred,
        with_value, CBORDecodable, CBORDecoder,
    };

    #[cfg(any(feature = "combinators", test))]
    pub use super::utils::{Allowable, Filter};

    #[cfg(any(feature = "combinators", test))]
    pub use super::constants::allow::*;

    #[cfg(any(all(feature = "full", feature = "combinators"), test))]
    pub use super::decode_combinators::{is_date_time, is_epoch};
}

/// The `encoder` module exports the [`encoder::CBORBuilder`] and [`encoder::EncodeBuffer`]
/// types, which are used to encode values as CBOR items.
pub mod encoder {
    pub use super::encode::{CBORBuilder, EncodeBuffer, EncodeContext, EncodeItem};
}

#[cfg(any(feature = "full", test))]
pub mod debug {
    #[cfg(any(feature = "full", test))]
    pub use super::cbor_diag::print_hex;
    #[cfg(any(feature = "full", test))]
    pub use super::cbor_diag::Diag;
}
