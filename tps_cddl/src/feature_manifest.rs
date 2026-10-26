/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Manifest-driven mandatory `.feature` (RFC 9165 §4) resolution for codegen.
//!
//! Every `.feature` name reachable in a codegen invocation's emitted rule
//! closure must be resolved to `enabled`/`disabled` via this manifest — there
//! is no third "unspecified" state, by construction. Shape:
//! `{"json": "disabled", "cbor": "enabled"}`.

use std::collections::HashMap;

/// A parsed `--feature-manifest` file: feature name → enabled/disabled.
#[derive(Debug, Clone, Default)]
pub struct FeatureManifest {
    resolved: HashMap<String, bool>,
}

/// Errors that can occur while parsing a feature manifest.
#[derive(Debug)]
pub enum FeatureManifestError {
    /// The manifest is not valid JSON, or not a flat object of strings.
    Parse(serde_json::Error),
    /// A feature name's value was neither `"enabled"` nor `"disabled"`.
    InvalidValue {
        /// The feature name whose value was invalid.
        name: String,
        /// The invalid value found for it.
        value: String,
    },
}

impl std::fmt::Display for FeatureManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureManifestError::Parse(e) => write!(f, "invalid feature manifest: {e}"),
            FeatureManifestError::InvalidValue { name, value } => write!(
                f,
                "feature manifest: `{name}` has value `{value}` — expected \"enabled\" or \"disabled\""
            ),
        }
    }
}

impl std::error::Error for FeatureManifestError {}

impl FeatureManifest {
    /// Parse a manifest from a JSON object mapping feature names to
    /// `"enabled"`/`"disabled"`.
    pub fn from_json(text: &str) -> Result<Self, FeatureManifestError> {
        let raw: HashMap<String, String> =
            serde_json::from_str(text).map_err(FeatureManifestError::Parse)?;
        let mut resolved = HashMap::with_capacity(raw.len());
        for (name, value) in raw {
            let enabled = match value.as_str() {
                "enabled" => true,
                "disabled" => false,
                _ => return Err(FeatureManifestError::InvalidValue { name, value }),
            };
            resolved.insert(name, enabled);
        }
        Ok(Self { resolved })
    }

    /// Looks up a feature name's resolution, or `None` if the manifest has no
    /// entry for it.
    pub fn resolve(&self, name: &str) -> Option<bool> {
        self.resolved.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enabled_and_disabled() {
        let m = FeatureManifest::from_json(r#"{"json": "disabled", "cbor": "enabled"}"#).unwrap();
        assert_eq!(m.resolve("json"), Some(false));
        assert_eq!(m.resolve("cbor"), Some(true));
    }

    #[test]
    fn unresolved_name_is_none() {
        let m = FeatureManifest::from_json(r#"{"cbor": "enabled"}"#).unwrap();
        assert_eq!(m.resolve("nope"), None);
    }

    #[test]
    fn invalid_value_is_error() {
        let err = FeatureManifest::from_json(r#"{"cbor": "maybe"}"#).unwrap_err();
        assert!(matches!(err, FeatureManifestError::InvalidValue { .. }));
    }

    #[test]
    fn invalid_json_is_error() {
        let err = FeatureManifest::from_json("not json").unwrap_err();
        assert!(matches!(err, FeatureManifestError::Parse(_)));
    }
}
