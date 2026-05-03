use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
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
        let path = env::temp_dir()
            .join(format!("rcc-driver-debug-info-{}-{id}-{name}", std::process::id()));
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

fn find_llvm_tool(base: &str) -> Option<PathBuf> {
    let names = llvm_tool_names(base);
    for env_name in ["LLVM_SYS_181_PREFIX", "LLVM_SYS_180_PREFIX", "LLVM_PREFIX"] {
        if let Some(prefix) = env::var_os(env_name) {
            let bin = PathBuf::from(prefix).join("bin");
            if let Some(path) = find_named_tool_in(&bin, &names) {
                return Some(path);
            }
        }
    }
    env::var_os("PATH")
        .and_then(|path| env::split_paths(&path).find_map(|dir| find_named_tool_in(&dir, &names)))
}

fn llvm_tool_names(base: &str) -> Vec<String> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    vec![format!("{base}{suffix}"), format!("{base}-18{suffix}")]
}

fn find_named_tool_in(dir: &Path, names: &[String]) -> Option<PathBuf> {
    names.iter().map(|name| dir.join(name)).find(|path| path.is_file())
}

fn read_tool_stdout(tool: &Path, args: &[&OsStr]) -> String {
    let output = Command::new(tool)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("run {}: {err}", tool.display()));
    assert!(
        output.status.success(),
        "{} failed with {}\nstdout:\n{}\nstderr:\n{}",
        tool.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("LLVM tool output is utf-8")
}

fn has_debug_section(sections: &str) -> bool {
    sections.contains(".debug_info")
        || sections.contains(".debug_line")
        || sections.contains(".debug$S")
        || sections.contains(".debug$T")
}

#[test]
fn g_flag_sets_debug_info_option() {
    let cli = parse(&["rcc", "-g", "-c", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(cli.debug_info);
    assert!(opts.debug_info);
}

#[test]
fn object_debug_sections_follow_g_flag_when_tools_are_available() {
    if !llvm_backend_enabled_for_this_build() {
        eprintln!("skipping debug object smoke: LLVM backend feature is disabled");
        return;
    }
    let Some(readobj) = find_llvm_tool("llvm-readobj") else {
        eprintln!("skipping debug object smoke: llvm-readobj is unavailable");
        return;
    };
    let dwarfdump = find_llvm_tool("llvm-dwarfdump");

    let dir = TempDir::new("object");
    let input =
        dir.file("hello.c", "int f(int param) {\n  int local = param + 1;\n  return local;\n}\n");
    let debug_obj = dir.path.join("debug.o");
    let plain_obj = dir.path.join("plain.o");

    let debug_cli =
        parse(&["rcc", "-g", "-c", "-o", debug_obj.to_str().unwrap(), input.to_str().unwrap()]);
    assert_eq!(run(debug_cli), 0);

    let plain_cli =
        parse(&["rcc", "-c", "-o", plain_obj.to_str().unwrap(), input.to_str().unwrap()]);
    assert_eq!(run(plain_cli), 0);

    let debug_sections =
        read_tool_stdout(&readobj, &[OsStr::new("--sections"), debug_obj.as_os_str()]);
    assert!(has_debug_section(&debug_sections), "sections:\n{debug_sections}");

    let plain_sections =
        read_tool_stdout(&readobj, &[OsStr::new("--sections"), plain_obj.as_os_str()]);
    assert!(!has_debug_section(&plain_sections), "sections:\n{plain_sections}");

    if let Some(dwarfdump) = dwarfdump {
        let dump = read_tool_stdout(
            &dwarfdump,
            &[OsStr::new("--debug-info"), OsStr::new("--debug-line"), debug_obj.as_os_str()],
        );
        assert!(dump.contains("DW_TAG_subprogram"), "dwarfdump:\n{dump}");
        assert!(dump.contains("DW_AT_name") && dump.contains("\"f\""), "dwarfdump:\n{dump}");
        assert!(dump.contains("\"param\""), "dwarfdump:\n{dump}");
        assert!(dump.contains("\"local\""), "dwarfdump:\n{dump}");
        assert!(dump.contains("debug_line") || dump.contains("Line table"), "dwarfdump:\n{dump}");
    } else {
        eprintln!("skipping debug name smoke: llvm-dwarfdump is unavailable");
    }
}
