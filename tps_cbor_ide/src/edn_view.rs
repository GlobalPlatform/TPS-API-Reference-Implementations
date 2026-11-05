/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The EDN tab's editor (`UI_DESIGN.md:86-99`).
//!
//! A gpui-kit [`Editor`] over the `edn` tree-sitter grammar, with the EDN parse error drawn as an
//! inline wavy underline and reported in the lower window. This is the crate's first consumer of
//! the syntax highlighter, which matters because an unregistered grammar fails *silently* — see
//! [`crate::languages`].
//!
//! ## Two kinds of marking, and why they are not one mechanism
//!
//! Three things want to draw on this editor: the EDN parse error, the validation mismatches a
//! `Validate` found in EDN coordinates, and the jump highlight a trace or diagnostic row produces.
//! They divide across the editor's two facilities, and for a reason each:
//!
//! * The **parse error and the mismatches are diagnostics** — they have messages, severities and
//!   squiggles — so both go into the one [`DiagnosticSet`](gpui_kit::base::input::DiagnosticSet).
//!   That set is emptied by `reset` on every mutation, so **one owner has to supply all of it**:
//!   hence [`EdnView::publish`] taking both kinds together rather than one entry point each, which
//!   would have the two silently deleting one another.
//! * The **jump highlight is geometry** — no message, no severity, and it must be able to sit over
//!   a mismatch without replacing it — so it gets its own `RangeDecorationCollection`, created
//!   **once** in [`EdnView::new`]. Once, because a collection has no `dispose` on the text-decoration
//!   side and creating one per publish would leak an id and a paint layer every time.
//!
//! ## Who owns the text
//!
//! Two candidates, so it is settled here rather than left to each call site: **[`EditorState`]
//! owns the text** (a `Rope`), and `workspace.edn` owns the path, the dirty flag, the read-only
//! flag, and the copy of the text that gets saved or converted. The two are kept in step in three
//! places and no others:
//!
//! * **Editor to model** on [`InputEvent::Change`], through [`EdnView::flush`].
//! * **Model to editor** on open and on `To EDN`, through [`EdnView::reload`] — applied at the next
//!   paint with `set_value`, which emits no `Change` (it clears `emit_events` internally), so there
//!   is no write-back, no loop, and no spurious `CoreError::ReadOnly` from `TextDoc::edit` while
//!   the tab is read-only.
//! * **Editor to model again**, explicitly, from [`EdnView::flush`] at the top of every consumer:
//!   save, `To CBOR`, `Validate`, the decode, and the quit guard.
//!
//! That third one is not belt and braces. **Plain undo and redo emit no `Change`** — they only
//! `cx.notify()` — so a mirror driven by the event alone would let `Save` write the text from
//! *before* a Ctrl-Z. `flush` compares the two sides and writes through only on a difference, so it
//! is idempotent and costs nothing in the common case.
//!
//! `set_value` also clears the undo history and resets the selection and scroll. That is correct
//! exactly where it is used: opening a file or converting from CBOR produces a *new* document, and
//! undoing across that boundary would be incoherent.
//!
//! ## Why the model-to-editor direction is deferred to paint
//!
//! `set_value` needs a `&mut Window` and `Ide::open_path` has none — it is reached from an explorer
//! event, from the open dialog's spawned task, and from the command line, none of which carry one.
//! So [`EdnView::reload`] and [`EdnView::publish`] record what to do and [`EdnView::render`] does
//! it, which is where a window always exists.
//!
//! That also fixes a subtler ordering bug rather than working around the first one. A diagnostic
//! carries a *byte span*, and the editor indexes its diagnostics by line and character — so the
//! conversion has to run against the text the span was computed from. Applying both at paint means
//! it always does; applying `publish` eagerly would convert the new document's span through the old
//! document's rope whenever a file was opened.
//!
//! ## Read-only
//!
//! Through the element builder, `Editor::readonly(..)`, and never `EditorState::set_readonly`. The
//! two are not alternatives: `Editor`'s render writes `readonly` into the state on every frame
//! (`gpui-component/src/input/input.rs:591`), so an imperative call would be overwritten by the
//! next paint. One mechanism, and it is the one that wins.
//!
//! ## The context menu
//!
//! `Editor::context_menu` *replaces* the built-in menu rather than extending it — the default items
//! are not re-added (`gpui-component/src/input/editor.rs:103-115`) — so Cut, Copy, Paste and Select
//! All are restated below. Omitting them to save a few lines would be a regression against the menu
//! the editor already had. The closure is handed only `&mut App`, not the capability set the
//! built-in menu consults, which is why the enabled state is re-derived here from the document's
//! own `read_only` flag and the live selection.
//!
//! No `set_language_config` call, which the plan had expected to need: the default language provider
//! answers *every* name with `LanguageConfig::default()`
//! (`gpui-base/src/input/editor/language.rs:20-22, 32-38`), and that default already pairs `()`,
//! `[]`, `{}`, `""` and `''` — which is exactly EDN's bracket set. Registering a configuration
//! identical to the fallback would only be a second place for it to drift.

use std::ops::Range;

use gpui_kit::base::input::{
    Diagnostic as EditorDiagnostic, DiagnosticSeverity, RangeDecoration, RangeDecorationCollection,
    RangeDecorationStyle,
};
// Deliberately the *component* `NativeMenu` and not `base::input::NativeMenu`. Two distinct types
// share the name — `gpui-base/src/input/base/native.rs:114` is a plain item list, while
// `gpui-component/src/native_menu/mod.rs:72` is the one with icons, checkmarks and `show` — and
// `Editor::context_menu` takes the component one (`gpui-component/src/input/editor.rs:111`).
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, RopeExt as _};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;
use tps_cbor_core::{Diagnostic, Severity, Workspace};

/// What the editor tells [`crate::Ide`] when the document changes under it.
pub enum EdnEvent {
    /// The text was edited. The parse result, the explorer's dirty set and the `Save` menu item all
    /// have to be re-derived.
    Edited,
    /// A write to the model was refused. Unreachable while the element is rendered `readonly` on a
    /// read-only document — which is precisely why it is reported rather than discarded: if it ever
    /// fires, the element and the document have disagreed about who may type.
    Failed(String),
}

/// The EDN tab's upper window.
pub struct EdnView {
    /// The editing engine, and the source of truth for the text. See the module docs.
    state: Entity<EditorState>,
    /// The shared model, held as an entity for the reason recorded in [`crate::hex_view`]: a copy
    /// of the text would have to be re-copied on every keystroke, and holding `Entity<Ide>` would
    /// be a reference cycle.
    workspace: Entity<Workspace>,
    /// The parse error to draw inline, as [`EdnView::publish`] last left it.
    ///
    /// Held rather than pushed straight into the editor because the push has to happen at paint;
    /// see the module docs.
    parse: Option<Diagnostic>,
    /// The validation mismatches to draw inline, in EDN coordinates.
    ///
    /// Separate from [`EdnView::parse`] because the two have different lifetimes — a parse error is
    /// re-derived on every keystroke, a mismatch survives until the next edit clears the trace —
    /// but they are published together, for the reason the module docs give.
    validation: Vec<Diagnostic>,
    /// The jump highlight's ranges, as [`EdnView::mark`] last left them.
    mark: Vec<Range<usize>>,
    /// The jump highlight's own paint layer, created once; see the module docs.
    jump: RangeDecorationCollection,
    /// Whether [`EdnView::apply`] has work to do on the next paint.
    pending: bool,
    /// Whether the editor has yet to be given the model's text.
    ///
    /// Distinct from [`EdnView::pending`], which is also raised by [`EdnView::publish`]: this one
    /// says the editor is *not authoritative*, and [`EdnView::flush`] refuses to write back while
    /// it is set. Without it, a `Save` landing between a [`EdnView::reload`] and the paint that
    /// honours it would push the previous document — or, before the first frame, an empty buffer —
    /// over the one just opened.
    stale: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EdnEvent> for EdnView {}

impl EdnView {
    /// Takes `window` — unlike `HexView::new` — because [`EditorState::new`] requires one.
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Exactly `"edn"`: grammar lookup is case-sensitive and un-normalized, and a name that
        // matches nothing yields no highlighting and no error at all.
        let state = cx.new(|cx| EditorState::new(window, cx).language("edn"));

        // Created here and never again: see the module docs. Empty, because there is nothing to
        // jump to before a trace has run.
        let jump = state.update(cx, |state, cx| {
            state.create_range_decorations_collection(Vec::new(), cx)
        });

        let subscriptions = vec![cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_change(cx);
            }
        })];

        Self {
            state,
            workspace,
            parse: None,
            validation: Vec::new(),
            mark: Vec::new(),
            jump,
            // A document may already be loaded — `main` replays the command line before the first
            // frame — so the first paint has to pick the model up.
            pending: true,
            stale: true,
            _subscriptions: subscriptions,
        }
    }

    /// The editing engine, for the actions [`crate::Ide`] forwards to whichever tab is selected:
    /// Find, Replace, Copy and Paste.
    ///
    /// Exposed as the state rather than wrapped in a method per action. Each of those is one call
    /// on the engine's own API (`open_search`, `next_search_match`) or one dispatched
    /// `input::` action, and five one-line delegations here would only be five more places for the
    /// dispatch to go wrong.
    pub fn editor(&self) -> &Entity<EditorState> {
        &self.state
    }

    /// Focuses the editor, so that the keyboard reaches it.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.state.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
    }

    /// Asks for the model's text to be pushed into the editor at the next paint, for a document
    /// that has been replaced wholesale — a file opened, or `To EDN` run.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.pending = true;
        self.stale = true;
        cx.notify();
    }

    /// Replaces every diagnostic drawn inline: the EDN parse error, and the validation mismatches
    /// a `Validate` found in EDN coordinates.
    ///
    /// Re-published on every change rather than pushed once, because every text edit calls
    /// `DiagnosticSet::reset`, which empties it. **Both kinds in one call** for the same reason:
    /// `reset` cannot be told to spare the other kind, so a caller publishing one alone would erase
    /// the other. See the module docs.
    pub fn publish(
        &mut self,
        parse: Option<Diagnostic>,
        validation: &[Diagnostic],
        cx: &mut Context<Self>,
    ) {
        self.parse = parse;
        self.validation = validation.to_vec();
        self.pending = true;
        cx.notify();
    }

    /// Replaces the jump highlight, or clears it when `ranges` is empty.
    ///
    /// Named and shaped after [`crate::hex_view::HexView::mark`], because it is the same gesture
    /// landing in a different pane; a trace row click reaches both. Its own layer rather than a
    /// diagnostic, so that highlighting a mismatch does not delete the mismatch.
    pub fn mark(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        if self.mark == ranges {
            return;
        }
        self.mark = ranges;
        self.pending = true;
        cx.notify();
    }

    /// Writes the editor's text into the model, if the two differ.
    ///
    /// `Ok(true)` means the model was updated, `Ok(false)` that it already agreed or that there was
    /// nothing to write. Called from the `Change` handler *and* from every consumer of the text,
    /// because undo and redo emit no `Change`; see the module docs.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> tps_cbor_core::Result<bool> {
        // A pending [`EdnView::reload`] means the model has been replaced and the editor has not
        // caught up, so the editor is not authoritative and writing back would push the *old* text
        // — or, before the first paint, an empty buffer — over the new document. The guard is what
        // makes "the editor owns the text" true only once it has been given it.
        if self.stale {
            return Ok(false);
        }

        let value = self.state.read(cx).value();
        if self.workspace.read(cx).edn.text() == value.as_ref() {
            return Ok(false);
        }

        let text = value.to_string();
        self.workspace.update(cx, |workspace, cx| {
            let outcome = workspace.edn.edit(text);
            cx.notify();
            outcome
        })?;
        Ok(true)
    }

    /// The selected text, or `None` when nothing is selected.
    ///
    /// `UI_DESIGN.md:93`'s decode is driven from this, and the empty case is what disables the menu
    /// item rather than reporting "nothing selected" after the fact.
    pub fn selection(&self, cx: &App) -> Option<String> {
        let state = self.state.read(cx);
        if state.selected_range().is_empty() {
            return None;
        }
        Some(state.selected_value().to_string())
    }

    /// Puts the caret at `offset` and scrolls it into view.
    ///
    /// `Validate` is the only caller, and the analogue of `HexView::reveal`: an explicit check
    /// should take the user to what it found. Moving the caret rather than only scrolling is the
    /// difference between "here it is" and "it is somewhere on this screen".
    pub fn reveal(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.set_selected_range(offset..offset, cx);
        });
    }

    /// Pushes the model's text, the stored diagnostics and the jump highlight into the editor. Runs
    /// from `render`, which is the only place a `&mut Window` is reliably to hand; see the module
    /// docs.
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.workspace.read(cx).edn.text().to_owned();
        // Guarded on a difference because `set_value` discards the undo history and the caret: an
        // edit the user just made, round-tripped through `flush`, must not throw those away.
        if self.state.read(cx).value().as_ref() != text {
            self.state
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.stale = false;

        let parse = self.parse.clone();
        // A mismatch the honesty rule could not verify is stored with its span cleared to `0..0`
        // (`PLAN.md`'s inert-row convention), and an empty range here would draw a squiggle at the
        // top of the document — pointing confidently at the wrong place, which is the one thing
        // that rule exists to prevent. The parse error is pushed unfiltered, because an EDN error
        // at end of input legitimately has an empty span and still has to be visible.
        let validation: Vec<Diagnostic> = self
            .validation
            .iter()
            .filter(|diagnostic| !diagnostic.span.is_empty())
            .cloned()
            .collect();
        self.state.update(cx, |state, cx| {
            // Cloned out first: `diagnostics_mut` takes `&mut state` and the conversion needs the
            // rope at the same time. A `Rope` clone shares its tree rather than copying the text.
            let rope = state.text().clone();
            let Some(set) = state.diagnostics_mut() else {
                return;
            };
            set.reset(&rope);
            for diagnostic in parse.iter().chain(validation.iter()) {
                let range = rope.offset_to_position(diagnostic.span.start)
                    ..rope.offset_to_position(diagnostic.span.end);
                set.push(
                    EditorDiagnostic::new(range, diagnostic.message.clone())
                        // `DiagnosticSeverity::Hint` is the `Default`, so the severity is set
                        // explicitly or an error is drawn as a hint.
                        .with_severity(severity_of(diagnostic.severity)),
                );
            }
            cx.notify();
        });

        // The colour is resolved here rather than per frame because `RangeDecorationCollection::set`
        // notifies unconditionally, so a `set` from `render` would schedule the next frame for ever.
        // Safe because the mode is fixed at startup by `crate::theme::init` and there is no in-app
        // switch; adding one means re-applying here when it fires.
        let fill = cx.theme().info.opacity(0.25);
        let decorations = self
            .mark
            .iter()
            .map(|range| {
                RangeDecoration::new(range.clone())
                    // `Fill` rather than the default `Frame`: this marks a *value* in the document,
                    // and a one-pixel box around a multi-line map reads as a border, not a
                    // selection. It also matches the hex grid's cell backgrounds, which is the
                    // point — the same click marks both.
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(fill)
            })
            .collect();
        self.jump.set(decorations, cx);
    }

    /// The model is only as current as the last `Change`, so this writes through before announcing
    /// the edit; a refusal is surfaced rather than swallowed, for the reason [`EdnEvent::Failed`]
    /// records.
    fn on_change(&mut self, cx: &mut Context<Self>) {
        match self.flush(cx) {
            Ok(_) => cx.emit(EdnEvent::Edited),
            Err(error) => cx.emit(EdnEvent::Failed(error.to_string())),
        }
    }

    /// `UI_DESIGN.md:93`'s entry point, plus the clipboard items the custom menu has to restate.
    fn context_menu(&self) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
        let state = self.state.clone();
        let workspace = self.workspace.clone();
        move |menu, _, cx| {
            let editable = !workspace.read(cx).edn.meta.read_only;
            let selected = !state.read(cx).selected_range().is_empty();
            menu.menu_with_disabled(
                "Cut",
                !(editable && selected),
                Box::new(gpui_kit::base::input::Cut),
            )
            .menu_with_disabled("Copy", !selected, Box::new(gpui_kit::base::input::Copy))
            // Offered on the strength of the read-only flag alone, as the built-in menu is: the
            // synchronous clipboard read is empty on some platforms, and pasting nothing is
            // harmless.
            .menu_with_disabled("Paste", !editable, Box::new(gpui_kit::base::input::Paste))
            .menu("Select All", Box::new(gpui_kit::base::input::SelectAll))
            .separator()
            .menu_with_disabled(
                "Decode selection as CBOR",
                !selected,
                Box::new(crate::actions::DecodeSelection),
            )
        }
    }
}

impl Focusable for EdnView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx).clone()
    }
}

impl Render for EdnView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Before the element is built, so the frame about to be painted shows the new text rather
        // than lagging it by one. The flag is cleared first: `apply` notifies, and leaving it set
        // would make that notification schedule another identical frame, for ever.
        if std::mem::take(&mut self.pending) {
            self.apply(window, cx);
        }

        let read_only = self.workspace.read(cx).edn.meta.read_only;
        Editor::new(&self.state)
            .h(relative(1.))
            .readonly(read_only)
            // The pane supplies the border and the background; a second bordered box inside the
            // tab view would draw a frame within a frame.
            .appearance(false)
            .bordered(false)
            .context_menu(self.context_menu())
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

#[cfg(test)]
mod tests {
    // Imported item by item rather than with `use super::*`, which is what gpui-kit's own
    // `lib.rs:98-99` asks for: the glob would pull in GPUI's `#[test]` attribute macro through this
    // module's `use gpui_kit::*`, shadow the standard one, and expand to itself until the compiler
    // gives up on the recursion limit.
    use super::severity_of;
    use gpui_kit::base::input::DiagnosticSeverity;
    use tps_cbor_core::Severity;

    /// `DiagnosticSeverity::Hint` is the `Default`, and `Hint` is drawn in the muted colour with no
    /// squiggle — so a mapping that fell through to it would make an EDN syntax error invisible in
    /// the editor while the lower window still called it an error.
    #[test]
    fn both_core_severities_map_onto_a_visible_editor_severity() {
        assert_eq!(severity_of(Severity::Error), DiagnosticSeverity::Error);
        assert_eq!(severity_of(Severity::Warning), DiagnosticSeverity::Warning);
        for severity in [Severity::Error, Severity::Warning] {
            assert_ne!(
                severity_of(severity),
                DiagnosticSeverity::Hint,
                "{severity:?} would be drawn as a hint"
            );
        }
    }
}
