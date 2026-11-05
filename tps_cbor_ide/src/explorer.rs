/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The left pane: a directory tree over the workspace root.
//!
//! `UI_DESIGN.md:55-56` asks for three things, and all three are here: the content does not depend
//! on the selected tab, open files are bold, and edited files are bold red. Clicking a file asks
//! the application to open it — [`ExplorerEvent::Open`] — and the application decides which tab
//! that means, since the explorer has no opinion about file kinds.
//!
//! Three things about gpui-component's tree shape this module, none of them documented upstream:
//!
//! 1. **The tree has no selection event.** `TreeEvent` carries only `Expanded`/`Collapsed`
//!    (`gpui-base/src/tree.rs:93`), and `set_selected_index` merely notifies. So the "open this
//!    file" click is ours: [`Explorer`] adds an `on_click` to the row content it renders. The base
//!    tree wraps that content in its own click handler which selects and toggles, and it documents
//!    that the content's handlers run first (`gpui-base/src/tree.rs:497`), so both fire and the
//!    selection still moves.
//! 2. **The row content owns indentation and the disclosure triangle.** The base tree renders a
//!    `uniform_list` of bare rows and passes the depth through `TreeEntry::depth()`; nothing is
//!    indented and no chevron is drawn for you.
//! 3. **`is_folder()` is `!children.is_empty()`** (`gpui-base/src/tree.rs:132`). A directory is a
//!    folder to the tree only once it has children, which is why [`Explorer`] keeps its own set of
//!    directory ids: an *empty* directory must still look like a directory even though it can
//!    neither expand nor hold anything. It is also why the scan is eager — see [`scan`].

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use tps_cbor_core::Workspace;

/// How deep the scan descends below the root.
///
/// A limit is needed because the scan is eager; eight levels covers every fixture tree in this
/// repository several times over, and a tree deeper than this is not navigable in a 200 px pane
/// anyway.
const MAX_DEPTH: usize = 8;

/// How many entries the scan will collect in total.
///
/// Guards against a root pointed at a home directory: the cost of being wrong is a stall at
/// startup, which a budget turns into a truncated tree instead.
const MAX_ENTRIES: usize = 20_000;

/// Directory names skipped regardless of depth.
///
/// Everything beginning with `.` is skipped too, which covers `.git`; these are the build outputs
/// that are large, uninteresting, and present in this very repository.
const SKIP: [&str; 2] = ["target", "node_modules"];

/// What the explorer asks the application to do.
///
/// Opening is the application's decision, not the explorer's: which tab a file belongs in depends
/// on its content and on `Workspace`, neither of which this module reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerEvent {
    /// A file row was clicked.
    Open(PathBuf),
}

/// One scanned directory entry, before it becomes a [`TreeItem`].
///
/// A separate type because `TreeItem` loses the distinction this module needs — see the module
/// docs' point 3 — and because a plain tree of `PathBuf`s is testable without a window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    path: PathBuf,
    name: String,
    is_dir: bool,
    children: Vec<Node>,
}

/// The left pane.
pub struct Explorer {
    tree: Entity<TreeState>,
    root: Option<PathBuf>,
    /// Ids of every directory in the tree, including the empty ones.
    dirs: Rc<HashSet<SharedString>>,
    /// Ids of the open files (`UI_DESIGN.md:56`: bold).
    open: Rc<HashSet<SharedString>>,
    /// Ids of the files with unsaved changes (`UI_DESIGN.md:56`: bold red).
    dirty: Rc<HashSet<SharedString>>,
    /// Ids the user has expanded.
    ///
    /// Held here rather than read back off the tree because `set_items` rebuilds every `TreeItem`,
    /// and expansion lives *in* the item (`Rc<RefCell<TreeItemState>>`). Without this, a rescan
    /// after an external change would collapse the whole tree under the user.
    expanded: HashSet<SharedString>,
    _tree_events: Subscription,
}

impl EventEmitter<ExplorerEvent> for Explorer {}

impl Explorer {
    /// An explorer with no root, which renders as an empty pane.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let subscription = cx.subscribe(&tree, |this, _, event: &TreeEvent, _| match event {
            TreeEvent::Expanded(id) => {
                this.expanded.insert(id.clone());
            }
            TreeEvent::Collapsed(id) => {
                this.expanded.remove(id);
            }
        });

        Self {
            tree,
            root: None,
            dirs: Rc::default(),
            open: Rc::default(),
            dirty: Rc::default(),
            expanded: HashSet::new(),
            _tree_events: subscription,
        }
    }

    /// Brings the tree into line with `workspace`.
    ///
    /// Cheap enough to call on every change the application notices: it rescans the disk only when
    /// the root itself changed, and otherwise just recomputes which rows are bold.
    ///
    /// Takes the *entity* rather than a `&Workspace` because Phase 3 moved the model behind one,
    /// so that the hex view can read the buffer during paint. A `&Workspace` here would hold a
    /// borrow of the `App` across the `&mut Context` calls below and not compile; reading it once
    /// per statement does, and each read is a pointer hop.
    pub fn sync(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        let root = workspace.read(cx).root.clone();
        if self.root != root {
            self.set_root(root, cx);
        }

        self.open = Rc::new(
            workspace
                .read(cx)
                .open_paths()
                .into_iter()
                .map(id)
                .collect(),
        );
        self.dirty = Rc::new(
            workspace
                .read(cx)
                .dirty_paths()
                .into_iter()
                .map(id)
                .collect(),
        );
        cx.notify();
    }

    /// Shows `root`, discarding any expansion state from the previous one.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.expanded.clear();
        // The root row starts expanded: a tree whose only visible row is the folder you just
        // chose is not a useful first impression.
        if let Some(root) = root.as_deref() {
            self.expanded.insert(id(root));
        }
        self.root = root;
        self.rescan(cx);
    }

    /// Re-reads the tree from disk, preserving which rows are expanded.
    ///
    /// Phase 2 has no watcher, so this is called on an explicit refresh and after a save. The
    /// plan's `notify` integration for external change detection is Phase 8 work; see `PLAN.md`.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            self.dirs = Rc::default();
            self.tree
                .update(cx, |state, cx| state.set_items(Vec::new(), cx));
            cx.notify();
            return;
        };

        let children = scan(&root);
        let mut dirs = HashSet::new();
        dirs.insert(id(&root));
        collect_dirs(&children, &mut dirs);
        self.dirs = Rc::new(dirs);

        let root_item = TreeItem::new(id(&root), display_name(&root))
            .children(items(&children, &self.expanded))
            .expanded(self.expanded.contains(&id(&root)));
        self.tree
            .update(cx, |state, cx| state.set_items(vec![root_item], cx));
        cx.notify();
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Cloned per frame rather than per row: the render closure outlives this call, so it
        // cannot borrow `self`, and an `Rc` bump is the cheapest way to let every row share one
        // copy of the three lookup sets.
        let dirs = self.dirs.clone();
        let open = self.open.clone();
        let dirty = self.dirty.clone();
        let explorer = cx.entity();

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(match self.root.as_deref() {
                        Some(root) => display_name(root),
                        None => "No folder open".into(),
                    }),
            )
            .child(
                tree(&self.tree, move |ix, entry, _selected, _, cx| {
                    let item = entry.item();
                    let row_id = item.id.clone();
                    let is_dir = dirs.contains(&row_id);
                    let is_dirty = dirty.contains(&row_id);
                    let is_open = is_dirty || open.contains(&row_id);

                    let icon = if is_dir {
                        if entry.is_expanded() {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        }
                    } else {
                        IconName::File
                    };

                    // An empty directory gets the gap but no triangle: the base tree will not
                    // toggle it, and offering a control that does nothing is worse than alignment
                    // that is merely consistent.
                    let chevron = match (is_dir, entry.is_folder(), entry.is_expanded()) {
                        (true, true, true) => Some(IconName::ChevronDown),
                        (true, true, false) => Some(IconName::ChevronRight),
                        _ => None,
                    };

                    let row = h_flex()
                        .gap_1()
                        .pl(px(entry.depth() as f32 * 12.0))
                        .child(match chevron {
                            Some(chevron) => Icon::new(chevron)
                                .size_3()
                                .text_color(cx.theme().muted_foreground)
                                .into_any_element(),
                            None => div().size_3().into_any_element(),
                        })
                        .child(Icon::new(icon).size_3p5().text_color(if is_dir {
                            cx.theme().muted_foreground
                        } else {
                            cx.theme().foreground
                        }))
                        .child(
                            div()
                                .when(is_open, |this| this.font_weight(FontWeight::BOLD))
                                // Dirty beats open: a file that is both is the one the user needs
                                // to see (`UI_DESIGN.md:56`).
                                .when(is_dirty, |this| this.text_color(cx.theme().danger))
                                .child(item.label.clone()),
                        );

                    let list_item = ListItem::new(ix).child(row);
                    if is_dir {
                        // Expansion is the base tree's own click handler; adding ours would only
                        // ask the application to open a directory.
                        return list_item;
                    }

                    let explorer = explorer.clone();
                    let path = PathBuf::from(row_id.to_string());
                    list_item.on_click(move |_, _, cx| {
                        let path = path.clone();
                        explorer.update(cx, |_, cx| cx.emit(ExplorerEvent::Open(path)));
                    })
                })
                .flex_1(),
            )
    }
}

/// A row's id: its path, which is what [`ExplorerEvent::Open`] has to carry back.
///
/// `to_string_lossy` is not reversible for a non-UTF-8 path, so such a file cannot be opened from
/// the tree. Worth knowing, not worth a second id table: every fixture and every file this
/// application can usefully open has a UTF-8 name.
fn id(path: &Path) -> SharedString {
    path.to_string_lossy().into_owned().into()
}

/// The label for a row: the file name, or the whole path for a root with none.
fn display_name(path: &Path) -> SharedString {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned().into(),
        None => path.to_string_lossy().into_owned().into(),
    }
}

/// Reads the tree below `root`, eagerly.
///
/// Eager rather than filled in on `TreeEvent::Expanded`, because an unvisited directory would have
/// no children and therefore would not be a folder at all — no triangle, no way to expand it, no
/// way to discover that it has contents (module docs, point 3). Making it work lazily would mean
/// giving every directory a placeholder child, which is a visible fake row. Bounded by
/// [`MAX_DEPTH`] and [`MAX_ENTRIES`] instead.
///
/// Unreadable directories are skipped silently: a permission error on one subdirectory should not
/// cost the user the rest of the tree.
fn scan(root: &Path) -> Vec<Node> {
    let mut budget = MAX_ENTRIES;
    scan_dir(root, MAX_DEPTH, &mut budget)
}

/// Reads one directory, recursing while `depth_left` and `budget` allow.
fn scan_dir(dir: &Path, depth_left: usize, budget: &mut usize) -> Vec<Node> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut nodes = Vec::new();
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        // `file_type` does not follow symlinks, so a symlinked directory reads as a file. That is
        // deliberate: it makes a cycle impossible without a visited-inode set.
        let Ok(kind) = entry.file_type() else {
            continue;
        };

        *budget -= 1;
        let path = entry.path();
        let is_dir = kind.is_dir();
        let children = if is_dir && depth_left > 1 {
            scan_dir(&path, depth_left - 1, budget)
        } else {
            Vec::new()
        };
        nodes.push(Node {
            path,
            name,
            is_dir,
            children,
        });
    }

    // Directories first, then case-insensitively by name — the ordering every file explorer uses,
    // and `read_dir` makes no promise of its own.
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    nodes
}

/// Converts scanned nodes into tree items, restoring expansion from `expanded`.
fn items(nodes: &[Node], expanded: &HashSet<SharedString>) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| {
            let row_id = id(&node.path);
            let is_expanded = expanded.contains(&row_id);
            TreeItem::new(row_id, node.name.clone())
                .children(items(&node.children, expanded))
                .expanded(is_expanded)
        })
        .collect()
}

/// Collects the ids of every directory, so the renderer can tell one from a file.
fn collect_dirs(nodes: &[Node], into: &mut HashSet<SharedString>) {
    for node in nodes {
        if node.is_dir {
            into.insert(id(&node.path));
        }
        collect_dirs(&node.children, into);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A scratch directory, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "cbor-ide-explorer-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create the scratch directory");
            Self(path)
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(&path).expect("create a directory");
            path
        }

        fn file(&self, rel: &str) {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create a parent directory");
            }
            fs::write(&path, b"").expect("create a file");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(nodes: &[Node]) -> Vec<&str> {
        nodes.iter().map(|node| node.name.as_str()).collect()
    }

    /// Directories before files, then case-insensitively. `read_dir` order is whatever the
    /// filesystem gives, so without the sort the tree's order changes between machines.
    #[test]
    fn directories_sort_before_files_and_case_does_not_matter() {
        let scratch = Scratch::new();
        scratch.file("zebra.cddl");
        scratch.file("Apple.cbor");
        scratch.dir("zoo");
        scratch.dir("Barn");

        assert_eq!(
            names(&scan(&scratch.0)),
            ["Barn", "zoo", "Apple.cbor", "zebra.cddl"]
        );
    }

    /// Hidden entries and build outputs are skipped. `target` matters most: this repository has a
    /// multi-gigabyte one, and scanning it eagerly would stall startup for minutes.
    #[test]
    fn hidden_entries_and_build_outputs_are_skipped() {
        let scratch = Scratch::new();
        scratch.file("keep.cbor");
        scratch.file(".hidden");
        scratch.dir(".git");
        scratch.file("target/debug/thing");
        scratch.file("node_modules/pkg/index.js");

        assert_eq!(names(&scan(&scratch.0)), ["keep.cbor"]);
    }

    /// The depth limit truncates rather than failing, and the truncation is invisible in the only
    /// way that matters: a directory at the limit has no children and so cannot be expanded.
    #[test]
    fn the_depth_limit_truncates_the_scan() {
        let scratch = Scratch::new();
        scratch.file("a/b/c/deep.cbor");

        let mut budget = MAX_ENTRIES;
        let shallow = scan_dir(&scratch.0, 2, &mut budget);
        assert_eq!(names(&shallow), ["a"]);
        assert_eq!(names(&shallow[0].children), ["b"]);
        assert!(
            shallow[0].children[0].children.is_empty(),
            "the scan must stop at the depth limit"
        );

        let full = scan(&scratch.0);
        assert_eq!(names(&full[0].children[0].children), ["c"]);
    }

    /// The entry budget is a total, not a per-directory one, so a pathological root truncates
    /// instead of stalling.
    #[test]
    fn the_entry_budget_is_spent_across_the_whole_tree() {
        let scratch = Scratch::new();
        for ix in 0..5 {
            scratch.file(&format!("file{ix}.cbor"));
        }

        let mut budget = 3;
        let nodes = scan_dir(&scratch.0, MAX_DEPTH, &mut budget);
        assert_eq!(nodes.len(), 3);
        assert_eq!(budget, 0);
    }

    /// An empty directory is still a directory, even though the tree will not call it a folder.
    /// This set is the only thing that lets the renderer draw it with a folder icon.
    #[test]
    fn an_empty_directory_is_still_recorded_as_a_directory() {
        let scratch = Scratch::new();
        let empty = scratch.dir("keys");

        let nodes = scan(&scratch.0);
        let mut dirs = HashSet::new();
        collect_dirs(&nodes, &mut dirs);

        assert!(dirs.contains(&id(&empty)));
        let item = &items(&nodes, &HashSet::new())[0];
        assert!(
            !item.is_folder(),
            "an empty directory has no children, so the tree does not treat it as a folder -- \
             which is what `dirs` exists to compensate for"
        );
    }

    /// Row ids are paths, because [`ExplorerEvent::Open`] has to carry one back and the tree hands
    /// the renderer nothing else that identifies a row.
    #[test]
    fn a_rows_id_is_its_path() {
        let scratch = Scratch::new();
        scratch.file("dir/thing.cbor");

        let nodes = scan(&scratch.0);
        let items = items(&nodes, &HashSet::new());
        let dir = &items[0];
        assert_eq!(dir.id, id(&scratch.0.join("dir")));
        assert_eq!(dir.children[0].id, id(&scratch.0.join("dir/thing.cbor")));
        assert_eq!(
            PathBuf::from(dir.children[0].id.to_string()),
            scratch.0.join("dir/thing.cbor")
        );
    }

    /// Expansion survives a rescan. `set_items` rebuilds every item and expansion lives in the
    /// item, so without the explorer's own `expanded` set an external change would collapse the
    /// tree under the user mid-edit.
    #[test]
    fn expansion_is_restored_from_the_explorers_own_set() {
        let scratch = Scratch::new();
        scratch.file("dir/thing.cbor");
        let nodes = scan(&scratch.0);

        let collapsed = items(&nodes, &HashSet::new());
        assert!(!collapsed[0].is_expanded());

        let expanded = items(&nodes, &HashSet::from([id(&scratch.0.join("dir"))]));
        assert!(expanded[0].is_expanded());
        assert!(
            !expanded[0].children[0].is_expanded(),
            "only the named row is expanded"
        );
    }
}
