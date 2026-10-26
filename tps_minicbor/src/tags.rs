/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/// Support for the standard CBOR tags defined in RFC 8949, section 3.4.
pub mod standard_tags {
    use crate::{
        Decode, DecodeError, EncodeError, EncodeItem, Encoder, Item, Sizer, Tag, TaggedItem,
    };

    #[cfg(feature = "float")]
    use crate::Float;
    #[cfg(feature = "float")]
    use half::f16;

    /// Tag number for a standard date/time string (RFC 8949 §3.4.1).
    pub const TAG_DATETIME: u64 = 0;
    /// Tag number for an epoch-based date/time (RFC 8949 §3.4.2).
    pub const TAG_EPOCH: u64 = 1;
    /// Tag number for a positive bignum (RFC 8949 §3.4.3).
    pub const TAG_POS_BIGNUM: u64 = 2;
    /// Tag number for a negative bignum (RFC 8949 §3.4.3).
    pub const TAG_NEG_BIGNUM: u64 = 3;
    /// Tag number for a decimal fraction (RFC 8949 §3.4.4).
    pub const TAG_DECIMAL_FRACTION: u64 = 4;
    /// Tag number for a bigfloat (RFC 8949 §3.4.4).
    pub const TAG_BIGFLOAT: u64 = 5;
    /// Tag number for a value expected to be encoded as base64url (RFC 8949 §3.4.5.2).
    pub const TAG_EXPECTED_BASE64URL: u64 = 21;
    /// Tag number for a value expected to be encoded as base64 (RFC 8949 §3.4.5.2).
    pub const TAG_EXPECTED_BASE64: u64 = 22;
    /// Tag number for a value expected to be encoded as base16 (RFC 8949 §3.4.5.2).
    pub const TAG_EXPECTED_BASE16: u64 = 23;
    /// Tag number for embedded CBOR data (RFC 8949 §3.4.5.1).
    pub const TAG_EMBEDDED_CBOR: u64 = 24;
    /// Tag number for a URI (RFC 8949 §3.4.5.3).
    pub const TAG_URI: u64 = 32;
    /// Tag number for a base64url-encoded string (RFC 8949 §3.4.5.3).
    pub const TAG_BASE64URL: u64 = 33;
    /// Tag number for a base64-encoded string (RFC 8949 §3.4.5.3).
    pub const TAG_BASE64: u64 = 34;
    /// Tag number for a regular expression (RFC 8949 §3.4.5.3).
    pub const TAG_REGEX: u64 = 35;
    /// Tag number for a MIME message (RFC 8949 §3.4.5.3).
    pub const TAG_MIME: u64 = 36;
    /// Tag number for self-described CBOR (RFC 8949 §3.4.6).
    pub const TAG_SELF_DESCRIBED_CBOR: u64 = 55799;

    fn size_tag<T: EncodeItem>(s: &mut Sizer, tag: u64, value: &T) -> Result<(), EncodeError> {
        // Tag header sizes match unsigned integer header sizes.
        s.uint(tag)?;
        value.size(s)
    }

    fn decode_tagged_item(
        tagged: TaggedItem,
        expected_tag: u64,
    ) -> Result<Item, DecodeError> {
        if tagged.tag != expected_tag {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        tagged.item()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A standard date/time string (RFC 8949 §3.4.1, tag 0).
    pub struct DateTime<'a>(pub &'a str);

    impl EncodeItem for DateTime<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_DATETIME, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_DATETIME)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for DateTime<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_DATETIME)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    /// An epoch-based date/time (RFC 8949 §3.4.2, tag 1).
    pub enum EpochTime {
        /// Number of seconds since the POSIX epoch, encoded as an integer.
        Int(i64),
        #[cfg(feature = "float")]
        /// Number of seconds since the POSIX epoch, encoded as a float.
        Float(f64),
    }

    impl EncodeItem for EpochTime {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            match self {
                EpochTime::Int(value) => size_tag(s, TAG_EPOCH, value),
                #[cfg(feature = "float")]
                EpochTime::Float(value) => size_tag(s, TAG_EPOCH, value),
            }
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            match self {
                EpochTime::Int(value) => e.tag(TAG_EPOCH)?.item(*value),
                #[cfg(feature = "float")]
                EpochTime::Float(value) => e.tag(TAG_EPOCH)?.item(*value),
            }
        }
    }

    impl<'a> Decode<'a> for EpochTime {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_EPOCH)?;
            match item {
                Item::Unsigned(value) => {
                    let value =
                        i64::try_from(value).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
                    Ok(EpochTime::Int(value))
                }
                Item::Negative(value) => Ok(EpochTime::Int(value)),
                #[cfg(feature = "float")]
                Item::Float(value) => Ok(EpochTime::Float(float_to_f64(value))),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A positive bignum, encoded as its big-endian byte representation (RFC 8949 §3.4.3, tag 2).
    pub struct PositiveBignum<'a>(pub &'a [u8]);

    impl EncodeItem for PositiveBignum<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_POS_BIGNUM, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_POS_BIGNUM)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for PositiveBignum<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_POS_BIGNUM)? {
                Item::Bytes(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A negative bignum, encoded as the big-endian byte representation of `-1 - n` (RFC 8949
    /// §3.4.3, tag 3).
    pub struct NegativeBignum<'a>(pub &'a [u8]);

    impl EncodeItem for NegativeBignum<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_NEG_BIGNUM, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_NEG_BIGNUM)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for NegativeBignum<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_NEG_BIGNUM)? {
                Item::Bytes(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// An arbitrary-precision integer: either a small value that fits in an `i64`, or a bignum
    /// carried as raw bytes (RFC 8949 §3.4.3, tags 2 and 3).
    pub enum BigInt<'a> {
        /// A value small enough to be represented as a native `i64`.
        Small(i64),
        /// A positive bignum's big-endian byte representation.
        Positive(&'a [u8]),
        /// A negative bignum's big-endian byte representation (of `-1 - n`).
        Negative(&'a [u8]),
    }

    impl EncodeItem for BigInt<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            match self {
                BigInt::Small(value) => value.size(s),
                BigInt::Positive(bytes) => size_tag(s, TAG_POS_BIGNUM, bytes),
                BigInt::Negative(bytes) => size_tag(s, TAG_NEG_BIGNUM, bytes),
            }
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            match self {
                BigInt::Small(value) => value.encode(e),
                BigInt::Positive(bytes) => e.tag(TAG_POS_BIGNUM)?.item(*bytes),
                BigInt::Negative(bytes) => e.tag(TAG_NEG_BIGNUM)?.item(*bytes),
            }
        }
    }

    impl<'a> Decode<'a> for BigInt<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let item = d.next()?;
            bigint_from_item(item)
        }
    }

    fn bigint_from_item(item: Item) -> Result<BigInt, DecodeError> {
        match item {
            Item::Unsigned(value) => {
                let value = i64::try_from(value).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
                Ok(BigInt::Small(value))
            }
            Item::Negative(value) => Ok(BigInt::Small(value)),
            Item::Tag(tagged) => match tagged.tag {
                TAG_POS_BIGNUM => match tagged.item()? {
                    Item::Bytes(bytes) => Ok(BigInt::Positive(bytes)),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_NEG_BIGNUM => match tagged.item()? {
                    Item::Bytes(bytes) => Ok(BigInt::Negative(bytes)),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                _ => Err(DecodeError::InvalidAdditionalInfo),
            },
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A decimal fraction, `mantissa * 10^exponent` (RFC 8949 §3.4.4, tag 4).
    pub struct DecimalFraction<'a> {
        /// The base-10 exponent.
        pub exponent: i64,
        /// The mantissa.
        pub mantissa: BigInt<'a>,
    }

    impl EncodeItem for DecimalFraction<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            let array = Array2(self.exponent, self.mantissa);
            size_tag(s, TAG_DECIMAL_FRACTION, &array)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            let array = Array2(self.exponent, self.mantissa);
            Tag {
                tag: TAG_DECIMAL_FRACTION,
                value: array,
            }
            .encode(e)
        }
    }

    impl<'a> Decode<'a> for DecimalFraction<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_DECIMAL_FRACTION)?;
            decode_decimal_fraction(item)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A bigfloat, `mantissa * 2^exponent` (RFC 8949 §3.4.4, tag 5).
    pub struct BigFloat<'a> {
        /// The base-2 exponent.
        pub exponent: i64,
        /// The mantissa.
        pub mantissa: BigInt<'a>,
    }

    impl EncodeItem for BigFloat<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            let array = Array2(self.exponent, self.mantissa);
            size_tag(s, TAG_BIGFLOAT, &array)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            let array = Array2(self.exponent, self.mantissa);
            Tag {
                tag: TAG_BIGFLOAT,
                value: array,
            }
            .encode(e)
        }
    }

    impl<'a> Decode<'a> for BigFloat<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_BIGFLOAT)?;
            decode_bigfloat(item)
        }
    }

    fn decode_decimal_fraction(item: Item) -> Result<DecimalFraction, DecodeError> {
        let (exponent, mantissa) = decode_exponent_mantissa(item)?;
        Ok(DecimalFraction { exponent, mantissa })
    }

    fn decode_bigfloat(item: Item) -> Result<BigFloat, DecodeError> {
        let (exponent, mantissa) = decode_exponent_mantissa(item)?;
        Ok(BigFloat { exponent, mantissa })
    }

    fn decode_exponent_mantissa(item: Item) -> Result<(i64, BigInt), DecodeError> {
        let Item::Array(array) = item else {
            return Err(DecodeError::InvalidAdditionalInfo);
        };
        if array.len() != 2 {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        let mut iter = array.iter();
        let exponent = match iter.next().transpose()? {
            Some(Item::Unsigned(value)) => {
                i64::try_from(value).map_err(|_| DecodeError::InvalidAdditionalInfo)?
            }
            Some(Item::Negative(value)) => value,
            _ => return Err(DecodeError::InvalidAdditionalInfo),
        };
        let mantissa_item = iter.next().transpose()?;
        if iter.next().is_some() {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        let Some(mantissa_item) = mantissa_item else {
            return Err(DecodeError::InvalidAdditionalInfo);
        };
        let mantissa = bigint_from_item(mantissa_item)?;
        Ok((exponent, mantissa))
    }

    #[derive(Debug, Clone, PartialEq)]
    /// A value tagged as expected to be re-encoded as base64url when converted to text (RFC 8949
    /// §3.4.5.2, tag 21).
    pub struct ExpectedBase64Url<T>(pub T);

    impl<T: EncodeItem> EncodeItem for ExpectedBase64Url<T> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_EXPECTED_BASE64URL, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_EXPECTED_BASE64URL)?.item_ref(&self.0)
        }
    }

    impl<'a> Decode<'a> for ExpectedBase64Url<Item<'a>> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_EXPECTED_BASE64URL)?;
            Ok(Self(item))
        }
    }

    /// Convenience alias for [`ExpectedBase64Url`] wrapping a decoded [`Item`].
    pub type ExpectedBase64UrlItem<'a> = ExpectedBase64Url<Item<'a>>;

    #[derive(Debug, Clone, PartialEq)]
    /// A value tagged as expected to be re-encoded as base64 when converted to text (RFC 8949
    /// §3.4.5.2, tag 22).
    pub struct ExpectedBase64<T>(pub T);

    impl<T: EncodeItem> EncodeItem for ExpectedBase64<T> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_EXPECTED_BASE64, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_EXPECTED_BASE64)?.item_ref(&self.0)
        }
    }

    impl<'a> Decode<'a> for ExpectedBase64<Item<'a>> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_EXPECTED_BASE64)?;
            Ok(Self(item))
        }
    }

    /// Convenience alias for [`ExpectedBase64`] wrapping a decoded [`Item`].
    pub type ExpectedBase64Item<'a> = ExpectedBase64<Item<'a>>;

    #[derive(Debug, Clone, PartialEq)]
    /// A value tagged as expected to be re-encoded as base16 when converted to text (RFC 8949
    /// §3.4.5.2, tag 23).
    pub struct ExpectedBase16<T>(pub T);

    impl<T: EncodeItem> EncodeItem for ExpectedBase16<T> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_EXPECTED_BASE16, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_EXPECTED_BASE16)?.item_ref(&self.0)
        }
    }

    impl<'a> Decode<'a> for ExpectedBase16<Item<'a>> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_EXPECTED_BASE16)?;
            Ok(Self(item))
        }
    }

    /// Convenience alias for [`ExpectedBase16`] wrapping a decoded [`Item`].
    pub type ExpectedBase16Item<'a> = ExpectedBase16<Item<'a>>;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// CBOR data embedded as a byte string (RFC 8949 §3.4.5.1, tag 24).
    pub struct EncodedCbor<'a>(pub &'a [u8]);

    impl EncodeItem for EncodedCbor<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_EMBEDDED_CBOR, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_EMBEDDED_CBOR)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for EncodedCbor<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_EMBEDDED_CBOR)? {
                Item::Bytes(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A URI (RFC 8949 §3.4.5.3, tag 32).
    pub struct Uri<'a>(pub &'a str);

    impl EncodeItem for Uri<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_URI, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_URI)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for Uri<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_URI)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A base64url-encoded string (RFC 8949 §3.4.5.3, tag 33).
    pub struct Base64Url<'a>(pub &'a str);

    impl EncodeItem for Base64Url<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_BASE64URL, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_BASE64URL)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for Base64Url<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_BASE64URL)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A base64-encoded string (RFC 8949 §3.4.5.3, tag 34).
    pub struct Base64<'a>(pub &'a str);

    impl EncodeItem for Base64<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_BASE64, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_BASE64)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for Base64<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_BASE64)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A regular expression, in the syntax defined by ECMA 262 (RFC 8949 §3.4.5.3, tag 35).
    pub struct Regex<'a>(pub &'a str);

    impl EncodeItem for Regex<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_REGEX, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_REGEX)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for Regex<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_REGEX)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// A MIME message, in the syntax defined by RFC 2045 (RFC 8949 §3.4.5.3, tag 36).
    pub struct Mime<'a>(pub &'a str);

    impl EncodeItem for Mime<'_> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_MIME, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_MIME)?.item(self.0)
        }
    }

    impl<'a> Decode<'a> for Mime<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            match decode_tagged_item(tagged, TAG_MIME)? {
                Item::Text(value) => Ok(Self(value)),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    /// A value tagged as self-described CBOR, i.e. prefixed by its own magic number (RFC 8949
    /// §3.4.6, tag 55799).
    pub struct SelfDescribe<T>(pub T);

    impl<T: EncodeItem> EncodeItem for SelfDescribe<T> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            size_tag(s, TAG_SELF_DESCRIBED_CBOR, &self.0)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            e.tag(TAG_SELF_DESCRIBED_CBOR)?.item_ref(&self.0)
        }
    }

    impl<'a> Decode<'a> for SelfDescribe<Item<'a>> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            let item = decode_tagged_item(tagged, TAG_SELF_DESCRIBED_CBOR)?;
            Ok(Self(item))
        }
    }

    /// Convenience alias for [`SelfDescribe`] wrapping a decoded [`Item`].
    pub type SelfDescribeItem<'a> = SelfDescribe<Item<'a>>;

    #[derive(Debug, Clone, PartialEq)]
    /// A decoded standard tag, dispatched to the variant matching its tag number.
    pub enum StandardTag<'a> {
        /// A standard date/time string (tag 0).
        DateTime(DateTime<'a>),
        /// An epoch-based date/time (tag 1).
        Epoch(EpochTime),
        /// A positive bignum (tag 2).
        PositiveBignum(PositiveBignum<'a>),
        /// A negative bignum (tag 3).
        NegativeBignum(NegativeBignum<'a>),
        /// A decimal fraction (tag 4).
        DecimalFraction(DecimalFraction<'a>),
        /// A bigfloat (tag 5).
        BigFloat(BigFloat<'a>),
        /// A value expected to be encoded as base64url (tag 21).
        ExpectedBase64Url(ExpectedBase64UrlItem<'a>),
        /// A value expected to be encoded as base64 (tag 22).
        ExpectedBase64(ExpectedBase64Item<'a>),
        /// A value expected to be encoded as base16 (tag 23).
        ExpectedBase16(ExpectedBase16Item<'a>),
        /// Embedded CBOR data (tag 24).
        EncodedCbor(EncodedCbor<'a>),
        /// A URI (tag 32).
        Uri(Uri<'a>),
        /// A base64url-encoded string (tag 33).
        Base64Url(Base64Url<'a>),
        /// A base64-encoded string (tag 34).
        Base64(Base64<'a>),
        /// A regular expression (tag 35).
        Regex(Regex<'a>),
        /// A MIME message (tag 36).
        Mime(Mime<'a>),
        /// Self-described CBOR (tag 55799).
        SelfDescribe(SelfDescribeItem<'a>),
    }

    impl<'a> StandardTag<'a> {
        /// Decode a standard tag from a tagged item.
        ///
        /// # Examples
        ///
        /// ```rust
        /// # #[cfg(feature = "standard_tags")]
        /// # {
        /// use tps_minicbor::{Decoder, Item};
        /// use tps_minicbor::standard_tags::{StandardTag, TAG_DATETIME};
        ///
        /// let data = [0xc0, 0x61, 0x61];
        /// let mut dec = Decoder::new(&data);
        /// let tagged = dec.next_tag().unwrap();
        /// let standard = StandardTag::from_tag(tagged).unwrap();
        /// assert!(matches!(standard, StandardTag::DateTime(_)));
        /// # }
        /// ```
        pub fn from_tag(tagged: TaggedItem<'a>) -> Result<Self, DecodeError> {
            match tagged.tag {
                TAG_DATETIME => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::DateTime(DateTime(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_EPOCH => match tagged.item()? {
                    Item::Unsigned(value) => {
                        let value =
                            i64::try_from(value).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
                        Ok(StandardTag::Epoch(EpochTime::Int(value)))
                    }
                    Item::Negative(value) => Ok(StandardTag::Epoch(EpochTime::Int(value))),
                    #[cfg(feature = "float")]
                    Item::Float(value) => {
                        Ok(StandardTag::Epoch(EpochTime::Float(float_to_f64(value))))
                    }
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_POS_BIGNUM => match tagged.item()? {
                    Item::Bytes(value) => Ok(StandardTag::PositiveBignum(PositiveBignum(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_NEG_BIGNUM => match tagged.item()? {
                    Item::Bytes(value) => Ok(StandardTag::NegativeBignum(NegativeBignum(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_DECIMAL_FRACTION => {
                    let item = tagged.item()?;
                    let value = decode_decimal_fraction(item)?;
                    Ok(StandardTag::DecimalFraction(value))
                }
                TAG_BIGFLOAT => {
                    let item = tagged.item()?;
                    let value = decode_bigfloat(item)?;
                    Ok(StandardTag::BigFloat(value))
                }
                TAG_EXPECTED_BASE64URL => Ok(StandardTag::ExpectedBase64Url(ExpectedBase64Url(
                    decode_tagged_item(tagged, TAG_EXPECTED_BASE64URL)?,
                ))),
                TAG_EXPECTED_BASE64 => Ok(StandardTag::ExpectedBase64(ExpectedBase64(
                    decode_tagged_item(tagged, TAG_EXPECTED_BASE64)?,
                ))),
                TAG_EXPECTED_BASE16 => Ok(StandardTag::ExpectedBase16(ExpectedBase16(
                    decode_tagged_item(tagged, TAG_EXPECTED_BASE16)?,
                ))),
                TAG_EMBEDDED_CBOR => match tagged.item()? {
                    Item::Bytes(value) => Ok(StandardTag::EncodedCbor(EncodedCbor(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_URI => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::Uri(Uri(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_BASE64URL => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::Base64Url(Base64Url(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_BASE64 => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::Base64(Base64(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_REGEX => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::Regex(Regex(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_MIME => match tagged.item()? {
                    Item::Text(value) => Ok(StandardTag::Mime(Mime(value))),
                    _ => Err(DecodeError::InvalidAdditionalInfo),
                },
                TAG_SELF_DESCRIBED_CBOR => Ok(StandardTag::SelfDescribe(SelfDescribe(
                    decode_tagged_item(tagged, TAG_SELF_DESCRIBED_CBOR)?,
                ))),
                _ => Err(DecodeError::InvalidAdditionalInfo),
            }
        }
    }

    impl<'a> Decode<'a> for StandardTag<'a> {
        fn decode(d: &mut crate::Decoder<'a>) -> Result<Self, DecodeError> {
            let tagged = d.next_tag()?;
            StandardTag::from_tag(tagged)
        }
    }

    struct Array2<A, B>(A, B);

    impl<A: EncodeItem, B: EncodeItem> EncodeItem for Array2<A, B> {
        fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
            s.array_len(2)?;
            self.0.size(s)?;
            self.1.size(s)
        }

        fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
            let mut array = e.array_with_len(2)?;
            array.insert_ref(&self.0)?;
            array.insert_ref(&self.1)?;
            array.end()
        }
    }

    #[cfg(feature = "float")]
    fn float_to_f64(value: Float) -> f64 {
        match value {
            Float::F16(bits) => f16::from_bits(bits).to_f64(),
            Float::F32(bits) => f32::from_bits(bits) as f64,
            Float::F64(bits) => f64::from_bits(bits),
        }
    }
}
