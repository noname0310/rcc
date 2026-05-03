//! Clap-based command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use rcc_session::{EmitKind, OptLevel, TargetInfo, TargetTriple};

use crate::ExitCode;

/// The `rcc` command-line interface.
#[derive(Debug, Parser, Clone)]
#[command(name = "rcc", about = "rcc: a Rust-based C99 compiler")]
pub struct Cli {
    /// Input `.c` file(s).
    #[arg(required = true)]
    pub input: Vec<PathBuf>,

    /// Output path (`-o`).
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    /// Preserve intermediate artifacts in the current directory or a chosen directory.
    #[arg(
        long = "save-temps",
        value_name = "DIR",
        num_args = 0..=1,
        default_missing_value = ".",
        require_equals = true
    )]
    pub save_temps: Option<PathBuf>,

    /// Compile to object file and stop before linking (`-c`).
    #[arg(short = 'c', conflicts_with_all = ["emit_assembly", "preprocess_only", "emit"])]
    pub compile_only: bool,

    /// Compile to assembly text and stop (`-S`).
    #[arg(short = 'S', conflicts_with_all = ["compile_only", "preprocess_only", "emit"])]
    pub emit_assembly: bool,

    /// Preprocess only and stop (`-E`).
    #[arg(short = 'E', conflicts_with_all = ["compile_only", "emit_assembly", "emit"])]
    pub preprocess_only: bool,

    /// Preprocessor include path (`-I`). May repeat.
    #[arg(short = 'I', long = "include-path")]
    pub include_paths: Vec<PathBuf>,

    /// Command-line `-D` macro definitions: `NAME` or `NAME=VAL`.
    #[arg(short = 'D', long = "define", value_parser = parse_define)]
    pub defines: Vec<(String, Option<String>)>,

    /// Intermediate stage(s) to emit.
    #[arg(long = "emit", value_enum)]
    pub emit: Vec<EmitKind>,

    /// Target triple (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, ...).
    #[arg(long = "target", value_parser = parse_target)]
    pub target: Option<TargetInfo>,

    /// C language standard (`-std=c99` only for now).
    #[arg(long = "std", value_name = "STANDARD", value_parser = parse_standard)]
    pub standard: Option<String>,

    /// GCC compatibility alias for C89 mode. Parsed, but currently unsupported.
    #[arg(long = "ansi", action = ArgAction::SetTrue)]
    pub ansi: bool,

    /// Optimisation level.
    #[arg(short = 'O', long = "opt-level", value_enum, default_value_t = OptLevel::None)]
    pub opt_level: OptLevel,

    /// Emit debug information in backend outputs (`-g`).
    #[arg(short = 'g', action = ArgAction::SetTrue)]
    pub debug_info: bool,

    /// Print selected tools and subprocess command lines.
    #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
    pub verbose: bool,

    /// GCC-style `-f<flag>` compatibility options.
    #[arg(short = 'f', value_name = "FLAG", action = ArgAction::Append)]
    pub feature_flags: Vec<String>,

    /// Suppress all warnings (`-w`).
    #[arg(short = 'w', action = ArgAction::SetTrue)]
    pub suppress_warnings: bool,

    /// Warning control flag (`-Wall`, `-Werror`, `-Wno-name`, ...).
    #[arg(short = 'W', value_name = "warning", action = ArgAction::Append)]
    pub warning_flags: Vec<String>,

    /// Link against library (`-l<lib>`). May repeat.
    #[arg(short = 'l', value_name = "LIB", action = ArgAction::Append)]
    pub libraries: Vec<String>,

    /// Add a library search path (`-L<path>`). May repeat.
    #[arg(short = 'L', value_name = "DIR", action = ArgAction::Append)]
    pub library_paths: Vec<PathBuf>,

    /// Produce a shared library.
    #[arg(long)]
    pub shared: bool,

    /// Request static linking.
    #[arg(long = "static")]
    pub static_link: bool,

    /// Request a position-independent executable.
    #[arg(long)]
    pub pie: bool,

    /// Disable position-independent executable linking.
    #[arg(long = "no-pie", conflicts_with = "pie")]
    pub no_pie: bool,

    /// Include GPL-licensed test suites during `fetch-testsuites` / conformance runs.
    #[arg(long)]
    pub include_gpl_tests: bool,
}

impl Cli {
    /// Parse CLI args, accepting GCC-style single-dash long linker flags.
    pub fn parse() -> Self {
        Self::try_parse_from(std::env::args_os()).unwrap_or_else(|err| {
            let _ = err.print();
            std::process::exit(ExitCode::Usage.code());
        })
    }

    /// Parse from an explicit iterator, normalising GCC driver spellings first.
    pub fn try_parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        <Self as Parser>::try_parse_from(normalize_driver_args(args))
    }
}

fn normalize_driver_args<I, T>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    args.into_iter()
        .map(|arg| {
            let arg = arg.into();
            let text = arg.to_string_lossy();
            if let Some(standard) = text.strip_prefix("-std=") {
                return OsString::from(format!("--std={standard}"));
            }
            match text.as_ref() {
                "-std" => OsString::from("--std"),
                "-ansi" => OsString::from("--ansi"),
                "-shared" => OsString::from("--shared"),
                "-static" => OsString::from("--static"),
                "-pie" => OsString::from("--pie"),
                "-no-pie" => OsString::from("--no-pie"),
                _ => arg,
            }
        })
        .collect()
}

fn parse_define(raw: &str) -> Result<(String, Option<String>), String> {
    if let Some((k, v)) = raw.split_once('=') {
        Ok((k.to_owned(), Some(v.to_owned())))
    } else {
        Ok((raw.to_owned(), None))
    }
}

fn parse_target(raw: &str) -> Result<TargetInfo, String> {
    let triple = TargetTriple::new(raw);
    TargetInfo::from_triple(&triple).map_err(|err| err.to_string())
}

fn parse_standard(raw: &str) -> Result<String, String> {
    match raw {
        "c99" => Ok(raw.to_owned()),
        other => Err(format!("unsupported standard '{other}'; only c99 is supported")),
    }
}
