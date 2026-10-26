/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! `edn-tool` — convert between CBOR diagnostic notation (EDN) and binary CBOR.
//!
//! # Usage
//!
//! ```text
//! edn-tool encode [<file>]   # EDN text  → binary CBOR on stdout
//! edn-tool decode [<file>]   # binary CBOR → EDN text on stdout
//! ```
//!
//! `<file>` defaults to stdin when omitted.  Use `--output` to write to a file
//! instead of stdout.

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use tps_minicbor::{Decoder, EdnError, parse_edn};

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "edn-tool",
    version,
    about = "Convert between CBOR diagnostic notation (EDN) and binary CBOR (RFC 8949 §8)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Encode EDN text to binary CBOR
    Encode {
        /// EDN input file (default: stdin)
        file: Option<PathBuf>,

        /// Write binary CBOR to this file instead of stdout
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Decode binary CBOR to EDN text
    Decode {
        /// Binary CBOR input file (default: stdin)
        file: Option<PathBuf>,

        /// Write EDN text to this file instead of stdout
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum ToolError {
    Io(io::Error),
    Edn(EdnError),
    Cbor(tps_minicbor::DecodeError),
    Diagnostic(tps_minicbor::DiagnosticError),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Io(e) => write!(f, "I/O error: {e}"),
            ToolError::Edn(e) => write!(f, "EDN parse error: {e}"),
            ToolError::Cbor(e) => write!(f, "CBOR decode error: {e}"),
            ToolError::Diagnostic(e) => write!(f, "CBOR diagnostic error: {e}"),
        }
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ToolError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ToolError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<EdnError> for ToolError {
    fn from(e: EdnError) -> Self {
        Self::Edn(e)
    }
}
impl From<tps_minicbor::DecodeError> for ToolError {
    fn from(e: tps_minicbor::DecodeError) -> Self {
        Self::Cbor(e)
    }
}
impl From<tps_minicbor::DiagnosticError> for ToolError {
    fn from(e: tps_minicbor::DiagnosticError) -> Self {
        Self::Diagnostic(e)
    }
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), ToolError> {
    match cli.command {
        Command::Encode { file, output } => {
            let src = read_text(file)?;
            let cbor = parse_edn(src.trim())?;
            write_bytes(output, &cbor)?;
        }
        Command::Decode { file, output } => {
            let cbor = read_bytes(file)?;
            let dec = Decoder::new(&cbor);
            let mut edn = String::new();
            dec.write_diagnostic_all_fmt(&mut edn)?;
            edn.push('\n');
            write_text(output, &edn)?;
        }
    }
    Ok(())
}

// ─── I/O helpers ─────────────────────────────────────────────────────────────

fn read_text(path: Option<PathBuf>) -> Result<String, ToolError> {
    match path {
        Some(p) => Ok(fs::read_to_string(&p)?),
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
    }
}

fn read_bytes(path: Option<PathBuf>) -> Result<Vec<u8>, ToolError> {
    match path {
        Some(p) => Ok(fs::read(&p)?),
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

fn write_bytes(path: Option<PathBuf>, data: &[u8]) -> Result<(), ToolError> {
    match path {
        Some(p) => Ok(fs::write(&p, data)?),
        None => {
            io::stdout().write_all(data)?;
            Ok(())
        }
    }
}

fn write_text(path: Option<PathBuf>, text: &str) -> Result<(), ToolError> {
    write_bytes(path, text.as_bytes())
}
