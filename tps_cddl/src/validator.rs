/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Semantic validator: name resolution, duplicate-definition checks, and
//! control-operator validation over a parsed [`Cddl`] AST.
//!
//! The standard prelude (RFC 8610 Appendix D) is embedded as a static string
//! and merged into every document's rule namespace automatically.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::error::{Diagnostic, Level};
use crate::lexer::lex;
use crate::parser::build_cddl;

// ─── Standard prelude ─────────────────────────────────────────────────────────

/// RFC 8610 Appendix D — automatically available in every CDDL document.
const PRELUDE: &str = r#"
any = #

uint = #0
nint = #1
int = uint / nint

bstr = #2
bytes = bstr
tstr = #3
text = tstr

tdate = #6.0(tstr)
time = #6.1(number)
number = int / float
biguint = #6.2(bstr)
bignint = #6.3(bstr)
bigint = biguint / bignint
integer = int / bigint
unsigned = uint / biguint
decfrac = #6.4([e10: int, m: integer])
bigfloat = #6.5([e2: int, m: integer])
eb64url = #6.21(any)
eb64legacy = #6.22(any)
eb16 = #6.23(any)
encoded-cbor = #6.24(bstr)
uri = #6.32(tstr)
b64url = #6.33(tstr)
b64legacy = #6.34(tstr)
regexp = #6.35(tstr)
mime-message = #6.36(tstr)
cbor-any = #6.55799(any)

float16 = #7.25
float32 = #7.26
float64 = #7.27
float16-32 = float16 / float32
float32-64 = float32 / float64
float = float16-32 / float64

false = #7.20
true = #7.21
bool = false / true
nil = #7.22
null = nil
undefined = #7.23
"#;

use crate::Config;

// ─── Public entry points ──────────────────────────────────────────────────────

/// Validate a parsed CDDL document using strict RFC 8610 rules.
///
/// Returns a list of [`Diagnostic`] items.  An empty list means the document is
/// valid.  Use [`validate_with_config`] to enable RFC 9165 / RFC 9682 extensions.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, validate};
///
/// let pairs = lex("person = { name: tstr }").unwrap();
/// let schema = build_cddl(pairs).unwrap();
/// let diags = validate(&schema);
/// assert!(diags.is_empty());
/// ```
pub fn validate(cddl: &Cddl) -> Vec<Diagnostic> {
    validate_with_config(cddl, &Config::strict())
}

/// Validate a parsed CDDL document with explicit extension settings.
///
/// Use [`crate::Config`] to enable RFC 9165 and/or RFC 9682 extensions.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, validate_with_config, Config};
///
/// let pairs = lex("x = uint").unwrap();
/// let schema = build_cddl(pairs).unwrap();
/// let diags = validate_with_config(&schema, &Config::all());
/// assert!(diags.is_empty());
/// ```
pub fn validate_with_config(cddl: &Cddl, config: &Config) -> Vec<Diagnostic> {
    validate_with_config_ex(cddl, config).0
}

/// Validate a parsed CDDL document, also returning every RFC 9165 §4
/// `.feature` control-operator sighting collected along the way.
///
/// Feature usages are collected unconditionally, independent of whether
/// `--rfc9165` is enabled (collection is informational; the RFC-9165-opt-in
/// warning is a separate concern handled by [`validate_with_config`]).
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, validate_with_config_ex, Config};
///
/// let pairs = lex(r#"x = uint .feature "optional-support""#).unwrap();
/// let schema = build_cddl(pairs).unwrap();
/// let (_diags, features) = validate_with_config_ex(&schema, &Config::all());
/// assert_eq!(features[0].name, "optional-support");
/// ```
pub fn validate_with_config_ex(cddl: &Cddl, config: &Config) -> (Vec<Diagnostic>, Vec<FeatureUsage>) {
    let mut ctx = ValidatorCtx::new(config.clone());
    ctx.load_prelude();
    ctx.collect_definitions(cddl);
    ctx.check_document(cddl);
    (ctx.diagnostics, ctx.features)
}

/// A `.feature` (RFC 9165 §4) sighting collected during validation.
///
/// The controller is either a bare string (the feature name, with no
/// detail) or a two-element array `[name, detail]` (RFC 9165 Figure 6).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureUsage {
    /// The feature name.
    pub name: String,
    /// The optional detail element from a two-element `[name, detail]` controller.
    pub detail: Option<String>,
    /// Span of the `.feature` control operator use.
    pub span: Span,
}

/// Unwrap `( type1 )` down to its innermost `Type2`, as long as each layer is
/// a single unconstrained choice (so parenthesisation is purely cosmetic).
fn unwrap_paren(t2: &Type2) -> &Type2 {
    match t2 {
        Type2::ParenType(inner)
            if inner.choices.len() == 1 && inner.choices[0].constraint.is_none() =>
        {
            unwrap_paren(&inner.choices[0].base)
        }
        _ => t2,
    }
}

/// If `t2` is (possibly parenthesised) a bare text-string literal, return it.
fn value_text(t2: &Type2) -> Option<&str> {
    match unwrap_paren(t2) {
        Type2::Value(Value::Text(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// If a group entry is a single, unlabelled, unconstrained text-string
/// literal, return it — the shape of each element in a `.feature`
/// `[name, detail]` array literal (which parses as `Type2::Array`, i.e. a
/// value array, not a CDDL array *type*).
fn grpent_text(ge: &GrpEnt) -> Option<&str> {
    match &ge.entry {
        GrpEntBody::Type(t) if t.choices.len() == 1 => value_text(&t.choices[0].base),
        _ => None,
    }
}

/// Extract a [`FeatureUsage`] from a `.feature` constraint's controller
/// (RFC 9165 §4, Figure 6): either a bare string (the feature name) or a
/// two-element array `[name, detail]`.
///
/// `pub(crate)` so [`crate::gen_common`] can reuse it for manifest-driven
/// codegen feature resolution without duplicating the extraction logic.
pub(crate) fn extract_feature_usage(controller: &Type2, span: &Span) -> Option<FeatureUsage> {
    let controller = unwrap_paren(controller);
    if let Some(name) = value_text(controller) {
        return Some(FeatureUsage {
            name: name.to_owned(),
            detail: None,
            span: span.clone(),
        });
    }
    if let Type2::Array(group) = controller {
        let entries: Vec<&GrpEnt> = group.choices.iter().flat_map(|c| c.entries.iter()).collect();
        if let [name_ent, detail_ent] = entries.as_slice()
            && let Some(name) = grpent_text(name_ent) {
                let detail = grpent_text(detail_ent).map(|s| s.to_owned());
                return Some(FeatureUsage {
                    name: name.to_owned(),
                    detail,
                    span: span.clone(),
                });
            }
    }
    None
}

// ─── Validator context ────────────────────────────────────────────────────────

struct ValidatorCtx {
    config: Config,
    /// All names defined in the prelude (never reported as undefined).
    prelude_names: HashSet<String>,
    /// Names defined by the user document: name → (first-definition span, was_strict_assign).
    /// `was_strict_assign` is true when the first definition used `=`.
    defined: HashMap<String, (Span, bool)>,
    /// Names that are generic (have parameters).
    generic_names: HashSet<String>,
    /// Generic rule name → declared parameter count, for arity checking at
    /// instantiation sites (RFC 8610 §3.10).
    generic_arity: HashMap<String, usize>,
    diagnostics: Vec<Diagnostic>,
    /// `.feature` (RFC 9165 §4) sightings, collected regardless of `--rfc9165`.
    features: Vec<FeatureUsage>,
}

impl ValidatorCtx {
    fn new(config: Config) -> Self {
        Self {
            config,
            prelude_names: HashSet::new(),
            defined: HashMap::new(),
            generic_names: HashSet::new(),
            generic_arity: HashMap::new(),
            diagnostics: Vec::new(),
            features: Vec::new(),
        }
    }

    fn error(&mut self, span: &Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: Level::Error,
            span: span.clone(),
            message: message.into(),
        });
    }

    fn warning(&mut self, span: &Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: Level::Warning,
            span: span.clone(),
            message: message.into(),
        });
    }

    /// Parse and register all names from the RFC 8610 standard prelude.
    fn load_prelude(&mut self) {
        let pairs = lex(PRELUDE).expect("prelude must parse");
        let prelude_cddl = build_cddl(pairs).expect("prelude must build");
        for rule in &prelude_cddl.rules {
            self.prelude_names.insert(rule.name.clone());
        }
    }

    /// First pass: collect all rule names defined in the document.
    /// Detect duplicate `=` assignments.
    fn collect_definitions(&mut self, cddl: &Cddl) {
        for rule in &cddl.rules {
            if let Some(parm) = &rule.generic_parm {
                self.generic_names.insert(rule.name.clone());
                self.generic_arity.insert(rule.name.clone(), parm.params.len());
            }

            match rule.assignment {
                Assignment::Assign => {
                    if let Some((prev_span, prev_strict)) = self.defined.get(&rule.name) {
                        let prev_strict = *prev_strict;
                        let prev_span = prev_span.clone();
                        if prev_strict {
                            // `=` after `=` — duplicate definition error.
                            self.error(
                                &rule.span,
                                format!(
                                    "rule `{}` is redefined with `=`; first definition at {}:{}",
                                    rule.name, prev_span.start, prev_span.end
                                ),
                            );
                        }
                        // `=` after `/=` or `//=` — permitted (adds a strict definition).
                        // Update to mark as now strictly defined.
                        self.defined.insert(rule.name.clone(), (prev_span, true));
                    } else {
                        self.defined
                            .insert(rule.name.clone(), (rule.span.clone(), true));
                    }
                }
                // /= and //= are additive; first use implicitly creates the rule.
                Assignment::TypeExtend | Assignment::GroupExtend => {
                    self.defined
                        .entry(rule.name.clone())
                        .or_insert((rule.span.clone(), false));
                }
            }
        }
    }

    /// Second pass: check that every name reference resolves.
    fn check_document(&mut self, cddl: &Cddl) {
        // RFC 8610 requires at least one rule; RFC 9682 relaxes this.
        if cddl.rules.is_empty() && !self.config.rfc9682 {
            self.error(
                &cddl.span,
                "CDDL document must contain at least one rule \
                 (use --rfc9682 to allow empty documents)",
            );
        }

        for rule in &cddl.rules {
            // Generic parameter names are in scope for the rule body.
            let generic_params: HashSet<String> = rule
                .generic_parm
                .as_ref()
                .map(|gp| gp.params.iter().cloned().collect())
                .unwrap_or_default();

            match &rule.body {
                RuleBody::Type(t) => self.check_type(t, &generic_params, None),
                RuleBody::Group(ge) => self.check_grpent(ge, &generic_params),
            }
        }
    }

    // ── Type expressions ──────────────────────────────────────────────────────

    /// `occur` is the occurrence marker (`?`, `+`, `n*m`) of the group entry
    /// this type expression is the immediate value of, if any — `None` when
    /// the type isn't the value of a group entry at all (e.g. a top-level
    /// rule body) or when the entry has no occurrence marker. It's threaded
    /// down to `validate_control_op` so `.default` (RFC 8610 §3.8.6) can be
    /// flagged when used outside an optional context (Stage 1 Finding 9).
    fn check_type(&mut self, t: &Type, scope: &HashSet<String>, occur: Option<&Occur>) {
        for t1 in &t.choices {
            self.check_type1(t1, scope, occur);
        }
    }

    fn check_type1(&mut self, t1: &Type1, scope: &HashSet<String>, occur: Option<&Occur>) {
        self.check_type2(&t1.base, scope, occur);
        if let Some(c) = &t1.constraint {
            self.check_constraint(c, &t1.base, scope, occur);
            self.check_type2(&c.target, scope, None);
        }
    }

    fn check_type2(&mut self, t2: &Type2, scope: &HashSet<String>, occur: Option<&Occur>) {
        match t2 {
            Type2::Value(_) | Type2::Any(_) => {}

            Type2::MajorType { ai, span, .. } => {
                if let Some(HeadNumber::Type(t)) = ai {
                    if !self.config.rfc9682 {
                        self.error(span, "non-literal head-number requires --rfc9682");
                    }
                    self.check_type(t, scope, None);
                }
            }

            Type2::Tagged { tag, inner, span } => {
                if let Some(HeadNumber::Type(t)) = tag {
                    if !self.config.rfc9682 {
                        self.error(span, "non-literal tag number requires --rfc9682");
                    }
                    self.check_type(t, scope, None);
                }
                self.check_type(inner, scope, None);
            }

            Type2::Typename { name, args, span } => {
                self.check_name_ref(name, span, scope);
                if let Some(ga) = args {
                    self.check_genericarg(ga, scope);
                    self.check_generic_arity(name, ga, span);
                }
            }
            Type2::Unwrap { name, args, span } => {
                self.check_name_ref(name, span, scope);
                if let Some(ga) = args {
                    self.check_genericarg(ga, scope);
                    self.check_generic_arity(name, ga, span);
                }
            }
            Type2::EnumRef { name, args, span } => {
                self.check_name_ref(name, span, scope);
                if let Some(ga) = args {
                    self.check_genericarg(ga, scope);
                    self.check_generic_arity(name, ga, span);
                }
            }

            Type2::ParenType(inner) => self.check_type(inner, scope, occur),
            Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => {
                self.check_group(g, scope);
            }
        }
    }

    fn check_constraint(
        &mut self,
        c: &Constraint,
        base: &Type2,
        scope: &HashSet<String>,
        occur: Option<&Occur>,
    ) {
        if let ConstraintOp::Control(name) = &c.op {
            self.validate_control_op(name.as_str(), base, &c.target, &c.span, scope, occur);
        }
        // Range ops are always valid structurally; value-type compatibility
        // is deferred to Phase 3 (CBOR matching).
    }

    fn check_genericarg(&mut self, ga: &GenericArg, scope: &HashSet<String>) {
        for t1 in &ga.args {
            self.check_type1(t1, scope, None);
        }
    }

    /// Check that a generic instantiation supplies exactly as many
    /// arguments as the rule declares parameters (RFC 8610 §3.10).
    ///
    /// A mismatched count cannot correspond to a coherent binding, so it is
    /// flagged as an error rather than silently accepted.
    fn check_generic_arity(&mut self, name: &str, ga: &GenericArg, span: &Span) {
        if let Some(&expected) = self.generic_arity.get(name) {
            let actual = ga.args.len();
            if actual != expected {
                self.error(
                    span,
                    format!(
                        "generic rule `{name}` takes {expected} argument(s), but {actual} were supplied"
                    ),
                );
            }
        }
    }

    // ── Groups ────────────────────────────────────────────────────────────────

    fn check_group(&mut self, g: &Group, scope: &HashSet<String>) {
        for choice in &g.choices {
            for entry in &choice.entries {
                self.check_grpent(entry, scope);
            }
        }
    }

    fn check_grpent(&mut self, ge: &GrpEnt, scope: &HashSet<String>) {
        if let Some(key) = &ge.key {
            self.check_memberkey(key, scope);
        }
        match &ge.entry {
            GrpEntBody::Type(t) => self.check_type(t, scope, ge.occur.as_ref()),
            GrpEntBody::InlineGroup(g) => self.check_group(g, scope),
            GrpEntBody::GroupRef { name, args, span } => {
                self.check_name_ref(name, span, scope);
                if let Some(ga) = args {
                    self.check_genericarg(ga, scope);
                    self.check_generic_arity(name, ga, span);
                }
            }
        }
    }

    fn check_memberkey(&mut self, key: &MemberKey, scope: &HashSet<String>) {
        match key {
            MemberKey::Type { key, .. } => self.check_type1(key, scope, None),
            MemberKey::Value { .. } | MemberKey::Bareword { .. } => {}
        }
    }

    // ── Name resolution ───────────────────────────────────────────────────────

    /// Check that `name` is defined in the prelude, in the document, or is a
    /// generic parameter in scope.
    ///
    /// RFC 8610 §3.9: a name starting with `$` (type socket) or `$$` (group
    /// socket) is exempt from the undefined-name check even when no plug
    /// (`/=`/`//=`) exists anywhere in the document — "it is not an error if
    /// there is no definition for a socket at all; this then means there is
    /// no way to satisfy the rule (i.e., the choice is empty)."
    fn check_name_ref(&mut self, name: &str, span: &Span, scope: &HashSet<String>) {
        if self.prelude_names.contains(name)
            || self.defined.contains_key(name)
            || scope.contains(name)
            || name.starts_with('$')
        {
            return;
        }
        self.error(span, format!("undefined name `{name}`"));
    }

    // ── Control operator validation ───────────────────────────────────────────

    /// Validate that a control operator is recognised and applied sensibly.
    ///
    /// This performs a best-effort structural check; full type compatibility
    /// requires value-level analysis done in Phase 3.
    fn validate_control_op(
        &mut self,
        op: &str,
        _base: &Type2,
        controller: &Type2,
        span: &Span,
        _scope: &HashSet<String>,
        occur: Option<&Occur>,
    ) {
        const RFC8610_OPS: &[&str] = &[
            "size", "bits", "regexp", "cbor", "cborseq", "lt", "le", "gt", "ge", "eq", "ne",
            "default", "and", "within",
        ];
        const RFC9165_OPS: &[&str] = &["plus", "cat", "det", "abnf", "abnfb", "feature"];

        if op == "feature"
            && let Some(usage) = extract_feature_usage(controller, span) {
                self.features.push(usage);
            }

        // `.size` (RFC 8610 §3.8.1) takes a non-negative integer (or range of
        // non-negative integers) controller — a byte/text string, array, or
        // map cannot have negative length. `cbor_check.rs`/`json_check.rs`
        // cast the controller literal to `u64` when checking a decoded item
        // against it, so a negative literal silently becomes a huge
        // sentinel value that can never match, rather than erroring — a
        // schema author's typo (e.g. `bstr .size -1`) would otherwise fail
        // silently at match time instead of being caught here at validation
        // time.
        if op == "size"
            && let Type2::Value(Value::Int(n)) = controller
            && *n < 0
        {
            self.error(
                span,
                format!("control operator `.size` requires a non-negative integer, found `{n}`"),
            );
            return;
        }

        // `.regexp` (RFC 8610 §3.8.3) requires matching an XSD regular
        // expression. No XSD-conformant regex engine is mature enough in the
        // Rust ecosystem to depend on (assessed 2026-08-08: `oxixml_regex`
        // and `xsd_regex_rs` are both brand new/unproven). Rather than
        // silently accept the control operator with no enforcement anywhere,
        // warn here so the implementer knows to handle matching separately.
        if op == "regexp" {
            self.warning(
                span,
                "control operator `.regexp` (XSD regular expression) is not enforced \
                 by this tool — the implementer must apply regex matching separately"
                    .to_string(),
            );
            return;
        }

        // `.default` (RFC 8610 §3.8.6) "is only meaningful when the control
        // type is used in an optional context... otherwise, there would be
        // no way to make use of the default value." The spec doesn't rule
        // on tooling behavior for the non-optional case; human ruling
        // (Stage 1 Finding 9, 2026-08-08): treat it as an error.
        if op == "default" && !matches!(occur, Some(Occur::Optional)) {
            self.error(
                span,
                "control operator `.default` is only meaningful in an optional (`?`) \
                 context — this group entry is not optional"
                    .to_string(),
            );
            return;
        }

        if RFC8610_OPS.contains(&op) {
            return;
        }
        if RFC9165_OPS.contains(&op) {
            if !self.config.rfc9165 {
                self.warning(
                    span,
                    format!(
                        "control operator `.{op}` is defined by RFC 9165; \
                         use --rfc9165 to suppress this warning"
                    ),
                );
            }
            return;
        }
        // When RFC 9165 is enabled, suppress warnings for any unknown operator:
        // the user has explicitly opted into extended control operator support.
        if !self.config.rfc9165 {
            self.warning(span, format!("unknown control operator `.{op}`"));
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::build_cddl;

    fn validate_src(src: &str) -> Vec<Diagnostic> {
        let pairs = lex(src).unwrap_or_else(|e| panic!("parse error: {e}"));
        let cddl = build_cddl(pairs).unwrap_or_else(|e| panic!("build error: {e}"));
        validate(&cddl)
    }

    fn errors(src: &str) -> Vec<String> {
        validate_src(src)
            .into_iter()
            .filter(|d| d.level == Level::Error)
            .map(|d| d.message)
            .collect()
    }

    fn warnings(src: &str) -> Vec<String> {
        validate_src(src)
            .into_iter()
            .filter(|d| d.level == Level::Warning)
            .map(|d| d.message)
            .collect()
    }

    // ── Valid documents ───────────────────────────────────────────────────────

    #[test]
    fn valid_prelude_references() {
        // All of these names come from the standard prelude — must resolve.
        assert!(errors("x = uint / tstr / bool / null / bytes / float").is_empty());
    }

    #[test]
    fn valid_self_reference() {
        // Recursive type — name is defined in the same file.
        assert!(errors("list = [* list / uint]").is_empty());
    }

    #[test]
    fn valid_forward_reference() {
        // Forward reference: `person` uses `address` defined later.
        assert!(errors("person = { addr: address }\naddress = tstr").is_empty());
    }

    #[test]
    fn valid_generic_rule() {
        assert!(errors("pair<a, b> = [a, b]").is_empty());
    }

    #[test]
    fn valid_type_extend() {
        // /= is additive — not a duplicate-definition error.
        assert!(errors("color = \"red\"\ncolor /= \"blue\"").is_empty());
    }

    #[test]
    fn valid_group_extend() {
        assert!(errors("delivery = (city: tstr)\ndelivery //= (zip: uint)").is_empty());
    }

    #[test]
    fn valid_all_prelude_names() {
        // Spot-check a selection of prelude-defined names.
        let src = "x = any\n\
                   y = int\n\
                   z = float\n\
                   w = bool\n\
                   v = tdate\n\
                   u = uri\n\
                   t = cbor-any";
        assert!(errors(src).is_empty());
    }

    #[test]
    fn valid_control_operators() {
        let src = "sz = bstr .size 4\n\
                   re = tstr .regexp \"[a-z]+\"\n\
                   lim = uint .le 255\n\
                   ex = tstr .eq \"ok\"";
        assert!(errors(src).is_empty());
    }

    #[test]
    fn valid_nested_map() {
        let src = "person = { name: tstr, addr: address }\naddress = { city: tstr }";
        assert!(errors(src).is_empty());
    }

    // ── Errors ────────────────────────────────────────────────────────────────

    #[test]
    fn error_undefined_name() {
        let diags = errors("x = no-such-type");
        assert!(!diags.is_empty(), "expected undefined-name error");
        assert!(diags[0].contains("undefined name `no-such-type`"));
    }

    #[test]
    fn error_duplicate_assign() {
        let diags = errors("color = \"red\"\ncolor = \"blue\"");
        assert!(!diags.is_empty(), "expected duplicate-definition error");
        assert!(diags[0].contains("redefined with `=`"));
    }

    #[test]
    fn error_undefined_in_generic_body() {
        // Generic parameter `a` is in scope; `missing` is not.
        let diags = errors("wrap<a> = [a, missing]");
        assert!(!diags.is_empty());
        assert!(diags[0].contains("undefined name `missing`"));
    }

    #[test]
    fn error_undefined_group_ref() {
        let diags = errors("x = { ghost }");
        assert!(!diags.is_empty());
        assert!(diags[0].contains("undefined name `ghost`"));
    }

    // ── Warnings ──────────────────────────────────────────────────────────────

    #[test]
    fn warning_unknown_control_op() {
        let w = warnings("x = tstr .fancy tstr");
        assert!(!w.is_empty(), "expected unknown-control-op warning");
        assert!(w[0].contains("unknown control operator `.fancy`"));
    }

    #[test]
    fn no_warning_known_control_ops() {
        // `.regexp` is excluded here — it deliberately produces a warning,
        // see `warning_regexp_not_enforced` below.
        let known = [
            "size", "bits", "cbor", "cborseq", "lt", "le", "gt", "ge", "eq", "ne", "default",
            "and", "within",
        ];
        for op in known {
            let src = format!("x = tstr .{op} tstr");
            let w = warnings(&src);
            assert!(w.is_empty(), "unexpected warning for .{op}: {w:?}");
        }
    }

    #[test]
    fn warning_regexp_not_enforced() {
        let w = warnings("x = tstr .regexp \"[a-z]+\"");
        assert!(!w.is_empty(), "expected `.regexp` not-enforced warning");
        assert!(w[0].contains(".regexp"));
        assert!(w[0].contains("not enforced"));
    }

    // ── `.default` occurrence-context check (RFC 8610 §3.8.6, Finding 9) ──

    #[test]
    fn error_default_control_outside_optional_context() {
        // RFC 8610 §3.8.6: `.default` "is only meaningful when the control
        // type is used in an optional context" — a bare (non-`?`) group
        // entry can never omit the key, so there's no way to make use of
        // the default. Human ruling (Finding 9): flag as an error.
        let e = errors("x = { displayed-step: (number .gt 0) .default 1 }");
        assert!(!e.is_empty(), "expected `.default` context error");
        assert!(e[0].contains(".default"));
    }

    #[test]
    fn no_error_default_control_in_optional_context() {
        // The RFC's own example, verbatim: `.default` on a `?` group entry
        // is exactly the intended usage and must not be flagged.
        let e = errors("x = { ? displayed-step: (number .gt 0) .default 1 }");
        assert!(e.is_empty(), "unexpected error: {e:?}");
    }

    #[test]
    fn error_default_control_at_top_level_rule() {
        // A top-level rule body has no occurrence context at all (it isn't
        // a group entry), so `.default` there is likewise never usable.
        let e = errors("x = uint .default 0");
        assert!(!e.is_empty(), "expected `.default` context error");
    }

    // ── `.size` with a negative controller (RFC 8610 §3.8.1) ───────────────

    #[test]
    fn error_size_control_with_negative_literal() {
        // `.size` requires a non-negative integer controller — no CBOR item
        // can have a negative byte/text/array/map length. Without this
        // check, `cbor_check.rs`/`json_check.rs` cast the negative literal
        // to `u64`, producing a huge sentinel that can never match anything,
        // so a schema author's typo (`bstr .size -1`) would silently never
        // match at check time instead of being caught here at validation.
        let e = errors("x = bstr .size -1");
        assert!(!e.is_empty(), "expected `.size` negative-literal error");
        assert!(e[0].contains(".size"));
    }

    #[test]
    fn no_error_size_control_with_nonnegative_literal() {
        let e = errors("x = bstr .size 4");
        assert!(e.is_empty(), "unexpected error: {e:?}");
    }

    #[test]
    fn no_error_size_control_with_range_controller() {
        // `.size (0..4)` is a range controller, not a bare `Value::Int` —
        // must not be misidentified as the negative-literal case.
        let e = errors("x = bstr .size (0..4)");
        assert!(e.is_empty(), "unexpected error: {e:?}");
    }

    // ── C3: /= before = must not cause false duplicate error ──────────────

    #[test]
    fn extend_before_assign_is_valid() {
        // /= before = should NOT be an error.
        assert!(errors("color /= \"blue\"\ncolor = \"red\"").is_empty());
    }

    #[test]
    fn assign_after_assign_is_error() {
        // Two = definitions is always an error.
        let diags = errors("color = \"red\"\ncolor = \"blue\"");
        assert!(!diags.is_empty(), "expected duplicate-definition error");
        assert!(diags[0].contains("redefined with `=`"));
    }

    // ── Generic type parameters/arguments (RFC 8610 §3.10) ─────────────────

    #[test]
    fn generic_argument_count_mismatch_is_flagged() {
        // RFC 8610 §3.10 doesn't formally require arity checking, but a
        // mismatched argument count cannot correspond to a coherent
        // binding.
        //
        // STAGE 1 FINDING (fixed): `check_generic_arity` now flags
        // `pair<a,b>` instantiated with the wrong number of arguments.
        let diags = errors("pair<a,b> = [a,b]\nx = pair<uint>");
        assert!(!diags.is_empty(), "expected an arity-mismatch error");
        assert!(diags[0].contains("takes 2 argument(s), but 1 were supplied"));

        assert!(errors("pair<a,b> = [a,b]\nx = pair<uint,tstr>").is_empty());
    }

    #[test]
    fn valid_generic_argument_with_nested_generic() {
        // A generic argument that is itself a generic instantiation must
        // resolve recursively.
        assert!(errors("wrap<a> = [a]\nx = wrap<wrap<uint>>").is_empty());
    }

    #[test]
    fn error_undefined_name_inside_generic_argument() {
        // `missing` appears only as a generic *argument*, not in the
        // generic body — `check_genericarg` must still resolve it.
        let diags = errors("wrap<a> = [a]\nx = wrap<missing>");
        assert!(!diags.is_empty());
        assert!(diags[0].contains("undefined name `missing`"));
    }

    #[test]
    fn valid_generic_group_rule() {
        // Generic parameters are also valid on group rules (RFC 8610 §3.10
        // doesn't restrict generics to type rules).
        assert!(errors("entry<k,v> = (k: v)\nx = { entry<tstr,uint> }").is_empty());
    }

    // ── Non-literal head-numbers for simple values (RFC 9682 §3.2) ─────────

    #[test]
    fn non_literal_simple_value_head_number_requires_rfc9682() {
        // `#7.<type>` (simple value with a non-literal head-number) is the
        // `Type2::MajorType` counterpart to the already-tested
        // `#6.<type>(...)` tag case; RFC 9682 §3.2's `head-number` grammar
        // applies uniformly to both `#6` and `#7`.
        let diags = errors("x = #7.<uint>");
        assert!(
            !diags.is_empty(),
            "expected error for non-literal simple-value head-number without --rfc9682"
        );
        assert!(diags[0].contains("non-literal head-number requires --rfc9682"));
    }

    #[test]
    fn non_literal_simple_value_head_number_ok_with_rfc9682() {
        let src = "x = #7.<uint>";
        let schema = validate_src_parse(src);
        let diags = validate_with_config(
            &schema,
            &Config {
                rfc9165: false,
                rfc9682: true,
            },
        );
        let errs: Vec<_> = diags.into_iter().filter(|d| d.level == Level::Error).collect();
        assert!(errs.is_empty(), "expected no errors: {errs:?}");
    }

    fn validate_src_parse(src: &str) -> Cddl {
        let pairs = lex(src).unwrap_or_else(|e| panic!("parse error: {e}"));
        build_cddl(pairs).unwrap_or_else(|e| panic!("build error: {e}"))
    }

    #[test]
    fn unplugged_group_socket_is_not_flagged_as_undefined_name() {
        // RFC 8610 §3.9: a `$name` ("type socket") or `$$name` ("group
        // socket") that has NO plug (`/=`/`//=`) definition anywhere in the
        // document is explicitly NOT an error per the RFC — "it is not an
        // error if there is no definition for a socket at all; this then
        // means there is no way to satisfy the rule (i.e., the choice is
        // empty)".
        //
        // STAGE 1 FINDING (fixed): `check_name_ref` now exempts any name
        // starting with `$` from the undefined-name check.
        let diags = errors("x = { * $$unplugged-socket }");
        assert!(
            diags.is_empty(),
            "unplugged socket must not be flagged as undefined, got: {diags:#?}"
        );
    }

    #[test]
    fn unplugged_type_socket_is_not_flagged_as_undefined_name() {
        let diags = errors("x = $unplugged-type-socket");
        assert!(
            diags.is_empty(),
            "unplugged socket must not be flagged as undefined, got: {diags:#?}"
        );
    }
}
