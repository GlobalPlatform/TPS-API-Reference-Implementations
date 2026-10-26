/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Abstract Syntax Tree node types for CDDL (RFC 8610).
//!
//! Each node carries a [`Span`] recording its byte offsets in the source so
//! that diagnostics can point back to the right location.

/// Byte-offset span into the original source string.
///
/// Used in all AST nodes so diagnostic messages can point back to the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the first character (inclusive).
    pub start: usize,
    /// Byte offset just past the last character (exclusive).
    pub end: usize,
}

impl Span {
    /// Create a new span from start and end byte offsets.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

impl From<pest::Span<'_>> for Span {
    fn from(s: pest::Span<'_>) -> Self {
        Self::new(s.start(), s.end())
    }
}

/// Convert a byte offset into `source` to a 1-based `(line, column)` pair.
///
/// `offset` is clamped to `source.len()` if it runs past the end. Both line
/// and column are 1-based, matching common editor/diagnostic conventions.
/// Column counts UTF-8 bytes since the last newline (or start of source),
/// not Unicode scalar values or grapheme clusters.
///
/// # Examples
///
/// ```
/// use tps_cddl::ast::offset_to_line_col;
///
/// assert_eq!(offset_to_line_col("abc", 0), (1, 1));
/// assert_eq!(offset_to_line_col("ab\ncd", 3), (2, 1));
/// assert_eq!(offset_to_line_col("", 0), (1, 1));
/// ```
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1usize;
    let mut col = 1usize;
    for &b in source.as_bytes().iter().take(offset) {
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// ─── Top-level ────────────────────────────────────────────────────────────────

/// A complete CDDL document: one or more rules.
///
/// Obtain a `Cddl` value by calling [`crate::lex`] followed by [`crate::build_cddl`].
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl};
///
/// let pairs = lex("color = \"red\" / \"green\" / \"blue\"").unwrap();
/// let cddl = build_cddl(pairs).unwrap();
/// assert_eq!(cddl.rules.len(), 1);
/// assert_eq!(cddl.rules[0].name, "color");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Cddl {
    /// All rules in document order.
    pub rules: Vec<Rule>,
    /// Span covering the entire document.
    pub span: Span,
}

// ─── Rules ────────────────────────────────────────────────────────────────────

/// A single CDDL rule (type rule or group rule).
///
/// Rules are the top-level definitions in a CDDL document.  The [`Assignment`]
/// field indicates whether this is a fresh definition (`=`) or an additive
/// extension (`/=` / `//=`).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The rule name (identifier on the left-hand side).
    pub name: String,
    /// Generic type parameters, if any (e.g. `<a, b>` in `pair<a,b> = [a, b]`).
    pub generic_parm: Option<GenericParm>,
    /// Assignment operator: `=`, `/=`, or `//=`.
    pub assignment: Assignment,
    /// The right-hand side.
    pub body: RuleBody,
    /// Source span of the entire rule.
    pub span: Span,
}

/// The right-hand side of a rule.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleBody {
    /// Type rule: `name = type` or `name /= type`
    Type(Type),
    /// Group rule: `name = grpent` or `name //= grpent`
    Group(GrpEnt),
}

/// Assignment operator kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assignment {
    /// `=`  — define or redefine
    Assign,
    /// `/=` — extend a type choice
    TypeExtend,
    /// `//=` — extend a group choice
    GroupExtend,
}

// ─── Generic parameters / arguments ──────────────────────────────────────────

/// `<id, id, ...>` in a rule definition.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericParm {
    /// The parameter names, in declaration order.
    pub params: Vec<String>,
    /// Span of the whole `<...>` parameter list.
    pub span: Span,
}

/// `<type1, type1, ...>` at a use site.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericArg {
    /// The argument types, in use-site order.
    pub args: Vec<Type1>,
    /// Span of the whole `<...>` argument list.
    pub span: Span,
}

// ─── Type expressions ─────────────────────────────────────────────────────────

/// `type1 / type1 / ...` — ordered choice (union).
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    /// The alternatives, in source order.
    pub choices: Vec<Type1>,
    /// Span of the whole type expression.
    pub span: Span,
}

/// `type2 [rangeop/ctlop type2]` — a type with optional constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct Type1 {
    /// The base type term.
    pub base: Type2,
    /// The optional range/control-operator constraint, if present.
    pub constraint: Option<Constraint>,
    /// Span of the whole `type1` expression.
    pub span: Span,
}

/// Optional constraint appended to a `type1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    /// The range or control operator applied.
    pub op: ConstraintOp,
    /// The right-hand operand of the constraint.
    pub target: Type2,
    /// Span of the whole constraint.
    pub span: Span,
}

/// The operator in a constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintOp {
    /// `..`  — inclusive range
    RangeInclusive,
    /// `...` — exclusive range
    RangeExclusive,
    /// `.name` — control operator
    Control(String),
}

/// A tag or simple-value number: either a literal `uint` (RFC 8610) or a
/// type expression (RFC 9682 §3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum HeadNumber {
    /// A literal `uint` head-number.
    Literal(u64),
    /// `<type>` — non-literal head-number, requires `--rfc9682`.
    Type(Box<Type>),
}

/// A single type term — the most specific type expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Type2 {
    /// A literal value: integer, float, text, bytes.
    Value(Value),
    /// A reference to a named type, with optional generic arguments.
    Typename {
        /// The referenced type name.
        name: String,
        /// Generic arguments supplied at this use site, if any.
        args: Option<GenericArg>,
        /// Span of the whole `typename` reference.
        span: Span,
    },
    /// `( type )` — parenthesised group.
    ParenType(Box<Type>),
    /// `{ group }` — map.
    Map(Group),
    /// `[ group ]` — array.
    Array(Group),
    /// `~ typename` — unwrap operator.
    Unwrap {
        /// The unwrapped type's name.
        name: String,
        /// Generic arguments supplied at this use site, if any.
        args: Option<GenericArg>,
        /// Span of the whole `~ typename` expression.
        span: Span,
    },
    /// `&( group )` — enumeration from inline group.
    EnumGroup(Group),
    /// `& groupname` — enumeration from named group.
    EnumRef {
        /// The referenced group name.
        name: String,
        /// Generic arguments supplied at this use site, if any.
        args: Option<GenericArg>,
        /// Span of the whole `& groupname` expression.
        span: Span,
    },
    /// `#6.N( type )` — tagged item; tag number is a literal or (RFC 9682) a type expression.
    Tagged {
        /// The tag number, or `None` for an unnumbered `#6( type )`.
        tag: Option<HeadNumber>,
        /// The tagged inner type.
        inner: Box<Type>,
        /// Span of the whole tagged expression.
        span: Span,
    },
    /// `#M.N` or `#7.head-number` — major type with additional info.
    MajorType {
        /// The CBOR major type number.
        major: u8,
        /// The additional-information value, or `None` if unspecified.
        ai: Option<HeadNumber>,
        /// Span of the whole major-type expression.
        span: Span,
    },
    /// `#` — any CBOR data item.
    Any(Span),
}

// ─── Groups ───────────────────────────────────────────────────────────────────

/// A group: one or more group choices separated by `//`.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The `//`-separated alternatives, in source order.
    pub choices: Vec<GrpChoice>,
    /// Span of the whole group.
    pub span: Span,
}

/// A single group choice: a sequence of group entries.
#[derive(Debug, Clone, PartialEq)]
pub struct GrpChoice {
    /// The entries making up this choice, in source order.
    pub entries: Vec<GrpEnt>,
    /// Span of the whole group choice.
    pub span: Span,
}

/// A group entry: optional occurrence, optional member key, and type or group ref.
#[derive(Debug, Clone, PartialEq)]
pub struct GrpEnt {
    /// The occurrence constraint prefix, if any.
    pub occur: Option<Occur>,
    /// The member key, if this entry is a map/array key-value pair.
    pub key: Option<MemberKey>,
    /// The entry's type or group reference body.
    pub entry: GrpEntBody,
    /// Span of the whole group entry.
    pub span: Span,
}

/// The body of a group entry.
#[derive(Debug, Clone, PartialEq)]
pub enum GrpEntBody {
    /// Inline type expression.
    Type(Type),
    /// Reference to a named group.
    GroupRef {
        /// The referenced group name.
        name: String,
        /// Generic arguments supplied at this use site, if any.
        args: Option<GenericArg>,
        /// Span of the whole group reference.
        span: Span,
    },
    /// Inline `( group )`.
    InlineGroup(Group),
}

/// The key part of a member entry.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberKey {
    /// `type1 [^] =>` — type-based key.
    Type {
        /// The key's type expression.
        key: Box<Type1>,
        /// Whether the `^` cut operator was present.
        cut: bool,
        /// Span of the whole member key.
        span: Span,
    },
    /// `bareword :` — shorthand text key.
    Bareword {
        /// The bareword text.
        name: String,
        /// Span of the whole `bareword :` key.
        span: Span,
    },
    /// `value :` — literal key.
    Value {
        /// The literal key value.
        key: Value,
        /// Span of the whole `value :` key.
        span: Span,
    },
}

// ─── Occurrence ───────────────────────────────────────────────────────────────

/// Occurrence constraint on a group entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Occur {
    /// `?`  — zero or one.
    Optional,
    /// `+`  — one or more.
    OneOrMore,
    /// `n*m` — bounded repetition (None = open-ended).
    Bounded {
        /// The lower bound, or `None` for an open lower bound.
        lower: Option<u64>,
        /// The upper bound, or `None` for an open upper bound.
        upper: Option<u64>,
    },
}

// ─── Values ───────────────────────────────────────────────────────────────────

/// A literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Integer literal (parsed from decimal, hex, or binary).
    Int(i64),
    /// Floating-point literal.
    Float(f64),
    /// Text string literal.
    Text(String),
    /// Byte string literal (raw, hex-encoded, or base64-encoded).
    Bytes(ByteStr),
}

/// A byte string value, together with its original encoding notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteStr {
    /// The notation the byte string was written in.
    pub encoding: ByteEncoding,
    /// The decoded byte string value.
    pub bytes: Vec<u8>,
}

/// How a byte string was written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ByteEncoding {
    /// `'...'` — raw UTF-8 bytes.
    Raw,
    /// `h'...'` — hexadecimal.
    Hex,
    /// `b64'...'` — base64.
    Base64,
}

// ─── Group flattening ─────────────────────────────────────────────────────────

/// Expand group-inclusion entries (entries with no member key) in a map group
/// into their constituent keyed entries, enabling correct matching of schemas
/// like `PersonalData = { ? name: tstr, NameComponents, ? age: uint }`.
///
/// Called by both the CBOR checker and the JSON checker before map matching.
/// A recursion depth limit prevents infinite expansion on pathological inputs.
pub fn flatten_map_entries(cddl: &Cddl, entries: &[GrpEnt], depth: u8) -> Vec<GrpEnt> {
    const MAX_DEPTH: u8 = 8;
    if depth >= MAX_DEPTH {
        return entries.to_vec();
    }
    let mut out = Vec::new();
    for entry in entries {
        if entry.key.is_none() {
            // RFC 8610 §3.2: "An occurrence indicator modifies the group
            // given to its right by requiring the group to match ... a
            // certain number of times". When the occurrence indicator on
            // this group-inclusion entry permits zero repetitions (`?`,
            // `*`, or an explicit lower bound of 0 — exactly the socket/plug
            // idiom of RFC 8610 §3.9, e.g. `* $$GP-Claims`), that permits
            // the *entire* included group to be absent, so every one of its
            // own entries that doesn't already carry a stricter occurrence
            // indicator must become individually optional too. Without
            // this, a bare `key: type` entry inside the included group
            // stays "exactly once" and is wrongly treated as mandatory.
            let force_optional = occur_allows_zero(entry.occur.as_ref());
            match &entry.entry {
                GrpEntBody::GroupRef { name, .. } => {
                    let expanded = expand_group_name(cddl, name);
                    if let Some(inner) = expanded {
                        let mut flattened = flatten_map_entries(cddl, &inner, depth + 1);
                        if force_optional {
                            promote_optional(&mut flattened);
                        }
                        out.extend(flattened);
                    } else {
                        out.push(entry.clone());
                    }
                }
                GrpEntBody::InlineGroup(g) => {
                    if let Some(choice) = g.choices.first() {
                        let mut flattened = flatten_map_entries(cddl, &choice.entries, depth + 1);
                        if force_optional {
                            promote_optional(&mut flattened);
                        }
                        out.extend(flattened);
                    }
                }
                GrpEntBody::Type(ty) => {
                    // A bare typename without a key in a map is a group inclusion.
                    // `NameComponents,` in a map body is parsed as GrpEntBody::Type
                    // with Type2::Typename because `type_` matches first in grpent.
                    if ty.choices.len() == 1
                        && ty.choices[0].constraint.is_none()
                        && let Type2::Typename {
                            name, args: None, ..
                        } = &ty.choices[0].base
                    {
                        let name = name.clone();
                        let expanded = expand_group_name(cddl, &name);
                        if let Some(inner) = expanded {
                            let mut flattened = flatten_map_entries(cddl, &inner, depth + 1);
                            if force_optional {
                                promote_optional(&mut flattened);
                            }
                            out.extend(flattened);
                            continue;
                        }
                    }
                    // Not a resolvable group inclusion — skip in map context.
                }
            }
        } else {
            out.push(entry.clone());
        }
    }
    out
}

/// Expand group-inclusion entries (entries with no member key) in an array
/// group into their constituent entries, enabling correct matching of
/// schemas like `COSE_Sign1 = [ Headers, payload: bstr / nil, signature: bstr ]`
/// where `Headers = ( protected: ..., unprotected: ... )` is a named group
/// spliced positionally into the surrounding array.
///
/// A bare `Foo` entry with no key is only a group inclusion when `Foo`
/// resolves to a named *group* rule (`expand_group_name` returns `Some`);
/// otherwise it's left untouched so the existing per-item `match_rule_name`
/// handling still applies (e.g. `[* label]` referencing an ordinary type
/// rule). Per the parser (`build_grpent`), a bare group-inclusion entry
/// parses as `GrpEntBody::Type` with a bare typename — `type_` matches
/// before `groupname` in the grammar — mirroring `flatten_map_entries`.
///
/// Called by both the CBOR checker and the JSON checker before array
/// matching. A recursion depth limit prevents infinite expansion on
/// pathological inputs.
pub fn flatten_array_entries(cddl: &Cddl, entries: &[GrpEnt], depth: u8) -> Vec<GrpEnt> {
    const MAX_DEPTH: u8 = 8;
    if depth >= MAX_DEPTH {
        return entries.to_vec();
    }
    let mut out = Vec::new();
    for entry in entries {
        if entry.key.is_none() {
            let force_optional = occur_allows_zero(entry.occur.as_ref());
            match &entry.entry {
                GrpEntBody::GroupRef { name, .. } => {
                    let expanded = expand_group_name(cddl, name);
                    if let Some(inner) = expanded {
                        let mut flattened = flatten_array_entries(cddl, &inner, depth + 1);
                        if force_optional {
                            promote_optional(&mut flattened);
                        }
                        out.extend(flattened);
                    } else {
                        out.push(entry.clone());
                    }
                }
                GrpEntBody::InlineGroup(g) => {
                    if let Some(choice) = g.choices.first() {
                        let mut flattened = flatten_array_entries(cddl, &choice.entries, depth + 1);
                        if force_optional {
                            promote_optional(&mut flattened);
                        }
                        out.extend(flattened);
                    }
                }
                GrpEntBody::Type(ty) => {
                    if ty.choices.len() == 1
                        && ty.choices[0].constraint.is_none()
                        && let Type2::Typename {
                            name, args: None, ..
                        } = &ty.choices[0].base
                    {
                        let name = name.clone();
                        let expanded = expand_group_name(cddl, &name);
                        if let Some(inner) = expanded {
                            let mut flattened = flatten_array_entries(cddl, &inner, depth + 1);
                            if force_optional {
                                promote_optional(&mut flattened);
                            }
                            out.extend(flattened);
                            continue;
                        }
                    }
                    out.push(entry.clone());
                }
            }
        } else {
            out.push(entry.clone());
        }
    }
    out
}

/// Whether an occurrence indicator permits the entry (or group) it modifies
/// to occur zero times: no indicator at all means "exactly once", so only
/// `?`, `*`, `n*m` with `n` absent/zero, and their explicit-zero-lower-bound
/// forms return `true`.
fn occur_allows_zero(occur: Option<&Occur>) -> bool {
    match occur {
        None => false,
        Some(Occur::Optional) => true,
        Some(Occur::OneOrMore) => false,
        Some(Occur::Bounded { lower, .. }) => lower.unwrap_or(0) == 0,
    }
}

/// Give every entry that has no occurrence indicator of its own an
/// `Optional` one, without touching entries that already specify a
/// (necessarily more specific) indicator.
fn promote_optional(entries: &mut [GrpEnt]) {
    for e in entries {
        if e.occur.is_none() {
            e.occur = Some(Occur::Optional);
        }
    }
}

/// Resolve a named group from the CDDL document, accumulating entries from
/// every matching rule's first choice, in declaration order.
///
/// A group name can be defined by more than one [`Rule`] when it is built via
/// `//=` extension (RFC 8610 §3.9's "group socket/plug" idiom) — each
/// extension statement produces its own `Rule` sharing the same `name`. Per
/// RFC 8610 §2, all such right-hand sides must be treated as if collected
/// into a single group choice, in the order the rules were given; returning
/// only the first match would silently drop every extension after the first.
fn expand_group_name(cddl: &Cddl, name: &str) -> Option<Vec<GrpEnt>> {
    let mut found = false;
    let mut out = Vec::new();
    for r in cddl.rules.iter().filter(|r| r.name == name) {
        if let RuleBody::Group(grp_ent) = &r.body
            && let GrpEntBody::InlineGroup(g) = &grp_ent.entry
        {
            found = true;
            if let Some(c) = g.choices.first() {
                out.extend(c.entries.clone());
            }
        }
    }
    found.then_some(out)
}

// ─── Generic instantiation ────────────────────────────────────────────────

/// Bind a generic rule's parameter names to the concrete type arguments
/// supplied at an instantiation site (RFC 8610 §3.10), e.g. `pair<a,b>`
/// bound by `pair<uint,tstr>` yields `{"a": uint, "b": tstr}`.
///
/// Returns `None` when the argument count doesn't match the parameter
/// count — callers should treat this as "don't know how to bind" (Finding
/// 8 tracks diagnosing the mismatch explicitly) rather than guess a partial
/// binding.
pub fn generic_bindings(
    parm: &GenericParm,
    arg: &GenericArg,
) -> Option<std::collections::HashMap<String, Type1>> {
    if parm.params.len() != arg.args.len() {
        return None;
    }
    Some(
        parm.params
            .iter()
            .cloned()
            .zip(arg.args.iter().cloned())
            .collect(),
    )
}

/// Substitute generic parameter names with their bound argument types
/// throughout a [`Type`], per RFC 8610 §3.10. A bare `Type2::Typename`
/// reference to a bound parameter is replaced by wrapping the bound
/// [`Type1`] in a [`Type2::ParenType`], which preserves the bound type's own
/// constraints and precedence regardless of the surrounding context.
pub fn substitute_generic_type(ty: &Type, bindings: &std::collections::HashMap<String, Type1>) -> Type {
    Type {
        choices: ty
            .choices
            .iter()
            .map(|t1| substitute_generic_type1(t1, bindings))
            .collect(),
        span: ty.span.clone(),
    }
}

fn substitute_generic_type1(t1: &Type1, bindings: &std::collections::HashMap<String, Type1>) -> Type1 {
    Type1 {
        base: substitute_generic_type2(&t1.base, bindings),
        constraint: t1.constraint.as_ref().map(|c| Constraint {
            op: c.op.clone(),
            target: substitute_generic_type2(&c.target, bindings),
            span: c.span.clone(),
        }),
        span: t1.span.clone(),
    }
}

fn substitute_generic_type2(t2: &Type2, bindings: &std::collections::HashMap<String, Type1>) -> Type2 {
    match t2 {
        Type2::Typename {
            name,
            args: None,
            span,
        } => match bindings.get(name) {
            Some(bound) => Type2::ParenType(Box::new(Type {
                choices: vec![bound.clone()],
                span: span.clone(),
            })),
            None => t2.clone(),
        },
        // A nested generic instantiation, e.g. `Wrapped<C>` inside the body
        // of `Outer<C> = Wrapped<C>`: the outer rule's own parameter `C`
        // must be substituted throughout the inner instantiation's argument
        // list before that instantiation is matched, or the bound type is
        // silently lost and `Wrapped` sees an unresolved reference to `C`.
        Type2::Typename {
            name,
            args: Some(ga),
            span,
        } => Type2::Typename {
            name: name.clone(),
            args: Some(substitute_generic_arg(ga, bindings)),
            span: span.clone(),
        },
        Type2::ParenType(inner) => {
            Type2::ParenType(Box::new(substitute_generic_type(inner, bindings)))
        }
        Type2::Map(g) => Type2::Map(substitute_generic_group(g, bindings)),
        Type2::Array(g) => Type2::Array(substitute_generic_group(g, bindings)),
        Type2::EnumGroup(g) => Type2::EnumGroup(substitute_generic_group(g, bindings)),
        Type2::Tagged { tag, inner, span } => Type2::Tagged {
            tag: tag.clone(),
            inner: Box::new(substitute_generic_type(inner, bindings)),
            span: span.clone(),
        },
        Type2::Unwrap { name, args, span } => Type2::Unwrap {
            name: name.clone(),
            args: args.as_ref().map(|ga| substitute_generic_arg(ga, bindings)),
            span: span.clone(),
        },
        Type2::EnumRef { name, args, span } => Type2::EnumRef {
            name: name.clone(),
            args: args.as_ref().map(|ga| substitute_generic_arg(ga, bindings)),
            span: span.clone(),
        },
        _ => t2.clone(),
    }
}

/// Substitute generic parameter names throughout a use-site [`GenericArg`]
/// list, so a bound parameter passed through as an argument to a *nested*
/// generic instantiation resolves to its outer binding.
fn substitute_generic_arg(ga: &GenericArg, bindings: &std::collections::HashMap<String, Type1>) -> GenericArg {
    GenericArg {
        args: ga
            .args
            .iter()
            .map(|t1| substitute_generic_type1(t1, bindings))
            .collect(),
        span: ga.span.clone(),
    }
}

fn substitute_generic_group(g: &Group, bindings: &std::collections::HashMap<String, Type1>) -> Group {
    Group {
        choices: g
            .choices
            .iter()
            .map(|c| GrpChoice {
                entries: c
                    .entries
                    .iter()
                    .map(|e| substitute_generic_grpent(e, bindings))
                    .collect(),
                span: c.span.clone(),
            })
            .collect(),
        span: g.span.clone(),
    }
}

/// Substitute generic parameter names throughout a single [`GrpEnt`] — used
/// both for group-rule bodies (`pair<a,b> = (a, b)`) and for individual
/// entries nested inside a substituted [`Group`].
pub fn substitute_generic_grpent(
    e: &GrpEnt,
    bindings: &std::collections::HashMap<String, Type1>,
) -> GrpEnt {
    GrpEnt {
        occur: e.occur.clone(),
        key: e.key.as_ref().map(|k| substitute_generic_memberkey(k, bindings)),
        entry: match &e.entry {
            GrpEntBody::Type(t) => GrpEntBody::Type(substitute_generic_type(t, bindings)),
            // Same nested-instantiation substitution gap as
            // `Type2::Typename`/`Unwrap`/`EnumRef`: a group reference's own
            // argument list must have the outer rule's bound parameters
            // substituted in too, e.g. `Outer<c> = (GroupRef<c>)`.
            GrpEntBody::GroupRef { name, args, span } => GrpEntBody::GroupRef {
                name: name.clone(),
                args: args.as_ref().map(|ga| substitute_generic_arg(ga, bindings)),
                span: span.clone(),
            },
            GrpEntBody::InlineGroup(g) => GrpEntBody::InlineGroup(substitute_generic_group(g, bindings)),
        },
        span: e.span.clone(),
    }
}

fn substitute_generic_memberkey(
    k: &MemberKey,
    bindings: &std::collections::HashMap<String, Type1>,
) -> MemberKey {
    match k {
        MemberKey::Type { key, cut, span } => MemberKey::Type {
            key: Box::new(substitute_generic_type1(key, bindings)),
            cut: *cut,
            span: span.clone(),
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod offset_to_line_col_tests {
    use super::offset_to_line_col;

    #[test]
    fn offset_zero_is_line_one_col_one() {
        assert_eq!(offset_to_line_col("abc\ndef", 0), (1, 1));
    }

    #[test]
    fn offset_at_newline_is_end_of_that_line() {
        // "ab\ncd" - offset 2 is the '\n' itself, still on line 1.
        assert_eq!(offset_to_line_col("ab\ncd", 2), (1, 3));
    }

    #[test]
    fn offset_just_after_newline_starts_next_line() {
        assert_eq!(offset_to_line_col("ab\ncd", 3), (2, 1));
    }

    #[test]
    fn offset_at_end_of_source() {
        assert_eq!(offset_to_line_col("abc", 3), (1, 4));
    }

    #[test]
    fn offset_past_end_is_clamped() {
        assert_eq!(offset_to_line_col("abc", 100), offset_to_line_col("abc", 3));
    }

    #[test]
    fn empty_source_is_line_one_col_one() {
        assert_eq!(offset_to_line_col("", 0), (1, 1));
    }

    #[test]
    fn multiple_newlines() {
        assert_eq!(offset_to_line_col("a\nb\nc", 4), (3, 1));
    }
}
