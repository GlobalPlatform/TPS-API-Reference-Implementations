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

    /// A human-readable description of why the parse failed.
    pub fn message(&self) -> String {
        self.0.variant.message().to_string()
    }
}
