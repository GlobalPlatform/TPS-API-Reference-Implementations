/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The provenance `tps_cddl::resolve_modules` discards.
//!
//! # The problem
//!
//! `tps_cddl` resolves a `;# import` / `;# include` chain by concatenating every module's text
//! into one virtual document and rebasing every span into it. The per-module offset table is
//! `LoaderCtx.offsets` (`tps_cddl/src/modules.rs:750`) -- private, on a private struct, dropped
//! when `resolve_modules` returns. [`crate::cddl::Resolved`] therefore hands back spans that index
//! a string matching no file on disk, and `UI_DESIGN.md:111-112` wants an error in an included
//! file reported against *that* file, on its own inner tab.
//!
//! # The solution, and why it is sound
//!
//! Rather than change `tps_cddl`, reconstruct the table and *prove* it. `modules.rs` guarantees
//! more than its public API admits:
//!
//! * `virtual_source` is the raw `read_to_string` text of each file concatenated with **no
//!   separators, headers or rewriting** -- `:864-865` is the only `push_str`, with the offset taken
//!   immediately before it.
//! * The cache is consulted *before* loading (`:803-805`), so each file is appended **exactly
//!   once**.
//! * `;# import ... as X` and `;# constrain` rewrite only the AST, and `stamp_span_type1`
//!   (`:1361-1365`) re-stamps synthetic spans onto the directive line *"so every span in the merged
//!   `Cddl` stays within `virtual_source`'s bounds"*.
//!
//! So the merged source is a **gap-free partition into exact file texts**, one region per file.
//!
//! [`walk`] collects the file *set* and [`partition`] recovers the regions by **scanning** the
//! merged source -- at each position matching the unique file whose text begins there -- rather
//! than by replaying `tps_cddl`'s load order. That is deliberately stronger than concatenating our
//! own read and comparing:
//!
//! * **It is order-independent.** If upstream ever changes pre-order to post-order, sorts
//!   directives, or interns a module earlier, the partition still succeeds and attribution stays
//!   exact. Order replication would fail and degrade the UI for no reason.
//! * Consuming the whole string *implies* byte equality, so it **cannot silently mis-attribute**.
//!   A separator, a header, rewritten text or a file pushed twice all break uniqueness or leave a
//!   tail, and [`Chain::attributed`] goes false -- the UI degrades visibly to unattributed rows
//!   rather than naming the wrong file.
//!
//! The one ambiguity the scan admits is two *distinct* files with byte-identical content. That
//! degrades only its own regions, not the whole document: see [`partition`].

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use tps_cddl::ModuleSearchPath;
use tps_cddl::modules::scan_directives;

use crate::diag::{Diagnostic, Severity, Source};

/// One file in a resolved `;# import` / `;# include` chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainFile {
    /// The path as `tps_cddl`'s search path resolved it, or as the caller opened it for the root.
    pub path: PathBuf,
    /// The file's text, read once during the [`walk`].
    pub text: String,
    /// Nesting depth: 0 for the file the user opened.
    pub depth: usize,
    /// Index of the file whose directive pulled this one in; `None` for the root.
    pub parent: Option<usize>,
    /// This file's slice of the merged source.
    ///
    /// Empty until [`Chain::locate`] runs, and empty afterwards for a file whose region could not
    /// be uniquely identified. An empty range means "not located", which is why
    /// [`Chain::attribute`] skips it rather than matching it.
    pub range: Range<usize>,
}

/// A resolved include chain, with provenance.
///
/// Files are in depth-first pre-order with the opened file first -- the order `CddlDocSet`'s
/// "entry 0 is the file the user opened" convention assumes, and the order the CDDL tab's inner
/// tab bar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// Every file reached from the root, deduplicated.
    pub files: Vec<ChainFile>,
    /// Whether [`ChainFile::range`]s were *proved* against the merged source by byte equality.
    ///
    /// False means attribution is unavailable and callers must not rebase spans. See the module
    /// documentation for why that degradation is the design and not a weakness.
    pub attributed: bool,
    /// Unresolvable modules, unreadable files, and directive-scan warnings, each attributed to the
    /// file whose directive named it.
    pub diagnostics: Vec<Diagnostic>,
}

impl Chain {
    /// The file a merged-source span belongs to, and the span rebased into that file.
    ///
    /// `None` when the chain is unattributed, when no located region contains `span.start`, or
    /// when the containing region belongs to two byte-identical files. A span that runs past its
    /// file's region -- which `stamp_span_type1`'s invariant says should not happen -- is clamped
    /// to that file rather than silently reported against the next one.
    pub fn attribute(&self, span: Range<usize>) -> Option<(usize, Range<usize>)> {
        if !self.attributed {
            return None;
        }
        let located = || {
            self.files
                .iter()
                .enumerate()
                .filter(|(_, file)| !file.range.is_empty())
        };
        // Strict containment first; a span starting exactly at a region's end belongs to the next
        // file, except at the very end of the merged source where there is no next file.
        let found = located()
            .find(|(_, file)| file.range.contains(&span.start))
            .or_else(|| located().find(|(_, file)| file.range.end == span.start))?;
        let (index, file) = found;
        let end = span.end.clamp(span.start, file.range.end);
        Some((index, span.start - file.range.start..end - file.range.start))
    }

    /// Index of the file at `path`, compared on the same lowercased canonical key the loader uses.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        let wanted = canonical_key(path);
        self.files
            .iter()
            .position(|file| canonical_key(&file.path) == wanted)
    }

    /// Fill in [`ChainFile::range`]s by partitioning `merged`, and set [`Chain::attributed`].
    ///
    /// Separate from [`walk`] because the merged source comes from
    /// [`crate::cddl::resolve`], which may fail for reasons the walk survives -- a circular
    /// include still has a perfectly good file list to put diagnostics on.
    pub fn locate(&mut self, merged: &str) {
        let texts: Vec<(PathBuf, String)> = self
            .files
            .iter()
            .map(|file| (file.path.clone(), file.text.clone()))
            .collect();
        let (ranges, attributed) = partition(merged, &texts);
        for (file, range) in self.files.iter_mut().zip(ranges) {
            file.range = range;
        }
        self.attributed = attributed;
    }
}

/// Walk `path`'s directives, reading each file once.
///
/// Files are deduplicated on the lowercased canonical path that `tps_cddl`'s own loader keys on
/// (`canonical_key`, `modules.rs:781-787`, whose doc says so explicitly). A case-*sensitive* key
/// would split one file into two entries on a case-insensitive filesystem and break the partition
/// for no visible reason.
///
/// `search_paths` must already have `path`'s parent prepended if that is wanted:
/// `ModuleSearchPath::resolve` iterates only its own directories (`modules.rs:670-699`) and never
/// the including file's. This mirrors `resolve_modules` exactly and deliberately -- a walk that
/// found files the resolver could not would produce a file set larger than the merge, and the
/// partition would fail for a reason the user cannot see.
///
/// Never fails. An unresolvable or unreadable module becomes a diagnostic on the directive line
/// that named it, which is what puts a missing include on the right inner tab instead of failing
/// the whole chain. A cycle terminates at the dedupe check, so the recursion is bounded by the
/// number of distinct files; `resolve_modules` is what reports the cycle itself.
///
/// [`Chain::attributed`] is false on return -- call [`Chain::locate`] with the merged source.
pub fn walk(path: &Path, search_paths: &[PathBuf]) -> Chain {
    let mut chain = Chain {
        files: Vec::new(),
        attributed: false,
        diagnostics: Vec::new(),
    };
    let search_path = ModuleSearchPath::new(search_paths.to_vec());
    let mut seen = HashSet::new();
    match std::fs::read_to_string(path) {
        Ok(text) => visit(
            path.to_path_buf(),
            text,
            0,
            None,
            &search_path,
            &mut seen,
            &mut chain,
        ),
        Err(error) => chain.diagnostics.push(
            Diagnostic::error(
                Source::CddlModules,
                0..0,
                format!("reading `{}`: {error}", path.display()),
            )
            .in_file(path),
        ),
    }
    chain
}

fn visit(
    path: PathBuf,
    text: String,
    depth: usize,
    parent: Option<usize>,
    search_path: &ModuleSearchPath,
    seen: &mut HashSet<String>,
    chain: &mut Chain,
) {
    seen.insert(canonical_key(&path));

    let (found, _starts, _constrains, warnings) = scan_directives(&text);
    chain.diagnostics.extend(warnings.iter().map(|warning| {
        Diagnostic::new(
            Severity::from(warning.level.clone()),
            Source::CddlModules,
            warning.span.start..warning.span.end,
            warning.message.clone(),
        )
        .in_file(&path)
    }));

    let index = chain.files.len();
    chain.files.push(ChainFile {
        path: path.clone(),
        text,
        depth,
        parent,
        range: 0..0,
    });

    for directive in &found {
        let span = directive.span.start..directive.span.end;
        let target = match search_path.resolve(&directive.module, directive.span.clone()) {
            Ok(target) => target,
            Err(error) => {
                chain.diagnostics.push(
                    Diagnostic::error(Source::CddlModules, span, error.to_string()).in_file(&path),
                );
                continue;
            }
        };
        if seen.contains(&canonical_key(&target)) {
            continue;
        }
        match std::fs::read_to_string(&target) {
            Ok(child) => visit(
                target,
                child,
                depth + 1,
                Some(index),
                search_path,
                seen,
                chain,
            ),
            Err(error) => chain.diagnostics.push(
                Diagnostic::error(
                    Source::CddlModules,
                    span,
                    format!("reading `{}`: {error}", target.display()),
                )
                .in_file(&path),
            ),
        }
    }
}

/// Partition `merged` into the regions contributed by `files`.
///
/// Pure -- no filesystem -- so the whole attribution rule is unit-testable from string literals.
///
/// Scans `merged` from offset 0, at each position taking the file whose text begins there and
/// advancing by its length. Returns one range per entry in `files`, in the same order, and whether
/// the partition was *proved*: true only if the scan consumed `merged` exactly and every non-empty
/// file text was placed.
///
/// # Degradation
///
/// The flag going false is the design, not a failure mode to paper over -- the alternative is
/// naming the wrong file, which looks authoritative. It happens when:
///
/// * no file's text begins at some position (a separator, a header, or rewritten text), or
/// * the files matching at a position have *differing* texts, meaning one file's content is a
///   prefix of another's and the merge is not a clean partition, or
/// * the scan leaves a tail, or a non-empty file never appeared in the merge.
///
/// Two *distinct* files with byte-identical content are the one case handled more gently: they
/// cannot be told apart, so each such region is left unlocated (an empty range) while the scan
/// still consumes it. Attribution for those files is unavailable; every other file in the chain
/// keeps an exact region.
pub fn partition(merged: &str, files: &[(PathBuf, String)]) -> (Vec<Range<usize>>, bool) {
    let mut ranges = vec![0..0; files.len()];
    // Only non-empty texts contribute bytes. A zero-length module is left unlocated: it has no
    // region to find, and admitting it as a candidate would match at every position.
    let mut remaining: Vec<usize> = (0..files.len())
        .filter(|&index| !files[index].1.is_empty())
        .collect();

    // Two distinct files with identical content cannot be told apart, so *neither* may be
    // located -- not even the second occurrence, where the first has already been consumed and
    // the match looks unique. Which of the two sits in which region is a coin toss, and a coin
    // toss is exactly the silent mis-attribution this whole module exists to rule out.
    let shared: Vec<bool> = (0..files.len())
        .map(|index| {
            files
                .iter()
                .enumerate()
                .any(|(other, (_, text))| other != index && *text == files[index].1)
        })
        .collect();

    let mut pos = 0usize;
    while pos < merged.len() {
        let rest = &merged[pos..];
        let matching: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|&index| rest.starts_with(files[index].1.as_str()))
            .collect();

        let Some(&first) = matching.first() else {
            return (ranges, false);
        };
        let text = files[first].1.as_str();
        if matching
            .iter()
            .any(|&index| files[index].1.as_str() != text)
        {
            // One file's content is a prefix of another's: not a clean partition.
            return (ranges, false);
        }
        if matching.len() == 1 && !shared[first] {
            ranges[first] = pos..pos + text.len();
        }
        // A tied or shared region stays unlocated, but exactly one candidate is consumed so a
        // second identical region later in the merge still matches rather than failing the scan.
        remaining.retain(|&index| index != first);
        pos += text.len();
    }

    (ranges, remaining.is_empty())
}

/// A `;#` directive and the file it names, for the open-include gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveRef {
    /// The directive line, from `;#` to end of line, in its own file's coordinates.
    pub span: Range<usize>,
    /// The bare module name as written, with no `.cddl` suffix.
    pub module: String,
    /// The resolved file, or `None` when the module does not resolve.
    pub target: Option<PathBuf>,
}

/// Every `;# import` / `;# include` in `text`, with its resolved target.
///
/// `search_paths` is used exactly as [`walk`] uses it, so a directive this reports as unresolvable
/// is one `resolve_modules` would also fail on -- the context menu and the diagnostics agree.
pub fn directives(text: &str, search_paths: &[PathBuf]) -> Vec<DirectiveRef> {
    let search_path = ModuleSearchPath::new(search_paths.to_vec());
    let (found, _, _, _) = scan_directives(text);
    found
        .into_iter()
        .map(|directive| DirectiveRef {
            span: directive.span.start..directive.span.end,
            target: search_path
                .resolve(&directive.module, directive.span.clone())
                .ok(),
            module: directive.module,
        })
        .collect()
}

/// The directive whose text contains `offset`, if any.
///
/// Drives the editor's "Open included file" context-menu item, so containment is over the
/// directive's own span -- `;#` through the last character before the line terminator. An offset
/// at `span.end` is the newline and belongs to no directive.
pub fn directive_at(text: &str, offset: usize, search_paths: &[PathBuf]) -> Option<DirectiveRef> {
    directives(text, search_paths)
        .into_iter()
        .find(|directive| directive.span.contains(&offset))
}

/// The key `tps_cddl`'s loader deduplicates modules on (`modules.rs:781-787`): the canonical path,
/// lowercased, so `Foo.cddl` and `foo.cddl` share one entry on a case-insensitive filesystem.
///
/// A path that cannot be canonicalized -- deleted since, or never written -- falls back to its
/// literal spelling rather than failing, matching [`crate::doc::CddlDocSet::index_of`].
fn canonical_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed corpus: `gp_eap.cddl` includes `rfc9711`, which includes `rfc8392`, which
    /// includes `rfc9052`. Four files, three levels of nesting.
    const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples");

    /// An incomplete copy of the corpus: its `rfc8392.cddl:4` names an `rfc9052` that is not there.
    const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/sample_project");

    fn examples() -> Vec<PathBuf> {
        vec![PathBuf::from(EXAMPLES)]
    }

    fn gp_eap() -> PathBuf {
        PathBuf::from(EXAMPLES).join("gp_eap.cddl")
    }

    fn names(chain: &Chain) -> Vec<String> {
        chain
            .files
            .iter()
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn entries(files: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
        files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
            .collect()
    }

    /// The crate's temp-dir convention, as `doc.rs:670` and `keystore.rs:413` use it: no
    /// `tempfile` dev-dependency, and the pid keeps parallel test binaries apart.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps-cbor-chain-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn the_opened_file_is_entry_zero() {
        let chain = walk(&gp_eap(), &examples());
        assert_eq!(chain.files[0].path, gp_eap());
        assert_eq!(chain.files[0].depth, 0);
        assert_eq!(chain.files[0].parent, None);
    }

    /// Pins the depth-first pre-order the inner tab bar's ordering depends on.
    #[test]
    fn every_file_in_the_chain_appears_exactly_once() {
        let chain = walk(&gp_eap(), &examples());
        assert_eq!(
            names(&chain),
            vec![
                "gp_eap.cddl",
                "rfc9711.cddl",
                "rfc8392.cddl",
                "rfc9052.cddl"
            ],
            "{:?}",
            chain.diagnostics
        );
        assert_eq!(
            chain.files.iter().map(|f| f.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(
            chain.files.iter().map(|f| f.parent).collect::<Vec<_>>(),
            vec![None, Some(0), Some(1), Some(2)]
        );
        let errors: Vec<_> = chain
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    /// The self-check, over the real four-deep chain: the reconstructed partition must reproduce
    /// `tps_cddl`'s merged source byte for byte. The single most important test in the phase --
    /// every attributed diagnostic rests on it.
    #[test]
    fn the_partition_reconstructs_the_merged_source_exactly() {
        let mut chain = walk(&gp_eap(), &examples());
        let resolved = crate::cddl::resolve(&gp_eap(), &examples(), &tps_cddl::Config::all())
            .expect("the committed chain resolves");
        chain.locate(&resolved.source);

        assert!(
            chain.attributed,
            "the partition did not prove: {:?}",
            chain.files.iter().map(|f| &f.range).collect::<Vec<_>>()
        );
        let total: usize = chain.files.iter().map(|f| f.text.len()).sum();
        assert_eq!(total, resolved.source.len());
        for file in &chain.files {
            assert_eq!(
                &resolved.source[file.range.clone()],
                file.text,
                "{} was located at the wrong region",
                file.path.display()
            );
        }
    }

    /// The `UI_DESIGN.md:112` requirement, end to end: a span in the merged source lands on the
    /// file it came from, at that file's own offset.
    #[test]
    fn a_span_in_an_included_file_is_attributed_to_that_file() {
        let mut chain = walk(&gp_eap(), &examples());
        let resolved = crate::cddl::resolve(&gp_eap(), &examples(), &tps_cddl::Config::all())
            .expect("the committed chain resolves");
        chain.locate(&resolved.source);

        let last = chain.files.len() - 1;
        let region = chain.files[last].range.clone();
        let offset = region.start + 5;
        let (index, local) = chain.attribute(offset..offset + 3).expect("attributed");
        assert_eq!(index, last);
        assert_eq!(local, 5..8);
        assert_eq!(
            &chain.files[last].text[local],
            &resolved.source[offset..offset + 3]
        );
    }

    #[test]
    fn an_unattributed_chain_rebases_nothing() {
        let mut chain = walk(&gp_eap(), &examples());
        chain.locate("not the merged source");
        assert!(!chain.attributed);
        assert!(chain.attribute(0..1).is_none());
    }

    /// Guards the safety property: a merge this code does not understand must degrade visibly, not
    /// produce a wrong file name.
    #[test]
    fn a_merged_source_with_an_injected_separator_degrades_rather_than_guessing() {
        let files = entries(&[("a.cddl", "a = uint\n"), ("b.cddl", "b = tstr\n")]);
        let (_, clean) = partition("a = uint\nb = tstr\n", &files);
        assert!(clean, "the uncorrupted merge must prove");

        let (ranges, attributed) = partition("a = uint\n; ---\nb = tstr\n", &files);
        assert!(!attributed);
        assert!(ranges[1].is_empty(), "nothing may be located past the gap");
    }

    #[test]
    fn a_trailing_byte_in_the_merge_is_not_proved() {
        let files = entries(&[("a.cddl", "a = uint\n")]);
        assert!(!partition("a = uint\n\n", &files).1);
    }

    #[test]
    fn a_file_missing_from_the_merge_is_not_proved() {
        let files = entries(&[("a.cddl", "a = uint\n"), ("b.cddl", "b = tstr\n")]);
        assert!(!partition("a = uint\n", &files).1);
    }

    /// The one ambiguity the scan admits. Both identical files lose their region; the third file
    /// keeps an exact one, and the merge still proves.
    #[test]
    fn two_files_with_identical_content_degrade_only_their_own_region() {
        let files = entries(&[
            ("a.cddl", "x = uint\n"),
            ("b.cddl", "x = uint\n"),
            ("c.cddl", "c = tstr\n"),
        ]);
        let (ranges, attributed) = partition("x = uint\nx = uint\nc = tstr\n", &files);
        assert!(attributed, "the merge is still fully consumed");
        assert!(ranges[0].is_empty());
        assert!(ranges[1].is_empty());
        assert_eq!(ranges[2], 18..27);
    }

    /// A file whose content is a *prefix* of another's is not a clean partition, so it degrades
    /// rather than guessing which one the region belongs to.
    #[test]
    fn a_file_whose_text_prefixes_another_is_not_proved() {
        let files = entries(&[("a.cddl", "a = uint\n"), ("b.cddl", "a = uint\nb = tstr\n")]);
        assert!(!partition("a = uint\nb = tstr\n", &files).1);
    }

    #[test]
    fn a_zero_length_module_is_left_unlocated_without_failing_the_scan() {
        let files = entries(&[("a.cddl", "a = uint\n"), ("empty.cddl", "")]);
        let (ranges, attributed) = partition("a = uint\n", &files);
        assert!(attributed);
        assert_eq!(ranges[0], 0..9);
        assert!(ranges[1].is_empty());
    }

    /// A missing include lands on its own directive line, in its own file -- which is what puts it
    /// on the right inner tab instead of failing the whole chain.
    #[test]
    fn a_missing_include_is_a_diagnostic_on_its_own_directive_line() {
        let root = PathBuf::from(SAMPLE).join("rfc8392.cddl");
        let chain = walk(&root, &[PathBuf::from(SAMPLE)]);

        assert_eq!(names(&chain), vec!["rfc8392.cddl"]);
        let error = chain
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .expect("the absent rfc9052 is an error");
        assert_eq!(error.file.as_deref(), Some(root.as_path()));
        let (start, _) = error.position(&chain.files[0].text);
        assert_eq!(start.line, 4, "reported line {}", start.line);
    }

    /// A cycle terminates at the dedupe check rather than recursing: `walk` is not where a cycle
    /// is reported, it is where a cycle must not hang.
    #[test]
    fn a_circular_include_terminates_the_walk() {
        let dir = temp_dir("circular");
        std::fs::write(dir.join("a.cddl"), ";# include b\na = uint\n").expect("write a");
        std::fs::write(dir.join("b.cddl"), ";# include a\nb = tstr\n").expect("write b");

        let chain = walk(&dir.join("a.cddl"), std::slice::from_ref(&dir));
        assert_eq!(names(&chain), vec!["a.cddl", "b.cddl"]);
    }

    #[test]
    fn the_chain_is_deduped_case_insensitively_like_the_loader() {
        let chain = walk(&gp_eap(), &examples());
        assert_eq!(chain.files.len(), 4);

        // `index_of` is the same key, so a case-varied spelling finds the same entry on a
        // case-insensitive filesystem and falls back to the literal compare elsewhere.
        assert_eq!(chain.index_of(&gp_eap()), Some(0));
        assert_eq!(
            chain.index_of(&PathBuf::from(EXAMPLES).join("rfc9052.cddl")),
            Some(3)
        );
    }

    #[test]
    fn index_of_rejects_a_file_outside_the_chain() {
        let chain = walk(&gp_eap(), &examples());
        assert_eq!(
            chain.index_of(&PathBuf::from(EXAMPLES).join("rfc9052_constrained_payload.cddl")),
            None
        );
    }

    #[test]
    fn a_bare_module_name_resolves_through_the_search_paths_in_order() {
        let dir = temp_dir("search-order");
        std::fs::write(dir.join("root.cddl"), ";# include rfc9052\nr = uint\n")
            .expect("write root");

        // `dir` has no `rfc9052.cddl`, so resolution must fall through to `examples/`.
        let chain = walk(
            &dir.join("root.cddl"),
            &[dir.clone(), PathBuf::from(EXAMPLES)],
        );
        assert_eq!(names(&chain), vec!["root.cddl", "rfc9052.cddl"]);
        assert_eq!(chain.files[1].path.parent(), Some(Path::new(EXAMPLES)));
    }

    #[test]
    fn a_directive_is_found_from_an_offset_anywhere_on_its_line() {
        let text = std::fs::read_to_string(gp_eap()).expect("read gp_eap.cddl");
        let found = directives(&text, &examples());
        let include = found
            .iter()
            .find(|d| d.module == "rfc9711")
            .expect("gp_eap.cddl includes rfc9711");
        assert_eq!(
            include.target.as_deref(),
            Some(PathBuf::from(EXAMPLES).join("rfc9711.cddl").as_path())
        );

        for offset in include.span.clone() {
            assert_eq!(
                directive_at(&text, offset, &examples()).as_ref(),
                Some(include),
                "offset {offset} is inside the directive"
            );
        }
        // One byte past the end is the line terminator, which belongs to no directive.
        assert!(directive_at(&text, include.span.end, &examples()).is_none());
        assert!(directive_at(&text, 0, &examples()).is_none());
    }

    #[test]
    fn an_unresolvable_directive_reports_no_target_rather_than_failing() {
        let found = directives(";# include nothing_here\na = uint\n", &examples());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].module, "nothing_here");
        assert_eq!(found[0].target, None);
    }
}
