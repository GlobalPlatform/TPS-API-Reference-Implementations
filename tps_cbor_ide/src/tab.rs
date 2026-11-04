/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Which of the four tabs is selected.
//!
//! This is the one piece of state that nearly everything in the window reads: the centre tab strip
//! renders it, the right pane's content is chosen by it (`UI_DESIGN.md:64`), the menu bar grows a
//! `Keystore` menu only on COSE (`:123`), and four keybindings set it.
//!
//! It lives here rather than in `tps_cbor_core` because it is UI state and nothing else —
//! `Workspace` holds all four documents at once and has no opinion about which one is on screen.
//! That is also why the plan's Phase 2 file list does not mention this module; see `PLAN.md`.

use crate::actions::{SelectCborTab, SelectCddlTab, SelectCoseTab, SelectEdnTab};
use gpui_kit::{Action, SharedString};

/// The four tabs of the centre pane, in `UI_DESIGN.md:57` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    /// Hex view over the CBOR binary document.
    #[default]
    Cbor,
    /// Editor over the EDN document.
    Edn,
    /// Editor over the CDDL include chain — the one tab holding more than one file.
    Cddl,
    /// EDN of the encoded COSE structure.
    Cose,
}

impl Tab {
    /// Every tab, in display order.
    pub const ALL: [Tab; 4] = [Tab::Cbor, Tab::Edn, Tab::Cddl, Tab::Cose];

    /// The tab strip's label.
    pub fn label(self) -> SharedString {
        match self {
            // `UI_DESIGN.md:66` names this one "CBOR Binary"; the others are named after their
            // format alone.
            Tab::Cbor => "CBOR Binary".into(),
            Tab::Edn => "EDN".into(),
            Tab::Cddl => "CDDL".into(),
            Tab::Cose => "COSE".into(),
        }
    }

    /// The name `Save As` offers for a document that has never had one.
    ///
    /// `UI_DESIGN.md:89` asks for `.edn` to be the EDN tab's default extension, and the same
    /// argument applies to each of the other three. Without this an *untitled* document — which is
    /// exactly what `To CBOR`, `To EDN` and the decode produce, since all three clear the path —
    /// reached the system picker with no name and no extension at all.
    ///
    /// On `Tab` rather than in the editors because the problem is general, because the exhaustive
    /// match means a fifth tab cannot be added without answering it, and because a pure function
    /// is testable without a window.
    pub fn default_file_name(self) -> &'static str {
        match self {
            Tab::Cbor => "untitled.cbor",
            Tab::Edn => "untitled.edn",
            Tab::Cddl => "untitled.cddl",
            // COSE artifacts are CBOR too (`main.rs`'s `open_path` says so), so this follows the
            // encoding and not the tab's name.
            Tab::Cose => "untitled.cbor",
        }
    }

    /// The action that selects this tab, for the menu and the keymap.
    ///
    /// Boxed because the four actions are distinct types; see `actions.rs` for why they are four
    /// actions rather than one carrying an index.
    pub fn select_action(self) -> Box<dyn Action> {
        match self {
            Tab::Cbor => Box::new(SelectCborTab),
            Tab::Edn => Box::new(SelectEdnTab),
            Tab::Cddl => Box::new(SelectCddlTab),
            Tab::Cose => Box::new(SelectCoseTab),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` is what the tab strip iterates, so a tab missing from it is a tab the user cannot
    /// reach. Checking the count against the variants is the cheapest guard available: Rust will
    /// not tell us that a new variant was left out of a `const` array.
    #[test]
    fn every_tab_is_in_all_exactly_once() {
        for tab in Tab::ALL {
            assert_eq!(
                Tab::ALL.iter().filter(|t| **t == tab).count(),
                1,
                "{tab:?} appears more than once in ALL"
            );
        }
        // Exhaustive match: adding a variant without adding it to `ALL` fails to compile here.
        for tab in Tab::ALL {
            match tab {
                Tab::Cbor | Tab::Edn | Tab::Cddl | Tab::Cose => {}
            }
        }
        assert_eq!(Tab::ALL.len(), 4);
    }

    /// Each tab must map to its *own* action. A copy-paste slip here would silently make two tabs
    /// unreachable from both the keymap and the menu.
    #[test]
    fn each_tab_selects_itself() {
        let names: Vec<_> = Tab::ALL
            .iter()
            .map(|tab| tab.select_action().name())
            .collect();
        for (ix, name) in names.iter().enumerate() {
            assert!(
                !names[..ix].contains(name),
                "{name} is the select action for two different tabs"
            );
        }
        assert_eq!(Tab::Cbor.select_action().name(), SelectCborTab.name());
        assert_eq!(Tab::Cose.select_action().name(), SelectCoseTab.name());
    }

    /// `UI_DESIGN.md:89` names `.edn` for the EDN tab, and `on_save_as` falls back to this for any
    /// untitled document — so a tab whose default carried no extension would put the system picker
    /// back in the state this exists to fix. Exhaustive over `ALL`, so a fifth tab has to answer
    /// the question too.
    #[test]
    fn every_tab_offers_a_default_file_name_with_its_own_extension() {
        for tab in Tab::ALL {
            let name = tab.default_file_name();
            let (stem, extension) = name
                .rsplit_once('.')
                .unwrap_or_else(|| panic!("{tab:?} defaults to `{name}`, which has no extension"));
            assert!(!stem.is_empty(), "{tab:?} defaults to `{name}`");
            assert!(!extension.is_empty(), "{tab:?} defaults to `{name}`");
        }
        assert_eq!(Tab::Edn.default_file_name(), "untitled.edn");
    }

    /// The extension names the encoding the document is *saved in*, which is not always the tab's
    /// own name: Phase 7 made `Workspace.cose` a byte document, so the COSE tab writes the COSE
    /// bytes and not the EDN text rendered from them, and `Save As` has to offer a name that
    /// agrees with what lands on disk. The two byte tabs therefore share one extension while the
    /// two text tabs each carry their own.
    ///
    /// Exhaustive over `ALL` and written as a match, so a fifth tab cannot be added without
    /// saying which side of that line it falls on — the same discipline
    /// [`Tab::default_file_name`] itself is written under. Pinned here rather than left to the
    /// loop above, which only checks that *an* extension exists: a COSE tab defaulting to
    /// `untitled.edn` would pass that and still write CBOR into a file named for text.
    #[test]
    fn the_default_extension_names_the_encoding_and_not_the_tab() {
        for tab in Tab::ALL {
            let encoding = match tab {
                Tab::Cbor | Tab::Cose => "cbor",
                Tab::Edn => "edn",
                Tab::Cddl => "cddl",
            };
            let name = tab.default_file_name();
            assert_eq!(
                name.rsplit_once('.').map(|(_, extension)| extension),
                Some(encoding),
                "{tab:?} saves {encoding} but offers the name `{name}`"
            );
        }
    }
}
