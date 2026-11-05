/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! A navigable tree view of a parsed CDDL document.
//!
//! [`build_tree`] walks a [`tps_cddl::ast::Cddl`] and produces a [`CddlNode`] per grammar
//! construct, each carrying the byte span it came from. The CDDL tab's right pane renders this as
//! a tree; clicking a node selects the matching source range.
//!
//! Ported from `tps_cbor_gui/src-tauri/src/cddl_tree.rs` with two changes:
//!
//! 1. **No serde.** The old version derived `Serialize`/`Deserialize` because every node crossed
//!    the Tauri IPC boundary as JSON. Nothing crosses a process boundary now.
//! 2. **Positions are derived, not stored.** Each node held four precomputed `usize` fields
//!    (`line`, `column`, `end_line`, `end_column`), which quadrupled the size of a tree over a
//!    17 KB schema and went stale the moment the buffer was edited. [`CddlNode::position`] computes
//!    them on demand from the span, through the one shared conversion. Same reasoning as
//!    [`crate::diag::Diagnostic`].

use std::collections::HashMap;
use std::ops::Range;

use tps_cddl::ast::{
    self, ByteEncoding, Cddl, Constraint, ConstraintOp, GenericArg, Group, GrpChoice, GrpEnt,
    GrpEntBody, HeadNumber, MemberKey, Occur, Rule, RuleBody, Span, Type, Type1, Type2, Value,
};

use crate::diag::Position;

/// One node of the CDDL parse tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CddlNode {
    /// The grammar construct this node represents, e.g. `"rule"`, `"map"`, `"typename"`.
    ///
    /// A `&'static str` rather than a `String`: the set is closed and known at compile time, so
    /// there is nothing to allocate. The old version built a `String` per node.
    pub kind: &'static str,
    /// The text to show beside the kind, where one is meaningful -- a rule name, an operator, a
    /// literal value.
    pub label: Option<String>,
    /// The byte range of the source this node was parsed from.
    pub span: Range<usize>,
    /// Child nodes, in source order.
    pub children: Vec<CddlNode>,
}

impl CddlNode {
    /// The start and end positions of this node's span within `source`.
    ///
    /// `source` must be the text the tree was built from; passing anything else yields positions
    /// that are arithmetically valid but meaningless.
    pub fn position(&self, source: &str) -> (Position, Position) {
        (
            Position::at(source, self.span.start),
            Position::at(source, self.span.end),
        )
    }

    /// The number of nodes in this subtree, including itself.
    pub fn len(&self) -> usize {
        1 + self.children.iter().map(CddlNode::len).sum::<usize>()
    }

    /// Whether this node has no children.
    ///
    /// Present because clippy requires it alongside [`CddlNode::len`]; note it is *not* `len() == 0`,
    /// which can never hold since a node always counts itself.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// The deepest node whose span contains `offset`, if any.
    ///
    /// This is what a click in the CDDL editor needs: given a caret byte offset, select the
    /// matching tree node. Ties go to the deeper node, so clicking inside a rule's type selects
    /// the type rather than the rule.
    pub fn innermost_at(&self, offset: usize) -> Option<&CddlNode> {
        if !(self.span.start <= offset && offset < self.span.end) {
            return None;
        }
        self.children
            .iter()
            .find_map(|child| child.innermost_at(offset))
            .or(Some(self))
    }
}

fn node(
    span: &Span,
    kind: &'static str,
    label: Option<String>,
    children: Vec<CddlNode>,
) -> CddlNode {
    CddlNode {
        kind,
        label,
        span: span.start..span.end,
        children,
    }
}

/// Build the tree for a parsed CDDL document.
///
/// Infallible: the document has already parsed, so every construct has a span.
pub fn build_tree(cddl: &Cddl) -> CddlNode {
    let children = cddl.rules.iter().map(rule_node).collect();
    node(&cddl.span, "document", None, children)
}

fn rule_node(rule: &Rule) -> CddlNode {
    let op = match rule.assignment {
        ast::Assignment::Assign => "=",
        ast::Assignment::TypeExtend => "/=",
        ast::Assignment::GroupExtend => "//=",
    };
    let label = match &rule.generic_parm {
        Some(gp) => format!("{}<{}> {op}", rule.name, gp.params.join(", ")),
        None => format!("{} {op}", rule.name),
    };
    let body_child = match &rule.body {
        RuleBody::Type(ty) => type_node(ty),
        RuleBody::Group(ge) => grp_ent_node(ge),
    };
    node(&rule.span, "rule", Some(label), vec![body_child])
}

fn type_node(ty: &Type) -> CddlNode {
    let children = ty.choices.iter().map(type1_node).collect();
    node(&ty.span, "type", None, children)
}

fn type1_node(t1: &Type1) -> CddlNode {
    let mut children = vec![type2_node(&t1.base, &t1.span)];
    if let Some(c) = &t1.constraint {
        children.push(constraint_node(c));
    }
    node(&t1.span, "type1", None, children)
}

fn constraint_node(c: &Constraint) -> CddlNode {
    let op_label = match &c.op {
        ConstraintOp::RangeInclusive => "..".to_owned(),
        ConstraintOp::RangeExclusive => "...".to_owned(),
        ConstraintOp::Control(name) => format!(".{name}"),
    };
    let children = vec![type2_node(&c.target, &c.span)];
    node(&c.span, "constraint", Some(op_label), children)
}

fn generic_arg_node(ga: &GenericArg) -> CddlNode {
    let children = ga.args.iter().map(type1_node).collect();
    node(&ga.span, "generic_arg", None, children)
}

fn head_number_label(hn: &HeadNumber) -> Option<String> {
    match hn {
        HeadNumber::Literal(n) => Some(n.to_string()),
        HeadNumber::Type(_) => None,
    }
}

/// A `Type2` that names something and may carry generic arguments: `typename`, `unwrap`,
/// `enum_ref`. All three have the same shape, so they share one constructor.
fn named_node(kind: &'static str, name: &str, args: Option<&GenericArg>, span: &Span) -> CddlNode {
    let children = args.map(generic_arg_node).into_iter().collect();
    node(span, kind, Some(name.to_owned()), children)
}

fn type2_node(t2: &Type2, fallback_span: &Span) -> CddlNode {
    match t2 {
        Type2::Value(v) => value_node(v, fallback_span),
        Type2::Typename { name, args, span } => named_node("typename", name, args.as_ref(), span),
        Type2::ParenType(inner) => node(&inner.span, "paren_type", None, vec![type_node(inner)]),
        Type2::Map(g) => group_node(g, "map"),
        Type2::Array(g) => group_node(g, "array"),
        Type2::Unwrap { name, args, span } => named_node("unwrap", name, args.as_ref(), span),
        Type2::EnumGroup(g) => group_node(g, "enum_group"),
        Type2::EnumRef { name, args, span } => named_node("enum_ref", name, args.as_ref(), span),
        Type2::Tagged { tag, inner, span } => {
            let label = tag
                .as_ref()
                .and_then(head_number_label)
                .map(|n| format!("#6.{n}"));
            node(span, "tagged", label, vec![type_node(inner)])
        }
        Type2::MajorType { major, ai, span } => {
            let label = match ai.as_ref().and_then(head_number_label) {
                Some(a) => format!("#{major}.{a}"),
                None => format!("#{major}"),
            };
            node(span, "major_type", Some(label), vec![])
        }
        Type2::Any(span) => node(span, "any", Some("#".to_owned()), vec![]),
    }
}

fn group_node(g: &Group, kind: &'static str) -> CddlNode {
    let children = g.choices.iter().map(grp_choice_node).collect();
    node(&g.span, kind, None, children)
}

fn grp_choice_node(gc: &GrpChoice) -> CddlNode {
    let children = gc.entries.iter().map(grp_ent_node).collect();
    node(&gc.span, "grp_choice", None, children)
}

fn occur_label(occur: &Occur) -> String {
    match occur {
        Occur::Optional => "?".to_owned(),
        Occur::OneOrMore => "+".to_owned(),
        Occur::Bounded { lower, upper } => format!(
            "{}*{}",
            lower.map(|n| n.to_string()).unwrap_or_default(),
            upper.map(|n| n.to_string()).unwrap_or_default()
        ),
    }
}

fn grp_ent_node(ge: &GrpEnt) -> CddlNode {
    let mut children = Vec::new();
    if let Some(key) = &ge.key {
        children.push(member_key_node(key));
    }
    children.push(match &ge.entry {
        GrpEntBody::Type(ty) => type_node(ty),
        GrpEntBody::GroupRef { name, args, span } => {
            named_node("group_ref", name, args.as_ref(), span)
        }
        GrpEntBody::InlineGroup(g) => group_node(g, "inline_group"),
    });
    node(
        &ge.span,
        "group_entry",
        ge.occur.as_ref().map(occur_label),
        children,
    )
}

fn member_key_node(key: &MemberKey) -> CddlNode {
    match key {
        MemberKey::Type { key, cut, span } => {
            let label = if *cut { "^ =>" } else { "=>" };
            node(
                span,
                "member_key_type",
                Some(label.to_owned()),
                vec![type1_node(key)],
            )
        }
        MemberKey::Bareword { name, span } => node(
            span,
            "member_key_bareword",
            Some(format!("{name}:")),
            vec![],
        ),
        MemberKey::Value { key, span } => {
            node(span, "member_key_value", None, vec![value_node(key, span)])
        }
    }
}

fn value_node(v: &Value, span: &Span) -> CddlNode {
    // `Value` itself carries no `Span` in the AST, so the enclosing Type2::Value or
    // MemberKey::Value span is used instead and navigation still lands correctly.
    let label = match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Text(s) => format!("\"{s}\""),
        Value::Bytes(b) => match b.encoding {
            ByteEncoding::Raw => format!("'{}'", String::from_utf8_lossy(&b.bytes)),
            ByteEncoding::Hex => format!("h'{}'", hex::encode(&b.bytes)),
            ByteEncoding::Base64 => "b64'...'".to_owned(),
        },
    };
    node(span, "value", Some(label), vec![])
}

/// One row of the flattened tree, with everything a renderer needs and nothing it must compute.
///
/// In particular `start` is precomputed. See [`flatten`] for why that matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstRow {
    /// Stable identity for this node, for keying expansion state across a rebuild.
    ///
    /// The document root is `"/"`. Below it the id is the rule name -- disambiguated with
    /// `#2`, `#3` and so on where a document defines the same name twice -- followed by the
    /// child index path within that rule: `"Claims-Set/1/0/2"`.
    ///
    /// # Why the rule name and not a pure index path
    ///
    /// Expansion state must survive a reparse of edited text. A pure index path from the root
    /// (`"0.3.1.2"`) shifts every sibling index when a rule is inserted above it, so adding one
    /// rule at the top of a file collapses the entire tree. Keying depth-1 nodes by rule name
    /// means an edit inside rule `A`, or a reordering of rules, leaves rule `B`'s subtree and its
    /// expansion untouched. `kind + span` was considered and rejected: every span after an edit
    /// shifts, making it the least stable option available.
    pub id: String,
    /// Nesting depth; 0 for the document root.
    pub depth: usize,
    /// The grammar construct, from [`CddlNode::kind`].
    pub kind: &'static str,
    /// The text to show beside the kind, from [`CddlNode::label`].
    pub label: Option<String>,
    /// The byte range in the source the tree was built from.
    pub span: Range<usize>,
    /// Where `span` starts, as a line and column.
    pub start: Position,
    /// Whether this node has children, so a renderer can draw a chevron without looking ahead.
    pub has_children: bool,
}

/// Flatten `root` into renderable rows, converting every position exactly once.
///
/// # Why this exists rather than calling [`CddlNode::position`] per row
///
/// [`Position::at`] goes through `tps_cddl::ast::offset_to_line_col`, which is an O(offset) byte
/// loop (`ast.rs:50-63`). Calling it per node per frame is quadratic in document size and visibly
/// stalls a repaint over the 17 KB `gp_eap.cddl`, whose tree runs to thousands of nodes. This
/// builds one line-start table and binary-searches it per node -- O(n log L) once per tree build
/// -- so the render closure never converts again.
///
/// `source` must be the text `root` was built from; passing anything else yields positions that
/// are arithmetically valid but meaningless, exactly as [`CddlNode::position`] documents.
pub fn flatten(root: &CddlNode, source: &str) -> Vec<AstRow> {
    // Offsets of the first byte of each line. Index 0 is always 0, so a `partition_point` over it
    // can never return 0 and the line number is the count of starts at or before the offset.
    let mut line_starts = vec![0usize];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(index, _)| index + 1),
    );

    let position = |offset: usize| {
        let offset = offset.min(source.len());
        let line = line_starts.partition_point(|&start| start <= offset);
        Position {
            line,
            column: offset - line_starts[line - 1] + 1,
        }
    };

    let mut rows = Vec::new();
    walk_rows(root, "/".to_owned(), 0, &position, &mut rows);
    rows
}

fn walk_rows(
    node: &CddlNode,
    id: String,
    depth: usize,
    position: &impl Fn(usize) -> Position,
    rows: &mut Vec<AstRow>,
) {
    rows.push(AstRow {
        depth,
        kind: node.kind,
        label: node.label.clone(),
        span: node.span.clone(),
        start: position(node.span.start),
        has_children: !node.children.is_empty(),
        id: id.clone(),
    });

    // Depth-1 children are the document's rules, and are keyed by name rather than by index. An
    // occurrence suffix keeps two rules of the same name -- which `validate_ast` reports as the
    // error it is -- from collapsing into one subtree.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (index, child) in node.children.iter().enumerate() {
        let child_id = if depth == 0 {
            let name = rule_name(child);
            let count = seen.entry(name).or_insert(0);
            *count += 1;
            if *count == 1 {
                name.to_owned()
            } else {
                format!("{name}#{count}")
            }
        } else if id == "/" {
            format!("/{index}")
        } else {
            format!("{id}/{index}")
        };
        walk_rows(child, child_id, depth + 1, position, rows);
    }
}

/// The identifying name of a depth-1 node, which is a rule whose label is `"name ="`.
///
/// Falls back to the kind for anything else, so a future grammar construct at the top level still
/// gets an id rather than an empty string.
fn rule_name(node: &CddlNode) -> &str {
    match node.label.as_deref() {
        Some(label) => label.split_once(['<', ' ']).map_or(label, |(name, _)| name),
        None => node.kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tps_cddl::{build_cddl, lex};

    fn tree(source: &str) -> CddlNode {
        let tokens = lex(source).expect("lex");
        let cddl = build_cddl(tokens).expect("parse");
        build_tree(&cddl)
    }

    #[test]
    fn a_document_node_holds_one_child_per_rule() {
        let root = tree("a = uint\nb = tstr\n");
        assert_eq!(root.kind, "document");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].label.as_deref(), Some("a ="));
        assert_eq!(root.children[1].label.as_deref(), Some("b ="));
    }

    #[test]
    fn every_span_lies_within_its_parent() {
        let source = "claims = {\n  ? 1: tstr,\n  * uint => any,\n}\n";
        let root = tree(source);

        fn walk(node: &CddlNode, parent: &Range<usize>, limit: usize) {
            assert!(
                node.span.start >= parent.start && node.span.end <= parent.end,
                "{} {:?} escapes its parent {parent:?}",
                node.kind,
                node.span
            );
            assert!(node.span.end <= limit, "{} runs past the source", node.kind);
            assert!(
                node.span.start <= node.span.end,
                "{} has an inverted span",
                node.kind
            );
            for child in &node.children {
                walk(child, &node.span, limit);
            }
        }

        walk(&root, &root.span, source.len());
    }

    #[test]
    fn a_rules_span_covers_its_own_text() {
        let source = "a = uint\nb = tstr\n";
        let root = tree(source);
        assert_eq!(&source[root.children[0].span.clone()], "a = uint");
        assert_eq!(&source[root.children[1].span.clone()], "b = tstr");
    }

    #[test]
    fn generic_parameters_appear_in_the_rule_label() {
        let root = tree("pair<K, V> = [K, V]\n");
        assert_eq!(root.children[0].label.as_deref(), Some("pair<K, V> ="));
    }

    #[test]
    fn the_assignment_operator_is_shown() {
        let root = tree("a = uint\na /= tstr\n");
        assert_eq!(root.children[0].label.as_deref(), Some("a ="));
        assert_eq!(root.children[1].label.as_deref(), Some("a /="));
    }

    #[test]
    fn maps_and_arrays_are_distinguished() {
        let root = tree("m = { a: uint }\nl = [ uint ]\n");
        let kinds = |rule: &CddlNode| {
            let mut found = Vec::new();
            fn walk(n: &CddlNode, out: &mut Vec<&'static str>) {
                out.push(n.kind);
                for c in &n.children {
                    walk(c, out);
                }
            }
            walk(rule, &mut found);
            found
        };
        assert!(kinds(&root.children[0]).contains(&"map"));
        assert!(kinds(&root.children[1]).contains(&"array"));
    }

    #[test]
    fn literal_values_are_labelled_with_their_text() {
        let root = tree("a = 42\nb = \"hi\"\nc = h'ff00'\n");
        let labels: Vec<Option<String>> = root
            .children
            .iter()
            .map(|rule| {
                fn find_value(n: &CddlNode) -> Option<String> {
                    if n.kind == "value" {
                        return n.label.clone();
                    }
                    n.children.iter().find_map(find_value)
                }
                find_value(rule)
            })
            .collect();
        assert_eq!(labels[0].as_deref(), Some("42"));
        assert_eq!(labels[1].as_deref(), Some("\"hi\""));
        assert_eq!(labels[2].as_deref(), Some("h'ff00'"));
    }

    #[test]
    fn occurrence_indicators_label_the_group_entry() {
        let root = tree("m = { ? a: uint, * b: tstr, + c: int, 2*4 d: any }\n");
        fn occurs(n: &CddlNode, out: &mut Vec<String>) {
            if n.kind == "group_entry"
                && let Some(label) = &n.label
            {
                out.push(label.clone());
            }
            for c in &n.children {
                occurs(c, out);
            }
        }
        let mut found = Vec::new();
        occurs(&root, &mut found);
        assert_eq!(found, vec!["?", "*", "+", "2*4"]);
    }

    #[test]
    fn innermost_at_finds_the_deepest_containing_node() {
        let source = "a = uint\n";
        let root = tree(source);
        // Offset 4 is inside `uint`.
        let node = root.innermost_at(4).expect("a node covers offset 4");
        assert!(node.children.is_empty(), "{} is not a leaf", node.kind);
        assert_eq!(&source[node.span.clone()], "uint");

        // Past the end of the document there is nothing.
        assert!(root.innermost_at(source.len() + 10).is_none());
    }

    #[test]
    fn positions_are_one_based_and_derived_from_the_source() {
        let source = "a = uint\nb = tstr\n";
        let root = tree(source);
        let (start, end) = root.children[1].position(source);
        assert_eq!((start.line, start.column), (2, 1));
        assert_eq!(end.line, 2);
    }

    #[test]
    fn len_counts_the_whole_subtree() {
        let root = tree("a = uint\n");
        let mut counted = 0usize;
        fn walk(n: &CddlNode, c: &mut usize) {
            *c += 1;
            for ch in &n.children {
                walk(ch, c);
            }
        }
        walk(&root, &mut counted);
        assert_eq!(root.len(), counted);
        assert!(!root.is_empty());
    }

    /// The real schema the IDE is developed against, as a smoke test that no construct in it
    /// panics or produces an out-of-range span.
    #[test]
    fn the_gp_eap_schema_builds_a_tree() {
        let source = include_str!("../../../examples/gp_eap.cddl");
        let tokens = lex(source).expect("lex gp_eap.cddl");
        let cddl = build_cddl(tokens).expect("parse gp_eap.cddl");
        let root = build_tree(&cddl);
        assert!(root.children.len() > 10, "expected many rules");
        assert!(root.len() > 100, "expected a deep tree, got {}", root.len());
        assert!(root.span.end <= source.len());
    }

    /// Guards the optimisation being subtly wrong, which would misplace every row in the tree.
    #[test]
    fn flatten_agrees_with_position_for_every_node() {
        let source = include_str!("../../../examples/gp_eap.cddl");
        let tokens = lex(source).expect("lex gp_eap.cddl");
        let cddl = build_cddl(tokens).expect("parse gp_eap.cddl");
        let root = build_tree(&cddl);

        let rows = flatten(&root, source);
        assert_eq!(rows.len(), root.len(), "one row per node");

        // Walk the tree in the same pre-order `flatten` uses and compare against the shared
        // conversion, node for node.
        fn pre_order<'a>(node: &'a CddlNode, out: &mut Vec<&'a CddlNode>) {
            out.push(node);
            for child in &node.children {
                pre_order(child, out);
            }
        }
        let mut nodes = Vec::new();
        pre_order(&root, &mut nodes);

        for (row, node) in rows.iter().zip(&nodes) {
            assert_eq!(row.span, node.span, "rows are out of order");
            assert_eq!(
                row.start,
                Position::at(source, node.span.start),
                "{} at {:?}",
                node.kind,
                node.span
            );
        }
    }

    #[test]
    fn flatten_handles_a_non_ascii_line_and_the_last_byte() {
        // "é" is two bytes, so a char-indexed table would drift. Columns count bytes, as
        // `Position` documents.
        let source = "; ééé\na = uint\n";
        let root = tree(source);
        for row in flatten(&root, source) {
            assert_eq!(row.start, Position::at(source, row.span.start));
        }
    }

    #[test]
    fn every_ast_node_id_is_unique_within_a_tree() {
        let source = include_str!("../../../examples/gp_eap.cddl");
        let tokens = lex(source).expect("lex gp_eap.cddl");
        let cddl = build_cddl(tokens).expect("parse gp_eap.cddl");
        let rows = flatten(&build_tree(&cddl), source);

        let mut seen = std::collections::HashSet::new();
        for row in &rows {
            assert!(seen.insert(row.id.clone()), "duplicate id `{}`", row.id);
        }
        assert_eq!(rows[0].id, "/");
        assert_eq!(rows[0].depth, 0);
    }

    /// The property the rule-name-prefixed scheme exists for, and the one a pure index path from
    /// the root fails: inserting a rule above `b` must not change any id inside `b`.
    #[test]
    fn an_ast_row_id_is_unchanged_by_an_edit_to_a_different_rule() {
        let before = "b = { x: uint }\n";
        let after = "a = tstr\nb = { x: uint }\n";

        let ids = |source: &str| -> Vec<String> {
            let root = tree(source);
            flatten(&root, source)
                .into_iter()
                .filter(|row| row.id.starts_with('b'))
                .map(|row| row.id)
                .collect()
        };

        let b_before = ids(before);
        assert!(b_before.len() > 3, "{b_before:?}");
        assert_eq!(b_before, ids(after));
    }

    /// Two rules of the same name are a real CDDL error, but the tree must still show both
    /// subtrees rather than collapsing them onto one id.
    #[test]
    fn a_duplicated_rule_name_gets_an_occurrence_suffix() {
        let source = "a = uint\na = tstr\n";
        let root = tree(source);
        let depth_one: Vec<String> = flatten(&root, source)
            .into_iter()
            .filter(|row| row.depth == 1)
            .map(|row| row.id)
            .collect();
        assert_eq!(depth_one, vec!["a", "a#2"]);
    }

    #[test]
    fn a_generic_rule_is_keyed_on_its_bare_name() {
        let source = "pair<K, V> = [K, V]\n";
        let root = tree(source);
        let rows = flatten(&root, source);
        assert_eq!(rows[1].id, "pair");
        assert!(rows[2].id.starts_with("pair/"), "{}", rows[2].id);
    }

    #[test]
    fn has_children_and_depth_match_the_tree() {
        let source = "m = { ? a: uint }\nl = [ tstr ]\n";
        let root = tree(source);
        let rows = flatten(&root, source);

        fn expected(node: &CddlNode, depth: usize, out: &mut Vec<(bool, usize)>) {
            out.push((!node.children.is_empty(), depth));
            for child in &node.children {
                expected(child, depth + 1, out);
            }
        }
        let mut want = Vec::new();
        expected(&root, 0, &mut want);

        let got: Vec<(bool, usize)> = rows
            .iter()
            .map(|row| (row.has_children, row.depth))
            .collect();
        assert_eq!(got, want);
    }
}
