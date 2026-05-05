use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, pipeline, Cli};
#[cfg(not(windows))]
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::LinkOptions;
#[cfg(not(windows))]
use rcc_session::{Options, Session};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str, bytes: &[u8]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-linker-flags-{}-{id}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        fs::write(&path, bytes).expect("write temp file");
        Self { path }
    }

    fn empty_path(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-linker-flags-{}-{id}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(not(windows))]
struct TempCFile {
    path: PathBuf,
}

#[cfg(not(windows))]
impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-linker-flags-{}-{id}-{name}.c", std::process::id()));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
    }
}

#[cfg(not(windows))]
impl Drop for TempCFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|err| panic!("parse {args:?}: {err}"))
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
fn compile_with_link_options(input: &Path, output: &Path, link: LinkOptions) -> Result<(), String> {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session = Session::with_handler(
        Options { output: Some(output.to_path_buf()), link, ..Options::default() },
        handler,
    );
    pipeline::compile(&mut session, input)
}

#[test]
fn cli_collects_common_linker_flags() {
    let cli = parse(&[
        "rcc",
        "-lm",
        "-L/native/lib",
        "-Wl,--version-script=map.txt",
        "-shared",
        "-static",
        "-no-pie",
        "-pthread",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert_eq!(opts.link.libraries, ["m"]);
    assert_eq!(opts.link.library_paths, [PathBuf::from("/native/lib")]);
    assert_eq!(opts.link.linker_args, ["-Wl,--version-script=map.txt"]);
    assert!(opts.link.shared);
    assert!(opts.link.static_link);
    assert!(opts.link.pthread);
    assert_eq!(opts.link.pie, Some(false));
    assert!(!opts.warning_config.warning_disabled("l,--version-script=map.txt"));
}

#[test]
fn pie_and_no_pie_are_mutually_exclusive() {
    let err = Cli::try_parse_from(["rcc", "-pie", "-no-pie", "hello.c"]).unwrap_err().to_string();
    assert!(err.contains("cannot be used with"), "{err}");
}

#[test]
fn link_command_forwards_options_to_clang_lld_driver() {
    let options = LinkOptions {
        libraries: vec!["m".to_owned(), "pthread".to_owned()],
        library_paths: vec![PathBuf::from("/native/lib")],
        linker_args: vec!["-Wl,--version-script=map.txt".to_owned()],
        shared: true,
        static_link: true,
        pie: Some(true),
        ..LinkOptions::default()
    };

    let command = pipeline::LinkCommand::with_options(
        PathBuf::from("clang"),
        Path::new("input.o"),
        Path::new("out"),
        &options,
    );
    let rendered = command.render();

    assert!(rendered.starts_with("clang -fuse-ld=lld"), "{rendered}");
    assert!(rendered.contains("input.o"), "{rendered}");
    assert!(rendered.contains("-o out"), "{rendered}");
    assert!(rendered.contains("-shared"), "{rendered}");
    assert!(rendered.contains("-static"), "{rendered}");
    assert!(rendered.contains("-pie"), "{rendered}");
    assert!(rendered.contains("-L/native/lib"), "{rendered}");
    assert!(rendered.contains("-lm"), "{rendered}");
    assert!(rendered.contains("-lpthread"), "{rendered}");
    assert!(rendered.contains("-Wl,--version-script=map.txt"), "{rendered}");
}

#[test]
fn link_command_forwards_pthread_driver_flag_once() {
    let options = LinkOptions { pthread: true, ..LinkOptions::default() };
    let command = pipeline::LinkCommand::with_options(
        PathBuf::from("clang"),
        Path::new("input.o"),
        Path::new("out"),
        &options,
    );
    let rendered = command.render();

    assert!(rendered.contains("-pthread"), "{rendered}");
    assert_eq!(rendered.matches("-pthread").count(), 1, "{rendered}");
}

#[test]
fn missing_linker_error_includes_forwarded_flags() {
    let linker = TempFile::empty_path("missing-cc");
    let obj = TempFile::new("input.o", b"not an object");
    let output = TempFile::empty_path("a.out");
    let options = LinkOptions {
        libraries: vec!["m".to_owned()],
        linker_args: vec!["-Wl,--version-script=map.txt".to_owned()],
        ..LinkOptions::default()
    };

    let err =
        pipeline::link_with_linker_and_options(&linker.path, &obj.path, &output.path, &options)
            .unwrap_err();

    assert!(err.contains("-lm"), "{err}");
    assert!(err.contains("-Wl,--version-script=map.txt"), "{err}");
}

#[cfg(not(windows))]
#[test]
fn e2e_link_with_pthread_when_enabled() {
    if std::env::var_os("RCC_RUN_LINK_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert!(llvm_backend_enabled_for_this_build(), "LLVM backend feature is required");

    let input = TempCFile::new(
        "pthread",
        r#"
#include <pthread.h>

static int value;
static void *worker(void *arg) {
    value = arg ? 7 : 3;
    return 0;
}

int main(void) {
    pthread_t thread;
    if (pthread_create(&thread, 0, worker, &value) != 0)
        return 10;
    if (pthread_join(thread, 0) != 0)
        return 11;
    return value == 7 ? 0 : 12;
}
"#,
    );
    let output = TempFile::empty_path("pthread-out");
    let link = LinkOptions { pthread: true, ..LinkOptions::default() };

    compile_with_link_options(&input.path, &output.path, link)
        .expect("compile and link with -pthread");

    let status = Command::new(&output.path).status().expect("run linked executable");
    assert_eq!(status.code(), Some(0));
}

#[cfg(not(windows))]
#[test]
fn e2e_link_with_libm_when_enabled() {
    if std::env::var_os("RCC_RUN_LINK_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert!(llvm_backend_enabled_for_this_build(), "LLVM backend feature is required");

    let input = TempCFile::new("libm", "int main(void) { return 0; }\n");
    let output = TempFile::empty_path("libm-out");
    let link = LinkOptions { libraries: vec!["m".to_owned()], ..LinkOptions::default() };

    compile_with_link_options(&input.path, &output.path, link).expect("compile and link with -lm");

    let status = Command::new(&output.path).status().expect("run linked executable");
    assert_eq!(status.code(), Some(0));
}

#[cfg(not(windows))]
#[test]
fn e2e_shared_library_when_enabled() {
    if std::env::var_os("RCC_RUN_LINK_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert!(llvm_backend_enabled_for_this_build(), "LLVM backend feature is required");

    let input = TempCFile::new("shared", "int add(int a, int b) { return a + b; }\n");
    let output = TempFile::empty_path("librcc_shared.so");
    let link = LinkOptions { shared: true, ..LinkOptions::default() };

    compile_with_link_options(&input.path, &output.path, link)
        .expect("compile and link shared object");

    assert!(output.path.exists(), "shared library was not written");
    assert!(fs::metadata(&output.path).expect("shared metadata").len() > 0);
}
