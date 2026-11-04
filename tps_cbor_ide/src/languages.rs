/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Registers this workspace's grammars with gpui-component's tree-sitter highlighter.
//!
//! The `highlights.scm` queries are taken raw from the grammar crates and used **unmodified**, so
//! the IDE, `tps_cbor_tui` and `tps_ts_highlight_tool` cannot drift apart. That is why they arrive
//! through `tps_cbor_core::highlight` rather than being read from the `.scm` files here, and why
//! this module iterates `highlight::ALL` instead of naming `cddl` and `edn`: adding a grammar to
//! the core crate is then the only edit needed to have the IDE register it.
//!
//! Our queries use nvim-treesitter capture names — `@type.parameter`, `@keyword.operator`,
//! `@type.builtin`, `@constant.builtin`, `@comment.documentation` — and gpui-component recognises a
//! fixed list of 41 (`highlighter/registry.rs:18`) that includes none of them. They still resolve,
//! because tree-sitter-highlight matches a capture against the recognised names by longest dotted
//! subset, so `type.parameter` reduces to `type` and `keyword.operator` to `keyword`. Phase 0
//! confirmed that on screen rather than on paper.
//!
//! One capture does lose its colour: `@comment.documentation` on CDDL's `directive_comment`.
//! `comment.doc` *is* one of the 41, but the reduction only matches whole dot-separated components
//! and `doc` is not a component of `comment.documentation`, so it falls back to plain `comment` and
//! `;# import` / `;# include` lines look like ordinary comments. [`crate::theme`] gives
//! `comment.doc` its own colour, so the fix is a one-line capture addition in
//! `tree-sitter-cddl/queries/highlights.scm` whenever that is approved — see `PLAN.md`.

use gpui_kit::component::highlighter::{LanguageConfig, LanguageRegistry};
use tps_cbor_core::highlight;

/// Registers every grammar `tps_cbor_core` knows about. Call once at startup, before any editor is
/// built.
///
/// `LanguageRegistry` is a process-wide singleton rather than state held in `cx`, so this takes no
/// context and is safe to call from a headless test.
///
/// The empty vector is the injection language list and the two empty strings are the injections and
/// locals queries, which neither grammar has. An empty injections query is how a grammar says it
/// embeds no other language — relevant here because EDN *can* carry CBOR inside a `bstr`, but
/// decoding that is an explicit user action (`UI_DESIGN.md:93`), not a highlighting concern.
pub fn register() {
    let registry = LanguageRegistry::singleton();
    for grammar in highlight::ALL {
        registry.register(
            grammar.name,
            &LanguageConfig::new(
                grammar.name,
                grammar.language.into(),
                vec![],
                grammar.highlights,
                "",
                "",
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registration has to survive being called more than once, because every headless test that
    /// builds an editor needs the grammars and they share one process-wide registry.
    #[test]
    fn registering_twice_is_harmless() {
        register();
        register();
    }

    /// The queries must be the grammars' own. If these ever stop being byte-identical to what the
    /// grammar crates export, the IDE has started maintaining a second copy of the highlighting,
    /// which is the exact drift this module exists to prevent.
    #[test]
    fn the_queries_come_from_the_grammar_crates_unmodified() {
        let cddl = highlight::by_name("cddl").expect("cddl is a known grammar");
        let edn = highlight::by_name("edn").expect("edn is a known grammar");
        assert_eq!(cddl.highlights, tree_sitter_cddl::HIGHLIGHTS_QUERY);
        assert_eq!(edn.highlights, tree_sitter_edn::HIGHLIGHTS_QUERY);
    }

    /// Both grammars must actually have a parser behind them once registered. `LanguageConfig`
    /// takes `Option<tree_sitter::Language>`, so a grammar can be registered with a name and a
    /// highlight query and no way to parse anything — which highlights nothing and reports no
    /// error. Phase 0 hit the inverse of this (an unregistered language silently yielding no tree)
    /// and it is just as quiet.
    #[test]
    fn every_registered_grammar_has_a_parser() {
        register();
        let registry = LanguageRegistry::singleton();
        for grammar in highlight::ALL {
            let config = registry
                .language(grammar.name)
                .unwrap_or_else(|| panic!("{} is registered", grammar.name));
            assert!(
                config.has_grammar(),
                "{} registered without a parser",
                grammar.name
            );
        }
    }
}
