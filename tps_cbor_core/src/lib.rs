/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Frontend-agnostic domain layer for the CBOR/EDN/CDDL/COSE workbenches.
//!
//! This crate holds everything `tps_cbor_ide` and `tps_cbor_tui` both need and neither should own:
//! the document model, CBOR/EDN conversion, the span machinery that links a CBOR byte range to its
//! rendered EDN text, CDDL validation and tracing, COSE operations, key generation, and the key
//! store. There is one copy of each algorithm, tested once, so the two frontends cannot drift.
//!
//! Nothing here depends on a UI toolkit, so all of it is testable with `cargo test` alone.
//!
//! # Layout
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`error`] | [`CoreError`], the single error type, replacing `Result<_, String>` |
//! | [`diag`] | [`Diagnostic`], a ranged, file-attributed message |
//! | [`convert`] | CBOR ↔ EDN conversion and hex dumping |
//! | [`spans`] | [`SpanMap`]: CBOR byte range → EDN text range |
//! | [`cddl`] | Validation, module resolution, the AST view, and the validation tracer |
//! | [`cose`] | Sign/verify/encrypt/decrypt and key generation |
//! | [`keystore`] | On-disk COSE key store |
//! | [`settings`] | Persisted settings, as TOML in the platform config directory |
//! | [`doc`] | [`Workspace`] and the per-kind open documents |
//! | [`highlight`] | Re-exports of the tree-sitter grammars and their highlight queries |

pub mod cddl;
pub mod convert;
pub mod cose;
pub mod diag;
pub mod doc;
pub mod error;
pub mod highlight;
pub mod keystore;
pub mod settings;
pub mod spans;

pub use diag::{Diagnostic, Severity, Source};
pub use doc::{CborDoc, CddlDocSet, CoseDoc, EdnDoc, Meta, TextDoc, Workspace};
pub use error::{CoreError, Result};
pub use spans::SpanMap;
