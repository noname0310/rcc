use std::fs;
use std::path::{Path, PathBuf};
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
            .join(format!("rcc-driver-emit-stages-{}-{id}-{name}.c", std::process::id()));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
    }
}

impl Drop for TempCFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct TempOutput {
    path: PathBuf,
}

impl TempOutput {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-emit-stages-{}-{id}-{name}", std::process::id()));
        cleanup_output_family(&path);
        Self { path }
    }

    fn stage(&self, ext: &str) -> PathBuf {
        PathBuf::from(format!("{}.{}", self.path.display(), ext))
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        cleanup_output_family(&self.path);
    }
}

fn cleanup_output_family(base: &Path) {
    let _ = fs::remove_file(base);
    for ext in ["tokens", "pp", "ast", "hir", "mir", "ll", "s", "o"] {
        let _ = fs::remove_file(PathBuf::from(format!("{}.{}", base.display(), ext)));
    }
}

fn compile_with(
    input: &TempCFile,
    emit: Vec<EmitKind>,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session =
        Session::with_handler(Options { emit, output, ..Options::default() }, handler);
    pipeline::compile(&mut session, &input.path)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn read_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
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
fn multi_emit_tokens_and_ast_write_stage_files() {
    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("out");

    compile_with(&input, vec![EmitKind::Tokens, EmitKind::Ast], Some(output.path.clone()))
        .expect("tokens + ast emit should succeed without LLVM");

    let tokens = read(&output.stage("tokens"));
    assert!(tokens.contains("Ident  main"), "tokens:\n{tokens}");
    let ast = read(&output.stage("ast"));
    assert!(ast.contains("TranslationUnit"), "ast:\n{ast}");
    assert!(!ast.contains("-- emit=ast"), "placeholder leaked into ast dump:\n{ast}");
}

#[test]
fn single_ast_emit_writes_exact_output_path() {
    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("single-ast");

    compile_with(&input, vec![EmitKind::Ast], Some(output.path.clone()))
        .expect("single ast emit should succeed without LLVM");

    let ast = read(&output.path);
    assert!(ast.contains("TranslationUnit"), "ast:\n{ast}");
    assert!(!ast.contains("-- emit=ast"), "placeholder leaked into ast dump:\n{ast}");
    assert!(!output.stage("ast").exists(), "single output must not append .ast");
}

#[test]
fn single_preprocess_emit_writes_exact_output_path() {
    let input = TempCFile::new("hello", "#define X 1\nint y = X;\n");
    let output = TempOutput::new("single-pp");

    compile_with(&input, vec![EmitKind::Pp], Some(output.path.clone()))
        .expect("single preprocessor emit should succeed without LLVM");

    let pp = read(&output.path);
    assert!(pp.contains("int y ="), "pp:\n{pp}");
    assert!(pp.contains("\n1\n"), "pp:\n{pp}");
    assert!(pp.ends_with(";\n"), "pp:\n{pp}");
    assert!(!output.stage("pp").exists(), "single output must not append .pp");
}

#[test]
fn frontend_hir_and_mir_write_stage_files_without_backend() {
    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("frontend");

    compile_with(&input, vec![EmitKind::Hir, EmitKind::Mir], Some(output.path.clone()))
        .expect("hir + mir emit should not need LLVM");

    let hir = read(&output.stage("hir"));
    assert!(hir.contains("HirCrate"), "hir:\n{hir}");
    let mir = read(&output.stage("mir"));
    assert!(mir.contains("fn def#"), "mir:\n{mir}");
}

#[test]
fn stddef_header_typechecks_without_backend() {
    let input = TempCFile::new(
        "stddef",
        r#"
        #include <stddef.h>
        struct S { char c; int field; };
        int f(void) {
            size_t n = sizeof(int);
            ptrdiff_t d = 0;
            wchar_t w = 0;
            max_align_t a;
            return (int)(n + (size_t)d + (size_t)w + sizeof(a)
                + (sizeof(size_t) == sizeof(void *))
                + (offsetof(struct S, field) == 4));
        }
        "#,
    );
    let output = TempOutput::new("stddef");

    compile_with(&input, vec![EmitKind::Hir], Some(output.path.clone()))
        .expect("stddef.h should parse, expand, and type-check without LLVM");

    let hir = read(&output.path);
    assert!(hir.contains("HirCrate"), "hir:\n{hir}");
}

#[test]
fn backend_required_emit_does_not_flush_partial_stage_files_without_backend() {
    if llvm_backend_enabled_for_this_build() {
        return;
    }

    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("backend-required");

    let err =
        compile_with(&input, vec![EmitKind::Tokens, EmitKind::LlvmIr], Some(output.path.clone()))
            .unwrap_err();

    assert!(
        err.contains("without the `llvm` feature"),
        "backend-required error should mention unavailable LLVM backend, got: {err}"
    );
    assert!(!output.stage("tokens").exists(), "failed backend-required emit wrote tokens");
    assert!(!output.stage("ll").exists(), "failed backend-required emit wrote LLVM IR");
}

#[test]
fn asm_and_obj_emit_require_backend_in_no_llvm_build() {
    if llvm_backend_enabled_for_this_build() {
        return;
    }

    for kind in [EmitKind::Asm, EmitKind::Obj] {
        let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
        let output = TempOutput::new("backend-stage");

        let err = compile_with(&input, vec![kind], Some(output.path.clone())).unwrap_err();

        assert!(
            err.contains("without the `llvm` feature"),
            "backend-required error should mention unavailable LLVM backend, got: {err}"
        );
        assert!(!output.path.exists(), "failed backend-required emit wrote output");
    }
}

#[test]
fn multi_emit_hir_mir_llvm_ir_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }

    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("llvm");

    compile_with(
        &input,
        vec![EmitKind::Hir, EmitKind::Mir, EmitKind::LlvmIr],
        Some(output.path.clone()),
    )
    .expect("hir + mir + llvm-ir emit should succeed with LLVM enabled");

    let hir = read(&output.stage("hir"));
    assert!(hir.contains("HirCrate"), "hir:\n{hir}");
    let mir = read(&output.stage("mir"));
    assert!(mir.contains("fn def#"), "mir:\n{mir}");
    let ir = read(&output.stage("ll"));
    assert!(ir.contains("define i32 @main"), "ir:\n{ir}");
}

#[test]
fn asm_and_obj_emit_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }

    let input = TempCFile::new("hello", "int main(void) { return 0; }\n");
    let output = TempOutput::new("asm-obj");

    compile_with(&input, vec![EmitKind::Asm, EmitKind::Obj], Some(output.path.clone()))
        .expect("asm + obj emit should succeed with LLVM enabled");

    let asm = read(&output.stage("s"));
    assert!(asm.contains("main"), "asm:\n{asm}");
    let obj = read_bytes(&output.stage("o"));
    assert!(obj.starts_with(b"\x7fELF"), "expected ELF object bytes, got: {obj:02x?}");
}
