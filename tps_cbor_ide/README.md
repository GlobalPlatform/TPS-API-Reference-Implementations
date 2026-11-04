# tps_cbor_ide

A native workbench for CBOR, EDN, CDDL and COSE artefacts, built on
[GPUI Kit](https://crates.io/crates/gpui-kit) over the shared domain layer
[`tps_cbor_core`](../tps_cbor_core).

It replaces `tps_cbor_gui`, the Tauri/React application, feature for feature —
see [`../tps_cbor_gui/RETIRED.md`](../tps_cbor_gui/RETIRED.md). The user interface is not a port
of that one: it is specified from scratch in [`UI_DESIGN.md`](UI_DESIGN.md), which is the
specification this crate is written against and is **never** edited to match the implementation.
Where the two diverge, the divergence is recorded in [`PLAN.md`](PLAN.md).

The binary is `cbor-ide`.

## What it does

Four tabs over one workspace, each with its own right-hand options pane
(`UI_DESIGN.md:57-64`):

| Tab | What it holds |
|---|---|
| **CBOR Binary** | A hex and ASCII grid over the raw buffer, editable in place, with selection, clipboard and byte or text search |
| **EDN** | The same bytes as CBOR extended diagnostic notation, syntax highlighted and editable, with a byte range ↔ text range map linking the two tabs |
| **CDDL** | A set of schema files — one inner tab per file in an `;# include` chain — with per-file and chain-wide syntax checking, an AST tree, and clickable includes |
| **COSE** | `COSE_Sign1` and `COSE_Encrypt0` artefacts: sign, verify, encrypt, decrypt, and an on-disk key store |

Below them is a diagnostics window: an append-only log of what was asked for and what came back,
with a validation trace page and a key-store page. A diagnostic row is clickable and reveals the
span it names, in the file it names, opening that file if it is not already an inner tab.

## Not for embedded targets

Like `tps_cbor_core` and `tps_cbor_tui`, this crate is deliberately exempt from the workspace's
`no_std` and cross-compilation requirements (root `CLAUDE.md`). It is an interactive desktop
application: it needs `std`, a filesystem, a window system and a GPU. It is `publish = false`, and
it is the only crate here that needs the MPL-2.0 exceptions in the root `deny.toml`.

Nothing in it is needed to use the libraries it is built on from an embedded target.

## It is its own Cargo workspace

**Build and test it from this directory**, or with `--manifest-path tps_cbor_ide/Cargo.toml`. A
root-level `cargo build` does not include it.

The reason is the `links = "tree-sitter"` key on the `tree-sitter` crate, which permits exactly
one version of it per resolution: `gpui-kit` requires ^0.26.13 while `tps_cbor_tui`,
`tps_ts_highlight_tool` and the two grammars' dev-dependencies require ^0.27. Those are
irreconcilable in one graph, and a workspace member's dev-dependencies count towards resolution
even when nothing is testing it. The grammars are shared as *path* dependencies, which is safe
because they expose only `tree_sitter_language::LanguageFn`, the ABI-stable handoff type, and a
path dependency's own dev-dependencies are not resolved from outside its workspace.

Undo the separation once `gpui-component` requires `tree-sitter` 0.27 or later.

## Layout

| Module | Responsibility |
|---|---|
| `main` | `Ide`: the window, the four tabs' shared state, every action handler, and the right and lower panes |
| `actions` | Every command, defined once as a GPUI `Action`, plus the keymap |
| `menus` | The menu bar, built from the actions, with one place that decides enablement |
| `tab` | `Tab` and `LowerTab`, and which lower pages each upper tab offers |
| `layout` | Pane geometry and the sizes that outlive the session |
| `explorer` | The left-hand file tree |
| `hex_view` | The CBOR tab's grid: cursor, selection, marks, clipboard and search |
| `edn_view` | The EDN tab's editor and its span-linked selection |
| `cddl_view` | The CDDL tab's editors — one per inner tab — the AST tree and the include list |
| `cose_view` | The COSE tab's read-only EDN rendering of the artefact |
| `keys` | The key-store page and the key-detail pane |
| `languages` | Registering the `cddl` and `edn` grammars with gpui-component's highlighter |
| `theme` | Mapping our tree-sitter captures onto gpui-component's syntax palette |

All domain logic — the document model, conversion, span mapping, CDDL validation and tracing,
COSE, key generation and the key store — is in `tps_cbor_core`. This crate is GPUI contact and
nothing else; that is what keeps one tested copy of each algorithm shared with `tps_cbor_tui`.

## Design notes

These are the decisions a reader is most likely to want explained, and the ones most likely to be
undone by accident. Each is also recorded beside the code it governs.

### `Editor::readonly` is a builder on the element, not state

`Editor::readonly(bool)` sets a flag on the *element* being built this paint, and the element is
rebuilt every paint — so it is not a place to keep a document's read-only flag. The state's own
`set_editable` is what persists. Setting one and reading the other is why a read-only CDDL file
accepted keystrokes for an entire phase.

### Text is pushed into the editors at the *next* paint, not when the model changes

`CddlView::reload` and `EdnView::reload` set a pending flag; `apply` honours it from `render`.
Both `EditorState::new` and `set_value` need a `&mut Window`, which a model-change notification
does not have. The deferral has a second consequence that is load-bearing: `flush` refuses to
write editor text back into the model while a reload is pending, because a `Save` landing between
the two would push the previous chain's text over the one just opened.

### The dialog builder closure is a `Fn` and may run again

`open_alert_dialog`'s builder re-runs whenever the dialog layer is rebuilt, so nothing expensive
is constructed inside it — the content is built once, outside, and the closure clones. Note also
that `AlertDialog`'s default is an OK button alone; `.confirm()` *adds* a Cancel, so it is wrong
for a dialog with nothing to cancel.

`Window::prompt` is used instead of a dialog in exactly one place, `confirm_quit`, because that
one has to *await* an answer across a spawned task, which a dialog cannot give.

### `taffy = "=0.15.0"` is pinned, and the pin is not cosmetic

`gpui-kit`'s optional `gpui-fast` feature requires `taffy "=0.15.0"` while `gpui-pre` requires
`^0.15.0` and resolves it to 0.15.1. Any resolution that considers optional dependencies is
therefore unsatisfiable — and that includes `cargo metadata`, so `cargo deny`, `cargo tree
--all-features`, `cargo audit` and the headless test harness all fail outright without this pin.
A plain `cargo build` is unaffected, which is what makes it easy to remove by mistake.

### Key contexts resolve from the focused element outwards

Our actions are bound in the single root `Ide` key context; gpui-base binds `ctrl-f`, `ctrl-h`,
`ctrl-c` and `ctrl-v` in the deeper `"Input"` context. A binding in an outer context **cannot**
shadow one in an inner context, so the editor's own shortcuts always win and the root bindings
fire only where no editor holds the focus. The two coexist rather than competing — and the
handlers forward to the editor anyway, so the design is right whichever wins.

The corollary matters for the menu: `Window::dispatch_action` reads the *currently* focused
element and then defers delivery up that element's chain, so a `Copy` dispatched from the menu
bar would be delivered to the menu bar. `Ide::forward` focuses the selected tab's pane first.

### `tree()` renders nothing without a bounded ancestor height

`gpui_kit`'s `tree()` is a `uniform_list`. The CDDL right pane is therefore `flex_1().min_h(0)`
where its sibling panes are content-sized; without that the pane sizes to its content, the tree's
own `flex_1` resolves against zero leftover space, and the AST section comes up empty with no
error anywhere.

### `use gpui_kit::*` shadows `#[test]`

The glob brings `gpui_macros::test` into scope, where it shadows the built-in attribute and makes
every `#[test]` expand into itself until the recursion limit stops the compiler. A module whose
`super` is the crate root must `use std::prelude::v1::test;`, and a module-level test suite must
import its items one by one rather than `use super::*`.

## Building and testing

From **this directory**:

```bash
CFLAGS=-fno-lto CXXFLAGS=-fno-lto cargo build
cargo test
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo deny check licenses
```

`CFLAGS=-fno-lto CXXFLAGS=-fno-lto` is needed whenever `ring` compiles: its C symbols fail to link
under LTO on current toolchains. The same reason is why the release profile has `lto = false`.

**Never run `cargo fmt --all`**, not even from here — it reaches the whole repository and rewrites
every `.rs` file in it, including crates with CRLF line endings. Plain `cargo fmt` from a crate
directory is the only safe form.

Every test runs with no display and no GPU: the suite is over pure functions — layout arithmetic,
menu enablement, routing, span mapping, the hex grid's geometry and search — plus gpui-kit's
headless harness.

On Linux under X11, including WSL2, run the built binary with `WAYLAND_DISPLAY` unset:

```bash
env -u WAYLAND_DISPLAY ./target/debug/cbor-ide ../examples/gp_eap.cddl
```

`libxkbcommon-x11-dev` must be installed for the X11 path.

## Fixtures

The corpus is the workspace's top-level [`examples/`](../examples) — notably the four-deep include
chain `gp_eap.cddl` → `rfc9711.cddl` → `rfc8392.cddl` → `rfc9052.cddl`, and the
`trace_demo.cddl` / `trace_demo_good.cbor` / `trace_demo_bad.cbor` trio, which is the one
positive/negative pair in it that actually discriminates. `tps_cbor_core/README.md` explains why
`flat_tee_claims_good.cbor` and `..._bad.cbor` are **not** such a pair.

It lived in `tps_cbor_gui/examples/` until that crate was retired.

## Releases

`.github/workflows/tps_cbor_ide-release.yml` builds one unsigned native binary per platform on a
`tps_cbor_ide-v*` tag. **It has never been run**, and Windows and macOS have never been built at
all — the only platform this application has been exercised on is Linux/X11. The workflow's own
header says the same thing.

## Status

Phase 8 of [`PLAN.md`](PLAN.md), complete. The largest known defect is carried forward there:
`Validate` runs on the main thread and freezes the window for about 30 seconds on
`large_uint_array.cbor`.
