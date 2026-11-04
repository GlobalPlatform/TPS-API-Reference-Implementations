/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The document model.
//!
//! This is the part of the old application that did not exist. The Tauri backend held no state at
//! all -- no `tauri::State`, no document type (`tps_cbor_gui/src-tauri/src/main.rs:1023-1046`) --
//! so the whole model lived in React `useState` (`src/App.tsx:45-104`) and every CBOR buffer
//! crossed the IPC boundary as a JSON array of numbers. Dirty tracking, read-only state and the
//! "same base name?" rule were each reimplemented per panel, and inconsistently.
//!
//! # Shape
//!
//! `UI_DESIGN.md:57` fixes the shape: *"There can be up to four open files: a binary CBOR file, an
//! EDN file, a CDDL file and a COSE artifact."* So a [`Workspace`] holds exactly one document of
//! each kind, not a list -- opening a second CBOR file replaces the first. The CDDL tab is the
//! stated exception (`UI_DESIGN.md:108`: *"this is the only window which allows multiple files to
//! be opened"*), so [`CddlDocSet`] holds the include chain, each entry with its own path, dirty
//! flag and read-only flag, because `UI_DESIGN.md:116` requires read-only state be *per inner tab*.
//!
//! # What is modelled here and what is not
//!
//! These types own content, identity, and the two flags. They do **not** own derived state --
//! diagnostics, the span map, the parsed AST, the trace. That is deliberate: derived state is
//! invalidated by editing, and a model that stored it would have to be trusted to invalidate it
//! everywhere. The frontend recomputes from [`crate::cddl`], [`crate::spans`] and friends, which
//! are cheap enough (`gp_eap.cddl` parses in ~1 ms) that caching is not worth the correctness risk.
//!
//! # Read-only means read-only
//!
//! Every mutating method checks [`Meta::read_only`] and returns [`CoreError::ReadOnly`] rather
//! than relying on the UI to grey out a control. `UI_DESIGN.md:53` requires Paste to be inoperative
//! on a read-only document, and a model that enforces it cannot be bypassed by a keyboard shortcut
//! that forgot to check.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::diag::{Diagnostic, Source};
use crate::error::{CoreError, Result};

/// Identity and status common to every open document.
///
/// Kept as one struct rather than three repeated fields so that "has a path", "is dirty" and
/// "is read-only" have exactly one implementation each, and so [`CddlDocSet`]'s per-inner-tab
/// copies are the same type as the top-level ones.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    /// Where this document came from, and where `Save` writes. `None` for a document produced by
    /// a conversion rather than opened, which is what greys out `File -> Save`
    /// (`UI_DESIGN.md:66`) and forces `Save As`.
    pub path: Option<PathBuf>,
    /// Whether the content has changed since it was last read or written.
    ///
    /// Drives the explorer's bold-red rendering (`UI_DESIGN.md:56`) and the quit confirmation.
    pub dirty: bool,
    /// Whether editing is refused. `true` by default for every kind, per `UI_DESIGN.md`.
    pub read_only: bool,
}

impl Meta {
    /// A read-only, clean document with no path.
    pub fn new() -> Self {
        Self {
            path: None,
            dirty: false,
            read_only: true,
        }
    }

    /// A read-only, clean document at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            dirty: false,
            read_only: true,
        }
    }

    /// Whether `File -> Save` can proceed without asking for a name (`UI_DESIGN.md:66`).
    pub fn can_save(&self) -> bool {
        self.path.is_some()
    }

    /// The file name for display on a tab, or `"(untitled)"`.
    pub fn title(&self) -> &str {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .unwrap_or("(untitled)")
    }

    /// The file name without its extension, for the base-name comparison described on
    /// [`Workspace::cbor_path_after_conversion_from_edn`].
    pub fn base_name(&self) -> Option<&str> {
        self.path
            .as_deref()
            .and_then(Path::file_stem)
            .and_then(OsStr::to_str)
    }

    /// Fail unless this document accepts edits.
    pub fn writable(&self) -> Result<()> {
        if self.read_only {
            Err(CoreError::ReadOnly {
                path: self.path.clone(),
            })
        } else {
            Ok(())
        }
    }
}

/// The CBOR binary document: the byte buffer behind the hex view.
#[derive(Debug, Clone, Default)]
pub struct CborDoc {
    /// Identity and status.
    pub meta: Meta,
    bytes: Vec<u8>,
}

impl CborDoc {
    /// An empty, read-only document.
    pub fn new() -> Self {
        Self {
            meta: Meta::new(),
            bytes: Vec::new(),
        }
    }

    /// Read `path` as raw bytes.
    ///
    /// Deliberately does **not** reject malformed CBOR. The hex editor's whole purpose is to let
    /// the user look at and repair a broken file, so well-formedness is a diagnostic
    /// ([`crate::convert::check_well_formed`]), not an open failure. The old GUI agreed by
    /// accident; this makes it the contract.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|error| CoreError::io(path, error))?;
        Ok(Self {
            meta: Meta::at(path),
            bytes,
        })
    }

    /// Adopt `bytes` as the content of an unnamed document, as `To CBOR` does.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            meta: Meta::new(),
            bytes,
        }
    }

    /// The buffer.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes there are.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Replace one byte.
    ///
    /// Both the hex cells and the ASCII gutter route through here, which is what keeps them in
    /// sync by construction rather than by two mirrored handlers -- the old `HexEditor.tsx` had
    /// one handler per side and they could disagree.
    pub fn set_byte(&mut self, offset: usize, value: u8) -> Result<()> {
        self.meta.writable()?;
        let len = self.bytes.len();
        let byte = self
            .bytes
            .get_mut(offset)
            .ok_or(CoreError::RangeOutOfBounds {
                range: offset..offset + 1,
                len,
            })?;
        if *byte != value {
            *byte = value;
            self.meta.dirty = true;
        }
        Ok(())
    }

    /// Replace the whole buffer, as `To CBOR` on the EDN tab does.
    ///
    /// Permitted on a read-only document: read-only guards the *user's* editing, and a conversion
    /// is the tool replacing the document wholesale, after which the result is unsaved and needs a
    /// name. This mirrors how the EDN tab's `To CBOR` is expected to behave even with the CBOR
    /// tab's default read-only checkbox still ticked.
    pub fn replace(&mut self, bytes: Vec<u8>) {
        self.bytes = bytes;
        self.meta.dirty = true;
    }

    /// Write the buffer to its own path.
    pub fn save(&mut self) -> Result<()> {
        let path = self
            .meta
            .path
            .clone()
            .ok_or(CoreError::NoPath("CBOR document"))?;
        self.save_as(path)
    }

    /// Write the buffer to `path` and adopt it.
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        std::fs::write(&path, &self.bytes).map_err(|error| CoreError::io(&path, error))?;
        self.meta.path = Some(path);
        self.meta.dirty = false;
        Ok(())
    }
}

/// A text document: EDN, CDDL or COSE-key source.
///
/// One type for all three, because identity, dirty tracking and read-only enforcement are
/// identical and only the default file extension differs. The kind lives in the tab, not here.
#[derive(Debug, Clone, Default)]
pub struct TextDoc {
    /// Identity and status.
    pub meta: Meta,
    text: String,
}

impl TextDoc {
    /// An empty, read-only document.
    pub fn new() -> Self {
        Self {
            meta: Meta::new(),
            text: String::new(),
        }
    }

    /// Read `path` as UTF-8 text.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|error| CoreError::io(path, error))?;
        Ok(Self {
            meta: Meta::at(path),
            text,
        })
    }

    /// Adopt `text` as the content of an unnamed document, as `To EDN` does.
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            meta: Meta::new(),
            text: text.into(),
        }
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether there is no text.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace the text as a user edit, which read-only refuses.
    pub fn edit(&mut self, text: impl Into<String>) -> Result<()> {
        self.meta.writable()?;
        let text = text.into();
        if text != self.text {
            self.text = text;
            self.meta.dirty = true;
        }
        Ok(())
    }

    /// Replace the text as the result of a conversion, which read-only permits.
    ///
    /// See [`CborDoc::replace`] for why the two differ.
    pub fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.meta.dirty = true;
    }

    /// Write the text to its own path.
    pub fn save(&mut self) -> Result<()> {
        let path = self
            .meta
            .path
            .clone()
            .ok_or(CoreError::NoPath("text document"))?;
        self.save_as(path)
    }

    /// Write the text to `path` and adopt it.
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        std::fs::write(&path, &self.text).map_err(|error| CoreError::io(&path, error))?;
        self.meta.path = Some(path);
        self.meta.dirty = false;
        Ok(())
    }
}

/// The EDN tab's document.
///
/// An alias rather than a newtype: the EDN and COSE tabs differ in what is *shown* beside them,
/// not in how the document behaves, and a newtype would duplicate every method to no end.
pub type EdnDoc = TextDoc;

/// The COSE tab's document: the encoded COSE structure, as bytes.
///
/// An alias of [`CborDoc`] rather than of [`TextDoc`], because a COSE object *is* CBOR and the tab
/// saves it as such. See [`Workspace::cose`].
pub type CoseDoc = CborDoc;

/// The CDDL tab's open include chain.
///
/// The one place `UI_DESIGN.md` allows several files at once. Entry 0 is the file the user opened;
/// the rest arrive by clicking a `;# import`/`;# include` line. Each carries its own read-only
/// flag, as `UI_DESIGN.md:116` requires, which is why this is a `Vec<TextDoc>` and not one
/// document plus a list of paths.
///
/// The start production is held here rather than per entry, because `UI_DESIGN.md:114` says it
/// *"must be the same regardless of the currently selected tab"*.
#[derive(Debug, Clone, Default)]
pub struct CddlDocSet {
    tabs: Vec<TextDoc>,
    active: usize,
    /// The start production the user typed, global across inner tabs.
    ///
    /// Empty means "not specified", which [`crate::cddl::start_rule`] turns into the first rule.
    pub start_production: String,
}

impl CddlDocSet {
    /// An empty set, with no inner tabs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every open inner tab, in tab-bar order.
    pub fn tabs(&self) -> &[TextDoc] {
        &self.tabs
    }

    /// How many inner tabs are open.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether no CDDL file is open.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The selected inner tab, which the right pane mirrors (`UI_DESIGN.md:110`).
    pub fn active(&self) -> Option<&TextDoc> {
        self.tabs.get(self.active)
    }

    /// The selected inner tab, mutably.
    pub fn active_mut(&mut self) -> Option<&mut TextDoc> {
        self.tabs.get_mut(self.active)
    }

    /// Inner tab `index`, mutably.
    ///
    /// The counterpart to [`CddlDocSet::tabs`], and the one accessor [`CddlDocSet::active_mut`]
    /// cannot stand in for. The CDDL view holds one editor per inner tab -- so that switching tabs
    /// does not clear the undo history, the caret and the scroll position -- and must write *every*
    /// editor back to *its own* document before a save, a quit or a syntax check. Routing that
    /// through `active_mut` would mean [`CddlDocSet::select`]ing each tab in turn, which moves the
    /// selection the user left behind; routing it through the selected tab alone would write the
    /// outgoing editor's text into the incoming document on every switch, silently corrupting a
    /// file.
    pub fn tab_mut(&mut self, index: usize) -> Option<&mut TextDoc> {
        self.tabs.get_mut(index)
    }

    /// Index of the selected inner tab.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Select inner tab `index`, if it exists.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Open `path` in an inner tab and select it.
    ///
    /// A file already open is selected rather than opened twice -- clicking the same
    /// `;# include rfc9052` from two different files must not produce two tabs that can disagree
    /// about their content.
    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<usize> {
        let path = path.as_ref();
        if let Some(index) = self.index_of(path) {
            self.active = index;
            return Ok(index);
        }
        self.tabs.push(TextDoc::open(path)?);
        self.active = self.tabs.len() - 1;
        Ok(self.active)
    }

    /// Open `path` as the root of a *new* chain, discarding every tab currently open.
    ///
    /// [`CddlDocSet::open`] appends, which is right for a `;# include` click adding to the chain
    /// under exploration. Opening from File->Open or the explorer is a different act: it starts a
    /// new chain. Without this, the inner tab bar accumulates every CDDL file opened in the
    /// session with no way to shed them -- gpui-component's `TabBar` has no close API.
    ///
    /// Returns the paths of any **dirty** tabs that were displaced, so a caller can warn rather
    /// than silently discard unsaved work. An empty vector means nothing was lost.
    ///
    /// The new file is read *before* anything is discarded, so a failure leaves the existing
    /// chain exactly as it was.
    pub fn open_root(&mut self, path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let doc = TextDoc::open(path.as_ref())?;
        let displaced = self
            .tabs
            .iter()
            .filter(|tab| tab.meta.dirty)
            .filter_map(|tab| tab.meta.path.clone())
            .collect();
        self.tabs = vec![doc];
        self.active = 0;
        Ok(displaced)
    }

    /// Index of the tab holding `path`, comparing canonical paths where both resolve.
    ///
    /// `;# include` resolution yields a path built from a search directory, which may name the
    /// same file as the one the user opened by a different route (`./x.cddl` versus
    /// `examples/x.cddl`). Canonicalizing is what stops that becoming two tabs. A path that cannot
    /// be canonicalized -- deleted since, or not yet written -- falls back to a literal compare
    /// rather than failing.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        let wanted = std::fs::canonicalize(path).ok();
        self.tabs.iter().position(|tab| {
            let Some(open) = tab.meta.path.as_deref() else {
                return false;
            };
            match (&wanted, std::fs::canonicalize(open).ok()) {
                (Some(wanted), Some(open)) => *wanted == open,
                _ => open == path,
            }
        })
    }

    /// Close inner tab `index`, returning it.
    ///
    /// Selection moves to the previous tab, which is what a tab bar does; closing the last tab
    /// leaves the set empty and `active` at 0.
    pub fn close(&mut self, index: usize) -> Option<TextDoc> {
        if index >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(index);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        Some(closed)
    }

    /// Whether any inner tab has unsaved changes.
    pub fn dirty(&self) -> bool {
        self.tabs.iter().any(|tab| tab.meta.dirty)
    }

    /// Save every dirty inner tab, reporting the ones that could not be written.
    ///
    /// Every tab is attempted even after one fails, so a single unwritable file does not strand
    /// the others unsaved. See [`Workspace::save_all`] for why the failures come back as
    /// diagnostics rather than as a `Result`.
    pub fn save_all(&mut self) -> Vec<Diagnostic> {
        self.tabs
            .iter_mut()
            .filter(|tab| tab.meta.dirty)
            .filter_map(|tab| {
                let path = tab.meta.path.clone();
                tab.save()
                    .err()
                    .map(|error| diagnose(Source::Cddl, path.as_deref(), &error))
            })
            .collect()
    }

    /// The start production to validate against, or `None` if the box is blank or whitespace.
    pub fn start(&self) -> Option<&str> {
        let trimmed = self.start_production.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }
}

/// The four open documents, plus the folder the explorer is showing.
///
/// One document per kind, per `UI_DESIGN.md:57`. The explorer's content does not depend on the
/// selected tab (`UI_DESIGN.md:55`), which is why `root` lives here and not on a tab.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// The folder the file tree shows. `None` before the user opens one.
    pub root: Option<PathBuf>,
    /// The CBOR binary tab's document.
    pub cbor: CborDoc,
    /// The EDN tab's document.
    pub edn: TextDoc,
    /// The CDDL tab's open include chain.
    pub cddl: CddlDocSet,
    /// The COSE tab's document: the encoded COSE structure itself.
    ///
    /// Bytes, not the EDN text of them. The tab *shows* EDN (`UI_DESIGN.md:127`), but
    /// `UI_DESIGN.md:120` asks that Save and Save As "operate as you would expect", and what a
    /// user expects of the `untitled.cbor` the COSE tab defaults to is the COSE object — which is
    /// also what another tool can read back.
    ///
    /// A private key is never put here: a `CborDoc` holds a plain `Vec<u8>`, and
    /// `tps_cbor_core/CLAUDE.md` requires private key material to stay in `Zeroizing`. Opening a
    /// COSE key offers an import into the key store instead.
    pub cose: CborDoc,
}

impl Workspace {
    /// An empty workspace with no folder open.
    pub fn new() -> Self {
        Self {
            root: None,
            cbor: CborDoc::new(),
            edn: TextDoc::new(),
            cddl: CddlDocSet::new(),
            cose: CborDoc::new(),
        }
    }

    /// An empty workspace showing `root`.
    pub fn at_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            ..Self::new()
        }
    }

    /// Whether anything would be lost by quitting now.
    pub fn dirty(&self) -> bool {
        self.cbor.meta.dirty || self.edn.meta.dirty || self.cose.meta.dirty || self.cddl.dirty()
    }

    /// Save every document with unsaved changes, reporting the ones that could not be written.
    ///
    /// An empty return means everything is on disk; this is what lets a frontend offer
    /// "save all and quit" and *cancel the quit* when a document could not be saved, rather than
    /// discarding it.
    ///
    /// Failures come back as [`Diagnostic`]s rather than as a `Result<()>` for two reasons. There
    /// can be more than one — four documents are attempted and each can fail for its own reason —
    /// and a bare error would not say *which* document it came from, which is the only part the
    /// user can act on. Every document is attempted even after one fails, so one unwritable file
    /// does not strand the rest.
    ///
    /// A document with no path is reported, not silently skipped: `CoreError::NoPath` is exactly
    /// the "this one needs `Save As`" message, and treating it as a failure is what stops a
    /// conversion result being thrown away by a quit that looked like it had saved everything.
    pub fn save_all(&mut self) -> Vec<Diagnostic> {
        let singles = [
            (Source::Cbor, self.cbor.meta.dirty),
            (Source::Edn, self.edn.meta.dirty),
            (Source::Cose, self.cose.meta.dirty),
        ];

        let mut failures = Vec::new();
        for (source, dirty) in singles {
            if !dirty {
                continue;
            }
            // Written out rather than looped over `&mut dyn` because the three documents are not
            // one type: the CBOR and COSE documents hold bytes and the EDN one a string.
            let (outcome, path) = match source {
                Source::Cbor => (self.cbor.save(), self.cbor.meta.path.clone()),
                Source::Edn => (self.edn.save(), self.edn.meta.path.clone()),
                _ => (self.cose.save(), self.cose.meta.path.clone()),
            };
            if let Err(error) = outcome {
                failures.push(diagnose(source, path.as_deref(), &error));
            }
        }

        failures.extend(self.cddl.save_all());
        failures
    }

    /// Every path currently open, for the explorer's bold rendering (`UI_DESIGN.md:56`).
    pub fn open_paths(&self) -> Vec<&Path> {
        let singles = [&self.cbor.meta, &self.edn.meta, &self.cose.meta]
            .into_iter()
            .filter_map(|meta| meta.path.as_deref());
        let cddl = self
            .cddl
            .tabs()
            .iter()
            .filter_map(|tab| tab.meta.path.as_deref());
        singles.chain(cddl).collect()
    }

    /// Every path with unsaved changes, for the explorer's bold-red rendering.
    pub fn dirty_paths(&self) -> Vec<&Path> {
        let singles = [&self.cbor.meta, &self.edn.meta, &self.cose.meta]
            .into_iter()
            .filter(|meta| meta.dirty)
            .filter_map(|meta| meta.path.as_deref());
        let cddl = self
            .cddl
            .tabs()
            .iter()
            .filter(|tab| tab.meta.dirty)
            .filter_map(|tab| tab.meta.path.as_deref());
        singles.chain(cddl).collect()
    }

    /// Render the CBOR document as EDN into the EDN tab, as the `To EDN` button does.
    ///
    /// Leaves the EDN tab untouched on failure. `UI_DESIGN.md` and
    /// `tps_cbor_gui/PLAN.md:623-628` both insist on this: the old GUI wrote the error message
    /// *into* the editor, destroying whatever the user had.
    pub fn cbor_to_edn(&mut self) -> Result<()> {
        let text = crate::convert::cbor_to_edn(self.cbor.bytes())?;
        let keep_name = self
            .cbor
            .meta
            .base_name()
            .map(|base| self.edn.meta.base_name() == Some(base))
            .unwrap_or(false);
        self.edn.replace(text);
        if !keep_name {
            // The EDN now holds something else's content, so `Save` must not silently overwrite
            // the file the name refers to.
            self.edn.meta.path = None;
        }
        Ok(())
    }

    /// Encode the EDN document into the CBOR tab, as the `To CBOR` button does.
    ///
    /// Implements `UI_DESIGN.md:96`: *"If there is a file loaded in the CBOR screen which has a
    /// different base name to the EDN file, the CBOR filename is cleared to force use of 'Save as'
    /// option rather than 'Save'."* Leaves the CBOR tab untouched on failure.
    pub fn edn_to_cbor(&mut self) -> Result<()> {
        let bytes = crate::convert::edn_to_cbor(self.edn.text())?;
        let keep_name = self.cbor_path_after_conversion_from_edn();
        self.cbor.replace(bytes);
        if !keep_name {
            self.cbor.meta.path = None;
        }
        Ok(())
    }

    /// `UI_DESIGN.md:93`: load the CBOR that a selected EDN fragment stands for into the CBOR
    /// document, returning how many bytes arrived.
    ///
    /// The filename is cleared **unconditionally**, unlike [`Workspace::edn_to_cbor`]'s base-name
    /// rule. A fragment of a document is not that document, so there is no provenance to keep and
    /// `Save As` is always the right next step; leaving the path would let `Save` overwrite an
    /// unrelated file with a decoded fragment of something else.
    ///
    /// Leaves the CBOR document untouched on failure, by the same construction `edn_to_cbor` uses:
    /// `?` is applied before anything is written. That is what makes the non-destructive
    /// requirement structural rather than a convention each caller has to honour.
    ///
    /// `replace` rather than a guarded write, so a read-only CBOR tab — which is the default — does
    /// not block the feature. This matches how both conversions already load into it.
    pub fn load_decoded_selection(&mut self, selection: &str) -> Result<usize> {
        let bytes = crate::convert::decode_edn_selection(selection)?;
        let count = bytes.len();
        self.cbor.replace(bytes);
        self.cbor.meta.path = None;
        Ok(count)
    }

    /// Whether the CBOR tab may keep its filename after `To CBOR`.
    ///
    /// `true` only when both documents are named and their base names agree, or when the CBOR tab
    /// has no name to lose. Exposed so the UI can warn before converting rather than only
    /// discovering afterwards that `Save` has become `Save As`.
    pub fn cbor_path_after_conversion_from_edn(&self) -> bool {
        match (self.cbor.meta.base_name(), self.edn.meta.base_name()) {
            (Some(cbor), Some(edn)) => cbor == edn,
            // Nothing to clear.
            (None, _) => true,
            // The EDN is unnamed, so there is no base name to compare; the CBOR name would be
            // claiming provenance it no longer has.
            (Some(_), None) => false,
        }
    }
}

/// A failed save, as a diagnostic naming the document it came from.
///
/// The span is empty because a write failure is not *at* an offset — nothing in the buffer is
/// wrong. `Diagnostic` requires one, and `0..0` is the honest answer; a frontend that highlights
/// spans has nothing to highlight here, which is correct.
fn diagnose(source: Source, path: Option<&Path>, error: &CoreError) -> Diagnostic {
    Diagnostic::error(source, 0..0, error.to_string()).in_file_opt(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `{1: 2}`
    const CBOR: &[u8] = &[0xa1, 0x01, 0x02];

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps-cbor-doc-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn documents_are_read_only_and_clean_by_default() {
        let workspace = Workspace::new();
        assert!(workspace.cbor.meta.read_only);
        assert!(workspace.edn.meta.read_only);
        assert!(workspace.cose.meta.read_only);
        assert!(!workspace.dirty());
        assert!(workspace.cddl.is_empty());
        assert!(workspace.open_paths().is_empty());
    }

    /// The model refuses the edit itself, rather than trusting the UI to grey out a control.
    #[test]
    fn a_read_only_document_refuses_every_user_edit() {
        let mut cbor = CborDoc::from_bytes(CBOR.to_vec());
        let error = cbor.set_byte(0, 0xff).expect_err("read-only by default");
        assert!(matches!(error, CoreError::ReadOnly { .. }), "{error:?}");
        assert_eq!(cbor.bytes(), CBOR, "the buffer was modified anyway");

        let mut text = TextDoc::from_text("a = uint\n");
        assert!(matches!(
            text.edit("b = tstr\n").expect_err("read-only"),
            CoreError::ReadOnly { .. }
        ));
        assert_eq!(text.text(), "a = uint\n");
    }

    #[test]
    fn making_a_document_editable_permits_edits() {
        let mut cbor = CborDoc::from_bytes(CBOR.to_vec());
        cbor.meta.read_only = false;
        cbor.meta.dirty = false;
        cbor.set_byte(2, 0x09).expect("now editable");
        assert_eq!(cbor.bytes(), &[0xa1, 0x01, 0x09]);
        assert!(cbor.meta.dirty);
    }

    #[test]
    fn an_edit_that_changes_nothing_does_not_dirty_the_document() {
        let mut cbor = CborDoc::from_bytes(CBOR.to_vec());
        cbor.meta.read_only = false;
        cbor.meta.dirty = false;
        cbor.set_byte(0, 0xa1).expect("editable");
        assert!(!cbor.meta.dirty, "rewriting the same byte dirtied the file");

        let mut text = TextDoc::from_text("same");
        text.meta.read_only = false;
        text.meta.dirty = false;
        text.edit("same").expect("editable");
        assert!(!text.meta.dirty);
    }

    #[test]
    fn an_out_of_range_byte_is_rejected_rather_than_panicking() {
        let mut cbor = CborDoc::from_bytes(CBOR.to_vec());
        cbor.meta.read_only = false;
        let error = cbor.set_byte(99, 0).expect_err("out of range");
        assert!(
            matches!(error, CoreError::RangeOutOfBounds { len: 3, .. }),
            "{error:?}"
        );
    }

    /// Read-only guards the user's keystrokes, not a conversion the user explicitly asked for.
    #[test]
    fn a_conversion_replaces_content_even_when_read_only() {
        let mut cbor = CborDoc::new();
        assert!(cbor.meta.read_only);
        cbor.replace(CBOR.to_vec());
        assert_eq!(cbor.bytes(), CBOR);
        assert!(cbor.meta.dirty);
    }

    #[test]
    fn save_without_a_path_is_refused_and_save_as_adopts_one() {
        let dir = temp_dir("save");
        let mut cbor = CborDoc::from_bytes(CBOR.to_vec());
        assert!(!cbor.meta.can_save());
        assert!(matches!(
            cbor.save().expect_err("no path"),
            CoreError::NoPath(_)
        ));

        let path = dir.join("out.cbor");
        cbor.save_as(&path).expect("save_as");
        assert!(cbor.meta.can_save());
        assert!(!cbor.meta.dirty);
        assert_eq!(std::fs::read(&path).expect("written"), CBOR);

        // And a second save now goes to the adopted path.
        cbor.meta.read_only = false;
        cbor.set_byte(2, 0x07).expect("editable");
        assert!(cbor.meta.dirty);
        cbor.save().expect("save");
        assert!(!cbor.meta.dirty);
        assert_eq!(std::fs::read(&path).expect("written"), &[0xa1, 0x01, 0x07]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Malformed CBOR must still open -- the hex editor exists to repair it.
    #[test]
    fn a_malformed_cbor_file_still_opens() {
        let dir = temp_dir("malformed");
        let path = dir.join("truncated.cbor");
        // 0xa2 promises two map entries and then ends.
        std::fs::write(&path, [0xa2u8]).expect("write");
        let doc = CborDoc::open(&path).expect("opens despite being malformed");
        assert_eq!(doc.bytes(), &[0xa2]);
        assert!(crate::convert::check_well_formed(doc.bytes()).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_a_missing_file_names_it_in_the_error() {
        let error = CborDoc::open("/nonexistent/x.cbor").expect_err("no such file");
        match error {
            CoreError::Io { path, .. } => assert_eq!(path, Path::new("/nonexistent/x.cbor")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn a_title_falls_back_when_there_is_no_path() {
        assert_eq!(Meta::new().title(), "(untitled)");
        assert_eq!(Meta::at("/tmp/claims.cddl").title(), "claims.cddl");
        assert_eq!(Meta::at("/tmp/claims.cddl").base_name(), Some("claims"));
        assert_eq!(Meta::new().base_name(), None);
    }

    /// `UI_DESIGN.md:96`, the rule the old GUI implemented in the frontend and inconsistently.
    #[test]
    fn to_cbor_keeps_the_name_only_when_the_base_names_agree() {
        let mut workspace = Workspace::new();
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.edn.meta.path = Some("/tmp/claims.edn".into());
        workspace.cbor.meta.path = Some("/tmp/claims.cbor".into());
        assert!(workspace.cbor_path_after_conversion_from_edn());
        workspace.edn_to_cbor().expect("converts");
        assert_eq!(
            workspace.cbor.meta.path.as_deref(),
            Some(Path::new("/tmp/claims.cbor")),
            "matching base names should keep the filename"
        );
        assert_eq!(workspace.cbor.bytes(), CBOR);

        // Differing base names force Save As.
        workspace.cbor.meta.path = Some("/tmp/other.cbor".into());
        assert!(!workspace.cbor_path_after_conversion_from_edn());
        workspace.edn_to_cbor().expect("converts");
        assert!(
            workspace.cbor.meta.path.is_none(),
            "a differing base name must clear the CBOR filename"
        );
    }

    /// `UI_DESIGN.md:93`. Unconditional, unlike [`Workspace::edn_to_cbor`]'s base-name rule: a
    /// fragment is not the document it came from, so a retained path would let `Save` overwrite an
    /// unrelated file with a decoded piece of something else.
    #[test]
    fn a_decoded_selection_clears_the_cbor_path_so_save_as_is_forced() {
        let mut workspace = Workspace::new();
        workspace.cbor.meta.path = Some("/tmp/claims.cbor".into());
        // Even when the EDN document shares the base name, which is the case `edn_to_cbor` keeps.
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.edn.meta.path = Some("/tmp/claims.edn".into());

        let count = workspace
            .load_decoded_selection("h'a10126'")
            .expect("decodes");
        assert_eq!(count, 3, "the bstr payload is three bytes");
        assert_eq!(workspace.cbor.bytes(), [0xa1, 0x01, 0x26]);
        assert!(
            workspace.cbor.meta.path.is_none(),
            "a decoded fragment must always force Save As"
        );
    }

    /// Non-destructive reporting, made structural: `?` precedes the write, so a selection that does
    /// not parse cannot cost the user the buffer they already had.
    #[test]
    fn a_failed_decode_leaves_the_cbor_document_untouched() {
        let mut workspace = Workspace::new();
        workspace.cbor.replace(CBOR.to_vec());
        workspace.cbor.meta.path = Some("/tmp/claims.cbor".into());
        workspace.cbor.meta.dirty = false;

        workspace
            .load_decoded_selection("h'nothex'")
            .expect_err("should not parse");

        assert_eq!(workspace.cbor.bytes(), CBOR);
        assert_eq!(
            workspace.cbor.meta.path.as_deref(),
            Some(Path::new("/tmp/claims.cbor"))
        );
        assert!(
            !workspace.cbor.meta.dirty,
            "a refused decode is not an edit"
        );
    }

    /// The decode has to mark the document dirty, or the quit guard would not protect it and the
    /// bytes the user just pulled out of a payload would be discarded on exit without a word.
    #[test]
    fn a_decoded_selection_marks_the_cbor_document_dirty() {
        let mut workspace = Workspace::new();
        assert!(!workspace.dirty());
        workspace
            .load_decoded_selection("h'a10126'")
            .expect("decodes");
        assert!(workspace.cbor.meta.dirty);
        assert!(workspace.dirty(), "the quit guard has to see this");
    }

    /// Read-only is the CBOR tab's default (`UI_DESIGN.md:83`), so going through the guarded setter
    /// would make the feature refuse itself out of the box. `replace` is the path that loads a new
    /// document, and both conversions already use it.
    #[test]
    fn a_decoded_selection_is_permitted_while_the_cbor_tab_is_read_only() {
        let mut workspace = Workspace::new();
        assert!(
            workspace.cbor.meta.read_only,
            "read-only is the default this test is about"
        );
        workspace
            .load_decoded_selection("h'a10126'")
            .expect("a read-only tab still accepts a decoded selection");
        assert_eq!(workspace.cbor.bytes(), [0xa1, 0x01, 0x26]);
    }

    #[test]
    fn to_cbor_from_an_unnamed_edn_clears_a_named_cbor() {
        let mut workspace = Workspace::new();
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.cbor.meta.path = Some("/tmp/claims.cbor".into());
        assert!(!workspace.cbor_path_after_conversion_from_edn());
        workspace.edn_to_cbor().expect("converts");
        assert!(workspace.cbor.meta.path.is_none());
    }

    /// `tps_cbor_gui/PLAN.md:623-628`: never overwrite the user's text with an error.
    #[test]
    fn a_failed_conversion_leaves_the_other_document_alone() {
        let mut workspace = Workspace::new();
        workspace.edn = TextDoc::from_text("this is not EDN at all %%%");
        workspace.cbor = CborDoc::from_bytes(CBOR.to_vec());
        workspace.cbor.meta.path = Some("/tmp/keep.cbor".into());
        workspace.cbor.meta.dirty = false;

        workspace.edn_to_cbor().expect_err("not EDN");
        assert_eq!(workspace.cbor.bytes(), CBOR, "the CBOR tab was clobbered");
        assert_eq!(
            workspace.cbor.meta.path.as_deref(),
            Some(Path::new("/tmp/keep.cbor"))
        );
        assert!(!workspace.cbor.meta.dirty);

        // And the same in the other direction.
        workspace.cbor = CborDoc::from_bytes(vec![0xa2]);
        workspace.edn = TextDoc::from_text("keep me");
        workspace.cbor_to_edn().expect_err("truncated CBOR");
        assert_eq!(workspace.edn.text(), "keep me");
    }

    #[test]
    fn to_edn_renders_the_cbor_and_tracks_the_name() {
        let mut workspace = Workspace::new();
        workspace.cbor = CborDoc::from_bytes(CBOR.to_vec());
        workspace.cbor.meta.path = Some("/tmp/claims.cbor".into());
        workspace.cbor_to_edn().expect("converts");
        assert!(workspace.edn.text().contains('1'));
        assert!(
            workspace.edn.meta.path.is_none(),
            "an unnamed EDN tab must not acquire a name from the CBOR tab"
        );
        assert!(workspace.edn.meta.dirty);

        // A matching base name is kept.
        workspace.edn.meta.path = Some("/tmp/claims.edn".into());
        workspace.cbor_to_edn().expect("converts");
        assert_eq!(
            workspace.edn.meta.path.as_deref(),
            Some(Path::new("/tmp/claims.edn"))
        );
    }

    #[test]
    fn the_cddl_set_opens_selects_and_closes_inner_tabs() {
        let dir = temp_dir("cddl");
        let first = dir.join("first.cddl");
        let second = dir.join("second.cddl");
        std::fs::write(&first, "a = uint\n").expect("write");
        std::fs::write(&second, "b = tstr\n").expect("write");

        let mut set = CddlDocSet::new();
        assert_eq!(set.open(&first).expect("opens"), 0);
        assert_eq!(set.open(&second).expect("opens"), 1);
        assert_eq!(set.len(), 2);
        assert_eq!(set.active_index(), 1);
        assert_eq!(set.active().expect("active").text(), "b = tstr\n");

        assert!(set.select(0));
        assert!(!set.select(9), "selecting a nonexistent tab should fail");
        assert_eq!(set.active_index(), 0);

        let closed = set.close(0).expect("closes");
        assert_eq!(closed.meta.title(), "first.cddl");
        assert_eq!(set.len(), 1);
        assert_eq!(set.active_index(), 0, "selection should stay in range");

        set.close(0).expect("closes the last tab");
        assert!(set.is_empty());
        assert_eq!(set.active_index(), 0);
        assert!(set.active().is_none());
        assert!(set.close(0).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Clicking the same `;# include` from two files must not make two tabs that can disagree.
    #[test]
    fn opening_the_same_file_twice_selects_the_existing_tab() {
        let dir = temp_dir("dedupe");
        let path = dir.join("shared.cddl");
        std::fs::write(&path, "a = uint\n").expect("write");

        let mut set = CddlDocSet::new();
        set.open(&path).expect("opens");
        set.open(dir.join("other.cddl").with_file_name("shared.cddl"))
            .expect("opens");
        assert_eq!(set.len(), 1, "the same file opened twice");

        // And by a path that names the same file indirectly.
        let indirect = dir.join(".").join("shared.cddl");
        assert_eq!(set.open(&indirect).expect("opens"), 0);
        assert_eq!(set.len(), 1, "a `.` component produced a second tab");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `open_root` starts a new chain rather than growing the old one, and says what unsaved work
    /// it displaced so the caller can warn instead of silently discarding it.
    #[test]
    fn open_root_replaces_the_chain_and_reports_what_it_displaced() {
        let dir = temp_dir("open-root");
        let first = dir.join("first.cddl");
        let second = dir.join("second.cddl");
        let third = dir.join("third.cddl");
        for path in [&first, &second, &third] {
            std::fs::write(path, "a = uint\n").expect("write");
        }

        let mut set = CddlDocSet::new();
        set.open(&first).expect("opens");
        set.open(&second).expect("opens");
        assert_eq!(set.len(), 2);

        // A clean chain is displaced silently.
        assert!(
            set.open_root(&third).expect("opens").is_empty(),
            "nothing was unsaved"
        );
        assert_eq!(set.len(), 1);
        assert_eq!(set.active_index(), 0);
        assert_eq!(set.active().expect("active").meta.title(), "third.cddl");

        // A dirty one is named.
        set.active_mut().expect("active").replace("b = tstr\n");
        set.open(&first).expect("opens");
        let displaced = set.open_root(&second).expect("opens");
        assert_eq!(displaced, vec![third.clone()]);
        assert_eq!(set.len(), 1);

        // And a failure leaves the chain untouched.
        set.open_root(dir.join("not_here.cddl"))
            .expect_err("no such file");
        assert_eq!(set.len(), 1);
        assert_eq!(set.active().expect("active").meta.title(), "second.cddl");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The invariant `CddlView::editors` is keyed on: without it, a `HashMap<PathBuf, _>` of
    /// per-tab editor states silently merges two tabs and one file's text overwrites the other's.
    #[test]
    fn every_inner_cddl_tab_has_a_distinct_path() {
        let dir = temp_dir("distinct");
        let first = dir.join("first.cddl");
        let second = dir.join("second.cddl");
        std::fs::write(&first, "a = uint\n").expect("write");
        std::fs::write(&second, "b = tstr\n").expect("write");

        let mut set = CddlDocSet::new();
        // Every route into the set: `open`, `open` of an alias, and `open_root`.
        set.open(&first).expect("opens");
        set.open(dir.join(".").join("first.cddl")).expect("opens");
        set.open(&second).expect("opens");
        set.open_root(&first).expect("opens");
        set.open(&second).expect("opens");

        let mut keys: Vec<PathBuf> = set
            .tabs()
            .iter()
            .map(|tab| {
                let path = tab.meta.path.as_deref().expect("an opened tab has a path");
                std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
            })
            .collect();
        let count = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), count, "two tabs share a canonical path");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// What `tab_mut` exists for: the CDDL view writes each editor back to its own tab without
    /// disturbing the selection the user left behind.
    #[test]
    fn a_tab_can_be_written_without_selecting_it() {
        let dir = temp_dir("tab-mut");
        let first = dir.join("first.cddl");
        let second = dir.join("second.cddl");
        std::fs::write(&first, "a = uint\n").expect("write");
        std::fs::write(&second, "b = tstr\n").expect("write");

        let mut set = CddlDocSet::new();
        set.open(&first).expect("opens");
        set.open(&second).expect("opens");
        assert_eq!(set.active_index(), 1);

        set.tab_mut(0).expect("tab 0").replace("c = bstr\n");

        assert_eq!(set.active_index(), 1, "the selection must not move");
        assert_eq!(set.tabs()[0].text(), "c = bstr\n");
        assert_eq!(
            set.tabs()[1].text(),
            "b = tstr\n",
            "the other tab is intact"
        );
        assert!(
            set.tab_mut(2).is_none(),
            "out of range is None, not a panic"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_start_production_is_global_and_blank_means_unset() {
        let mut set = CddlDocSet::new();
        assert_eq!(set.start(), None);
        set.start_production = "   ".into();
        assert_eq!(set.start(), None, "whitespace should count as unset");
        set.start_production = "  Claims-Set  ".into();
        assert_eq!(set.start(), Some("Claims-Set"));
    }

    #[test]
    fn dirty_tracking_covers_every_tab_including_inner_cddl_tabs() {
        let dir = temp_dir("dirty");
        let path = dir.join("x.cddl");
        std::fs::write(&path, "a = uint\n").expect("write");

        let mut workspace = Workspace::at_root(&dir);
        workspace.cddl.open(&path).expect("opens");
        assert!(!workspace.dirty());

        let tab = workspace.cddl.active_mut().expect("active");
        tab.meta.read_only = false;
        tab.edit("a = tstr\n").expect("editable");
        assert!(workspace.cddl.dirty());
        assert!(
            workspace.dirty(),
            "a dirty inner CDDL tab must dirty the workspace"
        );
        assert_eq!(workspace.dirty_paths(), vec![path.as_path()]);
        assert_eq!(workspace.open_paths(), vec![path.as_path()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_all_writes_every_dirty_document_and_clears_them() {
        let dir = temp_dir("save-all");
        let cddl = dir.join("schema.cddl");
        std::fs::write(&cddl, "a = uint\n").expect("write");

        let mut workspace = Workspace::at_root(&dir);
        workspace.cbor = CborDoc::from_bytes(CBOR.to_vec());
        workspace.cbor.meta.path = Some(dir.join("value.cbor"));
        workspace.cbor.meta.dirty = true;
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.edn.meta.path = Some(dir.join("value.edn"));
        workspace.edn.meta.dirty = true;
        workspace.cddl.open(&cddl).expect("opens");
        let tab = workspace.cddl.active_mut().expect("active");
        tab.meta.read_only = false;
        tab.edit("a = tstr\n").expect("editable");

        assert!(workspace.dirty());
        assert_eq!(
            workspace.save_all(),
            Vec::new(),
            "every document had a path, so none should have failed"
        );

        assert!(!workspace.dirty(), "save_all left something dirty");
        assert_eq!(
            std::fs::read(dir.join("value.cbor")).expect("read"),
            CBOR,
            "the CBOR buffer did not reach the disk"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("value.edn")).expect("read"),
            "{1: 2}"
        );
        assert_eq!(
            std::fs::read_to_string(&cddl).expect("read"),
            "a = tstr\n",
            "the inner CDDL tab was not saved"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The case the quit confirmation turns on: a document with nowhere to go must be reported, so
    /// the frontend can cancel the quit instead of discarding it.
    #[test]
    fn save_all_reports_an_unnamed_document_and_leaves_it_dirty() {
        let mut workspace = Workspace::new();
        workspace.edn = TextDoc::from_text("{1: 2}");
        workspace.edn.meta.dirty = true;

        let failures = workspace.save_all();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].source, Source::Edn);
        assert_eq!(failures[0].severity, crate::diag::Severity::Error);
        assert!(
            failures[0].file.is_none(),
            "there is no file to blame for a document with no path"
        );
        assert!(
            workspace.edn.meta.dirty,
            "a document that could not be saved must stay dirty"
        );
        assert!(workspace.dirty());
    }

    /// The same case for the COSE document, which is where a `Sign` or `Encrypt` result lands:
    /// a signature nobody has named yet must not be thrown away by a quit that looked clean.
    #[test]
    fn save_all_reports_an_unsaved_cose_document_with_no_path() {
        let mut workspace = Workspace::new();
        workspace.cose.replace(vec![0x84, 0x40, 0xa0, 0xf6]);
        assert!(workspace.cose.meta.dirty, "replace marks it unsaved");
        assert!(workspace.dirty());

        let failures = workspace.save_all();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].source, Source::Cose);
        assert!(failures[0].file.is_none());
        assert!(workspace.cose.meta.dirty);
    }

    /// The document-type change: the COSE tab saves the COSE object, not the EDN text of it.
    #[test]
    fn the_cose_document_round_trips_bytes() {
        let dir = temp_dir("cose-bytes");
        std::fs::create_dir_all(&dir).expect("create");
        let path = dir.join("signed.cbor");

        let mut workspace = Workspace::at_root(&dir);
        let object = vec![0x84, 0x43, 0xa1, 0x01, 0x26, 0xa0, 0x41, 0x01, 0x41, 0x02];
        workspace.cose.replace(object.clone());
        workspace.cose.save_as(&path).expect("save as");

        assert_eq!(std::fs::read(&path).expect("read"), object);
        assert!(!workspace.cose.meta.dirty);

        let reopened = CborDoc::open(&path).expect("open");
        assert_eq!(reopened.bytes(), object.as_slice());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_all_attempts_every_document_even_after_one_fails() {
        let dir = temp_dir("save-all-partial");
        let mut workspace = Workspace::at_root(&dir);

        // Unnamed, so it cannot be saved.
        workspace.edn = TextDoc::from_text("unsaveable");
        workspace.edn.meta.dirty = true;
        // Named, and comes *after* the EDN in save order.
        workspace.cose = CborDoc::from_bytes(b"saveable".to_vec());
        workspace.cose.meta.path = Some(dir.join("object.cbor"));
        workspace.cose.meta.dirty = true;

        let failures = workspace.save_all();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].source, Source::Edn);
        assert!(
            !workspace.cose.meta.dirty,
            "an earlier failure stranded a later document unsaved"
        );
        assert_eq!(
            std::fs::read(dir.join("object.cbor")).expect("read"),
            b"saveable"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_all_on_a_clean_workspace_writes_nothing() {
        let dir = temp_dir("save-all-clean");
        let mut workspace = Workspace::at_root(&dir);
        // A path that would fail loudly if it were written to: the directory does not exist.
        workspace.cbor.meta.path = Some(dir.join("missing").join("value.cbor"));

        assert!(!workspace.dirty());
        assert_eq!(workspace.save_all(), Vec::new());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_paths_lists_every_kind() {
        let mut workspace = Workspace::new();
        workspace.cbor.meta.path = Some("/tmp/a.cbor".into());
        workspace.edn.meta.path = Some("/tmp/b.edn".into());
        workspace.cose.meta.path = Some("/tmp/c.cbor".into());
        let paths = workspace.open_paths();
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&Path::new("/tmp/b.edn")));
        // None are dirty yet.
        assert!(workspace.dirty_paths().is_empty());
    }

    #[test]
    fn a_text_document_round_trips_through_a_file() {
        let dir = temp_dir("text");
        let path = dir.join("doc.edn");
        let mut doc = TextDoc::from_text("{1: 2}");
        doc.save_as(&path).expect("save_as");
        assert!(!doc.meta.dirty);

        let reopened = TextDoc::open(&path).expect("opens");
        assert_eq!(reopened.text(), "{1: 2}");
        assert!(reopened.meta.read_only, "reopened documents are read-only");
        assert!(!reopened.meta.dirty);
        assert!(!reopened.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
