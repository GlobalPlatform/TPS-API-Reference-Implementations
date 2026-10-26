/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/**
 * @file EDN (CBOR Extended Diagnostic Notation) grammar for tree-sitter
 * @license MIT
 *
 * Covers RFC 8610 Appendix G, as implemented by `tps_minicbor::edn`: base
 * diagnostic notation (maps, arrays, strings, byte strings, tags, simple
 * values), hex/octal/binary/hexfloat numbers (Appendix G.5), embedded-CBOR
 * `<<...>>` notation (Appendix G.3), concatenated string/byte-string
 * notation (Appendix G.4), and `/.../`-delimited comments (Appendix G.6).
 *
 * This grammar favors highlighting fidelity over strict validation. Only
 * hex byte strings (`h'...'`) model comments/whitespace as interleaved
 * grammar symbols (matching `tps_minicbor::edn`'s parser, which is the only
 * string form where it calls `skip_ws_comments` between characters); the
 * other string/byte-string forms are single tokens.
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: 'edn',

  extras: $ => [
    /\s/,
    $.comment,
  ],

  conflicts: $ => [],

  rules: {
    // EDN (per `tps_minicbor::edn::parse_edn`) is a single top-level data
    // item, not a sequence — matching `source_file` to exactly that (rather
    // than `repeat($.value)`) also avoids a genuine grammar ambiguity: two
    // adjacent top-level concatenated-string atoms would otherwise be
    // indistinguishable from one concatenated_string spanning both.
    source_file: $ => optional($.value),

    value: $ => choice(
      $.tag,
      $.number,
      'true',
      'false',
      'null',
      'undefined',
      $.simple_value,
      'NaN',
      'Infinity',
      seq('-', 'Infinity'),
      $.concatenated_string,
      $.embedded_cbor,
      $.array,
      $.map,
    ),

    // ─── Tags and simple values ──────────────────────────────────────────────
    tag: $ => seq(field('number', $.uint), '(', field('value', $.value), ')'),
    simple_value: $ => seq('simple', '(', $.uint, ')'),

    // ─── Numbers (RFC 8610 Appendix G.5) ─────────────────────────────────────
    // Bare non-negative integer is reused directly by `number` (rather than
    // re-derived as an identical alternative) to avoid a lexer tie between
    // two token rules matching the same text — see tree-sitter-cddl's
    // grammar.js for the full rationale; the same hazard applies here.
    uint: $ => token(choice(
      /[1-9][0-9]*/,
      /0[xX][0-9a-fA-F]+/,
      /0[bB][01]+/,
      /0[oO][0-7]+/,
      '0',
    )),

    number: $ => choice(
      $.uint,
      token(seq('-', choice(
        /[1-9][0-9]*/,
        /0[xX][0-9a-fA-F]+/,
        /0[bB][01]+/,
        /0[oO][0-7]+/,
        '0',
      ))),
      token(seq(
        optional('-'), choice(/[1-9][0-9]*/, '0'),
        '.', /[0-9]+/,
        optional(seq(/[eE]/, optional(/[+-]/), /[0-9]+/)),
      )),
      token(seq(
        optional('-'), choice(/[1-9][0-9]*/, '0'),
        /[eE]/, optional(/[+-]/), /[0-9]+/,
      )),
      token(seq(
        optional('-'), '0x', /[0-9a-fA-F]+/,
        optional(seq('.', /[0-9a-fA-F]+/)),
        /[pP][+-]?[0-9]+/,
      )),
    ),

    // ─── Strings and byte strings ────────────────────────────────────────────
    text: $ => token(seq(
      '"',
      repeat(choice(/[^"\\]/, seq('\\', /./))),
      '"',
    )),

    bytes: $ => choice($.hex_bytes, $.b64_bytes, $.raw_bytes),

    // `h'...'` is the one byte-string form whose grammar (per RFC 8610
    // Appendix G.1/G.6 and `tps_minicbor::edn::parse_hex_bstr`) allows
    // whitespace and comments between individual hex digits; modeled as a
    // repetition of single-digit tokens (rather than one `token()`) so the
    // grammar's `extras` can slot in between them.
    hex_bytes: $ => seq('h', "'", repeat($._hexdigit), "'"),
    _hexdigit: $ => /[0-9a-fA-F]/,

    b64_bytes: $ => token(seq(
      'b64', "'",
      repeat(/[^'\\]/),
      "'",
    )),

    raw_bytes: $ => token(seq(
      "'",
      repeat(choice(/[^'\\]/, seq('\\', /./))),
      "'",
    )),

    // RFC 8610 Appendix G.4: adjacent string/byte-string literals concatenate.
    // A single atom is itself a (trivial) concatenated_string, so `value`
    // only ever needs the one alternative. `prec.left` prefers the longer
    // (more-atoms) parse over stopping early and starting a second
    // top-level value, since both are otherwise complete, valid parses.
    _string_atom: $ => choice($.text, $.bytes),
    concatenated_string: $ => prec.left(1, repeat1($._string_atom)),

    // ─── Collections ─────────────────────────────────────────────────────────
    array: $ => seq('[', optional($.value_list), ']'),
    embedded_cbor: $ => seq('<<', optional($.value_list), '>>'),
    value_list: $ => seq($.value, repeat(seq(',', $.value))),

    map: $ => seq('{', optional($.pair_list), '}'),
    pair_list: $ => seq($.pair, repeat(seq(',', $.pair))),
    pair: $ => seq(field('key', $.value), ':', field('val', $.value)),

    // ─── Comments (RFC 8610 Appendix G.6: a pair of "/" delimits a comment) ──
    comment: $ => token(seq('/', /[^/]*/, '/')),
  },
});
