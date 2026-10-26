/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod error;
mod io;

use commands::{encrypt0, key, sign1};
use error::CliError;

#[derive(Parser)]
#[command(name = "cose_tool", about = "COSE_Sign1 and COSE_Encrypt0 operations")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// COSE_Sign1 operations
    Sign1 {
        #[command(subcommand)]
        subcommand: Sign1Commands,
    },
    /// COSE_Encrypt0 operations
    Encrypt0 {
        #[command(subcommand)]
        subcommand: Encrypt0Commands,
    },
    /// COSE key utilities
    Key {
        #[command(subcommand)]
        subcommand: KeyCommands,
    },
}

#[derive(Subcommand)]
enum Sign1Commands {
    /// Sign a payload and produce a COSE_Sign1 message
    Sign {
        /// COSE key file (CBOR, EC2 P-256 or OKP Ed25519 private key)
        #[arg(long)]
        key: PathBuf,
        /// Payload file (default: stdin)
        #[arg(long)]
        input: Option<PathBuf>,
        /// Output file for signed COSE message (default: stdout binary)
        #[arg(long)]
        output: Option<PathBuf>,
        /// External AAD as hex string (default: empty)
        #[arg(long)]
        aad: Option<String>,
    },
    /// Verify a COSE_Sign1 message
    Verify {
        /// COSE key file (CBOR, EC2 P-256 or OKP Ed25519 key, public or private)
        #[arg(long)]
        key: PathBuf,
        /// COSE_Sign1 binary file (default: stdin)
        #[arg(long)]
        input: Option<PathBuf>,
        /// Write payload to file (if omitted: hex to stdout)
        #[arg(long)]
        output: Option<PathBuf>,
        /// External AAD as hex string (default: empty)
        #[arg(long)]
        aad: Option<String>,
    },
}

#[derive(Subcommand)]
enum Encrypt0Commands {
    /// Encrypt plaintext and produce a COSE_Encrypt0 message
    Encrypt {
        /// COSE key file (CBOR, Symmetric key)
        #[arg(long)]
        key: PathBuf,
        /// Plaintext file (default: stdin)
        #[arg(long)]
        input: Option<PathBuf>,
        /// Output file for COSE message (default: stdout binary)
        #[arg(long)]
        output: Option<PathBuf>,
        /// Encryption algorithm: a128gcm (default), a128ccm-16-64-128, etc.
        #[arg(long)]
        alg: Option<String>,
        /// Nonce/IV as hex string (if omitted: randomly generated)
        #[arg(long)]
        nonce: Option<String>,
        /// External AAD as hex string (default: empty)
        #[arg(long)]
        aad: Option<String>,
    },
    /// Decrypt a COSE_Encrypt0 message
    Decrypt {
        /// COSE key file (CBOR, Symmetric key)
        #[arg(long)]
        key: PathBuf,
        /// COSE_Encrypt0 binary file (default: stdin)
        #[arg(long)]
        input: Option<PathBuf>,
        /// Plaintext output file (if omitted: hex to stdout)
        #[arg(long)]
        output: Option<PathBuf>,
        /// External AAD as hex string (default: empty)
        #[arg(long)]
        aad: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeyCommands {
    /// Display information about a COSE key file
    Info {
        /// COSE key file to inspect
        key_file: PathBuf,
    },
}

fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Sign1 { subcommand } => match subcommand {
            Sign1Commands::Sign {
                key,
                input,
                output,
                aad,
            } => sign1::sign(sign1::SignArgs {
                key,
                input,
                output,
                aad,
            }),
            Sign1Commands::Verify {
                key,
                input,
                output,
                aad,
            } => sign1::verify(sign1::VerifyArgs {
                key,
                input,
                output,
                aad,
            }),
        },
        Commands::Encrypt0 { subcommand } => match subcommand {
            Encrypt0Commands::Encrypt {
                key,
                input,
                output,
                alg,
                nonce,
                aad,
            } => encrypt0::encrypt(encrypt0::EncryptArgs {
                key,
                input,
                output,
                alg,
                nonce,
                aad,
            }),
            Encrypt0Commands::Decrypt {
                key,
                input,
                output,
                aad,
            } => encrypt0::decrypt(encrypt0::DecryptArgs {
                key,
                input,
                output,
                aad,
            }),
        },
        Commands::Key { subcommand } => match subcommand {
            KeyCommands::Info { key_file } => key::info(key::InfoArgs { key: key_file }),
        },
    }
}
