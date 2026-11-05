/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The window's three-pane geometry, and the sizes that outlive the session.
//!
//! `UI_DESIGN.md:20-47` draws the layout and `:58-64` states the rules: a left pane whose content
//! is tab-independent, a centre column split by a draggable separator, a lower window at about 15%
//! on start-up, and a right pane that can be hidden. That is
//! `h_resizable[ explorer | v_resizable[ main | diagnostics ] | options ]`.
//!
//! [`Panes`] owns the two `ResizableState`s and the persisted [`settings::Layout`]; [`panes`]
//! builds the element from them and four caller-supplied children. The geometry lives here and
//! nothing else does, so the root view does not have to know how a fraction becomes a pixel.
//!
//! Three details of gpui-component's resizable worth recording:
//!
//! - **A panel's `size` is an initial size, not a binding.** Once the user drags, the state's own
//!   size wins (`gpui-base/src/resizable/panel.rs:356-372`). So restoring a saved width is just a
//!   matter of passing it as the initial size on the first frame.
//! - **A sized panel next to a flexible one needs `flex_none`.** The panel sets `flex_grow: 1`
//!   internally, and without opting out a sized pane is stretched by its neighbour's collapse
//!   (`panel.rs:392-399`).
//! - **`on_resize` fires on mouse-up, not per frame**, which makes it the right place to write the
//!   sizes back: a drag produces one save, not one per pixel.

use gpui_kit::component::{ResizableState, h_resizable, resizable_panel, v_resizable};
use gpui_kit::{
    AnyElement, App, AppContext as _, Entity, IntoElement as _, ParentElement as _, Pixels,
    Styled as _, Window, px,
};
use tps_cbor_core::settings;

/// The explorer's width when nothing has been saved.
const DEFAULT_EXPLORER_WIDTH: f32 = 240.0;
/// The options pane's width when nothing has been saved.
const DEFAULT_OPTIONS_WIDTH: f32 = 280.0;

/// Width limits for the explorer. The lower bound keeps a file name readable at the indentation
/// the tree reaches on a four-deep include chain.
const EXPLORER_WIDTH: (f32, f32) = (160.0, 560.0);
/// Width limits for the options pane. Its widest content is the start-production text box.
const OPTIONS_WIDTH: (f32, f32) = (200.0, 640.0);

/// Height limits for the diagnostics pane, in pixels.
///
/// The lower bound is two rows plus the border: a diagnostics pane dragged shut would hide errors
/// without telling the user it had, and `UI_DESIGN.md:61` makes it the place errors are reported.
///
/// The upper bound is a backstop against a hand-edited settings file; the bound that actually
/// binds in a small window is [`ceiling`]'s.
const DIAGNOSTICS_HEIGHT: (f32, f32) = (48.0, 2_000.0);

/// The least height the upper window keeps, in pixels, and so what the lower window's ceiling is
/// measured down from.
///
/// It is set on the panel explicitly rather than left to gpui-base's own default. That default,
/// `resizable::PANEL_MIN_SIZE`, is 100 px, but it is `#[doc(hidden)]` and gpui-component does not
/// re-export it (`gpui-component-0.8.1/src/lib.rs:113-116`), so a copy of its value here could
/// drift from it without anything noticing. Deriving both panels' limits from this one constant
/// makes them agree by construction.
///
/// 120 px rather than 100 is three or four lines of an editor plus its chrome -- enough to see
/// where the caret is, which is the point of keeping any of it.
const MAIN_MIN_HEIGHT: f32 = 120.0;

/// The bounds `settings::Layout::diagnostics_fraction` documents.
const FRACTION: (f32, f32) = (0.05, 0.9);

/// The four children [`panes`] arranges.
///
/// Taken as built elements rather than as views so that the layout has no dependency on any
/// particular pane's type, which is what keeps this module testable and Phases 3 to 7 free to
/// replace a pane's content without touching it.
pub struct Content {
    /// The left pane: the file tree.
    pub explorer: AnyElement,
    /// The centre upper window: the selected tab's view of its document.
    pub main: AnyElement,
    /// The centre lower window: diagnostics and the trace log.
    pub diagnostics: AnyElement,
    /// The right pane: settings for the selected tab.
    pub options: AnyElement,
}

/// The window's pane geometry.
///
/// Held in an `Entity` so that the resize callbacks — which see only `&mut App` — can write the
/// dragged sizes back. Not a view: it renders nothing itself.
pub struct Panes {
    columns: Entity<ResizableState>,
    rows: Entity<ResizableState>,
    layout: settings::Layout,
}

impl Panes {
    /// Builds the geometry from the persisted layout.
    ///
    /// Values outside their documented range are clamped rather than rejected: the settings file is
    /// meant to be hand-editable, and a mistake in it should cost a sensible layout, not a session.
    pub fn new(layout: settings::Layout, cx: &mut App) -> Self {
        Self {
            columns: cx.new(|_| ResizableState::default()),
            rows: cx.new(|_| ResizableState::default()),
            layout: settings::Layout {
                diagnostics_fraction: clamp(layout.diagnostics_fraction, FRACTION),
                explorer_width: layout.explorer_width.map(|w| clamp(w, EXPLORER_WIDTH)),
                options_width: layout.options_width.map(|w| clamp(w, OPTIONS_WIDTH)),
                ..layout
            },
        }
    }

    /// The layout as it now stands, for `settings::Settings::save_default`.
    pub fn layout(&self) -> &settings::Layout {
        &self.layout
    }

    /// Shows or hides the right pane, as the `Options Pane` menu item does.
    pub fn toggle_options(&mut self) {
        self.layout.options_visible = !self.layout.options_visible;
    }
}

/// Builds the three-pane layout.
pub fn panes(
    state: &Entity<Panes>,
    content: Content,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let panes = state.read(cx);
    let layout = panes.layout.clone();
    let columns = panes.columns.clone();
    let rows = panes.rows.clone();

    // How much height the centre column has to divide. Once the group has been laid out it has
    // measured itself and `container_size` is exact; on the very first frame it is zero and the
    // viewport is the only number there is. The viewport overstates the column by the menu bar and
    // the window chrome, which is a few pixels on a 15% pane and only until the next frame.
    let available = match f32::from(rows.read(cx).container_size()) {
        measured if measured > 0.0 => measured,
        _ => f32::from(window.viewport_size().height),
    };

    // The diagnostics pane is specified as a fraction but sized in pixels, and only the window
    // knows how many.
    let diagnostics_height = clamp_panel(
        f32::from(window.viewport_size().height) * layout.diagnostics_fraction,
        available,
        DIAGNOSTICS_HEIGHT,
    );

    let centre = v_resizable("centre-column")
        .with_state(&rows)
        .child(
            resizable_panel()
                .size_range(px(MAIN_MIN_HEIGHT)..Pixels::MAX)
                .child(content.main),
        )
        .child(
            resizable_panel()
                .size(px(diagnostics_height))
                .flex_none()
                // The ceiling is recomputed every frame, and that is what keeps the lower window
                // on screen. A panel's `size` is only an *initial* size -- after the first drag
                // `ResizableState` holds an absolute pixel height and keeps it, and nothing in
                // gpui-base takes it back: `adjust_to_container_size` returns early the moment any
                // panel in the group has no size preference
                // (`gpui-base-0.8.1/src/resizable/mod.rs:396-398`), and the upper window is
                // exactly such a panel. So shrinking the window used to leave a 400 px lower
                // window in a 300 px column, pushing it off the bottom edge.
                //
                // `size_range`'s end is the lever because the panel applies it as a `max_h` and
                // clamps its own `flex_basis` to it on every frame (`panel.rs:351-356, 370-373`),
                // so a ceiling computed here is enforced by layout rather than by writing back
                // into the state -- which from a render pass would mean a mutation, a notify and a
                // settling frame.
                //
                // The saved *fraction* is deliberately not clamped with it: `record` runs only on
                // mouse-up from a real drag, so the user's chosen proportion survives a spell in a
                // small window and comes back when the window grows again.
                .size_range(px(DIAGNOSTICS_HEIGHT.0)..px(ceiling(available, DIAGNOSTICS_HEIGHT)))
                .child(content.diagnostics),
        )
        .on_resize({
            let state = state.clone();
            move |_, _, cx| capture(&state, cx)
        });

    let mut columns_children = vec![
        resizable_panel()
            .size(px(width(
                layout.explorer_width,
                DEFAULT_EXPLORER_WIDTH,
                EXPLORER_WIDTH,
            )))
            .flex_none()
            .size_range(px(EXPLORER_WIDTH.0)..px(EXPLORER_WIDTH.1))
            .child(content.explorer),
        resizable_panel().child(centre),
    ];

    // The hidden pane is left out of the group rather than rendered invisible: a panel that is
    // present but not visible still occupies a slot in `ResizableState`, and the slot's size is
    // then adjusted by every drag the user makes while the pane is hidden. Omitting it keeps the
    // saved width in `settings` instead, which is the copy that survives a restart anyway.
    if layout.options_visible {
        columns_children.push(
            resizable_panel()
                .size(px(width(
                    layout.options_width,
                    DEFAULT_OPTIONS_WIDTH,
                    OPTIONS_WIDTH,
                )))
                .flex_none()
                .size_range(px(OPTIONS_WIDTH.0)..px(OPTIONS_WIDTH.1))
                .child(content.options),
        );
    }

    h_resizable("workbench")
        .with_state(&columns)
        .children(columns_children)
        .on_resize({
            let state = state.clone();
            move |_, _, cx| capture(&state, cx)
        })
        .into_any_element()
}

/// Reads the dragged sizes out of the two resizable states and into the persisted layout.
///
/// Both states are read before the update, because `Entity::update` holds the one mutable borrow
/// of the app and reading another entity inside it is not possible.
fn capture(state: &Entity<Panes>, cx: &mut App) {
    let (columns, rows) = {
        let panes = state.read(cx);
        (
            panes.columns.read(cx).sizes().clone(),
            panes.rows.read(cx).sizes().clone(),
        )
    };
    state.update(cx, |panes, _| record(&mut panes.layout, &columns, &rows));
}

/// Writes pane sizes into `layout`, ignoring anything that does not describe this layout.
///
/// Separate from [`capture`] so the arithmetic is testable without an app: this is where a sign
/// error would silently shrink a pane a little on every drag.
fn record(layout: &mut settings::Layout, columns: &[Pixels], rows: &[Pixels]) {
    match columns {
        // The right pane is hidden, so there is no width of it to record and the saved one is the
        // width it should come back at.
        [explorer, _] => layout.explorer_width = Some(f32::from(*explorer)),
        [explorer, _, options] => {
            layout.explorer_width = Some(f32::from(*explorer));
            layout.options_width = Some(f32::from(*options));
        }
        _ => {}
    }

    if let [main, diagnostics] = rows {
        let total = f32::from(*main) + f32::from(*diagnostics);
        if total > 0.0 {
            layout.diagnostics_fraction = clamp(f32::from(*diagnostics) / total, FRACTION);
        }
    }
}

/// The tallest the lower window may be in a centre column `available` pixels high.
///
/// `available - MAIN_MIN_HEIGHT`, so the upper window always keeps its floor, then brought inside
/// `range` so the lower window keeps its own. Those two can conflict: a column shorter than
/// `MAIN_MIN_HEIGHT + range.0` cannot satisfy both, and this resolves it in the lower window's
/// favour, because `UI_DESIGN.md:61` makes it where errors are reported and a column that short is
/// already unusable. The window's own minimum size keeps it from arising in practice.
fn ceiling(available: f32, range: (f32, f32)) -> f32 {
    clamp(available - MAIN_MIN_HEIGHT, range)
}

/// `stored` brought inside the limits a centre column `available` pixels high allows.
fn clamp_panel(stored: f32, available: f32, range: (f32, f32)) -> f32 {
    clamp(stored, (range.0, ceiling(available, range)))
}

/// A saved width, or the default, brought inside the pane's limits.
fn width(saved: Option<f32>, default: f32, limits: (f32, f32)) -> f32 {
    clamp(saved.unwrap_or(default), limits)
}

/// `value` brought inside `(min, max)`, treating a NaN as the minimum.
///
/// `f32::clamp` panics on a NaN bound and propagates a NaN value; a NaN here would come from a
/// hand-edited settings file or a zero-height first frame, and a layout is not worth a panic.
fn clamp(value: f32, (min, max): (f32, f32)) -> f32 {
    if value.is_nan() {
        return min;
    }
    value.max(min).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> settings::Layout {
        settings::Layout::default()
    }

    /// A completed drag is what gets saved, and both axes are recorded from one callback.
    #[test]
    fn a_drag_records_both_the_widths_and_the_fraction() {
        let mut layout = layout();
        record(
            &mut layout,
            &[px(300.0), px(700.0), px(260.0)],
            &[px(800.0), px(200.0)],
        );

        assert_eq!(layout.explorer_width, Some(300.0));
        assert_eq!(layout.options_width, Some(260.0));
        assert_eq!(layout.diagnostics_fraction, 0.2);
    }

    /// With the right pane hidden the group has two panels, and the saved options width must
    /// survive that — it is the width the pane reopens at.
    #[test]
    fn hiding_the_options_pane_does_not_discard_its_saved_width() {
        let mut layout = settings::Layout {
            options_width: Some(320.0),
            options_visible: false,
            ..layout()
        };
        record(
            &mut layout,
            &[px(300.0), px(900.0)],
            &[px(800.0), px(200.0)],
        );

        assert_eq!(layout.explorer_width, Some(300.0));
        assert_eq!(
            layout.options_width,
            Some(320.0),
            "the options width must not be overwritten by the centre column's"
        );
    }

    /// `settings::Layout` documents the fraction as `0.05..=0.9`, and a pane dragged shut would
    /// otherwise persist a fraction that hides diagnostics on every subsequent start.
    #[test]
    fn the_diagnostics_fraction_stays_within_its_documented_range() {
        let mut layout = layout();

        record(&mut layout, &[], &[px(1000.0), px(0.0)]);
        assert_eq!(layout.diagnostics_fraction, FRACTION.0);

        record(&mut layout, &[], &[px(0.0), px(1000.0)]);
        assert_eq!(layout.diagnostics_fraction, FRACTION.1);
    }

    /// A zero-height group appears on the first frame, before layout has run. Dividing by it would
    /// write a NaN into the settings file, and a NaN fraction is not recoverable from.
    #[test]
    fn a_zero_sized_group_records_nothing() {
        let mut layout = layout();
        let before = layout.diagnostics_fraction;
        record(&mut layout, &[], &[px(0.0), px(0.0)]);
        assert_eq!(layout.diagnostics_fraction, before);
    }

    /// Saved widths are clamped on the way out, not just on the way in, since the settings file is
    /// hand-editable and a 4,000 px explorer leaves no window to work in.
    #[test]
    fn saved_widths_are_brought_inside_their_limits() {
        assert_eq!(
            width(Some(4000.0), DEFAULT_EXPLORER_WIDTH, EXPLORER_WIDTH),
            EXPLORER_WIDTH.1
        );
        assert_eq!(
            width(Some(1.0), DEFAULT_EXPLORER_WIDTH, EXPLORER_WIDTH),
            EXPLORER_WIDTH.0
        );
        assert_eq!(
            width(None, DEFAULT_EXPLORER_WIDTH, EXPLORER_WIDTH),
            DEFAULT_EXPLORER_WIDTH
        );
    }

    /// `UI_DESIGN.md:62`: the lower window starts at about 15% of the window. That is
    /// `settings::Layout`'s default, so the two must not drift apart.
    #[test]
    fn the_default_lower_window_is_about_fifteen_percent() {
        let fraction = settings::Layout::default().diagnostics_fraction;
        assert!(
            (0.1..=0.2).contains(&fraction),
            "UI_DESIGN.md:62 asks for about 15%, settings gives {fraction}"
        );
        assert_eq!(clamp(fraction, FRACTION), fraction);
    }

    /// The defect this fixes: `ResizableState` keeps an absolute pixel height once the user has
    /// dragged, and nothing in gpui-base reduces it when the window shrinks, so a lower window
    /// sized in a tall window was pushed off the bottom edge of a short one.
    #[test]
    fn clamp_panel_keeps_the_panel_on_screen() {
        // Tall window: the stored size is well inside the limits and is left alone.
        assert_eq!(clamp_panel(300.0, 1000.0, DIAGNOSTICS_HEIGHT), 300.0);

        // The same stored size in a 300 px column: the upper window keeps its floor and the lower
        // one gets the rest, rather than all 300 and an upper window of nothing.
        assert_eq!(
            clamp_panel(300.0, 300.0, DIAGNOSTICS_HEIGHT),
            300.0 - MAIN_MIN_HEIGHT
        );

        // And it is never pushed below its own minimum, however little room there is.
        assert_eq!(
            clamp_panel(300.0, 0.0, DIAGNOSTICS_HEIGHT),
            DIAGNOSTICS_HEIGHT.0
        );
    }

    /// The ceiling is what is handed to `size_range`, and the panel applies it as a `max_h`. A
    /// ceiling below the floor would be `min_h` above `max_h`, which is not a layout at all.
    #[test]
    fn the_ceiling_is_never_below_the_floor() {
        for available in [0.0, 1.0, MAIN_MIN_HEIGHT, 160.0, 4000.0, f32::NAN] {
            let ceiling = ceiling(available, DIAGNOSTICS_HEIGHT);
            assert!(
                ceiling >= DIAGNOSTICS_HEIGHT.0,
                "a {available} px column gave a ceiling of {ceiling}"
            );
            assert!(ceiling <= DIAGNOSTICS_HEIGHT.1);
        }
    }

    /// A window tall enough for both floors must leave the upper one at least its own, which is
    /// the whole purpose of measuring the ceiling down from it.
    #[test]
    fn the_upper_window_keeps_its_floor() {
        let available = 400.0;
        let lower = clamp_panel(f32::MAX, available, DIAGNOSTICS_HEIGHT);
        assert_eq!(available - lower, MAIN_MIN_HEIGHT);
    }

    /// A NaN reaches `clamp` from a hand-edited settings file, and `f32::clamp` would propagate it
    /// into a pixel size.
    #[test]
    fn a_nan_becomes_the_minimum_rather_than_a_nan_pane() {
        assert_eq!(clamp(f32::NAN, EXPLORER_WIDTH), EXPLORER_WIDTH.0);
        assert_eq!(
            width(Some(f32::NAN), 240.0, EXPLORER_WIDTH),
            EXPLORER_WIDTH.0
        );
    }
}
