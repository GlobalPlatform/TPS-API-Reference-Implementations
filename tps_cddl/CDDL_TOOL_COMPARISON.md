# CDDL tool comparison

Five CDDL/CBOR validators were run against `tps_cddl`'s own test fixtures
(`tests/fixtures/*.cddl`, `*.cbor`) to compare behavior on both well-formed
schemas/data and deliberately broken ones. This records what each tool does
and, where they disagree, which behavior RFC 8610 (+ RFC 9165, RFC 9682)
actually mandates.

## Tools compared

| Tool | Invocation | Version |
|---|---|---|
| `tps_cddl` (this crate) | `target/release/cddl check-cbor <cddl> <cbor> --rule <name>` | workspace HEAD |
| Rust `cddl` crate (anweiss/cddl) | `~/.cargo/bin/cddl validate --cddl <cddl> --cbor <cbor>` | 0.10.6 |
| Ruby `cddl` gem | `ruby /var/lib/gems/3.2.0/gems/cddl-0.12.6/bin/cddl <cddl> validate <cbor>` | 0.12.6 |
| Python `cddl_verifier` (sahebbiswas/cddl_verifier) | `python3 cbor_cddl_analyzer.py <cddl> <cbor> --type <name>` | HEAD (cloned for this comparison) |
| `@cbortech/cbor` CLI | `npx cbor validate --cddl <cddl> --cddl-rule <name> --type cbor <cbor>` | 0.26.8 |

All five agree on the straightforward cases: `valid_person.cbor` validates,
`mismatch_person.cbor` (an `int` where `person.name` expects `tstr`) fails,
and RFC 8610 §3.10 generics (`pair<a,b> = [a,b]` / `pair<tstr,uint>`)
validate/reject correctly when the CBOR does/doesn't match. The interesting
divergences are in error handling and semantic-validation depth, below.

## Divergences found

### 1. Rust `cddl` crate: exit code 0 on validation failure (without `--ci`)

Running `cddl validate --cddl valid_person.cddl --cbor mismatch_person.cbor`
prints `[ERROR] Validation ... failed: ...` to stderr but **exits 0**. Every
other tool (`tps_cddl`, Ruby, Python, cbortech) exits non-zero on a CDDL
mismatch. The Rust crate only exits non-zero if you additionally pass the
global `--ci` flag (`cddl --ci validate ...` → exit 1). This is a footgun
for any script/CI pipeline that checks exit code without `--ci`; the tool's
own `--help` text ("Enable CI mode, failing if files cannot be found or
other edge cases") doesn't make clear that this includes validation
failures themselves, not just I/O errors.

Not a spec question — RFC 8610 doesn't mandate an exit-code convention —
but worth flagging as an actual bug-shaped surprise for anyone scripting
against this tool.

### 2. Rust `cddl` crate: does not detect undefined type references

`error_undefined.cddl` (`person = {name: tstr, dept: department}` where
`department` is never defined) is reported **conformant** by
`cddl compile-cddl --cddl error_undefined.cddl` (exit 0). `tps_cddl`, the
Ruby gem, and `@cbortech/cbor` all reject it as a semantic error; the
Python tool's `--show-types` silently resolves `dept: department` without
flagging the missing definition either (see #4).

**Spec basis**: RFC 8610's ABNF grammar (Appendix B, `type2 = ... / id`)
permits any identifier as a type reference at the *syntax* level — an
undefined reference is not a grammar violation. But §1.1 states CDDL's
purpose is to "unambiguously specify [] representations", and every worked
example in the RFC defines every name it uses; a dangling reference cannot
be validated against (there is nothing to check the target data against),
so it is universally treated as a semantic error by implementations that
actually perform validation. `tps_cddl`'s stance (error, not just warning)
matches the majority behavior and is necessary for `check-cbor`/`check-json`
to mean anything.

### 3. Ruby `cddl` gem: uncaught Ruby exceptions instead of diagnostics

For `error_duplicate.cddl`, `error_undefined.cddl`, and
`warn_unknown_ctlop.cddl`, the Ruby gem does not produce a CDDL-level error
message — it raises a bare Ruby `RuntimeError` with a full backtrace
(`cddl.rb:219:in 'block in rules': Duplicate rule definition color as
[...] (RuntimeError)`), exiting 1. This works (it does fail, correctly),
but is unsuitable for any automated tooling that wants to parse *why* it
failed — the error text is an internal Ruby exception message and
backtrace, not a structured diagnostic with source location. `error_parse`
is handled slightly better (a caught `CDDL::ParseError` → `EX_DATAERR`
exit 65 with a "Look for syntax problems around the %%% markers" message),
but the crash-on-semantic-error behavior for the other three cases suggests
duplicate/undefined-name checks were never wrapped in the gem's own
`ParseError` handling.

### 4. Python `cddl_verifier`: unknown control operators and rule redefinition are silently accepted

`--show-types --verbose` on `warn_unknown_ctlop.cddl` (`x = tstr .custom
tstr`) parses the rule (`Parsed type alias: x = tstr .custom tstr`) with no
warning that `.custom` isn't a registered RFC 8610 §3.8 control operator —
it simply doesn't apply it at all (the type list ends up empty; `.custom`
is dropped rather than checked or flagged).

Similarly, `error_duplicate.cddl` (`color = "red"` then `color = "blue"`)
is parsed as two separate "type alias" events with no error or warning
that the second `=` illegally redefines `color` rather than extending it
via `/=` — last-definition-wins, silently.

**Spec basis** (redefinition): RFC 8610 §2.2.2 is explicit: additional
alternatives can only be added to an *existing* rule via `/=` or `//=`;
using bare `=` twice for the same name is not "extending", it's two
conflicting definitions, and no implementation is required to make sense
of that — but silently keeping the last one without any diagnostic hides
a real authoring bug. `tps_cddl`, Rust `cddl`, and `@cbortech/cbor` all
correctly reject this; Ruby crashes (see #3) rather than reporting the
duplication cleanly; Python accepts it silently.

**Spec basis** (unknown control operator): per RFC 8610 §6.1's IANA
registry, "All other control operator names are Unassigned" — an
unassigned name is not itself a syntax error, but a tool that silently
drops the constraint (rather than warning, or matching-target-only per
`@cbortech/cbor`'s approach, or erroring like `tps_cddl`) risks giving
false confidence: `.custom` in `tstr .custom tstr` reads as if it narrows
`tstr` somehow, when the Python tool applies no narrowing and issues no
warning either.

### 5. `@cbortech/cbor`: `.plus`/`.feature` (RFC 9165) require explicit opt-in per validation call, not per-schema

`@cbortech/cbor validate --cddl plus.cddl ...` reports `cddl warning:
.plus operands do not resolve to numbers; not checked` followed by a hard
**violation** (`.plus could not be computed`) causing overall invalid,
even for a syntactically well-formed `.plus` use (`uint .plus 1` against a
non-numeric target here, admittedly a bad example target — but the
`.feature` case is cleaner: `uint .feature "optional-support"` is rejected
as invalid with `feature "optional-support" is not enabled (pass it in
options.features to accept it)`).

This mirrors `tps_cddl`'s own `--rfc9165`/`Config::rfc9165` opt-in design
(RFC 9165 extensions are warnings/errors unless explicitly enabled) — the
two tools agree in spirit that RFC 9165 operators need an explicit flag,
they just apply the "not enabled" consequence differently (`tps_cddl`
warns on the *schema* if the flag is absent but still processes `.feature`
data collection per RFC 9165 §4; `@cbortech/cbor` turns it into a hard
per-value CDDL violation during CBOR validation instead). Neither the Rust
crate nor the Ruby gem gate `.plus`/`.feature`/`.cat` behind any flag at
all — both parse and (partially) apply them unconditionally, which is
arguably *more* permissive than RFC 9165 intends (§1 of RFC 9165 frames
these as opt-in extensions precisely because unextended CDDL tooling
should not silently start accepting `.plus` etc. as if it were core
RFC 8610).

### 6. RFC 9682 (`#6.<uint>(tstr)` non-literal tag numbers, empty documents)

All five tools accept a non-literal tag-number controller
(`my-tag = #6.<uint>(tstr)`) and an empty document without complaint —
i.e. all four *external* tools default to RFC-9682-permissive behavior
unconditionally, whereas `tps_cddl` is the only one that defaults to
strict RFC 8610 (rejecting both) unless `--rfc9682`/`Config { rfc9682:
true, .. }` is passed. This isn't a bug in the other tools — RFC 9682 is
a set of grammar *corrections* to fix defects in RFC 8610's original ABNF,
so treating its permissive grammar as the baseline is reasonable — but it
means `tps_cddl` is the strictest-by-default of the five, which is a
deliberate, documented design choice (`Config::strict()`), not an
oversight.

## Summary table

| Case | tps_cddl | Rust `cddl` | Ruby `cddl` | Python `cddl_verifier` | `@cbortech/cbor` |
|---|---|---|---|---|---|
| Valid CBOR | ok | ok (exit 0) | ok (exit 0) | ok (exit 0) | ok (exit 0) |
| Type-mismatch CBOR | reject (exit 1) | reject *(exit 0 without `--ci`!)* | reject (exit 1) | reject (exit 1) | reject (exit 1) |
| Undefined type ref | error | **not detected** | crash/backtrace (still exit 1) | **silently resolved, no warning** | error |
| Duplicate rule (`=` twice) | error | error | crash/backtrace (still exit 1) | **silently last-wins** | error |
| Unknown control operator | warning | parse error (rejects) | crash/backtrace (still exit 1) | **silently dropped** | warning + separate match failure |
| Parse error | clean diagnostic w/ source span | clean diagnostic w/ source span | raw parser-state dump | (not tested — no parse-tree API exposed for this) | clean diagnostic w/ line:col |
| RFC 9165 gating | opt-in flag | **unconditional** | **unconditional** | not implemented | opt-in via `options.features` |
| RFC 9682 grammar | opt-in flag (strict by default) | unconditional accept | unconditional accept | not tested | unconditional accept |

## Where this leaves `tps_cddl`

No correctness gap was found in `tps_cddl` itself during this comparison —
on every case where tools disagreed, `tps_cddl`'s behavior matches either
the spec text directly (undefined-reference and duplicate-rule rejection)
or a deliberate, documented policy choice (strict-by-default RFC 9165/9682
gating). The main external-tool risks worth remembering if either gets
reused elsewhere in this project:

- Never trust the Rust `cddl` crate's exit code for validation-failure
  detection without `--ci`, and don't rely on it to catch undefined type
  references at all.
- Don't rely on the Ruby gem's stderr text for anything other than "it
  failed" — it's an implementation-detail stack trace, not a diagnostic.
- Don't rely on the Python `cddl_verifier` tool to catch schema-authoring
  mistakes (duplicate rules, unknown control operators) — it optimizes for
  decoding/annotating CBOR against a schema it assumes is already correct.
