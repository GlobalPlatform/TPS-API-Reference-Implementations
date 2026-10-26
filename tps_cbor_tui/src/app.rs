/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Application state and mode dispatch.

use std::path::{Path, PathBuf};

use tps_cddl::{
    CborMismatch, Cddl, Config, Diagnostic, ModuleSearchPath, build_cddl, lex, resolve_modules,
    validate_with_config,
};
use tps_cose::CoseKey;
use tps_minicbor::Decoder;

use crate::artifact::{Artifact, ArtifactView};
use crate::error::AppError;
use crate::keystore::{KeyMetaInput, KeyStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    CborView,
    EdnEdit,
    HexEdit,
    CddlEdit,
    CborCheck,
    KeyStore,
    CoseSign,
    CoseEncrypt,
}

/// What an in-progress [`InputPrompt`] will do with the text once submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    GenerateAlias,
    ImportPath,
    LoadCborPath,
    LoadCddlPath,
    SavePath,
    CheckRootRule,
}

/// A single-line text prompt overlaid on the current screen (e.g. "alias:").
pub struct InputPrompt {
    pub kind: InputKind,
    pub editor: crate::ui::text_editor::TextEditor,
}

/// Content pending a write to disk, holding whichever form the source editor uses.
#[derive(Debug, Clone)]
pub enum WriteContent {
    Bytes(Vec<u8>),
    Text(String),
}

/// A save awaiting the user's yes/no answer to an overwrite prompt.
pub struct PendingWrite {
    pub path: PathBuf,
    pub content: WriteContent,
}

pub struct App {
    pub mode: Mode,
    pub artifact: Artifact,
    pub edn_editor: crate::ui::text_editor::TextEditor,
    pub hex_editor: crate::ui::text_editor::TextEditor,
    pub cddl_editor: crate::ui::text_editor::TextEditor,
    pub cddl_path: Option<PathBuf>,
    pub cddl_diagnostics: Vec<Diagnostic>,
    pub cddl_diag_focus: bool,
    pub cddl_diag_selected: usize,
    pub check_root_rule: Option<String>,
    /// The default check root rule seeded from the currently-loaded top-level
    /// CDDL file's own `;# start` directive, if it has one. Reset to `None`
    /// (then re-seeded) only when a *new* CDDL file is loaded — see
    /// [`App::load_cddl_path`]. Once the user types an explicit rule into
    /// the check-root prompt (`check_root_rule` becomes `Some`), that typed
    /// value takes precedence over this field for the rest of the session,
    /// until the next CDDL file load.
    pub cddl_start_rule: Option<String>,
    pub check_mismatches: Vec<CborMismatch>,
    pub check_selected: usize,
    pub check_error: Option<String>,
    pub keystore: KeyStore,
    pub keystore_selected: usize,
    pub active_key_alias: Option<String>,
    pub active_key_bytes: Option<Vec<u8>>,
    pub input_prompt: Option<InputPrompt>,
    pub pending_save_content: Option<WriteContent>,
    pub confirm_overwrite: Option<PendingWrite>,
    pub status: Option<String>,
    pub should_quit: bool,
    pub cbor_scroll: u16,
    pub edn_scroll: u16,
    pub hex_edit_scroll: u16,
    pub cddl_scroll: u16,
    /// Inner content width of the CDDL/EDN panes as last rendered, used to
    /// keep word-wrapped line counts and cursor placement consistent with
    /// what `draw()` actually renders. Refreshed every frame; the default of
    /// 80 only matters before the first frame is drawn.
    pub cddl_pane_width: u16,
    pub edn_pane_width: u16,
}

impl App {
    pub fn new(artifact: Artifact, keystore_dir: &Path) -> Result<Self, AppError> {
        Ok(Self {
            mode: Mode::CborView,
            artifact,
            edn_editor: crate::ui::text_editor::TextEditor::new(""),
            hex_editor: crate::ui::text_editor::TextEditor::new(""),
            cddl_editor: crate::ui::text_editor::TextEditor::new(""),
            cddl_path: None,
            cddl_diagnostics: Vec::new(),
            cddl_diag_focus: false,
            cddl_diag_selected: 0,
            check_root_rule: None,
            cddl_start_rule: None,
            check_mismatches: Vec::new(),
            check_selected: 0,
            check_error: None,
            keystore: KeyStore::open(keystore_dir)?,
            keystore_selected: 0,
            active_key_alias: None,
            active_key_bytes: None,
            input_prompt: None,
            pending_save_content: None,
            confirm_overwrite: None,
            status: None,
            should_quit: false,
            cbor_scroll: 0,
            edn_scroll: 0,
            hex_edit_scroll: 0,
            cddl_scroll: 0,
            cddl_pane_width: 80,
            edn_pane_width: 80,
        })
    }

    /// Number of lines the current mode's content occupies, used to clamp
    /// scrolling so it can't run away into empty space.
    ///
    /// Must match the line count the corresponding `draw()` branch actually
    /// renders, not a naive `str::lines()` count: `highlight_cddl`/
    /// `highlight_edn` emit one *more* line than `str::lines()` counts
    /// whenever the source has a trailing newline, since they split on every
    /// `\n` rather than treating a trailing one as EOF. Undercounting here
    /// clamps scrolling short of the real end of content.
    fn content_line_count(&self) -> usize {
        match self.mode {
            Mode::CddlEdit => {
                crate::ui::wrap::wrapped_line_count(self.cddl_editor.text(), self.cddl_pane_width as usize)
            }
            Mode::EdnEdit => {
                crate::ui::wrap::wrapped_line_count(self.edn_editor.text(), self.edn_pane_width as usize)
            }
            Mode::HexEdit => self.hex_editor.text().lines().count().max(1),
            _ => match self.artifact.view {
                ArtifactView::Hex => self.artifact.bytes.len().div_ceil(16).max(1),
                ArtifactView::Edn => {
                    crate::ui::wrap::wrapped_line_count(self.edn_editor.text(), self.edn_pane_width as usize)
                }
            },
        }
    }

    fn scroll_field_mut(&mut self) -> &mut u16 {
        match self.mode {
            Mode::CddlEdit => &mut self.cddl_scroll,
            Mode::EdnEdit => &mut self.edn_scroll,
            Mode::HexEdit => &mut self.hex_edit_scroll,
            _ => &mut self.cbor_scroll,
        }
    }

    pub fn scroll_up(&mut self, lines: u16) {
        let field = self.scroll_field_mut();
        *field = field.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        let max = self.content_line_count().saturating_sub(1) as u16;
        let field = self.scroll_field_mut();
        *field = (*field + lines).min(max);
    }

    /// Re-lex/build/validate the CDDL editor's current contents, updating
    /// `cddl_diagnostics`. A parse failure is surfaced as a single
    /// diagnostic at the parse error's location rather than propagated.
    pub fn revalidate_cddl(&mut self) {
        let source = self.cddl_editor.text();
        self.cddl_diagnostics = match lex(source).and_then(build_cddl) {
            Ok(cddl) => validate_with_config(&cddl, &Config::default()),
            Err(err) => vec![parse_error_diagnostic(&err)],
        };
        if self.cddl_diagnostics.is_empty() {
            self.cddl_diag_focus = false;
            self.cddl_diag_selected = 0;
        } else {
            self.cddl_diag_selected = self.cddl_diag_selected.min(self.cddl_diagnostics.len() - 1);
        }
    }

    /// Parse the CDDL editor's contents, returning the built [`Cddl`] on
    /// success (used to drive validate-against-artifact once schema and
    /// artifact are both loaded).
    pub fn parsed_cddl(&self) -> Result<Cddl, String> {
        let pairs = lex(self.cddl_editor.text()).map_err(|e| e.to_string())?;
        build_cddl(pairs).map_err(|e| e.to_string())
    }

    /// Build a [`Cddl`] for CBOR-content checking, resolving `;# import`/
    /// `;# include` module directives when the CDDL was loaded from (or
    /// already saved to) a file on disk. Falls back to parsing the in-editor
    /// text with no module resolution when there's no known path yet.
    fn parsed_cddl_for_check(&self) -> Result<Cddl, String> {
        match &self.cddl_path {
            Some(path) => {
                let extra: Vec<PathBuf> = path.parent().map(|p| p.to_path_buf()).into_iter().collect();
                let search = ModuleSearchPath::from_env_and_cwd(&extra).map_err(|e| e.to_string())?;
                let resolution = resolve_modules(path, &search).map_err(|e| e.to_string())?;
                Ok(resolution.cddl)
            }
            None => self.parsed_cddl(),
        }
    }

    /// Open a prompt for which CDDL rule to check the current artifact
    /// against. Pre-filled with, in order of precedence: the last rule the
    /// user explicitly typed this session (`check_root_rule`), else the
    /// loaded CDDL file's own `;# start` rule (`cddl_start_rule`), else empty
    /// ("use the schema's first rule").
    pub fn start_check_prompt(&mut self) {
        if self.cddl_editor.text().trim().is_empty() {
            self.status = Some("no CDDL loaded to check against".to_owned());
            return;
        }
        let default = self
            .check_root_rule
            .clone()
            .or_else(|| self.cddl_start_rule.clone())
            .unwrap_or_default();
        self.input_prompt = Some(InputPrompt {
            kind: InputKind::CheckRootRule,
            editor: crate::ui::text_editor::TextEditor::new(default),
        });
    }

    /// Check the current artifact's CBOR bytes against the loaded CDDL
    /// schema (rooted at `root`, or the schema's first rule if `None`),
    /// switching to [`Mode::CborCheck`] to show the results.
    fn run_check(&mut self, root: Option<String>) {
        self.check_root_rule = root.clone();
        self.check_selected = 0;
        match self.parsed_cddl_for_check() {
            Ok(cddl) => match crate::validate::validate_artifact_against_cddl(
                &self.artifact,
                &cddl,
                root.as_deref(),
                &Config::default(),
            ) {
                Ok(mismatches) => {
                    self.check_mismatches = mismatches;
                    self.check_error = None;
                }
                Err(e) => {
                    self.check_mismatches.clear();
                    self.check_error = Some(e);
                }
            },
            Err(e) => {
                self.check_mismatches.clear();
                self.check_error = Some(e);
            }
        }
        self.mode = Mode::CborCheck;
    }

    pub fn check_move_selection(&mut self, delta: isize) {
        let len = self.check_mismatches.len();
        if len == 0 {
            self.check_selected = 0;
            return;
        }
        let new = (self.check_selected as isize + delta).clamp(0, len as isize - 1);
        self.check_selected = new as usize;
    }

    /// Leave the check-results view, returning to the CBOR/EDN base view.
    pub fn exit_check(&mut self) {
        self.mode = Mode::CborView;
    }

    /// Move keyboard focus into the CDDL Diagnostics pane, if there's
    /// anything there to select.
    pub fn focus_cddl_diagnostics(&mut self) {
        if self.cddl_diagnostics.is_empty() {
            return;
        }
        self.cddl_diag_focus = true;
        self.cddl_diag_selected = self.cddl_diag_selected.min(self.cddl_diagnostics.len() - 1);
    }

    /// Return focus to the CDDL source pane.
    pub fn unfocus_cddl_diagnostics(&mut self) {
        self.cddl_diag_focus = false;
    }

    pub fn cddl_diag_move_selection(&mut self, delta: isize) {
        let len = self.cddl_diagnostics.len();
        if len == 0 {
            self.cddl_diag_selected = 0;
            return;
        }
        let new = (self.cddl_diag_selected as isize + delta).clamp(0, len as isize - 1);
        self.cddl_diag_selected = new as usize;
    }

    /// Move the CDDL source cursor and scroll to the currently-selected
    /// diagnostic's location, so the user can see and fix the error.
    pub fn jump_to_selected_cddl_diagnostic(&mut self) {
        let Some(diag) = self.cddl_diagnostics.get(self.cddl_diag_selected) else {
            return;
        };
        let offset = diag.span.start;
        self.cddl_editor.set_cursor(offset);
        let (line, _) = tps_cddl::ast::offset_to_line_col(self.cddl_editor.text(), offset);
        self.cddl_scroll = (line - 1) as u16;
        self.cddl_diag_focus = false;
    }

    /// Toggle between the hex and EDN views of the current artifact. An
    /// empty artifact (e.g. no file loaded yet) switches to an empty EDN
    /// editor rather than failing, so EDN can be loaded/typed from a fresh
    /// session. A non-empty artifact that can't be decoded as CBOR stays on
    /// the hex view and reports the error via `status` rather than crashing.
    ///
    /// If the artifact has unchanged, cached EDN text (see
    /// [`Artifact::edn_cache`]) it is shown as-is; otherwise the bytes are
    /// rendered fresh with the indented pretty-printer so nested structure
    /// is visible rather than collapsed onto one line.
    pub fn toggle_view(&mut self) {
        match self.artifact.view {
            ArtifactView::Hex if self.artifact.bytes.is_empty() => {
                self.edn_editor = crate::ui::text_editor::TextEditor::new("");
                self.artifact.toggle_view();
                self.mode = Mode::EdnEdit;
                self.edn_scroll = 0;
                self.status = None;
            }
            ArtifactView::Hex => {
                let edn = match self.artifact.edn_cache() {
                    Some(cached) => Ok(cached.to_owned()),
                    None => cbor_to_edn_pretty(&self.artifact.bytes),
                };
                match edn {
                    Ok(edn) => {
                        self.edn_editor = crate::ui::text_editor::TextEditor::new(edn);
                        self.artifact.toggle_view();
                        self.mode = Mode::EdnEdit;
                        self.edn_scroll = 0;
                        self.status = None;
                    }
                    Err(e) => self.status = Some(format!("cannot decode as CBOR: {e}")),
                }
            }
            ArtifactView::Edn => {
                self.artifact.toggle_view();
                self.mode = Mode::CborView;
                self.cbor_scroll = 0;
            }
        }
    }

    /// Enter hex-editing mode, seeding the hex editor from the current bytes.
    pub fn start_hex_edit(&mut self) {
        self.hex_editor = crate::ui::text_editor::TextEditor::new(hex::encode(&self.artifact.bytes));
        self.mode = Mode::HexEdit;
        self.hex_edit_scroll = 0;
        self.status = None;
    }

    /// Discard the in-progress EDN or hex edit and return to the base CBOR
    /// view, without touching `artifact.bytes`. Used to back out of an edit
    /// that failed to parse rather than being stuck re-attempting the same
    /// failed commit.
    pub fn cancel_edit(&mut self) {
        self.artifact.view = ArtifactView::Hex;
        self.mode = Mode::CborView;
        self.cbor_scroll = 0;
        self.status = None;
    }

    /// Parse the EDN editor's current contents and replace the artifact
    /// bytes. On a parse error, stays in `EdnEdit` with a status message
    /// rather than discarding the edit. The just-committed EDN text is
    /// cached verbatim, so toggling back to the EDN view shows exactly what
    /// was typed rather than a freshly-generated rendering of it.
    pub fn commit_edn(&mut self) {
        match tps_minicbor::parse_edn(self.edn_editor.text()) {
            Ok(bytes) => {
                self.artifact.set_bytes(bytes);
                self.artifact.set_edn_cache(self.edn_editor.text().to_owned());
                self.artifact.record("edited as EDN");
                self.artifact.view = ArtifactView::Hex;
                self.mode = Mode::CborView;
                self.cbor_scroll = 0;
            }
            Err(e) => self.status = Some(format!("EDN parse error: {e}")),
        }
    }

    /// Parse the hex editor's current contents and replace the artifact
    /// bytes. On a parse error, stays in `HexEdit` with a status message
    /// rather than discarding the edit.
    pub fn commit_hex(&mut self) {
        let cleaned: String = self.hex_editor.text().chars().filter(|c| !c.is_whitespace()).collect();
        match hex::decode(&cleaned) {
            Ok(bytes) => {
                self.artifact.set_bytes(bytes);
                self.artifact.record("edited as hex");
                self.mode = Mode::CborView;
                self.cbor_scroll = 0;
            }
            Err(e) => self.status = Some(format!("hex parse error: {e}")),
        }
    }

    // --- Key store browsing -------------------------------------------------

    pub fn keystore_move_selection(&mut self, delta: isize) {
        let len = self.keystore.list().len();
        if len == 0 {
            self.keystore_selected = 0;
            return;
        }
        let new = (self.keystore_selected as isize + delta).clamp(0, len as isize - 1);
        self.keystore_selected = new as usize;
    }

    /// Load the currently-selected key's bytes and mark it active for
    /// subsequent sign/verify/encrypt/decrypt operations.
    pub fn set_active_key(&mut self) {
        let Some(meta) = self.keystore.list().get(self.keystore_selected) else {
            return;
        };
        let alias = meta.alias.clone();
        match self.keystore.load(&alias) {
            Ok(bytes) => {
                self.active_key_alias = Some(alias.clone());
                self.active_key_bytes = Some(bytes);
                self.status = Some(format!("active key: {alias}"));
            }
            Err(e) => self.status = Some(format!("error: {e}")),
        }
    }

    pub fn delete_selected_key(&mut self) {
        let Some(meta) = self.keystore.list().get(self.keystore_selected).cloned() else {
            return;
        };
        match self.keystore.delete(&meta.alias) {
            Ok(()) => {
                if self.active_key_alias.as_deref() == Some(meta.alias.as_str()) {
                    self.active_key_alias = None;
                    self.active_key_bytes = None;
                }
                self.status = Some(format!("deleted '{}'", meta.alias));
                let len = self.keystore.list().len();
                if self.keystore_selected >= len && len > 0 {
                    self.keystore_selected = len - 1;
                }
            }
            Err(e) => self.status = Some(format!("error: {e}")),
        }
    }

    pub fn start_generate_prompt(&mut self) {
        self.input_prompt = Some(InputPrompt {
            kind: InputKind::GenerateAlias,
            editor: crate::ui::text_editor::TextEditor::new(""),
        });
    }

    /// Open the key-import path prompt, pre-filled with the key store's own
    /// directory so the user only needs to type the filename.
    pub fn start_import_prompt(&mut self) {
        let prefix = format!("{}{}", self.keystore.dir().display(), std::path::MAIN_SEPARATOR);
        self.input_prompt = Some(InputPrompt {
            kind: InputKind::ImportPath,
            editor: crate::ui::text_editor::TextEditor::new(prefix),
        });
    }

    /// Open a path-entry prompt to load a file, choosing CDDL vs CBOR/EDN
    /// based on the current mode.
    pub fn start_load_prompt(&mut self) {
        let kind = if self.mode == Mode::CddlEdit {
            InputKind::LoadCddlPath
        } else {
            InputKind::LoadCborPath
        };
        self.input_prompt = Some(InputPrompt {
            kind,
            editor: crate::ui::text_editor::TextEditor::new(""),
        });
    }

    /// Pick the content and known save path for the current mode, then
    /// either write immediately (path already known) or prompt for one.
    ///
    /// The known path's extension is swapped to match the representation
    /// being saved (`.edn` for the EDN editor's text, `.cbor` for raw
    /// bytes), so switching views and saving writes a sibling file (e.g.
    /// `foo.edn` -> `foo.cbor`) instead of re-prompting to overwrite the
    /// file that was just loaded.
    pub fn request_save(&mut self) {
        let (content, existing_path) = match self.mode {
            Mode::CddlEdit => (
                WriteContent::Text(self.cddl_editor.text().to_owned()),
                self.cddl_path.clone(),
            ),
            Mode::EdnEdit => (
                WriteContent::Text(self.edn_editor.text().to_owned()),
                self.artifact.source_path.clone().map(|p| with_extension_if_needed(p, "edn")),
            ),
            _ => (
                WriteContent::Bytes(self.artifact.bytes.clone()),
                self.artifact.source_path.clone().map(|p| with_extension_if_needed(p, "cbor")),
            ),
        };

        match existing_path {
            Some(path) => self.begin_write(path, content),
            None => {
                self.pending_save_content = Some(content);
                self.input_prompt = Some(InputPrompt {
                    kind: InputKind::SavePath,
                    editor: crate::ui::text_editor::TextEditor::new(""),
                });
            }
        }
    }

    /// Write `content` to `path`, or ask for overwrite confirmation first if
    /// `path` already exists.
    fn begin_write(&mut self, path: PathBuf, content: WriteContent) {
        if path.exists() {
            self.confirm_overwrite = Some(PendingWrite { path, content });
        } else {
            self.write_now(&path, &content);
        }
    }

    fn write_now(&mut self, path: &Path, content: &WriteContent) {
        let result = match content {
            WriteContent::Bytes(bytes) => std::fs::write(path, bytes),
            WriteContent::Text(text) => std::fs::write(path, text),
        };
        match result {
            Ok(()) => {
                self.status = Some(format!("saved {}", path.display()));
                match self.mode {
                    Mode::CddlEdit => self.cddl_path = Some(path.to_path_buf()),
                    _ => self.artifact.source_path = Some(path.to_path_buf()),
                }
            }
            Err(e) => self.status = Some(format!("save error: {e}")),
        }
    }

    /// Confirm the pending overwrite and perform the write.
    pub fn confirm_yes(&mut self) {
        let Some(pending) = self.confirm_overwrite.take() else {
            return;
        };
        self.write_now(&pending.path, &pending.content);
    }

    /// Cancel the pending overwrite without writing.
    pub fn confirm_no(&mut self) {
        self.confirm_overwrite = None;
        self.status = Some("save cancelled".to_owned());
    }

    /// Load a file into the artifact on the CBOR screen: `.edn` files, and
    /// any file loaded while already in `EdnEdit` (whatever its extension),
    /// are parsed as EDN text; everything else is treated as raw CBOR bytes.
    /// A loaded EDN file's exact text is cached on the artifact, so toggling
    /// to the EDN view shows it verbatim rather than a freshly-generated
    /// rendering.
    fn load_cbor_or_edn_path(&mut self, path: &str) -> Result<(), AppError> {
        let path = PathBuf::from(path);
        let is_edn = self.mode == Mode::EdnEdit
            || path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("edn"));

        let (bytes, edn_text) = if is_edn {
            let text = std::fs::read_to_string(&path)?;
            let bytes = tps_minicbor::parse_edn(&text)?;
            (bytes, Some(text))
        } else {
            (std::fs::read(&path)?, None)
        };

        self.artifact = Artifact::from_file(path, bytes);
        if let Some(text) = edn_text {
            self.artifact.set_edn_cache(text);
        }
        self.mode = Mode::CborView;
        self.cbor_scroll = 0;
        Ok(())
    }

    /// Load a CDDL source file into the CDDL editor.
    ///
    /// Resets `check_root_rule` and re-seeds `cddl_start_rule` from the newly
    /// loaded file's own `;# start` directive (via [`resolve_modules`]), if
    /// it has one — so [`App::start_check_prompt`] defaults to that rule
    /// until the user next types an explicit override or loads another CDDL
    /// file. A directive-resolution error here (e.g. an unresolvable
    /// `;# include`) is not fatal to the load: it only means no start rule
    /// is seeded, matching how `parsed_cddl_for_check` surfaces the same
    /// error separately once the user actually runs a check.
    fn load_cddl_path(&mut self, path: &str) -> Result<(), AppError> {
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path)?;
        self.cddl_editor = crate::ui::text_editor::TextEditor::new(text);
        self.cddl_path = Some(path.clone());
        self.cddl_scroll = 0;
        self.revalidate_cddl();

        self.check_root_rule = None;
        self.cddl_start_rule = None;
        if let Some(parent) = path.parent() {
            let extra = vec![parent.to_path_buf()];
            if let Ok(search) = ModuleSearchPath::from_env_and_cwd(&extra)
                && let Ok(resolution) = resolve_modules(&path, &search)
            {
                self.cddl_start_rule = resolution.start_rule;
            }
        }
        Ok(())
    }

    pub fn cancel_prompt(&mut self) {
        self.input_prompt = None;
        self.pending_save_content = None;
    }

    /// Submit the active input prompt, if any, dispatching on its `kind`.
    pub fn submit_prompt(&mut self) {
        let Some(prompt) = self.input_prompt.take() else {
            return;
        };
        match prompt.kind {
            InputKind::GenerateAlias => {
                let alias = prompt.editor.text().trim().to_owned();
                match self.keystore.generate_symmetric(
                    &alias,
                    tps_cose::EncryptAlgorithm::A128Gcm,
                    "generated in cbor-tui".to_owned(),
                ) {
                    Ok(()) => self.status = Some(format!("generated key '{alias}'")),
                    Err(e) => self.status = Some(format!("error: {e}")),
                }
            }
            InputKind::ImportPath => {
                let path = prompt.editor.text().trim().to_owned();
                match self.import_key_from_path(&path) {
                    Ok(alias) => self.status = Some(format!("imported key '{alias}'")),
                    Err(e) => self.status = Some(format!("error: {e}")),
                }
            }
            InputKind::LoadCborPath => {
                let path = prompt.editor.text().trim().to_owned();
                if let Err(e) = self.load_cbor_or_edn_path(&path) {
                    self.status = Some(format!("load error: {e}"));
                }
            }
            InputKind::LoadCddlPath => {
                let path = prompt.editor.text().trim().to_owned();
                if let Err(e) = self.load_cddl_path(&path) {
                    self.status = Some(format!("load error: {e}"));
                }
            }
            InputKind::SavePath => {
                let path = prompt.editor.text().trim().to_owned();
                if let Some(content) = self.pending_save_content.take() {
                    self.begin_write(PathBuf::from(path), content);
                }
            }
            InputKind::CheckRootRule => {
                let rule = prompt.editor.text().trim().to_owned();
                let root = if rule.is_empty() { None } else { Some(rule) };
                self.run_check(root);
            }
        }
    }

    fn import_key_from_path(&mut self, path: &str) -> Result<String, AppError> {
        let bytes = std::fs::read(path)?;
        let key = CoseKey::decode(&bytes).map_err(|e| AppError::msg(format!("invalid COSE key: {e}")))?;
        let alias = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported")
            .to_owned();
        let meta = key_meta_input_for(&key);
        self.keystore.import(&alias, &bytes, meta)?;
        Ok(alias)
    }

    // --- COSE operations on the current artifact ----------------------------

    pub fn sign_artifact(&mut self) {
        let Some(bytes) = self.active_key_bytes.as_ref() else {
            self.status = Some("no active key".to_owned());
            return;
        };
        let key = match CoseKey::decode(bytes) {
            Ok(k) => k,
            Err(e) => {
                self.status = Some(format!("invalid key: {e}"));
                return;
            }
        };
        match crate::cose_ops::sign(&self.artifact.bytes, &key) {
            Ok(signed) => {
                self.artifact.set_bytes(signed);
                self.artifact
                    .record(format!("signed with '{}'", self.active_key_alias.as_deref().unwrap_or("?")));
                self.status = Some("signed".to_owned());
                self.send_artifact_to_cbor_view();
            }
            Err(e) => self.status = Some(format!("sign error: {e}")),
        }
    }

    pub fn verify_artifact(&mut self) {
        let Some(bytes) = self.active_key_bytes.as_ref() else {
            self.status = Some("no active key".to_owned());
            return;
        };
        let key = match CoseKey::decode(bytes) {
            Ok(k) => k,
            Err(e) => {
                self.status = Some(format!("invalid key: {e}"));
                return;
            }
        };
        match crate::cose_ops::verify(&self.artifact.bytes, &key) {
            Ok(payload) => {
                self.artifact.set_bytes(payload);
                self.artifact.record(format!(
                    "verified with '{}'",
                    self.active_key_alias.as_deref().unwrap_or("?")
                ));
                self.status = Some("verified OK".to_owned());
                self.send_artifact_to_cbor_view();
            }
            Err(e) => self.status = Some(format!("verify error: {e}")),
        }
    }

    pub fn encrypt_artifact(&mut self) {
        let Some(bytes) = self.active_key_bytes.as_ref() else {
            self.status = Some("no active key".to_owned());
            return;
        };
        let key = match CoseKey::decode(bytes) {
            Ok(k) => k,
            Err(e) => {
                self.status = Some(format!("invalid key: {e}"));
                return;
            }
        };
        let alg = match &key {
            CoseKey::Symmetric(k) => k
                .common
                .algorithm
                .and_then(tps_cose::EncryptAlgorithm::try_from_i64)
                .unwrap_or(tps_cose::EncryptAlgorithm::A128Gcm),
            _ => {
                self.status = Some("active key is not symmetric".to_owned());
                return;
            }
        };
        match crate::cose_ops::encrypt(&self.artifact.bytes, &key, alg) {
            Ok(ciphertext) => {
                self.artifact.set_bytes(ciphertext);
                self.artifact.record(format!(
                    "encrypted with '{}'",
                    self.active_key_alias.as_deref().unwrap_or("?")
                ));
                self.status = Some("encrypted".to_owned());
                self.send_artifact_to_cbor_view();
            }
            Err(e) => self.status = Some(format!("encrypt error: {e}")),
        }
    }

    pub fn decrypt_artifact(&mut self) {
        let Some(bytes) = self.active_key_bytes.as_ref() else {
            self.status = Some("no active key".to_owned());
            return;
        };
        let key = match CoseKey::decode(bytes) {
            Ok(k) => k,
            Err(e) => {
                self.status = Some(format!("invalid key: {e}"));
                return;
            }
        };
        match crate::cose_ops::decrypt(&self.artifact.bytes, &key) {
            Ok(plaintext) => {
                self.artifact.set_bytes(plaintext);
                self.artifact.record(format!(
                    "decrypted with '{}'",
                    self.active_key_alias.as_deref().unwrap_or("?")
                ));
                self.status = Some("decrypted".to_owned());
                self.send_artifact_to_cbor_view();
            }
            Err(e) => self.status = Some(format!("decrypt error: {e}")),
        }
    }

    /// After a COSE op produces new artifact bytes, jump back to the CBOR
    /// view showing them, rather than requiring manual re-navigation.
    fn send_artifact_to_cbor_view(&mut self) {
        self.artifact.view = ArtifactView::Hex;
        self.mode = Mode::CborView;
        self.cbor_scroll = 0;
    }

    /// Dismiss the current status/error message, if any.
    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Bytes a hex-copy should act on: the selected key store entry's bytes
    /// while browsing the key store, otherwise the artifact's bytes.
    fn bytes_to_copy(&self) -> Option<Vec<u8>> {
        if self.mode == Mode::KeyStore {
            let meta = self.keystore.list().get(self.keystore_selected)?;
            self.keystore.load(&meta.alias).ok()
        } else {
            Some(self.artifact.bytes.clone())
        }
    }

    /// Copy the current context's bytes to the OS clipboard as an
    /// unformatted hex string, for pasting into documents.
    pub fn copy_hex_to_clipboard(&mut self) {
        let Some(bytes) = self.bytes_to_copy() else {
            self.status = Some("no key selected".to_owned());
            return;
        };
        let hex = hex::encode(&bytes);
        self.status = match arboard::Clipboard::new().and_then(|mut c| c.set_text(hex)) {
            Ok(()) => Some("copied hex to clipboard".to_owned()),
            Err(e) => Some(format!("clipboard error: {e}")),
        };
    }
}

/// Swap `path`'s extension to `ext` if it doesn't already have that
/// extension (case-insensitively), so saving a different representation of
/// the same artifact (e.g. EDN text loaded from `foo.cbor`) targets a
/// sibling file rather than the file that was just loaded.
fn with_extension_if_needed(path: PathBuf, ext: &str) -> PathBuf {
    let already_matches = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if already_matches { path } else { path.with_extension(ext) }
}

/// Convert a CDDL lex/build [`ParseError`](tps_cddl::ParseError) into a
/// single [`Diagnostic`] at the error's source location.
fn parse_error_diagnostic(err: &tps_cddl::ParseError) -> Diagnostic {
    let (start, _) = match err.location() {
        pest::error::InputLocation::Pos(p) => (*p, *p),
        pest::error::InputLocation::Span((s, e)) => (*s, *e),
    };
    Diagnostic {
        level: tps_cddl::Level::Error,
        span: tps_cddl::ast::Span::new(start, start),
        message: err.message(),
    }
}

fn key_meta_input_for(key: &CoseKey<'_>) -> KeyMetaInput {
    match key {
        CoseKey::Ec2(k) => KeyMetaInput {
            algorithm: k.common.algorithm,
            curve: Some(format!("{:?}", k.curve)),
            has_private: k.d.is_some(),
            created_at: "imported in cbor-tui".to_owned(),
        },
        CoseKey::Okp(k) => KeyMetaInput {
            algorithm: k.common.algorithm,
            curve: Some(format!("{:?}", k.curve)),
            has_private: k.d.is_some(),
            created_at: "imported in cbor-tui".to_owned(),
        },
        CoseKey::Symmetric(k) => KeyMetaInput {
            algorithm: k.common.algorithm,
            curve: None,
            has_private: true,
            created_at: "imported in cbor-tui".to_owned(),
        },
    }
}

/// CBOR bytes -> EDN diagnostic text, rejecting trailing bytes.
#[cfg(test)]
fn cbor_to_edn(bytes: &[u8]) -> Result<String, AppError> {
    let dec = Decoder::new(bytes);
    let mut out = String::new();
    dec.write_diagnostic_all_fmt(&mut out)
        .map_err(AppError::Diagnostic)?;
    Ok(out)
}

/// CBOR bytes -> indented, multi-line EDN diagnostic text, rejecting
/// trailing bytes. Used when there is no cached EDN text to show verbatim,
/// so the generated rendering shows nested structure rather than collapsing
/// everything onto one line.
pub fn cbor_to_edn_pretty(bytes: &[u8]) -> Result<String, AppError> {
    let dec = Decoder::new(bytes);
    let mut out = String::new();
    dec.write_diagnostic_pretty_all_fmt(&mut out)
        .map_err(AppError::Diagnostic)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbor_edn_round_trip() {
        let original = tps_minicbor::parse_edn(r#"{1: "a", 2: [true, false, null]}"#).unwrap();
        let edn = cbor_to_edn(&original).unwrap();
        let round_tripped = tps_minicbor::parse_edn(&edn).unwrap();
        assert_eq!(original, round_tripped);
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tps_cbor_tui_app_test_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn generate_select_sign_verify_round_trip() {
        let dir = temp_dir("sign_round_trip");
        let mut app = App::new(Artifact::new(b"payload bytes".to_vec()), &dir).unwrap();

        app.start_generate_prompt();
        app.input_prompt.as_mut().unwrap().editor.insert_str("k1");
        app.submit_prompt();
        assert_eq!(app.keystore.list().len(), 1);

        // The generated key is symmetric, so signing (which requires
        // EC2/OKP) must fail gracefully rather than panic.
        app.set_active_key();
        assert!(app.active_key_bytes.is_some());
        app.sign_artifact();
        assert!(app.status.as_deref().unwrap().contains("sign error"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn encrypt_decrypt_round_trip_via_app() {
        let dir = temp_dir("encrypt_round_trip");
        let plaintext = b"secret artifact bytes".to_vec();
        let mut app = App::new(Artifact::new(plaintext.clone()), &dir).unwrap();

        app.start_generate_prompt();
        app.input_prompt.as_mut().unwrap().editor.insert_str("k1");
        app.submit_prompt();
        app.set_active_key();

        app.encrypt_artifact();
        assert_eq!(app.status.as_deref(), Some("encrypted"));
        assert_ne!(app.artifact.bytes, plaintext);

        app.decrypt_artifact();
        assert_eq!(app.status.as_deref(), Some("decrypted"));
        assert_eq!(app.artifact.bytes, plaintext);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delete_selected_key_clears_active_alias() {
        let dir = temp_dir("delete_active");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.start_generate_prompt();
        app.input_prompt.as_mut().unwrap().editor.insert_str("k1");
        app.submit_prompt();
        app.set_active_key();
        assert!(app.active_key_alias.is_some());

        app.delete_selected_key();
        assert!(app.active_key_alias.is_none());
        assert!(app.active_key_bytes.is_none());
        assert!(app.keystore.list().is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_view_on_empty_artifact_switches_to_empty_edn_editor() {
        let dir = temp_dir("toggle_empty");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);
        assert_eq!(app.artifact.view, ArtifactView::Edn);
        assert_eq!(app.edn_editor.text(), "");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_view_on_undecodable_artifact_sets_status_instead_of_crashing() {
        let dir = temp_dir("toggle_bad");
        let mut app = App::new(Artifact::new(vec![0xff]), &dir).unwrap();
        app.toggle_view();
        assert_eq!(app.mode, Mode::CborView);
        assert_eq!(app.artifact.view, ArtifactView::Hex);
        assert!(app.status.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_edn_file_works_from_a_fresh_session_with_no_cbor_loaded() {
        let dir = temp_dir("load_edn_fresh");
        std::fs::create_dir_all(&dir).unwrap();
        let edn_path = dir.join("fixture.edn");
        std::fs::write(&edn_path, r#"{1: "a"}"#).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);

        app.load_cbor_or_edn_path(edn_path.to_str().unwrap()).unwrap();
        assert_eq!(app.artifact.bytes, tps_minicbor::parse_edn(r#"{1: "a"}"#).unwrap());
        assert_eq!(app.mode, Mode::CborView);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_view_shows_cached_edn_verbatim_after_commit_edn() {
        let dir = temp_dir("toggle_cache_hit");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.edn_editor = crate::ui::text_editor::TextEditor::new("{1: \"a\",\n 2: \"b\"}");
        app.mode = Mode::EdnEdit;
        app.commit_edn();
        assert_eq!(app.mode, Mode::CborView);

        // Toggle to EDN and back to hex, then back to EDN again: the
        // originally-committed text (with its own line break) must come back
        // unchanged, not a freshly pretty-printed rendering.
        app.toggle_view();
        assert_eq!(app.edn_editor.text(), "{1: \"a\",\n 2: \"b\"}");
        app.toggle_view();
        app.toggle_view();
        assert_eq!(app.edn_editor.text(), "{1: \"a\",\n 2: \"b\"}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_view_falls_back_to_pretty_print_after_hex_edit_invalidates_cache() {
        let dir = temp_dir("toggle_cache_invalidate_hex");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.edn_editor = crate::ui::text_editor::TextEditor::new(r#"{1: "a"}"#);
        app.mode = Mode::EdnEdit;
        app.commit_edn();
        assert!(app.artifact.edn_cache().is_some());

        // Editing via hex replaces the bytes with no known EDN text, so the
        // cache must be cleared and the next toggle must regenerate EDN.
        app.start_hex_edit();
        app.hex_editor = crate::ui::text_editor::TextEditor::new("83010203");
        app.commit_hex();
        assert!(app.artifact.edn_cache().is_none());

        app.toggle_view();
        assert_eq!(app.edn_editor.text(), "[\n  1,\n  2,\n  3\n]");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_edn_file_populates_cache_and_toggle_shows_it_verbatim() {
        let dir = temp_dir("load_edn_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let edn_path = dir.join("fixture.edn");
        let text = "{1: \"a\",\n 2: \"b\"}";
        std::fs::write(&edn_path, text).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cbor_or_edn_path(edn_path.to_str().unwrap()).unwrap();
        assert_eq!(app.artifact.edn_cache(), Some(text));

        app.toggle_view();
        assert_eq!(app.edn_editor.text(), text);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scroll_down_clamps_to_content_length() {
        let dir = temp_dir("scroll_clamp");
        let bytes: Vec<u8> = (0..64).collect();
        let mut app = App::new(Artifact::new(bytes), &dir).unwrap();
        // 64 bytes at 16/row = 4 rows, so max scroll offset is 3.
        app.scroll_down(100);
        assert_eq!(app.cbor_scroll, 3);
        app.scroll_up(100);
        assert_eq!(app.cbor_scroll, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_edn_round_trips_valid_input() {
        let dir = temp_dir("commit_edn_ok");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.edn_editor = crate::ui::text_editor::TextEditor::new(r#"{1: "a"}"#);
        app.mode = Mode::EdnEdit;
        app.commit_edn();
        assert_eq!(app.mode, Mode::CborView);
        assert_eq!(app.artifact.bytes, tps_minicbor::parse_edn(r#"{1: "a"}"#).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_edn_on_invalid_input_stays_in_edn_edit_with_status() {
        let dir = temp_dir("commit_edn_bad");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.edn_editor = crate::ui::text_editor::TextEditor::new("not valid edn {{{");
        app.mode = Mode::EdnEdit;
        app.commit_edn();
        assert_eq!(app.mode, Mode::EdnEdit);
        assert!(app.status.as_deref().unwrap().contains("EDN parse error"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_hex_round_trips_valid_input() {
        let dir = temp_dir("commit_hex_ok");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.start_hex_edit();
        app.hex_editor = crate::ui::text_editor::TextEditor::new("48656c6c6f");
        app.commit_hex();
        assert_eq!(app.mode, Mode::CborView);
        assert_eq!(app.artifact.bytes, b"Hello".to_vec());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_hex_on_invalid_input_stays_in_hex_edit_with_status() {
        let dir = temp_dir("commit_hex_bad");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.start_hex_edit();
        app.hex_editor = crate::ui::text_editor::TextEditor::new("zz not hex");
        app.commit_hex();
        assert_eq!(app.mode, Mode::HexEdit);
        assert!(app.status.as_deref().unwrap().contains("hex parse error"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn request_save_writes_immediately_when_no_existing_file() {
        let dir = temp_dir("save_new");
        let mut app = App::new(Artifact::new(b"hello".to_vec()), &dir).unwrap();
        let out_path = dir.join("out.cbor");
        app.artifact.source_path = Some(out_path.clone());
        app.request_save();
        assert!(app.confirm_overwrite.is_none());
        assert_eq!(std::fs::read(&out_path).unwrap(), b"hello".to_vec());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn request_save_prompts_overwrite_for_existing_file_and_only_writes_on_confirm() {
        let dir = temp_dir("save_overwrite");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.cbor");
        std::fs::write(&out_path, b"old").unwrap();

        let mut app = App::new(Artifact::new(b"new".to_vec()), &dir).unwrap();
        app.artifact.source_path = Some(out_path.clone());
        app.request_save();
        assert!(app.confirm_overwrite.is_some());
        assert_eq!(std::fs::read(&out_path).unwrap(), b"old".to_vec());

        app.confirm_no();
        assert!(app.confirm_overwrite.is_none());
        assert_eq!(std::fs::read(&out_path).unwrap(), b"old".to_vec());

        app.artifact.source_path = Some(out_path.clone());
        app.request_save();
        app.confirm_yes();
        assert_eq!(std::fs::read(&out_path).unwrap(), b"new".to_vec());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn request_save_from_edn_editor_writes_sibling_edn_file_not_original_cbor() {
        let dir = temp_dir("save_edn_sibling");
        std::fs::create_dir_all(&dir).unwrap();
        let cbor_path = dir.join("thing.cbor");
        std::fs::write(&cbor_path, [0x01]).unwrap();

        let mut app = App::new(Artifact::new(vec![0x01]), &dir).unwrap();
        app.artifact.source_path = Some(cbor_path.clone());
        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);

        app.request_save();
        assert!(app.confirm_overwrite.is_none(), "should not prompt to overwrite thing.cbor");

        let edn_path = dir.join("thing.edn");
        assert_eq!(std::fs::read_to_string(&edn_path).unwrap(), app.edn_editor.text());
        assert_eq!(std::fs::read(&cbor_path).unwrap(), vec![0x01]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn request_save_from_cbor_view_writes_sibling_cbor_file_not_original_edn() {
        let dir = temp_dir("save_cbor_sibling");
        std::fs::create_dir_all(&dir).unwrap();
        let edn_path = dir.join("thing.edn");
        std::fs::write(&edn_path, r#"{1: "a"}"#).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cbor_or_edn_path(edn_path.to_str().unwrap()).unwrap();
        assert_eq!(app.mode, Mode::CborView);
        assert_eq!(app.artifact.source_path, Some(edn_path.clone()));

        app.request_save();
        assert!(app.confirm_overwrite.is_none(), "should not prompt to overwrite thing.edn");

        let cbor_path = dir.join("thing.cbor");
        assert_eq!(std::fs::read(&cbor_path).unwrap(), app.artifact.bytes);
        assert_eq!(std::fs::read_to_string(&edn_path).unwrap(), r#"{1: "a"}"#);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cbor_or_edn_path_parses_as_edn_when_in_edn_mode_regardless_of_extension() {
        let dir = temp_dir("load_edn_no_ext");
        std::fs::create_dir_all(&dir).unwrap();
        let edn_path = dir.join("fixture.txt");
        std::fs::write(&edn_path, r#"{1: "a"}"#).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);

        app.load_cbor_or_edn_path(edn_path.to_str().unwrap()).unwrap();
        assert_eq!(app.artifact.bytes, tps_minicbor::parse_edn(r#"{1: "a"}"#).unwrap());
        assert_eq!(app.mode, Mode::CborView);

        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);
        assert!(app.status.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cbor_or_edn_path_parses_edn_extension_as_edn() {
        let dir = temp_dir("load_edn");
        std::fs::create_dir_all(&dir).unwrap();
        let edn_path = dir.join("fixture.edn");
        std::fs::write(&edn_path, r#"{1: "a"}"#).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cbor_or_edn_path(edn_path.to_str().unwrap()).unwrap();
        assert_eq!(app.artifact.bytes, tps_minicbor::parse_edn(r#"{1: "a"}"#).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cbor_or_edn_path_treats_other_extensions_as_raw_bytes() {
        let dir = temp_dir("load_cbor");
        std::fs::create_dir_all(&dir).unwrap();
        let cbor_path = dir.join("fixture.cbor");
        let raw = tps_minicbor::parse_edn(r#"[1, 2, 3]"#).unwrap();
        std::fs::write(&cbor_path, &raw).unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cbor_or_edn_path(cbor_path.to_str().unwrap()).unwrap();
        assert_eq!(app.artifact.bytes, raw);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cddl_path_populates_editor_and_revalidates() {
        let dir = temp_dir("load_cddl");
        std::fs::create_dir_all(&dir).unwrap();
        let cddl_path = dir.join("fixture.cddl");
        std::fs::write(&cddl_path, "foo = int").unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cddl_path(cddl_path.to_str().unwrap()).unwrap();
        assert_eq!(app.cddl_editor.text(), "foo = int");
        assert_eq!(app.cddl_path, Some(cddl_path));
        assert!(app.cddl_diagnostics.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scroll_down_on_cddl_reaches_last_line_with_trailing_newline() {
        let dir = temp_dir("cddl_scroll_trailing_newline");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.mode = Mode::CddlEdit;
        // `highlight_cddl` renders one more line than `str::lines()` counts
        // for a trailing newline: 3 rendered lines ("a = uint", "b = tstr",
        // "") vs. 2 per `str::lines()`. Scrolling must reach the true last
        // rendered line (offset 2), not stop short at 1.
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = uint\nb = tstr\n");
        app.scroll_down(100);
        assert_eq!(app.cddl_scroll, 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn focus_cddl_diagnostics_is_noop_when_there_are_none() {
        let dir = temp_dir("diag_focus_empty");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.mode = Mode::CddlEdit;
        app.focus_cddl_diagnostics();
        assert!(!app.cddl_diag_focus);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn jump_to_selected_cddl_diagnostic_moves_cursor_and_scroll() {
        let dir = temp_dir("diag_jump");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.mode = Mode::CddlEdit;
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = int\nb = @@@\nc = tstr");
        app.revalidate_cddl();
        assert!(!app.cddl_diagnostics.is_empty());

        app.focus_cddl_diagnostics();
        assert!(app.cddl_diag_focus);

        app.jump_to_selected_cddl_diagnostic();
        assert!(!app.cddl_diag_focus);
        let (line, _) = tps_cddl::ast::offset_to_line_col(app.cddl_editor.text(), app.cddl_editor.cursor());
        assert_eq!(app.cddl_scroll as usize, line - 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cddl_diag_move_selection_clamps_to_bounds() {
        let dir = temp_dir("diag_move");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.mode = Mode::CddlEdit;
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = @@@\nb = @@@");
        app.revalidate_cddl();
        app.focus_cddl_diagnostics();

        app.cddl_diag_move_selection(-5);
        assert_eq!(app.cddl_diag_selected, 0);
        app.cddl_diag_move_selection(5);
        assert_eq!(app.cddl_diag_selected, app.cddl_diagnostics.len() - 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_import_prompt_prefills_keystore_dir() {
        let dir = temp_dir("import_prefill");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.start_import_prompt();
        let text = app.input_prompt.unwrap().editor.text().to_owned();
        assert!(text.starts_with(&dir.display().to_string()));
        assert!(text.ends_with(std::path::MAIN_SEPARATOR));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_check_prompt_is_noop_with_no_cddl_loaded() {
        let dir = temp_dir("check_prompt_empty");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.start_check_prompt();
        assert!(app.input_prompt.is_none());
        assert!(app.status.as_deref().unwrap().contains("no CDDL loaded"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_check_prompt_prefills_last_used_rule() {
        let dir = temp_dir("check_prompt_prefill");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = int");
        app.check_root_rule = Some("a".to_owned());
        app.start_check_prompt();
        assert_eq!(app.input_prompt.unwrap().editor.text(), "a");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cddl_path_seeds_cddl_start_rule_from_start_directive() {
        let dir = temp_dir("start_rule_seed");
        std::fs::create_dir_all(&dir).unwrap();
        let cddl_path = dir.join("fixture.cddl");
        std::fs::write(&cddl_path, ";# start foo\nfoo = int").unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cddl_path(cddl_path.to_str().unwrap()).unwrap();
        assert_eq!(app.cddl_start_rule, Some("foo".to_owned()));
        assert_eq!(app.check_root_rule, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_check_prompt_prefills_cddl_start_rule_when_no_explicit_override() {
        let dir = temp_dir("start_rule_prefill");
        std::fs::create_dir_all(&dir).unwrap();
        let cddl_path = dir.join("fixture.cddl");
        std::fs::write(&cddl_path, ";# start foo\nfoo = int").unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cddl_path(cddl_path.to_str().unwrap()).unwrap();
        app.start_check_prompt();
        assert_eq!(app.input_prompt.unwrap().editor.text(), "foo");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn check_root_rule_takes_precedence_over_cddl_start_rule() {
        let dir = temp_dir("start_rule_precedence");
        std::fs::create_dir_all(&dir).unwrap();
        let cddl_path = dir.join("fixture.cddl");
        std::fs::write(&cddl_path, ";# start foo\nfoo = int\nbar = tstr").unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cddl_path(cddl_path.to_str().unwrap()).unwrap();
        app.check_root_rule = Some("bar".to_owned());
        app.start_check_prompt();
        assert_eq!(app.input_prompt.unwrap().editor.text(), "bar");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_cddl_path_resets_and_reseeds_cddl_start_rule_on_new_file() {
        let dir = temp_dir("start_rule_reset");
        std::fs::create_dir_all(&dir).unwrap();
        let first_path = dir.join("first.cddl");
        std::fs::write(&first_path, ";# start foo\nfoo = int").unwrap();
        let second_path = dir.join("second.cddl");
        std::fs::write(&second_path, "bar = tstr").unwrap();

        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.load_cddl_path(first_path.to_str().unwrap()).unwrap();
        assert_eq!(app.cddl_start_rule, Some("foo".to_owned()));

        app.check_root_rule = Some("foo".to_owned());
        app.load_cddl_path(second_path.to_str().unwrap()).unwrap();
        assert_eq!(app.cddl_start_rule, None);
        assert_eq!(app.check_root_rule, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_check_reports_no_mismatches_for_conforming_artifact() {
        let dir = temp_dir("check_conforms");
        let mut app = App::new(Artifact::new(vec![0x01]), &dir).unwrap();
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = uint");
        app.mode = Mode::CborView;

        app.start_check_prompt();
        app.submit_prompt();

        assert_eq!(app.mode, Mode::CborCheck);
        assert!(app.check_error.is_none());
        assert!(app.check_mismatches.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_check_reports_mismatches_for_non_conforming_artifact() {
        let dir = temp_dir("check_mismatch");
        // tstr "a" (0x61 0x61) does not match `a = uint`.
        let mut app = App::new(Artifact::new(vec![0x61, 0x61]), &dir).unwrap();
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = uint");
        app.mode = Mode::CborView;

        app.start_check_prompt();
        app.submit_prompt();

        assert_eq!(app.mode, Mode::CborCheck);
        assert!(app.check_error.is_none());
        assert!(!app.check_mismatches.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_check_resolves_submodule_rules_via_include_directive() {
        let dir = temp_dir("check_modules");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sub.cddl"), "b = uint").unwrap();
        let top_path = dir.join("top.cddl");
        std::fs::write(&top_path, ";# include sub\na = b").unwrap();

        let mut app = App::new(Artifact::new(vec![0x01]), &dir).unwrap();
        app.load_cddl_path(top_path.to_str().unwrap()).unwrap();
        app.mode = Mode::CborView;

        app.start_check_prompt();
        app.input_prompt.as_mut().unwrap().editor.insert_str("a");
        app.submit_prompt();

        assert_eq!(app.mode, Mode::CborCheck);
        assert!(app.check_error.is_none(), "check_error: {:?}", app.check_error);
        assert!(app.check_mismatches.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn check_move_selection_clamps_to_bounds() {
        let dir = temp_dir("check_move");
        let mut app = App::new(Artifact::new(vec![0x61, 0x61]), &dir).unwrap();
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = uint\na //= tstr");
        app.mode = Mode::CborView;
        app.start_check_prompt();
        app.submit_prompt();

        app.check_move_selection(-5);
        assert_eq!(app.check_selected, 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exit_check_returns_to_cbor_view() {
        let dir = temp_dir("check_exit");
        let mut app = App::new(Artifact::new(vec![0x01]), &dir).unwrap();
        app.cddl_editor = crate::ui::text_editor::TextEditor::new("a = uint");
        app.mode = Mode::CborView;
        app.start_check_prompt();
        app.submit_prompt();
        assert_eq!(app.mode, Mode::CborCheck);

        app.exit_check();
        assert_eq!(app.mode, Mode::CborView);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_edit_discards_edn_edit_without_touching_bytes() {
        let dir = temp_dir("cancel_edit_edn");
        let original = tps_minicbor::parse_edn(r#"{1: "a"}"#).unwrap();
        let mut app = App::new(Artifact::new(original.clone()), &dir).unwrap();
        app.toggle_view();
        assert_eq!(app.mode, Mode::EdnEdit);
        app.edn_editor = crate::ui::text_editor::TextEditor::new("not valid edn {{{");
        app.commit_edn();
        assert!(app.status.is_some());

        app.cancel_edit();
        assert_eq!(app.mode, Mode::CborView);
        assert_eq!(app.artifact.view, ArtifactView::Hex);
        assert!(app.status.is_none());
        assert_eq!(app.artifact.bytes, original);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_edit_discards_hex_edit_without_touching_bytes() {
        let dir = temp_dir("cancel_edit_hex");
        let original = b"hello".to_vec();
        let mut app = App::new(Artifact::new(original.clone()), &dir).unwrap();
        app.start_hex_edit();
        app.hex_editor = crate::ui::text_editor::TextEditor::new("zz not hex");
        app.commit_hex();
        assert!(app.status.is_some());

        app.cancel_edit();
        assert_eq!(app.mode, Mode::CborView);
        assert!(app.status.is_none());
        assert_eq!(app.artifact.bytes, original);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clear_status_clears_pending_message() {
        let dir = temp_dir("clear_status");
        let mut app = App::new(Artifact::new(Vec::new()), &dir).unwrap();
        app.status = Some("something happened".to_owned());
        app.clear_status();
        assert!(app.status.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bytes_to_copy_uses_artifact_bytes_outside_keystore_mode() {
        let dir = temp_dir("bytes_to_copy_artifact");
        let bytes = b"artifact bytes".to_vec();
        let app = App::new(Artifact::new(bytes.clone()), &dir).unwrap();
        assert_eq!(app.bytes_to_copy(), Some(bytes));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bytes_to_copy_uses_selected_key_bytes_in_keystore_mode() {
        let dir = temp_dir("bytes_to_copy_keystore");
        let mut app = App::new(Artifact::new(b"artifact bytes".to_vec()), &dir).unwrap();
        app.start_generate_prompt();
        app.input_prompt.as_mut().unwrap().editor.insert_str("k1");
        app.submit_prompt();
        app.mode = Mode::KeyStore;

        let key_bytes = app.keystore.load("k1").unwrap();
        assert_eq!(app.bytes_to_copy(), Some(key_bytes));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bytes_to_copy_is_none_in_keystore_mode_with_no_keys() {
        let dir = temp_dir("bytes_to_copy_keystore_empty");
        let mut app = App::new(Artifact::new(b"artifact bytes".to_vec()), &dir).unwrap();
        app.mode = Mode::KeyStore;
        assert_eq!(app.bytes_to_copy(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
