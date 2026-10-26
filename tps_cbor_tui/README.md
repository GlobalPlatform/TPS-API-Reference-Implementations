# tps_cbor_tui

Provides `cbor-tui`, a terminal workbench that unifies the CBOR, EDN, CDDL and COSE workflows
of this workspace behind one screen. The point of it is that an artifact can move between those
workflows in a single session: load a CBOR file, read it as EDN, edit it, validate it against a
CDDL schema, sign or encrypt it against a stored COSE key, and save the result, without
round-tripping through files and separate tools at each step.

It does nothing that `edn-tool`, `cddl` and `cose-tool` cannot do between them; it is for the
case where you are iterating and the file-at-a-time shape of those tools gets in the way.

Built on [`ratatui`](https://ratatui.rs) and `crossterm`, over `tps_minicbor`, `tps_cddl`,
`tps_cose` and `tree-sitter-edn`.

## Not for embedded targets

Unlike most crates here, this one is deliberately exempt from the workspace's `no_std` and
cross-compilation requirements. It is a developer-facing interactive tool with no embedded use
case, so it requires `std` and makes no cross-compilation guarantees.

## Usage

```bash
cbor-tui [<file>] [--keystore-dir <dir>]
```

`<file>` is an optional CBOR file to open at startup; with no argument the artifact starts
empty. `--keystore-dir` defaults to `cose_keys` in the current directory, and is created on
demand — note that running `cbor-tui` therefore leaves a `cose_keys/` directory behind unless
you point it somewhere else.

The terminal is restored on exit, including via a panic hook, so an unexpected panic should not
leave your shell in raw mode.

## Screens

`cbor-tui` is modal. It starts in the CBOR view; the status bar at the foot of the screen
always shows the keys available in the current mode, along with the artifact's provenance chain
and the last message.

| Screen | Entered by | Purpose |
|---|---|---|
| CBOR view | startup, `Esc` from most screens | Hex dump of the current artifact |
| EDN editor | `Tab` from the CBOR view | Edit the artifact as EDN, with syntax highlighting |
| Hex editor | `F4` | Edit the artifact's raw bytes as a hex string |
| CDDL editor | `F2` | Edit a CDDL schema, with live diagnostics |
| CDDL check | `F5` | Validate the artifact against the loaded schema |
| Key store | `F3` | List, generate, import and delete COSE keys; choose the active key |
| COSE sign | `s` from the key store | Sign or verify the artifact |
| COSE encrypt | `e` from the key store | Encrypt or decrypt the artifact |

### Key bindings

`Ctrl+L` clears the current message in every mode. In a prompt, `Enter` submits and `Esc`
cancels; an overwrite confirmation takes `y` or `n`.

**CBOR view** — `Tab` decode to EDN and edit · `F2` CDDL editor · `F3` key store · `F4` edit
hex · `F5` check against CDDL · `c` copy hex to clipboard · `Ctrl+O` load · `Ctrl+S` save ·
`↑`/`↓`/`PgUp`/`PgDn` scroll · `q` or `Esc` quit

**EDN and hex editors** — type to edit · `Esc` apply · `↑`/`↓`/`PgUp`/`PgDn` scroll ·
`Ctrl+O` load · `Ctrl+S` save · `F5` check against CDDL (EDN only)

Applying an edit re-parses it. If it does not parse, the editor keeps your text and shows the
error rather than discarding the edit; from that state `Esc` abandons the edit and returns to
the CBOR view, and `q` quits.

**CDDL editor** — type to edit, re-validating as you go · `Tab` focus the diagnostics pane ·
`Esc` back · `Ctrl+O` load · `Ctrl+S` save. With the diagnostics pane focused: `↑`/`↓` select,
`Enter` jump to that diagnostic's position in the source, `Esc` or `Tab` return to the source.

**Key store** — `↑`/`↓` select · `Enter` make the selected key active · `g` generate · `i`
import · `x` delete · `c` copy hex · `s` COSE sign screen · `e` COSE encrypt screen · `Esc`
back

**COSE sign** — `s` sign · `v` verify · `Esc` back
**COSE encrypt** — `e` encrypt · `d` decrypt · `Esc` back
**CDDL check** — `↑`/`↓` select a mismatch · `Esc` back

The COSE screens act on whichever key is active, so selecting a key with `Enter` in the key
store is a prerequisite for them.

## Key store

The key store is a flat directory, readable and editable without this tool:

```
<keystore-dir>/index.json        metadata: alias, key type, algorithm, curve,
                                 whether a private part is present, creation time
<keystore-dir>/<alias>.cbor      the key itself, as CoseKey::encode() bytes
```

Aliases must be non-empty and contain only ASCII letters, digits, `-` or `_`; they are
validated before being used as a filename, so an alias cannot escape the key store directory.
Importing or generating over an existing alias is a hard error rather than a silent overwrite.

`g` generates symmetric keys only. EC2 and OKP key generation is not implemented, so an
asymmetric key must be imported with `i` — for example one of the fixtures in
`tps_cose_tool/testdata/`, or any file `cose-tool key info` can read.

The key store holds private key material in plain CBOR files with no passphrase or encryption
at rest. It is intended for development keys and published test vectors; do not put a key you
care about in it.

## Implementation notes

`PLAN.md` records the four phases the crate was built in (app shell and viewer, CDDL editor,
COSE plus key store, cross-workflow artifact movement) and what was deliberately left out.

Two small duplications are intentional. The capture-name to style table in `src/highlight/` is
a deliberate copy of `tps_ts_highlight_tool`'s ANSI equivalent rather than a shared dependency,
and `src/cose_ops.rs` reuses `tps_cose_tool`'s exact builder call sequences — including its
buffer-sizing margins and nonce handling — adapted to operate on in-memory buffers instead of
files.

## License

`tps_cbor_tui` is MIT licensed.
