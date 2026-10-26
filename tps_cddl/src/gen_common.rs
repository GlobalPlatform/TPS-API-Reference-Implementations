/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Shared helpers for `gen_decoder` and `gen_encoder`.
//!
//! This module contains:
//! - Rule dependency collection and topological ordering
//! - Rule classification (`RuleKind`)
//! - Rust type-string derivation from CDDL AST nodes
//! - Naming utilities (snake_case → PascalCase, keyword escaping)
//! - Lifetime-inference heuristics

use std::collections::HashSet;

use crate::ast::{
    Cddl, ConstraintOp, Group, GrpEnt, GrpEntBody, MemberKey, Rule, RuleBody, Type, Type1, Type2,
    Value as CddlValue,
};
use crate::feature_manifest::FeatureManifest;
use crate::validator::{FeatureUsage, extract_feature_usage};

// ─── Rule collection & ordering ───────────────────────────────────────────────

pub fn collect_ordered(cddl: &Cddl, root: Option<&str>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();

    match root {
        Some(name) => collect_deps(name, cddl, &mut seen, &mut order),
        None => {
            for rule in &cddl.rules {
                collect_deps(&rule.name, cddl, &mut seen, &mut order);
            }
        }
    }
    order
}

pub(crate) fn collect_deps(
    name: &str,
    cddl: &Cddl,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    if seen.contains(name) || is_prelude(name) {
        return;
    }
    seen.insert(name.to_string());

    if let Some(rule) = cddl.rules.iter().find(|r| r.name == name) {
        for dep in rule_dependencies(rule) {
            collect_deps(&dep, cddl, seen, order);
        }
    }
    order.push(name.to_string());
}

pub(crate) fn rule_dependencies(rule: &Rule) -> Vec<String> {
    let mut deps = Vec::new();
    match &rule.body {
        RuleBody::Type(ty) => collect_type_deps(ty, &mut deps),
        RuleBody::Group(ent) => {
            let dummy_group = Group {
                choices: vec![crate::ast::GrpChoice {
                    entries: vec![ent.clone()],
                    span: ent.span.clone(),
                }],
                span: ent.span.clone(),
            };
            collect_group_deps(&dummy_group, &mut deps);
        }
    }
    deps
}

pub(crate) fn collect_type_deps(ty: &Type, out: &mut Vec<String>) {
    for t1 in &ty.choices {
        collect_type2_deps(&t1.base, out);
        if let Some(c) = &t1.constraint {
            collect_type2_deps(&c.target, out);
        }
    }
}

fn collect_type2_deps(t2: &Type2, out: &mut Vec<String>) {
    match t2 {
        Type2::Typename { name, args, .. } => {
            if args.is_none() {
                out.push(name.clone());
            }
        }
        Type2::ParenType(ty) => collect_type_deps(ty, out),
        Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => collect_group_deps(g, out),
        Type2::Tagged { inner, .. } => collect_type_deps(inner, out),
        Type2::EnumRef { name, .. } => out.push(name.clone()),
        Type2::Unwrap { name, .. } => out.push(name.clone()),
        _ => {}
    }
}

pub(crate) fn collect_group_deps(group: &Group, out: &mut Vec<String>) {
    for choice in &group.choices {
        for entry in &choice.entries {
            if let Some(MemberKey::Type { key, .. }) = &entry.key {
                collect_type2_deps(&key.base, out);
            }
            match &entry.entry {
                GrpEntBody::Type(ty) => collect_type_deps(ty, out),
                GrpEntBody::GroupRef { name, args, .. } => {
                    if args.is_none() {
                        out.push(name.clone());
                    }
                }
                GrpEntBody::InlineGroup(g) => collect_group_deps(g, out),
            }
        }
    }
}

// ─── Rule classification ───────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum RuleKind {
    /// `x = uint` — single prelude name alias
    PreludeAlias,
    /// `x = { … }` — map rule → struct
    Map,
    /// `x = [ T1, T2 ]` — fixed-length array → tuple struct
    FixedArray,
    /// `x = [ * T ]` or `[ + T ]` — repeated homogeneous array
    RepeatedArray,
    /// `x = A / B` — union → enum
    Union,
    /// Group rules, generic rules, complex cases — skip with comment
    Unsupported,
}

pub fn classify(rule: &Rule) -> RuleKind {
    if rule.generic_parm.is_some() {
        return RuleKind::Unsupported;
    }
    match &rule.body {
        RuleBody::Group(_) => RuleKind::Unsupported,
        RuleBody::Type(ty) => classify_type(ty),
    }
}

pub fn classify_type(ty: &Type) -> RuleKind {
    if ty.choices.len() > 1 {
        return RuleKind::Union;
    }
    let t1 = &ty.choices[0];
    if t1.constraint.is_some() {
        return RuleKind::Unsupported;
    }
    classify_type2(&t1.base)
}

pub fn classify_type2(t2: &Type2) -> RuleKind {
    match t2 {
        Type2::Typename { name, args, .. } if args.is_none() && is_prelude(name) => {
            RuleKind::PreludeAlias
        }
        Type2::Map(_) => RuleKind::Map,
        Type2::Array(g) => classify_array_group(g),
        Type2::ParenType(ty) => classify_type(ty),
        _ => RuleKind::Unsupported,
    }
}

pub fn classify_array_group(group: &Group) -> RuleKind {
    use crate::ast::Occur;

    if group.choices.len() != 1 {
        return RuleKind::Unsupported;
    }
    let choice = &group.choices[0];

    // Repeated array: single entry with * or + occur
    if choice.entries.len() == 1 {
        let entry = &choice.entries[0];
        if matches!(
            &entry.occur,
            Some(Occur::OneOrMore)
                | Some(Occur::Bounded {
                    lower: Some(0),
                    upper: None
                })
                | Some(Occur::Bounded {
                    lower: None,
                    upper: None
                })
        ) {
            return RuleKind::RepeatedArray;
        }
    }

    // Check for star wildcard (unbounded)
    let has_star = choice.entries.iter().any(|e| {
        matches!(
            e.occur,
            Some(Occur::Bounded {
                lower: None,
                upper: None
            }) | Some(Occur::Bounded {
                lower: Some(0),
                upper: None
            })
        )
    });
    if has_star {
        return RuleKind::Unsupported;
    }

    RuleKind::FixedArray
}

// ─── Struct field extraction helpers ─────────────────────────────────────────

pub fn map_group(rule: &Rule) -> Option<&Group> {
    match &rule.body {
        RuleBody::Type(ty) if ty.choices.len() == 1 => {
            if let Type2::Map(g) = &ty.choices[0].base {
                Some(g)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn array_group(rule: &Rule) -> Option<&Group> {
    match &rule.body {
        RuleBody::Type(ty) if ty.choices.len() == 1 => {
            if let Type2::Array(g) = &ty.choices[0].base {
                Some(g)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── Type → Rust string mapping ───────────────────────────────────────────────

/// Returns `(rust_type_string, needs_lifetime)` for a CDDL type2 node.
pub fn type2_to_rust_type(t2: &Type2, cddl: &Cddl) -> (String, bool) {
    match t2 {
        Type2::Any(_) => ("tps_minicbor::Item<'a>".to_string(), true),
        Type2::Value(v) => match v {
            CddlValue::Int(n) if *n >= 0 => ("u64".to_string(), false),
            CddlValue::Int(_) => ("i64".to_string(), false),
            CddlValue::Float(_) => ("f64".to_string(), false),
            CddlValue::Text(_) => ("&'a str".to_string(), true),
            CddlValue::Bytes(_) => ("&'a [u8]".to_string(), true),
        },
        Type2::Typename { name, args, .. } => {
            if args.is_some() {
                return ("tps_minicbor::Item<'a>".to_string(), true);
            }
            if is_prelude(name) {
                let rt = prelude_rust_type(name);
                let nl = rt.contains("'a");
                (rt, nl)
            } else {
                let rn = to_pascal_case(name);
                let nl = rule_needs_lifetime(name, cddl);
                let lt = if nl { "<'a>" } else { "" };
                (format!("{rn}{lt}"), nl)
            }
        }
        Type2::Map(_) | Type2::Array(_) => ("tps_minicbor::Item<'a>".to_string(), true),
        Type2::Tagged { inner, .. } => {
            if let Some(t1) = inner.choices.first() {
                type2_to_rust_type(&t1.base, cddl)
            } else {
                ("tps_minicbor::Item<'a>".to_string(), true)
            }
        }
        Type2::ParenType(inner) => {
            if inner.choices.len() == 1 {
                type2_to_rust_type(&inner.choices[0].base, cddl)
            } else {
                ("tps_minicbor::Item<'a>".to_string(), true)
            }
        }
        _ => ("tps_minicbor::Item<'a>".to_string(), true),
    }
}

pub fn prelude_rust_type(name: &str) -> String {
    match name {
        "uint" | "unsigned" => "u64",
        "int" | "integer" => "i64",
        "nint" => "i64",
        "tstr" | "text" | "tdate" | "time" | "uri" | "b64url" | "b64legacy" | "regexp"
        | "mime-message" => "&'a str",
        "bstr" | "bytes" | "encoded-cbor" => "&'a [u8]",
        "bool" | "true" | "false" => "bool",
        "null" | "nil" | "undefined" => "()",
        "float" | "float16" | "float32" | "float64" => "f64",
        "number" => "f64",
        "any" => "tps_minicbor::Item<'a>",
        "biguint" | "bignint" | "bigint" => "i64",
        _ => "tps_minicbor::Item<'a>",
    }
    .to_string()
}

pub fn is_prelude(name: &str) -> bool {
    matches!(
        name,
        "uint"
            | "int"
            | "nint"
            | "tstr"
            | "text"
            | "bstr"
            | "bytes"
            | "bool"
            | "true"
            | "false"
            | "null"
            | "nil"
            | "undefined"
            | "float"
            | "float16"
            | "float32"
            | "float64"
            | "number"
            | "any"
            | "tdate"
            | "time"
            | "uri"
            | "b64url"
            | "b64legacy"
            | "regexp"
            | "mime-message"
            | "cbor-any"
            | "encoded-cbor"
            | "biguint"
            | "bignint"
            | "bigint"
            | "integer"
            | "unsigned"
    )
}

// ─── `.cbor` / `.cborseq` constraint helpers (RFC 8610 §3.8.4) ────────────────

/// If `t1`'s constraint is a `.cbor`/`.cborseq` control operator, returns the
/// controller type and whether it's a sequence (`.cborseq`, `true`) or a
/// single item (`.cbor`, `false`). Returns `None` for any other constraint
/// (or no constraint at all).
pub fn cbor_constraint(t1: &Type1) -> Option<(&Type2, bool)> {
    let c = t1.constraint.as_ref()?;
    match &c.op {
        ConstraintOp::Control(name) if name == "cbor" => Some((&c.target, false)),
        ConstraintOp::Control(name) if name == "cborseq" => Some((&c.target, true)),
        _ => None,
    }
}

// ─── `.regexp` constraint helper (RFC 8610 §3.8.3) ────────────────────────────

/// If `t1`'s constraint is a `.regexp` control operator, returns the XSD
/// pattern's controller type. No XSD-conformant regex engine is available to
/// depend on yet (see `CODE_REVIEW_CAMPAIGN.md` Finding 4), so codegen never
/// enforces this constraint — callers use this to emit a "not enforced" TODO
/// comment in generated code instead.
pub fn regexp_constraint(t1: &Type1) -> Option<&Type2> {
    let c = t1.constraint.as_ref()?;
    match &c.op {
        ConstraintOp::Control(name) if name == "regexp" => Some(&c.target),
        _ => None,
    }
}

/// Appends a "not enforced" TODO comment to `out` if `t1` carries a
/// `.regexp` constraint. `context` identifies the field/element for the
/// reader (e.g. `` `foo` in `Bar` ``).
pub fn emit_regexp_todo(out: &mut String, t1: &Type1, context: &str) {
    if regexp_constraint(t1).is_some() {
        out.push_str(&format!(
            "// TODO: implement regex matching for `.regexp` constraint on {context} — not enforced\n"
        ));
    }
}

// ─── `.default` constraint helper (RFC 8610 §3.8.6) ───────────────────────────

/// If `t1`'s constraint is a `.default` control operator with a literal
/// controller, returns that literal value. Non-literal controllers (e.g. a
/// typename or an expression) can't be rendered as a Rust literal, so
/// callers get `None` and fall back to treating the field as plain
/// `Option<T>` with no default substitution.
pub fn default_constraint(t1: &Type1) -> Option<&CddlValue> {
    let c = t1.constraint.as_ref()?;
    match &c.op {
        ConstraintOp::Control(name) if name == "default" => match &c.target {
            Type2::Value(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

/// Renders a `.default` literal value as a Rust expression suitable both for
/// substituting an absent decoded field and for comparing an encoded field
/// against its default.
pub fn default_value_rust_literal(v: &CddlValue) -> String {
    match v {
        CddlValue::Int(n) => n.to_string(),
        CddlValue::Float(f) => format!("{f}f64"),
        CddlValue::Text(s) => format!("{s:?}"),
        CddlValue::Bytes(b) => format!("&{}[..]", byte_string_literal(&b.bytes)),
    }
}

/// The concrete shape of a rule used as a `.cborseq` array-shaped controller.
/// `.cborseq`'s wire format is the sequence's items concatenated with no
/// array framing (RFC 8610 §3.8.4), so encode/decode codegen needs to loop
/// over the target's elements individually rather than reusing its own
/// `Decode`/`EncodeItem` impl (which frames them as a CBOR array).
pub enum CborseqShape {
    /// `[* T]` / `[+ T]` — homogeneous, `Vec<T>` behind tuple field `.0`.
    Repeated,
    /// `[T0, T1, ...]` — heterogeneous fixed tuple, one field per index.
    Fixed(usize),
}

/// Resolves a `.cborseq` controller type to its array shape, or `None` if it
/// isn't a reference to a rule codegen recognises as array-shaped (inline
/// array literals and non-array controllers aren't supported by codegen —
/// callers fall back to the unconstrained `bstr` representation).
pub fn cborseq_shape(t2: &Type2, cddl: &Cddl) -> Option<CborseqShape> {
    let name = match t2 {
        Type2::Typename {
            name, args: None, ..
        } => name,
        _ => return None,
    };
    let rule = cddl.rules.iter().find(|r| r.name == *name)?;
    match classify(rule) {
        RuleKind::RepeatedArray => Some(CborseqShape::Repeated),
        RuleKind::FixedArray => {
            let group = array_group(rule)?;
            let count = group.choices.first()?.entries.len();
            Some(CborseqShape::Fixed(count))
        }
        _ => None,
    }
}

/// Resolves a `.cbor`/`.cborseq`-constrained `Type1`'s controller to a
/// `(rust_type, needs_lifetime)` pair for the field it constrains, or `None`
/// if the controller shape isn't one codegen supports (falls back to the
/// unconstrained `bstr` representation).
///
/// `.cbor` supports any controller type whose Rust representation isn't the
/// generic `Item<'a>` fallback (i.e. anything with a concrete `Decode`/
/// `EncodeItem` impl). `.cborseq` additionally requires the controller to be
/// array-shaped (see [`cborseq_shape`]) — codegen decodes/encodes straight
/// against the field's own byte slice (no reassembly into an owned buffer),
/// so a borrowed controller type is fine.
pub fn cbor_field_rust_type(target: &Type2, is_seq: bool, cddl: &Cddl) -> Option<(String, bool)> {
    let (rt, nl) = type2_to_rust_type(target, cddl);
    if rt == "tps_minicbor::Item<'a>" {
        return None;
    }
    if is_seq && cborseq_shape(target, cddl).is_none() {
        return None;
    }
    Some((rt, nl))
}

// ─── `.feature` constraint helpers (RFC 9165 §4) ──────────────────────────────

/// Error returned by [`crate::gen_decoder::gen_decoder`]/[`crate::gen_encoder::gen_encoder`]
/// when a schema's `.feature` usage can't be resolved against the supplied
/// [`FeatureManifest`].
#[derive(Debug)]
pub enum GenError {
    /// One or more `.feature` names reachable in the emitted rule closure
    /// have no entry in the feature manifest.
    UnresolvedFeatures(Vec<String>),
}

impl std::fmt::Display for GenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenError::UnresolvedFeatures(names) => write!(
                f,
                "unresolved `.feature` name(s) — add to the feature manifest: {}",
                names.join(", ")
            ),
        }
    }
}

impl std::error::Error for GenError {}

/// Collects every `.feature` (RFC 9165 §4) usage reachable in the given
/// rules' closure (as produced by [`collect_ordered`]), for cross-referencing
/// against a [`FeatureManifest`] before codegen emits anything.
pub fn collect_feature_usages(cddl: &Cddl, rule_names: &[String]) -> Vec<FeatureUsage> {
    let mut out = Vec::new();
    for name in rule_names {
        if let Some(rule) = cddl.rules.iter().find(|r| &r.name == name) {
            match &rule.body {
                RuleBody::Type(ty) => collect_type_feature_usages(ty, &mut out),
                RuleBody::Group(ent) => collect_grpent_feature_usages(ent, &mut out),
            }
        }
    }
    out
}

fn collect_type_feature_usages(ty: &Type, out: &mut Vec<FeatureUsage>) {
    for t1 in &ty.choices {
        collect_type1_feature_usages(t1, out);
    }
}

fn collect_type1_feature_usages(t1: &Type1, out: &mut Vec<FeatureUsage>) {
    if let Some(c) = &t1.constraint
        && let ConstraintOp::Control(name) = &c.op
        && name == "feature"
        && let Some(usage) = extract_feature_usage(&c.target, &c.span)
    {
        out.push(usage);
    }
    collect_type2_feature_usages(&t1.base, out);
}

fn collect_type2_feature_usages(t2: &Type2, out: &mut Vec<FeatureUsage>) {
    match t2 {
        Type2::ParenType(ty) => collect_type_feature_usages(ty, out),
        Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => {
            collect_group_feature_usages(g, out)
        }
        Type2::Tagged { inner, .. } => collect_type_feature_usages(inner, out),
        _ => {}
    }
}

fn collect_group_feature_usages(group: &Group, out: &mut Vec<FeatureUsage>) {
    for choice in &group.choices {
        for entry in &choice.entries {
            collect_grpent_feature_usages(entry, out);
        }
    }
}

fn collect_grpent_feature_usages(entry: &GrpEnt, out: &mut Vec<FeatureUsage>) {
    if let Some(MemberKey::Type { key, .. }) = &entry.key {
        collect_type1_feature_usages(key, out);
    }
    match &entry.entry {
        GrpEntBody::Type(ty) => collect_type_feature_usages(ty, out),
        GrpEntBody::InlineGroup(g) => collect_group_feature_usages(g, out),
        GrpEntBody::GroupRef { .. } => {}
    }
}

/// Verifies that every `.feature` name reachable in `rule_names`' closure has
/// an entry in `manifest`. Returns [`GenError::UnresolvedFeatures`] (deduped,
/// sorted) if not — codegen must never silently guess a resolution.
pub fn check_features_resolved(
    cddl: &Cddl,
    rule_names: &[String],
    manifest: &FeatureManifest,
) -> Result<(), GenError> {
    let usages = collect_feature_usages(cddl, rule_names);
    let mut unresolved: Vec<String> = usages
        .iter()
        .filter(|u| manifest.resolve(&u.name).is_none())
        .map(|u| u.name.clone())
        .collect();
    unresolved.sort();
    unresolved.dedup();
    if unresolved.is_empty() {
        Ok(())
    } else {
        Err(GenError::UnresolvedFeatures(unresolved))
    }
}

/// Resolves a `Type1`'s `.feature` constraint against `manifest`, or `None`
/// if it isn't a `.feature` constraint at all. Callers must only invoke this
/// after [`check_features_resolved`] has confirmed every reachable `.feature`
/// name is resolved — an unresolved name here is treated as "keep as-is"
/// rather than erroring a second time.
fn feature_resolution(t1: &Type1, manifest: &FeatureManifest) -> Option<bool> {
    let c = t1.constraint.as_ref()?;
    let ConstraintOp::Control(name) = &c.op else {
        return None;
    };
    if name != "feature" {
        return None;
    }
    let usage = extract_feature_usage(&c.target, &c.span)?;
    manifest.resolve(&usage.name)
}

/// Removes union choices whose `.feature` constraint resolves to `disabled`,
/// and clears the constraint on choices that resolve to `enabled` so they
/// codegen exactly like an unconstrained choice.
///
/// This only prunes at the top level of a union rule (`x = A .feature "f" / B`)
/// — the case the RFC 9165 §4 SenML `JC<J,C>` example (Figure 9) exists to
/// demonstrate. A `.feature` constraint on a map or array field entry is left
/// untouched: its name is still required to be resolved (via
/// [`check_features_resolved`]), but the field itself is not conditionally
/// omitted — that is not yet supported.
pub fn prune_disabled_features(rule: &Rule, manifest: &FeatureManifest) -> Rule {
    let mut rule = rule.clone();
    if let RuleBody::Type(ty) = &mut rule.body {
        ty.choices.retain_mut(|t1| match feature_resolution(t1, manifest) {
            Some(false) => false,
            Some(true) => {
                t1.constraint = None;
                true
            }
            None => true,
        });
    }
    rule
}

// ─── Union helpers ─────────────────────────────────────────────────────────────

pub(crate) fn prelude_variant_name(name: &str) -> String {
    match name {
        "uint" => "Uint",
        "int" | "nint" => "Int",
        "tstr" | "text" => "Tstr",
        "bstr" | "bytes" => "Bstr",
        "bool" | "true" | "false" => "Bool",
        "null" | "nil" => "Null",
        "float" | "float16" | "float32" | "float64" => "Float",
        "any" => "Any",
        _ => name,
    }
    .to_string()
}

/// Returns a string key that identifies the CBOR major type of a `Type2` node.
/// Used to detect when two union choices would produce the same pattern match arm.
pub fn type2_major_key(t2: &Type2) -> String {
    match t2 {
        Type2::Typename { name, .. } if is_prelude(name) => match name.as_str() {
            "uint" => "unsigned",
            "nint" => "negative",
            "int" => "int_any",
            "tstr" | "text" | "tdate" | "uri" | "b64url" | "b64legacy" => "text",
            "bstr" | "bytes" => "bytes",
            "bool" | "true" | "false" => "bool",
            "null" | "nil" => "null",
            "float" | "float16" | "float32" | "float64" => "float",
            _ => name.as_str(),
        }
        .to_string(),
        Type2::Map(_) => "map".to_string(),
        Type2::Array(_) => "array".to_string(),
        Type2::Tagged { .. } => "tag".to_string(),
        Type2::Value(v) => match v {
            CddlValue::Int(n) if *n >= 0 => "unsigned".to_string(),
            CddlValue::Int(_) => "negative".to_string(),
            CddlValue::Text(_) => "text".to_string(),
            CddlValue::Bytes(_) => "bytes".to_string(),
            CddlValue::Float(_) => "float".to_string(),
        },
        _ => "unknown".to_string(),
    }
}

pub fn union_variant_name(idx: usize, t1: &Type1, _cddl: &Cddl) -> String {
    match &t1.base {
        Type2::Typename { name, .. } => {
            if is_prelude(name) {
                prelude_variant_name(name)
            } else {
                to_pascal_case(name)
            }
        }
        Type2::Map(_) => format!("Map{idx}"),
        Type2::Array(_) => format!("Array{idx}"),
        Type2::Value(v) => match v {
            CddlValue::Int(n) => format!("Int{n}"),
            CddlValue::Text(s) => to_pascal_case(s),
            _ => format!("Variant{idx}"),
        },
        _ => format!("Variant{idx}"),
    }
}

/// Returns a `tps_minicbor::Item` match-arm pattern for decoding a union variant.
///
/// The generated pattern binds the inner value as `v`, suitable for use in
/// `match d.next()? { <pattern> => Ok(Enum::Variant(v)), ... }`.
pub fn type2_item_pattern(t2: &Type2, _cddl: &Cddl) -> String {
    match t2 {
        Type2::Typename { name, .. } if is_prelude(name) => match name.as_str() {
            "uint" => "tps_minicbor::Item::Unsigned(v)",
            "nint" => "tps_minicbor::Item::Negative(v)",
            "int" => "tps_minicbor::Item::Unsigned(v) | tps_minicbor::Item::Negative(v)",
            "tstr" | "text" | "tdate" | "uri" | "b64url" | "b64legacy" | "regexp"
            | "mime-message" | "time" => "tps_minicbor::Item::Text(v)",
            "bstr" | "bytes" | "encoded-cbor" => "tps_minicbor::Item::Bytes(v)",
            "bool" | "true" | "false" => "tps_minicbor::Item::Bool(v)",
            "null" | "nil" => "tps_minicbor::Item::Null",
            "float" | "float16" | "float32" | "float64" => "tps_minicbor::Item::Float(v)",
            _ => "item @ _",
        }
        .to_string(),
        Type2::Value(v) => match v {
            CddlValue::Int(n) if *n >= 0 => "tps_minicbor::Item::Unsigned(v)".to_string(),
            CddlValue::Int(_) => "tps_minicbor::Item::Negative(v)".to_string(),
            CddlValue::Text(_) => "tps_minicbor::Item::Text(v)".to_string(),
            CddlValue::Bytes(_) => "tps_minicbor::Item::Bytes(v)".to_string(),
            CddlValue::Float(_) => "tps_minicbor::Item::Float(v)".to_string(),
        },
        Type2::Map(_) => "tps_minicbor::Item::Map(v)".to_string(),
        Type2::Array(_) => "tps_minicbor::Item::Array(v)".to_string(),
        Type2::Tagged { .. } => "tps_minicbor::Item::Tag(v)".to_string(),
        _ => "item @ _".to_string(),
    }
}

// ─── Naming helpers ────────────────────────────────────────────────────────────

pub fn to_pascal_case(s: &str) -> String {
    let mut out = String::new();
    let mut cap_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' || ch == '.' {
            cap_next = true;
        } else if cap_next {
            out.extend(ch.to_uppercase());
            cap_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn sanitise_field_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c == '-' { '_' } else { c })
        .collect();
    if matches!(
        s.as_str(),
        "type"
            | "use"
            | "mod"
            | "fn"
            | "let"
            | "mut"
            | "pub"
            | "impl"
            | "for"
            | "in"
            | "loop"
            | "if"
            | "else"
            | "match"
            | "while"
            | "return"
            | "true"
            | "false"
            | "struct"
            | "enum"
            | "self"
            | "super"
            | "crate"
            | "where"
            | "async"
            | "await"
            | "dyn"
            | "static"
            | "const"
            | "trait"
            | "unsafe"
            | "move"
            | "ref"
            | "box"
            | "abstract"
            | "become"
            | "do"
            | "final"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    ) {
        format!("r#{s}")
    } else {
        s
    }
}

// ─── Map member-key representation ─────────────────────────────────────────────

/// A literal CDDL map member key that codegen knows how to emit — either a
/// text string (`name:` or `"name" =>`) or a byte string (`h'0102' =>`).
#[derive(Debug, Clone, PartialEq)]
pub enum MapKeyRepr {
    Text(String),
    Bytes(Vec<u8>),
}

impl MapKeyRepr {
    /// Rust field/struct-member name derived from this key.
    pub fn field_name(&self) -> String {
        match self {
            MapKeyRepr::Text(s) => sanitise_field_name(s),
            MapKeyRepr::Bytes(b) => bytes_field_name(b),
        }
    }

    /// A Rust expression of type `&str` or `&[u8]` denoting this key's literal value.
    pub fn literal_expr(&self) -> String {
        match self {
            MapKeyRepr::Text(s) => format!("{s:?}"),
            MapKeyRepr::Bytes(b) => format!("&{}[..]", byte_string_literal(b)),
        }
    }

    /// `tps_minicbor::Item` match-arm pattern that matches this key on decode.
    pub fn decode_pattern(&self) -> String {
        match self {
            MapKeyRepr::Text(s) => format!("tps_minicbor::Item::Text({s:?})"),
            MapKeyRepr::Bytes(b) => {
                format!("tps_minicbor::Item::Bytes({})", byte_string_literal(b))
            }
        }
    }
}

/// Extract the literal-key representation from a CDDL member key, if it is one
/// codegen can emit (`Bareword`, text `Value`, or byte-string `Value`). Type-based
/// wildcard keys (`MemberKey::Type`) are not literal and return `None`.
pub fn map_key_repr(key: &MemberKey) -> Option<MapKeyRepr> {
    match key {
        MemberKey::Bareword { name, .. } => Some(MapKeyRepr::Text(name.clone())),
        MemberKey::Value {
            key: CddlValue::Text(s),
            ..
        } => Some(MapKeyRepr::Text(s.clone())),
        MemberKey::Value {
            key: CddlValue::Bytes(b),
            ..
        } => Some(MapKeyRepr::Bytes(b.bytes.clone())),
        MemberKey::Value { .. } | MemberKey::Type { .. } => None,
    }
}

/// Rust byte-string literal (e.g. `b"\x01\x02"`) matching or holding the given bytes.
pub fn byte_string_literal(bytes: &[u8]) -> String {
    let mut s = String::from("b\"");
    for &b in bytes {
        match b {
            b'\\' => s.push_str("\\\\"),
            b'"' => s.push_str("\\\""),
            0x20..=0x7e => s.push(b as char),
            _ => s.push_str(&format!("\\x{b:02x}")),
        }
    }
    s.push('"');
    s
}

/// Field-name-safe identifier derived from a byte-string map key, e.g. `key_0102`.
pub fn bytes_field_name(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    sanitise_field_name(&format!("key_{hex}"))
}

// ─── Lifetime inference ────────────────────────────────────────────────────────

pub fn rule_needs_lifetime(name: &str, cddl: &Cddl) -> bool {
    let mut visited = HashSet::new();
    rule_needs_lifetime_inner(name, cddl, &mut visited)
}

fn rule_needs_lifetime_inner(name: &str, cddl: &Cddl, visited: &mut HashSet<String>) -> bool {
    if is_prelude(name) {
        return prelude_rust_type(name).contains("'a");
    }
    if visited.contains(name) {
        return false;
    }
    visited.insert(name.to_string());
    if let Some(rule) = cddl.rules.iter().find(|r| r.name == name) {
        type_needs_lifetime_check(&rule.body, cddl, visited)
    } else {
        false
    }
}

fn type_needs_lifetime_check(body: &RuleBody, cddl: &Cddl, visited: &mut HashSet<String>) -> bool {
    match body {
        RuleBody::Type(ty) => ty
            .choices
            .iter()
            .any(|t1| type2_needs_lifetime(&t1.base, cddl, visited)),
        RuleBody::Group(ent) => group_ent_needs_lifetime(ent, cddl, visited),
    }
}

fn type2_needs_lifetime(t2: &Type2, cddl: &Cddl, visited: &mut HashSet<String>) -> bool {
    match t2 {
        Type2::Typename { name, .. } => rule_needs_lifetime_inner(name, cddl, visited),
        Type2::Any(_) => true,
        Type2::Value(v) => matches!(v, CddlValue::Text(_) | CddlValue::Bytes(_)),
        Type2::Map(g) | Type2::Array(g) => group_needs_lifetime(g, cddl, visited),
        Type2::Tagged { inner, .. } => inner
            .choices
            .iter()
            .any(|t1| type2_needs_lifetime(&t1.base, cddl, visited)),
        Type2::ParenType(inner) => inner
            .choices
            .iter()
            .any(|t1| type2_needs_lifetime(&t1.base, cddl, visited)),
        _ => false,
    }
}

fn group_needs_lifetime(g: &Group, cddl: &Cddl, visited: &mut HashSet<String>) -> bool {
    g.choices.iter().any(|c| {
        c.entries.iter().any(|e| match &e.entry {
            GrpEntBody::Type(ty) => ty
                .choices
                .iter()
                .any(|t1| type2_needs_lifetime(&t1.base, cddl, visited)),
            GrpEntBody::GroupRef { name, .. } => rule_needs_lifetime_inner(name, cddl, visited),
            GrpEntBody::InlineGroup(g) => group_needs_lifetime(g, cddl, visited),
        })
    })
}

fn group_ent_needs_lifetime(ent: &GrpEnt, cddl: &Cddl, visited: &mut HashSet<String>) -> bool {
    match &ent.entry {
        GrpEntBody::Type(ty) => ty
            .choices
            .iter()
            .any(|t1| type2_needs_lifetime(&t1.base, cddl, visited)),
        GrpEntBody::GroupRef { name, .. } => rule_needs_lifetime_inner(name, cddl, visited),
        GrpEntBody::InlineGroup(g) => group_needs_lifetime(g, cddl, visited),
    }
}
