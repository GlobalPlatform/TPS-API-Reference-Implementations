/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Phase 3: match a decoded CBOR value against a CDDL schema.
//!
//! The public entry point is [`check_cbor`].  It returns a list of
//! [`CborMismatch`] values — one per structural inconsistency found.  An
//! empty list means the value conforms to the schema.

use std::collections::HashSet;
use std::ops::Range;

use tps_minicbor::{DecodeError, Decoder, Float, Item};

use crate::{
    Config,
    ast::{
        Cddl, Constraint, ConstraintOp, GenericArg, Group, GrpChoice, GrpEntBody, HeadNumber,
        MemberKey, Occur, Rule, RuleBody, Span, Type, Type1, Type2, Value, flatten_array_entries,
        flatten_map_entries, generic_bindings, substitute_generic_grpent, substitute_generic_type,
    },
};

// ─── Public types ──────────────────────────────────────────────────────────

/// A single mismatch between a CBOR value and the expected CDDL schema.
///
/// Mismatches are returned by [`check_cbor`].  The `path` field uses a
/// dot-separated / bracket-indexed syntax (e.g. `root.addr[2]`) so that
/// error messages can point to the exact location within a nested structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CborMismatch {
    /// JSON-pointer-like path to the mismatching value (e.g. `root.addr[2]`).
    pub path: String,
    /// Human-readable description of what was expected.
    pub expected: String,
    /// Human-readable description of what was found.
    pub got: String,
}

/// Absolute byte range of a CBOR value within the original input buffer, as
/// recorded by [`check_cbor_traced`]. Mirrors the CDDL-side [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CborSpan {
    /// Byte offset of the first byte (inclusive).
    pub start: usize,
    /// Byte offset just past the last byte (exclusive).
    pub end: usize,
}

/// The result of a single rule-match attempt, recorded by [`check_cbor_traced`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOutcome {
    /// The rule was entered; its outcome is recorded separately once known.
    Attempt,
    /// The rule matched — no new mismatches were recorded while matching it.
    Match,
    /// The rule failed — at least one new mismatch was recorded while matching it.
    Fail,
}

/// A single step in a [`check_cbor_traced`] validation trace: one attempt to
/// match `rule_name` against the CBOR value at `path`.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    /// Name of the rule being matched.
    pub rule_name: String,
    /// JSON-pointer-like path to the CBOR value being matched, as in [`CborMismatch::path`].
    pub path: String,
    /// Source span of the rule's own definition, if the rule is a normal
    /// (non-prelude) named rule.
    pub span: Option<Span>,
    /// Absolute byte range, within the original CBOR input, of the value
    /// being matched at `path`.
    pub cbor_span: Option<CborSpan>,
    /// Whether this attempt matched or failed.
    pub outcome: TraceOutcome,
    /// Call-stack depth at which this rule was entered (root rule = 0).
    pub depth: usize,
}

// ─── Public entry point ────────────────────────────────────────────────────

/// Validate a CBOR-encoded byte slice against a CDDL schema.
///
/// `root` selects the rule to validate against; `None` uses the first rule
/// in the schema.  Returns `Ok(Vec::new())` when the value conforms to the
/// schema, `Ok(mismatches)` when it does not, and `Err(message)` when `cbor`
/// is not valid CBOR.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, check_cbor, Config};
///
/// let schema_src = "x = uint";
/// let pairs = lex(schema_src).unwrap();
/// let schema = build_cddl(pairs).unwrap();
///
/// // CBOR encoding of the integer 42 (major type 0, value 42 = 0x18 0x2a)
/// let cbor_42: &[u8] = &[0x18, 0x2a];
/// let mismatches = check_cbor(cbor_42, &schema, None, &Config::strict()).unwrap();
/// assert!(mismatches.is_empty());
/// ```
pub fn check_cbor(
    cbor: &[u8],
    cddl: &Cddl,
    root: Option<&str>,
    config: &Config,
) -> Result<Vec<CborMismatch>, String> {
    run(cbor, cddl, root, config, false).map(|(mismatches, _)| mismatches)
}

/// Like [`check_cbor`], but also returns a step-by-step trace of every rule
/// match attempted while validating, in the order they were attempted.
///
/// Useful for debugging why a schema does or doesn't match: each
/// [`TraceEvent`] records which rule was tried, at what path, and whether it
/// matched or failed.
pub fn check_cbor_traced(
    cbor: &[u8],
    cddl: &Cddl,
    root: Option<&str>,
    config: &Config,
) -> Result<(Vec<CborMismatch>, Vec<TraceEvent>), String> {
    run(cbor, cddl, root, config, true)
}

fn run(
    cbor: &[u8],
    cddl: &Cddl,
    root: Option<&str>,
    _config: &Config,
    trace: bool,
) -> Result<(Vec<CborMismatch>, Vec<TraceEvent>), String> {
    let root_name = match resolve_root(cddl, root) {
        Some(n) => n,
        None => {
            return Ok((
                vec![CborMismatch {
                    path: String::new(),
                    expected: "a non-empty CDDL schema".into(),
                    got: "empty schema".into(),
                }],
                Vec::new(),
            ));
        }
    };

    let item = Decoder::new(cbor)
        .decode()
        .map_err(|e| format!("CBOR decode error: {e:?}"))?;

    let mut ctx = MatchCtx::new(cddl, trace);
    let root_span = CborSpan {
        start: 0,
        end: cbor.len(),
    };
    ctx.match_rule_name(&item, root_name, root_name, root_span);
    Ok((ctx.mismatches, ctx.trace.unwrap_or_default()))
}

// ─── Internal context ──────────────────────────────────────────────────────

/// Cap on named-rule recursion depth (see [`MatchCtx::match_rule_name`] and
/// [`MatchCtx::match_generic_instantiation`]).
///
/// `in_progress` only catches cycles that revisit the *same* rule name; a
/// schema with a long chain of distinct alias rules (`a0 = a1`, `a1 = a2`,
/// ...) recurses once per rule with no repeated name and no corresponding
/// CBOR nesting to bound it, so an independent depth cap is needed here
/// regardless of `tps_minicbor`'s own CBOR-nesting depth limit. The same
/// applies to a chain of distinct generic instantiations (`w0<a> = a`,
/// `w1<a> = w0<a>`, ...) — `match_generic_instantiation` recurses via
/// `match_type`/`match_array_group`/`match_map_group` without ever routing
/// back through `match_rule_name`'s own check, so it tracks `depth` itself
/// too rather than relying on the caller to have already bumped it.
const MAX_MATCH_DEPTH: usize = 64;

struct MatchCtx<'c> {
    cddl: &'c Cddl,
    mismatches: Vec<CborMismatch>,
    /// Rule names currently on the call stack — used for cycle detection.
    in_progress: HashSet<String>,
    /// `Some` only when tracing is enabled; kept as `None` otherwise so the
    /// untraced path has zero extra overhead.
    trace: Option<Vec<TraceEvent>>,
    /// Current call-stack depth of named-rule recursion, tracked
    /// unconditionally (not just while tracing) so [`MAX_MATCH_DEPTH`] can
    /// be enforced; trace events additionally read it when tracing.
    depth: usize,
}

impl<'c> MatchCtx<'c> {
    fn new(cddl: &'c Cddl, trace: bool) -> Self {
        Self {
            cddl,
            mismatches: Vec::new(),
            in_progress: HashSet::new(),
            trace: if trace { Some(Vec::new()) } else { None },
            depth: 0,
        }
    }

    fn mismatch(&mut self, path: &str, expected: impl Into<String>, got: impl Into<String>) {
        self.mismatches.push(CborMismatch {
            path: path.to_string(),
            expected: expected.into(),
            got: got.into(),
        });
    }

    // ── Rule lookup ─────────────────────────────────────────────────────

    fn find_rules(&self, name: &str) -> Vec<&'c Rule> {
        self.cddl.rules.iter().filter(|r| r.name == name).collect()
    }

    /// Resolve a range/size-control bound to a numeric value, following
    /// named rule references (RFC 8610 §2.2.2.1's `first-non-byte = 256`
    /// pattern) as well as literals. Bounded by [`MAX_MATCH_DEPTH`] to guard
    /// against reference cycles.
    fn resolve_type2_as_f64(&self, t2: &Type2, depth: usize) -> Option<f64> {
        if depth >= MAX_MATCH_DEPTH {
            return None;
        }
        match t2 {
            Type2::Value(v) => value_as_f64(v),
            Type2::Typename {
                name, args: None, ..
            } => {
                let rule = self.find_rules(name).into_iter().next()?;
                if rule.generic_parm.is_some() {
                    return None;
                }
                match &rule.body {
                    RuleBody::Type(ty) if ty.choices.len() == 1 && ty.choices[0].constraint.is_none() => {
                        self.resolve_type2_as_f64(&ty.choices[0].base, depth + 1)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ── Top-level dispatchers ───────────────────────────────────────────

    fn match_rule_name(&mut self, item: &Item<'_>, name: &str, path: &str, span: CborSpan) {
        if self.depth >= MAX_MATCH_DEPTH {
            self.mismatch(
                path,
                format!("type `{name}`"),
                "schema nesting too deep".to_string(),
            );
            return;
        }

        if self.trace.is_none() {
            self.depth += 1;
            self.match_rule_name_inner(item, name, path, span);
            self.depth -= 1;
            return;
        }

        let rule_span = self.find_rules(name).first().map(|r| r.span.clone());
        let depth = self.depth;
        self.trace.as_mut().unwrap().push(TraceEvent {
            rule_name: name.to_string(),
            path: path.to_string(),
            span: rule_span.clone(),
            cbor_span: Some(span),
            outcome: TraceOutcome::Attempt,
            depth,
        });

        let before = self.mismatches.len();
        self.depth += 1;
        self.match_rule_name_inner(item, name, path, span);
        self.depth -= 1;

        let outcome = if self.mismatches.len() > before {
            TraceOutcome::Fail
        } else {
            TraceOutcome::Match
        };
        self.trace.as_mut().unwrap().push(TraceEvent {
            rule_name: name.to_string(),
            path: path.to_string(),
            span: rule_span,
            cbor_span: Some(span),
            outcome,
            depth,
        });
    }

    fn match_rule_name_inner(&mut self, item: &Item<'_>, name: &str, path: &str, span: CborSpan) {
        if let Some(prelude_match) = self.match_prelude(item, name) {
            if !prelude_match {
                self.mismatch(path, format!("type `{name}`"), item_description(item));
            }
            return;
        }

        if self.in_progress.contains(name) {
            // Cycle — treat as matching `any`
            return;
        }

        let rules = self.find_rules(name);
        if rules.is_empty() {
            self.mismatch(path, format!("type `{name}`"), "undefined rule".to_string());
            return;
        }

        if rules.iter().any(|r| r.generic_parm.is_some()) {
            // Generic rules not supported in Phase 3 — match any
            return;
        }

        self.in_progress.insert(name.to_string());

        if rules.len() == 1 {
            let body = rules[0].body.clone();
            self.match_rule_body(item, body, name, path, span);
        } else {
            // Multiple definitions via /= or //= — synthesize a union.
            let mut type_choices: Vec<crate::ast::Type1> = Vec::new();
            let mut group_choices: Vec<GrpChoice> = Vec::new();
            let mut has_group = false;

            for rule in &rules {
                match &rule.body {
                    RuleBody::Type(t) => {
                        for choice in &t.choices {
                            type_choices.push(choice.clone());
                        }
                    }
                    RuleBody::Group(grp_ent) => {
                        has_group = true;
                        group_choices.push(GrpChoice {
                            entries: vec![grp_ent.clone()],
                            span: grp_ent.span.clone(),
                        });
                    }
                }
            }

            if has_group {
                let ast_span = rules[0].span.clone();
                let fake_group = Group {
                    choices: group_choices,
                    span: ast_span,
                };
                match item {
                    Item::Array(_) => self.match_array_group(item, &fake_group, path, span),
                    Item::Map(_) => self.match_map_group(item, &fake_group, path, span),
                    _ => self.mismatch(path, format!("group `{name}`"), item_description(item)),
                }
            } else {
                let ast_span = rules[0].span.clone();
                let union_type = crate::ast::Type {
                    choices: type_choices,
                    span: ast_span,
                };
                self.match_type(item, &union_type, path, span);
            }
        }

        self.in_progress.remove(name);
    }

    /// Match a generic instantiation (RFC 8610 §3.10), e.g. `pair<uint,tstr>`
    /// against a rule `pair<a,b> = [a, b]`: bind the rule's generic
    /// parameters to the supplied arguments and substitute them throughout
    /// the rule body before matching, rather than accepting `any` as the
    /// previous behaviour did.
    ///
    /// Falls back to accepting `any` (returning without recording a
    /// mismatch) when the name doesn't resolve to exactly one generic rule,
    /// or when the argument count doesn't match the parameter count — the
    /// latter is Finding 8's arity check, handled separately in the
    /// validator so this matcher doesn't need to double-report it.
    fn match_generic_instantiation(
        &mut self,
        item: &Item<'_>,
        name: &str,
        args: &GenericArg,
        path: &str,
        span: CborSpan,
    ) {
        if self.depth >= MAX_MATCH_DEPTH {
            self.mismatch(
                path,
                format!("type `{name}`"),
                "schema nesting too deep".to_string(),
            );
            return;
        }

        let rules = self.find_rules(name);
        let [rule] = rules.as_slice() else {
            return;
        };
        let Some(parm) = &rule.generic_parm else {
            return;
        };
        let Some(bindings) = generic_bindings(parm, args) else {
            return;
        };

        self.depth += 1;
        match &rule.body {
            RuleBody::Type(t) => {
                let substituted = substitute_generic_type(t, &bindings);
                self.match_type(item, &substituted, path, span);
            }
            RuleBody::Group(grp_ent) => {
                let substituted = substitute_generic_grpent(grp_ent, &bindings);
                let ast_span = substituted.span.clone();
                let fake_group = Group {
                    choices: vec![GrpChoice {
                        entries: vec![substituted],
                        span: ast_span.clone(),
                    }],
                    span: ast_span,
                };
                match item {
                    Item::Array(_) => self.match_array_group(item, &fake_group, path, span),
                    Item::Map(_) => self.match_map_group(item, &fake_group, path, span),
                    _ => self.mismatch(path, format!("group `{name}`"), item_description(item)),
                }
            }
        }
        self.depth -= 1;
    }

    fn match_rule_body(
        &mut self,
        item: &Item<'_>,
        body: RuleBody,
        name: &str,
        path: &str,
        span: CborSpan,
    ) {
        match body {
            RuleBody::Type(t) => self.match_type(item, &t, path, span),
            RuleBody::Group(grp_ent) => {
                let ast_span = grp_ent.span.clone();
                let fake_group = Group {
                    choices: vec![GrpChoice {
                        entries: vec![grp_ent],
                        span: ast_span.clone(),
                    }],
                    span: ast_span,
                };
                match item {
                    Item::Array(_) => self.match_array_group(item, &fake_group, path, span),
                    Item::Map(_) => self.match_map_group(item, &fake_group, path, span),
                    _ => self.mismatch(path, format!("group `{name}`"), item_description(item)),
                }
            }
        }
    }

    /// Returns `Some(true)` if the prelude name matches, `Some(false)` if it
    /// does not, or `None` if `name` is not a prelude name.
    fn match_prelude(&self, item: &Item<'_>, name: &str) -> Option<bool> {
        let ok = match name {
            "any" => true,
            "uint" => matches!(item, Item::Unsigned(_)),
            "nint" => matches!(item, Item::Negative(_)),
            "int" => matches!(item, Item::Unsigned(_) | Item::Negative(_)),
            "bstr" | "bytes" => matches!(item, Item::Bytes(_)),
            "tstr" | "text" => matches!(item, Item::Text(_)),
            "float16" => matches!(item, Item::Float(Float::F16(_))),
            "float32" => matches!(item, Item::Float(Float::F32(_))),
            "float64" => matches!(item, Item::Float(Float::F64(_))),
            "float" => matches!(item, Item::Float(_)),
            "number" => {
                matches!(item, Item::Unsigned(_) | Item::Negative(_) | Item::Float(_))
            }
            "bool" => matches!(item, Item::Bool(_)),
            "false" => matches!(item, Item::Bool(false)),
            "true" => matches!(item, Item::Bool(true)),
            "nil" | "null" => matches!(item, Item::Null),
            "undefined" => matches!(item, Item::Undefined),
            // Tag-based prelude names — match tagged items or underlying type
            "tdate" | "time" => match item {
                Item::Tag(t) => t.tag == 0 || t.tag == 1,
                _ => false,
            },
            "uri" | "b64url" | "b64legacy" | "regexp" | "mime-message" => {
                matches!(item, Item::Text(_))
            }
            "cbor-any" | "encoded-cbor" => matches!(item, Item::Bytes(_)),
            "biguint" => match item {
                Item::Tag(t) => t.tag == 2,
                _ => false,
            },
            "bignint" => match item {
                Item::Tag(t) => t.tag == 3,
                _ => false,
            },
            "bigint" => match item {
                Item::Tag(t) => t.tag == 2 || t.tag == 3,
                _ => false,
            },
            "integer" => {
                matches!(item, Item::Unsigned(_) | Item::Negative(_))
                    || matches!(item, Item::Tag(t) if t.tag == 2 || t.tag == 3)
            }
            "unsigned" => {
                matches!(item, Item::Unsigned(_)) || matches!(item, Item::Tag(t) if t.tag == 2)
            }
            _ => return None, // Not a prelude name
        };
        Some(ok)
    }

    fn match_type(&mut self, item: &Item<'_>, ty: &Type, path: &str, span: CborSpan) {
        if ty.choices.len() == 1 {
            self.match_type1(item, &ty.choices[0], path, span);
            return;
        }

        // Union: succeed at first matching choice.
        let save_len = self.mismatches.len();
        for choice in &ty.choices {
            let before = self.mismatches.len();
            self.match_type1(item, choice, path, span);
            if self.mismatches.len() == before {
                // This choice matched — strip any mismatches from earlier attempts.
                // (There can't be any new ones since `before == save_len` on the
                // first iteration and we return on first match.)
                return;
            }
            // Roll back mismatches from this failed attempt.
            self.mismatches.truncate(before);
        }
        // All choices failed.
        let choices: Vec<String> = ty.choices.iter().map(type1_description).collect();
        self.mismatch(path, choices.join(" / "), item_description(item));
        // Restore to save_len (we already truncated to before on each failure,
        // so len == save_len now, but be explicit).
        self.mismatches.truncate(save_len + 1); // keep the one we just added
    }

    fn match_type1(&mut self, item: &Item<'_>, t1: &Type1, path: &str, span: CborSpan) {
        if let Some(constraint) = &t1.constraint {
            // Range constraint: base is lower bound, constraint.target is upper bound.
            if matches!(
                constraint.op,
                ConstraintOp::RangeInclusive | ConstraintOp::RangeExclusive
            ) {
                self.check_full_range(item, &t1.base, constraint, path);
                return;
            }
        }

        // Non-range: base must match exactly, then check constraint.
        let before = self.mismatches.len();
        self.match_type2(item, &t1.base, path, span);
        if self.mismatches.len() > before {
            return;
        }
        if let Some(constraint) = &t1.constraint {
            self.check_constraint(item, constraint, path, span);
        }
    }

    fn match_type2(&mut self, item: &Item<'_>, t2: &Type2, path: &str, span: CborSpan) {
        match t2 {
            Type2::Any(_) => {}

            Type2::Value(v) => self.match_value_literal(item, v, path),

            Type2::Typename { name, args, .. } => match args {
                Some(ga) => self.match_generic_instantiation(item, name, ga, path, span),
                None => self.match_rule_name(item, name, path, span),
            },

            Type2::ParenType(ty) => self.match_type(item, ty, path, span),

            Type2::Map(g) => match item {
                Item::Map(_) => self.match_map_group(item, g, path, span),
                _ => self.mismatch(path, "map", item_description(item)),
            },

            Type2::Array(g) => match item {
                Item::Array(_) => self.match_array_group(item, g, path, span),
                _ => self.mismatch(path, "array", item_description(item)),
            },

            Type2::Tagged { tag, inner, .. } => {
                self.match_tagged(item, tag.as_ref(), inner, path, span)
            }

            Type2::MajorType { major, ai, .. } => self.match_major_type(item, *major, ai, path),

            Type2::EnumGroup(g) => self.match_enum_group(item, g, path, span),

            Type2::EnumRef { .. } => {
                // Enum refs to named groups are treated as any in Phase 3
            }

            Type2::Unwrap { name, args, .. } => {
                if args.is_some() {
                    return;
                }
                // Unwrap: strip one level of tag and match inner
                match item {
                    Item::Tag(t) => match t.item_with_span() {
                        Ok((inner_item, inner_span)) => {
                            let data_start = container_data_start(span.end, inner_span.end);
                            let abs_span = compose_span(data_start, inner_span);
                            self.match_rule_name(&inner_item, name, path, abs_span);
                        }
                        Err(e) => self.mismatch(
                            path,
                            format!("unwrap `~{name}`"),
                            format!("CBOR decode error: {e:?}"),
                        ),
                    },
                    _ => self.mismatch(
                        path,
                        format!("tagged item for `~{name}`"),
                        item_description(item),
                    ),
                }
            }
        }
    }

    // ── Literal value matching ──────────────────────────────────────────

    fn match_value_literal(&mut self, item: &Item<'_>, v: &Value, path: &str) {
        let matches = match v {
            Value::Int(n) => {
                if *n >= 0 {
                    matches!(item, Item::Unsigned(u) if *u == *n as u64)
                } else {
                    // CBOR negative: -1 - n  =>  Item::Negative(n) where n = -1 - cbor_n
                    // tps_minicbor stores as i64 directly
                    matches!(item, Item::Negative(i) if *i == *n)
                }
            }
            Value::Float(f) => match item {
                Item::Float(fl) => float_to_f64(fl) == *f,
                _ => false,
            },
            Value::Text(s) => matches!(item, Item::Text(t) if *t == s.as_str()),
            Value::Bytes(b) => matches!(item, Item::Bytes(bs) if *bs == b.bytes.as_slice()),
        };
        if !matches {
            self.mismatch(path, value_description(v), item_description(item));
        }
    }

    // ── Tag matching ────────────────────────────────────────────────────

    fn match_tagged(
        &mut self,
        item: &Item<'_>,
        tag: Option<&HeadNumber>,
        inner: &Type,
        path: &str,
        span: CborSpan,
    ) {
        match item {
            Item::Tag(t) => {
                // Check tag number if specified.
                if let Some(head) = tag {
                    match head {
                        HeadNumber::Literal(n) => {
                            if t.tag != *n {
                                self.mismatch(path, format!("tag #{n}"), format!("tag #{}", t.tag));
                                return;
                            }
                        }
                        HeadNumber::Type(ty) => {
                            let tag_item = Item::Unsigned(t.tag);
                            let before = self.mismatches.len();
                            self.match_type(&tag_item, ty, path, span);
                            if self.mismatches.len() > before {
                                self.mismatches.truncate(before);
                                let choices: Vec<String> =
                                    ty.choices.iter().map(type1_description).collect();
                                self.mismatch(
                                    path,
                                    format!("tag matching {}", choices.join(" / ")),
                                    format!("tag #{}", t.tag),
                                );
                                return;
                            }
                        }
                    }
                }
                // Decode and check inner item.
                match t.item_with_span() {
                    Ok((inner_item, inner_span)) => {
                        let inner_path = format!("{path}(tag {})", t.tag);
                        let data_start = container_data_start(span.end, inner_span.end);
                        let abs_span = compose_span(data_start, inner_span);
                        self.match_type(&inner_item, inner, &inner_path, abs_span);
                    }
                    Err(e) => {
                        self.mismatch(path, "tagged item", format!("CBOR decode error: {e:?}"))
                    }
                }
            }
            _ => self.mismatch(path, "tagged item", item_description(item)),
        }
    }

    // ── Major type matching ─────────────────────────────────────────────

    fn match_major_type(
        &mut self,
        item: &Item<'_>,
        major: u8,
        ai: &Option<HeadNumber>,
        path: &str,
    ) {
        let item_major = item_major_type(item);
        if item_major != major {
            self.mismatch(
                path,
                format!("major type {major}"),
                format!("major type {item_major}"),
            );
            return;
        }
        // If an additional-info value is specified, check it.
        if let Some(HeadNumber::Literal(n)) = ai {
            let item_ai = item_ai_value(item);
            if let Some(ai_val) = item_ai
                && ai_val != *n
            {
                self.mismatch(path, format!("#{major}.{n}"), format!("#{major}.{ai_val}"));
            }
        }
    }

    // ── Enum group matching ─────────────────────────────────────────────

    fn match_enum_group(&mut self, item: &Item<'_>, group: &Group, path: &str, span: CborSpan) {
        // An enum group enumerates legal values: &(label: value, ...).
        // The CBOR item must equal one of the values in the group.
        for choice in &group.choices {
            for entry in &choice.entries {
                if let GrpEntBody::Type(t) = &entry.entry {
                    let before = self.mismatches.len();
                    self.match_type(item, t, path, span);
                    if self.mismatches.len() == before {
                        return; // Matched one member
                    }
                    self.mismatches.truncate(before);
                }
            }
        }
        self.mismatch(path, "enum member", item_description(item));
    }

    // ── Constraint checking ─────────────────────────────────────────────

    fn check_constraint(
        &mut self,
        item: &Item<'_>,
        constraint: &Constraint,
        path: &str,
        span: CborSpan,
    ) {
        let target_val = match &constraint.target {
            Type2::Value(v) => Some(v.clone()),
            _ => None,
        };

        match &constraint.op {
            ConstraintOp::RangeInclusive | ConstraintOp::RangeExclusive => {
                let inclusive = constraint.op == ConstraintOp::RangeInclusive;
                if let Some(target) = target_val {
                    self.check_range(item, &target, inclusive, path);
                }
            }
            ConstraintOp::Control(op) => match op.as_str() {
                "size" => match &constraint.target {
                    Type2::Value(Value::Int(n)) => {
                        self.check_size(item, *n as u64, path);
                    }
                    Type2::ParenType(inner_type) => {
                        if let Some(t1) = inner_type.choices.first()
                            && let Some(range_constraint) = &t1.constraint
                            && matches!(
                                range_constraint.op,
                                ConstraintOp::RangeInclusive | ConstraintOp::RangeExclusive
                            )
                        {
                            let inclusive = range_constraint.op == ConstraintOp::RangeInclusive;
                            let lo = self.resolve_type2_as_f64(&t1.base, 0);
                            let hi = self.resolve_type2_as_f64(&range_constraint.target, 0);
                            let len = item_len(item);
                            if let (Some(l), Some(lo)) = (len, lo) {
                                let l = l as f64;
                                if l < lo {
                                    self.mismatch(
                                        path,
                                        format!(".size >= {lo}"),
                                        format!(".size {l}"),
                                    );
                                }
                            }
                            if let (Some(l), Some(hi)) = (len, hi) {
                                let l = l as f64;
                                let ok = if inclusive { l <= hi } else { l < hi };
                                if !ok {
                                    self.mismatch(
                                        path,
                                        format!(".size <= {hi}"),
                                        format!(".size {l}"),
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                },
                "eq" => {
                    if let Some(v) = target_val {
                        self.match_value_literal(item, &v, path);
                    }
                }
                "ne" => {
                    if let Some(v) = target_val {
                        let before = self.mismatches.len();
                        self.match_value_literal(item, &v, path);
                        if self.mismatches.len() == before {
                            // Matched — but .ne means it should NOT match
                            self.mismatch(
                                path,
                                format!(".ne {}", value_description(&v)),
                                item_description(item),
                            );
                        } else {
                            // Did not match — good, .ne is satisfied
                            self.mismatches.truncate(before);
                        }
                    }
                }
                "default" => {
                    // RFC 8610 §3.8.6: `.default` implies an `.ne` against
                    // the default value — a conformant sender omits the key
                    // rather than sending it explicitly equal to the
                    // default, so an exact match here is a mismatch.
                    if let Some(v) = target_val {
                        let before = self.mismatches.len();
                        self.match_value_literal(item, &v, path);
                        if self.mismatches.len() == before {
                            self.mismatch(
                                path,
                                format!(".default {} (value must be omitted, not sent)", value_description(&v)),
                                item_description(item),
                            );
                        } else {
                            self.mismatches.truncate(before);
                        }
                    }
                }
                "lt" | "le" | "gt" | "ge" => {
                    if let Some(target) = target_val {
                        self.check_comparison(item, op, &target, path);
                    }
                }
                "cbor" => self.check_cbor_control(item, constraint, path, span),
                "cborseq" => self.check_cborseq_control(item, constraint, path, span),
                "and" | "within" => {
                    self.match_type2(item, &constraint.target, path, span);
                }
                "bits" => self.check_bits_control(item, &constraint.target, path, span),
                _ => {
                    // Unsupported control operator — skip
                }
            },
        }
    }

    /// `.cbor`: the byte string carries a single CBOR-encoded data item that
    /// must match the controller type (RFC 8610 §3.8.4, "bytes .cbor type1").
    fn check_cbor_control(
        &mut self,
        item: &Item<'_>,
        constraint: &Constraint,
        path: &str,
        span: CborSpan,
    ) {
        let bytes = match item {
            Item::Bytes(b) => *b,
            _ => {
                self.mismatch(path, ".cbor (byte string)", item_description(item));
                return;
            }
        };
        match Decoder::new(bytes).decode() {
            Ok(inner) => {
                let inner_path = format!("{path}/.cbor");
                self.match_type2(&inner, &constraint.target, &inner_path, span);
            }
            Err(e) => {
                self.mismatch(
                    path,
                    ".cbor (valid CBOR data item)",
                    format!("CBOR decode error: {e:?}"),
                );
            }
        }
    }

    /// `.cborseq`: the byte string carries a sequence of CBOR-encoded data
    /// items which, taken together as an array, must match the controller
    /// type (RFC 8610 §3.8.4, "bytes .cborseq type2"). The RFC suggests
    /// implementing this by wrapping the byte string in an indefinite-length
    /// array (`0x9f`..`0xff`) and decoding that as an array; `tps_minicbor`
    /// does not support indefinite-length items at all (by design — see its
    /// CLAUDE.md), so instead we count the top-level items in a first pass
    /// and prepend a definite-length array header, which decodes to the same
    /// logical array (CDDL does not distinguish definite- from
    /// indefinite-length encoding when matching against a schema).
    fn check_cborseq_control(
        &mut self,
        item: &Item<'_>,
        constraint: &Constraint,
        path: &str,
        span: CborSpan,
    ) {
        let bytes = match item {
            Item::Bytes(b) => *b,
            _ => {
                self.mismatch(path, ".cborseq (byte string)", item_description(item));
                return;
            }
        };

        let mut counter = Decoder::new(bytes);
        let mut count: u64 = 0;
        while counter.pos() < bytes.len() {
            if let Err(e) = counter.next() {
                self.mismatch(
                    path,
                    ".cborseq (sequence of valid CBOR data items)",
                    format!("CBOR decode error at element {count}: {e:?}"),
                );
                return;
            }
            count += 1;
        }

        let mut synthetic = encode_array_header(count);
        synthetic.extend_from_slice(bytes);
        match Decoder::new(&synthetic).decode() {
            Ok(array_item) => {
                let inner_path = format!("{path}/.cborseq");
                self.match_type2(&array_item, &constraint.target, &inner_path, span);
            }
            Err(e) => {
                self.mismatch(
                    path,
                    ".cborseq (sequence reassembled as an array)",
                    format!("CBOR decode error: {e:?}"),
                );
            }
        }
    }

    fn check_range(&mut self, item: &Item<'_>, upper: &Value, inclusive: bool, path: &str) {
        let item_num = item_as_f64(item);
        let upper_num = value_as_f64(upper);
        if let (Some(v), Some(hi)) = (item_num, upper_num) {
            let ok = if inclusive { v <= hi } else { v < hi };
            if !ok {
                let op = if inclusive { ".." } else { "..." };
                self.mismatch(
                    path,
                    format!("{op} {}", value_description(upper)),
                    format!("{v}"),
                );
            }
        }
    }

    /// `.bits`: on a byte string, only bit positions named by the controller
    /// type may be set; on an unsigned integer, every set bit's position
    /// must be named by the controller type (RFC 8610 §3.8.2). Bit `n` is
    /// set in a byte string `str` when `(str[n >> 3] & (1 << (n & 7))) != 0`.
    fn check_bits_control(
        &mut self,
        item: &Item<'_>,
        controller: &Type2,
        path: &str,
        span: CborSpan,
    ) {
        let bit_is_set = |n: u64| -> bool {
            match item {
                Item::Bytes(bytes) => bytes
                    .get((n >> 3) as usize)
                    .is_some_and(|b| b & (1 << (n & 7)) != 0),
                Item::Unsigned(i) => i & (1u64 << n) != 0,
                _ => false,
            }
        };
        let bit_count = match item {
            Item::Bytes(bytes) => bytes.len() as u64 * 8,
            Item::Unsigned(_) => 64,
            _ => {
                self.mismatch(path, ".bits (byte string or uint)", item_description(item));
                return;
            }
        };
        for n in 0..bit_count {
            if bit_is_set(n) && !self.bit_position_allowed(n, controller, span) {
                self.mismatch(
                    path,
                    format!(".bits {}", type2_description(controller)),
                    format!("bit {n} set"),
                );
                return;
            }
        }
    }

    /// Whether bit position `n` satisfies the `.bits` controller type.
    fn bit_position_allowed(&mut self, n: u64, controller: &Type2, span: CborSpan) -> bool {
        let candidate = Item::Unsigned(n);
        let before = self.mismatches.len();
        self.match_type2(&candidate, controller, "", span);
        let allowed = self.mismatches.len() == before;
        self.mismatches.truncate(before);
        allowed
    }

    /// Check a full range constraint: lower_type2 op upper_type2.
    fn check_full_range(
        &mut self,
        item: &Item<'_>,
        lower_t2: &Type2,
        constraint: &Constraint,
        path: &str,
    ) {
        let inclusive = constraint.op == ConstraintOp::RangeInclusive;
        let item_num = item_as_f64(item);

        // Lower bound: item must be a number type.
        if item_num.is_none() {
            self.mismatch(path, "numeric type for range", item_description(item));
            return;
        }
        let v = item_num.unwrap();

        // Check lower bound.
        let lower = self.resolve_type2_as_f64(lower_t2, 0);
        if let Some(lo) = lower
            && v < lo
        {
            let op_str = if inclusive { ".." } else { "..." };
            self.mismatch(path, format!("{lo}{op_str}"), format!("{v}"));
            return;
        }

        // Check upper bound.
        let upper = self.resolve_type2_as_f64(&constraint.target, 0);
        if let Some(hi) = upper {
            let ok = if inclusive { v <= hi } else { v < hi };
            if !ok {
                let op_str = if inclusive { ".." } else { "..." };
                let lo_str = lower.map_or(String::new(), |lo| lo.to_string());
                self.mismatch(path, format!("{lo_str}{op_str}{hi}"), format!("{v}"));
            }
        }
    }

    fn check_size(&mut self, item: &Item<'_>, expected: u64, path: &str) {
        let len = match item {
            Item::Text(s) => Some(s.len() as u64),
            Item::Bytes(b) => Some(b.len() as u64),
            Item::Array(a) => Some(a.len()),
            Item::Map(m) => Some(m.len()),
            _ => None,
        };
        if let Some(l) = len
            && l != expected
        {
            self.mismatch(path, format!(".size {expected}"), format!(".size {l}"));
        }
    }

    fn check_comparison(&mut self, item: &Item<'_>, op: &str, target: &Value, path: &str) {
        let item_num = item_as_f64(item);
        let target_num = value_as_f64(target);
        if let (Some(v), Some(t)) = (item_num, target_num) {
            let ok = match op {
                "lt" => v < t,
                "le" => v <= t,
                "gt" => v > t,
                "ge" => v >= t,
                _ => true,
            };
            if !ok {
                self.mismatch(
                    path,
                    format!(".{op} {}", value_description(target)),
                    format!("{v}"),
                );
            }
        }
    }

    // ── Array group matching ────────────────────────────────────────────

    fn match_array_group(&mut self, item: &Item<'_>, group: &Group, path: &str, span: CborSpan) {
        let items: Vec<(Item<'_>, CborSpan)> = match item {
            Item::Array(a) => {
                let relative: Vec<(Item<'_>, Range<usize>)> =
                    match a.iter_with_span().collect::<Result<Vec<_>, DecodeError>>() {
                        Ok(v) => v,
                        Err(e) => {
                            self.mismatch(path, "array", format!("CBOR decode error: {e:?}"));
                            return;
                        }
                    };
                let data_start = relative
                    .last()
                    .map(|(_, r)| container_data_start(span.end, r.end))
                    .unwrap_or(span.end);
                relative
                    .into_iter()
                    .map(|(it, rel)| (it, compose_span(data_start, rel)))
                    .collect()
            }
            _ => {
                self.mismatch(path, "array", item_description(item));
                return;
            }
        };

        // Try each group choice; succeed at first that matches.
        for choice in &group.choices {
            let before = self.mismatches.len();
            if self.try_match_array_choice(&items, choice, path) {
                self.mismatches.truncate(before);
                return;
            }
            self.mismatches.truncate(before);
        }
        self.mismatch(
            path,
            "array matching schema",
            format!("array of {} items", items.len()),
        );
    }

    /// Returns true if the array items fully match the group choice.
    fn try_match_array_choice(
        &mut self,
        items: &[(Item<'_>, CborSpan)],
        choice: &GrpChoice,
        path: &str,
    ) -> bool {
        let mut pos = 0usize;
        let before = self.mismatches.len();

        // Expand group inclusions (entries with no member key, e.g. a bare
        // `Headers` referencing `Headers = ( protected: ..., unprotected: ... )`)
        // before matching, so a named group spliced into an array is matched
        // positionally against its own entries rather than as a single item.
        let flat = flatten_array_entries(self.cddl, &choice.entries, 0);

        for entry in &flat {
            if matches!(entry.entry, GrpEntBody::InlineGroup(_)) {
                continue; // Inline groups in arrays not supported — skip
            }

            let (min, max) = occur_bounds(entry.occur.as_ref());
            let mut count = 0u64;

            while max.is_none_or(|m| count < m) {
                if pos >= items.len() {
                    break;
                }
                let item_path = format!("{path}[{pos}]");
                let save = self.mismatches.len();
                let (item, item_span) = &items[pos];
                self.match_grp_entry_as_type(item, &entry.entry, &item_path, *item_span);
                if self.mismatches.len() > save {
                    self.mismatches.truncate(save);
                    break; // Item didn't match — stop consuming
                }
                pos += 1;
                count += 1;
            }

            if count < min {
                return false; // Didn't meet minimum occurrence
            }
        }

        // All entries consumed — check no items remain.
        if pos != items.len() {
            self.mismatch(
                path,
                format!("array of {pos} items"),
                format!("array of {} items", items.len()),
            );
            let _ = self.mismatches.len();
            return self.mismatches.len() == before;
        }

        self.mismatches.len() == before
    }

    fn match_grp_entry_as_type(
        &mut self,
        item: &Item<'_>,
        body: &GrpEntBody,
        path: &str,
        span: CborSpan,
    ) {
        match body {
            GrpEntBody::Type(t) => self.match_type(item, t, path, span),
            GrpEntBody::GroupRef { name, args, .. } => {
                if args.is_some() {
                    return;
                }
                self.match_rule_name(item, name, path, span);
            }
            GrpEntBody::InlineGroup(_) => {} // Not supported in array context
        }
    }

    // ── Map group matching ──────────────────────────────────────────────

    fn match_map_group(&mut self, item: &Item<'_>, group: &Group, path: &str, span: CborSpan) {
        type SpannedEntry<'a> = ((Item<'a>, Range<usize>), (Item<'a>, Range<usize>));
        let pairs: Vec<(Item<'_>, Item<'_>, CborSpan)> = match item {
            Item::Map(m) => {
                let relative: Vec<SpannedEntry<'_>> =
                    match m.iter_with_span().collect::<Result<Vec<_>, DecodeError>>() {
                        Ok(v) => v,
                        Err(e) => {
                            self.mismatch(path, "map", format!("CBOR decode error: {e:?}"));
                            return;
                        }
                    };
                let data_start = relative
                    .last()
                    .map(|(_, (_, vrel))| container_data_start(span.end, vrel.end))
                    .unwrap_or(span.end);
                relative
                    .into_iter()
                    .map(|((k, _krel), (v, vrel))| (k, v, compose_span(data_start, vrel)))
                    .collect()
            }
            _ => {
                self.mismatch(path, "map", item_description(item));
                return;
            }
        };

        // Try each group choice.
        for choice in &group.choices {
            let before = self.mismatches.len();
            if self.try_match_map_choice(&pairs, choice, path) {
                self.mismatches.truncate(before);
                return;
            }
            self.mismatches.truncate(before);
        }
        self.mismatch(
            path,
            "map matching schema",
            format!("map of {} keys", pairs.len()),
        );
    }

    fn try_match_map_choice(
        &mut self,
        pairs: &[(Item<'_>, Item<'_>, CborSpan)],
        choice: &GrpChoice,
        path: &str,
    ) -> bool {
        let before = self.mismatches.len();
        let mut consumed = vec![false; pairs.len()];
        // A key whose value fails an entry's value type, even though the
        // key itself matched that entry's key pattern, isn't necessarily an
        // error yet — a different (e.g. more permissive) entry appearing
        // later may still claim it. Remember the first such failure per key
        // so it can be reported precisely if no entry ends up consuming the
        // key, instead of silently discarding real mismatch detail.
        let mut pending_mismatch: Vec<Option<Vec<CborMismatch>>> = vec![None; pairs.len()];

        // Expand group inclusions (entries with no member key) before matching.
        let flat = flatten_map_entries(self.cddl, &choice.entries, 0);

        for entry in &flat {
            let (min, max) = occur_bounds(entry.occur.as_ref());
            let mut count = 0u64;

            for (idx, (k, v, vspan)) in pairs.iter().enumerate() {
                if consumed[idx] {
                    continue;
                }
                if max.is_some_and(|m| count >= m) {
                    break;
                }

                let key_matches = match &entry.key {
                    None => false,
                    Some(mk) => self.key_matches(k, mk),
                };
                if !key_matches {
                    continue;
                }

                let val_path = format!("{path}.{}", member_key_description(entry.key.as_ref()));
                let save = self.mismatches.len();
                self.match_grp_entry_as_type(v, &entry.entry, &val_path, *vspan);
                if self.mismatches.len() > save {
                    let failure = self.mismatches.split_off(save);
                    pending_mismatch[idx].get_or_insert(failure);
                    continue;
                }

                consumed[idx] = true;
                count += 1;
            }

            if count < min {
                return false;
            }
        }

        // Check for unclaimed keys. A genuine repeating catch-all entry
        // (`* type => type`, unbounded occurrence) legitimately covers keys
        // no other entry claims; a non-repeating type-keyed entry (e.g.
        // `&(name: N) => type`, whose key is a single-literal enum-group
        // but still parses as `MemberKey::Type`) covers exactly one literal
        // key and must not suppress detection of genuinely unexpected keys.
        let has_wildcard = flat.iter().any(|e| {
            matches!(e.key, Some(MemberKey::Type { .. })) && occur_bounds(e.occur.as_ref()).1.is_none()
        });
        for (idx, consumed) in consumed.iter().enumerate() {
            if *consumed {
                continue;
            }
            if let Some(failure) = pending_mismatch[idx].take() {
                self.mismatches.extend(failure);
            } else if !has_wildcard {
                let key_desc = item_description(&pairs[idx].0);
                self.mismatch(
                    path,
                    "no unexpected keys".to_string(),
                    format!("unexpected key {key_desc}"),
                );
            }
        }

        self.mismatches.len() == before
    }

    fn key_matches(&mut self, item: &Item<'_>, mk: &MemberKey) -> bool {
        match mk {
            MemberKey::Bareword { name, .. } => {
                matches!(item, Item::Text(t) if *t == name.as_str())
            }
            MemberKey::Value { key, .. } => {
                let before = self.mismatches.len();
                let trace_before = self.trace.as_ref().map(|t| t.len());
                self.match_value_literal(item, key, "__key__");
                let matched = self.mismatches.len() == before;
                self.mismatches.truncate(before);
                if let (Some(trace), Some(n)) = (self.trace.as_mut(), trace_before) {
                    trace.truncate(n);
                }
                matched
            }
            MemberKey::Type { key, .. } => {
                let before = self.mismatches.len();
                let trace_before = self.trace.as_ref().map(|t| t.len());
                // The span is irrelevant here — this is a probe whose
                // mismatches (and any trace events pushed by nested rule
                // lookups) are always truncated below.
                self.match_type1(item, key, "__key__", CborSpan { start: 0, end: 0 });
                let matched = self.mismatches.len() == before;
                self.mismatches.truncate(before);
                if let (Some(trace), Some(n)) = (self.trace.as_mut(), trace_before) {
                    trace.truncate(n);
                }
                matched
            }
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Absolute byte offset at which a container's `data` sub-slice (as used by
/// `iter_with_span`/`item_with_span`) begins.
///
/// A container item's `data` sub-slice holds exactly its contents with no
/// header and no trailing bytes, and its end coincides with the end of the
/// container item's own absolute span (`container_end`). So once we know
/// the relative end offset of the *last* thing decoded from that sub-slice
/// (which — because containers are packed tightly, with nothing left over
/// — equals the sub-slice's total length), `container_end - last_relative_end`
/// is exactly where the sub-slice starts in the original buffer.
fn container_data_start(container_end: usize, last_relative_end: usize) -> usize {
    container_end - last_relative_end
}

/// Compose a byte range relative to a container's `data` sub-slice with
/// that sub-slice's absolute start offset (from [`container_data_start`]),
/// yielding an absolute [`CborSpan`].
fn compose_span(data_start: usize, relative: Range<usize>) -> CborSpan {
    CborSpan {
        start: data_start + relative.start,
        end: data_start + relative.end,
    }
}

/// Encode a definite-length CBOR array header (major type 4) for `len`
/// items, used by [`MatchCtx::check_cborseq_control`] to reassemble a
/// `.cborseq`-constrained byte string's contents into a decodable array.
fn encode_array_header(len: u64) -> Vec<u8> {
    if len <= 23 {
        vec![0x80 | len as u8]
    } else if len <= 0xFF {
        vec![0x98, len as u8]
    } else if len <= 0xFFFF {
        let b = (len as u16).to_be_bytes();
        vec![0x99, b[0], b[1]]
    } else if len <= 0xFFFF_FFFF {
        let b = (len as u32).to_be_bytes();
        vec![0x9a, b[0], b[1], b[2], b[3]]
    } else {
        let b = len.to_be_bytes();
        vec![0x9b, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]
    }
}

fn resolve_root<'c>(cddl: &'c Cddl, root: Option<&'c str>) -> Option<&'c str> {
    match root {
        Some(name) => {
            if cddl.rules.iter().any(|r| r.name == name) {
                Some(name)
            } else {
                None
            }
        }
        None => cddl.rules.first().map(|r| r.name.as_str()),
    }
}

fn occur_bounds(occur: Option<&Occur>) -> (u64, Option<u64>) {
    match occur {
        None => (1, Some(1)), // Default: exactly once
        Some(Occur::Optional) => (0, Some(1)),
        Some(Occur::OneOrMore) => (1, None),
        Some(Occur::Bounded { lower, upper }) => (lower.unwrap_or(0), *upper),
    }
}

fn item_description(item: &Item<'_>) -> String {
    match item {
        Item::Unsigned(n) => format!("uint({n})"),
        Item::Negative(n) => format!("nint({n})"),
        Item::Bytes(b) => format!("bytes({} bytes)", b.len()),
        Item::Text(s) => {
            if s.len() > 32 {
                format!("tstr(\"{:.32}…\")", s)
            } else {
                format!("tstr({s:?})")
            }
        }
        Item::Array(a) => format!("array({} items)", a.len()),
        Item::Map(m) => format!("map({} keys)", m.len()),
        Item::Tag(t) => format!("tag({})", t.tag),
        Item::Bool(b) => format!("{b}"),
        Item::Null => "null".to_string(),
        Item::Undefined => "undefined".to_string(),
        Item::Simple(n) => format!("simple({n})"),
        Item::Float(f) => format!("float({})", float_to_f64(f)),
    }
}

fn value_description(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Text(s) => format!("{s:?}"),
        Value::Bytes(b) => {
            let hex: String = b.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("h'{hex}'")
        }
    }
}

fn type1_description(t1: &Type1) -> String {
    type2_description(&t1.base)
}

fn type2_description(t2: &Type2) -> String {
    match t2 {
        Type2::Any(_) => "#".to_string(),
        Type2::Value(v) => value_description(v),
        Type2::Typename { name, .. } => name.clone(),
        Type2::Map(_) => "map".to_string(),
        Type2::Array(_) => "array".to_string(),
        Type2::Tagged { tag, .. } => match tag {
            Some(HeadNumber::Literal(n)) => format!("#6.{n}(…)"),
            _ => "#6(…)".to_string(),
        },
        Type2::MajorType { major, ai, .. } => match ai {
            Some(HeadNumber::Literal(n)) => format!("#{major}.{n}"),
            _ => format!("#{major}"),
        },
        Type2::ParenType(_) => "(…)".to_string(),
        Type2::EnumGroup(_) | Type2::EnumRef { .. } => "&(…)".to_string(),
        Type2::Unwrap { name, .. } => format!("~{name}"),
    }
}

fn member_key_description(mk: Option<&MemberKey>) -> String {
    match mk {
        None => String::new(),
        Some(MemberKey::Bareword { name, .. }) => name.clone(),
        Some(MemberKey::Value { key, .. }) => value_description(key),
        Some(MemberKey::Type { key, .. }) => type1_description(key),
    }
}

fn float_to_f64(f: &Float) -> f64 {
    match f {
        Float::F16(bits) => f16_to_f64(*bits),
        Float::F32(bits) => f32::from_bits(*bits) as f64,
        Float::F64(bits) => f64::from_bits(*bits),
    }
}

/// IEEE 754 half-precision (f16) to f64 conversion.
fn f16_to_f64(bits: u16) -> f64 {
    let sign: u64 = ((bits >> 15) as u64) << 63;
    let exp = (bits >> 10) & 0x1F;
    let mant = (bits & 0x3FF) as u64;

    let result_bits: u64 = if exp == 0 {
        if mant == 0 {
            sign // ±0
        } else {
            // Subnormal: shift up to normal
            let leading = mant.leading_zeros() - (64 - 10);
            let e = (1023 - 14 - leading) as u64;
            let m = (mant << (leading + 1)) & 0x3FF;
            sign | (e << 52) | (m << 42)
        }
    } else if exp == 0x1F {
        if mant == 0 {
            sign | 0x7FF0_0000_0000_0000 // ±Inf
        } else {
            sign | 0x7FF8_0000_0000_0000 // NaN
        }
    } else {
        sign | ((exp as u64 + 1023 - 15) << 52) | (mant << 42)
    };
    f64::from_bits(result_bits)
}

fn item_as_f64(item: &Item<'_>) -> Option<f64> {
    match item {
        Item::Unsigned(n) => Some(*n as f64),
        Item::Negative(n) => Some(*n as f64),
        Item::Float(f) => Some(float_to_f64(f)),
        _ => None,
    }
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn item_len(item: &Item<'_>) -> Option<u64> {
    match item {
        Item::Text(s) => Some(s.len() as u64),
        Item::Bytes(b) => Some(b.len() as u64),
        Item::Array(a) => Some(a.len()),
        Item::Map(m) => Some(m.len()),
        _ => None,
    }
}

fn item_major_type(item: &Item<'_>) -> u8 {
    match item {
        Item::Unsigned(_) => 0,
        Item::Negative(_) => 1,
        Item::Bytes(_) => 2,
        Item::Text(_) => 3,
        Item::Array(_) => 4,
        Item::Map(_) => 5,
        Item::Tag(_) => 6,
        Item::Bool(_) | Item::Null | Item::Undefined | Item::Simple(_) | Item::Float(_) => 7,
    }
}

fn item_ai_value(item: &Item<'_>) -> Option<u64> {
    match item {
        Item::Bool(false) => Some(20),
        Item::Bool(true) => Some(21),
        Item::Null => Some(22),
        Item::Undefined => Some(23),
        Item::Simple(n) => Some(*n as u64),
        Item::Float(Float::F16(_)) => Some(25),
        Item::Float(Float::F32(_)) => Some(26),
        Item::Float(Float::F64(_)) => Some(27),
        _ => None,
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::lex, parser::build_cddl};

    fn schema(src: &str) -> Cddl {
        let pairs = lex(src).unwrap_or_else(|e| panic!("parse: {e}"));
        build_cddl(pairs).unwrap_or_else(|e| panic!("build: {e}"))
    }

    fn check(cbor: &[u8], cddl_src: &str) -> Vec<CborMismatch> {
        check_cbor(cbor, &schema(cddl_src), None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"))
    }

    fn ok(cbor: &[u8], cddl_src: &str) {
        let m = check(cbor, cddl_src);
        assert!(m.is_empty(), "expected ok, got mismatches: {m:#?}");
    }

    fn fail(cbor: &[u8], cddl_src: &str) {
        let m = check(cbor, cddl_src);
        assert!(!m.is_empty(), "expected mismatch but got none");
    }

    // Encode a single uint.
    fn uint(n: u64) -> Vec<u8> {
        if n <= 23 {
            vec![n as u8]
        } else if n <= 0xFF {
            vec![0x18, n as u8]
        } else if n <= 0xFFFF {
            let b = n.to_be_bytes();
            vec![0x19, b[6], b[7]]
        } else {
            let b = n.to_be_bytes();
            vec![0x1a, b[4], b[5], b[6], b[7]]
        }
    }

    // Encode a negative integer (value stored as -1-n CBOR-style; n >= 0)
    fn nint(n: u64) -> Vec<u8> {
        let mut v = uint(n);
        v[0] |= 0x20;
        v
    }

    fn text(s: &str) -> Vec<u8> {
        let mut v = encode_uint_head(0x60, s.len() as u64);
        v.extend_from_slice(s.as_bytes());
        v
    }

    fn bytes_val(b: &[u8]) -> Vec<u8> {
        let mut v = encode_uint_head(0x40, b.len() as u64);
        v.extend_from_slice(b);
        v
    }

    fn bool_val(b: bool) -> Vec<u8> {
        vec![if b { 0xf5 } else { 0xf4 }]
    }

    fn null_val() -> Vec<u8> {
        vec![0xf6]
    }

    fn array(items: &[Vec<u8>]) -> Vec<u8> {
        let mut v = encode_uint_head(0x80, items.len() as u64);
        for i in items {
            v.extend_from_slice(i);
        }
        v
    }

    fn map_val(pairs: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut v = encode_uint_head(0xa0, pairs.len() as u64);
        for (k, val) in pairs {
            v.extend_from_slice(k);
            v.extend_from_slice(val);
        }
        v
    }

    fn tagged(tag: u64, inner: Vec<u8>) -> Vec<u8> {
        let mut v = encode_uint_head(0xc0, tag);
        v.extend_from_slice(&inner);
        v
    }

    fn encode_uint_head(base: u8, n: u64) -> Vec<u8> {
        if n <= 23 {
            vec![base | n as u8]
        } else if n <= 0xFF {
            vec![base | 24, n as u8]
        } else if n <= 0xFFFF {
            let b = (n as u16).to_be_bytes();
            vec![base | 25, b[0], b[1]]
        } else if n <= 0xFFFF_FFFF {
            let b = (n as u32).to_be_bytes();
            vec![base | 26, b[0], b[1], b[2], b[3]]
        } else {
            let b = n.to_be_bytes();
            vec![base | 27, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]
        }
    }

    // ── Primitives ────────────────────────────────────────────────────────

    #[test]
    fn uint_matches_uint() {
        ok(&uint(42), "x = uint");
    }

    #[test]
    fn uint_rejects_tstr() {
        fail(&text("hi"), "x = uint");
    }

    #[test]
    fn nint_matches_nint() {
        ok(&nint(4), "x = nint");
    }

    #[test]
    fn int_matches_uint() {
        ok(&uint(0), "x = int");
    }

    #[test]
    fn int_matches_nint() {
        ok(&nint(1), "x = int");
    }

    #[test]
    fn tstr_matches_text() {
        ok(&text("hello"), "x = tstr");
    }

    #[test]
    fn tstr_rejects_uint() {
        fail(&uint(1), "x = tstr");
    }

    #[test]
    fn bstr_matches_bytes() {
        ok(&bytes_val(b"\xde\xad"), "x = bstr");
    }

    #[test]
    fn bool_matches() {
        ok(&bool_val(true), "x = bool");
    }

    #[test]
    fn null_matches() {
        ok(&null_val(), "x = null");
    }

    #[test]
    fn any_matches_anything() {
        ok(&uint(99), "x = #");
    }

    // ── Literal values ────────────────────────────────────────────────────

    #[test]
    fn literal_uint_matches() {
        ok(&uint(7), "x = 7");
    }

    #[test]
    fn literal_uint_mismatch() {
        fail(&uint(8), "x = 7");
    }

    #[test]
    fn literal_text_matches() {
        ok(&text("hi"), r#"x = "hi""#);
    }

    #[test]
    fn literal_text_mismatch() {
        fail(&text("bye"), r#"x = "hi""#);
    }

    // ── Union ─────────────────────────────────────────────────────────────

    #[test]
    fn union_first_branch() {
        ok(&uint(1), "x = uint / tstr");
    }

    #[test]
    fn union_second_branch() {
        ok(&text("a"), "x = uint / tstr");
    }

    #[test]
    fn union_no_match() {
        fail(&bool_val(true), "x = uint / tstr");
    }

    // ── Arrays ────────────────────────────────────────────────────────────

    #[test]
    fn array_fixed() {
        ok(&array(&[uint(1), text("two")]), "x = [uint, tstr]");
    }

    #[test]
    fn array_wrong_elem_type() {
        fail(&array(&[text("one"), text("two")]), "x = [uint, tstr]");
    }

    #[test]
    fn array_wrong_length() {
        fail(&array(&[uint(1)]), "x = [uint, tstr]");
    }

    #[test]
    fn array_optional_elem_absent() {
        ok(&array(&[uint(1)]), "x = [uint, ? tstr]");
    }

    #[test]
    fn array_optional_elem_present() {
        ok(&array(&[uint(1), text("y")]), "x = [uint, ? tstr]");
    }

    #[test]
    fn array_star_zero() {
        ok(&array(&[]), "x = [* uint]");
    }

    #[test]
    fn array_star_many() {
        ok(&array(&[uint(1), uint(2), uint(3)]), "x = [* uint]");
    }

    #[test]
    fn array_plus_zero() {
        fail(&array(&[]), "x = [+ uint]");
    }

    // ── Named group spliced into an array (bug fix) ─────────────────────────
    //
    // A bare `Foo,` entry with no member key inside an array group is a
    // group inclusion when `Foo` resolves to a named group rule (per RFC
    // 8610 §3.5's positional-array-splice reading), exactly like the
    // already-supported map-inclusion case. Regression: `try_match_array_choice`
    // used to hand the *whole* referenced group to a single array item
    // (via `match_rule_name`) instead of splicing its own entries in
    // positionally, so `Headers` (a 2-field group) never matched against
    // two separate array slots.

    #[test]
    fn named_group_spliced_into_array_matches() {
        ok(
            &array(&[bytes_val(b"\x01"), bytes_val(b"\x02")]),
            "x = [ Headers ]\nHeaders = ( protected: bstr, unprotected: bstr )",
        );
    }

    #[test]
    fn named_group_spliced_into_array_with_trailing_fields() {
        ok(
            &array(&[bytes_val(b"\x01"), bytes_val(b"\x02"), uint(3)]),
            "x = [ Headers, c: uint ]\nHeaders = ( a: bstr, b: bstr )",
        );
    }

    #[test]
    fn named_group_spliced_into_array_wrong_arity_fails() {
        // Only one of `Headers`'s two fields present — must fail, not
        // silently match `Headers` against a single item.
        fail(
            &array(&[bytes_val(b"\x01")]),
            "x = [ Headers ]\nHeaders = ( protected: bstr, unprotected: bstr )",
        );
    }

    #[test]
    fn bare_typename_in_array_still_matches_per_item() {
        // A bare reference to an ordinary *type* rule (not a group rule)
        // must still be matched per-item, exactly as before this fix.
        ok(&array(&[uint(1), uint(2)]), "x = [y, y]\ny = uint");
    }

    // ── Maps ──────────────────────────────────────────────────────────────

    #[test]
    fn map_simple() {
        let cbor = map_val(&[(text("name"), text("Alice")), (text("age"), uint(30))]);
        ok(&cbor, "x = { name: tstr, age: uint }");
    }

    #[test]
    fn map_wrong_value_type() {
        let cbor = map_val(&[(text("name"), uint(42)), (text("age"), uint(30))]);
        fail(&cbor, "x = { name: tstr, age: uint }");
    }

    #[test]
    fn map_missing_required_key() {
        let cbor = map_val(&[(text("name"), text("Bob"))]);
        fail(&cbor, "x = { name: tstr, age: uint }");
    }

    #[test]
    fn map_optional_key_absent() {
        let cbor = map_val(&[(text("name"), text("Bob"))]);
        ok(&cbor, "x = { name: tstr, ? age: uint }");
    }

    // ── Tagged items ──────────────────────────────────────────────────────

    #[test]
    fn tagged_matches() {
        ok(&tagged(1, uint(42)), "x = #6.1(uint)");
    }

    #[test]
    fn tagged_wrong_tag() {
        fail(&tagged(2, uint(42)), "x = #6.1(uint)");
    }

    #[test]
    fn tagged_wrong_inner() {
        fail(&tagged(1, text("nope")), "x = #6.1(uint)");
    }

    // ── Range constraints ─────────────────────────────────────────────────

    #[test]
    fn range_inclusive_ok() {
        ok(&uint(5), "x = 0..10");
    }

    #[test]
    fn range_inclusive_boundary() {
        ok(&uint(10), "x = 0..10");
    }

    #[test]
    fn range_exclusive_boundary_fail() {
        fail(&uint(10), "x = 0...10");
    }

    // RFC 8610 §2.2.2.1's own worked example: `byte1 = 0...first-non-byte`
    // with `first-non-byte = 256` is stated to be equivalent to
    // `byte = 0..255` — the named upper bound must resolve and enforce
    // exactly as if `256` had been written literally.
    #[test]
    fn range_named_upper_bound_boundary_ok() {
        ok(&uint(255), "x = 0...first-non-byte\nfirst-non-byte = 256");
    }

    #[test]
    fn range_named_upper_bound_excludes_value() {
        fail(&uint(256), "x = 0...first-non-byte\nfirst-non-byte = 256");
    }

    #[test]
    fn range_named_lower_bound_is_enforced() {
        fail(&uint(1), "x = lowest..10\nlowest = 2");
    }

    #[test]
    fn range_named_bound_chain_of_aliases_resolves() {
        ok(&uint(255), "x = 0...top\ntop = mid\nmid = 256");
        fail(&uint(256), "x = 0...top\ntop = mid\nmid = 256");
    }

    // ── Control size ──────────────────────────────────────────────────────

    #[test]
    fn size_ok() {
        ok(&text("hi"), "x = tstr .size 2");
    }

    #[test]
    fn size_fail() {
        fail(&text("hello"), "x = tstr .size 2");
    }

    #[test]
    fn size_range_ok() {
        ok(&text("hello"), "x = tstr .size (1..10)");
    }

    #[test]
    fn size_range_too_short() {
        fail(&text(""), "x = tstr .size (1..10)");
    }

    #[test]
    fn size_range_too_long() {
        fail(&text("hello world!!"), "x = tstr .size (1..10)");
    }

    // ── .ne control operator ──────────────────────────────────────────────

    #[test]
    fn ne_ok() {
        ok(&uint(5), "x = uint .ne 0");
    }

    #[test]
    fn ne_fail_equal() {
        fail(&uint(0), "x = uint .ne 0");
    }

    // ── /= type extension ─────────────────────────────────────────────────

    #[test]
    fn type_extend_matches_first() {
        ok(&uint(1), "x = uint\nx /= tstr");
    }

    #[test]
    fn type_extend_matches_second() {
        ok(&text("a"), "x = uint\nx /= tstr");
    }

    #[test]
    fn type_extend_no_match() {
        fail(&bool_val(true), "x = uint\nx /= tstr");
    }

    // ── check_cbor / check_cbor_traced equivalence ──────────────────────────

    fn assert_traced_matches_untraced(cbor: &[u8], cddl_src: &str) {
        let cddl = schema(cddl_src);
        let untraced = check_cbor(cbor, &cddl, None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        let (traced, trace) = check_cbor_traced(cbor, &cddl, None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert_eq!(untraced, traced, "trace wrapper changed mismatch results");
        assert!(!trace.is_empty(), "expected at least one trace event");
        let root_span = CborSpan {
            start: 0,
            end: cbor.len(),
        };
        assert_eq!(
            trace[0].cbor_span,
            Some(root_span),
            "root trace event should span the whole CBOR input"
        );
    }

    #[test]
    fn traced_matches_untraced_on_success() {
        assert_traced_matches_untraced(&uint(42), "x = uint");
    }

    #[test]
    fn traced_matches_untraced_on_failure() {
        assert_traced_matches_untraced(&bool_val(true), "x = uint");
    }

    #[test]
    fn traced_matches_untraced_on_nested_map() {
        assert_traced_matches_untraced(
            &{
                let mut v = vec![0xa1];
                v.extend(text("a"));
                v.extend(uint(1));
                v
            },
            "x = { a: uint }",
        );
    }

    #[test]
    fn nested_array_element_cbor_span_is_absolute() {
        // x = [uint, y], y = tstr
        // CBOR: [1, "hi"] => 0x82 0x01 0x62 'h' 'i' (5 bytes total).
        // Element 0 (uint 1) occupies byte 1..2; element 1 ("hi") occupies
        // bytes 2..5 (0x62 header + 2 chars). The inner rule `y` is matched
        // against element 1, so its trace event's cbor_span must reflect
        // element 1's *absolute* position in the outer buffer (2..5), not a
        // position relative to the array's own data sub-slice (1..4).
        let cbor = array(&[uint(1), text("hi")]);
        let cddl = schema("x = [uint, y]\ny = tstr");
        let (mismatches, trace) = check_cbor_traced(&cbor, &cddl, None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert!(mismatches.is_empty(), "expected match, got: {mismatches:#?}");

        let y_event = trace
            .iter()
            .find(|e| e.rule_name == "y")
            .expect("expected a trace event for rule `y`");
        assert_eq!(y_event.cbor_span, Some(CborSpan { start: 2, end: 5 }));
    }

    #[test]
    fn nested_map_value_cbor_span_is_absolute() {
        // x = { a: y }, y = tstr
        // CBOR: {"a": "v"} => 0xa1 0x61 'a' 0x61 'v' (5 bytes total).
        // The value "v" occupies bytes 3..5; the inner rule `y` is matched
        // against it, so its trace event's cbor_span must be 3..5.
        let cbor = map_val(&[(text("a"), text("v"))]);
        let cddl = schema("x = { a: y }\ny = tstr");
        let (mismatches, trace) = check_cbor_traced(&cbor, &cddl, None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert!(mismatches.is_empty(), "expected match, got: {mismatches:#?}");

        let y_event = trace
            .iter()
            .find(|e| e.rule_name == "y")
            .expect("expected a trace event for rule `y`");
        assert_eq!(y_event.cbor_span, Some(CborSpan { start: 3, end: 5 }));
    }

    #[test]
    fn tagged_inner_cbor_span_is_absolute() {
        // x = #6.1(y), y = uint
        // CBOR: #6.1(42) => 0xc1 0x18 0x2a (3 bytes total).
        // The inner value 42 occupies bytes 1..3; the inner rule `y` is
        // matched against it, so its trace event's cbor_span must be 1..3.
        let cbor = tagged(1, uint(42));
        let cddl = schema("x = #6.1(y)\ny = uint");
        let (mismatches, trace) = check_cbor_traced(&cbor, &cddl, None, &Config::strict())
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert!(mismatches.is_empty(), "expected match, got: {mismatches:#?}");

        let y_event = trace
            .iter()
            .find(|e| e.rule_name == "y")
            .expect("expected a trace event for rule `y`");
        assert_eq!(y_event.cbor_span, Some(CborSpan { start: 1, end: 3 }));
    }

    // ── RFC 8610 §3.9 socket/plug: `//=` group extension ────────────────────

    const TWO_EXTENSIONS: &str = "\
        x = { * $$c }\n\
        $$c //= ( &(a: 1) => uint )\n\
        $$c //= ( &(b: 2) => uint )\n";

    #[test]
    fn group_extension_first_choice_matches() {
        ok(&map_val(&[(uint(1), uint(99))]), TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_second_choice_matches() {
        // Regression: expand_group_name previously used find_map and only
        // ever returned the *first* rule sharing a `//=`-extended name, so
        // the second extension's key (2) was invisible to the matcher and
        // any value at that key passed silently.
        ok(&map_val(&[(uint(2), uint(99))]), TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_second_choice_wrong_value_type_fails() {
        fail(&map_val(&[(uint(2), text("garbage"))]), TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_both_choices_match() {
        ok(
            &map_val(&[(uint(1), uint(1)), (uint(2), uint(2))]),
            TWO_EXTENSIONS,
        );
    }

    // ── RFC 8610 §3.2/§3.9: occurrence on a group-inclusion entry ───────────

    #[test]
    fn star_group_inclusion_allows_empty_map() {
        // Regression: the `*` in `{ * $$c }` was dropped when the included
        // group's entries were flattened in, so a bare `&(a: 1) => uint`
        // extension (no occurrence indicator of its own) was treated as
        // mandatory — exactly the opposite of the RFC 8610 §3.9 socket/plug
        // idiom, which requires every plug to be individually optional.
        ok(&map_val(&[]), "x = { * $$c }\n$$c //= ( &(a: 1) => uint )");
    }

    #[test]
    fn optional_group_inclusion_allows_empty_map() {
        ok(&map_val(&[]), "x = { ? $$c }\n$$c //= ( &(a: 1) => uint )");
    }

    #[test]
    fn required_group_inclusion_still_mandatory() {
        // A group-inclusion with no occurrence indicator (exactly-once) must
        // still require its entries — only zero-permitting indicators (`?`,
        // `*`, `n*m` with n absent/0) propagate optionality.
        fail(&map_val(&[]), "x = { c }\nc = ( a: uint )");
    }

    // ── Map-choice matching: value-mismatch reporting, unexpected keys ──────

    #[test]
    fn wildcard_value_type_mismatch_is_reported() {
        // Regression: a key matching a repeating wildcard's key pattern
        // whose value failed the wildcard's value type used to have its
        // mismatch silently discarded (truncated) instead of reported.
        fail(&map_val(&[(text("a"), text("wrong"))]), "x = { * tstr => uint }");
    }

    #[test]
    fn non_repeating_type_key_does_not_suppress_unexpected_key() {
        // Regression: has_wildcard treated ANY MemberKey::Type entry (even
        // a single, non-repeating `&(name: N) => type` entry) as a
        // catch-all, suppressing "unexpected key" detection for the whole
        // map choice.
        fail(
            &map_val(&[(uint(0), text("n")), (uint(99), text("extra"))]),
            "x = { &( name: 0 ) => tstr, ? &( urlInfo: 1 ) => tstr }",
        );
    }

    #[test]
    fn genuine_wildcard_still_suppresses_unexpected_key() {
        ok(
            &map_val(&[(text("anything"), uint(1))]),
            "x = { * tstr => uint }",
        );
    }

    // ── `.cbor` / `.cborseq` (RFC 8610 §3.8.4) ──────────────────────────────

    #[test]
    fn cbor_control_valid_nested_matches() {
        ok(&bytes_val(&uint(42)), "x = bstr .cbor uint");
    }

    #[test]
    fn cbor_control_invalid_nested_mismatches() {
        fail(&bytes_val(&text("hi")), "x = bstr .cbor uint");
    }

    #[test]
    fn cbor_control_invalid_outer_bytes_reported_not_panic() {
        // 0xff alone (major type 7, additional info 31) is not a valid
        // standalone CBOR item — must be reported as a mismatch, not panic.
        fail(&bytes_val(&[0xff]), "x = bstr .cbor uint");
    }

    #[test]
    fn cborseq_control_zero_elements_ok() {
        ok(&bytes_val(&[]), "x = bstr .cborseq [* uint]");
    }

    #[test]
    fn cborseq_control_one_element_ok() {
        ok(&bytes_val(&uint(7)), "x = bstr .cborseq [* uint]");
    }

    #[test]
    fn cborseq_control_n_elements_ok() {
        let mut content = uint(1);
        content.extend(uint(2));
        content.extend(uint(3));
        ok(&bytes_val(&content), "x = bstr .cborseq [* uint]");
    }

    #[test]
    fn cborseq_control_fixed_shape_ok() {
        let mut content = uint(1);
        content.extend(text("two"));
        ok(&bytes_val(&content), "x = bstr .cborseq [uint, tstr]");
    }

    #[test]
    fn cborseq_control_element_mismatch_fails() {
        let mut content = uint(1);
        content.extend(text("nope"));
        fail(&bytes_val(&content), "x = bstr .cborseq [* uint]");
    }

    #[test]
    fn cborseq_control_invalid_bytes_reported_not_panic() {
        fail(&bytes_val(&[0xff]), "x = bstr .cborseq [* uint]");
    }

    // ── Control operators unimplemented at check time (Stage 1 findings) ──
    //
    // `check_constraint`'s `ConstraintOp::Control(op)` match falls through to
    // a silent `_ => {}` for any operator not in its explicit list. `.and`/
    // `.within`/`.bits`/`.default` are fixed — see below. `.regexp` is
    // deliberately, permanently unenforced here pending a mature XSD-regex
    // crate — see `regexp_control_is_deliberately_unenforced` below.

    #[test]
    fn and_control_is_enforced() {
        // RFC 8610 §3.8.5: `.and` requires the value to match BOTH the
        // target (`uint`) and the controller (`0..5`) — this is the
        // intersection semantics. 10 is a `uint` but is outside `0..5`, so a
        // conformant implementation must reject it.
        //
        // STAGE 1 FINDING (fixed): `check_constraint` now checks the value
        // against the controller type for `.and`/`.within` in addition to
        // the target type.
        fail(&uint(10), "x = uint .and (0..5)");
        ok(&uint(3), "x = uint .and (0..5)");
    }

    #[test]
    fn within_control_is_enforced() {
        // RFC 8610 §3.8.5: `.within` has identical formal semantics to
        // `.and` (intersection); same fix as `and_control_is_enforced`.
        fail(&uint(10), "x = uint .within (0..5)");
        ok(&uint(3), "x = uint .within (0..5)");
    }

    #[test]
    fn bits_control_is_enforced_on_bytestring() {
        // RFC 8610 §3.8.2: `.bits` on a byte string restricts which bit
        // positions may be set. Byte 0x20 sets bit 5, which is not in the
        // enumerated set {0, 1} for `flag-bits`, so a conformant
        // implementation must reject it.
        fail(
            &bytes_val(&[0x20]),
            "x = bstr .bits flag-bits\nflag-bits = &(a: 0, b: 1)",
        );
        // Byte 0x03 sets bits 0 and 1, both in the enumerated set.
        ok(
            &bytes_val(&[0x03]),
            "x = bstr .bits flag-bits\nflag-bits = &(a: 0, b: 1)",
        );
    }

    #[test]
    fn bits_control_is_enforced_on_uint() {
        // RFC 8610 §3.8.2, `rwxbits` example: on an unsigned integer, every
        // set bit's position must be named by the controller type. Bit 0
        // (value 1) is not in the enumerated set {r:2, w:1}.
        fail(&uint(1), "x = uint .bits rwx\nrwx = &(r: 2, w: 1)");
        ok(&uint(6), "x = uint .bits rwx\nrwx = &(r: 2, w: 1)");
    }

    #[test]
    fn regexp_control_is_deliberately_unenforced() {
        // RFC 8610 §3.8.3: `.regexp` requires the text string to match the
        // given XSD regular expression. "abc" does not match `^[0-9]+$`, so
        // a conformant implementation would reject it — but no XSD-conformant
        // regex engine is mature enough in the Rust ecosystem to depend on
        // (Stage 1 Finding 4, deferred by human ruling 2026-08-08). The
        // validator now warns on every `.regexp` usage instead (see
        // `validator.rs::warning_regexp_not_enforced`), and codegen emits a
        // "// TODO: implement regex matching" comment — but this checker
        // still cannot verify the match, so it passes by design.
        ok(&text("abc"), "x = tstr .regexp \"^[0-9]+$\"");
    }

    #[test]
    fn default_control_rejects_value_equal_to_default() {
        // RFC 8610 §3.8.6: `.default` implies a `.ne` against the default
        // value (the default is meant to be omitted from the wire, not
        // sent explicitly equal to itself). Sending 0 against
        // `uint .default 0` must therefore fail (Stage 1 Finding 2, fixed).
        fail(&uint(0), "x = uint .default 0");
        // Any other value is unaffected by the `.default` control.
        ok(&uint(1), "x = uint .default 0");
    }

    /// RFC 8610 §3.8.6's own worked example, verbatim (`timer`), against a
    /// stress-test comparison against Rust `cddl`, the Ruby `cddl` gem, and
    /// Python `cddl_verifier` (documented in `CDDL_STRESS_TEST.md`,
    /// Divergence 14) which found all three wrongly accept a
    /// `displayed-step` explicitly sent equal to its default (1) — only
    /// `tps_cddl` and `@cbortech/cbor` correctly reject it. This locks in
    /// the correct behaviour for the full worked example, not just the bare
    /// `.default` case above: omitted key, present-and-non-default, and the
    /// two rejection cases (equal to default; present but failing `.gt 0`).
    #[test]
    fn default_control_rfc_worked_example_timer() {
        const SCHEMA: &str = "x = timer\ntimer = { time: uint, ? displayed-step: (number .gt 0) .default 1 }";

        // Key omitted entirely — the conformant, expected wire form.
        ok(&map_val(&[(text("time"), uint(5))]), SCHEMA);

        // Present, non-default, satisfies `.gt 0`.
        ok(
            &map_val(&[(text("time"), uint(5)), (text("displayed-step"), uint(2))]),
            SCHEMA,
        );

        // Present and equal to the default — the implied `.ne` must reject this.
        fail(
            &map_val(&[(text("time"), uint(5)), (text("displayed-step"), uint(1))]),
            SCHEMA,
        );

        // Present but fails the base `.gt 0` constraint.
        fail(
            &map_val(&[(text("time"), uint(5)), (text("displayed-step"), uint(0))]),
            SCHEMA,
        );
    }

    // ── Non-literal tag numbers (RFC 9682 §3.2) unenforced at check time ──

    #[test]
    fn non_literal_tag_number_range_is_enforced() {
        // RFC 9682 §3.2 defines `#6.<type>(...)` where the head-number can
        // be a type expression (e.g. a range) rather than a literal.
        //
        // STAGE 1 FINDING (fixed): `match_tagged` now checks the actual tag
        // value against the head-number type when it is
        // `HeadNumber::Type(_)`. Tag 5 is outside the declared `100..200`
        // range and is rejected; tag 150 is within range and accepted.
        let cfg = Config::all();
        let schema_src = "x = #6.<tagrange>(uint)\ntagrange = 100..200";

        let bad_cbor = {
            let mut v = vec![0xc5]; // tag(5), one-byte tag encoding
            v.extend_from_slice(&uint(1));
            v
        };
        let m = check_cbor(&bad_cbor, &schema(schema_src), None, &cfg)
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert!(
            !m.is_empty(),
            "tag 5 is outside `tagrange = 100..200` and must be rejected"
        );

        let good_cbor = {
            let mut v = vec![0xd8, 150u8]; // tag(150), two-byte tag encoding
            v.extend_from_slice(&uint(1));
            v
        };
        let m = check_cbor(&good_cbor, &schema(schema_src), None, &cfg)
            .unwrap_or_else(|e| panic!("CBOR decode error: {e}"));
        assert!(
            m.is_empty(),
            "tag 150 is within `tagrange = 100..200` and should be accepted; got: {m:#?}"
        );
    }

    // ── Generic instantiation (RFC 8610 §3.10) ────────────────────────────

    /// RFC 8610 §3.10: `pair<a, b> = [a, b]` with usage `pair<uint, tstr>`
    /// binds `a = uint`, `b = tstr` and checks the array positionally
    /// against those bound types.
    ///
    /// STAGE 1 FINDING (fixed): `match_type2` used to treat any
    /// `Type2::Typename` with `args.is_some()` (i.e. any generic
    /// instantiation) as `any`, so a value that violates the bound types
    /// was wrongly accepted. `match_generic_instantiation` now binds the
    /// generic parameters to the supplied arguments and substitutes them
    /// throughout the rule body before matching.
    #[test]
    fn generic_instantiation_type_args_are_checked() {
        fail(
            &array(&[text("x"), text("a")]),
            "x = pair<uint,tstr>\npair<a,b> = [a,b]",
        );
        ok(
            &array(&[uint(1), text("a")]),
            "x = pair<uint,tstr>\npair<a,b> = [a,b]",
        );
    }

    /// STAGE 3 FINDING 5 (fixed): a schema with a long chain of distinct
    /// alias rules (`a0 = a1`, `a1 = a2`, ...) recurses once per rule via
    /// `match_rule_name`, with no repeated name for `in_progress` to catch
    /// and no CBOR nesting to bound it. `MAX_MATCH_DEPTH` now caps this
    /// independently of `tps_minicbor`'s own CBOR-nesting depth limit,
    /// reporting a mismatch instead of overflowing the stack.
    #[test]
    fn long_alias_chain_past_depth_limit_is_reported_not_stack_overflow() {
        let mut src = String::new();
        for i in 0..500 {
            src.push_str(&format!("a{i} = a{}\n", i + 1));
        }
        src.push_str("a500 = uint\n");
        fail(&uint(1), &src);
    }

    /// Same alias-chain shape, but short enough to stay under
    /// `MAX_MATCH_DEPTH` — must still match successfully.
    #[test]
    fn short_alias_chain_under_depth_limit_still_matches() {
        let mut src = String::new();
        for i in 0..10 {
            src.push_str(&format!("a{i} = a{}\n", i + 1));
        }
        src.push_str("a10 = uint\n");
        ok(&uint(1), &src);
    }

    /// STAGE 3 FINDING 4 (fixed): `match_generic_instantiation` recurses
    /// into `match_type`/`match_array_group`/`match_map_group` without ever
    /// routing back through `match_rule_name`'s own `MAX_MATCH_DEPTH`
    /// check, so a long chain of distinct *generic* alias rules
    /// (`w0<a> = uint`, `w1<a> = w0<a>`, ...) previously recursed
    /// unboundedly and stack-overflowed — confirmed live at n=1000 before
    /// this fix (n=100 also overflowed once the top-level rule itself is a
    /// generic instantiation, since `match_generic_instantiation` is the
    /// very first frame and never bumped `depth` at all).
    /// `match_generic_instantiation` now checks/increments `depth` itself,
    /// independent of and identical to `match_rule_name`'s cap. `w0`'s body
    /// deliberately ignores its own parameter (`uint`, not `a`) since
    /// substituting a bound parameter into a *nested* generic argument list
    /// is a separately-documented, unrelated limitation of
    /// `substitute_generic_type2` (see its doc comment in `ast.rs`) — this
    /// test targets the recursion depth alone.
    #[test]
    fn long_generic_alias_chain_past_depth_limit_is_reported_not_stack_overflow() {
        let mut src = String::from("x = w499<uint>\nw0<a> = uint\n");
        for i in 1..500 {
            src.push_str(&format!("w{i}<a> = w{}<a>\n", i - 1));
        }
        fail(&uint(1), &src);
    }

    /// Same generic-alias-chain shape, but short enough to stay under
    /// `MAX_MATCH_DEPTH` — must still match successfully.
    #[test]
    fn short_generic_alias_chain_under_depth_limit_still_matches() {
        let mut src = String::from("x = w9<uint>\nw0<a> = uint\n");
        for i in 1..10 {
            src.push_str(&format!("w{i}<a> = w{}<a>\n", i - 1));
        }
        ok(&uint(1), &src);
    }

    /// `substitute_generic_type2` used to leave nested generic
    /// instantiations untouched (`_ => t2.clone()` in `ast.rs`), so a
    /// generic rule's own parameter passed as an argument to *another*
    /// generic instantiation inside its body — e.g. `Outer<C> =
    /// Wrapped<C>` — was never substituted. `Wrapped` then saw a literal,
    /// unresolved reference to `C` (not the caller's bound type), which
    /// `find_rules` failed to resolve, silently falling through to "any"
    /// instead of enforcing `Wrapped`'s constraint (`.size (5..10)`).
    /// Found via `CDDL_STRESS_TEST.md`'s external-tool comparison work,
    /// while building fixtures resembling RFC 9711's nested `JC<...>`/
    /// `CBOR-ONLY<...>` generic pattern.
    #[test]
    fn nested_generic_instantiation_substitutes_outer_binding() {
        const SCHEMA: &str = "x = Outer<bstr>\nWrapped<a> = a .size (5..10)\nOuter<c> = Wrapped<c>";
        fail(&bytes_val(&[0u8; 3]), SCHEMA);
        ok(&bytes_val(&[0u8; 7]), SCHEMA);
        fail(&bytes_val(&[0u8; 12]), SCHEMA);
    }
}
