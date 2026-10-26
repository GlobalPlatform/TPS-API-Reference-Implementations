/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Integration tests exercising the public library API.
//!
//! These tests import `cddl` as a downstream consumer would, using only the
//! symbols that are re-exported from the crate root.

use tps_cddl::{
    Config, Diagnostic, FeatureManifest, Level, build_cddl, check_cbor, check_json, gen_decoder,
    gen_encoder, lex, validate, validate_with_config,
};

fn empty_manifest() -> FeatureManifest {
    FeatureManifest::from_json("{}").unwrap()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse(src: &str) -> tps_cddl::Cddl {
    let pairs = lex(src).expect("lex failed");
    build_cddl(pairs).expect("build failed")
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("tests/fixtures").join(name)
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

#[test]
fn lex_and_build_returns_rules() {
    let cddl = parse("color = \"red\" / \"green\" / \"blue\"");
    assert_eq!(cddl.rules.len(), 1);
    assert_eq!(cddl.rules[0].name, "color");
}

#[test]
fn lex_error_propagates() {
    let result = lex("@@invalid@@");
    assert!(result.is_err(), "expected parse error for invalid input");
}

// ─── Validation ──────────────────────────────────────────────────────────────

#[test]
fn validate_valid_schema_returns_no_errors() {
    let cddl = parse("person = { name: tstr, age: uint }");
    let diags = validate(&cddl);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn validate_undefined_name_returns_error() {
    let cddl = parse("person = { name: nosuchtype }");
    let diags = validate(&cddl);
    assert!(
        diags.iter().any(|d| d.level == Level::Error),
        "expected at least one error diagnostic"
    );
}

#[test]
fn validate_with_config_rfc9165_suppresses_warning() {
    let cddl = parse("x = tstr .plus 1");
    let strict_diags = validate_with_config(&cddl, &Config::strict());
    let permissive_diags = validate_with_config(
        &cddl,
        &Config {
            rfc9165: true,
            rfc9682: false,
        },
    );
    let strict_warns = strict_diags
        .iter()
        .filter(|d| d.level == Level::Warning)
        .count();
    let permissive_warns = permissive_diags
        .iter()
        .filter(|d| d.level == Level::Warning)
        .count();
    assert!(
        strict_warns >= permissive_warns,
        "strict mode should produce at least as many warnings as permissive"
    );
}

#[test]
fn diagnostic_fields_are_accessible() {
    let cddl = parse("a = nosuchtype");
    let diags = validate(&cddl);
    assert!(!diags.is_empty());
    let d: &Diagnostic = &diags[0];
    assert_eq!(d.level, Level::Error);
    assert!(!d.message.is_empty());
}

// ─── CBOR checking ───────────────────────────────────────────────────────────

#[test]
fn check_cbor_valid_returns_empty_mismatches() {
    let cddl_src = std::fs::read_to_string(fixture_path("valid_person.cddl")).unwrap();
    let cbor_bytes = std::fs::read(fixture_path("valid_person.cbor")).unwrap();
    let cddl = parse(&cddl_src);
    let mismatches =
        check_cbor(&cbor_bytes, &cddl, None, &Config::strict()).expect("CBOR decode error");
    assert!(
        mismatches.is_empty(),
        "expected no mismatches: {mismatches:?}"
    );
}

#[test]
fn check_cbor_mismatch_returns_errors() {
    let cddl_src = std::fs::read_to_string(fixture_path("valid_person.cddl")).unwrap();
    let cbor_bytes = std::fs::read(fixture_path("mismatch_person.cbor")).unwrap();
    let cddl = parse(&cddl_src);
    let mismatches =
        check_cbor(&cbor_bytes, &cddl, None, &Config::strict()).expect("CBOR decode error");
    assert!(!mismatches.is_empty(), "expected at least one mismatch");
}

// ─── JSON checking ────────────────────────────────────────────────────────────

#[test]
fn check_json_valid_returns_empty_mismatches() {
    let cddl_src = std::fs::read_to_string(fixture_path("valid_person.cddl")).unwrap();
    let json_str = std::fs::read_to_string(fixture_path("valid_person.json")).unwrap();
    let cddl = parse(&cddl_src);
    let json_bytes = json_str.as_bytes();
    let mismatches = check_json(json_bytes, &cddl, None, &Config::strict())
        .expect("check_json failed unexpectedly");
    assert!(
        mismatches.is_empty(),
        "expected no mismatches: {mismatches:?}"
    );
}

#[test]
fn check_json_mismatch_returns_errors() {
    let cddl_src = std::fs::read_to_string(fixture_path("valid_person.cddl")).unwrap();
    let json_str = std::fs::read_to_string(fixture_path("mismatch_person.json")).unwrap();
    let cddl = parse(&cddl_src);
    let mismatches = check_json(json_str.as_bytes(), &cddl, None, &Config::strict())
        .expect("check_json failed unexpectedly");
    assert!(!mismatches.is_empty(), "expected at least one mismatch");
}

// ─── Code generation — decoder ───────────────────────────────────────────────

#[test]
fn gen_decoder_produces_struct_for_map_rule() {
    let cddl = parse("person = { name: tstr, age: uint }");
    let code = gen_decoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(
        code.contains("struct Person"),
        "expected struct Person in:\n{code}"
    );
}

#[test]
fn gen_decoder_produces_enum_for_choice_rule() {
    let cddl = parse("id = uint / tstr");
    let code = gen_decoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(code.contains("enum Id"), "expected enum Id in:\n{code}");
}

#[test]
fn gen_decoder_rule_filter_excludes_other_rules() {
    let cddl = parse("person = { name: tstr, age: uint }\ncoords = [uint, uint]");
    let code = gen_decoder(&cddl, Some("person"), &Config::strict(), &empty_manifest()).unwrap();
    assert!(code.contains("struct Person"), "expected Person: {code}");
    assert!(
        !code.contains("struct Coords"),
        "should not contain Coords: {code}"
    );
}

#[test]
fn gen_decoder_produces_field_for_bstr_map_key() {
    let cddl = parse("foo = { h'0102': uint }");
    let code = gen_decoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(
        code.contains("pub key_0102: u64"),
        "expected key_0102 field in:\n{code}"
    );
    assert!(
        code.contains("tps_minicbor::Item::Bytes(b\"\\x01\\x02\")"),
        "expected bstr decode pattern in:\n{code}"
    );
}

// ─── Code generation — encoder ───────────────────────────────────────────────

#[test]
fn gen_encoder_produces_struct_for_map_rule() {
    let cddl = parse("person = { name: tstr, age: uint }");
    let code = gen_encoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(
        code.contains("struct Person"),
        "expected struct Person in:\n{code}"
    );
}

#[test]
fn gen_encoder_produces_encode_item_impl() {
    let cddl = parse("person = { name: tstr, age: uint }");
    let code = gen_encoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(
        code.contains("EncodeItem"),
        "expected EncodeItem impl in:\n{code}"
    );
}

#[test]
fn gen_encoder_rule_filter_excludes_other_rules() {
    let cddl = parse("person = { name: tstr, age: uint }\ncoords = [uint, uint]");
    let code = gen_encoder(&cddl, Some("person"), &Config::strict(), &empty_manifest()).unwrap();
    assert!(code.contains("struct Person"), "expected Person: {code}");
    assert!(
        !code.contains("struct Coords"),
        "should not contain Coords: {code}"
    );
}

#[test]
fn gen_encoder_produces_field_for_bstr_map_key() {
    let cddl = parse("foo = { h'0102': uint }");
    let code = gen_encoder(&cddl, None, &Config::strict(), &empty_manifest()).unwrap();
    assert!(
        code.contains("pub key_0102: u64"),
        "expected key_0102 field in:\n{code}"
    );
    assert!(
        code.contains("&b\"\\x01\\x02\"[..]"),
        "expected bstr key literal in:\n{code}"
    );
}

// ─── Config ───────────────────────────────────────────────────────────────────

#[test]
fn config_strict_has_no_extensions() {
    let cfg = Config::strict();
    assert!(!cfg.rfc9165);
    assert!(!cfg.rfc9682);
}

#[test]
fn config_all_enables_all_extensions() {
    let cfg = Config::all();
    assert!(cfg.rfc9165);
    assert!(cfg.rfc9682);
}
