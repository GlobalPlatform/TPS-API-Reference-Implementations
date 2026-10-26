/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
mod support;

mod rfc8949 {
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    use super::support::{TestValue, decode_value_all, hex_to_bytes};

    #[test]
    fn rfc8949_appendix_a_vectors() {
        #[derive(Debug)]
        enum Expected {
            Ok(TestValue),
            Err,
        }

        fn u(v: u64) -> TestValue {
            TestValue::Unsigned(v)
        }
        fn n(v: i64) -> TestValue {
            TestValue::Negative(v)
        }
        fn b(bytes: &[u8]) -> TestValue {
            TestValue::Bytes(bytes.to_vec())
        }
        fn t(s: &str) -> TestValue {
            TestValue::Text(s.to_string())
        }
        fn arr(items: Vec<TestValue>) -> TestValue {
            TestValue::Array(items)
        }
        fn map(items: Vec<(TestValue, TestValue)>) -> TestValue {
            TestValue::Map(items)
        }
        fn boolv(v: bool) -> TestValue {
            TestValue::Bool(v)
        }
        #[cfg(feature = "float")]
        fn float(v: f64) -> TestValue {
            TestValue::Float(v)
        }
        fn tag(tn: u64, inner: TestValue) -> TestValue {
            TestValue::Tag(tn, Box::new(inner))
        }

        let vectors: Vec<(&str, &str, Expected)> = vec![
            ("0", "0x00", Expected::Ok(u(0))),
            ("1", "0x01", Expected::Ok(u(1))),
            ("10", "0x0a", Expected::Ok(u(10))),
            ("23", "0x17", Expected::Ok(u(23))),
            ("24", "0x1818", Expected::Ok(u(24))),
            ("25", "0x1819", Expected::Ok(u(25))),
            ("100", "0x1864", Expected::Ok(u(100))),
            ("1000", "0x1903e8", Expected::Ok(u(1000))),
            ("1000000", "0x1a000f4240", Expected::Ok(u(1_000_000))),
            (
                "1000000000000",
                "0x1b000000e8d4a51000",
                Expected::Ok(u(1_000_000_000_000)),
            ),
            (
                "18446744073709551615",
                "0x1bffffffffffffffff",
                Expected::Ok(u(u64::MAX)),
            ),
            (
                "18446744073709551616",
                "0xc249010000000000000000",
                Expected::Ok(tag(2, b(b"\x01\x00\x00\x00\x00\x00\x00\x00\x00"))),
            ),
            (
                "-18446744073709551616",
                "0x3bffffffffffffffff",
                Expected::Err,
            ),
            (
                "-18446744073709551617",
                "0xc349010000000000000000",
                Expected::Ok(tag(3, b(b"\x01\x00\x00\x00\x00\x00\x00\x00\x00"))),
            ),
            ("-1", "0x20", Expected::Ok(n(-1))),
            ("-10", "0x29", Expected::Ok(n(-10))),
            ("-100", "0x3863", Expected::Ok(n(-100))),
            ("-1000", "0x3903e7", Expected::Ok(n(-1000))),
            ("false", "0xf4", Expected::Ok(boolv(false))),
            ("true", "0xf5", Expected::Ok(boolv(true))),
            ("null", "0xf6", Expected::Ok(TestValue::Null)),
            ("undefined", "0xf7", Expected::Ok(TestValue::Undefined)),
            ("simple(16)", "0xf0", Expected::Ok(TestValue::Simple(16))),
            (
                "simple(255)",
                "0xf8ff",
                Expected::Ok(TestValue::Simple(255)),
            ),
            (
                "0(\"2013-03-21T20:04:00Z\")",
                "0xc074323031332d30332d32315432303a30343a30305a",
                Expected::Ok(tag(0, t("2013-03-21T20:04:00Z"))),
            ),
            (
                "1(1363896240)",
                "0xc11a514b67b0",
                Expected::Ok(tag(1, u(1363896240))),
            ),
            (
                "23(h'01020304')",
                "0xd74401020304",
                Expected::Ok(tag(23, b(b"\x01\x02\x03\x04"))),
            ),
            (
                "24(h'6449455446')",
                "0xd818456449455446",
                Expected::Ok(tag(24, b(b"dIETF"))),
            ),
            (
                r#"32("http://www.example.com")"#,
                "0xd82076687474703a2f2f7777772e6578616d706c652e636f6d",
                Expected::Ok(tag(32, t("http://www.example.com"))),
            ),
            ("h''", "0x40", Expected::Ok(b(b""))),
            (
                "h'01020304'",
                "0x4401020304",
                Expected::Ok(b(b"\x01\x02\x03\x04")),
            ),
            ("\"\"", "0x60", Expected::Ok(t(""))),
            ("\"a\"", "0x6161", Expected::Ok(t("a"))),
            ("\"IETF\"", "0x6449455446", Expected::Ok(t("IETF"))),
            ("\"\\\\\\\"\\\\\"", "0x62225c", Expected::Ok(t("\"\\"))),
            ("\"\\u00fc\"", "0x62c3bc", Expected::Ok(t("\u{00fc}"))),
            ("\"\\u6c34\"", "0x63e6b0b4", Expected::Ok(t("\u{6c34}"))),
            (
                "\"\\ud800\\udd51\"",
                "0x64f0908591",
                Expected::Ok(t("\u{10151}")),
            ),
            ("[]", "0x80", Expected::Ok(arr(vec![]))),
            (
                "[1,2,3]",
                "0x83010203",
                Expected::Ok(arr(vec![u(1), u(2), u(3)])),
            ),
            (
                "[1,[2,3],[4,5]]",
                "0x8301820203820405",
                Expected::Ok(arr(vec![
                    u(1),
                    arr(vec![u(2), u(3)]),
                    arr(vec![u(4), u(5)]),
                ])),
            ),
            (
                "[1..25]",
                "0x98190102030405060708090a0b0c0d0e0f101112131415161718181819",
                Expected::Ok(arr((1u64..=25).map(u).collect())),
            ),
            ("{}", "0xa0", Expected::Ok(map(vec![]))),
            (
                "{1:2,3:4}",
                "0xa201020304",
                Expected::Ok(map(vec![(u(1), u(2)), (u(3), u(4))])),
            ),
            (
                "{\"a\":1,\"b\":[2,3]}",
                "0xa26161016162820203",
                Expected::Ok(map(vec![(t("a"), u(1)), (t("b"), arr(vec![u(2), u(3)]))])),
            ),
            (
                "[\"a\", {\"b\":\"c\"}]",
                "0x826161a161626163",
                Expected::Ok(arr(vec![t("a"), map(vec![(t("b"), t("c"))])])),
            ),
            (
                "{\"a\":\"A\",\"b\":\"B\",\"c\":\"C\",\"d\":\"D\",\"e\":\"E\"}",
                "0xa56161614161626142616361436164614461656145",
                Expected::Ok(map(vec![
                    (t("a"), t("A")),
                    (t("b"), t("B")),
                    (t("c"), t("C")),
                    (t("d"), t("D")),
                    (t("e"), t("E")),
                ])),
            ),
            (
                "(_ h'0102', h'030405')",
                "0x5f42010243030405ff",
                Expected::Err,
            ),
            (
                "(_ \"strea\", \"ming\")",
                "0x7f657374726561646d696e67ff",
                Expected::Err,
            ),
            ("[_ ]", "0x9fff", Expected::Err),
            (
                "[_ 1, [2, 3], [_ 4, 5]]",
                "0x9f018202039f0405ffff",
                Expected::Err,
            ),
            (
                "[_ 1, [2, 3], [4, 5]]",
                "0x9f01820203820405ff",
                Expected::Err,
            ),
            (
                "[1, [2, 3], [_ 4, 5]]",
                "0x83018202039f0405ff",
                Expected::Err,
            ),
            (
                "[1, [_ 2, 3], [4, 5]]",
                "0x83019f0203ff820405",
                Expected::Err,
            ),
            (
                "[_ 1..25]",
                "0x9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff",
                Expected::Err,
            ),
            (
                "{_ \"a\":1, \"b\":[_ 2, 3]}",
                "0xbf61610161629f0203ffff",
                Expected::Err,
            ),
            (
                "[\"a\", {_ \"b\":\"c\"}]",
                "0x826161bf61626163ff",
                Expected::Err,
            ),
            (
                "{_ \"Fun\": true, \"Amt\": -2}",
                "0xbf6346756ef563416d7421ff",
                Expected::Err,
            ),
        ];

        #[cfg(feature = "float")]
        let vectors = {
            let mut vectors = vectors;
            vectors.extend([
                ("0.0", "0xf90000", Expected::Ok(float(0.0))),
                ("-0.0", "0xf98000", Expected::Ok(float(-0.0))),
                ("1.0", "0xf93c00", Expected::Ok(float(1.0))),
                ("1.1", "0xfb3ff199999999999a", Expected::Ok(float(1.1))),
                ("1.5", "0xf93e00", Expected::Ok(float(1.5))),
                ("65504.0", "0xf97bff", Expected::Ok(float(65504.0))),
                ("100000.0", "0xfa47c35000", Expected::Ok(float(100000.0))),
                (
                    "3.4028234663852886e+38",
                    "0xfa7f7fffff",
                    Expected::Ok(float(3.4028234663852886e+38)),
                ),
                (
                    "1.0e+300",
                    "0xfb7e37e43c8800759c",
                    Expected::Ok(float(1.0e+300)),
                ),
                (
                    "5.960464477539063e-8",
                    "0xf90001",
                    Expected::Ok(float(5.960464477539063e-8)),
                ),
                (
                    "0.00006103515625",
                    "0xf90400",
                    Expected::Ok(float(0.00006103515625)),
                ),
                ("-4.0", "0xf9c400", Expected::Ok(float(-4.0))),
                ("-4.1", "0xfbc010666666666666", Expected::Ok(float(-4.1))),
                (
                    "Infinity (f16)",
                    "0xf97c00",
                    Expected::Ok(float(f64::INFINITY)),
                ),
                ("NaN (f16)", "0xf97e00", Expected::Ok(float(f64::NAN))),
                (
                    "-Infinity (f16)",
                    "0xf9fc00",
                    Expected::Ok(float(f64::NEG_INFINITY)),
                ),
                (
                    "Infinity (f32)",
                    "0xfa7f800000",
                    Expected::Ok(float(f64::INFINITY)),
                ),
                ("NaN (f32)", "0xfa7fc00000", Expected::Ok(float(f64::NAN))),
                (
                    "-Infinity (f32)",
                    "0xfaff800000",
                    Expected::Ok(float(f64::NEG_INFINITY)),
                ),
                (
                    "Infinity (f64)",
                    "0xfb7ff0000000000000",
                    Expected::Ok(float(f64::INFINITY)),
                ),
                (
                    "NaN (f64)",
                    "0xfb7ff8000000000000",
                    Expected::Ok(float(f64::NAN)),
                ),
                (
                    "-Infinity (f64)",
                    "0xfbfff0000000000000",
                    Expected::Ok(float(f64::NEG_INFINITY)),
                ),
                (
                    "1(1363896240.5)",
                    "0xc1fb41d452d9ec200000",
                    Expected::Ok(tag(1, float(1363896240.5))),
                ),
            ]);
            vectors
        };

        for (diag, hex, expected) in vectors {
            let bytes = hex_to_bytes(hex);
            let res = decode_value_all(&bytes);
            match expected {
                Expected::Ok(value) => {
                    let decoded = res.expect(diag);
                    match (&decoded, &value) {
                        (TestValue::Float(a), TestValue::Float(b)) if a.is_nan() && b.is_nan() => {}
                        _ => assert_eq!(decoded, value, "diag: {diag}"),
                    }
                }
                Expected::Err => {
                    assert!(res.is_err(), "expected error for {diag}");
                }
            }
        }
    }
}
