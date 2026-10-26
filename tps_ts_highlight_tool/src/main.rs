/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! `ts-highlight` — terminal syntax-highlighting preview for the
//! `tree-sitter-cddl` and `tree-sitter-edn` grammars.
//!
//! # Usage
//!
//! ```text
//! ts-highlight <file>              # language inferred from extension
//! ts-highlight <file> --lang edn   # language forced explicitly
//! ```

use std::fs;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

// Union of every capture name used by tree-sitter-cddl/queries/highlights.scm
// and tree-sitter-edn/queries/highlights.scm, each mapped to an ANSI SGR code.
const HIGHLIGHT_NAMES: &[&str] = &[
    "type",
    "type.parameter",
    "type.builtin",
    "property",
    "operator",
    "keyword.operator",
    "number",
    "string",
    "string.special",
    "punctuation.bracket",
    "punctuation.delimiter",
    "comment",
    "comment.documentation",
    "boolean",
    "constant.builtin",
    "constant.numeric.float",
];

fn ansi_for(name: &str) -> &'static str {
    match name {
        "type" | "type.builtin" => "\x1b[36m",           // cyan
        "type.parameter" => "\x1b[96m",                  // bright cyan
        "property" => "\x1b[34m",                        // blue
        "operator" | "keyword.operator" => "\x1b[35m",   // magenta
        "number" | "constant.numeric.float" => "\x1b[33m", // yellow
        "string" | "string.special" => "\x1b[32m",       // green
        "punctuation.bracket" | "punctuation.delimiter" => "\x1b[37m", // white
        "comment" => "\x1b[90m",                          // bright black
        "comment.documentation" => "\x1b[94m",            // bright blue
        "boolean" | "constant.builtin" => "\x1b[91m",    // bright red
        _ => "\x1b[0m",
    }
}

const RESET: &str = "\x1b[0m";

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Lang {
    Cddl,
    Edn,
}

impl Lang {
    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "cddl" => Some(Self::Cddl),
            "edn" | "diag" => Some(Self::Edn),
            _ => None,
        }
    }

    fn highlight_config(self) -> HighlightConfiguration {
        let (language, name, highlights_query) = match self {
            Self::Cddl => (
                tree_sitter_cddl::LANGUAGE.into(),
                "cddl",
                tree_sitter_cddl::HIGHLIGHTS_QUERY,
            ),
            Self::Edn => (
                tree_sitter_edn::LANGUAGE.into(),
                "edn",
                tree_sitter_edn::HIGHLIGHTS_QUERY,
            ),
        };
        let mut config = HighlightConfiguration::new(language, name, highlights_query, "", "")
            .expect("build highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    }
}

#[derive(Parser)]
#[command(
    name = "ts-highlight",
    version,
    about = "Preview tree-sitter-cddl/tree-sitter-edn syntax highlighting in a terminal"
)]
struct Cli {
    /// File to highlight
    file: PathBuf,

    /// Force the language instead of inferring it from the file extension
    #[arg(long)]
    lang: Option<Lang>,
}

fn main() {
    let cli = Cli::parse();

    let lang = cli.lang.or_else(|| {
        cli.file
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Lang::from_extension)
    });
    let Some(lang) = lang else {
        eprintln!(
            "error: could not infer language from '{}'; pass --lang cddl|edn",
            cli.file.display()
        );
        std::process::exit(1);
    };

    let source = match fs::read(&cli.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: reading '{}': {e}", cli.file.display());
            std::process::exit(1);
        }
    };

    let config = lang.highlight_config();
    let mut highlighter = Highlighter::new();
    let events = highlighter
        // Args after `config`/`source` are `encoding` and tree-sitter-highlight 0.27's
        // `cancellation_flag`: this is a one-shot CLI, so there is nothing to cancel from.
        .highlight(&config, &source, None, None, |_| None)
        .expect("run highlighter");

    for event in events {
        match event.expect("highlight event") {
            HighlightEvent::Source { start, end } => {
                print!("{}", String::from_utf8_lossy(&source[start..end]));
            }
            HighlightEvent::HighlightStart(idx) => {
                print!("{}", ansi_for(HIGHLIGHT_NAMES[idx.0]));
            }
            HighlightEvent::HighlightEnd => {
                print!("{RESET}");
            }
        }
    }
}
