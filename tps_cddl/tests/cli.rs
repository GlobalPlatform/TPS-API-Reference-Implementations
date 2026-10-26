/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Integration tests for the `cddl validate` CLI subcommand.
//!
//! Each test invokes the compiled binary against a fixture file and checks
//! exit code and output.

use std::process::Command;

fn cddl_bin() -> Command {
    // Use the binary built by cargo; CARGO_BIN_EXE_cddl is set by the test harness.
    Command::new(env!("CARGO_BIN_EXE_cddl"))
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = cddl_bin()
        .args(args)
        .output()
        .expect("failed to run cddl binary");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (code, stdout, stderr)
}

fn fixture(name: &str) -> String {
    format!("tests/fixtures/{name}")
}

// ─── Valid documents ──────────────────────────────────────────────────────────

#[test]
fn valid_file_exits_zero() {
    let (code, stdout, _) = run(&["validate", &fixture("valid_person.cddl")]);
    assert_eq!(code, 0, "expected exit 0 for valid file");
    assert!(stdout.contains("ok"), "expected 'ok' in output");
}

#[test]
fn valid_file_quiet_exits_zero_no_output() {
    let (code, stdout, stderr) = run(&["validate", "--quiet", &fixture("valid_person.cddl")]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "expected no stdout with --quiet");
    assert!(stderr.is_empty(), "expected no stderr with --quiet");
}

#[test]
fn valid_file_dump_ast_contains_rule_name() {
    let (code, stdout, _) = run(&["validate", "--dump-ast", &fixture("valid_person.cddl")]);
    assert_eq!(code, 0);
    assert!(stdout.contains("person"), "expected rule name in AST dump");
}

// ─── Parse errors ─────────────────────────────────────────────────────────────

#[test]
fn parse_error_exits_one() {
    let (code, _, stderr) = run(&["validate", &fixture("error_parse.cddl")]);
    assert_eq!(code, 1, "expected exit 1 for parse error");
    assert!(!stderr.is_empty(), "expected error message on stderr");
}

#[test]
fn parse_error_quiet_exits_one_no_output() {
    let (code, stdout, stderr) = run(&["validate", "--quiet", &fixture("error_parse.cddl")]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

// ─── Semantic errors ──────────────────────────────────────────────────────────

#[test]
fn undefined_name_exits_one() {
    let (code, _, stderr) = run(&["validate", &fixture("error_undefined.cddl")]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("undefined name"),
        "expected undefined-name message"
    );
}

#[test]
fn duplicate_definition_exits_one() {
    let (code, _, stderr) = run(&["validate", &fixture("error_duplicate.cddl")]);
    assert_eq!(code, 1);
    assert!(stderr.contains("redefined"), "expected redefined message");
}

// ─── Warnings ─────────────────────────────────────────────────────────────────

#[test]
fn unknown_ctlop_warns_exits_zero() {
    let (code, _, stderr) = run(&["validate", &fixture("warn_unknown_ctlop.cddl")]);
    assert_eq!(code, 0, "warning should not cause non-zero exit");
    assert!(
        stderr.contains("unknown control operator"),
        "expected warning text"
    );
}

#[test]
fn rfc9165_flag_suppresses_unknown_ctlop_warning() {
    let (code, _, stderr) = run(&["--rfc9165", "validate", &fixture("warn_unknown_ctlop.cddl")]);
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("unknown control operator"),
        "warning should be suppressed"
    );
}

#[test]
fn all_extensions_flag_suppresses_unknown_ctlop_warning() {
    let (code, _, stderr) = run(&[
        "--all-extensions",
        "validate",
        &fixture("warn_unknown_ctlop.cddl"),
    ]);
    assert_eq!(code, 0);
    assert!(!stderr.contains("unknown control operator"));
}

// ─── Missing file ─────────────────────────────────────────────────────────────

#[test]
fn missing_file_exits_one() {
    let (code, _, stderr) = run(&["validate", "nonexistent.cddl"]);
    assert_eq!(code, 1);
    assert!(
        !stderr.is_empty(),
        "expected error message for missing file"
    );
}

// ─── check-cbor subcommand ────────────────────────────────────────────────────

#[test]
fn check_cbor_valid_binary_exits_zero() {
    let (code, stdout, _) = run(&[
        "check-cbor",
        &fixture("valid_person.cddl"),
        &fixture("valid_person.cbor"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("ok"));
}

#[test]
fn check_cbor_valid_hex_exits_zero() {
    let (code, stdout, _) = run(&[
        "check-cbor",
        "--hex",
        &fixture("valid_person.cddl"),
        &fixture("valid_person.hex"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("ok"));
}

#[test]
fn check_cbor_mismatch_exits_one() {
    let (code, _, stderr) = run(&[
        "check-cbor",
        &fixture("valid_person.cddl"),
        &fixture("mismatch_person.cbor"),
    ]);
    assert_eq!(code, 1);
    assert!(!stderr.is_empty(), "expected mismatch message");
}

#[test]
fn check_cbor_invalid_schema_exits_one() {
    let (code, _, _) = run(&[
        "check-cbor",
        &fixture("error_undefined.cddl"),
        &fixture("valid_person.cbor"),
    ]);
    assert_eq!(code, 1);
}

#[test]
fn check_cbor_quiet_flag_suppresses_output() {
    let (code, stdout, stderr) = run(&[
        "check-cbor",
        "--quiet",
        &fixture("valid_person.cddl"),
        &fixture("valid_person.cbor"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn check_cbor_mismatch_quiet_exits_one_no_output() {
    let (code, stdout, stderr) = run(&[
        "check-cbor",
        "--quiet",
        &fixture("valid_person.cddl"),
        &fixture("mismatch_person.cbor"),
    ]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

// ─── check-json subcommand ────────────────────────────────────────────────────

#[test]
fn check_json_valid_exits_zero() {
    let (code, stdout, _) = run(&[
        "check-json",
        &fixture("valid_person.cddl"),
        &fixture("valid_person.json"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("ok"));
}

#[test]
fn check_json_mismatch_exits_one() {
    let (code, _, stderr) = run(&[
        "check-json",
        &fixture("valid_person.cddl"),
        &fixture("mismatch_person.json"),
    ]);
    assert_eq!(code, 1);
    assert!(!stderr.is_empty(), "expected mismatch message");
}

#[test]
fn check_json_invalid_json_exits_one() {
    let (code, _, stderr) = run(&[
        "check-json",
        &fixture("valid_person.cddl"),
        &fixture("invalid.json"),
    ]);
    assert_eq!(code, 1);
    assert!(
        !stderr.is_empty(),
        "expected error message for invalid JSON"
    );
}

#[test]
fn check_json_quiet_suppresses_output() {
    let (code, stdout, stderr) = run(&[
        "check-json",
        "--quiet",
        &fixture("valid_person.cddl"),
        &fixture("valid_person.json"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn check_json_mismatch_quiet_exits_one_no_output() {
    let (code, stdout, stderr) = run(&[
        "check-json",
        "--quiet",
        &fixture("valid_person.cddl"),
        &fixture("mismatch_person.json"),
    ]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

// ─── gen-encoder subcommand ───────────────────────────────────────────────────

#[test]
fn gen_encoder_exits_zero() {
    let (code, _, _) = run(&["gen-encoder", &fixture("gen_decoder_basic.cddl")]);
    assert_eq!(code, 0);
}

#[test]
fn gen_encoder_stdout_contains_struct() {
    let (code, stdout, _) = run(&["gen-encoder", &fixture("gen_decoder_basic.cddl")]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("struct Person"),
        "expected struct Person in output: {stdout}"
    );
}

#[test]
fn gen_encoder_stdout_contains_encode_item_impl() {
    let (code, stdout, _) = run(&["gen-encoder", &fixture("gen_decoder_basic.cddl")]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("EncodeItem"),
        "expected EncodeItem impl in output: {stdout}"
    );
}

#[test]
fn gen_encoder_invalid_schema_exits_one() {
    let (code, _, _) = run(&["gen-encoder", &fixture("error_undefined.cddl")]);
    assert_eq!(code, 1);
}

#[test]
fn gen_encoder_rule_filter() {
    let (code, stdout, _) = run(&[
        "gen-encoder",
        "--rule",
        "person",
        &fixture("gen_decoder_basic.cddl"),
    ]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("struct Person"),
        "expected Person: {stdout}"
    );
    assert!(
        !stdout.contains("struct Coords"),
        "should not contain Coords: {stdout}"
    );
    assert!(
        !stdout.contains("enum Id"),
        "should not contain Id: {stdout}"
    );
}

#[test]
fn gen_encoder_output_file() {
    let out_path = std::env::temp_dir().join("cddl_test_gen_encoder.rs");
    let out_path_str = out_path.to_string_lossy().into_owned();
    let (code, stdout, _) = run(&[
        "gen-encoder",
        "--output",
        &out_path_str,
        &fixture("gen_decoder_basic.cddl"),
    ]);
    assert_eq!(code, 0);
    assert!(
        stdout.is_empty(),
        "stdout should be empty when --output is given"
    );
    let content = std::fs::read_to_string(&out_path).expect("output file not created");
    assert!(
        content.contains("struct Person"),
        "file should contain struct Person"
    );
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn gen_decoder_exits_zero() {
    let (code, _, _) = run(&["gen-decoder", &fixture("gen_decoder_basic.cddl")]);
    assert_eq!(code, 0);
}

#[test]
fn gen_decoder_stdout_contains_struct() {
    let (code, stdout, _) = run(&["gen-decoder", &fixture("gen_decoder_basic.cddl")]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("struct Person"),
        "expected struct Person in output"
    );
}

#[test]
fn gen_decoder_invalid_schema_exits_one() {
    let (code, _, _) = run(&["gen-decoder", &fixture("error_undefined.cddl")]);
    assert_eq!(code, 1);
}

#[test]
fn gen_decoder_rule_filter() {
    let (code, stdout, _) = run(&[
        "gen-decoder",
        "--rule",
        "person",
        &fixture("gen_decoder_basic.cddl"),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("struct Person"), "expected Person");
    assert!(
        !stdout.contains("struct Coords"),
        "should not contain Coords"
    );
    assert!(!stdout.contains("enum Id"), "should not contain Id");
}

#[test]
fn gen_decoder_output_file() {
    let out_path = std::env::temp_dir().join("cddl_test_gen_decoder.rs");
    let out_path_str = out_path.to_string_lossy().into_owned();
    let (code, stdout, _) = run(&[
        "gen-decoder",
        "--output",
        &out_path_str,
        &fixture("gen_decoder_basic.cddl"),
    ]);
    assert_eq!(code, 0);
    assert!(
        stdout.is_empty(),
        "stdout should be empty when --output is given"
    );
    let content = std::fs::read_to_string(&out_path).expect("output file not created");
    assert!(
        content.contains("struct Person"),
        "file should contain struct Person"
    );
    let _ = std::fs::remove_file(&out_path);
}

// ─── Modules (`--include-path`) ───────────────────────────────────────────────
//
// A CLI-level ambiguous-match test is deliberately not included here, for the same
// cross-platform-fixture reason as the unit test in `src/modules.rs` (Step 9.3):
// this repo's filesystem (Windows/NTFS in particular) cannot reliably host two
// committed fixtures whose names differ only by case. That guarantee is covered by
// the pure, filesystem-free unit test instead.

#[cfg(feature = "modules")]
#[test]
fn modules_file_validates_via_include_path() {
    let (code, stdout, stderr) = run(&[
        "validate",
        "--include-path",
        "tests/fixtures/modules_cli/uses_include_path",
        &fixture("modules_cli/uses_include_path/mydata.cddl"),
    ]);
    assert_eq!(code, 0, "expected exit 0, stderr: {stderr}");
    assert!(stdout.contains("ok"), "expected 'ok' in output: {stdout}");
}

#[cfg(feature = "modules")]
#[test]
fn modules_missing_module_exits_one() {
    let (code, _, stderr) = run(&[
        "validate",
        &fixture("modules_cli/missing_module/mydata.cddl"),
    ]);
    assert_eq!(code, 1, "expected exit 1, stderr: {stderr}");
    assert!(
        stderr.contains("does_not_exist"),
        "expected the missing module's name in stderr: {stderr}"
    );
    assert!(
        stderr.contains("searched") || stderr.contains("not found"),
        "expected the searched directories to be named in stderr: {stderr}"
    );
}

#[cfg(feature = "modules")]
#[test]
fn modules_circular_import_exits_one() {
    let (code, _, stderr) = run(&[
        "validate",
        "--include-path",
        "tests/fixtures/modules_cli/circular",
        &fixture("modules_cli/circular/cycle_a.cddl"),
    ]);
    assert_eq!(code, 1, "expected exit 1, stderr: {stderr}");
    assert!(
        stderr.contains("cycle_a") && stderr.contains("cycle_b"),
        "expected the cycle to be named in stderr: {stderr}"
    );
}

#[cfg(feature = "modules")]
#[test]
fn modules_malformed_directive_warns_exits_zero() {
    let (code, _, stderr) = run(&[
        "validate",
        &fixture("modules_cli/malformed_directive/mydata.cddl"),
    ]);
    assert_eq!(code, 0, "malformed directive should warn, not error");
    assert!(
        stderr.contains("malformed"),
        "expected malformed-directive warning text: {stderr}"
    );
}
