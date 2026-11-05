/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Every IDE command, defined once as a GPUI [`Action`].
//!
//! `UI_DESIGN.md:49-54` asks for a menu that follows platform convention, with keyboard shortcuts
//! matching it. The only way to get that without the two drifting apart is for the menu and the
//! keymap to name the *same* thing, so an action is the single definition and three consumers read
//! it: [`crate::menus`] puts it in the menu bar, [`bind_keys`] gives it a shortcut, and the
//! options pane builds buttons that dispatch it.
//!
//! This matters more here than it sounds. The old GUI had no menu bar at all, so every command was
//! a React button with its own `onClick`, and the "Paste does not work on read-only files" rule
//! (`UI_DESIGN.md:53`) had nowhere to live. With one action per command there is exactly one place
//! to enforce it, and [`crate::menus`] does.
//!
//! All actions here are unit structs. That is deliberate: a parameterised action needs `serde` and
//! a JSON schema to survive the keymap, and nothing here needs a payload. Tab selection, which is
//! the one plausible candidate, is four separate actions instead — which also reads better in a
//! menu than `SelectTab { index: 2 }` would.

use gpui_kit::{App, KeyBinding, KeyBindingContextPredicate, actions};
use std::rc::Rc;

/// The key context the whole window runs in.
///
/// Bindings are registered against this rather than globally so that an editor or a text input can
/// still claim a key first — `secondary-c` has to mean "copy this text" when the focus is in an
/// editor, and GPUI resolves that by walking the context stack from the focused element outwards.
pub const CONTEXT: &str = "Ide";

actions!(
    cbor_ide,
    [
        // File
        Open,
        // Not in `UI_DESIGN.md`'s File menu, and an addition Phase 7's interactive verification
        // forced rather than one chosen: `UI_DESIGN.md:68` asks `File → Open` for a *file*, the
        // explorer needs a *directory* for its root, and on Linux one picker cannot offer both.
        // The portal's `OpenFile` request carries a single `directory` boolean, so GPUI turns
        // `PathPromptOptions { files: true, directories: true }` into a folder-*only* chooser,
        // titled "Open Folder", in which no file can be selected at all
        // (`gpui-pre-linux/src/linux/platform.rs:791-803`). `Open...` was therefore unable to open
        // a file on this platform. Splitting the command in two is what fixes that without taking
        // the explorer's root away. Recorded in `PLAN.md` as an addition.
        OpenFolder,
        Save,
        SaveAs,
        Quit,
        // Edit
        Copy,
        Paste,
        Find,
        FindNext,
        FindPrevious,
        // An addition to `UI_DESIGN.md`'s Edit menu, recorded as such in `PLAN.md`. The editing
        // engine already implements replace and gpui-base already binds `ctrl-h` to it, so a menu
        // that offers Find but not Replace hides a feature the user has anyway. Deliberately
        // unbound in [`keymap`] for that same reason: `ctrl-h` belongs to the editor's own `Input`
        // key context, and a second binding in ours would be a second answer to one gesture.
        Replace,
        // Tab selection. Four actions rather than one parameterised one; see the module docs.
        SelectCborTab,
        SelectEdnTab,
        SelectCddlTab,
        SelectCoseTab,
        // Conversions and checks. Which of these the right pane offers depends on the active tab
        // (`UI_DESIGN.md:79-84`, `:95-99`, `:113-117`), but the action exists regardless so the
        // keybinding does not have to be rebound as tabs change.
        ToEdn,
        ToCbor,
        Validate,
        SyntaxCheck,
        // `UI_DESIGN.md:93`: decode the selected EDN fragment as a CBOR object of its own. Reached
        // from the EDN editor's context menu, where the selection already is, and so deliberately
        // unbound in [`keymap`] — a shortcut would fire with nothing selected and could only
        // report that.
        DecodeSelection,
        // `UI_DESIGN.md:108`: open the `;# import`/`;# include` target the CDDL caret sits on.
        // Reached from the CDDL editor's context menu, which is where the caret already is, and so
        // deliberately unbound in [`keymap`] for the same reason as [`DecodeSelection`] — a
        // shortcut pressed anywhere else could only report that there was no directive there. The
        // action carries no payload: the handler asks [`crate::cddl_view::CddlView`] where the
        // caret is, so there is one answer to that question rather than two that can disagree.
        OpenInclude,
        // Keystore (`UI_DESIGN.md:123-125`), plus the two additions Phase 7 records in `PLAN.md`:
        // `DeleteKey`, because a store that can only be added to is a trap, and `ImportKey`,
        // because `UI_DESIGN.md:119`'s "File -> Open allows a COSE key to be opened" cannot put a
        // private key in a document (`tps_cbor_core/CLAUDE.md` forbids taking private material out
        // of `Zeroizing`), so opening one offers to store it instead.
        //
        // All three are deliberately unbound in [`keymap`]. Each one writes to or deletes from a
        // file on disk, and none of the three is the kind of command a user reaches for often
        // enough to want a chord for; `DeleteKey` in particular is one keystroke away from
        // destroying key material, which is a poor thing to put under a modifier.
        GenerateKey,
        DeleteKey,
        ImportKey,
        // The four COSE operations (`UI_DESIGN.md:129`). Deliberately unbound as well: all four
        // need a selected key *and* a loaded document, and a shortcut pressed without either could
        // only report that -- the same argument [`DecodeSelection`] is unbound under. The right
        // pane's four buttons dispatch them, so enablement is decided once in [`crate::menus`] and
        // both readers agree.
        Sign,
        Verify,
        Encrypt,
        Decrypt,
        // View
        ToggleOptions,
        // Help, which `UI_DESIGN.md:51` requires always be offered.
        About,
    ]
);

/// Registers the keymap.
///
/// `secondary-` is GPUI's platform-dependent modifier: `cmd` on macOS, `ctrl` everywhere else. It
/// is what `UI_DESIGN.md:54`'s "platform convention shortcuts" asks for, and using it means the
/// menu's own accelerator text and this keymap cannot disagree about which modifier a platform
/// uses.
///
/// It has to go through [`KeyBinding::load`] rather than `KeyBinding::new`, which is the one
/// non-obvious thing here: `new` parses with `Keystroke::parse`, and that deliberately refuses
/// `secondary-` because it has no host OS to resolve it against
/// (`gpui-pre/src/platform/keystroke.rs:156-166`). It does not *fail*, it panics —
/// `new` unwraps (`gpui-pre/src/keymap/binding.rs:47`) — so every binding below would have taken
/// the application down at start-up. `load` takes the OS from `cx` and resolves it.
///
/// `Quit` is deliberately *not* bound here. On macOS `cmd-q` is the OS's own and arrives through
/// the application menu; on Linux and Windows the convention is the window close button or
/// `alt-F4`, neither of which is ours to bind. Binding `secondary-q` as well would add a shortcut
/// no platform actually expects.
pub fn bind_keys(cx: &mut App) {
    let context = Rc::new(
        KeyBindingContextPredicate::parse(CONTEXT).expect("the `Ide` key context is a valid one"),
    );

    let bindings: Vec<KeyBinding> = keymap()
        .into_iter()
        .map(|(keystrokes, action)| {
            KeyBinding::load(keystrokes, action, Some(context.clone()), false, None, cx)
                .unwrap_or_else(|error| panic!("`{keystrokes}` is a valid keystroke: {error:?}"))
        })
        .collect();

    cx.bind_keys(bindings);
}

/// Every shortcut, as the keystroke text and the action it dispatches.
///
/// Separate from [`bind_keys`] so that the keystrokes can be parsed in a test without an
/// application; see `every_keystroke_parses_on_every_platform`.
fn keymap() -> Vec<(&'static str, Box<dyn gpui_kit::Action>)> {
    vec![
        ("secondary-o", Box::new(Open)),
        // The shifted form of the same letter, like `Save` and `Save As...` below: the two commands
        // are the same gesture over the two kinds of thing a picker can return.
        ("secondary-shift-o", Box::new(OpenFolder)),
        ("secondary-s", Box::new(Save)),
        ("secondary-shift-s", Box::new(SaveAs)),
        ("secondary-c", Box::new(Copy)),
        ("secondary-v", Box::new(Paste)),
        ("secondary-f", Box::new(Find)),
        // F3 and secondary-g are both conventional for "find next"; the shifted form of each walks
        // backwards. Offering both costs nothing and neither collides with anything else here.
        ("f3", Box::new(FindNext)),
        ("secondary-g", Box::new(FindNext)),
        ("shift-f3", Box::new(FindPrevious)),
        ("secondary-shift-g", Box::new(FindPrevious)),
        ("secondary-1", Box::new(SelectCborTab)),
        ("secondary-2", Box::new(SelectEdnTab)),
        ("secondary-3", Box::new(SelectCddlTab)),
        ("secondary-4", Box::new(SelectCoseTab)),
        ("secondary-shift-v", Box::new(Validate)),
        ("secondary-shift-k", Box::new(SyntaxCheck)),
        ("secondary-b", Box::new(ToggleOptions)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Action::name` on a `Box<dyn Action>` resolves without this, but not on a concrete action
    // struct, which is what `the_decode_selection_action_is_deliberately_unbound` holds.
    use gpui_kit::Action as _;
    use gpui_kit::{Keystroke, OperatingSystem};

    /// An unparseable keystroke panics inside `KeyBinding`, at start-up, before a window opens —
    /// so this is the test that stands between a typo here and an application that cannot run at
    /// all. It is checked against every OS because `secondary-` resolves differently on each and
    /// only the host's answer would otherwise be exercised.
    #[test]
    fn every_keystroke_parses_on_every_platform() {
        for (keystrokes, action) in keymap() {
            for os in [
                OperatingSystem::Mac,
                OperatingSystem::Linux,
                OperatingSystem::Windows,
            ] {
                Keystroke::parse_with_os(keystrokes, os).unwrap_or_else(|error| {
                    panic!(
                        "`{keystrokes}` ({}) does not parse on {os:?}: {error:?}",
                        action.name()
                    )
                });
            }
        }
    }

    /// The context predicate is parsed with `expect` in [`bind_keys`], which runs before the
    /// window opens; a malformed one would be a start-up panic too.
    #[test]
    fn the_key_context_parses() {
        assert!(KeyBindingContextPredicate::parse(CONTEXT).is_ok());
    }

    /// Two actions on one keystroke means the later binding wins and a documented shortcut
    /// silently does nothing. The reverse — two keystrokes for one action — is deliberate for
    /// `FindNext` and `FindPrevious`, so only the keystrokes are checked for uniqueness.
    #[test]
    fn no_keystroke_is_bound_twice() {
        let keymap = keymap();
        for (index, (keystrokes, _)) in keymap.iter().enumerate() {
            assert!(
                !keymap[..index].iter().any(|(other, _)| other == keystrokes),
                "`{keystrokes}` is bound twice"
            );
        }
    }

    /// `DecodeSelection` is reached from the EDN editor's context menu and from nowhere else, on
    /// purpose: a shortcut would fire with nothing selected and could only report that. Asserted
    /// rather than left as a comment so that adding one means reading the reason first.
    #[test]
    fn the_decode_selection_action_is_deliberately_unbound() {
        let name = DecodeSelection.name();
        assert!(
            !keymap().iter().any(|(_, action)| action.name() == name),
            "`{name}` has gained a keybinding; see the comment beside the action"
        );
    }

    /// `Replace` is deliberately unbound for a different reason from the two below it: not because
    /// a shortcut would fire in the wrong place, but because the gesture *already has* one.
    /// gpui-base binds `ctrl-h` to its own `input::Replace` in the deeper `"Input"` key context
    /// (`base/state.rs:310-316`), which is the context the focused editor runs in. Binding
    /// `secondary-h` here as well would put two actions behind one user intention, and the menu
    /// item is the only thing that was missing.
    #[test]
    fn the_replace_action_is_deliberately_unbound() {
        let name = Replace.name();
        assert!(
            !keymap().iter().any(|(_, action)| action.name() == name),
            "`{name}` has gained a keybinding; see the comment beside the action"
        );
    }

    /// `OpenInclude` is reached from the CDDL editor's context menu and from the right pane's
    /// chain list, and from nowhere else: a shortcut would fire with the caret on an ordinary line
    /// and could only report that there was no directive under it.
    #[test]
    fn the_open_include_action_is_deliberately_unbound() {
        let name = OpenInclude.name();
        assert!(
            !keymap().iter().any(|(_, action)| action.name() == name),
            "`{name}` has gained a keybinding; see the comment beside the action"
        );
    }

    /// The seven keystore and COSE commands, for the two reasons recorded beside them: three of
    /// them write to or delete from the key store, and four of them need both a selected key and a
    /// loaded document, so a chord pressed without either could only report that.
    ///
    /// One test over the set rather than seven near-identical ones, because the argument is the
    /// same for all seven and seven copies of it would be seven places to keep in step.
    #[test]
    fn the_keystore_and_cose_actions_are_deliberately_unbound() {
        let names = [
            GenerateKey.name(),
            DeleteKey.name(),
            ImportKey.name(),
            Sign.name(),
            Verify.name(),
            Encrypt.name(),
            Decrypt.name(),
        ];
        for name in names {
            assert!(
                !keymap().iter().any(|(_, action)| action.name() == name),
                "`{name}` has gained a keybinding; see the comment beside the action"
            );
        }
    }
}
