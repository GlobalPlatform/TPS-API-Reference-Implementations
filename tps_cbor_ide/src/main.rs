/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! `cbor-ide` — native workbench for CBOR, EDN, CDDL and COSE artifacts.
//!
//! This file is the shell `UI_DESIGN.md:20-64` describes and the root view that holds it together:
//! the three-pane layout, the menu bar, the tab strip, the file explorer, and the one
//! [`Workspace`] every pane reads. The panes' *contents* arrive in later phases — Phase 3 the hex
//! view, Phase 4 the EDN editor, Phase 5 the CDDL editor, Phase 6 the tracer, Phase 7 COSE — so
//! each is a labelled placeholder here rather than a half-built editor.
//!
//! The structural claim worth making explicit, because it is the one the old Tauri app got wrong:
//! **the document model lives in Rust and the views read it.** `tps_cbor_gui`'s backend held no
//! state at all, so every buffer crossed an IPC boundary and React owned the truth. Here there is
//! exactly one `Workspace`, behind an [`Entity`] that [`Ide`] and the views it builds share.
//!
//! The entity is Phase 3's doing and the reason is worth stating. [`hex_view::HexView`] renders the
//! visible rows from a closure that runs during *paint*, after `render` has returned, so a borrow
//! will not reach it; and it cannot hold `Entity<Ide>` without a reference cycle, since [`Ide`]
//! holds it. Sharing the model is the only shape with neither problem. Writes still go through one
//! path — `CborDoc::set_byte` — and the view reports them back as a [`hex_view::HexEvent`] rather
//! than relying on an observer, so there is one place that re-derives the diagnostics, the
//! explorer's bold-red set and the menus.
//!
//! Which commands are handled is still deliberately narrow: `Open`, `Save`, `SaveAs`, `Quit`,
//! `ToggleOptions`, the four tab selections, `ToEdn` and `Validate` from Phase 3, `ToCbor` and
//! `DecodeSelection` from Phase 4, and `SyntaxCheck` and `OpenInclude` from Phase 5. `Validate` is
//! Phase 6's: it traces the CBOR — or, on the EDN tab, the CBOR the displayed EDN encodes to —
//! against the CDDL tab's resolved include chain. `Copy`, `Paste`, the `Find` family, `GenerateKey`
//! and `About` are left unhandled on purpose: they act on panes that do not exist yet, or — for the
//! clipboard and the `Find` family — on an editor that claims them itself, and an action with a
//! handler that silently does nothing is worse than one that is visibly unbound. `PLAN.md` records
//! this.
//!
//! ## The honesty rule (Phase 6)
//!
//! A trace's spans are coordinates into the three artefacts *the trace saw*, and nothing guarantees
//! the three panes still hold them: `UI_DESIGN.md:97` has the EDN tab validate "the CBOR
//! corresponding to the displayed EDN", which is a transient buffer the hex grid never held, and a
//! hand-edited EDN document is not the canonical rendering an `edn` span indexes. Every one of those
//! is a way to draw a *confident* highlight in the wrong place, which is worse than drawing none. So:
//!
//! * **Any edit to any of the three documents clears the trace** — [`Ide::clear_trace`], called from
//!   all three edit paths. A trace is the answer to a command at a point in time.
//! * **A pane is highlighted only when it still holds the artefact the trace saw**, decided by
//!   comparison at use: [`hex_qualifies`] and [`edn_qualifies`]. The CDDL panes always qualify,
//!   because the schema came from them.
//!
//! Both are needed and neither subsumes the other: clearing handles staleness, the comparisons
//! handle the cross-pane case. In the ordinary workflow — open CBOR, `To EDN`, `Validate` — both
//! comparisons hold, because `SpanMap`'s rendering is byte-identical to the one `To EDN` used
//! (`tps_cbor_core/src/spans.rs:36`, pinned by a test there).

use crate::cddl_view::{CddlEvent, CddlView};
use crate::cose_view::CoseView;
use crate::edn_view::{EdnEvent, EdnView};
use crate::explorer::{Explorer, ExplorerEvent};
use crate::hex_view::{HexEvent, HexView, MarkKind};
use crate::keys::{AlgChoice, KeyDetail, NewKeyKind};
use crate::tab::Tab;
use gpui_kit::component::button::Button;
// `Button::outline` is inherent, but `link` lives on this trait — so the include rows need it.
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::checkbox::Checkbox;
// Neither of these is re-exported by `component::*` (`gpui-component/src/lib.rs:45`, `:76` declare
// the modules without re-exporting their contents), so both are named by their module path.
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::input::{EditorState, Input, InputEvent, InputState};
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tab::{Tab as TabItem, TabBar};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tps_cbor_core::keystore::{KeyMeta, KeyStore, Overwrite};
use tps_cbor_core::settings::Settings;
use tps_cbor_core::{
    CborDoc, CoreError, Diagnostic, Meta, Severity, Source, TextDoc, Workspace, cddl, convert,
    cose, diag,
};

mod actions;
mod cddl_view;
mod cose_view;
mod edn_view;
mod explorer;
mod hex_view;
mod keys;
mod languages;
mod layout;
mod menus;
mod tab;
mod theme;

/// The window size used when nothing has been saved.
const DEFAULT_WINDOW: (f32, f32) = (1280.0, 840.0);

/// The in-window menu bar's height, in pixels. Tall enough for the small ghost buttons
/// `AppMenuBar` builds its menus from, which have no intrinsic height to inherit.
#[cfg(not(target_os = "macos"))]
const MENU_BAR_HEIGHT: f32 = 30.0;

/// The outer tab strip's `ElementId`.
const TAB_BAR_ID: &str = "tabs";

/// The CDDL tab's inner tab strip's `ElementId`.
///
/// Named beside [`TAB_BAR_ID`] so a test can assert they differ. Two elements sharing an id does
/// not fail loudly: it makes the *outer* strip lose its selection, which looks like a bug in
/// something else entirely.
const CDDL_TAB_BAR_ID: &str = "cddl-tabs";

/// The lower window's tab strip's `ElementId`.
///
/// The third and last, named beside [`TAB_BAR_ID`] and [`CDDL_TAB_BAR_ID`] for the reason the latter
/// records: a duplicate id does not fail loudly, it makes the *outer* strip lose its selection.
const LOWER_TAB_BAR_ID: &str = "lower-tabs";

/// A trace row's height, in pixels. Matches the hex grid's, which is the other `uniform_list` here.
const TRACE_ROW_HEIGHT: f32 = 18.0;

/// The application's name, as `Help → About` shows it.
///
/// The binary is `cbor-ide` and the crate is `tps_cbor_ide`; neither is what to put in front of a
/// user, and both appear elsewhere, so this is the third spelling and the only one that is prose.
const ABOUT_NAME: &str = "CBOR IDE";

/// What the application is, in one line. `UI_DESIGN.md:1-10`, compressed.
const ABOUT_TAGLINE: &str = "A workbench for CBOR, CDDL and COSE artefacts.";

/// The licence, matching the SPDX header every file in the crate carries.
const ABOUT_LICENCE: &str =
    "MIT — Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries";

/// The crates that do the work, which is the one thing an About box can usefully say beyond the
/// version: every format this application understands is implemented by a workspace crate, and a
/// bug report is far more useful when the reporter knows which one to name.
const ABOUT_STACK: &[&str] = &[
    "tps_minicbor — CBOR encoding and decoding",
    "tps_cddl — CDDL parsing and validation",
    "tps_cose — COSE signing, verification and encryption",
    "tree-sitter — CDDL and EDN syntax highlighting",
    "GPUI Kit — the user interface",
];

/// The About box's body, line by line.
///
/// A free function returning owned lines rather than a `const` string, because the version has to
/// be interpolated and `format!` is not a `const` operation. Each line is its own element in the
/// dialog: a `\n` inside one text child is at the mercy of the text layout, and a `v_flex` of
/// lines is not.
///
/// Pinned by `the_about_text_names_the_version`, so bumping `Cargo.toml` cannot leave a stale
/// string behind here.
fn about_lines() -> Vec<String> {
    let mut lines = vec![
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        String::new(),
        ABOUT_TAGLINE.to_owned(),
        String::new(),
        ABOUT_LICENCE.to_owned(),
        String::new(),
        "Built on:".to_owned(),
    ];
    lines.extend(ABOUT_STACK.iter().map(|crate_| format!("  {crate_}")));
    lines
}

/// A trace row's indent per depth level, and its base, in pixels.
///
/// `12 + depth * 16`, which is `TraceLog.tsx`'s geometry verbatim. Not a number worth improving on:
/// the indentation is the one thing about the old log a returning user has muscle memory for.
const TRACE_INDENT_BASE: f32 = 12.0;
const TRACE_INDENT_STEP: f32 = 16.0;

/// Which page the lower window shows.
///
/// `UI_DESIGN.md:61` puts errors in the lower window and the banked decision (`PLAN.md`'s "Decisions
/// taken with the user" #1) puts the trace log there too; they share it through a tab bar, exactly
/// as the CDDL tab's files do. The inner tab bar is a divergence from the specification, which
/// describes only errors there, and is recorded as one in `PLAN.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LowerTab {
    #[default]
    Diagnostics,
    Trace,
    /// The key store's contents (Phase 7). Present only on [`Tab::Cose`] — see
    /// [`LowerTab::pages`].
    Keys,
}

impl LowerTab {
    /// The pages `tab` offers, in strip order.
    ///
    /// The array the strip is built from and the match in [`Ide::lower_pane`] are the same pages by
    /// construction, which is what keeps a strip entry from leading to a blank body.
    ///
    /// Per-tab rather than one global list because a key store is not a fact about the CBOR tab:
    /// `UI_DESIGN.md:128-130` gives keys to the COSE tab alone, and a `Keys (0)` page sitting beside
    /// `Diagnostics` on every other tab would advertise a pane that has nothing to do with the
    /// document in front of the user. Diagnostics and the trace *are* global — the trace is produced
    /// from the CDDL tab and read against the CBOR one — so only `Keys` is conditional.
    fn pages(tab: Tab) -> &'static [Self] {
        match tab {
            Tab::Cose => &[Self::Diagnostics, Self::Trace, Self::Keys],
            _ => &[Self::Diagnostics, Self::Trace],
        }
    }
}

/// A `Validate` in flight on the background executor.
///
/// `Validate` is the one command slow enough to need this: `cddl::ChainTrace::check` took ~30
/// seconds on `examples/large_uint_array.cbor`, and until it was moved off the main thread the
/// window did not repaint for any of it. Nothing in `UI_DESIGN.md` asks for progress reporting, so
/// both the state and the row below are additions recorded in `PLAN.md`; the reason is that a
/// 30-second operation with no indication is indistinguishable from a hang, which is what it was
/// taken for.
struct Validating {
    /// The root schema's name, for the refusal a second `Validate` gets.
    root: String,
    /// The progress row, built once at the start of the run rather than per frame.
    ///
    /// Held here rather than pushed into [`Ide::diagnostics`] because that list is the append-only
    /// history log: a row put there would have to be hunted down and removed again when the trace
    /// lands. Owned by the run it describes, it cannot outlive it — and it is still a *real*
    /// [`Diagnostic`], so it reaches the lower window through [`Ide::listed_diagnostics`] and is
    /// counted in the page's label like every other row.
    row: Diagnostic,
}

impl Validating {
    /// Begin a run against the schema named `root`.
    ///
    /// [`Source::Validation`] and a `0..0` span, which is the project's "there is no span" value:
    /// the row is a statement about the application, not about a place in a document, so it is
    /// inert and blanks its own location column through the machinery already in
    /// `diagnostic_target`. [`Severity::Info`] because it is the quietest kind of row there is —
    /// Phase 8 added that variant precisely so a note like this could stop looking like a warning.
    fn new(root: String) -> Self {
        let row = Diagnostic::info(
            Source::Validation,
            0..0,
            format!("validating against `{root}`…"),
        );
        Self { root, row }
    }
}

/// The root view: the whole window.
struct Ide {
    /// The persisted settings, reloaded at start-up and written back at exit.
    settings: Settings,
    /// The four documents and the workspace root. The single source of truth, shared with the
    /// views that render from it — see the module docs.
    workspace: Entity<Workspace>,
    /// Which tab is selected. UI state, so it lives here and not in `Workspace`.
    tab: Tab,
    /// The left pane.
    explorer: Entity<Explorer>,
    /// The CBOR tab's upper window.
    hex: Entity<HexView>,
    /// The EDN tab's upper window.
    edn: Entity<EdnView>,
    /// The CDDL tab's upper window, holding one editor per inner tab.
    cddl: Entity<CddlView>,
    /// The start-production text box (`UI_DESIGN.md:114`).
    ///
    /// Lives here rather than in [`CddlView`] because it is **global across inner tabs** — the one
    /// control on this tab that does not follow the selection, which `:114` and `:116` make the
    /// difference between deliberately. It writes through to `CddlDocSet::start_production`, so the
    /// model stays the source of truth and the box is only its editor.
    start_rule_input: Entity<InputState>,
    /// The module-search-path box (`Settings::module_search_paths`), in the CDDL right pane.
    ///
    /// The editor of a settings field rather than of the model, which is what makes it the odd one
    /// out beside [`Ide::start_rule_input`]: the paths are not part of any document, so they are
    /// written straight into [`Ide::settings`] and persisted by the quit-time [`Ide::persist`] with
    /// no new write path. One line of platform-path-separator-joined text, parsed by
    /// [`parse_search_paths`].
    search_paths_input: Entity<InputState>,
    /// The pane geometry, and the sizes that outlive the session.
    panes: Entity<layout::Panes>,
    /// What the lower window shows: an append-only log of what the user asked for and what came
    /// back. Seeded with the settings-file diagnostics, which is why `Settings::load_default`
    /// returns them rather than logging them.
    diagnostics: Vec<Diagnostic>,
    /// Whether the CBOR buffer currently decodes, re-derived after every byte written.
    ///
    /// A field of its own rather than a row in [`Ide::diagnostics`], and the separation is load
    /// bearing. This is a *derived* property with exactly one value at a time, while the list is a
    /// log; the first version of this kept both in the list and had [`Ide::recheck_cbor`] retain out
    /// every `Source::Cbor` row, which (a) duplicated the decode error whenever a conversion
    /// reported the same failure and (b) silently deleted a failed *save* on the next keystroke,
    /// because [`Ide::report`] stamps `Source::Cbor` too while the CBOR tab is selected. One slot
    /// makes the ownership structural instead of a convention nothing enforced.
    well_formed: Option<Diagnostic>,
    /// Whether the EDN buffer currently parses, re-derived after every edit.
    ///
    /// A slot of its own for the same reason [`Ide::well_formed`] is one, and a *second* slot
    /// rather than sharing that one because the two tabs hold independent documents: an EDN syntax
    /// error says nothing about the CBOR buffer, and one slot would let whichever was rechecked
    /// last erase the other's report.
    edn_parse: Option<Diagnostic>,
    /// What the last CDDL check found, re-derived on every edit and on every `Syntax Check`.
    ///
    /// A derived slot for the reason [`Ide::well_formed`]'s doc records, and a *list* rather than
    /// one value because a schema legitimately has many errors at once — unlike "does this buffer
    /// decode", which has one answer. Keyed by file, because the lower window's whole job on this
    /// tab is saying *which* file in the chain is wrong (`UI_DESIGN.md:111-112`), and because the
    /// per-file results have to be replaceable one file at a time: an edit in one inner tab must
    /// not drop another tab's findings.
    ///
    /// A `BTreeMap` so the rows come out in a stable order rather than a hash order that reshuffles
    /// the lower window on every rebuild.
    cddl_diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    /// What the last chain-wide `Syntax Check` found, as distinct from the per-file checks.
    ///
    /// Separate from [`Ide::cddl_diagnostics`] because the two have different lifetimes: per-file
    /// findings are replaced on every keystroke, while these come from reading the whole chain off
    /// disk and stay until the next explicit check. Merging them would make a keystroke in one
    /// file silently discard the cross-file findings that are still true.
    chain_diagnostics: Vec<Diagnostic>,
    /// What the last `Validate` produced, or `None` when there is no current answer.
    ///
    /// The one owner of every span the three highlight layers are drawn from, and the thing the
    /// honesty rule is about: see the module docs. Cleared by [`Ide::clear_trace`] on any edit to
    /// any of the three documents, which is why `None` is a normal state and not an error.
    trace: Option<cddl::ChainTrace>,
    /// What the last `Validate` found, with the inert-span adjustment already applied.
    ///
    /// A derived slot for the reason [`Ide::well_formed`]'s doc records, and *stored* rather than
    /// read back out of [`Ide::trace`] per render because the honesty rule's adjustment — a
    /// mismatch whose CBOR span the hex document cannot be shown to hold is stored with its span
    /// cleared to `0..0` — is a pass over the list. It also carries the start-production note, which
    /// `ChainTrace` deliberately does not: only this knows where to point it.
    trace_diagnostics: Vec<Diagnostic>,
    /// The `Validate` running on the background executor right now, if one is.
    ///
    /// One field behind all four things a 30-second operation needs: the re-entrancy guard in
    /// [`Ide::on_validate`], the `loading` flag on both `Validate` buttons,
    /// [`menus::MenuState::validating`], and the progress row. One field so they cannot disagree
    /// about whether the application is busy.
    validating: Option<Validating>,
    /// The trace log's rows, filtered and attributed, as [`Ide::rebuild_trace_rows`] last left them.
    ///
    /// Precomputed and behind an `Rc` because the log is a `uniform_list` whose closure is `'static`
    /// and runs per visible row per frame; filtering is a pass over every event and attribution is a
    /// scan over the chain's files, and neither belongs on that path. `cddl::trace::rows` exists for
    /// exactly this split.
    trace_rows: Rc<Vec<cddl::Row>>,
    /// Which page the lower window shows. Kept in step with the selected tab by
    /// [`Ide::set_tab`], which is the only thing that moves it other than a click on the strip.
    lower: LowerTab,
    /// The trace log's scroll position. Held here because `uniform_list` needs a handle that
    /// outlives the frame, the same way [`hex_view::HexView`]'s does.
    trace_scroll: UniformListScrollHandle,
    /// The key store, opened at start-up from [`Settings::keystore_dir`].
    ///
    /// Model state, so it lives here and not in a pane — exactly like [`Ide::settings`], and for the
    /// concrete reason that the four operation handlers need it and none of them is a pane's.
    /// `None` only when opening it failed, which [`Ide::keystore_error`] then explains.
    keystore: Option<KeyStore>,
    /// Why there is no key store, when there is none.
    ///
    /// Held rather than pushed straight into [`Ide::diagnostics`] because the `Keys` page shows it
    /// in place of the list: a row saying "no keys" over a store that could not be opened is a lie,
    /// and a diagnostic scrolled off the bottom of another page is not an answer to "where are my
    /// keys".
    keystore_error: Option<String>,
    /// The selected key's alias (`UI_DESIGN.md:128`). The alias and not an index, so a list rebuilt
    /// after a generate or a delete cannot silently move the selection to another key.
    selected_key: Option<String>,
    /// The key list, as [`Ide::sync_keys`] last read it out of the store.
    ///
    /// Behind an `Rc` and refreshed per change rather than per frame, the same discipline
    /// [`Ide::trace_rows`] keeps: `KeyStore::list` is a borrow of the store, and a render that held
    /// it could not also take `&mut self` to answer a click.
    key_list: Rc<Vec<KeyMeta>>,
    /// The selected key's detail rows, extracted once per selection change.
    ///
    /// `None` exactly when nothing is selected. A COSE document no stored key accounts for leaves
    /// this alone — the selection is still a real key and the `Keys` page still marks it;
    /// [`Ide::cose_options`] shows `UI_DESIGN.md:129`'s "Unknown key" *in place of* these rows,
    /// because the rows answer a question about the loaded object and that answer is not the
    /// selected key.
    key_detail: Option<KeyDetail>,
    /// Which stored key accounts for the loaded COSE document (`UI_DESIGN.md:129`).
    ///
    /// Re-derived by [`Ide::recheck_identity`] whenever that document changes and **never per
    /// render**: the second pass is a trial verification against every eligible key, which is real
    /// cryptography and has no business on a paint path. The Phase 6 discipline for
    /// [`Ide::trace_diagnostics`], applied again.
    key_identity: cose::KeyIdentity,
    /// Whether [`Ide::private_input`] still has to be given the selected key's value.
    ///
    /// The model-to-editor deferral `edn_view`'s module docs set out: `InputState::set_value` and
    /// `set_masked` both need a `&mut Window`, and [`Ide::sync_keys`] is reached from
    /// [`Ide::open_path`] and from dialog callbacks that have none.
    key_pending: bool,
    /// The masked box the private value is shown in (`UI_DESIGN.md:130`).
    ///
    /// An `InputState` rather than a `div` and a reveal flag, for one property that is not
    /// reproducible by hand: a masked input is not copyable, because `InputStyles::is_copyable` is
    /// `selection && !masked` (`gpui-base/src/input/base/mod.rs:107`), so Copy and Cut are
    /// unavailable while the value is hidden. The residual exposure is that revealing it puts
    /// private material into the editor's non-zeroizing `Rope`; `PLAN.md` records it, and it is what
    /// a reveal button costs.
    private_input: Entity<InputState>,
    /// The `Encrypt` algorithm picker's state, and the **only** owner of that choice.
    ///
    /// No `encrypt_alg: EncryptAlgorithm` field beside it: a second copy would need a
    /// `SelectEvent` subscription to stay in step, and two answers to "which algorithm"
    /// is the bug the old frontend had from the other direction, hardcoding `A128Gcm` in three
    /// places. [`Ide::on_encrypt`] reads `selected_value()` at the moment it needs it.
    encrypt_alg: Entity<SelectState<Vec<AlgChoice>>>,
    /// The COSE tab's upper window.
    cose: Entity<CoseView>,
    /// The in-window menu bar. macOS uses the system menu bar instead, so there is nothing to
    /// render there — see [`menus::install`].
    #[cfg(not(target_os = "macos"))]
    menu_bar: Entity<AppMenuBar>,
    /// The window's own focus. Keybindings are registered in the `Ide` context
    /// (`actions::CONTEXT`), and GPUI dispatches an action along the focus chain — so without
    /// something focused inside this view, not one shortcut would fire.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Ide {
    fn new(
        settings: Settings,
        diagnostics: Vec<Diagnostic>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let explorer = cx.new(Explorer::new);
        let panes = cx.new(|cx| layout::Panes::new(settings.layout.clone(), cx));

        let workspace = cx.new(|_| match settings.workspace_root.clone() {
            Some(root) => Workspace::at_root(root),
            None => Workspace::new(),
        });
        let hex = cx.new(|cx| HexView::new(workspace.clone(), cx));
        let edn = cx.new(|cx| EdnView::new(workspace.clone(), window, cx));
        // Takes a `Settings` clone rather than a config or a search path: `Settings::cddl_config`
        // returns a `tps_cddl::Config`, and this crate can produce one but cannot *name* it, so the
        // view calls that method itself. See `cddl_view`'s field documentation.
        let cddl = cx.new(|cx| CddlView::new(workspace.clone(), settings.clone(), cx));
        // Seeded from the settings rather than left blank: `Settings::start_rule` is what the user
        // last asked for, and `CddlDocSet::start_production` is seeded from the same place.
        let start_rule = settings.start_rule.clone().unwrap_or_default();
        let start_rule_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("first rule in the schema")
                .default_value(start_rule)
        });
        // The box and the model are seeded from one value, so the first `Validate` honours what
        // was persisted even if the box is never touched.
        workspace.update(cx, |workspace, _| {
            workspace.cddl.start_production = settings.start_rule.clone().unwrap_or_default();
        });
        // Seeded from the settings and *rendered* from the box thereafter, which is the asymmetry
        // worth noting: this box is the only editor of `module_search_paths` while the window is
        // open, so nothing else can change it behind the box's back and no re-seeding is needed.
        let search_paths_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(format!(
                    "directories holding `;# include` modules, {PATH_LIST_SEPARATOR}-separated"
                ))
                .default_value(format_search_paths(&settings.module_search_paths))
        });

        let cose = cx.new(|cx| CoseView::new(workspace.clone(), window, cx));
        // Masked from the start, and masked again on every selection change — the two sites
        // [`keys::MASKED`] exists to keep in step.
        let private_input = cx.new(|cx| InputState::new(window, cx).masked(keys::MASKED));
        // Seeded at row 0 so `selected_value()` always has an answer; the list is
        // `ops::ENCRYPT_ALGORITHMS`' own order, whose first entry is `A128GCM` — the one the old
        // frontend hardcoded, here as a default the user can change rather than a decision.
        let encrypt_alg = cx.new(|cx| {
            SelectState::new(
                keys::alg_choices(),
                Some(IndexPath::default().row(0)),
                window,
                cx,
            )
            .searchable(false)
        });

        // `KeyStore::open` creates the directory `0o700` and writes an empty index when there is
        // none, so this is also what brings the store into existence on a first run. Failing is
        // survivable — every other tab works without a key store — so it is recorded rather than
        // fatal, and the `Keys` page shows the reason in place of the list.
        let (keystore, keystore_error) = match settings.keystore_dir() {
            Some(dir) => match KeyStore::open(&dir) {
                Ok(store) => (Some(store), None),
                Err(error) => (None, Some(format!("{}: {error}", dir.display()))),
            },
            // `keystore_dir` resolves `None` in the settings file to `<config dir>/keys`, so this
            // is the narrower case of there being no config directory at all.
            None => (
                None,
                Some(
                    "there is no configuration directory, so there is nowhere to keep keys"
                        .to_owned(),
                ),
            ),
        };

        let subscriptions = vec![
            // `subscribe_in` rather than `subscribe`, which is the only subscription here that
            // needs the distinction: [`Ide::open_path`] takes a window as of Phase 7, because one
            // of its five destinations is a dialog rather than a document.
            cx.subscribe_in(
                &explorer,
                window,
                |this, _, event: &ExplorerEvent, window, cx| match event {
                    ExplorerEvent::Open(path) => this.open_path(path.clone(), window, cx),
                },
            ),
            // The hex view writes to the shared model directly; this is how the rest of the
            // application finds out. An observer on the workspace would fire for `Ide`'s own
            // writes too and re-enter this during a save.
            cx.subscribe(&hex, |this, _, event: &HexEvent, cx| match event {
                HexEvent::Edited => this.after_edit(cx),
                // A refusal is a warning and a report is a note, which is the distinction
                // [`Severity::Info`] was added for: `Paste` refused because the document is
                // read-only and `Copy` took 94 bytes must not look alike in the list.
                HexEvent::Failed(message) => this.warn(message.clone(), cx),
                HexEvent::Noted(message) => this.note(message.clone(), cx),
            }),
            cx.subscribe(&edn, |this, _, event: &EdnEvent, cx| match event {
                EdnEvent::Edited => this.after_edn_edit(cx),
                EdnEvent::Failed(message) => this.note(message.clone(), cx),
            }),
            cx.subscribe(&cddl, |this, _, event: &CddlEvent, cx| match event {
                CddlEvent::Edited => this.after_cddl_edit(cx),
                CddlEvent::Failed(message) => this.note(message.clone(), cx),
            }),
            // The box is the editor of `CddlDocSet::start_production`, not a second copy of it, so
            // every keystroke writes through. `start()` trims and treats blank as unset, so there
            // is no "is this empty or whitespace" rule duplicated here.
            cx.subscribe(&start_rule_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value().to_string();
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.cddl.start_production = value;
                        cx.notify();
                    });
                    cx.notify();
                }
            }),
            // Two events rather than one, which is the difference between this box and the start-
            // production box above it. `Change` *records* — the parsed list goes into the settings
            // on every keystroke, so a window quit mid-edit persists what was typed rather than
            // what was there before. `PressEnter` and `Blur` *apply* — re-resolving the include
            // chain means reading every file in it from disk, and doing that per keystroke would
            // re-read the chain a dozen times while a path is half-typed, each time against a
            // directory name that does not exist yet, each time appending a diagnostic row saying
            // so. The plan called for "a change re-runs the syntax check"; this is that at the
            // granularity of a completed edit, and `PLAN.md` records the divergence.
            cx.subscribe_in(
                &search_paths_input,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.settings.module_search_paths =
                            parse_search_paths(input.read(cx).value().as_ref());
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.apply_cddl_settings(window, cx)
                    }
                    InputEvent::Focus => {}
                },
            ),
        ];

        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        // The window button is a quit route too, so it asks the same question `File -> Quit` does.
        // Returning `false` only *blocks* the close — the prompt is raised here and whichever
        // answer the user gives calls `cx.quit()` itself, so both routes end up in one place.
        let entity = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            entity.update(cx, |ide, cx| {
                // Same reason as in [`Ide::on_quit`]: `dirty()` is only true once the edit has
                // reached the model, and the editor's last keystrokes may not have.
                ide.flush_edn(cx);
                ide.flush_cddl(cx);
                if !ide.workspace.read(cx).dirty() {
                    return true;
                }
                ide.confirm_quit(window, cx);
                false
            })
        });

        let mut ide = Self {
            settings,
            workspace,
            tab: Tab::default(),
            explorer,
            hex,
            edn,
            cddl,
            start_rule_input,
            search_paths_input,
            panes,
            diagnostics,
            well_formed: None,
            edn_parse: None,
            cddl_diagnostics: BTreeMap::new(),
            chain_diagnostics: Vec::new(),
            trace: None,
            trace_diagnostics: Vec::new(),
            validating: None,
            trace_rows: Rc::new(Vec::new()),
            lower: LowerTab::default(),
            trace_scroll: UniformListScrollHandle::new(),
            keystore,
            keystore_error,
            selected_key: None,
            key_list: Rc::new(Vec::new()),
            key_detail: None,
            key_identity: cose::KeyIdentity::default(),
            // The masked box starts empty and has to be *told* so, for the reason
            // [`CoseView::new`] sets its own flag: the first paint is what pushes model state into
            // an editor, and nothing else will.
            key_pending: true,
            private_input,
            encrypt_alg,
            cose,
            #[cfg(not(target_os = "macos"))]
            menu_bar: AppMenuBar::new(cx),
            focus,
            _subscriptions: subscriptions,
        };
        ide.sync_explorer(cx);
        // Before the menus: `menu_state` reads `selected_key`, and `sync_keys` is what fills the
        // list the selection has to be in.
        ide.sync_keys(cx);
        ide.refresh_menus(cx);
        ide
    }

    /// The state the menu bar reflects, read from whichever document the selected tab shows.
    fn menu_state(&self, cx: &App) -> menus::MenuState {
        let meta = self.active_meta(cx);
        menus::MenuState {
            tab: self.tab,
            has_path: meta.as_ref().is_some_and(Meta::can_save),
            // No document at all counts as read-only: there is nothing to paste into.
            read_only: meta.is_none_or(|meta| meta.read_only),
            has_content: self.has_content(cx),
            key_selected: self.selected_key.is_some(),
            // The CBOR document specifically, whatever tab is selected: `Sign` and `Encrypt` read
            // it as their input even from the COSE tab. See [`menus::MenuState::cbor_content`].
            cbor_content: !self.workspace.read(cx).cbor.is_empty(),
            validating: self.validating.is_some(),
        }
    }

    /// The selected tab's document metadata, if it has one. Only the CDDL tab can be empty of
    /// documents — it is the one tab holding a *set* of files (`UI_DESIGN.md:108`).
    ///
    /// Cloned rather than borrowed because the model is behind an entity: a `&Meta` would hold a
    /// borrow of the `App` for as long as it was alive, and every caller needs `&mut` access
    /// afterwards to push a diagnostic or rebuild a menu. `Meta` is a path and two flags.
    fn active_meta(&self, cx: &App) -> Option<Meta> {
        let workspace = self.workspace.read(cx);
        match self.tab {
            Tab::Cbor => Some(workspace.cbor.meta.clone()),
            Tab::Edn => Some(workspace.edn.meta.clone()),
            Tab::Cddl => workspace.cddl.active().map(|doc| doc.meta.clone()),
            Tab::Cose => Some(workspace.cose.meta.clone()),
        }
    }

    /// Whether the selected tab has anything to convert, validate or check.
    fn has_content(&self, cx: &App) -> bool {
        let workspace = self.workspace.read(cx);
        match self.tab {
            Tab::Cbor => !workspace.cbor.is_empty(),
            Tab::Edn => !workspace.edn.is_empty(),
            Tab::Cddl => workspace.cddl.active().is_some_and(|doc| !doc.is_empty()),
            Tab::Cose => !workspace.cose.is_empty(),
        }
    }

    /// The diagnostic source the selected tab's failures belong to.
    fn source(&self) -> Source {
        match self.tab {
            Tab::Cbor => Source::Cbor,
            Tab::Edn => Source::Edn,
            Tab::Cddl => Source::Cddl,
            Tab::Cose => Source::Cose,
        }
    }

    /// Rebuilds the menu and — off macOS — makes the bar pick the rebuild up.
    ///
    /// `AppMenuBar` does not observe the menu list, so the `reload` is not optional; see
    /// [`menus`]'s module documentation.
    fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let state = self.menu_state(cx);
        menus::install(state, cx);

        #[cfg(not(target_os = "macos"))]
        {
            let menu_bar = self.menu_bar.clone();
            menu_bar.update(cx, |bar, cx| bar.reload(cx));
        }
    }

    /// Hands the explorer the workspace, so it can rescan a changed root and recompute which rows
    /// are bold and which are bold red (`UI_DESIGN.md:56`).
    fn sync_explorer(&mut self, cx: &mut Context<Self>) {
        let (explorer, workspace) = (self.explorer.clone(), self.workspace.clone());
        explorer.update(cx, |explorer, cx| explorer.sync(&workspace, cx));
    }

    /// Re-reads the key list out of the store and re-derives the selected key's detail rows.
    ///
    /// The `Keys` page's counterpart to [`Ide::sync_explorer`], called after every write to the
    /// store and after every change of selection. Both lists are *snapshots*: `KeyStore::list`
    /// hands back a borrow of the store, and a render holding one could not also take `&mut self`
    /// to answer a click on it.
    ///
    /// Written as decide-clear-produce rather than in one pass, because the one-pass version does
    /// not compile: the detail is read through a borrow of `self.keystore`, and assigning to
    /// `self.key_detail` while that borrow is alive is a second borrow of `self`.
    fn sync_keys(&mut self, cx: &mut Context<Self>) {
        // A selection the store no longer holds is dropped rather than left naming nothing.
        // `Delete Key` is the obvious case; an index rewritten by another copy of the tool is the
        // same situation with no gesture of ours behind it to clear up after.
        let stale = self
            .keystore
            .as_ref()
            .zip(self.selected_key.as_deref())
            .is_some_and(|(store, alias)| !store.contains(alias));
        if stale || self.keystore.is_none() {
            self.selected_key = None;
        }

        let (list, detail) = match self.keystore.as_ref() {
            Some(store) => {
                let detail = self.selected_key.as_deref().and_then(|alias| {
                    let meta = store.meta(alias)?;
                    // The one place private material is read, and it is handed over for exactly
                    // the length of the [`KeyDetail::of`] call: `load` returns a `Zeroizing`
                    // buffer, dropped and zeroed at the end of this closure, and `KeyDetail` keeps
                    // only strings.
                    //
                    // A failed load still describes the key from its index entry, which is what
                    // makes a store whose files and index disagree recoverable: the row can be
                    // selected, and so deleted. Not reported from here — `sync_keys` runs on every
                    // selection change, and a diagnostic per click would bury the one that matters.
                    let loaded = store.load(alias).ok();
                    let bytes = loaded.as_ref().map_or(&[][..], |bytes| bytes.as_slice());
                    Some(KeyDetail::of(meta, bytes))
                });
                (store.list().to_vec(), detail)
            }
            None => (Vec::new(), None),
        };

        self.key_list = Rc::new(list);
        self.key_detail = detail;
        // The masked box holds the *previous* key's value until the next paint pushes this one in;
        // see [`Ide::key_pending`].
        self.key_pending = true;
        cx.notify();
    }

    /// Selects a key by alias, or clears the selection.
    ///
    /// Guarded on a change because re-selecting the row already selected would re-mask the private
    /// box under the user's own reveal, which reads as the eye button not working.
    fn select_key(&mut self, alias: Option<String>, cx: &mut Context<Self>) {
        if self.selected_key == alias {
            return;
        }
        self.selected_key = alias;
        self.sync_keys(cx);
        // The four operations are greyed on `key_selected`, so the menu bar has to hear about this.
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Re-derives which stored key accounts for the loaded COSE document (`UI_DESIGN.md:129`).
    ///
    /// Called after every change to that document and after every change to the store, and **never
    /// from a render**: [`cose::identify`]'s second pass is a trial verification against every
    /// eligible key, which is real cryptography. The Phase 6 discipline for
    /// [`Ide::rebuild_trace_rows`], applied where the cost is even less arguable.
    fn recheck_identity(&mut self, cx: &mut Context<Self>) {
        let identity = match self.keystore.as_ref() {
            Some(store) => cose::identify(self.workspace.read(cx).cose.bytes(), store),
            // With no store there is nothing to identify *against*. `NotCose` rather than
            // `Unknown`, which would claim the document is a COSE object nothing accounts for.
            None => cose::KeyIdentity::NotCose,
        };
        self.key_identity = identity;
        cx.notify();
    }

    /// Re-renders the COSE document into its read-only EDN view, and says so when it cannot.
    ///
    /// Called after every write to that document — an open, a `Sign`, an `Encrypt` — because the
    /// view holds a *rendering* and not the bytes: without this the pane would go on showing the
    /// previous object's EDN beside the new object's `Save`.
    ///
    /// A failure is reported rather than left as a silently empty pane, and it should be
    /// unreachable for anything this tool produces — `ops::sign` and `ops::encrypt` emit
    /// well-formed CBOR. It is reported *because* it should be unreachable.
    ///
    /// The span is `0..0` deliberately, which makes the row unclickable: [`diagnostic_target`]
    /// answers `None` for every `Source::Cose` diagnostic, because the offset `cbor_to_edn` failed
    /// at is a position in the COSE *bytes* while the pane shows EDN rendered from them — there is
    /// no character in that editor to move a caret to.
    fn reload_cose(&mut self, cx: &mut Context<Self>) {
        if let Err(message) = self.cose.update(cx, |cose, cx| cose.reload(cx)) {
            self.diagnostics
                .push(Diagnostic::error(Source::Cose, 0..0, message));
        }
        // The document changed, so which key accounts for it is a question with a new answer.
        self.recheck_identity(cx);
        cx.notify();
    }

    /// Why a keystore command cannot run, in the user's words.
    ///
    /// Always read into a local *before* the `as_mut()` that needs it, which is the reason it is a
    /// method and not an inline `match`: a `let ... else` arm that reads [`Ide::keystore_error`]
    /// while a mutable borrow of [`Ide::keystore`] is alive does not compile, and splitting it in
    /// two at every call site is how that is avoided — at the cost of one `String` on the path
    /// where there *is* a store, which is a fraction of what any of these commands then does.
    fn no_keystore(&self) -> String {
        match &self.keystore_error {
            Some(reason) => format!("there is no key store: {reason}"),
            // Unreachable: [`Ide::new`] sets the two together. The honest answer if they ever come
            // apart, rather than an `expect` in a menu handler.
            None => "there is no key store".to_owned(),
        }
    }

    /// Selects a tab. Everything tab-dependent — the menu, the right pane, the tab strip — follows
    /// from this one field.
    fn select(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.set_tab(tab);
        self.focus_tab(window, cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Gives the keyboard to the selected tab's upper window.
    ///
    /// Called from [`Ide::select`], where the newly shown pane has to be able to take the keyboard
    /// or the hex grid's arrow keys and digits go nowhere — and from the clipboard handlers, which
    /// need it for a quite different reason. `window.dispatch_action` captures the *currently*
    /// focused element and dispatches up its chain (`window.rs:2558-2570`), so forwarding `Copy` to
    /// an editor that does not hold focus would deliver it to whatever does: the menu bar, or the
    /// explorer tree. Focusing first is what makes the menu item act on the pane the user is
    /// looking at.
    ///
    /// Every tab has a pane that can hold focus. The COSE editor is read-only and still wants it,
    /// for Select All, Copy and the scroll keys.
    fn focus_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tab {
            Tab::Cbor => self.hex.update(cx, |hex, cx| hex.focus(window, cx)),
            Tab::Edn => self.edn.update(cx, |edn, cx| edn.focus(window, cx)),
            Tab::Cddl => self.cddl.update(cx, |cddl, cx| cddl.focus(window, cx)),
            Tab::Cose => self.cose.update(cx, |cose, cx| cose.focus(window, cx)),
        }
    }

    /// Selects `tab`, moving the lower window to a page that tab actually offers.
    ///
    /// The **only** writer of [`Ide::tab`], which is what makes [`page_after`]'s rules hold however
    /// the tab was reached — by a click, by opening a file, or by an operation's result landing
    /// somewhere. The rule itself is that free function, so it can be read and tested without a
    /// window; this is the assignment and nothing else, and the order matters: the page is decided
    /// from the tab being *left*.
    fn set_tab(&mut self, tab: Tab) {
        self.lower = page_after(self.tab, tab, self.lower);
        self.tab = tab;
    }

    /// Opens a directory as the workspace root, or a file into the tab [`route`] sends it to.
    ///
    /// Takes `window` as of Phase 7, which is why the explorer's subscription is a `subscribe_in`:
    /// one of the five destinations is a *dialog* rather than a document, because a COSE key cannot
    /// be loaded into one (`tps_cbor_core/CLAUDE.md` forbids taking private material out of
    /// `Zeroizing`, and a `CborDoc` holds a plain `Vec<u8>`), so opening one offers to store it.
    fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.is_dir() {
            self.workspace.update(cx, |workspace, cx| {
                workspace.root = Some(path);
                cx.notify();
            });
            self.sync_explorer(cx);
            cx.notify();
            return;
        }

        // Before the chain can be replaced: `open_root` reports the *dirty* tabs it displaced, and
        // `dirty()` is only true once the edit has reached the model. Without this an editor's
        // unsaved last keystrokes would be discarded with no warning at all.
        self.flush_cddl(cx);

        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        // The bytes, for the routes that are decided by *content* rather than by name. Read here
        // and once: whichever route they take, these are the bytes the destination adopts — and on
        // the import route no document adopts them at all, which is the reason the read is not left
        // to `CborDoc::open`.
        //
        // `CoreError::io` is the error `CborDoc::open` would itself have produced for an
        // unreadable file (`doc.rs:139`), so nothing about the message changes by reading it here.
        let bytes = match extension.as_str() {
            // Nothing is read for these two: they are routed by name, and `TextDoc::open` and
            // `open_root` below are what report an unreadable one.
            "edn" | "cddl" => Vec::new(),
            _ => match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.report(Err(CoreError::io(&path, error)), cx);
                    return;
                }
            },
        };

        let opened = match route(&extension, &bytes) {
            // Answered first, before anything touches a document, because nothing on this route
            // may; see this function's own documentation. The dialog is also why `open_path` needs
            // a window at all.
            Route::ImportKey => {
                self.offer_import(path, bytes, window, cx);
                return;
            }
            Route::Edn => self.workspace.update(cx, |workspace, cx| {
                cx.notify();
                TextDoc::open(&path).map(|doc| {
                    workspace.edn = doc;
                    (Tab::Edn, Vec::new())
                })
            }),
            // `open_root` and not `open`: opening from the menu or the explorer starts a *new*
            // chain, while `open` appends — which is what a directive click wants and what
            // [`Ide::open_include`] uses. Without the distinction the inner tab bar accumulates
            // every CDDL file touched in the session, and `TabBar` has no close affordance to shed
            // them with.
            Route::Cddl => self.workspace.update(cx, |workspace, cx| {
                cx.notify();
                workspace
                    .cddl
                    .open_root(&path)
                    .map(|displaced| (Tab::Cddl, displaced))
            }),
            // A COSE_Sign1 or COSE_Encrypt0, which is what `UI_DESIGN.md:129`'s "loaded COSE item"
            // means and what the comment that used to be here deferred to this phase.
            //
            // `from_bytes` plus `Meta::at` rather than `CborDoc::open`, which would read the file a
            // second time — and give the file a second chance to have changed between the sniff and
            // the load, so that the document shown would not be the one that was recognised.
            // `from_bytes` alone leaves the document pathless, and `Save` needs the path.
            Route::Cose => self.workspace.update(cx, |workspace, cx| {
                workspace.cose = CborDoc::from_bytes(bytes);
                workspace.cose.meta = Meta::at(&path);
                cx.notify();
                Ok((Tab::Cose, Vec::new()))
            }),
            // Ordinary CBOR, and anything unrecognised. `.cbor` is the documented default
            // (`UI_DESIGN.md:68`), and an unrecognised extension still has a well-defined reading
            // as CBOR — the hex view is the one tab that can show an arbitrary file.
            Route::Cbor => self.workspace.update(cx, |workspace, cx| {
                workspace.cbor = CborDoc::from_bytes(bytes);
                workspace.cbor.meta = Meta::at(&path);
                cx.notify();
                Ok((Tab::Cbor, Vec::new()))
            }),
        };

        match opened {
            Ok((tab, displaced)) => {
                // An explorer with no root is an empty explorer, and a user who opened a file by
                // name has told us which directory they are working in.
                self.workspace.update(cx, |workspace, cx| {
                    if workspace.root.is_none() {
                        workspace.root = path.parent().map(Path::to_path_buf);
                    }
                    cx.notify();
                });

                // Through [`Ide::set_tab`], so a COSE object opened from the explorer shows the key
                // list exactly as one reached by clicking the tab does.
                self.set_tab(tab);
                // The chain the merged findings describe has just been replaced, and unlike the
                // per-file ones they are a flat list with no file to key them on, so there is
                // nothing to retain them by: they go. Only on the CDDL path, because opening a
                // `.cbor` or `.edn` file leaves the chain — and so its findings — alone.
                if tab == Tab::Cddl {
                    self.chain_diagnostics.clear();
                }
                // Both documents may have been replaced wholesale, so both derived checks are
                // re-run rather than only the one the opened extension named — and the editor is
                // told to pick its text up, which it does at the next paint (`edn_view`'s docs say
                // why it cannot be done here).
                self.edn.update(cx, |edn, cx| edn.reload(cx));
                self.cddl.update(cx, |cddl, cx| cddl.reload(cx));
                // And the COSE pane, whichever route was taken: `Verify` and `Decrypt` read the
                // COSE document, so a *CBOR* file opened under a stale COSE rendering would leave
                // the two panes describing different bytes.
                self.reload_cose(cx);
                // A document replaced wholesale is the strongest form of the edit the honesty rule
                // is about: not one span moved, every one of them indexes a file that is gone.
                self.clear_trace(cx);
                self.recheck_cbor(cx);
                self.recheck_edn(cx);
                self.recheck_cddl(cx);
                // Said rather than swallowed: `open_root` has already dropped these documents, so
                // this row is the only record that there was unsaved work in them. Listed by name,
                // because "some files" is not something a user can act on.
                if !displaced.is_empty() {
                    let names = displaced
                        .iter()
                        .map(|path| base_name(path))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.warn(
                        format!("discarded unsaved changes in {names} to open a new chain"),
                        cx,
                    );
                }
                self.sync_explorer(cx);
                self.refresh_menus(cx);
                cx.notify();
            }
            Err(error) => self.report(Err(error), cx),
        }
    }

    /// Adds `path` to the CDDL chain under exploration and selects it (`UI_DESIGN.md:108`).
    ///
    /// The counterpart to [`Ide::open_path`]'s `open_root`: following an `;# include` is an act of
    /// *navigation* within one chain, so the tab is appended and the other tabs stay. A file
    /// already open is selected rather than opened twice — `CddlDocSet::open` dedupes canonically
    /// and returns the existing index, which is exactly the wanted behaviour here.
    fn open_include(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.flush_cddl(cx);
        let opened = self.workspace.update(cx, |workspace, cx| {
            let opened = workspace.cddl.open(&path);
            if let Ok(index) = opened {
                workspace.cddl.select(index);
            }
            cx.notify();
            opened
        });

        match opened {
            Ok(_) => {
                // `Keys` is not a page the CDDL tab offers, so the lower window has to move with
                // the selection — which is [`Ide::set_tab`]'s whole job.
                self.set_tab(Tab::Cddl);
                self.cddl.update(cx, |cddl, cx| cddl.reload(cx));
                self.recheck_cddl(cx);
                self.sync_explorer(cx);
                self.refresh_menus(cx);
                cx.notify();
            }
            Err(error) => self.report(Err(error), cx),
        }
    }

    /// Everything that has to happen after a byte is written.
    ///
    /// One function rather than a handler per writer, because every one of them invalidates the
    /// same three things: whether the buffer still decodes, which files the explorer should show
    /// as dirty, and whether `Save` should be offered.
    fn after_edit(&mut self, cx: &mut Context<Self>) {
        // Before the recheck, which puts the decode mark back: `clear_trace` drops every mark
        // layer, and doing it afterwards would erase the one this keystroke just produced.
        self.clear_trace(cx);
        self.recheck_cbor(cx);
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Re-derives [`Ide::well_formed`] and the hex view's highlight.
    ///
    /// Runs after every byte written, so it has to be quiet about the one failure a half-finished
    /// edit always produces. [`convert::is_truncation`] is what distinguishes "the input stops
    /// early" from "the input is wrong", and a truncation is reported as a **warning** rather than
    /// suppressed outright: a genuinely short file still has to be visible, but the error count
    /// does not swing on every keystroke while a length byte is being retyped.
    fn recheck_cbor(&mut self, cx: &mut Context<Self>) {
        let Err(error) = convert::check_cbor(self.workspace.read(cx).cbor.bytes()) else {
            self.well_formed = None;
            self.hex
                .update(cx, |hex, cx| hex.mark(MarkKind::Decode, Vec::new(), cx));
            return;
        };

        let span = decode_span(&error, self.workspace.read(cx).cbor.len());
        let severity = if convert::is_truncation(&error) {
            Severity::Warning
        } else {
            Severity::Error
        };

        let mut diagnostic =
            Diagnostic::new(severity, Source::Cbor, span.clone(), error.to_string());
        if let Some(path) = self.workspace.read(cx).cbor.meta.path.clone() {
            diagnostic = diagnostic.in_file(path);
        }
        self.well_formed = Some(diagnostic);

        self.hex
            .update(cx, |hex, cx| hex.mark(MarkKind::Decode, vec![span], cx));
    }

    /// Everything that has to happen after the EDN document is edited.
    ///
    /// The [`after_edit`](Ide::after_edit) of the EDN tab, and separate from it because the two
    /// re-derive different checks over different documents — a keystroke in the editor cannot
    /// change whether the CBOR buffer decodes.
    fn after_edn_edit(&mut self, cx: &mut Context<Self>) {
        self.clear_trace(cx);
        self.recheck_edn(cx);
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Re-derives [`Ide::edn_parse`] and the editor's inline squiggle.
    ///
    /// The EDN analogue of [`Ide::recheck_cbor`], and quiet about truncation for the same reason:
    /// [`diag::edn_diagnostic`] grades an unterminated value as a **warning**, because an editor
    /// mid-keystroke routinely holds an unclosed array and painting that red is noise.
    ///
    /// The empty-document rule lives in [`convert::check_edn`] rather than here, because it is a
    /// rule the TUI has to agree with and it is testable there without a display.
    ///
    /// Also re-publishes the **validation** squiggles, because the editor holds one `DiagnosticSet`
    /// and `reset` empties it on every mutation: one owner has to supply both kinds or whichever
    /// published last deletes the other. They are gated by [`edn_qualifies`] — a mismatch's `edn`
    /// span is a coordinate into the canonical rendering, and a hand-edited document is a different
    /// string.
    fn recheck_edn(&mut self, cx: &mut Context<Self>) {
        // Owned rather than borrowed: `publish` below needs `cx` mutably, and the path has to be
        // read before that borrow too.
        let text = self.workspace.read(cx).edn.text().to_owned();
        let path = self.workspace.read(cx).edn.meta.path.clone();

        let diagnostic = convert::check_edn(&text)
            .err()
            .map(|error| diag::edn_diagnostic(&text, &error).in_file_opt(path.as_deref()));

        let validation = self
            .trace
            .as_ref()
            .filter(|trace| edn_qualifies(trace, &text))
            .map(cddl::ChainTrace::edn_diagnostics)
            .unwrap_or_default();

        self.edn_parse = diagnostic.clone();
        self.edn
            .update(cx, |edn, cx| edn.publish(diagnostic, &validation, cx));
    }

    /// Writes the editor's text into the model before anything reads it.
    ///
    /// Called at the top of every consumer — save, both conversions, `Validate`, the decode and the
    /// quit guard — because **undo and redo emit no `Change`**, so the event-driven mirror alone
    /// would let `Save` write the text from before a Ctrl-Z. `edn_view`'s module docs carry the
    /// detail; this is the call site the argument is about.
    fn flush_edn(&mut self, cx: &mut Context<Self>) {
        let outcome = self.edn.update(cx, |edn, cx| edn.flush(cx));
        if let Err(error) = outcome {
            self.report(Err(error), cx);
        }
    }

    /// The CDDL analogue of [`Ide::flush_edn`], writing *every* inner tab's editor back.
    ///
    /// Every tab and not only the selected one, because an edit in one file followed by a switch to
    /// another leaves the first editor holding text the model has not seen — and a `Save All` or a
    /// quit guard asks about the model.
    fn flush_cddl(&mut self, cx: &mut Context<Self>) {
        let outcome = self.cddl.update(cx, |cddl, cx| cddl.flush(cx));
        if let Err(error) = outcome {
            self.report(Err(error), cx);
        }
    }

    /// Everything that has to happen after a CDDL file is edited.
    ///
    /// The [`after_edit`](Ide::after_edit) of the CDDL tab. The AST tree is deliberately *not*
    /// rebuilt here; `cddl_view`'s module docs give the three reasons, and the right pane says so
    /// on screen once the tree no longer matches the text.
    fn after_cddl_edit(&mut self, cx: &mut Context<Self>) {
        // The old frontend cleared the trace on a CDDL change too (`DebugPanel.tsx:131`); the
        // honesty rule only generalises that to the other two documents.
        self.clear_trace(cx);
        self.recheck_cddl(cx);
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Re-derives the selected CDDL tab's diagnostics and its directive table.
    ///
    /// Runs after every keystroke, so it does the least that keeps the editor honest:
    ///
    /// * [`cddl::check_module`] on the **selected tab only**, which is the only tab that can have
    ///   changed. Rechecking the others would reparse three unchanged files per keystroke.
    /// * `check_module` rather than `check`, so that `rfc8392.cddl`'s references to rules defined
    ///   in `rfc9052.cddl` are warnings and not a dozen red squiggles on correct code.
    /// * No chain resolution. That reads four files from disk, and — for the reason
    ///   [`Ide::on_syntax_check`] records — would describe the files on disk rather than the text on
    ///   screen.
    ///
    /// `lex_check` was considered for this path and is not needed: [`cddl::check`] returns before
    /// it calls `build_cddl` whenever the source does not lex, so it never offers half-typed
    /// unlexable text to the documented-panicking parser. The `catch_unwind` inside `check` covers
    /// the malformed-but-grammatical case that gets past the lexer.
    fn recheck_cddl(&mut self, cx: &mut Context<Self>) {
        let Some((path, text)) = self.workspace.read(cx).cddl.active().and_then(|doc| {
            doc.meta
                .path
                .clone()
                .map(|path| (path, doc.text().to_owned()))
        }) else {
            // Nothing open, or a document with no path — which the CDDL set cannot hold, since
            // every tab enters it through `open`. Clearing is still right: a closed chain's
            // findings are not about anything on screen.
            self.cddl_diagnostics.clear();
            self.chain_diagnostics.clear();
            return;
        };

        let search_paths = self.cddl_search_paths(&path);
        let result = cddl::check_module(&text, &self.settings.cddl_config());
        let diagnostics: Vec<Diagnostic> = result
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.in_file(path.clone()))
            .collect();

        let directives = cddl::chain::directives(&text, &search_paths);

        // Nothing on record may name a file that is not an inner tab. `CddlDocSet::open_root`
        // replaces the chain wholesale, and this map is only ever *inserted* into — one entry per
        // file as it is checked — so without the retain the lower window goes on reporting
        // findings against files the user can no longer select. Rows naming a file that is not
        // open read as a current problem with the chain that *is* open, which is worse than no row
        // at all. A chain is four files, so the linear scan is cheaper than a set.
        let open: Vec<PathBuf> = self
            .workspace
            .read(cx)
            .cddl
            .tabs()
            .iter()
            .filter_map(|doc| doc.meta.path.clone())
            .collect();
        self.cddl_diagnostics
            .retain(|file, _| open.iter().any(|path| path == file));

        self.cddl_diagnostics
            .insert(path.clone(), diagnostics.clone());
        self.cddl.update(cx, |cddl, cx| {
            cddl.publish(path.clone(), diagnostics, cx);
            cddl.publish_links(path, directives, cx);
        });
    }

    /// Drops the trace, its rows, its diagnostics and every highlight it produced.
    ///
    /// Called from all three edit paths, which is the first half of the honesty rule the module docs
    /// state: every span in all three coordinate spaces is displaced by one keystroke, and a
    /// highlight drawn from a displaced span points *confidently* at the wrong value. All three
    /// layers go together — there is no edit that invalidates one and spares the others.
    ///
    /// The EDN editor is re-published rather than told to forget, because `DiagnosticSet::reset`
    /// empties the set on every mutation: there is no way to drop the validation squiggles and keep
    /// the parse error separately, so the one owner supplies both (see `edn_view`'s module docs).
    fn clear_trace(&mut self, cx: &mut Context<Self>) {
        // This runs after every keystroke in three documents, and `EdnView::publish` notifies
        // unconditionally. The early return is what keeps a whole re-publish off that path.
        if self.trace.is_none() && self.trace_diagnostics.is_empty() {
            return;
        }
        self.trace = None;
        self.trace_diagnostics.clear();
        self.trace_rows = Rc::new(Vec::new());

        self.hex.update(cx, |hex, cx| hex.clear_marks(cx));
        let parse = self.edn_parse.clone();
        self.edn.update(cx, |edn, cx| {
            edn.publish(parse, &[], cx);
            edn.mark(Vec::new(), cx);
        });
        self.cddl.update(cx, |cddl, cx| {
            cddl.mark(None, cx);
            cddl.publish_status(HashMap::new(), cx);
        });
        cx.notify();
    }

    /// Recomputes [`Ide::trace_rows`] from the trace and the persisted filter.
    ///
    /// Called when a trace arrives and when a filter toggles, and **never** from a render — the
    /// reason is on the field.
    fn rebuild_trace_rows(&mut self, cx: &mut Context<Self>) {
        let show = cddl::TraceOutcome::from_filter(&self.settings.trace_filter);
        self.trace_rows = Rc::new(match &self.trace {
            Some(trace) => cddl::trace::rows(trace, &show),
            None => Vec::new(),
        });
        cx.notify();
    }

    /// Pushes a changed [`Settings`] into [`cddl_view::CddlView`] and re-derives everything that
    /// reads it.
    ///
    /// The one place the CDDL right pane's three settings controls land, because all three change
    /// the same thing: what `Settings::cddl_config` and [`Ide::cddl_search_paths`] answer, which is
    /// the dialect every parse in this tab is performed under.
    ///
    /// Two halves, and both are needed:
    ///
    /// * The **push**, because the view holds a *clone* of the settings for the reason its own field
    ///   documentation gives — this crate can produce a `tps_cddl::Config` but cannot name one — so
    ///   `Ide`'s copy changing does not change the view's.
    /// * The **re-check**, via [`Ide::on_syntax_check`], because a stale diagnostic list after a
    ///   dialect change is worse than none: the rows would describe the schema under the settings
    ///   the user has just stopped asking for, and would look authoritative doing it. Going through
    ///   the handler rather than reimplementing its two passes is also what makes an unticked
    ///   checkbox and the `Syntax Check` button produce the same thing — including its refusal when
    ///   an inner tab is unsaved and the chain cannot honestly be read from disk.
    ///
    /// The handler is called directly rather than dispatched. `window.dispatch_action` defers to
    /// the focused element's chain (`gpui-pre/src/window.rs:2557-2570`), and the focused element
    /// here is the checkbox or the path box in the right pane — so the action would arrive, but a
    /// paint later and by a route that only works because the right pane happens to sit inside the
    /// `Ide` key context. A method call says what is meant.
    fn apply_cddl_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        self.cddl
            .update(cx, |cddl, cx| cddl.set_settings(settings, cx));
        // Guarded even though the pane this is reached from only renders on the CDDL tab: the
        // handler's own first act is to refuse off that tab, and a refusal the user cannot have
        // caused would be a puzzle rather than a message.
        if syntax_check_applies(self.tab) {
            self.on_syntax_check(&actions::SyntaxCheck, window, cx);
        }
        cx.notify();
    }

    /// Hands the CDDL editors the rule tinting the current trace implies.
    ///
    /// One call for the whole chain, which is why [`cddl_view::CddlView::publish_status`] takes a
    /// map rather than one file: an empty map clears every file, so a file the new run did not reach
    /// cannot keep the previous run's tint.
    ///
    /// An unattributed chain produces an empty map — `rule_status_by_file` returns nothing rather
    /// than guessing at a file — so nothing is tinted anywhere, which is the same visible
    /// degradation Phase 5 chose for the same reason.
    fn publish_rule_status(&mut self, cx: &mut Context<Self>) {
        let status: HashMap<PathBuf, Vec<cddl::RuleStatus>> = match &self.trace {
            Some(trace) => trace
                .rule_status_by_file()
                .into_iter()
                .filter_map(|(index, spans)| Some((chain_file(trace, index)?, spans)))
                .collect(),
            None => HashMap::new(),
        };
        self.cddl
            .update(cx, |cddl, cx| cddl.publish_status(status, cx));
    }

    /// The module search path for a CDDL file: its own directory, then the configured list.
    ///
    /// The prepended parent is not optional, and its absence is silent.
    /// `tps_cddl::ModuleSearchPath::resolve` iterates only the directories it was given and never
    /// the including file's own, so without this a `;# include rfc9052` beside the file naming it
    /// does not resolve at all — which, on a settings file that has never been edited, is every
    /// chain in the corpus.
    fn cddl_search_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
        paths.extend(self.settings.module_search_paths.iter().cloned());
        paths
    }

    /// Records the outcome of an operation, showing failures in the lower window.
    ///
    /// Errors are *added* to the diagnostics list rather than replacing the user's content with an
    /// error string — `tps_cbor_gui/PLAN.md:623-628` records that mistake being made twice.
    fn report(&mut self, outcome: tps_cbor_core::Result<()>, cx: &mut Context<Self>) {
        if let Err(error) = outcome {
            let diagnostic = Diagnostic::error(self.source(), 0..0, error.to_string());
            let path = self.active_meta(cx).and_then(|meta| meta.path);
            self.diagnostics.push(match path {
                Some(path) => diagnostic.in_file(path),
                None => diagnostic,
            });
        }
        // A save clears a dirty flag, so the explorer's bold-red set has changed, and so has
        // whether `Save` should be offered at all.
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Adds an informational note to the lower window: *this is what the command did*.
    ///
    /// The lower window is the only place this application can report a success, so the note that
    /// an Encrypt produced 94 bytes goes in the same list as a CDDL error. [`Severity::Info`]
    /// exists to keep them apart; until Phase 8 this pushed a `warning`, so a refused Sign and a
    /// successful Encrypt were rendered identically and the list could not be skimmed.
    ///
    /// Use [`Ide::warn`] for a command that *declined* to do something.
    fn note(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let source = self.source();
        self.diagnostics
            .push(Diagnostic::info(source, 0..0, message));
        cx.notify();
    }

    /// Adds a warning to the lower window: *the command did not do what you asked, and here is
    /// why*.
    ///
    /// The counterpart to [`Ide::note`] and the reason that one could be demoted to
    /// [`Severity::Info`]. Three kinds of message belong here: a command refusing to run (no key
    /// selected, the buffer does not decode), a key that cannot perform the requested operation,
    /// and a side effect the user should notice but did not ask for (unsaved changes discarded to
    /// open a new chain).
    fn warn(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let source = self.source();
        self.diagnostics
            .push(Diagnostic::warning(source, 0..0, message));
        cx.notify();
    }

    /// Reports a failure that is not a [`CoreError`], so [`Ide::report`] cannot carry it.
    ///
    /// The one producer today is the system file picker, whose failures arrive as a
    /// `gpui::Result` from `prompt_for_paths` and `prompt_for_new_path`. Those were discarded
    /// until a WSLg session proved what that costs: `xdg-desktop-portal-gtk` there runs with
    /// neither `DISPLAY` nor `WAYLAND_DISPLAY` in its environment, so it can open no window at
    /// all, and `Open...` and `Save As...` did *nothing whatever* — no dialog, no row, no hint
    /// that the fault lay outside the application. GPUI has prose for exactly this case
    /// (`FILE_PICKER_PORTAL_MISSING`, `gpui-pre-linux/src/linux/platform.rs:810`); it was being
    /// thrown away one layer above.
    ///
    /// Attributed to the active tab, like [`Ide::report`]'s failures, because
    /// [`tps_cbor_core::Source`] has no variant for the application itself and inventing one for
    /// this would mean a new arm in every match over it. The message names the command, so the
    /// row reads sensibly whichever tab it is filed under; the span is `0..0`, which makes it
    /// unclickable, and there is nothing in any document to point at anyway.
    fn fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let source = self.source();
        self.diagnostics
            .push(Diagnostic::error(source, 0..0, message));
        cx.notify();
    }

    fn on_open(&mut self, _: &actions::Open, _: &mut Window, cx: &mut Context<Self>) {
        // `PathPromptOptions` has no extension filter, so `UI_DESIGN.md:68`'s "default to .cbor
        // files" cannot be expressed through the system picker; recorded in `PLAN.md`.
        //
        // Files only, which is a correction. Both were asked for here until Phase 7's interactive
        // verification tried it: on Linux the request becomes a folder-*only* chooser and this
        // command could not open a file at all. The explorer's root moved to
        // [`Ide::on_open_folder`]; see [`actions::OpenFolder`] for the whole account.
        self.prompt_and_open("Open", path_prompt(Kind::Files), cx);
    }

    /// `File → Open Folder...`, which sets the explorer's root.
    ///
    /// A separate command from [`Ide::on_open`] because one picker cannot offer files and
    /// directories at once; [`actions::OpenFolder`] records why. The path goes to the same
    /// [`Ide::open_path`], which already does the right thing with a directory — so the two
    /// commands differ in what the picker shows and in nothing else.
    fn on_open_folder(&mut self, _: &actions::OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open("Open Folder", path_prompt(Kind::Directories), cx);
    }

    /// Runs the system picker and opens what comes back.
    ///
    /// Shared by the two open commands rather than written twice, because the only difference
    /// between them is the `options` — and the error handling below is the part that must not be
    /// allowed to drift between two copies. `command` names the menu item in a failure row, so the
    /// user is told which of the two could not run.
    fn prompt_and_open(
        &mut self,
        command: &'static str,
        options: PathPromptOptions,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(options);

        cx.spawn(async move |this, cx| {
            match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        // `update_in` rather than `update`: the picker's answer arrives with no
                        // window, and `open_path` needs one for the import dialog. It takes the
                        // window the entity is in, which is the only one this application has.
                        let _ =
                            this.update_in(cx, |ide, window, cx| ide.open_path(path, window, cx));
                    }
                }
                // Cancelled, or dismissed with nothing chosen. Nothing to do and nothing to say.
                Ok(Ok(None)) => {}
                // The picker could not run at all — see [`Ide::fail`] for what that looked like
                // before it was reported.
                Ok(Err(error)) => {
                    let _ = this.update(cx, |ide, cx| ide.fail(format!("{command}: {error}"), cx));
                }
                // The sender was dropped, which happens when the application is shutting down.
                // There is no window left to report into.
                Err(_) => {}
            }
        })
        .detach();
    }

    fn on_save(&mut self, _: &actions::Save, _: &mut Window, cx: &mut Context<Self>) {
        self.flush_edn(cx);
        self.flush_cddl(cx);
        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = match self.tab {
                Tab::Cbor => workspace.cbor.save(),
                Tab::Edn => workspace.edn.save(),
                Tab::Cose => workspace.cose.save(),
                Tab::Cddl => match workspace.cddl.active_mut() {
                    Some(doc) => doc.save(),
                    // Unreachable from the menu, which greys `Save` out without a path — but the
                    // keybinding has no such guard.
                    None => Err(CoreError::NoPath("CDDL")),
                },
            };
            cx.notify();
            outcome
        });
        self.report(outcome, cx);
    }

    fn on_save_as(&mut self, _: &actions::SaveAs, _: &mut Window, cx: &mut Context<Self>) {
        self.flush_edn(cx);
        self.flush_cddl(cx);
        let meta = self.active_meta(cx);
        let directory = self
            .workspace
            .read(cx)
            .root
            .clone()
            .or_else(|| {
                meta.as_ref()
                    .and_then(|meta| meta.path.as_deref())
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        // A free function so the rule is testable without a window; see [`suggested_save_name`].
        let suggested = suggested_save_name(meta.as_ref(), self.tab);

        // Absolute, because the portal's `SaveFile` request takes `current_folder` as an absolute
        // path (`gpui-pre-linux/src/linux/platform.rs:861`) and every source above can be
        // relative: `workspace.root` is whatever the command line said, a document's parent
        // inherits that, and the fallback is literally `"."`. `std::path::absolute` does not touch
        // the filesystem, so a directory that has since been removed still yields a path and the
        // picker still opens where it can.
        let directory = std::path::absolute(&directory).unwrap_or(directory);
        let path = cx.prompt_for_new_path(&directory, Some(suggested.as_str()));
        cx.spawn(async move |this, cx| {
            match path.await {
                Ok(Ok(Some(path))) => {
                    let _ = this.update(cx, |ide, cx| ide.save_as(path, cx));
                }
                // Cancelled: the document keeps the name it had, and nothing is written.
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |ide, cx| ide.fail(format!("Save As: {error}"), cx));
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    fn save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = match self.tab {
                Tab::Cbor => workspace.cbor.save_as(path),
                Tab::Edn => workspace.edn.save_as(path),
                Tab::Cose => workspace.cose.save_as(path),
                Tab::Cddl => match workspace.cddl.active_mut() {
                    Some(doc) => doc.save_as(path),
                    None => Err(CoreError::NoPath("CDDL")),
                },
            };
            cx.notify();
            outcome
        });
        self.report(outcome, cx);
    }

    /// The selected tab's editing engine, or `None` when the tab has none.
    ///
    /// The one dispatch point for the six `Edit` menu commands, so that each handler is a sentence
    /// rather than a four-arm match. Two tabs answer `None` and they mean different things, which
    /// is why every caller below tests `self.tab` *before* calling this:
    ///
    /// * **CBOR** has no text editor at all; [`hex_view::HexView`] answers all six itself.
    /// * **CDDL** holds a *set* of documents (`UI_DESIGN.md:108`) and may hold none, in which case
    ///   there is nothing to search or paste into and the honest answer is to say so.
    ///
    /// Returns the entity by value rather than a borrow: every caller needs `&mut Context<Self>`
    /// afterwards, to `update` the editor or to push a diagnostic, and a `&Entity` would hold a
    /// borrow of the `App` across that.
    fn editor_for_tab(&self, cx: &App) -> Option<Entity<EditorState>> {
        match self.tab {
            Tab::Cbor => None,
            Tab::Edn => Some(self.edn.read(cx).editor().clone()),
            Tab::Cddl => self.cddl.read(cx).active_state(cx),
            Tab::Cose => Some(self.cose.read(cx).editor().clone()),
        }
    }

    /// `UI_DESIGN.md:52`'s `Find...`, and the `Replace...` beside it this phase adds.
    ///
    /// Both are **forwarded** to the editor rather than reimplemented: `EditorState::new` sets
    /// `searchable = true` (`gpui-base/src/input/base/state.rs:12266`), so all three text panes have
    /// answered `ctrl-f` and `ctrl-h` since Phase 3 — it was only the menu path that was dead. The
    /// two bindings coexist rather than shadowing, because gpui-base binds its own in the deeper
    /// `"Input"` key context while ours live in the single root `Ide` one; see
    /// [`Ide::unhandled_actions`], whose doc comment claimed the opposite for six phases.
    ///
    /// `open_search` is **not idempotent** — it advances `search_activation_revision` and the panel
    /// answers that by re-selecting the query field — so it is reached only from here, an action
    /// handler, and never from a render pass.
    fn open_search(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Cbor {
            if replace {
                // The menu item is disabled on this tab, so this is the keyboard path's backstop.
                // The reason is beside the item in [`menus`]: the hex grid searches bytes in a
                // fixed-length buffer, where "replace all" has no meaning the grid can express.
                return self.warn("Replace does not apply to the hex grid; use `Find...`", cx);
            }
            self.focus_tab(window, cx);
            return self.hex.update(cx, |hex, cx| hex.open_search(window, cx));
        }
        let Some(editor) = self.editor_for_tab(cx) else {
            return self.warn("there is no CDDL file open to search", cx);
        };
        // Before `open_search`, not after: the search panel claims the keyboard on the next paint
        // from the activation revision, and focusing the pane afterwards would take it straight
        // back off it.
        self.focus_tab(window, cx);
        editor.update(cx, |state, cx| state.open_search(replace, cx));
    }

    fn on_find(&mut self, _: &actions::Find, window: &mut Window, cx: &mut Context<Self>) {
        self.open_search(false, window, cx);
    }

    fn on_replace(&mut self, _: &actions::Replace, window: &mut Window, cx: &mut Context<Self>) {
        self.open_search(true, window, cx);
    }

    /// `Find Next` and `Find Previous`, which wrap in both engines.
    ///
    /// Takes no `&mut Window`: neither engine needs one. The editor's own
    /// `next_search_match` scrolls the match into view itself, and the hex grid's `step_search`
    /// moves the caret and calls its `reveal`.
    ///
    /// A `None` from the editor means there is no active search — the query is empty, or the
    /// session was never opened — rather than "no matches for a query you typed", which the panel
    /// reports itself. So the note says what to do about it.
    fn step_search(&mut self, forwards: bool, cx: &mut Context<Self>) {
        if self.tab == Tab::Cbor {
            return self.hex.update(cx, |hex, cx| hex.step_search(forwards, cx));
        }
        let Some(editor) = self.editor_for_tab(cx) else {
            return self.warn("there is no CDDL file open to search", cx);
        };
        let found = editor.update(cx, |state, cx| match forwards {
            true => state.next_search_match(cx),
            false => state.previous_search_match(cx),
        });
        if found.is_none() {
            self.note("nothing to step through; use `Find...` to set a search", cx);
        }
    }

    fn on_find_next(&mut self, _: &actions::FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step_search(true, cx);
    }

    fn on_find_previous(
        &mut self,
        _: &actions::FindPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_search(false, cx);
    }

    /// `UI_DESIGN.md:53`'s `Copy`, which must be offered on every tab in every state —
    /// `menus.rs`'s `quit_help_and_find_are_always_offered` asserts the item is enabled, and this
    /// is what makes that mean something.
    ///
    /// The editor path goes through the **action system** rather than calling the engine: its
    /// `copy` is `pub(super)` (`gpui-base/src/input/base/state.rs:3115`), and the public route is
    /// the `input::Copy` the engine binds for itself. The full path is written out at every use so
    /// it stays visibly distinct from [`actions::Copy`], which is *this* command.
    fn on_copy(&mut self, _: &actions::Copy, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Cbor {
            return self.hex.update(cx, |hex, cx| hex.copy(cx));
        }
        if self.editor_for_tab(cx).is_none() {
            return self.warn("there is no CDDL file open to copy from", cx);
        }
        self.forward(Box::new(gpui_kit::component::input::Copy), window, cx);
    }

    /// `UI_DESIGN.md:53`: *"Paste does not work on files which are marked read-only."*
    ///
    /// Refused here as well as in the model. [`menus`] disables the item, and
    /// [`CborDoc::set_byte`](tps_cbor_core::CborDoc::set_byte) and the engine's own `readonly` flag
    /// both refuse underneath — but the keyboard reaches this past a greyed menu item, and both
    /// backstops are *silent*. A row that names the reason is the difference between a read-only
    /// document and a broken Paste.
    fn on_paste(&mut self, _: &actions::Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(meta) = self.active_meta(cx) else {
            return self.warn("there is no CDDL file open to paste into", cx);
        };
        if meta.read_only {
            return self.warn("this document is read-only, so Paste is refused", cx);
        }
        if self.tab == Tab::Cbor {
            return self.hex.update(cx, |hex, cx| hex.paste(cx));
        }
        self.forward(Box::new(gpui_kit::component::input::Paste), window, cx);
    }

    /// Focuses the selected tab's pane and dispatches one of the editing engine's own actions to
    /// it.
    ///
    /// The focus is the load-bearing half. `dispatch_action` reads the *currently* focused element
    /// and then defers delivery up that element's chain (`gpui-pre/src/window.rs:2557-2570`), so a
    /// `Copy` dispatched while the menu bar or the explorer tree held focus would be delivered
    /// there — to an element that does not answer it. Focusing first is what makes the menu item
    /// act on the pane the user is looking at.
    fn forward(&mut self, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab(window, cx);
        window.dispatch_action(action, cx);
    }

    /// `UI_DESIGN.md:79`: the CBOR tab's `To EDN` button, which "is EDN decoded and copied into the
    /// top main window of the EDN tab".
    ///
    /// The tab is deliberately *not* switched. `UI_DESIGN.md:79` says the conversion "allows the
    /// Binary to be displayed as EDN **when switching** to the EDN screen", so switching is the
    /// user's move; a button that silently navigates away is also the harder one to undo.
    fn on_to_edn(&mut self, _: &actions::ToEdn, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab != Tab::Cbor {
            return self.warn(
                "`To EDN` converts the CBOR tab's buffer; select it first",
                cx,
            );
        }
        // Refused up front rather than letting the conversion fail, which would put the *same*
        // decode message in the log beside the one [`Ide::well_formed`] is already showing. One
        // authoritative row for what is wrong with the buffer, and this says what to do about it.
        if self.well_formed.is_some() {
            return self.warn("the buffer does not decode; fix the error below first", cx);
        }

        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = workspace.cbor_to_edn();
            cx.notify();
            outcome
        });
        if outcome.is_ok() {
            // The EDN document was replaced wholesale, so the editor is told to pick it up and the
            // parse check is re-derived against the new text.
            self.edn.update(cx, |edn, cx| edn.reload(cx));
            self.recheck_edn(cx);
            let bytes = self.workspace.read(cx).edn.text().len();
            self.note(format!("converted to {bytes} characters of EDN"), cx);
        }
        self.report(outcome, cx);
    }

    /// `UI_DESIGN.md:95`: the EDN tab's `To CBOR` button.
    ///
    /// The mirror of [`Ide::on_to_edn`], and it does not switch tabs either, for the same reason.
    ///
    /// Non-destructive by construction rather than by care taken here: [`Workspace::edn_to_cbor`]
    /// applies `?` to the encode before it touches `workspace.cbor`, so a parse failure leaves the
    /// CBOR document bit-for-bit as it was.
    fn on_to_cbor(&mut self, _: &actions::ToCbor, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab != Tab::Edn {
            return self.warn("`To CBOR` converts the EDN tab's text; select it first", cx);
        }
        self.flush_edn(cx);
        self.recheck_edn(cx);
        // Refused up front rather than letting the encode fail, which would put the *same* parse
        // message in the log beside the one [`Ide::edn_parse`] is already showing — the argument
        // [`Ide::on_to_edn`] makes about `well_formed`.
        if self.edn_parse.is_some() {
            return self.warn("the EDN does not parse; fix the error below first", cx);
        }

        // Read before the conversion, which is what clears the path: afterwards there is no way to
        // tell "kept its name" from "never had one".
        let kept_name = self
            .workspace
            .read(cx)
            .cbor_path_after_conversion_from_edn();

        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = workspace.edn_to_cbor();
            cx.notify();
            outcome
        });
        if outcome.is_ok() {
            let bytes = self.workspace.read(cx).cbor.len();
            let note = if kept_name {
                format!("converted to {bytes} bytes of CBOR")
            } else {
                // `UI_DESIGN.md:96`. Said out loud because the only other sign is `Save` going grey.
                format!(
                    "converted to {bytes} bytes of CBOR; the CBOR filename was cleared, so use `Save As`"
                )
            };
            self.note(note, cx);
            self.recheck_cbor(cx);
        }
        self.report(outcome, cx);
    }

    /// `UI_DESIGN.md:93`: decode the selected EDN "as a separate CBOR object".
    ///
    /// The spec says what to decode but not where the result goes or how the command is reached.
    /// Both were settled with the user and are recorded in `PLAN.md` as divergences, since
    /// `UI_DESIGN.md` is not edited to match the implementation: the bytes **load into the CBOR
    /// Binary tab** with the filename cleared, and the command is reached from the editor's
    /// right-click menu, where the selection already is. That reuses the hex view, `To EDN` and
    /// `Validate` instead of inventing a second viewer.
    ///
    /// A byte string is unwrapped to its payload — the spec's motivating case is a COSE_Sign1
    /// payload bstr — and anything else loads as its own encoding;
    /// [`convert::decode_edn_selection`] carries that rule and its tests. The payload is not
    /// re-checked here: [`Ide::recheck_cbor`] runs below and will mark a malformed one with a
    /// ranged diagnostic exactly as it does for a malformed file, so a second differently-worded
    /// check would only duplicate the row.
    fn on_decode_selection(
        &mut self,
        _: &actions::DecodeSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab != Tab::Edn {
            return self.warn(
                "decoding a selection reads the EDN tab; select it first",
                cx,
            );
        }
        self.flush_edn(cx);

        // The context-menu item is disabled without a selection, so this is the keyboard-less
        // equivalent of a guard that should not fire — but `report`'s precedent is to say what
        // happened rather than do nothing visible.
        let Some(selection) = self.edn.read(cx).selection(cx) else {
            return self.warn("select the EDN to decode first", cx);
        };

        // The decode, the load and the cleared path are one core operation, so the non-destructive
        // guarantee is structural: `load_decoded_selection` applies `?` before it writes anything.
        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = workspace.load_decoded_selection(&selection);
            cx.notify();
            outcome
        });

        let count = match outcome {
            Ok(count) => count,
            // No ranged marker: the error's offset is relative to the *trimmed fragment*, not to
            // the document, so drawing it in the editor would underline the wrong characters — and
            // the inline slot belongs to the whole-document parse, which has one owner by design.
            // Reported as a plain row instead, with both documents left as they were.
            Err(error) => return self.report(Err(error), cx),
        };

        self.recheck_cbor(cx);
        // The note has to name where the bytes went, because the tab is deliberately not switched
        // — the rule [`Ide::on_to_edn`] states — so otherwise nothing visible would change.
        self.note(
            format!(
                "decoded the selection into {count} bytes in the CBOR Binary tab (unnamed — use `Save As` to keep it)"
            ),
            cx,
        );
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// `UI_DESIGN.md:81-83` and `:97-98`: the `Validate` button, and with it the tracer.
    ///
    /// Validates a CBOR document against the **CDDL tab's resolved include chain** and keeps the
    /// trace, which is the decision log the lower window's `Trace` page renders and the three
    /// highlight layers are drawn from.
    ///
    /// Which bytes depends on the tab, and the difference is the specification's:
    ///
    /// * **CBOR tab** — the document itself.
    /// * **EDN tab** — `:97`'s "the CBOR corresponding to the displayed EDN", so the displayed text
    ///   is encoded to a *transient* buffer. `Validate` is not `To CBOR` and must not overwrite the
    ///   CBOR document, which is precisely why the hex grid may hold entirely different bytes and
    ///   why [`hex_qualifies`] exists.
    ///
    /// A buffer that does not decode, or EDN that does not parse, is answered with the failure
    /// already on screen rather than handed to the tracer: `ChainTrace::check` would report the
    /// same fault a second time in its own words, and two rows about one problem read as two
    /// problems.
    ///
    /// Which schema is not a choice either: the chain rooted at the **first** inner tab, through
    /// [`Ide::resolve_chain`] — the same schema `Syntax Check` checks and the one the
    /// start-production box applies to. Not the selected inner tab's own text, because a module on
    /// its own is not a schema, which is the whole reason `check_module` exists.
    ///
    /// The verdict is one more row in [`Ide::trace_diagnostics`] rather than a [`Ide::note`], so
    /// that it lives and dies with the trace it describes; see the push at the end for why that
    /// matters.
    fn on_validate(&mut self, _: &actions::Validate, _: &mut Window, cx: &mut Context<Self>) {
        // The menu item and both buttons go inert while a trace is in flight, but `ctrl-shift-v`
        // reaches this handler directly and needs its own guard. Refused rather than superseded:
        // `ChainTrace::check` is one synchronous call that cannot be interrupted, so a superseded
        // run would keep a core busy to completion and only have its result thrown away.
        if let Some(running) = &self.validating {
            let message = format!("a validation against `{}` is already running", running.root);
            return self.warn(message, cx);
        }
        if !validate_applies(self.tab) {
            return self.warn(
                "`Validate` checks a CBOR document against the CDDL tab's schema; \
                 select the CBOR Binary or EDN tab first",
                cx,
            );
        }

        let bytes = match self.tab {
            Tab::Cbor => {
                self.recheck_cbor(cx);
                if let Some(offset) = self.well_formed.as_ref().map(|found| found.span.start) {
                    self.hex.update(cx, |hex, cx| hex.reveal(offset, cx));
                    return;
                }
                self.workspace.read(cx).cbor.bytes().to_vec()
            }
            _ => {
                self.flush_edn(cx);
                self.recheck_edn(cx);
                if let Some(offset) = self.edn_parse.as_ref().map(|found| found.span.start) {
                    // The caret goes to the offending character; the squiggle is already there
                    // from [`Ide::recheck_edn`], and the row is already in the lower window.
                    self.edn.update(cx, |edn, cx| edn.reveal(offset, cx));
                    return;
                }
                let text = self.workspace.read(cx).edn.text().to_owned();
                match convert::edn_to_cbor(&text) {
                    Ok(bytes) => bytes,
                    // `check_edn` passed and the encode still failed, which is a fault in the
                    // document the parse check does not cover — a bignum too large to encode, say.
                    Err(error) => return self.report(Err(error), cx),
                }
            }
        };

        let Some((root, unsaved)) = self.resolve_chain(cx) else {
            return self.warn(
                "`Validate` needs a schema: open a .cddl file in the CDDL tab first",
                cx,
            );
        };
        if !unsaved.is_empty() {
            // The same gate `Syntax Check` applies, and for a sharper reason: a merged span
            // displaced by unsaved edits would attribute a trace row to the right file and the
            // wrong line, which looks authoritative. Named rather than counted, so the user knows
            // which.
            return self.warn(
                format!(
                    "`Validate` reads the include chain from disk; save {} and run it again",
                    unsaved.join(", ")
                ),
                cx,
            );
        }

        let search_paths = self.cddl_search_paths(&root);
        let override_rule = self.workspace.read(cx).cddl.start().map(str::to_owned);
        let had_override = override_rule.is_some();
        let config = self.settings.cddl_config();
        let chain_root = root.clone();

        self.validating = Some(Validating::new(base_name(&root)));
        cx.notify();

        // Everything above this line reads entities through `cx` and is fast; everything below is
        // one CPU-bound call that took ~30 seconds on `examples/large_uint_array.cbor` and froze
        // the window for all of it. `ChainTrace::check` needs no borrow of the application: it
        // takes slices and owned configuration, reads the whole include chain from disk itself,
        // and hands back an owned `ChainTrace` whose every field is plain data. So it moves onto
        // the background executor as it stands.
        //
        // Nothing guards against the document changing while it runs, deliberately.
        // [`hex_qualifies`] and [`edn_qualifies`] already compare `ChainTrace::cbor` against what
        // the panes hold and withhold the spans when they disagree, so an edit mid-trace degrades
        // through machinery that was written for the EDN tab's transient buffer and needs no
        // second copy here.
        cx.spawn(async move |this, cx| {
            let trace = cx
                .background_spawn(async move {
                    cddl::ChainTrace::check(
                        &bytes,
                        &chain_root,
                        &search_paths,
                        override_rule.as_deref(),
                        &config,
                    )
                })
                .await;
            let _ = this.update(cx, |ide, cx| ide.apply_trace(trace, root, had_override, cx));
        })
        .detach();
    }

    /// Put a finished [`cddl::ChainTrace`] on screen: the second half of [`Ide::on_validate`].
    ///
    /// Split out so the trace itself can run on the background executor. Everything here needs the
    /// main thread — it reads the three documents to decide whether the trace's spans still
    /// describe them, and writes the marks, the trace rows and the lower window.
    ///
    /// `root` and `had_override` are carried across the await rather than recovered from the trace.
    /// The root is in `trace.chain` and the override is *nearly* recoverable from `trace.start`,
    /// but "nearly" is the problem: `Start` records which rule was chosen, not whether the caller
    /// named it, and the span below has to be `0..0` exactly when the caller did.
    fn apply_trace(
        &mut self,
        trace: cddl::ChainTrace,
        root: PathBuf,
        had_override: bool,
        cx: &mut Context<Self>,
    ) {
        self.validating = None;

        // `ChainTrace` deliberately leaves `Start::note` to the caller, because only the caller
        // knows where to point it: a name typed into the start-production box has no line in any
        // file, so that report goes to the top of the root rather than onto a `;# start` line the
        // user did not write. The same split [`Ide::on_syntax_check`] makes.
        let span = match had_override {
            true => 0..0,
            false => {
                let text = self.workspace.read(cx).cddl.tabs()[0].text().to_owned();
                start_directive_span(&text).unwrap_or(0..0)
            }
        };

        let qualifies = hex_qualifies(&trace, self.workspace.read(cx).cbor.bytes());
        // The EDN pane's own gate, read here so the notes below can explain a disqualification
        // once, beside the verdict, rather than leaving the user to work out why an icon is grey.
        // An empty pane is not a stale rendering and gets no note: "run `To EDN`" is advice, not a
        // finding, and the hint line in the right pane already gives it.
        let edn_stale = {
            let edn = self.workspace.read(cx).edn.text();
            !edn.is_empty() && !edn_qualifies(&trace, edn)
        };
        let mut diagnostics = trace.diagnostics.clone();
        if !qualifies {
            // The honesty rule, in the one form the lower window understands: a row whose span
            // cannot be verified against the document the click would scroll is given the
            // project's "there is no span" value, which makes it inert and blanks its location
            // column through the machinery already in `diagnostic_target` and
            // `diagnostic_location`. The *message* is untouched — what the schema wanted is still
            // true and still worth reading.
            for diagnostic in &mut diagnostics {
                if diagnostic.source == Source::Validation {
                    diagnostic.span = 0..0;
                }
            }
        }
        diagnostics.extend(trace.start.note(span).map(|note| note.in_file(root)));
        diag::sort(&mut diagnostics);

        // The verdict, appended after the sort so it reads as the summary of the rows above it
        // rather than being filed among them by its span.
        //
        // Into the trace's own slot and **not** through [`Ide::note`], which is the history log:
        // "the document validates" is a statement about one document at one moment, and in the log
        // it would survive the next `Open` and sit beside the errors from a different file saying
        // the opposite. [`Ide::clear_trace`] drops it with the trace it belongs to. `Validation` as
        // the source and `0..0` as the span, which is the project's "there is no span" value — so
        // the row is inert and blanks its own location column, and the hex marks filter it out.
        diagnostics.push(Diagnostic::warning(
            Source::Validation,
            0..0,
            match (trace.is_valid(), &trace.start.name) {
                (true, Some(rule)) => format!("the document validates against `{rule}`"),
                // No rule at all means a schema with no rules; `check_chain` has said so.
                (true, None) => "the schema defines no rule to validate against".to_owned(),
                (false, Some(rule)) => {
                    format!("the document does not validate against `{rule}`; see the rows above")
                }
                (false, None) => "the schema defines no rule to validate against".to_owned(),
            },
        ));

        // Why a pane is not highlighted, said once rather than inferred from a grey icon. Both are
        // honest consequences of the comparison rule, and both name the command that makes the
        // comparison hold again — which is the only thing the user can do about it.
        if !qualifies {
            diagnostics.push(Diagnostic::warning(
                Source::Validation,
                0..0,
                "the hex grid holds different bytes from the ones validated, so the byte spans \
                 are withheld; `To CBOR` makes the document match",
            ));
        }
        if edn_stale {
            diagnostics.push(Diagnostic::warning(
                Source::Validation,
                0..0,
                "the EDN shown is not the canonical rendering of the bytes validated, so the EDN \
                 spans are withheld; `To EDN` re-renders it",
            ));
        }

        // The marks come from the same rows the lower window lists, so the grid and the log cannot
        // disagree about which bytes were rejected.
        let marks: Vec<Range<usize>> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.source == Source::Validation)
            .map(|diagnostic| diagnostic.span.clone())
            .filter(|span| !span.is_empty())
            .collect();

        self.trace = Some(trace);
        self.trace_diagnostics = diagnostics;
        self.rebuild_trace_rows(cx);
        // The page is selected rather than offered: a trace the user has to go looking for is a
        // trace they will not find. The `Diagnostics` page keeps its own count in its label, so
        // nothing is hidden by the switch.
        self.lower = LowerTab::Trace;

        // A new trace makes the previous row click's highlight an answer to a question nobody
        // asked: the row it came from may not even exist in this trace, and the span it marks is
        // read off the old one. So the three jump layers go with the trace they belonged to. The
        // validation layer needs no clearing — it is replaced wholesale just below.
        self.edn.update(cx, |edn, cx| edn.mark(Vec::new(), cx));
        self.cddl.update(cx, |cddl, cx| cddl.mark(None, cx));

        self.hex.update(cx, |hex, cx| {
            hex.mark(MarkKind::Jump, Vec::new(), cx);
            hex.mark(MarkKind::Validation, marks, cx);
        });
        self.publish_rule_status(cx);
        // Picks the EDN-coordinate mismatches up through [`edn_qualifies`]; harmless and idempotent
        // on the CBOR tab, where it re-derives the same parse result it already had.
        self.recheck_edn(cx);
        cx.notify();
    }

    /// The CDDL chain's root file, and the inner tabs standing in the way of reading it from disk.
    ///
    /// The gate [`Ide::on_syntax_check`] and [`Ide::on_validate`] share. Flushes the editors first,
    /// because both ask whether the tabs are clean and an editor's last keystrokes may not have
    /// reached the model yet; `None` means no CDDL file is open at all.
    ///
    /// The returned names are the **unsaved** tabs, empty when the chain may be read. The refusal is
    /// returned rather than noted here because the two callers have different things to say: by the
    /// time `Syntax Check` refuses it has already checked each file on its own and can offer that
    /// instead, while `Validate` has nothing to offer and says so. One copy of the *rule*, two
    /// sentences about it — which is the right way round.
    fn resolve_chain(&mut self, cx: &mut Context<Self>) -> Option<(PathBuf, Vec<String>)> {
        self.flush_cddl(cx);

        let workspace = self.workspace.read(cx);
        let root = workspace.cddl.tabs().first()?.meta.path.clone()?;
        // Named rather than counted: "two files are unsaved" leaves the user hunting for which.
        let unsaved = workspace
            .cddl
            .tabs()
            .iter()
            .filter(|doc| doc.meta.dirty)
            .filter_map(|doc| doc.meta.path.as_deref().map(base_name))
            .collect();
        Some((root, unsaved))
    }

    /// `UI_DESIGN.md:115`: the CDDL tab's `Syntax Check`.
    ///
    /// The action has been bound to `secondary-shift-k` and enabled in the menu since Phase 2 with
    /// no handler behind it; this is where that silence ends.
    ///
    /// Two passes, and the split is the whole design:
    ///
    /// * The **per-file** pass, [`Ide::recheck_cddl`], is re-run so the button is never a no-op
    ///   even when the chain-wide pass has to be skipped.
    /// * The **chain-wide** pass, [`cddl::check_chain`], resolves the include graph and is the only
    ///   thing that can find a cross-file error — an unknown rule, a duplicate rule, a missing
    ///   include. It reads every file **from disk**, so it runs only when every inner tab is clean.
    ///   With a dirty tab it is skipped and the unsaved files are named instead, because every
    ///   merged span would be displaced by the user's unsaved edits: the attribution would still be
    ///   internally consistent and would point at the wrong line of the right file, which is worse
    ///   than no attribution because it looks authoritative. A proper fix needs an
    ///   in-memory-overlay entry point in `tps_cddl`; `PLAN.md` records the request.
    ///
    /// The gate itself lives in [`Ide::resolve_chain`] from Phase 6, because `Validate` applies the
    /// same one and two copies of it would eventually answer differently.
    fn on_syntax_check(
        &mut self,
        _: &actions::SyntaxCheck,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !syntax_check_applies(self.tab) {
            return self.warn(
                "`Syntax Check` checks the CDDL tab's schema; select it first",
                cx,
            );
        }
        // `resolve_chain` flushes the editors before anything reads the model, which the chain pass
        // needs: it asks whether the tabs are clean, and an editor's last keystrokes may not have
        // reached the documents yet.
        let resolved = self.resolve_chain(cx);
        // Before the gate is answered, so that the button is never a no-op: the per-file pass and
        // the tree are about the text on screen and are re-derived whatever the chain pass does.
        self.recheck_cddl(cx);
        self.cddl.update(cx, |cddl, cx| cddl.rebuild_tree(cx));

        let Some((root, unsaved)) = resolved else {
            return self.warn("no CDDL file is open", cx);
        };

        if !unsaved.is_empty() {
            self.chain_diagnostics.clear();
            return self.warn(
                format!(
                    "checked each file on its own; save {} to check the include chain, \
                     which is read from disk",
                    unsaved.join(", ")
                ),
                cx,
            );
        }

        let search_paths = self.cddl_search_paths(&root);
        let checked = cddl::check_chain(&root, &search_paths, &self.settings.cddl_config());

        let mut diagnostics = checked.diagnostics;

        // The start-production warning does not come from `check_chain` — it is
        // `StartRule::note`'s, and it is the only report of a `;# start` the schema cannot satisfy.
        // `gp_eap.cddl:17` asks for `rfc8392.Claims-Set` and reaches `rfc8392` through
        // `;# include`, which merges rules *without* a module qualifier, so no such rule exists and
        // this warning appearing is the correct outcome rather than noise.
        if let Some(parsed) = checked.cddl.as_ref() {
            let override_rule = self.workspace.read(cx).cddl.start();
            let resolved = cddl::start_rule(parsed, checked.start_rule.as_deref(), override_rule);
            // Where to point depends on who asked. A name typed into the start-production box has
            // no line in any file, so that report goes to the top of the root rather than onto the
            // `;# start` line the user did not write.
            let span = match override_rule {
                Some(_) => 0..0,
                None => {
                    let text = self.workspace.read(cx).cddl.tabs()[0].text().to_owned();
                    start_directive_span(&text).unwrap_or(0..0)
                }
            };
            if let Some(note) = resolved.note(span) {
                diagnostics.push(note.in_file(root.clone()));
            }
        }

        let count = diagnostics.len();
        let files = checked.chain.files.len();
        self.chain_diagnostics = diagnostics;

        if !checked.chain.attributed && checked.cddl.is_some() {
            // Visible degradation rather than a wrong file name: `chain::partition` could not prove
            // its regions against the merged source, so no span can be rebased and the rows below
            // carry no file.
            //
            // Only when the chain *resolved*, though. A resolution failure never reaches the
            // partition at all, so `attributed` is false there for a reason the user is already
            // being told about in stronger terms — and adding this line under a "module not found"
            // error describes a second failure that did not happen.
            self.warn(
                "the include chain could not be partitioned, so these findings name no file",
                cx,
            );
        }
        if count == 0 {
            self.note(
                format!("the schema is valid across {files} file(s) in the include chain"),
                cx,
            );
        }

        self.refresh_menus(cx);
        cx.notify();
    }

    /// `UI_DESIGN.md:108`: open the include the CDDL caret sits on.
    ///
    /// Dispatched by the editor's context-menu item and by the right pane's chain rows, which is
    /// why the action carries no payload: the caret is asked about here, so there is one answer to
    /// "which directive?" rather than two that can disagree.
    fn on_open_include(
        &mut self,
        _: &actions::OpenInclude,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directive = self.cddl.read(cx).caret_directive(cx);
        match directive.and_then(|directive| directive.target) {
            Some(target) => self.open_include(target, cx),
            None => self.warn("the caret is not on a resolvable include directive", cx),
        }
    }

    /// `UI_DESIGN.md:124-125`: `Keystore → Generate Key...`.
    ///
    /// The action has existed since Phase 2 and been in the menu bar since then with **no handler
    /// at all** — selecting it did nothing, silently. This is that handler.
    fn on_generate_key(
        &mut self,
        _: &actions::GenerateKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reason = self.no_keystore();
        let Some(dir) = self
            .keystore
            .as_ref()
            .map(|store| store.dir().display().to_string())
        else {
            return self.warn(reason, cx);
        };

        // Weak, because the dialog outlives this call and the `Fn` closure is held by the dialog
        // layer: a strong handle would keep the root view alive past the window it renders in. A
        // released entity is a window that has gone away, which is a refusal rather than a panic.
        let ide = cx.entity().downgrade();
        keys::generate_key_dialog(dir, window, cx, move |alias, kind, window, cx| {
            let Some(ide) = ide.upgrade() else {
                return Err("the window has closed".to_owned());
            };
            let generated = ide.update(cx, |ide, cx| ide.generate_key(alias, kind, cx));
            if generated.is_ok() {
                // Deferred rather than opened here, for the reason [`keys::warn_randomness`]
                // records: this closure is about to return `Ok(())`, which closes "the" dialog by
                // popping the last one opened — and a warning opened first would be the one popped.
                window.defer(cx, keys::warn_randomness);
            }
            generated
        });
    }

    /// Mints a key into the store, which is all the form's OK button does.
    ///
    /// `Overwrite::Refuse`, so an alias already in use comes back as a message in the still-open
    /// dialog instead of quietly replacing someone's key material. Nothing here looks at the alias
    /// itself: [`tps_cbor_core::keystore::validate_alias`] is the one place that rule lives and the
    /// store applies it on every operation, so a refusal from it reaches the user by the same route
    /// a clash does.
    fn generate_key(
        &mut self,
        alias: &str,
        kind: NewKeyKind,
        cx: &mut Context<Self>,
    ) -> keys::Accepted {
        let reason = self.no_keystore();
        let Some(store) = self.keystore.as_mut() else {
            return Err(reason);
        };
        let created = created_stamp("generated in cbor-ide");
        // Two core functions, because a keypair and a shared key are made differently — which is
        // the whole reason [`NewKeyKind`] exists. The returned `KeyMeta` is dropped: the list is
        // re-read from the store below, so there is one answer to what is in it.
        let outcome = match kind {
            NewKeyKind::Asymmetric(kind) => store
                .generate(alias, kind, created, Overwrite::Refuse)
                .map(|_| ()),
            NewKeyKind::Symmetric(alg) => store
                .generate_symmetric(alias, alg, created, Overwrite::Refuse)
                .map(|_| ()),
        };

        match outcome {
            Ok(()) => {
                // Selected for the user, because a key is generated in order to be used and the
                // four operations are greyed out without a selection. Assigned directly rather
                // than through [`Ide::select_key`], whose no-op guard would be able to swallow the
                // list refresh: the new key has to appear in the `Keys` page whether or not its
                // alias happens to be the one already selected.
                self.selected_key = Some(alias.to_owned());
                self.sync_keys(cx);
                // A key that did not exist a moment ago may be the one that accounts for the
                // loaded COSE document, which is step 10 of the interactive check in reverse.
                self.recheck_identity(cx);
                self.refresh_menus(cx);
                self.note(format!("generated `{alias}` ({})", kind.label()), cx);
                Ok(())
            }
            // Shown in the dialog rather than the lower window, which is what keeps it open: a
            // clash or a rejected alias is correctable, and the form is where the correction is
            // typed. See [`keys::Accepted`].
            Err(error) => Err(error.to_string()),
        }
    }

    /// `Keystore → Delete Key...`, an addition to the specification recorded in `PLAN.md`.
    ///
    /// Behind the same [`Window::prompt`] the quit guard uses, because this is the one command in
    /// the application that destroys something unrecoverable.
    fn on_delete_key(
        &mut self,
        _: &actions::DeleteKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The menu item is greyed without a selection, so this is the keyboard-less equivalent of
        // a guard that should not fire — and [`Ide::report`]'s precedent is to say what happened
        // rather than do nothing visible.
        let Some(alias) = self.selected_key.clone() else {
            return self.warn("select a key in the `Keys` page first", cx);
        };

        // Hard-wrapped by [`delete_prompt_detail`], which is not a style choice; see there.
        let detail = delete_prompt_detail(&alias);
        // No destructive variant exists — `PromptButton` is `Ok`, `Cancel` or `Other`
        // (`gpui-pre/src/platform.rs:2923-2960`) — so the verb on the button is what marks it.
        let answer = window.prompt(
            PromptLevel::Warning,
            "Delete this key?",
            Some(&detail),
            &[PromptButton::ok("Delete"), PromptButton::cancel("Cancel")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            // Index 0 is `Delete`; anything else, including the dialog being dismissed, leaves the
            // store untouched.
            let Ok(0) = answer.await else {
                return;
            };
            let _ = this.update(cx, |ide, cx| ide.delete_key(&alias, cx));
        })
        .detach();
    }

    /// Removes a key from the store, after the confirmation.
    fn delete_key(&mut self, alias: &str, cx: &mut Context<Self>) {
        let reason = self.no_keystore();
        let Some(store) = self.keystore.as_mut() else {
            return self.warn(reason, cx);
        };
        let outcome = store.delete(alias).map_err(CoreError::from);
        if outcome.is_ok() {
            self.note(format!("deleted `{alias}` from the key store"), cx);
        }
        // `sync_keys` first, because it is what drops a selection the store no longer holds, and
        // both of the next two read that selection. A COSE document the deleted key accounted for
        // now reads "Unknown key", which is step 10 of the interactive check.
        self.sync_keys(cx);
        self.recheck_identity(cx);
        self.report(outcome, cx);
    }

    /// `Keystore → Import Key...`: the same destination File→Open sends a COSE key to, reached
    /// deliberately.
    ///
    /// Offered in the Keystore menu as well as through File→Open because a user who wants to add a
    /// key looks for it there, and `UI_DESIGN.md:119`'s route only works if they happen to know
    /// that opening a key file does an import.
    fn on_import_key(&mut self, _: &actions::ImportKey, _: &mut Window, cx: &mut Context<Self>) {
        // Files only, unlike [`Ide::on_open`]: a directory is not a key, and the explorer's root is
        // not what this command is for.
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |ide, window, cx| ide.import_from(path, window, cx));
            }
        })
        .detach();
    }

    /// Reads a file, insists that it is a COSE key, and offers to store it.
    ///
    /// Sniffed rather than trusted, even though the user named the file: `Import Key` names the
    /// *destination*, and a `COSE_Sign1` written into the store under an alias would be a key store
    /// entry that is not a key. It is the same recognition [`route`] uses, so a file that imports
    /// here is exactly one that would have offered an import from File→Open.
    fn import_from(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(Err(CoreError::io(&path, error)), cx),
        };
        if cose::sniff(&bytes) != Some(cose::Artefact::Key) {
            return self.warn(format!("{} does not hold a COSE key", base_name(&path)), cx);
        }
        self.offer_import(path, bytes, window, cx);
    }

    /// Raises `UI_DESIGN.md:119`'s alias prompt for a COSE key.
    ///
    /// Reached from [`Ide::open_path`] and from [`Ide::import_from`], so both routes say the same
    /// thing and write through the same call.
    fn offer_import(
        &mut self,
        path: PathBuf,
        bytes: Vec<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reason = self.no_keystore();
        let Some(dir) = self
            .keystore
            .as_ref()
            .map(|store| store.dir().display().to_string())
        else {
            return self.warn(reason, cx);
        };

        // The file's stem, which is what a key saved out of this tool or another one is named
        // after. Only a suggestion: the box is editable and `validate_alias` has the last word.
        let suggested = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ide = cx.entity().downgrade();
        // `Rc`, because the dialog's `accept` is `Fn` and runs once per press of OK — a rejected
        // alias leaves the dialog open and the bytes still needed. Moving them out would make the
        // second attempt import nothing.
        let bytes = Rc::new(bytes);
        keys::import_key_dialog(
            base_name(&path),
            dir,
            suggested,
            window,
            cx,
            move |alias, _, cx| {
                let Some(ide) = ide.upgrade() else {
                    return Err("the window has closed".to_owned());
                };
                let bytes = bytes.clone();
                ide.update(cx, |ide, cx| ide.import_key(alias, &bytes, cx))
            },
        );
    }

    /// Copies a COSE key into the store under `alias`.
    ///
    /// Through [`tps_cbor_core::keystore::KeyStore::import`] and nothing else, which is what keeps
    /// the two rules `tps_cbor_core/CLAUDE.md` names — `0o600` and `validate_alias` — on the only
    /// path by which a key gets in. `Overwrite::Refuse` because the alias was typed, and replacing
    /// someone's key material because two files share a stem is not a thing to do unasked.
    fn import_key(&mut self, alias: &str, bytes: &[u8], cx: &mut Context<Self>) -> keys::Accepted {
        let reason = self.no_keystore();
        let Some(store) = self.keystore.as_mut() else {
            return Err(reason);
        };
        store
            .import(
                alias,
                bytes,
                created_stamp("imported in cbor-ide"),
                Overwrite::Refuse,
            )
            .map_err(|error| error.to_string())?;

        // Said out loud, because whether what arrived carries private material is the difference
        // between a key that can sign and one that can only verify — and nothing else on screen
        // says so. A key that does not decode is still imported: the store took it, and
        // [`keys::KeyDetail::of`] describes such an entry from its index so it can be deleted.
        let private = tps_cose::decode_cose_key(bytes).is_ok_and(|key| keys::has_private(&key));
        let note = match private {
            true => format!("imported `{alias}` into the key store"),
            false => format!(
                "imported `{alias}`, which carries no private key: it can verify and encrypt to, \
                 but not sign"
            ),
        };

        self.selected_key = Some(alias.to_owned());
        self.sync_keys(cx);
        self.recheck_identity(cx);
        self.refresh_menus(cx);
        self.note(note, cx);
        Ok(())
    }

    fn on_sign(&mut self, _: &actions::Sign, _: &mut Window, cx: &mut Context<Self>) {
        self.run_operation(Operation::Sign, cx);
    }

    fn on_verify(&mut self, _: &actions::Verify, _: &mut Window, cx: &mut Context<Self>) {
        self.run_operation(Operation::Verify, cx);
    }

    fn on_encrypt(&mut self, _: &actions::Encrypt, _: &mut Window, cx: &mut Context<Self>) {
        self.run_operation(Operation::Encrypt, cx);
    }

    fn on_decrypt(&mut self, _: &actions::Decrypt, _: &mut Window, cx: &mut Context<Self>) {
        self.run_operation(Operation::Decrypt, cx);
    }

    /// `UI_DESIGN.md:129`'s four operations, as one function.
    ///
    /// One body rather than four, because the four differ in three lines and agree in nine: the key
    /// is loaded, decoded and checked the same way, a refusal is phrased the same way, and the
    /// result is a document in all four cases. Four copies of that would be four places for the
    /// refusal to go missing from, which is the state the old frontend was in — it answered
    /// `InvalidKey` for every unusable key and had no way to say which.
    ///
    /// The result **lands where that kind of artefact belongs**, which is the decision taken with
    /// the user for this phase: `Sign` and `Encrypt` produce a COSE object, so they fill the COSE
    /// document; `Verify` and `Decrypt` produce a bare payload, which is plain CBOR, so they fill
    /// the CBOR document. Either way the tab follows — unlike `To EDN`, which deliberately does
    /// not: a conversion leaves something to look at on the other tab, while these four are the
    /// answer the user pressed a button for.
    fn run_operation(&mut self, op: Operation, cx: &mut Context<Self>) {
        let Some(alias) = self.selected_key.clone() else {
            return self.warn(format!("select a key to {} with", op.label()), cx);
        };

        let reason = self.no_keystore();
        let Some(store) = self.keystore.as_ref() else {
            return self.warn(reason, cx);
        };
        // A statement of its own so the borrow of `self.keystore` ends before the arms below, two
        // of which need `&mut self` to report.
        let loaded = store.load(&alias);
        let key_bytes = match loaded {
            Ok(bytes) => bytes,
            Err(error) => return self.report(Err(error.into()), cx),
        };
        // Decoded here, borrowing a `Zeroizing` buffer that is dropped — and zeroed — when this
        // function returns: `ops::sign` and its three siblings take a `&CoseKey<'_>`, so nothing
        // holds decoded private material across a frame.
        let key = match tps_cose::decode_cose_key(&key_bytes) {
            Ok(key) => key,
            Err(error) => return self.report(Err(error.into()), cx),
        };

        if let Some(refusal) = refusal(op, &key) {
            return self.warn(
                format!("cannot {} with `{alias}`: {refusal}", op.label()),
                cx,
            );
        }

        // Copied out of the model rather than borrowed through it, because `ops` is called below
        // with `&mut self` still needed afterwards — and because `Verify` and `Decrypt` write back
        // into the same workspace the input came from.
        let input = {
            let workspace = self.workspace.read(cx);
            match op.reads_cose() {
                true => workspace.cose.bytes().to_vec(),
                false => workspace.cbor.bytes().to_vec(),
            }
        };
        if input.is_empty() {
            return self.warn(
                format!(
                    "the {} document is empty, so there is nothing to {}",
                    op.input_tab().label(),
                    op.label()
                ),
                cx,
            );
        }

        let produced = match op {
            Operation::Sign => cose::ops::sign(&input, &key),
            Operation::Verify => cose::ops::verify(&input, &key),
            // The algorithm is read from the picker at the moment it is needed, which is why there
            // is no `encrypt_alg: EncryptAlgorithm` field to keep in step. `A128GCM` is row 0 and
            // so the default, but it is a default the user can change rather than the hardcoded
            // choice `EncryptPanel.tsx:85-88` made.
            Operation::Encrypt => {
                let Some(alg) = self.encrypt_alg.read(cx).selected_value().copied() else {
                    return self.warn("choose an encryption algorithm first", cx);
                };
                cose::ops::encrypt(&input, &key, alg)
            }
            // No algorithm argument: a `COSE_Encrypt0` carries its own algorithm and nonce in its
            // protected header, so the picker has no say here and is not consulted.
            Operation::Decrypt => cose::ops::decrypt(&input, &key),
        };
        let bytes = match produced {
            Ok(bytes) => bytes,
            Err(error) => return self.report(Err(error), cx),
        };

        let (tab, count) = (op.result_tab(), bytes.len());
        self.workspace.update(cx, |workspace, cx| {
            // `replace` and not a new document: it keeps the path, so a signed object saved over
            // the file it came from still can be, and it sets `dirty` so the explorer and the quit
            // guard both know there is unsaved work. Permitted on a read-only document, which the
            // COSE one always is — read-only guards the *user's* typing, and this is the tool
            // replacing the document wholesale (`doc.rs:194-200`).
            match tab {
                Tab::Cbor => workspace.cbor.replace(bytes),
                _ => workspace.cose.replace(bytes),
            }
            cx.notify();
        });

        self.set_tab(tab);
        match tab {
            Tab::Cbor => {
                // A document replaced wholesale is the strongest form of the edit the honesty rule
                // is about, so the trace goes — before the recheck, which is what puts the decode
                // mark back ([`Ide::after_edit`] orders it the same way and for the same reason).
                self.clear_trace(cx);
                self.recheck_cbor(cx);
            }
            _ => self.reload_cose(cx),
        }
        self.note(
            format!(
                "{} {} bytes with `{alias}`: {count} bytes in the {} tab",
                op.done(),
                input.len(),
                tab.label()
            ),
            cx,
        );
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// `UI_DESIGN.md:64`: the right pane can be hidden.
    fn on_toggle_options(
        &mut self,
        _: &actions::ToggleOptions,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.panes.update(cx, |panes, _| panes.toggle_options());
        cx.notify();
    }

    /// `UI_DESIGN.md:51`'s Help menu, which is the one menu the specification requires always be
    /// offered and the one item in it.
    ///
    /// `open_alert_dialog` rather than the [`Ide::confirm_quit`] route, and the difference is which
    /// of the two mechanics each needs. `confirm_quit` has to *await an answer* across a spawned
    /// task, which `Window::prompt` gives and a dialog does not; this only has to appear, so the
    /// dialog's own styling and its default OK button are the whole interaction.
    ///
    /// Not `.confirm()`: that adds a Cancel button, and there is nothing here to cancel.
    ///
    /// The builder is a `Fn` closure that **may run again** whenever the dialog layer is rebuilt,
    /// so [`about_lines`] is called once, outside it, and the closure only clones. The same
    /// contract Phase 7's dialogs are written against.
    fn on_about(&mut self, _: &actions::About, window: &mut Window, cx: &mut Context<Self>) {
        let lines = about_lines();
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title(ABOUT_NAME)
                .description(v_flex().gap_0p5().children(lines.clone()))
                .ok_text("Close")
        });
    }

    /// Quitting goes through `cx.quit`, which runs the `on_app_quit` handler installed in [`main`]
    /// — so the settings are written once, by one route, however the application is closed.
    ///
    /// Unsaved work is guarded from Phase 3, now that the hex grid can produce some. A clean
    /// workspace quits immediately; a dirty one asks first, in [`Ide::confirm_quit`].
    fn on_quit(&mut self, _: &actions::Quit, window: &mut Window, cx: &mut Context<Self>) {
        // Before `dirty()` is read: an edit that has only reached the editor would otherwise let a
        // modified document quit without a question.
        self.flush_edn(cx);
        self.flush_cddl(cx);
        if self.workspace.read(cx).dirty() {
            self.confirm_quit(window, cx);
            return;
        }
        cx.quit();
    }

    /// Asks what to do about unsaved documents, and quits — or does not — accordingly.
    ///
    /// `Window::prompt` rather than gpui-kit's `AlertDialog`, which is built around a *trigger*
    /// element in the render tree; there is no element here, because the question is asked by an
    /// action and by the window's close button. The prompt is rendered in-window by GPUI's own
    /// `fallback_prompt_renderer` on platforms with no native dialog, which includes this one, so
    /// the one call works everywhere.
    ///
    /// `Save all and quit` quits **only if every save succeeded**. A document with no path cannot
    /// be written, and quitting anyway would discard exactly the work the user just asked to keep;
    /// instead the failures land in the lower window and the application stays up so `Save As` can
    /// be used. That is the whole reason `Workspace::save_all` reports per-document failures.
    fn confirm_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = unsaved_summary(self.workspace.read(cx));
        let answer = window.prompt(
            PromptLevel::Warning,
            "There are unsaved changes.",
            Some(&detail),
            &[
                PromptButton::ok("Save all and quit"),
                PromptButton::new("Discard and quit"),
                PromptButton::cancel("Cancel"),
            ],
            cx,
        );

        cx.spawn(async move |this, cx| {
            // An `Err` is the prompt being dropped without an answer — the same as cancelling.
            let Ok(choice) = answer.await else {
                return;
            };
            let _ = this.update(cx, |ide, cx| match choice {
                0 => ide.save_all_and_quit(cx),
                1 => cx.quit(),
                _ => {}
            });
        })
        .detach();
    }

    /// Saves everything dirty and quits if that left nothing behind.
    fn save_all_and_quit(&mut self, cx: &mut Context<Self>) {
        let failures = self.workspace.update(cx, |workspace, cx| {
            let failures = workspace.save_all();
            cx.notify();
            failures
        });

        if failures.is_empty() {
            cx.quit();
            return;
        }

        // Said before the failures are listed, because the headline — *you are still here* — is
        // the part that is not obvious from a row saying a file has no path.
        self.warn(
            format!(
                "quit cancelled: {} of the unsaved documents could not be written",
                failures.len()
            ),
            cx,
        );
        self.diagnostics.extend(failures);
        self.sync_explorer(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Writes the window geometry and the pane sizes back into the settings file.
    ///
    /// `size` is passed in rather than read here because this runs from the quit handler, which
    /// sees only the app and not the window; see [`main`].
    fn persist(&mut self, size: Option<(f32, f32)>, cx: &mut Context<Self>) {
        self.settings.layout = self.panes.read(cx).layout().clone();
        if let Some(size) = size {
            self.settings.layout.window = Some(size);
        }
        self.settings.workspace_root = self.workspace.read(cx).root.clone();
        // `start()` is what treats blank as unset, so a box the user cleared persists as `None`
        // rather than as an empty string that would later read back as a request for a rule named
        // "".
        self.settings.start_rule = self.workspace.read(cx).cddl.start().map(str::to_owned);

        if let Err(error) = self.settings.save_default() {
            // There is no pane left to show this in, and no user left to show it to.
            eprintln!("cbor-ide: could not save settings: {error}");
        }
    }

    /// The centre tab strip (`UI_DESIGN.md:57`).
    ///
    /// `TabBar::on_click` supersedes the children's own handlers, so the four tabs carry no
    /// handlers and the index is mapped back through `Tab::ALL` — the same array the menu's tab
    /// items are built from, so the two orders cannot disagree.
    fn tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = Tab::ALL.iter().position(|tab| *tab == self.tab);
        TabBar::new(TAB_BAR_ID)
            .underline()
            .selected_index(selected.unwrap_or(0))
            .children(Tab::ALL.map(|tab| TabItem::new().label(tab.label())))
            .on_click(cx.listener(|this, index: &usize, window, cx| {
                if let Some(tab) = Tab::ALL.get(*index) {
                    this.select(*tab, window, cx);
                }
            }))
            .into_any_element()
    }

    /// The centre upper window: the tab strip over the selected tab's view.
    fn main_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        // The strip is built first and bound, because it needs `&mut Context` while
        // [`Ide::tab_view`] holds a read borrow of the workspace through the `App`.
        let strip = self.tab_strip(cx);
        let view = self.tab_view(cx);
        v_flex()
            .size_full()
            .child(strip)
            .child(div().flex_1().overflow_hidden().child(view))
            .into_any_element()
    }

    /// The CDDL tab's own inner tab bar: one tab per file in the include chain
    /// (`UI_DESIGN.md:108`).
    ///
    /// A distinct `ElementId` from the outer strip's `"tabs"`, which is not cosmetic: two elements
    /// sharing an id makes the *outer* strip lose its selection, which looks like an unrelated bug.
    /// [`CDDL_TAB_BAR_ID`] names it so a test can assert the two differ.
    ///
    /// Selection is re-supplied every render because `TabBar` is stateless, and the handler lives
    /// on the bar rather than on the tabs because `TabBar::on_click` supersedes the children's own
    /// — the same finding [`Ide::tab_strip`] records.
    ///
    /// **No close button.** `TabBar` has no closable API; the only route is a `Button` in
    /// `Tab::suffix`, and since the bar's handler supersedes the child's, a close click would also
    /// select the tab it had just destroyed unless `Button`'s own `cx.stop_propagation()` saves it
    /// — which it does, but untested here. [`CddlDocSet::open_root`] removes the need by replacing
    /// the chain rather than accumulating it; `PLAN.md` records the route for Phase 8.
    fn cddl_tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let dirty_colour = cx.theme().danger;
        let tabs: Vec<_> = workspace
            .cddl
            .tabs()
            .iter()
            .map(|doc| {
                let label = doc
                    .meta
                    .path
                    .as_deref()
                    .map_or_else(|| doc.meta.title().to_owned(), base_name);
                TabItem::new().label(label).when(doc.meta.dirty, |tab| {
                    // A dot in the danger colour rather than a red label: the explorer's bold-red
                    // convention needs a text style the tab's label does not expose, and a marker
                    // beside the name reads the same way.
                    tab.suffix(div().text_color(dirty_colour).child("●"))
                })
            })
            .collect();
        let selected = workspace.cddl.active_index();

        TabBar::new(CDDL_TAB_BAR_ID)
            .underline()
            .selected_index(selected)
            .children(tabs)
            .on_click(cx.listener(|this, index: &usize, window, cx| {
                this.select_cddl_tab(*index, window, cx)
            }))
            .into_any_element()
    }

    /// Selects an inner CDDL tab.
    ///
    /// The editors are flushed first, because the switch itself is a moment at which text can be
    /// lost: `CddlView` holds one editor per tab and the model is only as current as the last
    /// `Change`, which undo and redo do not emit.
    ///
    /// The AST tree and the diagnostics follow the new tab (`UI_DESIGN.md:117`), while the
    /// start-production box does not — it is global across inner tabs per `:114`. Those two halves
    /// behaving differently is the specification, not an oversight.
    fn select_cddl_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_cddl(cx);
        let moved = self.workspace.update(cx, |workspace, cx| {
            let moved = workspace.cddl.select(index);
            if moved {
                cx.notify();
            }
            moved
        });
        if !moved {
            return;
        }
        // The per-file check and the tree are both about the selected file, so both are re-derived
        // — and the editor takes the keyboard, since the switch was a deliberate move to it.
        self.recheck_cddl(cx);
        self.cddl.update(cx, |cddl, cx| {
            cddl.rebuild_tree(cx);
            cddl.focus(window, cx);
        });
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Takes the user to what a diagnostic is about: its tab, its file, and its line.
    ///
    /// `UI_DESIGN.md:111` asks for clickable rows and `:112` for the file a chain error came from,
    /// and together they mean one click has to answer "where is this?" in full. That is three
    /// moves, and the order is not free:
    ///
    /// 1. **The outer tab**, first, because the CDDL inner tab bar and the editor it selects are
    ///    only on screen once the CDDL tab is.
    /// 2. **The inner tab**, second, because [`cddl_view::CddlView::reveal`] writes to whichever
    ///    editor is *active*. Revealing first would move the caret in the file the user was already
    ///    reading and then switch away from it, leaving the reveal where nobody can see it and the
    ///    previous file's caret displaced.
    /// 3. **The span**, last.
    ///
    /// [`diagnostic_target`] decides whether a row has anywhere to go at all; this assumes it said
    /// yes. The three moves themselves are [`Ide::reveal_at`], which a trace row's jump buttons use
    /// too — so the two gestures cannot disagree about the order.
    fn reveal_diagnostic(
        &mut self,
        diagnostic: &Diagnostic,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = diagnostic_target(diagnostic) else {
            return;
        };
        // `UI_DESIGN.md:77`'s highlight, for the row the user just clicked. Scrolling a validation
        // row's bytes into view without marking them leaves the user counting cells across a
        // 16-wide grid. No gate needed here: [`Ide::on_validate`] has already cleared the span of
        // any validation row the hex document cannot be shown to hold, so a row that is clickable
        // at all has passed the comparison.
        if diagnostic.source == Source::Validation {
            let span = diagnostic.span.clone();
            self.hex
                .update(cx, |hex, cx| hex.mark(MarkKind::Jump, vec![span], cx));
        }
        self.reveal_at(
            tab,
            diagnostic.file.clone(),
            diagnostic.span.start,
            window,
            cx,
        );
    }

    /// The three moves a jump is made of: the outer tab, the inner tab, then the span.
    ///
    /// Extracted from [`Ide::reveal_diagnostic`] in Phase 6 so that a trace row's `C`/`E`/`H` button
    /// and a diagnostic click are the same act. `file` is read only for [`Tab::Cddl`], which is the
    /// one tab holding a *set* of documents; the other tabs hold one each and ignore it.
    ///
    /// The one check [`diagnostic_target`] cannot make is whether the named CDDL file has a *tab* —
    /// that needs the document set — so this does it, and **opens the file when it does not**. That
    /// is not a convenience: `check_chain` resolves the whole chain from disk, so straight after a
    /// `Syntax Check` or a `Validate` almost every attributed row names a file with no tab yet, and
    /// a click that only selected the CDDL tab would answer nothing. Opening it is the same act as
    /// following an `;# include` — an append to the chain under exploration — so it goes through the
    /// same [`Ide::open_include`].
    fn reveal_at(
        &mut self,
        tab: Tab,
        file: Option<PathBuf>,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select(tab, window, cx);

        if tab == Tab::Cddl {
            // A target with no file is inert, so both callers have already refused it; this is the
            // type system catching up rather than a case.
            let Some(file) = file else {
                return;
            };
            match self.workspace.read(cx).cddl.index_of(&file) {
                Some(index) => self.select_cddl_tab(index, window, cx),
                None => {
                    self.open_include(file.clone(), cx);
                    // `open_include` reports its own failure, and a file that did not open — moved,
                    // deleted, or unreadable since the chain was resolved — leaves no editor these
                    // offsets index. Selecting the tab is still a true partial answer; moving a
                    // caret in whichever file happens to be selected would not be.
                    if self.workspace.read(cx).cddl.index_of(&file).is_none() {
                        return;
                    }
                }
            }
        }

        match tab {
            Tab::Cbor => self.hex.update(cx, |hex, cx| hex.reveal(offset, cx)),
            Tab::Edn => self.edn.update(cx, |edn, cx| edn.reveal(offset, cx)),
            Tab::Cddl => self.cddl.update(cx, |cddl, cx| cddl.reveal(offset, cx)),
            // Still unreachable, and now deliberately so rather than for want of a pane.
            // [`diagnostic_target`] answers `None` for every `Source::Cose` diagnostic, which is
            // the honest answer: the only one this phase produces is a `cbor_to_edn` failure, whose
            // offset is a position in the COSE *bytes* while the pane shows EDN rendered from them
            // — so there is no offset in that editor to move a caret to. Not an `unreachable!`,
            // because a later diagnostic that does name this tab should be a tab selection with no
            // reveal rather than a panic in a click handler.
            Tab::Cose => {}
        }
        cx.notify();
    }

    /// Highlights everything one trace row is about, in every pane that still qualifies, **without
    /// switching tabs**.
    ///
    /// The feature the tracer exists for: one click and the CDDL editor, the EDN editor and the hex
    /// grid all mark the same construct, so the correspondence between schema, bytes and diagnostic
    /// notation is visible rather than navigated to. The old frontend's `handleTraceRowClick`
    /// (`App.tsx:580-602`) established the "no tab switch" half, which is what makes the gesture
    /// worth having — switching would show one pane and hide the other two.
    ///
    /// Each layer is its own `RangeDecorationCollection` or mark kind, so none of the three can
    /// clear another's; `tps_cbor_gui/PLAN.md:521-525` and `:574-577` record that bug being
    /// introduced twice by sharing one list between two kinds.
    ///
    /// A pane that does not qualify is *cleared* rather than left alone, because the alternative is
    /// the last row's highlight sitting under this row's — two answers to one click.
    fn highlight_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.trace_rows.get(index).cloned() else {
            return;
        };
        let (cddl_target, edn_mark, hex_mark) = {
            let Some(trace) = self.trace.as_ref() else {
                return;
            };
            let workspace = self.workspace.read(cx);
            let edn_ok = edn_qualifies(trace, workspace.edn.text());
            let hex_ok = hex_qualifies(trace, workspace.cbor.bytes());
            (
                row.cddl.as_ref().and_then(|(file, span)| {
                    chain_file(trace, *file).map(|path| (path, span.clone()))
                }),
                row.edn.clone().filter(|_| edn_ok).into_iter().collect(),
                row.cbor.clone().filter(|_| hex_ok).into_iter().collect(),
            )
        };

        self.cddl.update(cx, |cddl, cx| cddl.mark(cddl_target, cx));
        self.edn.update(cx, |edn, cx| edn.mark(edn_mark, cx));
        self.hex
            .update(cx, |hex, cx| hex.mark(MarkKind::Jump, hex_mark, cx));
        cx.notify();
    }

    /// Switches to `tab` and scrolls to what a trace row says is there, on top of the row's own
    /// highlighting.
    ///
    /// The `C`/`E`/`H` buttons. [`row_jump`] decides whether the button is offered at all, so a
    /// refusal here is the row having changed under a click rather than a disabled button firing.
    fn jump_row(&mut self, index: usize, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        // The row click's work happens first and in full: `UI_DESIGN.md` wants the construct marked
        // in every pane, and arriving at one of them must not be the only thing that happened.
        self.highlight_row(index, cx);

        let target = {
            let (Some(row), Some(trace)) = (self.trace_rows.get(index), self.trace.as_ref()) else {
                return;
            };
            match tab {
                Tab::Cddl => row.cddl.as_ref().and_then(|(file, span)| {
                    chain_file(trace, *file).map(|path| (Some(path), span.start))
                }),
                Tab::Edn => row.edn.as_ref().map(|span| (None, span.start)),
                Tab::Cbor => row.cbor.as_ref().map(|span| (None, span.start)),
                Tab::Cose => None,
            }
        };
        let Some((file, offset)) = target else {
            return;
        };
        self.reveal_at(tab, file, offset, window, cx);
    }

    /// Where a diagnostic points, as `file:line:column`, for the lower window's third column.
    ///
    /// This is what makes the attribution work *visible*: a chain error rebased into
    /// `rfc8392.cddl` is only demonstrably attributed if the row says which file and which line
    /// (`UI_DESIGN.md:112`). The position is computed here, per render, rather than stored on the
    /// [`Diagnostic`] — `diag.rs`'s module docs record why: a stored line and column drift from the
    /// span as the user edits, and there would then be two answers to one question.
    ///
    /// Three shapes, because the sources do not share a coordinate space:
    ///
    /// * **CBOR and validation** index the byte buffer, where a line and column would be a
    ///   fiction. The hex offset is given in the base the grid's own gutter uses.
    /// * **EDN and CDDL** index text, so `Position::at` applies — against the right text, which for
    ///   CDDL means the inner tab the diagnostic names and not the active one.
    /// * **Everything else** gets the file name alone, or nothing. A merged-source span, an
    ///   unattributed finding, a file that is no longer open, and `0..0` all land here: a number
    ///   counted against the wrong text reads exactly like a correct one, which is the one outcome
    ///   worth avoiding.
    fn diagnostic_location(&self, diagnostic: &Diagnostic, cx: &App) -> String {
        let name = diagnostic
            .file
            .as_deref()
            .map(base_name)
            .unwrap_or_default();
        let workspace = self.workspace.read(cx);
        let locus = if diagnostic.span == (0..0) {
            None
        } else {
            match diagnostic.source {
                Source::Cbor | Source::Validation => Some(format!("{:#x}", diagnostic.span.start)),
                Source::Edn => Some(line_column(workspace.edn.text(), diagnostic.span.start)),
                Source::Cddl | Source::CddlModules => diagnostic
                    .file
                    .as_deref()
                    .and_then(|file| workspace.cddl.index_of(file))
                    .and_then(|index| workspace.cddl.tabs().get(index))
                    .map(|doc| line_column(doc.text(), diagnostic.span.start)),
                Source::Cose | Source::Settings => None,
            }
        };

        match (name.is_empty(), locus) {
            (true, locus) => locus.unwrap_or_default(),
            (false, Some(locus)) => format!("{name}:{locus}"),
            (false, None) => name,
        }
    }

    /// The selected tab's own view.
    ///
    /// All four are real as of Phase 7: the CBOR tab from Phase 3, the EDN tab from Phase 4, the
    /// CDDL tab from Phase 5 and the COSE tab now. Only the CDDL tab still has a placeholder, and
    /// for a different reason — it holds an include chain, and an empty chain has no file to show
    /// an editor of.
    fn tab_view(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.tab {
            Tab::Cbor => self.hex.clone().into_any_element(),
            Tab::Edn => self.edn.clone().into_any_element(),
            Tab::Cddl => {
                if self.workspace.read(cx).cddl.is_empty() {
                    return placeholder(
                        "(no CDDL file open)",
                        "Open a .cddl file to edit it and follow its include chain",
                        cx,
                    );
                }
                // Built before the column, for the reason [`Ide::main_pane`] records: the strip
                // needs `&mut Context` and the view below it borrows the workspace through the app.
                let strip = self.cddl_tab_strip(cx);
                v_flex()
                    .size_full()
                    .child(strip)
                    .child(div().flex_1().overflow_hidden().child(self.cddl.clone()))
                    .into_any_element()
            }
            // No empty-document placeholder, unlike the CDDL arm above: an empty COSE document
            // renders as an empty read-only editor, which is what the CBOR and EDN tabs do too, and
            // the right pane is where the user is told what to do next.
            Tab::Cose => self.cose.clone().into_any_element(),
        }
    }

    /// The centre lower window: the two pages and the strip that selects them.
    ///
    /// `UI_DESIGN.md:61` puts errors here and the banked decision puts the trace log here too; see
    /// [`LowerTab`] for the divergence that represents. The border is on this wrapper rather than on
    /// either page, so the strip sits inside the frame and the pages do not each have to know they
    /// are in one.
    fn lower_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        // The strip first and bound, for the reason [`Ide::main_pane`] records: it needs
        // `&mut Context` while the pages below hold read borrows through the `App`.
        let strip = self.lower_tab_strip(cx);
        let body = match self.lower {
            LowerTab::Diagnostics => self.diagnostics_page(cx),
            LowerTab::Trace => self.trace_page(cx),
            LowerTab::Keys => self.keys_page(cx),
        };

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(strip)
            .child(div().flex_1().overflow_hidden().child(body))
            .into_any_element()
    }

    /// The lower window's own tab strip.
    ///
    /// Every count is in the labels, so a page that is not showing still announces itself — the
    /// pages are not a mode the user has to find. `Trace` reads `visible/total`, following
    /// `TraceLog.tsx`'s header: the filter hiding 2,000 of 2,041 rows is otherwise invisible.
    ///
    /// The tabs are built by mapping [`LowerTab::pages`] — the same slice [`Ide::lower_pane`]'s
    /// match is written against — rather than from a literal array, which is what makes the strip
    /// and the bodies the same set of pages by construction. A literal array would have had to be
    /// `.when(tab == Cose)`-ed in step with `pages`, and the two going out of step is a strip entry
    /// that selects an index no page answers.
    ///
    /// A third distinct `ElementId` ([`LOWER_TAB_BAR_ID`]), and the handler on the bar rather than
    /// the tabs, for the two reasons [`Ide::cddl_tab_strip`] records.
    fn lower_tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let diagnostics = self.listed_diagnostics().count();
        let visible = self.trace_rows.len();
        let total = self
            .trace
            .as_ref()
            .map_or(0, |trace| trace.trace.events.len());
        let keys = self.key_list.len();

        let pages = LowerTab::pages(self.tab);
        // A selection outside this tab's own list draws no underline at all, so the fallback is a
        // real one rather than defensive: [`Ide::set_tab`] keeps it from happening, and if it
        // ever did the strip would still show `Diagnostics` as selected instead of nothing.
        let selected = pages
            .iter()
            .position(|page| *page == self.lower)
            .unwrap_or(0);

        TabBar::new(LOWER_TAB_BAR_ID)
            .underline()
            .selected_index(selected)
            .children(pages.iter().map(|page| {
                TabItem::new().label(match page {
                    LowerTab::Diagnostics => format!("Diagnostics ({diagnostics})"),
                    LowerTab::Trace => format!("Trace ({visible}/{total})"),
                    LowerTab::Keys => format!("Keys ({keys})"),
                })
            }))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(page) = LowerTab::pages(this.tab).get(*index) {
                    this.lower = *page;
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// Every diagnostic the lower window lists, in the order it lists them.
    ///
    /// The derived slots are shown first and the log after them, which is also the order they are
    /// useful in: what is wrong with the buffers right now, then what happened when things were
    /// asked for. The log is not re-sorted — it is a history, and `diag::sort` on every push would
    /// shuffle the user's last action out from under them.
    ///
    /// Every slot is listed whichever tab is selected, rather than only the active one's. An EDN
    /// syntax error is still a fact about the workspace while the CBOR tab is up, and hiding it
    /// would make `To CBOR` refuse with a reference to a row that was not on screen.
    ///
    /// An iterator rather than the rows themselves, because the strip's label needs the *count* and
    /// counting a second list is how the number and the page come to disagree.
    fn listed_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.well_formed
            .iter()
            .chain(self.edn_parse.iter())
            // The CDDL findings before the log for the same reason the other two slots are: these
            // are facts about the buffers right now, and the log is a history of what was asked
            // for. Per-file first, then chain-wide, so a file's own syntax errors are not buried
            // under cross-file findings that may be consequences of them.
            .chain(self.cddl_diagnostics.values().flatten())
            .chain(self.chain_diagnostics.iter())
            // A `Validate` in flight, immediately above the trace it is about to replace — so the
            // row that says a run is under way sits directly over the rows it is making stale,
            // which is the only arrangement in which the user can tell those rows are old.
            .chain(self.validating.as_ref().map(|running| &running.row))
            // The last `Validate`'s findings: a fact about the documents right now, so beside the
            // other derived slots and not in the log.
            .chain(self.trace_diagnostics.iter())
            .chain(self.diagnostics.iter())
    }

    /// The `Diagnostics` page: one clickable row per finding (`UI_DESIGN.md:111-112`).
    fn diagnostics_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .listed_diagnostics()
            .enumerate()
            .map(|(index, diagnostic)| {
                let colour = match diagnostic.severity {
                    Severity::Error => cx.theme().danger,
                    Severity::Warning => cx.theme().warning,
                    // Deliberately the muted foreground and not `info`: an informational row
                    // reports that a command worked, and it has to be the *quietest* thing in the
                    // list rather than a third colour competing with the two that mean something
                    // is wrong.
                    Severity::Info => cx.theme().muted_foreground,
                };
                let row = h_flex()
                    // Stateful because `on_click` requires it, and from the position in the list
                    // because nothing else here is unique: two files can carry the same message
                    // and one file can carry the same message twice. The list is rebuilt whole on
                    // every change, so a shifting index costs nothing — no row holds state across
                    // a rebuild.
                    .id(("diagnostic", index))
                    .gap_2()
                    .items_start()
                    .px_1()
                    .rounded_sm()
                    .child(div().w_16().flex_none().text_color(colour).child(
                        match diagnostic.severity {
                            Severity::Error => "error",
                            Severity::Warning => "warning",
                            Severity::Info => "info",
                        },
                    ))
                    .child(
                        div()
                            .w_24()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            .child(source_label(diagnostic.source)),
                    )
                    // Which file and where in it, which is the whole point of Phase 5's attribution
                    // work: a cross-file CDDL error is only actionable if it names the file it is in
                    // (`UI_DESIGN.md:112`). The base name rather than the path, because the chain's
                    // files all live in one directory and the full path would crowd out the message.
                    // Blank for a diagnostic about no particular place; see
                    // [`Ide::diagnostic_location`] for which those are and why they stay blank
                    // rather than being given a plausible-looking number.
                    .child(
                        div()
                            .w_48()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.diagnostic_location(diagnostic, cx)),
                    )
                    // `overflow_hidden` is what makes the message *wrap* rather than run off the
                    // right edge, which is the opposite of what the name suggests and so is worth
                    // the paragraph. A flex item's automatic minimum size is its min-content width,
                    // and GPUI measures text under `AvailableSpace::MinContent` with wrapping
                    // disabled (`elements/text.rs:658-664` sets `wrap_width` only for a *definite*
                    // width), so the cell's floor is the width of the whole message on one line and
                    // `flex_1` can never shrink it. Taffy replaces that floor with zero for an item
                    // that contains its contents (`compute/flexbox.rs:1122` via
                    // `Overflow::maybe_into_automatic_min_size`), after which the cell takes the
                    // width the row has left, the text is measured against a definite width, and it
                    // wraps. `Overflow::Clip` would *not* do this -- taffy excludes it deliberately.
                    // GPUI's own prompt does the same thing for the same reason
                    // (`window/prompts.rs:118`).
                    //
                    // Nothing is actually hidden: the row is `items_start`, so it grows to the
                    // wrapped height, and the page scrolls. No tooltip is needed as a backstop --
                    // the whole message is on screen.
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .child(diagnostic.message.clone()),
                    );

                // Clickable only where there is somewhere to go (`UI_DESIGN.md:111`). The hover
                // tint and the pointer are part of that same answer rather than decoration: a row
                // that does nothing when clicked must not look like one that does, and the only
                // affordance a one-line row has is how it behaves under the mouse.
                match diagnostic_target(diagnostic) {
                    Some(_) => {
                        let diagnostic = diagnostic.clone();
                        row.cursor_pointer()
                            .hover(|this| this.bg(cx.theme().accent))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.reveal_diagnostic(&diagnostic, window, cx)
                            }))
                    }
                    None => row,
                }
            })
            .collect::<Vec<_>>();

        // No border here: it is on [`Ide::lower_pane`]'s wrapper, so the strip sits inside the
        // frame rather than above it.
        v_flex()
            .id("diagnostics")
            .size_full()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .when(rows.is_empty(), |this| {
                this.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("No diagnostics."),
                )
            })
            .children(rows)
            .into_any_element()
    }

    /// The `Trace` page: the filter checkboxes over the checker's decision log.
    ///
    /// The log is a [`uniform_list`] rather than the `v_flex` the diagnostics page uses, and the
    /// difference is not stylistic: GPUI paints only visible children but lays out every one, so a
    /// 2,000-row trace — ordinary for a 196 KB document — would cost 2,000 layouts a frame. This is
    /// `hex_view.rs`'s pattern, chosen there for the same reason and over `v_virtual_list` because
    /// it takes a count.
    ///
    /// Three things are therefore computed **here and once**, never inside the closure, which is
    /// `'static` and runs per visible row per frame: the rows themselves (in
    /// [`Ide::rebuild_trace_rows`], since filtering is a pass over every event), the two honesty
    /// comparisons (each a buffer compare against a document that can be 196 KB), and the colours
    /// (`cx.theme()` is not reachable from an `&mut App`, and the mode cannot change mid-session —
    /// see `theme.rs`).
    fn trace_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let filters = self.trace_filters(cx);

        let Some(trace) = self.trace.as_ref() else {
            return v_flex()
                .size_full()
                .p_2()
                .gap_2()
                .child(filters)
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        // Rather than an empty page: "nothing here" and "run the command that
                        // fills this" look identical, and only one of them is true.
                        .child("No trace. `Validate` produces one."),
                )
                .into_any_element();
        };

        let workspace = self.workspace.read(cx);
        let hex_ok = hex_qualifies(trace, workspace.cbor.bytes());
        let edn_ok = edn_qualifies(trace, workspace.edn.text());
        let colours = TraceColours::resolve(cx);

        let rows = self.trace_rows.clone();
        let count = rows.len();
        // Dispatch through a weak handle for the reason `hex_view.rs` records: the closure outlives
        // the frame and must not keep the view alive.
        let view = cx.entity().downgrade();
        let log = uniform_list(
            "trace-rows",
            count,
            move |range: Range<usize>, _window: &mut Window, _cx: &mut App| {
                range
                    .map(|index| trace_row(index, &rows, hex_ok, edn_ok, colours, &view))
                    .collect()
            },
        )
        .track_scroll(&self.trace_scroll)
        .size_full();

        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .child(filters)
            .child(div().flex_1().overflow_hidden().child(log))
            .into_any_element()
    }

    /// The three outcome filters.
    ///
    /// `attempt` is off by default and the other two on, which is `settings::TraceFilter::default`
    /// and the old log's default: an attempt row is emitted for every rule the checker enters, so
    /// leaving it on makes the interesting rows a minority of the list.
    ///
    /// Writing straight into `self.settings` is what makes these persist: [`Ide::persist`] already
    /// writes the whole `Settings`, so there is no separate plumbing and no way for the two to
    /// disagree about what was last chosen.
    fn trace_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.settings.trace_filter;
        h_flex()
            .gap_4()
            .child(
                Checkbox::new("trace-attempt")
                    .label("attempt")
                    .checked(filter.attempt)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.settings.trace_filter.attempt = *checked;
                        this.rebuild_trace_rows(cx);
                    })),
            )
            .child(
                Checkbox::new("trace-matched")
                    .label("match")
                    .checked(filter.matched)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.settings.trace_filter.matched = *checked;
                        this.rebuild_trace_rows(cx);
                    })),
            )
            .child(
                Checkbox::new("trace-failed")
                    .label("fail")
                    .checked(filter.failed)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.settings.trace_filter.failed = *checked;
                        this.rebuild_trace_rows(cx);
                    })),
            )
            .into_any_element()
    }

    /// The `Keys` page: the key store's contents, one selectable row each (`UI_DESIGN.md:128`).
    ///
    /// The third page in the lower window, which `UI_DESIGN.md:128` does not describe — it gives
    /// the right pane both the list and the detail, and a list, five detail rows, an algorithm
    /// picker and four buttons do not fit a 280 px pane. The second divergence of this kind after
    /// Phase 6's `Trace` page, recorded in `PLAN.md` for the same reason.
    ///
    /// An eager `v_flex` in a scroll container, like [`Ide::diagnostics_page`] and **unlike**
    /// [`Ide::trace_page`], whose own documentation explains why it is a `uniform_list`: GPUI lays
    /// out every child it does not paint, so a 2,000-row trace would cost 2,000 layouts a frame. A
    /// key store is bounded by how many aliases a human has typed, and a `uniform_list` would cost
    /// a scroll handle, a fixed row height and a `'static` closure to save nothing.
    fn keys_page(&self, cx: &mut Context<Self>) -> AnyElement {
        // The reason in place of the list and not beside it: a "No keys." line over a store that
        // could not be opened is a lie, and this page is where a user comes to find out where
        // their keys are. [`Ide::keystore_error`] is held rather than logged for exactly this.
        if let Some(reason) = &self.keystore_error {
            return v_flex()
                .id("keys")
                .size_full()
                .p_2()
                .child(muted_note(
                    format!("The key store could not be opened: {reason}"),
                    cx,
                ))
                .into_any_element();
        }

        let selected = self.selected_key.as_deref();
        let rows = self
            .key_list
            .iter()
            .enumerate()
            .map(|(index, meta)| {
                let chosen = selected == Some(meta.alias.as_str());
                let alias = meta.alias.clone();
                h_flex()
                    // From the position in the list, for the reason [`Ide::diagnostics_page`]
                    // records: the list is rebuilt whole on every change, so no row holds state
                    // across a rebuild and a shifting index costs nothing. The *selection* is an
                    // alias and not an index, which is the part that has to survive a rebuild.
                    .id(("key", index))
                    .gap_2()
                    .items_start()
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    // Both, and in this order: the selected row keeps its tint under the pointer,
                    // and an unselected one takes it only while hovered.
                    .when(chosen, |this| this.bg(cx.theme().accent))
                    .hover(|this| this.bg(cx.theme().accent))
                    .child(
                        div()
                            .w_4()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            // A mark rather than colour alone, so the selection survives a theme
                            // whose accent is close to its background.
                            .child(if chosen { "●" } else { "" }),
                    )
                    .child(div().w_48().flex_none().child(meta.alias.clone()))
                    .child(
                        div()
                            .w_48()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            .child(keys::describe_kind(meta)),
                    )
                    // Whether it can sign, which is the one thing about a stored key that changes
                    // what the four buttons can do with it.
                    .child(
                        div()
                            .w_24()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            .child(match meta.has_private {
                                true => "private",
                                false => "public only",
                            }),
                    )
                    // The free-text provenance [`created_stamp`] wrote. Last and elastic, because
                    // it is the longest field and the least often read.
                    .child(
                        div()
                            .flex_1()
                            .text_color(cx.theme().muted_foreground)
                            .child(meta.created_at.clone()),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_key(Some(alias.clone()), cx)),
                    )
            })
            .collect::<Vec<_>>();

        // Names the directory, because "no keys" and "looking in the wrong place" are the same
        // sight and only one of them is a thing to act on.
        let empty = match self.keystore.as_ref() {
            Some(store) => format!(
                "No keys in {}. Use `Keystore → Generate Key...` or `Import Key...`.",
                store.dir().display()
            ),
            // Unreachable: a store that failed to open was answered above.
            None => "No key store.".to_owned(),
        };

        v_flex()
            .id("keys")
            .size_full()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .when(rows.is_empty(), |this| this.child(muted_note(empty, cx)))
            .children(rows)
            .into_any_element()
    }

    /// The right pane: settings for the selected tab (`UI_DESIGN.md:64`).
    fn options_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.tab {
            Tab::Cbor => self.cbor_options(cx),
            Tab::Edn => self.edn_options(cx),
            Tab::Cddl => self.cddl_options(cx),
            Tab::Cose => self.cose_options(cx),
        };

        v_flex()
            .size_full()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .p_3()
            .gap_2()
            .child(div().font_weight(FontWeight::BOLD).child(self.tab.label()))
            .child(body)
            .into_any_element()
    }

    /// The CBOR tab's right pane: `UI_DESIGN.md:79-84`.
    ///
    /// Both buttons dispatch their [`actions`] rather than calling the handler, which is the whole
    /// point of an action existing: the button, the menu item and the keybinding are then the same
    /// command, and `UI_DESIGN.md:54`'s shortcuts cannot drift from what the pane offers. They are
    /// disabled on an empty buffer because there is nothing to convert or check, and a button that
    /// only ever reports "nothing to do" is noise.
    ///
    /// `Validate` is additionally `loading` while a trace runs, and gains a `Spinner` in its icon
    /// slot for exactly the same duration. The spinner is not redundant with the flag. The plan for
    /// this work said `loading(true)` "renders its own spinner"; it does, but only for a button that
    /// already has an icon, and neither of these had one. `gpui-component` puts the spinner in the
    /// icon slot (`button/button_icon.rs:118-126`) and otherwise only fades the button by
    /// `opacity(0.8)`, so without the icon a running trace was invisible here. The flag still earns
    /// its place: it swallows the click and stops propagation, so the button needs no separate
    /// guard. The keyboard path does, and has one in [`Ide::on_validate`].
    fn cbor_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = !self.has_content(cx);
        let read_only = self.workspace.read(cx).cbor.meta.read_only;

        v_flex()
            .gap_2()
            .child(
                Button::new("to-edn")
                    .label("To EDN")
                    .outline()
                    .w_full()
                    .disabled(empty)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::ToEdn), cx);
                    }),
            )
            .child(
                Button::new("validate")
                    .label("Validate")
                    .outline()
                    .w_full()
                    .disabled(empty)
                    .loading(self.validating.is_some())
                    // Attached only while running — see this pane's doc comment for why `loading`
                    // cannot show it on its own, and why the idle button keeps no icon.
                    .when(self.validating.is_some(), |button| {
                        button.icon(Spinner::new())
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::Validate), cx);
                    }),
            )
            .child(
                Checkbox::new("read-only")
                    .label("Read-only")
                    .checked(read_only)
                    .on_click(
                        cx.listener(|this, checked: &bool, _, cx| this.set_read_only(*checked, cx)),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    // Read-only is the default (`UI_DESIGN.md:83`), so the grid ignoring keystrokes
                    // is the expected state and needs saying once, here, where it is changed.
                    .child("Clear this to type hex digits or ASCII into the grid."),
            )
            .into_any_element()
    }

    /// The EDN tab's right pane: `UI_DESIGN.md:95-99`.
    ///
    /// Modelled on [`Ide::cbor_options`], including dispatching the actions rather than calling the
    /// handlers, for the reason recorded there.
    fn edn_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = !self.has_content(cx);
        let read_only = self.workspace.read(cx).edn.meta.read_only;

        v_flex()
            .gap_2()
            .child(
                Button::new("to-cbor")
                    .label("To CBOR")
                    .outline()
                    .w_full()
                    .disabled(empty)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::ToCbor), cx);
                    }),
            )
            .child(
                Button::new("validate")
                    .label("Validate")
                    .outline()
                    .w_full()
                    .disabled(empty)
                    .loading(self.validating.is_some())
                    // Attached only while running — see this pane's doc comment for why `loading`
                    // cannot show it on its own, and why the idle button keeps no icon.
                    .when(self.validating.is_some(), |button| {
                        button.icon(Spinner::new())
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::Validate), cx);
                    }),
            )
            .child(
                Checkbox::new("read-only")
                    .label("Read-only")
                    .checked(read_only)
                    .on_click(
                        cx.listener(|this, checked: &bool, _, cx| this.set_read_only(*checked, cx)),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    // Read-only is the default (`UI_DESIGN.md:99`), so the editor ignoring
                    // keystrokes is the expected state and needs saying once, here, where it is
                    // changed. The second line is the only place the context menu is advertised.
                    .child(
                        "Clear this to edit the EDN. Right-click a selection to decode it as CBOR.",
                    ),
            )
            .into_any_element()
    }

    /// The CDDL tab's right pane: `UI_DESIGN.md:113-117`.
    ///
    /// Modelled on [`Ide::cbor_options`], including dispatching the actions rather than calling the
    /// handlers, for the reason recorded there. Five controls, and which of them follow the inner
    /// tab selection is specified rather than chosen:
    ///
    /// * the start production is **global** across inner tabs (`:114`);
    /// * read-only is **per inner tab** (`:116`);
    /// * the chain list and the AST tree are about the selected tab (`:117`).
    fn cddl_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = !self.has_content(cx);
        let read_only = self
            .workspace
            .read(cx)
            .cddl
            .active()
            .is_some_and(|doc| doc.meta.read_only);
        let stale = self.cddl.read(cx).tree_is_stale(cx);
        let includes = self.cddl.read(cx).directives(cx);
        let tree = CddlView::ast_tree(&self.cddl, cx);

        // Clickable rows as well as the editor's context menu, for the reason `cddl_view`'s
        // `context_menu` records: a gesture reachable only by right-clicking the correct line is
        // not discoverable. An unresolvable directive is listed greyed rather than hidden, so a
        // typo in a module name is visible where the includes are.
        let include_rows = includes
            .iter()
            .map(|directive| {
                let label = SharedString::from(directive.module.clone());
                match directive.target.clone() {
                    // Wrapped in an `h_flex` rather than given `w_full`, which is what the
                    // buttons above it use: a `Button` centres its own label, so a full-width one
                    // puts each module name in the middle of the pane and a list of them reads as
                    // a ragged column. The wrapper takes the width and the button takes its
                    // content, so the names line up on the left with the "(not found)" rows below.
                    Some(target) => h_flex()
                        .child(
                            Button::new(SharedString::from(format!(
                                "include-{}",
                                directive.span.start
                            )))
                            .label(label)
                            // Reads as a link because that is what it is: following an include is
                            // navigation, not a command.
                            .link()
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.open_include(target.clone(), cx),
                            )),
                        )
                        .into_any_element(),
                    None => div()
                        .px_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{label} (not found)"))
                        .into_any_element(),
                }
            })
            .collect::<Vec<_>>();

        v_flex()
            .gap_2()
            // `flex_1`, where the sibling panes are content-sized, because this one ends in the
            // AST tree and `gpui_kit`'s `tree()` is a `uniform_list` (`tree.rs:451`): it renders
            // *nothing at all* unless an ancestor gives it a bounded height. Without this the
            // pane's own column sizes to its content, the tree's `flex_1` resolves against zero
            // leftover space, and the AST section comes up empty with no error anywhere — which is
            // exactly how it behaved until this line.
            .flex_1()
            .min_h(px(0.))
            .child(div().text_color(cx.theme().muted_foreground).child(
                // Said here because it is the one control on this tab that does *not* follow the
                // inner tab selection, which is surprising unless stated.
                "Start production (all files)",
            ))
            .child(Input::new(&self.start_rule_input).cleanable(true))
            .child(
                Button::new("syntax-check")
                    .label("Syntax Check")
                    .outline()
                    .w_full()
                    .disabled(empty)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::SyntaxCheck), cx);
                    }),
            )
            .child(
                Checkbox::new("read-only")
                    .label("Read-only")
                    .checked(read_only)
                    .on_click(
                        cx.listener(|this, checked: &bool, _, cx| this.set_read_only(*checked, cx)),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    // Read-only is the default (`UI_DESIGN.md:116`), so the editor ignoring
                    // keystrokes is the expected state and needs saying once, here, where it is
                    // changed. The second line is the only place the context menu is advertised.
                    .child("Clear this to edit this file. Right-click an include to open it."),
            )
            .when(!include_rows.is_empty(), |this| {
                this.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Includes"),
                )
                .child(v_flex().gap_1().children(include_rows))
            })
            // The CDDL dialect, which `UI_DESIGN.md:64` gives the right pane by asking it for
            // "configurable settings related to the selected tab" — and these three are the only
            // settings that are. `keystore_dir`, `workspace_root`, `layout` and `trace_filter`
            // stay file-only; `PLAN.md` records that as the scope of the settings UI.
            //
            // Both flags default **on**, as `Settings` does (`settings.rs:79-95`), and for the
            // reason recorded there: every schema in `examples/` uses at least one RFC 9165
            // operator, so a strict-by-default IDE would greet a new user with warnings on the
            // fixtures it ships with.
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Dialect"),
            )
            .child(
                Checkbox::new("rfc9165")
                    .label("RFC 9165 control operators")
                    .checked(self.settings.rfc9165)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        this.settings.rfc9165 = *checked;
                        this.apply_cddl_settings(window, cx);
                    })),
            )
            .child(
                Checkbox::new("rfc9682")
                    // What each flag actually gates, in `tps_cddl`'s own words
                    // (`tps_cddl/src/lib.rs:15-16`): 9165 is the *additional control operators*
                    // — `.plus`, `.cat`, `.feature` and the rest — while 9682 is the *grammar*
                    // extensions, empty models and non-literal tags. Naming the RFC alone would
                    // leave a user to look up which is which; naming only the effect would lose
                    // the number they need to read about it.
                    .label("RFC 9682 grammar extensions")
                    .checked(self.settings.rfc9682)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        this.settings.rfc9682 = *checked;
                        this.apply_cddl_settings(window, cx);
                    })),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Module search paths"),
            )
            .child(Input::new(&self.search_paths_input).cleanable(true))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    // Said because the two halves are not guessable from an empty box. The file's
                    // own directory is searched whatever this holds ([`Ide::cddl_search_paths`]
                    // prepends it), and the re-resolve waits for the edit to be finished rather
                    // than following each keystroke — which is visible as nothing happening until
                    // Enter, and would otherwise read as the box not working.
                    .child("The file's own directory is always searched. Press Enter to apply."),
            )
            .child(div().text_color(cx.theme().muted_foreground).child("AST"))
            .when(stale, |this| {
                // Said rather than shown: a tree built from text the user has since changed has
                // every span in the wrong place, and a quietly wrong tree that still responds to
                // clicks is worse than an acknowledged stale one. `cddl_view`'s module docs give
                // the three reasons the tree is not rebuilt per keystroke.
                this.child(
                    div()
                        .text_color(cx.theme().warning)
                        .child("stale — re-run Syntax Check"),
                )
            })
            .child(div().flex_1().overflow_hidden().child(tree))
            .into_any_element()
    }

    /// The COSE tab's right pane: key detail and the four operations (`UI_DESIGN.md:128-130`).
    ///
    /// The list that `:128` also puts here is a page in the lower window instead; see
    /// [`Ide::keys_page`] for why, and `PLAN.md` for the record of it.
    fn cose_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let ops = menus::operations(self.menu_state(cx));
        let unknown = self.key_identity == cose::KeyIdentity::Unknown;

        v_flex()
            .gap_2()
            // What made the *loaded object*, which is a different question from which key is
            // selected and is answered first because it is the one the user cannot work out for
            // themselves. Nothing at all for `NotCose`: an empty document, or ordinary CBOR shown
            // on this tab, has no key behind it, and a line saying so on every empty workspace
            // would be noise.
            .when_some(
                match &self.key_identity {
                    cose::KeyIdentity::Known { alias, by } => Some(match by {
                        // The weaker claim of the two, and said as such: a `kid` is an assertion
                        // the object carries about itself, not a proof.
                        cose::Evidence::Kid => format!("This object names `{alias}` as its key."),
                        // The stronger one. An object a key verifies, or one it opens, *was* made
                        // by that key.
                        cose::Evidence::TrialVerification => {
                            format!("This object was made by `{alias}`.")
                        }
                    }),
                    _ => None,
                },
                |this, line| this.child(div().child(line)),
            )
            // `UI_DESIGN.md:129`'s "Unknown key", in place of the detail rows rather than beside
            // them: showing the *selected* key's detail under a heading about the loaded object
            // would attribute that object to a key that did not make it.
            .child(if unknown {
                muted_note(
                    "Unknown key: no key in the store accounts for this COSE object.",
                    cx,
                )
            } else {
                match &self.key_detail {
                    Some(detail) => self.key_rows(detail),
                    None => muted_note("Select a key in the `Keys` page below.", cx),
                }
            })
            // A muted section label, like `cddl_options`' "Includes" and "AST", above a border
            // that separates the loaded object's key from what can be done with it: the rows above
            // describe an artefact and a stored key, and everything below is a command.
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .pt_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("Encryption algorithm"),
            )
            // Only `Encrypt` takes an algorithm — a `COSE_Encrypt0` carries its own in its
            // protected header, and signing uses the one the key names — so the picker sits above
            // the four buttons rather than beside one of them. It is deliberately *not* disabled
            // with them: choosing an algorithm before selecting a key is a reasonable order to
            // work in.
            .child(
                // No `menu_width`: the default `Length::Auto` *is* the trigger's width
                // (`select.rs:642-645`), and the trigger fills the pane, so the menu does too.
                // `relative(1.)` is the trap — it looks like "as wide as the pane" and is not,
                // because the menu is `deferred` into the window overlay and the fraction resolves
                // against that. See the matching comment in [`keys::generate_key_dialog`], where
                // it truncated the key-type labels.
                Select::new(&self.encrypt_alg).id("encrypt-alg"),
            )
            // All four dispatch their [`actions`] rather than calling the handlers, for the reason
            // [`Ide::cbor_options`] records, and all four are greyed from [`menus::operations`] —
            // the same call the menu bar greys its items by, so the pane and the menu cannot
            // disagree about whether a command would do anything.
            //
            // Two rows of two rather than four full-width buttons: the pane already ends in a
            // scrolling key list on every other tab, and `Sign`/`Verify` and `Encrypt`/`Decrypt`
            // are inverse pairs, which a row of two says and a column of four does not.
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("sign")
                            .label("Sign")
                            .outline()
                            .flex_1()
                            .disabled(!ops.sign)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(actions::Sign), cx);
                            }),
                    )
                    .child(
                        Button::new("verify")
                            .label("Verify")
                            .outline()
                            .flex_1()
                            .disabled(!ops.verify)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(actions::Verify), cx);
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("encrypt")
                            .label("Encrypt")
                            .outline()
                            .flex_1()
                            .disabled(!ops.encrypt)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(actions::Encrypt), cx);
                            }),
                    )
                    .child(
                        Button::new("decrypt")
                            .label("Decrypt")
                            .outline()
                            .flex_1()
                            .disabled(!ops.decrypt)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(actions::Decrypt), cx);
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The selected key's detail rows (`UI_DESIGN.md:128-130`).
    ///
    /// A [`DescriptionList`] so the masked box sits in the *same* label/value grid as the plain
    /// rows: its `DescriptionText::AnyElement` variant takes an element as a value
    /// (`gpui-component/src/description_list.rs:49`), which is the one thing a hand-built
    /// `h_flex` grid could not have done without hard-coding the label column twice.
    ///
    /// One column and unbordered, which is not the default: the list is built for a wide form
    /// (three columns, a 120 px label, a border) and this is a 280 px pane.
    ///
    /// The rows a key does not have are **absent rather than empty**. A `Curve` row reading `—`
    /// beside a symmetric key says nothing a missing row does not, and `UI_DESIGN.md:130`'s masked
    /// box in particular must not appear for a public-only key: a box with nothing behind it
    /// invites a click that reveals nothing, which reads as the eye button being broken.
    fn key_rows(&self, detail: &KeyDetail) -> AnyElement {
        let mut list = DescriptionList::new()
            .columns(1)
            .label_width(px(64.))
            .bordered(false)
            .item("Alias", detail.alias.clone(), 1)
            .item("Type", detail.key_type.clone(), 1);
        if let Some(curve) = &detail.curve {
            list = list.item("Curve", curve.clone(), 1);
        }
        if let Some(algorithm) = &detail.algorithm {
            list = list.item("Alg", algorithm.clone(), 1);
        }
        if let Some(key_id) = &detail.key_id {
            list = list.item("Key id", key_id.clone(), 1);
        }
        if detail.private.is_some() {
            // `mask_toggle` is what supplies the eye `UI_DESIGN.md:130` asks for; the value and the
            // masked state are pushed into the box at paint, in [`Render::render`]. The label is
            // the key's own field name — `d` for a keypair's private scalar, `k` for a shared key —
            // because that is what the COSE registry calls it and what the user will see if they
            // ever read the file.
            let label = match detail.key_type.as_str() {
                "Symmetric" => "k",
                _ => "d",
            };
            list = list.item(
                label,
                Input::new(&self.private_input)
                    .mask_toggle()
                    .into_any_element(),
                1,
            );
        }
        // Wrapped in a `flex_none` box rather than returned directly, which is not cosmetic.
        // `DescriptionList` renders itself inside an `overflow_hidden` column
        // (`description_list.rs:284`) and asks for no height of its own, so in the options pane's
        // flex column it was given two rows' worth and **clipped the rest** — a P-256 key showed
        // `Alias` and `Type` and silently dropped `Curve`, `Alg` and the masked private box. There
        // is no overflow indication, so the pane looked like a key with no curve rather than a
        // layout fault. `flex_none` makes the list keep its own content height.
        div().flex_none().child(list).into_any_element()
    }

    /// Flips the selected tab's document between read-only and editable (`UI_DESIGN.md:83`, `:99`).
    ///
    /// The flag is checked by the view *and* enforced by the document's setters; this only moves it.
    /// `Paste` is menu-gated on the same flag, which is why the menus are rebuilt.
    ///
    /// Tab-aware rather than hardcoded to the CBOR document, which is what Phase 3 left it as when
    /// the CBOR tab was the only one with a checkbox: with two panes wired to the same listener, a
    /// fixed target would have let the EDN tab's checkbox silently lock the hex grid. The match is
    /// exhaustive so the remaining two cannot be forgotten when their panes arrive.
    fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        let tab = self.tab;
        self.workspace.update(cx, |workspace, cx| {
            match tab {
                Tab::Cbor => workspace.cbor.meta.read_only = read_only,
                Tab::Edn => workspace.edn.meta.read_only = read_only,
                Tab::Cose => workspace.cose.meta.read_only = read_only,
                // The CDDL tab holds a whole include chain, so the flag belongs to the file on
                // screen. With no file open there is nothing to flip.
                Tab::Cddl => {
                    if let Some(doc) = workspace.cddl.active_mut() {
                        doc.meta.read_only = read_only;
                    }
                }
            }
            cx.notify();
        });
        self.refresh_menus(cx);
        cx.notify();
    }
}

impl Render for Ide {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The one model-to-editor write this view does itself, and it has to happen here: both
        // `InputState::set_value` and `set_masked` need a `&mut Window`, and every caller of
        // [`Ide::sync_keys`] — an open, a dialog's OK, a click in the key list — has none. The
        // deferral `edn_view`'s module documentation sets out, with the flag taken rather than
        // cleared so a paint that does nothing costs one `bool`.
        //
        // Re-masked on every selection change, not only on the first: `UI_DESIGN.md:130` asks for a
        // reveal, which makes hidden the resting state. Without this, revealing one key's private
        // value would silently reveal the next one the user clicked.
        if std::mem::take(&mut self.key_pending) {
            let private = self
                .key_detail
                .as_ref()
                .and_then(|detail| detail.private.clone())
                .unwrap_or_default();
            self.private_input.update(cx, |input, cx| {
                input.set_value(private, window, cx);
                input.set_masked(keys::MASKED, window, cx);
            });
        }

        let content = layout::Content {
            explorer: self.explorer.clone().into_any_element(),
            main: self.main_pane(cx),
            diagnostics: self.lower_pane(cx),
            options: self.options_pane(cx),
        };
        let body = layout::panes(&self.panes, content, window, cx);

        #[allow(unused_mut)]
        let mut root = v_flex()
            .size_full()
            .key_context(actions::CONTEXT)
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            // Every action registered below is also listed in `handled_actions` in this file's
            // test module, and every action in a menu that is *not* registered is listed in
            // `unhandled_actions` with the reason. Adding or removing a line here means editing
            // one of those two lists; `every_menu_item_is_handled_or_deliberately_not` is what
            // makes forgetting visible rather than silent.
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_quit))
            .on_action(cx.listener(Self::on_toggle_options))
            .on_action(cx.listener(Self::on_to_edn))
            .on_action(cx.listener(Self::on_to_cbor))
            .on_action(cx.listener(Self::on_decode_selection))
            .on_action(cx.listener(Self::on_validate))
            .on_action(cx.listener(Self::on_syntax_check))
            .on_action(cx.listener(Self::on_open_include))
            // The Edit menu and the Help menu, live from Phase 8. All seven were rendered,
            // enabled, and handled by nothing from Phase 2 until now — the state
            // `unhandled_actions` recorded, and the one `no_menu_action_is_unhandled` now forbids.
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_replace))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_previous))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_about))
            // The seven Phase 7 commands. `GenerateKey` was in the menu bar from Phase 2 with
            // nothing listening for it, which is what `the_keystore_items_have_handlers` is
            // against: an action registered in a menu and in no `on_action` is a command that
            // silently does nothing.
            .on_action(cx.listener(Self::on_generate_key))
            .on_action(cx.listener(Self::on_delete_key))
            .on_action(cx.listener(Self::on_import_key))
            .on_action(cx.listener(Self::on_sign))
            .on_action(cx.listener(Self::on_verify))
            .on_action(cx.listener(Self::on_encrypt))
            .on_action(cx.listener(Self::on_decrypt))
            .on_action(cx.listener(|this, _: &actions::SelectCborTab, window, cx| {
                this.select(Tab::Cbor, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectEdnTab, window, cx| {
                this.select(Tab::Edn, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectCddlTab, window, cx| {
                this.select(Tab::Cddl, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectCoseTab, window, cx| {
                this.select(Tab::Cose, window, cx)
            }));

        // `AppMenuBar` renders `div().size_full()` (`gpui-component/src/menu/app_menu_bar.rs:125`),
        // so it has no height of its own: dropped straight into a flex column it takes half the
        // window and pushes the workbench into the lower half. The bar's height belongs to
        // whatever hosts it, which is here.
        #[cfg(not(target_os = "macos"))]
        {
            root = root.child(
                div()
                    .h(px(MENU_BAR_HEIGHT))
                    .flex_none()
                    .w_full()
                    .px_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().title_bar)
                    .child(self.menu_bar.clone()),
            );
        }

        // `flex_1` rather than letting the group size itself: it makes the workbench exactly the
        // space the menu bar left, whatever that is, and does not depend on the resizable group's
        // own flex settings.
        root.child(div().flex_1().w_full().child(body))
    }
}

/// Which tabs [`Ide::on_syntax_check`] acts on.
///
/// A function rather than an inline comparison so that a test can read the handler's rule and
/// compare it against the one [`menus`] greys the menu item by. Those two disagreeing is exactly the
/// state this tab was in from Phase 2 until Phase 5: the item was enabled, the shortcut was bound,
/// and nothing was listening.
fn syntax_check_applies(tab: Tab) -> bool {
    tab == Tab::Cddl
}

/// The platform's path-*list* separator: `;` on Windows, `:` everywhere else.
///
/// Spelled out because `std` exposes no constant for it — `std::path::MAIN_SEPARATOR` is the
/// *directory* separator, a different character for a different job — and the one thing that has to
/// agree with this is `std::env::split_paths`, which [`parse_search_paths`] parses with.
const PATH_LIST_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// Splits the module-search-path box's text into directories.
///
/// `std::env::split_paths` rather than a hand-rolled split, so the box behaves exactly as `$PATH`
/// does on the platform it is running on — which on Windows includes its quoting rules, and is a
/// thing to borrow rather than reimplement. Empty entries are dropped: `split_paths("")` yields one
/// empty path, and a trailing separator yields another, neither of which is a directory anyone
/// meant to name.
fn parse_search_paths(text: &str) -> Vec<PathBuf> {
    std::env::split_paths(text)
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// Joins directories back into the box's text, the inverse of [`parse_search_paths`].
///
/// Not `std::env::join_paths`, which *errors* when a path contains the separator rather than
/// producing anything — and the box has to be able to show whatever is in the settings file, since a
/// field it renders as blank is a field the next quit would silently erase. Such a path does not
/// round-trip: it comes back as two. That is inherent to a one-line separated list and is the same
/// bargain `$PATH` itself makes, so it is recorded here rather than guarded against.
///
/// `display()` is likewise lossy for a path that is not UTF-8. Settings arrive from TOML, which is
/// UTF-8 by definition, so the only way to get one is to have typed it here — where it was a
/// `String` first.
fn format_search_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(&PATH_LIST_SEPARATOR.to_string())
}

/// Which tabs [`Ide::on_validate`] acts on.
///
/// The companion to [`syntax_check_applies`], free for the same reason: a test reads this and
/// compares it against the rule `menus.rs:116-117` greys the item by. Both CBOR tabs, because
/// `UI_DESIGN.md:82-83` and `:97` both offer `Validate` — the EDN tab's checks the CBOR the
/// displayed EDN encodes to, which is a different buffer but the same command.
fn validate_applies(tab: Tab) -> bool {
    matches!(tab, Tab::Cbor | Tab::Edn)
}

/// Where a file opened by name goes, and what to do with it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// `.edn`: the EDN tab's text document.
    Edn,
    /// `.cddl`: the CDDL tab, as the root of a new include chain.
    Cddl,
    /// A `COSE_Sign1` or a `COSE_Encrypt0`: the COSE document.
    Cose,
    /// A `COSE_Key`: offer to import it under an alias. Deliberately **not** a document — a private
    /// key in a [`CborDoc`] would be a plain `Vec<u8>`, and `tps_cbor_core/CLAUDE.md` forbids a
    /// path that takes private material out of `Zeroizing`.
    ImportKey,
    /// Anything else, including ordinary CBOR: the hex grid, exactly as before.
    Cbor,
}

/// Which route a file takes: by extension for the two text documents, by content for the rest.
///
/// A free function so the table is testable without a window, which is how everything in this
/// crate is tested. `bytes` is read only on the content path, so the two text routes can be
/// answered for a file that has not been read at all — which is what [`Ide::open_path`] relies on
/// when it hands them an empty slice.
///
/// `extension` is expected already lowercased, as `open_path` leaves it; nothing here folds case a
/// second time.
///
/// The content path is where `open_path`'s Phase 2 comment pointed — *"COSE artifacts are `.cbor`
/// too, and telling them apart means decoding them, which is Phase 7's job"*. `.cbor` is the
/// documented default (`UI_DESIGN.md:68`) and carries all three COSE artefacts as well as ordinary
/// CBOR, so the name cannot decide this and [`cose::sniff`] does. `None` is both the common case
/// and the one that must not be got wrong: a false positive silently routes a user's document to a
/// tab they did not ask for, which is what `route_sends_ordinary_cbor_to_the_hex_tab` is against.
fn route(extension: &str, bytes: &[u8]) -> Route {
    match extension {
        "edn" => Route::Edn,
        "cddl" => Route::Cddl,
        _ => match cose::sniff(bytes) {
            Some(cose::Artefact::Key) => Route::ImportKey,
            Some(cose::Artefact::Sign1 | cose::Artefact::Encrypt0) => Route::Cose,
            None => Route::Cbor,
        },
    }
}

/// Which lower-window page is showing after a move from `from` to `to`, given `page` now.
///
/// Two rules, and both are about [`LowerTab::pages`] being per-tab:
///
/// * **Arriving at the COSE tab selects `Keys`.** The page is selected rather than merely offered,
///   for the reason `Validate` selects `Trace` ([`Ide::on_validate`]): a pane the user has to go
///   looking for is a pane they will not find, and the keystore is the COSE tab's whole point.
/// * **Leaving it falls back to `Diagnostics`**, because no other tab has a `Keys` page and a strip
///   whose selection is not in its own list draws no underline at all.
///
/// *Arriving* is the operative word, and the reason `from` is a parameter rather than something to
/// be assumed: `Sign` and `Encrypt` land their result on the tab the user is already on, and
/// pulling the lower window to `Keys` there would hide the very line [`Ide::run_operation`] is
/// about to write about what it did. So a page chosen while on a tab stands until the user leaves
/// it and comes back.
///
/// The answer is always in `pages(to)`, which is the property that matters and the one
/// `the_lower_window_never_shows_a_page_the_tab_does_not_offer` asserts over every combination.
fn page_after(from: Tab, to: Tab, page: LowerTab) -> LowerTab {
    match to {
        Tab::Cose if from != to => LowerTab::Keys,
        Tab::Cose => page,
        _ if page == LowerTab::Keys => LowerTab::Diagnostics,
        _ => page,
    }
}

/// A `created_at` stamp for a key store entry: what wrote it, and when.
///
/// [`KeyMeta::created_at`](tps_cbor_core::keystore::KeyMeta::created_at) is free text and nothing
/// parses it back — `tps_cbor_tui/src/app.rs:902` writes `"imported in cbor-tui"` and stops there —
/// so this is **provenance**, not a timestamp. Deliberately not a formatted date: an RFC 3339
/// spelling would mean taking on `chrono` or `time` for a string only a human reads. Unix seconds
/// beside the provenance cost nothing, are orderable, and are unambiguous about their timezone.
///
/// A clock that predates 1970 gives the provenance alone rather than a negative number: a nonsense
/// stamp is worse than no stamp, and this is the one case where `duration_since` can fail.
fn created_stamp(what: &str) -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => format!("{what} at {} (Unix seconds)", since.as_secs()),
        Err(_) => what.to_owned(),
    }
}

/// Which of `UI_DESIGN.md:129`'s four COSE operations [`Ide::run_operation`] is running.
///
/// The four answers each operation differs in, as five small functions rather than four copies of
/// one handler; see [`Ide::run_operation`] for why there is one handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Sign,
    Verify,
    Encrypt,
    Decrypt,
}

impl Operation {
    /// The verb, for a refusal: "cannot sign with `k`".
    fn label(self) -> &'static str {
        match self {
            Self::Sign => "sign",
            Self::Verify => "verify",
            Self::Encrypt => "encrypt",
            Self::Decrypt => "decrypt",
        }
    }

    /// The verb in the past tense, for the note saying what happened.
    fn done(self) -> &'static str {
        match self {
            Self::Sign => "signed",
            Self::Verify => "verified",
            Self::Encrypt => "encrypted",
            Self::Decrypt => "decrypted",
        }
    }

    /// Whether the input is the COSE document rather than the CBOR one.
    ///
    /// The two that *consume* a COSE object are exactly the two that produce a bare payload, so
    /// this one predicate decides both ends — which is why [`Operation::input_tab`] and
    /// [`Operation::result_tab`] are each one line and cannot disagree with it.
    fn reads_cose(self) -> bool {
        matches!(self, Self::Verify | Self::Decrypt)
    }

    /// The tab whose document is the input, for the "nothing to do" message.
    fn input_tab(self) -> Tab {
        match self.reads_cose() {
            true => Tab::Cose,
            false => Tab::Cbor,
        }
    }

    /// The tab the result lands in, which is the decision taken with the user for this phase: a
    /// result goes where that *kind* of artefact belongs.
    fn result_tab(self) -> Tab {
        match self.reads_cose() {
            true => Tab::Cbor,
            false => Tab::Cose,
        }
    }
}

/// Why `key` cannot be used for `op`, in the reason's **own sentence**.
///
/// Never `CoseError::InvalidKey`: an enum of reasons with reviewed prose is what
/// [`cose::UnsupportedSign`] exists for, and the undifferentiated answer is the thing this phase
/// replaces — `SignPanel.tsx` could only ever say "invalid key" and leave the user to guess which
/// of six things was wrong with it.
///
/// A free function so that the refusals can be read without a window, which is how everything in
/// this crate is tested; [`Ide::run_operation`] is its only caller.
///
/// Each predicate answers for **two** operations, which is sound rather than convenient:
///
/// * `unsupported_sign_reason` matches only on key type and curve (`cose/mod.rs:225-241`), so it
///   never answers `NoPrivateKey` and therefore cannot refuse a public-only key that a
///   *verification* would have accepted.
/// * `unsupported_encrypt_reason` answers only `Asymmetric`, which is as true of `Decrypt` as of
///   `Encrypt`. The other half of that question — a symmetric key of the wrong length for the
///   chosen algorithm — needs the algorithm, so `ops::encrypt` raises it itself
///   (`cose/ops.rs:253`) and it reaches the user through [`Ide::report`].
fn refusal(op: Operation, key: &tps_cose::CoseKey<'_>) -> Option<String> {
    match op {
        Operation::Sign | Operation::Verify => {
            cose::unsupported_sign_reason(key).map(|reason| reason.to_string())
        }
        Operation::Encrypt | Operation::Decrypt => {
            cose::unsupported_encrypt_reason(key).map(|reason| reason.to_string())
        }
    }
}

/// Whether the hex grid still holds the bytes `trace` saw.
///
/// Half of the honesty rule's second part; see the module docs. A whole-buffer compare, which is
/// why it is called once per render of the trace page and never per row: the corpus has a 196 KB
/// document in it.
fn hex_qualifies(trace: &cddl::ChainTrace, cbor: &[u8]) -> bool {
    trace.cbor == cbor
}

/// Whether the EDN editor still holds the text `trace`'s `edn` spans index.
///
/// The other half. `Trace::edn` is the canonical pretty-printed rendering; a document with one
/// space added, a comment, or a reordered map is a different string, and an `edn` span indexes the
/// canonical one. `None` — a trace that produced no EDN at all — qualifies nothing.
fn edn_qualifies(trace: &cddl::ChainTrace, edn: &str) -> bool {
    trace.trace.edn.as_deref() == Some(edn)
}

/// The path of a chain file by index, which is what the CDDL editors are keyed by.
///
/// [`cddl::Row::cddl`] and [`cddl::ChainTrace::rule_status_by_file`] both carry an index into
/// `Chain::files`, and core's documentation is explicit that it is **not** a tab index: the two
/// agree today only because both are depth-first pre-order with the opened file first. Every use
/// goes through here, so that stops being something each call site has to remember.
fn chain_file(trace: &cddl::ChainTrace, index: usize) -> Option<PathBuf> {
    trace.chain.files.get(index).map(|file| file.path.clone())
}

/// Whether a trace row's jump to `tab` is offered.
///
/// Free so that the disabled look and the click target cannot disagree: [`trace_row`] styles the
/// button by this and [`Ide::jump_row`] would refuse by the same facts. Two reasons to refuse, and
/// they are different — the row may carry no span in that space at all, or the pane may no longer
/// hold the artefact the trace saw, which is the honesty rule. The CDDL panes have no gate, because
/// the schema came from them.
fn row_jump(row: &cddl::Row, tab: Tab, hex_ok: bool, edn_ok: bool) -> bool {
    match tab {
        Tab::Cddl => row.cddl.is_some(),
        Tab::Edn => edn_ok && row.edn.is_some(),
        Tab::Cbor => hex_ok && row.cbor.is_some(),
        // No trace row is about a COSE structure; `Validate` does not run on that tab at all.
        Tab::Cose => false,
    }
}

/// The trace log's colours, resolved once per frame.
///
/// For the reason `hex_view.rs`'s `Colours` records: a [`uniform_list`] closure is `'static` and
/// receives `&mut App`, which it has already lent out, so `cx.theme()` is not reachable from
/// inside it. `Copy`, so each row takes it by value. Safe to resolve ahead of paint because the
/// theme mode is set once at startup and there is no in-app switch (`theme.rs`).
#[derive(Debug, Clone, Copy)]
struct TraceColours {
    matched: Hsla,
    failed: Hsla,
    attempt: Hsla,
    muted: Hsla,
    hover: Hsla,
}

impl TraceColours {
    fn resolve(cx: &App) -> Self {
        Self {
            matched: cx.theme().success,
            failed: cx.theme().danger,
            // An attempt is not a finding, so it reads as the quiet one even when shown.
            attempt: cx.theme().muted_foreground,
            muted: cx.theme().muted_foreground,
            hover: cx.theme().accent,
        }
    }
}

/// One trace log row: outcome, rule, path, and the three jump buttons.
///
/// Free rather than a method because the list closure it is called from is `'static` and holds no
/// `&Ide`; `view` is how it reaches back, and `rows` is the precomputed slice the closure captured.
///
/// The indentation is `TraceLog.tsx`'s geometry verbatim — see [`TRACE_INDENT_BASE`]. The row
/// background is clickable and the buttons sit on top of it, which is two gestures on one row:
/// `Button`'s own click path calls `cx.stop_propagation()`, so a button press does not also fire
/// the row click underneath it.
fn trace_row(
    index: usize,
    rows: &[cddl::Row],
    hex_ok: bool,
    edn_ok: bool,
    colours: TraceColours,
    view: &WeakEntity<Ide>,
) -> AnyElement {
    let Some(row) = rows.get(index) else {
        // Out of range is not reachable — `uniform_list` asks only for indices below the count it
        // was given — but an empty row is the honest answer if the two ever disagree.
        return div().h(px(TRACE_ROW_HEIGHT)).into_any_element();
    };

    let outcome = match row.outcome {
        cddl::TraceOutcome::Match => colours.matched,
        cddl::TraceOutcome::Fail => colours.failed,
        cddl::TraceOutcome::Attempt => colours.attempt,
    };

    // The three buttons differ only in tab and label, and the enablement rule they share is the
    // one thing about them that must not be written three times.
    let jump = |tab: Tab, label: &'static str| {
        jump_button(index, tab, label, row_jump(row, tab, hex_ok, edn_ok), view)
    };

    h_flex()
        // Stateful because `on_click` requires it, and from the list position for the reason
        // [`Ide::diagnostics_page`] records: the list is rebuilt whole, so no row holds state
        // across a rebuild.
        .id(("trace", index))
        .h(px(TRACE_ROW_HEIGHT))
        .w_full()
        .items_center()
        .gap_2()
        .pl(px(TRACE_INDENT_BASE + row.depth as f32 * TRACE_INDENT_STEP))
        .pr_2()
        .cursor_pointer()
        .hover(|this| this.bg(colours.hover))
        .on_click({
            let view = view.clone();
            move |_, _, cx| {
                // The row click highlights every qualifying pane and switches no tab
                // (`App.tsx:580-602`). The dropped `Result` is a closed window.
                let _ = view.update(cx, |ide, cx| ide.highlight_row(index, cx));
            }
        })
        .child(
            div()
                .w_16()
                .flex_none()
                .text_color(outcome)
                .child(row.outcome.as_str()),
        )
        .child(div().w_48().flex_none().child(row.rule.clone()))
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_color(colours.muted)
                .child(row.path.clone()),
        )
        .child(jump(Tab::Cddl, "C"))
        .child(jump(Tab::Edn, "E"))
        .child(jump(Tab::Cbor, "H"))
        .into_any_element()
}

/// One of a trace row's three jump buttons.
///
/// `label` doubles as the id's discriminator, which is what keeps the three distinct within a row;
/// the index keeps them distinct between rows.
fn jump_button(
    index: usize,
    tab: Tab,
    label: &'static str,
    enabled: bool,
    view: &WeakEntity<Ide>,
) -> impl IntoElement {
    let view = view.clone();
    Button::new((label, index))
        .label(label)
        .ghost()
        .xsmall()
        // Disabled rather than absent, so the three columns stay in the same place from row to row
        // and a missing span reads as "not here" rather than as a layout shift.
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            let _ = view.update(cx, |ide, cx| ide.jump_row(index, tab, window, cx));
        })
}

/// The byte range of a `;# start` directive line, if `text` has one.
///
/// Where [`StartRule::note`](tps_cbor_core::cddl::StartRule::note)'s warning is pointed when the
/// requested rule came from the file rather than from the start-production box. Found here rather
/// than taken from `check_chain`, which reports the *resolution* and not the line that asked for
/// it; and by scanning for the line rather than by re-running the directive parser, because the
/// span wanted is the whole line a user can be sent to, not the module name inside it.
///
/// Only the first is returned: a second `;# start` is a question about precedence this does not
/// have to answer, and `tps_cddl`'s own scan takes the first too.
fn start_directive_span(text: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(";#") {
            // `;#start` is as valid as `;# start`, so the keyword is checked after the marker
            // rather than as part of one fixed prefix.
            if rest.trim_start().starts_with("start") {
                let start = offset + (line.len() - trimmed.len());
                return Some(start..offset + line.trim_end_matches(['\r', '\n']).len());
            }
        }
        offset += line.len();
    }
    None
}

/// A file's name without its directory, for a label or a message.
///
/// Falls back to the whole path when there is no final component, which is the only case
/// `file_name` returns `None` for — a path ending in `..`. A lossy conversion rather than a
/// refusal, because this only ever produces text to show.
fn base_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// A diagnostic's origin, for the lower window's second column.
fn source_label(source: Source) -> &'static str {
    match source {
        Source::Cbor => "CBOR",
        Source::Edn => "EDN",
        Source::Cddl => "CDDL",
        Source::CddlModules => "includes",
        Source::Validation => "validation",
        Source::Cose => "COSE",
        Source::Settings => "settings",
    }
}

/// The tab whose document a diagnostic's span indexes into, and so the tab a click on its row
/// should go to. `None` makes the row inert (`UI_DESIGN.md:111`).
///
/// A free function rather than a method because it is the *rule*, and the rule is worth testing on
/// its own: it decides both whether a row looks clickable and where clicking it lands, so the two
/// cannot disagree about which rows have a destination.
///
/// Four kinds of row have none, and each is a deliberate refusal rather than a gap:
///
/// * **No position to offer.** `0..0` is the project's "there is no span" convention —
///   `tps_cbor_core::cddl::validate::diagnostic_for` uses it for every error with no CDDL span to
///   give — so a click could only land on the first byte of a document and claim a location the
///   diagnostic does not have. Checked first, because it cuts across every source.
/// * **A chain-wide finding naming no file.** A [`Source::CddlModules`] span indexes the *merged*
///   source until `check_chain` attributes it, and the merged document is not one the IDE holds:
///   there is no editor those offsets are coordinates in. The row still says what is wrong, and
///   the warning beside it says why it names no file.
/// * **COSE.** The tab is real as of Phase 7, and still has no reveal: the one diagnostic this
///   phase produces for it is an EDN rendering failure, and its offset is a position in the COSE
///   *bytes* while the pane shows the EDN rendered from them. A caret moved to that offset would
///   point at an unrelated character, which is worse than not moving it. Recorded as a divergence
///   from the plan in `PLAN.md`.
/// * **Settings.** Spans index the TOML file, which no tab opens.
fn diagnostic_target(diagnostic: &Diagnostic) -> Option<Tab> {
    if diagnostic.span == (0..0) {
        return None;
    }
    match diagnostic.source {
        // Both index the CBOR byte buffer, which is the hex grid's own coordinate space — a
        // validation mismatch is about a byte of CBOR, not about the schema that rejected it.
        Source::Cbor | Source::Validation => Some(Tab::Cbor),
        Source::Edn => Some(Tab::Edn),
        Source::Cddl => Some(Tab::Cddl),
        Source::CddlModules => diagnostic.file.is_some().then_some(Tab::Cddl),
        Source::Cose | Source::Settings => None,
    }
}

/// `line:column` for `offset` in `text`, 1-based, as [`diag::Position`] gives it.
fn line_column(text: &str, offset: usize) -> String {
    let position = diag::Position::at(text, offset);
    format!("{}:{}", position.line, position.column)
}

/// The name `Save As` should offer for the document on `tab`.
///
/// `UI_DESIGN.md:89` asks each tab for a default file extension, and an *untitled* document — which
/// is what both conversions and the decode produce, since all three clear the path — otherwise
/// reached the picker with no name at all. [`Tab::default_file_name`] answers it, so every tab has
/// one and a fifth tab cannot be added without supplying it.
///
/// A free function rather than a few lines inside [`Ide::on_save_as`] because the picker it feeds
/// is an `ashpd` portal request. The dialog is a separate process, so nothing about the name it
/// prefills can be asserted from this crate's tests, which left the rule the one part of the phase
/// with no check over it.
fn suggested_save_name(meta: Option<&Meta>, tab: Tab) -> String {
    meta.filter(|meta| meta.can_save())
        .map(|meta| meta.title().to_owned())
        .unwrap_or_else(|| tab.default_file_name().to_owned())
}

/// The confirmation detail for `Keystore → Delete Key...`, wrapped by hand.
///
/// **The hand-wrapping is the point, and it is not cosmetic.** The fallback prompt renders its
/// detail as one `div` inside a `flex_row` with no width of its own, inside an `overflow_hidden`
/// box (`gpui-pre/src/window/prompts.rs:104-129`), so a long line does not wrap — it is *clipped*,
/// mid-word, with no ellipsis and no indication that anything is missing. The first version of this
/// text read `` `my-es256` and its private material are removed from the key store. This cannot be
/// undone.`` and reached the user as "`my-es256` and its private material are remo", which loses
/// exactly the clause that matters on the one dialog in the application that destroys something
/// unrecoverable. GPUI does break on `\n`, so the lines are chosen here.
///
/// The alias is given a line of its own because it is the one part whose length this code does not
/// decide: `validate_alias` bounds it, but not to anything that is guaranteed to fit. A long alias
/// therefore clips *itself*, and the three sentences the user has to read survive.
fn delete_prompt_detail(alias: &str) -> String {
    format!(
        "`{alias}`\nis removed from the key store,\nwith its private material.\nThis cannot be \
         undone."
    )
}

/// Which of the two things a picker can return.
///
/// An enum of two rather than a `bool`, so the call sites read as `path_prompt(Kind::Files)` and
/// not as `path_prompt(true)` — the two commands differ in exactly this and nothing else, and a
/// bare `true` at the call site is how they came to be one command asking for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `File → Open...`: a document.
    Files,
    /// `File → Open Folder...`: the explorer's root.
    Directories,
}

/// What the system picker is asked for.
///
/// **One kind or the other, never both**, which is the whole reason this is a function with a test
/// over it. `PathPromptOptions` has a flag for each and macOS honours both at once, so asking for
/// both looks reasonable and is what this code did for five phases; on Linux the request reaches
/// the portal as a single `directory` boolean, and GPUI passes `options.directories` straight into
/// it (`gpui-pre-linux/src/linux/platform.rs:803`). Asking for both therefore produced a
/// folder-only chooser in which no file could be selected, which is how `File → Open...` came to
/// be unable to open a file.
fn path_prompt(kind: Kind) -> PathPromptOptions {
    PathPromptOptions {
        files: kind == Kind::Files,
        directories: kind == Kind::Directories,
        // One at a time: the window holds one workspace, and a second path would have to replace
        // the first.
        multiple: false,
        // The portal's own label, which is what `UI_DESIGN.md:49`'s "comply generally with the
        // recommendations of the running system" asks for.
        prompt: None,
    }
}

/// The byte range a decode failure blames, as something the hex view can highlight.
///
/// `CoreError::CborDecode` carries `decoder.pos()`, which for a truncated input is one *past* the
/// last byte. Clamping to the last real byte points at the item whose length ran off the end, which
/// is the byte the user has to change.
///
/// An empty buffer gets an empty range, not `0..1`. There is no byte zero to blame — the error is
/// that the file *is* empty — and a range naming a byte that does not exist is a highlight the hex
/// view cannot draw and a span a later phase would have to special-case anyway.
fn decode_span(error: &CoreError, len: usize) -> Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let offset = match error {
        CoreError::CborDecode { offset, .. } => *offset,
        _ => 0,
    };
    let start = offset.min(len - 1);
    start..start + 1
}

/// The unsaved documents, named, for the quit confirmation's detail line.
///
/// Tab labels rather than paths: a buffer may never have been on disk at all, and
/// "`(untitled)` (EDN)" is something the user can act on where a missing path is not. A free
/// function taking the model rather than a method taking `&App`, so it is testable without a
/// window — which is the only way anything in this crate gets tested.
fn unsaved_summary(workspace: &Workspace) -> String {
    let singles = [
        (Tab::Cbor, &workspace.cbor.meta),
        (Tab::Edn, &workspace.edn.meta),
        (Tab::Cose, &workspace.cose.meta),
    ];

    let named = singles
        .into_iter()
        .filter(|(_, meta)| meta.dirty)
        .map(|(tab, meta)| format!("{} ({})", meta.title(), tab.label()))
        .chain(
            workspace
                .cddl
                .tabs()
                .iter()
                .filter(|tab| tab.meta.dirty)
                .map(|tab| format!("{} (CDDL)", tab.meta.title())),
        )
        .collect::<Vec<_>>();

    format!("Unsaved: {}.", named.join(", "))
}

/// A not-yet-built pane, named and with a line saying which phase brings it.
///
/// Honest about being unfinished rather than rendering an empty editor that looks broken.
fn placeholder(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .flex_1()
        .p_4()
        .gap_1()
        .child(div().font_weight(FontWeight::BOLD).child(title.into()))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
        .into_any_element()
}

/// A muted line of prose, for a right pane or a page with something to say and nothing to offer.
///
/// Was `phase_note`, the stand-in for a tab whose options were not built yet. Phase 7 built the
/// last of them, so what is left is the real use: the `Keys` page's "no keys" and "the store could
/// not be opened" lines, and the right pane's "Unknown key". `String` rather than `&'static str`
/// now, because two of those three carry a path or a reason.
fn muted_note(note: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_color(cx.theme().muted_foreground)
        .child(note.into())
        .into_any_element()
}

/// The window's options, with the saved size restored.
///
/// Only the size is restored, not the position: a window reopened off-screen — which is what a
/// saved position does after a monitor changes — is unrecoverable without editing the settings
/// file, and `UI_DESIGN.md` asks for neither.
fn window_options(settings: &Settings, cx: &mut App) -> WindowOptions {
    let (width, height) = settings.layout.window.unwrap_or(DEFAULT_WINDOW);
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("cbor-ide".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(720.0), px(480.0))),
        app_id: Some("cbor-ide".to_owned()),
        ..Default::default()
    }
}

/// Paths named on the command line, which are opened as if the explorer had been clicked.
///
/// Not in `UI_DESIGN.md`, and added in Phase 3 for a plain reason: without it the hex grid can only
/// be reached by clicking the explorer, so nothing can drive the view from a script — and
/// `cbor-ide file.cbor` is what every editor does anyway. Every interactive check in Phase 3 starts
/// from this argument.
/// Each path goes through [`Ide::open_path`], so the extension decides the tab and a bad path
/// reports itself in the lower window rather than on stderr. Recorded in `PLAN.md`.
///
/// Hand-rolled rather than via `clap`: one positional list, no flags, and a whole argument parser is
/// a poor trade for that. `--` is honoured so a file named like a flag is still openable.
fn paths_from_args() -> Vec<PathBuf> {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|arg| arg == "--") {
        args.next();
    }
    args.map(PathBuf::from).collect()
}

fn main() {
    let paths = paths_from_args();

    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        languages::register();
        actions::bind_keys(cx);

        // A broken settings file costs its contents, not the session: `load` reports what it could
        // not read as diagnostics and carries on with defaults. They go straight to the lower
        // window, where the user can see them.
        let (settings, mut diagnostics) = Settings::load_default();
        diag::sort(&mut diagnostics);

        let options = window_options(&settings, cx);
        let (window, ide) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Ide::new(settings, diagnostics, window, cx))
        })
        .expect("open the main window");

        // After the window exists, so that a failure to read one has somewhere to be reported.
        //
        // Nested rather than a plain `ide.update`, because `open_path` needs a `&mut Window` as
        // well as the entity — a COSE key named on the command line raises the import dialog, like
        // one opened from the menu — and only the window handle can produce one. The handle's own
        // view is `AnyView`, which is why the first parameter is discarded and the entity is
        // reached through the `&mut App` the closure also hands over.
        let _ = window.update(cx, |_, window, cx| {
            ide.update(cx, |ide, cx| {
                for path in paths {
                    ide.open_path(path, window, cx);
                }
            })
        });

        // One save, at one moment, however the application is closed — the window button, the
        // menu, `Quit`, or a signal. The window is still alive here, which is the only reason the
        // geometry can be read at all.
        cx.on_app_quit(move |cx| {
            let size = window
                .update(cx, |_, window, _| {
                    let size = window.bounds().size;
                    (f32::from(size.width), f32::from(size.height))
                })
                .ok();
            ide.update(cx, |ide, cx| ide.persist(size, cx));
            async {}
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    // `use gpui_kit::*` at the crate root brings `gpui_macros::test` into scope, where it shadows
    // the built-in attribute and makes every `#[test]` here expand into itself until the recursion
    // limit stops it. An explicit import beats a glob one, so this restores the real `#[test]`.
    // The module-level test suites in this crate are unaffected: their `super` is a module, not the
    // crate root, where the glob does not reach macro resolution the same way.
    use std::prelude::v1::test;
    use tps_cbor_core::TextDoc;

    /// The progress row a background `Validate` puts in the lower window. `Info`, so it is the
    /// quietest row on the page rather than looking like a finding, and it names the schema,
    /// because the keyboard path gives no other indication that anything is happening.
    #[test]
    fn the_progress_row_is_informational_and_names_the_schema() {
        let running = Validating::new("rfc9052.cddl".to_owned());

        assert_eq!(running.root, "rfc9052.cddl");
        assert_eq!(running.row.severity, Severity::Info);
        assert_eq!(running.row.source, Source::Validation);
        // `0..0` is the project's "there is no span" value, which makes the row inert.
        assert!(running.row.span.is_empty());
        assert!(
            running.row.message.contains("rfc9052.cddl"),
            "the progress row must name the schema: {}",
            running.row.message
        );
    }

    #[test]
    fn the_quit_prompt_names_every_unsaved_document() {
        let mut workspace = Workspace::new();
        workspace.cbor.meta.path = Some(PathBuf::from("/tmp/value.cbor"));
        workspace.cbor.meta.dirty = true;
        // Dirty from a conversion, so it has no name of its own.
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.edn.meta.dirty = true;

        let summary = unsaved_summary(&workspace);
        assert!(summary.contains("value.cbor (CBOR Binary)"), "{summary}");
        assert!(summary.contains("(untitled) (EDN)"), "{summary}");
        assert!(
            !summary.contains("COSE"),
            "a clean document was listed: {summary}"
        );
    }

    /// The dirty CDDL tab is the one the summary could most easily miss: it is the only kind held
    /// in a set rather than a field.
    #[test]
    fn an_unsaved_inner_cddl_tab_is_listed_too() {
        let dir = std::env::temp_dir().join(format!("tps-ide-quit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create");
        let path = dir.join("schema.cddl");
        std::fs::write(&path, "a = uint\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.cddl.open(&path).expect("opens");
        let tab = workspace.cddl.active_mut().expect("active");
        tab.meta.read_only = false;
        tab.edit("a = tstr\n").expect("editable");

        let summary = unsaved_summary(&workspace);
        assert!(summary.contains("schema.cddl (CDDL)"), "{summary}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `UI_DESIGN.md:89`. The untitled case is the one that matters and the one that was broken:
    /// `To EDN`, `To CBOR` and the decode all clear the path, so the document `Save As` is most
    /// often asked about has no name of its own and previously reached the picker with none.
    #[test]
    fn save_as_offers_the_tabs_own_extension_for_an_untitled_document() {
        for tab in Tab::ALL {
            let suggested = suggested_save_name(None, tab);
            assert_eq!(
                suggested,
                tab.default_file_name(),
                "{tab:?} should fall back to its own default name"
            );
            assert!(
                suggested.contains('.'),
                "{tab:?} offers `{suggested}`, which carries no extension"
            );
        }
    }

    /// A document with a path keeps its own name, so `Save As` opens on the file the user is
    /// looking at rather than resetting it to `untitled.*`.
    #[test]
    fn save_as_keeps_the_name_of_a_document_that_has_one() {
        let meta = Meta::at("/tmp/claims.edn");
        assert_eq!(suggested_save_name(Some(&meta), Tab::Edn), "claims.edn");
    }

    /// `Meta::can_save` is the gate, not `path.is_some()`, and a pathless `Meta` must not reach
    /// `title()` — which answers `"(untitled)"`, a name with no extension that would be offered to
    /// the picker verbatim.
    #[test]
    fn save_as_never_offers_the_untitled_placeholder_as_a_file_name() {
        let meta = Meta::new();
        let suggested = suggested_save_name(Some(&meta), Tab::Cbor);
        assert_eq!(suggested, Tab::Cbor.default_file_name());
        assert!(!suggested.contains("(untitled)"), "{suggested}");
    }

    /// The longest line [`delete_prompt_detail`] may put in a [`Window::prompt`] detail.
    ///
    /// Measured rather than chosen: the fallback prompt is `w_72` — 288 px — with `p_3` padding
    /// either side, leaving 264 px, and the detail renders at `text_sm`
    /// (`gpui-pre/src/window/prompts.rs:107`, `:126`). 40 characters of the default proportional
    /// font fit inside that with room to spare for a wider one.
    ///
    /// Here rather than beside the function it bounds, because only the test reads it and a
    /// `const` in the module proper is dead code in a non-test build.
    const PROMPT_LINE: usize = 40;

    /// The delete confirmation is the one dialog here that destroys something unrecoverable, and
    /// the fallback prompt clips a long line instead of wrapping it — so a line that grew would
    /// take the warning with it. Every line but the alias's own is checked, for the reason
    /// [`delete_prompt_detail`] gives.
    #[test]
    fn the_delete_confirmation_fits_the_prompt() {
        let detail = delete_prompt_detail("my-es256");
        for line in detail.lines().skip(1) {
            assert!(
                line.chars().count() <= PROMPT_LINE,
                "`{line}` is {} characters and will be clipped",
                line.chars().count()
            );
        }
        // The clause that was lost to clipping, and the reason the wrapping exists at all.
        assert!(detail.contains("cannot be undone"), "{detail}");
        assert!(detail.contains("my-es256"), "{detail}");
    }

    /// The defect this split exists to fix, pinned so it cannot come back: a picker asked for
    /// files *and* directories is, on Linux, a picker that offers only directories — so
    /// `File → Open...` could not open a file at all. Neither request may ask for both, and each
    /// must ask for something.
    ///
    /// Exhaustive over the two kinds, so a third could not be added without answering the same
    /// question. This is the one assertion available over the picker from a unit test: the dialog
    /// itself belongs to another process.
    #[test]
    fn each_open_command_asks_for_exactly_one_kind_of_path() {
        for kind in [Kind::Files, Kind::Directories] {
            let options = path_prompt(kind);
            assert!(
                options.files != options.directories,
                "{kind:?} asks for files={} and directories={}; on Linux asking for both yields a \
                 folder-only chooser",
                options.files,
                options.directories
            );
            assert!(!options.multiple, "{kind:?} takes one path");
        }
        assert!(path_prompt(Kind::Files).files);
        assert!(path_prompt(Kind::Directories).directories);
    }

    #[test]
    fn a_truncated_decode_blames_a_byte_that_exists() {
        // A map header promising one pair, with nothing after it.
        let bytes = [0xa1];
        let error = convert::check_well_formed(&bytes).expect_err("truncated");
        let span = decode_span(&error, bytes.len());
        assert_eq!(
            span,
            0..1,
            "the only byte in the buffer is the one to blame"
        );
    }

    #[test]
    fn an_empty_buffer_gets_an_empty_span_rather_than_a_byte_that_is_not_there() {
        let error = convert::check_well_formed(&[]).expect_err("empty is not a CBOR item");
        assert_eq!(decode_span(&error, 0), 0..0);
    }

    #[test]
    fn a_decode_offset_past_the_end_is_pulled_back_into_the_buffer() {
        // `0x19` is "unsigned, two more bytes"; only one follows, so the decoder stops past the end.
        let bytes = [0x19, 0x01];
        let error = convert::check_well_formed(&bytes).expect_err("truncated");
        let span = decode_span(&error, bytes.len());
        assert!(
            span.end <= bytes.len(),
            "span {span:?} points outside a {}-byte buffer",
            bytes.len()
        );
    }

    /// Two `TabBar`s with the same `ElementId` share one piece of GPUI element state, and the
    /// symptom is not where the mistake is: the *outer* strip loses its selection when the inner
    /// one is clicked. Trivial to assert and very hard to diagnose.
    #[test]
    fn the_three_tab_bar_ids_are_all_distinct() {
        let ids = [TAB_BAR_ID, CDDL_TAB_BAR_ID, LOWER_TAB_BAR_ID];
        let mut seen = ids.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), ids.len(), "two tab bars share an `ElementId`");
    }

    /// The Phase-2-to-Phase-5 gap, as a test. `menus.rs` enabled `Syntax Check` on the CDDL tab
    /// from Phase 2 and `actions.rs` bound `secondary-shift-k` to it, with no handler anywhere — so
    /// the item and the shortcut were silent. Cross-referencing the two rules means a future tab
    /// cannot be given the item without being given the behaviour.
    #[test]
    fn the_syntax_check_item_has_a_handler_for_every_tab_it_is_enabled_on() {
        for tab in Tab::ALL {
            let menus = menus::menus(menus::MenuState {
                tab,
                has_content: true,
                ..menus::MenuState::default()
            });
            let offered = menus
                .iter()
                .find(|menu| menu.name == "View")
                .expect("a View menu")
                .items
                .iter()
                .any(|item| match item {
                    gpui_kit::MenuItem::Action { name, .. } => {
                        name.as_ref() == "Syntax Check" && !item.is_disabled()
                    }
                    _ => false,
                });
            assert_eq!(
                offered,
                syntax_check_applies(tab),
                "{tab:?}: the menu offers `Syntax Check` but the handler {} act on it",
                if offered { "does not" } else { "does" }
            );
        }
    }

    /// The same cross-reference for `Validate`, which until Phase 6 was the worse version of that
    /// gap: the item was enabled on two tabs and the handler *answered* on both, but with a
    /// well-formedness check that said so in small print. Enabled-and-wrong is harder to notice
    /// than enabled-and-silent.
    #[test]
    fn validate_is_answered_on_every_tab_it_is_enabled_on() {
        for tab in Tab::ALL {
            let menus = menus::menus(menus::MenuState {
                tab,
                has_content: true,
                ..menus::MenuState::default()
            });
            let offered = menus
                .iter()
                .find(|menu| menu.name == "View")
                .expect("a View menu")
                .items
                .iter()
                .any(|item| match item {
                    gpui_kit::MenuItem::Action { name, .. } => {
                        name.as_ref() == "Validate" && !item.is_disabled()
                    }
                    _ => false,
                });
            assert_eq!(
                offered,
                validate_applies(tab),
                "{tab:?}: the menu offers `Validate` but the handler {} act on it",
                if offered { "does not" } else { "does" }
            );
        }
    }

    /// A trace of `cbor`, with `edn` as the rendering it saw. Nothing else is needed by the two
    /// comparison rules, which is the point of their being pure functions over these two fields.
    fn trace_of(cbor: &[u8], edn: Option<&str>) -> cddl::ChainTrace {
        cddl::ChainTrace {
            chain: cddl::Chain {
                files: Vec::new(),
                attributed: true,
                diagnostics: Vec::new(),
            },
            trace: cddl::Trace {
                mismatches: Vec::new(),
                events: Vec::new(),
                edn: edn.map(str::to_owned),
            },
            diagnostics: Vec::new(),
            start: cddl::Start {
                name: Some("root".to_owned()),
                rejected: None,
                defaulted: false,
            },
            cbor: cbor.to_vec(),
        }
    }

    /// The honesty rule, half one. `Validate` on the EDN tab traces the CBOR the *displayed EDN*
    /// encodes to (`UI_DESIGN.md:97`), which is a transient buffer the hex grid never held — so a
    /// `cbor` span is a coordinate into bytes that may be nowhere on screen. Highlighting them
    /// anyway would point confidently at the wrong byte, which is worse than pointing at none.
    #[test]
    fn a_pane_holding_different_bytes_is_not_highlighted() {
        let trace = trace_of(&[0xa1, 0x01, 0x02], None);

        assert!(
            hex_qualifies(&trace, &[0xa1, 0x01, 0x02]),
            "the bytes the trace saw must qualify"
        );
        assert!(!hex_qualifies(&trace, &[0xa1, 0x01, 0x03]));
        // The near miss that matters most: a prefix is not a match, and a length compare alone
        // would have let the longer buffer through.
        assert!(!hex_qualifies(&trace, &[0xa1, 0x01]));
        assert!(!hex_qualifies(&trace, &[0xa1, 0x01, 0x02, 0x03]));
        assert!(!hex_qualifies(&trace, &[]));
    }

    /// The honesty rule, half two. `Trace::edn` is the canonical pretty-printed rendering; a
    /// hand-edited document is a different string and an `edn` span indexes the canonical one. One
    /// added space is enough, and is exactly the edit a user makes without thinking of it as one.
    #[test]
    fn a_pane_holding_different_edn_is_not_highlighted() {
        let canonical = "{\n  1: 2\n}";
        let trace = trace_of(&[], Some(canonical));

        assert!(
            edn_qualifies(&trace, canonical),
            "the rendering the trace saw must qualify"
        );
        assert!(!edn_qualifies(&trace, "{\n  1:  2\n}"), "one added space");
        assert!(!edn_qualifies(&trace, "{1: 2}"), "reflowed");
        assert!(!edn_qualifies(&trace, ""));

        // A trace that produced no rendering at all qualifies nothing — not even the empty
        // document, which `None == Some("")` would otherwise wave through.
        let no_edn = trace_of(&[], None);
        assert!(!edn_qualifies(&no_edn, ""));
        assert!(!edn_qualifies(&no_edn, canonical));
    }

    /// Per-icon enablement, so the disabled look and the click target cannot disagree: a row with
    /// no span in a space offers no jump into it, and neither does a row whose pane is disqualified
    /// by the comparison rule above. The CDDL pane has no gate, because the schema came from it.
    #[test]
    fn a_row_with_no_span_offers_no_jump() {
        let full = cddl::Row {
            depth: 0,
            outcome: cddl::TraceOutcome::Fail,
            rule: "root".to_owned(),
            path: "(root)".to_owned(),
            cddl: Some((0, 0..8)),
            cbor: Some(0..3),
            edn: Some(0..5),
        };
        for tab in Tab::ALL {
            assert_eq!(
                row_jump(&full, tab, true, true),
                tab != Tab::Cose,
                "{tab:?}: a row with every span, and every pane qualifying"
            );
        }

        // No span in a space: no jump into it, whatever the panes hold.
        let bare = cddl::Row {
            cddl: None,
            cbor: None,
            edn: None,
            ..full.clone()
        };
        for tab in Tab::ALL {
            assert!(!row_jump(&bare, tab, true, true), "{tab:?}");
        }

        // A disqualified pane: the span exists and the jump is still refused.
        assert!(!row_jump(&full, Tab::Cbor, false, true));
        assert!(!row_jump(&full, Tab::Edn, true, false));
        assert!(
            row_jump(&full, Tab::Cddl, false, false),
            "the CDDL panes always qualify"
        );
    }

    /// The span `StartRule::note`'s warning is pointed at. The whole line, because that is what a
    /// click on the diagnostic row can usefully scroll to, and trimmed of its indentation so the
    /// highlight starts on the `;`.
    #[test]
    fn the_start_directive_span_covers_its_whole_line() {
        let text = "a = uint\n  ;# start a\nb = tstr\n";
        let span = start_directive_span(text).expect("there is a `;# start`");
        assert_eq!(&text[span], ";# start a");
    }

    /// `;#start` is as valid to `tps_cddl` as `;# start`, so the keyword is looked for after the
    /// marker rather than as one fixed prefix. A fixed `";# start"` would silently find nothing.
    #[test]
    fn the_start_directive_is_found_without_a_space_after_the_marker() {
        assert!(start_directive_span(";#start a\n").is_some());
    }

    /// Ordinary comments and other directives are not it. `0..0` is what the caller substitutes,
    /// and it must come from a real absence rather than from a near miss.
    #[test]
    fn an_ordinary_comment_is_not_a_start_directive() {
        assert_eq!(start_directive_span("; start here\na = uint\n"), None);
        assert_eq!(start_directive_span(";# include rfc9052\n"), None);
    }

    /// `UI_DESIGN.md:111`'s clickable rows, in the negative. `0..0` is the project's "no position
    /// to offer" convention, and a row carrying it must stay inert for *every* source: a click that
    /// scrolled to byte 0 would look like a located error and point at a line nobody wrote.
    ///
    /// Exhaustive over [`Source`] on purpose. A new variant added to `diag.rs` has to be thought
    /// about here, and the failure mode without this test is a row that navigates to the wrong
    /// place — which is indistinguishable, on screen, from one that navigates correctly.
    #[test]
    fn a_diagnostic_without_a_span_is_not_clickable() {
        for source in [
            Source::Cbor,
            Source::Edn,
            Source::Cddl,
            Source::CddlModules,
            Source::Validation,
            Source::Cose,
            Source::Settings,
        ] {
            let diagnostic = Diagnostic::new(Severity::Error, source, 0..0, "no position")
                .in_file(PathBuf::from("/tmp/schema.cddl"));
            assert_eq!(
                diagnostic_target(&diagnostic),
                None,
                "{source:?} with an empty span offers a destination"
            );
        }

        // And the inverse, so the test cannot pass by `diagnostic_target` refusing everything: the
        // same diagnostic with a real span is clickable.
        let located = Diagnostic::new(Severity::Error, Source::Cddl, 4..8, "located")
            .in_file(PathBuf::from("/tmp/schema.cddl"));
        assert_eq!(diagnostic_target(&located), Some(Tab::Cddl));
    }

    /// `UI_DESIGN.md:112` in one assertion: a finding attributed to an included file routes to the
    /// *inner* tab holding that file, not to whichever one the user happens to be on.
    ///
    /// Two halves, because two different pieces of code answer them and either can be wrong on its
    /// own: [`diagnostic_target`] names the outer tab, and `CddlDocSet::index_of` names the inner
    /// one. `Ide::reveal_diagnostic` composes them, and cannot be called here — it needs a window —
    /// so the composition is what the interactive script's step 10 checks.
    ///
    /// The unattributed case is checked alongside it: a `CddlModules` diagnostic with no file still
    /// carries merged-source coordinates, which index no document the IDE holds, so it must stay
    /// inert even though its span is real.
    #[test]
    fn a_cddl_diagnostic_routes_to_the_inner_tab_naming_its_file() {
        let dir = std::env::temp_dir().join(format!("tps-ide-route-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create");
        let root = dir.join("root.cddl");
        let included = dir.join("included.cddl");
        std::fs::write(&root, "a = uint\n").expect("write");
        std::fs::write(&included, "b = tstr\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.cddl.open(&root).expect("opens the root");
        workspace.cddl.open(&included).expect("opens the include");
        // Selected tab 1, so that "routes to the file it names" is not satisfied by accident.
        assert!(workspace.cddl.select(0));

        let diagnostic = Diagnostic::new(Severity::Error, Source::CddlModules, 4..8, "undefined")
            .in_file(included.clone());
        assert_eq!(diagnostic_target(&diagnostic), Some(Tab::Cddl));
        assert_eq!(
            workspace.cddl.index_of(&included),
            Some(1),
            "the include is the second inner tab"
        );

        let unattributed = Diagnostic::new(Severity::Error, Source::CddlModules, 4..8, "undefined");
        assert_eq!(
            diagnostic_target(&unattributed),
            None,
            "a merged-source span indexes no open document"
        );

        // A file the chain named but that has no tab is still a destination, because
        // `Ide::reveal_diagnostic` opens it. This is the common case rather than the exception:
        // `check_chain` resolves from disk, so immediately after a `Syntax Check` only the root
        // has a tab and every other attributed row names a file like this one. Were the rule to
        // consult the document set, those rows would all be inert and the attribution work would
        // be invisible at exactly the moment it is produced.
        let absent = dir.join("not-open.cddl");
        assert_eq!(workspace.cddl.index_of(&absent), None, "it has no tab");
        let elsewhere = Diagnostic::new(Severity::Error, Source::CddlModules, 4..8, "undefined")
            .in_file(absent);
        assert_eq!(diagnostic_target(&elsewhere), Some(Tab::Cddl));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every page the lower window can show, derived from [`LowerTab::pages`] rather than written
    /// out, so a fourth page cannot be added without appearing here.
    fn every_lower_page() -> Vec<LowerTab> {
        let mut pages: Vec<LowerTab> = Tab::ALL
            .iter()
            .flat_map(|tab| LowerTab::pages(*tab).iter().copied())
            .collect();
        pages.dedup();
        pages.sort_by_key(|page| format!("{page:?}"));
        pages.dedup();
        pages
    }

    /// `Keys` belongs to the COSE tab and to no other (`UI_DESIGN.md:128-130`), and the two global
    /// pages belong to all four. Exhaustive over `Tab::ALL`, because the strip is built from this
    /// slice and a page missing from it is a page with no way to reach it.
    #[test]
    fn the_lower_pages_depend_on_the_tab() {
        for tab in Tab::ALL {
            let pages = LowerTab::pages(tab);
            assert_eq!(
                pages.contains(&LowerTab::Keys),
                tab == Tab::Cose,
                "{tab:?}: the `Keys` page belongs to the COSE tab alone"
            );
            for global in [LowerTab::Diagnostics, LowerTab::Trace] {
                assert!(pages.contains(&global), "{tab:?} must offer {global:?}");
            }
            // The strip falls back to index 0 for a selection it cannot find, so the first entry
            // is the one a confused selection lands on — it had better be the page that is always
            // available and always has something to say.
            assert_eq!(
                pages.first(),
                Some(&LowerTab::Diagnostics),
                "{tab:?}: `Diagnostics` must be the strip's first page"
            );
        }
        assert_eq!(every_lower_page().len(), 3, "three pages in total");
    }

    /// The property [`page_after`] exists for, over every tab pair and every page: whatever the
    /// lower window ends up showing, the strip offers it. A page outside the strip's own list draws
    /// no underline at all, so the symptom of getting this wrong is a lower window that looks
    /// unselected while showing something.
    #[test]
    fn the_lower_window_never_shows_a_page_the_tab_does_not_offer() {
        for from in Tab::ALL {
            for to in Tab::ALL {
                for page in every_lower_page() {
                    let after = page_after(from, to, page);
                    assert!(
                        LowerTab::pages(to).contains(&after),
                        "{from:?} -> {to:?} with {page:?} showing left {after:?}, which {to:?} \
                         does not offer"
                    );
                }
            }
        }
    }

    /// The `Keys` page is *selected* on arrival rather than merely offered, and — the half that is
    /// easy to get wrong — left alone once the user is already there. An operation's result landing
    /// on the tab it was invoked from must not pull the page out from under the line reporting it.
    #[test]
    fn arriving_at_the_cose_tab_selects_the_keys_page_but_staying_does_not() {
        assert_eq!(
            page_after(Tab::Cbor, Tab::Cose, LowerTab::Diagnostics),
            LowerTab::Keys,
            "arriving at COSE shows the key list"
        );
        for page in every_lower_page() {
            assert_eq!(
                page_after(Tab::Cose, Tab::Cose, page),
                page,
                "a `Sign` on the COSE tab must leave {page:?} showing"
            );
        }
        assert_eq!(
            page_after(Tab::Cose, Tab::Cbor, LowerTab::Keys),
            LowerTab::Diagnostics,
            "leaving COSE falls back to Diagnostics"
        );
        assert_eq!(
            page_after(Tab::Cose, Tab::Cbor, LowerTab::Trace),
            LowerTab::Trace,
            "leaving COSE must not disturb a page the new tab does offer"
        );
    }

    /// The extension decides first, so a `.edn` or `.cddl` file is never sniffed — those two are
    /// text, and a user who named a file is not to be second-guessed about it.
    #[test]
    fn route_follows_the_extension_before_the_bytes() {
        for (extension, expected) in [("edn", Route::Edn), ("cddl", Route::Cddl)] {
            assert_eq!(route(extension, b""), expected);
            // Even holding bytes that would have sniffed as something else.
            assert_eq!(route(extension, &cose_key()), expected);
        }
    }

    /// The false-positive case, and the one that matters most: an ordinary CBOR document must land
    /// in the hex grid exactly as it did before there was a sniffer at all. `tps_cbor_core` tests
    /// the recognition itself over the corpus; this tests that [`route`] believes it.
    #[test]
    fn route_sends_ordinary_cbor_to_the_hex_tab() {
        // A CWT-shaped claims set: a map whose label 1 is a `tstr` issuer. The nearest miss for a
        // COSE key, whose label 1 is the key type.
        let claims = [0xa1, 0x01, 0x66, b'i', b's', b's', b'u', b'e', b'r'];
        assert_eq!(route("cbor", &claims), Route::Cbor);
        assert_eq!(route("cbor", &[0x18, 0x2a]), Route::Cbor, "an integer");
        assert_eq!(route("", b""), Route::Cbor, "an empty file has no artefact");
    }

    /// The module-search-path box's text is the only *encoding* of a settings field this crate
    /// does, so it is the only one that can be lost on a round trip — and a lost search path does
    /// not fail, it makes a `;# include` stop resolving a session later, which is a long way from
    /// its cause. The empty list is in here because it is the state of a fresh settings file, where
    /// `join` on an empty slice and `split_paths` on an empty string are each the easy thing to get
    /// wrong in the opposite direction.
    #[test]
    fn parse_and_format_search_paths_round_trip() {
        let cases: [Vec<PathBuf>; 4] = [
            Vec::new(),
            vec![PathBuf::from("/usr/share/cddl")],
            vec![
                PathBuf::from("/usr/share/cddl"),
                PathBuf::from("/home/u/My Schemas"),
                PathBuf::from("relative/modules"),
            ],
            // A trailing separator and an empty entry, which is what a half-finished edit in the
            // box looks like: both drop out rather than becoming a directory named "".
            parse_search_paths(&format!("/a{PATH_LIST_SEPARATOR}{PATH_LIST_SEPARATOR}")),
        ];

        for paths in cases {
            let text = format_search_paths(&paths);
            assert_eq!(
                parse_search_paths(&text),
                paths,
                "`{text}` does not parse back to what formatted it"
            );
            assert!(
                !text.contains(&format!("{PATH_LIST_SEPARATOR}{PATH_LIST_SEPARATOR}")),
                "`{text}` holds an empty entry"
            );
        }

        assert_eq!(format_search_paths(&[]), "", "the fresh-settings case");
        assert!(
            parse_search_paths("").is_empty(),
            "an empty box is no paths, not one unnamed one"
        );
    }

    /// `UI_DESIGN.md:119`'s "File → Open allows a COSE key to be opened" — as an import, because a
    /// document would hold the private scalar in a plain `Vec<u8>` and `tps_cbor_core/CLAUDE.md`
    /// forbids a path that takes private material out of `Zeroizing`.
    #[test]
    fn route_sends_a_cose_key_to_an_import() {
        assert_eq!(route("cbor", &cose_key()), Route::ImportKey);
        // And a signature made with it is a document, on the COSE tab.
        let key_bytes = cose_key();
        let key = tps_cose::decode_cose_key(&key_bytes).expect("decodes");
        let signed = cose::ops::sign(&[0x18, 0x2a], &key).expect("signs");
        assert_eq!(route("cbor", &signed), Route::Cose);
    }

    /// A freshly generated P-256 key, as stored bytes.
    fn cose_key() -> Vec<u8> {
        cose::keygen::generate_cose_key(cose::keygen::GenKeyKind::Ec2P256)
            .expect("generates")
            .to_vec()
    }

    /// Every action the root view installs a handler for, in the order the `.on_action` chain in
    /// [`Render::render`] installs them. **Edited beside that chain**, which is the one thing this
    /// list asks of a future reader; the tests below are what make forgetting visible.
    ///
    /// Named through `Action::name` rather than by string literal, so a renamed action cannot
    /// quietly drop out of the comparison.
    fn handled_actions() -> Vec<&'static str> {
        use gpui_kit::Action as _;
        vec![
            actions::Open.name(),
            actions::OpenFolder.name(),
            actions::Save.name(),
            actions::SaveAs.name(),
            actions::Quit.name(),
            actions::SelectCborTab.name(),
            actions::SelectEdnTab.name(),
            actions::SelectCddlTab.name(),
            actions::SelectCoseTab.name(),
            actions::ToEdn.name(),
            actions::ToCbor.name(),
            actions::Validate.name(),
            actions::SyntaxCheck.name(),
            actions::DecodeSelection.name(),
            actions::OpenInclude.name(),
            actions::GenerateKey.name(),
            actions::DeleteKey.name(),
            actions::ImportKey.name(),
            actions::Sign.name(),
            actions::Verify.name(),
            actions::Encrypt.name(),
            actions::Decrypt.name(),
            actions::ToggleOptions.name(),
            actions::Find.name(),
            actions::Replace.name(),
            actions::FindNext.name(),
            actions::FindPrevious.name(),
            actions::Copy.name(),
            actions::Paste.name(),
            actions::About.name(),
        ]
    }

    /// Every action that appears in a menu with no handler **on purpose**, with the reason.
    ///
    /// **Empty, as of Phase 8.** Kept — with its guard tests — because the list is the mechanism
    /// that makes a future unhandled menu item a deliberate, documented choice rather than an
    /// oversight; `no_menu_action_is_unhandled` is what asserts the choice is not being made today.
    ///
    /// It held the clipboard and the `Find` family for six phases, under a reason that was **not
    /// true**: that a root handler "would intercept them *before* the editor and break the thing
    /// they are for". It would not. GPUI resolves a keystroke from the focused element outwards,
    /// and gpui-base binds `ctrl-f`, `ctrl-h`, `ctrl-c` and `ctrl-v` in the deeper `"Input"` key
    /// context (`gpui-base/src/input/base/state.rs:139`, `:310-316`) while ours are bound in the
    /// single root `Ide` one (`actions.rs:148`). A binding in an outer context cannot shadow one in
    /// an inner context, so the editor's own shortcuts always win and the root bindings only fire
    /// where no editor has the focus. The correction is recorded rather than quietly made, because
    /// the claim is the sort that reads plausibly and would be believed again.
    ///
    /// And it was the wrong shape of argument anyway: [`Ide::on_copy`] *forwards to the editor*,
    /// so the design is right whichever binding wins.
    fn unhandled_actions() -> Vec<&'static str> {
        Vec::new()
    }

    /// Phase 2 put `Generate Key...` in the menu bar with **no handler anywhere**, so selecting it
    /// did nothing at all for five phases. This is the cross-reference that catches that class of
    /// mistake: every action offered in any menu, on any tab, is either handled by the root view or
    /// listed as deliberately unhandled with a reason recorded beside it.
    #[test]
    fn every_menu_item_is_handled_or_deliberately_not() {
        let handled = handled_actions();
        let unhandled = unhandled_actions();
        for tab in Tab::ALL {
            let menus = menus::menus(menus::MenuState {
                tab,
                ..menus::MenuState::default()
            });
            for menu in &menus {
                for item in &menu.items {
                    let gpui_kit::MenuItem::Action { action, .. } = item else {
                        continue;
                    };
                    let name = action.name();
                    assert!(
                        handled.contains(&name) || unhandled.contains(&name),
                        "`{name}` is in the {} menu on {tab:?} and is neither handled nor \
                         recorded as deliberately unhandled",
                        menu.name
                    );
                }
            }
        }
    }

    /// The two lists must not overlap, or the test above would pass whichever list an action was
    /// really in — and neither may list an action twice, which is how a copy-paste edit hides one.
    #[test]
    fn no_action_is_both_handled_and_deliberately_unhandled() {
        let handled = handled_actions();
        let unhandled = unhandled_actions();
        for name in &handled {
            assert!(!unhandled.contains(name), "`{name}` is in both lists");
        }
        for list in [&handled, &unhandled] {
            let mut sorted = (*list).clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), list.len(), "an action is listed twice");
        }
    }

    /// Phase 8's headline assertion, and the inverse of the test above: nothing in any menu is
    /// unhandled any more.
    ///
    /// Stated as its own test rather than left implicit in an empty list, because
    /// `every_menu_item_is_handled_or_deliberately_not` passes just as happily with an item moved
    /// *into* `unhandled_actions` as with one given a handler. This is what makes the next such
    /// move a decision someone has to argue for.
    #[test]
    fn no_menu_action_is_unhandled() {
        assert!(
            unhandled_actions().is_empty(),
            "`unhandled_actions` is {:?}; a menu item with no handler is a command that silently \
             does nothing",
            unhandled_actions()
        );
    }

    /// `UI_DESIGN.md:50-53` requires Find, Copy and Paste to be offered on every tab, and
    /// `menus::quit_help_and_find_are_always_offered` asserts the items are *enabled*. This is the
    /// other half: that an enabled item leads somewhere.
    ///
    /// Exhaustive over `Tab::ALL` rather than over the menu, because the Edit menu does not vary by
    /// tab and the thing worth pinning is that the **handlers** answer on all four — which they do
    /// by dispatching through [`Ide::editor_for_tab`], whose `None` arms are the CBOR tab (the hex
    /// grid answers for itself) and a CDDL tab with no file open (which says so).
    #[test]
    fn find_and_copy_are_answered_on_every_tab() {
        use gpui_kit::Action as _;

        let handled = handled_actions();
        for name in [
            actions::Find.name(),
            actions::FindNext.name(),
            actions::FindPrevious.name(),
            actions::Copy.name(),
            actions::Paste.name(),
            actions::About.name(),
        ] {
            assert!(handled.contains(&name), "`{name}` has no handler");
        }

        // The menu offers them whatever the tab, so the handler is reached on all four and the
        // dispatch has to have an arm for each. A `Tab` gaining a fifth variant without an arm is
        // a compile error in `editor_for_tab`; this is the reminder that the *menu* is what makes
        // that arm reachable.
        for tab in Tab::ALL {
            let menus = menus::menus(menus::MenuState {
                tab,
                ..menus::MenuState::default()
            });
            let edit = menus
                .iter()
                .find(|menu| menu.name == "Edit")
                .expect("every tab has an Edit menu");
            for item in &edit.items {
                let gpui_kit::MenuItem::Action { action, .. } = item else {
                    continue;
                };
                assert!(
                    handled.contains(&action.name()),
                    "`{}` is offered in Edit on {tab:?} with no handler",
                    action.name()
                );
            }
        }
    }

    /// The version in the About box comes from `Cargo.toml` and nowhere else, so bumping it cannot
    /// leave a stale string behind. The licence and the stack are pinned too: an About box that
    /// names the wrong licence is worse than none.
    #[test]
    fn the_about_text_names_the_version() {
        let text = about_lines().join("\n");
        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "the About text does not name the version: {text}"
        );
        assert!(
            text.contains("MIT"),
            "the About text does not name the licence"
        );
        for crate_ in ABOUT_STACK {
            assert!(text.contains(crate_), "the About text omits `{crate_}`");
        }
        assert!(!ABOUT_NAME.is_empty() && !ABOUT_TAGLINE.is_empty());
    }

    /// Phase 7's seven commands, named one by one rather than left to the loop above, because the
    /// loop only sees the Keystore menu on the COSE tab and six of the seven are greyed there
    /// without a key and a document — and a *greyed* item is still an item with a handler to owe.
    #[test]
    fn the_keystore_items_have_handlers() {
        use gpui_kit::Action as _;

        let handled = handled_actions();
        for name in [
            actions::GenerateKey.name(),
            actions::DeleteKey.name(),
            actions::ImportKey.name(),
            actions::Sign.name(),
            actions::Verify.name(),
            actions::Encrypt.name(),
            actions::Decrypt.name(),
        ] {
            assert!(
                handled.contains(&name),
                "`{name}` has no handler; see the `.on_action` chain in `Render::render`"
            );
        }
    }

    /// The point of `cose::UnsupportedSign` being an enum of reviewed sentences: a key that cannot
    /// sign is refused with *its own reason*, where both old copies answered `InvalidKey` for all
    /// six cases and left the user to guess.
    #[test]
    fn an_unsupported_key_is_refused_with_its_own_reason() {
        let bytes =
            cose::keygen::generate_cose_key(cose::keygen::GenKeyKind::Ec2P521).expect("generates");
        let key = tps_cose::decode_cose_key(&bytes).expect("decodes");

        let reason = refusal(Operation::Sign, &key).expect("P-521 cannot sign here");
        assert_eq!(reason, cose::UnsupportedSign::Es512.to_string());
        assert!(reason.contains("P-521"), "{reason}");
        assert!(
            !reason.to_lowercase().contains("invalid key"),
            "the undifferentiated answer is what this replaces: {reason}"
        );
        // An asymmetric key cannot encrypt either, and that refusal is its own sentence too.
        let encrypt = refusal(Operation::Encrypt, &key).expect("P-521 cannot encrypt");
        assert_eq!(encrypt, cose::UnsupportedEncrypt::Asymmetric.to_string());
    }

    /// The companion case, and the reason `unsupported_sign_reason` is asked for `Verify` as well:
    /// it matches only on key type and curve, so a key the library *can* sign with is never
    /// refused — including for the two operations that only read.
    #[test]
    fn a_supported_key_is_refused_for_nothing_it_can_do() {
        let bytes = cose_key();
        let key = tps_cose::decode_cose_key(&bytes).expect("decodes");
        for op in [Operation::Sign, Operation::Verify] {
            assert_eq!(refusal(op, &key), None, "{op:?} with a P-256 key");
        }

        // Built rather than generated: the symmetric kinds are minted by
        // `KeyStore::generate_symmetric`, and a store means a directory on disk for a question
        // that is only about the key's type.
        let key = tps_cose::CoseKey::Symmetric(tps_cose::SymmetricKey::new(&[0u8; 16]));
        for op in [Operation::Encrypt, Operation::Decrypt] {
            assert_eq!(refusal(op, &key), None, "{op:?} with a symmetric key");
        }
    }

    /// The four operations read one document and write the other, and which is which is the
    /// decision taken with the user for this phase: a result goes where that *kind* of artefact
    /// belongs. Asserted as an inversion so the two tables cannot drift apart.
    #[test]
    fn an_operations_result_lands_on_the_other_tab() {
        for op in [
            Operation::Sign,
            Operation::Verify,
            Operation::Encrypt,
            Operation::Decrypt,
        ] {
            assert_ne!(
                op.input_tab(),
                op.result_tab(),
                "{op:?} would read and write the same document"
            );
        }
        assert_eq!(Operation::Sign.result_tab(), Tab::Cose);
        assert_eq!(Operation::Verify.result_tab(), Tab::Cbor);
        assert_eq!(Operation::Encrypt.result_tab(), Tab::Cose);
        assert_eq!(Operation::Decrypt.result_tab(), Tab::Cbor);
    }
}
