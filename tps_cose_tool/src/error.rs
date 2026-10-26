/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("COSE error: {0:?}")]
    Cose(#[from] tps_cose::CoseError),

    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("{0}")]
    Other(String),
}

impl CliError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Other(s.into())
    }
}
