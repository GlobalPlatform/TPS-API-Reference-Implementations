/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The CDDL tab's editor and AST tree (`UI_DESIGN.md:101-117`).
//!
//! The largest tab, because it is the only one holding several files at once: `UI_DESIGN.md:108`
//! makes the CDDL tab the one place where *"multiple files to be opened"*, so that an
//! `;# import`/`;# include` chain can be read and edited where it is used.
//!
//! Modelled on [`crate::edn_view`], and every constraint recorded there applies here unchanged:
//! `set_value` emits no [`InputEvent::Change`]; **plain undo and redo emit no `Change` either**, so
//! every consumer of the text calls [`CddlView::flush`] first; `DiagnosticSet::reset` runs on every
//! mutation, so squiggles are re-published per change; read-only goes through the *element*
//! builder, because `Editor`'s render writes it into the state every frame; `context_menu`
//! *replaces* the built-in menu, so the clipboard items are restated; and the grammar name must be
//! exactly `"cddl"`, because lookup is case-sensitive and fails **silently**.
//!
//! ## One editor per inner tab
//!
//! [`CddlView::editors`] holds an [`EditorState`] per open file, keyed by that tab's path, rather
//! than one state re-pointed by `set_value` on each tab switch. The cheap design was rejected for
//! two reasons, neither cosmetic:
//!
//! * `set_value` clears the undo history and resets the selection and the scroll offset. Switching
//!   from `rfc8392.cddl` to `rfc9052.cddl` and back would lose the caret, the scroll position and
//!   the whole undo stack — on a tab bar whose only purpose is moving around a four-deep chain,
//!   that is the feature not working.
//! * A shared state makes `flush` **wrong by default**: it must write to the tab the editor last
//!   *showed*, not the selected one, or the switch itself writes the outgoing text into the
//!   incoming document and silently corrupts a file. That is a data-loss bug one line of
//!   inattention away, and no amount of care makes it structurally safe.
//!
//! A chain is four files, not four hundred, so the memory argument does not pay for either cost.
//! Keying on the path is sound because [`tps_cbor_core::CddlDocSet::open`] and `open_root` are the
//! only ways a tab enters the set and both dedupe canonically — pinned by that crate's
//! `every_inner_cddl_tab_has_a_distinct_path`, because the map is unsound without it. The one
//! consequence worth knowing: `Save As` changes a tab's path, so its editor is rebuilt from the
//! model on the next paint and that file's undo history ends there. The text is never at risk; the
//! model is written before any save.
//!
//! The states are created in [`CddlView::apply`], which runs from `render` — not in
//! [`CddlView::new`] — because [`EditorState::new`] needs a `&mut Window` and the paths arrive from
//! [`crate::Ide::open_path`], which has none.
//!
//! ## The AST tree
//!
//! `tps_cbor_core::cddl::ast_tree` does the work; this only renders it. Three decisions:
//!
//! * **Per file, not per chain.** `UI_DESIGN.md:117` asks for *"the AST for the currently selected
//!   tab"*, so the tree is built from that tab's own text and every span is file-local, which makes
//!   click-to-reveal trivial.
//! * **Rebuilt on demand, not per keystroke.** The AST is simply *absent* for most keystrokes —
//!   `a = ` does not parse, and mid-identifier `Claims-Se` is a different document — so a
//!   per-keystroke tree would blink empty constantly; a tree that reshapes under the typist is
//!   hostile to use; and no id scheme is stable across a reparse of changing text.
//!   `UI_DESIGN.md:115` ties the check to an explicit button anyway. When the buffer has moved on,
//!   [`CddlView::tree_is_stale`] says so and the right pane shows a line to that effect rather than
//!   a quietly wrong tree.
//! * **Expansion lives here.** `TreeState::set_items` rebuilds every `TreeItem` and expansion lives
//!   *in* the item, so it survives a rebuild only through [`CddlView::expanded`] —
//!   [`crate::explorer`] records the same finding. [`CddlView::seen`] is what makes the
//!   default-expand of the top two levels a *default* rather than a rule: it applies once per node
//!   id, so a node the user then collapses stays collapsed across every later rebuild.
//!
//! ## Two decoration layers per editor
//!
//! Three things draw on a CDDL editor, and they divide the way [`crate::edn_view`]'s do and for the
//! same reasons: the **module check's diagnostics** go into the one `DiagnosticSet`, while the
//! **rule-status tinting** ([`CddlView::publish_status`]) and the **jump highlight**
//! ([`CddlView::mark`]) get a `RangeDecorationCollection` each, held in [`Editing`] beside the
//! editor they belong to and created with it.
//!
//! Two layers rather than one list, because sharing one between two kinds is the bug
//! `tps_cbor_gui/PLAN.md:521-525` and `:574-577` record being introduced **twice** — whichever kind
//! wrote last erased the other. The order matters and is fixed at creation: `status` is created
//! first and `jump` second, because for *range* decorations later collections paint **over** earlier
//! ones (`gpui-base/src/input/editor/decorations.rs:500-502`), so a jump into a rule that also
//! carries a status tint is visible on top of it. Note that *text* decorations layer the opposite
//! way — the **first** collection wins (`:533-535`) — so the two APIs must not be reasoned about
//! together.
//!
//! Created once per editor and never per publish: a collection is released only by `dispose`, so a
//! fresh one each time would leak an id and a paint layer on every `Validate`.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::input::{
    Diagnostic as EditorDiagnostic, DiagnosticSeverity, RangeDecoration, RangeDecorationCollection,
    RangeDecorationStyle,
};
// Deliberately the *component* `NativeMenu`, for the reason `edn_view` records: two distinct types
// share the name and `Editor::context_menu` takes this one.
use gpui_kit::component::input::{Editor, EditorState, InputEvent, RopeExt as _};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tps_cbor_core::cddl::{self, AstRow, DirectiveRef, RuleStatus, TraceOutcome};
use tps_cbor_core::settings::Settings;
use tps_cbor_core::{Diagnostic, Severity, Workspace};

/// What the CDDL editor tells [`crate::Ide`].
///
/// Note what is *not* here: there is no "open this include" event. The gesture dispatches
/// [`crate::actions::OpenInclude`] instead, because the context menu is built from
/// `NativeMenu::menu`, which takes a `Box<dyn Action>` and not a closure — and because that keeps
/// the editor's menu item, the right pane's rows and any future keybinding one command rather than
/// two routes that can disagree. The caret is then read back through
/// [`CddlView::caret_directive`], so there is one answer to "which directive?".
pub enum CddlEvent {
    /// The selected file was edited. The syntax check, the explorer's dirty set and the `Save` menu
    /// item all have to be re-derived.
    Edited,
    /// A write to the model was refused. Unreachable while the element is rendered `readonly` on a
    /// read-only document — which is precisely why it is reported rather than discarded: if it ever
    /// fires, the element and the document have disagreed about who may type.
    Failed(String),
}

/// One inner tab's editing state, its two decoration layers, and the subscription that outlives it
/// by exactly as long.
///
/// The subscription is held *beside* the state rather than in a shared `Vec` so that dropping an
/// editor — which happens whenever a tab closes or is replaced — drops its `InputEvent` listener
/// with it. A shared vector would keep listening to a state nothing renders. The collections are
/// held here for the same reason: they belong to one `EditorState` and are meaningless without it.
struct Editing {
    state: Entity<EditorState>,
    /// The rule-status tinting. Created *before* `jump`; see the module docs on layer order.
    status: RangeDecorationCollection,
    /// The jump highlight, painted over `status`.
    jump: RangeDecorationCollection,
    _subscription: Subscription,
}

/// The CDDL tab's upper window.
pub struct CddlView {
    /// One editor per inner tab, keyed by that tab's path. See the module docs.
    editors: HashMap<PathBuf, Editing>,
    /// The shared model, held as an entity for the reason [`crate::hex_view`] records.
    workspace: Entity<Workspace>,
    /// The settings, for the module search path and the validation config.
    ///
    /// A clone rather than a borrow of [`crate::Ide`]'s copy, and the whole struct rather than the
    /// two fields it needs, because `Settings::cddl_config` returns a `tps_cddl::Config` and this
    /// crate cannot *name* that type — it can only produce one and pass it straight on.
    ///
    /// It *is* a snapshot and it **can** go stale, which it could not until Phase 8 gave the right
    /// pane controls for `rfc9165`, `rfc9682` and the module search paths. The one way to refresh
    /// it is [`CddlView::set_settings`], which `Ide` calls on every change to those three; this
    /// field being private is what keeps that the only way.
    settings: Settings,
    /// The AST tree's own state.
    tree: Entity<TreeState>,
    /// Node ids the user has expanded, or that were default-expanded. See the module docs.
    expanded: HashSet<SharedString>,
    /// Node ids that have appeared in a tree at least once, so the default-expand applies once.
    seen: HashSet<SharedString>,
    /// Every row of the current tree, by node id, for the render closure and for click-to-reveal.
    ///
    /// An `Rc` because the closure the tree renders rows from outlives `render` and so cannot
    /// borrow `self`; a map rather than the `Vec` [`cddl::ast_tree::flatten`] returns because the closure is
    /// handed a `TreeEntry` and has only its id to look the row up by.
    rows: Rc<HashMap<SharedString, AstRow>>,
    /// Which file the current tree was built from, and the text it was built from the length of.
    ///
    /// Both halves are needed to answer "is this tree still about what is on screen?": the file,
    /// because the tree is per tab, and the text, because an edit invalidates every span in it.
    tree_source: Option<(PathBuf, String)>,
    /// Per-file diagnostics to draw inline, as [`CddlView::publish`] last left them.
    inline: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Per-file rule-status spans to tint, as [`CddlView::publish_status`] last left them.
    ///
    /// In each file's own coordinates — `cddl::ChainTrace::rule_status_by_file` does the rebasing —
    /// because that is the only space an editor can index. A file absent from the map, and a rule
    /// absent from its list, are both tinted nothing: the user's decision, recorded in `PLAN.md`.
    status: HashMap<PathBuf, Vec<RuleStatus>>,
    /// The jump highlight: which file, and where in it. At most one, because it is the answer to
    /// one click, and a second one left behind in another file would read as two answers.
    mark: Option<(PathBuf, Range<usize>)>,
    /// Per-file `;#` directives and their resolved targets, for the context menu and the right
    /// pane's clickable list.
    links: HashMap<PathBuf, Rc<Vec<DirectiveRef>>>,
    /// Whether [`CddlView::apply`] has work to do on the next paint.
    pending: bool,
    /// Whether the editors have yet to be given the model's text.
    ///
    /// [`CddlView::flush`] refuses to write back while it is set, for the reason
    /// [`crate::edn_view`] records: a `Save` landing between a [`CddlView::reload`] and the paint
    /// that honours it would push the previous chain's text over the one just opened.
    stale: bool,
    /// A [`CddlView::reveal`] asked for before the editor it is about existed.
    ///
    /// Set only when a tab was opened in the same update that revealed into it; drained by the
    /// next [`CddlView::apply`], which is the paint that builds that tab's editor.
    deferred_reveal: Option<usize>,
    /// A handle to focus when no file is open, so the tab is never without a focus target.
    focus: FocusHandle,
    /// This view's own entity, recorded at the first paint.
    ///
    /// The context-menu closure has to read the caret back out of the selected editor, which means
    /// reaching `self` from a `'static` closure. `cx.entity()` cannot be called from `new` — the
    /// entity does not exist until the constructor returns — so it is picked up in `render`, which
    /// always runs before a menu can be opened.
    entity: Option<Entity<Self>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CddlEvent> for CddlView {}

impl CddlView {
    /// Needs no `&mut Window`, unlike [`crate::edn_view::EdnView::new`]: the editors are built in
    /// [`CddlView::apply`] as tabs appear, and `TreeState::new` takes only a context.
    pub fn new(workspace: Entity<Workspace>, settings: Settings, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let subscriptions =
            vec![
                cx.subscribe(&tree, |this, _, event: &TreeEvent, _| match event {
                    TreeEvent::Expanded(id) => {
                        this.expanded.insert(id.clone());
                    }
                    TreeEvent::Collapsed(id) => {
                        this.expanded.remove(id);
                    }
                }),
            ];

        Self {
            editors: HashMap::new(),
            workspace,
            settings,
            tree,
            expanded: HashSet::new(),
            seen: HashSet::new(),
            rows: Rc::default(),
            tree_source: None,
            inline: HashMap::new(),
            status: HashMap::new(),
            mark: None,
            links: HashMap::new(),
            // A file may already be open — `main` replays the command line before the first frame
            // — so the first paint has to pick the model up.
            pending: true,
            stale: true,
            deferred_reveal: None,
            focus: cx.focus_handle(),
            entity: None,
            _subscriptions: subscriptions,
        }
    }

    /// Focuses the selected inner tab's editor, so that the keyboard reaches it.
    ///
    /// Silently does nothing with no file open, which is the only state in which the CDDL tab has
    /// no editor at all; [`crate::Ide::select`] keeps the window's own handle focused in that case.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.active_state(cx) {
            let handle = state.read(cx).focus_handle(cx).clone();
            window.focus(&handle, cx);
        }
    }

    /// Replaces the settings snapshot, after the right pane has changed one of the three it offers.
    ///
    /// Only the value, and a notify. In particular **not** [`CddlView::stale`], which reads as the
    /// obvious thing to set here and is the wrong flag entirely: it means *the editors have yet to
    /// be given the model's text*, and [`CddlView::flush`] refuses to write back while it is set —
    /// so setting it on a dialect change would strand whatever the user had typed in the editors
    /// and then discard it at the next reload.
    ///
    /// The AST tree does need rebuilding, since [`CddlView::rebuild_tree`] parses through
    /// `Settings::cddl_config` and a dialect change can turn a parse that succeeded into one that
    /// fails. That is the caller's job and [`crate::Ide::apply_cddl_settings`] does it by re-running
    /// the syntax check, which is the same route the `Syntax Check` button takes. It is not done
    /// here because the per-file and chain-wide passes it needs live on `Ide`, not on this view.
    pub fn set_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        self.settings = settings;
        cx.notify();
    }

    /// Asks for the model's text to be pushed into the editors at the next paint, for a chain that
    /// has been replaced or extended.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.pending = true;
        self.stale = true;
        cx.notify();
    }

    /// Replaces the diagnostics drawn inline in `path`'s editor.
    ///
    /// Per file rather than for the whole chain, because only the selected tab can have been
    /// edited: rechecking the others on every keystroke would parse three files to confirm they
    /// have not changed.
    ///
    /// Re-published on every change rather than pushed once, because every text edit calls
    /// `DiagnosticSet::reset`, which empties it.
    pub fn publish(&mut self, path: PathBuf, diagnostics: Vec<Diagnostic>, cx: &mut Context<Self>) {
        self.inline.insert(path, diagnostics);
        self.pending = true;
        cx.notify();
    }

    /// Replaces the rule-status tinting across the whole chain.
    ///
    /// The whole chain in one call, not per file like [`CddlView::publish`], because a trace is one
    /// answer about one merged schema: publishing per file would leave the last run's tint on any
    /// file the new run did not reach, which is exactly the stale-highlight failure the phase's
    /// honesty rule exists to prevent. An empty map therefore *clears* every file.
    ///
    /// A port of `DebugPanel.tsx:42-80`'s `collectRuleSpans`/`highlightsFromTrace`, with the
    /// fail-beats-match rule now living in `Trace::rule_status` where it is tested.
    pub fn publish_status(
        &mut self,
        status: HashMap<PathBuf, Vec<RuleStatus>>,
        cx: &mut Context<Self>,
    ) {
        self.status = status;
        self.pending = true;
        cx.notify();
    }

    /// Replaces the jump highlight, or clears it when `target` is `None`.
    ///
    /// The counterpart of [`crate::hex_view::HexView::mark`] and
    /// [`crate::edn_view::EdnView::mark`]: one trace-row click reaches all three.
    pub fn mark(&mut self, target: Option<(PathBuf, Range<usize>)>, cx: &mut Context<Self>) {
        if self.mark == target {
            return;
        }
        self.mark = target;
        self.pending = true;
        cx.notify();
    }

    /// Records `path`'s `;#` directives, for the context menu and the right pane's list.
    pub fn publish_links(
        &mut self,
        path: PathBuf,
        directives: Vec<DirectiveRef>,
        cx: &mut Context<Self>,
    ) {
        self.links.insert(path, Rc::new(directives));
        cx.notify();
    }

    /// Writes every editor's text into its *own* inner tab, where the two differ.
    ///
    /// `Ok(true)` means at least one document was updated. Called from the `Change` handler *and*
    /// from every consumer of the text — save, `Syntax Check`, the quit guard — because undo and
    /// redo emit no `Change`; see the module docs.
    ///
    /// Every tab is attempted even after one write is refused, so a single read-only file in the
    /// chain does not strand the others' edits in the editor where a save cannot see them. The
    /// first refusal is what comes back.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> tps_cbor_core::Result<bool> {
        // A pending [`CddlView::reload`] means the model has been replaced and the editors have not
        // caught up, so they are not authoritative and writing back would push the old chain's text
        // over the new one.
        if self.stale {
            return Ok(false);
        }

        let mut writes: Vec<(usize, String)> = Vec::new();
        for (index, tab) in self.workspace.read(cx).cddl.tabs().iter().enumerate() {
            let Some(editing) = tab.meta.path.as_deref().and_then(|p| self.editors.get(p)) else {
                continue;
            };
            let value = editing.state.read(cx).value();
            if tab.text() != value.as_ref() {
                writes.push((index, value.to_string()));
            }
        }
        if writes.is_empty() {
            return Ok(false);
        }

        let outcome = self.workspace.update(cx, |workspace, cx| {
            let mut first = Ok(());
            for (index, text) in writes {
                // `tab_mut` rather than `select` then `active_mut`: writing a tab must not move the
                // selection the user left behind. See `CddlDocSet::tab_mut`.
                if let Some(tab) = workspace.cddl.tab_mut(index) {
                    let outcome = tab.edit(text);
                    if first.is_ok() {
                        first = outcome;
                    }
                }
            }
            cx.notify();
            first
        });
        outcome.map(|()| true)
    }

    /// Rebuilds the AST tree from the selected tab's text (`UI_DESIGN.md:117`).
    ///
    /// Called on open, on an inner-tab switch and from `Syntax Check`, and not per keystroke; the
    /// module docs give the three reasons. A file that does not parse yields an empty tree and the
    /// parse error is already in the lower window, so nothing is reported twice here.
    pub fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let Some((path, text)) = self.active_text(cx) else {
            self.rows = Rc::default();
            self.tree_source = None;
            self.tree
                .update(cx, |state, cx| state.set_items(Vec::new(), cx));
            cx.notify();
            return;
        };

        let rows = match cddl::check(&text, &self.settings.cddl_config()).cddl {
            Some(parsed) => cddl::ast_tree::flatten(&cddl::ast_tree::build_tree(&parsed), &text),
            None => Vec::new(),
        };

        // Before the items are built, because the default is expressed *through* `expanded`: a node
        // seen for the first time at depth 0 or 1 starts open, and from then on only the user's own
        // expand and collapse events move it.
        default_expand(&rows, &mut self.seen, &mut self.expanded);

        let items = tree_items(&rows, 0, &mut 0, &self.expanded);
        self.rows = Rc::new(
            rows.into_iter()
                .map(|row| (SharedString::from(row.id.clone()), row))
                .collect(),
        );
        self.tree_source = Some((path, text));
        self.tree.update(cx, |state, cx| state.set_items(items, cx));
        cx.notify();
    }

    /// Whether the tree describes text the user has since changed.
    ///
    /// Drives the right pane's *"stale — re-run Syntax Check"* line. Compares the text itself
    /// rather than tracking a dirty flag, so an edit that is undone stops being stale again.
    pub fn tree_is_stale(&self, cx: &App) -> bool {
        match (&self.tree_source, self.active_text(cx)) {
            (Some((built_path, built_text)), Some((path, text))) => {
                *built_path != path || *built_text != text
            }
            // With no tree there is nothing to *be* stale: [`CddlView::ast_tree`] already says
            // "run Syntax Check", and a "stale — re-run Syntax Check" line above it says the same
            // thing again in different words, which reads as two separate problems.
            (None, _) => false,
            // A tree for a file that is no longer the selected one describes something the pane is
            // not showing, so it is stale in the only sense that matters here.
            (Some(_), None) => true,
        }
    }

    /// Puts the caret at `offset` in the selected tab's editor and scrolls it into view.
    ///
    /// The target of both a clickable diagnostic row and a click on an AST node.
    ///
    /// Held over to the next paint when the tab has no editor yet, which happens for exactly one
    /// caller and is the one that matters most: a diagnostic row whose file was not open until the
    /// click itself opened it ([`crate::Ide::reveal_diagnostic`]). `EditorState::new` needs a
    /// `&mut Window`, so a tab opened mid-update has no editor until [`CddlView::apply`] runs, and
    /// a reveal dropped there would land the user on line 1 of the file they just opened.
    pub fn reveal(&mut self, offset: usize, cx: &mut Context<Self>) {
        if !self.place_caret(offset, cx) {
            self.deferred_reveal = Some(offset);
            cx.notify();
        }
    }

    /// Puts the caret at `offset` in the selected tab's editor, reporting whether there was one.
    fn place_caret(&mut self, offset: usize, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.active_state(cx) else {
            return false;
        };
        state.update(cx, |state, cx| {
            state.set_selected_range(offset..offset, cx);
        });
        true
    }

    /// The `;#` directive the caret currently sits on, if any (`UI_DESIGN.md:108`).
    ///
    /// Read from the *editor*, not the model, so it answers for the line on screen even before a
    /// flush. The target is `None` for a directive that does not resolve, which is what greys the
    /// context-menu item out rather than offering to open a file that is not there.
    pub fn caret_directive(&self, cx: &App) -> Option<DirectiveRef> {
        let path = self.active_path(cx)?;
        let state = self.editors.get(&path)?.state.read(cx);
        let offset = state.selected_range().start;
        cddl::chain::directive_at(&state.value(), offset, &self.search_paths(&path))
    }

    /// The selected tab's directives, for the right pane's clickable list.
    pub fn directives(&self, cx: &App) -> Rc<Vec<DirectiveRef>> {
        self.active_path(cx)
            .and_then(|path| self.links.get(&path).cloned())
            .unwrap_or_default()
    }

    /// The module search path to use for `path`: the file's own directory, then the settings' list.
    ///
    /// The prepended parent is not optional. `tps_cddl::ModuleSearchPath::resolve` iterates only
    /// the directories it was given and never the including file's own, so without this line a
    /// `;# include rfc9052` beside the file that names it does not resolve at all — which on a
    /// fresh settings file is every chain in the corpus.
    pub fn search_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
        paths.extend(self.settings.module_search_paths.iter().cloned());
        paths
    }

    /// The AST tree, as an element (`UI_DESIGN.md:117`).
    ///
    /// An associated function taking the entity rather than a method, because it is rendered by
    /// [`crate::Ide`]'s right pane — where only a `Context<Ide>` exists — while the tree state and
    /// the rows live here. The click closure captures the entity so that it can call
    /// [`CddlView::reveal`] on the editor the row belongs to.
    pub fn ast_tree(view: &Entity<Self>, cx: &App) -> AnyElement {
        let this = view.read(cx);
        let state = this.tree.clone();
        if this.rows.is_empty() {
            return div()
                .text_color(cx.theme().muted_foreground)
                .child("No AST — run `Syntax Check`.")
                .into_any_element();
        }

        // Cloned per frame rather than per row: the closure outlives this call, so it cannot borrow
        // `self`, and an `Rc` bump is the cheapest way to share one row table with every row.
        let rows = this.rows.clone();
        let entity = view.clone();

        tree(&state, move |ix, entry, _selected, _, cx| {
            let item = entry.item();
            let id = item.id.clone();
            let Some(row) = rows.get(&id) else {
                return ListItem::new(ix).child(item.label.clone());
            };

            // The row owns its indentation and its chevron: the base tree renders a flat list and
            // passes the depth through, drawing neither (`explorer`'s module docs, point 2).
            let chevron = match (entry.is_folder(), entry.is_expanded()) {
                (true, true) => Some(IconName::ChevronDown),
                (true, false) => Some(IconName::ChevronRight),
                _ => None,
            };

            let content = h_flex()
                .gap_1()
                .pl(px(entry.depth() as f32 * 12.0))
                .child(match chevron {
                    Some(chevron) => Icon::new(chevron)
                        .size_3()
                        .text_color(cx.theme().muted_foreground)
                        .into_any_element(),
                    None => div().size_3().into_any_element(),
                })
                .child(div().child(SharedString::from(row.kind)))
                .when_some(row.label.clone(), |this, label| {
                    this.child(div().font_weight(FontWeight::BOLD).child(label))
                })
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}:{}", row.start.line, row.start.column)),
                );

            let entity = entity.clone();
            let offset = row.span.start;
            // Ours rather than the tree's: `TreeEvent` carries only `Expanded`/`Collapsed`
            // (`explorer`'s module docs, point 1), so there is no selection event to listen for.
            // The base tree's own handler still runs afterwards, so expanding a folder row and
            // revealing its span are not alternatives.
            ListItem::new(ix).child(content).on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| this.reveal(offset, cx));
            })
        })
        .into_any_element()
    }

    /// The selected tab's path, if a file is open.
    fn active_path(&self, cx: &App) -> Option<PathBuf> {
        self.workspace
            .read(cx)
            .cddl
            .active()
            .and_then(|tab| tab.meta.path.clone())
    }

    /// The selected tab's path and the *editor's* text — which is what is on screen, and so what
    /// the tree and the directive scan must be built from.
    fn active_text(&self, cx: &App) -> Option<(PathBuf, String)> {
        let path = self.active_path(cx)?;
        match self.editors.get(&path) {
            Some(editing) => Some((path, editing.state.read(cx).value().to_string())),
            // Before the first paint there is no editor yet, so the model is the best answer.
            None => {
                let text = self.workspace.read(cx).cddl.active()?.text().to_owned();
                Some((path, text))
            }
        }
    }

    /// The selected inner tab's editor, if there is one.
    ///
    /// Public because [`crate::Ide`] forwards Find, Replace, Copy and Paste to it. `None` with no
    /// file open is the one state this tab has that the others do not — it holds a *set* of
    /// documents (`UI_DESIGN.md:108`) — and it is why the forwarding handlers take an `Option`.
    pub fn active_state(&self, cx: &App) -> Option<Entity<EditorState>> {
        let path = self.active_path(cx)?;
        self.editors.get(&path).map(|editing| editing.state.clone())
    }

    /// Reconciles the editors with the open tabs, then pushes text and diagnostics into them.
    ///
    /// Runs from `render`, which is the only place a `&mut Window` is reliably to hand — and
    /// [`EditorState::new`] needs one. See the module docs.
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tabs: Vec<(PathBuf, String)> = self
            .workspace
            .read(cx)
            .cddl
            .tabs()
            .iter()
            .filter_map(|tab| {
                tab.meta
                    .path
                    .clone()
                    .map(|path| (path, tab.text().to_owned()))
            })
            .collect();

        // A closed or replaced tab's editor goes with it, and so does its subscription.
        let live: HashSet<&PathBuf> = tabs.iter().map(|(path, _)| path).collect();
        self.editors.retain(|path, _| live.contains(path));

        for (path, text) in &tabs {
            if !self.editors.contains_key(path) {
                // Exactly `"cddl"`: grammar lookup is case-sensitive and un-normalized, and a name
                // that matches nothing yields no highlighting and no error at all.
                let state = cx.new(|cx| EditorState::new(window, cx).language("cddl"));
                // Both empty, and in this order: `status` must be the earlier collection so that
                // `jump` paints over it. See the module docs.
                let (status, jump) = state.update(cx, |state, cx| {
                    (
                        state.create_range_decorations_collection(Vec::new(), cx),
                        state.create_range_decorations_collection(Vec::new(), cx),
                    )
                });
                let subscription = cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_change(cx);
                    }
                });
                self.editors.insert(
                    path.clone(),
                    Editing {
                        state,
                        status,
                        jump,
                        _subscription: subscription,
                    },
                );
            }

            let state = self.editors[path].state.clone();
            // Guarded on a difference because `set_value` discards the undo history and the caret:
            // an edit the user just made, round-tripped through `flush`, must not throw those away.
            // It is also what keeps a diagnostic-only `publish` from resetting every open editor.
            if state.read(cx).value().as_ref() != text.as_str() {
                state.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
            }

            let diagnostics = self.inline.get(path).cloned().unwrap_or_default();
            state.update(cx, |state, cx| {
                // Cloned out first: `diagnostics_mut` takes `&mut state` and the conversion needs
                // the rope at the same time. A `Rope` clone shares its tree rather than the text.
                let rope = state.text().clone();
                let Some(set) = state.diagnostics_mut() else {
                    return;
                };
                set.reset(&rope);
                for diagnostic in diagnostics {
                    let span = squiggle_span(&diagnostic.span, text);
                    let range =
                        rope.offset_to_position(span.start)..rope.offset_to_position(span.end);
                    set.push(
                        EditorDiagnostic::new(range, diagnostic.message.clone())
                            // `DiagnosticSeverity::Hint` is the `Default`, so the severity is set
                            // explicitly or an error is drawn as a hint.
                            .with_severity(severity_of(diagnostic.severity)),
                    );
                }
                cx.notify();
            });

            // The colours are resolved here rather than per frame because
            // `RangeDecorationCollection::set` notifies unconditionally, so a `set` from `render`
            // would schedule the next frame for ever. Safe because `crate::theme::init` fixes the
            // mode at startup and there is no in-app switch; adding one means re-applying here.
            let theme = cx.theme();
            let (success, danger, info) = (theme.success, theme.danger, theme.info);
            let tints: Vec<RangeDecoration> = self
                .status
                .get(path)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|(span, outcome)| {
                    Some(fill(span.clone(), status_fill(*outcome, success, danger)?))
                })
                .collect();
            let jumps: Vec<RangeDecoration> = match &self.mark {
                Some((marked, span)) if marked == path => {
                    vec![fill(span.clone(), info.opacity(JUMP_OPACITY))]
                }
                // Empty for every other file, including when a mark exists in one of them: a
                // highlight left behind in a tab the user is about to switch to is a confident
                // answer to a question they have since asked somewhere else.
                _ => Vec::new(),
            };

            let editing = &self.editors[path];
            // Cloned because `set` needs `cx` mutably while `self.editors` is borrowed; a clone
            // addresses the same collection (`decorations.rs:154`), so this is a handle copy and
            // not a second layer.
            let (status, jump) = (editing.status.clone(), editing.jump.clone());
            status.set(tints, cx);
            jump.set(jumps, cx);
        }

        self.stale = false;

        // The editors exist now, so a reveal that arrived before them can land. Called directly
        // rather than through [`CddlView::reveal`], which would re-defer and notify again — and so
        // spin a repaint loop — if the tab it names has somehow gone away in the meantime.
        if let Some(offset) = self.deferred_reveal.take() {
            self.place_caret(offset, cx);
        }
    }

    /// The model is only as current as the last `Change`, so this writes through before announcing
    /// the edit; a refusal is surfaced rather than swallowed, for the reason [`CddlEvent::Failed`]
    /// records.
    fn on_change(&mut self, cx: &mut Context<Self>) {
        match self.flush(cx) {
            Ok(_) => cx.emit(CddlEvent::Edited),
            Err(error) => cx.emit(CddlEvent::Failed(error.to_string())),
        }
    }

    /// `UI_DESIGN.md:108`'s clickable include, plus the clipboard items the custom menu has to
    /// restate.
    ///
    /// The include item is the gesture the spec points at: the `Editor` element has no click or
    /// hover hook of its own — a left click on these lines can only move the caret, and must, since
    /// they stay editable CDDL text — so the right-click menu is where "open this" can live. The
    /// right pane lists the same directives as always-clickable rows, because a gesture only
    /// reachable by right-clicking the correct line is not discoverable.
    fn context_menu(&self) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
        let entity = self.entity.clone();
        let workspace = self.workspace.clone();
        move |menu, _, cx| {
            let editable = !workspace
                .read(cx)
                .cddl
                .active()
                .is_some_and(|tab| tab.meta.read_only);
            let directive = entity
                .as_ref()
                .and_then(|entity| entity.read(cx).caret_directive(cx));
            let selected = entity
                .as_ref()
                .and_then(|entity| entity.read(cx).active_state(cx))
                .is_some_and(|state| !state.read(cx).selected_range().is_empty());

            let menu = menu
                .menu_with_disabled(
                    "Cut",
                    !(editable && selected),
                    Box::new(gpui_kit::base::input::Cut),
                )
                .menu_with_disabled("Copy", !selected, Box::new(gpui_kit::base::input::Copy))
                // Offered on the strength of the read-only flag alone, as the built-in menu is:
                // the synchronous clipboard read is empty on some platforms, and pasting nothing
                // is harmless.
                .menu_with_disabled("Paste", !editable, Box::new(gpui_kit::base::input::Paste))
                .menu("Select All", Box::new(gpui_kit::base::input::SelectAll));

            // Listed only when the caret is on a directive, rather than always-but-greyed: a
            // permanently greyed item on every other line reads as broken, and the right pane
            // carries the always-available version of this gesture. A directive that does not
            // resolve *is* shown greyed, with the module name, because there the user has asked
            // about a specific line and "that include is not findable" is the answer.
            match directive {
                Some(directive) => menu.separator().menu_with_disabled(
                    format!("Open included file {}", directive.module),
                    directive.target.is_none(),
                    Box::new(crate::actions::OpenInclude),
                ),
                None => menu,
            }
        }
    }
}

impl Focusable for CddlView {
    /// Falls back to the view's own handle with no file open, which is the one state in which there
    /// is no editor to hand the keyboard to.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_state(cx) {
            Some(state) => state.read(cx).focus_handle(cx).clone(),
            None => self.focus.clone(),
        }
    }
}

impl Render for CddlView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Recorded here rather than in `new` because `cx.entity()` is not available during
        // construction: the entity does not exist until the constructor returns. The context menu
        // closure needs it to read the caret back out of the editor.
        if self.entity.is_none() {
            self.entity = Some(cx.entity());
        }

        // Before the element is built, so the frame about to be painted shows the new text rather
        // than lagging it by one. The flag is cleared first: `apply` notifies, and leaving it set
        // would make that notification schedule another identical frame, for ever.
        if std::mem::take(&mut self.pending) {
            self.apply(window, cx);
        }

        let Some(state) = self.active_state(cx) else {
            // No file open. `Ide::tab_view` shows its own placeholder in that case, so this is
            // only reachable transiently, between a close and the paint that follows it.
            return div().track_focus(&self.focus).into_any_element();
        };

        let read_only = self
            .workspace
            .read(cx)
            .cddl
            .active()
            .is_some_and(|tab| tab.meta.read_only);

        Editor::new(&state)
            .h(relative(1.))
            // Through the element, never `EditorState`: `Editor`'s render writes this into the
            // state every frame, so a value set on the state directly is overwritten before it is
            // ever read.
            .readonly(read_only)
            // The pane supplies the border and the background; a second bordered box inside the
            // tab view would draw a frame within a frame.
            .appearance(false)
            .bordered(false)
            .context_menu(self.context_menu())
            .into_any_element()
    }
}

/// The span to *draw* a diagnostic over, which is not always the span it carries.
///
/// `tps_cddl` reports an unexpected token as the position where it expected something else, not as
/// a range: ` @@@` typed after a rule comes back as `50..50`, which is the start of the *following*
/// line — the point at which an assignment operator was due. A zero-width range draws a
/// zero-width squiggle, so until this existed a typed syntax error produced a row in the lower
/// window and no mark in the editor at all, which is the one place the user is looking.
///
/// One character is enough to be seen, and which one depends on where the position sits:
///
/// * At the start of a line, reach *backwards* past the line break to the end of the line before.
///   A zero-width "expected an assignment operator" at column 1 means something is missing
///   *before* here, and marking the following line accuses the rule the user did not touch. This
///   is the case the real parse error produces, and `rustc` reads it the same way.
/// * At the end of a line or of the file, reach backwards for the same reason.
/// * Anywhere else, mark the character at the position.
///
/// `0..0` is left alone. It is the project's "no position to offer" convention
/// (`tps_cbor_core::cddl::validate::diagnostic_for` uses it for every error with no CDDL span), and
/// a mark on the first character of the file would claim a location the diagnostic does not have.
///
/// Byte offsets are stepped by whole characters throughout: `offset_to_position` indexes a rope by
/// byte, and a range splitting a multi-byte character would panic on the first non-ASCII CDDL
/// comment — which the corpus has.
fn squiggle_span(span: &Range<usize>, text: &str) -> Range<usize> {
    if span.start != span.end || span.start == 0 {
        return span.clone();
    }

    let at = span.start.min(text.len());
    let starts_a_line = matches!(text[..at].chars().next_back(), Some('\n' | '\r'));
    if !starts_a_line {
        match text[at..].chars().next() {
            Some(next) if next != '\n' && next != '\r' => return at..at + next.len_utf8(),
            _ => {}
        }
    }

    // Backwards, skipping the line break itself: a squiggle on a newline has nothing to cover.
    let mut end = at;
    for previous in text[..at].chars().rev() {
        if previous == '\n' || previous == '\r' {
            end -= previous.len_utf8();
        } else {
            return end - previous.len_utf8()..end;
        }
    }
    at..at
}

/// How strongly a rule-status tint is painted over the text.
///
/// Low, and deliberately: these cover whole rule definitions — a `Claims-Set` body runs to dozens
/// of lines — and a tint strong enough to read as a highlight makes the syntax colouring underneath
/// it unreadable. The job is to let the eye find the failed rule in a 17 KB schema, not to replace
/// the editor's own palette.
const STATUS_OPACITY: f32 = 0.18;

/// How strongly the jump highlight is painted. Stronger than [`STATUS_OPACITY`], because it answers
/// a click the user has just made and has to be findable over a schema that is already tinted.
const JUMP_OPACITY: f32 = 0.25;

/// A filled range decoration in `colour`.
///
/// `Fill` rather than the default `Frame`: these mark *regions* of text, and a one-pixel box around
/// a multi-line rule definition reads as a border on the editor rather than as a mark on the rule.
fn fill(range: Range<usize>, colour: Hsla) -> RangeDecoration {
    RangeDecoration::new(range)
        .with_style(RangeDecorationStyle::Fill)
        .with_color(colour)
}

/// The tint for one rule-status outcome, or `None` for an outcome that gets none.
///
/// Two outcomes get no tint, for two different reasons:
///
/// * **`Attempt`** — an attempt is immediately followed by its own outcome, so tinting it would
///   paint every rule the trace entered regardless of how it went. `Trace::rule_status` already
///   drops these; this is the second guard, because a tint that says nothing is worse than none.
/// * **A rule the trace never reached** is *absent* from the status list rather than being an
///   outcome here at all — the user's decision, recorded in `PLAN.md` against the old frontend's
///   grey. Tinting ~200 untouched blocks over `gp_eap.cddl` would make the editor unreadable in
///   order to say something the trace log already says.
///
/// Takes the two colours rather than a theme so that the mapping is testable without a window.
fn status_fill(outcome: TraceOutcome, success: Hsla, danger: Hsla) -> Option<Hsla> {
    match outcome {
        TraceOutcome::Match => Some(success.opacity(STATUS_OPACITY)),
        TraceOutcome::Fail => Some(danger.opacity(STATUS_OPACITY)),
        TraceOutcome::Attempt => None,
    }
}

/// The editor's severity for one of the core crate's three.
///
/// A free function so that the mapping is testable without a window. It exists at all because
/// `DiagnosticSeverity` defaults to `Hint`, so a missing or wrong arm here is the difference
/// between a red squiggle and a grey one. The two enums happen to agree variant for variant --
/// `DiagnosticSeverity` has `Hint` as well, which nothing here produces -- so this is a rename
/// rather than a decision.
fn severity_of(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::Error,
        Severity::Warning => DiagnosticSeverity::Warning,
        Severity::Info => DiagnosticSeverity::Info,
    }
}

/// Opens the top two levels of a freshly built tree, once per node.
///
/// Follows `tps_cbor_gui/src/ParseTree.tsx`, which expanded the same two levels: the rules of a
/// schema are what a reader wants to see, and their innards are not.
///
/// `seen` is what makes this a *default* rather than a rule. `TreeState::set_items` rebuilds every
/// item, so expansion can only come from `expanded` — and if the depth test were applied on every
/// rebuild, a node the user collapsed would spring open again at the next `Syntax Check`. Gating on
/// first sight means the default applies once and the user's own expand and collapse events own it
/// from then on.
///
/// A free function, and `&mut` rather than returning a set, so that both halves of that contract
/// can be asserted without a window; see `the_tree_default_expands_to_depth_two`.
fn default_expand(
    rows: &[AstRow],
    seen: &mut HashSet<SharedString>,
    expanded: &mut HashSet<SharedString>,
) {
    for row in rows {
        let id = SharedString::from(row.id.clone());
        if seen.insert(id.clone()) && row.depth < 2 {
            expanded.insert(id);
        }
    }
}

/// Rebuilds the nesting [`cddl::ast_tree::flatten`] flattened, as [`TreeItem`]s.
///
/// `flatten` returns a depth-first pre-order list with a depth per row, which is exactly enough to
/// recover the shape: every row after a given one that is *deeper* is one of its descendants. Doing
/// it this way rather than walking `CddlNode` again keeps the node-id scheme in one place —
/// `ast_tree`, where it is documented and tested — instead of reimplementing it here, where it
/// could drift and silently lose every expansion.
fn tree_items(
    rows: &[AstRow],
    depth: usize,
    cursor: &mut usize,
    expanded: &HashSet<SharedString>,
) -> Vec<TreeItem> {
    let mut items = Vec::new();
    while let Some(row) = rows.get(*cursor) {
        if row.depth != depth {
            break;
        }
        *cursor += 1;
        let children = tree_items(rows, depth + 1, cursor, expanded);
        let id = SharedString::from(row.id.clone());
        let label = row.label.clone().unwrap_or_else(|| row.kind.to_owned());
        items.push(
            TreeItem::new(id.clone(), label)
                .children(children)
                .expanded(expanded.contains(&id)),
        );
    }
    items
}

#[cfg(test)]
mod tests {
    // Imported item by item rather than with `use super::*`, which `gpui_kit`'s own `lib.rs:98-99`
    // asks for: the glob would pull GPUI's `#[test]` attribute macro in through this module's
    // `use gpui_kit::*`, shadow the standard one, and expand to itself until the recursion limit
    // stops the compiler. `edn_view` carries the same note.
    use super::{
        JUMP_OPACITY, STATUS_OPACITY, default_expand, fill, severity_of, squiggle_span,
        status_fill, tree_items,
    };
    use gpui_kit::SharedString;
    use gpui_kit::base::input::{DiagnosticSeverity, RangeDecorationStyle};
    use gpui_kit::component::tree::TreeItem;
    use gpui_kit::{Hsla, hsla};
    use std::collections::HashSet;
    use tps_cbor_core::Severity;
    use tps_cbor_core::cddl::{self, AstRow, TraceOutcome};
    use tps_cbor_core::settings::Settings;

    /// Two rules, one of them nested deeply enough to have rows at depth 2 and below — which is
    /// what the default-expand rule has to *not* open.
    const SCHEMA: &str = "a = uint\nb = { x: uint, y: tstr }\n";

    fn rows(source: &str) -> Vec<AstRow> {
        let parsed = cddl::check(source, &Settings::default().cddl_config())
            .cddl
            .expect("the test schema parses");
        cddl::ast_tree::flatten(&cddl::ast_tree::build_tree(&parsed), source)
    }

    /// `ParseTree.tsx`'s two levels: the document root and its rules are open, and nothing below
    /// them is. Getting this wrong in either direction is immediately visible — a fully collapsed
    /// tree shows one row, a fully expanded one shows every node of a 17 KB schema.
    #[test]
    fn the_tree_default_expands_to_depth_two() {
        let rows = rows(SCHEMA);
        assert!(
            rows.iter().any(|row| row.depth >= 2),
            "the fixture must reach depth 2 for this test to mean anything"
        );

        let mut seen = HashSet::new();
        let mut expanded = HashSet::new();
        default_expand(&rows, &mut seen, &mut expanded);

        for row in &rows {
            let id = SharedString::from(row.id.clone());
            assert_eq!(
                expanded.contains(&id),
                row.depth < 2,
                "`{}` at depth {} is {}expanded",
                row.id,
                row.depth,
                if expanded.contains(&id) { "" } else { "not " }
            );
        }
    }

    /// The `seen` half of the contract, and the reason the default is applied through `expanded`
    /// rather than recomputed: `TreeState::set_items` rebuilds every item, so a depth test re-run
    /// on each rebuild would re-open a node the user had collapsed at every `Syntax Check`.
    #[test]
    fn a_node_the_user_collapsed_stays_collapsed_across_a_rebuild() {
        let rows = rows(SCHEMA);
        let mut seen = HashSet::new();
        let mut expanded = HashSet::new();
        default_expand(&rows, &mut seen, &mut expanded);

        // What `TreeEvent::Collapsed` does.
        let collapsed = SharedString::from(
            rows.iter()
                .find(|row| row.depth == 1)
                .expect("a rule row")
                .id
                .clone(),
        );
        expanded.remove(&collapsed);

        default_expand(&rows, &mut seen, &mut expanded);
        assert!(
            !expanded.contains(&collapsed),
            "`{collapsed}` was re-expanded by a rebuild"
        );
    }

    /// `flatten` returns a flat pre-order list with a depth per row; the tree needs the nesting
    /// back. A cursor that advanced wrongly would not fail loudly — it would silently drop the
    /// tail of the document, so the count is what is asserted.
    #[test]
    fn the_item_tree_holds_every_flattened_row_exactly_once() {
        let rows = rows(SCHEMA);
        let ids: HashSet<SharedString> = rows
            .iter()
            .map(|row| SharedString::from(row.id.clone()))
            .collect();

        let items = tree_items(&rows, 0, &mut 0, &HashSet::new());

        fn collect(items: &[TreeItem], into: &mut Vec<SharedString>) {
            for item in items {
                into.push(item.id.clone());
                collect(&item.children, into);
            }
        }
        let mut flat = Vec::new();
        collect(&items, &mut flat);

        assert_eq!(flat.len(), rows.len(), "{flat:?}");
        assert_eq!(flat.iter().cloned().collect::<HashSet<_>>(), ids);
        // The flattening is pre-order, so rebuilding and re-flattening must give the original
        // order back and not merely the same set.
        assert_eq!(
            flat,
            rows.iter()
                .map(|row| SharedString::from(row.id.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// A matched rule and a failed rule must not look the same, and an `Attempt` must not be drawn
    /// at all: an attempt is immediately followed by its own outcome, so tinting it would paint
    /// every rule the trace entered the one colour and say nothing about any of them.
    #[test]
    fn only_a_decided_outcome_is_tinted_and_the_two_differ() {
        let success: Hsla = hsla(0.3, 0.6, 0.5, 1.0);
        let danger: Hsla = hsla(0.0, 0.8, 0.5, 1.0);

        let matched = status_fill(TraceOutcome::Match, success, danger).expect("a match is tinted");
        let failed = status_fill(TraceOutcome::Fail, success, danger).expect("a fail is tinted");
        assert_ne!(
            matched, failed,
            "a failed rule is drawn the same as a matched one"
        );
        assert_eq!(
            status_fill(TraceOutcome::Attempt, success, danger),
            None,
            "an attempt was tinted"
        );

        // Both are translucent, or the tint hides the syntax colouring it is drawn over — which on
        // a rule body running to dozens of lines means the editor stops being readable.
        for tint in [matched, failed] {
            assert!(tint.a < 1.0, "{tint:?} is opaque");
        }
        // And the jump, which is painted over these, is the stronger of the two.
        const {
            assert!(
                JUMP_OPACITY > STATUS_OPACITY,
                "the jump highlight is no more visible than the tint it lands on"
            )
        };
    }

    /// A `Frame` round a multi-line rule definition reads as a border on the editor rather than a
    /// mark on the rule, and the default *is* `Frame` — so the style is set explicitly, and this is
    /// what notices if it stops being.
    #[test]
    fn decorations_are_fills_in_the_colour_they_were_given() {
        let colour: Hsla = hsla(0.5, 0.5, 0.5, 0.2);
        let decoration = fill(3..9, colour);
        assert_eq!(decoration.style(), RangeDecorationStyle::Fill);
        assert_eq!(decoration.color(), Some(colour));
        assert_eq!(*decoration.range(), 3..9);
    }

    /// `DiagnosticSeverity::Hint` is the `Default`, and a hint is drawn muted with no squiggle — so
    /// a mapping that fell through to it would make a CDDL syntax error invisible in the editor
    /// while the lower window still called it an error.
    #[test]
    fn both_core_severities_map_onto_a_visible_editor_severity() {
        assert_eq!(severity_of(Severity::Error), DiagnosticSeverity::Error);
        assert_eq!(severity_of(Severity::Warning), DiagnosticSeverity::Warning);
    }

    /// The observed failure: `tps_cddl` reports an unexpected token as the *position* where it
    /// expected something else, so a typed error arrives as `n..n` and underlines nothing. Checked
    /// against the span the real parse error produces — the start of the line *after* the one the
    /// user broke, which has to reach backwards — as well as the plain mid-line case.
    #[test]
    fn a_zero_width_diagnostic_is_widened_so_it_can_be_seen() {
        let text = "a = uint @@@\nb = tstr\n";
        let next_line = text.find('\n').expect("the fixture has two lines") + 1;

        // Backwards past the line break, onto the last `@` of the line that is actually wrong.
        let span = squiggle_span(&(next_line..next_line), text);
        assert_eq!(&text[span.clone()], "@", "widened to {span:?}");

        let mid = text.find('@').expect("the fixture has the typo");
        assert_eq!(&text[squiggle_span(&(mid..mid), text)], "@");
    }

    /// At the end of the file there is no character after the position at all, so the only
    /// direction left is backwards — and a mark on the trailing newline would show nothing.
    #[test]
    fn a_zero_width_diagnostic_at_the_end_of_the_file_reaches_backwards() {
        let text = "a = uint\n";
        let end = text.len();
        assert_eq!(&text[squiggle_span(&(end..end), text)], "t");
    }

    /// `0..0` is "no position to offer", not "an error on the first character", and the two have
    /// to look different: a module that cannot be found has nothing to underline, and underlining
    /// the first byte of the file would point at a line that is not the problem.
    #[test]
    fn a_diagnostic_with_no_position_is_not_widened_into_one() {
        assert_eq!(squiggle_span(&(0..0), "a = uint\n"), 0..0);
    }

    /// A span that already has width is the parser's own answer and is passed through untouched —
    /// widening it would stretch a squiggle past the token it belongs to.
    #[test]
    fn a_ranged_diagnostic_is_left_alone() {
        assert_eq!(squiggle_span(&(4..8), "a = uint\n"), 4..8);
    }

    /// Stepping by bytes rather than characters would split a multi-byte character and panic
    /// inside the rope the moment a CDDL file carried a non-ASCII comment — which the corpus does.
    #[test]
    fn widening_steps_whole_characters() {
        let text = "a = uint ; é\n";
        let at_accent = text.find('é').expect("the fixture has the accent");

        assert_eq!(&text[squiggle_span(&(at_accent..at_accent), text)], "é");
        let after = at_accent + 'é'.len_utf8();
        assert_eq!(&text[squiggle_span(&(after..after), text)], "é");
    }
}
