;***************************************************************************************************
; Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
; SPDX-License-Identifier: MIT
;*************************************************************************************************/
; EDN highlight query (CBOR Extended Diagnostic Notation, RFC 8610 Appendix G)
; Capture names follow the conventions shared by nvim-treesitter and Zed
; (https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html).

(tag number: (uint) @type.builtin)
(simple_value) @type.builtin

["true" "false"] @boolean
"null" @constant.builtin
"undefined" @constant.builtin
["NaN" "Infinity"] @constant.numeric.float

(uint) @number
(number) @number

(text) @string
(hex_bytes) @string.special
(b64_bytes) @string.special
(raw_bytes) @string.special

(pair key: (value) @property)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "<<"
  ">>"
] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter

(comment) @comment
