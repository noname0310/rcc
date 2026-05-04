//! `rcc_driver`: CLI parsing + pipeline orchestration for the `rcc` compiler.
//!
//! Analogous to `rustc_driver`. The public API is thin because the real work
//! lives in subordinate crates; the driver's job is wiring + error propagation.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_session::{EmitKind, LinkOptions, Options, Session, TargetInfo, WarningConfig};

pub mod cli;
mod deps;
pub mod pipeline;
pub mod toolchain;

pub use cli::Cli;

static NEXT_MULTI_OBJECT_ID: AtomicUsize = AtomicUsize::new(0);

/// Stable process exit codes returned by the driver.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Successful compilation.
    Success = 0,
    /// Source-level compilation diagnostics were emitted.
    CompilationFailure = 1,
    /// Command-line usage failed before compilation could start.
    Usage = 64,
    /// I/O, backend, linker, or subprocess infrastructure failed.
    InfrastructureFailure = 70,
}

impl ExitCode {
    /// Numeric process status code.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// Result classification returned by driver orchestration before process exit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DriverStatus {
    /// Stable exit code category.
    pub exit_code: ExitCode,
}

impl DriverStatus {
    /// Successful status.
    pub const SUCCESS: Self = Self { exit_code: ExitCode::Success };

    /// Numeric process status code.
    #[must_use]
    pub const fn code(self) -> i32 {
        self.exit_code.code()
    }

    fn from_compile_result(result: Result<(), String>, has_errors: bool) -> Self {
        match result {
            Ok(()) if has_errors => Self { exit_code: ExitCode::CompilationFailure },
            Ok(()) => Self::SUCCESS,
            Err(msg) => Self { exit_code: classify_driver_error(&msg) },
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self.exit_code, other.exit_code) {
            (ExitCode::InfrastructureFailure, _) | (_, ExitCode::InfrastructureFailure) => {
                Self { exit_code: ExitCode::InfrastructureFailure }
            }
            (ExitCode::Usage, _) | (_, ExitCode::Usage) => Self { exit_code: ExitCode::Usage },
            (ExitCode::CompilationFailure, _) | (_, ExitCode::CompilationFailure) => {
                Self { exit_code: ExitCode::CompilationFailure }
            }
            _ => Self::SUCCESS,
        }
    }
}

/// Run the driver with a pre-parsed CLI. Returns a UNIX-style exit code.
pub fn run(cli: Cli) -> i32 {
    run_status(cli).code()
}

/// Run the driver with a pre-parsed CLI and return the classified status.
pub fn run_status(cli: Cli) -> DriverStatus {
    if let Err(msg) = validate_driver_cli(&cli) {
        eprintln!("rcc: {msg}");
        return DriverStatus { exit_code: classify_driver_error(&msg) };
    }
    emit_ignored_feature_flag_notes(&cli);
    let opts = options_from_cli(&cli);
    if cli.verbose {
        emit_verbose_trace(&cli, &opts);
    }
    if cli.input.len() > 1 {
        return run_many(&cli, &opts);
    }
    let mut session = Session::new(opts);
    let result = pipeline::compile(&mut session, first_input(&cli));
    if let Err(msg) = &result {
        eprintln!("rcc: {msg}");
    }
    DriverStatus::from_compile_result(result, session.handler.has_errors())
}

fn classify_driver_error(message: &str) -> ExitCode {
    if message.starts_with("unsupported standard")
        || message.starts_with("cannot specify -o")
        || message.starts_with("refusing to overwrite input file")
    {
        ExitCode::Usage
    } else {
        ExitCode::InfrastructureFailure
    }
}

fn validate_driver_cli(cli: &Cli) -> Result<(), String> {
    if cli.ansi {
        return Err("unsupported standard '-ansi'; only -std=c99 is supported".to_owned());
    }
    Ok(())
}

fn emit_ignored_feature_flag_notes(cli: &Cli) {
    for flag in &cli.feature_flags {
        if is_supported_feature_flag(flag) {
            continue;
        }
        let spelling = format!("-f{flag}");
        if is_known_ignored_feature_flag(flag) {
            eprintln!("rcc: note: ignoring compatibility flag {spelling}");
        } else {
            eprintln!("rcc: warning: ignoring unknown compatibility flag {spelling}");
        }
    }
}

fn is_supported_feature_flag(flag: &str) -> bool {
    matches!(
        flag,
        "gnu-binary-literals"
            | "gnu-binary-integer-literals"
            | "gnu-va-args-elision"
            | "gnu-comma-va-args"
            | "gnu-permissive-redefinition"
            | "gnu-permissive-macro-redefinition"
            | "gnu-named-variadic"
            | "gnu-named-variadic-macros"
            | "gnu-permissive-paste"
            | "gnu-permissive-token-paste"
            | "gnu-statement-expressions"
            | "gnu-omitted-conditional-operand"
            | "gnu-omitted-conditional"
            | "gnu-conditional-void-operand"
            | "gnu-conditional-void"
            | "gnu-range-designators"
            | "gnu-ranges"
            | "gnu-attributes"
            | "gnu-attribute"
            | "gnu-inline-asm"
            | "gnu-asm"
            | "gnu-case-ranges"
            | "gnu-case-range"
            | "gnu-labels-as-values"
            | "gnu-computed-goto"
            | "gnu-lvalue-comma"
            | "gnu-function-names"
            | "gnu-function-name"
            | "gnu-function"
            | "instrument-functions"
            | "gnu-va-area"
            | "chibicc-va-area"
            | "gnu89-inline"
            | "gnu-inline"
            | "chibicc-inline"
            | "gnu-builtin-libcalls"
            | "gnu-libc-builtins"
            | "gnu-common-builtins"
    )
}

fn emit_verbose_trace(cli: &Cli, opts: &Options) {
    eprintln!("rcc version {}", env!("CARGO_PKG_VERSION"));
    eprintln!("target: {}", opts.target.triple);
    if opts.include_paths.is_empty() {
        eprintln!("include paths: <none>");
    } else {
        eprintln!("include paths:");
        for path in &opts.include_paths {
            eprintln!("  {}", path.display());
        }
    }
    eprintln!("selected tools:");
    let finder = toolchain::ToolFinder::from_env();
    match &opts.link.linker_driver {
        Some(path) => eprintln!("  linker driver: {} (from command line/options)", path.display()),
        None => match finder.find_linker_driver() {
            Ok(path) => eprintln!("  linker driver: {}", path.display()),
            Err(err) => eprintln!("  linker driver: <not found: {err}>"),
        },
    }
    match finder.find_lld() {
        Ok(path) => eprintln!("  lld: {}", path.display()),
        Err(err) => eprintln!("  lld: <not found: {err}>"),
    }
    if let Some(prefix) = finder.find_llvm_prefix() {
        eprintln!("  llvm prefix: {}", prefix.display());
    } else {
        eprintln!("  llvm prefix: <none>");
    }
    if !cli.libraries.is_empty()
        || !cli.library_paths.is_empty()
        || !opts.link.linker_args.is_empty()
    {
        eprintln!("link inputs:");
        for path in &cli.library_paths {
            eprintln!("  -L{}", path.display());
        }
        for lib in &cli.libraries {
            eprintln!("  -l{lib}");
        }
        for flag in &opts.link.linker_args {
            eprintln!("  {flag}");
        }
    }
}

fn is_known_ignored_feature_flag(flag: &str) -> bool {
    matches!(
        flag,
        "PIC"
            | "pic"
            | "no-strict-aliasing"
            | "wrapv"
            | "stack-protector"
            | "no-common"
            | "visibility=hidden"
    )
}

fn run_many(cli: &Cli, base_opts: &Options) -> DriverStatus {
    if (cli.compile_only || cli.emit_assembly) && cli.output.is_some() {
        eprintln!("rcc: cannot specify -o with multiple input files when using -c or -S");
        return DriverStatus { exit_code: ExitCode::Usage };
    }
    if matches!(base_opts.dependencies.mode, Some(rcc_session::DependencyMode::PreprocessOnly))
        || cli.preprocess_only
        || !cli.emit.is_empty()
        || cli.emit_assembly
    {
        return compile_many_without_link(cli, base_opts);
    }
    if cli.compile_only {
        return compile_many_to_default_objects(cli, base_opts);
    }
    compile_many_and_link(cli, base_opts)
}

fn compile_many_without_link(cli: &Cli, base_opts: &Options) -> DriverStatus {
    let mut status = DriverStatus::SUCCESS;
    for input in &cli.input {
        let mut opts = base_opts.clone();
        opts.output = None;
        let mut session = Session::new(opts);
        let result = pipeline::compile(&mut session, input);
        if let Err(msg) = &result {
            eprintln!("rcc: {}: {msg}", input.display());
        }
        status =
            status.merge(DriverStatus::from_compile_result(result, session.handler.has_errors()));
    }
    status
}

fn compile_many_to_default_objects(cli: &Cli, base_opts: &Options) -> DriverStatus {
    let mut status = DriverStatus::SUCCESS;
    for input in &cli.input {
        let output = default_output_path(input, "o");
        status = status.merge(compile_one_to_object(input, &output, base_opts));
    }
    status
}

fn compile_many_and_link(cli: &Cli, base_opts: &Options) -> DriverStatus {
    let mut temps = TempObjects::default();
    let mut status = DriverStatus::SUCCESS;
    for input in &cli.input {
        let output = match temps.alloc(input) {
            Ok(output) => output,
            Err(msg) => {
                eprintln!("rcc: {msg}");
                status = status.merge(DriverStatus { exit_code: ExitCode::InfrastructureFailure });
                continue;
            }
        };
        let compile_status = compile_one_to_object(input, &output, base_opts);
        if compile_status.exit_code == ExitCode::Success {
            temps.keep(output);
        } else {
            let _ = fs::remove_file(&output);
            status = status.merge(compile_status);
        }
    }
    if status.exit_code != ExitCode::Success {
        return status;
    }
    let output = cli.output.clone().unwrap_or_else(|| PathBuf::from("a.out"));
    if cli.input.iter().any(|input| same_file_or_same_path(&output, input)) {
        eprintln!("rcc: refusing to overwrite input file {}", output.display());
        return DriverStatus { exit_code: ExitCode::Usage };
    }
    match pipeline::link_objects_with_options(temps.paths(), &output, &base_opts.link) {
        Ok(()) => DriverStatus::SUCCESS,
        Err(msg) => {
            eprintln!("rcc: {msg}");
            DriverStatus { exit_code: ExitCode::InfrastructureFailure }
        }
    }
}

fn compile_one_to_object(input: &Path, output: &Path, base_opts: &Options) -> DriverStatus {
    let mut opts = base_opts.clone();
    opts.emit = vec![EmitKind::Obj];
    opts.output = Some(output.to_path_buf());
    let mut session = Session::new(opts);
    let result = pipeline::compile(&mut session, input);
    if let Err(msg) = &result {
        eprintln!("rcc: {}: {msg}", input.display());
    }
    DriverStatus::from_compile_result(result, session.handler.has_errors())
}

/// Convert parsed CLI flags into a `rcc_session::Options`.
pub fn options_from_cli(cli: &Cli) -> Options {
    let (emit, output) = emit_and_output_from_cli(cli);
    Options {
        include_paths: cli.include_paths.clone(),
        cli_defines: cli.defines.clone(),
        target: cli.target.clone().unwrap_or_else(TargetInfo::host),
        emit,
        output,
        save_temps: cli.save_temps.clone(),
        dependencies: dependency_options_from_cli(cli),
        opt_level: cli.opt_level,
        warning_config: warning_config_from_cli(cli),
        link: link_options_from_cli(cli),
        debug_info: cli.debug_info,
        include_gpl_tests: cli.include_gpl_tests,
        gnu_va_args_elision: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-va-args-elision" | "gnu-comma-va-args")),
        gnu_permissive_redefinition: cli.feature_flags.iter().any(|flag| {
            matches!(
                flag.as_str(),
                "gnu-permissive-redefinition" | "gnu-permissive-macro-redefinition"
            )
        }),
        gnu_named_variadic: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-named-variadic" | "gnu-named-variadic-macros")
        }),
        gnu_permissive_paste: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-permissive-paste" | "gnu-permissive-token-paste")
        }),
        gnu_binary_integer_literals: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-binary-literals" | "gnu-binary-integer-literals")
        }),
        gnu_statement_expressions: cli
            .feature_flags
            .iter()
            .any(|flag| flag == "gnu-statement-expressions"),
        gnu_omitted_conditional_operand: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-omitted-conditional-operand" | "gnu-omitted-conditional")
        }),
        gnu_conditional_void_operand: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-conditional-void-operand" | "gnu-conditional-void")
        }),
        gnu_range_designators: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-range-designators" | "gnu-ranges")),
        gnu_attributes: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-attributes" | "gnu-attribute")),
        gnu_inline_asm: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-inline-asm" | "gnu-asm")),
        gnu_case_ranges: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-case-ranges" | "gnu-case-range")),
        gnu_labels_as_values: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-labels-as-values" | "gnu-computed-goto")),
        gnu_lvalue_comma: cli.feature_flags.iter().any(|flag| flag == "gnu-lvalue-comma"),
        gnu_function_names: cli.feature_flags.iter().any(|flag| {
            matches!(flag.as_str(), "gnu-function-names" | "gnu-function-name" | "gnu-function")
        }),
        instrument_functions: cli.feature_flags.iter().any(|flag| flag == "instrument-functions"),
        gnu_va_area: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu-va-area" | "chibicc-va-area")),
        gnu89_inline: cli
            .feature_flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "gnu89-inline" | "gnu-inline" | "chibicc-inline")),
        gnu_builtin_libcalls: cli.feature_flags.iter().any(|flag| {
            matches!(
                flag.as_str(),
                "gnu-builtin-libcalls" | "gnu-libc-builtins" | "gnu-common-builtins"
            )
        }),
    }
}

fn dependency_options_from_cli(cli: &Cli) -> rcc_session::DependencyOptions {
    use rcc_session::{DependencyMode, DependencyOptions, DependencyTarget};

    let mode = if cli.dep_only || cli.user_dep_only {
        Some(DependencyMode::PreprocessOnly)
    } else if cli.dep_side_effect || cli.user_dep_side_effect {
        Some(DependencyMode::SideEffect)
    } else {
        None
    };
    DependencyOptions {
        mode,
        include_system_headers: !(cli.user_dep_only || cli.user_dep_side_effect),
        output: cli.dependency_file.clone(),
        targets: cli
            .dependency_targets
            .iter()
            .map(|text| DependencyTarget { text: text.clone(), quote: false })
            .chain(
                cli.quoted_dependency_targets
                    .iter()
                    .map(|text| DependencyTarget { text: text.clone(), quote: true }),
            )
            .collect(),
    }
}

fn warning_config_from_cli(cli: &Cli) -> WarningConfig {
    let mut config = WarningConfig::default();
    if cli.suppress_warnings {
        config.suppress_all();
    }
    for flag in &cli.warning_flags {
        apply_warning_flag(&mut config, flag);
    }
    config
}

fn apply_warning_flag(config: &mut WarningConfig, raw: &str) {
    if raw.starts_with("l,") {
        return;
    }
    match raw {
        "all" => config.enable_wall(),
        "extra" => config.enable_extra(),
        "pedantic" => config.enable_pedantic(),
        "error" => config.set_warnings_as_errors(true),
        "no-error" => config.set_warnings_as_errors(false),
        name if name.starts_with("error=") => {
            config.promote_warning(name.trim_start_matches("error="));
        }
        name if name.starts_with("no-error=") => {
            config.demote_warning(name.trim_start_matches("no-error="));
        }
        name if name.starts_with("no-") => {
            config.disable_warning(name.trim_start_matches("no-"));
        }
        name => config.enable_warning(name),
    }
}

fn link_options_from_cli(cli: &Cli) -> LinkOptions {
    let mut link = LinkOptions {
        linker_driver: std::env::var_os("RCC_LINKER_DRIVER")
            .or_else(|| std::env::var_os("RCC_CLANG"))
            .or_else(|| std::env::var_os("CLANG"))
            .map(PathBuf::from),
        libraries: cli.libraries.clone(),
        library_paths: cli.library_paths.clone(),
        shared: cli.shared,
        static_link: cli.static_link,
        pie: cli.pie.then_some(true).or_else(|| cli.no_pie.then_some(false)),
        verbose: cli.verbose,
        ..LinkOptions::default()
    };
    link.linker_args.extend(
        cli.warning_flags
            .iter()
            .filter_map(|flag| flag.strip_prefix("l,").map(|rest| format!("-Wl,{rest}"))),
    );
    link
}

fn emit_and_output_from_cli(cli: &Cli) -> (Vec<EmitKind>, Option<PathBuf>) {
    if cli.compile_only {
        if cli.input.len() > 1 {
            return (vec![EmitKind::Obj], None);
        }
        return (
            vec![EmitKind::Obj],
            Some(cli.output.clone().unwrap_or_else(|| default_output_path(first_input(cli), "o"))),
        );
    }
    if cli.emit_assembly {
        if cli.input.len() > 1 {
            return (vec![EmitKind::Asm], None);
        }
        return (
            vec![EmitKind::Asm],
            Some(cli.output.clone().unwrap_or_else(|| default_output_path(first_input(cli), "s"))),
        );
    }
    if cli.preprocess_only {
        return (vec![EmitKind::Pp], cli.output.clone());
    }
    (cli.emit.clone(), cli.output.clone())
}

fn first_input(cli: &Cli) -> &Path {
    cli.input.first().expect("clap enforces at least one input").as_path()
}

fn default_output_path(input: &Path, extension: &str) -> PathBuf {
    let mut output = input.to_path_buf();
    output.set_extension(extension);
    output
}

#[derive(Default)]
struct TempObjects {
    dir: Option<PathBuf>,
    paths: Vec<PathBuf>,
}

impl TempObjects {
    fn alloc(&mut self, input: &Path) -> Result<PathBuf, String> {
        let id = NEXT_MULTI_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = match &self.dir {
            Some(dir) => dir.clone(),
            None => {
                let dir =
                    std::env::temp_dir().join(format!("rcc-multi-{}-{id}.tmp", std::process::id()));
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                self.dir = Some(dir.clone());
                dir
            }
        };
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("input");
        let mut path = dir.join(format!("{id}-{stem}"));
        path.set_extension("o");
        let _ = fs::remove_file(&path);
        Ok(path)
    }

    fn keep(&mut self, path: PathBuf) {
        self.paths.push(path);
    }

    fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

impl Drop for TempObjects {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn same_file_or_same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => absolutize(a) == absolutize(b),
    }
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    }
}

/// Helper used in tests: turn a `&str` into a temporary file path.
pub type InputPath = PathBuf;

/// Re-exports for tests / external users.
pub mod reexports {
    pub use rcc_session::EmitKind;
    pub use rcc_session::OptLevel;
}
