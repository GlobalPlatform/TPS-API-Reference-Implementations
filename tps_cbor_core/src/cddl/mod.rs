/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Everything CDDL: parsing, validation, module resolution, the navigable AST, and validating
//! CBOR against a schema with a trace.
//!
//! The four submodules divide along what the UI asks of them:
//!
//! * [`validate`] answers "is this schema well formed?" -- the CDDL tab's `Syntax Check`.
//! * [`chain`] answers "which file is this error *in*?" -- the inner tab bar and attribution.
//! * [`ast_tree`] answers "what is in this schema?" -- the right pane's tree view.
//! * [`trace`] answers "why did this CBOR not match?" -- the `Validate` button and the trace log.
//!
//! The commonly used items are re-exported here, so a caller writes `cddl::check` rather than
//! `cddl::validate::check`. [`ast_tree`], [`chain`] and [`trace`] keep their namespaces, because
//! `ast_tree::build_tree`, `chain::walk` and `trace::check` read better qualified than bare --
//! `trace::check` in particular, which would otherwise collide outright with [`validate::check`].
//! [`trace::check_source`] is the one exception: it is the `Validate` button's whole entry point,
//! and its `cbor` first argument distinguishes it from [`check`] at every call site.

pub mod ast_tree;
pub mod chain;
pub mod trace;
pub mod validate;

pub use ast_tree::{AstRow, CddlNode};
pub use chain::{Chain, ChainFile, DirectiveRef};
// `ChainTrace::check` rather than a free `trace::check_chain`, because `check_chain` is already
// taken here by `validate::check_chain` -- the schema-only pass this one composes with a trace.
pub use trace::{
    ChainTrace, Mismatch, Row, RuleStatus, Start, Trace, TraceEvent, TraceOutcome, check_source,
};
pub use validate::{
    ChainCheck, Check, Resolved, StartRule, check, check_chain, check_module, diagnostic_for,
    lex_check, parse, resolve, start_rule, validate, validate_ast,
};
