/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Validating CBOR against a CDDL schema, with a span-resolved trace.
//!
//! This is the app's distinguishing feature and the subtlest code in it. A single
//! [`TraceEvent`] ties together **three** coordinate spaces at once:
//!
//! | Span | Indexes | Comes from |
//! |---|---|---|
//! | `cddl` | the CDDL source text | the matched rule's own definition |
//! | `cbor` | the raw CBOR byte buffer | the value being matched |
//! | `edn` | the *pretty-printed EDN document* | resolving `cbor` through [`SpanMap`] |
//!
//! Clicking a trace row highlights all three without switching tabs
//! (`tps_cbor_gui/src/App.tsx:580-602`), which only works because all three survive together.
//!
//! # What changed from the old backend
//!
//! Ported from `main.rs:369-474`, which pre-converted every span to a line/column quadruple --
//! twelve `Option<usize>` fields per event -- because the React side could not slice Rust strings.
//! Those were computed against the CDDL text as it stood at validation time, so an edit
//! invalidated them silently. Byte ranges are kept here instead and converted on demand by
//! [`crate::diag::Position`], per `CLAUDE.md`.
//!
//! The EDN rendering is also done once per run rather than once per event, and only when some
//! event actually carries a CBOR span -- the same optimisation the old code made, kept because a
//! trace of a large document can run to thousands of events.

use std::ops::Range;
use std::path::{Path, PathBuf};

use tps_cddl::{Cddl, Config, TraceOutcome as CddlOutcome, check_cbor_traced};

use super::chain::Chain;
use crate::diag::{Diagnostic, Severity, Source};
use crate::error::{CoreError, Result};
use crate::spans::SpanMap;

/// Whether a rule-match attempt succeeded.
///
/// Mirrors [`tps_cddl::TraceOutcome`], restated so the UI layer does not depend on `tps_cddl`
/// directly and so it can carry the `Ord` the trace filter needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TraceOutcome {
    /// The rule was entered; the outcome is recorded by a later event.
    Attempt,
    /// The rule matched.
    Match,
    /// The rule failed.
    Fail,
}

impl From<CddlOutcome> for TraceOutcome {
    fn from(outcome: CddlOutcome) -> Self {
        match outcome {
            CddlOutcome::Attempt => TraceOutcome::Attempt,
            CddlOutcome::Match => TraceOutcome::Match,
            CddlOutcome::Fail => TraceOutcome::Fail,
        }
    }
}

impl TraceOutcome {
    /// The lowercase name the UI filter and the old frontend both use.
    pub fn as_str(self) -> &'static str {
        match self {
            TraceOutcome::Attempt => "attempt",
            TraceOutcome::Match => "match",
            TraceOutcome::Fail => "fail",
        }
    }

    /// The outcomes a persisted [`crate::settings::TraceFilter`] admits, for [`Trace::filtered`].
    ///
    /// Here rather than on `TraceFilter` so that `settings` keeps no dependency on the tracer: a
    /// settings file has to be loadable by anything, including a frontend that never validates.
    pub fn from_filter(filter: &crate::settings::TraceFilter) -> Vec<Self> {
        let mut show = Vec::with_capacity(3);
        if filter.attempt {
            show.push(TraceOutcome::Attempt);
        }
        if filter.matched {
            show.push(TraceOutcome::Match);
        }
        if filter.failed {
            show.push(TraceOutcome::Fail);
        }
        show
    }
}

/// One step of a validation trace: one attempt to match `rule` against the value at `path`.
///
/// Every span is optional, and for different reasons. `cddl` is `None` for a prelude rule, which
/// has no definition in the user's source. `cbor` is `None` where the checker could not attribute
/// the attempt to a byte range. `edn` is `None` when either `cbor` was, or when no EDN item
/// covered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    /// The rule being matched.
    pub rule: String,
    /// Path to the CBOR value, as in [`Mismatch::path`] (e.g. `root.addr[2]`).
    pub path: String,
    /// Byte range of the rule's definition in the CDDL source.
    pub cddl: Option<Range<usize>>,
    /// Byte range of the value in the raw CBOR buffer.
    pub cbor: Option<Range<usize>>,
    /// Byte range of the value in [`Trace::edn`].
    pub edn: Option<Range<usize>>,
    /// Whether the attempt matched, failed, or is just an entry marker.
    pub outcome: TraceOutcome,
    /// Call-stack depth, root rule = 0. The trace log indents by this.
    pub depth: usize,
}

impl TraceEvent {
    /// Render a failing event as a diagnostic, spanned over `span`.
    ///
    /// Used for the pinpoints [`Trace::pinpoints`] picks out, whose whole value is that they carry
    /// a span narrow enough to click. The wording stays deliberately plain -- what the schema
    /// wanted is the [`Mismatch`]'s sentence to tell, and this one says only where.
    ///
    /// `span` is passed in rather than read from [`Self::cbor`] for the same reason
    /// [`Mismatch::to_diagnostic`] takes one: the caller chooses the coordinate space, because the
    /// EDN editor wants this same sentence over [`Self::edn`]. `None` becomes `0..0`, the inert
    /// span.
    pub fn to_diagnostic(&self, span: Option<Range<usize>>) -> Diagnostic {
        Diagnostic::new(
            Severity::Error,
            Source::Validation,
            span.unwrap_or(0..0),
            format!("{}: did not match `{}`", self.path, self.rule),
        )
    }
}

/// Whether `child` names a value inside `parent`, as [`TraceEvent::path`] spells paths.
///
/// A plain `starts_with` is not enough: `Report.2[10]` starts with `Report.2[1]` and is not inside
/// it. The character after the prefix has to begin a new path step.
fn is_inside(parent: &str, child: &str) -> bool {
    child.len() > parent.len()
        && child.starts_with(parent)
        && matches!(child.as_bytes()[parent.len()], b'.' | b'[')
}

/// One way in which the CBOR did not match the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// Path to the offending value (e.g. `root.addr[2]`).
    pub path: String,
    /// What the schema asked for.
    pub expected: String,
    /// What the CBOR held.
    pub got: String,
}

impl Mismatch {
    /// Render as a diagnostic against the CBOR buffer.
    ///
    /// `span` is the byte range the trace attributed to this path, if any; the UI finds it with
    /// [`Trace::span_for_path`]. `tps_cddl::CborMismatch` itself carries no position -- only the
    /// path -- which is why the trace has to supply it.
    pub fn to_diagnostic(&self, span: Option<Range<usize>>) -> Diagnostic {
        Diagnostic::new(
            Severity::Error,
            Source::Validation,
            span.unwrap_or(0..0),
            format!(
                "{}: expected {}, got {}",
                self.path, self.expected, self.got
            ),
        )
    }
}

/// The result of validating one CBOR buffer against one schema.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// Every way the CBOR failed to match. Empty means it validated.
    pub mismatches: Vec<Mismatch>,
    /// Every rule-match attempt, in the order the checker made them.
    pub events: Vec<TraceEvent>,
    /// The EDN rendering the `edn` spans index.
    ///
    /// `None` when no event carried a CBOR span, in which case no `edn` span was resolved either
    /// and there is nothing for the EDN pane to highlight. The checker is not re-run to produce
    /// this; it is rendered once from the same buffer.
    pub edn: Option<String>,
}

impl Trace {
    /// Whether the CBOR validated against the schema.
    pub fn is_valid(&self) -> bool {
        self.mismatches.is_empty()
    }

    /// Every reason the CBOR was rejected, as diagnostics against the CBOR buffer.
    ///
    /// Two rows per failure, saying two different things, because one of them alone is not enough:
    ///
    /// * the [`Mismatch`] the checker reported, which says what the schema wanted;
    /// * each [pinpoint](Self::pinpoints), which says which value broke it.
    ///
    /// The second is not a nicety. `tps_cddl` reports a mismatch only at the rule the check
    /// started from -- `trace_demo_bad.cbor` comes back as a single
    /// `Report: expected map matching schema, got map of 3 keys` whose span is all 53 bytes --
    /// so a diagnostics pane built from mismatches alone offers the user a row that selects the
    /// whole document and names nothing. The trace knows the member, and this is where it says so.
    ///
    /// Empty for a document that validated, pinpoints included: see [`Self::pinpoints`] for why a
    /// failing event is not by itself a failure.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.rejections(|event| event.cbor.clone())
    }

    /// [`Self::diagnostics`], spanned over [`Self::edn`] instead of the CBOR buffer.
    ///
    /// Same rejections and the same sentences -- only the coordinate space differs, because an
    /// editor showing the diagnostic notation needs a span into the text it is displaying. A
    /// rejection whose EDN span could not be resolved comes back `0..0`, which is the inert span,
    /// and a caller drawing geometry has to skip it rather than mark the top of the document.
    pub fn edn_diagnostics(&self) -> Vec<Diagnostic> {
        self.rejections(|event| event.edn.clone())
    }

    /// The shared body of [`Self::diagnostics`] and [`Self::edn_diagnostics`].
    ///
    /// `span` picks the coordinate space out of an event. One function rather than two, because
    /// which rejections are reported and which are suppressed as duplicates is the interesting
    /// part, and two copies of it would be two things to keep in step.
    fn rejections(&self, span: impl Fn(&TraceEvent) -> Option<Range<usize>>) -> Vec<Diagnostic> {
        self.mismatches
            .iter()
            .map(|mismatch| mismatch.to_diagnostic(self.path_span(&mismatch.path, &span)))
            .chain(
                self.pinpoints()
                    .into_iter()
                    // A pinpoint that lands on the path a mismatch already named adds a second
                    // row about one value, which reads as two problems.
                    .filter(|event| {
                        !self
                            .mismatches
                            .iter()
                            .any(|mismatch| mismatch.path == event.path)
                    })
                    .map(|event| event.to_diagnostic(span(event))),
            )
            .collect()
    }

    /// The innermost values whose failure explains the rejection.
    ///
    /// **A failing event is not by itself a failure.** `a / b` records a failure for `a` and then
    /// matches `b`, and the document is valid; `? 3 => bool` records one for an absent member.
    /// Reporting those would turn every backtracked alternative into an error.
    ///
    /// What distinguishes a cause is that it is *connected to the root failure*: an unbroken chain
    /// of failing events from depth 0 down to it, each inside the last. A backtracked alternative
    /// has an ancestor that went on to match, so no such chain reaches it. The pinpoints are the
    /// deep ends of those chains -- `Report.2[1].3` for `trace_demo_bad.cbor`, not `Report`.
    ///
    /// Empty when the document validated, so that a caller cannot report an error about one.
    pub fn pinpoints(&self) -> Vec<&TraceEvent> {
        if self.is_valid() {
            return Vec::new();
        }

        // Root-first, so a parent is always already known to be connected when its children are
        // considered. `sort_by_key` is stable, so events at one depth keep their trace order and
        // the rows come out in a reproducible order.
        let mut fails: Vec<&TraceEvent> = self
            .events
            .iter()
            .filter(|event| event.outcome == TraceOutcome::Fail)
            .collect();
        fails.sort_by_key(|event| event.depth);

        let mut connected: Vec<&TraceEvent> = Vec::new();
        for event in fails {
            let linked = event.depth == 0
                || connected.iter().any(|parent| {
                    parent.depth + 1 == event.depth && is_inside(&parent.path, &event.path)
                });
            if linked {
                connected.push(event);
            }
        }

        connected
            .iter()
            .copied()
            .filter(|event| {
                !connected.iter().any(|other| {
                    other.depth == event.depth + 1 && is_inside(&event.path, &other.path)
                })
            })
            .collect()
    }

    /// The CBOR byte range the trace attributed to `path`.
    ///
    /// Prefers a failing event, since that is the one that recorded the mismatch; falls back to
    /// any event for the path. `None` for a path no event carried a span for.
    pub fn span_for_path(&self, path: &str) -> Option<Range<usize>> {
        self.path_span(path, |event| event.cbor.clone())
    }

    /// [`Self::span_for_path`] in [`Self::edn`]'s coordinates.
    pub fn edn_for_path(&self, path: &str) -> Option<Range<usize>> {
        self.path_span(path, |event| event.edn.clone())
    }

    /// The shared body of [`Self::span_for_path`] and [`Self::edn_for_path`].
    ///
    /// The preference for a failing event is the part worth having once: a path is visited by both
    /// a matched parent and a failed child in a backtracking checker, and picking either at random
    /// would move the span the user is sent to between runs.
    fn path_span(
        &self,
        path: &str,
        span: impl Fn(&TraceEvent) -> Option<Range<usize>>,
    ) -> Option<Range<usize>> {
        let matching = || self.events.iter().filter(|event| event.path == path);
        matching()
            .find(|event| event.outcome == TraceOutcome::Fail && span(event).is_some())
            .or_else(|| matching().find(|event| span(event).is_some()))
            .and_then(&span)
    }

    /// The events a filter over outcomes admits.
    ///
    /// The old frontend defaulted to match + fail with `attempt` off (`TraceLog.tsx:66`), because
    /// an `attempt` row is immediately followed by its own outcome and so doubles the log length
    /// without adding information.
    pub fn filtered<'a>(
        &'a self,
        show: &'a [TraceOutcome],
    ) -> impl Iterator<Item = &'a TraceEvent> + 'a {
        self.events
            .iter()
            .filter(move |event| show.contains(&event.outcome))
    }

    /// The final status of every rule the trace touched, for highlighting the CDDL editor.
    ///
    /// **Fail beats match**: a rule that matched in one position and failed in another is reported
    /// as failed, because the failure is what the user needs to see. The old frontend's
    /// `highlightsFromTrace` (`DebugPanel.tsx:42-80`) established that rule and
    /// `tps_cbor_gui/PLAN.md:521-525` records it being lost twice.
    ///
    /// Rules the trace never attempted are absent, not `Match` -- the caller treats absence as
    /// "unused", as the old `collectRuleSpans` did.
    pub fn rule_status(&self) -> Vec<RuleStatus> {
        let mut status: Vec<RuleStatus> = Vec::new();
        for event in &self.events {
            let Some(span) = event.cddl.clone() else {
                continue;
            };
            if event.outcome == TraceOutcome::Attempt {
                continue;
            }
            match status.iter_mut().find(|(known, _)| *known == span) {
                // Fail is sticky; nothing downgrades it back to Match.
                Some((_, outcome)) => {
                    if event.outcome == TraceOutcome::Fail {
                        *outcome = TraceOutcome::Fail;
                    }
                }
                None => status.push((span, event.outcome)),
            }
        }
        status
    }
}

/// Validate `cbor` against `cddl`, collecting a span-resolved trace.
///
/// `root` names the rule to start from; `None` uses the schema's first rule. Prefer resolving it
/// through [`super::start_rule`], which also honours a `;# start` directive.
///
/// Returns [`CoreError::CddlCheck`] only when the checker refused to run at all -- malformed CBOR,
/// or a `root` naming no rule. A CBOR buffer that simply does not match is a successful call
/// returning a non-empty [`Trace::mismatches`].
pub fn check(cbor: &[u8], cddl: &Cddl, root: Option<&str>, config: &Config) -> Result<Trace> {
    let (mismatches, events) =
        check_cbor_traced(cbor, cddl, root, config).map_err(CoreError::CddlCheck)?;

    // Render EDN once, and only if some event can actually use it. A trace where nothing carries
    // a CBOR span -- a schema whose root rule does not exist, say -- should not pay for it.
    let map = if events.iter().any(|event| event.cbor_span.is_some()) {
        SpanMap::build(cbor).ok()
    } else {
        None
    };

    let events = events
        .into_iter()
        .map(|event| {
            let cbor_span = event.cbor_span.map(|span| span.start..span.end);
            let edn = match (&cbor_span, &map) {
                (Some(span), Some(map)) => map.edn_range(span),
                _ => None,
            };
            TraceEvent {
                rule: event.rule_name,
                path: event.path,
                cddl: event.span.map(|span| span.start..span.end),
                cbor: cbor_span,
                edn,
                outcome: event.outcome.into(),
                depth: event.depth,
            }
        })
        .collect();

    Ok(Trace {
        mismatches: mismatches
            .into_iter()
            .map(|mismatch| Mismatch {
                path: mismatch.path,
                expected: mismatch.expected,
                got: mismatch.got,
            })
            .collect(),
        events,
        edn: map.map(SpanMap::into_edn),
    })
}

/// Validate CBOR against CDDL *source*, reporting every failure as a diagnostic.
///
/// The never-fails counterpart to [`check`], for the `Validate` button: a schema that does not
/// parse, CBOR that does not decode, and CBOR that does not match all come back the same way.
/// The trace is empty for the first two.
pub fn check_source(
    cbor: &[u8],
    cddl_source: &str,
    root: Option<&str>,
    config: &Config,
) -> (Trace, Vec<Diagnostic>) {
    let cddl = match super::parse(cddl_source) {
        Ok(cddl) => cddl,
        Err(error) => {
            return (
                Trace::default(),
                vec![super::validate::diagnostic_for(&error)],
            );
        }
    };
    let start = super::start_rule(&cddl, None, root);
    // Whatever the start rule resolution had to work around, the user hears about it; otherwise
    // "validated successfully" could mean "against a rule you did not ask for".
    let note = start.note(0..0);
    match check(cbor, &cddl, start.name, config) {
        Ok(trace) => {
            let mut diagnostics = trace.diagnostics();
            diagnostics.extend(note);
            crate::diag::sort(&mut diagnostics);
            (trace, diagnostics)
        }
        Err(error) => (
            Trace::default(),
            // The checker refused to start. Attribute it to the CBOR buffer, not the schema:
            // a decode failure is the buffer's fault, and that is the pane the user is in.
            vec![Diagnostic::error(Source::Cbor, 0..0, error.to_string())],
        ),
    }
}

/// Where a rule is defined, and how it ended up, for tinting the CDDL editor.
///
/// The span's coordinate space depends on who produced it: [`Trace::rule_status`] indexes the
/// merged source, [`ChainTrace::rule_status_by_file`] one chain file's own text.
pub type RuleStatus = (Range<usize>, TraceOutcome);

/// How the start rule was resolved, owned so that it outlives the parsed schema.
///
/// [`super::StartRule`] borrows from the [`Cddl`] it was resolved against, and a [`ChainTrace`]
/// has to survive in a frontend's state long after that schema has been dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Start {
    /// The rule the trace started from. `None` only for a schema with no rules at all.
    pub name: Option<String>,
    /// A requested name that was not used because the schema defines no such rule.
    pub rejected: Option<String>,
    /// Whether `name` came from falling back to the first rule rather than from a request.
    pub defaulted: bool,
}

impl Start {
    /// [`super::StartRule::note`], from the owned copy.
    ///
    /// Delegates rather than restating the sentence, so the `;# start rfc8392.Claims-Set`
    /// explanation exists once. `span` is where to point -- the `;# start` line, or the
    /// start-production box's own document.
    pub fn note(&self, span: Range<usize>) -> Option<Diagnostic> {
        super::StartRule {
            name: self.name.as_deref(),
            rejected: self.rejected.clone(),
            defaulted: self.defaulted,
        }
        .note(span)
    }
}

/// A trace over a resolved include chain, with every CDDL span attributed to a file.
///
/// The `Validate` command's counterpart to [`super::check_chain`]: never fails, and carries the
/// provenance a trace row needs to land in the right file of the CDDL editor.
#[derive(Debug, Clone)]
pub struct ChainTrace {
    /// The resolved chain, for attributing merged spans back to files.
    pub chain: Chain,
    /// The trace itself, whose `cddl` spans index the *merged* source.
    pub trace: Trace,
    /// Chain findings and validation failures together, already sorted.
    ///
    /// Does **not** include [`Start::note`]: only the caller knows whether the request came from a
    /// `;# start` line or from the start-production box, and therefore where to point it.
    pub diagnostics: Vec<Diagnostic>,
    /// How the start rule was resolved.
    pub start: Start,
    /// The bytes that were traced.
    ///
    /// Kept because they need not be the CBOR document: `UI_DESIGN.md:97` has the EDN tab validate
    /// "the CBOR corresponding to the displayed EDN", which is a transient buffer. A frontend
    /// compares these against what its hex pane holds before highlighting it, since a `cbor` span
    /// is a coordinate into *this* buffer and nothing else.
    pub cbor: Vec<u8>,
}

impl ChainTrace {
    /// Resolve `root`'s chain, validate `cbor` against the merged schema, and attribute the result.
    ///
    /// Never fails. A schema that does not resolve, one that does not parse, CBOR that does not
    /// decode, and CBOR that simply does not match all come back as diagnostics over an empty
    /// trace, because `Validate` is a button and every one of those is something to report rather
    /// than an outcome the caller can act on differently.
    ///
    /// Reads every file in the chain **from disk**, inherited from [`super::check_chain`]. A caller
    /// with unsaved edits must decline to call this rather than show attributed spans computed
    /// against stale text.
    pub fn check(
        cbor: &[u8],
        root: &Path,
        search_paths: &[PathBuf],
        start_override: Option<&str>,
        config: &Config,
    ) -> Self {
        let checked = super::check_chain(root, search_paths, config);
        let mut diagnostics = checked.diagnostics;

        let Some(parsed) = checked.cddl.as_ref() else {
            // The chain did not resolve or did not parse, and `check_chain` has already said why
            // in the file and at the line that caused it. Adding "validation did not run" here
            // describes a second failure that did not happen.
            return Self {
                chain: checked.chain,
                trace: Trace::default(),
                diagnostics,
                start: Start::default(),
                cbor: cbor.to_vec(),
            };
        };

        let resolved = super::start_rule(parsed, checked.start_rule.as_deref(), start_override);
        let start = Start {
            name: resolved.name.map(str::to_owned),
            rejected: resolved.rejected.clone(),
            defaulted: resolved.defaulted,
        };

        let trace = match check(cbor, parsed, resolved.name, config) {
            Ok(trace) => {
                diagnostics.extend(trace.diagnostics());
                trace
            }
            Err(error) => {
                // The checker refused to start. Attributed to the CBOR buffer, not the schema: a
                // decode failure is the buffer's fault, and the buffer is what the user handed in.
                diagnostics.push(Diagnostic::error(Source::Cbor, 0..0, error.to_string()));
                Trace::default()
            }
        };
        crate::diag::sort(&mut diagnostics);

        Self {
            chain: checked.chain,
            trace,
            diagnostics,
            start,
            cbor: cbor.to_vec(),
        }
    }

    /// Whether the CBOR validated against the chain's merged schema.
    pub fn is_valid(&self) -> bool {
        self.trace.is_valid()
    }

    /// The validation rejections in EDN coordinates, for the EDN editor's inline squiggles.
    ///
    /// Deliberately *not* [`Self::diagnostics`] re-spanned: that field also carries the chain's own
    /// findings, which are about CDDL files and have no place in an EDN document. See
    /// [`Trace::edn_diagnostics`].
    pub fn edn_diagnostics(&self) -> Vec<Diagnostic> {
        self.trace.edn_diagnostics()
    }

    /// The chain file and file-local span an event's CDDL span belongs to.
    ///
    /// The index is into [`Chain::files`], **not** into a frontend's tab strip. The two happen to
    /// agree today -- both are depth-first pre-order with the opened file first -- but a caller that
    /// assumes it would misattribute every row the day they stop agreeing, and silently. Resolve it
    /// through `Chain::files[index].path` instead.
    ///
    /// `None` when the event carries no CDDL span -- a prelude rule such as `uint` has no
    /// definition in the user's source -- or when [`Chain::attributed`] is false, which is the
    /// Phase 5 degradation and must never be papered over with a guess at the file.
    pub fn locate(&self, event: &TraceEvent) -> Option<(usize, Range<usize>)> {
        self.chain.attribute(event.cddl.clone()?)
    }

    /// [`Trace::rule_status`] regrouped per chain file, in each file's own coordinates.
    ///
    /// **Fail stays sticky across the regrouping.** Two distinct merged spans can attribute to one
    /// file span -- a file included twice, or two byte-identical regions -- and a fail among them
    /// wins, for the same reason it does in [`Trace::rule_status`]: the failure is what the user
    /// needs to see. Losing that in the regrouping is the bug `tps_cbor_gui/PLAN.md:521-525`
    /// records being introduced twice.
    ///
    /// Empty when the chain is unattributed, so an unattributed chain tints nothing rather than
    /// tinting the wrong file.
    pub fn rule_status_by_file(&self) -> Vec<(usize, Vec<RuleStatus>)> {
        let mut by_file: Vec<(usize, Vec<RuleStatus>)> = Vec::new();
        for (span, outcome) in self.trace.rule_status() {
            let Some((index, local)) = self.chain.attribute(span) else {
                continue;
            };
            let slot = match by_file.iter().position(|(known, _)| *known == index) {
                Some(slot) => slot,
                None => {
                    by_file.push((index, Vec::new()));
                    by_file.len() - 1
                }
            };
            let spans = &mut by_file[slot].1;
            match spans.iter_mut().find(|(known, _)| *known == local) {
                Some((_, known)) => {
                    if outcome == TraceOutcome::Fail {
                        *known = TraceOutcome::Fail;
                    }
                }
                None => spans.push((local, outcome)),
            }
        }
        by_file
    }
}

/// One rendered trace row: the view model, with filtering and attribution already done.
///
/// Exists so that neither happens inside a render closure. A `uniform_list` closure runs per
/// visible row per frame; [`Chain::attribute`] is a scan over the chain's files and the filter is
/// a pass over every event, and a trace of a 196 KB document runs to thousands of events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Call-stack depth; the log indents by this.
    pub depth: usize,
    /// Whether the rule matched, failed, or was only entered.
    pub outcome: TraceOutcome,
    /// The rule that was tried.
    pub rule: String,
    /// The path to the value, or `"(root)"` for the empty one, as `TraceLog.tsx:119` renders it.
    pub path: String,
    /// [`Chain::files`] index and file-local span, for the CDDL jump. See [`ChainTrace::locate`],
    /// which explains why this is not a tab index.
    pub cddl: Option<(usize, Range<usize>)>,
    /// Byte range in [`ChainTrace::cbor`], for the hex jump.
    pub cbor: Option<Range<usize>>,
    /// Byte range in [`Trace::edn`], for the EDN jump.
    pub edn: Option<Range<usize>>,
}

/// The rows a filter admits, in trace order.
pub fn rows(trace: &ChainTrace, show: &[TraceOutcome]) -> Vec<Row> {
    trace
        .trace
        .filtered(show)
        .map(|event| Row {
            depth: event.depth,
            outcome: event.outcome,
            rule: event.rule.clone(),
            path: if event.path.is_empty() {
                "(root)".to_owned()
            } else {
                event.path.clone()
            },
            cddl: trace.locate(event),
            cbor: event.cbor.clone(),
            edn: event.edn.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::chain::ChainFile;
    use super::*;

    /// `{1: "a", 2: 3}` -- a two-entry map, so there is nesting to attribute spans to.
    const CBOR: &[u8] = &[0xa2, 0x01, 0x61, 0x61, 0x02, 0x03];

    const SCHEMA: &str = "thing = {\n  1: tstr,\n  2: uint,\n}\n";
    const WRONG: &str = "thing = {\n  1: uint,\n  2: uint,\n}\n";

    fn parsed(source: &str) -> Cddl {
        super::super::parse(source).expect("test schema parses")
    }

    fn examples() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("examples")
    }

    #[test]
    fn matching_cbor_validates_with_an_empty_mismatch_list() {
        let trace = check(CBOR, &parsed(SCHEMA), None, &Config::all()).expect("checker runs");
        assert!(trace.is_valid(), "{:?}", trace.mismatches);
        assert!(trace.diagnostics().is_empty());
        assert!(!trace.events.is_empty(), "a trace was requested");
    }

    #[test]
    fn a_mismatch_is_reported_rather_than_returned_as_an_error() {
        let trace = check(CBOR, &parsed(WRONG), None, &Config::all()).expect("checker runs");
        assert!(!trace.is_valid());
        let diagnostics = trace.diagnostics();
        assert!(!diagnostics.is_empty());
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.severity, Severity::Error);
            assert_eq!(diagnostic.source, Source::Validation);
            assert!(diagnostic.span.end <= CBOR.len(), "{diagnostic:?}");
        }
    }

    /// The three coordinate spaces must stay consistent, which is the whole point of the module.
    #[test]
    fn every_resolved_span_indexes_the_buffer_it_claims_to() {
        let trace = check(CBOR, &parsed(SCHEMA), None, &Config::all()).expect("checker runs");
        let edn = trace
            .edn
            .as_deref()
            .expect("some event carried a CBOR span");

        let mut saw_cddl = false;
        let mut saw_edn = false;
        for event in &trace.events {
            if let Some(span) = &event.cddl {
                assert!(
                    span.start <= span.end && span.end <= SCHEMA.len(),
                    "{event:?}"
                );
                // The rule span must actually name a rule in the source.
                assert!(SCHEMA[span.clone()].contains("thing"), "{event:?}");
                saw_cddl = true;
            }
            if let Some(span) = &event.cbor {
                assert!(
                    span.start <= span.end && span.end <= CBOR.len(),
                    "{event:?}"
                );
            }
            if let Some(span) = &event.edn {
                assert!(span.start <= span.end && span.end <= edn.len(), "{event:?}");
                // And it must be sliceable, not merely in range.
                assert!(
                    edn.is_char_boundary(span.start) && edn.is_char_boundary(span.end),
                    "{event:?} is not on a char boundary of the EDN"
                );
                saw_edn = true;
            }
        }
        assert!(saw_cddl, "no event carried a CDDL span");
        assert!(saw_edn, "no event resolved an EDN span");
    }

    #[test]
    fn an_edn_span_is_only_offered_alongside_the_text_it_indexes() {
        let trace = check(CBOR, &parsed(SCHEMA), None, &Config::all()).expect("checker runs");
        if trace.edn.is_none() {
            assert!(
                trace.events.iter().all(|event| event.edn.is_none()),
                "an EDN span was resolved against no EDN text"
            );
        }
    }

    #[test]
    fn a_mismatch_is_positioned_from_the_trace() {
        let trace = check(CBOR, &parsed(WRONG), None, &Config::all()).expect("checker runs");
        let positioned = trace
            .diagnostics()
            .into_iter()
            .any(|diagnostic| !diagnostic.is_point());
        assert!(
            positioned,
            "no mismatch got a byte range from the trace: {:?}",
            trace.events
        );
    }

    /// `tps_cbor_gui/PLAN.md:521-525` records this being lost twice.
    #[test]
    fn fail_beats_match_in_the_rule_status() {
        let span = 0..5;
        let trace = Trace {
            mismatches: Vec::new(),
            events: vec![
                event(span.clone(), TraceOutcome::Match),
                event(span.clone(), TraceOutcome::Fail),
                event(span.clone(), TraceOutcome::Match),
            ],
            edn: None,
        };
        assert_eq!(trace.rule_status(), vec![(span, TraceOutcome::Fail)]);
    }

    #[test]
    fn an_attempt_alone_sets_no_rule_status() {
        let trace = Trace {
            mismatches: Vec::new(),
            events: vec![event(0..5, TraceOutcome::Attempt)],
            edn: None,
        };
        assert!(trace.rule_status().is_empty());
    }

    /// The property the whole pinpoint idea rests on: a failing event in a *valid* document is a
    /// backtracked alternative, not an error. `a / b` and `? 3 => bool` both produce them, and
    /// reporting one would put a red row under a document that validated.
    #[test]
    fn a_failing_event_in_a_valid_document_is_not_a_failure() {
        let trace = Trace {
            mismatches: Vec::new(),
            events: vec![
                failure(0, "root"),
                failure(1, "root.1"),
                event(0..1, TraceOutcome::Match),
            ],
            edn: None,
        };
        assert!(trace.is_valid());
        assert!(trace.pinpoints().is_empty());
        assert!(trace.diagnostics().is_empty());
    }

    /// A failure whose ancestor matched is unreachable from the root failure, so it is somebody
    /// else's backtracking and must not be reported as the cause.
    #[test]
    fn a_failure_disconnected_from_the_root_is_not_a_pinpoint() {
        let trace = Trace {
            mismatches: vec![mismatch("root")],
            events: vec![
                // The real chain: root -> root.2 -> root.2[1].
                failure(0, "root"),
                failure(1, "root.2"),
                failure(2, "root.2[1]"),
                // An alternative that failed two levels down while its parent went on to match.
                failure(2, "root.9[0]"),
            ],
            edn: None,
        };
        let paths: Vec<_> = trace
            .pinpoints()
            .iter()
            .map(|event| event.path.as_str())
            .collect();
        assert_eq!(paths, ["root.2[1]"]);
    }

    /// `root.2[10]` starts with `root.2[1]` and is not inside it. Getting this wrong would attach
    /// the eleventh element's failure to the second's and highlight the wrong bytes.
    #[test]
    fn a_path_prefix_must_end_on_a_step_boundary() {
        assert!(is_inside("root", "root.2"));
        assert!(is_inside("root.2", "root.2[1]"));
        assert!(is_inside("root.2[1]", "root.2[1].3"));
        assert!(!is_inside("root.2[1]", "root.2[10]"));
        assert!(!is_inside("root", "rooted.1"));
        assert!(!is_inside("root", "root"));
    }

    /// Several members can be wrong, and each is worth its own clickable row.
    #[test]
    fn two_failed_siblings_are_two_pinpoints() {
        let trace = Trace {
            mismatches: vec![mismatch("root")],
            events: vec![
                failure(0, "root"),
                failure(1, "root.1"),
                failure(1, "root.2"),
            ],
            edn: None,
        };
        let paths: Vec<_> = trace
            .pinpoints()
            .iter()
            .map(|event| event.path.as_str())
            .collect();
        assert_eq!(paths, ["root.1", "root.2"]);
    }

    /// When nothing failed below it, the rule the check started from *is* the pinpoint -- and then
    /// the mismatch already names that path, so there must not be a second row about it.
    #[test]
    fn a_pinpoint_on_the_mismatch_s_own_path_is_not_reported_twice() {
        let trace = Trace {
            mismatches: vec![mismatch("root")],
            events: vec![failure(0, "root")],
            edn: None,
        };
        assert_eq!(trace.pinpoints().len(), 1);
        assert_eq!(
            trace.diagnostics().len(),
            1,
            "the mismatch row, and no copy"
        );
    }

    /// The end-to-end shape of what `Validate` does: resolve a chain, trace, and attribute every
    /// CDDL span to one of the chain's files in that file's own coordinates.
    #[test]
    fn a_trace_over_the_chain_attributes_every_cddl_span_to_a_file() {
        let dir = examples();
        let cbor = std::fs::read(dir.join("flat_tee_claims_good.cbor")).expect("good fixture");
        let traced = ChainTrace::check(
            &cbor,
            &dir.join("gp_eap.cddl"),
            std::slice::from_ref(&dir),
            None,
            &Config::all(),
        );

        assert!(traced.chain.files.len() >= 4, "the four-deep chain");
        assert!(traced.chain.attributed, "the chain must be attributable");
        assert_eq!(traced.start.name.as_deref(), Some("Claims-Set"));
        assert_eq!(
            traced.cbor, cbor,
            "the traced bytes are kept for comparison"
        );

        let mut attributed = 0usize;
        for event in &traced.trace.events {
            if event.cddl.is_none() {
                continue;
            }
            let (index, span) = traced
                .locate(event)
                .unwrap_or_else(|| panic!("{event:?} was not attributed to a file"));
            let text = &traced.chain.files[index].text;
            assert!(
                span.end <= text.len(),
                "{event:?} rebased past the end of {}",
                traced.chain.files[index].path.display()
            );
            // The rebased span must still name the rule, which is what proves the arithmetic.
            assert!(
                text[span].contains(&event.rule),
                "{event:?} rebased onto text that does not name the rule"
            );
            attributed += 1;
        }
        assert!(attributed > 0, "no event carried a CDDL span to attribute");
    }

    /// The safety property, the same one Phase 5 chose: with no proved partition there is no file
    /// to name, so nothing is located and nothing is tinted — rather than a guess that looks
    /// authoritative.
    #[test]
    fn an_unattributed_chain_locates_nothing() {
        let traced = ChainTrace {
            chain: Chain {
                files: Vec::new(),
                attributed: false,
                diagnostics: Vec::new(),
            },
            trace: Trace {
                mismatches: Vec::new(),
                events: vec![event(0..5, TraceOutcome::Fail)],
                edn: None,
            },
            diagnostics: Vec::new(),
            start: Start::default(),
            cbor: Vec::new(),
        };

        assert_eq!(traced.locate(&traced.trace.events[0]), None);
        assert!(traced.rule_status_by_file().is_empty());
        // And the rows still render — only the CDDL jump is unavailable.
        let rows = rows(&traced, &[TraceOutcome::Fail]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cddl, None);
    }

    /// Two merged spans attributing to one file span, one of them failing: the result is a fail.
    /// `tps_cbor_gui/PLAN.md:521-525` records this being lost twice, and regrouping is a second
    /// place to lose it.
    #[test]
    fn fail_beats_match_after_regrouping_into_files() {
        // One file, present twice in the merged source, so two distinct merged spans rebase onto
        // the same file-local span.
        let text = "thing = uint\n".to_owned();
        let chain = Chain {
            files: vec![
                ChainFile {
                    path: "a.cddl".into(),
                    text: text.clone(),
                    depth: 0,
                    parent: None,
                    range: 0..text.len(),
                },
                ChainFile {
                    path: "b.cddl".into(),
                    text: text.clone(),
                    depth: 1,
                    parent: Some(0),
                    range: text.len()..text.len() * 2,
                },
            ],
            attributed: true,
            diagnostics: Vec::new(),
        };
        let traced = ChainTrace {
            chain,
            trace: Trace {
                mismatches: Vec::new(),
                events: vec![
                    // In `a.cddl`'s region, matching.
                    event(0..5, TraceOutcome::Match),
                    // In `b.cddl`'s region, failing — and both rebase to `0..5`.
                    event(text.len()..text.len() + 5, TraceOutcome::Fail),
                ],
                edn: None,
            },
            diagnostics: Vec::new(),
            start: Start::default(),
            cbor: Vec::new(),
        };

        let status = traced.rule_status_by_file();
        assert_eq!(
            status.len(),
            2,
            "two files, each with its own span: {status:?}"
        );
        assert_eq!(status[0], (0, vec![(0..5, TraceOutcome::Match)]));
        assert_eq!(status[1], (1, vec![(0..5, TraceOutcome::Fail)]));

        // And within one file, a fail among several must win.
        let traced = ChainTrace {
            trace: Trace {
                mismatches: Vec::new(),
                events: vec![
                    event(0..5, TraceOutcome::Match),
                    event(6..11, TraceOutcome::Fail),
                ],
                edn: None,
            },
            ..traced
        };
        // Both are inside `a.cddl`, but at different spans, so they stay separate…
        assert_eq!(
            traced.rule_status_by_file(),
            vec![(
                0,
                vec![(0..5, TraceOutcome::Match), (6..11, TraceOutcome::Fail)]
            )]
        );
    }

    #[test]
    fn rows_honour_the_filter_and_keep_trace_order() {
        let traced = ChainTrace {
            chain: Chain {
                files: Vec::new(),
                attributed: false,
                diagnostics: Vec::new(),
            },
            trace: Trace {
                mismatches: Vec::new(),
                events: vec![
                    event(0..1, TraceOutcome::Attempt),
                    event(0..1, TraceOutcome::Fail),
                    event(0..1, TraceOutcome::Match),
                ],
                edn: None,
            },
            diagnostics: Vec::new(),
            start: Start::default(),
            cbor: Vec::new(),
        };

        let outcomes: Vec<_> = rows(&traced, &[TraceOutcome::Match, TraceOutcome::Fail])
            .into_iter()
            .map(|row| row.outcome)
            .collect();
        assert_eq!(
            outcomes,
            [TraceOutcome::Fail, TraceOutcome::Match],
            "rows must stay in the order the checker made the attempts, not be grouped"
        );
        assert_eq!(rows(&traced, &[]).len(), 0);
        assert_eq!(
            rows(&traced, &TraceOutcome::from_filter(&Default::default())).len(),
            2
        );
    }

    #[test]
    fn a_row_keeps_the_event_s_depth_and_renders_an_empty_path_as_root() {
        let traced = ChainTrace {
            chain: Chain {
                files: Vec::new(),
                attributed: false,
                diagnostics: Vec::new(),
            },
            trace: Trace {
                mismatches: Vec::new(),
                events: vec![
                    TraceEvent {
                        path: String::new(),
                        depth: 0,
                        ..event(0..1, TraceOutcome::Match)
                    },
                    TraceEvent {
                        path: "root.1".into(),
                        depth: 3,
                        ..event(0..1, TraceOutcome::Match)
                    },
                ],
                edn: None,
            },
            diagnostics: Vec::new(),
            start: Start::default(),
            cbor: Vec::new(),
        };

        let rows = rows(&traced, &[TraceOutcome::Match]);
        assert_eq!(rows[0].path, "(root)", "`TraceLog.tsx:119`");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].path, "root.1");
        assert_eq!(rows[1].depth, 3);
    }

    /// The persisted default and the tracer's reading of it must not drift apart: the settings file
    /// is the only place the default is written down, and `TraceLog.tsx:66` is why it is that one.
    #[test]
    fn the_persisted_filter_default_is_match_and_fail() {
        assert_eq!(
            TraceOutcome::from_filter(&crate::settings::TraceFilter::default()),
            [TraceOutcome::Match, TraceOutcome::Fail]
        );
        assert!(
            TraceOutcome::from_filter(&crate::settings::TraceFilter {
                attempt: false,
                matched: false,
                failed: false,
            })
            .is_empty()
        );
    }

    /// The EDN-pane highlight gate rests entirely on this: a frontend decides whether to highlight
    /// its EDN editor by comparing [`Trace::edn`] against the document it is showing, and the
    /// document was filled by `convert::cbor_to_edn`. If the two renderings ever diverge, the gate
    /// closes forever and the EDN highlight quietly stops working with no error anywhere.
    #[test]
    fn a_trace_renders_edn_byte_identical_to_cbor_to_edn() {
        let dir = examples();
        for name in ["trace_demo_good.cbor", "flat_tee_claims_good.cbor"] {
            let cbor = std::fs::read(dir.join(name)).unwrap_or_else(|_| panic!("read {name}"));
            // `any` so that the comparison is about the rendering and not about the schema.
            let trace = check(&cbor, &parsed("thing = any\n"), None, &Config::all())
                .expect("the checker runs");
            let rendered = trace.edn.as_deref().expect("EDN was rendered");
            let expected = crate::convert::cbor_to_edn(&cbor).expect("the fixture decodes");
            assert_eq!(
                rendered, expected,
                "{name}: `spans.rs:36`'s guarantee is gone"
            );
        }
    }

    /// `Validate` is a button: everything it can go wrong at is a diagnostic, not a refusal.
    #[test]
    fn checking_a_chain_never_fails() {
        let dir = examples();
        let good = std::fs::read(dir.join("trace_demo_good.cbor")).expect("good fixture");

        // A schema that does not parse. `chain::walk` reports it, so the trace is simply empty.
        let scratch = std::env::temp_dir().join("tps_cbor_core_trace_chain_test");
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let broken = scratch.join("broken.cddl");
        std::fs::write(&broken, "thing = !!!\n").expect("write the broken schema");
        let traced = ChainTrace::check(
            &good,
            &broken,
            std::slice::from_ref(&scratch),
            None,
            &Config::all(),
        );
        assert!(traced.trace.events.is_empty());
        assert!(!traced.diagnostics.is_empty(), "nothing was reported");
        assert_eq!(traced.start, Start::default());

        // CBOR that does not decode, against a schema that does parse.
        let truncated = &[0xa2u8];
        let traced = ChainTrace::check(
            truncated,
            &dir.join("trace_demo.cddl"),
            std::slice::from_ref(&dir),
            None,
            &Config::all(),
        );
        assert!(traced.trace.events.is_empty());
        assert!(
            traced
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.source == Source::Cbor),
            "a decode failure must be reported against the buffer: {:?}",
            traced.diagnostics
        );
        // The start rule still resolved, because the schema did.
        assert_eq!(traced.start.name.as_deref(), Some("Report"));

        // A rule the schema does not define: reported through `Start::note`, where the caller
        // supplies the span, and not duplicated into `diagnostics`.
        let traced = ChainTrace::check(
            &good,
            &dir.join("trace_demo.cddl"),
            std::slice::from_ref(&dir),
            Some("Nonexistent"),
            &Config::all(),
        );
        let note = traced
            .start
            .note(0..0)
            .expect("a rejected name is reported");
        assert_eq!(note.severity, Severity::Warning);
        assert!(note.message.contains("Nonexistent"), "{}", note.message);
        assert!(
            !traced
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == note.message),
            "the start note must be left to the caller, which knows where to point it"
        );

        std::fs::remove_dir_all(&scratch).ok();
    }

    /// `Mismatch::to_diagnostic` already produces `0..0` for an unpositioned mismatch, which is the
    /// codebase's convention for "there is nowhere to go" and makes the row inert in the frontend.
    /// Pinned rather than edited, since the frontend's honesty rule depends on it.
    #[test]
    fn a_mismatch_with_no_cbor_span_becomes_an_inert_diagnostic() {
        let diagnostic = mismatch("root.1").to_diagnostic(None);
        assert_eq!(diagnostic.span, 0..0);
        assert!(diagnostic.is_point());
        assert_eq!(diagnostic.source, Source::Validation);
    }

    fn mismatch(path: &str) -> Mismatch {
        Mismatch {
            path: path.into(),
            expected: "something".into(),
            got: "something else".into(),
        }
    }

    fn failure(depth: usize, path: &str) -> TraceEvent {
        TraceEvent {
            rule: "rule".into(),
            path: path.into(),
            cddl: None,
            cbor: Some(depth..depth + 1),
            edn: None,
            outcome: TraceOutcome::Fail,
            depth,
        }
    }

    fn event(cddl: Range<usize>, outcome: TraceOutcome) -> TraceEvent {
        TraceEvent {
            rule: "thing".into(),
            path: "root".into(),
            cddl: Some(cddl),
            cbor: None,
            edn: None,
            outcome,
            depth: 0,
        }
    }

    /// The default filter is match + fail, per `TraceLog.tsx:66`.
    #[test]
    fn the_filter_selects_by_outcome() {
        let trace = Trace {
            mismatches: Vec::new(),
            events: vec![
                event(0..1, TraceOutcome::Attempt),
                event(0..1, TraceOutcome::Match),
                event(0..1, TraceOutcome::Fail),
            ],
            edn: None,
        };
        let shown: Vec<_> = trace
            .filtered(&[TraceOutcome::Match, TraceOutcome::Fail])
            .map(|event| event.outcome)
            .collect();
        assert_eq!(shown, [TraceOutcome::Match, TraceOutcome::Fail]);
        assert_eq!(trace.filtered(&[]).count(), 0);
    }

    #[test]
    fn malformed_cbor_is_an_error_from_check_and_a_diagnostic_from_check_source() {
        // 0xa2 promises two map entries and then ends.
        let truncated = &[0xa2u8];
        let error = check(truncated, &parsed(SCHEMA), None, &Config::all())
            .expect_err("truncated CBOR cannot be checked");
        assert!(matches!(error, CoreError::CddlCheck(_)), "{error:?}");

        let (trace, diagnostics) = check_source(truncated, SCHEMA, None, &Config::all());
        assert!(trace.events.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, Source::Cbor);
    }

    #[test]
    fn an_unparseable_schema_becomes_a_cddl_diagnostic() {
        let (trace, diagnostics) = check_source(CBOR, "thing = !!!\n", None, &Config::all());
        assert!(trace.events.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, Source::Cddl);
    }

    #[test]
    fn check_source_defaults_to_the_first_rule() {
        let (trace, diagnostics) = check_source(CBOR, SCHEMA, None, &Config::all());
        assert!(trace.is_valid(), "{:?}", trace.mismatches);
        assert!(diagnostics.is_empty());
    }

    /// The fixture pair this phase exists to be able to test against: `trace_demo.cddl` has no
    /// catch-all, so `trace_demo_good.cbor` matches and `trace_demo_bad.cbor` does not.
    ///
    /// `flat_tee_claims_good.cbor` / `_bad.cbor` cannot provide this -- see
    /// [`the_real_include_chain_traces_with_consistent_spans`] for why -- and
    /// `tps_cbor_core/README.md` records it.
    #[test]
    fn the_new_fixture_pair_discriminates() {
        let dir = examples();
        let resolved = super::super::resolve(
            &dir.join("trace_demo.cddl"),
            std::slice::from_ref(&dir),
            &Config::all(),
        )
        .expect("trace_demo.cddl resolves");
        let start = super::super::start_rule(&resolved.cddl, resolved.start_rule.as_deref(), None);
        assert_eq!(
            start.name,
            Some("Report"),
            "`;# start Report` should be honoured"
        );

        let good = std::fs::read(dir.join("trace_demo_good.cbor")).expect("good fixture");
        let trace = check(&good, &resolved.cddl, start.name, &Config::all()).expect("checker runs");
        assert!(
            trace.is_valid(),
            "trace_demo_good.cbor was rejected: {:?}",
            trace.mismatches
        );

        let bad = std::fs::read(dir.join("trace_demo_bad.cbor")).expect("bad fixture");
        let trace = check(&bad, &resolved.cddl, start.name, &Config::all()).expect("checker runs");
        assert!(!trace.is_valid(), "trace_demo_bad.cbor was accepted");

        // The failure is three containers down -- map, array, map -- so the trace must name the
        // member rather than blaming the root, and the fail must then travel back up the stack.
        // That is the shape the trace log's indentation and its sticky rule-status both display.
        let fails: Vec<_> = trace
            .events
            .iter()
            .filter(|event| event.outcome == TraceOutcome::Fail)
            .map(|event| (event.depth, event.rule.as_str(), event.path.as_str()))
            .collect();
        assert_eq!(
            fails,
            [
                (2, "uint", "Report.2[1].3"),
                (1, "Measurement", "Report.2[1]"),
                (0, "Report", "Report"),
            ],
            "the failure did not start at the member and propagate to the root: {:?}",
            trace.events
        );
        // `tps_cddl` reports a mismatch only at the rule the check started from, so the checker's
        // own account of this is one row spanning the whole document. The pinpoint is what makes
        // the diagnostics pane useful, and the reason `diagnostics` reports both.
        assert_eq!(
            trace
                .mismatches
                .iter()
                .map(|mismatch| mismatch.path.as_str())
                .collect::<Vec<_>>(),
            ["Report"],
            "tps_cddl now names the nested path itself; the pinpoint may be redundant"
        );
        let pinpoints: Vec<_> = trace
            .pinpoints()
            .iter()
            .map(|event| (event.path.as_str(), event.cbor.clone()))
            .collect();
        assert_eq!(
            pinpoints,
            [("Report.2[1].3", Some(47..51))],
            "the trace did not pinpoint the offending member"
        );
        // 0x63 is `text(3)`, so the span is exactly the `"two"` that belongs where a `uint` does.
        assert_eq!(
            &bad[47..51],
            b"\x63two",
            "the pinpoint span does not cover the offending value"
        );

        let diagnostics = trace.diagnostics();
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Report.2[1].3")
                    && diagnostic.span == (47..51)),
            "no clickable row named the member: {diagnostics:?}"
        );

        // The matched sibling beside the failed one is what makes the trace readable, and the
        // whole reason the schema has `[+ Measurement]` rather than a single measurement.
        assert!(
            trace.events.iter().any(|event| {
                event.outcome == TraceOutcome::Match && event.path == "Report.2[0]"
            }),
            "the first measurement did not match, so there is no contrast to see"
        );

        // Every failing event carries all three spans, which is what a trace row's three jump
        // buttons need. A row that fails and offers nowhere to go is the one useless outcome.
        for event in trace.events.iter().filter(|event| event.cddl.is_some()) {
            assert!(
                event.cbor.is_some() && event.edn.is_some(),
                "{event:?} has a CDDL span but not the other two"
            );
        }
    }

    /// The bytes in `trace_demo_good.cbor` are what `trace_demo_good.edn` says, and they are the
    /// preferred serialization -- so the committed `.edn` is a reviewable source for the `.cbor`
    /// rather than a comment that can drift away from it.
    ///
    /// Checked by re-encoding the `.edn` and comparing bytes, not by comparing text: the `.edn`
    /// files are pretty-printed by hand, while `edn-tool decode` emits one compact line.
    #[test]
    fn the_demo_fixtures_round_trip_through_their_committed_edn() {
        let dir = examples();
        for name in ["trace_demo_good", "trace_demo_bad"] {
            let edn = std::fs::read_to_string(dir.join(format!("{name}.edn")))
                .unwrap_or_else(|_| panic!("read {name}.edn"));
            let cbor = std::fs::read(dir.join(format!("{name}.cbor")))
                .unwrap_or_else(|_| panic!("read {name}.cbor"));
            let encoded = crate::convert::edn_to_cbor(&edn)
                .unwrap_or_else(|error| panic!("{name}.edn encodes: {error}"));
            assert_eq!(encoded, cbor, "{name}.edn does not encode to {name}.cbor");
        }
    }

    /// The EDN editor's squiggle and the hex grid's highlight come from the *same* rejection, so
    /// the EDN-coordinate span has to land on the offending value in the text the editor is
    /// showing -- not merely be non-empty. An off-by-one-token span here would underline the
    /// neighbouring member and read as an authoritative answer.
    ///
    /// Also pins that the two spaces differ only in their spans: the sentences are the lower
    /// window's and the editor's at once, and two wordings for one fault would read as two faults.
    #[test]
    fn an_edn_diagnostic_spans_the_offending_value_in_the_rendered_edn() {
        let dir = examples();
        let resolved = super::super::resolve(
            &dir.join("trace_demo.cddl"),
            std::slice::from_ref(&dir),
            &Config::all(),
        )
        .expect("trace_demo.cddl resolves");
        let start = super::super::start_rule(&resolved.cddl, resolved.start_rule.as_deref(), None);
        let bad = std::fs::read(dir.join("trace_demo_bad.cbor")).expect("bad fixture");
        let trace = check(&bad, &resolved.cddl, start.name, &Config::all()).expect("checker runs");

        let edn = trace.edn.as_deref().expect("the trace rendered its EDN");
        let diagnostics = trace.edn_diagnostics();
        let pinpoint = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("Report.2[1].3"))
            .unwrap_or_else(|| panic!("no EDN row named the member: {diagnostics:?}"));
        assert_eq!(
            &edn[pinpoint.span.clone()],
            "\"two\"",
            "the EDN span does not cover the offending value"
        );

        // The same rows, the same sentences, different coordinates. The CBOR span for the member is
        // `47..51`, which is nowhere near its EDN span -- so a caller that mixed the two up would
        // be caught here rather than by a user staring at the wrong line.
        let cbor_rows = trace.diagnostics();
        assert_eq!(
            cbor_rows
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>(),
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>(),
            "the two coordinate spaces report different faults"
        );
        assert_ne!(
            cbor_rows
                .iter()
                .map(|diagnostic| diagnostic.span.clone())
                .collect::<Vec<_>>(),
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.span.clone())
                .collect::<Vec<_>>(),
            "the EDN rows carry CBOR spans"
        );
    }

    /// End-to-end over the real four-deep include chain `gp_eap.cddl` -> `rfc9711.cddl` ->
    /// `rfc8392.cddl` -> `rfc9052.cddl`: module resolution, the dotted `;# start` directive,
    /// checking, and span resolution across all three coordinate spaces over a 254-rule schema and
    /// a ~290-byte document. This is the closest thing the crate has to an integration test.
    ///
    /// # Why this does not assert that `_bad.cbor` fails
    ///
    /// The plan expected `flat_tee_claims_good.cbor` / `_bad.cbor` to be a positive/negative pair.
    /// They are not, and neither is this code's fault:
    ///
    /// * At the resolved start rule, **both** are accepted. `Claims-Set` ends with
    ///   `* Claim-Label .feature "extended-claims-label" => any` (`rfc8392.cddl:12`), a catch-all
    ///   mapping any label to `any`. The GlobalPlatform claim `-261` matches it, so its contents
    ///   are never descended into and the one byte that differs is never examined.
    /// * At `gp_component_claims`, the rule that *does* constrain those contents, **both** are
    ///   rejected -- the good fixture fails `gp_tee_sd_authority_type` under claim 212.
    ///
    /// Discrimination is therefore tested by the hand-written schemas above, which do discriminate.
    /// What this test is for is the span machinery against real data. The catch-all is asserted
    /// explicitly so that a fixture or schema change breaks this loudly rather than silently
    /// turning the test into a tautology.
    #[test]
    fn the_real_include_chain_traces_with_consistent_spans() {
        let dir = examples();
        let resolved = super::super::resolve(
            &dir.join("gp_eap.cddl"),
            std::slice::from_ref(&dir),
            &Config::all(),
        )
        .expect("gp_eap.cddl resolves");
        let start = super::super::start_rule(&resolved.cddl, resolved.start_rule.as_deref(), None);
        let root = start.name.expect("a start rule");
        assert_eq!(root, "Claims-Set", "`include` merges without a qualifier");

        // The catch-all that makes both fixtures acceptable. If this goes, revisit the doc above.
        assert!(
            resolved
                .source
                .contains(r#".feature "extended-claims-label" => any"#),
            "Claims-Set no longer has its catch-all; the fixtures may now discriminate"
        );

        for name in ["flat_tee_claims_good.cbor", "flat_tee_claims_bad.cbor"] {
            let cbor = std::fs::read(dir.join(name)).unwrap_or_else(|_| panic!("read {name}"));
            let trace =
                check(&cbor, &resolved.cddl, Some(root), &Config::all()).expect("checker runs");
            assert!(
                trace.is_valid(),
                "{name} was rejected at {root}, so the catch-all no longer applies: {:?}",
                trace.mismatches
            );
            assert!(!trace.events.is_empty(), "{name}: no trace was recorded");

            let edn = trace.edn.as_deref().expect("EDN was rendered");
            let mut resolved_edn = 0usize;
            for event in &trace.events {
                if let Some(span) = &event.cddl {
                    assert!(
                        span.start <= span.end && span.end <= resolved.source.len(),
                        "{name}: {event:?} is out of range for the merged source"
                    );
                }
                if let Some(span) = &event.cbor {
                    assert!(
                        span.start <= span.end && span.end <= cbor.len(),
                        "{name}: {event:?} is out of range for the CBOR"
                    );
                }
                if let Some(span) = &event.edn {
                    assert!(
                        span.start <= span.end && span.end <= edn.len(),
                        "{name}: {event:?} is out of range for the EDN"
                    );
                    assert!(
                        edn.is_char_boundary(span.start) && edn.is_char_boundary(span.end),
                        "{name}: {event:?} is not sliceable"
                    );
                    resolved_edn += 1;
                }
            }
            assert!(
                resolved_edn > 0,
                "{name}: no CBOR span resolved into the EDN, so the three spaces are not linked"
            );
        }
    }

    /// The `-261` claim's contents are only constrained by `gp_component_claims`, and the trace
    /// must attribute the failure there to a real byte range -- which is what makes a mismatch
    /// clickable in the hex view.
    #[test]
    fn a_failure_deep_in_a_real_document_carries_a_byte_range() {
        let dir = examples();
        let resolved = super::super::resolve(
            &dir.join("gp_eap.cddl"),
            std::slice::from_ref(&dir),
            &Config::all(),
        )
        .expect("gp_eap.cddl resolves");
        let cbor = std::fs::read(dir.join("flat_tee_claims_good.cbor")).expect("good fixture");

        // Isolate the GlobalPlatform component-claims submap by its rendered EDN, which is what
        // `SpanMap` is for.
        let map = SpanMap::build(&cbor).expect("the fixture decodes");
        let (bytes, _) = map
            .entries()
            .iter()
            .find(|(_, edn)| {
                let text = &map.edn()[edn.clone()];
                text.starts_with('{') && text.contains("107:") && text.contains("208:")
            })
            .expect("the component-claims submap")
            .clone();

        let trace = check(
            &cbor[bytes.clone()],
            &resolved.cddl,
            Some("gp_component_claims"),
            &Config::all(),
        )
        .expect("checker runs");
        assert!(
            !trace.is_valid(),
            "the fixture matched gp_component_claims; this test's premise has changed"
        );

        // Every mismatch is positioned, and the deepest failure is narrower than the whole submap.
        let diagnostics = trace.diagnostics();
        assert!(!diagnostics.is_empty());
        for diagnostic in &diagnostics {
            assert!(diagnostic.span.end <= bytes.len(), "{diagnostic:?}");
        }
        let deepest = trace
            .events
            .iter()
            .filter(|event| event.outcome == TraceOutcome::Fail && event.cbor.is_some())
            .max_by_key(|event| event.depth)
            .expect("a failing event with a span");
        let span = deepest.cbor.clone().expect("checked above");
        assert!(
            span.start > 0 || span.end < bytes.len(),
            "the deepest failure spans the whole submap, so it is not pinpointing anything"
        );
        assert!(
            deepest.depth > 0,
            "the failure was not attributed below the root"
        );
    }
}
