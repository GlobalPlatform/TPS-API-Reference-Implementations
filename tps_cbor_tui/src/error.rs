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

    #[error("key store error: {0}")]
    KeyStore(#[from] crate::keystore::KeyStoreError),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Other(s.into())
    }
}
