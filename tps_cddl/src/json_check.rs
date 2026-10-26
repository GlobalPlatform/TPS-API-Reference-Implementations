/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Phase 4: match a JSON value against a CDDL schema.
//!
//! The public entry point is [`check_json`].  It returns a list of
//! [`JsonMismatch`] values — one per structural inconsistency found.  An
//! empty list means the value conforms to the schema.

use std::collections::HashSet;

use serde_json::Value;

use crate::{
    Config,
    ast::{
        Cddl, Constraint, ConstraintOp, GenericArg, Group, GrpChoice, GrpEntBody, HeadNumber,
        MemberKey, Occur, Rule, RuleBody, Type, Type1, Type2, Value as CddlValue,
        flatten_array_entries, flatten_map_entries, generic_bindings, substitute_generic_grpent,
        substitute_generic_type,
    },
};

// ─── Public types ──────────────────────────────────────────────────────────

/// A single mismatch between a JSON value and the expected CDDL schema.
///
/// Mismatches are returned by [`check_json`].  The `path` field uses a
/// dot-separated / bracket-indexed syntax so error messages can point to
/// the exact location within a nested JSON structure.
#[derive(Debug, Clone)]
pub struct JsonMismatch {
    /// JSON-pointer-like path to the mismatching value (e.g. `root.address[2]`).
    pub path: String,
    /// Human-readable description of what was expected.
    pub expected: String,
    /// Human-readable description of what was found.
    pub got: String,
}

// ─── Public entry point ────────────────────────────────────────────────────

/// Validate a UTF-8 JSON byte slice against a CDDL schema.
///
/// `root` selects the rule to validate against; `None` uses the first rule in
/// the schema.  Returns `Ok(Vec::new())` when the JSON conforms to the schema,
/// `Ok(mismatches)` when it does not, and `Err(message)` when `json` is not
/// valid JSON.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, check_json, Config};
///
/// let schema_src = r#"x = { name: tstr }"#;
/// let pairs = lex(schema_src).unwrap();
/// let schema = build_cddl(pairs).unwrap();
///
/// let json = br#"{"name": "Alice"}"#;
/// let result = check_json(json, &schema, None, &Config::strict()).unwrap();
/// assert!(result.is_empty());
/// ```
pub fn check_json(
    json: &[u8],
    cddl: &Cddl,
    root: Option<&str>,
    _config: &Config,
) -> Result<Vec<JsonMismatch>, String> {
    let root_name = match resolve_root(cddl, root) {
        Some(n) => n,
        None => {
            return Ok(vec![JsonMismatch {
                path: String::new(),
                expected: "a non-empty CDDL schema".into(),
                got: "empty schema".into(),
            }]);
        }
    };

    let value: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;

    let mut ctx = MatchCtx::new(cddl);
    ctx.match_rule_name(&value, root_name, root_name);
    Ok(ctx.mismatches)
}

// ─── Internal context ──────────────────────────────────────────────────────

/// Cap on named-rule recursion depth (see [`MatchCtx::match_rule_name`] and
/// [`MatchCtx::match_generic_instantiation`]).
///
/// Mirrors [`crate::cbor_check::MAX_MATCH_DEPTH`] — `in_progress` only
/// catches cycles that revisit the *same* rule name; a schema with a long
/// chain of distinct alias rules (`a0 = a1`, `a1 = a2`, ...) recurses once
/// per rule with no repeated name, and JSON parsing has no independent
/// nesting bound of its own the way `tps_minicbor` does for CBOR. The same
/// applies to a chain of distinct generic instantiations — see the CBOR
/// module's identical note.
const MAX_MATCH_DEPTH: usize = 64;

struct MatchCtx<'c> {
    cddl: &'c Cddl,
    mismatches: Vec<JsonMismatch>,
    in_progress: HashSet<String>,
    depth: usize,
}

impl<'c> MatchCtx<'c> {
    fn new(cddl: &'c Cddl) -> Self {
        Self {
            cddl,
            mismatches: Vec::new(),
            in_progress: HashSet::new(),
            depth: 0,
        }
    }

    fn mismatch(&mut self, path: &str, expected: impl Into<String>, got: impl Into<String>) {
        self.mismatches.push(JsonMismatch {
            path: path.to_string(),
            expected: expected.into(),
            got: got.into(),
        });
    }

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
            Type2::Value(v) => cddl_value_as_f64(v),
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

    fn match_rule_name(&mut self, val: &Value, name: &str, path: &str) {
        if self.depth >= MAX_MATCH_DEPTH {
            self.mismatch(
                path,
                format!("type `{name}`"),
                "schema nesting too deep".to_string(),
            );
            return;
        }

        if let Some(prelude_match) = self.match_prelude(val, name) {
            if !prelude_match {
                self.mismatch(path, format!("type `{name}`"), json_description(val));
            }
            return;
        }

        if self.in_progress.contains(name) {
            return; // Cycle — treat as any
        }

        let rules = self.find_rules(name);
        if rules.is_empty() {
            self.mismatch(path, format!("type `{name}`"), "undefined rule".to_string());
            return;
        }

        if rules.iter().any(|r| r.generic_parm.is_some()) {
            return; // Generic rules not supported — treat as any
        }

        self.in_progress.insert(name.to_string());
        self.depth += 1;

        if rules.len() == 1 {
            let body = rules[0].body.clone();
            self.match_rule_body(val, body, name, path);
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
                let span = rules[0].span.clone();
                let fake_group = Group {
                    choices: group_choices,
                    span,
                };
                match val {
                    Value::Array(_) => self.match_array_group(val, &fake_group, path),
                    Value::Object(_) => self.match_map_group(val, &fake_group, path),
                    _ => self.mismatch(path, format!("group `{name}`"), json_description(val)),
                }
            } else {
                let span = rules[0].span.clone();
                let union_type = crate::ast::Type {
                    choices: type_choices,
                    span,
                };
                self.match_type(val, &union_type, path);
            }
        }

        self.in_progress.remove(name);
        self.depth -= 1;
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
    fn match_generic_instantiation(&mut self, val: &Value, name: &str, args: &GenericArg, path: &str) {
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
                self.match_type(val, &substituted, path);
            }
            RuleBody::Group(grp_ent) => {
                let substituted = substitute_generic_grpent(grp_ent, &bindings);
                let span = substituted.span.clone();
                let fake_group = Group {
                    choices: vec![GrpChoice {
                        entries: vec![substituted],
                        span: span.clone(),
                    }],
                    span,
                };
                match val {
                    Value::Array(_) => self.match_array_group(val, &fake_group, path),
                    Value::Object(_) => self.match_map_group(val, &fake_group, path),
                    _ => self.mismatch(path, format!("group `{name}`"), json_description(val)),
                }
            }
        }
        self.depth -= 1;
    }

    fn match_rule_body(&mut self, val: &Value, body: RuleBody, name: &str, path: &str) {
        match body {
            RuleBody::Type(t) => self.match_type(val, &t, path),
            RuleBody::Group(grp_ent) => {
                let span = grp_ent.span.clone();
                let fake_group = Group {
                    choices: vec![GrpChoice {
                        entries: vec![grp_ent],
                        span: span.clone(),
                    }],
                    span,
                };
                match val {
                    Value::Array(_) => self.match_array_group(val, &fake_group, path),
                    Value::Object(_) => self.match_map_group(val, &fake_group, path),
                    _ => self.mismatch(path, format!("group `{name}`"), json_description(val)),
                }
            }
        }
    }

    /// Returns `Some(true)` if the prelude name matches, `Some(false)` if not,
    /// `None` if the name is not a prelude name.
    fn match_prelude(&self, val: &Value, name: &str) -> Option<bool> {
        let ok = match name {
            "any" => true,
            "uint" => val.as_u64().is_some(),
            "nint" => {
                // JSON negative integer: i64 < 0
                matches!(val.as_i64(), Some(n) if n < 0)
            }
            "int" => val.as_i64().is_some(),
            "bstr" | "bytes" => false, // No JSON representation for bytes
            "tstr" | "text" => val.is_string(),
            // JSON has no float-width distinctions; map all to any JSON number
            "float16" | "float32" | "float64" | "float" => val.is_number(),
            "number" => val.is_number(),
            "bool" => val.is_boolean(),
            "false" => matches!(val, Value::Bool(false)),
            "true" => matches!(val, Value::Bool(true)),
            "nil" | "null" => val.is_null(),
            "undefined" => false, // JSON has no undefined
            // Semantically string-backed prelude types — accept JSON strings
            "tdate" | "time" | "uri" | "b64url" | "b64legacy" | "regexp" | "mime-message" => {
                val.is_string()
            }
            "cbor-any" | "encoded-cbor" => false, // No JSON representation
            // Big integer types — no JSON representation
            "biguint" | "bignint" | "bigint" | "integer" | "unsigned" => {
                // Accept JSON integers as approximation
                val.as_i64().is_some()
            }
            _ => return None,
        };
        Some(ok)
    }

    fn match_type(&mut self, val: &Value, ty: &Type, path: &str) {
        if ty.choices.len() == 1 {
            self.match_type1(val, &ty.choices[0], path);
            return;
        }

        let save_len = self.mismatches.len();
        for choice in &ty.choices {
            let before = self.mismatches.len();
            self.match_type1(val, choice, path);
            if self.mismatches.len() == before {
                return; // This choice matched
            }
            self.mismatches.truncate(before);
        }
        // All choices failed
        let choices: Vec<String> = ty.choices.iter().map(type1_description).collect();
        self.mismatch(path, choices.join(" / "), json_description(val));
        self.mismatches.truncate(save_len + 1);
    }

    fn match_type1(&mut self, val: &Value, t1: &Type1, path: &str) {
        if let Some(constraint) = &t1.constraint
            && matches!(
                constraint.op,
                ConstraintOp::RangeInclusive | ConstraintOp::RangeExclusive
            )
        {
            self.check_full_range(val, &t1.base, constraint, path);
            return;
        }

        let before = self.mismatches.len();
        self.match_type2(val, &t1.base, path);
        if self.mismatches.len() > before {
            return;
        }
        if let Some(constraint) = &t1.constraint {
            self.check_constraint(val, constraint, path);
        }
    }

    fn match_type2(&mut self, val: &Value, t2: &Type2, path: &str) {
        match t2 {
            Type2::Any(_) => {}

            Type2::Value(v) => self.match_value_literal(val, v, path),

            Type2::Typename { name, args, .. } => match args {
                Some(ga) => self.match_generic_instantiation(val, name, ga, path),
                None => self.match_rule_name(val, name, path),
            },

            Type2::ParenType(ty) => self.match_type(val, ty, path),

            Type2::Map(g) => match val {
                Value::Object(_) => self.match_map_group(val, g, path),
                _ => self.mismatch(path, "object", json_description(val)),
            },

            Type2::Array(g) => match val {
                Value::Array(_) => self.match_array_group(val, g, path),
                _ => self.mismatch(path, "array", json_description(val)),
            },

            // JSON has no tagged types or major types
            Type2::Tagged { .. } => {
                self.mismatch(
                    path,
                    "tagged CBOR value (not representable in JSON)",
                    json_description(val),
                );
            }
            Type2::MajorType { major, ai, .. } => {
                self.mismatch(
                    path,
                    format!(
                        "CBOR major type {} (not representable in JSON)",
                        match ai {
                            Some(HeadNumber::Literal(n)) => format!("#{major}.{n}"),
                            _ => format!("#{major}"),
                        }
                    ),
                    json_description(val),
                );
            }

            Type2::EnumGroup(g) => self.match_enum_group(val, g, path),

            Type2::EnumRef { .. } => {
                // Enum refs to named groups treated as any in Phase 4
            }

            Type2::Unwrap { .. } => {
                self.mismatch(
                    path,
                    "unwrapped CBOR tag (not representable in JSON)",
                    json_description(val),
                );
            }
        }
    }

    // ── Literal value matching ──────────────────────────────────────────

    fn match_value_literal(&mut self, val: &Value, v: &CddlValue, path: &str) {
        let matches = match v {
            CddlValue::Int(n) => {
                if *n >= 0 {
                    val.as_u64() == Some(*n as u64)
                } else {
                    val.as_i64() == Some(*n)
                }
            }
            CddlValue::Float(f) => val.as_f64() == Some(*f),
            CddlValue::Text(s) => val.as_str() == Some(s.as_str()),
            CddlValue::Bytes(_) => false,
        };
        if !matches {
            self.mismatch(path, cddl_value_description(v), json_description(val));
        }
    }

    // ── Enum group matching ─────────────────────────────────────────────

    fn match_enum_group(&mut self, val: &Value, group: &Group, path: &str) {
        for choice in &group.choices {
            for entry in &choice.entries {
                if let GrpEntBody::Type(t) = &entry.entry {
                    let before = self.mismatches.len();
                    self.match_type(val, t, path);
                    if self.mismatches.len() == before {
                        return;
                    }
                    self.mismatches.truncate(before);
                }
            }
        }
        self.mismatch(path, "enum member", json_description(val));
    }

    // ── Constraint checking ─────────────────────────────────────────────

    fn check_constraint(&mut self, val: &Value, constraint: &Constraint, path: &str) {
        let target_val = match &constraint.target {
            Type2::Value(v) => Some(v.clone()),
            _ => None,
        };

        match &constraint.op {
            ConstraintOp::RangeInclusive | ConstraintOp::RangeExclusive => {
                let inclusive = constraint.op == ConstraintOp::RangeInclusive;
                if let Some(target) = target_val {
                    self.check_range(val, &target, inclusive, path);
                }
            }
            ConstraintOp::Control(op) => match op.as_str() {
                "size" => match &constraint.target {
                    Type2::Value(CddlValue::Int(n)) => {
                        self.check_size(val, *n as u64, path);
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
                            let len = json_len(val);
                            if let (Some(l), Some(lo)) = (len, lo)
                                && (l as f64) < lo
                            {
                                self.mismatch(path, format!(".size >= {lo}"), format!(".size {l}"));
                            }
                            if let (Some(l), Some(hi)) = (len, hi) {
                                let lf = l as f64;
                                let ok = if inclusive { lf <= hi } else { lf < hi };
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
                        self.match_value_literal(val, &v, path);
                    }
                }
                "ne" => {
                    if let Some(v) = target_val {
                        let before = self.mismatches.len();
                        self.match_value_literal(val, &v, path);
                        if self.mismatches.len() == before {
                            // Matched — but .ne means it should NOT match
                            self.mismatch(
                                path,
                                format!(".ne {}", cddl_value_description(&v)),
                                json_description(val),
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
                        self.match_value_literal(val, &v, path);
                        if self.mismatches.len() == before {
                            self.mismatch(
                                path,
                                format!(
                                    ".default {} (value must be omitted, not sent)",
                                    cddl_value_description(&v)
                                ),
                                json_description(val),
                            );
                        } else {
                            self.mismatches.truncate(before);
                        }
                    }
                }
                "lt" | "le" | "gt" | "ge" => {
                    if let Some(target) = target_val {
                        self.check_comparison(val, op, &target, path);
                    }
                }
                "and" | "within" => {
                    self.match_type2(val, &constraint.target, path);
                }
                "bits" => self.check_bits_control(val, &constraint.target, path),
                _ => {} // Unsupported control operator — skip
            },
        }
    }

    fn check_full_range(
        &mut self,
        val: &Value,
        lower_t2: &Type2,
        constraint: &Constraint,
        path: &str,
    ) {
        let inclusive = constraint.op == ConstraintOp::RangeInclusive;
        let v = match val.as_f64() {
            Some(n) => n,
            None => {
                self.mismatch(path, "numeric type for range", json_description(val));
                return;
            }
        };

        let lower = self.resolve_type2_as_f64(lower_t2, 0);
        if let Some(lo) = lower
            && v < lo
        {
            let op_str = if inclusive { ".." } else { "..." };
            self.mismatch(path, format!("{lo}{op_str}"), format!("{v}"));
            return;
        }

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

    fn check_range(&mut self, val: &Value, upper: &CddlValue, inclusive: bool, path: &str) {
        let v = val.as_f64();
        let hi = cddl_value_as_f64(upper);
        if let (Some(v), Some(hi)) = (v, hi) {
            let ok = if inclusive { v <= hi } else { v < hi };
            if !ok {
                let op = if inclusive { ".." } else { "..." };
                self.mismatch(
                    path,
                    format!("{op} {}", cddl_value_description(upper)),
                    format!("{v}"),
                );
            }
        }
    }

    fn check_size(&mut self, val: &Value, expected: u64, path: &str) {
        let len = match val {
            Value::String(s) => Some(s.len() as u64),
            Value::Array(a) => Some(a.len() as u64),
            Value::Object(o) => Some(o.len() as u64),
            _ => None,
        };
        if let Some(l) = len
            && l != expected
        {
            self.mismatch(path, format!(".size {expected}"), format!(".size {l}"));
        }
    }

    fn check_comparison(&mut self, val: &Value, op: &str, target: &CddlValue, path: &str) {
        let v = val.as_f64();
        let t = cddl_value_as_f64(target);
        if let (Some(v), Some(t)) = (v, t) {
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
                    format!(".{op} {}", cddl_value_description(target)),
                    format!("{v}"),
                );
            }
        }
    }

    /// `.bits`: JSON has no byte-string representation, so this control only
    /// applies to a numeric target — every set bit's position must be named
    /// by the controller type (RFC 8610 §3.8.2).
    fn check_bits_control(&mut self, val: &Value, controller: &Type2, path: &str) {
        let Some(i) = val.as_u64() else {
            self.mismatch(path, ".bits (uint)", json_description(val));
            return;
        };
        for n in 0..64 {
            if i & (1u64 << n) != 0 && !self.bit_position_allowed(n, controller) {
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
    fn bit_position_allowed(&mut self, n: u64, controller: &Type2) -> bool {
        let candidate = serde_json::Value::from(n);
        let before = self.mismatches.len();
        self.match_type2(&candidate, controller, "");
        let allowed = self.mismatches.len() == before;
        self.mismatches.truncate(before);
        allowed
    }

    // ── Array group matching ────────────────────────────────────────────

    fn match_array_group(&mut self, val: &Value, group: &Group, path: &str) {
        let items = match val.as_array() {
            Some(a) => a,
            None => {
                self.mismatch(path, "array", json_description(val));
                return;
            }
        };

        for choice in &group.choices {
            let before = self.mismatches.len();
            if self.try_match_array_choice(items, choice, path) {
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

    fn try_match_array_choice(&mut self, items: &[Value], choice: &GrpChoice, path: &str) -> bool {
        let before = self.mismatches.len();
        let mut pos = 0usize;

        // See the identical comment in cbor_check.rs's try_match_array_choice.
        let flat = flatten_array_entries(self.cddl, &choice.entries, 0);

        for entry in &flat {
            if matches!(entry.entry, GrpEntBody::InlineGroup(_)) {
                continue;
            }
            let (min, max) = occur_bounds(entry.occur.as_ref());
            let mut count = 0u64;

            while max.is_none_or(|m| count < m) {
                if pos >= items.len() {
                    break;
                }
                let item_path = format!("{path}[{pos}]");
                let save = self.mismatches.len();
                self.match_grp_entry_as_type(&items[pos], &entry.entry, &item_path);
                if self.mismatches.len() > save {
                    self.mismatches.truncate(save);
                    break;
                }
                pos += 1;
                count += 1;
            }

            if count < min {
                return false;
            }
        }

        if pos != items.len() {
            self.mismatch(
                path,
                format!("array of {pos} items"),
                format!("array of {} items", items.len()),
            );
            return self.mismatches.len() == before;
        }

        self.mismatches.len() == before
    }

    fn match_grp_entry_as_type(&mut self, val: &Value, body: &GrpEntBody, path: &str) {
        match body {
            GrpEntBody::Type(t) => self.match_type(val, t, path),
            GrpEntBody::GroupRef { name, args, .. } => {
                if args.is_some() {
                    return;
                }
                self.match_rule_name(val, name, path);
            }
            GrpEntBody::InlineGroup(_) => {}
        }
    }

    // ── Map group matching ──────────────────────────────────────────────

    fn match_map_group(&mut self, val: &Value, group: &Group, path: &str) {
        let obj = match val.as_object() {
            Some(o) => o,
            None => {
                self.mismatch(path, "object", json_description(val));
                return;
            }
        };

        for choice in &group.choices {
            let before = self.mismatches.len();
            if self.try_match_map_choice(obj, choice, path) {
                self.mismatches.truncate(before);
                return;
            }
            self.mismatches.truncate(before);
        }
        self.mismatch(
            path,
            "object matching schema",
            format!("object of {} keys", obj.len()),
        );
    }

    fn try_match_map_choice(
        &mut self,
        obj: &serde_json::Map<String, Value>,
        choice: &GrpChoice,
        path: &str,
    ) -> bool {
        let before = self.mismatches.len();
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        let mut consumed = vec![false; keys.len()];
        // A key whose value fails an entry's value type, even though the
        // key itself matched that entry's key pattern, isn't necessarily an
        // error yet — a different (e.g. more permissive) entry appearing
        // later may still claim it. Remember the first such failure per key
        // so it can be reported precisely if no entry ends up consuming the
        // key, instead of silently discarding real mismatch detail.
        let mut pending_mismatch: Vec<Option<Vec<JsonMismatch>>> = vec![None; keys.len()];

        // Expand group inclusions (entries with no member key) into flat keyed entries
        // before matching, so that `NameComponents,` in a map is properly inlined.
        let flat = flatten_map_entries(self.cddl, &choice.entries, 0);

        for entry in &flat {
            let (min, max) = occur_bounds(entry.occur.as_ref());
            let mut count = 0u64;

            for (idx, key) in keys.iter().enumerate() {
                if consumed[idx] {
                    continue;
                }
                if max.is_some_and(|m| count >= m) {
                    break;
                }

                let key_matches = match &entry.key {
                    None => false,
                    Some(mk) => json_key_matches(key, mk),
                };
                if !key_matches {
                    continue;
                }

                let val_path = format!("{path}.{key}");
                let save = self.mismatches.len();
                self.match_grp_entry_as_type(&obj[*key], &entry.entry, &val_path);
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

        // Report unclaimed keys. A genuine repeating catch-all entry
        // (`* type => type`, unbounded occurrence) legitimately covers keys
        // no other entry claims; a non-repeating type-keyed entry (e.g.
        // `&(name: N) => type`, whose key is a single-literal enum-group
        // but still parses as `MemberKey::Type`) covers exactly one literal
        // key and must not suppress detection of genuinely unexpected keys.
        let has_wildcard = flat.iter().any(|e| {
            matches!(e.key, Some(MemberKey::Type { .. })) && occur_bounds(e.occur.as_ref()).1.is_none()
        });
        for (idx, c) in consumed.iter().enumerate() {
            if *c {
                continue;
            }
            if let Some(failure) = pending_mismatch[idx].take() {
                self.mismatches.extend(failure);
            } else if !has_wildcard {
                self.mismatch(
                    path,
                    "no unexpected keys".to_string(),
                    format!("unexpected key {:?}", keys[idx]),
                );
            }
        }

        self.mismatches.len() == before
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

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
        None => (1, Some(1)),
        Some(Occur::Optional) => (0, Some(1)),
        Some(Occur::OneOrMore) => (1, None),
        Some(Occur::Bounded { lower, upper }) => (lower.unwrap_or(0), *upper),
    }
}

fn json_key_matches(key: &str, mk: &MemberKey) -> bool {
    match mk {
        MemberKey::Bareword { name, .. } => key == name,
        MemberKey::Value { key: v, .. } => {
            // JSON keys are always strings
            matches!(v, CddlValue::Text(s) if s == key)
        }
        MemberKey::Type { .. } => {
            // Type-based keys not easily checked for JSON strings — accept any
            true
        }
    }
}

fn json_description(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => format!("number({n})"),
        Value::String(s) => {
            if s.len() > 32 {
                format!("string(\"{:.32}…\")", s)
            } else {
                format!("string({s:?})")
            }
        }
        Value::Array(a) => format!("array({} items)", a.len()),
        Value::Object(o) => format!("object({} keys)", o.len()),
    }
}

fn cddl_value_description(v: &CddlValue) -> String {
    match v {
        CddlValue::Int(n) => n.to_string(),
        CddlValue::Float(f) => f.to_string(),
        CddlValue::Text(s) => format!("{s:?}"),
        CddlValue::Bytes(b) => {
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
        Type2::Value(v) => cddl_value_description(v),
        Type2::Typename { name, .. } => name.clone(),
        Type2::Map(_) => "object".to_string(),
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

fn cddl_value_as_f64(v: &CddlValue) -> Option<f64> {
    match v {
        CddlValue::Int(n) => Some(*n as f64),
        CddlValue::Float(f) => Some(*f),
        _ => None,
    }
}

fn json_len(val: &Value) -> Option<u64> {
    match val {
        Value::String(s) => Some(s.len() as u64),
        Value::Array(a) => Some(a.len() as u64),
        Value::Object(o) => Some(o.len() as u64),
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

    fn check(json: &str, cddl_src: &str) -> Vec<JsonMismatch> {
        check_json(json.as_bytes(), &schema(cddl_src), None, &Config::strict())
            .expect("JSON parse error")
    }

    fn ok(json: &str, cddl_src: &str) {
        let m = check(json, cddl_src);
        assert!(m.is_empty(), "expected ok, got mismatches: {m:#?}");
    }

    fn fail(json: &str, cddl_src: &str) {
        let m = check(json, cddl_src);
        assert!(!m.is_empty(), "expected mismatch but got none");
    }

    // ── Primitives ────────────────────────────────────────────────────────

    #[test]
    fn uint_matches_positive_int() {
        ok("42", "x = uint");
    }

    #[test]
    fn uint_rejects_negative() {
        fail("-1", "x = uint");
    }

    #[test]
    fn uint_rejects_string() {
        fail("\"hi\"", "x = uint");
    }

    #[test]
    fn nint_matches_negative() {
        ok("-5", "x = nint");
    }

    #[test]
    fn nint_rejects_positive() {
        fail("5", "x = nint");
    }

    #[test]
    fn int_matches_positive() {
        ok("1", "x = int");
    }

    #[test]
    fn int_matches_negative() {
        ok("-1", "x = int");
    }

    #[test]
    fn tstr_matches_string() {
        ok("\"hello\"", "x = tstr");
    }

    #[test]
    fn tstr_rejects_number() {
        fail("42", "x = tstr");
    }

    #[test]
    fn bool_matches_true() {
        ok("true", "x = bool");
    }

    #[test]
    fn bool_matches_false() {
        ok("false", "x = bool");
    }

    #[test]
    fn null_matches() {
        ok("null", "x = null");
    }

    #[test]
    fn any_matches_anything() {
        ok("42", "x = #");
    }

    #[test]
    fn number_matches_int() {
        ok("10", "x = number");
    }

    #[test]
    fn number_matches_float() {
        ok("3.14", "x = number");
    }

    // ── Literal values ────────────────────────────────────────────────────

    #[test]
    fn literal_int_matches() {
        ok("7", "x = 7");
    }

    #[test]
    fn literal_int_mismatch() {
        fail("8", "x = 7");
    }

    #[test]
    fn literal_string_matches() {
        ok("\"hi\"", r#"x = "hi""#);
    }

    #[test]
    fn literal_string_mismatch() {
        fail("\"bye\"", r#"x = "hi""#);
    }

    // ── Union ─────────────────────────────────────────────────────────────

    #[test]
    fn union_first_branch() {
        ok("1", "x = uint / tstr");
    }

    #[test]
    fn union_second_branch() {
        ok("\"a\"", "x = uint / tstr");
    }

    #[test]
    fn union_no_match() {
        fail("true", "x = uint / tstr");
    }

    // ── Arrays ────────────────────────────────────────────────────────────

    #[test]
    fn array_fixed() {
        ok("[1, \"two\"]", "x = [uint, tstr]");
    }

    #[test]
    fn array_wrong_elem_type() {
        fail("[\"one\", \"two\"]", "x = [uint, tstr]");
    }

    #[test]
    fn array_wrong_length() {
        fail("[1]", "x = [uint, tstr]");
    }

    #[test]
    fn array_optional_absent() {
        ok("[1]", "x = [uint, ? tstr]");
    }

    #[test]
    fn array_optional_present() {
        ok("[1, \"y\"]", "x = [uint, ? tstr]");
    }

    #[test]
    fn array_star_zero() {
        ok("[]", "x = [* uint]");
    }

    #[test]
    fn array_star_many() {
        ok("[1, 2, 3]", "x = [* uint]");
    }

    #[test]
    fn array_plus_zero_fails() {
        fail("[]", "x = [+ uint]");
    }

    // ── Named group spliced into an array (bug fix, mirrors cbor_check.rs) ──

    #[test]
    fn named_group_spliced_into_array_matches() {
        ok(
            r#"["a","b"]"#,
            "x = [ Headers ]\nHeaders = ( protected: tstr, unprotected: tstr )",
        );
    }

    #[test]
    fn named_group_spliced_into_array_wrong_arity_fails() {
        fail(
            r#"["a"]"#,
            "x = [ Headers ]\nHeaders = ( protected: tstr, unprotected: tstr )",
        );
    }

    #[test]
    fn bare_typename_in_array_still_matches_per_item() {
        ok(r#"[1,2]"#, "x = [y, y]\ny = uint");
    }

    // ── Maps ──────────────────────────────────────────────────────────────

    #[test]
    fn map_simple() {
        ok(
            r#"{"name":"Alice","age":30}"#,
            "x = { name: tstr, age: uint }",
        );
    }

    #[test]
    fn map_wrong_value_type() {
        fail(r#"{"name":42,"age":30}"#, "x = { name: tstr, age: uint }");
    }

    #[test]
    fn map_missing_required_key() {
        fail(r#"{"name":"Bob"}"#, "x = { name: tstr, age: uint }");
    }

    #[test]
    fn map_optional_key_absent() {
        ok(r#"{"name":"Bob"}"#, "x = { name: tstr, ? age: uint }");
    }

    // ── Range constraints ─────────────────────────────────────────────────

    #[test]
    fn range_inclusive_ok() {
        ok("5", "x = 0..10");
    }

    #[test]
    fn range_inclusive_boundary() {
        ok("10", "x = 0..10");
    }

    #[test]
    fn range_exclusive_boundary_fails() {
        fail("10", "x = 0...10");
    }

    // RFC 8610 §2.2.2.1's own worked example: `byte1 = 0...first-non-byte`
    // with `first-non-byte = 256` is stated to be equivalent to
    // `byte = 0..255` — the named upper bound must resolve and enforce
    // exactly as if `256` had been written literally.
    #[test]
    fn range_named_upper_bound_boundary_ok() {
        ok("255", "x = 0...first-non-byte\nfirst-non-byte = 256");
    }

    #[test]
    fn range_named_upper_bound_excludes_value() {
        fail("256", "x = 0...first-non-byte\nfirst-non-byte = 256");
    }

    #[test]
    fn range_named_lower_bound_is_enforced() {
        fail("1", "x = lowest..10\nlowest = 2");
    }

    #[test]
    fn range_named_bound_chain_of_aliases_resolves() {
        ok("255", "x = 0...top\ntop = mid\nmid = 256");
        fail("256", "x = 0...top\ntop = mid\nmid = 256");
    }

    // ── Control .size ──────────────────────────────────────────────────────

    #[test]
    fn size_ok() {
        ok("\"hi\"", "x = tstr .size 2");
    }

    #[test]
    fn size_fail() {
        fail("\"hello\"", "x = tstr .size 2");
    }

    #[test]
    fn size_range_ok() {
        ok("\"hello\"", "x = tstr .size (1..10)");
    }

    #[test]
    fn size_range_too_short() {
        fail("\"\"", "x = tstr .size (1..10)");
    }

    #[test]
    fn size_range_too_long() {
        fail("\"hello world!!\"", "x = tstr .size (1..10)");
    }

    // ── .ne control operator ──────────────────────────────────────────────

    #[test]
    fn ne_ok() {
        ok("5", "x = uint .ne 0");
    }

    #[test]
    fn ne_fail_equal() {
        fail("0", "x = uint .ne 0");
    }

    // ── /= type extension ─────────────────────────────────────────────────

    #[test]
    fn type_extend_matches_first() {
        ok("1", "x = uint\nx /= tstr");
    }

    #[test]
    fn type_extend_matches_second() {
        ok("\"a\"", "x = uint\nx /= tstr");
    }

    #[test]
    fn type_extend_no_match() {
        fail("true", "x = uint\nx /= tstr");
    }

    // ── Invalid JSON input ────────────────────────────────────────────────

    #[test]
    fn invalid_json_returns_err() {
        let cddl_ast = schema("x = uint");
        let result = check_json(b"not json", &cddl_ast, None, &Config::strict());
        assert!(result.is_err());
    }

    // ── RFC 8610 §3.9 socket/plug: `//=` group extension ────────────────────

    const TWO_EXTENSIONS: &str = "\
        x = { * $c }\n\
        $c //= ( a: uint )\n\
        $c //= ( b: uint )\n";

    #[test]
    fn group_extension_first_choice_matches() {
        ok(r#"{"a":1}"#, TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_second_choice_matches() {
        // Regression: expand_group_name previously used find_map and only
        // ever returned the *first* rule sharing a `//=`-extended name, so
        // the second extension's key ("b") was invisible to the matcher and
        // reported as an unexpected key.
        ok(r#"{"b":2}"#, TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_second_choice_wrong_value_type_fails() {
        fail(r#"{"b":"garbage"}"#, TWO_EXTENSIONS);
    }

    #[test]
    fn group_extension_both_choices_match() {
        ok(r#"{"a":1,"b":2}"#, TWO_EXTENSIONS);
    }

    // ── RFC 8610 §3.2/§3.9: occurrence on a group-inclusion entry ───────────

    #[test]
    fn star_group_inclusion_allows_empty_map() {
        // Regression: the `*` in `{ * $c }` was dropped when the included
        // group's entries were flattened in, so a bare `a: uint` extension
        // (no occurrence indicator of its own) was treated as mandatory —
        // exactly the opposite of the RFC 8610 §3.9 socket/plug idiom, which
        // requires every plug to be individually optional.
        ok("{}", "x = { * $c }\n$c //= ( a: uint )");
    }

    #[test]
    fn optional_group_inclusion_allows_empty_map() {
        ok("{}", "x = { ? c }\nc = ( a: uint )");
    }

    #[test]
    fn required_group_inclusion_still_mandatory() {
        // A group-inclusion with no occurrence indicator (exactly-once) must
        // still require its entries — only zero-permitting indicators (`?`,
        // `*`, `n*m` with n absent/0) propagate optionality.
        fail("{}", "x = { c }\nc = ( a: uint )");
    }

    // ── Map-choice matching: value-mismatch reporting, unexpected keys ──────

    #[test]
    fn wildcard_value_type_mismatch_is_reported() {
        // Regression: a key matching a repeating wildcard's key pattern
        // whose value failed the wildcard's value type used to have its
        // mismatch silently discarded instead of reported.
        fail(r#"{"a":"wrong"}"#, "x = { * tstr => uint }");
    }

    #[test]
    fn non_repeating_type_key_does_not_suppress_unexpected_key() {
        // Regression: has_wildcard treated ANY MemberKey::Type entry (even a
        // single, non-repeating `tstr => tstr` entry) as a catch-all,
        // suppressing "unexpected key" detection for the whole map choice.
        fail(r#"{"a":"1","b":"2"}"#, "x = { tstr => tstr }");
    }

    #[test]
    fn genuine_wildcard_still_suppresses_unexpected_key() {
        ok(r#"{"anything":1}"#, "x = { * tstr => uint }");
    }

    // ── Control operators unimplemented at check time (Stage 1 findings) ──
    //
    // Mirrors the equivalent `cbor_check::tests` block: `check_constraint`'s
    // `ConstraintOp::Control(op)` match falls through to a silent `_ => {}`
    // for any operator not in its explicit list. `.and`/`.within`/`.bits`/
    // `.default` are fixed — see below. `.regexp` is deliberately,
    // permanently unenforced here pending a mature XSD-regex crate — see
    // `regexp_control_is_deliberately_unenforced` below.

    #[test]
    fn and_control_is_enforced() {
        // RFC 8610 §3.8.5: `.and` is the intersection of target and
        // controller. 10 is a `uint` but outside `0..5`; a conformant
        // implementation must reject it.
        //
        // STAGE 1 FINDING (fixed): `check_constraint` now checks the value
        // against the controller type for `.and`/`.within` in addition to
        // the target type.
        fail("10", "x = uint .and (0..5)");
        ok("3", "x = uint .and (0..5)");
    }

    #[test]
    fn within_control_is_enforced() {
        // Same intersection semantics as `.and` (RFC 8610 §3.8.5).
        fail("10", "x = uint .within (0..5)");
        ok("3", "x = uint .within (0..5)");
    }

    #[test]
    fn bits_control_is_enforced_on_number() {
        // RFC 8610 §3.8.2, `rwxbits` example: JSON has no byte-string
        // representation, so `.bits` on a JSON number checks that every set
        // bit's position is named by the controller type. 1 sets bit 0,
        // which is not in the enumerated set {r:2, w:1}.
        fail("1", "x = uint .bits rwx\nrwx = &(r: 2, w: 1)");
        ok("6", "x = uint .bits rwx\nrwx = &(r: 2, w: 1)");
    }

    #[test]
    fn regexp_control_is_deliberately_unenforced() {
        // RFC 8610 §3.8.3: `.regexp` requires the text to match the given
        // XSD regular expression. "abc" does not match `^[0-9]+$` — but no
        // XSD-conformant regex engine is mature enough in the Rust ecosystem
        // to depend on (Stage 1 Finding 4, deferred by human ruling
        // 2026-08-08). See `cbor_check::tests::regexp_control_is_deliberately_unenforced`.
        ok(r#""abc""#, "x = tstr .regexp \"^[0-9]+$\"");
    }

    #[test]
    fn default_control_rejects_value_equal_to_default() {
        // RFC 8610 §3.8.6: `.default` implies an underlying `.ne` against
        // the default value. Sending 0 against `uint .default 0` must fail
        // (Stage 1 Finding 2, fixed).
        fail("0", "x = uint .default 0");
        ok("1", "x = uint .default 0");
    }

    /// See the identical CBOR-side test in `cbor_check::tests` for the full
    /// rationale (`CDDL_STRESS_TEST.md` Divergence 14): RFC 8610 §3.8.6's own
    /// `timer` worked example, locking in the omitted/non-default/
    /// equal-to-default/fails-base-constraint cases together.
    #[test]
    fn default_control_rfc_worked_example_timer() {
        const SCHEMA: &str = "x = timer\ntimer = { time: uint, ? displayed-step: (number .gt 0) .default 1 }";
        ok(r#"{"time":5}"#, SCHEMA);
        ok(r#"{"time":5,"displayed-step":2}"#, SCHEMA);
        fail(r#"{"time":5,"displayed-step":1}"#, SCHEMA);
        fail(r#"{"time":5,"displayed-step":0}"#, SCHEMA);
    }

    // ── Generic instantiation (RFC 8610 §3.10) ────────────────────────────

    /// See the identical CBOR-side test in `cbor_check::tests`.
    ///
    /// STAGE 1 FINDING (fixed): `match_type2` used to treat any generic
    /// instantiation (`args.is_some()`) as `any`, so type args bound by
    /// `pair<uint, tstr>` were never actually checked against the array
    /// elements. `match_generic_instantiation` now binds the generic
    /// parameters to the supplied arguments and substitutes them throughout
    /// the rule body before matching.
    #[test]
    fn generic_instantiation_type_args_are_checked() {
        fail(r#"["x","a"]"#, "x = pair<uint,tstr>\npair<a,b> = [a,b]");
        ok(r#"[1,"a"]"#, "x = pair<uint,tstr>\npair<a,b> = [a,b]");
    }

    /// See the identical CBOR-side test in `cbor_check::tests`.
    ///
    /// STAGE 3 FINDING 5 (fixed): a long chain of distinct alias rules
    /// recurses once per rule via `match_rule_name`, with no repeated name
    /// for `in_progress` to catch. `MAX_MATCH_DEPTH` now caps this
    /// independently, reporting a mismatch instead of overflowing the stack.
    #[test]
    fn long_alias_chain_past_depth_limit_is_reported_not_stack_overflow() {
        let mut src = String::new();
        for i in 0..500 {
            src.push_str(&format!("a{i} = a{}\n", i + 1));
        }
        src.push_str("a500 = uint\n");
        fail("1", &src);
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
        ok("1", &src);
    }

    /// See the identical CBOR-side test in `cbor_check::tests`.
    ///
    /// STAGE 3 FINDING 4 (fixed): `match_generic_instantiation` recursed
    /// into `match_type`/`match_array_group`/`match_map_group` without ever
    /// routing back through `match_rule_name`'s own `MAX_MATCH_DEPTH`
    /// check, so a long chain of distinct *generic* alias rules previously
    /// recursed unboundedly and stack-overflowed. `match_generic_instantiation`
    /// now checks/increments `depth` itself, independent of and identical
    /// to `match_rule_name`'s cap. `w0`'s body deliberately ignores its own
    /// parameter (`uint`, not `a`) since substituting a bound parameter
    /// into a *nested* generic argument list is a separately-documented,
    /// unrelated limitation of `substitute_generic_type2` — this test
    /// targets the recursion depth alone.
    #[test]
    fn long_generic_alias_chain_past_depth_limit_is_reported_not_stack_overflow() {
        let mut src = String::from("x = w499<uint>\nw0<a> = uint\n");
        for i in 1..500 {
            src.push_str(&format!("w{i}<a> = w{}<a>\n", i - 1));
        }
        fail("1", &src);
    }

    /// Same generic-alias-chain shape, but short enough to stay under
    /// `MAX_MATCH_DEPTH` — must still match successfully.
    #[test]
    fn short_generic_alias_chain_under_depth_limit_still_matches() {
        let mut src = String::from("x = w9<uint>\nw0<a> = uint\n");
        for i in 1..10 {
            src.push_str(&format!("w{i}<a> = w{}<a>\n", i - 1));
        }
        ok("1", &src);
    }

    /// See the identical CBOR-side test in `cbor_check::tests` for the full
    /// rationale: `substitute_generic_type2` previously left nested generic
    /// instantiations un-substituted, so `Outer<C> = Wrapped<C>` never
    /// resolved `C` to its bound argument, and `Wrapped`'s `.size (5..10)`
    /// constraint went unenforced. JSON has no `bstr` representation, so
    /// this uses `tstr` (as the existing `.size` control tests above do).
    #[test]
    fn nested_generic_instantiation_substitutes_outer_binding() {
        const SCHEMA: &str = "x = Outer<tstr>\nWrapped<a> = a .size (5..10)\nOuter<c> = Wrapped<c>";
        fail("\"ab\"", SCHEMA);
        ok("\"hello!\"", SCHEMA);
        fail("\"hello world!!\"", SCHEMA);
    }
}
