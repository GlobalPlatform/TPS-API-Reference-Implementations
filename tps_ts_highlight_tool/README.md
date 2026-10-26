# tps_ts_highlight_tool

Provides `ts-highlight`, a terminal syntax-highlighting preview for this workspace's
`tree-sitter-cddl` and `tree-sitter-edn` grammars. It parses a file with the appropriate
grammar, runs the grammar's own `queries/highlights.scm` through `tree-sitter-highlight`, and
writes the source back out with ANSI colour.

Its purpose is to exercise and eyeball those two grammars — if a construct comes out the wrong
colour, the grammar or its highlight query is wrong. It is a development aid, not a pager or a
general-purpose `cat` replacement: there is no paging, no line numbering, and no `NO_COLOR` or
non-TTY detection, so redirecting the output to a file captures the escape sequences.

## What is provided

- Binary
  - `ts-highlight <file>` — highlight a file, inferring the grammar from its extension.
  - `--lang cddl|edn` — force the grammar instead of inferring it.
  - `-V`/`--version`, `-h`/`--help`.

Extensions are mapped as `.cddl` → CDDL and `.edn`/`.diag` → EDN. Any other extension is an
error telling you to pass `--lang`, so `--lang` is what you need for a file with an unusual or
absent extension.

This crate has no library target. The capture-name to ANSI colour table lives in
`src/main.rs`; the equivalent capture-name to `ratatui::style::Style` table used by `cbor-tui`
is a deliberate duplicate of it rather than a shared dependency.

## Usage

```bash
# Grammar inferred from the extension
ts-highlight schema.cddl

# Forced, for a file whose extension says nothing useful
ts-highlight item.txt --lang edn
```

## License

`tps_ts_highlight_tool` is MIT licensed.
