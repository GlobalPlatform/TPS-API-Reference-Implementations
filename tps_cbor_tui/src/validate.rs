/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Validate a loaded [`crate::artifact::Artifact`]'s CBOR bytes against a
//! CDDL schema.
//!
//! Thin wrapper around [`tps_cddl::cbor_check::check_cbor`] — the crate's
//! first concrete wiring of "load CBOR -> validate against CDDL".

use tps_cddl::{Cddl, CborMismatch, Config};

use crate::artifact::Artifact;

/// Validate `artifact`'s bytes against `cddl`, optionally rooted at `root`.
///
/// Returns the list of mismatches found; an empty list means the artifact
/// conforms to the schema.
pub fn validate_artifact_against_cddl(
    artifact: &Artifact,
    cddl: &Cddl,
    root: Option<&str>,
    config: &Config,
) -> Result<Vec<CborMismatch>, String> {
    tps_cddl::check_cbor(&artifact.bytes, cddl, root, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tps_cddl::{build_cddl, lex};

    fn cddl_from(source: &str) -> Cddl {
        let pairs = lex(source).expect("lex CDDL");
        build_cddl(pairs).expect("build CDDL")
    }

    // RFC 8610 Appendix H, §3.3 Geography example.
    const GEOGRAPHY_CDDL: &str = "Geography = [city: tstr, gpsCoordinates: GpsCoordinates]\nGpsCoordinates = { longitude: uint, latitude: uint }";

    #[test]
    fn conforming_artifact_has_no_mismatches() {
        let cddl = cddl_from(GEOGRAPHY_CDDL);
        // ["Bern", {longitude: 20, latitude: 46}]
        let bytes: Vec<u8> = vec![
            0x82, 0x64, 0x42, 0x65, 0x72, 0x6e, 0xa2, 0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74,
            0x75, 0x64, 0x65, 0x14, 0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, 0x18,
            0x2e,
        ];
        let artifact = Artifact::new(bytes);
        let config = Config::default();
        let mismatches =
            validate_artifact_against_cddl(&artifact, &cddl, Some("Geography"), &config).unwrap();
        assert!(mismatches.is_empty());
    }

    #[test]
    fn non_conforming_artifact_reports_mismatches() {
        let cddl = cddl_from(GEOGRAPHY_CDDL);
        // ["Bern", {longitude: "not a uint", latitude: 46}]
        let bytes: Vec<u8> = vec![
            0x82, 0x64, 0x42, 0x65, 0x72, 0x6e, 0xa2, 0x69, 0x6c, 0x6f, 0x6e, 0x67, 0x69, 0x74,
            0x75, 0x64, 0x65, 0x6a, 0x6e, 0x6f, 0x74, 0x20, 0x61, 0x20, 0x75, 0x69, 0x6e, 0x74,
            0x68, 0x6c, 0x61, 0x74, 0x69, 0x74, 0x75, 0x64, 0x65, 0x18, 0x2e,
        ];
        let artifact = Artifact::new(bytes);
        let config = Config::default();
        let mismatches =
            validate_artifact_against_cddl(&artifact, &cddl, Some("Geography"), &config).unwrap();
        assert!(!mismatches.is_empty());
    }
}
