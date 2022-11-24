/***************************************************************************************************
 * Copyright (c) 2021 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * rs_minicbor CBOR utilities API
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use crate::constants::allow;
#[cfg(feature = "trace")]
use func_trace::trace;

use crate::ast::CBOR;
use crate::error::CBORError;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/// Return `true` if it is possible to obtain a slice of length `len` starting from `start` from
/// `buf`
#[cfg_attr(feature = "trace", trace)]
#[inline]
pub fn within(buf: &[u8], start: usize, len: usize) -> bool {
    start + len <= buf.len()
}

#[cfg(feature = "combinators")]
#[derive(Debug, Copy, Clone)]
pub struct Allowable(u32);

#[cfg(all(feature = "combinators", feature = "float"))]
impl Allowable {
    pub fn new(v: u32) -> Self {
        Allowable(v)
    }

    pub fn allow_none(&self) -> bool {
        self.0 & allow::NONE != 0
    }

    pub fn allow_uint(&self) -> bool {
        self.0 & allow::UINT != 0
    }

    pub fn allow_nint(&self) -> bool {
        self.0 & allow::NINT != 0
    }

    pub fn allow_bstr(&self) -> bool {
        self.0 & allow::BSTR != 0
    }

    pub fn allow_tstr(&self) -> bool {
        self.0 & allow::TSTR != 0
    }

    pub fn allow_array(&self) -> bool {
        self.0 & allow::ARRAY != 0
    }

    pub fn allow_map(&self) -> bool {
        self.0 & allow::MAP != 0
    }

    pub fn allow_tag(&self) -> bool {
        self.0 & allow::TAG != 0
    }

    pub fn allow_simple(&self) -> bool {
        self.0 & allow::SIMPLE != 0
    }

    pub fn allow_float(&self) -> bool {
        self.0 & allow::FLOAT != 0
    }
}

#[cfg(all(feature = "combinators", not(feature = "float")))]
impl Allowable {
    pub fn new(v: u32) -> Self {
        Allowable(v)
    }

    pub fn allow_none(&self) -> bool {
        self.0 & allow::NONE != 0
    }

    pub fn allow_uint(&self) -> bool {
        self.0 & allow::UINT != 0
    }

    pub fn allow_nint(&self) -> bool {
        self.0 & allow::NINT != 0
    }

    pub fn allow_bstr(&self) -> bool {
        self.0 & allow::BSTR != 0
    }

    pub fn allow_tstr(&self) -> bool {
        self.0 & allow::TSTR != 0
    }

    pub fn allow_array(&self) -> bool {
        self.0 & allow::ARRAY != 0
    }

    pub fn allow_map(&self) -> bool {
        self.0 & allow::MAP != 0
    }

    pub fn allow_tag(&self) -> bool {
        self.0 & allow::TAG != 0
    }

    pub fn allow_simple(&self) -> bool {
        self.0 & allow::SIMPLE != 0
    }

    pub fn allow_float(&self) -> bool {
        self.0 & allow::FLOAT != 0
    }
}
#[cfg(feature = "combinators")]
pub trait Filter {
    type Error;

    fn allow(self, allow: Allowable) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

#[cfg(feature = "combinators")]
impl<'buf> Filter for Option<CBOR<'buf>> {
    type Error = CBORError;

    fn allow(self, allow: Allowable) -> Result<Option<CBOR<'buf>>, Self::Error> {
        match self {
            Some(cbor) => {
                let result = cbor.allow(allow);
                if result.is_ok() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            None => {
                if allow.allow_none() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
        }
    }
}

#[cfg(all(feature = "combinators", feature = "float"))]
impl<'buf> Filter for CBOR<'buf> {
    type Error = CBORError;

    fn allow(self, allow: Allowable) -> Result<CBOR<'buf>, Self::Error> {
        match self {
            CBOR::UInt(_) => {
                if allow.allow_uint() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::NInt(_) => {
                if allow.allow_nint() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Bstr(_) => {
                if allow.allow_bstr() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Tstr(_) => {
                if allow.allow_tstr() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Array(_) => {
                if allow.allow_array() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Map(_) => {
                if allow.allow_map() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Tag(_) => {
                if allow.allow_tag() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Simple(_) | CBOR::True | CBOR::False | CBOR::Null | CBOR::Undefined => {
                if allow.allow_simple() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Float64(_) | CBOR::Float32(_) | CBOR::Float16(_) => {
                if allow.allow_float() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            _ => Err(CBORError::NotAllowed),
        }
    }
}

#[cfg(all(feature = "combinators", not(feature = "float")))]
impl<'buf> Filter for CBOR<'buf> {
    type Error = CBORError;

    fn allow(self, allow: Allowable) -> Result<CBOR<'buf>, Self::Error> {
        match self {
            CBOR::UInt(_) => {
                if allow.allow_uint() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::NInt(_) => {
                if allow.allow_nint() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Bstr(_) => {
                if allow.allow_bstr() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Tstr(_) => {
                if allow.allow_tstr() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Array(_) => {
                if allow.allow_array() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Map(_) => {
                if allow.allow_map() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Tag(_) => {
                if allow.allow_tag() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            CBOR::Simple(_) | CBOR::True | CBOR::False | CBOR::Null | CBOR::Undefined => {
                if allow.allow_simple() {
                    Ok(self)
                } else {
                    Err(CBORError::NotAllowed)
                }
            }
            _ => Err(CBORError::NotAllowed),
        }
    }
}
