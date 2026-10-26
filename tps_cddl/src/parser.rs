/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! AST builder: walks the pest CST (`Pairs`) and produces a typed [`Cddl`] AST.
//!
//! Each `build_*` function corresponds to one grammar production.

use pest::iterators::{Pair, Pairs};

use crate::ast::*;
use crate::error::ParseError;
use crate::grammar::Rule;

/// Cap on `build_type`/`build_type1`/`build_type2`/`build_group`/
/// `build_grpchoice`/`build_grpent`/`build_genericarg`/`build_memberkey`/
/// `build_head_number`'s mutual recursion depth (Stage 3 Finding 3/4).
///
/// These functions walk the pest CST built by a recursive-descent grammar
/// with no nesting ceiling of its own; a schema with deeply nested
/// parenthesized/bracketed types or deeply nested generic instantiations
/// can drive this AST builder to a stack overflow. Erroring past this depth
/// turns that abort into a diagnosable [`ParseError`].
const MAX_AST_DEPTH: u32 = 128;

/// Build a [`ParseError`] reporting that `span` exceeds [`MAX_AST_DEPTH`].
fn depth_error(span: pest::Span<'_>) -> ParseError {
    Box::new(pest::error::Error::<Rule>::new_from_span(
        pest::error::ErrorVariant::CustomError {
            message: format!(
                "type/group nesting exceeds the maximum supported depth ({MAX_AST_DEPTH})"
            ),
        },
        span,
    ))
    .into()
}

/// Returns `Err` once `depth` reaches [`MAX_AST_DEPTH`], else `Ok(())`.
fn check_depth(depth: u32, span: pest::Span<'_>) -> Result<(), ParseError> {
    if depth >= MAX_AST_DEPTH {
        Err(depth_error(span))
    } else {
        Ok(())
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Build a [`Cddl`] AST from a pest `Pairs` stream rooted at `Rule::cddl`.
///
/// Call [`crate::lex`] first to obtain the `Pairs` stream. Returns a
/// [`ParseError`] if the source contains an integer literal (a `uint`, a
/// signed `int`, an occurrence bound, or a tag/major-type annotation) that
/// does not fit in 64 bits — CDDL's grammar places no magnitude limit on
/// integer literals, so this is a semantic check rather than a syntax error
/// caught by [`crate::lex`]. Panics on other malformed input (i.e. input
/// that passed the grammar but contains unexpected structure) — this should
/// not happen if `pairs` came from a successful [`crate::lex`] call.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl};
///
/// let pairs = lex("age = uint").expect("parse error");
/// let cddl = build_cddl(pairs).expect("build error");
/// assert_eq!(cddl.rules.len(), 1);
/// assert_eq!(cddl.rules[0].name, "age");
/// ```
pub fn build_cddl(pairs: Pairs<'_, Rule>) -> Result<Cddl, ParseError> {
    let mut rules = Vec::new();
    let mut span = Span::new(0, 0);

    for pair in pairs {
        match pair.as_rule() {
            Rule::cddl => {
                span = pair.as_span().into();
                for inner in pair.into_inner() {
                    if inner.as_rule() == Rule::rule {
                        rules.push(build_rule(inner)?);
                    }
                }
            }
            Rule::EOI => {}
            _ => {}
        }
    }

    Ok(Cddl { rules, span })
}

// ─── Rule ─────────────────────────────────────────────────────────────────────

fn build_rule(pair: Pair<'_, Rule>) -> Result<crate::ast::Rule, ParseError> {
    let span: Span = pair.as_span().into();
    let pairs: Vec<_> = pair.into_inner().collect();
    let mut i = 0;

    // First pair: typename or groupname (both wrap `id`)
    let name = pairs[i]
        .clone()
        .into_inner()
        .next()
        .expect("rule: missing id in name")
        .as_str()
        .to_owned();
    i += 1;

    // Optional genericparm
    let mut generic_parm: Option<GenericParm> = None;
    if i < pairs.len() && pairs[i].as_rule() == Rule::genericparm {
        generic_parm = Some(build_genericparm(pairs[i].clone()));
        i += 1;
    }

    // Assignment operator
    let (assignment, is_type_rule) = match pairs[i].as_rule() {
        Rule::assignt => {
            let a = match pairs[i].as_str() {
                "=" => Assignment::Assign,
                "/=" => Assignment::TypeExtend,
                _ => unreachable!(),
            };
            i += 1;
            (a, true)
        }
        Rule::assigng => {
            let a = match pairs[i].as_str() {
                "=" => Assignment::Assign,
                "//=" => Assignment::GroupExtend,
                _ => unreachable!(),
            };
            i += 1;
            (a, false)
        }
        r => panic!("rule: unexpected assignment token {r:?}"),
    };

    let body = if is_type_rule {
        RuleBody::Type(build_type(pairs[i].clone(), 0)?)
    } else {
        RuleBody::Group(build_grpent(pairs[i].clone(), 0)?)
    };

    Ok(crate::ast::Rule {
        name,
        generic_parm,
        assignment,
        body,
        span,
    })
}

// ─── Generic parameters / arguments ──────────────────────────────────────────

fn build_genericparm(pair: Pair<'_, Rule>) -> GenericParm {
    let span: Span = pair.as_span().into();
    let params = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::id)
        .map(|p| p.as_str().to_owned())
        .collect();
    GenericParm { params, span }
}

fn build_genericarg(pair: Pair<'_, Rule>, depth: u32) -> Result<GenericArg, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let args = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type1)
        .map(|p| build_type1(p, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GenericArg { args, span })
}

// ─── Type expressions ─────────────────────────────────────────────────────────

fn build_type(pair: Pair<'_, Rule>, depth: u32) -> Result<Type, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let choices = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type1)
        .map(|p| build_type1(p, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Type { choices, span })
}

fn build_type1(pair: Pair<'_, Rule>, depth: u32) -> Result<Type1, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let pairs: Vec<_> = pair.into_inner().collect();

    let base = build_type2(pairs[0].clone(), depth + 1)?;

    let constraint = if pairs.len() >= 3 {
        let op_pair = &pairs[1];
        let rhs_pair = &pairs[2];
        let op = match op_pair.as_rule() {
            Rule::rangeop => match op_pair.as_str() {
                ".." => ConstraintOp::RangeInclusive,
                "..." => ConstraintOp::RangeExclusive,
                _ => unreachable!(),
            },
            Rule::ctlop => {
                // ctlop text is ".name" — strip the leading dot
                ConstraintOp::Control(op_pair.as_str()[1..].to_owned())
            }
            r => panic!("type1: unexpected op rule {r:?}"),
        };
        let op_span: Span = op_pair.as_span().into();
        let rhs_span: Span = rhs_pair.as_span().into();
        let target = build_type2(rhs_pair.clone(), depth + 1)?;
        Some(Constraint {
            op,
            target,
            span: Span::new(op_span.start, rhs_span.end),
        })
    } else {
        None
    };

    Ok(Type1 {
        base,
        constraint,
        span,
    })
}

/// Dispatch on the first inner rule of `type2`, using `as_str()` only where
/// the same inner rule can arise from multiple grammar alternatives.
fn build_type2(pair: Pair<'_, Rule>, depth: u32) -> Result<Type2, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let src = pair.as_str();
    let trimmed = src.trim_start();

    let mut pairs: Vec<_> = pair.into_inner().collect();
    let first_rule = pairs.first().map(|p| p.as_rule());

    Ok(match first_rule {
        // ── value literal ────────────────────────────────────────────────────
        Some(Rule::value) => Type2::Value(build_value(pairs.remove(0))?),

        // ── "(" type ")" — parenthesised type ────────────────────────────────
        // Also catches #6(type) where there is no uint before the type_;
        // distinguish by source text.
        Some(Rule::type_) => {
            let t = build_type(pairs.remove(0), depth + 1)?;
            if trimmed.starts_with('#') {
                Type2::Tagged {
                    tag: None,
                    inner: Box::new(t),
                    span,
                }
            } else {
                Type2::ParenType(Box::new(t))
            }
        }

        // ── typename or "~" typename ─────────────────────────────────────────
        Some(Rule::typename) => {
            let name = pairs
                .remove(0)
                .into_inner()
                .next()
                .expect("typename: missing id")
                .as_str()
                .to_owned();
            let args = if pairs.first().map(|p| p.as_rule()) == Some(Rule::genericarg) {
                Some(build_genericarg(pairs.remove(0), depth + 1)?)
            } else {
                None
            };
            if trimmed.starts_with('~') {
                Type2::Unwrap { name, args, span }
            } else {
                Type2::Typename { name, args, span }
            }
        }

        // ── "& groupname" — enum from named group ────────────────────────────
        Some(Rule::groupname) => {
            let name = pairs
                .remove(0)
                .into_inner()
                .next()
                .expect("groupname: missing id")
                .as_str()
                .to_owned();
            let args = if pairs.first().map(|p| p.as_rule()) == Some(Rule::genericarg) {
                Some(build_genericarg(pairs.remove(0), depth + 1)?)
            } else {
                None
            };
            Type2::EnumRef { name, args, span }
        }

        // ── group: "{group}", "[group]", or "&(group)" ────────────────────────
        Some(Rule::group) => {
            let g = build_group(pairs.remove(0), depth + 1)?;
            if trimmed.starts_with('{') {
                Type2::Map(g)
            } else if trimmed.starts_with('[') {
                Type2::Array(g)
            } else {
                // & ( group )
                Type2::EnumGroup(g)
            }
        }

        // ── head_number present: #6.head(type), #7.head, or #M.uint ─────────
        Some(Rule::head_number) => {
            let head = build_head_number(pairs.remove(0), depth + 1)?;
            if trimmed.starts_with("#6") {
                let t = build_type(pairs.remove(0), depth + 1)?;
                Type2::Tagged {
                    tag: Some(head),
                    inner: Box::new(t),
                    span,
                }
            } else {
                let major = trimmed
                    .chars()
                    .nth(1)
                    .and_then(|c| c.to_digit(10))
                    .expect("major type: missing digit") as u8;
                Type2::MajorType {
                    major,
                    ai: Some(head),
                    span,
                }
            }
        }

        // ── uint directly: #M.uint for non-6/7 major types ───────────────────
        Some(Rule::uint) => {
            let uint_pair = pairs.remove(0);
            let uint_span = uint_pair.as_span();
            let ai_val = parse_uint_str(uint_pair.as_str(), uint_span)?;
            if trimmed.starts_with("#6") {
                let t = build_type(pairs.remove(0), depth + 1)?;
                Type2::Tagged {
                    tag: Some(HeadNumber::Literal(ai_val)),
                    inner: Box::new(t),
                    span,
                }
            } else {
                let major = trimmed
                    .chars()
                    .nth(1)
                    .and_then(|c| c.to_digit(10))
                    .expect("major type: missing digit") as u8;
                Type2::MajorType {
                    major,
                    ai: Some(HeadNumber::Literal(ai_val)),
                    span,
                }
            }
        }

        // ── no inner pairs: bare "#" or "#M" (major with no ai) ──────────────
        None => {
            if trimmed == "#" {
                Type2::Any(span)
            } else {
                let major = trimmed
                    .chars()
                    .nth(1)
                    .and_then(|c| c.to_digit(10))
                    .expect("major type: missing digit") as u8;
                Type2::MajorType {
                    major,
                    ai: None,
                    span,
                }
            }
        }

        Some(r) => panic!("type2: unexpected first inner rule {r:?} in {trimmed:?}"),
    })
}

// ─── Head number ──────────────────────────────────────────────────────────────

fn build_head_number(pair: Pair<'_, Rule>, depth: u32) -> Result<HeadNumber, ParseError> {
    let mut inner: Vec<_> = pair.into_inner().collect();
    match inner.first().map(|p| p.as_rule()) {
        Some(Rule::uint) => {
            let uint_pair = inner.remove(0);
            let uint_span = uint_pair.as_span();
            Ok(HeadNumber::Literal(parse_uint_str(
                uint_pair.as_str(),
                uint_span,
            )?))
        }
        Some(Rule::type_) => Ok(HeadNumber::Type(Box::new(build_type(
            inner.remove(0),
            depth + 1,
        )?))),
        _ => panic!("head_number: unexpected inner rule"),
    }
}

// ─── Groups ───────────────────────────────────────────────────────────────────

fn build_group(pair: Pair<'_, Rule>, depth: u32) -> Result<Group, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let choices = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::grpchoice)
        .map(|p| build_grpchoice(p, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Group { choices, span })
}

fn build_grpchoice(pair: Pair<'_, Rule>, depth: u32) -> Result<GrpChoice, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let entries = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::grpent)
        .map(|p| build_grpent(p, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GrpChoice { entries, span })
}

fn build_grpent(pair: Pair<'_, Rule>, depth: u32) -> Result<GrpEnt, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let pairs: Vec<_> = pair.into_inner().collect();
    let mut i = 0;

    // Optional occur
    let occur = if i < pairs.len() && pairs[i].as_rule() == Rule::occur {
        let o = build_occur(&pairs[i])?;
        i += 1;
        Some(o)
    } else {
        None
    };

    // Optional memberkey
    let key = if i < pairs.len() && pairs[i].as_rule() == Rule::memberkey {
        let k = build_memberkey(pairs[i].clone(), depth + 1)?;
        i += 1;
        Some(k)
    } else {
        None
    };

    // Body
    let entry = if i >= pairs.len() {
        GrpEntBody::InlineGroup(Group {
            choices: vec![],
            span: span.clone(),
        })
    } else {
        match pairs[i].as_rule() {
            Rule::type_ => GrpEntBody::Type(build_type(pairs[i].clone(), depth + 1)?),
            Rule::group => GrpEntBody::InlineGroup(build_group(pairs[i].clone(), depth + 1)?),
            Rule::groupname => {
                let grp_span: Span = pairs[i].as_span().into();
                let name = pairs[i]
                    .clone()
                    .into_inner()
                    .next()
                    .expect("grpent: groupname missing id")
                    .as_str()
                    .to_owned();
                let args = match pairs.get(i + 1).filter(|p| p.as_rule() == Rule::genericarg) {
                    Some(p) => Some(build_genericarg(p.clone(), depth + 1)?),
                    None => None,
                };
                GrpEntBody::GroupRef {
                    name,
                    args,
                    span: grp_span,
                }
            }
            r => panic!("grpent: unexpected body rule {r:?}"),
        }
    };

    Ok(GrpEnt {
        occur,
        key,
        entry,
        span,
    })
}

fn build_memberkey(pair: Pair<'_, Rule>, depth: u32) -> Result<MemberKey, ParseError> {
    let span: Span = pair.as_span().into();
    check_depth(depth, pair.as_span())?;
    let src = pair.as_str();
    let pairs: Vec<_> = pair.into_inner().collect();

    if src.trim_end().ends_with("=>") {
        let t1 = build_type1(pairs[0].clone(), depth + 1)?;
        let cut = src.contains('^');
        Ok(MemberKey::Type {
            key: Box::new(t1),
            cut,
            span,
        })
    } else {
        // ends with ":"
        match pairs[0].as_rule() {
            Rule::bareword => {
                let name = pairs[0]
                    .clone()
                    .into_inner()
                    .next()
                    .expect("bareword: missing id")
                    .as_str()
                    .to_owned();
                Ok(MemberKey::Bareword { name, span })
            }
            Rule::value => {
                let val = build_value(pairs[0].clone())?;
                Ok(MemberKey::Value { key: val, span })
            }
            r => panic!("memberkey: unexpected first child {r:?}"),
        }
    }
}

// ─── Occurrence ───────────────────────────────────────────────────────────────

fn build_occur(pair: &Pair<'_, Rule>) -> Result<Occur, ParseError> {
    let s = pair.as_str().trim();
    match s {
        "?" => Ok(Occur::Optional),
        "+" => Ok(Occur::OneOrMore),
        _ => {
            let star = s.find('*').expect("occur: no '*'");
            let lower_s = s[..star].trim();
            let upper_s = s[star + 1..].trim();
            let span = pair.as_span();
            let lower = if lower_s.is_empty() {
                None
            } else {
                Some(parse_uint_str(lower_s, span)?)
            };
            let upper = if upper_s.is_empty() {
                None
            } else {
                Some(parse_uint_str(upper_s, span)?)
            };
            Ok(Occur::Bounded { lower, upper })
        }
    }
}

// ─── Values ───────────────────────────────────────────────────────────────────

fn build_value(pair: Pair<'_, Rule>) -> Result<Value, ParseError> {
    let inner = pair.into_inner().next().expect("value: empty");
    match inner.as_rule() {
        Rule::number => build_number(inner),
        Rule::text => Ok(build_text(inner)),
        Rule::bytes => Ok(build_bytes(inner)),
        r => panic!("value: unexpected rule {r:?}"),
    }
}

fn build_number(pair: Pair<'_, Rule>) -> Result<Value, ParseError> {
    let s = pair.as_str();
    let span = pair.as_span();
    // hexfloat: starts with optional '-' then '0x'/'0X' and contains 'p'/'P'
    let lo = s.to_lowercase();
    if (lo.starts_with("0x") || lo.starts_with("-0x")) && (lo.contains('p')) {
        Ok(Value::Float(parse_hexfloat(s)))
    } else if s.contains('.') || (s.to_ascii_lowercase().contains('e') && !lo.starts_with("0x")) {
        Ok(Value::Float(s.parse::<f64>().unwrap_or(f64::NAN)))
    } else {
        Ok(Value::Int(parse_int_str(s, span)?))
    }
}

/// Parse a C99-style hexadecimal float literal (e.g. `0x1.8p+1`).
fn parse_hexfloat(s: &str) -> f64 {
    let s = s.trim();
    let (neg, rest) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else {
        (false, s)
    };
    // Strip "0x" or "0X"
    let rest = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .unwrap_or(rest);
    // Split on 'p' or 'P'
    let (mantissa_str, exp_str) = if let Some(p) = rest.to_lowercase().find('p') {
        (&rest[..p], &rest[p + 1..])
    } else {
        return f64::NAN;
    };
    // Parse mantissa: integer part + optional fractional part
    let (int_part, frac_part) = if let Some(dot) = mantissa_str.find('.') {
        (&mantissa_str[..dot], &mantissa_str[dot + 1..])
    } else {
        (mantissa_str, "")
    };
    let int_val = if int_part.is_empty() {
        0u64
    } else {
        u64::from_str_radix(int_part, 16).unwrap_or(0)
    };
    let mut mantissa = int_val as f64;
    let mut frac_scale = 1.0_f64 / 16.0;
    for c in frac_part.chars() {
        if let Some(d) = c.to_digit(16) {
            mantissa += d as f64 * frac_scale;
            frac_scale /= 16.0;
        }
    }
    // Parse binary exponent
    let exp: i32 = exp_str.parse().unwrap_or(0);
    let value = mantissa * (2.0_f64).powi(exp);
    if neg { -value } else { value }
}

fn build_text(pair: Pair<'_, Rule>) -> Value {
    let raw = pair.as_str();
    let inner = &raw[1..raw.len() - 1];
    Value::Text(unescape_text(inner))
}

fn build_bytes(pair: Pair<'_, Rule>) -> Value {
    let raw = pair.as_str();
    if raw.starts_with("h'") {
        let hex_content = &raw[2..raw.len() - 1];
        Value::Bytes(ByteStr {
            encoding: ByteEncoding::Hex,
            bytes: decode_hex_lenient(hex_content),
        })
    } else if raw.starts_with("b64'") {
        let b64_content = &raw[4..raw.len() - 1];
        Value::Bytes(ByteStr {
            encoding: ByteEncoding::Base64,
            bytes: decode_base64_lenient(b64_content),
        })
    } else {
        // raw: strip surrounding single quotes
        let inner = &raw[1..raw.len() - 1];
        Value::Bytes(ByteStr {
            encoding: ByteEncoding::Raw,
            bytes: inner.as_bytes().to_vec(),
        })
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a [`ParseError`] pointing at `span`, e.g. for an integer literal
/// that is syntactically valid CDDL but does not fit in the AST's 64-bit
/// integer representation.
fn overflow_error(span: pest::Span<'_>, message: String) -> ParseError {
    Box::new(pest::error::Error::<Rule>::new_from_span(
        pest::error::ErrorVariant::CustomError { message },
        span,
    ))
    .into()
}

/// Parse a `uint` string (decimal, `0x` hex, or `0b` binary) to `u64`.
/// `span` locates `s` in the original source, for diagnostics if `s` is out
/// of range.
fn parse_uint_str(s: &str, span: pest::Span<'_>) -> Result<u64, ParseError> {
    let s = s.trim();
    let result = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16)
    } else if let Some(b) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u64::from_str_radix(b, 2)
    } else {
        s.parse::<u64>()
    };
    result.map_err(|_| {
        overflow_error(
            span,
            format!("integer literal `{s}` does not fit in a 64-bit unsigned integer"),
        )
    })
}

/// Parse a signed int string (optional leading `-`, then uint). Computes the
/// negated magnitude directly rather than via `-(x as i64)`, since that cast
/// would overflow for `x == i64::MIN`'s unsigned magnitude
/// (9223372036854775808).
fn parse_int_str(s: &str, span: pest::Span<'_>) -> Result<i64, ParseError> {
    let s = s.trim();
    if let Some(pos) = s.strip_prefix('-') {
        let magnitude = parse_uint_str(pos, span)?;
        if magnitude == i64::MIN.unsigned_abs() {
            Ok(i64::MIN)
        } else {
            i64::try_from(magnitude).map(|v| -v).map_err(|_| {
                overflow_error(
                    span,
                    format!("integer literal `{s}` does not fit in a 64-bit signed integer"),
                )
            })
        }
    } else {
        i64::try_from(parse_uint_str(s, span)?).map_err(|_| {
            overflow_error(
                span,
                format!("integer literal `{s}` does not fit in a 64-bit signed integer"),
            )
        })
    }
}

/// Unescape a CDDL text string body (content between `"`…`"`).
/// Implements RFC 9682 §2.1.1 SESC: \n \r \t \b \f \\ \" \/ \uXXXX \u{hex}.
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            None => {}
            Some('"') => out.push('"'),
            Some('/') => out.push('/'),
            Some('\\') => out.push('\\'),
            Some('b') => out.push('\x08'),
            Some('f') => out.push('\x0C'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut hex = String::new();
                    while let Some(&h) = chars.peek() {
                        if h == '}' {
                            chars.next();
                            break;
                        }
                        hex.push(chars.next().unwrap());
                    }
                    if let Ok(n) = u32::from_str_radix(&hex, 16)
                        && let Some(ch) = char::from_u32(n)
                    {
                        out.push(ch);
                    }
                } else {
                    // \uXXXX — consume 4 hex digits
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    if let Ok(n) = u16::from_str_radix(&hex, 16) {
                        // Check for surrogate pair
                        if (0xD800..=0xDBFF).contains(&n) {
                            // High surrogate — expect \uDCXX
                            if chars.next() == Some('\\') && chars.next() == Some('u') {
                                let hex2: String = (0..4).filter_map(|_| chars.next()).collect();
                                if let Ok(low) = u16::from_str_radix(&hex2, 16)
                                    && (0xDC00..=0xDFFF).contains(&low)
                                {
                                    let codepoint = 0x10000u32
                                        + ((n as u32 - 0xD800) << 10)
                                        + (low as u32 - 0xDC00);
                                    if let Some(ch) = char::from_u32(codepoint) {
                                        out.push(ch);
                                    }
                                }
                            }
                        } else if let Some(ch) = char::from_u32(n as u32) {
                            out.push(ch);
                        }
                    }
                }
            }
            Some(other) => out.push(other), // fallback: keep char as-is
        }
    }
    out
}

/// Decode hex content from `h'...'`, ignoring whitespace.
fn decode_hex_lenient(s: &str) -> Vec<u8> {
    let digits: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..digits.len())
        .step_by(2)
        .filter_map(|i| {
            let chunk = digits.get(i..i + 2)?;
            u8::from_str_radix(chunk, 16).ok()
        })
        .collect()
}

/// Decode base64 content from `b64'...'`, ignoring whitespace.
/// Handles both standard base64 (`+/`) and base64url (`-_`) per RFC 8610 §3.1.
fn decode_base64_lenient(s: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [0xffu8; 256];
    for (i, &b) in TABLE.iter().enumerate() {
        lookup[b as usize] = i as u8;
    }
    // base64url aliases: '-' → '+' (62), '_' → '/' (63)
    lookup[b'-' as usize] = 62;
    lookup[b'_' as usize] = 63;
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace() && b != b'=')
        .collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let b: Vec<u8> = chunk.iter().map(|&b| lookup[b as usize]).collect();
        if b.len() >= 2 {
            out.push((b[0] << 2) | (b[1] >> 4));
        }
        if b.len() >= 3 {
            out.push((b[1] << 4) | (b[2] >> 2));
        }
        if b.len() >= 4 {
            out.push((b[2] << 6) | b[3]);
        }
    }
    out
}

// ─── Span helper on Type2 ─────────────────────────────────────────────────────

impl Type2 {
    /// Return the span of this type2 node.
    ///
    /// `Type2::Value` carries its span in the parent `Type1`; calling `.span()`
    /// on it returns a zero-length placeholder.  Callers that need the
    /// value-literal span should use the enclosing `Type1::span` instead.
    pub fn span(&self) -> &Span {
        match self {
            Type2::Value(_) => &Span { start: 0, end: 0 },
            Type2::Typename { span, .. }
            | Type2::Unwrap { span, .. }
            | Type2::EnumRef { span, .. }
            | Type2::Tagged { span, .. }
            | Type2::MajorType { span, .. }
            | Type2::Any(span) => span,
            Type2::ParenType(t) => &t.span,
            Type2::Map(g) | Type2::Array(g) | Type2::EnumGroup(g) => &g.span,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse(src: &str) -> Cddl {
        let pairs = lex(src).unwrap_or_else(|e| panic!("lex failed:\n{e}"));
        build_cddl(pairs).unwrap_or_else(|e| panic!("build failed:\n{e}"))
    }

    #[test]
    fn simple_type_rule() {
        let cddl = parse("age = uint");
        assert_eq!(cddl.rules.len(), 1);
        let rule = &cddl.rules[0];
        assert_eq!(rule.name, "age");
        assert_eq!(rule.assignment, Assignment::Assign);
        assert!(matches!(rule.body, RuleBody::Type(_)));
    }

    #[test]
    fn type_extend() {
        let cddl = parse("color = \"red\"\ncolor /= \"blue\"");
        assert_eq!(cddl.rules.len(), 2);
        assert_eq!(cddl.rules[1].assignment, Assignment::TypeExtend);
    }

    #[test]
    fn group_extend() {
        let cddl = parse("delivery = (city: tstr)\ndelivery //= (zip: uint)");
        assert_eq!(cddl.rules.len(), 2);
        assert_eq!(cddl.rules[1].assignment, Assignment::GroupExtend);
    }

    #[test]
    fn map_with_keys() {
        let cddl = parse("person = { name: tstr, age: uint }");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            let t2 = &t.choices[0].base;
            assert!(matches!(t2, Type2::Map(_)));
        } else {
            panic!("expected type rule");
        }
    }

    #[test]
    fn array_type() {
        let cddl = parse("point = [x: float, y: float]");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            assert!(matches!(t.choices[0].base, Type2::Array(_)));
        }
    }

    #[test]
    fn type_union_choices() {
        let cddl = parse("shape = \"circle\" / \"square\"");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            assert_eq!(t.choices.len(), 2);
        }
    }

    #[test]
    fn range_inclusive() {
        let cddl = parse("byte = 0..255");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            let constraint = t.choices[0].constraint.as_ref().unwrap();
            assert_eq!(constraint.op, ConstraintOp::RangeInclusive);
        }
    }

    #[test]
    fn range_exclusive() {
        let cddl = parse("short = 0...256");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            let constraint = t.choices[0].constraint.as_ref().unwrap();
            assert_eq!(constraint.op, ConstraintOp::RangeExclusive);
        }
    }

    #[test]
    fn control_operator() {
        let cddl = parse("bounded = tstr .size (1..10)");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            let constraint = t.choices[0].constraint.as_ref().unwrap();
            assert_eq!(constraint.op, ConstraintOp::Control("size".to_owned()));
        }
    }

    #[test]
    fn any_type() {
        let cddl = parse("anything = #");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            assert!(matches!(t.choices[0].base, Type2::Any(_)));
        }
    }

    #[test]
    fn tagged_with_tag() {
        let cddl = parse("ts = #6.1(uint)");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            if let Type2::Tagged { tag, .. } = &t.choices[0].base {
                assert_eq!(*tag, Some(HeadNumber::Literal(1u64)));
            } else {
                panic!("expected Tagged");
            }
        }
    }

    #[test]
    fn major_type() {
        let cddl = parse("raw = #2");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            if let Type2::MajorType { major, ai, .. } = &t.choices[0].base {
                assert_eq!(*major, 2);
                assert_eq!(*ai, None);
            } else {
                panic!("expected MajorType");
            }
        }
    }

    #[test]
    fn occurrence_optional() {
        let cddl = parse("record = { name: tstr, ? age: uint }");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body
            && let Type2::Map(g) = &t.choices[0].base
        {
            let second = &g.choices[0].entries[1];
            assert_eq!(second.occur, Some(Occur::Optional));
        }
    }

    #[test]
    fn generic_rule() {
        let cddl = parse("pair<a, b> = [a, b]");
        let rule = &cddl.rules[0];
        assert_eq!(rule.name, "pair");
        let gp = rule.generic_parm.as_ref().unwrap();
        assert_eq!(gp.params, vec!["a", "b"]);
    }

    #[test]
    fn text_value() {
        let cddl = parse(r#"greeting = "hello""#);
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            if let Type2::Value(Value::Text(s)) = &t.choices[0].base {
                assert_eq!(s, "hello");
            } else {
                panic!("expected Text value");
            }
        }
    }

    #[test]
    fn int_value() {
        let cddl = parse("port = 443");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            if let Type2::Value(Value::Int(n)) = &t.choices[0].base {
                assert_eq!(*n, 443);
            } else {
                panic!("expected Int value");
            }
        }
    }

    #[test]
    fn hex_bytes_value() {
        let cddl = parse("magic = h'deadbeef'");
        let rule = &cddl.rules[0];
        if let RuleBody::Type(t) = &rule.body {
            if let Type2::Value(Value::Bytes(bs)) = &t.choices[0].base {
                assert_eq!(bs.encoding, ByteEncoding::Hex);
                assert_eq!(bs.bytes, vec![0xde, 0xad, 0xbe, 0xef]);
            } else {
                panic!("expected Bytes value");
            }
        }
    }

    #[test]
    fn multiple_rules() {
        let cddl = parse("a = uint\nb = tstr\nc = bool");
        assert_eq!(cddl.rules.len(), 3);
        assert_eq!(cddl.rules[0].name, "a");
        assert_eq!(cddl.rules[1].name, "b");
        assert_eq!(cddl.rules[2].name, "c");
    }

    // ── Numeric case sensitivity (mn1-3) ──────────────────────────────────

    #[test]
    fn hex_int_uppercase_prefix() {
        // 0X prefix must be accepted
        let cddl = parse("x = 0XFF");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            assert!(matches!(t.choices[0].base, Type2::Value(Value::Int(255))));
        }
    }

    #[test]
    fn binary_int_uppercase_prefix() {
        let cddl = parse("x = 0B1010");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            assert!(matches!(t.choices[0].base, Type2::Value(Value::Int(10))));
        }
    }

    #[test]
    fn float_uppercase_exponent() {
        // 1E2 must parse as float 100.0
        let cddl = parse("x = 1E2");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Float(f)) = t.choices[0].base {
                assert!((f - 100.0).abs() < 1e-9, "expected 100.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }
    }

    // ── Hexfloat parsing (H2) ─────────────────────────────────────────────

    #[test]
    fn hexfloat_basic() {
        // 0x1.8p+1 = (1 + 8/16) * 2^1 = 1.5 * 2 = 3.0
        let cddl = parse("x = 0x1.8p+1");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Float(f)) = t.choices[0].base {
                assert!((f - 3.0).abs() < 1e-9, "expected 3.0, got {f}");
            } else {
                panic!("expected Float, got {:?}", t.choices[0].base);
            }
        }
    }

    #[test]
    fn hexfloat_negative() {
        let cddl = parse("x = -0x1p0");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Float(f)) = t.choices[0].base {
                assert!((f - (-1.0)).abs() < 1e-9, "expected -1.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }
    }

    // ── Text escape sequences (C2) ────────────────────────────────────────

    #[test]
    fn text_escape_newline() {
        let cddl = parse(r#"x = "\n""#);
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Text(s)) = &t.choices[0].base {
                assert_eq!(s, "\n");
            } else {
                panic!("expected Text");
            }
        }
    }

    #[test]
    fn text_escape_unicode() {
        let cddl = parse(r#"x = "A""#); // A = 'A'
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Text(s)) = &t.choices[0].base {
                assert_eq!(s, "A");
            } else {
                panic!("expected Text");
            }
        }
    }

    #[test]
    fn text_escape_unicode_braced() {
        let cddl = parse(r#"x = "\u{1F600}""#);
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Text(s)) = &t.choices[0].base {
                assert_eq!(s, "\u{1F600}");
            } else {
                panic!("expected Text");
            }
        }
    }

    // ── base64url (M2) ────────────────────────────────────────────────────

    #[test]
    fn b64_standard() {
        let cddl = parse("x = b64'SGVsbG8='"); // "Hello"
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Bytes(bs)) = &t.choices[0].base {
                assert_eq!(bs.bytes, b"Hello");
            } else {
                panic!("expected Bytes");
            }
        }
    }

    #[test]
    fn b64_url_safe() {
        // base64url: '-' maps to '+', '_' maps to '/'
        // Encode [0xFB, 0xFF] in standard = +/... in url = -_...
        // 0xFB = 11111011, 0xFF = 11111111
        // Group into 6-bit chunks: 111110 111111 111100 (need padding)
        // Standard: +// = b64'//8='
        // url-safe:  b64'-_8='  — same byte values
        let cddl = parse("x = b64'-_8='");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            if let Type2::Value(Value::Bytes(bs)) = &t.choices[0].base {
                assert_eq!(bs.bytes, vec![0xfb, 0xff]);
            } else {
                panic!("expected Bytes, got {:?}", t.choices[0].base);
            }
        }
    }

    // ── Stage 3 Findings 1/2: integer-literal overflow (security review) ──
    //
    // `parse_uint_str`/`parse_int_str` used to `.expect()` on overflow, so
    // an otherwise grammar-valid CDDL document with an oversized integer
    // literal (CDDL places no magnitude limit on `uint`) panicked instead
    // of being rejected. Human ruling: reject with a diagnostic error, no
    // bignum support — `build_cddl` now returns `Result` and these cases
    // surface as `Err(ParseError)`.

    #[test]
    fn huge_decimal_uint_literal_is_rejected() {
        let pairs = lex("x = 99999999999999999999999999").unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    #[test]
    fn huge_hex_uint_literal_is_rejected() {
        let pairs = lex("x = 0xFFFFFFFFFFFFFFFFFF").unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    #[test]
    fn huge_binary_uint_literal_is_rejected() {
        let src = format!("x = 0b{}", "1".repeat(65));
        let pairs = lex(&src).unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    #[test]
    fn huge_occurrence_bound_literal_is_rejected() {
        let pairs = lex("x = [99999999999999999999999999*5 uint]").unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    #[test]
    fn huge_tag_number_literal_is_rejected() {
        let pairs = lex("x = #6.99999999999999999999999999(uint)").unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    #[test]
    fn i64_min_negation_literal_does_not_overflow() {
        // -9223372036854775808 is exactly i64::MIN: the unsigned magnitude
        // (9223372036854775808) does not fit in i64, so a naive
        // `-(magnitude as i64)` cast-then-negate overflows. Must parse
        // cleanly to i64::MIN, not panic and not get rejected.
        let cddl = parse("x = -9223372036854775808");
        if let RuleBody::Type(t) = &cddl.rules[0].body {
            assert!(matches!(
                t.choices[0].base,
                Type2::Value(Value::Int(i64::MIN))
            ));
        } else {
            panic!("expected type rule");
        }
    }

    #[test]
    fn i64_min_minus_one_literal_is_rejected() {
        // One further from zero than i64::MIN — must be cleanly rejected,
        // not silently wrapped.
        let pairs = lex("x = -9223372036854775809").unwrap();
        assert!(build_cddl(pairs).is_err());
    }

    /// STAGE 3 FINDING 3 (fixed): `build_type`/`build_type1`/`build_type2`
    /// recurse once per nesting level of `(type)` with no depth ceiling of
    /// their own. `MAX_AST_DEPTH` turns what used to be a stack-overflow
    /// abort into a diagnosable `ParseError` well before the AST builder's
    /// own stack budget is threatened.
    ///
    /// 1000-deep textual nesting is now rejected even earlier, by
    /// `lexer::check_nesting_depth`'s much lower `MAX_LEX_NESTING_DEPTH`
    /// (12) — see that module for the two `pest`-level failure modes
    /// (stack overflow, exponential backtracking) which `MAX_AST_DEPTH`
    /// alone cannot prevent since it only guards the AST builder, run
    /// after `pest` has already produced a full parse tree. `lex` itself
    /// now returns `Err` for this input, so this test asserts on `lex`
    /// rather than reaching `build_cddl` at all.
    #[test]
    fn deeply_nested_parenthesized_type_is_rejected_not_stack_overflow() {
        let src = format!("x = {}uint{}", "(".repeat(1000), ")".repeat(1000));
        assert!(lex(&src).is_err());
    }

    /// Same shape via nested bracketed array types; see above.
    #[test]
    fn deeply_nested_array_type_is_rejected_not_stack_overflow() {
        let src = format!("x = {}uint{}", "[".repeat(1000), "]".repeat(1000));
        assert!(lex(&src).is_err());
    }

    /// A shallow nesting well under both `MAX_LEX_NESTING_DEPTH` and
    /// `MAX_AST_DEPTH` must still build fine.
    #[test]
    fn shallowly_nested_parenthesized_type_still_builds() {
        let src = format!("x = {}uint{}", "(".repeat(10), ")".repeat(10));
        let pairs = lex(&src).unwrap();
        assert!(build_cddl(pairs).is_ok());
    }

    /// Nesting between `MAX_LEX_NESTING_DEPTH` (12) and `MAX_AST_DEPTH`
    /// (128) is rejected by the lexer's cheaper textual scan before
    /// `build_cddl`'s own depth check ever runs — confirms the two caps
    /// compose (the lower one wins) rather than one masking the other in
    /// a way that leaves a depth range unchecked.
    #[test]
    fn nesting_between_lex_and_ast_caps_is_rejected_by_lexer() {
        let src = format!("x = {}uint{}", "(".repeat(20), ")".repeat(20));
        assert!(lex(&src).is_err());
    }
}
