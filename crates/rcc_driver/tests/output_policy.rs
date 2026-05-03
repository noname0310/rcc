use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
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
            .join(format!("rcc-driver-output-{}-{id}-{name}", std::process::id()));
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
fn save_temps_cli_accepts_optional_directory() {
    let implicit = parse(&["rcc", "--save-temps", "hello.c"]);
    let explicit = parse(&["rcc", "--save-temps=build/temps", "hello.c"]);

    assert_eq!(options_from_cli(&implicit).save_temps.as_deref(), Some(Path::new(".")));
    assert_eq!(options_from_cli(&explicit).save_temps.as_deref(), Some(Path::new("build/temps")));
}

#[test]
fn output_path_must_not_clobber_input_file() {
    let dir = TempDir::new("collision");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let cli = parse(&["rcc", "-E", "-o", input.to_str().unwrap(), input.to_str().unwrap()]);

    assert_eq!(run(cli), 1);
    let source = fs::read_to_string(&input).expect("read input after rejected run");
    assert!(source.contains("return 0"), "{source}");
}

#[test]
fn frontend_multi_emit_uses_deterministic_stage_paths() {
    let dir = TempDir::new("frontend-stages");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let base = dir.path.join("build").join("out");
    let cli = parse(&[
        "rcc",
        "--emit=tokens",
        "--emit=ast",
        "-o",
        base.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 0);
    assert!(!base.exists(), "multi-stage emit must not write the base output path");
    assert!(PathBuf::from(format!("{}.tokens", base.display())).exists());
    assert!(PathBuf::from(format!("{}.ast", base.display())).exists());
}

#[test]
fn save_temps_preserves_preprocessed_output_for_frontend_runs() {
    let dir = TempDir::new("save-pp");
    let input = dir.file("hello.c", "#define X 7\nint main(void) { return X; }\n");
    let temps = dir.path.join("temps");
    let cli = parse(&[
        "rcc",
        &format!("--save-temps={}", temps.display()),
        "-E",
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 0);
    let saved = fs::read_to_string(temps.join("hello.i")).expect("read saved .i");
    assert!(saved.contains("return"), "{saved}");
    assert!(saved.contains("7"), "{saved}");
}

#[test]
fn backend_save_temps_preserves_codegen_intermediates_when_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("save-codegen");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let output = dir.path.join("hello-out");
    let temps = dir.path.join("temps");
    let cli = parse(&[
        "rcc",
        &format!("--save-temps={}", temps.display()),
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 0);
    for ext in ["i", "ll", "s", "o"] {
        assert!(temps.join(format!("hello.{ext}")).exists(), "missing saved {ext}");
    }
}

#[test]
fn failed_link_removes_private_temp_directory_when_save_temps_is_off() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("failed-link-cleanup");
    let stem = "cleanup_marker";
    let input = dir.file(&format!("{stem}.c"), "int main(void) { return 0; }\n");
    let before = matching_temp_entries(stem);
    let cli = parse(&["rcc", "-l__rcc_missing_output_policy__", input.to_str().unwrap()]);

    assert_eq!(run(cli), 1);

    let after = matching_temp_entries(stem);
    assert_eq!(after, before, "private temp dirs leaked: before={before:?} after={after:?}");
}

#[cfg(not(windows))]
#[test]
fn backend_multi_emit_mir_and_llvm_ir_uses_stage_paths_when_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("backend-stages");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let base = dir.path.join("build").join("out");
    let cli = parse(&[
        "rcc",
        "--emit=mir",
        "--emit=llvm-ir",
        "-o",
        base.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 0);
    assert!(!base.exists(), "multi-stage emit must not write the base output path");
    assert!(PathBuf::from(format!("{}.mir", base.display())).exists());
    assert!(PathBuf::from(format!("{}.ll", base.display())).exists());
}

#[cfg(not(windows))]
#[test]
fn save_temps_object_survives_failed_link_when_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("save-after-failed-link");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let temps = dir.path.join("temps");
    let cli = parse(&[
        "rcc",
        &format!("--save-temps={}", temps.display()),
        "-l__rcc_missing_output_policy__",
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 1);
    assert!(temps.join("hello.o").exists());
}

#[cfg(not(windows))]
#[test]
fn saved_temp_object_can_be_linked_by_host_cc() {
    if !llvm_backend_enabled_for_this_build() {
        return;
    }
    let dir = TempDir::new("saved-object-link");
    let input = dir.file("hello.c", "int main(void) { return 3; }\n");
    let output = dir.path.join("hello");
    let temps = dir.path.join("temps");
    let cli = parse(&[
        "rcc",
        &format!("--save-temps={}", temps.display()),
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);

    assert_eq!(run(cli), 0);
    let status = Command::new(&output).status().expect("run executable");
    assert_eq!(status.code(), Some(3));
    assert!(temps.join("hello.o").exists());
}

fn matching_temp_entries(stem: &str) -> BTreeSet<PathBuf> {
    let prefix = format!("rcc-{}-", std::process::id());
    fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            name.starts_with(&prefix) && name.contains(stem)
        })
        .collect()
}
