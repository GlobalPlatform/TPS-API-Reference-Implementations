/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

use crate::error::CliError;

/// Read all bytes from a file path, or from stdin if `path` is `None`.
pub fn read_input(path: Option<&Path>) -> Result<Vec<u8>, CliError> {
    match path {
        Some(p) => Ok(fs::read(p)?),
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

/// Write bytes to a file path, or to stdout if `path` is `None`.
pub fn write_output(path: Option<&Path>, data: &[u8]) -> Result<(), CliError> {
    match path {
        Some(p) => Ok(fs::write(p, data)?),
        None => Ok(io::stdout().write_all(data)?),
    }
}

/// Decode a hex string argument (empty → empty slice).
pub fn decode_hex_arg(hex_str: Option<&str>) -> Result<Vec<u8>, CliError> {
    match hex_str {
        Some(s) if !s.is_empty() => Ok(hex::decode(s)?),
        _ => Ok(Vec::new()),
    }
}
