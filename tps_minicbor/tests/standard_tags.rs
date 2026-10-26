/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
mod support;

#[cfg(feature = "standard_tags")]
use support::hex_to_bytes;

#[cfg(feature = "standard_tags")]
use tps_minicbor::standard_tags::{
    Base64, Base64Url, BigFloat, BigInt, DateTime, DecimalFraction, EncodedCbor, EpochTime,
    ExpectedBase16, ExpectedBase64, ExpectedBase64Url, Mime, NegativeBignum, PositiveBignum, Regex,
    SelfDescribe, StandardTag, Uri,
};
#[cfg(feature = "standard_tags")]
use tps_minicbor::{Decode, Decoder, encode};

#[cfg(feature = "standard_tags")]
#[test]
fn encode_standard_tags_rfc_examples() {
    let mut buf = [0u8; 128];

    let len = encode(&DateTime("2013-03-21T20:04:00Z"), &mut buf).unwrap();
    assert_eq!(
        &buf[..len],
        hex_to_bytes("0xc074323031332d30332d32315432303a30343a30305a")
    );

    let len = encode(&EpochTime::Int(1363896240), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xc11a514b67b0"));

    let len = encode(
        &PositiveBignum(b"\x01\x00\x00\x00\x00\x00\x00\x00\x00"),
        &mut buf,
    )
    .unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xc249010000000000000000"));

    let len = encode(
        &NegativeBignum(b"\x01\x00\x00\x00\x00\x00\x00\x00\x00"),
        &mut buf,
    )
    .unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xc349010000000000000000"));

    let len = encode(&ExpectedBase16(&b"\x01\x02\x03\x04"[..]), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd74401020304"));

    let len = encode(&EncodedCbor(b"dIETF"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd818456449455446"));

    let len = encode(&Uri("http://www.example.com"), &mut buf).unwrap();
    assert_eq!(
        &buf[..len],
        hex_to_bytes("0xd82076687474703a2f2f7777772e6578616d706c652e636f6d")
    );
}

#[cfg(feature = "standard_tags")]
#[test]
fn encode_standard_tags_custom_examples() {
    let mut buf = [0u8; 128];

    let fraction = DecimalFraction {
        exponent: -2,
        mantissa: BigInt::Small(27315),
    };
    let len = encode(&fraction, &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xc48221196ab3"));

    let bigfloat = BigFloat {
        exponent: 2,
        mantissa: BigInt::Small(123456),
    };
    let len = encode(&bigfloat, &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xc582021a0001e240"));

    let len = encode(&ExpectedBase64Url("hello"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd56568656c6c6f"));

    let len = encode(&ExpectedBase64("AQID"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd66441514944"));

    let len = encode(&Base64Url("AQID"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd8216441514944"));

    let len = encode(&Base64("AQID"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd8226441514944"));

    let len = encode(&Regex("^[a-z]+$"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd823685e5b612d7a5d2b24"));

    let len = encode(&Mime("text/plain"), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd8246a746578742f706c61696e"));

    let len = encode(&SelfDescribe(1u8), &mut buf).unwrap();
    assert_eq!(&buf[..len], hex_to_bytes("0xd9d9f701"));
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_standard_tags() {
    let datetime = hex_to_bytes("0xc074323031332d30332d32315432303a30343a30305a");
    let mut dec = Decoder::new(&datetime);
    let tag = dec.next_standard_tag().unwrap();
    assert!(matches!(tag, StandardTag::DateTime(_)));

    let epoch = hex_to_bytes("0xc11a514b67b0");
    let mut dec = Decoder::new(&epoch);
    let tag = dec.next_standard_tag().unwrap();
    assert!(matches!(
        tag,
        StandardTag::Epoch(EpochTime::Int(1363896240))
    ));

    let encoded = hex_to_bytes("0xd818456449455446");
    let mut dec = Decoder::new(&encoded);
    let tag = dec.next_standard_tag().unwrap();
    assert!(matches!(tag, StandardTag::EncodedCbor(_)));

    let uri = hex_to_bytes("0xd82076687474703a2f2f7777772e6578616d706c652e636f6d");
    let mut dec = Decoder::new(&uri);
    let tag = dec.next_standard_tag().unwrap();
    assert!(matches!(tag, StandardTag::Uri(_)));
}

// ── Regression coverage: decode round trips for tag types not previously
// exercised on the decode side (only encode-direction coverage existed above
// for these). Each vector is taken from `encode_standard_tags_rfc_examples`
// / `encode_standard_tags_custom_examples` above so encode and decode are
// cross-checked against the same hex. ─────────────────────────────────────────

#[cfg(feature = "standard_tags")]
#[test]
fn decode_positive_bignum() {
    let data = hex_to_bytes("0xc249010000000000000000");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::PositiveBignum(PositiveBignum(bytes)) => {
            assert_eq!(bytes, b"\x01\x00\x00\x00\x00\x00\x00\x00\x00");
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_negative_bignum() {
    let data = hex_to_bytes("0xc349010000000000000000");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::NegativeBignum(NegativeBignum(bytes)) => {
            assert_eq!(bytes, b"\x01\x00\x00\x00\x00\x00\x00\x00\x00");
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_decimal_fraction() {
    let data = hex_to_bytes("0xc48221196ab3");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::DecimalFraction(DecimalFraction {
            exponent,
            mantissa: BigInt::Small(mantissa),
        }) => {
            assert_eq!(exponent, -2);
            assert_eq!(mantissa, 27315);
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_bigfloat() {
    let data = hex_to_bytes("0xc582021a0001e240");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::BigFloat(BigFloat {
            exponent,
            mantissa: BigInt::Small(mantissa),
        }) => {
            assert_eq!(exponent, 2);
            assert_eq!(mantissa, 123456);
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_expected_base64url() {
    let data = hex_to_bytes("0xd56568656c6c6f");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::ExpectedBase64Url(ExpectedBase64Url(tps_minicbor::Item::Text(text))) => {
            assert_eq!(text, "hello")
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_expected_base64() {
    let data = hex_to_bytes("0xd66441514944");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::ExpectedBase64(ExpectedBase64(tps_minicbor::Item::Text(text))) => {
            assert_eq!(text, "AQID")
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_expected_base16() {
    let data = hex_to_bytes("0xd74401020304");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::ExpectedBase16(ExpectedBase16(tps_minicbor::Item::Bytes(bytes))) => {
            assert_eq!(bytes, &b"\x01\x02\x03\x04"[..]);
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_base64url() {
    let data = hex_to_bytes("0xd8216441514944");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::Base64Url(Base64Url(text)) => assert_eq!(text, "AQID"),
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_base64() {
    let data = hex_to_bytes("0xd8226441514944");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::Base64(Base64(text)) => assert_eq!(text, "AQID"),
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_regex() {
    let data = hex_to_bytes("0xd823685e5b612d7a5d2b24");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::Regex(Regex(text)) => assert_eq!(text, "^[a-z]+$"),
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_mime() {
    let data = hex_to_bytes("0xd8246a746578742f706c61696e");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::Mime(Mime(text)) => assert_eq!(text, "text/plain"),
        other => panic!("unexpected tag {other:?}"),
    }
}

#[cfg(feature = "standard_tags")]
#[test]
fn decode_self_describe() {
    let data = hex_to_bytes("0xd9d9f701");
    let mut dec = Decoder::new(&data);
    let tag = dec.next_standard_tag().unwrap();
    match tag {
        StandardTag::SelfDescribe(SelfDescribe(item)) => {
            // Inner item is a raw `Item`; re-decode it as u8.
            assert!(matches!(item, tps_minicbor::Item::Unsigned(1)));
        }
        other => panic!("unexpected tag {other:?}"),
    }
}

// ── BigInt dispatch: raw ints vs. tag 2 / tag 3 bignums ────────────────────────

#[cfg(feature = "standard_tags")]
#[test]
fn bigint_decodes_small_unsigned_and_negative() {
    let data = hex_to_bytes("0x01"); // unsigned 1
    let mut dec = Decoder::new(&data);
    assert!(matches!(
        BigInt::decode(&mut dec),
        Ok(BigInt::Small(1))
    ));

    let data = hex_to_bytes("0x20"); // -1
    let mut dec = Decoder::new(&data);
    assert!(matches!(
        BigInt::decode(&mut dec),
        Ok(BigInt::Small(-1))
    ));
}

#[cfg(feature = "standard_tags")]
#[test]
fn bigint_decodes_tag_2_and_tag_3_bignums() {
    let data = hex_to_bytes("0xc249010000000000000000");
    let mut dec = Decoder::new(&data);
    match BigInt::decode(&mut dec) {
        Ok(BigInt::Positive(bytes)) => {
            assert_eq!(bytes, b"\x01\x00\x00\x00\x00\x00\x00\x00\x00");
        }
        other => panic!("unexpected {other:?}"),
    }

    let data = hex_to_bytes("0xc349010000000000000000");
    let mut dec = Decoder::new(&data);
    match BigInt::decode(&mut dec) {
        Ok(BigInt::Negative(bytes)) => {
            assert_eq!(bytes, b"\x01\x00\x00\x00\x00\x00\x00\x00\x00");
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ── Error paths: wrong tag number and wrong inner CBOR type ───────────────────

#[cfg(feature = "standard_tags")]
#[test]
fn from_tag_rejects_unrecognized_tag_number() {
    // Tag 999 wrapping a text string: not a registered standard tag.
    let data = hex_to_bytes("0xd903e76161");
    let mut dec = Decoder::new(&data);
    let tagged = dec.next_tag().unwrap();
    assert!(matches!(
        StandardTag::from_tag(tagged),
        Err(tps_minicbor::DecodeError::InvalidAdditionalInfo)
    ));
}

#[cfg(feature = "standard_tags")]
#[test]
fn from_tag_rejects_wrong_inner_type_for_datetime() {
    // Tag 0 (DateTime) must wrap a text string; wrap an unsigned int instead.
    let data = hex_to_bytes("0xc001");
    let mut dec = Decoder::new(&data);
    let tagged = dec.next_tag().unwrap();
    assert!(matches!(
        StandardTag::from_tag(tagged),
        Err(tps_minicbor::DecodeError::InvalidAdditionalInfo)
    ));
}

#[cfg(feature = "standard_tags")]
#[test]
fn from_tag_rejects_wrong_inner_type_for_positive_bignum() {
    // Tag 2 (PositiveBignum) must wrap a byte string; wrap a text string instead.
    let data = hex_to_bytes("0xc26161");
    let mut dec = Decoder::new(&data);
    let tagged = dec.next_tag().unwrap();
    assert!(matches!(
        StandardTag::from_tag(tagged),
        Err(tps_minicbor::DecodeError::InvalidAdditionalInfo)
    ));
}

#[cfg(feature = "standard_tags")]
#[test]
fn from_tag_rejects_decimal_fraction_with_wrong_array_len() {
    // Tag 4 (DecimalFraction) must wrap a 2-element array [exponent, mantissa];
    // supply a 1-element array instead.
    let data = hex_to_bytes("0xc48120");
    let mut dec = Decoder::new(&data);
    let tagged = dec.next_tag().unwrap();
    assert!(matches!(
        StandardTag::from_tag(tagged),
        Err(tps_minicbor::DecodeError::InvalidAdditionalInfo)
    ));
}
