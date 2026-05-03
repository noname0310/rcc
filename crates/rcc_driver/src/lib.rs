//! `rcc_driver`: CLI parsing + pipeline orchestration for the `rcc` compiler.
//!
//! Analogous to `rustc_driver`. The public API is thin because the real work
//! lives in subordinate crates; the driver's job is wiring + error propagation.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use rcc_session::{EmitKind, Options, Session, TargetInfo, WarningConfig};

pub mod cli;
pub mod pipeline;

pub use cli::Cli;

/// Run the driver with a pre-parsed CLI. Returns a UNIX-style exit code.
pub fn run(cli: Cli) -> i32 {
    let opts = options_from_cli(&cli);
    let mut session = Session::new(opts);
    match pipeline::compile(&mut session, &cli.input) {
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

fn emit_and_output_from_cli(cli: &Cli) -> (Vec<EmitKind>, Option<PathBuf>) {
    if cli.compile_only {
        return (
            vec![EmitKind::Obj],
            Some(cli.output.clone().unwrap_or_else(|| default_output_path(&cli.input, "o"))),
        );
    }
    if cli.emit_assembly {
        return (
            vec![EmitKind::Asm],
            Some(cli.output.clone().unwrap_or_else(|| default_output_path(&cli.input, "s"))),
        );
    }
    if cli.preprocess_only {
        return (vec![EmitKind::Pp], cli.output.clone());
    }
    (cli.emit.clone(), cli.output.clone())
}

fn default_output_path(input: &Path, extension: &str) -> PathBuf {
    let mut output = input.to_path_buf();
    output.set_extension(extension);
    output
}

/// Helper used in tests: turn a `&str` into a temporary file path.
pub type InputPath = PathBuf;

/// Re-exports for tests / external users.
pub mod reexports {
    pub use rcc_session::EmitKind;
    pub use rcc_session::OptLevel;
}
