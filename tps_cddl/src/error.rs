/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Diagnostic types used throughout the cddl crate.

use crate::ast::Span;
use crate::grammar::Rule;

/// Severity level of a diagnostic.
///
/// Returned as part of [`Diagnostic`].  An [`Error`](Level::Error) means the
/// CDDL document is invalid; a [`Warning`](Level::Warning) means it was
/// accepted but something looks suspicious.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    /// A fatal error: the CDDL document is invalid.
    Error,
    /// A non-fatal warning: the document is accepted but something looks suspicious.
    Warning,
}

/// A single diagnostic message pointing at a source location.
///
/// Diagnostics are returned by [`crate::validate`] and [`crate::validate_with_config`].
/// Check [`Level`] to distinguish errors from warnings.
///
/// # Examples
///
/// ```
/// use tps_cddl::{lex, build_cddl, validate};
///
/// let pairs = lex("x = uint").unwrap();
/// let schema = build_cddl(pairs).unwrap();
/// let diags = validate(&schema);
/// assert!(diags.is_empty(), "expected no errors");
/// ```
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Whether this is an error or a warning.
    pub level: Level,
    /// Byte-offset range in the source where the issue was detected.
    pub span: Span,
    /// Human-readable description of the issue.
    pub message: String,
}

/// A parse error returned when the CDDL source does not conform to the grammar.
///
/// Wraps the internal pest error, hiding the dependency on the `pest` crate from
/// library callers.  Use the [`Display`](std::fmt::Display) implementation to
/// obtain a human-readable, line/column-annotated message.
///
/// # Examples
///
/// ```
/// use tps_cddl::lex;
///
/// let err = lex("not valid ! cddl @@").unwrap_err();
/// let msg = err.to_string();
/// assert!(!msg.is_empty());
/// ```
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(#[from] pub(crate) Box<pest::error::Error<Rule>>);

impl ParseError {
    /// The source location (byte offset) where the parse error occurred.
    pub fn location(&self) -> &pest::error::InputLocation {
        &self.0.location
    }

    /// The byte range in the source where the parse failed.
    ///
    /// Prefer this to [`ParseError::location`]: it reports the same information without
    /// requiring the caller to name a `pest` type, which is what this wrapper exists to avoid.
    /// A position-only error — one pest reported at a single point rather than over a span —
    /// yields an empty range at that offset, so `start == end`.
    ///
    /// The range is suitable for pairing with
    /// [`ast::offset_to_line_col`](crate::ast::offset_to_line_col) to place a marker, or for
    /// highlighting directly in an editor.
    ///
    /// # Examples
    ///
    /// ```
    /// use tps_cddl::lex;
    ///
    /// let err = lex("a = uint\nb = !\n").unwrap_err();
    /// let span = err.span();
    /// assert!(span.start <= span.end);
    /// // The failure is on the second line, not at the start of the file.
    /// assert!(span.start > 8, "expected an offset past the first line, got {span:?}");
    /// ```
    pub fn span(&self) -> core::ops::Range<usize> {
        match self.0.location {
            pest::error::InputLocation::Pos(pos) => pos..pos,
            pest::error::InputLocation::Span((start, end)) => start..end,
        }
    }

    /// A human-readable description of why the parse failed.
    pub fn message(&self) -> String {
        self.0.variant.message().to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{build_cddl, lex};

    #[test]
    fn a_lex_error_reports_where_it_failed() {
        let source = "a = uint\nb = !!!\n";
        let error = lex(source).expect_err("`!!!` is not CDDL");
        let span = error.span();
        assert!(span.start <= span.end, "inverted span {span:?}");
        assert!(
            span.end <= source.len(),
            "span {span:?} runs past the {}-byte source",
            source.len()
        );
        let (line, _) = crate::ast::offset_to_line_col(source, span.start);
        assert_eq!(line, 2, "the error is on line 2, not line {line}");
    }

    #[test]
    fn a_parse_error_reports_where_it_failed() {
        // Lexes, but is not a well-formed CDDL document.
        let source = "a = uint\na = \n";
        let Err(error) = lex(source).and_then(build_cddl) else {
            return; // Accepted by this grammar; nothing to assert.
        };
        let span = error.span();
        assert!(span.start <= span.end, "inverted span {span:?}");
        assert!(span.end <= source.len(), "span {span:?} is out of range");
    }

    /// `span()` must agree with `location()`, since both read the same field.
    #[test]
    fn span_agrees_with_location() {
        let error = lex("x = !!!").expect_err("not CDDL");
        let expected = match error.location() {
            pest::error::InputLocation::Pos(pos) => *pos..*pos,
            pest::error::InputLocation::Span((start, end)) => *start..*end,
        };
        assert_eq!(error.span(), expected);
    }

    #[test]
    fn a_valid_document_produces_no_error() {
        assert!(lex("a = uint\n").and_then(build_cddl).is_ok());
    }
}
