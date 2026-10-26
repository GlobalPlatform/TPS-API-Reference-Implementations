/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Smoke-tests the CDDL grammar against real fixtures shared with `tps_cddl`.
//!
//! `error_parse.cddl` is deliberately malformed (used to test `tps_cddl`'s
//! error path) and is the one fixture expected to contain an `ERROR` node.

use std::path::Path;

fn has_error(node: tree_sitter::Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(has_error)
}

fn parse_fixture(path: &Path) -> bool {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cddl::LANGUAGE.into())
        .expect("load CDDL grammar");
    let tree = parser.parse(&source, None).expect("parse fixture");
    has_error(tree.root_node())
}

#[test]
fn fixtures_parse_without_errors() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tps_cddl/tests/fixtures");
    let expected_to_error = ["error_parse.cddl"];

    let mut checked = 0;
    let mut stack = vec![fixtures_dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read fixtures dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("cddl") {
                continue;
            }

            let file_name = path.file_name().unwrap().to_str().unwrap();
            let had_error = parse_fixture(&path);
            checked += 1;

            if expected_to_error.contains(&file_name) {
                assert!(had_error, "{} was expected to fail to parse", path.display());
            } else {
                assert!(!had_error, "{} produced an ERROR/MISSING node", path.display());
            }
        }
    }

    assert!(checked > 0, "expected to find at least one .cddl fixture");
}
