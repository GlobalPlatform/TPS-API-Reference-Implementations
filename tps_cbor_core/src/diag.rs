/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Ranged, attributed diagnostics.
//!
//! Two deliberate differences from the old GUI's diagnostic type:
//!
//! * **Both ends of the span are kept.** `validate_cddl` discarded `span.end` and reported only a
//!   line and column, which is why the old editor could never draw a ranged squiggle.
//! * **Positions are derived, not stored.** A [`Diagnostic`] holds byte offsets; line and column
//!   are computed on demand by [`Diagnostic::position`] against the text being displayed. Storing
//!   them invites the two from drifting apart as the user edits, and the conversion has exactly
//!   one implementation ([`tps_cddl::ast::offset_to_line_col`]) rather than the three the old
//!   backend had.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// How serious a diagnostic is.
///
/// The variant order is load-bearing: the derived [`Ord`] is used to sort and to pick the worst of
/// a set, and it ranks `Error` first because `Error` is the most severe. Add new variants in
/// decreasing severity, not alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The document is unusable as it stands.
    Error,
    /// The document is usable but something is suspect.
    Warning,
    /// Not a defect at all: a report of what an operation did.
    ///
    /// The diagnostics list is the only place this application can tell the user that something
    /// succeeded, so a successful Encrypt and a refused Sign both end up in it. Without this
    /// variant both are filed as warnings and look alike, which is the opposite of what the list
    /// is for.
    Info,
}

impl From<tps_cddl::Level> for Severity {
    fn from(level: tps_cddl::Level) -> Self {
        match level {
            tps_cddl::Level::Error => Severity::Error,
            tps_cddl::Level::Warning => Severity::Warning,
        }
    }
}

/// Which stage produced a diagnostic.
///
/// The UI uses this to decide which pane a diagnostic belongs to and which editor its span
/// indexes into: a [`Source::Cbor`] span is a byte offset into the CBOR buffer, a [`Source::Edn`]
/// span is a byte offset into the rendered EDN text, and the rest index into CDDL source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// CBOR well-formedness. Spans index the raw CBOR byte buffer.
    Cbor,
    /// EDN parsing. Spans index the EDN text.
    Edn,
    /// CDDL tokenizing, parsing or validation. Spans index CDDL source.
    Cddl,
    /// `;# import` / `;# include` resolution, or validation of the merged document.
    ///
    /// Records *which pass* produced the diagnostic, not which coordinate space its span is in.
    /// [`crate::cddl::check_chain`] attributes these to a file and rebases the span into it, so a
    /// `CddlModules` diagnostic with [`Diagnostic::file`] set indexes that file; one without still
    /// indexes the merged source.
    CddlModules,
    /// A CBOR-against-CDDL validation mismatch. Spans index the CBOR byte buffer.
    Validation,
    /// A COSE operation.
    Cose,
    /// Reading or writing the settings file. Spans index the TOML text.
    Settings,
}

/// A line and column pair, both 1-based, as produced by [`tps_cddl::ast::offset_to_line_col`].
///
/// Note that `column` counts bytes, not characters or grapheme clusters, because that is what the
/// shared conversion does. Editors that want a character column must convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column, in bytes.
    pub column: usize,
}

impl Position {
    /// The position of byte `offset` within `text`.
    ///
    /// This is the crate's single entry point to line/column conversion -- it delegates to
    /// [`tps_cddl::ast::offset_to_line_col`] and nothing else does. An offset past the end of
    /// `text` clamps to the end rather than panicking, which matters because a diagnostic's span
    /// can outlive the edit that shortened the buffer.
    pub fn at(text: &str, offset: usize) -> Self {
        let (line, column) = tps_cddl::ast::offset_to_line_col(text, offset);
        Self { line, column }
    }
}

/// One message about one range of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// How serious this is.
    pub severity: Severity,
    /// Which stage produced it, and therefore what `span` indexes into.
    pub source: Source,
    /// The byte range at fault. May be empty, for a point marker.
    pub span: Range<usize>,
    /// The file at fault, where one is known.
    ///
    /// This matters for the CDDL include chain: a syntax error in `rfc8392.cddl` reached through
    /// two levels of `;# include` must be attributed to that file, not to the file the user has
    /// open.
    pub file: Option<PathBuf>,
    /// The human-readable message.
    pub message: String,
}

impl Diagnostic {
    /// A diagnostic covering `span`, with no file attribution.
    pub fn new(
        severity: Severity,
        source: Source,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            source,
            span,
            file: None,
            message: message.into(),
        }
    }

    /// An error covering `span`.
    pub fn error(source: Source, span: Range<usize>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, source, span, message)
    }

    /// A warning covering `span`.
    pub fn warning(source: Source, span: Range<usize>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, source, span, message)
    }

    /// An informational note covering `span`.
    ///
    /// Use `0..0` for a note that reports an operation rather than a position in a document.
    pub fn info(source: Source, span: Range<usize>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, source, span, message)
    }

    /// Attribute this diagnostic to `file`.
    #[must_use]
    pub fn in_file(mut self, file: impl Into<PathBuf>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Attribute this diagnostic to `file` if there is one.
    #[must_use]
    pub fn in_file_opt(mut self, file: Option<&Path>) -> Self {
        self.file = file.map(Path::to_path_buf);
        self
    }

    /// Resolve this diagnostic's byte range to start and end positions within `text`.
    ///
    /// `text` must be the document `span` indexes into; see [`Source`]. Offsets past the end of
    /// `text` clamp to its end rather than panicking, so a stale span cannot bring down the UI.
    pub fn position(&self, text: &str) -> (Position, Position) {
        (
            Position::at(text, self.span.start),
            Position::at(text, self.span.end),
        )
    }

    /// Whether this diagnostic covers no characters, and so can only be drawn as a point.
    pub fn is_point(&self) -> bool {
        self.span.start >= self.span.end
    }
}

/// Sort diagnostics into the order a diagnostics pane should list them: errors before warnings
/// before informational notes, then by file, then by position.
///
/// The severity ordering is [`Severity`]'s own variant order, which is why that order is declared
/// to be load-bearing there.
pub fn sort(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.span.start.cmp(&b.span.start))
            .then_with(|| a.span.end.cmp(&b.span.end))
    });
}

/// Turn an EDN parse failure into a diagnostic against `text`.
///
/// Only one of [`tps_minicbor::EdnError`]'s ten variants carries a position
/// ([`UnexpectedChar`](tps_minicbor::EdnError::UnexpectedChar)); `InvalidHex`, `UnexpectedEof`,
/// `TrailingInput` and five others do not. Those get a **whole-document span** rather than a
/// fabricated one at offset zero, which is what `error.rs` asks callers to do: a marker on the
/// first character would point at text that is very likely correct.
///
/// A truncation is a [`Severity::Warning`], matching how the CBOR tab treats a half-typed buffer --
/// an editor mid-edit routinely holds an unclosed array, and painting it red on every keystroke
/// is noise rather than information.
///
/// Spans are byte offsets, so the marked range is the single *character* at the offset and not its
/// first byte: ending the span mid-character would split a multi-byte char and misplace the
/// squiggle's right edge.
pub fn edn_diagnostic(text: &str, error: &crate::error::CoreError) -> Diagnostic {
    let severity = if crate::convert::is_edn_truncation(error) {
        Severity::Warning
    } else {
        Severity::Error
    };

    let span = match error {
        crate::error::CoreError::EdnParse {
            offset: Some(offset),
            ..
        } => {
            let start = (*offset).min(text.len());
            let width = text[start..].chars().next().map_or(0, char::len_utf8);
            start..start + width
        }
        _ => 0..text.len(),
    };

    Diagnostic::new(severity, Source::Edn, span, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "first line\nsecond line\nthird line\n";

    #[test]
    fn position_resolves_both_ends() {
        // "second" starts at offset 11 and ends at 17.
        let diagnostic = Diagnostic::error(Source::Cddl, 11..17, "nope");
        let (start, end) = diagnostic.position(TEXT);
        assert_eq!(start, Position { line: 2, column: 1 });
        assert_eq!(end, Position { line: 2, column: 7 });
    }

    /// The old GUI dropped `span.end`, so this is the regression test for ranged squiggles.
    #[test]
    fn a_span_crossing_a_newline_keeps_its_end() {
        let diagnostic = Diagnostic::error(Source::Cddl, 6..17, "nope");
        let (start, end) = diagnostic.position(TEXT);
        assert_eq!(start.line, 1);
        assert_eq!(end.line, 2);
        assert!(!diagnostic.is_point());
    }

    #[test]
    fn an_offset_past_the_end_clamps_instead_of_panicking() {
        let diagnostic = Diagnostic::error(Source::Cddl, 9000..9001, "stale");
        let (start, end) = diagnostic.position(TEXT);
        assert_eq!(start, end);
        assert_eq!(start.line, 4);
    }

    #[test]
    fn non_ascii_offsets_are_byte_based() {
        // The old GUI's local offset_to_line_col walked chars and compared the char index against
        // a byte offset, so any multi-byte character shifted every later position. "é" is two
        // bytes, so "x" sits at byte offset 3 on line 2.
        let text = "é\nx";
        let diagnostic = Diagnostic::error(Source::Cddl, 3..4, "here");
        let (start, _) = diagnostic.position(text);
        assert_eq!(start, Position { line: 2, column: 1 });
    }

    #[test]
    fn empty_spans_are_points() {
        assert!(Diagnostic::error(Source::Edn, 5..5, "point").is_point());
    }

    /// The positional case: `UnexpectedChar` is the one variant of ten that knows where it is, so
    /// its offset must survive into a span rather than be flattened to `0..0` the way
    /// `Ide::report` does for a generic error.
    #[test]
    fn an_edn_error_with_an_offset_marks_one_character() {
        let text = "[1, 2, %]";
        let error = crate::convert::edn_to_cbor(text).expect_err("should not parse");
        let diagnostic = edn_diagnostic(text, &error);
        assert_eq!(diagnostic.span, 7..8);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.source, Source::Edn);
    }

    /// Eight of the ten variants carry no position at all. `error.rs` asks callers to mark the
    /// whole document rather than invent line 1 column 1, because a marker on the first character
    /// points at text that is very probably correct.
    #[test]
    fn an_edn_error_without_an_offset_marks_the_whole_document() {
        let text = "h'nothex'";
        let error = crate::convert::edn_to_cbor(text).expect_err("should not parse");
        let diagnostic = edn_diagnostic(text, &error);
        assert_eq!(diagnostic.span, 0..text.len());
        assert!(
            !diagnostic.is_point(),
            "a whole-document span is not a point"
        );
    }

    /// Half-typed EDN is the editor's normal state, so it is a warning and not an error.
    #[test]
    fn an_unterminated_edn_value_is_reported_as_a_warning() {
        let text = "[1, 2";
        let error = crate::convert::edn_to_cbor(text).expect_err("should not parse");
        assert_eq!(edn_diagnostic(text, &error).severity, Severity::Warning);
    }

    /// The span is a byte range, so it must cover the whole character at the offset. Ending it one
    /// byte later would split a multi-byte char and put the squiggle's right edge inside it.
    #[test]
    fn an_edn_error_on_a_multibyte_character_spans_the_whole_character() {
        let text = "[1, é]";
        let error = crate::convert::edn_to_cbor(text).expect_err("should not parse");
        let diagnostic = edn_diagnostic(text, &error);
        assert_eq!(diagnostic.span, 4..6, "é is two bytes");
        assert!(
            text.get(diagnostic.span.clone()).is_some(),
            "span splits a char"
        );
    }

    /// An offset at the very end of the input has no character to mark, so it degrades to a point
    /// rather than slicing past the end and panicking.
    #[test]
    fn an_edn_offset_at_the_end_of_the_text_becomes_a_point() {
        let error = crate::error::CoreError::EdnParse {
            offset: Some(5),
            source: tps_minicbor::EdnError::UnexpectedChar('x', 5),
        };
        assert_eq!(edn_diagnostic("12345", &error).span, 5..5);
    }

    #[test]
    fn sort_puts_errors_first_then_position() {
        let mut diagnostics = vec![
            Diagnostic::warning(Source::Cddl, 5..6, "w1"),
            Diagnostic::error(Source::Cddl, 90..91, "e2"),
            Diagnostic::error(Source::Cddl, 10..11, "e1"),
        ];
        sort(&mut diagnostics);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["e1", "e2", "w1"]);
    }

    #[test]
    fn cddl_levels_map_onto_severity() {
        assert_eq!(Severity::from(tps_cddl::Level::Error), Severity::Error);
        assert_eq!(Severity::from(tps_cddl::Level::Warning), Severity::Warning);
    }

    /// The variant order is what [`sort`] and every "worst of these" comparison rest on, so it is
    /// asserted rather than left to the declaration order being noticed.
    #[test]
    fn info_is_the_least_severe_and_error_the_most() {
        assert!(Severity::Error < Severity::Warning);
        assert!(Severity::Warning < Severity::Info);

        let mut diagnostics = vec![
            Diagnostic::info(Source::Cose, 0..0, "note"),
            Diagnostic::warning(Source::Cose, 0..0, "suspect"),
            Diagnostic::error(Source::Cose, 0..0, "broken"),
        ];
        sort(&mut diagnostics);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["broken", "suspect", "note"]);
    }
}
