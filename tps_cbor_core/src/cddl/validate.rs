/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Tokenizing, parsing, validation and module resolution.
//!
//! Ported from `tps_cbor_gui/src-tauri/src/main.rs:145-299`, with the three defects that version
//! carried removed:
//!
//! 1. **Positions come from one shared conversion.** The old file had its own
//!    `offset_to_line_col` (`main.rs:187-202`) that walked `text.chars().enumerate()` and compared
//!    the resulting *character* index against a *byte* offset. Every position after the first
//!    non-ASCII character in a schema was wrong. Diagnostics here carry byte spans and convert
//!    through [`crate::diag::Position`] only.
//! 2. **`span.end` survives.** `validate_cddl` kept only `diag.span.start` (`main.rs:174`), which
//!    is why the old UI could draw a marker but never a ranged squiggle.
//! 3. **Tokenize and parse errors report where they failed.** `parse_cddl_tree` reported them at
//!    `line: 1, column: 1` (`main.rs:279-299`) because `tps_cddl::ParseError` exposed its offset
//!    only behind a `pest` type. It now has [`tps_cddl::ParseError::span`], added for this.
//!
//! # Fallible and infallible entry points
//!
//! [`parse`] and [`validate`] return a [`Result`](crate::Result), for a caller that wants to stop.
//! [`check`] never fails: it returns whatever diagnostics it found, which is what an editor wants
//! while the user is mid-keystroke and the document is usually invalid. [`check_chain`] is the
//! same for a whole `;# include` chain, and additionally attributes every diagnostic to the file
//! it came from.

use std::ops::Range;
use std::path::{Path, PathBuf};

use tps_cddl::modules::scan_directives;
use tps_cddl::{
    Cddl, Config, ModuleError, ModuleResolution, ModuleSearchPath, ParseError, build_cddl, lex,
    resolve_modules, validate_with_config,
};

use super::chain::{self, Chain};
use crate::diag::{Diagnostic, Severity, Source};
use crate::error::{CoreError, Result};

/// What a caught [`build_cddl`] panic is reported as.
///
/// Phrased as a request for the file because that is the only useful action: the input reached a
/// state `tps_cddl`'s parser does not model, and only the text can find out which.
const PARSER_PANIC: &str = "the CDDL parser failed on this input in a way it does not report as \
                            an error. The document is intact and unsaved work is safe; please \
                            report the file as a bug.";

/// Run `build`, turning a panic into `Err(())` rather than letting it unwind out.
///
/// # Why this exists, and why it is the only place in the project that catches a panic
///
/// `tps_cddl::build_cddl`'s own documentation says it *"panics on other malformed input (i.e.
/// input that passed the grammar but contains unexpected structure)"*, and `parser.rs` carries
/// around fifty `unwrap`/`panic!` sites. Phase 5 is the first code to run that parser on
/// arbitrary half-typed text, from an editor keystroke path, where an unwind takes the window --
/// and every unsaved inner tab of the user's include chain -- down with it.
///
/// Catching is sound here specifically: `build_cddl` allocates a fresh AST and mutates nothing
/// shared, so a panic leaves no poisoned state behind, and neither workspace sets
/// `panic = "abort"`. [`lex_check`] keeps the per-keystroke path off `build_cddl` entirely; this
/// is the backstop for the paths that must parse.
///
/// No panic hook is installed: the default stderr line is wanted evidence for the bug report
/// [`PARSER_PANIC`] asks for.
fn guarded<T>(build: impl FnOnce() -> T) -> std::result::Result<T, ()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)).map_err(|_| ())
}

/// Tokenize and parse `source` into an AST.
pub fn parse(source: &str) -> Result<Cddl> {
    let tokens = lex(source).map_err(lex_error)?;
    guarded(|| build_cddl(tokens))
        .map_err(|()| CoreError::CddlParse {
            span: 0..source.len(),
            message: PARSER_PANIC.to_owned(),
        })?
        .map_err(parse_error)
}

/// Tokenize, parse and validate `source`, returning the AST and any diagnostics.
///
/// Fails only if the source does not parse; a document that parses but fails validation yields
/// the AST *and* the error diagnostics, because the AST is still useful -- the tree view and the
/// highlighter both want it.
pub fn validate(source: &str, config: &Config) -> Result<(Cddl, Vec<Diagnostic>)> {
    let cddl = parse(source)?;
    let diagnostics = validate_ast(&cddl, config);
    Ok((cddl, diagnostics))
}

/// Validate an already-parsed AST.
///
/// Separate from [`validate`] so a caller holding an AST -- from [`parse`], or from
/// [`Resolved::cddl`] after module resolution -- does not re-parse to validate it.
pub fn validate_ast(cddl: &Cddl, config: &Config) -> Vec<Diagnostic> {
    validate_with_config(cddl, config)
        .iter()
        .map(|diagnostic| {
            Diagnostic::new(
                Severity::from(diagnostic.level.clone()),
                Source::Cddl,
                diagnostic.span.start..diagnostic.span.end,
                diagnostic.message.clone(),
            )
        })
        .collect()
}

/// Everything known about `source`, without ever failing.
///
/// This is the entry point for an editor: it runs as far as it can and reports what went wrong,
/// rather than returning an error the caller must turn into a diagnostic itself. `cddl` is `None`
/// exactly when the source did not parse, in which case `diagnostics` holds one error saying where.
#[derive(Debug)]
pub struct Check {
    /// The parsed AST, if the source parsed.
    pub cddl: Option<Cddl>,
    /// Errors and warnings, spans indexing `source`.
    pub diagnostics: Vec<Diagnostic>,
}

/// Tokenize, parse and validate `source`, reporting failures as diagnostics.
///
/// Never fails. A tokenize or parse error yields `cddl: None` and exactly one error diagnostic,
/// positioned at the offending byte range. A parser *panic* -- which `build_cddl` documents as
/// possible on malformed-but-grammatical input -- likewise yields one error diagnostic, over the
/// whole source; see [`guarded`].
pub fn check(source: &str, config: &Config) -> Check {
    let tokens = match lex(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            return Check {
                cddl: None,
                diagnostics: vec![diagnostic_for(&lex_error(error))],
            };
        }
    };
    let built = match guarded(|| build_cddl(tokens)) {
        Ok(built) => built,
        Err(()) => {
            return Check {
                cddl: None,
                diagnostics: vec![Diagnostic::error(
                    Source::Cddl,
                    0..source.len(),
                    PARSER_PANIC,
                )],
            };
        }
    };
    match built {
        Ok(cddl) => {
            let diagnostics = validate_ast(&cddl, config);
            Check {
                cddl: Some(cddl),
                diagnostics,
            }
        }
        Err(error) => Check {
            cddl: None,
            diagnostics: vec![diagnostic_for(&parse_error(error))],
        },
    }
}

/// Tokenize `source`, reporting a failure as a diagnostic.
///
/// Deliberately does not parse. [`build_cddl`] is documented to *panic* on
/// malformed-but-grammatical input, and while [`guarded`] stops that killing the process, a
/// per-keystroke path should not be provoking it dozens of times a minute. Lexing is fallible,
/// cheap, and catches the syntax errors a user actually types.
pub fn lex_check(source: &str) -> Option<Diagnostic> {
    lex(source)
        .err()
        .map(|error| diagnostic_for(&lex_error(error)))
}

/// [`check`], graded for a file that may be one module of a chain.
///
/// Identical to [`check`] except that a *semantic* finding is demoted to a warning when `source`
/// declares any `;# import` / `;# include`. `rfc8392.cddl` legitimately references rules defined
/// in `rfc9052.cddl`, so plain [`check`] paints a dozen red squiggles on correct code the moment
/// the file is opened -- the worst possible first impression, and the reason this function exists.
/// A file with *no* directives is self-contained, so its undefined references stay errors.
///
/// # What is and is not demoted
///
/// The directive test comes from `scan_directives`, not from matching diagnostic message text, so
/// a reworded upstream diagnostic cannot silently re-grade anything. The cost of refusing to read
/// messages is that the demotion cannot pick out *only* cross-reference findings: every validator
/// finding is demoted, including ones that are genuine errors within a single file, such as a
/// duplicated rule name.
///
/// That is the right trade for this caller. Tokenize and parse failures are untouched -- they
/// arrive before the validator runs and keep `cddl: None` -- so real syntax errors still show red
/// while typing, and the chain-wide [`check_chain`] behind `Syntax Check` reports the full set at
/// full severity. Per-tab feedback is advisory; the explicit check is authoritative.
pub fn check_module(source: &str, config: &Config) -> Check {
    let mut result = check(source, config);
    let (directives, _, _, _) = scan_directives(source);
    // `cddl.is_some()` is what distinguishes a validator finding from a lex or parse failure,
    // without reading a single message.
    if !directives.is_empty() && result.cddl.is_some() {
        for diagnostic in &mut result.diagnostics {
            diagnostic.severity = Severity::Warning;
        }
    }
    result
}

/// A resolved `;# import` / `;# include` chain.
///
/// # Spans index the merged source, not any one file
///
/// `tps_cddl` resolves a chain by concatenating every module's text into one virtual document and
/// rebasing every span into it. So `source` is not the content of any file on disk, and a span is
/// only meaningful against `source`.
///
/// Attributing a merged-source span back to the file it came from needs the per-module offset
/// table, which `tps_cddl` keeps private (`modules.rs:750`). [`super::chain`] reconstructs that
/// table and proves it, and [`check_chain`] is the entry point that does so -- use it in
/// preference to this one unless you specifically want the merged document. What is left here is
/// the raw, unattributed resolution.
#[derive(Debug)]
pub struct Resolved {
    /// The concatenation of every loaded module's source, in load order.
    pub source: String,
    /// The flat, merged, renamed AST, usable exactly like a single-file one.
    pub cddl: Cddl,
    /// The start rule from the top-level file's own `;# start` directive, if it has one.
    pub start_rule: Option<String>,
    /// Warnings from resolution, plus any from validating the merged AST.
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolution warnings plus the merged AST's own validation findings, all with merged-source
/// spans.
///
/// Shared by [`resolve`] and [`check_chain`] so the two cannot drift: the only difference between
/// them is what happens to the spans afterwards.
fn resolution_diagnostics(resolution: &ModuleResolution, config: &Config) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = resolution
        .warnings
        .iter()
        .map(|warning| {
            Diagnostic::new(
                Severity::from(warning.level.clone()),
                Source::CddlModules,
                warning.span.start..warning.span.end,
                warning.message.clone(),
            )
        })
        .collect();

    // The merged AST is validated as a whole: a rule imported from one module can only be checked
    // against a definition in another once both are present.
    diagnostics.extend(
        validate_with_config(&resolution.cddl, config)
            .iter()
            .map(|diagnostic| {
                Diagnostic::new(
                    Severity::from(diagnostic.level.clone()),
                    Source::CddlModules,
                    diagnostic.span.start..diagnostic.span.end,
                    diagnostic.message.clone(),
                )
            }),
    );
    diagnostics
}

/// Resolve `path`'s module directives and validate the merged result.
///
/// `search_paths` are searched in order for a bare module name. The file's own directory is *not*
/// added implicitly -- pass it if you want it, which is what the IDE does for the file being
/// edited. `tps_cddl::ModuleSearchPath::from_env_and_cwd` is deliberately not used: the process
/// working directory of a GUI launched from a desktop menu is meaningless, and
/// `CDDL_INCLUDE_PATH` leaking in would make the same project behave differently per shell.
pub fn resolve(path: &Path, search_paths: &[PathBuf], config: &Config) -> Result<Resolved> {
    let search_path = ModuleSearchPath::new(search_paths.to_vec());
    let resolution = resolve_modules(path, &search_path)
        .map_err(|error| CoreError::CddlModules(error.to_string()))?;

    let mut diagnostics = resolution_diagnostics(&resolution, config);
    crate::diag::sort(&mut diagnostics);

    Ok(Resolved {
        source: resolution.source,
        cddl: resolution.cddl,
        start_rule: resolution.start_rule,
        diagnostics,
    })
}

/// Everything known about an include chain, with every diagnostic on the file it belongs to.
///
/// The never-fails, file-attributed counterpart to [`Resolved`], and what the CDDL tab's
/// `Syntax Check` reports.
#[derive(Debug)]
pub struct ChainCheck {
    /// The files reached from the root, in inner-tab order, with their proved regions.
    ///
    /// [`Chain::diagnostics`] has been drained into [`ChainCheck::diagnostics`], so this is the
    /// file list and nothing else -- iterate one or the other, not both.
    pub chain: Chain,
    /// The merged AST, for start-rule resolution and for Phase 6's tracer. `None` when the chain
    /// did not resolve.
    pub cddl: Option<Cddl>,
    /// The root file's own `;# start`, if it declares one.
    pub start_rule: Option<String>,
    /// Every finding, sorted. Spans are **file-local** and [`Diagnostic::file`] is set wherever
    /// attribution succeeded, so a caller needs no knowledge of the merged coordinate space.
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolve, validate, and attribute every diagnostic to the file it came from.
///
/// Never fails: a chain that does not resolve still yields the file list the walk found and a
/// diagnostic saying what stopped it, which is what keeps a missing include on its own inner tab
/// instead of emptying the window.
///
/// `search_paths` must already have `path`'s parent prepended if that is wanted, for the reason
/// given on [`super::chain::walk`] -- and it must be the *same* list passed to the walk and the
/// resolver, or the two disagree about which files exist.
///
/// # Reading from disk
///
/// This reads every file in the chain from disk, so unsaved edits in an editor are not reflected.
/// A caller showing a dirty buffer must not present these results as describing what is on
/// screen: every merged span would be displaced by the user's edits, and attribution -- though
/// internally consistent -- would point at the wrong line of the right file, which is worse than
/// no attribution because it looks authoritative. Fixing that needs an in-memory-overlay entry
/// point in `tps_cddl`; until then, check that the buffers are clean first.
pub fn check_chain(path: &Path, search_paths: &[PathBuf], config: &Config) -> ChainCheck {
    let mut chain = chain::walk(path, search_paths);
    let mut diagnostics = std::mem::take(&mut chain.diagnostics);

    let search_path = ModuleSearchPath::new(search_paths.to_vec());
    let resolution = match resolve_modules(path, &search_path) {
        Ok(resolution) => resolution,
        Err(error) => {
            let diagnostic = module_diagnostic(&error, path);
            // Not pushed unconditionally, because [`super::chain::walk`] has usually reported the
            // same thing already and reported it *better*: on the directive line that named the
            // module, where [`module_diagnostic`] can only offer `0..0`. Both went into the lower
            // window as two rows carrying one identical sentence, which reads as two problems.
            // Compared on the message because that is what a reader sees as the duplicate; the
            // spans differ by construction, which is the whole reason one of them is redundant.
            if !diagnostics
                .iter()
                .any(|existing| existing.message == diagnostic.message)
            {
                diagnostics.push(diagnostic);
            }
            crate::diag::sort(&mut diagnostics);
            return ChainCheck {
                chain,
                cddl: None,
                start_rule: None,
                diagnostics,
            };
        }
    };

    chain.locate(&resolution.source);
    diagnostics.extend(
        resolution_diagnostics(&resolution, config)
            .into_iter()
            .map(|diagnostic| attribute(&chain, diagnostic)),
    );
    crate::diag::sort(&mut diagnostics);

    ChainCheck {
        chain,
        cddl: Some(resolution.cddl),
        start_rule: resolution.start_rule,
        diagnostics,
    }
}

/// Rebase one merged-source diagnostic into the file it came from.
///
/// A diagnostic that cannot be attributed is returned with its merged span untouched and no file.
/// That is deliberately not an error: an unattributed row is still worth showing, and the span is
/// still internally consistent for anyone holding the merged source.
fn attribute(chain: &Chain, diagnostic: Diagnostic) -> Diagnostic {
    match chain.attribute(diagnostic.span.clone()) {
        Some((index, span)) => Diagnostic {
            span,
            file: Some(chain.files[index].path.clone()),
            ..diagnostic
        },
        None => diagnostic,
    }
}

/// Turn a resolution failure into a diagnostic, keeping the typed structure `.to_string()` throws
/// away.
///
/// Only two variants carry a span this code can trust. `ModuleError::Parse`'s span comes from
/// parsing one file's own text, so it is exact in that file's coordinates. Everything else is
/// raised part-way through building the merged source, so its span indexes a document that was
/// never completed -- reporting it would point at an arbitrary line. Those cases are covered more
/// precisely by [`super::chain::walk`], which computes an unresolvable or unreadable module's
/// position from the directive line in the file that named it, and whose diagnostics the caller
/// already holds.
fn module_diagnostic(error: &ModuleError, root: &Path) -> Diagnostic {
    match error {
        ModuleError::Parse { path, source } => {
            Diagnostic::error(Source::CddlModules, clamp(source.span()), source.message())
                .in_file(path)
        }
        ModuleError::Io { path, .. } => {
            Diagnostic::error(Source::CddlModules, 0..0, error.to_string()).in_file(path)
        }
        ModuleError::Circular { cycle, .. } => {
            let route: Vec<String> = cycle
                .iter()
                .map(|path| {
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            Diagnostic::error(
                Source::CddlModules,
                0..0,
                format!("circular `;# include`: {}", route.join(" -> ")),
            )
            .in_file_opt(cycle.first().map(PathBuf::as_path).or(Some(root)))
        }
        other => Diagnostic::error(Source::CddlModules, 0..0, other.to_string()).in_file(root),
    }
}

/// The outcome of resolving which rule validation should start from.
///
/// Carries *why* as well as *what*, because every interesting case here is one the user needs
/// told about: see [`StartRule::note`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRule<'a> {
    /// The rule to validate against. `None` only for a schema with no rules at all.
    pub name: Option<&'a str>,
    /// A requested name that was not used because the schema defines no such rule.
    pub rejected: Option<String>,
    /// Whether `name` came from falling back to the first rule rather than from a request.
    pub defaulted: bool,
}

impl StartRule<'_> {
    /// A warning describing anything surprising about this resolution, for the diagnostics pane.
    ///
    /// `span` is where to point -- the `;# start` line, or the start-rule text box's own document.
    /// `None` when the resolution was unremarkable.
    pub fn note(&self, span: Range<usize>) -> Option<Diagnostic> {
        let rejected = self.rejected.as_deref()?;
        let message = match self.name {
            Some(used) => format!(
                "no rule named `{rejected}`; validating against `{used}` instead. \
                 `;# include` merges rules without a module qualifier, so a dotted name only \
                 exists under `;# import ... as`."
            ),
            None => format!("no rule named `{rejected}`, and the schema defines no rules"),
        };
        Some(Diagnostic::warning(Source::Cddl, span, message))
    }
}

/// The rule validation should start from.
///
/// In precedence order, taking the first candidate the schema actually defines:
///
/// 1. `override_rule` -- the start-production text box (`UI_DESIGN.md:114`).
/// 2. `directive` -- the top-level file's own `;# start`.
/// 3. `directive`'s last dotted segment.
/// 4. the first rule in the document, which RFC 8610 §3.1 makes the entry point anyway.
///
/// # Why a candidate is checked against the schema at all
///
/// Because the project's own flagship fixture needs it. `gp_eap.cddl:17` and `rfc9711.cddl:10`
/// both declare `;# start rfc8392.Claims-Set`, but both reach `rfc8392` through `;# include`,
/// which merges rules **without** a module qualifier -- only `;# import ... as rfc8392` creates
/// that prefix. So the name those files ask for does not exist in the 254-rule merged schema;
/// `Claims-Set` does. Step 3 is the exact inverse of what `include` did to the name, not a guess.
///
/// Handing an undefined name straight through is also worse than it sounds:
/// `tps_cddl::check_cbor_traced` reports an unresolvable root as *"expected a non-empty CDDL
/// schema, got empty schema"*, which is actively misleading for a schema with 254 rules.
///
/// The old frontend did none of this -- it offered no default at all, so a first-time `Validate`
/// against a multi-rule schema simply failed.
pub fn start_rule<'a>(
    cddl: &'a Cddl,
    directive: Option<&'a str>,
    override_rule: Option<&'a str>,
) -> StartRule<'a> {
    let defines = |name: &str| cddl.rules.iter().any(|rule| rule.name == name);
    let requested = override_rule
        .or(directive)
        .map(str::trim)
        .filter(|name| !name.is_empty());

    if let Some(requested) = requested {
        if defines(requested) {
            return StartRule {
                name: cddl
                    .rules
                    .iter()
                    .find(|rule| rule.name == requested)
                    .map(|rule| rule.name.as_str()),
                rejected: None,
                defaulted: false,
            };
        }
        // Undo the module qualifier `include` never applied.
        if let Some((_, bare)) = requested.rsplit_once('.')
            && defines(bare)
        {
            return StartRule {
                name: cddl
                    .rules
                    .iter()
                    .find(|rule| rule.name == bare)
                    .map(|rule| rule.name.as_str()),
                rejected: Some(requested.to_owned()),
                defaulted: false,
            };
        }
        return StartRule {
            name: cddl.rules.first().map(|rule| rule.name.as_str()),
            rejected: Some(requested.to_owned()),
            defaulted: true,
        };
    }

    StartRule {
        name: cddl.rules.first().map(|rule| rule.name.as_str()),
        rejected: None,
        defaulted: true,
    }
}

/// Convert a tokenize or parse failure into a diagnostic, keeping its span.
///
/// Public because the IDE reports every failure through the diagnostics pane, including the ones
/// that arrive as a `CoreError` from a fallible entry point.
pub fn diagnostic_for(error: &CoreError) -> Diagnostic {
    match error {
        CoreError::CddlLex { span, message } | CoreError::CddlParse { span, message } => {
            Diagnostic::error(Source::Cddl, span.clone(), message.clone())
        }
        CoreError::CddlModules(message) => {
            Diagnostic::error(Source::CddlModules, 0..0, message.clone())
        }
        // Everything else has no CDDL span to offer; report it against the whole document rather
        // than inventing a position.
        other => Diagnostic::error(Source::Cddl, 0..0, other.to_string()),
    }
}

fn lex_error(error: ParseError) -> CoreError {
    CoreError::CddlLex {
        span: clamp(error.span()),
        message: error.message(),
    }
}

fn parse_error(error: ParseError) -> CoreError {
    CoreError::CddlParse {
        span: clamp(error.span()),
        message: error.message(),
    }
}

/// Guard against an inverted range, which would panic on any slicing use.
fn clamp(span: Range<usize>) -> Range<usize> {
    if span.start <= span.end {
        span
    } else {
        span.end..span.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "claims = {\n  ? 1: tstr,\n  * uint => any,\n}\n";

    fn all() -> Config {
        Config::all()
    }

    #[test]
    fn a_valid_schema_parses_and_validates_cleanly() {
        let (cddl, diagnostics) = validate(GOOD, &all()).expect("valid");
        assert_eq!(cddl.rules.len(), 1);
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    /// The headline fix: a syntax error reports where it is, not line 1 column 1.
    #[test]
    fn a_tokenize_error_reports_its_own_position() {
        let source = "a = uint\nb = !!!\n";
        let result = check(source, &all());
        assert!(result.cddl.is_none());
        assert_eq!(result.diagnostics.len(), 1);

        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.source, Source::Cddl);
        let (start, _) = diagnostic.position(source);
        assert_eq!(start.line, 2, "reported line {} instead of 2", start.line);
        assert!(
            start.column > 1,
            "reported column {} -- the old line-1-column-1 fallback",
            start.column
        );
    }

    /// The regression test for the char-vs-byte bug: a non-ASCII comment before the error must
    /// not shift the reported position.
    #[test]
    fn a_non_ascii_comment_does_not_shift_the_reported_position() {
        // "é" is two bytes, so a char-indexed conversion drifts by one per occurrence.
        let source = "; ééééé\na = !!!\n";
        let result = check(source, &all());
        let diagnostic = &result.diagnostics[0];
        let (start, _) = diagnostic.position(source);
        assert_eq!(
            start.line, 2,
            "a byte/char mix-up would move this off line 2"
        );

        // And the span must land on a character boundary, so an editor can slice with it.
        assert!(
            source.is_char_boundary(diagnostic.span.start)
                && source.is_char_boundary(diagnostic.span.end),
            "span {:?} is not on a char boundary",
            diagnostic.span
        );
    }

    #[test]
    fn every_diagnostic_span_lies_within_the_source() {
        for source in [
            "a = !!!\n",
            "a = uint\nb = \n",
            "",
            "a = { b: !!! }\n",
            "; just a comment\n",
        ] {
            let result = check(source, &all());
            for diagnostic in &result.diagnostics {
                assert!(
                    diagnostic.span.start <= diagnostic.span.end,
                    "{source:?}: inverted span {:?}",
                    diagnostic.span
                );
                assert!(
                    diagnostic.span.end <= source.len(),
                    "{source:?}: span {:?} exceeds {} bytes",
                    diagnostic.span,
                    source.len()
                );
            }
        }
    }

    /// `span.end` must survive, since it is what makes a ranged squiggle possible.
    #[test]
    fn validation_diagnostics_keep_both_ends_of_their_span() {
        // `undefined-rule` is referenced but never defined, which validation reports.
        let (_, diagnostics) = validate("a = undefined_rule\n", &all()).expect("parses");
        let ranged = diagnostics.iter().find(|d| !d.is_point());
        assert!(
            ranged.is_some(),
            "no diagnostic kept a non-empty span: {diagnostics:?}"
        );
    }

    #[test]
    fn parse_returns_an_error_rather_than_a_diagnostic() {
        let error = parse("a = !!!\n").expect_err("not CDDL");
        assert!(
            matches!(
                error,
                CoreError::CddlLex { .. } | CoreError::CddlParse { .. }
            ),
            "unexpected error: {error:?}"
        );
        // And converting it loses nothing.
        let diagnostic = diagnostic_for(&error);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(!diagnostic.message.is_empty());
    }

    #[test]
    fn validate_ast_does_not_reparse() {
        let cddl = parse(GOOD).expect("parses");
        let from_ast = validate_ast(&cddl, &all());
        let (_, from_source) = validate(GOOD, &all()).expect("parses");
        assert_eq!(from_ast, from_source);
    }

    #[test]
    fn the_rfc_flags_change_what_is_accepted() {
        // `.plus` is an RFC 9165 control operator: a warning under strict RFC 8610, fine with the
        // extension enabled.
        let source = "a = 1 .plus 2\n";
        let strict = check(source, &Config::strict());
        let lenient = check(source, &Config::all());
        assert!(
            strict.diagnostics.len() > lenient.diagnostics.len(),
            "strict {:?} vs all {:?}",
            strict.diagnostics,
            lenient.diagnostics
        );
    }

    #[test]
    fn the_start_rule_falls_back_to_the_first_rule() {
        let cddl = parse("first = uint\nsecond = tstr\n").expect("parses");
        let resolved = start_rule(&cddl, None, None);
        assert_eq!(resolved.name, Some("first"));
        assert!(resolved.defaulted);
        assert_eq!(resolved.rejected, None);

        assert_eq!(start_rule(&cddl, Some("second"), None).name, Some("second"));
        assert_eq!(
            start_rule(&cddl, Some("second"), Some("first")).name,
            Some("first")
        );
    }

    #[test]
    fn a_blank_start_rule_override_is_ignored() {
        let cddl = parse("first = uint\n").expect("parses");
        assert_eq!(start_rule(&cddl, None, Some("   ")).name, Some("first"));
        assert_eq!(start_rule(&cddl, Some("  "), Some("")).name, Some("first"));
    }

    /// The `gp_eap.cddl` case: a dotted name that `;# include` never created.
    #[test]
    fn a_dotted_start_rule_falls_back_to_its_bare_name() {
        let cddl = parse("Claims-Set = { 1: tstr }\n").expect("parses");
        let resolved = start_rule(&cddl, Some("rfc8392.Claims-Set"), None);
        assert_eq!(resolved.name, Some("Claims-Set"));
        assert_eq!(resolved.rejected.as_deref(), Some("rfc8392.Claims-Set"));
        assert!(
            !resolved.defaulted,
            "stripping the qualifier is a resolution, not a default"
        );

        // And the user is told, rather than silently getting a different rule.
        let note = resolved.note(0..10).expect("a warning");
        assert_eq!(note.severity, Severity::Warning);
        assert!(note.message.contains("rfc8392.Claims-Set"));
        assert!(note.message.contains("Claims-Set"));
    }

    #[test]
    fn an_undefined_start_rule_defaults_and_says_so() {
        let cddl = parse("first = uint\n").expect("parses");
        let resolved = start_rule(&cddl, None, Some("nope"));
        assert_eq!(resolved.name, Some("first"));
        assert_eq!(resolved.rejected.as_deref(), Some("nope"));
        assert!(resolved.defaulted);
        assert!(resolved.note(0..0).is_some());
    }

    /// A name the schema really does define must produce no warning at all.
    #[test]
    fn a_resolvable_start_rule_produces_no_note() {
        let cddl = parse("first = uint\nsecond = tstr\n").expect("parses");
        assert!(start_rule(&cddl, None, Some("second")).note(0..0).is_none());
        assert!(start_rule(&cddl, None, None).note(0..0).is_none());
    }

    #[test]
    fn an_empty_schema_resolves_to_no_rule() {
        let cddl = parse("; nothing but a comment\n").expect("parses");
        let resolved = start_rule(&cddl, None, Some("whatever"));
        assert_eq!(resolved.name, None);
        assert!(
            resolved
                .note(0..0)
                .expect("a warning")
                .message
                .contains("no rules")
        );
    }

    #[test]
    fn the_gp_eap_chain_resolves_and_reports_its_start_rule() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("examples");
        let resolved = resolve(
            &examples.join("gp_eap.cddl"),
            std::slice::from_ref(&examples),
            &all(),
        )
        .expect("gp_eap.cddl resolves");

        // The chain is gp_eap -> rfc9711 -> rfc8392, so the merged source must exceed any one file.
        let own = std::fs::read_to_string(examples.join("gp_eap.cddl")).expect("read gp_eap.cddl");
        assert!(
            resolved.source.len() > own.len(),
            "merged source ({} bytes) is no larger than gp_eap.cddl ({} bytes)",
            resolved.source.len(),
            own.len()
        );
        assert!(
            resolved.cddl.rules.len() > 20,
            "expected the merged rule set"
        );

        for diagnostic in &resolved.diagnostics {
            assert!(
                diagnostic.span.end <= resolved.source.len(),
                "{diagnostic:?} is out of range for the merged source"
            );
            assert_eq!(diagnostic.source, Source::CddlModules);
        }

        // The start rule is dotted, naming a rule the `include` chain merged unqualified.
        let resolved_start = start_rule(&resolved.cddl, resolved.start_rule.as_deref(), None);
        assert_eq!(
            resolved_start.name,
            Some("Claims-Set"),
            "`;# start rfc8392.Claims-Set` should resolve to the bare merged name"
        );
        assert_eq!(
            resolved_start.rejected.as_deref(),
            Some("rfc8392.Claims-Set"),
            "the qualifier really was stripped, so the user must be told"
        );
    }

    #[test]
    fn a_missing_module_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tps-cbor-cddl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("broken.cddl");
        std::fs::write(&path, ";# import definitely_not_a_module\na = uint\n").expect("write");

        let error =
            resolve(&path, std::slice::from_ref(&dir), &all()).expect_err("module is missing");
        assert!(
            matches!(error, CoreError::CddlModules(_)),
            "unexpected error: {error:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolving_a_nonexistent_file_is_an_error() {
        let error =
            resolve(Path::new("/nonexistent/x.cddl"), &[], &all()).expect_err("no such file");
        assert!(matches!(error, CoreError::CddlModules(_)), "{error:?}");
    }

    // ─── check_chain, check_module, lex_check ─────────────────────────────────

    fn examples() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("examples")
    }

    fn sample_project() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("examples/sample_project")
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tps-cbor-validate-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Every merged-source diagnostic must come back with a file and a span that is in range for
    /// *that* file -- the whole point of the attribution design.
    #[test]
    fn the_gp_eap_chain_attributes_every_diagnostic_to_a_file() {
        let dir = examples();
        let checked = check_chain(&dir.join("gp_eap.cddl"), std::slice::from_ref(&dir), &all());

        assert!(checked.chain.attributed, "the partition must prove");
        assert_eq!(checked.chain.files.len(), 4);
        assert!(checked.cddl.is_some());

        for diagnostic in &checked.diagnostics {
            let path = diagnostic
                .file
                .as_deref()
                .unwrap_or_else(|| panic!("unattributed: {diagnostic:?}"));
            let index = checked
                .chain
                .index_of(path)
                .unwrap_or_else(|| panic!("{} is not in the chain", path.display()));
            assert!(
                diagnostic.span.end <= checked.chain.files[index].text.len(),
                "{diagnostic:?} is out of range for {}",
                path.display()
            );
        }
    }

    /// The known-unsatisfiable `;# start rfc8392.Claims-Set` in `gp_eap.cddl:17`: `;# include`
    /// merges rules unqualified, so no rule can carry a dot. This warning appearing is the pass
    /// condition, not noise.
    #[test]
    fn the_dotted_start_directive_still_resolves_to_the_bare_rule() {
        let dir = examples();
        let checked = check_chain(&dir.join("gp_eap.cddl"), std::slice::from_ref(&dir), &all());
        let cddl = checked.cddl.as_ref().expect("the chain resolves");
        let resolved = start_rule(cddl, checked.start_rule.as_deref(), None);
        assert_eq!(resolved.name, Some("Claims-Set"));
        assert_eq!(resolved.rejected.as_deref(), Some("rfc8392.Claims-Set"));
    }

    /// `sample_project/rfc8392.cddl:4` names an `rfc9052` that is not there. The report must land
    /// on that directive line, in that file, so the IDE can put it on the right inner tab -- and
    /// the file list must survive, because an empty window is not a diagnosis.
    #[test]
    fn a_missing_include_is_a_diagnostic_on_its_own_directive_line() {
        let dir = sample_project();
        let root = dir.join("rfc8392.cddl");
        let checked = check_chain(&root, std::slice::from_ref(&dir), &all());

        assert!(checked.cddl.is_none(), "the chain cannot resolve");
        assert_eq!(checked.chain.files.len(), 1, "the root is still listed");

        let source = &checked.chain.files[0].text;
        let found = checked
            .diagnostics
            .iter()
            .find(|d| {
                d.severity == Severity::Error
                    && d.file.as_deref() == Some(root.as_path())
                    && d.position(source).0.line == 4
            })
            .unwrap_or_else(|| panic!("{:#?}", checked.diagnostics));
        assert!(found.message.contains("rfc9052"), "{}", found.message);

        // Once, not twice. The walk and the failed resolution both notice the absent module and
        // say the same sentence about it, and only the walk's copy knows which line asked for it.
        let saying_it = checked
            .diagnostics
            .iter()
            .filter(|d| d.message == found.message)
            .count();
        assert_eq!(saying_it, 1, "{:#?}", checked.diagnostics);
    }

    #[test]
    fn a_circular_include_names_every_file_in_the_cycle() {
        let dir = temp_dir("circular");
        std::fs::write(dir.join("a.cddl"), ";# include b\na = uint\n").expect("write a");
        std::fs::write(dir.join("b.cddl"), ";# include a\nb = tstr\n").expect("write b");

        let checked = check_chain(&dir.join("a.cddl"), std::slice::from_ref(&dir), &all());
        assert!(checked.cddl.is_none());
        // The walk still found both files, so both get an inner tab.
        assert_eq!(checked.chain.files.len(), 2);

        let cycle = checked
            .diagnostics
            .iter()
            .find(|d| d.message.contains("circular"))
            .unwrap_or_else(|| panic!("{:#?}", checked.diagnostics));
        assert!(cycle.message.contains("a.cddl"), "{}", cycle.message);
        assert!(cycle.message.contains("b.cddl"), "{}", cycle.message);
        assert!(cycle.file.is_some(), "the cycle must name a file");
    }

    #[test]
    fn check_chain_on_a_nonexistent_file_reports_rather_than_failing() {
        let checked = check_chain(Path::new("/nonexistent/x.cddl"), &[], &all());
        assert!(checked.cddl.is_none());
        assert!(checked.chain.files.is_empty());
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }

    /// `check_module`'s whole reason for existing: `rfc8392.cddl` legitimately references rules
    /// `rfc9052.cddl` defines, and plain `check` paints about a dozen red squiggles on it.
    #[test]
    fn a_module_referencing_a_sibling_module_is_not_reported_as_an_error() {
        let source =
            std::fs::read_to_string(examples().join("rfc8392.cddl")).expect("read rfc8392.cddl");

        let strict = check(&source, &all());
        assert!(
            strict
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "the fixture no longer exercises this: plain `check` found no errors"
        );

        let lenient = check_module(&source, &all());
        assert!(lenient.cddl.is_some(), "it parses, so the AST is available");
        let errors: Vec<_> = lenient
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            lenient.diagnostics.len(),
            strict.diagnostics.len(),
            "demotion must not drop findings, only re-grade them"
        );
    }

    /// The inverse, so the demotion cannot over-apply: with no directives there is no sibling
    /// module to blame, so an undefined reference stays an error.
    #[test]
    fn a_self_contained_file_still_reports_an_undefined_rule_as_an_error() {
        let result = check_module("a = not_a_rule\n", &all());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{:#?}",
            result.diagnostics
        );
    }

    /// A syntax error arrives before the validator runs, so it is red while typing even in a file
    /// full of directives.
    #[test]
    fn check_module_does_not_demote_a_syntax_error() {
        let result = check_module(";# include rfc9052\na = !!!\n", &all());
        assert!(result.cddl.is_none());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn lex_check_reports_a_position_and_never_parses() {
        assert!(lex_check(GOOD).is_none());

        let source = "a = uint\nb = !!!\n";
        let diagnostic = lex_check(source).expect("a lex error");
        assert_eq!(diagnostic.severity, Severity::Error);
        let (start, _) = diagnostic.position(source);
        assert_eq!(start.line, 2);

        // `build_cddl` is documented to panic on malformed-but-grammatical input; `lex_check`
        // must report a *grammatical* document as clean rather than parsing it to find out.
        assert!(lex_check("a = 99999999999999999999999999\n").is_none());
    }

    /// The backstop for the paths that must parse. `build_cddl` returns a `ParseError` for an
    /// out-of-range integer literal, so this exercises the guard's sibling path and asserts the
    /// process is still here afterwards.
    #[test]
    fn a_build_cddl_failure_becomes_a_diagnostic_rather_than_killing_the_process() {
        let source = "a = 99999999999999999999999999\n";
        let result = check(source, &all());
        assert!(result.cddl.is_none());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].severity, Severity::Error);
        assert!(result.diagnostics[0].span.end <= source.len());

        assert!(parse(source).is_err());
        // Still running.
        assert!(check(GOOD, &all()).cddl.is_some());
    }

    /// Directly exercises the panic guard, independently of whether any CDDL input currently
    /// provokes one. Without `guarded`, this test aborts the whole test binary.
    #[test]
    fn the_parser_guard_converts_a_panic_into_an_error() {
        assert_eq!(guarded(|| 1 + 1), Ok(2));

        // Silence the default hook for this one deliberate panic, so its backtrace is not
        // mistaken for a real failure in the test output.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let caught = guarded(|| panic!("as build_cddl does"));
        std::panic::set_hook(previous);

        assert_eq!(caught, Err(()));
    }
}
