use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::pipeline;
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::{EmitKind, Options, Session};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-backend-required-{}-{id}-{name}.c", std::process::id()));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
    }
}

impl Drop for TempCFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn temp_output_path(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!("rcc-driver-backend-required-{}-{id}-{name}", std::process::id()));
    let _ = fs::remove_file(&path);
    path
}

fn compile_with(emit: Vec<EmitKind>, output: Option<PathBuf>) -> Result<(), String> {
    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session =
        Session::with_handler(Options { emit, output, ..Options::default() }, handler);
    pipeline::compile(&mut session, &input.path)
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
fn default_compile_requires_backend_in_no_llvm_build() {
    if llvm_backend_enabled_for_this_build() {
        return;
    }

    let output = temp_output_path("default-out");
    let err = compile_with(Vec::new(), Some(output.clone())).unwrap_err();

    assert!(
        err.contains("without the `llvm` feature"),
        "backend-required failure should mention unavailable LLVM backend, got: {err}"
    );
    assert!(!output.exists(), "failed backend-required invocation must not create output");
}

#[test]
fn llvm_ir_emit_requires_backend_in_no_llvm_build() {
    if llvm_backend_enabled_for_this_build() {
        return;
    }

    let output = temp_output_path("llvm-ir-out");
    let err = compile_with(vec![EmitKind::LlvmIr], Some(output.clone())).unwrap_err();

    assert!(
        err.contains("without the `llvm` feature"),
        "backend-required failure should mention unavailable LLVM backend, got: {err}"
    );
    assert!(!output.exists(), "failed llvm-ir invocation must not create output");
}

#[test]
fn frontend_only_tokens_do_not_require_backend() {
    compile_with(vec![EmitKind::Tokens], None).expect("tokens-only emit should not need LLVM");
}

#[test]
fn frontend_only_preprocess_do_not_require_backend() {
    compile_with(vec![EmitKind::Pp], None).expect("preprocess-only emit should not need LLVM");
}
