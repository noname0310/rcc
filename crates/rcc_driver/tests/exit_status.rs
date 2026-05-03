use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{run, Cli, ExitCode};
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::{Options, Session};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-exit-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn file(&self, name: &str, src: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, src).expect("write C source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|err| panic!("parse {args:?}: {err}"))
}

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

fn llvm_backend_enabled_for_this_build() -> bool {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session = Session::with_handler(Options::default(), handler);
    let tcx = rcc_hir::TyCtxt::new();
    let hir = rcc_hir::HirCrate::default();
    let bodies = rcc_data_structures::FxHashMap::default();
    !matches!(
        rcc_codegen_llvm::codegen(&mut session, &tcx, &hir, &bodies),
        Err(rcc_codegen_llvm::CodegenError::BackendDisabled)
    )
}

#[test]
fn exit_code_values_are_stable() {
    assert_eq!(ExitCode::Success.code(), 0);
    assert_eq!(ExitCode::CompilationFailure.code(), 1);
    assert_eq!(ExitCode::Usage.code(), 64);
    assert_eq!(ExitCode::InfrastructureFailure.code(), 70);
}

#[test]
fn successful_frontend_compile_returns_success() {
    let dir = TempDir::new("success");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let output = dir.path.join("hello.ast");
    let cli =
        parse(&["rcc", "--emit=ast", "-o", output.to_str().unwrap(), input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::Success.code());
    assert!(output.exists());
}

#[test]
fn parse_error_returns_compilation_failure() {
    let dir = TempDir::new("parse-error");
    let input = dir.file("bad.c", "int main( { return 0; }\n");
    let output = dir.path.join("bad.ast");
    let cli =
        parse(&["rcc", "--emit=ast", "-o", output.to_str().unwrap(), input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::CompilationFailure.code());
}

#[test]
fn type_error_returns_compilation_failure() {
    let dir = TempDir::new("type-error");
    let input = dir.file("bad.c", "int main(void) { int *p; p = 1; return 0; }\n");
    let cli = parse(&["rcc", "--emit=mir", input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::CompilationFailure.code());
}

#[test]
fn frontend_error_does_not_fall_through_to_link_failure() {
    let dir = TempDir::new("no-link-after-type-error");
    let input = dir.file("bad.c", "int main(void) { int *p; p = 1; return 0; }\n");
    let cli = parse(&["rcc", "-l__rcc_missing_exit_status__", input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::CompilationFailure.code());
}

#[test]
fn missing_input_file_returns_infrastructure_failure() {
    let dir = TempDir::new("missing-input");
    let input = dir.path.join("does-not-exist.c");
    let cli = parse(&["rcc", "--emit=ast", input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::InfrastructureFailure.code());
}

#[test]
fn backend_disabled_default_compile_returns_infrastructure_failure() {
    if llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("backend-disabled");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let cli = parse(&["rcc", input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::InfrastructureFailure.code());
}

#[test]
fn output_collision_returns_usage_failure() {
    let dir = TempDir::new("collision");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let cli = parse(&["rcc", "-E", "-o", input.to_str().unwrap(), input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::Usage.code());
}

#[test]
fn clap_cli_misuse_exits_with_usage_failure() {
    let output = Command::new(rcc_bin()).arg("--unknown").output().expect("run rcc");

    assert_eq!(output.status.code(), Some(ExitCode::Usage.code()));
}

#[test]
fn unsupported_standard_exits_with_usage_failure() {
    let output = Command::new(rcc_bin()).arg("-std=c11").arg("hello.c").output().expect("run rcc");

    assert_eq!(output.status.code(), Some(ExitCode::Usage.code()));
}

#[test]
fn failed_link_subprocess_returns_infrastructure_failure_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("failed-link");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let cli = parse(&["rcc", "-l__rcc_missing_exit_status__", input.to_str().unwrap()]);

    assert_eq!(run(cli), ExitCode::InfrastructureFailure.code());
}
