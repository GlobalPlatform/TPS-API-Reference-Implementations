/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The project's syntax palette, mapped onto gpui-component's theme.
//!
//! `tps_cbor_gui` and `tps_ts_highlight_tool` already share one palette — the VS Code Dark+ family
//! of colours pinned in `tps_cbor_gui/src/App.css:170-215`. A CDDL file should look the same in the
//! IDE as it does in the old GUI and in the highlighter CLI, so those six colours are pinned here
//! rather than left to whichever theme happens to be loaded.
//!
//! Everything else is inherited: [`init`] starts from gpui-component's own default theme for each
//! mode and overrides only the capture names the project has an opinion about, so the other 35 of
//! its 41 names keep styles that were designed to work together. Phase 0 noted `@property` and
//! `@type.builtin` rendering in the default foreground; that is what this fixes.
//!
//! Two things about gpui-component's theme API shape this module, and neither is obvious:
//!
//! 1. **The palette is defined as JSON, not as struct literals.** `ThemeStyle` — the per-capture
//!    `{ color, font_style, font_weight }` — has private fields, no constructor and no `From`
//!    impl (`gpui-component/src/highlighter/registry.rs:226-230`). `Deserialize` is its only
//!    public route in. That turns out to read well, because the serde key for each field *is* the
//!    capture name, so [`PALETTE`] says what it means.
//! 2. **The override is installed as a [`ThemeConfig`](gpui_kit::component::ThemeConfig), not by
//!    assigning `highlight_theme`.**
//!    Assigning the field directly works until the first light/dark switch: `Theme::change` ends
//!    in `Theme::edit(.., reload_mode = true, ..)`, which re-runs `apply_config` for the new mode
//!    and replaces `highlight_theme` wholesale (`theme/mod.rs:323-326`, `theme/schema.rs:1069-1077`).
//!    Registering a patched config as the theme for *both* modes instead means the reload reloads
//!    ours, so the palette survives a mode switch with no observer and no re-entrancy to guard.

use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::component::highlighter::SyntaxColors;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};

/// The capture names this project pins, in gpui-component's own theme JSON format.
///
/// Keys are `SyntaxColors`' serde keys, which for 40 of its 41 fields *are* the capture names the
/// highlighter looks up. The exception is `comment_doc`: it is the one field missing a
/// `#[serde(rename = ...)]`, so its JSON key is `comment_doc` while the capture name is
/// `comment.doc` (`gpui-component/src/highlighter/registry.rs:120` against `:253`). Upstream's own
/// `default-theme.json` writes `"comment.doc"` there and so never sets it at all. Writing the
/// working key here, with [`tests::the_serde_key_for_comment_doc_is_not_its_capture_name`] pinning
/// the quirk, means an upstream fix shows up as a test failure rather than as a silent loss.
///
/// Anything absent from this table is inherited from the default theme.
///
/// The colours are the old GUI's (`App.css:170-215`) and a VS Code Dark+ derivative. They are
/// applied in light mode too, which is deliberate: one palette across modes, frontends and tools
/// beats a better-matched light palette that nothing else shares. If light mode proves unreadable
/// the fix is a second table here, not a divergence between the IDE and `tps_ts_highlight_tool`.
///
/// The `r##` delimiters are needed rather than `r#`, because every value here contains `"#`.
const PALETTE: &str = r##"{
    "type":        { "color": "#4ec9b0" },
    "property":    { "color": "#569cd6" },
    "operator":    { "color": "#c586c0" },
    "number":      { "color": "#d7ba7d" },
    "boolean":     { "color": "#d7ba7d" },
    "string":      { "color": "#ce9178" },
    "comment":     { "color": "#6a9955" },
    "comment_doc": { "color": "#4fc1ff" }
}"##;

/// The theme names this module registers, one per mode.
const LIGHT_NAME: &str = "CBOR IDE Light";
const DARK_NAME: &str = "CBOR IDE Dark";

/// Installs the palette for both modes and reloads the active one.
///
/// Call once, after `gpui_kit::init(cx)` — which is what creates the `Theme` global and the
/// default theme registry this builds on.
pub fn init(cx: &mut App) {
    install(ThemeMode::Light, cx);
    install(ThemeMode::Dark, cx);

    // `install` applied each mode's config in turn, so the inactive mode's colours are currently
    // the live ones. Going back through `Theme::change` reloads the config for the active mode and
    // reconciles the rest of the theme's state, which assigning fields by hand would not.
    let mode = cx.theme().mode;
    Theme::change(mode, None, cx);
}

/// Registers one mode's patched config and makes it that mode's theme.
fn install(mode: ThemeMode, cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let base = if mode.is_dark() {
        registry.default_dark_theme()
    } else {
        registry.default_light_theme()
    };

    let mut config = (**base).clone();
    config.name = if mode.is_dark() {
        DARK_NAME
    } else {
        LIGHT_NAME
    }
    .into();
    // Not a default theme: a theme picker sorts the defaults first, and ours is a variant of one,
    // not an alternative to it.
    config.is_default = false;

    // `apply_config` only touches `highlight_theme` when `highlight` is `Some`, and replaces it
    // entirely when it is — so the patched style has to carry the whole block, which is why this
    // patches the base's own style rather than building one from nothing.
    let mut style = config.highlight.clone().unwrap_or_default();
    style.syntax = palette_over(&style.syntax);
    config.highlight = Some(style);

    Theme::global_mut(cx).apply_config(&Rc::new(config));
}

/// Returns `base` with [`PALETTE`]'s entries replacing the matching ones.
///
/// The merge happens as JSON rather than field by field so that adding a capture name to
/// [`PALETTE`] needs no corresponding Rust change — `SyntaxColors` has 41 near-identically named
/// fields and assigning a colour to the wrong one is both easy and silent.
///
/// Both conversions are infallible in practice: `SyntaxColors` round-trips through its own serde
/// impls, and [`PALETTE`] is a literal that [`tests::the_palette_is_valid_json_for_every_key`]
/// covers. A malformed `PALETTE` would panic here at startup, which is the right moment for it.
fn palette_over(base: &SyntaxColors) -> SyntaxColors {
    let mut merged = serde_json::to_value(base).expect("SyntaxColors serializes");
    let patch: serde_json::Value = serde_json::from_str(PALETTE).expect("PALETTE is valid JSON");

    let (Some(merged_map), Some(patch_map)) = (merged.as_object_mut(), patch.as_object()) else {
        unreachable!("both a serialized SyntaxColors and PALETTE are JSON objects");
    };
    for (name, style) in patch_map {
        merged_map.insert(name.clone(), style.clone());
    }

    serde_json::from_value(merged).expect("a patched SyntaxColors deserializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::highlighter::HighlightTheme;

    /// Resolves a capture name to the `#rrggbbaa` form of its colour.
    ///
    /// Comparing hex rather than `Hsla` keeps the assertions readable and sidesteps the float
    /// round trip through `Rgba` that `Hsla`'s serde impls make (`gpui/src/color.rs:725-741`).
    fn colour_of(syntax: &SyntaxColors, name: &str) -> Option<String> {
        let style = syntax.style(name)?;
        let hex = serde_json::to_value(style.color?).expect("Hsla serializes as a hex string");
        Some(
            hex.as_str()
                .expect("Hsla serializes as a string")
                .to_string(),
        )
    }

    fn patched() -> SyntaxColors {
        palette_over(&SyntaxColors::default())
    }

    /// The capture name the highlighter will look up for a given `PALETTE` key.
    ///
    /// Identity for every key but one; see [`PALETTE`] for why `comment_doc` differs.
    fn capture_name(key: &str) -> &str {
        if key == "comment_doc" {
            "comment.doc"
        } else {
            key
        }
    }

    /// Every key in `PALETTE` has to be one `SyntaxColors` actually recognises. Nothing else would
    /// catch a typo: serde ignores unknown fields here, so a misspelled key is silently dropped
    /// and the capture quietly keeps the inherited colour.
    #[test]
    fn every_palette_key_reaches_the_highlighter() {
        let patch: serde_json::Value = serde_json::from_str(PALETTE).expect("PALETTE parses");
        let patch = patch.as_object().expect("PALETTE is an object");
        assert_eq!(patch.len(), 8, "PALETTE gained or lost an entry");

        let syntax = patched();
        for (key, value) in patch {
            let expected = value
                .get("color")
                .and_then(|c| c.as_str())
                .unwrap_or_else(|| panic!("`{key}` has no colour"));
            assert_eq!(
                colour_of(&syntax, capture_name(key)).as_deref(),
                Some(format!("{expected}ff").as_str()),
                "`{key}` is not a key SyntaxColors recognises, or did not take the palette colour"
            );
        }
    }

    /// `comment_doc` is the one `SyntaxColors` field whose serde key is not the capture name the
    /// highlighter looks up, because it alone is missing a `#[serde(rename)]`. That is why
    /// `PALETTE` spells it the unobvious way, and this is what will tell us when it stops being
    /// necessary — upstream's own `default-theme.json` writes `"comment.doc"` and so sets nothing.
    #[test]
    fn the_serde_key_for_comment_doc_is_not_its_capture_name() {
        let by_capture_name: SyntaxColors =
            serde_json::from_str(r##"{ "comment.doc": { "color": "#4fc1ff" } }"##)
                .expect("unknown keys are ignored, not rejected");
        assert_eq!(
            colour_of(&by_capture_name, "comment.doc"),
            None,
            "upstream now accepts the capture name as the key -- PALETTE can use it too, and this \
             test and the note on PALETTE can go"
        );

        let by_serde_key: SyntaxColors =
            serde_json::from_str(r##"{ "comment_doc": { "color": "#4fc1ff" } }"##)
                .expect("`comment_doc` is the field's serde key");
        assert_eq!(
            colour_of(&by_serde_key, "comment.doc").as_deref(),
            Some("#4fc1ffff")
        );
    }

    /// Patching must not disturb anything it does not name — the whole point of merging onto the
    /// default theme rather than shipping a complete one. `keyword` is the case that matters most:
    /// our CDDL queries lean on it heavily through `@keyword.operator`, and it is inherited.
    #[test]
    fn unnamed_captures_keep_the_inherited_style() {
        let base = HighlightTheme::default_dark().style.syntax.clone();
        let inherited =
            colour_of(&base, "keyword").expect("the default dark theme styles `keyword`");

        let patched = palette_over(&base);
        assert_eq!(
            colour_of(&patched, "keyword"),
            Some(inherited),
            "`keyword` is not in PALETTE and must come through untouched"
        );
        assert_eq!(
            colour_of(&patched, "type").as_deref(),
            Some("#4ec9b0ff"),
            "and the keys that *are* in PALETTE must still win over the base"
        );
    }

    /// The capture names our grammars emit are not among gpui-component's 41; they resolve only
    /// because a dotted name falls back to its prefix (`registry.rs:295-306`). If that ever stops
    /// happening, CDDL loses most of its colour at once and nothing else would notice.
    #[test]
    fn our_dotted_capture_names_fall_back_to_the_palette() {
        let syntax = patched();
        for name in ["type.parameter", "type.builtin", "type.definition"] {
            assert_eq!(
                colour_of(&syntax, name).as_deref(),
                Some("#4ec9b0ff"),
                "`{name}` did not fall back to `type`"
            );
        }
    }

    /// `@comment.documentation` reducing to `comment` rather than to `comment.doc` is why the
    /// directive colour has no visible effect yet. Pinning the behaviour here means the grammar
    /// change that fixes it shows up as this test needing an update, rather than being forgotten.
    #[test]
    fn comment_documentation_still_reduces_to_plain_comment() {
        let syntax = patched();
        assert_eq!(
            colour_of(&syntax, "comment.documentation").as_deref(),
            Some("#6a9955ff"),
            "if this is now the directive colour, tree-sitter-cddl's capture can stay as it is"
        );
        assert_eq!(
            colour_of(&syntax, "comment.doc").as_deref(),
            Some("#4fc1ffff")
        );
    }
}
