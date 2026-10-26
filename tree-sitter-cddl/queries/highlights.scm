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
(directive_comment) @comment.documentation
