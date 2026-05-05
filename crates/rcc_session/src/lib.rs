//! `rcc_session`: compilation-wide state (options, source map, diagnostics).
//!
//! Analogous to `rustc_session`. Owns the three things every pass needs:
//! - an [`Options`] bundle parsed from CLI/args,
//! - a [`rcc_span::SourceMap`] holding every loaded file,
//! - a [`rcc_errors::Handler`] accepting diagnostics.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub use rcc_errors::WarningConfig;
use rcc_errors::{CaptureEmitter, Handler, StderrEmitter};
use rcc_span::{Interner, SourceMap};
pub use rcc_target::{
    Arch, DataModel, Endianness, Environment, Os, TargetError, TargetInfo, TargetTriple,
};

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
    /// Command-line `-U` macro undefines, applied after `-D` and predefined macros.
    pub cli_undefines: Vec<String>,
    /// Target-specific C layout and backend metadata.
    pub target: TargetInfo,
    /// What to emit (may be multiple).
    pub emit: Vec<EmitKind>,
    /// Output path. `None` = stdout / default.
    pub output: Option<PathBuf>,
    /// Preserve intermediate artifacts. `None` removes private temporaries;
    /// `Some(dir)` writes saved temporaries under `dir`.
    pub save_temps: Option<PathBuf>,
    /// Make-compatible dependency-file emission options.
    pub dependencies: DependencyOptions,
    /// Optimisation level.
    pub opt_level: OptLevel,
    /// Warning filtering and promotion policy.
    pub warning_config: WarningConfig,
    /// LLVM linker-driver options for final executable/shared-library emission.
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
    /// Enable GNU binary integer constants (`0b1010` / `0B1010`).
    ///
    /// C99 has decimal, octal, and hexadecimal integer constants only.
    /// GCC/Clang accept a `0b` prefix as an extension. With this option
    /// off, `0b10` is diagnosed as the strict C99 malformed-octal case.
    pub gnu_binary_integer_literals: bool,
    /// Enable GNU statement expressions `({ ... })` without a warning.
    ///
    /// The parser accepts the extension in all modes so GNU-flavoured
    /// test suites can keep an AST shape for downstream HIR/CFG work.
    /// With this option off, use of the construct emits W0013 as a
    /// strict-C99 compatibility warning.
    pub gnu_statement_expressions: bool,
    /// Enable GNU omitted-middle conditional expressions `a ?: b` without a warning.
    ///
    /// The parser accepts this syntax in all modes so downstream phases can
    /// preserve the required "evaluate `a` exactly once" semantics. With this
    /// option off, use of the construct emits W0017 as a strict-C99
    /// compatibility warning.
    pub gnu_omitted_conditional_operand: bool,
    /// Enable GNU conditional expressions with exactly one `void` operand.
    ///
    /// GNU C accepts `cond ? value : (void)expr` as a void expression. C99
    /// requires a diagnostic unless both arms are void; with this option off,
    /// type checking emits W0018 while still preserving the GNU-compatible
    /// void result so statement-position uses can continue.
    pub gnu_conditional_void_operand: bool,
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
    /// Enable GNU case ranges `case lo ... hi:` without a warning.
    ///
    /// The parser accepts the syntax in all modes so switch lowering can
    /// preserve the range. With this option off, each range emits W0019 as
    /// a strict-C99 compatibility warning.
    pub gnu_case_ranges: bool,
    /// Enable GNU labels-as-values (`&&label`) and computed goto (`goto *expr`)
    /// without a warning.
    ///
    /// The parser accepts the syntax in all modes and HIR/CFG/codegen lower it
    /// through LLVM `blockaddress` / `indirectbr`. With this option off, uses
    /// emit W0020 as strict-C99 compatibility warnings.
    pub gnu_labels_as_values: bool,
    /// Enable GNU lvalue comma expressions without a warning.
    ///
    /// GNU C treats `a, b` as an lvalue when `b` is an lvalue. C99 always
    /// classifies comma expressions as rvalues, so with this option off the
    /// type checker emits W0021 while preserving GNU semantics for recovery.
    pub gnu_lvalue_comma: bool,
    /// Enable GNU `typeof` type specifiers without a warning.
    ///
    /// GNU C accepts `typeof (expr)` and `typeof (type-name)` as declaration
    /// specifiers. The parser preserves the syntax in all modes for
    /// compatibility suites; with this option off, each use emits W0024 as a
    /// strict-C99 compatibility warning.
    pub gnu_typeof: bool,
    /// Enable GNU `__alignof__` expressions without a warning.
    ///
    /// GNU C accepts `__alignof__(expr)` and `__alignof__(type-name)` as an
    /// extension. The parser preserves both forms in strict mode so downstream
    /// layout-sensitive tests can still run, but emits W0025 unless this flag
    /// is enabled.
    pub gnu_alignof: bool,
    /// Enable `#pragma pack(push, N)` / `#pragma pack(pop)` record packing.
    ///
    /// GCC and MSVC use this pragma family to alter subsequent record layout.
    /// rcc lowers supported `pack(push,1)` regions into an explicit packed
    /// record attribute in the preprocessor so later phases do not need a
    /// source-span side table.
    pub gnu_pragma_pack: bool,
    /// Use Microsoft-compatible bit-field allocation for record layout.
    ///
    /// This is off for the C99/Linux SysV default. Compatibility suites can
    /// opt in through `-fms-bitfields` or the GCC-compatible
    /// `-mms-bitfields` spelling normalized by the driver.
    pub ms_bitfields: bool,
    /// Enable GNU `__FUNCTION__` predefined function name alias without a warning.
    ///
    /// C99 defines `__func__` as an implicit function-scope identifier. GNU C
    /// also accepts `__FUNCTION__` with the same string payload. With this
    /// option off, HIR lowering emits W0022 while preserving the alias for
    /// compatibility tests.
    pub gnu_function_names: bool,
    /// Enable GCC `-finstrument-functions` entry/exit hooks.
    ///
    /// When enabled, generated functions call `__cyg_profile_func_enter` at
    /// function entry and `__cyg_profile_func_exit` before each return unless
    /// the function has GNU `no_instrument_function`.
    pub instrument_functions: bool,
    /// Enable chibicc/GNU `__va_area__` without a warning.
    ///
    /// `__va_area__` is a non-standard compatibility identifier used by
    /// chibicc tests to read the current variadic function's `va_list` save
    /// area. With this option off, HIR lowering still preserves it inside
    /// variadic functions but emits W0023.
    pub gnu_va_area: bool,
    /// Enable GNU89/chibicc inline emission semantics.
    ///
    /// Strict C99 plain `inline` external-linkage definitions do not provide an
    /// external definition. chibicc fixtures expect the older GNU89 behaviour
    /// where a plain inline definition is emitted normally; this option keeps
    /// that compatibility mode explicit.
    pub gnu89_inline: bool,
    /// Enable common GNU builtin libc aliases and predefined scalar limits.
    ///
    /// GCC torture tests often use identifiers such as `__builtin_abort`,
    /// `__builtin_memcpy`, `__CHAR_BIT__`, and `__INT_MAX__` without including
    /// libc headers. Strict C99 keeps these names undeclared; this compatibility
    /// mode maps builtin libc names to normal external libc calls and injects
    /// the matching prototypes.
    pub gnu_builtin_libcalls: bool,
    /// Enable GNU/C89-style implicit function declarations for call expressions.
    ///
    /// C99 removed implicit `int` declarations. With this option off, calling an
    /// undeclared identifier remains E0071. With it enabled, HIR lowering
    /// synthesizes a prototype-less `extern int name()` declaration for the
    /// callee and emits W0029 when the named warning is enabled.
    pub gnu_implicit_function_declaration: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            cli_defines: Vec::new(),
            cli_undefines: Vec::new(),
            target: TargetInfo::baseline(),
            emit: Vec::new(),
            output: None,
            save_temps: None,
            dependencies: DependencyOptions::default(),
            opt_level: OptLevel::None,
            warning_config: WarningConfig::default(),
            link: LinkOptions::default(),
            debug_info: false,
            include_gpl_tests: false,
            gnu_va_args_elision: false,
            gnu_permissive_redefinition: false,
            gnu_named_variadic: false,
            gnu_permissive_paste: false,
            gnu_binary_integer_literals: false,
            gnu_statement_expressions: false,
            gnu_omitted_conditional_operand: false,
            gnu_conditional_void_operand: false,
            gnu_range_designators: false,
            gnu_attributes: false,
            gnu_inline_asm: false,
            gnu_case_ranges: false,
            gnu_labels_as_values: false,
            gnu_lvalue_comma: false,
            gnu_typeof: false,
            gnu_alignof: false,
            gnu_pragma_pack: false,
            ms_bitfields: false,
            gnu_function_names: false,
            instrument_functions: false,
            gnu_va_area: false,
            gnu89_inline: false,
            gnu_builtin_libcalls: false,
            gnu_implicit_function_declaration: false,
        }
    }
}

/// Make-compatible dependency generation mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DependencyMode {
    /// Emit dependencies and stop after preprocessing (`-M` / `-MM`).
    PreprocessOnly,
    /// Emit dependencies as a side effect of normal compilation (`-MD` / `-MMD`).
    SideEffect,
}

/// A makefile dependency target supplied by the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyTarget {
    /// Raw target spelling from `-MT` / `-MQ`.
    pub text: String,
    /// Whether the spelling came from `-MQ` and must be make-escaped.
    pub quote: bool,
}

/// Options for make-compatible dependency generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyOptions {
    /// Active dependency generation mode. `None` means disabled.
    pub mode: Option<DependencyMode>,
    /// Include headers reached through `<...>` search. False for `-MM` / `-MMD`.
    pub include_system_headers: bool,
    /// Explicit dependency output path from `-MF`.
    pub output: Option<PathBuf>,
    /// Explicit make targets from `-MT` / `-MQ`.
    pub targets: Vec<DependencyTarget>,
}

impl Default for DependencyOptions {
    fn default() -> Self {
        Self { mode: None, include_system_headers: true, output: None, targets: Vec::new() }
    }
}

/// One source-file prerequisite discovered during preprocessing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDependency {
    /// Header path that was resolved and loaded.
    pub path: PathBuf,
    /// True when the include used the `<...>` system-header form.
    pub system: bool,
}

/// Options forwarded to the LLVM linker driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkOptions {
    /// Explicit clang-compatible linker driver path. `None` discovers `clang`.
    pub linker_driver: Option<PathBuf>,
    /// Force the linker driver to use LLVM lld (`-fuse-ld=lld`).
    pub use_lld: bool,
    /// Library names passed as `-l<name>`.
    pub libraries: Vec<String>,
    /// Library search paths passed as `-L<path>`.
    pub library_paths: Vec<PathBuf>,
    /// Raw `-Wl,...` arguments passed through to the clang-compatible driver.
    pub linker_args: Vec<String>,
    /// Produce a shared library (`-shared`).
    pub shared: bool,
    /// Request static linking (`-static`).
    pub static_link: bool,
    /// PIE control: `Some(true)` => `-pie`, `Some(false)` => `-no-pie`.
    pub pie: Option<bool>,
    /// Print selected tools and subprocess command lines to stderr.
    pub verbose: bool,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            linker_driver: None,
            use_lld: true,
            libraries: Vec::new(),
            library_paths: Vec::new(),
            linker_args: Vec::new(),
            shared: false,
            static_link: false,
            pie: None,
            verbose: false,
        }
    }
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
    /// Headers resolved by preprocessing, in encounter order.
    pub source_dependencies: Arc<RwLock<Vec<SourceDependency>>>,
    /// In-memory files used by tests and fuzz targets.
    virtual_files: Arc<RwLock<HashMap<PathBuf, Arc<str>>>>,
}

impl Session {
    /// Build a session that prints diagnostics to stderr.
    pub fn new(opts: Options) -> Self {
        let sm = Arc::new(RwLock::new(SourceMap::new()));
        let mut handler = Handler::with_emitter(Box::new(StderrEmitter::new(sm.clone())));
        handler.set_warning_config(opts.warning_config.clone());
        Self {
            opts,
            source_map: sm.clone(),
            interner: Interner::new(),
            handler,
            source_dependencies: Arc::new(RwLock::new(Vec::new())),
            virtual_files: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Build a session with a user-supplied `Handler`. Used by tests.
    pub fn with_handler(opts: Options, mut handler: Handler) -> Self {
        handler.set_warning_config(opts.warning_config.clone());
        Self {
            source_map: Arc::new(RwLock::new(SourceMap::new())),
            opts,
            interner: Interner::new(),
            handler,
            source_dependencies: Arc::new(RwLock::new(Vec::new())),
            virtual_files: Arc::new(RwLock::new(HashMap::new())),
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
        let sess = Self {
            opts: Options::default(),
            source_map: sm,
            interner: Interner::new(),
            handler,
            source_dependencies: Arc::new(RwLock::new(Vec::new())),
            virtual_files: Arc::new(RwLock::new(HashMap::new())),
        };
        (sess, cap)
    }

    /// Register an in-memory file under `path`.
    ///
    /// This is deliberately session-local rather than a global hook:
    /// fuzz targets and tests can synthesize small include trees while
    /// production driver paths keep using the host filesystem.
    pub fn add_virtual_file(&self, path: PathBuf, src: Arc<str>) {
        self.virtual_files.write().unwrap().insert(path, src);
    }

    /// Return `true` when `path` names a session-local virtual file.
    pub fn has_virtual_file(&self, path: &Path) -> bool {
        self.virtual_files.read().unwrap().contains_key(path)
    }

    /// Load a source file from the virtual layer first, then disk.
    ///
    /// The returned file is always registered in the [`SourceMap`], so
    /// downstream diagnostics can render spans exactly as they do for
    /// ordinary files.
    pub fn load_source_file(&self, path: &Path) -> std::io::Result<rcc_span::FileId> {
        if let Some(src) = self.virtual_files.read().unwrap().get(path).cloned() {
            return Ok(self.source_map.write().unwrap().add_file(path.to_path_buf(), src));
        }
        self.source_map.write().unwrap().load_file(path)
    }

    /// Record a source dependency resolved by the preprocessor.
    pub fn record_source_dependency(&self, path: PathBuf, system: bool) {
        self.source_dependencies.write().unwrap().push(SourceDependency { path, system });
    }

    /// Return source dependencies in encounter order.
    pub fn source_dependencies(&self) -> Vec<SourceDependency> {
        self.source_dependencies.read().unwrap().clone()
    }
}
