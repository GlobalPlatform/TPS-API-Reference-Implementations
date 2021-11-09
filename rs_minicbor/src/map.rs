/***************************************************************************************************
 * Copyright (c) 2021 Jeremy O'Donoghue. All rights reserved.
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
 * rs_minicbor CBOR Map deserialser API
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use crate::ast::CBOR;
use crate::decode::{DecodeBufIterator, DecodeBufIteratorSource};
use crate::error::CBORError;

#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/// A buffer which contains a CBOR Map to be decoded. The buffer has lifetime `'buf`,
/// which must be longer than any borrow from the buffer itself. This is generally used to represent
/// a CBOR map with an exposed map-like API.
///
/// This CBOR buffer implementation does not support indefinite length items.
#[derive(PartialEq, Debug, Copy, Clone)]
pub struct MapBuf<'buf> {
    bytes: &'buf [u8],
    n_pairs: usize,
}

impl<'buf> MapBuf<'buf> {
    /// Construct a new instance of `ArrayBuf` with all context initialized.
    #[cfg_attr(feature = "trace", trace)]
    pub fn new(init: &'buf [u8], n_pairs: usize) -> MapBuf<'buf> {
        MapBuf {
            bytes: init,
            n_pairs,
        }
    }

    /// Return the number of item pairs in the `MapBuf`.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn len(self) -> usize {
        self.n_pairs
    }

    /// Return `true` if `MapBuf` is empty.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn is_empty(self) -> bool {
        self.n_pairs == 0 && self.bytes.len() == 0
    }

    /// Return `true` if `MapBuf` contains the provided key
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn contains_key(self, key: &CBOR) -> bool {
        match self.find_key_with_value(key) {
            Ok((_, _)) => true,
            _ => false,
        }
    }

    /// Return the value corresponding to key.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get(self, key: &CBOR) -> Option<CBOR<'buf>> {
        match self.find_key_with_value(key) {
            Ok((_, value)) => value,
            _ => None,
        }
    }

    /// Return the value corresponding to an integer key.
    ///
    /// In general, integers and strings are the recommended types to be used for map keys, so it
    /// makes sense to simplify this use-case.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_int(self, v: i64) -> Option<CBOR<'buf>> {
        self.get(&CBOR::from_i64(v))
    }

    /// Return the value corresponding to an integer key.
    ///
    /// In general, integers and strings are the recommended types to be used for map keys, so it
    /// makes sense to simplify this use-case.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_tstr(self, v: &str) -> Option<CBOR<'buf>> {
        self.get(&CBOR::from_str(v))
    }

    /// Return value corresponding to a map item that can have either an integer or a string
    /// key. This is a common use-case in IETF standards where human readability vs compactness
    /// tradeoff is supported.
    ///
    /// In general, integers and strings are the recommended types to be used for map keys, so it
    /// makes sense to simplify this use-case.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_int_or_tstr(self, v: i64, s: &str) -> Option<CBOR<'buf>> {
        if let Some(cbor) = self.get_int(v) {
            Some(cbor)
        } else {
            self.get_tstr(s)
        }
    }

    /// Return the key, value pair corresponding to key.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_key_value(self, key: &CBOR) -> Option<(CBOR<'buf>, CBOR<'buf>)> {
        match self.find_key_with_value(key) {
            Ok((found_key, Some(found_value))) => Some((found_key, found_value)),
            _ => None,
        }
    }

    /// Return the (key, value) pair corresponding to an integer used as a key
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_int_key_value(self, key: i64) -> Option<(CBOR<'buf>, CBOR<'buf>)> {
        self.get_key_value(&CBOR::from_i64(key))
    }

    /// Return the (key, value) pair corresponding to an tstr used as a key
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_tstr_key_value(self, key: &str) -> Option<(CBOR<'buf>, CBOR<'buf>)> {
        self.get_key_value(&CBOR::from_str(key))
    }

    /// Return (key, value) pair  corresponding to a map item that can have either an integer or a
    /// string key. This is a common use-case in IETF standards where human readability vs
    /// compactness tradeoff is supported.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn get_int_or_tstr_key_value(self, v: i64, s: &str) -> Option<(CBOR<'buf>, CBOR<'buf>)> {
        if let Some(pair) = self.get_int_key_value(v) {
            Some(pair)
        } else {
            self.get_tstr_key_value(s)
        }
    }

    /// (private) If there is a key matching `search_key`, return the
    /// key and corresponding value, otherwise return a `KeyNotPresent` error.
    #[cfg_attr(feature = "trace", trace)]
    fn find_key_with_value(
        self,
        search_key: &CBOR,
    ) -> Result<(CBOR<'buf>, Option<CBOR<'buf>>), CBORError> {
        let mut it: DecodeBufIterator<'buf> = self.into_iter();
        let mut current_key = it.next();
        while current_key.is_some() {
            if let Some(item_key) = current_key {
                if item_key == *search_key {
                    return Ok((item_key, it.next()));
                }
                let _ = it.next(); // skip the next value as it doesn't match key
                current_key = it.next(); // This one is a key again
            }
        }
        return Err(CBORError::KeyNotPresent);
    }
}

impl<'buf> IntoIterator for MapBuf<'buf> {
    type Item = CBOR<'buf>;
    type IntoIter = DecodeBufIterator<'buf>;

    /// Construct an Iterator adapter from a `DecodeBuf`.
    #[cfg_attr(feature = "trace", trace)]
    fn into_iter(self) -> Self::IntoIter {
        DecodeBufIterator {
            buf: self.bytes,
            index: 0,
            source: DecodeBufIteratorSource::Map,
        }
    }
}
