# Editor setup — tree-sitter-cddl

## Neovim

Requires the classic `nvim-treesitter` plugin (nvim-treesitter/nvim-treesitter).

1. Edit `editors/nvim/cddl.lua`'s `GRAMMAR_DIR` to point at your checkout of
   this repository.
2. Source it from your `init.lua`, e.g. `dofile("<path>/editors/nvim/cddl.lua")`.
3. Run `:TSInstall cddl` once to compile the parser.
4. Copy (or symlink) `queries/highlights.scm` to
   `~/.config/nvim/queries/cddl/highlights.scm` so nvim-treesitter picks it up.

If you're on the newer nvim-treesitter "main" rewrite, its parser
registration API differs from the classic `parser_configs` table used here —
consult that plugin's README for the current equivalent.

## Zed

`editors/zed/extension.toml`'s `[grammars.cddl]` section points at a
placeholder `rev` — Zed's extension installer fetches grammars from a
published git repository, and this repository isn't published anywhere Zed
can fetch it from yet. Until that changes, load it as a **dev extension**
instead, which builds the grammar directly from your local checkout:

1. Open Zed → command palette → `zed: install dev extension`.
2. Select the `editors/zed/` directory in this crate.
3. Zed compiles `tree-sitter-cddl`'s `grammar.js`-generated parser (via
   `src/parser.c`) locally and applies `languages/cddl/highlights.scm`.
4. Open a `.cddl` file to see highlighting.

Once the grammar is published to a fetchable git repository, replace the
`repository`/`rev` TODOs in `extension.toml` and the extension can be
distributed normally (or submitted to Zed's extension registry).
