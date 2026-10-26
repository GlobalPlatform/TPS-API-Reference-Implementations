# CDDL stress test: real-world CDDL + individual-construct fixtures

`CDDL_TOOL_COMPARISON.md` ran five tools against `tps_cddl`'s own small
test fixtures. This document goes further in two directions:

1. **Real-world CDDL** — the four non-trivial example files under
   `tps_cbor_gui/examples/` (`rfc9052.cddl` (COSE), `rfc8392.cddl` (CWT),
   `rfc9711.cddl` (EAT), `gp_eap.cddl` (GlobalPlatform TEE/SE attestation
   profile)), each hundreds of lines, using modules
   (`;# include`/`;# start`), generics, sockets/plugs, and every RFC 8610/
   9165/9682 control operator this project actually needs.
2. **A hand-built fixture corpus** — 14 minimal `.cddl` schemas, each
   isolating exactly one RFC 8610 §2/3 construct (member-key cuts, group
   enums, ranges with literal/named/exclusive bounds, `.size`, occurrence
   bounds, tags, `bstr .cbor` embedding, RFC 8610 §3.10 generics, RFC 8610
   §3.9 sockets), each paired with valid and invalid CBOR instances, so
   every tool is exercised on the same narrow, unambiguous case.

## Tools compared

Same five as `CDDL_TOOL_COMPARISON.md`; invocations repeated here because
this document also records a root-rule-selection footgun (below) that
changes how the corpus had to be authored.

| Tool | Invocation | Version |
|---|---|---|
| `tps_cddl` (this crate) | `target/release/cddl check-cbor <cddl> <cbor> --rule <name>` / `validate <cddl>` | workspace HEAD |
| Rust `cddl` crate (anweiss/cddl) | `~/.cargo/bin/cddl --ci validate --cddl <cddl> --cbor <cbor>` / `compile-cddl --cddl <cddl>` | 0.10.6 |
| Ruby `cddl` gem | `ruby .../cddl-0.12.6/bin/cddl <cddl> validate <cbor>` / `<cddl> generate 1` | 0.12.6 |
| Python `cddl_verifier` (sahebbiswas/cddl_verifier) | `python3 cbor_cddl_analyzer.py <cddl> <cbor> --type <name>` | HEAD (cloned for this comparison) |
| `@cbortech/cbor` CLI | `npx cbor validate --cddl <cddl> --cddl-rule <name> --type cbor <cbor>` | 0.26.8 |

## A bug this stress test found in `tps_cddl` itself (fixed before the rest of this doc)

Building the RFC 8610 §2.2.2.1 fixture (`byte1 = 0...first-non-byte` with
`first-non-byte = 256`, which the RFC states is "equivalent to
`byte = 0..255`") surfaced a real correctness bug: `tps_cddl`'s range/size
bound resolver (`type2_as_f64` in both `cbor_check.rs` and `json_check.rs`)
only resolved *literal* bounds (`Type2::Value`); a *named* bound
(`Type2::Typename`, i.e. a rule reference) silently fell through to `None`,
which disabled the range check entirely. `first-non-byte`, `min-byte`, and
every other bound written as a name rather than a literal number went
completely unchecked.

**Fixed**: added `MatchCtx::resolve_type2_as_f64`, which resolves a
`Typename` bound by looking up the rule (reusing the existing
`find_rules` dynamic lookup) and recursing into simple, single-choice,
unconstrained alias bodies, bounded by `MAX_MATCH_DEPTH` against reference
cycles. Eight regression tests were added (four in `cbor_check.rs`, four
in `json_check.rs`) covering the exact RFC 8610 example, a named lower
bound, and a multi-hop alias chain. Full existing test suite passes with
no regressions. See `git log` for the commit; not repeated here since the
code is the source of truth.

This is why `range_named_bound.cddl`/`range_named_lower.cddl` exist as
their own fixtures below — they're the regression corpus for this fix, not
just a divergence-finding exercise.

## Divergence 7: Rust `cddl` crate's undefined-reference blind spot holds on real-world CDDL

`CDDL_TOOL_COMPARISON.md` divergence #2 found this on a 3-line minimal
fixture. Re-confirmed here on the flattened `rfc9711_flat.cddl` (RFC 9711
Appendix D's own CDDL, concatenated with its RFC 9052/8392 dependencies —
392 lines): `cddl compile-cddl --cddl rfc9711_flat.cddl` reports
**conformant** (exit 0) despite two genuine undefined references in the
RFC's own text: `general-oid = JC< json-oid, ~oid >` (`oid` is not an
RFC 8610 prelude name — `tps_cddl` and `@cbortech/cbor` both correctly
flag it) and `$manifest-body-cbor /= bytes .cbor untagged-coswid`
(×2, `untagged-coswid` is simply never defined anywhere in the RFC's
appendix). Same result on `gp_eap_flat.cddl`. This is not a fixture-scale
curiosity — it's the same blind spot at real-world scale, and it means the
Rust crate cannot be used as a schema linter for catching authoring
mistakes in specs of this size.

## Divergence 8: Ruby `cddl` gem cannot parse literal tab characters

`gp_eap.cddl`'s `gp_tee_options_type` rule uses a literal tab character for
indentation inside a map body:
```
gp_tee_options_type = {
\t* tstr => uint
}
```
`ruby cddl gp_eap_flat.cddl generate 1` fails with a raw parser-state dump
pointing at that line, rather than a diagnostic. Isolated with a 3-line
minimal repro (tab byte vs. 4-space indent vs. CRLF line endings as
controls): **only** the literal tab character triggers the failure —
4-space indentation and CRLF line endings both parse fine. `tps_cddl` and
`@cbortech/cbor` both parse the tab-indented original without complaint
(cbortech with a different caveat, below).

**Spec basis**: RFC 8610's ABNF (Appendix B) inherits `WSP` from
RFC 5234 §B.1, which is `WSP = SP / HTAB` — a literal horizontal tab is
explicitly valid CDDL whitespace. This is a genuine Ruby gem lexer defect,
not a spec-conformance judgment call; a schema that is valid per the RFC's
own grammar fails to parse cleanly. If any downstream tooling on this
project ever depends on the Ruby gem against author-supplied CDDL (as
opposed to `tps_cddl`-generated/normalized CDDL), tabs in source files are
a landmine.

`@cbortech/cbor` reaches the same line in `gp_eap_flat.cddl` and reports
it explicitly: `CDDL parse error at line 462, column 1: horizontal tab is
not valid whitespace in CDDL (RFC 8610 ABNF allows only space and
newline)` — cbortech is *also* non-conformant here (RFC 5234 permits
tabs), just with an honest, RFC-citing error message instead of a crash.
So this is a genuine three-way split: `tps_cddl` correctly accepts tabs;
the Ruby gem crashes on them without diagnosing why; `@cbortech/cbor`
explicitly (and, per RFC 5234, incorrectly) rejects them.

## Divergence 9: root-rule selection is a footgun for tools with no `--rule`/`--type` override

`tps_cddl` (`--rule`), `@cbortech/cbor` (`--cddl-rule`), and the Python
tool (`--type`) all let you name which rule to validate against. The Rust
`cddl` crate's `validate` subcommand and the Ruby gem's `validate`
subcommand have **no such flag** — both silently validate against
whichever rule is written *first* in the file. Every fixture in this
corpus originally defined a helper alias (`uuid = ...`, `JC<J,C> = ...`,
`max-byte = 255`, etc.) before the type actually meant to be tested
(`top = ...`), because forward references are legal CDDL and that's the
natural authoring order. Under that ordering, the Rust crate and Ruby gem
were silently validating CBOR data against the *helper* rule, not `top` —
producing results that looked like real divergences but were actually
just testing the wrong rule.

**Fix applied to this corpus**: every fixture `.cddl` file was rewritten
so `top` is the first rule (`fixtures_reordered/`), with helper aliases
following. This is spec-legal (RFC 8610 doesn't require definition-before-
use) and makes all five tools agree on what's being validated. Any future
fixture added to this kind of corpus should put the rule under test first,
specifically because two of five tools in real-world use have no way to
override it. All results below use `fixtures_reordered/`.

## Fixture corpus: results by construct

Columns read `tps_cddl / Rust cddl / Ruby gem / Python cddl_verifier /
@cbortech/cbor`, each cell `ok` (validated successfully) or `reject`
(validation failed) — the tool's actual, verified behavior against
`fixtures_reordered/`. The `want` column is what RFC 8610 (+9165 where
relevant) mandates. **Bold** marks a cell that disagrees with `want`.
*Italic* marks a cell that matches `want` only by coincidence, not because
the tool correctly evaluated the construct (explained below the table).

| Construct (RFC 8610 §) | Fixture | want | tps_cddl | Rust `cddl` | Ruby gem | Python | cbortech |
|---|---|---|---|---|---|---|---|
| Member-key cut `&(name: N)` (§3.5.4) | ok | ok | ok | ok | ok | ok | ok |
| Member-key cut, bad `.size` | bad | reject | reject | reject | reject | reject | reject |
| Group-choice int enum `&(...)` (§3.5.1) | ok | ok | ok | ok | ok | **reject** | ok |
| Group-choice int enum, out-of-set value | bad | reject | reject | reject | reject | reject | reject |
| Inclusive range `2..127` (§3.8.1) | ok (upper bound) | ok | ok | ok | ok | **reject** | ok |
| Inclusive range, one past upper bound | bad | reject | reject | reject | reject | reject | reject |
| `.size` exact count (§3.8.1) | ok | ok | ok | ok | ok | **reject** | ok |
| `.size` exact count, wrong length | bad | reject | reject | reject | reject | reject | reject |
| Occurrence bounds `2*4` (§3.3) | ok (3 items) | ok | ok | ok | ok | ok | ok |
| Occurrence bounds, too many (5) | bad | reject | reject | reject | **ok** | reject |
| Occurrence bounds, too few (1) | bad | reject | reject | reject | **ok** | reject |
| Tag match `#6.1(int)` (§3.6) | ok | ok | ok | ok | ok | **reject** | ok |
| Tag match, wrong tag number | bad | reject | reject | reject | reject | reject | reject |
| Tag match, right tag/wrong content | bad | reject | reject | reject | reject | reject | reject |
| `bstr .cbor <type>` embedding (§3.8.4) | ok | ok | ok | ok | ok | **reject** | ok |
| `bstr .cbor`, empty alt (`.size 0`) | ok | ok | ok | ok | ok | **reject** | ok |
| `bstr .cbor`, embedded bytes aren't CBOR | bad | reject | reject | reject | reject | reject | reject |
| Generic `JC<J,C>` w/ `.feature` (§3.10, RFC 9165 §3.6) | ok (`"iss"` under no feature → falls to `.feature "cbor"` branch, matches `1`) | ok | ok | ok | **reject** | *reject* |
| Generic `JC<J,C>`, content matches neither branch | bad | reject | **ok** | reject | *reject* | *reject* |
| Generic + nested `.size` range | ok (12 bytes, in 7..33) | ok | ok | ok | **reject** | *reject* |
| Generic + nested `.size` range, below range (6 bytes) | bad | reject | **ok** | reject | *reject* | *reject* |
| Exclusive range `0...first-non-byte`, named+literal chain (§2.2.2.1) | ok (255) | ok | ok | ok | ok | **reject** | ok |
| Exclusive range, at excluded bound (256) | bad | reject | reject | reject | reject | reject | reject |
| Exclusive range, inline literal bound `0...256` | ok (255) | ok | ok | ok | ok | **reject** | ok |
| Exclusive range, inline literal, at excluded bound | bad | reject | reject | reject | reject | reject | reject |
| Named lower bound `min-byte..255`, below lower bound | bad | reject | reject | reject | reject | reject | reject |
| Socket/plug `$$name //= (...)` (§3.9) | ok | ok | ok | ok | ok | **reject** | ok |
| Socket/plug, value not matching any plugged alternative | bad | reject | reject | reject | reject | reject | reject |

`tps_cddl`, the Rust crate, and the Ruby gem agree with `want` on every
row except one Rust-crate case (below). The remaining two columns each
have one systematic failure mode rather than scattered misses:

- **Python** rejects every "ok" fixture whose root rule isn't itself a
  bare map/array/simple-alias — 10 of the 15 "ok" rows — regardless of
  whether the CBOR data is actually valid (Divergence 10 below). It also
  *accepts* both occurrence-bound "too many"/"too few" bad fixtures
  (Divergence 11).
- **cbortech** rejects both `.feature`-gated "ok" fixtures
  (`nested_generic_size_ok`, `generics_jc_cbor_ok`) because its CLI never
  exposes a `--features` flag at all, so it always validates with zero
  features enabled (Divergence 13, a distinct root cause from Rust's bug
  below). Its *reject* on the two `.feature`-gated "bad" fixtures is
  italicized, not a clean pass: it rejects those unconditionally
  regardless of content, so the matching exit code isn't evidence it
  evaluated the `.feature` semantics. Python's *reject* on those same two
  "bad" rows is the same situation for the same reason — its
  root-concreteness check (Divergence 10) fires before `.feature` is ever
  reached.
- **Rust `cddl`** is otherwise clean, but on the two `.feature`-gated
  "bad" fixtures (content matching neither branch) it **accepts** — the
  one place in this whole corpus where Rust disagrees with `want`
  (Divergence 12).

## Divergence 10: Python `cddl_verifier` requires the root type to be a "concrete" map/array/alias — group-choices, bare `.size`/tag/`.cbor`/generic expressions as a root type are rejected regardless of data validity

Every fixture whose `top` rule is *not* itself a map or array (i.e. `top =
&(...)`, `top = bstr .size 3`, `top = #6.1(int)`, `top = bstr .cbor X /
bstr .size 0`, `top = 0...first-non-byte`, `top = JC<...>`, `top =
tcp-header` where `tcp-header` is a map alias one level removed) fails in
the Python tool with `Type 'top' not found` or `Type 'top' resolves to
'<expr>' which is not a concrete type definition` — **even for CBOR data
that correctly matches the type**. This isn't a parsing gap
(`--show-types` shows the tool did register the rule); it's the
resolver's "is this a validatable root" check being too narrow, requiring
a small set of top-level shapes. Every one of these constructs is legal
RFC 8610 (§3.5.1 group choices, §3.8.1 `.size`, §3.6 tags, §3.8.4
`bstr .cbor`, §3.10 generics are all ordinary `type2`/`type1` productions,
valid anywhere a type is valid, including as a whole rule's body) — so
this is a real functional gap in the Python tool for schemas that alias a
bare control-operator expression, tag, or generic at the top level, which
is common in the real-world files here (`time-int = #6.1(int)` in
RFC 9711, `empty_or_serialized_map = bstr .cbor header_map / bstr .size 0`
in RFC 9052, every `JC<...>`-based claim label in RFC 9711).

## Divergence 11: Python `cddl_verifier` doesn't enforce occurrence-count bounds on arrays

`occur_bounds.cddl` (`top = [ 2*4 uint ]`) — the Python tool accepts a
5-item array (should reject, exceeds max 4) and a 1-item array (should
reject, below min 2) as valid, both with `[OK] Validation successful`. The
1-item case additionally logs `Unknown element type: 2*4`, suggesting the
occurrence-count portion of the array-entry grammar isn't parsed into a
bound at all — the tool validates that array elements are `uint`-typed but
never checks how many of them there are. This is a real correctness gap
per RFC 8610 §3.3 (occurrence indicators are core, mandatory grammar, not
an extension), not a design choice.

## Divergence 12: Rust `cddl` crate's `.feature`/RFC 9165 support does not validate content in the reject direction, with or without `--features`

`generics_jc.cddl` (`top = iss-claim-label`, `iss-claim-label =
JC<"iss", 1>`, `JC<J,C> = J .feature "json" / C .feature "cbor"`) against
CBOR text string `"foo"` — a value that is neither `"iss"` (the JSON
branch's expected literal) nor `1` (the CBOR branch's expected literal) —
**validates successfully** with the Rust crate, both with no `--features`
flag and with `--features json,cbor` explicitly passed. Tracing this into
the crate's own source (`src/validator/cbor.rs`, `ControlOperator::FEATURE`
arm): without `--features`, `self.state.enabled_features` is `None` and
the entire `if let Some(ef) = ...` branch is skipped — no validation of
either side occurs at all, and the choice silently succeeds. This
reproduces even with `--features` supplying both `json` and `cbor`: the
crate's own test suite (`validate_feature`, in the same file) only
exercises a single-feature enable (`Some(&["cbor"])`) against data that
happens to match that one branch's literal — it does not test the reject
case this fixture targets (content matching *neither* enabled branch).
`nested_generic_size.cddl` shows the same gap concretely: a 6-byte string
(below the CBOR branch's `.size (7..33)` lower bound) also validates
successfully. Every other tool in this comparison correctly rejects both
cases (though Python and cbortech do so for unrelated reasons — see the
table notes above). This is a real, actionable gap: the Rust crate's
RFC 9165 `.feature` implementation does not perform semantic validation
at all in the reject direction, independent of whether the governing
feature is enabled.

## Divergence 13: `@cbortech/cbor`'s CLI has no way to enable RFC 9165 features, so `.feature`-gated content always fails, even when valid

`@cbortech/cbor`'s underlying schema engine supports a `features` option
(confirmed via its bundled source, `dist/schema-*.js`: `new
Set(n?.features ?? [])`, defaulting to empty), but its CLI (`cbor
validate --help`) exposes no flag to set it — there is no `--features`
option alongside `--cddl`/`--cddl-rule`. As a result, every `.feature`
controller is evaluated against an empty enabled-set and unconditionally
fails with `cddl violation: feature "cbor" is not enabled`, regardless of
whether the underlying value would otherwise match. This causes
`nested_generic_size_ok.cbor` and `generics_jc_cbor_ok.cbor` — both valid
per RFC 8610/9165 — to be rejected. Unlike Divergence 12 (Rust's gap only
shows up in the reject direction), this is unconditional in both
directions: cbortech's CLI simply cannot be used to validate any schema
that uses `.feature` at all, since there's no way to opt in to the
features being tested.

## Divergence 14: `.default` (RFC 8610 §3.8.6) — the implied `.ne` is unenforced by Rust `cddl`, the Ruby gem, and Python `cddl_verifier`

RFC 8610 §3.8.6 defines `.default` as implying a `.ne` against the default
value: a conformant sender omits the key rather than sending it explicitly
equal to the default, so a present value that equals the default is a
violation. The RFC's own worked example was used verbatim as the fixture
(key renamed `time`→`moment` to sidestep the unrelated bareword/prelude-name
bug in Divergence 15 below, so this table isolates `.default` specifically):

```cddl
top = timer
timer = {
  moment: uint,
  ? displayed-step: (number .gt 0) .default 1
}
```

| case                                   | tps_cddl | Rust `cddl` | Ruby gem | Python `cddl_verifier` | cbortech |
|-----------------------------------------|----------|-------------|----------|--------------------------|----------|
| key omitted (ok)                        | pass     | pass        | pass     | pass                     | pass     |
| present, non-default, satisfies `.gt 0` (ok) | pass | pass        | pass     | pass                     | pass     |
| present, **equals default** — should be rejected | **reject** | *accept* | *accept* | *accept*                 | **reject** |
| present, fails `.gt 0` — should be rejected      | **reject** | **reject** | **reject** | *accept*               | **reject** |

Only `tps_cddl` and `@cbortech/cbor` implement §3.8.6 correctly. The other
three all accept a value explicitly sent equal to its declared default,
which RFC 8610 says must not happen:

- **Ruby `cddl` gem**: a confirmed, self-documented no-op. Every run against
  a schema containing `.default` prints `*** Ignoring .default for now.` to
  the console before validating — the gem does not implement the control at
  all, so it falls through to validating only the base type (`number .gt
  0`), which a present-and-equal-to-default value still satisfies.
- **Rust `cddl` 0.10.6**: implements `.default` (`src/validator/cbor.rs`,
  `ControlOperator::DEFAULT` arm) but not as an independent `.ne` check. It
  validates the target against its *base* type only; if that succeeds (as
  `1 .gt 0` does), no error is added and the presence of a default-equal
  value is never flagged. The arm only ever adds an error when the base-type
  validation *already* failed and the entry was optional — i.e. it can only
  ever add a redundant error message on top of an existing one, never
  detect the specific "value equals default" case on its own.
- **Python `cddl_verifier`**: accepts the default-equal case for the same
  reason it accepts the `.gt 0`-violating case (see below) — it does not
  enforce numeric comparison controls (`.gt`/`.lt`/`.ge`/`.le`) on map
  values at all, confirmed by testing an isolated schema with `.gt 0` and no
  `.default` involved (`step: number .gt 0`), which passed against a `step:
  0` value. This means `.default`'s failure here is a symptom of a broader,
  pre-existing gap (comparison controls in map member values), not a
  `.default`-specific one.

## Divergence 15: Rust `cddl` crate fails to validate *any* bareword map key whose name collides with a CDDL prelude type identifier

Discovered as a side effect of testing Divergence 14 with the RFC's own
`time:` key name — before renaming it to `moment` to isolate `.default`,
every fixture (including the fully valid "ok" ones) failed with `error
validating at cbor location : expected object value of type time, got
object`. Isolation (stripping `.default`/`.gt` entirely, reducing to a
single required bareword key, and testing other prelude names) showed this
is unrelated to `.default`:

```cddl
top = timer
timer = { time: uint }       ; fails
timer = { moment: uint }     ; passes, identical CBOR shape
timer = { int: uint }        ; fails: "map requires entry key of type int"
timer = { bool: uint }       ; fails: "map requires entry key of type bool"
```

Root cause (`src/validator/cbor.rs`, `visit_identifier`): when visiting a
member key, the visitor only special-cases the ident as a literal text key
if `is_colon_shortcut_present` *and* the ident does not resolve to a
user-defined rule (`rule_from_ident`) — but it never checks whether the
ident matches a CDDL *prelude* type name first. A bareword key named `time`,
`int`, `bool`, etc. therefore falls through into the branches that treat
the ident as a type reference to validate the whole map against (`is_ident_*_data_type`/`token::lookup_ident(...).in_standard_prelude()`),
producing `map requires entry key of type <name>` or `expected object value
of type <name>, got object` instead of treating `<name>` as the map's text
string key. Any bareword key name that happens to match a CDDL prelude
keyword breaks validation of that map entirely, in both directions (valid
and invalid CBOR both fail to validate correctly) — this is a
correctness bug in the Rust crate independent of anything this project
authored, filed here since it directly explains why the RFC 8610 §3.8.6
worked example (which uses `time:` as its key) initially appeared to fail
across the board on Rust `cddl` before the cause was isolated.

## Where this leaves `tps_cddl`

No new correctness gap was found in `tps_cddl` beyond the one bug this
exercise itself found and fixed (named range/size bounds, see above,
already covered by 8 new regression tests). On every fixture and every
real-world file, `tps_cddl`'s check-cbor/check-json results matched what
RFC 8610 (+ 9165/9682 where relevant) mandates. Points reconfirmed at
larger scale than the original `CDDL_TOOL_COMPARISON.md`:

- `tps_cddl` is the only tool of the five, alongside `@cbortech/cbor`, that
  correctly enforces RFC 8610 §3.8.6's `.default` control — rejecting a
  present value that equals its declared default (the implied `.ne`), while
  still requiring the value to satisfy any other base constraint. Rust
  `cddl`, the Ruby gem, and Python `cddl_verifier` all wrongly accept the
  default-equal case (Divergence 14).
- `tps_cddl` has no equivalent to Divergence 15's Rust-crate bug — a
  bareword member-key name that happens to match a CDDL prelude type
  keyword (`time`, `int`, `bool`, ...) validates correctly as a plain text
  key, not as an accidental type reference against the whole map.
- `tps_cddl` correctly distinguishes RFC 8610 §3.9 *type* sockets
  (`$name`, e.g. RFC 9711's `$version-scheme` — never defined anywhere in
  the real files, correctly produces **no** error, since "it is not an
  error if there is no definition for a socket at all") from plain
  undefined typename references (`untagged-coswid` — correctly **is**
  flagged as an error, since it's an ordinary reference, not a socket).
- `tps_cddl` recognizes the full RFC 8610 Appendix D prelude (including
  `uri`, `tdate`) so `~uri` in RFC 9711's `general-uri` resolves cleanly,
  while correctly flagging `~oid` as undefined — `oid` genuinely isn't
  part of the RFC 8610 prelude, so this is a real content gap in RFC 9711
  Appendix D's own CDDL, not a `tps_cddl` false positive.
- `tps_cddl` is the only one of the five tools that both accepts
  RFC 8610-legal tab whitespace (per RFC 5234 §B.1) *and* correctly
  evaluates `.feature`-gated generics in both the accept and reject
  direction, without any extra CLI flag needed.

## Practical takeaways for reuse of these external tools elsewhere in this project

Everything in `CDDL_TOOL_COMPARISON.md`'s "Where this leaves tps_cddl"
section still applies. Additionally, from this pass:

- If reusing the Rust `cddl` crate or Ruby `cddl` gem for anything beyond
  a quick sanity check, always put the rule you actually care about
  *first* in the file — neither supports selecting an arbitrary root rule.
- Do not use the Rust `cddl` crate to validate RFC 9165 `.feature`
  schemas for anything beyond "does this parse" — it does not perform the
  actual semantic check in the reject direction, with or without
  `--features`.
- Do not use `@cbortech/cbor`'s CLI to validate any schema using
  `.feature` at all — there is no way to enable features from the CLI, so
  every feature-gated alternative always fails.
- Do not rely on Rust `cddl`, the Ruby gem, or Python `cddl_verifier` to
  catch a `.default`-violating value (one explicitly sent equal to its
  declared default) — none of the three enforce RFC 8610 §3.8.6's implied
  `.ne`. Only `tps_cddl` and `@cbortech/cbor` do.
- Do not use Rust `cddl` on any schema with a bareword map key that
  happens to match a CDDL prelude type name (`time`, `int`, `bool`, `uint`,
  etc.) — validation breaks for that entry regardless of whether the CBOR
  is actually valid. Rename the key or switch to explicit `"key" => type`
  syntax to work around it.
- Do not rely on Python `cddl_verifier` to enforce numeric comparison
  controls (`.gt`/`.lt`/`.ge`/`.le`) on map member values at all — this is
  a broader gap than `.default` alone (Divergence 14) and was not
  previously documented in the fixture-corpus table above, since none of
  the original 14 fixtures exercised a bare comparison control on a map
  value.
- Do not use the Python `cddl_verifier` tool against a schema whose root
  rule is a bare group-choice, tag, generic, or other control-operator
  expression (only map/array/simple-alias roots work) — and do not trust
  it to enforce array occurrence-count bounds at all.
- Avoid tab characters in any CDDL file that might ever be fed to the
  Ruby gem or to `@cbortech/cbor` — both are non-conformant here (crash vs.
  hard-reject) despite RFC 5234 permitting tabs as CDDL whitespace.
