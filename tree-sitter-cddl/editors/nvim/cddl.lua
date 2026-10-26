-- **************************************************************************************************
--  Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
--  SPDX-License-Identifier: MIT
-- **************************************************************************************************/
-- Drop-in Neovim configuration for the tree-sitter-cddl grammar.
--
-- This grammar is not (yet) published in nvim-treesitter's parser registry,
-- so it must be registered manually, pointing at this repository's checkout.
--
-- Usage: adjust GRAMMAR_DIR below (or set it from an environment variable/
-- plugin option) and `require` this file from your `init.lua`, e.g.:
--
--   require("cddl")   -- if this file is placed on your runtimepath as
--                      -- lua/cddl.lua, or dofile("/path/to/this/file.lua")
--
-- Tested against the classic nvim-treesitter parser_configs API
-- (nvim-treesitter/nvim-treesitter, pre-2024 "master" branch). If you are on
-- the newer nvim-treesitter "main" rewrite, its parser-registration API has
-- changed; consult that plugin's README for the current equivalent of
-- `parser_configs` and adapt the `install_info` block below accordingly.

local GRAMMAR_DIR = vim.fn.expand("~/Code/OpenSource/TPS-API-Reference-Implementations/tree-sitter-cddl")

vim.filetype.add({
  extension = {
    cddl = "cddl",
  },
})

local ok, parsers = pcall(require, "nvim-treesitter.parsers")
if not ok then
  vim.notify("cddl.lua: nvim-treesitter not found; filetype registered, but no highlighting", vim.log.levels.WARN)
  return
end

local parser_config = parsers.get_parser_configs()
parser_config.cddl = {
  install_info = {
    url = GRAMMAR_DIR, -- local path; nvim-treesitter also accepts git URLs here
    files = { "src/parser.c" },
    branch = "main",
    generate_requires_npm = false,
    requires_generate_from_grammar = false,
  },
  filetype = "cddl",
}

-- After sourcing this file, run `:TSInstall cddl` once to compile the parser.
--
-- Highlight queries: nvim-treesitter looks for `queries/cddl/highlights.scm`
-- on the runtimepath. Either symlink/copy this crate's queries/highlights.scm
-- there, e.g.:
--
--   mkdir -p ~/.config/nvim/queries/cddl
--   cp <GRAMMAR_DIR>/queries/highlights.scm ~/.config/nvim/queries/cddl/highlights.scm
--
-- or add `<GRAMMAR_DIR>/queries` conceptually via `after/queries/cddl/highlights.scm`.
