/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The application menu, defined once and installed differently per platform.
//!
//! `UI_DESIGN.md:49` asks for a menu that "should comply generally with the recommendations of the
//! running system", which on macOS means the system menu bar and on Windows and Linux an in-window
//! one. gpui-component supplies the latter as `AppMenuBar`, and — usefully — it reads the *same*
//! menu description that `cx.set_menus` takes, out of `GlobalState::app_menus()`. So [`menus`]
//! builds the structure once and [`install`] only chooses where to put it.
//!
//! Two facts about that worth knowing, since neither is documented upstream:
//!
//! - **Populating `GlobalState` is the application's job.** gpui-component never calls
//!   `set_app_menus` itself, so an `AppMenuBar` in a fresh app renders empty. The plan expected to
//!   have to hand-roll a `PopupMenu` bar instead; this is the simplification that replaced it.
//! - **`AppMenuBar` does not observe the menu list.** `AppMenuBar::reload` is public and must be
//!   called after each [`install`] (`gpui-component/src/menu/app_menu_bar.rs:50`), which is why
//!   [`install`] is cheap to call again and the caller is expected to do so whenever [`MenuState`]
//!   changes. Greying `Save` out is a state change like any other.

use crate::actions::{
    About, Copy, Decrypt, DeleteKey, Encrypt, Find, FindNext, FindPrevious, GenerateKey, ImportKey,
    Open, OpenFolder, Paste, Quit, Replace, Save, SaveAs, Sign, SyntaxCheck, ToCbor, ToEdn,
    ToggleOptions, Validate, Verify,
};
use crate::tab::Tab;
use gpui_kit::{App, Menu, MenuItem};

/// The parts of the application's state the menu reflects.
///
/// Passing this in rather than reading a `Workspace` keeps [`menus`] a pure function of some bools
/// and a tab, which is what makes the enable/disable rules testable without a window.
///
/// This said "four bools" from Phase 2 until the field count passed six without anyone noticing —
/// it was already wrong by Phase 7. Counted prose in a doc comment goes stale silently, so it is
/// not counted here any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuState {
    /// The selected tab. COSE gains a `Keystore` menu (`UI_DESIGN.md:123`); the conversion items
    /// differ between the CBOR and EDN tabs (`:79`, `:95`).
    pub tab: Tab,
    /// Whether the active document has a path. `UI_DESIGN.md:70`: *"This option is greyed out if
    /// the current file doesn't have a name."*
    pub has_path: bool,
    /// Whether the active document is read-only. `UI_DESIGN.md:53`: *"Paste does not work on files
    /// which are marked read-only."*
    pub read_only: bool,
    /// Whether there is anything to convert or validate. Keeps the verbs greyed out on an empty
    /// document rather than letting them report an error about one.
    pub has_content: bool,
    /// Whether a key is selected in the keystore list (`UI_DESIGN.md:128`).
    ///
    /// All five keystore commands but `Generate Key...` and `Import Key...` need one: there is
    /// nothing to sign *with*, and nothing to delete, until the user has picked a key.
    pub key_selected: bool,
    /// Whether the **CBOR** document has content, whatever tab is selected.
    ///
    /// A second content flag rather than a reuse of [`MenuState::has_content`], because on the COSE
    /// tab the two describe different documents. `has_content` follows the selected tab, so on COSE
    /// it is about the COSE object — which is what `Verify` and `Decrypt` read. `Sign` and
    /// `Encrypt` read the *CBOR* document as their input (Phase 7's decision 3), so they need this
    /// one. Conflating them would offer `Sign` on an empty workspace holding only a loaded
    /// signature, and grey it out on exactly the workspace it is for.
    pub cbor_content: bool,
    /// Whether a `Validate` is already running on the background executor.
    ///
    /// The only flag here that is about the application rather than the documents. It greys
    /// `Validate` alone: everything else stays usable, which is the point of the trace having been
    /// moved off the main thread in the first place.
    pub validating: bool,
}

/// Which of the four COSE operations apply, given the state.
///
/// One rule with two readers: [`menus`] greys the menu items by it and the right pane greys its
/// four buttons by it. Phase 5 and Phase 6 both found the same class of bug the other way round --
/// a menu item enabled with no handler listening -- and a struct returned from one function is what
/// stops the pane and the menu bar from disagreeing about whether a button does anything.
///
/// The asymmetry in the content flag is the only subtle part, and [`MenuState::cbor_content`]
/// records why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Operations {
    /// Sign the CBOR document under the selected key, producing a `COSE_Sign1`.
    pub sign: bool,
    /// Verify the loaded COSE object under the selected key, producing its payload.
    pub verify: bool,
    /// Encrypt the CBOR document under the selected key, producing a `COSE_Encrypt0`.
    pub encrypt: bool,
    /// Decrypt the loaded COSE object under the selected key, producing its plaintext.
    pub decrypt: bool,
}

/// The four operations' enablement rule.
///
/// Free and public so that both readers call it rather than each writing the conjunction out, and
/// so that a test can read it without a window.
pub fn operations(state: MenuState) -> Operations {
    // Not a per-field repetition: every one of the four needs the COSE tab and a selected key, and
    // only the input document differs.
    let ready = state.tab == Tab::Cose && state.key_selected;
    Operations {
        sign: ready && state.cbor_content,
        verify: ready && state.has_content,
        encrypt: ready && state.cbor_content,
        decrypt: ready && state.has_content,
    }
}

/// Builds the menu bar.
///
/// `Quit` is in the File menu on Windows and Linux and in the application menu on macOS, which the
/// OS supplies — so it is listed here unconditionally and simply appears twice on macOS, the same
/// compromise Zed makes. It is the one item `UI_DESIGN.md:50` requires always be offered.
pub fn menus(state: MenuState) -> Vec<Menu> {
    let mut menus = vec![file_menu(state), edit_menu(state), view_menu(state)];
    if state.tab == Tab::Cose {
        menus.push(keystore_menu(state));
    }
    menus.push(help_menu());
    menus
}

fn file_menu(state: MenuState) -> Menu {
    Menu::new("File").items([
        MenuItem::action("Open...", Open),
        // An addition to `UI_DESIGN.md:68`'s File menu, and the reason is beside
        // [`crate::actions::OpenFolder`]: one picker cannot offer files and directories at once on
        // Linux, so the command that sets the explorer's root is its own item rather than a second
        // meaning of `Open...`. Never disabled — a root can always be chosen.
        MenuItem::action("Open Folder...", OpenFolder),
        MenuItem::separator(),
        MenuItem::action("Save", Save).disabled(!state.has_path),
        MenuItem::action("Save As...", SaveAs),
        MenuItem::separator(),
        MenuItem::action("Quit", Quit),
    ])
}

fn edit_menu(state: MenuState) -> Menu {
    Menu::new("Edit").items([
        MenuItem::action("Copy", Copy),
        // The only item whose enablement `UI_DESIGN.md` states outright.
        MenuItem::action("Paste", Paste).disabled(state.read_only),
        MenuItem::separator(),
        MenuItem::action("Find...", Find),
        MenuItem::action("Find Next", FindNext),
        MenuItem::action("Find Previous", FindPrevious),
        // An addition to `UI_DESIGN.md:52`'s Edit menu, recorded in `PLAN.md`. Disabled on two
        // counts rather than one: a read-only document cannot be written to, and the CBOR tab's
        // find bar searches *bytes* in a fixed-length buffer, where "replace all" would mean
        // either an overwrite of every match or a length change the hex grid has no gesture for.
        // Find is offered there and Replace is not, which is the honest pair.
        MenuItem::action("Replace...", Replace).disabled(state.read_only || state.tab == Tab::Cbor),
    ])
}

/// The View menu carries the tab selection and the right pane toggle.
///
/// The four tabs are listed here, not in a tab-strip-only control, because `UI_DESIGN.md:54` asks
/// for platform-convention shortcuts and a shortcut the user cannot discover from a menu is a
/// shortcut nobody finds. The conversion verbs sit below them: which ones apply depends on the
/// selected tab (`:79-84`, `:95-99`, `:113-117`), so the inapplicable ones are greyed rather than
/// removed — a menu whose items move around is harder to learn than one whose items grey out.
fn view_menu(state: MenuState) -> Menu {
    let mut items: Vec<MenuItem> = Tab::ALL
        .iter()
        .map(|tab| MenuItem::Action {
            name: tab.label(),
            action: tab.select_action(),
            os_action: None,
            checked: *tab == state.tab,
            disabled: false,
        })
        .collect();

    items.push(MenuItem::separator());
    items.push(
        MenuItem::action("Convert to EDN", ToEdn)
            .disabled(state.tab != Tab::Cbor || !state.has_content),
    );
    items.push(
        MenuItem::action("Convert to CBOR", ToCbor)
            .disabled(state.tab != Tab::Edn || !state.has_content),
    );
    items.push(MenuItem::action("Validate", Validate).disabled(
        !matches!(state.tab, Tab::Cbor | Tab::Edn) || !state.has_content || state.validating,
    ));
    items.push(
        MenuItem::action("Syntax Check", SyntaxCheck)
            .disabled(state.tab != Tab::Cddl || !state.has_content),
    );
    items.push(MenuItem::separator());
    items.push(MenuItem::action("Options Pane", ToggleOptions));

    Menu::new("View").items(items)
}

/// `UI_DESIGN.md:123`: *"A new top-level menu: 'Keystore' is added."* — added, so it is offered on
/// the COSE tab only.
///
/// Three groups, and two of the three are additions to the specification recorded in `PLAN.md`:
///
/// * `Generate Key...`, which is `:124-125` and all the spec names.
/// * `Import Key...` and `Delete Key...`. The spec offers no way to remove a key, and a store that
///   can only be added to is a trap; the import is the same gesture `UI_DESIGN.md:119` reaches
///   through File → Open, offered here as well because a user looking for "get a key into the
///   tool" looks in the Keystore menu.
/// * The four operations. `:129` puts them in the right pane, and they are there — but as *buttons
///   dispatching these actions*, so they also belong in the menu bar, where `UI_DESIGN.md:54`'s
///   discoverability argument applies and where [`operations`] greys them by the same rule.
fn keystore_menu(state: MenuState) -> Menu {
    let ops = operations(state);
    Menu::new("Keystore").items([
        MenuItem::action("Generate Key...", GenerateKey),
        MenuItem::action("Import Key...", ImportKey),
        // Destructive, so it is ellipsised: it goes through a confirmation, like Quit's guard.
        MenuItem::action("Delete Key...", DeleteKey).disabled(!state.key_selected),
        MenuItem::separator(),
        MenuItem::action("Sign", Sign).disabled(!ops.sign),
        MenuItem::action("Verify", Verify).disabled(!ops.verify),
        MenuItem::action("Encrypt", Encrypt).disabled(!ops.encrypt),
        MenuItem::action("Decrypt", Decrypt).disabled(!ops.decrypt),
    ])
}

fn help_menu() -> Menu {
    Menu::new("Help").items([MenuItem::action("About cbor-ide", About)])
}

/// Installs the menu for the running platform.
///
/// macOS gets the real system menu bar. Everywhere else the description goes into `GlobalState`,
/// where an `AppMenuBar` rendered in the window picks it up — see the module docs for the reload
/// the caller still owes.
pub fn install(state: MenuState, cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.set_menus(menus(state));

    #[cfg(not(target_os = "macos"))]
    {
        let owned = menus(state).into_iter().map(Menu::owned).collect();
        gpui_kit::component::global_state::GlobalState::global_mut(cx).set_app_menus(owned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name and enabled state of every item, flattened, for assertions below.
    fn items(menu: &Menu) -> Vec<(String, bool)> {
        menu.items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some((name.to_string(), !item.is_disabled())),
                _ => None,
            })
            .collect()
    }

    fn menu<'a>(menus: &'a [Menu], name: &str) -> &'a Menu {
        menus
            .iter()
            .find(|menu| menu.name == name)
            .unwrap_or_else(|| panic!("there is a `{name}` menu"))
    }

    fn enabled(menus: &[Menu], menu_name: &str, item: &str) -> bool {
        items(menu(menus, menu_name))
            .into_iter()
            .find(|(name, _)| name == item)
            .unwrap_or_else(|| panic!("`{menu_name}` has an item `{item}`"))
            .1
    }

    /// The three things `UI_DESIGN.md:50-52` says must *always* be offered, on every tab and in
    /// every state. This is the one test that would catch them being lost to a refactor.
    #[test]
    fn quit_help_and_find_are_always_offered() {
        for tab in Tab::ALL {
            for read_only in [false, true] {
                let state = MenuState {
                    tab,
                    read_only,
                    ..MenuState::default()
                };
                let menus = menus(state);

                assert!(
                    enabled(&menus, "File", "Quit"),
                    "{tab:?}: Quit must be offered and usable"
                );
                assert!(
                    !menu(&menus, "Help").items.is_empty(),
                    "{tab:?}: a Help menu must be offered"
                );
                for item in ["Find...", "Find Next"] {
                    assert!(
                        enabled(&menus, "Edit", item),
                        "{tab:?}: `{item}` must be offered"
                    );
                }
                assert!(
                    enabled(&menus, "Edit", "Copy"),
                    "{tab:?}: Copy must be offered"
                );
            }
        }
    }

    /// `UI_DESIGN.md:53`: Paste does not work on read-only files. Copy still does — the asymmetry
    /// is the point, since every tab but EDN starts read-only.
    #[test]
    fn paste_is_disabled_on_a_read_only_document() {
        let writable = menus(MenuState::default());
        assert!(enabled(&writable, "Edit", "Paste"));

        let read_only = menus(MenuState {
            read_only: true,
            ..MenuState::default()
        });
        assert!(!enabled(&read_only, "Edit", "Paste"));
        assert!(
            enabled(&read_only, "Edit", "Copy"),
            "read-only must not disable Copy"
        );
    }

    /// `Replace...` is the one Edit item that is *not* always offered, and the two reasons it is
    /// disabled are both recorded beside it. Exhaustive over `Tab::ALL` and both read-only states,
    /// because the rule is a disjunction and a test of one tab would pass with either half of it
    /// deleted.
    #[test]
    fn replace_needs_a_writable_text_document() {
        for tab in Tab::ALL {
            for read_only in [false, true] {
                let state = MenuState {
                    tab,
                    read_only,
                    ..MenuState::default()
                };
                assert_eq!(
                    enabled(&menus(state), "Edit", "Replace..."),
                    !read_only && tab != Tab::Cbor,
                    "Replace enablement is wrong on {tab:?} (read_only: {read_only})"
                );
            }
        }
    }

    /// `UI_DESIGN.md:70`: Save is greyed out when the document has no name. Save As never is —
    /// being unnamed is exactly when it is needed.
    #[test]
    fn save_needs_a_name_but_save_as_does_not() {
        let unnamed = menus(MenuState::default());
        assert!(!enabled(&unnamed, "File", "Save"));
        assert!(enabled(&unnamed, "File", "Save As..."));

        let named = menus(MenuState {
            has_path: true,
            ..MenuState::default()
        });
        assert!(enabled(&named, "File", "Save"));
    }

    /// `UI_DESIGN.md:123`: the Keystore menu is *added* on the COSE tab, so it must not be there
    /// on the others.
    #[test]
    fn the_keystore_menu_belongs_to_the_cose_tab_only() {
        for tab in Tab::ALL {
            let present = menus(MenuState {
                tab,
                ..MenuState::default()
            })
            .iter()
            .any(|menu| menu.name == "Keystore");
            assert_eq!(
                present,
                tab == Tab::Cose,
                "{tab:?}: Keystore menu presence is wrong"
            );
        }
    }

    /// The conversion verbs follow the tab. Each is enabled on exactly one tab, and only with
    /// content to act on.
    #[test]
    fn the_conversion_verbs_follow_the_selected_tab() {
        for (item, owner) in [
            ("Convert to EDN", Tab::Cbor),
            ("Convert to CBOR", Tab::Edn),
            ("Syntax Check", Tab::Cddl),
        ] {
            for tab in Tab::ALL {
                let state = MenuState {
                    tab,
                    has_content: true,
                    ..MenuState::default()
                };
                assert_eq!(
                    enabled(&menus(state), "View", item),
                    tab == owner,
                    "`{item}` enablement is wrong on {tab:?}"
                );
            }

            let empty = MenuState {
                tab: owner,
                has_content: false,
                ..MenuState::default()
            };
            assert!(
                !enabled(&menus(empty), "View", item),
                "`{item}` must be greyed out with nothing to act on"
            );
        }
    }

    /// Validate is the exception: it is offered on both tabs that hold a CBOR value
    /// (`UI_DESIGN.md:79`, `:95`), and on neither of the other two.
    #[test]
    fn validate_is_offered_on_both_the_cbor_and_edn_tabs() {
        for tab in Tab::ALL {
            let state = MenuState {
                tab,
                has_content: true,
                ..MenuState::default()
            };
            assert_eq!(
                enabled(&menus(state), "View", "Validate"),
                matches!(tab, Tab::Cbor | Tab::Edn),
                "Validate enablement is wrong on {tab:?}"
            );
        }
    }

    /// A `Validate` already in flight greys the menu item, on every tab that would otherwise offer
    /// it — the half of the re-entrancy guard that is visible. The other half is in
    /// `Ide::on_validate`, because `ctrl-shift-v` never consults this.
    #[test]
    fn validate_is_greyed_out_while_one_is_already_running() {
        for tab in Tab::ALL {
            let state = MenuState {
                tab,
                has_content: true,
                validating: true,
                ..MenuState::default()
            };
            assert!(
                !enabled(&menus(state), "View", "Validate"),
                "Validate must be greyed out on {tab:?} while a validation is running"
            );
        }
    }

    /// The four operations need the COSE tab, a selected key, and something to act on — and
    /// exhaustively over `Tab::ALL`, because the menu they live in is only built on one tab and a
    /// rule that reads `state.tab == Tab::Cose` is only as good as the tab it is given.
    #[test]
    fn the_cose_operations_need_a_key_and_a_document() {
        for tab in Tab::ALL {
            let ops = operations(MenuState {
                tab,
                key_selected: true,
                has_content: true,
                cbor_content: true,
                ..MenuState::default()
            });
            let expected = tab == Tab::Cose;
            assert_eq!(ops.sign, expected, "Sign on {tab:?}");
            assert_eq!(ops.verify, expected, "Verify on {tab:?}");
            assert_eq!(ops.encrypt, expected, "Encrypt on {tab:?}");
            assert_eq!(ops.decrypt, expected, "Decrypt on {tab:?}");
        }

        let no_key = operations(MenuState {
            tab: Tab::Cose,
            has_content: true,
            cbor_content: true,
            ..MenuState::default()
        });
        assert_eq!(no_key, Operations::default(), "nothing applies with no key");
    }

    /// The asymmetry [`MenuState::cbor_content`] exists for: on the COSE tab the two content flags
    /// describe different documents, so a loaded signature with an empty CBOR document offers
    /// `Verify` and not `Sign`, and the reverse for a loaded CBOR document with no signature.
    #[test]
    fn signing_reads_the_cbor_document_and_verifying_reads_the_cose_one() {
        let base = MenuState {
            tab: Tab::Cose,
            key_selected: true,
            ..MenuState::default()
        };

        let to_sign = operations(MenuState {
            cbor_content: true,
            ..base
        });
        assert!(to_sign.sign && to_sign.encrypt, "a CBOR document to sign");
        assert!(
            !to_sign.verify && !to_sign.decrypt,
            "there is no COSE object to verify"
        );

        let to_verify = operations(MenuState {
            has_content: true,
            ..base
        });
        assert!(
            to_verify.verify && to_verify.decrypt,
            "a COSE object to verify"
        );
        assert!(
            !to_verify.sign && !to_verify.encrypt,
            "there is no payload to sign"
        );
    }

    /// The menu items are greyed by the same rule the right pane's buttons are, which is the whole
    /// reason [`operations`] is a function rather than four inline conjunctions.
    #[test]
    fn the_keystore_menu_items_follow_the_operations_rule() {
        for key_selected in [false, true] {
            for cbor_content in [false, true] {
                for has_content in [false, true] {
                    let state = MenuState {
                        tab: Tab::Cose,
                        key_selected,
                        cbor_content,
                        has_content,
                        ..MenuState::default()
                    };
                    let menus = menus(state);
                    let ops = operations(state);
                    assert_eq!(enabled(&menus, "Keystore", "Sign"), ops.sign, "{state:?}");
                    assert_eq!(
                        enabled(&menus, "Keystore", "Verify"),
                        ops.verify,
                        "{state:?}"
                    );
                    assert_eq!(
                        enabled(&menus, "Keystore", "Encrypt"),
                        ops.encrypt,
                        "{state:?}"
                    );
                    assert_eq!(
                        enabled(&menus, "Keystore", "Decrypt"),
                        ops.decrypt,
                        "{state:?}"
                    );
                    // The two that need no document, and the one that needs only a key.
                    assert!(enabled(&menus, "Keystore", "Generate Key..."));
                    assert!(enabled(&menus, "Keystore", "Import Key..."));
                    assert_eq!(
                        enabled(&menus, "Keystore", "Delete Key..."),
                        key_selected,
                        "{state:?}"
                    );
                }
            }
        }
    }

    /// The selected tab is the checked one, and only it. Without this the View menu would be a
    /// list of four identical-looking items with no indication of where you are.
    #[test]
    fn exactly_the_selected_tab_is_checked() {
        for selected in Tab::ALL {
            let menus = menus(MenuState {
                tab: selected,
                ..MenuState::default()
            });
            let checked: Vec<_> = menu(&menus, "View")
                .items
                .iter()
                .filter(|item| item.is_checked())
                .filter_map(|item| match item {
                    MenuItem::Action { name, .. } => Some(name.to_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(checked, vec![selected.label().to_string()]);
        }
    }
}
