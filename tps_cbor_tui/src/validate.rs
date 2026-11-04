/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Validate a loaded [`crate::artifact::Artifact`]'s CBOR bytes against a
//! CDDL schema, through `tps_cbor_core`'s CDDL layer.
//!
//! This module is the *dispatch*: it chooses between the two entry points
//! core offers and normalises what they return. Which one to use is a
//! question only [`crate::app::App`] can answer — it owns the path and knows
//! whether the editor has unsaved edits — so the choice arrives here as a
//! [`Basis`] rather than being inferred from the artifact.
//!
//! It used to be a wrapper of a wrapper around `tps_cddl::check_cbor`, which
//! reported a mismatch only at the rule the check started from: one row
//! spanning the whole document, naming nothing. Core's trace knows which
//! member broke the schema, and [`Outcome::diagnostics`] is where it says so.

use std::ops::Range;
use std::path::{Path, PathBuf};

use tps_cbor_core::cddl::{self, Chain, ChainTrace, Trace};
use tps_cbor_core::diag::{Diagnostic, Position, Severity, Source};
use tps_cddl::Config;

use crate::artifact::Artifact;
use crate::panic_hook::HookSuppressed;

/// Which CDDL text a check runs against, and therefore whether `;# import` /
/// `;# include` directives are resolved.
#[derive(Debug)]
pub enum Basis<'a> {
    /// A saved file. Core reads the whole include chain **from disk** and
    /// attributes every diagnostic to the file it belongs to.
    Chain {
        /// The top-level CDDL file.
        root: &'a Path,
        /// Where a bare module name is looked up; see
        /// `crate::app::cddl_search_paths`.
        search_paths: &'a [PathBuf],
    },
    /// The CDDL editor's text as it stands, with **no** module resolution.
    ///
    /// Used when there is no file yet, and when the buffer has unsaved edits:
    /// core's chain entry points read from disk, and checking the saved file
    /// while the user looks at edited text is the silent wrong answer this
    /// module exists to avoid. The cost — unresolved directives — is reported
    /// rather than hidden; see [`Outcome::buffer_only`].
    Buffer(&'a str),
}

/// Everything a check produced, ready for the results pane.
#[derive(Debug)]
pub struct Outcome {
    /// Sorted by severity then position. **Never empty**: a conforming
    /// document gets one informational row, so the pane always says something.
    pub diagnostics: Vec<Diagnostic>,
    /// The trace itself, for callers wanting more than the diagnostics.
    pub trace: Trace,
    /// The resolved include chain. `Some` only for [`Basis::Chain`], and the
    /// only way to turn a [`Diagnostic::file`] span into a line and column:
    /// each [`tps_cbor_core::cddl::ChainFile`] carries the text its span
    /// indexes.
    pub chain: Option<Chain>,
    /// Whether the check ran against the editor buffer with directives
    /// unresolved. The results pane says so in its title, because a row
    /// attributed to a line of the file on disk and a row attributed to a line
    /// of the buffer read identically.
    pub buffer_only: bool,
}

/// Validate `artifact`'s bytes against the schema named by `basis`.
///
/// Never fails: all three of core's entry points report a schema that does not
/// resolve, a schema that does not parse, CBOR that does not decode and CBOR
/// that does not match the same way, as diagnostics.
pub fn validate_artifact(
    artifact: &Artifact,
    basis: Basis<'_>,
    start_override: Option<&str>,
    config: &Config,
) -> Outcome {
    let mut outcome = match basis {
        Basis::Chain { root, search_paths } => {
            let checked = {
                // `check_chain` parses, and `build_cddl` can panic.
                let _guard = HookSuppressed::new();
                ChainTrace::check(&artifact.bytes, root, search_paths, start_override, config)
            };
            let mut diagnostics = checked.diagnostics;
            // `ChainTrace` leaves this out deliberately — only the caller
            // knows where the start-rule request came from, and therefore
            // where to point it. Dropping it here would let "conforms to
            // schema" mean "conforms to a rule you did not ask for".
            //
            // `0..0` because this crate's check root comes from a prompt, not
            // from a line in a document: there is no position to point at, and
            // core documents the empty span for a note that reports an
            // operation rather than a place.
            diagnostics.extend(checked.start.note(0..0).map(|note| note.in_file(root)));
            Outcome {
                diagnostics,
                trace: checked.trace,
                chain: Some(checked.chain),
                buffer_only: false,
            }
        }
        Basis::Buffer(source) => {
            let (trace, mut diagnostics) = {
                let _guard = HookSuppressed::new();
                cddl::check_source(&artifact.bytes, source, start_override, config)
            };
            // The title suffix alone would not do: it is equally true of a
            // schema with no directives, where it reads as a warning about
            // nothing. This row appears only when something was actually left
            // unresolved.
            if has_module_directive(source) {
                diagnostics.push(Diagnostic::info(
                    Source::CddlModules,
                    0..0,
                    "`;# import` / `;# include` not resolved: checked against the editor buffer, \
                     not the files on disk",
                ));
            }
            Outcome {
                diagnostics,
                trace,
                chain: None,
                buffer_only: true,
            }
        }
    };

    // `Trace::is_valid` means "nothing mismatched", which a trace that checked
    // *nothing* also satisfies: a schema that does not parse, or a start rule
    // that does not exist, produces an error and an empty, valid trace. Saying
    // "conforms to schema" next to that error is the pane contradicting itself.
    // The claim is only made when a check actually ran.
    let errors = outcome
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    if outcome.trace.is_valid() && !errors {
        // Synthesised here rather than in the renderer so that it is covered by
        // these tests, and so that a *valid* document carrying a start-rule
        // note does not come back showing the note alone.
        outcome.diagnostics.push(Diagnostic::info(
            Source::Validation,
            0..0,
            "conforms to schema — no mismatches",
        ));
    }
    tps_cbor_core::diag::sort(&mut outcome.diagnostics);
    outcome
}

/// Whether `source` has a module directive whose resolution [`Basis::Buffer`]
/// skips.
///
/// Matched on the line, not with a substring search, because `;# import` can
/// only appear as a directive at the start of a comment line and a `"; # import"`
/// inside a string literal must not be mistaken for one.
fn has_module_directive(source: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with(";# import") || line.starts_with(";# include")
    })
}

/// The CDDL text `diagnostic`'s span indexes into, if this crate holds it.
///
/// `buffer` is the CDDL editor's text. A [`Source::Cddl`] or
/// [`Source::CddlModules`] diagnostic attributed to a chain file is resolved
/// against *that file's* text, which only the chain has; one with no file
/// attribution indexes the buffer. `None` for a CBOR or EDN span, which is not
/// CDDL text at all, and for a file the chain does not know.
///
/// A free function rather than a method on [`Outcome`] because the results
/// pane holds the parts on [`crate::app::App`], not the whole `Outcome`.
pub fn cddl_text_for<'a>(
    diagnostic: &Diagnostic,
    chain: Option<&'a Chain>,
    buffer: &'a str,
) -> Option<&'a str> {
    if !matches!(diagnostic.source, Source::Cddl | Source::CddlModules) {
        return None;
    }
    match (&diagnostic.file, chain) {
        (Some(path), Some(chain)) => {
            let index = chain.index_of(path)?;
            Some(chain.files[index].text.as_str())
        }
        (Some(_), None) => None,
        (None, _) => Some(buffer),
    }
}

/// Whether `severity` and `span` describe a row with a position worth showing.
///
/// An empty span is core's inert span, used for a diagnostic that reports an
/// operation rather than a place — `@0x0` or `1:1` against one of those names
/// a byte that has nothing to do with it.
pub fn positioned(span: &Range<usize>) -> bool {
    span.start < span.end
}

/// The leading word for a diagnostic row.
///
/// Shared by the check-results pane and the CDDL editor's diagnostics pane:
/// both lists now mix severities, where every row in the one used to be a
/// mismatch and every row in the other a syntax error.
pub fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

/// One results-pane row: severity, where it is, and what it says.
///
/// The position is in the coordinate space of whatever `diagnostic.source`
/// says its span indexes — a byte offset into the CBOR buffer for a mismatch,
/// a line and column into CDDL text for a schema finding — because those are
/// not interchangeable and a bare number would be read as whichever the user
/// expected. A row whose span is empty, or whose CDDL text this crate does not
/// hold, gets no position rather than a misleading `0x0`.
///
/// `buffer` is the CDDL editor's text and `open` the path it came from, so
/// that a finding in an *included* file is labelled with that file's name.
pub fn row_text(
    diagnostic: &Diagnostic,
    chain: Option<&Chain>,
    buffer: &str,
    open: Option<&Path>,
) -> String {
    let severity = severity_label(diagnostic.severity);
    let Some(position) = row_position(diagnostic, chain, buffer, open) else {
        return format!("{severity}: {}", diagnostic.message);
    };
    format!("{severity}: {position} {}", diagnostic.message)
}

/// The `@0x1f` or `rfc8392.cddl:12:4` part of [`row_text`], if there is one.
fn row_position(
    diagnostic: &Diagnostic,
    chain: Option<&Chain>,
    buffer: &str,
    open: Option<&Path>,
) -> Option<String> {
    if !positioned(&diagnostic.span) {
        return None;
    }
    match diagnostic.source {
        Source::Cbor | Source::Validation => Some(format!("@0x{:x}", diagnostic.span.start)),
        Source::Cddl | Source::CddlModules => {
            let text = cddl_text_for(diagnostic, chain, buffer)?;
            let at = Position::at(text, diagnostic.span.start);
            let elsewhere = match (&diagnostic.file, open) {
                (Some(file), Some(open)) => file.as_path() != open,
                (Some(_), None) => true,
                (None, _) => false,
            };
            let name = match (elsewhere, &diagnostic.file) {
                (true, Some(file)) => file
                    .file_name()
                    .map(|n| format!("{}:", n.to_string_lossy()))
                    .unwrap_or_default(),
                _ => String::new(),
            };
            Some(format!("{name}{}:{}", at.line, at.column))
        }
        Source::Edn | Source::Cose | Source::Settings => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 8610 Appendix H, §3.3 Geography example.
    const GEOGRAPHY_CDDL: &str = "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\nGpsCoordinates = { longitude: uint, latitude: uint }";

    /// ["Bern", {longitude: 20, latitude: 46}]
    fn conforming_bytes() -> Vec<u8> {
        vec![
            0x82, 0x64, 0x42, 0x65, 0x72, 0x6e, 0xa2, 0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74,
            0x75, 0x64, 0x65, 0x14, 0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, 0x18,
            0x2e,
        ]
    }

    /// ["Bern", {longitude: "not a uint", latitude: 46}]
    fn non_conforming_bytes() -> Vec<u8> {
        vec![
            0x82, 0x64, 0x42, 0x65, 0x72, 0x6e, 0xa2, 0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74,
            0x75, 0x64, 0x65, 0x6a, 0x6e, 0x6f, 0x74, 0x20, 0x61, 0x20, 0x75, 0x69, 0x6e, 0x74,
            0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, 0x18, 0x2e,
        ]
    }

    fn check_buffer(bytes: Vec<u8>, source: &str, root: Option<&str>) -> Outcome {
        validate_artifact(
            &Artifact::new(bytes),
            Basis::Buffer(source),
            root,
            &Config::default(),
        )
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps_cbor_tui_validate_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn conforming_artifact_has_no_mismatches() {
        let outcome = check_buffer(conforming_bytes(), GEOGRAPHY_CDDL, Some("Geography"));
        assert!(outcome.trace.is_valid());
        assert!(
            outcome
                .diagnostics
                .iter()
                .all(|d| d.severity == Severity::Info),
            "a conforming document has nothing above a note: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_conforming_artifact_always_gets_a_row() {
        let outcome = check_buffer(conforming_bytes(), GEOGRAPHY_CDDL, Some("Geography"));
        assert_eq!(outcome.diagnostics.len(), 1);
        assert!(
            outcome.diagnostics[0]
                .message
                .contains("conforms to schema")
        );
    }

    #[test]
    fn non_conforming_artifact_reports_mismatches() {
        let outcome = check_buffer(non_conforming_bytes(), GEOGRAPHY_CDDL, Some("Geography"));
        assert!(!outcome.trace.is_valid());
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_rejection_names_the_offending_member_not_just_the_document() {
        // The capability the old `tps_cddl::check_cbor` seam could not express:
        // it reported one mismatch at the start rule, spanning everything. The
        // trace's pinpoints name the member that actually broke the schema.
        let bytes = non_conforming_bytes();
        let len = bytes.len();
        let outcome = check_buffer(bytes, GEOGRAPHY_CDDL, Some("Geography"));
        let errors: Vec<&Diagnostic> = outcome
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.iter().any(|d| d.message.contains("longitude")),
            "no row named the failing member: {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|d| positioned(&d.span) && (d.span.start > 0 || d.span.end < len)),
            "a pinpoint row should span the member, not the whole {len}-byte document: {errors:?}"
        );
    }

    #[test]
    fn a_start_rule_the_schema_does_not_define_is_reported() {
        let outcome = check_buffer(conforming_bytes(), GEOGRAPHY_CDDL, Some("NoSuchRule"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("NoSuchRule")),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_buffer_with_an_include_says_it_was_not_resolved() {
        let source = format!(";# include coords\n{GEOGRAPHY_CDDL}");
        let outcome = check_buffer(conforming_bytes(), &source, Some("Geography"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("not resolved")),
            "{:?}",
            outcome.diagnostics
        );
        assert!(outcome.buffer_only);
    }

    #[test]
    fn a_buffer_with_no_directive_says_nothing_about_resolution() {
        let outcome = check_buffer(conforming_bytes(), GEOGRAPHY_CDDL, Some("Geography"));
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("not resolved")),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_directive_like_string_inside_the_schema_is_not_a_directive() {
        let source = format!("{GEOGRAPHY_CDDL}\nnote = \";# include coords\"");
        assert!(!has_module_directive(&source));
    }

    #[test]
    fn a_chain_basis_resolves_the_include_and_validates_the_merged_schema() {
        let dir = temp_dir("chain");
        std::fs::write(
            dir.join("coords.cddl"),
            "GpsCoordinates = { longitude: uint, latitude: uint }\n",
        )
        .unwrap();
        let root = dir.join("geo.cddl");
        std::fs::write(
            &root,
            ";# include coords\nGeography = [city: tstr, gpsCoordinates: GpsCoordinates]\n",
        )
        .unwrap();

        let search_paths = vec![dir.clone()];
        let outcome = validate_artifact(
            &Artifact::new(conforming_bytes()),
            Basis::Chain {
                root: &root,
                search_paths: &search_paths,
            },
            Some("Geography"),
            &Config::default(),
        );

        assert!(
            outcome.trace.is_valid(),
            "the merged schema should accept the document: {:?}",
            outcome.diagnostics
        );
        assert!(!outcome.buffer_only);
        let chain = outcome.chain.as_ref().expect("a chain basis resolves one");
        assert_eq!(chain.files.len(), 2, "root plus the included module");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unresolvable_include_is_reported_against_the_root_file() {
        let dir = temp_dir("missing_include");
        let root = dir.join("geo.cddl");
        std::fs::write(&root, ";# include nowhere\nGeography = tstr\n").unwrap();

        let search_paths = vec![dir.clone()];
        let outcome = validate_artifact(
            &Artifact::new(conforming_bytes()),
            Basis::Chain {
                root: &root,
                search_paths: &search_paths,
            },
            None,
            &Config::default(),
        );

        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{:?}",
            outcome.diagnostics
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cddl_row_with_no_file_resolves_against_the_buffer() {
        // `a = @@@` *parses* — `@@@` is taken for a rule name, and the
        // complaint arrives as a `Source::Validation` row. A genuine parse
        // error needs a schema the grammar rejects outright.
        const UNPARSEABLE: &str = "Geography = ";
        let outcome = check_buffer(conforming_bytes(), UNPARSEABLE, None);
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|d| matches!(d.source, Source::Cddl))
            .unwrap_or_else(|| panic!("a parse error is a Cddl row: {:?}", outcome.diagnostics));
        assert_eq!(
            cddl_text_for(diagnostic, outcome.chain.as_ref(), UNPARSEABLE),
            Some(UNPARSEABLE)
        );
    }

    #[test]
    fn a_schema_that_did_not_parse_does_not_also_claim_conformance() {
        // The trace is empty, and an empty trace is `is_valid()` — nothing
        // mismatched because nothing was checked. The pane must not print
        // "conforms to schema" under the parse error.
        let outcome = check_buffer(conforming_bytes(), "Geography = ", None);
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{:?}",
            outcome.diagnostics
        );
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("conforms to schema")),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn an_unknown_start_rule_is_a_warning_and_the_conformance_claim_stands() {
        // Core does not refuse an unknown start rule: it warns, names the rule
        // it fell back to, and checks against that. So the conformance row is
        // *true* here, and the pair of rows has to be read together — which is
        // exactly why `ChainTrace`'s start note is not dropped on the floor.
        let outcome = check_buffer(conforming_bytes(), GEOGRAPHY_CDDL, Some("NoSuchRule"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("NoSuchRule")),
            "{:?}",
            outcome.diagnostics
        );
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("conforms to schema")),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_cbor_row_has_no_cddl_text() {
        let outcome = check_buffer(non_conforming_bytes(), GEOGRAPHY_CDDL, Some("Geography"));
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|d| matches!(d.source, Source::Validation))
            .expect("a mismatch is a Validation row");
        assert_eq!(
            cddl_text_for(diagnostic, outcome.chain.as_ref(), GEOGRAPHY_CDDL),
            None
        );
    }

    #[test]
    fn an_empty_span_is_not_positioned() {
        assert!(!positioned(&(0..0)));
        assert!(!positioned(&(7..7)));
        assert!(positioned(&(0..1)));
    }
}
