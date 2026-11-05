/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Persisted settings, as TOML in the platform configuration directory.
//!
//! Replaces the old GUI's `localStorage`, which was per-webview, invisible to the user, unreadable
//! outside the app and unavailable to the TUI. A TOML file is editable by hand, diffable, and
//! shared by both frontends.
//!
//! # Forward compatibility
//!
//! [`Settings::load`] never fails on an unreadable or malformed file: it reports the problem as a
//! [`Diagnostic`](crate::Diagnostic) alongside the defaults. Losing a window size is not worth
//! refusing to start, and a user who hand-edits the file deserves to be told what is wrong rather
//! than to have their file silently replaced. Unknown keys are preserved on load and written back
//! on save, so a newer version's settings survive a round trip through an older one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::{Diagnostic, Source};

/// The file name within the configuration directory.
pub const FILE_NAME: &str = "settings.toml";

/// The directory name under the platform config root, shared by both frontends.
pub const APP_DIR: &str = "tps-cbor";

/// The lower (diagnostics) pane's share of the centre column on first run.
///
/// `UI_DESIGN.md` specifies "approximately 15%".
pub const DEFAULT_DIAGNOSTICS_FRACTION: f32 = 0.15;

/// Everything the frontends remember between runs.
///
/// Every field has a `#[serde(default)]`, so a file written by an older version loads cleanly and
/// a hand-written file may contain only the keys the user cares about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The folder the explorer opens, if the user has opened one.
    pub workspace_root: Option<PathBuf>,

    /// Directories searched for `;# import` and `;# include` targets, in order.
    ///
    /// A `Vec<PathBuf>` rather than the old backend's single `String` of
    /// platform-separator-joined paths, which had to be split at every use site and could not
    /// represent a path containing the separator.
    pub module_search_paths: Vec<PathBuf>,

    /// Where the COSE key store lives. `None` means `<config dir>/keys`.
    pub keystore_dir: Option<PathBuf>,

    /// Accept RFC 9165 control operators without warning.
    pub rfc9165: bool,

    /// Accept RFC 9682 grammar extensions.
    pub rfc9682: bool,

    /// The CDDL rule validation starts from. `None` means the first rule in the document.
    pub start_rule: Option<String>,

    /// Pane sizes and window geometry.
    pub layout: Layout,

    /// Which trace outcomes the tracer's filter shows.
    pub trace_filter: TraceFilter,

    /// Keys present in the file that this version does not know about.
    ///
    /// Captured so that saving does not discard a newer version's settings. Not part of the public
    /// contract; do not read it.
    #[serde(flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub unknown: toml::Table,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            workspace_root: None,
            module_search_paths: Vec::new(),
            keystore_dir: None,
            // Both default on: every schema in `examples/` uses at least one RFC 9165 operator,
            // and strict mode would greet a new user with warnings on the shipped fixtures.
            rfc9165: true,
            rfc9682: true,
            start_rule: None,
            layout: Layout::default(),
            trace_filter: TraceFilter::default(),
            unknown: toml::Table::new(),
        }
    }
}

/// Window geometry and pane sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Window size in logical pixels, if a window has been sized.
    pub window: Option<(f32, f32)>,
    /// The explorer pane's width in logical pixels.
    pub explorer_width: Option<f32>,
    /// The options pane's width in logical pixels.
    pub options_width: Option<f32>,
    /// The diagnostics pane's share of the centre column, as a fraction in `0.05..=0.9`.
    pub diagnostics_fraction: f32,
    /// Whether the options pane is shown.
    pub options_visible: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            window: None,
            explorer_width: None,
            options_width: None,
            diagnostics_fraction: DEFAULT_DIAGNOSTICS_FRACTION,
            options_visible: true,
        }
    }
}

/// Which trace outcomes the tracer's filter shows.
///
/// Defaults to match + fail and not attempt, following `TraceLog.tsx:66`: a trace over a real
/// schema is mostly attempts, and showing them by default buried the useful rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraceFilter {
    /// Show rows where a rule was tried.
    pub attempt: bool,
    /// Show rows where a rule matched.
    pub matched: bool,
    /// Show rows where a rule failed.
    pub failed: bool,
}

impl Default for TraceFilter {
    fn default() -> Self {
        Self {
            attempt: false,
            matched: true,
            failed: true,
        }
    }
}

impl Settings {
    /// Load settings from `path`, falling back to defaults for anything missing or broken.
    ///
    /// Returns the settings and any diagnostics describing what could not be read. A missing file
    /// is not a problem and produces no diagnostic; an unreadable or malformed one produces
    /// exactly one.
    pub fn load(path: &Path) -> (Self, Vec<Diagnostic>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            // Nothing saved yet: the overwhelmingly common case on first run.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), Vec::new());
            }
            Err(error) => {
                return (
                    Self::default(),
                    vec![
                        Diagnostic::warning(
                            Source::Settings,
                            0..0,
                            format!("could not read settings: {error}"),
                        )
                        .in_file(path),
                    ],
                );
            }
        };

        match toml::from_str::<Self>(&text) {
            Ok(settings) => (settings.clamped(), Vec::new()),
            Err(error) => {
                // toml reports a byte range into the text, which the editor can squiggle directly.
                let span = error.span().unwrap_or(0..0);
                (
                    Self::default(),
                    vec![
                        Diagnostic::warning(
                            Source::Settings,
                            span,
                            format!("ignoring malformed settings: {}", error.message()),
                        )
                        .in_file(path),
                    ],
                )
            }
        }
    }

    /// Load settings from the default location for this platform.
    ///
    /// Also reports a diagnostic if the config directory itself could not be determined, in which
    /// case nothing will be persisted this session.
    pub fn load_default() -> (Self, Vec<Diagnostic>) {
        match default_config_dir() {
            Some(dir) => Self::load(&dir.join(FILE_NAME)),
            None => (
                Self::default(),
                vec![Diagnostic::warning(
                    Source::Settings,
                    0..0,
                    "no configuration directory: settings will not be saved this session",
                )],
            ),
        }
    }

    /// Write settings to `path`, creating the parent directory if needed.
    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| crate::CoreError::io(parent, error))?;
        }
        let text = toml::to_string_pretty(self).map_err(|error| {
            crate::CoreError::Settings(format!("could not serialise settings: {error}"))
        })?;
        std::fs::write(path, text).map_err(|error| crate::CoreError::io(path, error))?;
        Ok(())
    }

    /// Write settings to the default location for this platform.
    pub fn save_default(&self) -> crate::Result<()> {
        let dir = default_config_dir().ok_or_else(|| {
            crate::CoreError::Settings("no configuration directory on this platform".to_owned())
        })?;
        self.save(&dir.join(FILE_NAME))
    }

    /// The key store directory, resolving the `None` default.
    pub fn keystore_dir(&self) -> Option<PathBuf> {
        self.keystore_dir
            .clone()
            .or_else(|| default_config_dir().map(|dir| dir.join("keys")))
    }

    /// The validation config these settings describe.
    pub fn cddl_config(&self) -> tps_cddl::Config {
        tps_cddl::Config {
            rfc9165: self.rfc9165,
            rfc9682: self.rfc9682,
        }
    }

    /// Bring out-of-range values back into range.
    ///
    /// A hand-edited `diagnostics_fraction = 400` would otherwise collapse the editor to nothing
    /// with no way to drag it back, since the dragged size is itself what gets saved.
    fn clamped(mut self) -> Self {
        let f = self.layout.diagnostics_fraction;
        self.layout.diagnostics_fraction = if f.is_finite() {
            f.clamp(0.05, 0.9)
        } else {
            DEFAULT_DIAGNOSTICS_FRACTION
        };
        self.layout.window = self
            .layout
            .window
            .filter(|(w, h)| w.is_finite() && h.is_finite() && *w >= 320.0 && *h >= 240.0);
        self.layout.explorer_width = self
            .layout
            .explorer_width
            .filter(|w| w.is_finite() && *w >= 0.0);
        self.layout.options_width = self
            .layout
            .options_width
            .filter(|w| w.is_finite() && *w >= 0.0);
        self
    }
}

/// The platform configuration directory for this application, or `None` if it cannot be
/// determined.
///
/// Reads the environment directly rather than using the `dirs` crate, which reaches `option-ext`
/// (MPL-2.0) and would put a licence exception obligation on `tps_cbor_tui` for the sake of a
/// dozen lines. `tps_cbor_ide` acquires `option-ext` through GPUI's font loading regardless, but
/// that is GPUI's choice and not one this crate needs to propagate.
///
/// The rules are the platform conventions `dirs` implements:
///
/// - **Unix**: `$XDG_CONFIG_HOME/tps-cbor`, else `$HOME/.config/tps-cbor`.
/// - **Windows**: `%APPDATA%\tps-cbor`.
/// - **macOS**: `$HOME/Library/Application Support/tps-cbor`.
///
/// Per the XDG base directory specification, a relative `XDG_CONFIG_HOME` is invalid and ignored.
pub fn default_config_dir() -> Option<PathBuf> {
    config_root().map(|root| root.join(APP_DIR))
}

fn config_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return non_empty("APPDATA").map(PathBuf::from);
    }

    #[cfg(target_os = "macos")]
    {
        return non_empty("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = non_empty("XDG_CONFIG_HOME") {
            let path = PathBuf::from(xdg);
            // The spec requires an absolute path; a relative one is to be ignored, not resolved
            // against the current directory.
            if path.is_absolute() {
                return Some(path);
            }
        }
        non_empty("HOME").map(|home| PathBuf::from(home).join(".config"))
    }

    #[cfg(not(any(windows, unix)))]
    {
        None
    }
}

/// `std::env::var` with the empty string treated as unset, which is how an unset variable is
/// usually spelled in a shell that exports it anyway.
fn non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tps-cbor-settings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn a_missing_file_yields_defaults_without_complaint() {
        let dir = temp_dir("missing");
        let (settings, diagnostics) = Settings::load(&dir.join("nope.toml"));
        assert_eq!(settings, Settings::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn settings_round_trip_through_a_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(FILE_NAME);

        let mut written = Settings {
            workspace_root: Some(PathBuf::from("/tmp/project")),
            module_search_paths: vec![PathBuf::from("/a"), PathBuf::from("/b c")],
            start_rule: Some("claims".to_owned()),
            rfc9165: false,
            ..Settings::default()
        };
        written.layout.window = Some((1600.0, 900.0));
        written.layout.diagnostics_fraction = 0.25;
        written.layout.options_visible = false;
        written.trace_filter.attempt = true;

        written.save(&path).expect("save");
        let (read, diagnostics) = Settings::load(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(read, written);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A search path containing the platform separator must survive, which the old
    /// separator-joined `String` could not represent.
    #[test]
    fn a_search_path_containing_a_colon_survives() {
        let dir = temp_dir("colon");
        let path = dir.join(FILE_NAME);
        let written = Settings {
            module_search_paths: vec![PathBuf::from("/odd:name/schemas")],
            ..Settings::default()
        };
        written.save(&path).expect("save");
        let (read, _) = Settings::load(&path);
        assert_eq!(read.module_search_paths, written.module_search_paths);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_creates_the_parent_directory() {
        let dir = temp_dir("mkdir");
        let path = dir.join("a").join("b").join(FILE_NAME);
        Settings::default().save(&path).expect("save");
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_file_is_reported_and_defaults_are_used() {
        let dir = temp_dir("malformed");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "rfc9165 = = = nonsense\n").expect("write");

        let (settings, diagnostics) = Settings::load(&path);
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, Source::Settings);
        assert_eq!(diagnostics[0].file.as_deref(), Some(path.as_path()));
        assert!(
            diagnostics[0].message.contains("malformed"),
            "{}",
            diagnostics[0].message
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A partial file is legitimate: everything absent falls back to the default.
    #[test]
    fn a_partial_file_loads_with_defaults_for_the_rest() {
        let dir = temp_dir("partial");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "rfc9165 = false\n").expect("write");

        let (settings, diagnostics) = Settings::load(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(!settings.rfc9165);
        assert!(settings.rfc9682, "unlisted keys should keep their default");
        assert_eq!(
            settings.layout.diagnostics_fraction,
            DEFAULT_DIAGNOSTICS_FRACTION
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A newer version's keys must survive a load/save cycle through this one.
    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let dir = temp_dir("unknown");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "rfc9165 = false\nfuture_feature = \"on\"\n").expect("write");

        let (settings, diagnostics) = Settings::load(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        settings.save(&path).expect("save");

        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(text.contains("future_feature"), "key was dropped:\n{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_absurd_pane_fraction_is_clamped_rather_than_obeyed() {
        let dir = temp_dir("clamp");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "[layout]\ndiagnostics_fraction = 400.0\n").expect("write");
        let (settings, _) = Settings::load(&path);
        assert_eq!(settings.layout.diagnostics_fraction, 0.9);

        std::fs::write(&path, "[layout]\ndiagnostics_fraction = -3.0\n").expect("write");
        let (settings, _) = Settings::load(&path);
        assert_eq!(settings.layout.diagnostics_fraction, 0.05);

        std::fs::write(&path, "[layout]\ndiagnostics_fraction = nan\n").expect("write");
        let (settings, _) = Settings::load(&path);
        assert_eq!(
            settings.layout.diagnostics_fraction,
            DEFAULT_DIAGNOSTICS_FRACTION
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unusably_small_window_is_discarded() {
        let dir = temp_dir("window");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "[layout]\nwindow = [4.0, 4.0]\n").expect("write");
        let (settings, _) = Settings::load(&path);
        assert_eq!(settings.layout.window, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_trace_filter_defaults_to_match_and_fail() {
        let filter = TraceFilter::default();
        assert!(!filter.attempt, "attempt rows would bury the useful ones");
        assert!(filter.matched);
        assert!(filter.failed);
    }

    #[test]
    fn the_cddl_config_follows_the_rfc_flags() {
        let settings = Settings {
            rfc9165: true,
            rfc9682: false,
            ..Settings::default()
        };
        let config = settings.cddl_config();
        assert!(config.rfc9165);
        assert!(!config.rfc9682);
    }

    #[test]
    fn the_default_config_dir_ends_in_the_app_directory() {
        if let Some(dir) = default_config_dir() {
            assert!(dir.is_absolute(), "{dir:?} is not absolute");
            assert_eq!(dir.file_name().and_then(|n| n.to_str()), Some(APP_DIR));
        }
    }

    #[test]
    fn the_keystore_dir_defaults_under_the_config_dir() {
        let settings = Settings::default();
        match (settings.keystore_dir(), default_config_dir()) {
            (Some(keys), Some(config)) => assert_eq!(keys, config.join("keys")),
            (keys, None) => assert_eq!(keys, None),
            (None, Some(_)) => panic!("config dir exists but keystore dir does not"),
        }
    }

    #[test]
    fn an_explicit_keystore_dir_wins() {
        let settings = Settings {
            keystore_dir: Some(PathBuf::from("/srv/keys")),
            ..Settings::default()
        };
        assert_eq!(settings.keystore_dir(), Some(PathBuf::from("/srv/keys")));
    }

    #[test]
    fn an_empty_environment_variable_counts_as_unset() {
        // SAFETY: single-threaded within this test, and the key is unique to it.
        unsafe {
            std::env::set_var("TPS_CBOR_TEST_EMPTY", "");
        }
        assert_eq!(non_empty("TPS_CBOR_TEST_EMPTY"), None);
        unsafe {
            std::env::set_var("TPS_CBOR_TEST_EMPTY", "x");
        }
        assert_eq!(non_empty("TPS_CBOR_TEST_EMPTY").as_deref(), Some("x"));
        unsafe {
            std::env::remove_var("TPS_CBOR_TEST_EMPTY");
        }
    }
}
