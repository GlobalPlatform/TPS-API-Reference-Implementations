/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/**
 * @file CDDL grammar for tree-sitter
 * @license MIT
 *
 * Covers RFC 8610 (base CDDL), RFC 9165 (additional control operators),
 * RFC 9682 (tag head-number syntax, updated string escapes), and the
 * `draft-ietf-cbor-cddl-modules` `;#` directive-comment convention.
 *
 * This grammar favors highlighting fidelity over strict validation: CDDL's
 * PEG-based reference grammar (RFC 8610 Appendix B) has several places where
 * "type rule" and "group rule" bodies structurally overlap (a bare type is
 * always also a valid single-entry group). Rather than replicate that
 * ambiguity as parser conflicts, every rule body is parsed uniformly through
 * `group`, which already subsumes the bare-type case. Strict CDDL semantic
 * validation is the job of `tps_cddl`, not this grammar.
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: 'cddl',

  extras: $ => [
    /\s/,
    $.comment,
    $.directive_comment,
  ],

  word: $ => $.identifier,

  // `#7` followed by `.` is ambiguous between "#7.<head_number>" (this tag)
  // and "#7" as a complete type2 immediately followed by a ctlop starting
  // with `.` (e.g. in a hypothetical `#7 .bits ...`). Real-world CDDL always
  // means the former; GLR + a dynamic-precedence preference on `tag` settles
  // it without rejecting the (unusual) latter reading outright.
  conflicts: $ => [[$.tag], [$.grpchoice], [$.occur, $.number], [$.occur], [$.grpent, $.type2]],

  rules: {
    source_file: $ => repeat($.rule),

    rule: $ => seq(
      field('name', $.identifier),
      optional($.genericparm),
      field('assign', $.assign_op),
      field('value', $.group),
    ),

    assign_op: $ => choice('/=', '//=', '='),

    // ─── Generic parameters / arguments ──────────────────────────────────
    genericparm: $ => seq('<', $.identifier, repeat(seq(',', $.identifier)), '>'),
    genericarg: $ => seq('<', $.type, repeat(seq(',', $.type)), '>'),

    // ─── Groups ───────────────────────────────────────────────────────────
    // `grpchoice` is required to hold at least one `grpent` because
    // tree-sitter forbids a rule from matching the empty string. Truly empty
    // groups/maps/arrays (e.g. `{}`) are represented by omitting `group`
    // entirely — every call site wraps it in `optional($.group)`.
    group: $ => seq($.grpchoice, repeat(seq('//', $.grpchoice))),
    grpchoice: $ => repeat1(seq($.grpent, optional(','))),

    grpent: $ => seq(
      optional($.occur),
      choice(
        seq(optional($.memberkey), field('type', $.type)),
        seq('(', $.group, ')'),
      ),
    ),

    memberkey: $ => choice(
      seq(field('key', $.type1), optional('^'), '=>'),
      seq(field('key', $.identifier), ':'),
      seq(field('key', $.value), ':'),
    ),

    // `n*m` occurrence (e.g. `1*2`) is genuinely ambiguous with "occur `n*`,
    // followed by a memberkey/type that happens to start with the uint `m`"
    // at the grammar level; `prec.dynamic` breaks the tie in occur's favor,
    // matching every real CDDL corpus's intent (RFC 8610 §3.3).
    occur: $ => prec.dynamic(1, choice(
      seq(optional($.uint), '*', optional($.uint)),
      '+',
      '?',
    )),

    // ─── Type expressions ───────────────────────────────────────────────────
    type: $ => seq($.type1, repeat(seq('/', $.type1))),

    type1: $ => seq(
      $.type2,
      optional(seq(choice($.rangeop, $.ctlop), $.type2)),
    ),

    rangeop: $ => choice('...', '..'),
    ctlop: $ => seq('.', $.identifier),

    type2: $ => choice(
      $.value,
      seq(field('name', $.identifier), optional($.genericarg)),
      seq('(', $.type, ')'),
      seq('{', optional($.group), '}'),
      seq('[', optional($.group), ']'),
      seq('~', field('name', $.identifier), optional($.genericarg)),
      seq('&', '(', $.group, ')'),
      seq('&', field('name', $.identifier), optional($.genericarg)),
      $.tag,
    ),

    // RFC 8610 #DIGIT[.uint] plus RFC 9682 #6[.head_number](type) / #7[.head_number]
    tag: $ => choice(
      seq('#6', optional(seq('.', $.head_number)), '(', $.type, ')'),
      seq('#7', optional(seq('.', $.head_number))),
      seq('#', /[0-9]/, optional(seq('.', $.uint))),
      '#',
    ),

    // RFC 9682 §3.2: head-number = uint / ("<" type ">")
    head_number: $ => choice($.uint, seq('<', $.type, '>')),

    // ─── Values ─────────────────────────────────────────────────────────────
    value: $ => choice($.number, $.text, $.bytes),

    uint: $ => token(choice(
      /[1-9][0-9]*/,
      /0[xX][0-9a-fA-F]+/,
      /0[bB][01]+/,
      '0',
    )),

    number: $ => choice(
      // Bare non-negative integer: reuse the exact `uint` token rather than
      // re-deriving an identical pattern. Two distinct token rules matching
      // the same text (e.g. "1") tie at the lexer level and tree-sitter can
      // only ever emit one of them per position — since `uint` also has to
      // win at that position for `occur`'s leading `uint? '*'...`, a
      // separately-defined "plain integer" alternative here would simply
      // never be reachable. Every other alternative below is required to
      // consume at least one extra character (a leading '-', a fraction,
      // an exponent, or hexfloat's mandatory 'p'/'P' exponent) so it never
      // ties in length with a bare `uint` match.
      $.uint,
      token(seq(
        '-',
        choice(/[1-9][0-9]*/, /0[xX][0-9a-fA-F]+/, /0[bB][01]+/, '0'),
        optional(seq('.', /[0-9]+/)),
        optional(seq(/[eE]/, optional(/[+-]/), /[0-9]+/)),
      )),
      token(seq(
        choice(/[1-9][0-9]*/, /0[xX][0-9a-fA-F]+/, /0[bB][01]+/, '0'),
        '.', /[0-9]+/,
        optional(seq(/[eE]/, optional(/[+-]/), /[0-9]+/)),
      )),
      token(seq(
        choice(/[1-9][0-9]*/, /0[xX][0-9a-fA-F]+/, /0[bB][01]+/, '0'),
        /[eE]/, optional(/[+-]/), /[0-9]+/,
      )),
      token(seq(
        optional('-'), /0[xX][0-9a-fA-F]+/,
        optional(seq('.', /[0-9a-fA-F]+/)),
        /[pP][+-]?[0-9]+/,
      )),
    ),

    // Text string: RFC 9682 §2.1.1 SESC allows \uXXXX / surrogate pairs; kept
    // permissive here since precise validation is not this grammar's job.
    text: $ => token(seq(
      '"',
      repeat(choice(
        /[^"\\\x00-\x1f]/,
        seq('\\', /./),
      )),
      '"',
    )),

    // Byte string: h'...' / b64'...' / '...' — RFC 9682 §2.1 BCHAR.
    bytes: $ => token(seq(
      optional(choice('h', 'b64')),
      "'",
      repeat(choice(
        /[^'\\\x00-\x1f\x7f-\x9f]/,
        seq('\\', /./),
        '\r\n',
      )),
      "'",
    )),

    identifier: $ => token(seq(
      /[A-Za-z@_$]/,
      repeat(choice(
        /[A-Za-z0-9@_$]/,
        /[-.][A-Za-z0-9@_$]/,
      )),
    )),

    // ─── Comments ───────────────────────────────────────────────────────────
    // Modules-draft directive comments (";# import ..."/ ";# include ...")
    // take priority over the general ";" line comment when both could match.
    directive_comment: $ => token(prec(2, seq(';#', /[^\r\n]*/))),
    comment: $ => token(prec(1, seq(';', /[^\r\n]*/))),
  },
});
