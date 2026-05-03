use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, Cli};
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
            .join(format!("rcc-driver-deps-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn file(&self, name: &str, src: &str) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, src).expect("write file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn parse(args: &[String]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|err| panic!("parse {args:?}: {err}"))
}

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

fn run(args: &[String]) -> std::process::Output {
    Command::new(rcc_bin()).args(args).output().expect("run rcc")
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
fn dependency_flags_parse_to_session_options() {
    let cli = parse(&[
        "rcc".to_owned(),
        "-MMD".to_owned(),
        "-MFdeps/out.d".to_owned(),
        "-MT".to_owned(),
        "raw target".to_owned(),
        "-MQquoted target".to_owned(),
        "hello.c".to_owned(),
    ]);
    let opts = options_from_cli(&cli);

    assert_eq!(opts.dependencies.mode, Some(rcc_session::DependencyMode::SideEffect));
    assert!(!opts.dependencies.include_system_headers);
    assert_eq!(opts.dependencies.output.as_deref(), Some(Path::new("deps/out.d")));
    assert_eq!(opts.dependencies.targets.len(), 2);
    assert_eq!(opts.dependencies.targets[0].text, "raw target");
    assert!(!opts.dependencies.targets[0].quote);
    assert_eq!(opts.dependencies.targets[1].text, "quoted target");
    assert!(opts.dependencies.targets[1].quote);
}

#[test]
fn m_writes_dependencies_to_stdout_and_stops_before_compile() {
    let dir = TempDir::new("stdout");
    let header = dir.file("include/util.h", "int util;\n");
    let input = dir.file("hello.c", "#include \"include/util.h\"\nint main(void) { return 0; }\n");

    let output = run(&["-M".to_owned(), input.display().to_string()]);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.starts_with(&format!("{}:", escaped_stem_target(&input))), "{stdout}");
    assert!(stdout.contains(&escape_for_make(&input)), "{stdout}");
    assert!(stdout.contains(&escape_for_make(&header)), "{stdout}");
}

#[test]
fn mm_excludes_angle_headers_from_dependency_output() {
    let dir = TempDir::new("user-only");
    let header = dir.file("sys.h", "int sys;\n");
    let input = dir.file("hello.c", "#include <sys.h>\nint main(void) { return 0; }\n");

    let output = run(&[
        "-MM".to_owned(),
        "-I".to_owned(),
        dir.path.display().to_string(),
        input.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.contains(&escape_for_make(&input)), "{stdout}");
    assert!(!stdout.contains(&escape_for_make(&header)), "{stdout}");
}

#[test]
fn mq_target_is_make_escaped() {
    let dir = TempDir::new("mq");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");

    let output = run(&[
        "-M".to_owned(),
        "-MQ".to_owned(),
        "obj dir/a#b$.o".to_owned(),
        input.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.starts_with(r"obj\ dir/a\#b$$.o:"), "{stdout}");
}

#[test]
fn missing_include_fails_without_writing_dependency_output() {
    let dir = TempDir::new("missing");
    let input = dir.file("bad.c", "#include \"missing.h\"\nint main(void) { return 0; }\n");
    let dep = dir.path.join("bad.d");

    let output = run(&[
        "-M".to_owned(),
        "-MF".to_owned(),
        dep.display().to_string(),
        input.display().to_string(),
    ]);

    assert!(!output.status.success(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    assert!(!dep.exists(), "dependency file must not be written after include failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot find header `missing.h`"), "{stderr}");
}

#[test]
fn mmd_side_effect_writes_dependency_file_and_object_when_backend_enabled() {
    if !llvm_backend_enabled_for_this_build() {
        eprintln!("skipping -MMD -c object smoke: LLVM backend feature is disabled");
        return;
    }
    let dir = TempDir::new("mmd");
    let header = dir.file("util.h", "int util;\n");
    let input = dir.file("hello.c", "#include \"util.h\"\nint main(void) { return 0; }\n");
    let object = dir.path.join("hello.o");
    let dep = dir.path.join("hello.d");

    let output = run(&[
        "-MMD".to_owned(),
        "-MF".to_owned(),
        dep.display().to_string(),
        "-c".to_owned(),
        "-o".to_owned(),
        object.display().to_string(),
        input.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(object.is_file(), "object output missing: {}", object.display());
    let deps = fs::read_to_string(&dep).expect("read dependency file");
    assert!(deps.starts_with(&format!("{}:", escape_for_make(&object))), "{deps}");
    assert!(deps.contains(&escape_for_make(&input)), "{deps}");
    assert!(deps.contains(&escape_for_make(&header)), "{deps}");
}

fn escaped_stem_target(input: &Path) -> String {
    let mut target = input.to_path_buf();
    target.set_extension("o");
    escape_for_make(&target)
}

fn escape_for_make(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(|ch| match ch {
            ' ' | '\t' => vec!['\\', ch],
            '#' => vec!['\\', '#'],
            '$' => vec!['$', '$'],
            '\\' => vec!['\\', '\\'],
            _ => vec![ch],
        })
        .collect()
}
