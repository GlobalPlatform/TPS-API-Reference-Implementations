/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Shared artifact abstraction that flows between every screen.

use std::path::PathBuf;

/// Which representation of the current artifact is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactView {
    Hex,
    Edn,
}

/// The canonical CBOR bytes currently loaded, plus provenance and view state.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub view: ArtifactView,
    pub source_path: Option<PathBuf>,
    pub provenance: Vec<String>,
    edn_cache: Option<String>,
}

impl Artifact {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            view: ArtifactView::Hex,
            source_path: None,
            provenance: Vec::new(),
            edn_cache: None,
        }
    }

    pub fn from_file(path: PathBuf, bytes: Vec<u8>) -> Self {
        let mut a = Self::new(bytes);
        a.provenance.push(format!("loaded {}", path.display()));
        a.source_path = Some(path);
        a
    }

    pub fn record(&mut self, step: impl Into<String>) {
        self.provenance.push(step.into());
    }

    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            ArtifactView::Hex => ArtifactView::Edn,
            ArtifactView::Edn => ArtifactView::Hex,
        };
    }

    /// Replace the artifact bytes, invalidating any cached EDN text (it no
    /// longer represents these bytes).
    pub fn set_bytes(&mut self, bytes: Vec<u8>) {
        self.bytes = bytes;
        self.edn_cache = None;
    }

    /// The EDN text known to represent the current bytes exactly, if any —
    /// e.g. text just parsed from the EDN editor, or loaded verbatim from an
    /// `.edn` file. Caller is responsible for keeping this in sync with
    /// `bytes` via [`Artifact::set_bytes`] and [`Artifact::set_edn_cache`].
    pub fn edn_cache(&self) -> Option<&str> {
        self.edn_cache.as_deref()
    }

    /// Record `edn` as the exact EDN text for the current bytes.
    pub fn set_edn_cache(&mut self, edn: String) {
        self.edn_cache = Some(edn);
    }
}
