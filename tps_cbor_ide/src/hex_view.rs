/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The CBOR tab's hex editor (`UI_DESIGN.md:72-77`).
//!
//! A row is an offset, `n * 8` hex cells and an ASCII gutter, with `n` following the available
//! width. Both fields are live: clicking either moves one caret, and typing in either writes
//! through the *same* [`CborDoc::set_byte`](tps_cbor_core::CborDoc::set_byte). The old
//! `HexEditor.tsx` had a handler per side, which is how the two could disagree about what the
//! buffer held; here there is one write path and the second field is a second *view* of it, drawn
//! with a faint caret so it is visible that both cells are the same byte.
//!
//! **There is no size cap.** `HexEditor.tsx:23` refused files over 64 KiB because it rendered one
//! `<input>` element per byte, so a 1 MB file meant a million DOM nodes. Here only the visible
//! rows are built, so the cost is the window, not the file.
//!
//! Three things diverge from the plan or are worth not rediscovering:
//!
//! 1. **`uniform_list`, not `v_virtual_list`.** The plan named the latter, but it takes
//!    `Rc<Vec<Size<Pixels>>>` — one measurement per row, which is 65,536 entries for a 1 MB file
//!    and has to be rebuilt every time the column count changes. Every hex row is the same height,
//!    which is exactly the case `uniform_list` exists for: it takes a count. It also already
//!    implements [`ScrollbarHandle`](gpui_kit::base::ScrollbarHandle)
//!    (`gpui-base/src/scrollbar.rs:102`), so the scrollbar pairs with it directly. Recorded in
//!    `PLAN.md`.
//! 2. **The cell widths are fixed pixel constants, not measured font advances.** That is what keeps
//!    [`columns_for_width`] a pure function with a unit test: the only thing that has to be
//!    measured is the container, and `ElementExt::on_prepaint` reports that. The cost is one frame
//!    of lag after a resize, which is not perceptible and is inherent to measuring during layout.
//! 3. **The row closure cannot touch the theme.** It is `'static` and receives `&mut App`, which it
//!    has already lent out to read the buffer. So the colours are resolved once per frame into
//!    [`Colours`] and captured — see the note there.
//!
//! The view holds `Entity<Workspace>` rather than a copy of the bytes. A copy would duplicate the
//! file and have to be re-copied on every keystroke; the alternative — holding `Entity<Ide>` — is a
//! reference cycle, since `Ide` holds this view. Sharing the model is the only shape with neither
//! problem, and it is what GPUI entities are for.

use gpui_kit::base::{ElementExt as _, Scrollbar};
// None of these three is re-exported by `component::*`; the reason is recorded at `main.rs:64-66`.
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
// `InputEvent` is named explicitly because `gpui_kit::*` glob-exports a *trait* of the same name;
// an explicit import takes precedence over a glob, and without it the subscription's closure
// parameter is read as `&dyn InputEvent`.
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use std::rc::Rc;
use tps_cbor_core::{Workspace, convert};

/// Bytes per group. `UI_DESIGN.md:74` asks for multiples of eight, and it is also what
/// `hexdump -C` and `tps_cbor_core::convert::hex_dump` group by — that crate's own `GROUP` is
/// private, so this restates it rather than sharing it.
const GROUP: usize = 8;

/// A row's height, in pixels.
///
/// Pinned rather than inferred for two reasons that have to agree: `uniform_list` measures its
/// first row and uses that height for all of them, and [`HexView::page_rows`] divides the measured
/// viewport by it. Setting it explicitly on each row makes both true by construction.
const ROW_HEIGHT: f32 = 18.0;

/// Width of the offset gutter — eight hex digits and a little air.
const OFFSET_WIDTH: f32 = 72.0;
/// Width of one hex cell: two digits, centred, with the inter-byte space built in.
const HEX_CELL: f32 = 21.0;
/// Width of one ASCII cell.
const ASCII_CELL: f32 = 9.0;
/// Space after each eight-byte group, in both fields.
const GROUP_GAP: f32 = 8.0;
/// Space between the hex field and the ASCII gutter.
const FIELD_GAP: f32 = 16.0;
/// The grid's horizontal padding, per side.
const PADDING: f32 = 8.0;
/// Width reserved for the scrollbar, which overlays the list rather than taking layout space.
const SCROLLBAR: f32 = 14.0;
/// Width of the find bar's query box.
///
/// Wide enough for sixteen hex bytes with their separating spaces, which is a longer needle than
/// anyone searches a CBOR document for.
const QUERY_WIDTH: f32 = 220.0;

/// Which of the two fields the caret is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Hex,
    Ascii,
}

/// The caret, and the selection it is one end of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    /// The byte being edited.
    offset: usize,
    /// Where the selection being held started, if one is.
    ///
    /// The *anchor* and not the selection's low end: a selection extended leftwards has its anchor
    /// after the caret, and [`Cursor::selection`] is what normalises the two. `None` means no
    /// selection at all, which is a different thing from a one-byte selection — [`HexView::copy`]
    /// takes the whole buffer in the first case and one byte in the second.
    anchor: Option<usize>,
    /// Which field has the focus. Only this one accepts typing; the other draws a faint caret.
    field: Field,
    /// Whether the next hex digit typed lands in the low nibble.
    ///
    /// A byte takes two keystrokes, and the first one is applied immediately — so a half-typed
    /// byte is a visible state, not a hidden one. Any caret movement abandons it.
    low: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            offset: 0,
            anchor: None,
            field: Field::Hex,
            low: false,
        }
    }
}

impl Cursor {
    /// The selected byte range, **including** the byte under the caret.
    ///
    /// Inclusive because a caret in a hex grid sits *on* a byte rather than between two: with shift
    /// held and no movement yet, the one byte under the caret is what is selected. An exclusive end
    /// would make that an empty range and `Copy` would take nothing from it.
    fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        Some(anchor.min(self.offset)..anchor.max(self.offset) + 1)
    }

    /// The anchor a gesture should carry forward.
    ///
    /// Extending keeps an anchor that is already set and otherwise plants one where the caret *is*,
    /// so the first shift-arrow selects the byte it leaves as well as the byte it arrives on. Not
    /// extending clears it, which is what makes a plain arrow key or a plain click drop the
    /// selection.
    fn anchor_after(&self, extend: bool) -> Option<usize> {
        match extend {
            true => Some(self.anchor.unwrap_or(self.offset)),
            false => None,
        }
    }
}

/// A caret movement, named so that [`step`] can be tested without a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    RowStart,
    RowEnd,
    PageUp,
    PageDown,
}

/// What the hex view tells [`crate::Ide`] when the buffer changes under it.
pub enum HexEvent {
    /// A byte was written. The buffer's well-formedness, its dirty flag, and therefore the
    /// explorer's bold-red set and the `Save` menu item all have to be re-derived.
    Edited,
    /// A write or a paste was refused, with the reason.
    ///
    /// A *keystroke* reaching here is unreachable while [`HexView::write`] checks `read_only`
    /// first and [`step`] keeps the caret inside the buffer, which is exactly why it is reported
    /// rather than discarded: if it ever fires, one of those two guarantees has broken. A refused
    /// **paste** is the ordinary case — the clipboard is not under those guarantees, so
    /// [`HexView::paste`] has three refusals of its own and each names its reason.
    Failed(String),
    /// Something worth telling the user that is not a failure: how many bytes went to the
    /// clipboard, how many came back from it.
    ///
    /// The grid has no other way to report a copy. Nothing visible changes, so without this a
    /// `Copy` and a `Copy` that silently took nothing look identical.
    Noted(String),
}

/// Why a byte range is highlighted on the grid.
///
/// Three independent layers rather than one list, because three different things write to them and
/// at three different rates: a decode error after every keystroke, a validation failure on
/// `Validate`, and a jump when a trace or diagnostic row is clicked. Sharing one list between two
/// kinds is the bug `tps_cbor_gui/PLAN.md:521-525` and `:574-577` record being introduced twice —
/// whichever kind wrote last silently erased the other.
///
/// The variants are ordered by precedence: where ranges overlap, the **last** wins, because it is
/// the one the user's most recent gesture put there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkKind {
    /// Where the find bar's query matched. Written by [`HexView::recompute_search`].
    ///
    /// Declared first, so it is the layer everything else draws over. A search can match hundreds
    /// of ranges across the whole document, and a decode error has to stay visible through them:
    /// the search is a question the user asked, the error is one the document answered.
    Search,
    /// The CBOR decode error's span. Written by `Ide::recheck_cbor` after every byte.
    Decode,
    /// A CDDL validation failure's span. Written by `Validate`.
    Validation,
    /// Where a clicked trace or diagnostic row points. Written by the reveal.
    Jump,
}

impl MarkKind {
    /// Every kind, in precedence order, so a caller clearing all layers cannot miss one.
    pub const ALL: [MarkKind; 4] = [
        MarkKind::Search,
        MarkKind::Decode,
        MarkKind::Validation,
        MarkKind::Jump,
    ];
}

/// One layer of marks: a kind and the ranges highlighted for it.
///
/// A `Vec` of these rather than a map, because there are three and the row closure walks all of
/// them per cell — an ordered slice is both the cheapest and the one that makes "last wins" the
/// obvious reading of the code.
type Marks = Vec<(MarkKind, Vec<Range<usize>>)>;

/// The find bar's state, present exactly while the bar is open.
///
/// An `Option<HexSearch>` on the view rather than a `bool` beside four always-present fields,
/// because closing the bar has to drop the subscription on its query box — and a `None` that
/// cannot hold one is the only shape where forgetting that is impossible.
struct HexSearch {
    /// The query box. An [`InputState`] and not a `String` because a text box needs a caret, a
    /// selection and a clipboard of its own, and that is what the toolkit's input already is.
    query: Entity<InputState>,
    /// Whether the query is read as hex digits or as literal text.
    ///
    /// Hex by default: this is a hex editor and the usual question is *where is this byte
    /// sequence*. Text mode is what makes the bar useful on a CBOR document with string keys in
    /// it, where the user knows the key and not its encoding.
    hex: bool,
    /// Every match, ascending by start offset, as [`HexView::recompute_search`] last left them.
    matches: Vec<Range<usize>>,
    /// Which match the caret was last taken to. Meaningless, and ignored, when `matches` is empty.
    current: usize,
    /// Why the query named no bytes, when that is why there are no matches.
    ///
    /// Only hex mode can produce this, and mid-typing it routinely does: `a1 0` is an odd digit
    /// count. Shown in the bar rather than pushed into the diagnostics list, which would otherwise
    /// gain a row per keystroke.
    error: Option<String>,
    /// Whether the query box still has to be given the keyboard, honoured once by
    /// [`HexView::pending_search_focus`] from the next render.
    ///
    /// It exists because [`HexView::open_search`]'s own focus call is **not enough when the bar was
    /// opened from the `Edit` menu**: a menu popup restores the focus it took when it closes, and
    /// that happens *after* the action it dispatched has run, so the focus lands back on the grid a
    /// moment later. The grid writes hex digits into the document, so the symptom was not a find
    /// bar that ignored the keyboard but a document quietly overwritten by the query.
    focus_pending: bool,
    /// Keeps the query box's change subscription alive for as long as the bar is open.
    _subscription: Subscription,
}

/// The CBOR tab's upper window.
pub struct HexView {
    /// The shared model. See the module docs for why this is an entity.
    workspace: Entity<Workspace>,
    scroll: UniformListScrollHandle,
    cursor: Cursor,
    /// Byte ranges to highlight, one layer per [`MarkKind`], handed over by [`HexView::mark`].
    ///
    /// `Rc` because the row closure is rebuilt every frame and would otherwise clone the vectors.
    marks: Rc<Marks>,
    /// The find bar, while one is open. See [`HexSearch`].
    search: Option<HexSearch>,
    /// Bytes per row, from the last measured width.
    columns: usize,
    /// Rows per viewport, from the last measured height. Page up and page down use it.
    page_rows: usize,
    focus: FocusHandle,
}

impl EventEmitter<HexEvent> for HexView {}

impl HexView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            scroll: UniformListScrollHandle::new(),
            cursor: Cursor::default(),
            marks: Rc::new(MarkKind::ALL.map(|kind| (kind, Vec::new())).into()),
            search: None,
            // The classic width, used until the first frame reports a real one.
            columns: tps_cbor_core::convert::DEFAULT_BYTES_PER_ROW,
            page_rows: 1,
            focus: cx.focus_handle(),
        }
    }

    /// Replaces the highlighted byte ranges of **one** kind, leaving the other kinds alone.
    ///
    /// This is how `UI_DESIGN.md:77`'s "the failing byte range is highlighted" is delivered: the
    /// lower window shows the decode error and this puts it on the grid. Phase 6 added two more
    /// writers, which is why the layer is a parameter — see [`MarkKind`].
    ///
    /// Deliberately does *not* scroll — see [`HexView::reveal`]. It is called after every byte
    /// written, so it compares before notifying, **per layer**: a decode error that stays in the
    /// same place must not cost a redraw per keystroke, and that has to keep holding now that a
    /// validation mark can be sitting beside it unchanged.
    pub fn mark(&mut self, kind: MarkKind, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        let Some(layer) = self.marks.iter().position(|(known, _)| *known == kind) else {
            return;
        };
        if self.marks[layer].1 == ranges {
            return;
        }
        // One `Rc` per change rather than per frame; `Rc::make_mut` clones only when the row
        // closure of a frame still in flight is holding the old one.
        Rc::make_mut(&mut self.marks)[layer].1 = ranges;
        cx.notify();
    }

    /// Clears every mark layer, as an edit to the buffer must.
    ///
    /// A mark is a coordinate into the bytes as they were; after an edit every range past the
    /// insertion point names a different value. `Decode` is included because
    /// `Ide::recheck_cbor` writes it again immediately, and leaving the old one in place for that
    /// one frame flashes the previous error's position.
    pub fn clear_marks(&mut self, cx: &mut Context<Self>) {
        for kind in MarkKind::ALL {
            self.mark(kind, Vec::new(), cx);
        }
        // `Search` is cleared by the loop above along with the rest, and then immediately put back
        // from the *new* bytes. Recomputing rather than leaving it cleared is what keeps the bar's
        // "3 of 17" from counting tints that are no longer on the grid: the query is still what the
        // user asked, so the honest answer after an edit is the answer to it now.
        self.recompute_search(cx);
    }

    /// Scrolls `offset` into view, centred.
    ///
    /// Separate from [`HexView::mark`] because marking happens after every keystroke, and
    /// scrolling there would drag the viewport away from the caret mid-edit. An explicit
    /// `Validate`, which is the only caller, should take the user to what it found — in a 1 MB
    /// file that is otherwise 65,000 rows away.
    pub fn reveal(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.scroll
            .scroll_to_item(offset / self.columns.max(1), ScrollStrategy::Center);
        cx.notify();
    }

    /// Focuses the grid, so that the keyboard reaches it.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    fn len(&self, cx: &App) -> usize {
        self.workspace.read(cx).cbor.len()
    }

    fn read_only(&self, cx: &App) -> bool {
        self.workspace.read(cx).cbor.meta.read_only
    }

    /// The byte under the caret, or `None` when the buffer is empty.
    fn byte(&self, cx: &App) -> Option<u8> {
        self.workspace
            .read(cx)
            .cbor
            .bytes()
            .get(self.cursor.offset)
            .copied()
    }

    /// The one route from a keystroke to the buffer.
    ///
    /// `read_only` is checked here rather than relying on `set_byte`'s own check, so that typing
    /// into a read-only document is simply ignored — the way a read-only editor ignores it — rather
    /// than appending a diagnostic per keystroke. The state is visible: the right pane has the
    /// checkbox and the status line below the grid says so.
    fn write(&mut self, offset: usize, value: u8, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = workspace.cbor.set_byte(offset, value);
            cx.notify();
            outcome
        });

        match outcome {
            Ok(()) => cx.emit(HexEvent::Edited),
            Err(error) => cx.emit(HexEvent::Failed(error.to_string())),
        }
        cx.notify();
    }

    /// Writes `bytes` over the buffer from `offset`, as **one** edit.
    ///
    /// One [`HexEvent::Edited`] for the whole run and not one per byte: every `Edited` re-decodes
    /// the buffer, re-derives the EDN tab, rebuilds the explorer's dirty set and rebuilds the
    /// menus, so a 200-byte paste would do all of that 200 times.
    ///
    /// The range is the caller's responsibility ([`overrun`]); `set_byte`'s own bounds check is
    /// still the backstop, and its first failure stops the run. A half-applied paste reported as a
    /// success is the one outcome worse than a refused one, so the failure is emitted rather than
    /// swallowed — and `Edited` is emitted too, because the bytes that did land are real.
    fn write_all(&mut self, offset: usize, bytes: &[u8], cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        let outcome = self.workspace.update(cx, |workspace, cx| {
            let outcome = bytes
                .iter()
                .enumerate()
                .try_for_each(|(index, byte)| workspace.cbor.set_byte(offset + index, *byte));
            cx.notify();
            outcome
        });

        cx.emit(HexEvent::Edited);
        if let Err(error) = outcome {
            cx.emit(HexEvent::Failed(error.to_string()));
        }
        cx.notify();
    }

    /// `Edit → Copy`: the selection as hex, or the whole buffer when nothing is selected.
    ///
    /// Whole-buffer rather than nothing, because `UI_DESIGN.md:51` has `Copy` always offered and
    /// `menus.rs`'s `quit_help_and_find_are_always_offered` asserts it is always *enabled* — an
    /// always-enabled item that does nothing on a document with no selection in it is worse than
    /// no item at all. Recorded in `PLAN.md` as an addition.
    ///
    /// The format is [`convert::to_hex`]'s, which is exactly what [`HexView::paste`] accepts, so
    /// bytes copied out of one document paste into another.
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        let len = self.len(cx);
        if len == 0 {
            return cx.emit(HexEvent::Failed(
                "there is no CBOR document to copy from".to_owned(),
            ));
        }

        // Clamped rather than trusted: a selection is a pair of offsets into the buffer as it was,
        // and `To CBOR` or an `Open` can have shortened it since.
        let range = match self.cursor.selection() {
            Some(range) => range.start.min(len)..range.end.min(len),
            None => 0..len,
        };
        let bytes = self.workspace.read(cx).cbor.bytes()[range.clone()].to_vec();
        cx.write_to_clipboard(ClipboardItem::new_string(convert::to_hex(&bytes)));
        cx.emit(HexEvent::Noted(format!(
            "copied {} bytes as hex, from offset 0x{:08x}",
            bytes.len(),
            range.start
        )));
    }

    /// `Edit → Paste`: overwrites the bytes at the caret with the clipboard's hex.
    ///
    /// **Overwrite and not insert.** `UI_DESIGN.md:77` says the hex and ASCII fields "can be
    /// changed in-place", which makes overwrite the specified editing model for this grid — and it
    /// is why no splice API on `CborDoc` is needed. Recorded in `PLAN.md` as a divergence from what
    /// a text editor's `Paste` would do.
    ///
    /// Four refusals, each naming a reason the user can act on instead of a silent no-op: no
    /// document, a read-only document, text that is not hex, and a paste that would run past the
    /// end of the buffer.
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let len = self.len(cx);
        if len == 0 {
            return cx.emit(HexEvent::Failed(
                "there is no CBOR document to paste into".to_owned(),
            ));
        }
        if self.read_only(cx) {
            return cx.emit(HexEvent::Failed(
                "the CBOR document is read-only, so nothing was pasted".to_owned(),
            ));
        }

        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return cx.emit(HexEvent::Failed(
                "the clipboard holds no text to paste".to_owned(),
            ));
        };

        let bytes = match convert::from_hex(&text) {
            Ok(bytes) => bytes,
            // The clipboard's content is quoted back, trimmed: the user may have copied it from
            // somewhere else entirely and the fastest way to see that is to be shown it.
            Err(error) => {
                return cx.emit(HexEvent::Failed(format!(
                    "the clipboard is not hex, so nothing was pasted: {error} of `{}`",
                    elide(&text)
                )));
            }
        };
        if bytes.is_empty() {
            return cx.emit(HexEvent::Failed(
                "the clipboard holds no hex digits, so nothing was pasted".to_owned(),
            ));
        }

        let offset = self.cursor.offset;
        if let Some(refusal) = overrun(offset, bytes.len(), len) {
            return cx.emit(HexEvent::Failed(refusal));
        }

        self.write_all(offset, &bytes, cx);
        // The pasted range is left selected, so what just landed is visible as a block and a second
        // Copy takes it back. The caret stays where the paste started, which is where another paste
        // of the same bytes would go.
        self.cursor.anchor = Some(offset + bytes.len() - 1);
        cx.emit(HexEvent::Noted(format!(
            "pasted {} bytes over offset 0x{offset:08x}",
            bytes.len()
        )));
        cx.notify();
    }

    /// `Edit → Find...`: opens the find bar, or re-focuses the one already open.
    ///
    /// Takes a `&mut Window` because [`InputState::new`] needs one and because the query box has to
    /// be given the keyboard — a find bar that opens without the focus is a find bar the user has
    /// to click. Re-opening deliberately keeps the existing query: `Find` twice in a row is how a
    /// search is resumed.
    ///
    /// The focus is given **twice**, here and again from the next render through
    /// [`HexSearch::focus_pending`]. The second one is the one that survives a menu dismissal; see
    /// that field for why. Doing it here as well is what keeps `ctrl-f` immediate.
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            let query =
                cx.new(|cx| InputState::new(window, cx).placeholder("hex bytes, e.g. a1 01"));
            // The subscription is held by the `HexSearch` and so dies with it. Incremental: every
            // keystroke re-searches and takes the caret to the first match, which is what makes the
            // bar worth having on a 65,000-row document.
            let subscription = cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_search(cx);
                }
            });
            self.search = Some(HexSearch {
                query,
                hex: true,
                matches: Vec::new(),
                current: 0,
                error: None,
                focus_pending: true,
                _subscription: subscription,
            });
        }

        if let Some(search) = &mut self.search {
            // Set on a bar that was already open too: `Find` on an open bar is how the user asks
            // for the keyboard back, and that request has to survive the same menu dismissal.
            search.focus_pending = true;
            let handle = search.query.read(cx).focus_handle(cx).clone();
            window.focus(&handle, cx);
        }
        self.refresh_search(cx);
    }

    /// The query box's focus handle, the first time it is asked for after the find bar was opened.
    ///
    /// Called from `render`, where it is the only chance to take the focus back after a menu popup
    /// has restored it to the grid. One-shot: returning the handle on every frame would mean the
    /// find bar could never be left, since a click into the grid would be undone by the paint that
    /// followed it.
    fn pending_search_focus(&mut self, cx: &App) -> Option<FocusHandle> {
        let search = self.search.as_mut()?;
        if !search.focus_pending {
            return None;
        }
        search.focus_pending = false;
        Some(search.query.read(cx).focus_handle(cx).clone())
    }

    /// Closes the find bar, dropping its matches and its tints with it.
    ///
    /// The focus goes back to the grid: the bar is where the keyboard was, and leaving it on a box
    /// that is no longer rendered would leave the arrow keys doing nothing.
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.mark(MarkKind::Search, Vec::new(), cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Switches the query between hex digits and literal text, re-running it.
    fn set_search_hex(&mut self, hex: bool, cx: &mut Context<Self>) {
        if let Some(search) = &mut self.search {
            search.hex = hex;
            self.refresh_search(cx);
        }
    }

    /// `Edit → Find Next` / `Find Previous`: takes the caret to the next match, wrapping.
    ///
    /// Wrapping rather than stopping at the ends, because the grid gives no sense of where in a
    /// 65,000-row file the caret is, and a `Find Next` that silently does nothing at the last match
    /// is indistinguishable from one that is broken.
    ///
    /// With no matches this does nothing at all: the bar already says so, and a diagnostic per
    /// keypress would bury the list. With no *bar* it says so once, because there the user has
    /// pressed `F3` with nothing to advance and nothing on screen tells them why.
    pub fn step_search(&mut self, forwards: bool, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return cx.emit(HexEvent::Noted(
                "there is no search to advance; `Edit -> Find...` starts one".to_owned(),
            ));
        };
        if search.matches.is_empty() {
            return;
        }
        search.current = wrap(search.current, search.matches.len(), forwards);
        self.go_to_current_match(cx);
    }

    /// Re-runs the query and takes the caret to the first match.
    ///
    /// The incremental path: called on every keystroke in the query box and when the bar opens.
    /// Distinct from [`HexView::recompute_search`], which must *not* move the caret — that one runs
    /// after an edit, where dragging the viewport away from what the user is typing would be the
    /// opposite of helpful.
    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        self.recompute_search(cx);
        if let Some(search) = &mut self.search {
            search.current = 0;
            if !search.matches.is_empty() {
                self.go_to_current_match(cx);
            }
        }
    }

    /// Re-runs the query against the bytes as they now are, leaving the caret alone.
    ///
    /// Also the edit path, through [`HexView::clear_marks`]: the matches are coordinates into the
    /// bytes, so an edit invalidates them and the only honest replacement is the answer to the same
    /// query now.
    fn recompute_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &self.search else {
            return;
        };

        let query = search.query.read(cx).value().to_string();
        let (needle, error) = match needle_for(&query, search.hex) {
            Ok(needle) => (needle, None),
            Err(message) => (Vec::new(), Some(message)),
        };
        let matches = find_all(self.workspace.read(cx).cbor.bytes(), &needle);

        let ranges = matches.clone();
        if let Some(search) = &mut self.search {
            // Clamped rather than reset: a recompute after an edit usually finds the same matches,
            // and sending the user back to the first one every time they type a byte would make the
            // bar unusable while editing.
            search.current = search.current.min(matches.len().saturating_sub(1));
            search.matches = matches;
            search.error = error;
        }
        self.mark(MarkKind::Search, ranges, cx);
        cx.notify();
    }

    /// Puts the caret on the first byte of the current match and scrolls it into view.
    ///
    /// The caret *is* the current-match indicator — the [`MarkKind::Search`] layer already shows
    /// every match's extent, so a fifth layer for "this one" would say nothing the caret does not.
    fn go_to_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(offset) = self
            .search
            .as_ref()
            .and_then(|search| search.matches.get(search.current))
            .map(|range| range.start)
        else {
            return;
        };
        self.cursor.anchor = None;
        self.cursor.offset = offset;
        self.cursor.low = false;
        self.reveal(offset, cx);
    }

    /// Moves the caret and keeps it on screen, abandoning any half-typed byte.
    ///
    /// `extend` is the shift key: it carries the selection anchor forward instead of dropping it,
    /// so shift-arrow grows a selection and a plain arrow collapses one. See
    /// [`Cursor::anchor_after`].
    fn move_to(&mut self, motion: Motion, extend: bool, cx: &mut Context<Self>) {
        let len = self.len(cx);
        self.cursor.anchor = self.cursor.anchor_after(extend);
        self.cursor.offset = step(
            self.cursor.offset,
            len,
            self.columns,
            self.page_rows,
            motion,
        );
        self.cursor.low = false;
        // Non-strict, so a caret that is already visible does not drag the viewport about.
        self.scroll.scroll_to_item(
            self.cursor.offset / self.columns.max(1),
            ScrollStrategy::Top,
        );
        cx.notify();
    }

    /// Puts the caret on a clicked cell, in the field that was clicked.
    ///
    /// With shift held the click *extends* instead, which is the whole selection gesture the grid
    /// offers by mouse: drag-select is a follow-up, recorded in `PLAN.md`. Shift-click reaches
    /// anything a drag would and costs no mouse-move handling on a grid that rebuilds its rows
    /// from a virtual list.
    fn click(
        &mut self,
        offset: usize,
        field: Field,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.cursor = Cursor {
            offset: offset.min(self.len(cx).saturating_sub(1)),
            anchor: self.cursor.anchor_after(extend),
            field,
            low: false,
        };
        cx.notify();
    }

    /// Typing.
    ///
    /// Modified keystrokes are left alone: every one of them belongs to an action binding in
    /// [`crate::actions`], and claiming `ctrl-s` here would swallow `Save`. Plain `shift` is not a
    /// modifier in that sense — it is how capital letters are typed into the ASCII gutter.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function
        {
            return;
        }

        // Escape closes the find bar from either side of it. This listener is on the view's root
        // element, so a keystroke bubbles up to it from whichever of the grid and the query box has
        // the focus, and the bar is modeless — the gesture has to work from both.
        if keystroke.key == "escape" && self.search.is_some() {
            return self.close_search(window, cx);
        }

        // Everything below here moves the caret or writes to the buffer, and the same bubbling means
        // it would otherwise run for keystrokes aimed at the find bar: typing `a1` into the query
        // box would also write `0xa1` into the document. `is_focused` is exact — it is false while
        // a descendant has the focus, which is precisely the case to ignore.
        if !self.focus.is_focused(window) {
            return;
        }

        if let Some(motion) = motion_for(&keystroke.key) {
            return self.move_to(motion, keystroke.modifiers.shift, cx);
        }

        if keystroke.key == "tab" {
            self.cursor.field = match self.cursor.field {
                Field::Hex => Field::Ascii,
                Field::Ascii => Field::Hex,
            };
            self.cursor.low = false;
            return cx.notify();
        }

        let len = self.len(cx);
        if len == 0 {
            return;
        }

        match self.cursor.field {
            Field::Hex => {
                let Some(digit) = hex_digit(&keystroke.key) else {
                    return;
                };
                // A buffer cannot shrink under the caret here, but `unwrap_or` beats a panic if it
                // ever does: the write below is range-checked regardless.
                let byte = self.byte(cx).unwrap_or(0);
                let low = self.cursor.low;
                self.write(self.cursor.offset, nibble(byte, digit, low), cx);
                // The high nibble leaves the caret in place for its partner; the low one completes
                // the byte and moves on.
                if low {
                    self.move_to(Motion::Right, false, cx);
                } else {
                    self.cursor.low = true;
                    cx.notify();
                }
            }
            Field::Ascii => {
                // `key_char` rather than `key`: it is the character the layout would actually have
                // typed, so a keyboard that is not US-ASCII writes what its keys print.
                let Some([byte]) = keystroke.key_char.as_deref().map(str::as_bytes) else {
                    return;
                };
                if printable(*byte) == '.' {
                    return;
                }
                self.write(self.cursor.offset, *byte, cx);
                self.move_to(Motion::Right, false, cx);
            }
        }
    }

    /// The column ruler, which lines the grid's two fields up with a label each.
    fn header(&self, colours: Colours, cx: &App) -> AnyElement {
        let mut hex = h_flex().flex_none();
        for group in 0..self.columns / GROUP {
            let mut cells = h_flex().flex_none();
            for column in 0..GROUP {
                let index = group * GROUP + column;
                cells = cells.child(
                    div()
                        .w(px(HEX_CELL))
                        .flex_none()
                        .text_center()
                        .child(format!("{index:02x}")),
                );
            }
            hex = hex.child(cells.mr(px(GROUP_GAP)));
        }

        h_flex()
            .flex_none()
            .h(px(ROW_HEIGHT))
            .px(px(PADDING))
            .text_color(colours.gutter)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().w(px(OFFSET_WIDTH)).flex_none().child("offset"))
            .child(hex)
            .child(div().w(px(FIELD_GAP)).flex_none())
            .child("ascii")
            .into_any_element()
    }

    /// The find bar, between the column ruler and the grid.
    ///
    /// Laid out on the ruler's grid — the label in the offset gutter's width — so that opening it
    /// does not shift the columns; only the grid below loses the rows it occupies.
    fn find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(search) = &self.search else {
            return div().into_any_element();
        };
        let (status, trouble) = match (&search.error, search.matches.len()) {
            (Some(error), _) => (error.clone(), true),
            (None, 0) => ("no matches".to_owned(), true),
            (None, total) => (format!("{} of {total}", search.current + 1), false),
        };
        let hex = search.hex;

        h_flex()
            .flex_none()
            .h(px(ROW_HEIGHT + 12.0))
            .items_center()
            .gap_2()
            .px(px(PADDING))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w(px(OFFSET_WIDTH))
                    .flex_none()
                    .text_color(cx.theme().muted_foreground)
                    .child("find"),
            )
            .child(
                div()
                    .w(px(QUERY_WIDTH))
                    .flex_none()
                    .child(Input::new(&search.query).cleanable(true)),
            )
            .child(
                Checkbox::new("hex-search-mode")
                    .label("hex")
                    .checked(hex)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.set_search_hex(*checked, cx);
                    })),
            )
            .child(
                div()
                    .text_color(match trouble {
                        true => cx.theme().muted_foreground,
                        false => cx.theme().foreground,
                    })
                    .child(status),
            )
            .child(div().flex_1())
            .child(
                Button::new("hex-search-close")
                    .ghost()
                    .label("Close")
                    .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
            )
            .into_any_element()
    }

    /// The scrolling grid, with its scrollbar overlaid.
    fn grid(&self, colours: Colours, cx: &mut Context<Self>) -> AnyElement {
        let rows = row_count(self.len(cx), self.columns);
        let columns = self.columns;
        let cursor = self.cursor;
        let marks = self.marks.clone();
        let workspace = self.workspace.clone();
        let view = cx.entity().downgrade();

        let list = uniform_list("hex-rows", rows, move |range, _, cx| {
            // One borrow of `cx`, which is why `colours` was resolved before the closure was built.
            let bytes = workspace.read(cx).cbor.bytes();
            range
                .map(|index| row(index, bytes, columns, cursor, &marks, colours, &view))
                .collect::<Vec<_>>()
        })
        .size_full()
        .track_scroll(&self.scroll);

        div()
            .relative()
            .flex_1()
            .overflow_hidden()
            .child(list)
            // The scrollbar is painted after the rows, not before: an overlay drawn first would
            // have its thumb and its hitbox covered by the cell backgrounds.
            .child(
                div().absolute().inset_0().child(
                    Scrollbar::vertical(&self.scroll)
                        .id("hex-scroll")
                        .viewport_from_layout(),
                ),
            )
            .into_any_element()
    }

    /// The status line: what the caret is on, how big the buffer is, and whether it can be edited.
    ///
    /// The read-only state is shown here as well as on the right pane's checkbox because this is
    /// where the user finds out why their typing did nothing, and the right pane can be hidden
    /// (`UI_DESIGN.md:64`).
    fn status(&self, cx: &App) -> AnyElement {
        let len = self.len(cx);
        h_flex()
            .flex_none()
            .h(px(ROW_HEIGHT + 4.0))
            .items_center()
            .gap_3()
            .px(px(PADDING))
            .border_t_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(format!("{len} bytes"))
            .when(len > 0, |this| {
                this.child(format!("offset 0x{:08x}", self.cursor.offset))
                    .child(format!("{} per row", self.columns))
            })
            // Shown as a count rather than as two offsets, because the count is what the user is
            // about to copy and the offsets are already on the grid.
            .when_some(self.cursor.selection(), |this, range| {
                this.child(format!(
                    "selected {} bytes from 0x{:08x}",
                    range.len(),
                    range.start
                ))
            })
            .child(div().flex_1())
            .when(self.read_only(cx), |this| {
                this.child(div().text_color(cx.theme().warning).child("read-only"))
            })
            .into_any_element()
    }
}

impl Focusable for HexView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HexView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Before anything is built, because it is the one thing here that is not about this frame's
        // elements: the find bar's query box asking for the keyboard a frame after it was opened.
        // See [`HexSearch::focus_pending`].
        if let Some(handle) = self.pending_search_focus(cx) {
            window.focus(&handle, cx);
        }

        // A new document may be shorter than the last, so the caret and the selection anchor are
        // clamped on the way in rather than trusted from the last keystroke.
        let len = self.len(cx);
        self.cursor.offset = self.cursor.offset.min(len.saturating_sub(1));
        self.cursor.anchor = self
            .cursor
            .anchor
            .map(|anchor| anchor.min(len.saturating_sub(1)));

        let colours = Colours::of(cx);
        // Built before the chain rather than inside a `when` closure, which would hold a borrow of
        // `cx` across the rest of it.
        let find_bar = self.search.is_some().then(|| self.find_bar(cx));
        let view = cx.entity().downgrade();

        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .font_family(cx.theme().mono_font_family.clone())
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.header(colours, cx))
            .when_some(find_bar, ParentElement::child)
            .map(|this| {
                if len == 0 {
                    this.child(
                        div()
                            .flex_1()
                            .p(px(PADDING))
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "No CBOR document. Open a file, or convert one from the EDN tab.",
                            ),
                    )
                } else {
                    this.child(self.grid(colours, cx))
                }
            })
            .child(self.status(cx))
            // Measured on the way out, applied on the way in: `on_prepaint` runs during layout, so
            // a change here is visible on the next frame. `notify` is called only when the derived
            // numbers actually change, because notifying unconditionally from prepaint is an
            // endless redraw.
            .on_prepaint(move |bounds, _, cx| {
                let columns = columns_for_width(bounds.size.width);
                let page_rows = page_rows_for_height(bounds.size.height);
                let _ = view.update(cx, |this, cx| {
                    if this.columns == columns && this.page_rows == page_rows {
                        return;
                    }
                    this.columns = columns;
                    this.page_rows = page_rows;
                    cx.notify();
                });
            })
    }
}

/// The colours a row needs, resolved once per frame.
///
/// The `uniform_list` row closure is `'static` and receives `&mut App`, which it has already lent
/// out immutably to read the buffer — so it cannot also ask the theme. Resolving first and
/// capturing by value keeps the closure to that one borrow.
#[derive(Debug, Clone, Copy)]
struct Colours {
    /// The offset gutter and the column ruler.
    gutter: Hsla,
    /// The caret, in the field that has the focus.
    cursor: Hsla,
    cursor_text: Hsla,
    /// The caret's twin, in the field that does not.
    paired: Hsla,
    /// A byte inside the selection, in both fields.
    selection: Hsla,
    /// A byte inside a [`MarkKind::Search`] range: the find bar's query matched here.
    search: Hsla,
    /// A byte inside a [`MarkKind::Decode`] range: a malformed CBOR item.
    decode: Hsla,
    /// A byte inside a [`MarkKind::Validation`] range: well-formed, but not what the schema said.
    validation: Hsla,
    /// A byte inside a [`MarkKind::Jump`] range: where a clicked row pointed.
    jump: Hsla,
    /// `00`, dimmed. A page of padding is much easier to read past when it recedes.
    zero: Hsla,
    /// The `.` standing in for an unprintable byte, dimmed for the same reason.
    unprintable: Hsla,
}

impl Colours {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            gutter: theme.muted_foreground,
            // `selection` and not `accent`: in the default dark theme `accent` is `neutral-800`
            // against a `neutral-950` background, which left the caret invisible on the grid —
            // only the brighter `accent_foreground` text hinted at where it was. `selection` is
            // the theme's "this is the bit you are working on" colour and reads at a glance in
            // both modes.
            cursor: theme.selection,
            // `selection` has no paired foreground in the theme, and it is dark enough in both
            // modes that the ordinary body colour is the readable choice over it.
            cursor_text: theme.foreground,
            paired: theme.selection.opacity(0.4),
            // A faint wash of the caret's own colour, and deliberately fainter than the *paired*
            // caret: a selection can be the whole screen, and both carets have to stay findable
            // inside it. Same hue because it is the same gesture at a different extent.
            selection: theme.selection.opacity(0.22),
            // `success` is the one semantic colour not already spoken for by the other three
            // layers, and a search result is neither a fault nor a finding — it is just the answer
            // to a question.
            search: theme.success.opacity(0.3),
            decode: theme.danger.opacity(0.3),
            // Distinct from `decode` so that "these bytes are not CBOR" and "these bytes are CBOR
            // the schema rejected" do not look like the same finding. Warning rather than danger
            // for the same reason: the buffer itself is intact.
            validation: theme.warning.opacity(0.3),
            // `info`, and stronger than the other two, because a jump is the answer to a click the
            // user has just made and has to be findable on a screen that may hold both others.
            jump: theme.info.opacity(0.45),
            zero: theme.muted_foreground.opacity(0.6),
            unprintable: theme.muted_foreground.opacity(0.6),
        }
    }

    /// The background colour a [`Fill`] is drawn in, and the text colour it needs over it.
    ///
    /// Only the caret overrides the text colour: `selection` has no paired foreground in the theme
    /// and the mark layers are all translucent washes, so the ordinary body colour reads over every
    /// one of them.
    fn fill(self, fill: Fill) -> Option<(Hsla, Option<Hsla>)> {
        Some(match fill {
            Fill::Caret => (self.cursor, Some(self.cursor_text)),
            Fill::Paired => (self.paired, None),
            Fill::Selected => (self.selection, None),
            Fill::Marked(MarkKind::Search) => (self.search, None),
            Fill::Marked(MarkKind::Decode) => (self.decode, None),
            Fill::Marked(MarkKind::Validation) => (self.validation, None),
            Fill::Marked(MarkKind::Jump) => (self.jump, None),
            Fill::None => return None,
        })
    }
}

/// What a cell's background says about it.
///
/// An enum and not a colour, so that [`fill_for`] is pure, theme-free and testable: the precedence
/// between these is the decision worth pinning, and the colour each one resolves to is a separate
/// and much less dangerous choice ([`Colours::fill`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// The caret, in the field that has the focus.
    Caret,
    /// The caret's twin, in the field that does not.
    Paired,
    /// Inside the selection.
    Selected,
    /// Inside a mark layer, and which one.
    Marked(MarkKind),
    /// Nothing: the cell takes the pane's own background.
    None,
}

/// How one cell is filled.
///
/// **Caret above selection above mark**, which is the order of how directly each answers something
/// the user just did: the caret is where the next keystroke lands, the selection is the gesture in
/// progress, and a mark is a finding from a pass that has already finished. Getting it backwards
/// hides the caret inside its own selection, or hides a selection under a screen of search hits.
///
/// Pure because the rule is otherwise spread across a chain of `when`s inside a render closure,
/// where it can only be checked by looking at it.
fn fill_for(offset: usize, field: Field, cursor: Cursor, marked: Option<MarkKind>) -> Fill {
    if cursor.offset == offset {
        return match cursor.field == field {
            true => Fill::Caret,
            false => Fill::Paired,
        };
    }
    if cursor
        .selection()
        .is_some_and(|range| range.contains(&offset))
    {
        return Fill::Selected;
    }
    match marked {
        Some(kind) => Fill::Marked(kind),
        None => Fill::None,
    }
}

/// The highest-precedence kind marking `offset`, or `None` if nothing does.
///
/// A free function so that the precedence rule is testable without a window: it decides what the
/// user sees where a jump lands on top of a validation failure, and getting it backwards would
/// hide the thing they just clicked.
fn mark_at(marks: &Marks, offset: usize) -> Option<MarkKind> {
    marks
        .iter()
        .filter(|(_, ranges)| ranges.iter().any(|range| range.contains(&offset)))
        .map(|(kind, _)| *kind)
        .max()
}

/// One row: the offset, `columns` hex cells in groups of eight, and the ASCII gutter.
///
/// A free function rather than a method because the row closure cannot hold `&self` — it outlives
/// the `render` call that built it.
#[allow(clippy::too_many_arguments)]
fn row(
    index: usize,
    bytes: &[u8],
    columns: usize,
    cursor: Cursor,
    marks: &Marks,
    colours: Colours,
    view: &WeakEntity<HexView>,
) -> AnyElement {
    let start = index * columns;

    let mut hex = h_flex().flex_none();
    let mut ascii = h_flex().flex_none();
    for group in 0..columns / GROUP {
        let mut hex_group = h_flex().flex_none();
        let mut ascii_group = h_flex().flex_none();
        for column in 0..GROUP {
            let offset = start + group * GROUP + column;
            let byte = bytes.get(offset).copied();
            let marked = mark_at(marks, offset);
            hex_group = hex_group.child(cell(
                offset,
                byte,
                Field::Hex,
                cursor,
                marked,
                colours,
                view,
            ));
            ascii_group = ascii_group.child(cell(
                offset,
                byte,
                Field::Ascii,
                cursor,
                marked,
                colours,
                view,
            ));
        }
        hex = hex.child(hex_group.mr(px(GROUP_GAP)));
        ascii = ascii.child(ascii_group.mr(px(GROUP_GAP)));
    }

    h_flex()
        .h(px(ROW_HEIGHT))
        .flex_none()
        .px(px(PADDING))
        .child(
            div()
                .w(px(OFFSET_WIDTH))
                .flex_none()
                .text_color(colours.gutter)
                .child(format!("{start:08x}")),
        )
        .child(hex)
        .child(div().w(px(FIELD_GAP)).flex_none())
        .child(ascii)
        .into_any_element()
}

/// One cell, in either field.
///
/// Cells past the end of the buffer are rendered blank but full width, so a short final row keeps
/// the ASCII gutter where the rows above put it.
#[allow(clippy::too_many_arguments)]
fn cell(
    offset: usize,
    byte: Option<u8>,
    field: Field,
    cursor: Cursor,
    marked: Option<MarkKind>,
    colours: Colours,
    view: &WeakEntity<HexView>,
) -> AnyElement {
    let (width, text, dim) = match (field, byte) {
        (Field::Hex, Some(byte)) => (HEX_CELL, format!("{byte:02x}"), byte == 0),
        (Field::Hex, None) => (HEX_CELL, String::new(), false),
        (Field::Ascii, Some(byte)) => {
            let character = printable(byte);
            (ASCII_CELL, character.to_string(), character == '.')
        }
        (Field::Ascii, None) => (ASCII_CELL, String::new(), false),
    };

    // Past the end of the buffer there is nothing to be the caret on, nothing to select, and a mark
    // there would be a stale span — so a blank cell stays blank rather than carrying a tint that
    // names no byte.
    let fill = match byte {
        Some(_) => fill_for(offset, field, cursor, marked),
        None => Fill::None,
    };

    let mut element = div()
        .w(px(width))
        .flex_none()
        .when(field == Field::Hex, Styled::text_center)
        .when(dim, |this| {
            this.text_color(match field {
                Field::Hex => colours.zero,
                Field::Ascii => colours.unprintable,
            })
        })
        // The caret is drawn in both fields at once, solid where the focus is and faint in the
        // other. That is what the single `set_byte` write path buys: the two cells are visibly the
        // same byte, rather than two numbers that happen to agree.
        .when_some(colours.fill(fill), |this, (background, foreground)| {
            this.bg(background)
                .when_some(foreground, Styled::text_color)
        })
        .child(text);

    if byte.is_some() {
        let view = view.clone();
        element = element.on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                let extend = event.modifiers.shift;
                let _ = view.update(cx, |this, cx| this.click(offset, field, extend, window, cx));
            },
        );
    }
    element.into_any_element()
}

/// The refusal message for a paste that would not fit, or `None` when it fits.
///
/// Both lengths are named. The user is about to be told "no" about a gesture they have every reason
/// to think worked, and "6 bytes do not fit" without saying how much room there is leaves them
/// guessing — so the message carries the paste, the room, the offset and the document's size.
///
/// A paste that exactly reaches the end is allowed: `offset + pasted == len` writes the last byte,
/// not one past it.
fn overrun(offset: usize, pasted: usize, len: usize) -> Option<String> {
    let room = len.saturating_sub(offset);
    (pasted > room).then(|| {
        format!(
            "{pasted} bytes do not fit: there are {room} bytes from offset 0x{offset:08x} to the \
             end of a {len}-byte document, and paste is in-place. Nothing was written."
        )
    })
}

/// `text` shortened to something that can sit in a diagnostic row.
///
/// Whitespace is collapsed as well as the length cut, because the usual reason a paste is not hex
/// is that it is a wall of prose or a whole file, and either would otherwise push every other
/// column of the diagnostics list off the screen.
fn elide(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(ELIDE_AT) {
        Some((end, _)) => format!("{}...", &flat[..end]),
        None => flat,
    }
}

/// How many characters of unparseable clipboard text a diagnostic quotes back. See [`elide`].
const ELIDE_AT: usize = 40;

/// The index after — or before — `current` in a ring of `len`.
///
/// Wrapping is the point: see [`HexView::step_search`]. `len == 0` has no valid index at all and
/// gives zero, which the caller has already guarded against.
fn wrap(current: usize, len: usize, forwards: bool) -> usize {
    if len == 0 {
        return 0;
    }
    match forwards {
        true => (current + 1) % len,
        false => (current + len - 1) % len,
    }
}

/// The bytes a query names, or the reason it names none.
///
/// Hex mode goes through [`convert::from_hex`], so the find bar accepts exactly what `Copy` puts on
/// the clipboard and what `Paste` takes off it — one hex syntax across the whole tab. Mid-typing
/// that syntax is routinely invalid (`a1 0` is an odd digit count), which is why the failure is a
/// message for the bar rather than an error for the diagnostics list.
///
/// Text mode is the query's own UTF-8. No escapes: `\n` in the box searches for a backslash and an
/// `n`, because the alternative is a second syntax to learn in a field that already has one, and
/// hex mode covers any byte the user cannot type.
fn needle_for(query: &str, hex: bool) -> Result<Vec<u8>, String> {
    match hex {
        true => convert::from_hex(query).map_err(|error| error.to_string()),
        false => Ok(query.as_bytes().to_vec()),
    }
}

/// Every occurrence of `needle` in `haystack`, ascending by start offset.
///
/// **Overlapping occurrences are all reported**: `aa` in `aa aa aa` matches at 0, 1 and 2. The user
/// asked where those bytes are, and skipping the overlaps would leave the pattern visibly on screen
/// at an offset the search said was not a match.
///
/// An empty needle matches nothing rather than everything. Every offset matching is a tint over the
/// whole document and a match count equal to its length, which is no information at all — and it is
/// the state the bar is in for as long as the box is empty.
fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<Range<usize>> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&start| &haystack[start..start + needle.len()] == needle)
        .map(|start| start..start + needle.len())
        .collect()
}

/// How many bytes fit on one row at `width`, as a multiple of [`GROUP`].
///
/// `UI_DESIGN.md:74` asks for "multiples of n*8 bytes where n depends on the width of the window",
/// which this is. See the module docs for why the widths are constants rather than measurements.
fn columns_for_width(width: Pixels) -> usize {
    let overhead = PADDING * 2.0 + OFFSET_WIDTH + FIELD_GAP + SCROLLBAR;
    // The gap is counted after every group in both fields, including each field's last — so the
    // estimate errs by up to two gaps towards one group too few. That direction is the safe one:
    // it wastes a little width and never overflows into a horizontal scrollbar.
    let per_group = GROUP as f32 * (HEX_CELL + ASCII_CELL) + GROUP_GAP * 2.0;
    let groups = ((f32::from(width) - overhead) / per_group).floor();

    // A float cast to `usize` saturates at zero for negatives and for NaN, so the zero-sized
    // bounds of a pane that has not been laid out yet give one group rather than a panic.
    (groups as usize).max(1) * GROUP
}

/// How many rows fit in `height`, for page up and page down.
fn page_rows_for_height(height: Pixels) -> usize {
    // The header and the status line are not rows; subtracting them keeps a page scroll from
    // overshooting by two rows every time.
    let grid = f32::from(height) - ROW_HEIGHT * 2.0 - 4.0;
    ((grid / ROW_HEIGHT).floor() as usize).max(1)
}

/// How many rows `len` bytes occupy.
///
/// At least one, so that an empty buffer still has a well-defined row count — `uniform_list` with
/// a count of zero is fine, but the arithmetic above it is easier to reason about this way.
fn row_count(len: usize, columns: usize) -> usize {
    len.div_ceil(columns.max(1)).max(1)
}

/// A byte as the ASCII gutter shows it: itself if printable, `.` otherwise.
///
/// Printable means `0x20..=0x7e`, which includes space — a run of spaces in a text payload is
/// worth being able to see. Everything from `0x7f` up is `.` rather than decoded: the gutter has
/// to stay one column per byte, and UTF-8 would not.
fn printable(byte: u8) -> char {
    if (0x20..0x7f).contains(&byte) {
        byte as char
    } else {
        '.'
    }
}

/// The caret movement a key names, if it names one.
fn motion_for(key: &str) -> Option<Motion> {
    Some(match key {
        "left" => Motion::Left,
        "right" => Motion::Right,
        "up" => Motion::Up,
        "down" => Motion::Down,
        "home" => Motion::RowStart,
        "end" => Motion::RowEnd,
        "pageup" => Motion::PageUp,
        "pagedown" => Motion::PageDown,
        _ => return None,
    })
}

/// A hex digit's value, from a keystroke's `key`.
fn hex_digit(key: &str) -> Option<u8> {
    let [byte] = key.as_bytes() else {
        return None;
    };
    u8::try_from((*byte as char).to_digit(16)?).ok()
}

/// `byte` with one nibble replaced.
fn nibble(byte: u8, digit: u8, low: bool) -> u8 {
    if low {
        (byte & 0xf0) | (digit & 0x0f)
    } else {
        (byte & 0x0f) | (digit << 4)
    }
}

/// Where `motion` moves a caret at `offset`.
///
/// Every clamp in the view is here. That is deliberate: the caret is the only thing that decides
/// which offset gets written, so a caret that could leave the buffer would mean `set_byte` taking
/// an out-of-range offset on the next keystroke. `columns` and `page` are taken as arguments
/// rather than read from the view so that this is a pure function with a unit test.
fn step(offset: usize, len: usize, columns: usize, page: usize, motion: Motion) -> usize {
    let columns = columns.max(1);
    let last = len.saturating_sub(1);

    let moved = match motion {
        Motion::Left => offset.saturating_sub(1),
        Motion::Right => offset.saturating_add(1),
        // Up and down stay in the column they started in, and the first and last rows simply
        // refuse rather than jumping to the start or the end of the buffer.
        Motion::Up => offset.checked_sub(columns).unwrap_or(offset),
        Motion::Down => match offset.saturating_add(columns) {
            below if below > last => offset,
            below => below,
        },
        Motion::RowStart => offset - offset % columns,
        Motion::RowEnd => offset - offset % columns + columns - 1,
        Motion::PageUp => offset.saturating_sub(columns * page),
        Motion::PageDown => offset.saturating_add(columns * page),
    };
    moved.min(last)
}

#[cfg(test)]
mod tests {
    // Imported item by item rather than with `use super::*`, which is what gpui-kit's own
    // `lib.rs:98-99` asks for: the glob would pull in GPUI's `#[test]` attribute macro through this
    // module's `use gpui_kit::*`, shadow the standard one, and expand to itself until the compiler
    // gives up on the recursion limit.
    use super::{
        ASCII_CELL, Cursor, ELIDE_AT, FIELD_GAP, Field, Fill, GROUP, GROUP_GAP, HEX_CELL, MarkKind,
        Marks, Motion, OFFSET_WIDTH, PADDING, SCROLLBAR, columns_for_width, elide, fill_for,
        find_all, hex_digit, mark_at, motion_for, needle_for, nibble, overrun,
        page_rows_for_height, printable, row_count, step, wrap,
    };
    use gpui_kit::px;
    use std::ops::Range;
    use tps_cbor_core::CborDoc;

    /// One mark layer.
    ///
    /// Through a helper because clippy reads `vec![0..10]` as an attempt to build a `Vec<usize>`
    /// out of a range rather than a one-element `Vec<Range<usize>>`, and says so for every layer.
    fn layer(kind: MarkKind, span: Range<usize>) -> (MarkKind, Vec<Range<usize>>) {
        (kind, vec![span])
    }

    /// Where two layers cover one byte, the user sees the one their most recent gesture put there.
    /// Getting this backwards hides the thing they just clicked underneath a stale decode error.
    #[test]
    fn overlapping_hex_marks_resolve_to_the_most_recent_gesture() {
        let marks: Marks = vec![
            layer(MarkKind::Decode, 0..10),
            layer(MarkKind::Validation, 4..10),
            layer(MarkKind::Jump, 8..10),
        ];

        assert_eq!(mark_at(&marks, 0), Some(MarkKind::Decode));
        assert_eq!(mark_at(&marks, 4), Some(MarkKind::Validation));
        assert_eq!(mark_at(&marks, 8), Some(MarkKind::Jump));
        assert_eq!(mark_at(&marks, 10), None, "the range end is exclusive");
    }

    /// An empty layer must not shadow a populated one, which is the state every layer starts in
    /// and the one two of the three are in most of the time.
    #[test]
    fn an_empty_mark_layer_marks_nothing() {
        let marks: Marks = vec![
            layer(MarkKind::Decode, 0..4),
            (MarkKind::Validation, Vec::new()),
            (MarkKind::Jump, Vec::new()),
        ];
        assert_eq!(mark_at(&marks, 0), Some(MarkKind::Decode));

        let empty: Marks = MarkKind::ALL.map(|kind| (kind, Vec::new())).into();
        assert_eq!(mark_at(&empty, 0), None);
    }

    /// `MarkKind::ALL` is what `HexView::clear_marks` iterates and what the initial layer list is
    /// built from, so a variant missing from it is a layer that can never be written or cleared.
    #[test]
    fn every_mark_kind_is_in_all_and_in_precedence_order() {
        let mut sorted = MarkKind::ALL;
        sorted.sort();
        assert_eq!(MarkKind::ALL, sorted, "`ALL` must be in precedence order");

        // One entry per kind in the layer list the view starts with.
        let layers: Marks = MarkKind::ALL.map(|kind| (kind, Vec::new())).into();
        assert_eq!(layers.len(), MarkKind::ALL.len());
    }

    /// A cursor with an anchor on each side of the caret, for the selection tests.
    fn held(offset: usize, anchor: usize) -> Cursor {
        Cursor {
            offset,
            anchor: Some(anchor),
            ..Cursor::default()
        }
    }

    /// A selection dragged leftwards names the same bytes as the same one dragged rightwards. The
    /// anchor is stored raw, so this normalisation is the only thing standing between a backwards
    /// selection and a `Range` whose start is after its end — which would panic the moment
    /// `contains` or a slice touched it.
    #[test]
    fn selection_normalises_either_direction() {
        assert_eq!(held(9, 4).selection(), Some(4..10));
        assert_eq!(held(4, 9).selection(), Some(4..10));
    }

    /// Shift held with no movement yet selects the byte under the caret, and that is *not* the same
    /// state as no selection at all: `HexView::copy` takes one byte in the first case and the whole
    /// buffer in the second.
    #[test]
    fn a_selection_includes_the_byte_under_the_caret() {
        assert_eq!(held(7, 7).selection(), Some(7..8));
        assert_eq!(Cursor::default().selection(), None);
    }

    /// The first shift-arrow has to select the byte it *leaves* as well as the one it arrives on,
    /// which is what planting the anchor at the current offset does. A second one must not move the
    /// anchor, or the selection would never be more than two bytes long.
    #[test]
    fn extending_plants_the_anchor_once_and_dropping_it_clears_it() {
        let fresh = Cursor {
            offset: 5,
            ..Cursor::default()
        };
        assert_eq!(
            fresh.anchor_after(true),
            Some(5),
            "planted where the caret is"
        );
        assert_eq!(fresh.anchor_after(false), None);

        let holding = held(9, 5);
        assert_eq!(
            holding.anchor_after(true),
            Some(5),
            "the anchor must not move"
        );
        assert_eq!(holding.anchor_after(false), None, "a plain arrow drops it");
    }

    /// Caret above selection above mark, exhaustively over `MarkKind` so that a new layer cannot
    /// quietly outrank the user's own gesture. Getting this backwards hides the caret inside its
    /// own selection, or hides a selection under a screen of search hits.
    #[test]
    fn the_caret_fills_above_the_selection_and_the_selection_above_every_mark() {
        // Caret at 9, selection 4..10, and every byte in it also marked.
        let cursor = held(9, 4);
        for kind in MarkKind::ALL {
            assert_eq!(
                fill_for(9, Field::Hex, cursor, Some(kind)),
                Fill::Caret,
                "{kind:?} must not cover the caret"
            );
            assert_eq!(
                fill_for(9, Field::Ascii, cursor, Some(kind)),
                Fill::Paired,
                "{kind:?} must not cover the caret's twin"
            );
            assert_eq!(
                fill_for(6, Field::Hex, cursor, Some(kind)),
                Fill::Selected,
                "{kind:?} must not cover the selection"
            );
            // Outside the selection a mark is the only thing left, which is what makes the two
            // assertions above about precedence rather than about the mark being ignored.
            assert_eq!(
                fill_for(20, Field::Hex, cursor, Some(kind)),
                Fill::Marked(kind)
            );
        }

        let plain = Cursor::default();
        assert_eq!(fill_for(20, Field::Hex, plain, None), Fill::None);
        // The caret's own cell with no selection and no mark is still the caret.
        assert_eq!(fill_for(0, Field::Hex, plain, None), Fill::Caret);
    }

    /// Overlaps are all reported, and the three edges are all reachable: offset zero, the last
    /// byte, and a needle that cannot fit at all. An empty needle matching everything would tint
    /// the whole document for as long as the query box is empty.
    #[test]
    fn find_all_reports_every_occurrence_including_overlaps() {
        assert_eq!(
            find_all(&[0xaa, 0xaa, 0xaa, 0xaa], &[0xaa, 0xaa]),
            vec![0..2, 1..3, 2..4],
            "overlapping occurrences are all matches"
        );
        assert_eq!(find_all(&[1, 2, 3], &[1]), vec![0..1], "offset zero");
        assert_eq!(find_all(&[1, 2, 3], &[3]), vec![2..3], "the last byte");
        assert_eq!(
            find_all(&[1, 2, 3], &[1, 2, 3]),
            vec![0..3],
            "the whole lot"
        );
        assert!(find_all(&[1, 2, 3], &[]).is_empty(), "an empty needle");
        assert!(find_all(&[], &[1]).is_empty(), "an empty haystack");
        assert!(
            find_all(&[1, 2], &[1, 2, 3]).is_empty(),
            "a needle longer than the haystack must not index past the end"
        );
        assert!(find_all(&[1, 2, 3], &[4]).is_empty(), "no match");
    }

    /// `Find Next` wraps, in both directions, and from both ends — which is the whole reason it is
    /// a separate function: `current + 1` and `current - 1` are each one subtraction away from
    /// either falling off the end of the list or underflowing.
    #[test]
    fn stepping_through_matches_wraps_at_both_ends() {
        assert_eq!(wrap(0, 3, true), 1);
        assert_eq!(wrap(2, 3, true), 0, "forwards past the last");
        assert_eq!(wrap(2, 3, false), 1);
        assert_eq!(wrap(0, 3, false), 2, "backwards past the first");
        assert_eq!(wrap(0, 1, true), 0, "a single match stays put");
        assert_eq!(wrap(0, 1, false), 0);
        assert_eq!(wrap(0, 0, true), 0, "no matches at all must not underflow");
        assert_eq!(wrap(0, 0, false), 0);
    }

    /// Paste is in-place (`UI_DESIGN.md:77`), so a paste that would run past the end is refused
    /// rather than truncated — truncating would corrupt a document the user believes they pasted
    /// into. The exact fit is the edge that matters: `offset + pasted == len` writes the last byte,
    /// not one past it, and refusing it would make the last byte of every document unpasteable.
    #[test]
    fn a_paste_past_the_end_is_refused_and_an_exact_fit_is_not() {
        assert!(overrun(0, 4, 4).is_none(), "the whole document");
        assert!(overrun(2, 2, 4).is_none(), "an exact fit at the end");
        assert!(overrun(0, 1, 4).is_none());

        let refused = overrun(2, 3, 4).expect("three bytes do not fit in two");
        // Both lengths, the offset and the size: the user is being told "no" about a gesture they
        // had every reason to think worked.
        assert!(refused.contains('3'), "the paste length: {refused}");
        assert!(refused.contains('2'), "the room: {refused}");
        assert!(refused.contains('4'), "the document size: {refused}");

        assert!(
            overrun(4, 1, 4).is_some(),
            "a caret at the end of the document has no room at all"
        );
        assert!(
            overrun(9, 1, 4).is_some(),
            "an offset past the end must saturate rather than underflow"
        );
    }

    /// The premise of [`super::HexView::write_all`]'s backstop, and of
    /// [`crate::Ide::on_paste`]'s: both refuse a read-only document *before* writing, and both
    /// rely on `set_byte` refusing underneath if they ever stop. Pinned here because that reliance
    /// is invisible from the call site — a `set_byte` that silently started accepting writes to a
    /// read-only document would leave three layers of guard and no check behind any of them.
    #[test]
    fn a_paste_into_a_read_only_document_is_refused() {
        // Read-only is the *default* for every kind of document (`doc.rs:58`), so this is also the
        // state the CBOR tab is in until something makes it writable.
        let mut doc = CborDoc::from_bytes(vec![0xa1, 0x01, 0x26]);
        assert!(
            doc.set_byte(1, 0xff).is_err(),
            "`set_byte` must refuse a read-only document"
        );
        assert_eq!(doc.bytes(), [0xa1, 0x01, 0x26], "and must write nothing");

        doc.meta.read_only = false;
        assert!(doc.set_byte(1, 0xff).is_ok(), "a writable document accepts");
        assert_eq!(doc.bytes(), [0xa1, 0xff, 0x26]);
    }

    /// The find bar and the clipboard share one hex syntax, so a query accepts exactly what `Copy`
    /// produces. Text mode takes the query's own UTF-8 with no escapes, which is the thing most
    /// likely to be "improved" later: `\n` searching for a backslash and an `n` is deliberate.
    #[test]
    fn a_query_parses_as_hex_or_as_its_own_bytes() {
        assert_eq!(needle_for("a1 01 26", true), Ok(vec![0xa1, 0x01, 0x26]));
        assert_eq!(needle_for("A1", true), Ok(vec![0xa1]), "upper case");
        assert!(needle_for("a", true).is_err(), "an odd digit count");
        assert!(needle_for("zz", true).is_err(), "not hex at all");
        assert_eq!(needle_for("", true), Ok(Vec::new()), "an empty query");

        assert_eq!(needle_for("eat", false), Ok(b"eat".to_vec()));
        assert_eq!(
            needle_for("\\n", false),
            Ok(br"\n".to_vec()),
            "no escape processing, on purpose"
        );
        assert_eq!(needle_for("é", false), Ok("é".as_bytes().to_vec()));
    }

    /// Clipboard text that is not hex is quoted back, which means it has to be made to fit a
    /// diagnostic row first — the usual reason a paste is not hex is that it is a whole file.
    #[test]
    fn unparseable_clipboard_text_is_cut_down_to_a_row() {
        assert_eq!(elide("a1 01"), "a1 01", "short text is untouched");
        assert_eq!(
            elide("lots\n  of\twhitespace"),
            "lots of whitespace",
            "whitespace is collapsed as well as the length cut"
        );

        let long = "x".repeat(ELIDE_AT * 3);
        let cut = elide(&long);
        assert!(cut.ends_with("..."), "{cut}");
        assert_eq!(cut.chars().count(), ELIDE_AT + 3);

        // Cut by characters and not by bytes: slicing a multi-byte character in half panics.
        let wide = "é".repeat(ELIDE_AT * 2);
        assert!(elide(&wide).ends_with("..."));
    }

    /// `UI_DESIGN.md:74`: the row is a multiple of eight bytes, and the multiple follows the
    /// width. The two properties that matter are that it is *always* a multiple of eight — the
    /// grouping in both fields depends on it — and that it never returns zero, which would make
    /// `row_count` divide by it and `step` take a remainder of it.
    #[test]
    fn the_column_count_is_always_a_usable_multiple_of_eight() {
        for width in [
            0.0, 1.0, 100.0, 118.0, 400.0, 700.0, 1024.0, 4096.0, 20000.0,
        ] {
            let columns = columns_for_width(px(width));
            assert_eq!(columns % GROUP, 0, "{width}px gave {columns} columns");
            assert!(columns >= GROUP, "{width}px gave {columns} columns");
        }
    }

    /// A row must fit the width it was computed from. This is the property the whole fixed-cell
    /// scheme exists to guarantee: get it wrong and the ASCII gutter is pushed off the edge.
    #[test]
    fn a_row_fits_the_width_it_was_derived_from() {
        for width in [200.0, 400.0, 640.0, 700.0, 1024.0, 1600.0, 2560.0] {
            let columns = columns_for_width(px(width));
            let groups = (columns / GROUP) as f32;
            let needed = PADDING * 2.0
                + OFFSET_WIDTH
                + FIELD_GAP
                + SCROLLBAR
                + groups * (GROUP as f32 * (HEX_CELL + ASCII_CELL) + GROUP_GAP * 2.0);
            // The narrowest widths cannot fit even one group; a minimum of eight columns is the
            // right answer there anyway, and the grid scrolls horizontally rather than lying.
            if columns > GROUP {
                assert!(
                    needed <= width,
                    "{columns} columns need {needed}px but only {width}px was available"
                );
            }
        }
    }

    /// The classic 16-byte row has to come out of a pane of ordinary size, because every hex dump
    /// anyone has ever read is 16 wide and `convert::hex_dump` writes that width too.
    #[test]
    fn an_ordinary_pane_width_gives_the_classic_sixteen() {
        assert_eq!(columns_for_width(px(700.0)), 16);
    }

    #[test]
    fn the_row_count_covers_every_byte_and_is_never_zero() {
        assert_eq!(row_count(0, 16), 1, "an empty buffer still has a row");
        assert_eq!(row_count(1, 16), 1);
        assert_eq!(row_count(16, 16), 1);
        assert_eq!(row_count(17, 16), 2, "one byte over must not be dropped");
        assert_eq!(row_count(1_048_576, 16), 65_536);
        assert_eq!(row_count(10, 0), 10, "a zero width must not divide by zero");
    }

    /// Space is printable and `0x7f` is not; those are the two boundaries that get mixed up.
    #[test]
    fn the_ascii_gutter_shows_exactly_the_printable_range() {
        assert_eq!(printable(b' '), ' ');
        assert_eq!(printable(b'~'), '~');
        assert_eq!(printable(0x7f), '.', "DEL is not printable");
        assert_eq!(printable(0x1f), '.');
        assert_eq!(printable(0x00), '.');
        assert_eq!(printable(0xff), '.', "a high byte must not become U+00FF");

        // Every byte must map to exactly one column's worth of output.
        for byte in 0..=u8::MAX {
            assert!(printable(byte).is_ascii());
        }
    }

    #[test]
    fn hex_digits_parse_and_nothing_else_does() {
        assert_eq!(hex_digit("0"), Some(0));
        assert_eq!(hex_digit("9"), Some(9));
        assert_eq!(hex_digit("a"), Some(10));
        assert_eq!(hex_digit("f"), Some(15));
        assert_eq!(hex_digit("A"), Some(10), "a shifted digit still counts");
        assert_eq!(hex_digit("g"), None);
        assert_eq!(hex_digit("left"), None, "a named key is not a digit");
        assert_eq!(hex_digit(""), None);
        assert_eq!(hex_digit("é"), None, "a multi-byte key is not a digit");
    }

    /// Two keystrokes make a byte, high nibble first, and each leaves the other half alone.
    #[test]
    fn typing_two_digits_replaces_exactly_the_nibble_typed() {
        assert_eq!(nibble(0x00, 0xa, false), 0xa0);
        assert_eq!(nibble(0xa0, 0xb, true), 0xab);
        assert_eq!(nibble(0xff, 0x0, false), 0x0f, "the low nibble survives");
        assert_eq!(nibble(0xff, 0x0, true), 0xf0, "the high nibble survives");
    }

    /// The caret may never leave the buffer: it is the only thing that decides which offset
    /// `set_byte` is handed, so an unclamped motion is an out-of-range write.
    #[test]
    fn no_motion_can_take_the_caret_out_of_the_buffer() {
        let motions = [
            Motion::Left,
            Motion::Right,
            Motion::Up,
            Motion::Down,
            Motion::RowStart,
            Motion::RowEnd,
            Motion::PageUp,
            Motion::PageDown,
        ];
        for len in [1_usize, 7, 8, 16, 17, 100] {
            for offset in 0..len {
                for motion in motions {
                    let moved = step(offset, len, 16, 20, motion);
                    assert!(
                        moved < len,
                        "{motion:?} from {offset} in {len} bytes gave {moved}"
                    );
                }
            }
        }
    }

    /// An empty buffer has no valid offset at all, so every motion has to resolve to zero rather
    /// than underflow — this is the path a freshly started IDE is on.
    #[test]
    fn an_empty_buffer_pins_the_caret_at_zero() {
        for motion in [Motion::Left, Motion::Right, Motion::Up, Motion::RowEnd] {
            assert_eq!(step(0, 0, 16, 20, motion), 0);
        }
    }

    /// Up and down hold their column, which is what makes a hex view navigable at all; and the
    /// first and last rows refuse rather than jumping to the ends of the buffer.
    #[test]
    fn vertical_motion_keeps_its_column() {
        assert_eq!(step(0x25, 0x100, 16, 20, Motion::Down), 0x35);
        assert_eq!(step(0x25, 0x100, 16, 20, Motion::Up), 0x15);
        assert_eq!(step(0x05, 0x100, 16, 20, Motion::Up), 0x05, "the first row");
        assert_eq!(
            step(0xf5, 0x100, 16, 20, Motion::Down),
            0xf5,
            "the last full row must not slide along to the final byte"
        );
        // A ragged last row is the case that tempts an off-by-one: there is no byte below 0x1a.
        assert_eq!(step(0x0a, 0x15, 16, 20, Motion::Down), 0x0a);
        assert_eq!(step(0x02, 0x15, 16, 20, Motion::Down), 0x12);
    }

    #[test]
    fn home_and_end_work_on_the_row_not_the_buffer() {
        assert_eq!(step(0x25, 0x100, 16, 20, Motion::RowStart), 0x20);
        assert_eq!(step(0x25, 0x100, 16, 20, Motion::RowEnd), 0x2f);
        // On a ragged last row, End is the last byte there is.
        assert_eq!(step(0x11, 0x15, 16, 20, Motion::RowEnd), 0x14);
    }

    #[test]
    fn a_page_is_a_viewport_of_rows() {
        assert_eq!(step(0x500, 0x1000, 16, 20, Motion::PageDown), 0x640);
        assert_eq!(step(0x500, 0x1000, 16, 20, Motion::PageUp), 0x3c0);
        assert_eq!(step(0x10, 0x1000, 16, 20, Motion::PageUp), 0x00);
    }

    /// `page_rows_for_height` subtracts the two non-row rows, and must stay positive however small
    /// the pane gets — a page of zero rows is a page down that does nothing.
    #[test]
    fn a_page_is_always_at_least_one_row() {
        for height in [0.0, 1.0, 20.0, 40.0, 100.0, 840.0] {
            assert!(page_rows_for_height(px(height)) >= 1);
        }
        assert_eq!(page_rows_for_height(px(400.0)), 20);
    }

    /// Only the named keys move the caret. `h`, `j`, `k` and `l` deliberately do not: they are hex
    /// digits in the hex field and literal text in the ASCII one.
    #[test]
    fn only_the_navigation_keys_name_a_motion() {
        assert_eq!(motion_for("up"), Some(Motion::Up));
        assert_eq!(motion_for("pagedown"), Some(Motion::PageDown));
        assert_eq!(motion_for("j"), None);
        assert_eq!(motion_for("a"), None);
        assert_eq!(motion_for("enter"), None);
    }
}
