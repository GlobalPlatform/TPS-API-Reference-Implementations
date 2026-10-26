/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CDDL — Concise Data Definition Language (IETF RFC 8610 / RFC 9165 / RFC 9682)
//!
//! This crate provides a parser, semantic validator, CBOR/JSON value checkers,
//! and Rust code generators for CDDL schema files.  It can be used both as a
//! CLI tool (`cddl`) and as a library in other Rust crates.
//!
//! # Feature flags
//!
//! | Flag | Effect |
//! |------|--------|
//! | `rfc9165` | Accept RFC 9165 additional control operators without warning |
//! | `rfc9682` | Accept RFC 9682 grammar extensions (empty models, non-literal tags) |
//! | `all_extensions` | Enables both `rfc9165` and `rfc9682` |
//!
//! # Quick start — validation
//!
//! ```
//! use tps_cddl::{lex, build_cddl, validate};
//!
//! let source = r#"person = { name: tstr, age: uint }"#;
//! let pairs = lex(source).expect("parse error");
//! let schema = build_cddl(pairs).unwrap();
//! let diagnostics = validate(&schema);
//! assert!(diagnostics.is_empty());
//! ```
//!
//! # Quick start — code generation
//!
//! ```
//! use tps_cddl::{lex, build_cddl, gen_decoder, Config, FeatureManifest};
//!
//! let source = r#"point = [x: float, y: float]"#;
//! let pairs = lex(source).expect("parse error");
//! let schema = build_cddl(pairs).unwrap();
//! let manifest = FeatureManifest::from_json("{}").unwrap();
//! let rust_code = gen_decoder(&schema, None, &Config::strict(), &manifest).unwrap();
//! assert!(rust_code.contains("struct Point"));
//! ```

pub mod ast;
pub mod cbor_check;
pub mod error;
pub mod feature_manifest;
pub(crate) mod gen_common;
pub mod gen_decoder;
pub mod gen_encoder;
pub(crate) mod grammar;
pub mod json_check;
pub mod lexer;
#[cfg(feature = "modules")]
pub mod modules;
pub mod parser;
pub mod tokenize;
pub mod validator;

// ─── Convenience re-exports ───────────────────────────────────────────────────

pub use ast::Cddl;
pub use cbor_check::{
    CborMismatch, CborSpan, TraceEvent, TraceOutcome, check_cbor, check_cbor_traced,
};
pub use error::{Diagnostic, Level, ParseError};
pub use feature_manifest::{FeatureManifest, FeatureManifestError};
pub use gen_common::GenError;
pub use gen_decoder::gen_decoder;
pub use gen_encoder::gen_encoder;
pub use json_check::{JsonMismatch, check_json};
pub use lexer::lex;
#[cfg(feature = "modules")]
pub use modules::{ModuleError, ModuleResolution, ModuleSearchPath, resolve_modules};
pub use parser::build_cddl;
pub use validator::{FeatureUsage, validate, validate_with_config, validate_with_config_ex};

// ─── Configuration ────────────────────────────────────────────────────────────

/// Runtime configuration controlling which RFC extensions are active.
///
/// Pass this to [`validate_with_config`] to enable or disable specific RFC extensions.
///
/// # Examples
///
/// ```
/// use tps_cddl::Config;
///
/// let strict = Config::strict();
/// let all = Config::all();
/// ```
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Accept RFC 9165 control operators (`.plus`, `.cat`, `.det`, `.abnf`,
    /// `.abnfb`, `.feature`) without emitting an unknown-operator warning.
    pub rfc9165: bool,
    /// Accept RFC 9682 grammar extensions: empty CDDL files, non-literal
    /// tag/simple-value head-numbers.
    pub rfc9682: bool,
}

impl Config {
    /// Strict RFC 8610 mode — no extensions enabled.
    pub fn strict() -> Self {
        Self {
            rfc9165: false,
            rfc9682: false,
        }
    }

    /// All extensions enabled (RFC 9165 + RFC 9682).
    pub fn all() -> Self {
        Self {
            rfc9165: true,
            rfc9682: true,
        }
    }
}
