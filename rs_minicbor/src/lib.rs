/***************************************************************************************************
 * Copyright (c) 2020-2022, Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all copies or
 * substantial portions of the Software.
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
/**
RS-MINICBOR: a small implementation of CBOR (RFC8949) which can be configured for bare-metal
embedded systems.
 */

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

mod cbor_diag;
pub(crate) mod array;
pub(crate) mod ast;
pub(crate) mod constants;
pub(crate) mod decode;
pub(crate) mod decode_combinators;
pub(crate) mod encode;
pub(crate) mod map;
pub(crate) mod tag;
pub(crate) mod utils;

pub mod error;

pub mod types {
    pub use super::ast::CBOR;
    pub use super::array::array;
    pub use super::map::map;
    pub use super::tag::tag;
}

pub mod decoder {
    // Low-level API
    pub use super::array::ArrayBuf;
    pub use super::decode::{DecodeBufIterator, SequenceBuffer};
    pub use super::map::MapBuf;
    pub use super::tag::TagBuf;

    // Decode Combinators API
    #[cfg(any(feature = "combinators", test))]
    pub use super::decode_combinators::{
        apply, cond, is_any, is_array, is_bool, is_bstr, is_eof, is_false, is_int, is_map, is_nint,
        is_null, is_simple, is_tag, is_tag_with_value, is_true, is_tstr, is_uint, is_undefined,
        opt, or, with_pred, with_value, CBORDecoder,
    };

    #[cfg(any(feature = "combinators", test))]
    pub use super::utils::{Allowable, Filter};

    #[cfg(any(feature = "combinators", test))]
    pub use super::constants::allow::*;

    #[cfg(any(all(feature = "full", feature = "combinators"), test))]
    pub use super::decode_combinators::{is_date_time, is_epoch};
}

pub mod encoder {
    pub use super::encode::{CBORBuilder, EncodeBuffer, EncodeContext, EncodeItem};
}

pub mod debug {
    #[cfg(any(feature = "full", test))]
    pub use super::cbor_diag::Diag;
    pub use super::cbor_diag::print_hex;
}
