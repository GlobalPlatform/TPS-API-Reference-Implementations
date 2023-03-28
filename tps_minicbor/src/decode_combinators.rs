/***************************************************************************************************
 * Copyright (c) 2021-2023, Qualcomm Innovation Center, Inc. All rights reserved.
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
 * CBOR Decoder Combinators
 *
 * A fairly comprehensive, memory efficient, deserializer for CBOR (RFC7049). This deserializer
 * is designed for use in constrained systems and requires neither the Rust standard library
 * nor an allocator.
 *
 * The decode combinators do make use of dynamic dispatch and have some memory penalty in return
 * for a more comfortable API. They can be disabled using the `embedded` build feature.
 **************************************************************************************************/
/// # CBOR Decode Combinators
///
/// The decode combinator library is designed to make it much easier to write CBOR decoding
/// libraries. It supports two different approaches to writing code, depending on the use-case.
///
/// ## Direct decoding of values
///
/// This approach updates mutable variables with values read from a [`CBORDecoder`] buffer.
/// It has the advantage that does not require potentially deep nesting of closures, and works
/// well when decoding into mutable data structures.
///
/// In general, this approach is recommended as it generally results in shorter and more concise
/// code.
///
/// The example shows how several common CBOR items can be decoded:
///
/// ```
///# use tps_minicbor::decoder::*;
///# use tps_minicbor::encoder::*;
///# use tps_minicbor::error::CBORError;
///# use tps_minicbor::types::{array, map, tag, CBOR};
///
///# fn main() -> Result<(), CBORError> {
///     let mut input: &[u8] = &[
///         0xa2, 0x0a, 0x48, 0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e,
///         0x19, 0x01, 0x04, 0x82, 0x63, 0x33, 0x2e, 0x31, 0x01
///     ];
///     let mut vsn_string = String::new();
///     let mut nonce = [].as_slice();
///     let mut hw_vsn_s = "";
///     let mut hw_vsn_v = 0u64;
///
///     let decoder = CBORDecoder::from_slice(&mut input);
///     let _d = decoder.map(|mb| {
///         nonce = mb.lookup(10)?;
///         let ab: ArrayBuf = mb.lookup(260)?;
///         // Any bstr or tstr needs to be deep copied somewhere that
///         // definitely lives longer than the closure if we wish to use it
///         // outside. Here we copy into a string which is inferred to outlive
///         // the closure.
///         vsn_string = String::from(ab.item::<&str>(0)?);
///         hw_vsn_s = &vsn_string;
///         hw_vsn_v = ab.item(1)?;
///          Ok(())
///     })?;
///     assert_eq!(nonce, &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e]);
///     assert_eq!(hw_vsn_v, 1);
///     assert_eq!(hw_vsn_s, "3.1");
///     Ok(())
///# }
/// ```
///
/// ## Decode Items within a closure
///
/// This approach is always used with [`Array`] and [`Map`] types, and may be used with any CBOR
/// item if it is best handled in closure context.
///
/// > **Note:** Support for use of this style outside of closures may be removed in a future
/// > version of the library.
///
/// In this style, a call to [`CBORDecoder::decode_with`] is made using one of the CBOR matching
/// parsers such as `is_tstr()`. If the expected type is matched, the provided closure is called
/// with the current CBOR item, and the caller can act on it.
///
/// ```
///# use std::convert::TryFrom;
///# use tps_minicbor::decoder::*;
///# use tps_minicbor::encoder::*;
///# use tps_minicbor::error::CBORError;
///# use tps_minicbor::types::{array, map, tag, CBOR};
///# fn main() -> Result<(), CBORError> {
///     let mut bytes = [0u8; 128];
///     let mut encoded_cbor = CBORBuilder::new(&mut bytes);
///     encoded_cbor
///         .insert(&32u8)?
///         .insert(&(-(0xa5a5a5i32)))?
///         .insert(&"新年快乐")?
///         .insert(&array(|buf| {
///             buf.insert(&42u8)?
///                 .insert(&CBOR::Undefined)
///         }))?;
///
///     let _decoder = CBORDecoder::new(encoded_cbor.build()?)
///         .decode_with(is_uint(), |cbor| {
///             Ok(assert_eq!(u32::try_from(cbor)?, 32u32))
///         })?
///         .decode_with(is_nint(), |cbor| {
///             Ok(assert_eq!(i32::try_from(cbor)?, -(0xa5a5a5i32)))
///         })?
///         .decode_with(is_tstr(), |cbor| {
///             Ok(assert_eq!(<&str>::try_from(cbor)?, "新年快乐"))
///         })?
///         .decode_with(is_array(), |cbor| {
///             CBORDecoder::from_array(cbor)?
///                 .decode_with(is_uint(), |cbor| Ok(assert_eq!(u8::try_from(cbor)?, 42)))?
///                 .decode_with(is_undefined(), |cbor| Ok(assert_eq!(cbor, CBOR::Undefined)))?
///                 .finalize()
///         })?;
///     Ok(())
///# }
/// ```

use crate::array::ArrayBuf;
use crate::ast::CBOR;
use crate::decode::{DecodeBufIterator, SequenceBuffer};
use crate::error::CBORError;
use crate::map::MapBuf;
use crate::tag::TagBuf;
use core::convert::TryFrom;

use std::cell::{Ref, RefCell};
use std::convert::From;

/// Alias for the Result type for all CBOR decode combinators.
type DCResult<'buf> = core::result::Result<(DecodeBufIterator<'buf>, CBOR<'buf>), CBORError>;
/// Alias for the Result type where the output type, `O`, is generic.
type DCPResult<'buf, O> = core::result::Result<(DecodeBufIterator<'buf>, O), CBORError>;

/***************************************************************************************************
 * Top Level Decoder API
 **************************************************************************************************/

/// CBORDecoder provides a smart wrapper over a byte slice, keeping information on the current
/// state of CBOR decoding.
///
/// It is a wrapper over an instance of a [`DecodeBufIterator`], since CBOR decoding is basically
/// performed by iterating over instances of [`CBOR`] which map over the buffer.
///
/// You can build instances of `CBORDecoder` from byte slices, tagged items, arrays and maps
pub struct CBORDecoder<'buf> {
    decode_buf_iter: RefCell<DecodeBufIterator<'buf>>,
}

impl<'buf> CBORDecoder<'buf> {
    /// Construct a new instance of a `CBORDecoder` from a `SequenceBuffer`.
    #[inline]
    pub fn new(b: SequenceBuffer<'buf>) -> Self {
        Self {
            decode_buf_iter: RefCell::new(b.into_iter()),
        }
    }

    /// Construct a new instance of a `CBORDecoder` from a &[u8] slice.
    ///
    /// # Example
    ///
    /// ```
    /// use tps_minicbor::decoder::{CBORDecoder};
    ///
    /// let decoder = CBORDecoder::from_slice(&[0x73, 0x49, 0x20, 0x6c, 0x6f, 0x76, 0x65, 0x20,
    ///   0x74, 0x70, 0x73, 0x5f, 0x6d, 0x69, 0x6e, 0x69, 0x63, 0x62, 0x6f, 0x72]);
    /// ```
    #[inline]
    pub fn from_slice(b: &'buf [u8]) -> Self {
        Self {
            decode_buf_iter: RefCell::new(SequenceBuffer::new(b).into_iter()),
        }
    }

    /// Construct an instance of `CBORDecoder` from the CBOR item enclosed within a Tag, allowing
    /// decode within a CBOR Tag using the CBORDecoder API.
    #[inline]
    pub fn from_tag(cbor: CBOR<'buf>, tag_value: &mut u64) -> Result<Self, CBORError> {
        if let CBOR::Tag(tb) = cbor {
            *tag_value = tb.get_tag();
            Ok(Self {
                decode_buf_iter: RefCell::new(tb.into_iter()),
            })
        } else {
            Err(CBORError::ExpectedType("CBOR Map"))
        }
    }

    /// Construct an instance of `CBORDecoder` from a CBOR Array, allowing decoding within a CBOR
    /// Array using the CBORDecoder API.
    ///
    /// The main use-case for this function occurs when dealing with an array which is nested in a
    /// map or other array.
    ///
    /// # Example
    ///
    /// ```
    ///  use tps_minicbor::decoder::{CBORDecoder};
    ///
    ///  let _ = CBORDecoder::from_slice(&[0x82, 0x61, 0x61, 0xa1, 0x61, 0x62, 0x61, 0x63])
    ///             .array(|ab| {
    ///                 assert_eq!(ab.len(), 2);
    ///                 let _ = CBORDecoder::from_array(ab.item(2)?)?
    ///                     .map(|mb| {
    ///                     assert_eq!(mb.lookup::<&str, &str>("b")?, "c");
    ///                     Ok(())
    ///                 });
    ///                 Ok(())
    ///             });
    /// ```
    #[inline]
    pub fn from_array(cbor: CBOR<'buf>) -> Result<Self, CBORError> {
        if let CBOR::Array(ab) = cbor {
            Ok(Self {
                decode_buf_iter: RefCell::new(ab.into_iter()),
            })
        } else {
            Err(CBORError::ExpectedType("CBOR Array"))
        }
    }

    /// Construct an instance of `CBORDecoder` from a CBOR Array, allowing decoding within a CBOR
    /// Map using the CBORDecoder API.
    #[inline]
    pub fn from_map(cbor: CBOR<'buf>) -> Result<Self, CBORError> {
        if let CBOR::Map(mb) = cbor {
            Ok(Self {
                decode_buf_iter: RefCell::new(mb.into_iter()),
            })
        } else {
            Err(CBORError::ExpectedType("CBOR Map"))
        }
    }

    /// Obtain the internal iterator of a `CBORDecoder`
    #[inline]
    pub fn into_inner(&self) -> Ref<DecodeBufIterator> {
        self.decode_buf_iter.borrow()
    }

    /// When decoding maps, arrays and tags, the closures require finalizing to obtain
    /// the correct return type.
    #[inline]
    pub fn finalize(&self) -> Result<(), CBORError> {
        Ok(())
    }

    /// Decode a value from a [`CBORDecoder`] instance.
    ///
    /// The compiler will attempt, if required, to convert the returned value, which depends on the
    /// parsing function called, into the type of `value`, which is mutably borrowed from the
    /// caller.
    ///
    /// # Parameters
    ///
    /// - `parser`: A parsing function which attempts to decode the expected type at the current
    ///   position in the `CBORDecoder` buffer, `self`. For example, if you are expecting an
    ///   integer in the buffer, you could call [`decode_int`].
    /// - `value`: A mutably borrowed value which is updated with the value read from CBOR if:
    ///   (1) the parse succeeded and (2) the parsed value can be converted into the provided type,
    ///   which depends on having a suitable instance of `TryFrom`. The crate provides suitable
    ///   instances for common types.
    ///
    /// # Return
    ///
    /// - **Good case**: `&Self` is returned, i.e. the CBORDecoder is returned with the last value
    ///   parsed, and the internal state pointing to the start of the next CBOR item.
    /// - **Failure case**: A [`CBORError`] instance which provides information about the reason
    ///  for the failure
    ///
    /// # Example
    ///
    /// ```
    /// use tps_minicbor::decoder::{CBORDecoder, decode_tstr};
    ///
    /// let mut result = "deadbeef";
    /// let _decoder = CBORDecoder::from_slice(&[0x73, 0x49, 0x20, 0x6c, 0x6f, 0x76, 0x65, 0x20,
    ///   0x74, 0x70, 0x73, 0x5f, 0x6d, 0x69, 0x6e, 0x69, 0x63, 0x62, 0x6f, 0x72])
    ///   .value(decode_tstr(), &mut result);
    /// assert_eq!(result, "I love tps_minicbor");
    ///
    /// ```
    pub fn value<'t, F, T: Copy, V>(&self, parser: F, value: &'t mut V) -> Result<&Self, CBORError>
    where
        'buf: 't,
        V: TryFrom<T> + Clone,
        F: Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, T>,
    {
        let (it, v) = parser(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        match V::try_from(v) {
            Ok(val) => {
                *value = val;
                Ok(self)
            }
            Err(_) => Err(CBORError::IncompatibleType),
        }
    }

    /// Obtain a [`MapBuf`] from a [`CBORDecoder`] instance, to allow decoding of the map contents.
    ///
    /// The library will attempt to obtain a [`MapBuf`] instance, which will succeed if the current
    /// decoding position in the `CBORDecoder` is the start of a CBOR map.
    ///
    /// The [`MapBuf`] structure provides convenience methods for decoding CBOR items contained
    /// in a map, such as lookup value with key.
    ///
    /// # Parameters
    ///
    /// - `closure`: A `FnOnce` closure which takes a [`MapBuf`] parameter. If the current decoding
    ///   position is at the start of a map, this closure will be called with a [`MapBuf`] instance
    ///   that allows processing of the map in a map-like manner, using the methods of the
    ///   [`MapBuf`] structure.
    ///
    /// # Return
    ///
    /// - **Good case**: `&Self` is returned, i.e. the CBORDecoder is returned with the internal
    ///   state pointing to the start of the CBOR item after the end of the map.
    /// - **Failure case**: A [`CBORError`] instance which provides information about the reason
    ///  for the failure, for example, that the current decoding position is not pointing to a map.
    ///
    /// # Example
    ///
    /// ```
    /// use tps_minicbor::decoder::{CBORDecoder, decode_tstr};
    ///
    /// let _ = CBORDecoder::from_slice(&[0xa2, 0x01, 0x02, 0x03, 0x04])
    ///     .map(|mb| {
    ///        assert_eq!(mb.len(), 2);
    ///        assert_eq!(mb.lookup::<u8, u8>(1)?, 2);
    ///        assert_eq!(mb.lookup::<u8, u8>(3)?, 4);
    ///        Ok(())
    ///     });
    /// ```
    pub fn map<C>(&self, closure: C) -> Result<&Self, CBORError>
    where
        C: FnOnce(MapBuf<'buf>) -> Result<(), CBORError>,
    {
        let (it, mb) = decode_map()(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        closure(mb)?;
        Ok(self)
    }

    /// Obtain an [`ArrayBuf`] from a [`CBORDecoder`] instance, to allow decoding of the array
    /// contents.
    ///
    /// The library will attempt to obtain an [`ArrayBuf`] instance, which will succeed if the
    /// current decoding position in the `CBORDecoder` is the start of a CBOR map.
    ///
    /// The [`ArrayBuf`] structure provides convenience methods for decoding CBOR items stored in
    /// in an array structure, such as lookup by position in the array.
    ///
    /// # Parameters
    ///
    /// - `closure`: A `FnOnce` closure which takes a [`MapBuf`] parameter. If the current decoding
    ///   position is at the start of an array, this closure will be called with an [`ArrayBuf`]
    ///   instance that allows processing of the array in an array-like manner, using the methods
    ///   of the [`ArrayBuf`] structure.
    ///
    /// # Return
    ///
    /// - **Good case**: `&Self` is returned, i.e. the CBORDecoder is returned with the internal
    ///   state pointing to the start of the CBOR item after the end of the array.
    /// - **Failure case**: A [`CBORError`] instance which provides information about the reason
    ///  for the failure, for example, that the current decoding position is not pointing to an
    /// array.
    ///
    /// # Example
    ///
    /// ```
    /// use tps_minicbor::decoder::{CBORDecoder, decode_tstr};
    ///
    /// let _ = CBORDecoder::from_slice(&[
    ///    0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
    ///    0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18,
    ///    0x19,
    /// ])
    ///   .array(|ab| {
    ///     for i in 0..=ab.len() {
    ///        assert_eq!(ab.item::<u8>(i)?, i.clone() as u8 + 1);
    ///     }
    ///     Ok(())
    /// });
    /// ```
    pub fn array<C>(&self, closure: C) -> Result<&Self, CBORError>
    where
        C: FnOnce(ArrayBuf<'buf>) -> Result<(), CBORError>,
    {
        let (it, ab) = decode_array()(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        closure(ab)?;
        Ok(self)
    }

    /// Obtain an [`TagBuf`] from a [`CBORDecoder`] instance, to allow decoding of the tagged
    /// contents.
    ///
    /// The library will attempt to obtain an [`TagBuf`] instance, which will succeed if the
    /// current decoding position in the `CBORDecoder` is the start of a tagged CBOR item.
    ///
    /// The [`TagBuf`] structure provides convenience methods for decoding the CBOR item stored in
    /// in the tagged structure.
    ///
    /// # Parameters
    ///
    /// - `closure`: A `FnOnce` closure which takes a [`TagBuf`] parameter. If the current decoding
    ///   position is at the start of a tagged item, this closure will be called with a [`TagBuf`]
    ///   instance that allows processing of the item.
    ///
    /// # Return
    ///
    /// - **Good case**: `&Self` is returned, i.e. the CBORDecoder is returned with the internal
    ///   state pointing to the start of the CBOR item after the tagged item.
    /// - **Failure case**: A [`CBORError`] instance which provides information about the reason
    ///  for the failure, for example, that the current decoding position is not pointing to an
    /// array.
    ///
    /// # Example
    ///
    /// ```
    /// use tps_minicbor::decoder::{CBORDecoder, decode_tstr};
    ///
    /// let _ = CBORDecoder::from_slice(&[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0])
    ///     .tag(|tb| {
    ///         if tb.get_tag() == 1 {
    ///             let result = tb.item::<u64>().unwrap();
    ///             assert_eq!(result, 1363896240);
    ///         } else {
    ///             assert!(false)
    ///         }
    ///         Ok(())
    ///     });
    /// ```
    pub fn tag<C>(&self, closure: C) -> Result<&Self, CBORError>
    where
        C: FnOnce(TagBuf<'buf>) -> Result<(), CBORError>,
    {
        let (it, tb) = decode_tag()(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        closure(tb)?;
        Ok(self)
    }

    /// Run `parser` over the next item in the iterator. If it completes successfully, run
    /// `closure` using the result obtained. This allows some result to be built up from
    /// parsing.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn decode_with<F, C>(&'buf self, parser: F, mut closure: C) -> Result<&'buf Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(CBOR<'buf>) -> Result<(), CBORError>,
    {
        let (it, cbor) = parser(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        closure(cbor)?;
        Ok(self)
    }

    /// Optionally run `parser` over the next item in the iterator. If parsing is successful,
    /// run `closure` using the result obtained. If parsing is unsuccessful, continue with the
    /// iterator state unchanged.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn opt<F, C>(&self, parser: F, closure: C) -> Result<&Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: Fn(CBOR<'buf>) -> Result<(), CBORError>,
    {
        let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        if let Some(cbor) = opt_cbor {
            closure(cbor)?;
        }
        Ok(self)
    }

    /// Run `parser` over the next item in the iterator. If it completes successfully, do nothing.
    /// If the parse fails, an error value will be returned.
    #[inline]
    pub fn ignore<F, C>(&self, parser: F) -> Result<&Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
    {
        let (it, _cbor) = parser(self.decode_buf_iter.borrow().clone())?;
        self.decode_buf_iter.replace(it);
        Ok(self)
    }

    /// Run `parser` if `condition` is true. If parsing runs and is successful,
    /// run `closure` using the result obtained.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn cond<F, C>(&self, condition: bool, parser: F, closure: C) -> Result<&Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: Fn(CBOR<'buf>) -> Result<(), CBORError>,
    {
        if condition {
            let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.borrow().clone())?;
            self.decode_buf_iter.replace(it);
            if let Some(cbor) = opt_cbor {
                closure(cbor)?;
            }
        }
        Ok(self)
    }

    /// Run `parser` at least `min` and no more than `max` times. Each time `parser` executes
    /// successfully, `closure` is executed with the result of the parse.
    ///
    /// Note that for the repetitive functions, the iteration number over the parser is passed
    /// as well as the result of the parse.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn range<F, C>(
        &self,
        min: usize,
        max: usize,
        parser: F,
        mut closure: C,
    ) -> Result<&Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(usize, CBOR<'buf>) -> Result<(), CBORError>,
    {
        let mut no_parse = 0;

        loop {
            // Have to borrow parser here because we call many times.
            let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.borrow().clone())?;
            self.decode_buf_iter.replace(it);
            if let Some(cbor) = opt_cbor {
                no_parse += 1;
                closure(no_parse, cbor)?;
            } else {
                // Parse failed, but this is not necessarily an error
                if no_parse < min && min != 0 {
                    // Case 1: Failure: we have not parsed min no of times and min != 0
                    return Err(CBORError::RangeUnderflow(no_parse));
                } else {
                    // Case 2: Success: we have parsed the minimum number of times or min == 0
                    return Ok(self);
                }
            }
            if no_parse == max {
                // Case 3: we have parsed the maximum number of times
                return Ok(self);
            }
        }
    }

    /// Execute `parser` zero or more times, calling `closure` each time `parser` executes
    /// successfully.
    ///
    /// The `closure` function takes a `usize` for the iteration number and a `cbor` for the
    /// result of the parse.
    pub fn many0<F, C>(&self, parser: F, closure: C) -> Result<&Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(usize, CBOR<'buf>) -> Result<(), CBORError>,
    {
        self.range(0, usize::MAX, parser, closure)
    }
}

/***************************************************************************************************
 * CBOR decoding helpers
 **************************************************************************************************/

/// The CBOR parsing monad. Design is very similar to implementation of `Parser` in the `nom` crate.
///
/// `DecodeParser` provides functions to manipulate parsers in various useful ways.
pub trait DecodeParser<'buf, O> {
    /// Parse monad: start with an input type and return `Result` containing (remaining input,
    /// output) or an error.
    ///
    /// Instance must be provided.
    fn parse(&self, input: DecodeBufIterator<'buf>) -> DCPResult<'buf, O>;

    /// Map a function over the result of a parser.
    fn map<F2, O2>(self, f2: F2) -> Map<Self, F2, O>
    where
        F2: Fn(O) -> O2,
        Self: core::marker::Sized,
    {
        Map {
            f1: self,
            f2,
            phantom: core::marker::PhantomData,
        }
    }

    /// Create a second parser from the output of the first and apply this to remaining input
    fn flat_map<F1, F2, O2>(self, f2: F2) -> FlatMap<Self, F2, O>
    where
        F1: Fn(O) -> F2,
        F2: DecodeParser<'buf, O2>,
        Self: core::marker::Sized,
    {
        FlatMap {
            f1: self,
            f2,
            phantom: core::marker::PhantomData,
        }
    }

    /// Apply a second parser over the first, returning results as a tuple
    fn and<F2, O2>(self, f2: F2) -> And<Self, F2>
    where
        F2: DecodeParser<'buf, O2>,
        Self: core::marker::Sized,
    {
        And { f1: self, f2 }
    }

    fn or<F2>(self, f2: F2) -> Or<Self, F2>
    where
        F2: DecodeParser<'buf, O>,
        Self: core::marker::Sized,
    {
        Or { f1: self, f2 }
    }

    fn into<O2: From<O>>(self) -> Into<Self, O, O2>
    where
        Self: core::marker::Sized,
    {
        Into {
            f: self,
            phantom_o1: core::marker::PhantomData,
            phantom_o2: core::marker::PhantomData,
        }
    }
}

/// This instance of `DecodeParser` allows any function `F` of type
/// `Fn(DecodeBufIterator) -> DCPResult<O>' to be used as a `DecodeParser` instance, which
/// simplifies the definition of all of the simple parsers.
impl<'buf, O, F> DecodeParser<'buf, O> for F
where
    F: Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, O>,
{
    fn parse(&self, i: DecodeBufIterator<'buf>) -> DCPResult<'buf, O> {
        self(i)
    }
}

/// Helper structure for `DecodeParser::map`.
pub struct Map<F1, F2, O1> {
    f1: F1,
    f2: F2,
    phantom: core::marker::PhantomData<O1>,
}

impl<'buf, O1, O2, F1: DecodeParser<'buf, O1>, F2: Fn(O1) -> O2> DecodeParser<'buf, O2>
    for Map<F1, F2, O1>
{
    /// Parse over a `Map` structure.
    fn parse(&self, i: DecodeBufIterator<'buf>) -> DCPResult<'buf, O2> {
        match self.f1.parse(i) {
            Ok((i2, o1)) => Ok((i2, (self.f2)(o1))),
            Err(e) => Err(e),
        }
    }
}

/// Helper structure for `DecodeParser::flat_map`.
pub struct FlatMap<F1, F2, O1> {
    f1: F1,
    f2: F2,
    phantom: core::marker::PhantomData<O1>,
}

impl<'buf, O1, O2, F1: DecodeParser<'buf, O1>, F2: Fn(O1) -> P2, P2: DecodeParser<'buf, O2>>
    DecodeParser<'buf, O2> for FlatMap<F1, F2, O1>
{
    /// Parse over a `FlatMap` structure
    fn parse(&self, i1: DecodeBufIterator<'buf>) -> DCPResult<'buf, O2> {
        let (i2, o1) = self.f1.parse(i1)?;
        (self.f2)(o1).parse(i2)
    }
}

/// Helper structure for `DecodeParser::and`.
pub struct And<F1, F2> {
    f1: F1,
    f2: F2,
}

impl<'buf, O1, O2, F1: DecodeParser<'buf, O1>, F2: DecodeParser<'buf, O2>>
    DecodeParser<'buf, (O1, O2)> for And<F1, F2>
{
    /// Parse over `And` structure
    fn parse(&self, i1: DecodeBufIterator<'buf>) -> DCPResult<'buf, (O1, O2)> {
        let (i2, o1) = self.f1.parse(i1)?;
        let (i3, o2) = self.f2.parse(i2)?;
        Ok((i3, (o1, o2)))
    }
}

/// Helper structure for `DecodeParser::or`
pub struct Or<F1, F2> {
    f1: F1,
    f2: F2,
}

impl<'buf, O, F1: DecodeParser<'buf, O>, F2: DecodeParser<'buf, O>> DecodeParser<'buf, O>
    for Or<F1, F2>
{
    /// Parse over `Or` structure.
    fn parse(&self, i: DecodeBufIterator<'buf>) -> DCPResult<'buf, O> {
        match self.f1.parse(i.clone()) {
            Err(_e1) => match self.f2.parse(i) {
                Err(e2) => Err(e2),
                res => res,
            },
            res => res,
        }
    }
}

/// Helper structure for `DecodeParser::into`.
pub struct Into<F, O1, O2: From<O1>> {
    f: F,
    phantom_o1: core::marker::PhantomData<O1>,
    phantom_o2: core::marker::PhantomData<O2>,
}

impl<'buf, O1, O2: From<O1>, F: DecodeParser<'buf, O1>> DecodeParser<'buf, O2> for Into<F, O1, O2> {
    /// Parse over `Into` structure.
    fn parse(&self, i: DecodeBufIterator<'buf>) -> DCPResult<'buf, O2> {
        match self.f.parse(i) {
            Ok((i2, o)) => Ok((i2, o.into())),
            Err(e) => Err(e),
        }
    }
}

/***************************************************************************************************
 * CBOR decoding combinators
 **************************************************************************************************/

/// Match a CBOR positive integer
pub fn is_uint<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::UInt(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("uint")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR positive integer
pub fn is_nint<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::NInt(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("uint")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR bytestring
pub fn is_bstr<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Bstr(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("bstr")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR text string
pub fn is_tstr<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Tstr(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("tstr")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `simple` value
pub fn is_simple<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Simple(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("simple")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `Array` value
pub fn is_array<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Array(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("array")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `Map` value
pub fn is_map<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Map(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("map")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `true` value
pub fn is_true<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::True) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("true")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `false` value
pub fn is_false<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::False) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("false")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `false` value
pub fn is_null<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Null) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("null")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR `false` value
pub fn is_undefined<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Undefined) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("undefined")),
            None => Err(CBORError::EndOfBuffer)
        }
    }
}

/// Match a CBOR integer
pub fn is_int<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |iter| DecodeParser::or(is_uint(), is_nint()).parse(iter)
}

/// Match a CBOR boolean value
pub fn is_bool<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |iter| match decode_bool()(iter)? {
        (it, false) => Ok((it, CBOR::False)),
        (it, true) => Ok((it, CBOR::True)),
    }
}

/// Match any CBOR type
pub fn is_any<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(v) => Ok((iter, v)),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR tagged value
pub fn is_tag<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Tag(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("tag")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR tag with a specific value
pub fn is_tag_with_value<'buf>(v: u64) -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Tag(_)) => {
                if let CBOR::Tag(tb) = cbor {
                    if tb.get_tag() == v {
                        Ok((iter, cbor))
                    } else {
                        Err(CBORError::ExpectedTag(v))
                    }
                } else {
                    Err(CBORError::ExpectedTag(v))
                }
            }
            Some(_) => Err(CBORError::ExpectedType("tag")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR tag with a CBOR date_time
#[cfg_attr(feature = "trace", trace)]
#[cfg(feature = "full")]
pub fn is_date_time<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    is_tag_helper(0, |iter: DecodeBufIterator| {
        if let (_, CBOR::Tstr(date_time)) = is_tstr()(iter)? {
            match chrono::DateTime::parse_from_rfc3339(date_time) {
                Ok(dt) => Ok(CBOR::DateTime(dt)),
                _ => Err(CBORError::BadDateTime),
            }
        } else {
            Err(CBORError::ExpectedType("tstr"))
        }
    })
}

/// Match a CBOR tag with a CBOR epoch
#[cfg_attr(feature = "trace", trace)]
#[cfg(feature = "full")]
pub fn is_epoch<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    use core::convert::TryInto;

    is_tag_helper(1, |iter| {
        let (_, cbor) = is_any()(iter)?;
        match cbor {
            CBOR::UInt(_) | CBOR::NInt(_) => Ok(CBOR::Epoch(cbor.try_into()?)),
            _ => Err(CBORError::ExpectedType("uint/nint")),
        }
    })
}

#[cfg_attr(feature = "trace", trace)]
#[cfg(feature = "full")]
fn is_tag_helper<'buf, F>(tag: u64, f: F) -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>
where
    F: Fn(DecodeBufIterator<'buf>) -> Result<CBOR<'buf>, CBORError>,
{
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Tag(tb)) => {
                if tb.get_tag() == tag {
                    Ok((iter, f(tb.into_iter())?))
                } else {
                    Err(CBORError::ExpectedTag(tag))
                }
            }
            Some(_) => Err(CBORError::ExpectedType("tag")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match the end of the CBOR decode buffer
pub fn is_eof<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(_) => Err(CBORError::EofExpected),
            None => Ok((iter, CBOR::Eof)),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Decoders start here
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Decode a CBOR positive integer
pub fn decode_uint<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, i128> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::UInt(v)) => Ok((iter, v as i128)),
            Some(_) => Err(CBORError::ExpectedType("uint")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR negative integer
pub fn decode_nint<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, i128> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::NInt(v)) => Ok((iter, -1 - (v as i128))),
            Some(_) => Err(CBORError::ExpectedType("nint")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR integer
pub fn decode_int<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, i128> {
    move |iter| DecodeParser::or(decode_uint(), decode_nint()).parse(iter)
}

/// Decode a CBOR bytestring
pub fn decode_bstr<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, &[u8]> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Bstr(bs)) => Ok((iter, bs)),
            Some(_) => Err(CBORError::ExpectedType("bstr")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR bytestring
pub fn decode_tstr<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, &str> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Tstr(ts)) => Ok((iter, ts)),
            Some(_) => Err(CBORError::ExpectedType("tstr")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR `bool` value
pub fn decode_bool<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, bool> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::True) => Ok((iter, true)),
            Some(CBOR::False) => Ok((iter, false)),
            Some(_) => Err(CBORError::ExpectedType("bool")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR `null` value
pub fn decode_null<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, CBOR> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Null) => Ok((iter, CBOR::Null)),
            Some(_) => Err(CBORError::ExpectedType("null")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR `null` value
pub fn decode_undefined<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, CBOR> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Undefined) => Ok((iter, CBOR::Undefined)),
            Some(_) => Err(CBORError::ExpectedType("undefined")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR `null` value
pub fn decode_simple<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, u8> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Simple(v)) => Ok((iter, v)),
            Some(_) => Err(CBORError::ExpectedType("simple")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR array
pub fn decode_array<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, ArrayBuf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Array(ab)) => Ok((iter, ab)),
            Some(_) => Err(CBORError::ExpectedType("array")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR map
pub fn decode_map<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, MapBuf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Map(mb)) => Ok((iter, mb)),
            Some(_) => Err(CBORError::ExpectedType("map")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Decode a CBOR tag
pub fn decode_tag<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, TagBuf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(CBOR::Tag(tb)) => Ok((iter, tb)),
            Some(_) => Err(CBORError::ExpectedType("tag")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/***************************************************************************************************
 * Generic combinators (combinators over DecodeParser)
 **************************************************************************************************/

/// Conditionally execute a parser, returning the result in an `Option<CBOR>`
pub fn cond<'buf, O, F>(
    b: bool,
    f: F,
) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, Option<O>>
where
    F: DecodeParser<'buf, O>,
{
    move |input| {
        if b {
            match f.parse(input) {
                Ok((i, o)) => Ok((i, Some(o))),
                Err(e) => Err(e),
            }
        } else {
            Ok((input, None))
        }
    }
}

// Continue to match a rule until the provided mutable slice is filled.

/// Optionally match a rule, returning result in `Option<CBOR>`.
pub fn opt<'buf, O, F>(f: F) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, Option<O>>
where
    F: DecodeParser<'buf, O>,
{
    move |i| match f.parse(i.clone()) {
        Ok((i, o)) => Ok((i, Some(o))),
        Err(_) => Ok((i, None)),
    }
}

/// Match one of two rules, returning result in `Option<CBOR>`.
pub fn or<'buf, O, F>(
    f1: F,
    f2: F,
) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, Option<O>>
where
    F: DecodeParser<'buf, O>,
{
    move |i| {
        let i1 = i.clone();
        let i2 = i.clone();

        let r1 = f1.parse(i1);
        let r2 = f2.parse(i2);
        match (r1, r2) {
            (Ok((it1, o1)), _) => Ok((it1, Some(o1))),
            (_, Ok((it2, o2))) => Ok((it2, Some(o2))),
            (_, _) => Err(CBORError::FailedPredicate),
        }
    }
}

/// If a rule succeeds, apply a verification function to its output. The verification function
/// should return `true` if verification succeeded.
pub fn with_pred<'buf, O, F>(
    f: F,
    g: impl Fn(&O) -> bool,
) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, O>
where
    F: DecodeParser<'buf, O>,
{
    move |i| {
        let (i, o) = f.parse(i)?;
        if g(&o) {
            Ok((i, o))
        } else {
            Err(CBORError::FailedPredicate)
        }
    }
}

/// If a rule succeeds, unconditionally apply a function to its output and continue.
///
/// This combinator is particularly useful where side-effects from parsing success are desired.
pub fn apply<'buf, O, F>(
    f: F,
    g: impl Fn(&O) -> (),
) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, O>
where
    F: DecodeParser<'buf, O>,
{
    move |i| {
        let (i, o) = f.parse(i)?;
        g(&o);
        Ok((i, o))
    }
}

/// Succeeds if a value is matched.
pub fn with_value<'buf, O: PartialEq, F>(
    f: F,
    v: O,
) -> impl Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, O>
where
    F: DecodeParser<'buf, O>,
{
    move |i| {
        let (i, o) = f.parse(i)?;
        if o == v {
            Ok((i, o))
        } else {
            Err(CBORError::FailedPredicate)
        }
    }
}
