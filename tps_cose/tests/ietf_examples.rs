/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#[allow(dead_code)]
mod common;

#[cfg(feature = "ring")]
use common::ietf_examples::{ExecutionStatus, VectorResult};
#[cfg(feature = "ring")]
use common::ietf_examples::{
    load_supported_encrypt0_cases, load_supported_sign1_cases, run_encrypt0_cases, run_sign1_cases,
};

#[cfg(feature = "ring")]
fn assert_results(label: &str, results: &[VectorResult]) {
    let mismatches: Vec<&VectorResult> = results
        .iter()
        .filter(|result| !result.matches_expectation())
        .collect();
    if mismatches.is_empty() {
        return;
    }
    let mut msg = format!("{label}: {} mismatches\n", mismatches.len());
    for item in mismatches {
        let observed = match &item.status {
            ExecutionStatus::Success => "success".to_string(),
            ExecutionStatus::Error(err) => format!("error ({err})"),
        };
        msg.push_str(&format!(
            "- {} ({}) expected_fail={} observed={}\n",
            item.id, item.title, item.expected_fail, observed
        ));
    }
    panic!("{msg}");
}

#[cfg(feature = "ring")]
#[test]
fn sign1_ietf_examples_es256() {
    let cases = load_supported_sign1_cases().expect("load sign1 example vectors");
    assert!(!cases.is_empty(), "no supported sign1 examples found");
    let results = run_sign1_cases(&cases);
    assert_results("sign1", &results);
}

#[cfg(feature = "ring")]
#[test]
fn encrypt0_ietf_examples_a128gcm() {
    let cases = load_supported_encrypt0_cases().expect("load encrypt0 example vectors");
    assert!(!cases.is_empty(), "no supported encrypt0 examples found");
    let results = run_encrypt0_cases(&cases);
    assert_results("encrypt0", &results);
}
