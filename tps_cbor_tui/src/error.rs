/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CBOR diagnostic error: {0:?}")]
    Diagnostic(#[from] tps_minicbor::DiagnosticError),

    #[error("EDN parse error: {0}")]
    Edn(#[from] tps_minicbor::EdnError),

    /// A key store operation `tps_cbor_core` refused.
    ///
    /// Named after `tps_cbor_core::keystore::KeyStoreError` rather than the broader `CoreError`
    /// the migration plan called for: `?` applies exactly one `From`, and `CoreError` reaches
    /// `KeyStoreError` only through its own `#[from]`, so a `Core(CoreError)` variant would not
    /// have converted `KeyStore::open(..)?` at all. The COSE operations need no variant — every
    /// one of them is `match`ed and formatted into the status line, never propagated.
    #[error("key store error: {0}")]
    KeyStore(#[from] tps_cbor_core::keystore::KeyStoreError),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Other(s.into())
    }
}
