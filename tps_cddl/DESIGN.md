# cddl Design

## Overview

The `cddl` crate implements the Concise Data Definition Language (RFC 8610) with
optional extensions from RFC 9165 and RFC 9682. It provides:

1. A parser that produces a typed AST.
2. A semantic validator.
3. CBOR value checker (validates binary CBOR against a schema).
4. JSON value checker (validates JSON against a schema).
5. A Rust code generator for `tps_minicbor`-based decoders.
6. A Rust code generator for `tps_minicbor`-based encoders.
7. A public library interface.
8. A Slint GUI application (`cddl_gui`).

The crate targets host systems and uses the Rust standard library throughout.

---

## Parser pipeline

```
Source text
    │
    ▼
lexer::lex()          — PEG parsing via `pest`, produces Pairs<Rule>
    │
    ▼
parser::build_cddl()  — AST construction, produces Cddl
    │
    ▼
validator::validate() — semantic checks, produces Vec<Diagnostic>
```

### Grammar

The grammar is defined in `src/grammar.pest` using the
[pest](https://pest.rs) PEG parser generator. The grammar closely follows
the ABNF in RFC 8610, with additions for RFC 9165 control operators and
RFC 9682 non-literal head numbers. `pest` produces a `Pairs<Rule>` iterator
which the builder consumes in a single pass.

### Lexer

`lexer::lex(source)` is a thin wrapper: it calls `pest::Parser::parse()` and
returns `Pairs<Rule>`, or a `ParseError` describing the location and expected
tokens. The `pest` error is converted to `ParseError` to decouple callers from
the `pest` API.

### AST

The AST is defined in `src/ast.rs`. Every node carries a `Span` (start/end
byte offsets into the original source) to support accurate diagnostics.

Key types:

| Type | Description |
|---|---|
| `Cddl` | Document root; a `Vec<Rule>` |
| `Rule` | Top-level definition: name, assignment (`=`/`/=`/`//=`), body |
| `RuleBody` | `Type(Type)` or `Group(GrpEnt)` |
| `Type` | Ordered choice: `Vec<Type1>` separated by `/` |
| `Type1` | Base type (`Type2`) plus optional range/control constraint |
| `Type2` | Leaf: literal, typename, map, array, tag, major type, `#` (any) |
| `Group` | One or more `GrpChoice`s separated by `//` |
| `GrpEnt` | Optional occurrence, optional member key, and body |
| `Value` | Literal: integer, float, text, or byte string |

### Parser

`parser::build_cddl(pairs)` walks the `Pairs<Rule>` tree recursively,
constructing the AST bottom-up. Group entries without an explicit member key
that reference a named group are represented as `GrpEntBody::GroupRef` (or
`GrpEntBody::Type` with `Type2::Typename` when parsed through the `type_`
rule), so the map-checker can detect and expand group inclusions.

---

## Semantic validator

`validator::validate_with_config(cddl, config)` checks the AST for:

1. **Undefined references** — every `Typename` or `GroupRef` must resolve to
   a rule or a prelude name.
2. **Duplicate definitions** — a plain `=` rule may not redefine an existing rule.
3. **Unknown control operators** — `.size`, `.bits`, `.regexp`, `.lt/.le/.gt/.ge`
   are always accepted; RFC 9165 operators (`.plus`, `.cat`, `.det`, `.abnf`,
   `.abnfb`, `.feature`) require `config.rfc9165 = true`.
4. **Non-literal head numbers** — `#6.<type>` requires `config.rfc9682 = true`.
5. **Empty documents** — an empty CDDL file requires `config.rfc9682 = true`.

The RFC 8610 standard prelude (Appendix D) is embedded as a static string and
always merged into the rule namespace before validation, so names like `tstr`,
`uint`, `bool`, and `cbor-any` are always available.

`Diagnostic` carries a `Level` (`Error` or `Warning`), a `message: String`, and
an optional source `Span`.

---

## CBOR checker

`cbor_check::check_cbor(bytes, schema, root, config)` validates a CBOR byte
slice against a CDDL schema.  Internally it:

1. Decodes the CBOR using `tps_minicbor::Decoder`, producing an `Item` tree.
2. Recursively matches each `Item` against the resolved CDDL type.

Matching logic:

- **Maps** — flattens group inclusions via `ast::flatten_map_entries()`, then
  checks that every required key is present and all values match their declared
  types. Extra keys in the CBOR map are currently ignored (open-world matching).
- **Arrays** — iterates CBOR items against the group entries, respecting
  occurrence constraints (`?`, `+`, `n*m`).
- **Choice types** — tries each alternative and accepts on the first match.
- **Tagged items** — checks the tag number and recursively matches the inner value.
- **Prelude types** — `tstr`, `uint`, `bstr`, etc., match the corresponding
  CBOR major type / simple value.

Mismatches are accumulated as `CborMismatch { path, expected, got }` values;
the checker always continues after a mismatch rather than short-circuiting, so
all errors are reported at once.

---

## JSON checker

`json_check::check_json(bytes, schema, root, config)` validates a JSON file
(parsed via `serde_json`) against a CDDL schema. The CDDL → JSON mapping
follows RFC 8949 §6 and the informal CDDL-for-JSON convention:

| CDDL type | JSON type |
|---|---|
| `tstr` | string |
| `uint`, `int`, `float` | number |
| `bool`, `true`, `false` | boolean |
| `null`, `nil` | null |
| `{…}` | object |
| `[…]` | array |

Group inclusions in map schemas are flattened with the same
`ast::flatten_map_entries()` helper used by the CBOR checker, sharing the
expansion logic between both checkers.

---

## Code generation

Both generators share the `gen_common` module which provides:

- `classify(rule)` — categorises a rule as Struct, Enum, Array, or Scalar.
- `collect_ordered(cddl, root)` — topological sort of rules needed by `root`;
  prelude names are excluded.
- `to_pascal_case` / `sanitise_field_name` — identifier normalisation.
- `type2_to_rust_type` — maps a CDDL type to a Rust type string.

### Decoder generator

`gen_decoder::gen_decoder(cddl, root, config)` emits Rust source that depends
on `tps_minicbor`. For each rule it emits:

- **Struct** (`{ key: type, … }`) — a Rust struct with a `decode` method that
  calls `Decoder::next_map()` and reads each field.
- **Array struct** (`[field, …]`) — reads items sequentially from a CBOR array.
- **Enum** (`type1 / type2 / …`) — a Rust enum with `TryFrom<tps_minicbor::Item>`.
- **Scalar** — a type alias or wrapper implementing `TryFrom<tps_minicbor::Item>`.

### Encoder generator

`gen_encoder::gen_encoder(cddl, root, config)` emits Rust source with an
`encode` method for each type. For structs it calls `Encoder::map()` and inserts
each field; for array structs it calls `Encoder::array()`.

---

## Tokenisation for syntax highlighting

`tokenize::tokenize(source)` runs the PEG grammar and maps each `Pair` to a
`TokenKind`:

- `RuleName` — identifiers on rule left-hand sides and typename references.
- `Literal` — numeric, text, and byte string literals.
- `Operator` — `=`, `/=`, `//=`, `..`, `...`, `.name`, `?`, `+`, `*`.
- `Comment` — from `;` to end of line.
- `Default` — structural punctuation and whitespace.

Source regions not covered by any grammar `Pair` are filled with `Default`
spans, ensuring the returned token list covers the entire source without gaps.

---

## RFC extension flags

The `Config` struct controls which extensions are active at runtime:

| Flag | Controls |
|---|---|
| `config.rfc9165` | Accept `.plus`, `.cat`, `.det`, `.abnf`, `.abnfb`, `.feature` without warning |
| `config.rfc9682` | Accept non-literal tag head-numbers, empty documents |

Both can be set via `Config::all()`, or individually. At the CLI layer,
`--rfc9165` and `--rfc9682` flags map to these fields.

---

## GUI crate (`cddl_gui`)

`cddl_gui` is a separate crate in the workspace, licensed under GPL-3.0 (Slint
requirement). It depends on `cddl` and wraps it with a Slint desktop UI. The GUI
crate is host-only and uses `std`. See `cddl_gui/` for its own structure.

---

## CDDL modules (`modules` feature)

`src/modules.rs`, gated behind the `modules` Cargo feature, implements
`;# import` / `;# include` directives from
`draft-ietf-cbor-cddl-modules-06.txt` (vendored in the crate).

### Preprocess-before-validate architecture

Module resolution is a pass that runs entirely *before* the existing,
unmodified pipeline:

```
top-level file path
    │
    ▼
modules::resolve_modules(path, search_path)
    │  recursively: read file → scan ";#" directives → lex/build_cddl its own
    │  rules → recursively resolve each directive's target module →
    │  select/rename/merge its rules into the accumulated Cddl
    ▼
ModuleResolution { source, cddl, warnings }
    │
    ▼
validate_with_config / check_cbor / check_json / gen_decoder / gen_encoder
```

`resolve_modules` produces one flat, merged `Cddl` AST plus the concatenated
source it was parsed against, and hands that to the same
`validate_with_config`/`check_cbor`/`check_json`/`gen_decoder`/`gen_encoder`
entry points used for a single, module-free file. None of those five
functions have any awareness that their input may have come from more than
one file.

### Virtual concatenated source and span rebasing

Every module's raw text is appended to one growing `virtual_source: String`
as it is loaded. A rule's (or diagnostic's) `Span` is a byte range into
whichever single file it was originally parsed from; when that rule is
merged into the accumulated AST, its span is rebased by adding the offset at
which that file's text was appended to `virtual_source`. No changes to
`ast::Span` or `Diagnostic` were needed to support this.

Caveat: a diagnostic whose span originates in an included module renders
against the concatenated blob under the top-level file's name — the
line/column miette computes is relative to the *virtual* source, not the
module file's own text on disk. This is a rendering wart, not a correctness
problem (the byte range is still exact), and is left as future work rather
than plumbing a second, per-span source identity through `Diagnostic`.

### Search-path precedence

`ModuleSearchPath::from_env_and_cwd` builds the search order:

1. Current working directory (implicit).
2. `--include-path` CLI flag directories, in the order given (repeatable).
3. `CDDL_INCLUDE_PATH` environment variable, split on the platform-native
   path-list separator (`std::env::split_paths`), in listed order.

Within a single directory, a case-insensitive match on `{module}.cddl` is
required to be unique — more than one matching entry is a hard
`ModuleError::Ambiguous` with no fallthrough to later directories; zero
matches tries the next directory; exhausting every directory is
`ModuleError::NotFound`, which lists every directory searched.

### ABNF fidelity: hyphens excluded from directive-clause identifiers

The `from`/`as` clause grammar (`draft-ietf-cbor-cddl-modules-06.txt`,
Appendix A) defines `id` as `($/ALPHA/"_") *(ALPHA/DIGIT/"_"/"$")` — it does
not include `-`. This is implemented literally: a hyphenated rule name (e.g.
the RFC 8610 prelude's `encoded-cbor`) cannot appear in a directive's `from`
or `as` clause. This is very likely an oversight in the draft rather than an
intentional restriction, but `scan_directives` follows the published ABNF
exactly rather than a more permissive superset, so behavior tracks the
specification as written and stays easy to reconcile if a future draft
revision fixes it. A regression test pins this down (`src/modules.rs`).

### Single-pass rename/rebase visitor

`transform_rule` applies both the span-rebase and the (optional)
namespace-prefix rename in one combined AST walk, rather than as two
separate passes. Keeping "every place a name can be referenced" enumerated
in exactly one function (rather than duplicated across a renamer and a
rebaser) was judged more maintainable than the marginal efficiency of
skipping the rebase step when no rename is needed. `MemberKey::Bareword` is
deliberately *not* visited — a bareword map key is a literal text string,
never a rule reference — while `Type2::Typename`, `Type2::Unwrap`,
`Type2::EnumRef`, `GrpEntBody::GroupRef`, and `MemberKey::Type` all are, since
each can hold a name that must follow its rule into the new namespace.

### Free collision detection

No new duplicate-name-detection code was written for merged rules. Every
rule pulled in by a directive — after selection and any prefix-rename — is
appended to the importer's `Cddl.rules` as an ordinary `Rule { assignment:
Assignment::Assign, .. }`. The existing duplicate-`=` check in
`validator.rs`'s `ValidatorCtx::collect_definitions` therefore catches every
post-merge collision automatically: two modules imported under the same
prefix that both define the same name, or a bare-name import alias
(`§2.6`'s single-name special case) colliding with a pre-existing top-level
rule. This is verified by test (`tests/modules.rs`), not by new logic —
the validator has no idea the merged AST came from more than one file.

