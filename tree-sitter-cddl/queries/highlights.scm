;***************************************************************************************************
; Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
; SPDX-License-Identifier: MIT
;*************************************************************************************************/
; CDDL highlight query (RFC 8610 / RFC 9165 / RFC 9682 / cddl-modules)
; Capture names follow the conventions shared by nvim-treesitter and Zed
; (https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html).

(rule name: (identifier) @type)
(genericparm (identifier) @type.parameter)

(memberkey key: (identifier) @property)
(memberkey key: (type1 (type2 (value (text)))) @property)

(type2 name: (identifier) @type)

(assign_op) @operator
(rangeop) @operator
(ctlop) @keyword.operator
(occur) @operator
"~" @operator
"&" @operator
"//" @operator
"/" @operator
"=>" @operator
"^" @operator

(uint) @number
(number) @number
(text) @string
(bytes) @string.special

(tag) @type.builtin
(head_number) @number

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "<"
  ">"
] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter

"?" @operator
"+" @operator
"*" @operator

(comment) @comment
; Both names for one node, and the order is load-bearing. `comment.documentation` is the
; nvim-treesitter name and `comment.doc` is the one gpui-component's 41-capture list recognises
; (`highlighter/registry.rs:22`); with only the former, `comment.doc` is never reached and the
; reduction falls back to plain `comment`, so `;#` lines look like ordinary comments in the IDE.
; The two consumers disagree about which capture wins: tree-sitter-highlight — nvim, Zed and
; `tps_ts_highlight_tool` — takes the *last*, while gpui-component keeps the *first* of two
; captures on an identical range (`highlighter/highlighter.rs:1105-1118`). Listing `comment.doc`
; first therefore gives each consumer the name it understands.
(directive_comment) @comment.doc @comment.documentation
