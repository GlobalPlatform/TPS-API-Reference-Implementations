/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Integration tests for manifest-driven mandatory `.feature` (RFC 9165 §4)
//! resolution in codegen (`gen_decoder`/`gen_encoder`).

use tps_cddl::{Config, FeatureManifest, GenError, build_cddl, gen_decoder, gen_encoder, lex};

fn parse(src: &str) -> tps_cddl::Cddl {
    let pairs = lex(src).expect("lex failed");
    build_cddl(pairs).expect("build failed")
}

fn manifest(json: &str) -> FeatureManifest {
    FeatureManifest::from_json(json).expect("valid manifest JSON")
}

// ─── Resolved manifest — disabled variant omitted, enabled variant kept ──────

#[test]
fn disabled_choice_is_omitted_from_generated_union() {
    let cddl =
        parse(r#"id = uint .feature "numeric" / tstr .feature "textual" / bool .feature "flag""#);
    let m = manifest(r#"{"numeric": "disabled", "textual": "enabled", "flag": "enabled"}"#);
    let code = gen_decoder(&cddl, None, &Config::strict(), &m).unwrap();

    assert!(code.contains("enum Id"), "expected enum Id in:\n{code}");
    assert!(
        !code.contains("Uint(u64)"),
        "disabled `numeric` variant should be omitted from:\n{code}"
    );
    assert!(
        code.contains("Tstr"),
        "enabled `textual` variant should survive in:\n{code}"
    );
    assert!(
        code.contains("Bool"),
        "enabled `flag` variant should survive in:\n{code}"
    );
}

#[test]
fn enabled_choice_generates_as_if_unconstrained() {
    let cddl = parse(r#"id = uint .feature "numeric" / tstr"#);
    let m = manifest(r#"{"numeric": "enabled"}"#);
    let code = gen_decoder(&cddl, None, &Config::strict(), &m).unwrap();

    let baseline_cddl = parse("id = uint / tstr");
    let baseline = gen_decoder(&baseline_cddl, None, &Config::strict(), &manifest("{}")).unwrap();
    assert_eq!(
        code, baseline,
        "enabled `.feature` choice should codegen identically to an unconstrained one"
    );
}

#[test]
fn encoder_omits_disabled_choice_too() {
    let cddl =
        parse(r#"id = uint .feature "numeric" / tstr .feature "textual" / bool .feature "flag""#);
    let m = manifest(r#"{"numeric": "disabled", "textual": "enabled", "flag": "enabled"}"#);
    let code = gen_encoder(&cddl, None, &Config::strict(), &m).unwrap();

    assert!(code.contains("enum Id"), "expected enum Id in:\n{code}");
    assert!(
        !code.contains("Uint(u64)"),
        "disabled `numeric` variant should be omitted from:\n{code}"
    );
}

// ─── Unresolved feature name — hard error, not a silent guess ────────────────

#[test]
fn unresolved_feature_name_is_a_hard_error() {
    let cddl = parse(r#"id = uint .feature "numeric" / tstr"#);
    let err = gen_decoder(&cddl, None, &Config::strict(), &manifest("{}")).unwrap_err();

    let GenError::UnresolvedFeatures(names) = &err;
    assert_eq!(names, &["numeric".to_owned()]);
    assert!(
        err.to_string().contains("numeric"),
        "error message should name the unresolved feature: {err}"
    );
}

#[test]
fn unresolved_feature_name_is_a_hard_error_for_encoder_too() {
    let cddl = parse(r#"id = uint .feature "numeric" / tstr"#);
    let err = gen_encoder(&cddl, None, &Config::strict(), &manifest("{}")).unwrap_err();

    let GenError::UnresolvedFeatures(names) = &err;
    assert_eq!(names, &["numeric".to_owned()]);
}

#[test]
fn schema_with_no_feature_usage_needs_no_manifest_entries() {
    let cddl = parse("id = uint / tstr");
    let code = gen_decoder(&cddl, None, &Config::strict(), &manifest("{}")).unwrap();
    assert!(code.contains("enum Id"));
}

// ─── RFC 9165 §4 Figure 9 — SenML `JC<J,C>`-style mutually exclusive union ───
//
// Every choice is `.feature`-guarded and exactly one survives pruning.
// `prune_disabled_features` runs before `classify`/`emit_rule`, so a union
// pruned down to a single surviving choice is classified (and emitted) as
// whatever that lone choice actually is — here a prelude alias, not a
// redundant single-variant enum. This "collapse" falls out of reusing the
// existing classify/emit pipeline on the pruned rule, rather than needing a
// dedicated collapse step.

#[test]
fn mutually_exclusive_feature_union_prunes_to_surviving_choice() {
    let cddl = parse(r#"jc = int .feature "json" / tstr .feature "cbor""#);
    let m = manifest(r#"{"json": "disabled", "cbor": "enabled"}"#);
    let code = gen_decoder(&cddl, None, &Config::strict(), &m).unwrap();

    assert!(
        !code.contains("enum Jc"),
        "single surviving choice should not generate a redundant enum:\n{code}"
    );
    assert!(
        code.contains("pub type Jc"),
        "single surviving `tstr` choice should collapse to a prelude alias:\n{code}"
    );
}
