/***************************************************************************************************
 * Copyright (c) 2021, 2022, Qualcomm Innovation Center, Inc. All rights reserved.
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
 * CBOR Decoder Combinators
 *
 * A fairly comprehensive, memory efficient, deserializer for CBOR (RFC7049). This deserializer
 * is designed for use in constrained systems and requires neither the Rust standard library
 * nor an allocator.
 *
 * The decode combinators do make use of dynamic dispatch and have some memory penalty in return
 * for a more comfortable API. They can be disabled using the `embedded` build feature.
 **************************************************************************************************/
use crate::ast::CBOR;
use crate::decode::{DecodeBufIterator, SequenceBuffer};
use crate::error::CBORError;

use std::convert::From;

/// Alias for the Result type for all CBOR decode combinators.
type DCResult<'buf> = core::result::Result<(DecodeBufIterator<'buf>, CBOR<'buf>), CBORError>;
/// Alias for the Result type where the output type, `O`, is generic.
type DCPResult<'buf, O> = core::result::Result<(DecodeBufIterator<'buf>, O), CBORError>;

/***************************************************************************************************
 * Top Level Decoder API
 **************************************************************************************************/

#[cfg(feature = "combinators")]
pub struct CBORDecoder<'buf> {
    decode_buf_iter: DecodeBufIterator<'buf>,
}

#[cfg(feature = "combinators")]
impl<'buf> CBORDecoder<'buf> {
    /// Construct a new instance of a `CBORDecoder` from a `SequenceBuffer`.
    #[inline]
    pub fn new(b: SequenceBuffer<'buf>) -> Self {
        Self {
            decode_buf_iter: b.into_iter(),
        }
    }

    /// Construct a new instance of a `CBORDecoder` from a `SequenceBuffer`.
    #[inline]
    pub fn from_slice(b: &'buf [u8]) -> Self {
        Self {
            decode_buf_iter: SequenceBuffer::new(b).into_iter(),
        }
    }

    /// Construct an instance of `CBORDecoder` from the CBOR item enclosed within a Tag, allowing
    /// decode within a CBOR Tag using the CBORDecoder API.
    #[inline]
    pub fn from_tag(cbor: CBOR<'buf>, tag_value: &mut u64) -> Result<Self, CBORError> {
        if let CBOR::Tag(tb) = cbor {
            *tag_value = tb.get_tag();
            Ok(Self {
                decode_buf_iter: tb.into_iter(),
            })
        } else {
            Err(CBORError::ExpectedType("CBOR Map"))
        }
    }

    /// Construct an instance of `CBORDecoder` from a CBOR Array, allowing decoding within a CBOR
    /// Array using the CBORDecoder API.
    #[inline]
    pub fn from_array(cbor: CBOR<'buf>) -> Result<Self, CBORError> {
        if let CBOR::Array(ab) = cbor {
            Ok(Self {
                decode_buf_iter: ab.into_iter(),
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
                decode_buf_iter: mb.into_iter(),
            })
        } else {
            Err(CBORError::ExpectedType("CBOR Map"))
        }
    }

    /// Obtain the internal iterator of a `CBORDecoder`
    #[inline]
    pub fn into_inner(&self) -> DecodeBufIterator {
        self.decode_buf_iter
    }

    /// When decoding maps, arrays and tags, the closures require finalizing to obtain
    /// the correct return type.
    #[inline]
    pub fn finalize(&self) -> Result<(), CBORError> {
        Ok(())
    }

    /// Run `parser` over the next item in the iterator. If it completes successfully, run
    /// `closure` using the result obtained. This allows some result to be built up from
    /// parsing.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn decode_with<F, C>(
        &'buf mut self,
        parser: F,
        mut closure: C,
    ) -> Result<&'buf mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(CBOR<'buf>) -> Result<(), CBORError>,
    {
        let (it, cbor) = parser(self.decode_buf_iter.clone())?;
        self.decode_buf_iter = it;
        closure(cbor)?;
        Ok(self)
    }

    /// Optionally run `parser` over the next item in the iterator. If parsing is successful,
    /// run `closure` using the result obtained. If parsing is unsuccessful, continue with the
    /// iterator state unchanged.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn opt<F, C>(&mut self, parser: F, closure: C) -> Result<&mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: Fn(CBOR<'buf>) -> Result<(), CBORError>,
    {
        let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.clone())?;
        self.decode_buf_iter = it;
        if let Some(cbor) = opt_cbor {
            closure(cbor)?;
        }
        Ok(self)
    }

    /// Run `parser` over the next item in the iterator. If it completes successfully, do nothing.
    /// If the parse fails, an error value will be returned.
    #[inline]
    pub fn ignore<F, C>(&mut self, parser: F) -> Result<&mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
    {
        let (it, _cbor) = parser(self.decode_buf_iter.clone())?;
        self.decode_buf_iter = it;
        Ok(self)
    }

    /// Run `parser` if `condition` is true. If parsing runs and is successful,
    /// run `closure` using the result obtained.
    ///
    /// TODO: currently the lifetime management does not allow assignment of references to `self`
    /// within the `closure`.
    pub fn cond<F, C>(
        &mut self,
        condition: bool,
        parser: F,
        closure: C,
    ) -> Result<&mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: Fn(CBOR<'buf>) -> Result<(), CBORError>,
    {
        if condition {
            let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.clone())?;
            self.decode_buf_iter = it;
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
        &mut self,
        min: usize,
        max: usize,
        parser: F,
        mut closure: C,
    ) -> Result<&mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(usize, CBOR<'buf>) -> Result<(), CBORError>,
    {
        let mut no_parse = 0;

        loop {
            // Have to borrow parser here because we call many times.
            let (it, opt_cbor) = opt(&parser)(self.decode_buf_iter.clone())?;
            self.decode_buf_iter = it;
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
    pub fn many0<F, C>(&mut self, parser: F, closure: C) -> Result<&mut Self, CBORError>
    where
        F: Fn(DecodeBufIterator<'buf>) -> DCResult<'buf>,
        C: FnMut(usize, CBOR<'buf>) -> Result<(), CBORError>
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
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
impl<'buf, O, F> DecodeParser<'buf, O> for F
where
    F: Fn(DecodeBufIterator<'buf>) -> DCPResult<'buf, O>,
{
    fn parse(&self, i: DecodeBufIterator<'buf>) -> DCPResult<'buf, O> {
        self(i)
    }
}

/// Helper structure for `DecodeParser::map`.
#[cfg(feature = "combinators")]
pub struct Map<F1, F2, O1> {
    f1: F1,
    f2: F2,
    phantom: core::marker::PhantomData<O1>,
}

#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
pub struct FlatMap<F1, F2, O1> {
    f1: F1,
    f2: F2,
    phantom: core::marker::PhantomData<O1>,
}

#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
pub struct And<F1, F2> {
    f1: F1,
    f2: F2,
}

#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
pub struct Or<F1, F2> {
    f1: F1,
    f2: F2,
}

#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
pub struct Into<F, O1, O2: From<O1>> {
    f: F,
    phantom_o1: core::marker::PhantomData<O1>,
    phantom_o2: core::marker::PhantomData<O2>,
}

#[cfg(feature = "combinators")]
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

/// Match any CBOR type
#[cfg(feature = "combinators")]
pub fn is_any<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(v) => Ok((iter, v)),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match the end of the CBOR decode buffer
#[cfg(feature = "combinators")]
pub fn is_eof<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(_) => Err(CBORError::EofExpected),
            None => Ok((iter, CBOR::Eof)),
        }
    }
}

/// Match a CBOR positive integer
#[cfg(feature = "combinators")]
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

/// Match a CBOR negative integer
#[cfg(feature = "combinators")]
pub fn is_nint<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::NInt(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("nint")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR integer
#[cfg(feature = "combinators")]
pub fn is_int<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |iter| DecodeParser::or(is_uint(), is_int()).parse(iter)
}

/// Match a CBOR bytestring
#[cfg(feature = "combinators")]
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

/// Match a CBOR bytestring
#[cfg(feature = "combinators")]
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

/// Match a CBOR `false` value
#[cfg(feature = "combinators")]
pub fn is_false<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::False) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("false")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `true` value
#[cfg(feature = "combinators")]
pub fn is_true<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::True) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("true")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `bool` value
#[cfg(feature = "combinators")]
pub fn is_bool<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::True) => Ok((iter, cbor)),
            Some(cbor @ CBOR::False) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("bool")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `null` value
#[cfg(feature = "combinators")]
pub fn is_null<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Null) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("null")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `undefined` value
#[cfg(feature = "combinators")]
pub fn is_undefined<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Undefined) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("undefined")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR `simple` value
#[cfg(feature = "combinators")]
pub fn is_simple<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Simple(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("simple")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR array
#[cfg(feature = "combinators")]
pub fn is_array<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Array(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("array")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR map
#[cfg(feature = "combinators")]
pub fn is_map<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    move |mut iter| {
        let item = iter.next();
        match item {
            Some(cbor @ CBOR::Map(_)) => Ok((iter, cbor)),
            Some(_) => Err(CBORError::ExpectedType("map")),
            None => Err(CBORError::EndOfBuffer),
        }
    }
}

/// Match a CBOR tagged value
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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

#[cfg_attr(feature = "trace", trace)]
#[cfg(any(test, all(feature = "combinators", feature = "full")))]
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

#[cfg_attr(feature = "trace", trace)]
#[cfg(any(test, all(feature = "combinators", feature = "full")))]
pub fn is_epoch<'buf>() -> impl Fn(DecodeBufIterator<'buf>) -> DCResult<'buf> {
    is_tag_helper(1, |iter| {
        let (_, cbor) = is_any()(iter)?;
        match cbor {
            CBOR::UInt(_) | CBOR::NInt(_) => Ok(CBOR::Epoch(cbor.try_into_i64()?)),
            _ => Err(CBORError::ExpectedType("uint/nint")),
        }
    })
}

#[cfg_attr(feature = "trace", trace)]
#[cfg(any(test, all(feature = "combinators", feature = "full")))]
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

/***************************************************************************************************
 * Generic combinators (combinators over DecodeParser)
 **************************************************************************************************/

/// Conditionally execute a parser, returning the result in an `Option<CBOR>`
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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
#[cfg(feature = "combinators")]
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
