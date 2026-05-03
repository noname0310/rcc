use std::fs;
#[cfg(not(windows))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::pipeline;
#[cfg(not(windows))]
use rcc_errors::{CaptureEmitter, Handler};
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
            .join(format!("rcc-driver-link-{}-{id}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        fs::write(&path, bytes).expect("write temp file");
        Self { path }
    }

    fn empty_path(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-link-{}-{id}-{name}", std::process::id()));
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
            .join(format!("rcc-driver-link-{}-{id}-{name}.c", std::process::id()));
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
fn compile_default(input: &Path, output: &Path) -> Result<(), String> {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session = Session::with_handler(
        Options { output: Some(output.to_path_buf()), ..Options::default() },
        handler,
    );
    pipeline::compile(&mut session, input)
}

#[cfg(not(windows))]
fn fake_failing_linker() -> TempFile {
    use std::os::unix::fs::PermissionsExt;

    let tool = TempFile::new("fake-cc", b"#!/bin/sh\necho fake linker stderr >&2\nexit 7\n");
    let mut perms = fs::metadata(&tool.path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&tool.path, perms).expect("chmod fake linker");
    tool
}

#[cfg(not(windows))]
#[test]
fn linker_failure_includes_command_and_stderr() {
    let linker = fake_failing_linker();
    let obj = TempFile::new("input.o", b"not an object");
    let output = TempFile::empty_path("a.out");

    let err = pipeline::link_with_linker(&linker.path, &obj.path, &output.path).unwrap_err();

    assert!(err.contains("linker failed with status"), "{err}");
    assert!(err.contains("command:"), "{err}");
    assert!(err.contains(obj.path.to_string_lossy().as_ref()), "{err}");
    assert!(err.contains("-o"), "{err}");
    assert!(err.contains("fake linker stderr"), "{err}");
}

#[test]
fn missing_linker_reports_program_and_command() {
    let linker = TempFile::empty_path("missing-cc");
    let obj = TempFile::new("input.o", b"not an object");
    let output = TempFile::empty_path("a.out");

    let err = pipeline::link_with_linker(&linker.path, &obj.path, &output.path).unwrap_err();

    assert!(err.contains("failed to run linker"), "{err}");
    assert!(err.contains(linker.path.to_string_lossy().as_ref()), "{err}");
}

#[cfg(not(windows))]
#[test]
fn e2e_compile_link_and_run_returns_42_when_enabled() {
    if std::env::var_os("RCC_RUN_LINK_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert!(llvm_backend_enabled_for_this_build(), "LLVM backend feature is required");

    let input = TempCFile::new("return42", "int main(void) { return 42; }\n");
    let output = TempFile::empty_path("return42");

    compile_default(&input.path, &output.path).expect("compile and link");

    let status = Command::new(&output.path).status().expect("run linked executable");
    assert_eq!(status.code(), Some(42));
}
