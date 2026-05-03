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
pub mod pipeline;

pub use cli::Cli;

static NEXT_MULTI_OBJECT_ID: AtomicUsize = AtomicUsize::new(0);

/// Run the driver with a pre-parsed CLI. Returns a UNIX-style exit code.
pub fn run(cli: Cli) -> i32 {
    let opts = options_from_cli(&cli);
    if cli.input.len() > 1 {
        return run_many(&cli, &opts);
    }
    let mut session = Session::new(opts);
    match pipeline::compile(&mut session, first_input(&cli)) {
        Ok(()) => {
            if session.handler.has_errors() {
                1
            } else {
                0
            }
        }
        Err(msg) => {
            eprintln!("rcc: {msg}");
            1
        }
    }
}

fn run_many(cli: &Cli, base_opts: &Options) -> i32 {
    if (cli.compile_only || cli.emit_assembly) && cli.output.is_some() {
        eprintln!("rcc: cannot specify -o with multiple input files when using -c or -S");
        return 1;
    }
    if cli.preprocess_only || !cli.emit.is_empty() || cli.emit_assembly {
        return compile_many_without_link(cli, base_opts);
    }
    if cli.compile_only {
        return compile_many_to_default_objects(cli, base_opts);
    }
    compile_many_and_link(cli, base_opts)
}

fn compile_many_without_link(cli: &Cli, base_opts: &Options) -> i32 {
    let mut failed = false;
    for input in &cli.input {
        let mut opts = base_opts.clone();
        opts.output = None;
        let mut session = Session::new(opts);
        match pipeline::compile(&mut session, input) {
            Ok(()) if !session.handler.has_errors() => {}
            Ok(()) => failed = true,
            Err(msg) => {
                eprintln!("rcc: {}: {msg}", input.display());
                failed = true;
            }
        }
    }
    i32::from(failed)
}

fn compile_many_to_default_objects(cli: &Cli, base_opts: &Options) -> i32 {
    let mut failed = false;
    for input in &cli.input {
        let output = default_output_path(input, "o");
        if !compile_one_to_object(input, &output, base_opts) {
            failed = true;
        }
    }
    i32::from(failed)
}

fn compile_many_and_link(cli: &Cli, base_opts: &Options) -> i32 {
    let mut temps = TempObjects::default();
    let mut failed = false;
    for input in &cli.input {
        let output = temps.alloc(input);
        if compile_one_to_object(input, &output, base_opts) {
            temps.keep(output);
        } else {
            let _ = fs::remove_file(&output);
            failed = true;
        }
    }
    if failed {
        return 1;
    }
    let output = cli.output.clone().unwrap_or_else(|| PathBuf::from("a.out"));
    match pipeline::link_objects_with_options(temps.paths(), &output, &base_opts.link) {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("rcc: {msg}");
            1
        }
    }
}

fn compile_one_to_object(input: &Path, output: &Path, base_opts: &Options) -> bool {
    let mut opts = base_opts.clone();
    opts.emit = vec![EmitKind::Obj];
    opts.output = Some(output.to_path_buf());
    let mut session = Session::new(opts);
    match pipeline::compile(&mut session, input) {
        Ok(()) => !session.handler.has_errors(),
        Err(msg) => {
            eprintln!("rcc: {}: {msg}", input.display());
            false
        }
    }
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
        opt_level: cli.opt_level,
        warning_config: warning_config_from_cli(cli),
        link: link_options_from_cli(cli),
        debug_info: false,
        include_gpl_tests: cli.include_gpl_tests,
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
        libraries: cli.libraries.clone(),
        library_paths: cli.library_paths.clone(),
        shared: cli.shared,
        static_link: cli.static_link,
        pie: cli.pie.then_some(true).or_else(|| cli.no_pie.then_some(false)),
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
    paths: Vec<PathBuf>,
}

impl TempObjects {
    fn alloc(&self, input: &Path) -> PathBuf {
        let id = NEXT_MULTI_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("input");
        let mut path =
            std::env::temp_dir().join(format!("rcc-multi-{}-{id}-{stem}", std::process::id()));
        path.set_extension("o");
        let _ = fs::remove_file(&path);
        path
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
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

/// Helper used in tests: turn a `&str` into a temporary file path.
pub type InputPath = PathBuf;

/// Re-exports for tests / external users.
pub mod reexports {
    pub use rcc_session::EmitKind;
    pub use rcc_session::OptLevel;
}
