/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Public tokenisation API for syntax highlighting.
//!
//! This module exposes [`tokenize`], which returns a flat list of [`Token`]
//! values carrying byte-offset ranges and semantic [`TokenKind`]s.  Consumers
//! that need per-line highlight spans should split on newlines themselves.
//!
//! The underlying grammar representation is kept `pub(crate)` — this API is
//! the stable surface for external crates such as `cddl_gui`.

use pest::iterators::Pair;

use crate::grammar::Rule;
use crate::lexer::lex;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Semantic category of a CDDL token, used for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A rule name or type name identifier.
    RuleName,
    /// A numeric, text, byte-string, or float literal.
    Literal,
    /// An operator: range (`..`/`...`), control (`.size` …), assignment (`=`/`/=`/`//=`),
    /// optional comma, or occurrence specifier.
    Operator,
    /// A comment (`;` to end of line).
    Comment,
    /// Everything else (punctuation, whitespace, structural tokens).
    Default,
}

/// A single token: a source fragment with its byte-offset range and kind.
///
/// The `text` field is a sub-slice of the original source.
#[derive(Debug, Clone)]
pub struct Token<'src> {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
    /// The source text of this token.
    pub text: &'src str,
    /// Semantic category.
    pub kind: TokenKind,
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Tokenise `source` into a flat, sorted sequence of [`Token`]s.
///
/// Overlapping spans are resolved by preferring the longer (outer) span.
/// Any source range not covered by a grammar rule is emitted as
/// [`TokenKind::Default`].
///
/// Returns an empty `Vec` when `source` is empty.  When the source is not
/// valid CDDL the entire source is returned as a single `Default` token.
///
/// # Examples
///
/// ```
/// use tps_cddl::tokenize::{tokenize, TokenKind};
///
/// let tokens = tokenize("x = uint");
/// assert!(!tokens.is_empty());
/// // At least one token should be a literal (uint is a literal value/typename).
/// let has_non_default = tokens.iter().any(|t| t.kind != TokenKind::Default);
/// assert!(has_non_default);
/// ```
pub fn tokenize(source: &str) -> Vec<Token<'_>> {
    if source.is_empty() {
        return Vec::new();
    }

    let pairs = match lex(source) {
        Ok(p) => p,
        Err(_) => {
            return vec![Token {
                start: 0,
                end: source.len(),
                text: source,
                kind: TokenKind::Default,
            }];
        }
    };

    let mut raw: Vec<(usize, usize, TokenKind)> = Vec::new();
    for pair in pairs.flatten() {
        let kind = pair_kind(&pair);
        if kind != TokenKind::Default {
            let s = pair.as_span();
            raw.push((s.start(), s.end(), kind));
        }
    }

    // Sort by start ascending, then by ascending length so shorter (inner) spans come first
    // when multiple semantically-meaningful spans cover the same start position.
    raw.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // Emit meaningful spans; fill uncovered gaps with Default.
    let mut out: Vec<(usize, usize, TokenKind)> = Vec::new();
    let mut cursor = 0usize;
    for (start, end, kind) in raw {
        if start < cursor {
            continue; // Already covered by an earlier span.
        }
        if start > cursor {
            out.push((cursor, start, TokenKind::Default));
        }
        out.push((start, end, kind));
        cursor = end;
    }
    if cursor < source.len() {
        out.push((cursor, source.len(), TokenKind::Default));
    }

    out.into_iter()
        .map(|(start, end, kind)| Token {
            start,
            end,
            text: &source[start..end],
            kind,
        })
        .collect()
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn pair_kind(pair: &Pair<'_, Rule>) -> TokenKind {
    match pair.as_rule() {
        Rule::id => TokenKind::RuleName,
        Rule::uint
        | Rule::int
        | Rule::number
        | Rule::text
        | Rule::bytes
        | Rule::value
        | Rule::hexfloat => TokenKind::Literal,
        Rule::rangeop
        | Rule::ctlop
        | Rule::assignt
        | Rule::assigng
        | Rule::optcom
        | Rule::occur => TokenKind::Operator,
        Rule::COMMENT => TokenKind::Comment,
        _ => TokenKind::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_returns_id_tokens() {
        let tokens = tokenize("x = uint");
        assert!(
            tokens.iter().any(|t| t.kind == TokenKind::RuleName),
            "expected at least one RuleName token: {tokens:?}"
        );
    }

    #[test]
    fn tokenize_returns_operator_for_assignment() {
        let tokens = tokenize("x = uint");
        assert!(
            tokens.iter().any(|t| t.kind == TokenKind::Operator),
            "expected at least one Operator token: {tokens:?}"
        );
    }

    #[test]
    fn tokenize_fallback_on_invalid_input() {
        let tokens = tokenize("@@@ not valid");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Default);
    }

    #[test]
    fn tokenize_empty_returns_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }
}
