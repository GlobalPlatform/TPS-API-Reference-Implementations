/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! RFC-sourced examples for the CDDL parser, validator, and checker.
//!
//! These tests use schemas and data directly from RFC 8610, RFC 9165, and
//! RFC 9682 so that we can track conformance against the published standards.
//!
//! References:
//!  - RFC 8610 §3 and Appendix H
//!  - RFC 9165 §2
//!  - RFC 9682 §2 and §3

use tps_cddl::{
    Config, Level, build_cddl, check_cbor, check_json, lex, validate, validate_with_config,
    validate_with_config_ex,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse(src: &str) -> tps_cddl::Cddl {
    let pairs = lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    build_cddl(pairs).unwrap_or_else(|e| panic!("build failed: {e}"))
}

fn valid(src: &str) {
    let schema = parse(src);
    let diags = validate(&schema);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        errors.is_empty(),
        "expected no errors for:\n{src}\nGot: {errors:?}"
    );
}

fn valid_cfg(src: &str, cfg: Config) {
    let schema = parse(src);
    let diags = validate_with_config(&schema, &cfg);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

fn cbor_ok(cbor: &[u8], schema_src: &str) {
    let schema = parse(schema_src);
    let mismatches = check_cbor(cbor, &schema, None, &Config::strict()).expect("CBOR decode error");
    assert!(mismatches.is_empty(), "expected ok, got: {mismatches:#?}");
}

fn cbor_fail(cbor: &[u8], schema_src: &str) {
    let schema = parse(schema_src);
    let mismatches = check_cbor(cbor, &schema, None, &Config::strict()).expect("CBOR decode error");
    assert!(!mismatches.is_empty(), "expected mismatch but got none");
}

fn json_ok(json: &[u8], schema_src: &str) {
    let schema = parse(schema_src);
    let mismatches = check_json(json, &schema, None, &Config::strict()).expect("JSON parse error");
    assert!(mismatches.is_empty(), "expected ok, got: {mismatches:#?}");
}

fn json_fail(json: &[u8], schema_src: &str) {
    let schema = parse(schema_src);
    let mismatches = check_json(json, &schema, None, &Config::strict()).expect("JSON parse error");
    assert!(!mismatches.is_empty(), "expected mismatch but got none");
}

// ─── RFC 8610 §1 — Introductory examples ─────────────────────────────────────

// RFC 8610 §1.1 — simple type rule (first example in the spec).
#[test]
fn rfc8610_first_example_uint() {
    valid("my-type = uint");
}

// RFC 8610 §2 — basic type definitions.
#[test]
fn rfc8610_basic_prelude_types() {
    valid(
        "a = uint\n\
         b = nint\n\
         c = int\n\
         d = tstr\n\
         e = bstr\n\
         f = bool\n\
         g = null\n\
         h = float\n\
         i = any",
    );
}

// ─── RFC 8610 §3 — Structure examples ────────────────────────────────────────

// RFC 8610 §3.3 — Geography record (array with embedded map).
// The example in the RFC defines a GPS coordinate pair.
#[test]
fn rfc8610_geography_parses_and_validates() {
    valid(
        "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\n\
         GpsCoordinates = { longitude: uint, latitude: uint }",
    );
}

// RFC 8610 §3.3 — A valid CBOR Geography value:
//   ["London", {longitude: 1234567, latitude: 5678901}]
//   CBOR: 82               -- array(2)
//          66 4C6F6E646F6E  -- text("London")
//          a2               -- map(2)
//           69 6C6F6E676974756465  -- text("longitude")
//           1A 0012D687      -- uint(1234567)
//           68 6C61746974756465    -- text("latitude")
//           1A 00569C75      -- uint(5678901)
#[test]
fn rfc8610_geography_valid_cbor() {
    cbor_ok(
        &[
            0x82, // array(2)
            0x66, 0x4c, 0x6f, 0x6e, 0x64, 0x6f, 0x6e, // "London"
            0xa2, // map(2)
            0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74, 0x75, 0x64, 0x65, // "longitude"
            0x1a, 0x00, 0x12, 0xd6, 0x87, // uint(1234567)
            0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, // "latitude"
            0x1a, 0x00, 0x56, 0x9c, 0x75, // uint(5678901)
        ],
        "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\n\
         GpsCoordinates = { longitude: uint, latitude: uint }",
    );
}

// RFC 8610 §3.3 — Invalid: text where uint expected.
#[test]
fn rfc8610_geography_wrong_coord_type() {
    cbor_fail(
        &[
            0x82, // array(2)
            0x66, 0x4c, 0x6f, 0x6e, 0x64, 0x6f, 0x6e, // "London"
            0xa2, // map(2)
            0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74, 0x75, 0x64, 0x65, // "longitude"
            0x64, 0x74, 0x65, 0x73, 0x74, // text "test" (wrong type!)
            0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, // "latitude"
            0x0a, // uint(10)
        ],
        "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\n\
         GpsCoordinates = { longitude: uint, latitude: uint }",
    );
}

// RFC 8610 §3.5 — PersonalData with optional fields and group inclusion.
#[test]
fn rfc8610_personal_data_parses() {
    valid(
        "PersonalData = {\n\
           ? displayName: tstr,\n\
           NameComponents,\n\
           ? age: uint,\n\
         }\n\
         NameComponents = (\n\
           ? firstName: tstr,\n\
           ? familyName: tstr,\n\
         )",
    );
}

// RFC 8610 §3.5 — valid JSON for PersonalData (all fields optional).
#[test]
fn rfc8610_personal_data_minimal_json() {
    json_ok(
        b"{}",
        "PersonalData = {\n\
           ? displayName: tstr,\n\
           NameComponents,\n\
           ? age: uint,\n\
         }\n\
         NameComponents = (\n\
           ? firstName: tstr,\n\
           ? familyName: tstr,\n\
         )",
    );
}

// RFC 8610 §3.5 — full JSON for PersonalData.
#[test]
fn rfc8610_personal_data_full_json() {
    json_ok(
        br#"{"displayName": "Alice Smith", "firstName": "Alice", "familyName": "Smith", "age": 30}"#,
        "PersonalData = {\n\
           ? displayName: tstr,\n\
           NameComponents,\n\
           ? age: uint,\n\
         }\n\
         NameComponents = (\n\
           ? firstName: tstr,\n\
           ? familyName: tstr,\n\
         )",
    );
}

// RFC 8610 §3.5 — valid CBOR PersonalData: empty map `{}` = 0xa0.
// All fields are optional so an empty map is valid.
#[test]
fn rfc8610_personal_data_minimal_cbor() {
    cbor_ok(
        &[0xa0], // map(0)
        "PersonalData = {\n\
           ? displayName: tstr,\n\
           NameComponents,\n\
           ? age: uint,\n\
         }\n\
         NameComponents = (\n\
           ? firstName: tstr,\n\
           ? familyName: tstr,\n\
         )",
    );
}

// RFC 8610 §3.5 — CBOR PersonalData with all fields:
// {"displayName": "Alice", "firstName": "Alice", "familyName": "Smith", "age": 30}
#[test]
fn rfc8610_personal_data_full_cbor() {
    cbor_ok(
        &[
            0xa4, // map(4)
            0x6b, 0x64, 0x69, 0x73, 0x70, 0x6c, 0x61, 0x79, 0x4e, 0x61, 0x6d,
            0x65, // "displayName"
            0x65, 0x41, 0x6c, 0x69, 0x63, 0x65, // "Alice"
            0x69, 0x66, 0x69, 0x72, 0x73, 0x74, 0x4e, 0x61, 0x6d, 0x65, // "firstName"
            0x65, 0x41, 0x6c, 0x69, 0x63, 0x65, // "Alice"
            0x6a, 0x66, 0x61, 0x6d, 0x69, 0x6c, 0x79, 0x4e, 0x61, 0x6d, 0x65, // "familyName"
            0x65, 0x53, 0x6d, 0x69, 0x74, 0x68, // "Smith"
            0x63, 0x61, 0x67, 0x65, // "age"
            0x18, 0x1e, // uint(30)
        ],
        "PersonalData = {\n\
           ? displayName: tstr,\n\
           NameComponents,\n\
           ? age: uint,\n\
         }\n\
         NameComponents = (\n\
           ? firstName: tstr,\n\
           ? familyName: tstr,\n\
         )",
    );
}

// ─── RFC 8610 Appendix H — Reputon (compact form) ────────────────────────────

// RFC 8610 Appendix H — compact reputon schema (verbatim from the RFC).
const REPUTON_COMPACT: &str = r#"
reputation-object = {
  application: text
  reputons: [* reputon]
}

reputon = {
  rater: text
  assertion: text
  rated: text
  rating: float16
  ? confidence: float16
  ? normal-rating: float16
  ? sample-size: uint
  ? generated: uint
  ? expires: uint
  * text => any
}
"#;

#[test]
fn rfc8610_reputon_parses_and_validates() {
    valid(REPUTON_COMPACT);
}

// RFC 8610 Appendix H — minimal valid reputon JSON.
#[test]
fn rfc8610_reputon_minimal_json() {
    json_ok(
        br#"{
          "application": "antispam",
          "reputons": [
            {
              "rater": "example.com",
              "assertion": "is-spam",
              "rated": "spammer@example.com",
              "rating": 0.98
            }
          ]
        }"#,
        REPUTON_COMPACT,
    );
}

// RFC 8610 Appendix H — invalid: missing required "application" field.
#[test]
fn rfc8610_reputon_missing_required_field() {
    json_fail(br#"{"reputons": []}"#, REPUTON_COMPACT);
}

// ─── RFC 8610 §3.2 — Choice and union types ───────────────────────────────────

// RFC 8610 §3.2 — type choice with `/`.
#[test]
fn rfc8610_type_choice_validates() {
    valid("coin = 0 / 1");
}

// RFC 8610 §3.2 — group choice with `//`.
#[test]
fn rfc8610_group_choice_validates() {
    valid("point = {x: uint, y: uint}");
}

// RFC 8610 §3.2 — additive type extension with `/=`.
#[test]
fn rfc8610_type_extension_validates() {
    valid("attire = \"bow tie\"\nattire /= \"necktie\"");
}

// ─── RFC 8610 §3.4 — Generics ────────────────────────────────────────────────

// RFC 8610 §3.4 — generic rule definition.
#[test]
fn rfc8610_generic_rule_validates() {
    valid("pair<a, b> = [a, b]");
}

// RFC 8610 §3.4 — generic rule with named parameters.
#[test]
fn rfc8610_generic_rule_with_usage() {
    valid("pair<a, b> = [a, b]\nmy-pair = pair<tstr, uint>");
}

// ─── RFC 8610 §3.6 — Tagged values ───────────────────────────────────────────

// RFC 8610 §3.6 — tagged CBOR value.
#[test]
fn rfc8610_tagged_type_validates() {
    valid("tagged-date = #6.0(tstr)");
}

// RFC 8610 §3.6 — major type reference.
#[test]
fn rfc8610_major_type_validates() {
    valid("any-text = #3");
}

// ─── RFC 8610 §3.8 — Control operators ───────────────────────────────────────

// RFC 8610 §3.8.1 — .size operator.
#[test]
fn rfc8610_size_operator() {
    valid("fixed-str = tstr .size 8");
}

#[test]
fn rfc8610_size_range_operator() {
    valid("short-str = tstr .size (1..100)");
}

// RFC 8610 §3.8.2 — .regexp operator.
#[test]
fn rfc8610_regexp_operator() {
    valid(r#"date-string = tstr .regexp "[0-9]{4}-[0-9]{2}-[0-9]{2}""#);
}

// RFC 8610 §3.8.3 — .cbor operator.
#[test]
fn rfc8610_cbor_operator() {
    valid("encoded-point = bstr .cbor GpsCoordinates\nGpsCoordinates = { x: uint, y: uint }");
}

// RFC 8610 §3.8.5 — .and operator (type intersection).
#[test]
fn rfc8610_and_operator() {
    valid("named-uint = uint .and tstr");
}

// RFC 8610 §3.8.5 — .within operator (containment).
// The RHS must be type2; a range must be parenthesised.
#[test]
fn rfc8610_within_operator() {
    valid("small-uint = uint .within (0..100)");
}

// RFC 8610 §3.8.4 — comparison operators.
#[test]
fn rfc8610_comparison_operators() {
    valid(
        "positive = uint .gt 0\n\
         at-most-100 = uint .le 100\n\
         non-zero = uint .ne 0\n\
         exactly-42 = uint .eq 42",
    );
}

// RFC 8610 §3.8.6 — .default operator, from the spec's own example
// (`? displayed-step: (number .gt 0) .default 1`). `.default` is only
// meaningful in an optional (`?`) context (Stage 1 Finding 9 — human
// ruling: a non-optional use is an error), so the example must be a `?`
// group entry rather than a bare top-level rule.
#[test]
fn rfc8610_default_operator() {
    valid("with-default = { ? displayed-step: (number .gt 0) .default 1 }");
}

#[test]
fn rfc8610_default_operator_outside_optional_context_is_error() {
    let schema = parse("with-default = uint .default 0");
    let diags = validate(&schema);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        !errors.is_empty(),
        "expected an error for `.default` outside an optional context"
    );
    assert!(errors[0].message.contains(".default"));
}

// ─── RFC 8610 §3.8.6 — .bits operator ────────────────────────────────────────

#[test]
fn rfc8610_bits_operator() {
    valid("flags = uint .bits flag-bits\nflag-bits = &(option-a: 0, option-b: 1, option-c: 2)");
}

// ─── RFC 8610 §3.8 — Recursive types ─────────────────────────────────────────

// RFC 8610 §3.4 — recursive definition (linked list).
#[test]
fn rfc8610_recursive_type() {
    valid("list = [* item]\nitem = uint / list");
}

// ─── RFC 8610 §3.3 — Occurrence indicators ───────────────────────────────────

// RFC 8610 §3.3 — `*` zero-or-more.
#[test]
fn rfc8610_occurrence_star() {
    valid("bag = [* tstr]");
}

// RFC 8610 §3.3 — `+` one-or-more.
#[test]
fn rfc8610_occurrence_plus() {
    valid("nonempty-bag = [+ tstr]");
}

// RFC 8610 §3.3 — `?` optional.
#[test]
fn rfc8610_occurrence_optional() {
    valid("maybe-str = [? tstr]");
}

// RFC 8610 §3.3 — bounded occurrence `m*n`.
#[test]
fn rfc8610_occurrence_bounded() {
    valid("two-to-four = [2*4 uint]");
}

// RFC 8610 §3.3 — bounded occurrence enforced against actual CBOR data:
// below the minimum count must fail.
#[test]
fn rfc8610_occurrence_bounded_array_below_min_fails() {
    // [2*4 uint] against a 1-element array — below the minimum of 2.
    cbor_fail(&[0x81, 0x01], "x = [2*4 uint]");
}

// RFC 8610 §3.3 — bounded occurrence enforced against actual CBOR data:
// above the maximum count must fail.
#[test]
fn rfc8610_occurrence_bounded_array_above_max_fails() {
    // [2*4 uint] against a 5-element array — above the maximum of 4.
    cbor_fail(&[0x85, 0x01, 0x02, 0x03, 0x04, 0x05], "x = [2*4 uint]");
}

// RFC 8610 §3.3 — bounded occurrence enforced against actual CBOR data:
// a count within range must succeed.
#[test]
fn rfc8610_occurrence_bounded_array_within_range_ok() {
    cbor_ok(&[0x83, 0x01, 0x02, 0x03], "x = [2*4 uint]");
}

// RFC 8610 §3.3 — bounded occurrence on a map's key/value entry, enforced
// against actual CBOR data (distinct code path from the array case above —
// `try_match_map_choice` rather than `try_match_array_choice`).
#[test]
fn rfc8610_occurrence_bounded_map_below_min_fails() {
    // { 2*3 tstr => uint } against a 1-pair map — below the minimum of 2.
    cbor_fail(&[0xa1, 0x61, 0x61, 0x01], "x = { 2*3 tstr => uint }");
}

#[test]
fn rfc8610_occurrence_bounded_map_above_max_fails() {
    // 4 pairs — above the maximum of 3.
    cbor_fail(
        &[
            0xa4, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x61, 0x63, 0x03, 0x61, 0x64, 0x04,
        ],
        "x = { 2*3 tstr => uint }",
    );
}

// ─── RFC 8610 §3.8 — .eq and .ne operators (CBOR) ────────────────────────────

// CBOR uint(42) = 0x182a — should match `uint .eq 42`
#[test]
fn rfc8610_eq_operator_cbor() {
    cbor_ok(&[0x18, 0x2a], "x = uint .eq 42");
}

// CBOR uint(99) should not match `uint .eq 42`
#[test]
fn rfc8610_eq_operator_cbor_mismatch() {
    cbor_fail(&[0x18, 0x63], "x = uint .eq 42");
}

// CBOR uint(0) should not match `uint .ne 0`
#[test]
fn rfc8610_ne_operator_cbor_mismatch() {
    cbor_fail(&[0x00], "x = uint .ne 0");
}

// CBOR uint(1) should match `uint .ne 0`
#[test]
fn rfc8610_ne_operator_cbor_ok() {
    cbor_ok(&[0x01], "x = uint .ne 0");
}

// ─── RFC 9165 — Additional control operators ─────────────────────────────────

// RFC 9165 §2.1 — .plus operator (numeric).
// The RFC defines it for adding to base values; validation just checks it
// parses without unknown-operator warnings in rfc9165 mode.
#[test]
fn rfc9165_plus_operator_no_warning_with_flag() {
    let src = "x = uint .plus 1";
    let schema = parse(src);
    let diags = validate_with_config(
        &schema,
        &Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
    let warns: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(warns.is_empty(), "unexpected warnings: {warns:?}");
}

// RFC 9165 §2.1 — .plus operator warns without the flag.
#[test]
fn rfc9165_plus_operator_warns_without_flag() {
    let src = "x = uint .plus 1";
    let schema = parse(src);
    let diags = validate_with_config(&schema, &Config::strict());
    let warns: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(
        !warns.is_empty(),
        "expected warning for .plus without rfc9165 flag"
    );
}

// RFC 9165 §2.2 — .cat operator (string concatenation).
#[test]
fn rfc9165_cat_operator_no_warning_with_flag() {
    let src = r#"x = "foo" .cat "bar""#;
    let schema = parse(src);
    let diags = validate_with_config(
        &schema,
        &Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
    let warns: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(warns.is_empty(), "unexpected warnings: {warns:?}");
}

// RFC 9165 §2.3 — .det operator (deterministic concatenation).
#[test]
fn rfc9165_det_operator() {
    valid_cfg(
        r#"x = "foo" .det "bar""#,
        Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
}

// RFC 9165 §2.4 — .feature operator.
#[test]
fn rfc9165_feature_operator() {
    valid_cfg(
        "x = uint .feature \"optional-support\"",
        Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
}

// RFC 9165 §4, Figure 6 — bare string controller yields a name with no detail.
#[test]
fn rfc9165_feature_bare_string_name_only() {
    let schema = parse(r#"x = uint .feature "optional-support""#);
    let (_diags, features) = validate_with_config_ex(&schema, &Config::all());
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].name, "optional-support");
    assert_eq!(features[0].detail, None);
}

// RFC 9165 §4, Figure 6 — `[name, detail]` array controller.
#[test]
fn rfc9165_feature_array_name_and_detail() {
    let schema = parse(
        "foo = {\n  kind: bar / baz .feature ([\"foo-extensions\", \"bazify\"])\n}\n\
         bar = int\nbaz = int\n",
    );
    let (_diags, features) = validate_with_config_ex(&schema, &Config::all());
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].name, "foo-extensions");
    assert_eq!(features[0].detail, Some("bazify".to_owned()));
}

// RFC 9165 §4 — collection happens regardless of --rfc9165 (informational,
// independent of whether the RFC-9165-opt-in warning fires).
#[test]
fn rfc9165_feature_collected_without_rfc9165_flag() {
    let schema = parse(r#"x = uint .feature "optional-support""#);
    let (_diags, features) = validate_with_config_ex(
        &schema,
        &Config {
            rfc9165: false,
            rfc9682: false,
        },
    );
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].name, "optional-support");
}

// RFC 9165 §4 — repeated same-name usages are each collected (dedup is a
// presentation concern for callers, not the validator's job).
#[test]
fn rfc9165_feature_repeated_name_collected_each_time() {
    let schema = parse(r#"x = uint .feature "dup" / tstr .feature "dup""#);
    let (_diags, features) = validate_with_config_ex(&schema, &Config::all());
    assert_eq!(features.len(), 2);
    assert!(features.iter().all(|f| f.name == "dup"));
}

// RFC 9165 §2.5 — .abnf operator (ABNF string constraint).
#[test]
fn rfc9165_abnf_operator() {
    valid_cfg(
        r#"date = tstr .abnf "date-time""#,
        Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
}

// RFC 9165 §2.5 — .abnfb operator (ABNF byte-string constraint).
#[test]
fn rfc9165_abnfb_operator() {
    valid_cfg(
        r#"oid = bstr .abnfb "oid""#,
        Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
}

// All RFC 9165 operators together (using all_extensions config).
#[test]
fn rfc9165_all_operators_no_warnings_with_all_extensions() {
    let schemas = [
        "x = uint .plus 1",
        r#"x = "a" .cat "b""#,
        r#"x = "a" .det "b""#,
        r#"x = tstr .abnf "date-time""#,
        r#"x = bstr .abnfb "oid""#,
        r#"x = uint .feature "f""#,
    ];
    for src in &schemas {
        let schema = parse(src);
        let diags = validate_with_config(&schema, &Config::all());
        let warns: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
        assert!(warns.is_empty(), "unexpected warning for {src}: {warns:?}");
    }
}

// ─── RFC 9682 — Grammar corrections ──────────────────────────────────────────

// RFC 9682 §3.1 — empty document is allowed with rfc9682 flag.
#[test]
fn rfc9682_empty_document_allowed_with_flag() {
    let schema = parse("");
    let diags = validate_with_config(
        &schema,
        &Config {
            rfc9165: false,
            rfc9682: true,
        },
    );
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        errors.is_empty(),
        "expected no errors for empty doc with rfc9682: {errors:?}"
    );
}

// RFC 9682 §3.1 — empty document is rejected under RFC 8610 strict mode.
#[test]
fn rfc9682_empty_document_rejected_without_flag() {
    let schema = parse("");
    let diags = validate_with_config(&schema, &Config::strict());
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        !errors.is_empty(),
        "expected error for empty doc without rfc9682"
    );
}

// RFC 9682 §3.2 — non-literal tag numbers require rfc9682 flag.
#[test]
fn rfc9682_non_literal_tag_requires_flag() {
    let src = "my-tag = #6.<uint>(tstr)";
    let schema = parse(src);
    let diags_strict = validate_with_config(&schema, &Config::strict());
    let diags_ext = validate_with_config(
        &schema,
        &Config {
            rfc9165: false,
            rfc9682: true,
        },
    );
    let errors_strict: Vec<_> = diags_strict
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    let errors_ext: Vec<_> = diags_ext
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    assert!(
        !errors_strict.is_empty(),
        "expected error for non-literal tag in strict mode"
    );
    assert!(
        errors_ext.is_empty(),
        "expected no error for non-literal tag with rfc9682: {errors_ext:?}"
    );
}

// ─── RFC 8610 §3.4 — Standard prelude completeness ───────────────────────────

// RFC 8610 Appendix D — all prelude names must be valid references.
#[test]
fn rfc8610_prelude_completeness() {
    let prelude_types = [
        "uint",
        "nint",
        "int",
        "bstr",
        "bytes",
        "tstr",
        "text",
        "tdate",
        "time",
        "number",
        "biguint",
        "bignint",
        "bigint",
        "integer",
        "unsigned",
        "eb64url",
        "eb64legacy",
        "eb16",
        "encoded-cbor",
        "uri",
        "b64url",
        "b64legacy",
        "regexp",
        "mime-message",
        "cbor-any",
        "float16",
        "float32",
        "float64",
        "float16-32",
        "float32-64",
        "float",
        "false",
        "true",
        "bool",
        "nil",
        "null",
        "undefined",
        "any",
    ];
    for name in &prelude_types {
        let src = format!("x = {name}");
        let schema = parse(&src);
        let diags = validate(&schema);
        let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
        assert!(
            errors.is_empty(),
            "prelude name `{name}` caused errors: {errors:?}"
        );
    }
}

// ─── Cross-reference: CBOR check with RFC 8949 encodings ─────────────────────

// RFC 8949 Appendix A — CBOR null matches CDDL `null`.
#[test]
fn rfc8949_null_matches_cddl_null() {
    cbor_ok(&[0xf6], "x = null");
}

// RFC 8949 Appendix A — CBOR true matches CDDL `bool`.
#[test]
fn rfc8949_true_matches_cddl_bool() {
    cbor_ok(&[0xf5], "x = bool");
}

// RFC 8949 Appendix A — CBOR false matches CDDL `bool`.
#[test]
fn rfc8949_false_matches_cddl_bool() {
    cbor_ok(&[0xf4], "x = bool");
}

// RFC 8949 Appendix A — CBOR text "IETF" (0x6449455446) matches `tstr`.
#[test]
fn rfc8949_ietf_string_matches_tstr() {
    cbor_ok(&[0x64, 0x49, 0x45, 0x54, 0x46], "x = tstr");
}

// RFC 8949 Appendix A — tagged RFC 3339 date matches `#6.0(tstr)`.
#[test]
fn rfc8949_tagged_date_matches_cddl() {
    // 0xc0 = tag(0), then the date string
    let mut cbor = vec![0xc0u8];
    let date = "2013-03-21T20:04:00Z";
    cbor.push(0x60 | date.len() as u8);
    cbor.extend_from_slice(date.as_bytes());
    cbor_ok(&cbor, "x = #6.0(tstr)");
}

// RFC 8949 Appendix A — tag(1) with uint matches CDDL `#6.1(uint)`.
#[test]
fn rfc8949_epoch_tag_matches_cddl() {
    // tag(1) + uint(1363896240) = 0xc11a514b67b0
    cbor_ok(&[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0], "x = #6.1(uint)");
}

// RFC 8949 — bignum tag(2) wrapping bytes.
#[test]
fn rfc8949_bignum_tag_matches_cddl() {
    // tag(2) + bytes([0x01, 0x00...]) = positive bignum
    cbor_ok(
        &[
            0xc2, 0x49, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        "x = biguint",
    );
}

// ─── External-tool comparison findings — see CDDL_TOOL_COMPARISON.md ────────
//
// The following pin down `tps_cddl`'s own behavior on cases where the Rust
// `cddl` crate (anweiss/cddl 0.10.6), the Ruby `cddl` gem (0.12.6), the
// Python `cddl_verifier` tool, and/or `@cbortech/cbor` (0.26.8) disagreed
// with each other during a manual comparison run. `tps_cddl` matched either
// the RFC text directly or a documented strictness policy in every case; the
// tests below guard against regressing to any of the observed divergent
// behaviors (undetected undefined references, silently-accepted duplicate
// rules, silently-dropped unknown control operators).

// RFC 8610 §2.2 only defines rule *extension* via `/=`/`//=`; reusing bare
// `=` for a name that already has a rule is a conflicting redefinition, not
// an extension (§2.2.2: "additional alternatives can be added ... later ...
// by using '/=' and '//='" — implying plain `=` is for first definition
// only). The Python `cddl_verifier` tool silently accepts this
// (last-definition-wins, no diagnostic); the Ruby gem detects it but only as
// an uncaught `RuntimeError` backtrace, not a structured diagnostic.
// `tps_cddl` must reject it as a validation error.
#[test]
fn rfc8610_duplicate_rule_definition_is_error() {
    let schema = parse("color = \"red\"\ncolor = \"blue\"");
    let diags = validate(&schema);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        !errors.is_empty(),
        "expected an error for a rule redefined with bare `=`"
    );
}

// RFC 8610 §2.2.2's `/=` example (`attire /= "swimwear"`) explicitly permits
// reusing `/=` when the name doesn't exist yet ("It is not an error if a
// name is first used with a '/=' or '//=' "), so this must NOT be flagged as
// a duplicate.
#[test]
fn rfc8610_slash_equals_first_use_is_not_duplicate() {
    valid("attire /= \"bow tie\"");
}

// RFC 8610's ABNF (Appendix B) makes an identifier reference syntactically
// valid regardless of whether a rule with that name exists, but a reference
// to an undefined name can never be checked against actual data — every
// worked example in the RFC defines every name it uses. The Rust `cddl`
// crate reports this schema "conformant" (does not detect it at all); the
// Python `cddl_verifier` tool resolves the reference silently with no
// warning. `tps_cddl` must reject it.
#[test]
fn rfc8610_undefined_type_reference_is_error() {
    let schema = parse("person = {\n  name: tstr,\n  dept: department,\n}");
    let diags = validate(&schema);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        !errors.is_empty(),
        "expected an error for a reference to an undefined type `department`"
    );
}

// RFC 8610 §6.1: control-operator names not in the IANA registry are simply
// "Unassigned" — not a grammar violation — so `tps_cddl` treats an unknown
// control operator as a warning (schema likely has a typo or targets a
// future/private extension) rather than a hard parse error. This is a
// deliberate middle ground: the Rust `cddl` crate hard-rejects unknown
// control operators as a parse error (stricter than the registry text
// requires), while the Python `cddl_verifier` tool silently drops the
// constraint with no diagnostic at all (more permissive than is safe for a
// schema author to rely on). `tps_cddl` must warn, not silently ignore and
// not hard-fail.
#[test]
fn rfc8610_unknown_control_operator_is_warning_not_error() {
    let schema = parse("x = tstr .custom tstr");
    let diags = validate(&schema);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    let warns: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(
        errors.is_empty(),
        "unknown control operator should not be a hard error: {errors:?}"
    );
    assert!(
        !warns.is_empty(),
        "expected a warning for the unregistered control operator `.custom`"
    );
}
