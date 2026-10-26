/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![cfg(feature = "diagnostic")]

mod support;

use support::hex_to_bytes;
use tps_minicbor::Decoder;

fn normalize_ws(input: &str) -> String {
    input.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

#[derive(Clone, Copy)]
struct Vector {
    diag: &'static str,
    hex: &'static str,
    expect_ok: bool,
    requires_float: bool,
}

fn rfc8949_vectors() -> Vec<Vector> {
    let vectors = vec![
        Vector {
            diag: "0",
            hex: "0x00",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1",
            hex: "0x01",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "10",
            hex: "0x0a",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "23",
            hex: "0x17",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "24",
            hex: "0x1818",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "25",
            hex: "0x1819",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "100",
            hex: "0x1864",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1000",
            hex: "0x1903e8",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1000000",
            hex: "0x1a000f4240",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1000000000000",
            hex: "0x1b000000e8d4a51000",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "18446744073709551615",
            hex: "0x1bffffffffffffffff",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "18446744073709551616",
            hex: "0xc249010000000000000000",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-18446744073709551616",
            hex: "0x3bffffffffffffffff",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-18446744073709551617",
            hex: "0xc349010000000000000000",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-1",
            hex: "0x20",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-10",
            hex: "0x29",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-100",
            hex: "0x3863",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "-1000",
            hex: "0x3903e7",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "0.0",
            hex: "0xf90000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-0.0",
            hex: "0xf98000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "1.0",
            hex: "0xf93c00",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "1.1",
            hex: "0xfb3ff199999999999a",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "1.5",
            hex: "0xf93e00",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "65504.0",
            hex: "0xf97bff",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "100000.0",
            hex: "0xfa47c35000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "3.4028234663852886e+38",
            hex: "0xfa7f7fffff",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "1.0e+300",
            hex: "0xfb7e37e43c8800759c",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "5.960464477539063e-8",
            hex: "0xf90001",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "0.00006103515625",
            hex: "0xf90400",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-4.0",
            hex: "0xf9c400",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-4.1",
            hex: "0xfbc010666666666666",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "Infinity",
            hex: "0xf97c00",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "NaN",
            hex: "0xf97e00",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-Infinity",
            hex: "0xf9fc00",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "Infinity",
            hex: "0xfa7f800000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "NaN",
            hex: "0xfa7fc00000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-Infinity",
            hex: "0xfaff800000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "Infinity",
            hex: "0xfb7ff0000000000000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "NaN",
            hex: "0xfb7ff8000000000000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "-Infinity",
            hex: "0xfbfff0000000000000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "false",
            hex: "0xf4",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "true",
            hex: "0xf5",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "null",
            hex: "0xf6",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "undefined",
            hex: "0xf7",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "simple(16)",
            hex: "0xf0",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "simple(255)",
            hex: "0xf8ff",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "0(\"2013-03-21T20:04:00Z\")",
            hex: "0xc074323031332d30332d32315432303a30343a30305a",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1(1363896240)",
            hex: "0xc11a514b67b0",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "1(1363896240.5)",
            hex: "0xc1fb41d452d9ec200000",
            expect_ok: true,
            requires_float: true,
        },
        Vector {
            diag: "23(h'01020304')",
            hex: "0xd74401020304",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "24(h'6449455446')",
            hex: "0xd818456449455446",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "32(\"http://www.example.com\")",
            hex: "0xd82076687474703a2f2f7777772e6578616d706c652e636f6d",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "h''",
            hex: "0x40",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "h'01020304'",
            hex: "0x4401020304",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"\"",
            hex: "0x60",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"a\"",
            hex: "0x6161",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"IETF\"",
            hex: "0x6449455446",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: r#""\"\\""#,
            hex: "0x62225c",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"\\u00fc\"",
            hex: "0x62c3bc",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"\\u6c34\"",
            hex: "0x63e6b0b4",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "\"\\ud800\\udd51\"",
            hex: "0x64f0908591",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "[]",
            hex: "0x80",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "[1, 2, 3]",
            hex: "0x83010203",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "[1, [2, 3], [4, 5]]",
            hex: "0x8301820203820405",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]",
            hex: "0x98190102030405060708090a0b0c0d0e0f101112131415161718181819",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "{}",
            hex: "0xa0",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "{1: 2, 3: 4}",
            hex: "0xa201020304",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "{\"a\": 1, \"b\": [2, 3]}",
            hex: "0xa26161016162820203",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "[\"a\", {\"b\": \"c\"}]",
            hex: "0x826161a161626163",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "{\"a\": \"A\", \"b\": \"B\", \"c\": \"C\", \"d\": \"D\", \"e\": \"E\"}",
            hex: "0xa56161614161626142616361436164614461656145",
            expect_ok: true,
            requires_float: false,
        },
        Vector {
            diag: "(_ h'0102', h'030405')",
            hex: "0x5f42010243030405ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "(_ \"strea\", \"ming\")",
            hex: "0x7f657374726561646d696e67ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[_ ]",
            hex: "0x9fff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[_ 1, [2, 3], [_ 4, 5]]",
            hex: "0x9f018202039f0405ffff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[_ 1, [2, 3], [4, 5]]",
            hex: "0x9f01820203820405ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[1, [2, 3], [_ 4, 5]]",
            hex: "0x83018202039f0405ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[1, [_ 2, 3], [4, 5]]",
            hex: "0x83019f0203ff820405",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[_ 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]",
            hex: "0x9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "{_ \"a\": 1, \"b\": [_ 2, 3]}",
            hex: "0xbf61610161629f0203ffff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "[\"a\", {_ \"b\": \"c\"}]",
            hex: "0x826161bf61626163ff",
            expect_ok: false,
            requires_float: false,
        },
        Vector {
            diag: "{_ \"Fun\": true, \"Amt\": -2}",
            hex: "0xbf6346756ef563416d7421ff",
            expect_ok: false,
            requires_float: false,
        },
    ];

    vectors
}

fn diagnostics_to_test() -> Vec<Vector> {
    let mut out = Vec::new();
    for vector in rfc8949_vectors() {
        if vector.requires_float && !cfg!(feature = "float") {
            continue;
        }
        out.push(vector);
    }
    out
}

fn parse_rfc8949_appendix_a() -> Vec<(String, String)> {
    let text = include_str!("../rfc8949.txt");
    let mut rows = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut in_appendix = false;
    let mut in_table = false;

    for raw_line in text.lines() {
        let line = raw_line.trim_start();
        if line.starts_with("Appendix A.  Examples of Encoded CBOR Data Items") {
            in_appendix = true;
            continue;
        }
        if line.starts_with("Table 6:") {
            break;
        }
        if !in_appendix {
            continue;
        }
        if line.starts_with("+==============================+====================================+")
        {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if line.starts_with('+') {
            continue;
        }
        if !line.starts_with('|') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 {
            continue;
        }
        let diag = parts[1].trim();
        let hex = parts[2].trim();
        if diag == "Diagnostic" && hex == "Encoded" {
            continue;
        }
        if diag.is_empty() && hex.is_empty() {
            continue;
        }
        match current {
            None => {
                current = Some((diag.to_string(), hex.to_string()));
            }
            Some((ref mut current_diag, ref mut current_hex)) => {
                let hex_continues = hex.is_empty() || !hex.starts_with("0x");
                if hex_continues {
                    if !diag.is_empty() {
                        current_diag.push_str(diag);
                    }
                    if !hex.is_empty() {
                        current_hex.push_str(hex);
                    }
                } else {
                    rows.push((current_diag.clone(), current_hex.clone()));
                    *current_diag = diag.to_string();
                    *current_hex = hex.to_string();
                }
            }
        }
    }

    if let Some((diag, hex)) = current {
        rows.push((diag, hex));
    }

    rows
}

#[test]
fn rfc8949_vectors_match_rfc_text() {
    let expected: Vec<(String, String)> = rfc8949_vectors()
        .into_iter()
        .map(|vector| (normalize_ws(vector.diag), normalize_ws(vector.hex)))
        .collect();
    let parsed: Vec<(String, String)> = parse_rfc8949_appendix_a()
        .into_iter()
        .map(|(diag, hex)| (normalize_ws(&diag), normalize_ws(&hex)))
        .collect();
    assert_eq!(expected, parsed, "RFC 8949 Appendix A vectors differ");
}

#[test]
fn rfc8949_diagnostic_vectors() {
    for vector in diagnostics_to_test() {
        let bytes = hex_to_bytes(vector.hex);
        let dec = Decoder::new(&bytes);

        if vector.expect_ok {
            let mut out = String::new();
            dec.write_diagnostic_all_fmt(&mut out).unwrap();
            assert_eq!(
                normalize_ws(&out),
                normalize_ws(vector.diag),
                "vector {}",
                vector.diag
            );

            let mut buf = vec![0u8; out.len() + 16];
            let len = dec.write_diagnostic_all(&mut buf).unwrap();
            let diag = core::str::from_utf8(&buf[..len]).unwrap();
            assert_eq!(
                normalize_ws(diag),
                normalize_ws(vector.diag),
                "buffer {}",
                vector.diag
            );
        } else {
            let mut out = String::new();
            assert!(
                dec.write_diagnostic_all_fmt(&mut out).is_err(),
                "vector {}",
                vector.diag
            );
            let mut buf = vec![0u8; 256];
            assert!(
                dec.write_diagnostic_all(&mut buf).is_err(),
                "buffer {}",
                vector.diag
            );
        }
    }
}

// Stage 3: `DiagnosticDecoder::take_bytes` shares the same `pos + len`
// overflow finding as the binary decode path (see cbor_tests.rs) — a byte
// string header lying about its length must be rejected, not panic or
// silently produce diagnostic output for the wrong bytes.
#[test]
fn diagnostic_write_rejects_max_u64_length_header() {
    let data = [0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let dec = Decoder::new(&data);
    let mut out = String::new();
    assert!(dec.write_diagnostic_fmt(&mut out).is_err());
}

// Stage 3: `DiagnosticDecoder::write_item`/`write_item_with_spans` recurse
// into nested arrays/maps/tags with no depth limit, independent of the
// binary `skip_item` decode path — a deeply nested payload must be rejected
// with an error rather than overflow the stack while rendering diagnostic
// output, in both compact and pretty-printed modes.
#[test]
fn diagnostic_write_rejects_deeply_nested_tags() {
    let mut data = vec![0xc0u8; 1000];
    data.push(0x00);
    let dec = Decoder::new(&data);
    let mut out = String::new();
    assert!(dec.write_diagnostic_fmt(&mut out).is_err());
}

#[test]
fn diagnostic_write_rejects_deeply_nested_arrays() {
    let mut data = vec![0x81u8; 1000];
    data.push(0x00);
    let dec = Decoder::new(&data);
    let mut out = String::new();
    assert!(dec.write_diagnostic_fmt(&mut out).is_err());
}

#[test]
fn diagnostic_write_pretty_rejects_deeply_nested_arrays() {
    let mut data = vec![0x81u8; 1000];
    data.push(0x00);
    let dec = Decoder::new(&data);
    let mut out = String::new();
    assert!(dec.write_diagnostic_pretty_fmt(&mut out).is_err());
}
