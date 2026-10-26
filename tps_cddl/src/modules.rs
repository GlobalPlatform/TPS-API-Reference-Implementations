/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CDDL module structure (`draft-ietf-cbor-cddl-modules-06`): `;# import` / `;# include`
//! directives with optional namespace prefixes (`as`) and explicit rule selection (`from`).
//!
//! This module implements a **preprocessing pass** that runs entirely before
//! [`crate::validate_with_config`], [`crate::check_cbor`], [`crate::check_json`],
//! [`crate::gen_decoder`], and [`crate::gen_encoder`]. It reads a top-level CDDL file plus
//! every transitively `;#`-referenced module file, and produces one flat, renamed, merged
//! [`crate::ast::Cddl`] AST that the rest of the crate consumes unmodified:
//!
//! ```text
//! top-level file path
//!      │
//!      ▼
//! resolve_modules(path, search_path)
//!      │  - recursively: read file → scan ";#" directives (raw text) →
//!      │    lex()+build_cddl() that file's own rules → recursively resolve
//!      │    each directive's target module → select/rename/merge its rules
//!      │    into this file's own accumulated Cddl
//!      ▼
//! ModuleResolution { source, cddl, warnings }
//!      │
//!      ▼
//! validate_with_config / check_cbor / check_json / gen_decoder / gen_encoder   (unchanged)
//! ```
//!
//! Directive lines (`;#...`) are already parsed as ordinary comments by the pest grammar
//! (see `grammar.pest`'s `COMMENT` rule), so a file using directives remains valid, parseable
//! basic CDDL even when this feature is disabled — directive extraction happens via a
//! separate raw-text scan over the source, not via the pest CST.

#![allow(dead_code)] // populated incrementally across Phase 9 steps

use crate::ast::{
    Assignment, Cddl, Constraint, ConstraintOp, GenericArg, Group, GrpChoice, GrpEnt, GrpEntBody,
    HeadNumber, MemberKey, Rule, RuleBody, Span, Type, Type1, Type2, Value,
};
use crate::error::{Diagnostic, Level, ParseError};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

// ─── Directives ────────────────────────────────────────────────────────────────

/// The kind of a CDDL module directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `;# import ...` — pulls in only the transitive closure of referenced rules.
    Import,
    /// `;# include ...` — pulls in every rule the module defines (or exactly the
    /// explicitly named ones, with no further closure).
    Include,
}

/// The rule-name selector in a directive's optional `from` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromSelector {
    /// `from mod` clause used a wildcard `*` — select every rule from the module.
    All,
    /// `from mod` clause named specific rules, exactly as written in the source
    /// (either bare, e.g. `label`, or already prefix-qualified, e.g. `cose.label`).
    Named(Vec<String>),
}

/// A single parsed `;# import` / `;# include` directive.
#[derive(Debug, Clone, PartialEq)]
pub struct Directive {
    /// Whether this is an `import` or `include` directive.
    pub kind: DirectiveKind,
    /// Explicit rule selection (`from` clause), or `None` if absent.
    pub from: Option<FromSelector>,
    /// The bare module name as written (no `.cddl` suffix), e.g. `"rfc9052"`.
    pub module: String,
    /// Namespace prefix from an `as` clause, or `None` if absent.
    pub as_prefix: Option<String>,
    /// Span of the whole directive line (from `;#` to end of line, excluding the
    /// line terminator).
    pub span: Span,
}

/// A single parsed `;# start` directive — a project-specific extension (not
/// part of `draft-ietf-cbor-cddl-modules-06`) letting a CDDL file declare its
/// own preferred checking entry point, e.g. `;# start rfc8392.Claims-Set` for
/// a file that extends another module's rule. Only the directly-loaded
/// top-level file's own `;# start` is ever surfaced by [`resolve_modules`];
/// one declared on a transitively-included submodule is ignored.
#[derive(Debug, Clone, PartialEq)]
pub struct StartDirective {
    /// The rule name as written, e.g. `"rfc8392.Claims-Set"`. Not validated
    /// against the final merged ruleset — a name that doesn't resolve there
    /// is reported the same way any other bad root-rule name is, at check
    /// time.
    pub rule_name: String,
    /// Span of the whole directive line.
    pub span: Span,
}

/// A single parsed `;# constrain` directive — a project-specific extension (not
/// part of `draft-ietf-cbor-cddl-modules-06`) that clones a target rule under a
/// new name with exactly one field's type replaced, without mutating the
/// original rule. Motivating use case: constraining `COSE_Sign1.payload` (a
/// `bstr`) to a `.cborseq`-typed profile schema without modifying the upstream
/// module that defines `COSE_Sign1`.
///
/// Grammar: `;# constrain <qualified-rule> as <new-name> { <field>: <type1> }`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstrainDirective {
    /// The target rule name as written, possibly prefixed (e.g. `cose.COSE_Sign1`).
    pub target: String,
    /// The name of the new rule to synthesize.
    pub new_name: String,
    /// The single overridden field: `(field-name, replacement-type)`. v1 supports
    /// exactly one field override per directive.
    pub field_override: (String, Type1),
    /// Span of the whole directive line.
    pub span: Span,
}

// ─── Line scanner ──────────────────────────────────────────────────────────────

/// Scan `source` for `;#`-prefixed directive lines.
///
/// Returns the successfully parsed [`Directive`]s (in document order) and a list of
/// [`Diagnostic`] warnings for lines that start with `;#` but do not match the directive
/// grammar (treated as ordinary comments, per the specification's backward-compatibility
/// design — see `draft-ietf-cbor-cddl-modules-06.txt` §2.1).
///
/// Only lines whose *first two bytes* are literally `;#` are considered candidates; this
/// matches the specification exactly ("lines that start with the prefix `;#`") and means
/// ordinary comments elsewhere in the file (including ones that happen to contain `;#`
/// mid-line) are never scanned.
pub fn scan_directives(
    source: &str,
) -> (
    Vec<Directive>,
    Vec<StartDirective>,
    Vec<ConstrainDirective>,
    Vec<Diagnostic>,
) {
    let mut directives = Vec::new();
    let mut start_directives = Vec::new();
    let mut constrain_directives = Vec::new();
    let mut diagnostics = Vec::new();

    let bytes = source.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let line_start = pos;
        let mut line_end = pos;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        let mut content_end = line_end;
        if content_end > line_start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }

        if bytes[line_start..content_end].starts_with(b";#") {
            let span = Span::new(line_start, content_end);
            // Safety of the slice: `;#` is ASCII, so byte offsets line up with `str`
            // char boundaries at this point.
            let line = &source[line_start..content_end];
            match parse_directive_line(&line[2..]) {
                Ok(DirectiveLine::Module(kind, from, module, as_prefix)) => {
                    directives.push(Directive {
                        kind,
                        from,
                        module,
                        as_prefix,
                        span,
                    });
                }
                Ok(DirectiveLine::Start(rule_name)) => {
                    start_directives.push(StartDirective { rule_name, span });
                }
                Ok(DirectiveLine::Constrain(target, new_name, field_override)) => {
                    constrain_directives.push(ConstrainDirective {
                        target,
                        new_name,
                        field_override: *field_override,
                        span,
                    });
                }
                Err(reason) => {
                    diagnostics.push(Diagnostic {
                        level: Level::Warning,
                        span,
                        message: format!("malformed `;#` directive: {reason}"),
                    });
                }
            }
        }

        pos = if line_end < bytes.len() {
            line_end + 1
        } else {
            line_end
        };
    }

    (directives, start_directives, constrain_directives, diagnostics)
}

/// The parsed shape of one directive line, before it is wrapped in its
/// public [`Directive`]/[`StartDirective`] struct (which also needs the
/// caller's [`Span`]).
enum DirectiveLine {
    Module(DirectiveKind, Option<FromSelector>, String, Option<String>),
    /// A project-specific `;# start <rule-name>` line — see [`StartDirective`].
    Start(String),
    /// A project-specific `;# constrain ... as ... { field: type }` line — see
    /// [`ConstrainDirective`].
    Constrain(String, String, Box<(String, Type1)>),
}

/// Parse the portion of a directive line after the leading `;#`.
fn parse_directive_line(rest: &str) -> Result<DirectiveLine, String> {
    let after_rs1 = consume_rs(rest).ok_or_else(|| "expected a space after `;#`".to_string())?;

    if let Some(after_kw) = consume_word(after_rs1, "start") {
        let rest2 = consume_rs(after_kw)
            .ok_or_else(|| "expected a space after directive keyword".to_string())?;
        if rest2.contains('\t') {
            return Err("tab characters are not permitted within a directive".to_string());
        }
        let tokens: Vec<&str> = rest2.split(' ').filter(|t| !t.is_empty()).collect();
        match tokens.as_slice() {
            [] => Err("missing rule name in `start` directive".to_string()),
            [name] if is_valid_start_rule_name(name) => Ok(DirectiveLine::Start(name.to_string())),
            [name] => Err(format!("invalid rule name `{name}` in `start` directive")),
            _ => Err("`start` directive takes exactly one rule name".to_string()),
        }
    } else if let Some(after_kw) = consume_word(after_rs1, "constrain") {
        let rest2 = consume_rs(after_kw)
            .ok_or_else(|| "expected a space after directive keyword".to_string())?;
        let (target, new_name, field_override) = parse_constrain_line(rest2)?;
        Ok(DirectiveLine::Constrain(target, new_name, Box::new(field_override)))
    } else {
        let (kind, from, module, as_prefix) = parse_import_or_include_line(after_rs1)?;
        Ok(DirectiveLine::Module(kind, from, module, as_prefix))
    }
}

type ParsedDirective = (DirectiveKind, Option<FromSelector>, String, Option<String>);

/// Parse an `import`/`include` directive line, starting right after the `;#`'s
/// required leading space (i.e. `after_rs1` from [`parse_directive_line`]).
fn parse_import_or_include_line(after_rs1: &str) -> Result<ParsedDirective, String> {
    let (kind, after_kw) = consume_keyword(after_rs1).ok_or_else(|| {
        "unknown directive keyword (expected `import`, `include`, or `start`)".to_string()
    })?;
    let rest2 =
        consume_rs(after_kw).ok_or_else(|| "expected a space after directive keyword".to_string())?;

    if rest2.contains('\t') {
        return Err("tab characters are not permitted within a directive".to_string());
    }

    let tokens: Vec<&str> = rest2.split(' ').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return Err("missing module filename".to_string());
    }

    // The from-clause, if present, is terminated by a literal "from" token; per the
    // grammar (`1*(id-or-all [","] RS) %s"from" RS`) there must be at least one
    // id-or-all token before it, so we only recognise "from" at index >= 1.
    let from_idx = tokens
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, t)| **t == "from")
        .map(|(i, _)| i);

    let (from_tokens, filename_tok, as_tokens): (&[&str], &str, &[&str]) = match from_idx {
        Some(idx) => {
            let filename = *tokens
                .get(idx + 1)
                .ok_or_else(|| "missing module filename after `from`".to_string())?;
            (&tokens[0..idx], filename, &tokens[idx + 2..])
        }
        None => (&[], tokens[0], &tokens[1..]),
    };

    let from = if from_idx.is_some() {
        Some(parse_from_clause(from_tokens)?)
    } else {
        None
    };

    if !is_valid_filename(filename_tok) {
        return Err(format!("invalid module filename `{filename_tok}`"));
    }
    let module = filename_tok.to_string();

    let as_prefix = parse_as_clause(as_tokens)?;

    Ok((kind, from, module, as_prefix))
}

fn parse_from_clause(tokens: &[&str]) -> Result<FromSelector, String> {
    let mut names = Vec::new();
    let mut has_wildcard = false;
    for raw in tokens {
        let name = raw.strip_suffix(',').unwrap_or(raw);
        if name.is_empty() {
            return Err("empty name in `from` clause".to_string());
        }
        if name == "*" {
            has_wildcard = true;
        } else if !is_valid_id(name) {
            return Err(format!("invalid identifier `{name}` in `from` clause"));
        } else {
            names.push(name.to_string());
        }
    }
    if names.is_empty() && !has_wildcard {
        return Err("empty `from` clause".to_string());
    }
    if has_wildcard {
        Ok(FromSelector::All)
    } else {
        Ok(FromSelector::Named(names))
    }
}

fn parse_as_clause(tokens: &[&str]) -> Result<Option<String>, String> {
    match tokens {
        [] => Ok(None),
        [kw, prefix] if *kw == "as" => {
            if !is_valid_id(prefix) {
                return Err(format!("invalid identifier `{prefix}` in `as` clause"));
            }
            Ok(Some(prefix.to_string()))
        }
        _ => Err("malformed `as` clause".to_string()),
    }
}

/// Parse a `;# constrain <target> as <new-name> { <field>: <type1> }` directive line,
/// starting right after the `;# constrain `'s required leading space.
///
/// The `{ field: type }` clause's right-hand side is a full `Type1` expression, so
/// rather than hand-rolling a second type-expression parser, the brace-delimited
/// clause is extracted as a raw substring and handed to the crate's own `lex`/
/// `build_cddl` pipeline via a throwaway one-rule document — this reuses the full
/// grammar (including `.cborseq`, generic args, etc.) for free.
fn parse_constrain_line(rest: &str) -> Result<(String, String, (String, Type1)), String> {
    let brace_start = rest
        .find('{')
        .ok_or_else(|| "missing `{ field: type }` clause in `constrain` directive".to_string())?;
    let head = rest[..brace_start].trim_end_matches(' ');
    let clause = &rest[brace_start..];

    let tokens: Vec<&str> = head.split(' ').filter(|t| !t.is_empty()).collect();
    let (target, new_name) = match tokens.as_slice() {
        [target, kw, new_name] if *kw == "as" => (*target, *new_name),
        _ => {
            return Err(
                "expected `<target> as <new-name>` before `{...}` in `constrain` directive"
                    .to_string(),
            );
        }
    };
    if !is_valid_id(target) {
        return Err(format!("invalid identifier `{target}` in `constrain` directive"));
    }
    if !is_valid_id(new_name) {
        return Err(format!("invalid identifier `{new_name}` in `constrain` directive"));
    }

    let field_override = parse_constrain_field_clause(clause)?;
    Ok((target.to_string(), new_name.to_string(), field_override))
}

/// Parse the `{ field: type }` clause of a `;# constrain` directive (the brace-delimited
/// substring, including both braces).
fn parse_constrain_field_clause(clause: &str) -> Result<(String, Type1), String> {
    let inner = clause
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| "malformed `{...}` clause in `constrain` directive".to_string())?;

    let colon_idx = inner
        .find(':')
        .ok_or_else(|| "expected `field: type` in `constrain` directive's `{...}` clause".to_string())?;
    let field = inner[..colon_idx].trim();
    let type_src = inner[colon_idx + 1..].trim();
    if field.is_empty() {
        return Err("missing field name in `constrain` directive's `{...}` clause".to_string());
    }
    if type_src.is_empty() {
        return Err("missing field type in `constrain` directive's `{...}` clause".to_string());
    }

    let synth = format!("__constrain_tmp = {type_src}");
    let pairs = crate::lex(&synth)
        .map_err(|e| format!("invalid type expression in `constrain` directive: {e}"))?;
    let cddl = crate::build_cddl(pairs)
        .map_err(|e| format!("invalid type expression in `constrain` directive: {e}"))?;
    let rule = cddl
        .rules
        .into_iter()
        .next()
        .ok_or_else(|| "invalid type expression in `constrain` directive".to_string())?;
    let RuleBody::Type(ty) = rule.body else {
        return Err("invalid type expression in `constrain` directive".to_string());
    };
    let t1 = ty
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "invalid type expression in `constrain` directive".to_string())?;

    Ok((field.to_string(), t1))
}

/// Consume one-or-more literal ASCII spaces (`RS = 1*WS`, `WS = SP`). Returns `None` if
/// `s` does not start with at least one space (a tab does not count — this deliberately
/// matches the spec's `WS = SP` production, which excludes tabs).
fn consume_rs(s: &str) -> Option<&str> {
    let stripped = s.trim_start_matches(' ');
    if stripped.len() < s.len() {
        Some(stripped)
    } else {
        None
    }
}

/// Consume the literal, case-sensitive keyword `import` or `include`, requiring that it
/// be followed by a space or end-of-input (so `importx` is not mistaken for `import`).
fn consume_keyword(s: &str) -> Option<(DirectiveKind, &str)> {
    for (word, kind) in [
        ("import", DirectiveKind::Import),
        ("include", DirectiveKind::Include),
    ] {
        if let Some(rest) = s.strip_prefix(word)
            && (rest.is_empty() || rest.starts_with(' '))
        {
            return Some((kind, rest));
        }
    }
    None
}

/// Consume the literal, case-sensitive word `word`, requiring that it be followed by a
/// space or end-of-input (so e.g. `startx` is not mistaken for `start`).
fn consume_word<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(word)?;
    if rest.is_empty() || rest.starts_with(' ') {
        Some(rest)
    } else {
        None
    }
}

/// Rule-name charset for a `;# start` directive's argument: a dotted, possibly
/// hyphenated CDDL identifier such as `rfc8392.Claims-Set` — i.e. `id [ "." id ]`,
/// where `id` follows `grammar.pest`'s own production (`EALPHA` first, then any mix
/// of `EALPHA`/digit/`-`/`.`). Unlike [`is_valid_id`] (the stricter charset used for
/// directive `from`/`as` clauses per the modules draft's own ABNF), this deliberately
/// allows hyphens, since `;# start` names a real CDDL rule rather than a
/// modules-draft-only identifier.
fn is_valid_start_rule_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '$' || c == '_' || c == '@' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| matches!(c, '$' | '_' | '@' | '.' | '-') || c.is_ascii_alphanumeric())
}

/// Identifier charset for directive `from`/`as` clauses, per Appendix A of
/// `draft-ietf-cbor-cddl-modules-06.txt`: `$`, `@`, `A`-`Z`, `_`, `a`-`z` (first char),
/// plus digits and `.` in subsequent positions.
///
/// Deviation from the literal ABNF: the grammar's `id` production has no `.` at all, yet
/// the worked examples in §2.6 use dotted, prefix-qualified names (`cose.label`) as
/// `from`-clause entries — an apparent oversight in this early draft. To make those
/// examples parse, `.` is accepted in non-leading position. Hyphens are *not* accepted
/// here (unlike core CDDL identifiers), matching the ABNF exactly — a rule name such as
/// the prelude's `encoded-cbor` therefore cannot be named in a directive clause.
fn is_valid_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '$' || c == '_' || c == '@' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '$' || c == '_' || c == '@' || c == '.' || c.is_ascii_alphanumeric())
}

/// Module filename charset, per Appendix A: `-`, `.`, digits, letters, `_`.
fn is_valid_filename(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c == '-' || c == '.' || c == '_' || c.is_ascii_alphanumeric())
}

// ─── Module search path ────────────────────────────────────────────────────────

/// Ordered list of directories searched for `{module}.cddl` files.
///
/// Construct via [`ModuleSearchPath::new`] (explicit list, for testing) or
/// [`ModuleSearchPath::from_env_and_cwd`] (the real search order: current working
/// directory, then caller-supplied `--include-path` directories, then
/// `CDDL_INCLUDE_PATH`).
#[derive(Debug, Clone)]
pub struct ModuleSearchPath {
    dirs: Vec<PathBuf>,
}

/// Errors that can occur while resolving or loading CDDL modules.
#[derive(Debug)]
pub enum ModuleError {
    /// No directory in the search path contained `{module}.cddl`.
    NotFound {
        /// The bare module name that could not be found.
        module: String,
        /// The search directories that were checked, in order.
        searched: Vec<PathBuf>,
        /// Span of the directive that referenced the module.
        span: Span,
    },
    /// A single search directory contained more than one filename that matches
    /// `{module}.cddl` case-insensitively.
    Ambiguous {
        /// The bare module name that was ambiguous.
        module: String,
        /// The search directory containing the conflicting filenames.
        dir: PathBuf,
        /// The conflicting filenames found in `dir`.
        candidates: Vec<String>,
        /// Span of the directive that referenced the module.
        span: Span,
    },
    /// A module (transitively) references itself via `;# import`/`;# include`.
    Circular {
        /// The chain of module file paths forming the cycle, in reference order.
        cycle: Vec<PathBuf>,
        /// Span of the directive that closed the cycle.
        span: Span,
    },
    /// A directive's `from` clause named a rule the target module does not define.
    UndefinedFromName {
        /// The rule name that was not found.
        name: String,
        /// The bare module name it was looked up in.
        module: String,
        /// Span of the directive naming the rule.
        span: Span,
    },
    /// Reading a module file from disk failed.
    Io {
        /// Path of the module file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
    /// A module file's own CDDL source failed to parse.
    Parse {
        /// Path of the module file whose source failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        source: ParseError,
    },
    /// A `;# constrain` directive's target rule was not found among the rules
    /// accumulated so far (own rules plus everything already imported/included).
    ConstrainTargetNotFound {
        /// The target rule name as written in the directive.
        target: String,
        /// Span of the directive naming the target.
        span: Span,
    },
    /// A `;# constrain` directive's field name was not found in the target
    /// rule's group (neither a map nor an array entry with that key).
    ConstrainFieldNotFound {
        /// The target rule name as written in the directive.
        target: String,
        /// The field name that was not found in the target rule.
        field: String,
        /// Span of the directive naming the field.
        span: Span,
    },
}

impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleError::NotFound { module, searched, .. } => {
                write!(
                    f,
                    "module `{module}` not found; searched: {}",
                    searched
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ModuleError::Ambiguous {
                module,
                dir,
                candidates,
                ..
            } => write!(
                f,
                "module `{module}` is ambiguous in `{}`: matches {}",
                dir.display(),
                candidates.join(", ")
            ),
            ModuleError::Circular { cycle, .. } => write!(
                f,
                "circular module reference: {}",
                cycle
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            ModuleError::UndefinedFromName { name, module, .. } => {
                write!(f, "`{name}` is not defined in module `{module}`")
            }
            ModuleError::Io { path, source } => {
                write!(f, "failed to read `{}`: {source}", path.display())
            }
            ModuleError::Parse { path, source } => {
                write!(f, "failed to parse `{}`: {source}", path.display())
            }
            ModuleError::ConstrainTargetNotFound { target, .. } => {
                write!(f, "`constrain` directive's target rule `{target}` was not found")
            }
            ModuleError::ConstrainFieldNotFound { target, field, .. } => {
                write!(
                    f,
                    "`constrain` directive's field `{field}` was not found in target rule `{target}`"
                )
            }
        }
    }
}

impl std::error::Error for ModuleError {}

impl ModuleSearchPath {
    /// Build a search path from an explicit, already-ordered list of directories.
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self { dirs }
    }

    /// Build the real search path: current working directory, then `extra` (in the
    /// order given, e.g. from repeated `--include-path` CLI flags), then every
    /// directory in `CDDL_INCLUDE_PATH` (platform-native separator, in listed order).
    pub fn from_env_and_cwd(extra: &[PathBuf]) -> io::Result<Self> {
        let cwd = std::env::current_dir()?;
        let env_value = std::env::var_os("CDDL_INCLUDE_PATH");
        Ok(Self {
            dirs: build_search_dirs(cwd, extra, env_value.as_deref()),
        })
    }

    /// Resolve `module_name` (bare, no `.cddl` suffix) to a file path by searching
    /// each directory in order for a case-insensitive match on `{module_name}.cddl`.
    ///
    /// Within a single directory, more than one matching entry is a hard
    /// [`ModuleError::Ambiguous`] (no fallthrough to later directories). Zero matches
    /// in a directory tries the next; no match anywhere is [`ModuleError::NotFound`].
    pub fn resolve(&self, module_name: &str, span: Span) -> Result<PathBuf, ModuleError> {
        let target = format!("{module_name}.cddl");
        for dir in &self.dirs {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            let names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            let matches = find_case_insensitive_matches(names.iter().map(String::as_str), &target);
            match matches.len() {
                0 => continue,
                1 => return Ok(dir.join(matches[0])),
                _ => {
                    return Err(ModuleError::Ambiguous {
                        module: module_name.to_string(),
                        dir: dir.clone(),
                        candidates: matches.into_iter().map(str::to_string).collect(),
                        span,
                    });
                }
            }
        }
        Err(ModuleError::NotFound {
            module: module_name.to_string(),
            searched: self.dirs.clone(),
            span,
        })
    }
}

/// Pure, filesystem-free core of directory-entry matching: return every entry in
/// `entries` that equals `target` under ASCII case folding.
///
/// Exists standalone so ambiguity detection (multiple case-variant matches in one
/// directory) can be unit-tested with synthetic name lists — real filesystems
/// (especially case-insensitive ones like NTFS) cannot host two committed fixtures
/// differing only by case.
fn find_case_insensitive_matches<'a>(
    entries: impl Iterator<Item = &'a str>,
    target: &str,
) -> Vec<&'a str> {
    entries
        .filter(|e| e.eq_ignore_ascii_case(target))
        .collect()
}

/// Pure ordering logic behind [`ModuleSearchPath::from_env_and_cwd`], separated out so
/// it can be unit-tested without mutating the process-global `CDDL_INCLUDE_PATH`
/// environment variable (which is racy under parallel test execution).
fn build_search_dirs(
    cwd: PathBuf,
    extra: &[PathBuf],
    env_value: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    let mut dirs = vec![cwd];
    dirs.extend(extra.iter().cloned());
    if let Some(path_var) = env_value {
        dirs.extend(std::env::split_paths(path_var));
    }
    dirs
}

// ─── Recursive loader ───────────────────────────────────────────────────────────

/// Shared, mutable state threaded through recursive module resolution.
///
/// Rules loaded once (fully expanded, but — until Step 9.5 — with directive targets
/// merged in unprefixed and un-renamed) are cached by canonical path and reused for
/// every importer; callers must clone before mutating (Step 9.5's per-importer
/// selection/rename pass never mutates the cached copy).
struct LoaderCtx {
    /// Fully-resolved `Cddl` per module, keyed by lowercased canonical path (so
    /// case-insensitive filesystems and case-varying directive references collapse
    /// to the same cache entry).
    cache: HashMap<String, Cddl>,
    /// Byte offset at which each module's source was appended to `virtual_source`,
    /// same keying as `cache`. Consumed by Step 9.5's span-rebasing.
    offsets: HashMap<String, usize>,
    /// Canonical paths currently being resolved (innermost last), for cycle detection
    /// and cycle-error reporting.
    stack: Vec<PathBuf>,
    /// Directories searched to resolve a bare module name to a file.
    search_path: ModuleSearchPath,
    /// Concatenation of every module's source loaded so far, in load order.
    virtual_source: String,
    /// Malformed-directive warnings collected across every module loaded so far.
    warnings: Vec<Diagnostic>,
    /// The topmost/outermost loaded file's own `;# start` directive, if it has one.
    /// Populated at most once, the first time `load_module_uncached` runs with
    /// `stack.len() == 1` (i.e. resolving the file passed directly to
    /// [`resolve_modules`], not a transitively-included submodule).
    start_rule: Option<StartDirective>,
}

impl LoaderCtx {
    fn new(search_path: ModuleSearchPath) -> Self {
        Self {
            cache: HashMap::new(),
            offsets: HashMap::new(),
            stack: Vec::new(),
            search_path,
            virtual_source: String::new(),
            warnings: Vec::new(),
            start_rule: None,
        }
    }
}

/// Canonicalize `path`, returning both the canonical path (for display/cycle
/// reporting) and a lowercased-string cache key (so `Foo.cddl` and `foo.cddl` share
/// one cache entry on case-insensitive filesystems).
fn canonical_key(path: &Path) -> io::Result<(PathBuf, String)> {
    let canon = std::fs::canonicalize(path)?;
    let key = canon.to_string_lossy().to_lowercase();
    Ok((canon, key))
}

/// Recursively resolve `path`, following every `;#` directive it contains.
///
/// Every rule's [`Span`] is rebased into `virtual_source` coordinates as soon as
/// its owning module is loaded, so a module cached here (and every rule cloned out
/// of it by an importer) always carries spans valid against `ctx.virtual_source`.
fn resolve_module(ctx: &mut LoaderCtx, path: &Path, span: Span) -> Result<Cddl, ModuleError> {
    let (canon, key) = canonical_key(path).map_err(|source| ModuleError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    if let Some(cddl) = ctx.cache.get(&key) {
        return Ok(cddl.clone());
    }

    if let Some(pos) = ctx.stack.iter().position(|p| *p == canon) {
        let mut cycle = ctx.stack[pos..].to_vec();
        cycle.push(canon);
        return Err(ModuleError::Circular { cycle, span });
    }

    ctx.stack.push(canon.clone());
    let result = load_module_uncached(ctx, &canon, &key);
    ctx.stack.pop();
    result
}

/// The result of resolving a top-level CDDL file's module directives: the flat,
/// merged, renamed [`Cddl`] AST ready for [`crate::validate_with_config`] and friends,
/// the concatenated virtual source it was parsed against (needed to render any
/// [`Diagnostic`]'s span), and any malformed-`;#`-line warnings collected along the way.
#[derive(Debug, Clone)]
pub struct ModuleResolution {
    /// Concatenation of every loaded module's source, in load order. Every [`Span`]
    /// in `cddl` and `warnings` is a byte range into this string.
    pub source: String,
    /// The flat, merged, renamed AST — hand this to [`crate::validate_with_config`],
    /// [`crate::check_cbor`], [`crate::check_json`], [`crate::gen_decoder`], or
    /// [`crate::gen_encoder`] exactly as if it were an ordinary single-file `Cddl`.
    pub cddl: Cddl,
    /// Warnings for `;#`-prefixed lines that did not match the directive grammar
    /// (treated as ordinary comments), collected across every module loaded.
    pub warnings: Vec<Diagnostic>,
    /// The rule name from the topmost/outermost file's own `;# start` directive, if
    /// any — e.g. `"rfc8392.Claims-Set"`. A `;# start` declared on a transitively
    /// `;# import`/`;# include`d submodule is not surfaced here; only the directly-
    /// loaded top-level file's own directive counts as the resolved default entry
    /// point. `None` if that file has no `;# start` line at all.
    pub start_rule: Option<String>,
}

/// Resolve `path`'s `;# import` / `;# include` directives, transitively, into one
/// flat [`ModuleResolution`]. This is the crate's public entry point for module
/// resolution — see the module-level doc comment for the overall pipeline.
pub fn resolve_modules(
    path: &Path,
    search_path: &ModuleSearchPath,
) -> Result<ModuleResolution, ModuleError> {
    let mut ctx = LoaderCtx::new(search_path.clone());
    let cddl = resolve_module(&mut ctx, path, Span::new(0, 0))?;
    Ok(ModuleResolution {
        source: ctx.virtual_source,
        cddl,
        warnings: ctx.warnings,
        start_rule: ctx.start_rule.map(|d| d.rule_name),
    })
}

fn load_module_uncached(ctx: &mut LoaderCtx, canon: &Path, key: &str) -> Result<Cddl, ModuleError> {
    let source = std::fs::read_to_string(canon).map_err(|source| ModuleError::Io {
        path: canon.to_path_buf(),
        source,
    })?;

    let offset = ctx.virtual_source.len();
    ctx.virtual_source.push_str(&source);
    ctx.offsets.insert(key.to_string(), offset);

    let pairs = crate::lex(&source).map_err(|source| ModuleError::Parse {
        path: canon.to_path_buf(),
        source,
    })?;
    let mut cddl = crate::build_cddl(pairs).map_err(|source| ModuleError::Parse {
        path: canon.to_path_buf(),
        source,
    })?;
    for rule in &mut cddl.rules {
        transform_rule(rule, offset, None);
    }
    cddl.span = Span::new(offset, offset + source.len());

    let (directives, start_directives, constrain_directives, mut warnings) =
        scan_directives(&source);
    for w in &mut warnings {
        offset_span(&mut w.span, offset);
    }
    ctx.warnings.append(&mut warnings);

    // Only the directly-loaded top-level file (stack depth 1 — this file, and
    // nothing above it) contributes its `;# start` to the resolution result; one
    // declared on a transitively-included submodule is ignored. A file with more
    // than one `;# start` line uses the first (extras are otherwise-unreported,
    // matching how a raw scan of any other unhandled `;#`-prefixed content
    // silently succeeds when it does match the grammar).
    if ctx.stack.len() == 1 {
        ctx.start_rule = start_directives.into_iter().next().map(|mut d| {
            offset_span(&mut d.span, offset);
            d
        });
    }

    // Bare `;# import` only pulls in rules *referenced* by the importing
    // file's own content (see `collect_referenced_names` below). A `;#
    // constrain` directive's target and its override clause's type
    // expression (e.g. `eap.Something` in `bstr .cborseq eap.Something`)
    // live only in `ConstrainDirective`, never in any rule body's AST, so
    // both must be added to the reference set explicitly or a bare import
    // won't pull in the names a constrain directive itself depends on.
    let mut constrain_targets: Vec<String> = constrain_directives.iter().map(|cd| cd.target.clone()).collect();
    for cd in &constrain_directives {
        let wrapper = Type {
            choices: vec![cd.field_override.1.clone()],
            span: cd.field_override.1.span.clone(),
        };
        crate::gen_common::collect_type_deps(&wrapper, &mut constrain_targets);
    }

    for directive in &directives {
        let mut directive_span = directive.span.clone();
        offset_span(&mut directive_span, offset);

        let target_path = ctx
            .search_path
            .resolve(&directive.module, directive_span.clone())?;
        let target_cddl = resolve_module(ctx, &target_path, directive_span.clone())?;

        let mut selected = select_rules(
            directive,
            &directive_span,
            &target_cddl,
            &cddl.rules,
            &constrain_targets,
        )?;

        if let Some(prefix) = &directive.as_prefix {
            let local_names: HashSet<String> =
                target_cddl.rules.iter().map(|r| r.name.clone()).collect();
            for rule in &mut selected {
                transform_rule(rule, 0, Some((&local_names, prefix)));
            }
            selected.extend(bare_alias_rules(directive, prefix, &directive_span));
        }

        cddl.rules.extend(selected);
    }

    for cd in &constrain_directives {
        let mut directive_span = cd.span.clone();
        offset_span(&mut directive_span, offset);

        let target_rule = cddl
            .rules
            .iter()
            .find(|r| r.name == cd.target)
            .ok_or_else(|| ModuleError::ConstrainTargetNotFound {
                target: cd.target.clone(),
                span: directive_span.clone(),
            })?
            .clone();

        let mut new_body = target_rule.body.clone();
        let (field, replacement) = &cd.field_override;
        let mut replacement = replacement.clone();
        stamp_span_type1(&mut replacement, &directive_span);

        if !replace_field_in_rule_body(&mut new_body, field, replacement) {
            return Err(ModuleError::ConstrainFieldNotFound {
                target: cd.target.clone(),
                field: field.clone(),
                span: directive_span.clone(),
            });
        }

        cddl.rules.push(Rule {
            name: cd.new_name.clone(),
            generic_parm: None,
            assignment: Assignment::Assign,
            body: new_body,
            span: directive_span,
        });
    }

    ctx.cache.insert(key.to_string(), cddl.clone());
    Ok(cddl)
}

/// Step 9.6: for an `import` directive with an `as` prefix, the draft specifies that
/// each `from`-clause name written **bare** (no `.` in the source token) gets an
/// additional top-level alias rule `{bare_name} = {prefix}.{bare_name}` synthesized
/// alongside the real (prefixed) definition — see
/// `draft-ietf-cbor-cddl-modules-06.txt` §2.6 "import", lines ~437-471. A from-name
/// already written prefixed (e.g. `cose.foo`) produces no alias. Aliasing is
/// import-only: `include` directives never synthesize aliases, even with `as`.
fn bare_alias_rules(directive: &Directive, prefix: &str, directive_span: &Span) -> Vec<Rule> {
    if directive.kind != DirectiveKind::Import {
        return Vec::new();
    }
    let Some(FromSelector::Named(names)) = &directive.from else {
        return Vec::new();
    };
    names
        .iter()
        .filter(|raw_name| !raw_name.contains('.'))
        .map(|bare_name| {
            let prefixed = format!("{prefix}.{bare_name}");
            Rule {
                name: bare_name.clone(),
                generic_parm: None,
                assignment: Assignment::Assign,
                body: RuleBody::Type(Type {
                    choices: vec![Type1 {
                        base: Type2::Typename {
                            name: prefixed,
                            args: None,
                            span: directive_span.clone(),
                        },
                        constraint: None,
                        span: directive_span.clone(),
                    }],
                    span: directive_span.clone(),
                }),
                span: directive_span.clone(),
            }
        })
        .collect()
}

fn offset_span(span: &mut Span, offset: usize) {
    span.start += offset;
    span.end += offset;
}

/// Best-effort prefix-stripping for a `from`-clause name: if `prefix` is present and
/// `name` is written as `{prefix}.{rest}`, returns `rest`; otherwise returns `name`
/// unchanged. Never errors — both bare and pre-prefixed forms are valid per the
/// draft's worked examples (see `draft-ietf-cbor-cddl-modules-06.txt` §2.5/§2.6).
fn strip_prefix_if_matches(name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) => {
            let full = format!("{p}.");
            name.strip_prefix(&full).unwrap_or(name).to_string()
        }
        None => name.to_string(),
    }
}

/// Select which of `module_cddl`'s rules a single directive pulls in, per its kind
/// and `from` clause (see Step 9.5 in `PLAN.md`/the module design doc for the four
/// cases). `accumulated` is the importer's own rules merged so far, consulted only
/// for the implicit-import case (root inference by reference-scanning).
fn select_rules(
    directive: &Directive,
    directive_span: &Span,
    module_cddl: &Cddl,
    accumulated: &[Rule],
    constrain_targets: &[String],
) -> Result<Vec<Rule>, ModuleError> {
    let module_rule_names: HashSet<&str> =
        module_cddl.rules.iter().map(|r| r.name.as_str()).collect();

    match (directive.kind, &directive.from) {
        (DirectiveKind::Include, None | Some(FromSelector::All)) => {
            Ok(module_cddl.rules.clone())
        }
        (DirectiveKind::Include, Some(FromSelector::Named(names))) => {
            let mut out = Vec::new();
            for raw_name in names {
                let stripped = strip_prefix_if_matches(raw_name, directive.as_prefix.as_deref());
                let rule = module_cddl
                    .rules
                    .iter()
                    .find(|r| r.name == stripped)
                    .ok_or_else(|| ModuleError::UndefinedFromName {
                        name: raw_name.clone(),
                        module: directive.module.clone(),
                        span: directive_span.clone(),
                    })?;
                out.push(rule.clone());
            }
            Ok(out)
        }
        (DirectiveKind::Import, Some(FromSelector::Named(names))) => {
            let mut seen = HashSet::new();
            let mut order = Vec::new();
            for raw_name in names {
                let stripped = strip_prefix_if_matches(raw_name, directive.as_prefix.as_deref());
                if !module_rule_names.contains(stripped.as_str()) {
                    return Err(ModuleError::UndefinedFromName {
                        name: raw_name.clone(),
                        module: directive.module.clone(),
                        span: directive_span.clone(),
                    });
                }
                crate::gen_common::collect_deps(&stripped, module_cddl, &mut seen, &mut order);
            }
            Ok(module_cddl
                .rules
                .iter()
                .filter(|r| seen.contains(&r.name))
                .cloned()
                .collect())
        }
        (DirectiveKind::Import, None | Some(FromSelector::All)) => {
            let mut seen = HashSet::new();
            let mut order = Vec::new();
            let mut referenced = collect_referenced_names(accumulated);
            referenced.extend(constrain_targets.iter().cloned());
            for raw_name in &referenced {
                let stripped = strip_prefix_if_matches(raw_name, directive.as_prefix.as_deref());
                if module_rule_names.contains(stripped.as_str()) {
                    crate::gen_common::collect_deps(&stripped, module_cddl, &mut seen, &mut order);
                }
            }
            Ok(module_cddl
                .rules
                .iter()
                .filter(|r| seen.contains(&r.name))
                .cloned()
                .collect())
        }
    }
}

/// Every name referenced anywhere in `rules` (via `crate::gen_common::rule_dependencies`,
/// the same reference-collector used for codegen ordering). Not filtered against the
/// prelude — callers intersect against the target module's own rule names.
fn collect_referenced_names(rules: &[Rule]) -> HashSet<String> {
    let mut out = HashSet::new();
    for rule in rules {
        out.extend(crate::gen_common::rule_dependencies(rule));
    }
    out
}

// ─── Merge/rename AST transform ─────────────────────────────────────────────────

/// Rebase every [`Span`] in `rule` by `span_offset`, and — when `rename` is
/// `Some((local_names, prefix))` — rewrite every reference to a name in
/// `local_names` (including `rule.name` itself) to `"{prefix}.{name}"`.
///
/// A single combined pass keeps "every place a name can be referenced" defined in
/// exactly one place (see the node-by-node table in the module design doc). Prelude
/// names (`tstr`, `any`, …) are never in `local_names`, so they pass through
/// unrenamed automatically.
fn transform_rule(rule: &mut Rule, span_offset: usize, rename: Option<(&HashSet<String>, &str)>) {
    offset_span(&mut rule.span, span_offset);
    if let Some(gp) = &mut rule.generic_parm {
        offset_span(&mut gp.span, span_offset);
        // `gp.params` are locally-bound generic parameter names — never renamed.
    }
    rename_name(&mut rule.name, rename);
    match &mut rule.body {
        RuleBody::Type(ty) => transform_type(ty, span_offset, rename),
        RuleBody::Group(ent) => transform_grp_ent(ent, span_offset, rename),
    }
}

fn transform_type(ty: &mut Type, span_offset: usize, rename: Option<(&HashSet<String>, &str)>) {
    offset_span(&mut ty.span, span_offset);
    for t1 in &mut ty.choices {
        transform_type1(t1, span_offset, rename);
    }
}

fn transform_type1(t1: &mut Type1, span_offset: usize, rename: Option<(&HashSet<String>, &str)>) {
    offset_span(&mut t1.span, span_offset);
    transform_type2(&mut t1.base, span_offset, rename);
    if let Some(c) = &mut t1.constraint {
        transform_constraint(c, span_offset, rename);
    }
}

fn transform_constraint(
    c: &mut Constraint,
    span_offset: usize,
    rename: Option<(&HashSet<String>, &str)>,
) {
    offset_span(&mut c.span, span_offset);
    // `ConstraintOp::Control(name)` is the control operator's own name (e.g. `size`),
    // not a rule reference, so it is never renamed.
    let _: &ConstraintOp = &c.op;
    transform_type2(&mut c.target, span_offset, rename);
}

fn transform_type2(t2: &mut Type2, span_offset: usize, rename: Option<(&HashSet<String>, &str)>) {
    match t2 {
        Type2::Value(_) => {}
        Type2::Typename { name, args, span } | Type2::Unwrap { name, args, span } => {
            offset_span(span, span_offset);
            rename_name(name, rename);
            if let Some(a) = args {
                transform_generic_arg(a, span_offset, rename);
            }
        }
        Type2::EnumRef { name, args, span } => {
            offset_span(span, span_offset);
            rename_name(name, rename);
            if let Some(a) = args {
                transform_generic_arg(a, span_offset, rename);
            }
        }
        Type2::ParenType(inner) => transform_type(inner, span_offset, rename),
        Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => {
            transform_group(g, span_offset, rename)
        }
        Type2::Tagged { tag, inner, span } => {
            offset_span(span, span_offset);
            if let Some(HeadNumber::Type(t)) = tag {
                transform_type(t, span_offset, rename);
            }
            transform_type(inner, span_offset, rename);
        }
        Type2::MajorType { ai, span, .. } => {
            offset_span(span, span_offset);
            if let Some(HeadNumber::Type(t)) = ai {
                transform_type(t, span_offset, rename);
            }
        }
        Type2::Any(span) => offset_span(span, span_offset),
    }
}

fn transform_generic_arg(
    a: &mut GenericArg,
    span_offset: usize,
    rename: Option<(&HashSet<String>, &str)>,
) {
    offset_span(&mut a.span, span_offset);
    for t1 in &mut a.args {
        transform_type1(t1, span_offset, rename);
    }
}

fn transform_group(g: &mut Group, span_offset: usize, rename: Option<(&HashSet<String>, &str)>) {
    offset_span(&mut g.span, span_offset);
    for choice in &mut g.choices {
        transform_grp_choice(choice, span_offset, rename);
    }
}

fn transform_grp_choice(
    choice: &mut GrpChoice,
    span_offset: usize,
    rename: Option<(&HashSet<String>, &str)>,
) {
    offset_span(&mut choice.span, span_offset);
    for entry in &mut choice.entries {
        transform_grp_ent(entry, span_offset, rename);
    }
}

fn transform_grp_ent(
    entry: &mut GrpEnt,
    span_offset: usize,
    rename: Option<(&HashSet<String>, &str)>,
) {
    offset_span(&mut entry.span, span_offset);
    if let Some(key) = &mut entry.key {
        transform_member_key(key, span_offset, rename);
    }
    match &mut entry.entry {
        GrpEntBody::Type(ty) => transform_type(ty, span_offset, rename),
        GrpEntBody::GroupRef { name, args, span } => {
            offset_span(span, span_offset);
            rename_name(name, rename);
            if let Some(a) = args {
                transform_generic_arg(a, span_offset, rename);
            }
        }
        GrpEntBody::InlineGroup(g) => transform_group(g, span_offset, rename),
    }
}

fn transform_member_key(
    key: &mut MemberKey,
    span_offset: usize,
    rename: Option<(&HashSet<String>, &str)>,
) {
    match key {
        MemberKey::Type { key, span, .. } => {
            offset_span(span, span_offset);
            transform_type1(key, span_offset, rename);
        }
        // A bareword/literal map key is text, never a rule reference — offset only.
        MemberKey::Bareword { span, .. } => offset_span(span, span_offset),
        MemberKey::Value { span, .. } => offset_span(span, span_offset),
    }
}

fn rename_name(name: &mut String, rename: Option<(&HashSet<String>, &str)>) {
    if let Some((local_names, prefix)) = rename
        && local_names.contains(name.as_str())
    {
        *name = format!("{prefix}.{name}");
    }
}

// ─── `;# constrain` field replacement ──────────────────────────────────────────

/// Replace the field named `field` in `body`'s group (map or array — both share
/// the same [`Group`]/[`GrpChoice`]/[`GrpEnt`] shape) with `replacement`, searching
/// every group choice. Returns `true` if a matching field was found and replaced.
///
/// A [`RuleBody::Type`] whose sole `Type1` is a bare [`Type2::Map`]/[`Type2::Array`]
/// is unwrapped transparently, so this works for `Foo = { ... }` and `Foo = [ ... ]`
/// rules alike (COSE_Sign1 is array-shaped). A [`RuleBody::Group`] is checked as a
/// single [`GrpEnt`] whose own body is an [`GrpEntBody::InlineGroup`] — matches only
/// if that inline group contains the field.
fn replace_field_in_rule_body(body: &mut RuleBody, field: &str, replacement: Type1) -> bool {
    match body {
        RuleBody::Type(ty) => {
            for t1 in &mut ty.choices {
                if let Type2::Map(g) | Type2::Array(g) = &mut t1.base
                    && replace_field_in_group(g, field, replacement.clone())
                {
                    return true;
                }
            }
            false
        }
        RuleBody::Group(ent) => {
            if let GrpEntBody::InlineGroup(g) = &mut ent.entry {
                replace_field_in_group(g, field, replacement)
            } else {
                false
            }
        }
    }
}

/// Search every choice of `group` for a [`GrpEnt`] whose [`MemberKey`] text equals
/// `field`, replacing its `entry: GrpEntBody::Type(...)` with `replacement`. Matches
/// [`MemberKey::Bareword`] (e.g. `payload:`) and [`MemberKey::Value`] text keys the
/// same way, since both a map body and an array-with-labels body use the same
/// [`GrpEnt::key`] shape.
fn replace_field_in_group(group: &mut Group, field: &str, replacement: Type1) -> bool {
    for choice in &mut group.choices {
        for entry in &mut choice.entries {
            if member_key_matches(&entry.key, field) {
                entry.entry = GrpEntBody::Type(Type {
                    choices: vec![replacement],
                    span: entry.span.clone(),
                });
                return true;
            }
        }
    }
    false
}

fn member_key_matches(key: &Option<MemberKey>, field: &str) -> bool {
    match key {
        Some(MemberKey::Bareword { name, .. }) => name == field,
        Some(MemberKey::Value {
            key: Value::Text(s),
            ..
        }) => s == field,
        _ => false,
    }
}

/// Recursively stamp every [`Span`] in a freshly-parsed override [`Type1`] (parsed
/// against a throwaway synthetic document, so its spans are meaningless offsets
/// into a string that isn't part of `virtual_source`) to `span` — the `;# constrain`
/// directive line's own span, so every span in the merged [`Cddl`] stays within
/// `virtual_source`'s bounds.
fn stamp_span_type1(t1: &mut Type1, span: &Span) {
    t1.span = span.clone();
    stamp_span_type2(&mut t1.base, span);
    if let Some(c) = &mut t1.constraint {
        c.span = span.clone();
        stamp_span_type2(&mut c.target, span);
    }
}

fn stamp_span_type2(t2: &mut Type2, span: &Span) {
    match t2 {
        Type2::Value(_) => {}
        Type2::Typename { span: s, args, .. } | Type2::Unwrap { span: s, args, .. } => {
            *s = span.clone();
            if let Some(a) = args {
                stamp_span_generic_arg(a, span);
            }
        }
        Type2::EnumRef { span: s, args, .. } => {
            *s = span.clone();
            if let Some(a) = args {
                stamp_span_generic_arg(a, span);
            }
        }
        Type2::ParenType(inner) => stamp_span_type(inner, span),
        Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => stamp_span_group(g, span),
        Type2::Tagged { tag, inner, span: s } => {
            *s = span.clone();
            if let Some(HeadNumber::Type(t)) = tag {
                stamp_span_type(t, span);
            }
            stamp_span_type(inner, span);
        }
        Type2::MajorType { ai, span: s, .. } => {
            *s = span.clone();
            if let Some(HeadNumber::Type(t)) = ai {
                stamp_span_type(t, span);
            }
        }
        Type2::Any(s) => *s = span.clone(),
    }
}

fn stamp_span_type(ty: &mut Type, span: &Span) {
    ty.span = span.clone();
    for t1 in &mut ty.choices {
        stamp_span_type1(t1, span);
    }
}

fn stamp_span_generic_arg(a: &mut GenericArg, span: &Span) {
    a.span = span.clone();
    for t1 in &mut a.args {
        stamp_span_type1(t1, span);
    }
}

fn stamp_span_group(g: &mut Group, span: &Span) {
    g.span = span.clone();
    for choice in &mut g.choices {
        choice.span = span.clone();
        for entry in &mut choice.entries {
            stamp_span_grp_ent(entry, span);
        }
    }
}

fn stamp_span_grp_ent(entry: &mut GrpEnt, span: &Span) {
    entry.span = span.clone();
    if let Some(key) = &mut entry.key {
        stamp_span_member_key(key, span);
    }
    match &mut entry.entry {
        GrpEntBody::Type(ty) => stamp_span_type(ty, span),
        GrpEntBody::GroupRef { span: s, args, .. } => {
            *s = span.clone();
            if let Some(a) = args {
                stamp_span_generic_arg(a, span);
            }
        }
        GrpEntBody::InlineGroup(g) => stamp_span_group(g, span),
    }
}

fn stamp_span_member_key(key: &mut MemberKey, span: &Span) {
    match key {
        MemberKey::Type { key, span: s, .. } => {
            *s = span.clone();
            stamp_span_type1(key, span);
        }
        MemberKey::Bareword { span: s, .. } => *s = span.clone(),
        MemberKey::Value { span: s, .. } => *s = span.clone(),
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn one(source: &str) -> Directive {
        let (mut ds, starts, constrains, warnings) = scan_directives(source);
        assert!(
            warnings.is_empty(),
            "unexpected warnings: {warnings:?} (source: {source:?})"
        );
        assert!(starts.is_empty(), "unexpected start directives: {starts:?}");
        assert!(constrains.is_empty(), "unexpected constrain directives: {constrains:?}");
        assert_eq!(ds.len(), 1, "expected exactly one directive in {source:?}");
        ds.remove(0)
    }

    fn warnings_of(source: &str) -> Vec<String> {
        let (ds, starts, constrains, warnings) = scan_directives(source);
        assert!(
            ds.is_empty(),
            "expected no directives, got {ds:?} (source: {source:?})"
        );
        assert!(starts.is_empty(), "unexpected start directives: {starts:?}");
        assert!(constrains.is_empty(), "unexpected constrain directives: {constrains:?}");
        warnings.into_iter().map(|d| d.message).collect()
    }

    fn one_start(source: &str) -> StartDirective {
        let (ds, mut starts, constrains, warnings) = scan_directives(source);
        assert!(
            warnings.is_empty(),
            "unexpected warnings: {warnings:?} (source: {source:?})"
        );
        assert!(ds.is_empty(), "unexpected import/include directives: {ds:?}");
        assert!(constrains.is_empty(), "unexpected constrain directives: {constrains:?}");
        assert_eq!(starts.len(), 1, "expected exactly one start directive in {source:?}");
        starts.remove(0)
    }

    fn one_constrain(source: &str) -> ConstrainDirective {
        let (ds, starts, mut constrains, warnings) = scan_directives(source);
        assert!(
            warnings.is_empty(),
            "unexpected warnings: {warnings:?} (source: {source:?})"
        );
        assert!(ds.is_empty(), "unexpected import/include directives: {ds:?}");
        assert!(starts.is_empty(), "unexpected start directives: {starts:?}");
        assert_eq!(
            constrains.len(),
            1,
            "expected exactly one constrain directive in {source:?}"
        );
        constrains.remove(0)
    }


    // ── The six draft worked examples (draft §2.5/§2.6) ──────────────────────

    #[test]
    fn example1_plain_import() {
        let d = one(";# import rfc9052");
        assert_eq!(d.kind, DirectiveKind::Import);
        assert_eq!(d.from, None);
        assert_eq!(d.module, "rfc9052");
        assert_eq!(d.as_prefix, None);
    }

    #[test]
    fn example2_import_as_prefix() {
        let d = one(";# import rfc9052 as cose");
        assert_eq!(d.kind, DirectiveKind::Import);
        assert_eq!(d.from, None);
        assert_eq!(d.module, "rfc9052");
        assert_eq!(d.as_prefix, Some("cose".to_string()));
    }

    #[test]
    fn example3_include_explicit_from() {
        let d = one(";# include label, values from rfc9052");
        assert_eq!(d.kind, DirectiveKind::Include);
        assert_eq!(
            d.from,
            Some(FromSelector::Named(vec![
                "label".to_string(),
                "values".to_string()
            ]))
        );
        assert_eq!(d.module, "rfc9052");
        assert_eq!(d.as_prefix, None);
    }

    #[test]
    fn example4_include_from_as() {
        let d = one(";# include cose.label, cose.values from rfc9052 as cose");
        assert_eq!(d.kind, DirectiveKind::Include);
        assert_eq!(
            d.from,
            Some(FromSelector::Named(vec![
                "cose.label".to_string(),
                "cose.values".to_string()
            ]))
        );
        assert_eq!(d.module, "rfc9052");
        assert_eq!(d.as_prefix, Some("cose".to_string()));
    }

    #[test]
    fn example5_import_transitive_prefixed_from_name() {
        let d = one(";# import cose.empty_or_serialized_map from rfc9052 as cose");
        assert_eq!(d.kind, DirectiveKind::Import);
        assert_eq!(
            d.from,
            Some(FromSelector::Named(vec![
                "cose.empty_or_serialized_map".to_string()
            ]))
        );
        assert_eq!(d.as_prefix, Some("cose".to_string()));
    }

    #[test]
    fn example6_import_bare_from_name_alias() {
        let d = one(";# import empty_or_serialized_map from rfc9052 as cose");
        assert_eq!(d.kind, DirectiveKind::Import);
        assert_eq!(
            d.from,
            Some(FromSelector::Named(vec![
                "empty_or_serialized_map".to_string()
            ]))
        );
        assert_eq!(d.as_prefix, Some("cose".to_string()));
    }

    // ── Edge cases ─────────────────────────────────────────────────────────────

    #[test]
    fn no_space_after_marker_is_not_a_directive() {
        let w = warnings_of(";#import rfc9052");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("expected a space after `;#`"));
    }

    #[test]
    fn tab_makes_it_not_a_directive() {
        let w = warnings_of(";# import\trfc9052");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn unknown_keyword_warns() {
        let w = warnings_of(";# export rfc9052");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("unknown directive keyword"));
    }

    #[test]
    fn empty_from_list_warns() {
        let w = warnings_of(";# include from rfc9052");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn mixed_wildcard_and_names_becomes_all() {
        let d = one(";# import a, *, b from mod");
        assert_eq!(d.from, Some(FromSelector::All));
    }

    #[test]
    fn ordinary_comment_starting_with_hash_is_not_scanned() {
        let (ds, starts, constrains, warnings) = scan_directives(";#TODO fix this\n; a normal comment\nx = uint");
        assert!(warnings.len() == 1, "expected exactly one warning: {warnings:?}");
        assert!(ds.is_empty());
        assert!(starts.is_empty());
        assert!(constrains.is_empty());
    }

    #[test]
    fn plain_comments_elsewhere_are_unaffected() {
        let (ds, starts, constrains, warnings) = scan_directives("; just a comment\nx = uint ; trailing comment\n");
        assert!(ds.is_empty());
        assert!(starts.is_empty());
        assert!(constrains.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn hyphenated_name_rejected_in_from_clause() {
        let w = warnings_of(";# import encoded-cbor from rfc9052");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("invalid identifier"));
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        let (ds, starts, constrains, warnings) = scan_directives(";# import rfc9052\r\nstart = rfc9052.thing\r\n");
        assert!(warnings.is_empty());
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].module, "rfc9052");
        assert!(starts.is_empty());
        assert!(constrains.is_empty());
    }

    #[test]
    fn directive_without_trailing_newline_is_scanned() {
        let d = one(";# import rfc9052");
        assert_eq!(d.module, "rfc9052");
    }

    #[test]
    fn span_covers_whole_directive_line() {
        let source = "start = 1\n;# import rfc9052\nend = 2\n";
        let d = one(source);
        assert_eq!(&source[d.span.start..d.span.end], ";# import rfc9052");
    }

    // ── `;# start` (project-specific extension, not part of the draft) ─────────

    #[test]
    fn start_directive_plain_rule_name() {
        let d = one_start(";# start Claims-Set");
        assert_eq!(d.rule_name, "Claims-Set");
    }

    #[test]
    fn start_directive_dotted_hyphenated_rule_name() {
        let d = one_start(";# start rfc8392.Claims-Set");
        assert_eq!(d.rule_name, "rfc8392.Claims-Set");
    }

    #[test]
    fn start_directive_missing_rule_name_warns() {
        let w = warnings_of(";# start");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("expected a space after directive keyword"));
    }

    #[test]
    fn start_directive_extra_token_warns() {
        let w = warnings_of(";# start foo bar");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("exactly one rule name"));
    }

    #[test]
    fn start_directive_invalid_rule_name_warns() {
        let w = warnings_of(";# start 9bad");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("invalid rule name"));
    }

    #[test]
    fn start_directive_span_covers_whole_line() {
        let source = ";# start rfc8392.Claims-Set\nend = 2\n";
        let d = one_start(source);
        assert_eq!(&source[d.span.start..d.span.end], ";# start rfc8392.Claims-Set");
    }

    #[test]
    fn start_directive_coexists_with_import() {
        let (ds, starts, constrains, warnings) =
            scan_directives(";# import rfc9711\n;# start rfc8392.Claims-Set\n");
        assert!(warnings.is_empty());
        assert_eq!(ds.len(), 1);
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].rule_name, "rfc8392.Claims-Set");
        assert!(constrains.is_empty());
    }

    // ── Step 9.3: search path ───────────────────────────────────────────────────

    fn fixtures_dir(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/modules")
            .join(rel)
    }

    #[test]
    fn find_case_insensitive_matches_returns_all_case_variants() {
        let names = ["Foo.cddl", "FOO.CDDL", "bar.cddl", "foo.cddl"];
        let matches = find_case_insensitive_matches(names.into_iter(), "foo.cddl");
        assert_eq!(matches, vec!["Foo.cddl", "FOO.CDDL", "foo.cddl"]);
    }

    #[test]
    fn find_case_insensitive_matches_none() {
        let names = ["bar.cddl", "baz.cddl"];
        let matches = find_case_insensitive_matches(names.into_iter(), "foo.cddl");
        assert!(matches.is_empty());
    }

    #[test]
    fn find_case_insensitive_matches_single() {
        let names = ["Foo.cddl", "bar.cddl"];
        let matches = find_case_insensitive_matches(names.into_iter(), "foo.cddl");
        assert_eq!(matches, vec!["Foo.cddl"]);
    }

    #[test]
    fn resolve_finds_module_in_second_directory() {
        let search = ModuleSearchPath::new(vec![
            fixtures_dir("search_path/dir_a"),
            fixtures_dir("search_path/dir_b"),
        ]);
        let found = search
            .resolve("target", Span::new(0, 0))
            .expect("should find target.cddl in dir_b");
        assert_eq!(found, fixtures_dir("search_path/dir_b/target.cddl"));
    }

    #[test]
    fn resolve_not_found_lists_all_searched_dirs() {
        let dirs = vec![fixtures_dir("search_path/dir_a"), fixtures_dir("search_path/dir_b")];
        let search = ModuleSearchPath::new(dirs.clone());
        match search.resolve("nonexistent", Span::new(0, 0)) {
            Err(ModuleError::NotFound { module, searched, .. }) => {
                assert_eq!(module, "nonexistent");
                assert_eq!(searched, dirs);
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn resolve_skips_nonexistent_directory() {
        let search = ModuleSearchPath::new(vec![
            fixtures_dir("search_path/does_not_exist"),
            fixtures_dir("search_path/dir_b"),
        ]);
        let found = search
            .resolve("target", Span::new(0, 0))
            .expect("should skip the missing dir and find target.cddl in dir_b");
        assert_eq!(found, fixtures_dir("search_path/dir_b/target.cddl"));
    }

    #[test]
    fn build_search_dirs_orders_cwd_extra_then_env() {
        let cwd = PathBuf::from("/cwd");
        let extra = vec![PathBuf::from("/extra1"), PathBuf::from("/extra2")];
        let env = std::env::join_paths([PathBuf::from("/env1"), PathBuf::from("/env2")]).unwrap();
        let dirs = build_search_dirs(cwd.clone(), &extra, Some(env.as_os_str()));
        assert_eq!(
            dirs,
            vec![
                cwd,
                PathBuf::from("/extra1"),
                PathBuf::from("/extra2"),
                PathBuf::from("/env1"),
                PathBuf::from("/env2"),
            ]
        );
    }

    #[test]
    fn build_search_dirs_no_env_var() {
        let cwd = PathBuf::from("/cwd");
        let dirs = build_search_dirs(cwd.clone(), &[], None);
        assert_eq!(dirs, vec![cwd]);
    }

    // ── Step 9.4: recursive loader ──────────────────────────────────────────────

    fn resolve_fixture(rel_dir: &str, top_module: &str) -> Result<Cddl, ModuleError> {
        let dir = fixtures_dir(rel_dir);
        let search = ModuleSearchPath::new(vec![dir.clone()]);
        let mut ctx = LoaderCtx::new(search);
        resolve_module(&mut ctx, &dir.join(format!("{top_module}.cddl")), Span::new(0, 0))
    }

    #[test]
    fn loader_merges_two_independent_modules() {
        let cddl = resolve_fixture("loader/two_independent", "top").expect("should resolve");
        let names: Vec<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["top", "mod_x", "mod_y"]);
    }

    #[test]
    fn loader_follows_linear_chain() {
        let cddl = resolve_fixture("loader/linear_chain", "top").expect("should resolve");
        let names: Vec<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["top", "level_b", "level_c"]);
    }

    #[test]
    fn loader_detects_self_referencing_cycle() {
        match resolve_fixture("loader/cycle", "cycle_a") {
            Err(ModuleError::Circular { cycle, .. }) => {
                assert!(cycle.len() >= 2, "cycle should list at least 2 paths: {cycle:?}");
            }
            other => panic!("expected Circular, got {other:?}"),
        }
    }

    #[test]
    fn loader_resolves_diamond_without_infinite_recursion() {
        let cddl = resolve_fixture("loader/diamond", "top").expect("should resolve");
        let names: Vec<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        // Naive Step 9.4 merge does not yet dedupe; the diamond deliberately proves
        // recursion terminates via the cache, not that there are no duplicate rules
        // (real de-duplication-by-selection arrives in Step 9.5).
        assert!(names.contains(&"top"));
        assert!(names.contains(&"diamond_a"));
        assert!(names.contains(&"diamond_b"));
        assert!(names.iter().filter(|n| **n == "diamond_c").count() >= 1);
    }

    // ── `;# start` end-to-end via `resolve_modules` ─────────────────────────────

    fn resolve_modules_fixture(rel_dir: &str, top_module: &str) -> ModuleResolution {
        let dir = fixtures_dir(rel_dir);
        let search = ModuleSearchPath::new(vec![dir.clone()]);
        resolve_modules(&dir.join(format!("{top_module}.cddl")), &search).expect("should resolve")
    }

    #[test]
    fn resolve_modules_surfaces_topmost_start_rule_not_submodules() {
        let resolution = resolve_modules_fixture("start_rule/topmost_wins", "top");
        assert_eq!(resolution.start_rule, Some("top_rule".to_string()));
    }

    #[test]
    fn resolve_modules_start_rule_is_none_when_absent() {
        let resolution = resolve_modules_fixture("start_rule/no_start", "top");
        assert_eq!(resolution.start_rule, None);
    }

    // ── Step 9.5: per-directive-kind selection + merge/rename transform ────────
    // The six draft worked examples (draft-ietf-cbor-cddl-modules-06.txt §2.5/§2.6),
    // resolved end-to-end through the real loader.

    fn rule_named<'a>(cddl: &'a Cddl, name: &str) -> &'a Rule {
        cddl.rules
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("rule `{name}` not found; have: {:?}", cddl.rules.iter().map(|r| &r.name).collect::<Vec<_>>()))
    }

    #[test]
    fn ex1_plain_import_pulls_in_every_rule_unprefixed() {
        let cddl = resolve_fixture("ex1_plain_import", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        rule_named(&cddl, "start");
        rule_named(&cddl, "COSE_Key");
        rule_named(&cddl, "label");
        rule_named(&cddl, "values");
    }

    #[test]
    fn ex2_import_as_prefix_renames_every_pulled_in_rule() {
        let cddl = resolve_fixture("ex2_import_as_prefix", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let key = rule_named(&cddl, "cose.COSE_Key");
        assert!(cddl.rules.iter().all(|r| r.name != "COSE_Key"));
        rule_named(&cddl, "cose.label");
        rule_named(&cddl, "cose.values");

        // `* label => values` inside COSE_Key must have been rewritten to
        // `* cose.label => cose.values`.
        let RuleBody::Type(ty) = &key.body else {
            panic!("expected a type rule")
        };
        let Type2::Map(g) = &ty.choices[0].base else {
            panic!("expected a map")
        };
        let star_entry = g
            .choices[0]
            .entries
            .iter()
            .find(|e| {
                matches!(
                    &e.key,
                    Some(MemberKey::Type { key, .. }) if matches!(&key.base, Type2::Typename { .. })
                )
            })
            .expect("expected the `* label => values` entry");
        let Some(MemberKey::Type { key, .. }) = &star_entry.key else {
            unreachable!()
        };
        assert!(matches!(
            &key.base,
            Type2::Typename { name, .. } if name == "cose.label"
        ));
        let GrpEntBody::Type(entry_ty) = &star_entry.entry else {
            panic!("expected a type entry")
        };
        assert!(matches!(
            &entry_ty.choices[0].base,
            Type2::Typename { name, .. } if name == "cose.values"
        ));
    }

    #[test]
    fn ex3_include_explicit_from_pulls_in_only_named_rules() {
        let cddl = resolve_fixture("ex3_include_explicit_from", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let names: HashSet<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, HashSet::from(["mydata", "label", "values"]));
    }

    #[test]
    fn ex4_include_from_as_pulls_in_only_named_rules_prefixed() {
        let cddl = resolve_fixture("ex4_include_from_as", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let names: HashSet<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, HashSet::from(["mydata", "cose.label", "cose.values"]));
    }

    #[test]
    fn ex5_import_transitive_closure_prefixed() {
        let cddl = resolve_fixture("ex5_import_transitive", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let names: HashSet<&str> = cddl.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            HashSet::from([
                "mydata",
                "cose.empty_or_serialized_map",
                "cose.header_map",
                "cose.Generic_Headers",
                "cose.label",
                "cose.values",
            ])
        );
        // The from-name was written already-prefixed (`cose.empty_or_serialized_map`),
        // so no bare alias rule should be synthesized.
        assert!(cddl.rules.iter().all(|r| r.name != "empty_or_serialized_map"));
    }

    #[test]
    fn ex6_import_bare_from_name_pulls_in_transitive_closure_prefixed() {
        let cddl = resolve_fixture("ex6_import_alias", "mydata").expect("should resolve");
        let diags = crate::validate_with_config(&cddl, &crate::Config::strict());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        rule_named(&cddl, "cose.empty_or_serialized_map");
        rule_named(&cddl, "cose.header_map");
        rule_named(&cddl, "cose.Generic_Headers");
        rule_named(&cddl, "cose.label");
        rule_named(&cddl, "cose.values");

        // The from-name was written bare, so a top-level alias rule must exist.
        let alias = rule_named(&cddl, "empty_or_serialized_map");
        let RuleBody::Type(ty) = &alias.body else {
            panic!("expected a type rule")
        };
        assert!(matches!(
            &ty.choices[0].base,
            Type2::Typename { name, .. } if name == "cose.empty_or_serialized_map"
        ));
    }

    #[test]
    fn bare_alias_generalizes_to_multiple_from_names() {
        let directive = one(";# import a, b.c from mod as p");
        let span = directive.span.clone();
        let aliases = bare_alias_rules(&directive, "p", &span);
        let names: HashSet<&str> = aliases.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, HashSet::from(["a"]), "only the bare name `a` aliases; `b.c` is already prefixed");
    }

    #[test]
    fn bare_alias_not_synthesized_for_include_directive() {
        let directive = one(";# include a from mod as p");
        let span = directive.span.clone();
        let aliases = bare_alias_rules(&directive, "p", &span);
        assert!(aliases.is_empty(), "aliasing is import-only, per the draft");
    }

    // ── `;# constrain` (project-specific extension, not part of the draft) ─────

    #[test]
    fn constrain_directive_parses_target_new_name_and_field() {
        let d = one_constrain(
            ";# constrain cose.COSE_Sign1 as COSE_Sign1_EAT { payload: bstr .cborseq eap.Something }",
        );
        assert_eq!(d.target, "cose.COSE_Sign1");
        assert_eq!(d.new_name, "COSE_Sign1_EAT");
        assert_eq!(d.field_override.0, "payload");
        let Type2::Typename { name, .. } = &d.field_override.1.base else {
            panic!("expected a typename base, got {:?}", d.field_override.1.base);
        };
        assert_eq!(name, "bstr");
        let constraint = d.field_override.1.constraint.as_ref().expect("expected a constraint");
        assert_eq!(constraint.op, ConstraintOp::Control("cborseq".to_string()));
        let Type2::Typename { name, .. } = &constraint.target else {
            panic!("expected a typename constraint target, got {:?}", constraint.target);
        };
        assert_eq!(name, "eap.Something");
    }

    #[test]
    fn constrain_directive_simple_field_type() {
        let d = one_constrain(";# constrain Foo as Bar { x: uint }");
        assert_eq!(d.target, "Foo");
        assert_eq!(d.new_name, "Bar");
        assert_eq!(d.field_override.0, "x");
        assert!(matches!(&d.field_override.1.base, Type2::Typename { name, .. } if name == "uint"));
    }

    #[test]
    fn constrain_directive_missing_brace_clause_warns() {
        let w = warnings_of(";# constrain Foo as Bar");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("missing `{ field: type }` clause"));
    }

    #[test]
    fn constrain_directive_missing_as_clause_warns() {
        let w = warnings_of(";# constrain Foo Bar { x: uint }");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("expected `<target> as <new-name>`"));
    }

    #[test]
    fn constrain_directive_missing_colon_in_clause_warns() {
        let w = warnings_of(";# constrain Foo as Bar { x uint }");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("expected `field: type`"));
    }

    #[test]
    fn constrain_directive_invalid_type_expression_warns() {
        let w = warnings_of(";# constrain Foo as Bar { x: 1 2 3 }");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("invalid type expression"));
    }

    #[test]
    fn constrain_directive_span_covers_whole_line() {
        let source = ";# constrain Foo as Bar { x: uint }\nend = 2\n";
        let d = one_constrain(source);
        assert_eq!(
            &source[d.span.start..d.span.end],
            ";# constrain Foo as Bar { x: uint }"
        );
    }

    #[test]
    fn replace_field_in_group_replaces_array_labelled_entry() {
        let cddl = parse_test_cddl("Foo = [ a: uint, b: tstr ]");
        let mut body = cddl.rules[0].body.clone();
        let replacement = Type1 {
            base: Type2::Typename {
                name: "bool".to_string(),
                args: None,
                span: Span::new(0, 0),
            },
            constraint: None,
            span: Span::new(0, 0),
        };
        assert!(replace_field_in_rule_body(&mut body, "b", replacement));
        let RuleBody::Type(ty) = &body else { panic!("expected type rule") };
        let Type2::Array(g) = &ty.choices[0].base else { panic!("expected array") };
        let entry = &g.choices[0].entries[1];
        let GrpEntBody::Type(t) = &entry.entry else { panic!("expected type entry") };
        assert!(matches!(&t.choices[0].base, Type2::Typename { name, .. } if name == "bool"));
    }

    #[test]
    fn replace_field_in_group_returns_false_when_field_absent() {
        let cddl = parse_test_cddl("Foo = { a: uint }");
        let mut body = cddl.rules[0].body.clone();
        let replacement = Type1 {
            base: Type2::Typename {
                name: "bool".to_string(),
                args: None,
                span: Span::new(0, 0),
            },
            constraint: None,
            span: Span::new(0, 0),
        };
        assert!(!replace_field_in_rule_body(&mut body, "nope", replacement));
    }

    fn parse_test_cddl(source: &str) -> Cddl {
        let pairs = crate::lex(source).expect("lex failed");
        crate::build_cddl(pairs).expect("build failed")
    }

    fn resolve_fixture_modules(rel_dir: &str, top_module: &str) -> Result<ModuleResolution, ModuleError> {
        let dir = fixtures_dir(rel_dir);
        let search = ModuleSearchPath::new(vec![dir.clone()]);
        resolve_modules(&dir.join(format!("{top_module}.cddl")), &search)
    }

    #[test]
    fn constrain_end_to_end_replaces_field_without_mutating_original() {
        let res = resolve_fixture_modules("constrain_cose_sign1", "profile")
            .expect("should resolve");
        let constrained = rule_named(&res.cddl, "COSE_Sign1_EAT");
        let RuleBody::Type(ty) = &constrained.body else { panic!("expected type rule") };
        let Type2::Array(g) = &ty.choices[0].base else { panic!("expected array") };
        let payload_entry = g
            .choices[0]
            .entries
            .iter()
            .find(|e| member_key_matches(&e.key, "payload"))
            .expect("expected payload field");
        let GrpEntBody::Type(pt) = &payload_entry.entry else { panic!("expected type entry") };
        let base = &pt.choices[0].base;
        assert!(matches!(base, Type2::Typename { name, .. } if name == "bstr"));
        let constraint = pt.choices[0].constraint.as_ref().expect("expected .cborseq constraint");
        assert_eq!(constraint.op, ConstraintOp::Control("cborseq".to_string()));
        assert!(matches!(
            &constraint.target,
            Type2::Typename { name, .. } if name == "eap.Something"
        ));

        // Original rule must be untouched.
        let original = rule_named(&res.cddl, "cose.COSE_Sign1");
        let RuleBody::Type(orig_ty) = &original.body else { panic!("expected type rule") };
        let Type2::Array(orig_g) = &orig_ty.choices[0].base else { panic!("expected array") };
        let orig_payload = orig_g
            .choices[0]
            .entries
            .iter()
            .find(|e| member_key_matches(&e.key, "payload"))
            .expect("expected payload field");
        let GrpEntBody::Type(opt) = &orig_payload.entry else { panic!("expected type entry") };
        assert!(opt.choices[0].constraint.is_none(), "original COSE_Sign1.payload must remain unconstrained bstr/nil");
    }

    #[test]
    fn constrain_target_not_found_is_a_module_error() {
        match resolve_fixture_modules("constrain_cose_sign1", "missing_target") {
            Err(ModuleError::ConstrainTargetNotFound { target, .. }) => {
                assert_eq!(target, "cose.NoSuchRule");
            }
            other => panic!("expected ConstrainTargetNotFound, got {other:?}"),
        }
    }

    #[test]
    fn constrain_field_not_found_is_a_module_error() {
        match resolve_fixture_modules("constrain_cose_sign1", "missing_field") {
            Err(ModuleError::ConstrainFieldNotFound { target, field, .. }) => {
                assert_eq!(target, "cose.COSE_Sign1");
                assert_eq!(field, "no_such_field");
            }
            other => panic!("expected ConstrainFieldNotFound, got {other:?}"),
        }
    }
}
