# tps_cbor_core

The frontend-agnostic domain layer shared by `tps_cbor_ide` (GPUI) and `tps_cbor_tui`
(ratatui): the document model, CBOR ↔ EDN conversion, the span machinery that links a CBOR byte
range to its rendered EDN text, CDDL validation and tracing, COSE operations, key generation and
the key store.

The point of it is that there is **one** tested copy of each algorithm, so the two frontends
cannot drift apart. It exists because the application it replaces had no such layer: the old
`tps_cbor_gui` Tauri backend held no state at all, so the whole document model lived in React
`useState`, every CBOR buffer crossed the IPC boundary as a JSON array of numbers, and the
`tps_cbor_tui` COSE and key-store code was a second, independent implementation of the same
operations.

Nothing here depends on a UI toolkit — no GPUI, no ratatui, no serde-for-IPC — so every item is
exercised by `cargo test` alone, with no display and no GPU.

Built over `tps_minicbor`, `tps_cddl`, `tps_cose`, `tree-sitter-cddl` and `tree-sitter-edn`.

## Not for embedded targets

Unlike most crates here, this one is deliberately exempt from the workspace's `no_std` and
cross-compilation requirements, on the same basis as `tps_cbor_tui`. It serves developer-facing
interactive tools only, it is filesystem- and clock-bound throughout, and it is
`publish = false`. `std` is required and no cross-compilation guarantees are made.

The libraries it wraps remain `no_std`-capable, and nothing here is needed to use them from an
embedded target.

## Layout

| Module | Responsibility |
|---|---|
| `error` | `CoreError`, the single error type, replacing `Result<_, String>` |
| `diag` | `Diagnostic`, a ranged, file-attributed message |
| `convert` | CBOR ↔ EDN conversion and hex dumping |
| `spans` | `SpanMap`: CBOR byte range → EDN text range, and back |
| `cddl` | Validation, module resolution, the AST view, and the validation tracer |
| `cose` | Sign / verify / encrypt / decrypt, key generation, and recognising a COSE artefact and the stored key behind it |
| `keystore` | On-disk COSE key store |
| `settings` | Persisted settings, as TOML in the platform config directory |
| `doc` | `Workspace` and the per-kind open documents |
| `highlight` | Re-exports of the tree-sitter grammars and their highlight queries |

## Design notes

These are the decisions a reader is most likely to want explained, and the ones most likely to be
undone by accident.

### Diagnostics keep both ends of their span

`Diagnostic` carries a `Range<usize>` of byte offsets, not a point. The old backend kept only
`span.start`, which is why it could draw a marker but never a ranged squiggle. Line and column
are computed on demand and go through `tps_cddl::ast::offset_to_line_col` and **nowhere else** —
the old GUI had a second, char-indexed copy that compared a character index against a byte offset
and misreported every position after the first non-ASCII character in a schema.

### Three coordinate spaces, kept separate

A trace event may carry a span in CDDL source, in the raw CBOR buffer, and in the pretty-printed
EDN document. All three are stored as byte ranges and resolved on demand, so that clicking a
trace row can highlight all three panes at once. The old version pre-converted each into
line/column pairs at trace time, which went stale on the next edit.

### Read-only is enforced by the model

Every mutating method on a document checks the read-only flag and returns `CoreError::ReadOnly`.
`UI_DESIGN.md` requires Paste to be inoperative on a read-only document; a model that enforces it
cannot be bypassed by a keyboard shortcut that forgot to check. A *conversion* (`To EDN`,
`To CBOR`) is permitted on a read-only document, because that is the tool replacing the document
wholesale at the user's explicit request, not the user typing into it.

### Documents own content, not derived state

`doc` holds content, identity, and the dirty and read-only flags. It does not hold diagnostics,
span maps, parsed ASTs or traces. Derived state is invalidated by editing, and a model that
cached it would have to be trusted to invalidate it everywhere; recomputing is cheap enough
(`gp_eap.cddl`, 17 KB, parses in about 1 ms) that caching is not worth the correctness risk.

### Private key material

Private scalars are wrapped in `Zeroizing` and files holding them are written `0o600` on Unix.
Key store aliases pass through `validate_alias` before becoming a filename, on *every* operation
— the old GUI's `load_key` validated nothing, so a `../` alias read arbitrary files. Do not add a
path that bypasses any of the three.

### No `dirs` dependency

`settings::default_config_dir` reads `XDG_CONFIG_HOME`, `HOME` and the Windows equivalents
directly. The `dirs` crate would pull in `option-ext`, which is MPL-2.0 and needs a `deny.toml`
exception. `tps_cbor_ide` acquires `option-ext` through GPUI regardless, but `tps_cbor_tui`
should not have to.

### No `tree-sitter` version constraint

`highlight` names only `tree_sitter_language::LanguageFn`, the ABI-stable function-pointer
newtype the grammar crates expose, and the highlight queries as `&str`. `tree-sitter` itself
appears in neither the grammars' dependencies nor this crate's. That matters because
`tree-sitter` declares `links = "tree-sitter"`, so Cargo permits exactly one version per
resolution: `tps_cbor_ide` is on 0.26 via `gpui-component` and `tps_cbor_tui` is on 0.27, and
they can share this crate only because it constrains neither.

The `queries/highlights.scm` files are re-exported **unmodified**, so the IDE, the TUI and
`tps_ts_highlight_tool` cannot drift in what they highlight.

## Testing

```bash
cargo test -p tps_cbor_core
```

No display, no GPU and no UI stack are required. The fixtures in the workspace's `examples/` are the
integration corpus — notably the four-deep include chain `gp_eap.cddl` → `rfc9711.cddl` →
`rfc8392.cddl` → `rfc9052.cddl`, which the CDDL tests resolve and trace against. It was described
here as three-deep until `cddl::chain` resolved it and counted the regions.

Two behaviours of that corpus are worth knowing before writing a test against it, because both
are surprising and both are asserted here:

- `gp_eap.cddl` declares `;# start rfc8392.Claims-Set`, but it reaches `rfc8392` through
  `;# include`, which merges rules **without** a module qualifier — only `;# import ... as X`
  creates an `X.` prefix. So no rule of that name exists in the merged schema. `cddl::start_rule`
  strips the failed qualifier and reports that it did so, rather than silently validating against
  something the caller did not ask for.
- `flat_tee_claims_good.cbor` and `flat_tee_claims_bad.cbor` do **not** form a positive/negative
  pair at the resolved start rule. `Claims-Set` ends with
  `* Claim-Label .feature "extended-claims-label" => any`, and that arm absorbs any unrecognised
  label without descending into it, so both documents are accepted. Use
  `trace_demo.cddl` with `trace_demo_good.cbor` / `trace_demo_bad.cbor` for a pair that does
  discriminate: a small schema with no catch-all, whose bad document puts a `tstr` where a `uint`
  belongs three containers down. `trace_demo_good.edn` / `_bad.edn` are committed beside them, and
  `cddl::trace`'s tests re-encode those to prove the bytes are what the text says.
- `tps_cddl` reports a `CborMismatch` only at the rule the check started from, so the whole of
  `trace_demo_bad.cbor` being rejected comes back as one mismatch spanning all 53 bytes.
  `Trace::pinpoints` recovers the offending member from the trace, which is why
  `Trace::diagnostics` reports both.

## Status

Phase 1 of `../tps_cbor_ide/PLAN.md`, complete. `tps_cbor_tui` has not yet been migrated onto it.
