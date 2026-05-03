use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, run, Cli, ExitCode};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcc-driver-misc-cli-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{name}.c"));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
    }

    fn sibling(&self, extension: &str) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(extension);
        path
    }
}

impl Drop for TempCFile {
    fn drop(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|err| panic!("parse {args:?}: {err}"))
}

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

#[test]
fn std_c99_is_accepted_and_preserves_default_options() {
    let cli = parse(&["rcc", "-std=c99", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert_eq!(cli.standard.as_deref(), Some("c99"));
    assert_eq!(opts.emit, Vec::new());
}

#[test]
fn unsupported_std_is_rejected_during_cli_parse() {
    let err = Cli::try_parse_from(["rcc", "-std=c11", "hello.c"]).unwrap_err().to_string();

    assert!(err.contains("unsupported standard 'c11'"), "{err}");
}

#[test]
fn ansi_alias_is_parsed_but_rejected_before_compilation() {
    let cli = parse(&["rcc", "-ansi", "does-not-need-to-exist.c"]);

    assert!(cli.ansi);
    assert_eq!(run(cli), ExitCode::Usage.code());
}

#[test]
fn known_f_flags_parse_and_do_not_change_options() {
    let cli = parse(&[
        "rcc",
        "-fPIC",
        "-fno-strict-aliasing",
        "-fwrapv",
        "-fstack-protector",
        "-fno-common",
        "-fvisibility=hidden",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert_eq!(
        cli.feature_flags,
        ["PIC", "no-strict-aliasing", "wrapv", "stack-protector", "no-common", "visibility=hidden"]
    );
    assert_eq!(opts.emit, Vec::new());
}

#[test]
fn fpic_frontend_compile_succeeds_and_reports_ignored_note() {
    let input = TempCFile::new("fpic", "int main(void) { return 0; }\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("-fPIC")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("note: ignoring compatibility flag -fPIC"), "{stderr}");
}

#[test]
fn unknown_f_flag_frontend_compile_succeeds_with_warning() {
    let input = TempCFile::new("unknown-f", "int main(void) { return 0; }\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("-fexperimental-thing")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("warning: ignoring unknown compatibility flag -fexperimental-thing"),
        "{stderr}"
    );
}
