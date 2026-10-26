/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use miette::{Diagnostic, GraphicalReportHandler, LabeledSpan, NamedSource, SourceSpan};
use pest::error::InputLocation;

use tps_cddl::{
    Config, FeatureManifest,
    cbor_check::check_cbor,
    error::{Diagnostic as CddlDiag, Level},
    gen_decoder::gen_decoder,
    gen_encoder::gen_encoder,
    json_check::check_json,
    validator::{validate_with_config, validate_with_config_ex},
};
#[cfg(not(feature = "modules"))]
use tps_cddl::{error::ParseError, lexer::lex, parser::build_cddl};
#[cfg(feature = "modules")]
use tps_cddl::{ModuleError, ModuleSearchPath, resolve_modules};

// ─── CLI definition ───────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "cddl",
    version,
    about = "IETF CDDL (RFC 8610) parser, validator, and code generator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Enable RFC 9165 additional control operators
    #[arg(long, global = true)]
    rfc9165: bool,

    /// Enable RFC 9682 grammar corrections and additions
    #[arg(long, global = true)]
    rfc9682: bool,

    /// Enable all RFC extensions (equivalent to --rfc9165 --rfc9682)
    #[arg(long, global = true)]
    all_extensions: bool,

    /// Extra directory to search for `;# import`/`;# include` modules (repeatable)
    #[arg(long = "include-path", global = true, value_name = "DIR")]
    include_path: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and validate a CDDL file
    Validate {
        /// CDDL file to validate
        file: PathBuf,

        /// Suppress all output; communicate only via exit code
        #[arg(short, long)]
        quiet: bool,

        /// Print the parsed AST to stdout after a successful parse
        #[arg(long)]
        dump_ast: bool,

        /// Print deduplicated `.feature` (RFC 9165 §4) usages found in the schema
        #[arg(long)]
        list_features: bool,
    },

    /// Check a CBOR file against a CDDL schema
    CheckCbor {
        /// CDDL schema file
        cddl_file: PathBuf,

        /// CBOR file to validate (default: read from stdin)
        cbor_file: Option<PathBuf>,

        /// Treat the CBOR file as ASCII hex (whitespace ignored)
        #[arg(long)]
        hex: bool,

        /// After successful validation write the CBOR bytes to stdout (for pipelines)
        #[arg(long)]
        passthrough: bool,

        /// CDDL rule to validate against (default: first rule in schema)
        #[arg(long)]
        rule: Option<String>,

        /// Suppress all output; communicate only via exit code
        #[arg(short, long)]
        quiet: bool,
    },

    /// Check a JSON file against a CDDL schema
    CheckJson {
        /// CDDL schema file
        cddl_file: PathBuf,

        /// JSON file to validate
        json_file: PathBuf,

        /// CDDL rule to validate against (default: first rule in schema)
        #[arg(long)]
        rule: Option<String>,

        /// Suppress all output; communicate only via exit code
        #[arg(short, long)]
        quiet: bool,
    },

    /// Generate a tps_minicbor CBOR decoder from a CDDL schema
    GenDecoder {
        /// CDDL schema file
        cddl_file: PathBuf,

        /// Only generate the decoder for this rule (and its dependencies)
        #[arg(long)]
        rule: Option<String>,

        /// Write generated code to this file instead of stdout
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// JSON file resolving every `.feature` (RFC 9165 §4) name used by the
        /// schema to "enabled"/"disabled" (required only if the schema uses
        /// `.feature`)
        #[arg(long)]
        feature_manifest: Option<PathBuf>,

        /// Suppress all output on error; communicate only via exit code
        #[arg(short, long)]
        quiet: bool,
    },

    /// Generate a tps_minicbor CBOR encoder from a CDDL schema
    GenEncoder {
        /// CDDL schema file
        cddl_file: PathBuf,

        /// Only generate the encoder for this rule (and its dependencies)
        #[arg(long)]
        rule: Option<String>,

        /// Write generated code to this file instead of stdout
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// JSON file resolving every `.feature` (RFC 9165 §4) name used by the
        /// schema to "enabled"/"disabled" (required only if the schema uses
        /// `.feature`)
        #[arg(long)]
        feature_manifest: Option<PathBuf>,

        /// Suppress all output on error; communicate only via exit code
        #[arg(short, long)]
        quiet: bool,
    },
}

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let include_path = cli.include_path.clone();
    let exit_code = match cli.command {
        Command::Validate {
            file,
            quiet,
            dump_ast,
            list_features,
        } => {
            let config = Config {
                rfc9165: cli.rfc9165 || cli.all_extensions,
                rfc9682: cli.rfc9682 || cli.all_extensions,
            };
            run_validate(file, quiet, dump_ast, list_features, config, &include_path)
        }
        Command::CheckCbor {
            cddl_file,
            cbor_file,
            hex,
            passthrough,
            rule,
            quiet,
        } => {
            let config = Config {
                rfc9165: cli.rfc9165 || cli.all_extensions,
                rfc9682: cli.rfc9682 || cli.all_extensions,
            };
            run_check_cbor(
                cddl_file,
                cbor_file,
                hex,
                passthrough,
                rule.as_deref(),
                quiet,
                config,
                &include_path,
            )
        }
        Command::CheckJson {
            cddl_file,
            json_file,
            rule,
            quiet,
        } => {
            let config = Config {
                rfc9165: cli.rfc9165 || cli.all_extensions,
                rfc9682: cli.rfc9682 || cli.all_extensions,
            };
            run_check_json(
                cddl_file,
                json_file,
                rule.as_deref(),
                quiet,
                config,
                &include_path,
            )
        }
        Command::GenDecoder {
            cddl_file,
            rule,
            output,
            feature_manifest,
            quiet,
        } => {
            let config = Config {
                rfc9165: cli.rfc9165 || cli.all_extensions,
                rfc9682: cli.rfc9682 || cli.all_extensions,
            };
            run_gen_decoder(
                cddl_file,
                rule.as_deref(),
                output,
                feature_manifest,
                quiet,
                config,
                &include_path,
            )
        }
        Command::GenEncoder {
            cddl_file,
            rule,
            output,
            feature_manifest,
            quiet,
        } => {
            let config = Config {
                rfc9165: cli.rfc9165 || cli.all_extensions,
                rfc9682: cli.rfc9682 || cli.all_extensions,
            };
            run_gen_encoder(
                cddl_file,
                rule.as_deref(),
                output,
                feature_manifest,
                quiet,
                config,
                &include_path,
            )
        }
    };
    std::process::exit(exit_code);
}

// ─── Shared CDDL loading (handles module resolution when enabled) ────────────

/// Errors that can occur while loading a top-level CDDL file, whether or not
/// module resolution (`;# import`/`;# include`) is compiled in.
enum LoadCddlError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[cfg(not(feature = "modules"))]
    Parse {
        path: PathBuf,
        source: ParseError,
        text: String,
    },
    #[cfg(feature = "modules")]
    Module(ModuleError),
}

#[cfg(feature = "modules")]
fn load_cddl_file(
    path: &Path,
    include_dirs: &[PathBuf],
) -> Result<(String, tps_cddl::Cddl, Vec<CddlDiag>), LoadCddlError> {
    let search = ModuleSearchPath::from_env_and_cwd(include_dirs).map_err(|e| LoadCddlError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let resolution = resolve_modules(path, &search).map_err(LoadCddlError::Module)?;
    Ok((resolution.source, resolution.cddl, resolution.warnings))
}

#[cfg(not(feature = "modules"))]
fn load_cddl_file(
    path: &Path,
    _include_dirs: &[PathBuf],
) -> Result<(String, tps_cddl::Cddl, Vec<CddlDiag>), LoadCddlError> {
    let source = fs::read_to_string(path).map_err(|e| LoadCddlError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let pairs = lex(&source).map_err(|e| LoadCddlError::Parse {
        path: path.to_path_buf(),
        source: e,
        text: source.clone(),
    })?;
    let cddl_ast = build_cddl(pairs).map_err(|e| LoadCddlError::Parse {
        path: path.to_path_buf(),
        source: e,
        text: source.clone(),
    })?;
    Ok((source, cddl_ast, Vec::new()))
}

/// Renders a [`LoadCddlError`] (unless `quiet`) and returns the process exit code (always 1).
fn render_load_error(err: LoadCddlError, quiet: bool) -> i32 {
    match err {
        LoadCddlError::Io { path, source } => {
            if !quiet {
                eprintln!("error: cannot read {}: {source}", path.display());
            }
        }
        #[cfg(not(feature = "modules"))]
        LoadCddlError::Parse { path, source, text } => {
            let filename = path.to_string_lossy().into_owned();
            let span = match *source.location() {
                InputLocation::Pos(p) => (p, 0),
                InputLocation::Span((s, end)) => (s, end.saturating_sub(s)),
            };
            let report = ParseReport {
                message: source.message(),
                src: NamedSource::new(&filename, text),
                span,
            };
            render(&report, quiet);
        }
        #[cfg(feature = "modules")]
        LoadCddlError::Module(err) => {
            // Best effort: a parse failure inside a module file gets the same
            // graphical rendering as a top-level parse failure. Everything else
            // (not found / ambiguous / circular / undefined `from` name) is
            // rendered via `ModuleError`'s `Display` impl.
            if let ModuleError::Parse { path, source } = &err
                && let Ok(text) = fs::read_to_string(path)
            {
                let filename = path.to_string_lossy().into_owned();
                let span = match *source.location() {
                    InputLocation::Pos(p) => (p, 0),
                    InputLocation::Span((s, end)) => (s, end.saturating_sub(s)),
                };
                let report = ParseReport {
                    message: source.message(),
                    src: NamedSource::new(&filename, text),
                    span,
                };
                render(&report, quiet);
                return 1;
            }
            if !quiet {
                eprintln!("error: {err}");
            }
        }
    }
    1
}

// ─── validate subcommand ──────────────────────────────────────────────────────

fn run_validate(
    file: PathBuf,
    quiet: bool,
    dump_ast: bool,
    list_features: bool,
    config: Config,
    include_path: &[PathBuf],
) -> i32 {
    let (source, cddl_ast, module_warnings) = match load_cddl_file(&file, include_path) {
        Ok(v) => v,
        Err(e) => return render_load_error(e, quiet),
    };

    let filename = file.to_string_lossy().into_owned();

    if dump_ast {
        println!("{cddl_ast:#?}");
    }

    // ── Semantic validation ───────────────────────────────────────────────────
    let (mut diags, features) = validate_with_config_ex(&cddl_ast, &config);
    diags.extend(module_warnings);

    if !quiet {
        let handler = GraphicalReportHandler::new();
        for diag in &diags {
            let report = SemanticReport {
                diag,
                src: NamedSource::new(&filename, source.clone()),
            };
            let mut buf = String::new();
            handler.render_report(&mut buf, &report).unwrap_or(());
            eprintln!("{buf}");
        }
    }

    if list_features {
        let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
        for usage in &features {
            match grouped.iter_mut().find(|(name, _)| name == &usage.name) {
                Some((_, details)) => {
                    if let Some(detail) = &usage.detail
                        && !details.contains(detail) {
                            details.push(detail.clone());
                        }
                }
                None => grouped.push((
                    usage.name.clone(),
                    usage.detail.clone().into_iter().collect(),
                )),
            }
        }
        if grouped.is_empty() {
            println!("features used: none");
        } else {
            println!("features used:");
            for (name, details) in &grouped {
                if details.is_empty() {
                    println!("  {name}");
                } else {
                    println!("  {name} ({})", details.join(", "));
                }
            }
        }
    }

    let has_errors = diags.iter().any(|d| d.level == Level::Error);

    if !quiet && !has_errors && diags.is_empty() {
        println!("{filename}: ok");
    }

    if has_errors { 1 } else { 0 }
}

// ─── check-cbor subcommand ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_check_cbor(
    cddl_file: PathBuf,
    cbor_file: Option<PathBuf>,
    is_hex: bool,
    passthrough: bool,
    rule: Option<&str>,
    quiet: bool,
    config: Config,
    include_path: &[PathBuf],
) -> i32 {
    let (cddl_src, cddl_ast, _module_warnings) = match load_cddl_file(&cddl_file, include_path) {
        Ok(v) => v,
        Err(e) => return render_load_error(e, quiet),
    };
    let cddl_filename = cddl_file.to_string_lossy().into_owned();

    // Check schema itself is valid.
    let schema_diags = validate_with_config(&cddl_ast, &config);
    let schema_errors: Vec<_> = schema_diags
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    if !schema_errors.is_empty() {
        if !quiet {
            let handler = GraphicalReportHandler::new();
            for diag in &schema_diags {
                let report = SemanticReport {
                    diag,
                    src: NamedSource::new(&cddl_filename, cddl_src.clone()),
                };
                let mut buf = String::new();
                handler.render_report(&mut buf, &report).unwrap_or(());
                eprintln!("{buf}");
            }
        }
        return 1;
    }

    // ── Read CBOR bytes ───────────────────────────────────────────────────────
    let cbor_filename = cbor_file.as_ref().map_or_else(
        || "<stdin>".to_owned(),
        |p| p.to_string_lossy().into_owned(),
    );
    let raw: Vec<u8> = match cbor_file {
        Some(ref p) => match fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                if !quiet {
                    eprintln!("error: cannot read {}: {e}", p.display());
                }
                return 1;
            }
        },
        None => {
            use std::io::Read;
            let mut buf = Vec::new();
            if let Err(e) = std::io::stdin().read_to_end(&mut buf) {
                if !quiet {
                    eprintln!("error: cannot read stdin: {e}");
                }
                return 1;
            }
            buf
        }
    };

    let cbor_bytes = if is_hex {
        match decode_hex(&raw) {
            Ok(b) => b,
            Err(e) => {
                if !quiet {
                    eprintln!("error: invalid hex in {cbor_filename}: {e}");
                }
                return 1;
            }
        }
    } else {
        raw
    };

    // ── Run CBOR check ────────────────────────────────────────────────────────
    let mismatches = match check_cbor(&cbor_bytes, &cddl_ast, rule, &config) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    if !quiet {
        for m in &mismatches {
            let path = if m.path.is_empty() { "<root>" } else { &m.path };
            eprintln!("mismatch at {path}: expected {}, got {}", m.expected, m.got);
        }
    }

    if mismatches.is_empty() {
        if passthrough {
            use std::io::Write;
            if let Err(e) = std::io::stdout().write_all(&cbor_bytes) {
                if !quiet {
                    eprintln!("error: cannot write to stdout: {e}");
                }
                return 1;
            }
        } else if !quiet {
            println!("{cbor_filename}: ok");
        }
        0
    } else {
        1
    }
}

// ─── check-json subcommand ────────────────────────────────────────────────────

fn run_check_json(
    cddl_file: PathBuf,
    json_file: PathBuf,
    rule: Option<&str>,
    quiet: bool,
    config: Config,
    include_path: &[PathBuf],
) -> i32 {
    let (cddl_src, cddl_ast, _module_warnings) = match load_cddl_file(&cddl_file, include_path) {
        Ok(v) => v,
        Err(e) => return render_load_error(e, quiet),
    };
    let cddl_filename = cddl_file.to_string_lossy().into_owned();

    let schema_diags = validate_with_config(&cddl_ast, &config);
    let schema_errors: Vec<_> = schema_diags
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    if !schema_errors.is_empty() {
        if !quiet {
            let handler = GraphicalReportHandler::new();
            for diag in &schema_diags {
                let report = SemanticReport {
                    diag,
                    src: NamedSource::new(&cddl_filename, cddl_src.clone()),
                };
                let mut buf = String::new();
                handler.render_report(&mut buf, &report).unwrap_or(());
                eprintln!("{buf}");
            }
        }
        return 1;
    }

    // ── Read JSON bytes ───────────────────────────────────────────────────────
    let json_filename = json_file.to_string_lossy().into_owned();
    let json_bytes = match fs::read(&json_file) {
        Ok(b) => b,
        Err(e) => {
            if !quiet {
                eprintln!("error: cannot read {}: {e}", json_file.display());
            }
            return 1;
        }
    };

    // ── Run JSON check ────────────────────────────────────────────────────────
    let mismatches = match check_json(&json_bytes, &cddl_ast, rule, &config) {
        Ok(m) => m,
        Err(e) => {
            if !quiet {
                eprintln!("error: invalid JSON in {json_filename}: {e}");
            }
            return 1;
        }
    };

    if !quiet {
        for m in &mismatches {
            let path = if m.path.is_empty() { "<root>" } else { &m.path };
            eprintln!("mismatch at {path}: expected {}, got {}", m.expected, m.got);
        }
    }

    if mismatches.is_empty() {
        if !quiet {
            println!("{json_filename}: ok");
        }
        0
    } else {
        1
    }
}

// ─── gen-decoder subcommand ───────────────────────────────────────────────────

fn run_gen_decoder(
    cddl_file: PathBuf,
    rule: Option<&str>,
    output: Option<PathBuf>,
    feature_manifest: Option<PathBuf>,
    quiet: bool,
    config: Config,
    include_path: &[PathBuf],
) -> i32 {
    let (cddl_src, cddl_ast, _module_warnings) = match load_cddl_file(&cddl_file, include_path) {
        Ok(v) => v,
        Err(e) => return render_load_error(e, quiet),
    };
    let cddl_filename = cddl_file.to_string_lossy().into_owned();

    let schema_diags = validate_with_config(&cddl_ast, &config);
    let schema_errors: Vec<_> = schema_diags
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    if !schema_errors.is_empty() {
        if !quiet {
            let handler = GraphicalReportHandler::new();
            for diag in &schema_diags {
                let report = SemanticReport {
                    diag,
                    src: NamedSource::new(&cddl_filename, cddl_src.clone()),
                };
                let mut buf = String::new();
                handler.render_report(&mut buf, &report).unwrap_or(());
                eprintln!("{buf}");
            }
        }
        return 1;
    }

    let manifest = match load_feature_manifest(feature_manifest.as_deref()) {
        Ok(m) => m,
        Err(e) => {
            if !quiet {
                eprintln!("error: {e}");
            }
            return 1;
        }
    };

    // ── Generate ──────────────────────────────────────────────────────────────
    let generated = match gen_decoder(&cddl_ast, rule, &config, &manifest) {
        Ok(code) => code,
        Err(e) => {
            if !quiet {
                eprintln!("error: {e}");
            }
            return 1;
        }
    };

    match output {
        Some(path) => {
            if let Err(e) = fs::write(&path, &generated) {
                if !quiet {
                    eprintln!("error: cannot write {}: {e}", path.display());
                }
                return 1;
            }
        }
        None => {
            print!("{generated}");
        }
    }
    0
}

// ─── gen-encoder subcommand ───────────────────────────────────────────────────

fn run_gen_encoder(
    cddl_file: PathBuf,
    rule: Option<&str>,
    output: Option<PathBuf>,
    feature_manifest: Option<PathBuf>,
    quiet: bool,
    config: Config,
    include_path: &[PathBuf],
) -> i32 {
    let (cddl_src, cddl_ast, _module_warnings) = match load_cddl_file(&cddl_file, include_path) {
        Ok(v) => v,
        Err(e) => return render_load_error(e, quiet),
    };
    let cddl_filename = cddl_file.to_string_lossy().into_owned();

    let schema_diags = validate_with_config(&cddl_ast, &config);
    let schema_errors: Vec<_> = schema_diags
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    if !schema_errors.is_empty() {
        if !quiet {
            let handler = GraphicalReportHandler::new();
            for diag in &schema_diags {
                let report = SemanticReport {
                    diag,
                    src: NamedSource::new(&cddl_filename, cddl_src.clone()),
                };
                let mut buf = String::new();
                handler.render_report(&mut buf, &report).unwrap_or(());
                eprintln!("{buf}");
            }
        }
        return 1;
    }

    let manifest = match load_feature_manifest(feature_manifest.as_deref()) {
        Ok(m) => m,
        Err(e) => {
            if !quiet {
                eprintln!("error: {e}");
            }
            return 1;
        }
    };

    let generated = match gen_encoder(&cddl_ast, rule, &config, &manifest) {
        Ok(code) => code,
        Err(e) => {
            if !quiet {
                eprintln!("error: {e}");
            }
            return 1;
        }
    };

    match output {
        Some(path) => {
            if let Err(e) = fs::write(&path, &generated) {
                if !quiet {
                    eprintln!("error: cannot write {}: {e}", path.display());
                }
                return 1;
            }
        }
        None => {
            print!("{generated}");
        }
    }
    0
}

/// Loads a `--feature-manifest` file, or an empty manifest if none was given
/// (schemas with no `.feature` usage don't need one).
fn load_feature_manifest(path: Option<&Path>) -> Result<FeatureManifest, String> {
    match path {
        None => Ok(FeatureManifest::from_json("{}").expect("empty manifest is valid JSON")),
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            FeatureManifest::from_json(&text)
                .map_err(|e| format!("{}: {e}", path.display()))
        }
    }
}

// ─── Miette diagnostic wrappers ───────────────────────────────────────────────

/// Wraps a semantic [`CddlDiag`] for rendering via miette.
struct SemanticReport<'s> {
    diag: &'s CddlDiag,
    src: NamedSource<String>,
}

impl fmt::Display for SemanticReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diag.message)
    }
}

impl fmt::Debug for SemanticReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diag.message)
    }
}

impl std::error::Error for SemanticReport<'_> {}

impl Diagnostic for SemanticReport<'_> {
    fn severity(&self) -> Option<miette::Severity> {
        Some(match self.diag.level {
            Level::Error => miette::Severity::Error,
            Level::Warning => miette::Severity::Warning,
        })
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let start = self.diag.span.start;
        let len = self.diag.span.end.saturating_sub(start);
        // Skip zero-width spans at position 0 (value literals with no stored span).
        if start == 0 && len == 0 {
            return None;
        }
        Some(Box::new(std::iter::once(LabeledSpan::at(
            SourceSpan::new(start.into(), len),
            match self.diag.level {
                Level::Error => "error here",
                Level::Warning => "warning here",
            },
        ))))
    }
}

/// Wraps a pest parse error for rendering via miette.
struct ParseReport {
    message: String,
    src: NamedSource<String>,
    span: (usize, usize), // (start, len)
}

impl fmt::Display for ParseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Debug for ParseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseReport {}

impl Diagnostic for ParseReport {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let (start, len) = self.span;
        Some(Box::new(std::iter::once(LabeledSpan::at(
            SourceSpan::new(start.into(), len.max(1)),
            "unexpected input",
        ))))
    }
}

fn render(diag: &dyn Diagnostic, quiet: bool) {
    if quiet {
        return;
    }
    let mut buf = String::new();
    GraphicalReportHandler::new()
        .render_report(&mut buf, diag)
        .unwrap_or(());
    eprintln!("{buf}");
}

/// Decode ASCII hex bytes (whitespace stripped) into raw bytes.
fn decode_hex(input: &[u8]) -> Result<Vec<u8>, String> {
    let digits: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits ({})", digits.len()));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks(2) {
        let hi = hex_digit(pair[0]).map_err(|c| format!("invalid hex character: {c:?}"))?;
        let lo = hex_digit(pair[1]).map_err(|c| format!("invalid hex character: {c:?}"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Result<u8, char> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(b as char),
    }
}
