use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use rcc_driver::{options_from_cli, pipeline, Cli};
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::{EmitKind, Options, Session};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcc-driver-stop-flags-{}-{id}", std::process::id()));
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

fn compile_cli(cli: &Cli) -> Result<(), String> {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut session = Session::with_handler(options_from_cli(cli), handler);
    pipeline::compile(&mut session, &cli.input)
}

#[test]
fn stop_flags_map_to_internal_emit_modes() {
    let input = Path::new("hello.c");

    let obj = options_from_cli(&parse(&["rcc", "-c", "hello.c"]));
    assert_eq!(obj.emit, vec![EmitKind::Obj]);
    assert_eq!(obj.output.as_deref(), Some(Path::new("hello.o")));

    let asm = options_from_cli(&parse(&["rcc", "-S", "hello.c"]));
    assert_eq!(asm.emit, vec![EmitKind::Asm]);
    assert_eq!(asm.output.as_deref(), Some(Path::new("hello.s")));

    let pp = options_from_cli(&parse(&["rcc", "-E", "hello.c"]));
    assert_eq!(pp.emit, vec![EmitKind::Pp]);
    assert_eq!(pp.output, None);

    let explicit = options_from_cli(&parse(&["rcc", "-c", "-o", "custom.obj", "hello.c"]));
    assert_eq!(explicit.emit, vec![EmitKind::Obj]);
    assert_eq!(explicit.output.as_deref(), Some(Path::new("custom.obj")));

    assert_eq!(input.with_extension("o"), PathBuf::from("hello.o"));
}

#[test]
fn stop_flags_are_mutually_exclusive() {
    let err = Cli::try_parse_from(["rcc", "-c", "-S", "hello.c"]).unwrap_err().to_string();
    assert!(err.contains("cannot be used with"), "{err}");

    let err =
        Cli::try_parse_from(["rcc", "-E", "--emit", "tokens", "hello.c"]).unwrap_err().to_string();
    assert!(err.contains("cannot be used with"), "{err}");
}

#[test]
fn preprocess_only_can_write_to_explicit_output() {
    let input = TempCFile::new("pp", "#define X 42\nint x = X;\n");
    let output = input.sibling("i");
    let cli = parse(&["rcc", "-E", "-o", output.to_str().unwrap(), input.path.to_str().unwrap()]);

    compile_cli(&cli).expect("-E -o should write preprocessed output");

    let pp = fs::read_to_string(&output).expect("read preprocessed output");
    assert!(pp.contains("int x ="), "{pp}");
    assert!(pp.contains("42"), "{pp}");
}

#[test]
fn compile_only_writes_default_object_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let input = TempCFile::new("obj", "int main(void) { return 0; }\n");
    let output = input.sibling("o");
    let cli = parse(&["rcc", "-c", input.path.to_str().unwrap()]);

    compile_cli(&cli).expect("-c should write object");

    let bytes = fs::read(&output).expect("read object output");
    assert!(bytes.starts_with(b"\x7fELF"), "expected ELF object header, got {bytes:02x?}");
}

#[test]
fn assembly_writes_default_s_file_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let input = TempCFile::new("asm", "int main(void) { return 0; }\n");
    let output = input.sibling("s");
    let cli = parse(&["rcc", "-S", input.path.to_str().unwrap()]);

    compile_cli(&cli).expect("-S should write assembly");

    let asm = fs::read_to_string(&output).expect("read assembly output");
    assert!(asm.contains("main"), "{asm}");
}
