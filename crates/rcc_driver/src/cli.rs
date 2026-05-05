//! Clap-based command-line interface.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{error::ErrorKind, ArgAction, Parser};
use rcc_session::{EmitKind, OptLevel, TargetInfo, TargetTriple};

use crate::ExitCode;

/// The `rcc` command-line interface.
#[derive(Debug, Parser, Clone)]
#[command(name = "rcc", about = "rcc: a Rust-based C99 compiler", disable_version_flag = true)]
pub struct Cli {
    /// Input `.c` file(s).
    pub input: Vec<PathBuf>,

    /// Print rcc version information and exit.
    #[arg(long = "version", action = ArgAction::SetTrue)]
    pub show_version: bool,

    /// Print tool-search directories and selected external tools, then exit.
    #[arg(long = "print-search-dirs", action = ArgAction::SetTrue)]
    pub print_search_dirs: bool,

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

    /// System include path (`-isystem`). May repeat.
    #[arg(long = "isystem", value_name = "DIR", action = ArgAction::Append)]
    pub system_include_paths: Vec<PathBuf>,

    /// Prefix target-default system include paths with this root.
    #[arg(long = "sysroot", value_name = "DIR")]
    pub sysroot: Option<PathBuf>,

    /// Command-line `-D` macro definitions: `NAME` or `NAME=VAL`.
    #[arg(short = 'D', long = "define", value_parser = parse_define)]
    pub defines: Vec<(String, Option<String>)>,

    /// Command-line macro undefines (`-U NAME` or `-UNAME`). May repeat.
    #[arg(short = 'U', long = "undefine", value_name = "NAME", action = ArgAction::Append)]
    pub undefines: Vec<String>,

    /// Emit make dependencies to stdout and stop after preprocessing (`-M`).
    #[arg(short = 'M', action = ArgAction::SetTrue)]
    pub dep_only: bool,

    /// Emit user-header make dependencies to stdout and stop after preprocessing (`-MM`).
    #[arg(long = "user-dependencies", hide = true, action = ArgAction::SetTrue)]
    pub user_dep_only: bool,

    /// Emit make dependencies as a side effect of normal compilation (`-MD`).
    #[arg(long = "emit-dependencies", hide = true, action = ArgAction::SetTrue)]
    pub dep_side_effect: bool,

    /// Emit user-header make dependencies as a side effect of normal compilation (`-MMD`).
    #[arg(long = "emit-user-dependencies", hide = true, action = ArgAction::SetTrue)]
    pub user_dep_side_effect: bool,

    /// Dependency output file (`-MF`).
    #[arg(long = "dependency-file", value_name = "FILE")]
    pub dependency_file: Option<PathBuf>,

    /// Add an explicit dependency target (`-MT`).
    #[arg(long = "dependency-target", value_name = "TARGET", action = ArgAction::Append)]
    pub dependency_targets: Vec<String>,

    /// Add an explicit make-quoted dependency target (`-MQ`).
    #[arg(long = "quoted-dependency-target", value_name = "TARGET", action = ArgAction::Append)]
    pub quoted_dependency_targets: Vec<String>,

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

    /// Parallel compile jobs for multiple input files. Defaults to host parallelism.
    #[arg(short = 'j', long = "jobs", value_name = "N", value_parser = parse_jobs)]
    pub jobs: Option<usize>,

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

    /// Include optional external test suites during `fetch-testsuites` / conformance runs.
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
        let args = expand_response_args(args).map_err(|err| {
            clap::Error::raw(ErrorKind::ValueValidation, format!("response file error: {err}\n"))
        })?;
        <Self as Parser>::try_parse_from(normalize_driver_args(args))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResponseFileError {
    path: PathBuf,
    line: usize,
    column: usize,
    message: String,
}

impl ResponseFileError {
    fn new(path: PathBuf, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { path, line, column, message: message.into() }
    }
}

impl std::fmt::Display for ResponseFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}: {}", self.path.display(), self.line, self.column, self.message)
    }
}

fn expand_response_args<I, T>(args: I) -> Result<Vec<OsString>, ResponseFileError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut out = Vec::new();
    let mut args = args.into_iter();
    if let Some(program) = args.next() {
        out.push(program.into());
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut stack = Vec::new();
    for arg in args {
        expand_response_arg(arg.into(), &cwd, &mut stack, &mut out)?;
    }
    Ok(out)
}

fn expand_response_arg(
    arg: OsString,
    base_dir: &Path,
    stack: &mut Vec<PathBuf>,
    out: &mut Vec<OsString>,
) -> Result<(), ResponseFileError> {
    let Some(text) = arg.to_str() else {
        out.push(arg);
        return Ok(());
    };
    let Some(path) = text.strip_prefix('@').filter(|path| !path.is_empty()) else {
        out.push(arg);
        return Ok(());
    };
    expand_response_file(base_dir.join(path), stack, out)
}

fn expand_response_file(
    path: PathBuf,
    stack: &mut Vec<PathBuf>,
    out: &mut Vec<OsString>,
) -> Result<(), ResponseFileError> {
    let canonical = fs::canonicalize(&path)
        .map_err(|err| ResponseFileError::new(path.clone(), 1, 1, format!("cannot read: {err}")))?;
    if let Some(first) = stack.iter().position(|open| open == &canonical) {
        let cycle = stack[first..]
            .iter()
            .chain(std::iter::once(&canonical))
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(ResponseFileError::new(path, 1, 1, format!("response file cycle: {cycle}")));
    }
    let text = fs::read_to_string(&canonical).map_err(|err| {
        ResponseFileError::new(canonical.clone(), 1, 1, format!("cannot decode UTF-8: {err}"))
    })?;
    stack.push(canonical.clone());
    let base_dir = canonical.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let args = parse_response_file_args(&canonical, &text)?;
    for arg in args {
        expand_response_arg(OsString::from(arg), &base_dir, stack, out)?;
    }
    stack.pop();
    Ok(())
}

fn parse_response_file_args(path: &Path, text: &str) -> Result<Vec<String>, ResponseFileError> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut line = 1usize;
    let mut col = 1usize;
    let mut token_start: Option<(usize, usize)> = None;
    let mut quote: Option<(char, usize, usize)> = None;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let here = (line, col);
        advance_line_col(ch, &mut line, &mut col);

        if let Some((q, _, _)) = quote {
            if ch == q {
                quote = None;
                continue;
            }
            if ch == '\\' && q == '"' {
                let Some(next) = chars.peek().copied() else {
                    cur.push('\\');
                    continue;
                };
                if matches!(next, '"' | '\\' | '\'' | '#' | ' ' | '\t' | '\r' | '\n') {
                    chars.next();
                    advance_line_col(next, &mut line, &mut col);
                    cur.push(next);
                } else {
                    cur.push('\\');
                }
                continue;
            }
            cur.push(ch);
            continue;
        }

        match ch {
            '\'' | '"' => {
                token_start.get_or_insert(here);
                quote = Some((ch, here.0, here.1));
            }
            '#' if cur.is_empty() && token_start.is_none() => {
                while let Some(next) = chars.peek().copied() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                    advance_line_col(next, &mut line, &mut col);
                }
            }
            ch if ch.is_whitespace() => {
                if token_start.is_some() {
                    args.push(std::mem::take(&mut cur));
                    token_start = None;
                }
            }
            '\\' => {
                token_start.get_or_insert(here);
                let Some(next) = chars.peek().copied() else {
                    cur.push('\\');
                    continue;
                };
                if matches!(next, '"' | '\'' | '\\' | '#' | ' ' | '\t' | '\r' | '\n') {
                    chars.next();
                    advance_line_col(next, &mut line, &mut col);
                    cur.push(next);
                } else {
                    cur.push('\\');
                }
            }
            _ => {
                token_start.get_or_insert(here);
                cur.push(ch);
            }
        }
    }

    if let Some((q, q_line, q_col)) = quote {
        return Err(ResponseFileError::new(
            path.to_path_buf(),
            q_line,
            q_col,
            format!("unterminated {q} quote"),
        ));
    }
    if token_start.is_some() {
        args.push(cur);
    }
    Ok(args)
}

fn advance_line_col(ch: char, line: &mut usize, col: &mut usize) {
    if ch == '\n' {
        *line += 1;
        *col = 1;
    } else {
        *col += 1;
    }
}

fn parse_jobs(text: &str) -> Result<usize, String> {
    let jobs = text.parse::<usize>().map_err(|_| format!("invalid job count `{text}`"))?;
    if jobs == 0 {
        Err("job count must be at least 1".to_owned())
    } else {
        Ok(jobs)
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
            if let Some(level) = normalize_opt_level_arg(&text) {
                return OsString::from(format!("--opt-level={level}"));
            }
            if let Some(path) = text.strip_prefix("-MF").filter(|rest| !rest.is_empty()) {
                return OsString::from(format!("--dependency-file={path}"));
            }
            if let Some(path) = text.strip_prefix("-isystem").filter(|rest| !rest.is_empty()) {
                let path = path.strip_prefix('=').unwrap_or(path);
                return OsString::from(format!("--isystem={path}"));
            }
            if let Some(path) = text.strip_prefix("-isysroot").filter(|rest| !rest.is_empty()) {
                let path = path.strip_prefix('=').unwrap_or(path);
                return OsString::from(format!("--sysroot={path}"));
            }
            if let Some(target) = text.strip_prefix("-MT").filter(|rest| !rest.is_empty()) {
                return OsString::from(format!("--dependency-target={target}"));
            }
            if let Some(target) = text.strip_prefix("-MQ").filter(|rest| !rest.is_empty()) {
                return OsString::from(format!("--quoted-dependency-target={target}"));
            }
            match text.as_ref() {
                "-mms-bitfields" => OsString::from("-fms-bitfields"),
                "-std" => OsString::from("--std"),
                "-isystem" => OsString::from("--isystem"),
                "-isysroot" => OsString::from("--sysroot"),
                "-ansi" => OsString::from("--ansi"),
                "-MM" => OsString::from("--user-dependencies"),
                "-MD" => OsString::from("--emit-dependencies"),
                "-MMD" => OsString::from("--emit-user-dependencies"),
                "-MF" => OsString::from("--dependency-file"),
                "-MT" => OsString::from("--dependency-target"),
                "-MQ" => OsString::from("--quoted-dependency-target"),
                "-shared" => OsString::from("--shared"),
                "-static" => OsString::from("--static"),
                "-pie" => OsString::from("--pie"),
                "-no-pie" => OsString::from("--no-pie"),
                _ => arg,
            }
        })
        .collect()
}

fn normalize_opt_level_arg(arg: &str) -> Option<&'static str> {
    match arg {
        "-O" | "-O1" => Some("less"),
        "-O0" => Some("none"),
        "-O2" => Some("default"),
        "-O3" => Some("aggressive"),
        _ => None,
    }
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
