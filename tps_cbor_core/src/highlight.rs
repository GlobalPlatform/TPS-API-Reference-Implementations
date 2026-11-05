/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The tree-sitter grammars the frontends highlight with.
//!
//! One list of languages, so the IDE, the TUI and `tps_ts_highlight_tool` cannot disagree about
//! which grammars exist, what they are called or which files they apply to.
//!
//! # Why this module takes no `tree-sitter` dependency
//!
//! [`Grammar::language`] is a [`LanguageFn`], not a `tree_sitter::Language`. `LanguageFn` comes
//! from `tree-sitter-language`, a tiny crate that exists precisely to be the ABI-stable handoff
//! point between a grammar and whichever `tree-sitter` version the host happens to use: it is a
//! newtype over a C function pointer and nothing else.
//!
//! That indirection is load-bearing here. `tps_cbor_ide` builds against gpui-component's
//! `tree-sitter 0.26`, while `tps_cbor_tui` and `tps_ts_highlight_tool` use `0.27`, and the
//! `tree-sitter` crate carries `links = "tree-sitter"`, so no single Cargo resolution may contain
//! both. Because this module names only `LanguageFn`, both sides can depend on it and convert with
//! `.into()` at the call site against their own `tree-sitter`. See the `[workspace]` comment in
//! `tps_cbor_ide/Cargo.toml`.
//!
//! # Highlight queries are used verbatim
//!
//! [`Grammar::highlights`] is each grammar crate's own `queries/highlights.scm`, unmodified. The
//! queries use nvim-treesitter capture conventions (`@type.parameter`, `@keyword.operator`,
//! `@constant.builtin`); a consumer whose theme does not know a name is expected to fall back by
//! stripping to the prefix, which is what GPUI's `SyntaxColors` does. Rewriting the queries per
//! frontend is exactly the drift this crate exists to prevent.

use tree_sitter_language::LanguageFn;

/// One highlightable language.
#[derive(Clone, Copy)]
pub struct Grammar {
    /// The language's short name, as a registry key: `"cddl"` or `"edn"`.
    pub name: &'static str,
    /// A human-readable name, for a status bar or a language picker.
    pub label: &'static str,
    /// Filename extensions that select this language, without the leading dot.
    pub extensions: &'static [&'static str],
    /// The parser entry point. Convert with `.into()` to get a `tree_sitter::Language`.
    pub language: LanguageFn,
    /// The grammar's own highlight query, verbatim.
    pub highlights: &'static str,
}

// `LanguageFn` is a function pointer wrapper and implements none of these, so they are written out.
// `language` is deliberately excluded from both: two grammars with the same name are the same
// grammar as far as a registry is concerned, and comparing function pointers is not meaningful.
impl std::fmt::Debug for Grammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grammar")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("extensions", &self.extensions)
            .field(
                "highlights",
                &format_args!("<{} bytes>", self.highlights.len()),
            )
            .finish()
    }
}

impl PartialEq for Grammar {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Grammar {}

/// CBOR Extended Diagnostic Notation.
pub const EDN: Grammar = Grammar {
    name: "edn",
    label: "CBOR EDN",
    extensions: &["edn"],
    language: tree_sitter_edn::LANGUAGE,
    highlights: tree_sitter_edn::HIGHLIGHTS_QUERY,
};

/// Concise Data Definition Language (RFC 8610).
pub const CDDL: Grammar = Grammar {
    name: "cddl",
    label: "CDDL",
    extensions: &["cddl"],
    language: tree_sitter_cddl::LANGUAGE,
    highlights: tree_sitter_cddl::HIGHLIGHTS_QUERY,
};

/// Every grammar, for a frontend to register in one loop at startup.
pub const ALL: [Grammar; 2] = [CDDL, EDN];

/// The grammar that handles `path`'s extension, if any.
///
/// The comparison is ASCII-case-insensitive, so `GP_EAP.CDDL` resolves like `gp_eap.cddl`.
pub fn for_path(path: &std::path::Path) -> Option<Grammar> {
    let ext = path.extension()?.to_str()?;
    for_extension(ext)
}

/// The grammar that handles the extension `ext`, given without a leading dot.
pub fn for_extension(ext: &str) -> Option<Grammar> {
    ALL.into_iter().find(|grammar| {
        grammar
            .extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(ext))
    })
}

/// The grammar registered under `name`, if any.
pub fn by_name(name: &str) -> Option<Grammar> {
    ALL.into_iter().find(|grammar| grammar.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn extensions_resolve_to_their_grammar() {
        assert_eq!(for_extension("cddl"), Some(CDDL));
        assert_eq!(for_extension("edn"), Some(EDN));
        assert_eq!(for_extension("cbor"), None);
        assert_eq!(for_extension(""), None);
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(for_extension("CDDL"), Some(CDDL));
        assert_eq!(for_path(Path::new("/x/GP_EAP.CDDL")), Some(CDDL));
    }

    #[test]
    fn paths_without_a_known_extension_resolve_to_nothing() {
        assert_eq!(for_path(Path::new("/x/claims.cbor")), None);
        assert_eq!(for_path(Path::new("/x/Makefile")), None);
        assert_eq!(for_path(Path::new("/x/")), None);
    }

    #[test]
    fn names_are_unique_and_resolvable() {
        for grammar in ALL {
            assert_eq!(by_name(grammar.name), Some(grammar));
        }
        assert_eq!(by_name("rust"), None);
        let mut names: Vec<&str> = ALL.iter().map(|g| g.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len(), "duplicate grammar name");
    }

    #[test]
    fn no_extension_is_claimed_by_two_grammars() {
        let mut seen: Vec<&str> = ALL
            .iter()
            .flat_map(|g| g.extensions.iter().copied())
            .collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "an extension is claimed twice");
    }

    /// The queries must be non-empty and must be the grammar crates' own, not a copy made here.
    #[test]
    fn every_grammar_carries_a_highlight_query() {
        for grammar in ALL {
            assert!(
                grammar.highlights.len() > 100,
                "{}: query is only {} bytes",
                grammar.name,
                grammar.highlights.len()
            );
            assert!(
                grammar.highlights.contains('@'),
                "{}: query has no captures",
                grammar.name
            );
        }
        assert_eq!(CDDL.highlights, tree_sitter_cddl::HIGHLIGHTS_QUERY);
        assert_eq!(EDN.highlights, tree_sitter_edn::HIGHLIGHTS_QUERY);
    }

    #[test]
    fn debug_does_not_dump_the_whole_query() {
        let shown = format!("{CDDL:?}");
        assert!(shown.contains("cddl"), "{shown}");
        assert!(shown.contains("bytes"), "{shown}");
        assert!(!shown.contains('@'), "the query body leaked into Debug");
    }
}
