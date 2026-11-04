/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The crate's single error type.
//!
//! The old Tauri backend returned `Result<_, String>` from every one of its commands, formatting
//! the underlying error at the point it was caught. That threw away everything a caller might act
//! on -- notably the byte offset inside [`tps_minicbor::EdnError::UnexpectedChar`], which the GUI
//! discarded by way of `format!("{:?}")` and then could not use to place a marker. Each variant
//! here keeps the structured error, so a frontend can still reach the offset, and `Display` is
//! only consulted when the message is actually shown.

use std::ops::Range;
use std::path::PathBuf;

use thiserror::Error;

/// The result type used throughout this crate.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Anything that can go wrong in the domain layer.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A file could not be read or written. Carries the path, which `std::io::Error` does not.
    #[error("{path}: {source}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The CBOR input is not well formed.
    ///
    /// `offset` is where the decoder stopped, read from [`tps_minicbor::Decoder::pos`];
    /// [`tps_minicbor::DecodeError`] itself carries no position.
    #[error("CBOR decode error at byte {offset}: {source}")]
    CborDecode {
        /// Byte offset at which decoding stopped.
        offset: usize,
        /// The underlying decode error.
        #[source]
        source: tps_minicbor::DecodeError,
    },

    /// The CBOR input could not be rendered as EDN diagnostic notation.
    #[error("CBOR diagnostic error: {0}")]
    CborDiagnostic(String),

    /// The EDN input could not be parsed.
    ///
    /// `offset` is `Some` only for [`tps_minicbor::EdnError::UnexpectedChar`], the one variant
    /// that records a position. Callers should fall back to a whole-document marker when it is
    /// `None` rather than inventing line 1, column 1.
    #[error("EDN parse error: {source}")]
    EdnParse {
        /// Byte offset of the offending character, when the error records one.
        offset: Option<usize>,
        /// The underlying EDN error.
        #[source]
        source: tps_minicbor::EdnError,
    },

    /// The CDDL source could not be tokenized.
    ///
    /// `span` indexes the CDDL source and comes from
    /// [`tps_cddl::ParseError::span`]; it is empty (`start == end`) when the parser reported a
    /// point rather than a range. The old GUI formatted this error to a string and reported it at
    /// line 1, column 1, so a syntax error was never clickable.
    #[error("CDDL tokenize error: {message}")]
    CddlLex {
        /// Byte range in the CDDL source where tokenizing failed.
        span: Range<usize>,
        /// What the parser could not accept.
        message: String,
    },

    /// The CDDL source tokenized but could not be parsed into an AST.
    #[error("CDDL parse error: {message}")]
    CddlParse {
        /// Byte range in the CDDL source where parsing failed.
        span: Range<usize>,
        /// What the parser could not accept.
        message: String,
    },

    /// A `;# import` or `;# include` chain could not be resolved.
    #[error("CDDL module resolution error: {0}")]
    CddlModules(String),

    /// Validation ran but the CBOR could not be decoded against the schema at all.
    ///
    /// Distinct from [`CoreError::CborDecode`]: this is `check_cbor_traced` refusing to start,
    /// and it is reported as a diagnostic rather than as a crash.
    #[error("CDDL check error: {0}")]
    CddlCheck(String),

    /// A COSE encode, decode, sign, verify, encrypt or decrypt operation failed.
    #[error("COSE error: {0:?}")]
    Cose(tps_cose::CoseError),

    /// The key is structurally valid but cannot be used for the requested operation.
    ///
    /// Kept as an enum rather than a string so the UI can distinguish "you need a different key"
    /// from "this will never work"; see [`crate::cose::UnsupportedSign`].
    #[error("{0}")]
    UnsupportedSign(#[from] crate::cose::UnsupportedSign),

    /// The key cannot be used for the requested COSE_Encrypt0 operation.
    #[error("{0}")]
    UnsupportedEncrypt(#[from] crate::cose::UnsupportedEncrypt),

    /// A key store operation failed.
    #[error(transparent)]
    KeyStore(#[from] crate::keystore::KeyStoreError),

    /// The settings file could not be written, or no configuration directory exists.
    ///
    /// Reading a malformed settings file is deliberately *not* an error: it yields the defaults
    /// plus a warning diagnostic, because losing a window size should not stop the app starting.
    /// See [`crate::settings::Settings::load`].
    #[error("settings error: {0}")]
    Settings(String),

    /// An edit was attempted on a document whose read-only flag is set.
    ///
    /// Raised by the model rather than prevented by the UI, so a keyboard shortcut that forgot to
    /// check cannot bypass it. See [`crate::doc::Meta::writable`].
    #[error("document is read-only{}", match path {
        Some(path) => format!(" ({})", path.display()),
        None => String::new(),
    })]
    ReadOnly {
        /// The document's path, or `None` if it has never been saved.
        path: Option<PathBuf>,
    },

    /// `Save` was requested on a document that has no path, so there is nowhere to write.
    ///
    /// The caller should fall back to `Save As`. Carries a short description of which document,
    /// because the user may have several open.
    #[error("{0} has no file name; use Save As")]
    NoPath(&'static str),

    /// A byte range did not lie within the buffer it indexes.
    #[error("range {range:?} is outside the {len}-byte buffer")]
    RangeOutOfBounds {
        /// The offending range.
        range: Range<usize>,
        /// Length of the buffer it was applied to.
        len: usize,
    },

    /// Text offered as hex could not be parsed as hex.
    ///
    /// Carries the character position so the UI can say *where*: the usual source is a clipboard
    /// paste that a human assembled by hand, and "not hex" without a position leaves them hunting.
    /// See [`crate::convert::from_hex`].
    #[error("{message} at character {position}")]
    NotHex {
        /// Zero-based character position in the input.
        position: usize,
        /// What was wrong there.
        message: String,
    },
}

impl CoreError {
    /// Wrap an [`std::io::Error`] together with the path that produced it.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Wrap a [`tps_minicbor::EdnError`], extracting its byte offset where it has one.
    pub fn edn(source: tps_minicbor::EdnError) -> Self {
        let offset = match source {
            tps_minicbor::EdnError::UnexpectedChar(_, offset) => Some(offset),
            _ => None,
        };
        Self::EdnParse { offset, source }
    }
}

impl From<tps_cose::CoseError> for CoreError {
    fn from(error: tps_cose::CoseError) -> Self {
        Self::Cose(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edn_error_offset_survives() {
        let error = CoreError::edn(tps_minicbor::EdnError::UnexpectedChar('%', 17));
        match error {
            CoreError::EdnParse {
                offset: Some(17), ..
            } => {}
            other => panic!("offset was dropped: {other:?}"),
        }
    }

    #[test]
    fn edn_errors_without_a_position_report_none() {
        let error = CoreError::edn(tps_minicbor::EdnError::UnexpectedEof);
        match error {
            CoreError::EdnParse { offset: None, .. } => {}
            other => panic!("invented a position: {other:?}"),
        }
    }

    /// The whole point of keeping the structured error: the message still reads well.
    #[test]
    fn display_mentions_the_offset() {
        let error = CoreError::edn(tps_minicbor::EdnError::UnexpectedChar('%', 17));
        assert_eq!(
            error.to_string(),
            "EDN parse error: unexpected '%' at byte offset 17"
        );
    }
}
