/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![cfg(feature = "modules")]
//! End-to-end integration tests for CDDL module resolution, exercised through the
//! real public API (`resolve_modules`), not `modules.rs`'s internal test helpers.

use std::collections::HashSet;
use std::path::PathBuf;

use tps_cddl::ast::{RuleBody, Type2};
use tps_cddl::{
    Config, FeatureManifest, ModuleSearchPath, check_cbor, gen_decoder, resolve_modules,
    validate_with_config,
};

fn fixtures_dir(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/modules")
        .join(rel)
}

fn resolve(rel_dir: &str, top_module: &str) -> tps_cddl::modules::ModuleResolution {
    let dir = fixtures_dir(rel_dir);
    let search = ModuleSearchPath::new(vec![dir.clone()]);
    resolve_modules(&dir.join(format!("{top_module}.cddl")), &search)
        .expect("module resolution should succeed")
}

fn rule_names(cddl: &tps_cddl::Cddl) -> HashSet<&str> {
    cddl.rules.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn ex1_plain_import_end_to_end() {
    let res = resolve("ex1_plain_import", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from(["start", "COSE_Key", "label", "values"])
    );
}

#[test]
fn ex2_import_as_prefix_end_to_end() {
    let res = resolve("ex2_import_as_prefix", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from(["start", "cose.COSE_Key", "cose.label", "cose.values"])
    );
}

#[test]
fn ex3_include_explicit_from_end_to_end() {
    let res = resolve("ex3_include_explicit_from", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from(["mydata", "label", "values"])
    );
}

#[test]
fn ex4_include_from_as_end_to_end() {
    let res = resolve("ex4_include_from_as", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from(["mydata", "cose.label", "cose.values"])
    );
}

#[test]
fn ex5_import_transitive_end_to_end() {
    let res = resolve("ex5_import_transitive", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from([
            "mydata",
            "cose.empty_or_serialized_map",
            "cose.header_map",
            "cose.Generic_Headers",
            "cose.label",
            "cose.values",
        ])
    );
}

#[test]
fn ex6_import_alias_end_to_end() {
    let res = resolve("ex6_import_alias", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        rule_names(&res.cddl),
        HashSet::from([
            "mydata",
            "empty_or_serialized_map",
            "cose.empty_or_serialized_map",
            "cose.header_map",
            "cose.Generic_Headers",
            "cose.label",
            "cose.values",
        ])
    );
    let alias = res
        .cddl
        .rules
        .iter()
        .find(|r| r.name == "empty_or_serialized_map")
        .expect("alias rule should exist");
    let RuleBody::Type(ty) = &alias.body else {
        panic!("expected a type rule")
    };
    assert!(matches!(
        &ty.choices[0].base,
        Type2::Typename { name, .. } if name == "cose.empty_or_serialized_map"
    ));
}

#[test]
fn source_covers_every_span_in_the_merged_cddl() {
    let res = resolve("ex5_import_transitive", "mydata");
    for rule in &res.cddl.rules {
        assert!(
            rule.span.end <= res.source.len(),
            "rule `{}` span {:?} out of bounds for source of length {}",
            rule.name,
            rule.span,
            res.source.len()
        );
    }
}

#[test]
fn gen_decoder_on_prefixed_rule_names_has_no_literal_dot() {
    let res = resolve("ex2_import_as_prefix", "mydata");
    let manifest = FeatureManifest::from_json("{}").unwrap();
    let code = gen_decoder(&res.cddl, None, &Config::strict(), &manifest).unwrap();
    assert!(
        code.contains("struct CoseCOSEKey"),
        "expected a dot-free struct name, got: {code}"
    );
    for line in code.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("struct ") {
            assert!(
                !rest.contains('.'),
                "generated struct name contains a literal `.`: {line}"
            );
        }
    }
}

// ─── Negative-path integration coverage (Step 9.10) ───────────────────────────

#[test]
fn undefined_from_name_names_module_and_bad_name() {
    let dir = fixtures_dir("undefined_from_name");
    let search = ModuleSearchPath::new(vec![dir.clone()]);
    let err = resolve_modules(&dir.join("mydata.cddl"), &search)
        .expect_err("expected an UndefinedFromName error");
    match &err {
        tps_cddl::ModuleError::UndefinedFromName { name, module, .. } => {
            assert_eq!(name, "does_not_exist");
            assert_eq!(module, "rfc9052");
        }
        other => panic!("expected UndefinedFromName, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("does_not_exist"), "message: {msg}");
    assert!(msg.contains("rfc9052"), "message: {msg}");
}

#[test]
fn duplicate_name_after_merge_is_a_validator_error() {
    let res = resolve("duplicate_after_merge", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(
        diags.iter().any(|d| d.message.contains("redefined")),
        "expected a redefined diagnostic for the merged `cose.Foo` collision: {diags:?}"
    );
}

#[test]
fn alias_colliding_with_toplevel_rule_is_a_validator_error() {
    let res = resolve("alias_collides_toplevel", "mydata");
    let diags = validate_with_config(&res.cddl, &Config::strict());
    assert!(
        diags.iter().any(|d| d.message.contains("redefined")),
        "expected a redefined diagnostic for the `Foo` alias colliding with the \
         top-level `Foo` rule: {diags:?}"
    );
}

#[test]
fn inner_module_missing_module_attributes_failure_to_inner_file() {
    let dir = fixtures_dir("inner_missing_module");
    let search = ModuleSearchPath::new(vec![dir.clone()]);
    let err = resolve_modules(&dir.join("mydata.cddl"), &search)
        .expect_err("expected a NotFound error attributed to the inner directive");
    match &err {
        tps_cddl::ModuleError::NotFound { module, .. } => {
            assert_eq!(
                module, "inner_does_not_exist",
                "error should name the module referenced by outer_mod.cddl's own \
                 directive, not mydata.cddl's"
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// ─── `;# constrain` (project-specific extension) — end-to-end via check_cbor ───
//
// Proves the whole chain works together: `;# constrain` synthesizes
// `COSE_Sign1_EAT` with `payload` narrowed to `bstr .cborseq eap.Something`,
// and `check_cbor` (Piece 0) actually validates the nested `.cborseq` payload
// against real encoded bytes.

#[test]
fn constrain_cose_sign1_eat_check_cbor_valid_matches() {
    let res = resolve("constrain_cose_sign1", "profile");

    // [ protected: bstr(len 0), unprotected: {}, payload: bstr .cborseq
    //   [tstr "hello", int 42], signature: bstr [1,2,3] ]
    let cbor: Vec<u8> = vec![
        0x84, // array(4)
        0x40, // protected: bstr(0)
        0xa0, // unprotected: map(0)
        0x48, 0x65, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x18, 0x2a, // payload: bstr(8) = "hello" ++ uint(42)
        0x43, 0x01, 0x02, 0x03, // signature: bstr(3)
    ];

    let mismatches = check_cbor(&cbor, &res.cddl, Some("COSE_Sign1_EAT"), &Config::strict())
        .expect("CBOR decode error");
    assert!(
        mismatches.is_empty(),
        "expected no mismatches: {mismatches:?}"
    );
}

#[test]
fn constrain_cose_sign1_eat_check_cbor_rejects_wrong_nested_shape() {
    let res = resolve("constrain_cose_sign1", "profile");

    // Same shape, but payload's nested sequence has an int where `Something`
    // expects a tstr for `claim` — must be reported as a mismatch, not
    // silently accepted (the whole point of narrowing `payload` at all).
    let cbor: Vec<u8> = vec![
        0x84, // array(4)
        0x40, // protected: bstr(0)
        0xa0, // unprotected: map(0)
        0x43, 0x01, 0x02, 0x03, // payload: bstr(3) = uint(1), uint(2), uint(3) — not [tstr, int]
        0x43, 0x01, 0x02, 0x03, // signature: bstr(3)
    ];

    let mismatches = check_cbor(&cbor, &res.cddl, Some("COSE_Sign1_EAT"), &Config::strict())
        .expect("CBOR decode error");
    assert!(!mismatches.is_empty(), "expected at least one mismatch");
}

