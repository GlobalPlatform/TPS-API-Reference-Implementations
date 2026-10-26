/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Smoke-tests the EDN grammar against real fixtures adapted from
//! `tps_minicbor/tests/edn.rs` and RFC 8610 Appendix G/H.

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
        .set_language(&tree_sitter_edn::LANGUAGE.into())
        .expect("load EDN grammar");
    let tree = parser.parse(&source, None).expect("parse fixture");
    has_error(tree.root_node())
}

#[test]
fn fixtures_parse_without_errors() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");

    let mut checked = 0;
    for entry in std::fs::read_dir(&fixtures_dir).expect("read fixtures dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("edn") {
            continue;
        }

        let had_error = parse_fixture(&path);
        checked += 1;
        assert!(!had_error, "{} produced an ERROR/MISSING node", path.display());
    }

    assert!(checked > 0, "expected to find at least one .edn fixture");
}
