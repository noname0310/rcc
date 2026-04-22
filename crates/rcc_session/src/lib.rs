//! `rcc_session`: compilation-wide state (options, source map, diagnostics).
//!
//! Analogous to `rustc_session`. Owns the three things every pass needs:
//! - an [`Options`] bundle parsed from CLI/args,
//! - a [`rcc_span::SourceMap`] holding every loaded file,
//! - a [`rcc_errors::Handler`] accepting diagnostics.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;

use rcc_errors::{Handler, StderrEmitter};
use rcc_span::{Interner, SourceMap};

/// Stages at which the driver can dump intermediate state.
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EmitKind {
    /// Raw preprocessing tokens.
    Tokens,
    /// Preprocessed token stream.
    Pp,
    /// AST pretty-print.
    Ast,
    /// HIR pretty-print.
    Hir,
    /// MIR/CFG pretty-print.
    Mir,
    /// Textual LLVM IR.
    LlvmIr,
    /// Target assembly.
    Asm,
    /// Object file.
    Obj,
}

/// Target triple (parsed lazily by `rcc_codegen_llvm`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetTriple(pub String);

/// LLVM-style optimisation level, mapped 1:1 to `OptimizationLevel`.
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum OptLevel {
    /// `-O0`
    None,
    /// `-O1`
    Less,
    /// `-O2`
    Default,
    /// `-O3`
    Aggressive,
}

/// CLI / driver options. Intentionally plain data for easy wiring by clap.
#[derive(Clone, Debug)]
pub struct Options {
    /// `-I` include paths.
    pub include_paths: Vec<PathBuf>,
    /// Command-line `-D` macro definitions: `(name, value)`.
    pub cli_defines: Vec<(String, Option<String>)>,
    /// Target triple. `None` = host.
    pub target: Option<TargetTriple>,
    /// What to emit (may be multiple).
    pub emit: Vec<EmitKind>,
    /// Output path. `None` = stdout / default.
    pub output: Option<PathBuf>,
    /// Optimisation level.
    pub opt_level: OptLevel,
    /// Enable `--include-gpl` test suites.
    pub include_gpl_tests: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            cli_defines: Vec::new(),
            target: None,
            emit: Vec::new(),
            output: None,
            opt_level: OptLevel::None,
            include_gpl_tests: false,
        }
    }
}

/// Compilation-wide state. Usually passed `&mut` down the pipeline.
pub struct Session {
    /// Parsed CLI options.
    pub opts: Options,
    /// All loaded source files.
    pub source_map: SourceMap,
    /// Symbol interner (identifiers + string literals).
    pub interner: Interner,
    /// Diagnostic sink.
    pub handler: Handler,
}

impl Session {
    /// Build a session that prints diagnostics to stderr.
    pub fn new(opts: Options) -> Self {
        Self {
            opts,
            source_map: SourceMap::new(),
            interner: Interner::new(),
            handler: Handler::with_emitter(Box::new(StderrEmitter)),
        }
    }

    /// Build a session with a user-supplied `Handler`. Used by tests.
    pub fn with_handler(opts: Options, handler: Handler) -> Self {
        Self { opts, source_map: SourceMap::new(), interner: Interner::new(), handler }
    }
}
