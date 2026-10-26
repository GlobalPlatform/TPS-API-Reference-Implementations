/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Lexer: runs the pest PEG parser over a CDDL source string and returns the
//! flat token (`Pairs`) stream.
//!
//! With pest, tokenisation and parsing are unified — there is no separate
//! tokeniser phase.  This module exposes the top-level parse entry point.

use pest::Parser;

use crate::error::ParseError;
use crate::grammar::{CddlParser, Rule};

/// Cap on textual bracket/angle-bracket nesting depth accepted before
/// `pest` is asked to parse at all (Stage 3 Finding 3, lex-level half).
///
/// `pest`'s generated recursive-descent parser for this grammar has no
/// nesting ceiling of its own for `(type)`, `[group]`, `{group}`, or
/// `<genericarg>`. Two independent failure modes were measured directly
/// against `pest::Parser::parse` on this grammar:
/// - Parenthesized types (`(((...)))`) blow `pest`'s own call stack around
///   depth 550-700 — a straightforward stack overflow.
/// - Bracketed array/map types (`[[[...]]]`, `{{{...}}}`) exhibit
///   *exponential*-time backtracking (roughly 2x parse time per extra
///   nesting level — depth 20 measured at ~45s) because `grpent`'s first
///   alternative speculatively parses its body as a `memberkey` (itself a
///   full `type1` parse) before backtracking to parse the same nested type
///   again as the real entry; `pest` has no packrat memoization, so this
///   cost compounds at every level.
///
/// Rejecting before calling into `pest` at all, via a cheap O(n) textual
/// scan, closes both failure modes at once — neither `pest`'s stack use
/// nor its backtracking cost is bounded by this crate's own
/// `MAX_AST_DEPTH` (`parser.rs`), which only guards the AST *builder*, run
/// after `pest` has already produced a full parse tree. `<`/`>` are
/// included because deeply nested generic arguments (`wrap<wrap<...>>>`)
/// hit the same stack-overflow failure mode in `pest`'s `genericarg`/
/// `type1` mutual recursion (measured depth 300-500) — `<`/`>` have no
/// other role in this grammar (no comparison operators), so counting them
/// alongside the other three bracket types is unambiguous outside of the
/// `=>` memberkey cut token, special-cased below.
///
/// The cap is set well above any nesting depth seen in this workspace's
/// own CDDL fixtures (deepest observed: 4) while keeping worst-case
/// accepted-input parse time under ~200ms.
const MAX_LEX_NESTING_DEPTH: u32 = 12;

/// Build a [`ParseError`] reporting that `pos` exceeds [`MAX_LEX_NESTING_DEPTH`].
fn nesting_error(pos: pest::Position<'_>) -> ParseError {
    Box::new(pest::error::Error::<Rule>::new_from_pos(
        pest::error::ErrorVariant::CustomError {
            message: format!(
                "bracket/angle-bracket nesting exceeds the maximum supported depth ({MAX_LEX_NESTING_DEPTH})"
            ),
        },
        pos,
    ))
    .into()
}

/// Scan `source` for `(`/`[`/`{`/`<` nesting depth exceeding
/// [`MAX_LEX_NESTING_DEPTH`], before any `pest` parsing is attempted.
///
/// Operates byte-wise: every byte tested here is ASCII (`< 0x80`), and
/// UTF-8 continuation/lead bytes for non-ASCII characters are always
/// `>= 0x80`, so this can never misinterpret part of a multi-byte
/// character as a delimiter. Skips the contents of text/byte-string
/// literals and `;` comments (where brackets are just data, not syntax),
/// and the `>` in the `=>` memberkey cut (not a generic-argument close).
/// An unterminated string/comment is left for `pest` itself to diagnose;
/// this scan only ever rejects for excess depth, never for malformed
/// input.
fn check_nesting_depth(source: &str) -> Result<(), ParseError> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut depth: u32 = 0;
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < len && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'\'' => {
                i += 1;
                while i < len && bytes[i] != b'\'' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b';' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'=' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
            }
            b'(' | b'[' | b'{' | b'<' => {
                depth += 1;
                if depth > MAX_LEX_NESTING_DEPTH {
                    let pos = pest::Position::new(source, i)
                        .expect("i is a byte offset onto an ASCII delimiter, always a char boundary");
                    return Err(nesting_error(pos));
                }
                i += 1;
            }
            b')' | b']' | b'}' | b'>' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// Parse `source` as a complete CDDL document.
///
/// Returns the `Pairs` iterator rooted at the `cddl` rule on success, or a
/// [`ParseError`] on failure.  The caller (parser / AST builder) walks the
/// pairs to construct the typed AST.
///
/// # Errors
///
/// Returns a [`ParseError`] if `source` does not conform to the CDDL grammar,
/// or if its bracket/angle-bracket nesting exceeds [`MAX_LEX_NESTING_DEPTH`].
/// Use `ParseError`'s [`Display`](std::fmt::Display) implementation to obtain
/// a human-readable error message with line and column information.
///
/// # Examples
///
/// ```
/// use tps_cddl::lex;
///
/// let pairs = lex("x = uint").expect("valid CDDL");
/// assert!(pairs.as_str().contains("uint"));
/// ```
pub fn lex(source: &str) -> Result<pest::iterators::Pairs<'_, Rule>, ParseError> {
    check_nesting_depth(source)?;
    CddlParser::parse(Rule::cddl, source).map_err(|e| ParseError(Box::new(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parses(src: &str) {
        lex(src).unwrap_or_else(|e| panic!("parse failed:\n{e}"));
    }

    #[test]
    fn simple_type_rule() {
        parses("my-type = uint");
    }

    #[test]
    fn type_union() {
        parses("age = uint / tstr");
    }

    #[test]
    fn map_rule() {
        parses("person = { name: tstr, age: uint }");
    }

    #[test]
    fn array_rule() {
        parses("point = [x: float, y: float]");
    }

    #[test]
    fn group_choice() {
        parses("attire = (\"bow tie\" // \"necktie\")");
    }

    #[test]
    fn occurrence_star() {
        parses("bag = [* tstr]");
    }

    #[test]
    fn occurrence_plus() {
        parses("nonempty = [+ uint]");
    }

    #[test]
    fn occurrence_range() {
        parses("fixed = [1*3 uint]");
    }

    #[test]
    fn range_operator_inclusive() {
        parses("byte = 0..255");
    }

    #[test]
    fn range_operator_exclusive() {
        parses("byte1 = 0...256");
    }

    #[test]
    fn control_operator() {
        parses("short = tstr .size (1..10)");
    }

    #[test]
    fn text_string_literal() {
        parses(r#"greeting = "hello""#);
    }

    #[test]
    fn byte_string_hex() {
        parses("magic = h'deadbeef'");
    }

    #[test]
    fn byte_string_raw() {
        parses("raw = 'abc'");
    }

    #[test]
    fn tagged_type() {
        parses("t = #6.1(uint)");
    }

    #[test]
    fn any_type() {
        parses("anything = #");
    }

    #[test]
    fn generic_rule() {
        parses("pair<a, b> = [a, b]");
    }

    #[test]
    fn additive_assignment_type() {
        parses("attire = \"bow tie\"\nattire /= \"necktie\"");
    }

    #[test]
    fn additive_assignment_group() {
        parses("delivery = (city: tstr)\ndelivery //= (zip: uint)");
    }

    #[test]
    fn comment_ignored() {
        parses("; this is a comment\nfoo = uint");
    }

    #[test]
    fn empty_input_accepted_by_grammar() {
        // RFC 9682 §3.1 relaxed the grammar to allow empty documents.
        // RFC 8610 strictness (requiring at least one rule) is enforced by
        // the semantic validator, not the grammar/lexer.
        parses("");
    }

    #[test]
    fn rfc_example_person() {
        parses(
            r#"person = {
  name: tstr,
  age: uint,
  ? employer: tstr,
}"#,
        );
    }

    #[test]
    fn rfc_example_nested() {
        parses(
            r#"apartment = {
  floor: uint,
  ? unit: uint,
  address: address,
}
address = { street: tstr, city: tstr }"#,
        );
    }

    /// Stage 3 Finding 3 (lex-level half): `pest`'s own recursive-descent
    /// parser for this grammar has no depth ceiling, and stack-overflows
    /// well before `MAX_LEX_NESTING_DEPTH` is even reached at these input
    /// sizes — `check_nesting_depth` must reject before `pest` ever runs.
    #[test]
    fn deeply_nested_parens_rejected_before_pest_runs() {
        let src = format!("x = {}uint{}", "(".repeat(1000), ")".repeat(1000));
        assert!(lex(&src).is_err());
    }

    #[test]
    fn deeply_nested_brackets_rejected_before_pest_runs() {
        let src = format!("x = {}uint{}", "[".repeat(1000), "]".repeat(1000));
        assert!(lex(&src).is_err());
    }

    #[test]
    fn deeply_nested_braces_rejected_before_pest_runs() {
        let src = format!(
            "x = {}uint{}",
            "{a: ".repeat(1000),
            "}".repeat(1000)
        );
        assert!(lex(&src).is_err());
    }

    /// Deeply nested generic-argument instantiation (`wrap<wrap<...>>>`)
    /// hits the same `pest` stack-overflow failure mode via `<`/`>` —
    /// confirms those are counted too, not just `([{`.
    #[test]
    fn deeply_nested_generic_args_rejected_before_pest_runs() {
        let mut src = String::from("wrap<a> = [a]\nx = ");
        for _ in 0..1000 {
            src.push_str("wrap<");
        }
        src.push_str("uint");
        for _ in 0..1000 {
            src.push('>');
        }
        assert!(lex(&src).is_err());
    }

    /// A mix of all four bracket types, nested past the cap, must also be
    /// rejected — confirms the running depth counter is shared across
    /// bracket kinds rather than tracked independently per kind.
    #[test]
    fn deeply_nested_mixed_brackets_rejected() {
        let mut open = String::new();
        let mut close = String::new();
        for i in 0..50 {
            match i % 3 {
                0 => {
                    open.push('[');
                    close.push(']');
                }
                1 => {
                    open.push_str("{a: ");
                    close.push('}');
                }
                _ => {
                    open.push('(');
                    close.push(')');
                }
            }
        }
        let src = format!("x = {open}uint{close}");
        assert!(lex(&src).is_err());
    }

    /// Nesting at exactly the cap must still be accepted — off-by-one
    /// check against `MAX_LEX_NESTING_DEPTH`.
    #[test]
    fn nesting_at_cap_is_accepted() {
        let n = MAX_LEX_NESTING_DEPTH as usize;
        let src = format!("x = {}uint{}", "(".repeat(n), ")".repeat(n));
        parses(&src);
    }

    /// One level past the cap must be rejected.
    #[test]
    fn nesting_one_past_cap_is_rejected() {
        let n = MAX_LEX_NESTING_DEPTH as usize + 1;
        let src = format!("x = {}uint{}", "(".repeat(n), ")".repeat(n));
        assert!(lex(&src).is_err());
    }

    /// Brackets inside a text-string literal are data, not syntax — they
    /// must not be counted toward nesting depth.
    #[test]
    fn brackets_inside_text_string_are_not_counted() {
        let opens = "(".repeat(MAX_LEX_NESTING_DEPTH as usize + 5);
        let src = format!(r#"x = "{opens}""#);
        parses(&src);
    }

    /// Brackets inside a byte-string literal are likewise data, not
    /// syntax.
    #[test]
    fn brackets_inside_byte_string_are_not_counted() {
        let opens = "[".repeat(MAX_LEX_NESTING_DEPTH as usize + 5);
        let src = format!("x = '{opens}'");
        parses(&src);
    }

    /// Brackets inside a `;` comment are likewise data, not syntax.
    #[test]
    fn brackets_inside_comment_are_not_counted() {
        let opens = "{".repeat(MAX_LEX_NESTING_DEPTH as usize + 5);
        let src = format!(";{opens}\nx = uint");
        parses(&src);
    }

    /// The `>` in a memberkey's `=>` cut is not a generic-argument close;
    /// a long run of `k => v` group entries must not be misread as
    /// closing brackets that could otherwise let the depth counter go
    /// negative (it saturates at 0, but this confirms the `=>` bigram is
    /// actually recognized and skipped rather than relying on saturation).
    #[test]
    fn memberkey_cut_arrow_is_not_counted_as_generic_close() {
        let mut src = String::from("x = {\n");
        for i in 0..(MAX_LEX_NESTING_DEPTH as usize + 5) {
            src.push_str(&format!("  (\"k{i}\" => uint),\n"));
        }
        src.push('}');
        parses(&src);
    }
}
