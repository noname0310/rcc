use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, pipeline, Cli};
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::{Options, Session, TargetInfo};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcc-driver-target-{}-{id}", std::process::id()));
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

fn compile_cli(cli: &Cli) -> Result<(), String> {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session = Session::with_handler(options_from_cli(cli), handler);
    pipeline::compile(&mut session, &cli.input)
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
fn target_flag_populates_options_target_info() {
    let cli = parse(&["rcc", "--target=aarch64-unknown-linux-gnu", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert_eq!(opts.target.triple.as_str(), "aarch64-unknown-linux-gnu");
    assert_eq!(opts.target.pointer_width, 64);
    assert!(opts.target.llvm_data_layout.contains("n32:64"));
}

#[test]
fn missing_target_defaults_to_host_target() {
    let cli = parse(&["rcc", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert_eq!(opts.target, TargetInfo::host());
}

#[test]
fn invalid_target_is_rejected_by_cli() {
    let err = Cli::try_parse_from(["rcc", "--target=not-a-real-target", "hello.c"]).unwrap_err();
    let rendered = err.to_string();

    assert!(rendered.contains("unsupported target triple `not-a-real-target`"), "{rendered}");
}

#[test]
fn target_flag_drives_predefined_macros() {
    let input = TempCFile::new(
        "target-macros",
        "#ifdef __aarch64__\nARCH_OK\n#else\nARCH_BAD\n#endif\n__SIZEOF_POINTER__\n__SIZEOF_LONG__\n",
    );
    let output = input.sibling("i");
    let cli = parse(&[
        "rcc",
        "--target=aarch64-unknown-linux-gnu",
        "-E",
        "-o",
        output.to_str().unwrap(),
        input.path.to_str().unwrap(),
    ]);

    compile_cli(&cli).expect("targeted -E should preprocess");

    let pp = fs::read_to_string(&output).expect("read preprocessed output");
    assert!(pp.contains("ARCH_OK"), "{pp}");
    assert!(!pp.contains("ARCH_BAD"), "{pp}");
    assert!(pp.contains("\n8\n"), "pointer/long size macros should be target-derived:\n{pp}");
}

#[test]
fn windows_msvc_target_uses_llp64_predefined_long_size() {
    let input = TempCFile::new("llp64", "__SIZEOF_POINTER__\n__SIZEOF_LONG__\n");
    let output = input.sibling("i");
    let cli = parse(&[
        "rcc",
        "--target=x86_64-pc-windows-msvc",
        "-E",
        "-o",
        output.to_str().unwrap(),
        input.path.to_str().unwrap(),
    ]);

    compile_cli(&cli).expect("targeted -E should preprocess");

    let pp = fs::read_to_string(&output).expect("read preprocessed output");
    assert_eq!(pp, "8\n4\n");
}

#[test]
fn llvm_ir_emit_contains_requested_target_triple_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }

    let input = TempCFile::new("ir-target", "int main(void) { return 0; }\n");
    let output = input.sibling("ll");
    let cli = parse(&[
        "rcc",
        "--target=x86_64-unknown-linux-gnu",
        "--emit=llvm-ir",
        "-o",
        output.to_str().unwrap(),
        input.path.to_str().unwrap(),
    ]);

    compile_cli(&cli).expect("targeted LLVM IR emit should succeed");

    let ir = fs::read_to_string(&output).expect("read LLVM IR");
    assert!(ir.contains("target triple = \"x86_64-unknown-linux-gnu\""), "{ir}");
}
