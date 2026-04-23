//! Clap-based command-line interface.

use std::path::PathBuf;

use clap::Parser;
use rcc_session::{EmitKind, OptLevel};

/// The `rcc` command-line interface.
#[derive(Debug, Parser, Clone)]
#[command(name = "rcc", about = "rcc: a Rust-based C99 compiler")]
pub struct Cli {
    /// Input `.c` file.
    pub input: PathBuf,

    /// Output path (`-o`).
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    /// Preprocessor include path (`-I`). May repeat.
    #[arg(short = 'I', long = "include-path")]
    pub include_paths: Vec<PathBuf>,

    /// Command-line `-D` macro definitions: `NAME` or `NAME=VAL`.
    #[arg(short = 'D', long = "define", value_parser = parse_define)]
    pub defines: Vec<(String, Option<String>)>,

    /// Intermediate stage(s) to emit.
    #[arg(long = "emit", value_enum)]
    pub emit: Vec<EmitKind>,

    /// Optimisation level.
    #[arg(short = 'O', long = "opt-level", value_enum, default_value_t = OptLevel::None)]
    pub opt_level: OptLevel,

    /// Include GPL-licensed test suites during `fetch-testsuites` / conformance runs.
    #[arg(long)]
    pub include_gpl_tests: bool,
}

fn parse_define(raw: &str) -> Result<(String, Option<String>), String> {
    if let Some((k, v)) = raw.split_once('=') {
        Ok((k.to_owned(), Some(v.to_owned())))
    } else {
        Ok((raw.to_owned(), None))
    }
}
