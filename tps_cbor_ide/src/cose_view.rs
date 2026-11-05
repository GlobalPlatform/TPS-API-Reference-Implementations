/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The COSE tab's upper window (`UI_DESIGN.md:127`).
//!
//! A read-only [`Editor`] showing the EDN rendering of `workspace.cose`, which is the one thing
//! `:127` asks the centre window for: "EDN for the CBOR loaded in the CBOR and/or EDN tabs when the
//! selected encoding has been applied". The text comes from [`convert::cbor_to_edn`] — the same
//! function the EDN tab is filled from, so a `COSE_Sign1` is displayed by the same renderer that
//! displays everything else and there is no second EDN writer to drift.
//!
//! A deliberately smaller sibling of [`crate::edn_view::EdnView`], and the list of what it *omits*
//! is the interesting part. Nothing edits this document through the editor, so there is no
//! editor-to-model direction at all: no `flush`, no `Change` subscription, no dirty tracking, and no
//! [`DiagnosticSet`](gpui_kit::base::input::DiagnosticSet) — the document is a derived view of
//! bytes the user never types into. What remains is one direction, model to editor, and the two
//! mechanics `EdnView` had to establish for it.
//!
//! ## Read-only
//!
//! Through the element builder, `Editor::readonly(true)`, and never `EditorState::set_readonly`.
//! `Editor`'s render writes `readonly` into the state on every frame
//! (`gpui-component/src/input/input.rs:591`), so an imperative call would be overwritten by the next
//! paint. The flag is a constant `true` here rather than read from `meta.read_only`: the COSE
//! document's read-only flag guards *saving*, and this pane is not an editor of it under any
//! setting. A user who wants to edit the EDN of a COSE object can `To EDN` it on the CBOR tab.
//!
//! ## The conversion runs in `reload`, not in `render`
//!
//! [`CoseView::reload`] decodes the bytes and renders the EDN **once per change**, and
//! [`CoseView::render`] only pushes the result into the editor. Two reasons, and the second is why
//! the first is not merely a tidiness argument:
//!
//! * A full CBOR decode and re-render on every frame would put a document-sized pass on the paint
//!   path, which is the discipline [`crate::Ide`] already keeps for the trace rows.
//! * **A failure needs somewhere to go.** `reload` returns the message, so [`crate::Ide`] can report
//!   it in the lower window as a [`Source::Cose`](tps_cbor_core::Source) diagnostic; a conversion
//!   attempted from `render` could only be swallowed, because a render has no route to the
//!   diagnostic list that does not schedule another frame.
//!
//! That a failure is reported at all, rather than ignored, is the point: every byte string this pane
//! is given came from [`tps_cbor_core::cose::ops`] or from a file that
//! [`tps_cbor_core::cose::sniff`] has already decoded, so an EDN rendering failure here is
//! unreachable — and an unreachable condition that goes unsaid is how the next phase inherits a
//! blank pane with no explanation.
//!
//! ## Why the push to the editor is deferred to paint
//!
//! `set_value` needs a `&mut Window`, and none of the callers has one: `reload` is reached from
//! `Ide::open_path` (an explorer event, a spawned open dialog, or the command line) and from the
//! `Sign` and `Encrypt` handlers. So `reload` records the text and `render` applies it, which is
//! exactly the deferral `edn_view`'s module docs set out.

use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;
use tps_cbor_core::{Workspace, convert};

/// The COSE tab's upper window.
pub struct CoseView {
    /// The editing engine. Still an `EditorState` on a pane nothing edits, because the syntax
    /// highlighting, the selection and the scrolling all come with it; a plain text element would
    /// show a COSE object as an unhighlighted wall.
    state: Entity<EditorState>,
    /// The shared model, held as an entity for the reason [`crate::hex_view`] records.
    ///
    /// Read only in [`CoseView::reload`], never in `render`: the EDN this pane shows is derived
    /// from `workspace.cose` at the moment it changes, not re-derived per frame.
    workspace: Entity<Workspace>,
    /// The EDN to show, as [`CoseView::reload`] last rendered it. Empty for an empty document,
    /// which is the state the tab starts in.
    text: String,
    /// Whether [`CoseView::apply`] has work to do on the next paint.
    pending: bool,
}

impl CoseView {
    /// Takes `window` because [`EditorState::new`] requires one.
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Exactly `"edn"`, and the same grammar the EDN tab uses: lookup is case-sensitive and
        // un-normalized, and a name that matches nothing yields no highlighting and no error at all
        // (see [`crate::languages`]).
        let state = cx.new(|cx| EditorState::new(window, cx).language("edn"));

        Self {
            state,
            workspace,
            text: String::new(),
            // A document may already be loaded — `main` replays the command line before the first
            // frame — so the first paint has to pick the model up. The text is rendered by the
            // `reload` that loading it performs; this only guarantees an empty one is applied.
            pending: true,
        }
    }

    /// The editing engine, for the actions [`crate::Ide`] forwards to the selected tab.
    ///
    /// A read-only pane still answers Find and Copy; `Paste` reaches the engine too and is refused
    /// by it, which is the point of forwarding rather than deciding here. See
    /// [`crate::edn_view::EdnView::editor`].
    pub fn editor(&self) -> &Entity<EditorState> {
        &self.state
    }

    /// Focuses the editor, so that the keyboard reaches it — Select All and Copy, and the scroll
    /// keys, all of which a read-only pane still wants.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.state.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
    }

    /// Re-renders the COSE document as EDN, to be shown at the next paint.
    ///
    /// Returns the failure message when the bytes do not decode, for the caller to report; see the
    /// module docs on why that is a return value rather than something handled here. The pane is
    /// left showing nothing in that case, because half a rendering of a document that does not
    /// decode would be worse than an empty pane with an error beside it.
    ///
    /// An **empty** document is not one of those cases. It is the resting state of this pane —
    /// every session starts there, and so does every session that opens a `.cbor` file, since the
    /// open path reloads this view whichever tab the file landed in. `cbor_to_edn` answers
    /// "unexpected end of CBOR input" for no bytes at all, which is true and useless: reporting it
    /// put a red `COSE` row in the lower window at start-up, before the user had touched anything.
    /// So an empty document renders as empty text, and the error path keeps the meaning its doc
    /// comment claims — bytes that are *there* and do not decode.
    pub fn reload(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if self.workspace.read(cx).cose.is_empty() {
            self.text = String::new();
            self.pending = true;
            cx.notify();
            return Ok(());
        }
        let outcome = convert::cbor_to_edn(self.workspace.read(cx).cose.bytes());
        self.pending = true;
        cx.notify();
        match outcome {
            Ok(text) => {
                self.text = text;
                Ok(())
            }
            Err(error) => {
                self.text = String::new();
                Err(error.to_string())
            }
        }
    }

    /// Pushes the rendered EDN into the editor. Runs from `render`, which is the only place a
    /// `&mut Window` is reliably to hand; see the module docs.
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Guarded on a difference because `set_value` resets the selection and the scroll position:
        // a user who has scrolled down a long signature should not be sent back to the top by a
        // repaint that changes nothing.
        if self.state.read(cx).value().as_ref() == self.text {
            return;
        }
        let text = self.text.clone();
        self.state
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// Copy and Select All, which are the only two the built-in menu offers that apply here.
    ///
    /// `Editor::context_menu` *replaces* the built-in menu rather than extending it
    /// (`gpui-component/src/input/editor.rs:103-115`), so the items that still make sense have to be
    /// restated. Cut and Paste are left out rather than disabled: they are not temporarily
    /// unavailable on this pane, they will never apply to it, and a permanently greyed item is noise.
    fn context_menu(&self) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
        let state = self.state.clone();
        move |menu, _, cx| {
            let selected = !state.read(cx).selected_range().is_empty();
            menu.menu_with_disabled("Copy", !selected, Box::new(gpui_kit::base::input::Copy))
                .menu("Select All", Box::new(gpui_kit::base::input::SelectAll))
        }
    }
}

impl Focusable for CoseView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx).clone()
    }
}

impl Render for CoseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Before the element is built, so the frame about to be painted shows the new text rather
        // than lagging it by one. The flag is cleared first: `apply` can notify, and leaving it set
        // would make that notification schedule another identical frame, for ever.
        if std::mem::take(&mut self.pending) {
            self.apply(window, cx);
        }

        Editor::new(&self.state)
            .h(relative(1.))
            // Constant, not `meta.read_only`: see the module docs.
            .readonly(true)
            // The pane supplies the border and the background; a second bordered box inside the tab
            // view would draw a frame within a frame.
            .appearance(false)
            .bordered(false)
            .context_menu(self.context_menu())
    }
}

#[cfg(test)]
mod tests {
    // Imported item by item and never through `use super::*`: the glob would reach this module's
    // `use gpui_kit::*` and shadow the standard `#[test]` with GPUI's own attribute macro, which
    // expands to itself until the compiler gives up on the recursion limit (`lib.rs:98-99`).
    use std::prelude::v1::test;
    use tps_cbor_core::convert;

    /// The premise of [`super::CoseView::reload`]'s empty-document guard, pinned so that the guard
    /// is not left in place after the reason for it has gone. No bytes at all is a *failure* of the
    /// conversion rather than an empty rendering, and the COSE document is empty in every session
    /// until something fills it — which is how a red `COSE` row came to be in the lower window at
    /// start-up. If this ever answers `Ok("")`, the guard is redundant and can go.
    #[test]
    fn no_bytes_at_all_is_a_conversion_failure_and_not_empty_edn() {
        assert!(
            convert::cbor_to_edn(&[]).is_err(),
            "an empty document now renders, so `CoseView::reload`'s empty guard is dead code"
        );
    }
}
