use std::fs;
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, run, Cli};
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
            .join(format!("rcc-driver-multi-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn file(&self, name: &str, src: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, src).expect("write C file");
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

#[cfg(not(windows))]
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
fn cli_accepts_multiple_input_files() {
    let cli = parse(&["rcc", "main.c", "util.c"]);
    assert_eq!(cli.input, [PathBuf::from("main.c"), PathBuf::from("util.c")]);

    let compile_only = options_from_cli(&parse(&["rcc", "-c", "main.c", "util.c"]));
    assert_eq!(compile_only.output, None);

    let parallel = parse(&["rcc", "-j", "2", "main.c", "util.c"]);
    assert_eq!(parallel.jobs, Some(2));
    assert!(Cli::try_parse_from(["rcc", "-j", "0", "main.c"]).is_err());
}

#[test]
fn compile_only_multiple_inputs_write_one_object_per_file_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("compile-only");
    let main = dir.file("main.c", "int main(void) { return 0; }\n");
    let util = dir.file("util.c", "int util(void) { return 1; }\n");

    let cli = parse(&["rcc", "-j", "2", "-c", main.to_str().unwrap(), util.to_str().unwrap()]);
    let code = run(cli);

    assert_eq!(code, 0);
    assert!(main.with_extension("o").exists());
    assert!(util.with_extension("o").exists());
}

#[test]
fn compile_only_continues_after_one_file_errors_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("partial-error");
    let good = dir.file("good.c", "int good(void) { return 0; }\n");
    let bad = dir.file("bad.c", "int bad(void) { return ; }\n");

    let cli = parse(&["rcc", "-j2", "-c", bad.to_str().unwrap(), good.to_str().unwrap()]);
    let code = run(cli);

    assert_eq!(code, 1);
    assert!(!bad.with_extension("o").exists());
    assert!(good.with_extension("o").exists());
}

#[cfg(not(windows))]
#[test]
fn e2e_multi_file_link_when_enabled() {
    if std::env::var_os("RCC_RUN_LINK_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert!(llvm_backend_enabled_for_this_build(), "LLVM backend feature is required");

    let dir = TempDir::new("link");
    let main = dir.file("main.c", "int util(void); int main(void) { return util(); }\n");
    let util = dir.file("util.c", "int util(void) { return 7; }\n");
    let output = dir.path.join("prog");

    let cli = parse(&[
        "rcc",
        "-j",
        "2",
        "-o",
        output.to_str().unwrap(),
        main.to_str().unwrap(),
        util.to_str().unwrap(),
    ]);
    let code = run(cli);

    assert_eq!(code, 0);
    let status = Command::new(&output).status().expect("run linked program");
    assert_eq!(status.code(), Some(7));
}

#[cfg(not(windows))]
#[test]
fn compile_only_parallel_success_is_quiet_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("quiet-parallel");
    let main = dir.file("main.c", "int main(void) { return 0; }\n");
    let util = dir.file("util.c", "int util(void) { return 1; }\n");

    let output = Command::new(rcc_bin())
        .arg("-j")
        .arg("2")
        .arg("-c")
        .arg(&main)
        .arg(&util)
        .output()
        .expect("run rcc -j2 -c");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    assert!(output.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
}
