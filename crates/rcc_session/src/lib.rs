//! `rcc_session`: compilation-wide state (options, source map, diagnostics).
//!
//! Analogous to `rustc_session`. Owns the three things every pass needs:
//! - an [`Options`] bundle parsed from CLI/args,
//! - a [`rcc_span::SourceMap`] holding every loaded file,
//! - a [`rcc_errors::Handler`] accepting diagnostics.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use rcc_errors::WarningConfig;
use rcc_errors::{CaptureEmitter, Handler, StderrEmitter};
use rcc_span::{Interner, SourceMap};
pub use rcc_target::{Arch, DataModel, Environment, Os, TargetError, TargetInfo, TargetTriple};

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
    /// Target-specific C layout and backend metadata.
    pub target: TargetInfo,
    /// What to emit (may be multiple).
    pub emit: Vec<EmitKind>,
    /// Output path. `None` = stdout / default.
    pub output: Option<PathBuf>,
    /// Preserve intermediate artifacts. `None` removes private temporaries;
    /// `Some(dir)` writes saved temporaries under `dir`.
    pub save_temps: Option<PathBuf>,
    /// Optimisation level.
    pub opt_level: OptLevel,
    /// Warning filtering and promotion policy.
    pub warning_config: WarningConfig,
    /// Host-cc linker options for final executable/shared-library emission.
    pub link: LinkOptions,
    /// Emit LLVM debug metadata when the LLVM backend is enabled.
    ///
    /// CLI `-g` wiring is owned by the driver phase; backend tests can set this
    /// directly to exercise the debug-info path without depending on object
    /// emission or linker support.
    pub debug_info: bool,
    /// Enable `--include-gpl` test suites.
    pub include_gpl_tests: bool,
    /// Enable the GNU `, ## __VA_ARGS__` comma-elision extension.
    ///
    /// When a variadic function-like macro is invoked with zero
    /// trailing arguments and its replacement list contains the token
    /// sequence `,`-`##`-`__VA_ARGS__`, GCC / Clang drop the comma
    /// together with the empty `__VA_ARGS__`. This is a popular
    /// extension but not part of C99; turning it on here opts in.
    /// Off by default — the strict C99 behaviour is to expand
    /// `__VA_ARGS__` to empty and leave the comma in place.
    pub gnu_va_args_elision: bool,
    /// Enable permissive macro redefinition (GNU extension).
    ///
    /// When `true`, a non-identical redefinition of a macro that
    /// preserves the *kind* (object-like ↔ object-like, or
    /// function-like with the same arity and variadicity) is
    /// downgraded from E0022 (error) to W0006 (warning), and the
    /// new definition replaces the old one. Off by default.
    pub gnu_permissive_redefinition: bool,
    /// Enable the GNU `args...` named-variadic extension.
    ///
    /// When `true`, `parse_function_like_signature` accepts a final
    /// `IDENT...` (identifier immediately followed by `...`) as a
    /// named variadic parameter. In the replacement list, uses of
    /// that identifier resolve to the variadic argument slot. Off
    /// by default.
    pub gnu_named_variadic: bool,
    /// Enable permissive token-paste across pp-number boundaries.
    ///
    /// When `true`, if `##` concatenation re-lexes to multiple tokens
    /// but the combined text forms a valid pp-number, the paste
    /// succeeds as a single pp-number token instead of emitting
    /// E0025. Off by default.
    pub gnu_permissive_paste: bool,
    /// Enable GNU statement expressions `({ ... })` without a warning.
    ///
    /// The parser accepts the extension in all modes so GNU-flavoured
    /// test suites can keep an AST shape for downstream HIR/CFG work.
    /// With this option off, use of the construct emits W0013 as a
    /// strict-C99 compatibility warning.
    pub gnu_statement_expressions: bool,
    /// Enable GNU initializer range designators `[lo ... hi]` without a warning.
    ///
    /// The parser accepts the syntax in all modes so initializer
    /// lowering can see an explicit range node. With this option off,
    /// use of the construct emits W0014 as a strict-C99 compatibility
    /// warning.
    pub gnu_range_designators: bool,

    /// Enable GNU `__attribute__((...))` syntax without a warning.
    ///
    /// The parser preserves attributes for later phase-14 semantic
    /// handling. When this flag is false, syntax still parses but each
    /// attribute group emits W0015 as a strict-C99 compatibility warning.
    pub gnu_attributes: bool,
    /// Enable GNU inline assembly syntax without a warning.
    ///
    /// The parser preserves `asm` / `__asm` / `__asm__` statements for
    /// later extension validation and LLVM lowering. When this flag is
    /// false, syntax still parses but emits W0016 as a strict-C99
    /// compatibility warning.
    pub gnu_inline_asm: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            cli_defines: Vec::new(),
            target: TargetInfo::baseline(),
            emit: Vec::new(),
            output: None,
            save_temps: None,
            opt_level: OptLevel::None,
            warning_config: WarningConfig::default(),
            link: LinkOptions::default(),
            debug_info: false,
            include_gpl_tests: false,
            gnu_va_args_elision: false,
            gnu_permissive_redefinition: false,
            gnu_named_variadic: false,
            gnu_permissive_paste: false,
            gnu_statement_expressions: false,
            gnu_range_designators: false,
            gnu_attributes: false,
            gnu_inline_asm: false,
        }
    }
}

/// Options forwarded to the host C compiler when it is used as linker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkOptions {
    /// Library names passed as `-l<name>`.
    pub libraries: Vec<String>,
    /// Library search paths passed as `-L<path>`.
    pub library_paths: Vec<PathBuf>,
    /// Raw `-Wl,...` arguments passed through to the host C compiler.
    pub linker_args: Vec<String>,
    /// Produce a shared library (`-shared`).
    pub shared: bool,
    /// Request static linking (`-static`).
    pub static_link: bool,
    /// PIE control: `Some(true)` => `-pie`, `Some(false)` => `-no-pie`.
    pub pie: Option<bool>,
}

/// Compilation-wide state. Usually passed `&mut` down the pipeline.
pub struct Session {
    /// Parsed CLI options.
    pub opts: Options,
    /// All loaded source files (shared with the diagnostic emitter).
    pub source_map: Arc<RwLock<SourceMap>>,
    /// Symbol interner (identifiers + string literals).
    pub interner: Interner,
    /// Diagnostic sink.
    pub handler: Handler,
}

impl Session {
    /// Build a session that prints diagnostics to stderr.
    pub fn new(opts: Options) -> Self {
        let sm = Arc::new(RwLock::new(SourceMap::new()));
        let mut handler = Handler::with_emitter(Box::new(StderrEmitter::new(sm.clone())));
        handler.set_warning_config(opts.warning_config.clone());
        Self { opts, source_map: sm.clone(), interner: Interner::new(), handler }
    }

    /// Build a session with a user-supplied `Handler`. Used by tests.
    pub fn with_handler(opts: Options, mut handler: Handler) -> Self {
        handler.set_warning_config(opts.warning_config.clone());
        Self {
            source_map: Arc::new(RwLock::new(SourceMap::new())),
            opts,
            interner: Interner::new(),
            handler,
        }
    }

    /// Build a test session wired to a [`CaptureEmitter`].
    ///
    /// Returns `(Session, CaptureEmitter)` so tests can emit diagnostics
    /// through the session and then inspect them via the capture handle.
    pub fn for_test() -> (Self, CaptureEmitter) {
        let sm = Arc::new(RwLock::new(SourceMap::new()));
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap.clone()));
        let sess =
            Self { opts: Options::default(), source_map: sm, interner: Interner::new(), handler };
        (sess, cap)
    }
}
