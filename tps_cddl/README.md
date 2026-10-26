# tps_cddl

Tooling to parse, validate, and generate code from CDDL
([RFC 8610](https://www.rfc-editor.org/rfc/rfc8610.txt)), with optional
support for the RFC 9165 and RFC 9682 extensions and the CDDL modules draft
(`draft-ietf-cbor-cddl-modules-06`'s `;# import`/`;# include` directives).

## License

`tps_cddl` is MIT licensed. See LICENSE.

## What is provided

- Binary
  - `cddl`, a command line tool with subcommands:
    - `validate` — parse and validate a CDDL file, optionally dumping the AST
      or listing `.feature` (RFC 9165 §4) usages.
    - `check-cbor` — validate a CBOR file (binary or hex) against a CDDL
      schema/rule.
    - `check-json` — validate a JSON file against a CDDL schema/rule.
    - `gen-decoder` / `gen-encoder` — generate a `tps_minicbor`-based CBOR
      decoder/encoder for a CDDL schema, optimized for code size.
  - `--rfc9165`, `--rfc9682`, `--all-extensions`, and `--include-path` flags
    are available globally across subcommands.
- Library
  - A complete parser for RFC 8610 CDDL, including generics, with optional
    RFC 9165/RFC 9682 extensions gated behind the `rfc9165`/`rfc9682`/
    `all_extensions` Cargo features.
  - An Abstract Syntax Tree type definition representing the output from
    parsing a CDDL file.
  - Span-accurate error reporting via `miette`-backed diagnostics.
  - CBOR and JSON validators, and `tps_minicbor` decoder/encoder generators.
  - Optional CDDL module resolution (`modules` feature): `;# import`/
    `;# include` directives across multiple files.
- Test cases
  - CDDL files extracted from RFC 8610 and others, exercised by the crate's
    integration tests.
