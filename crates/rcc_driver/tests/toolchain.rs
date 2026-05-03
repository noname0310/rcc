use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, Cli};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-toolchain-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn file(&self, name: &str, src: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, src).expect("write file");
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

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

#[test]
fn v_flag_sets_verbose_link_options_and_keeps_lld_default() {
    let cli = parse(&["rcc", "-v", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(cli.verbose);
    assert!(opts.link.verbose);
    assert!(opts.link.use_lld);
}

#[test]
fn verbose_frontend_run_prints_llvm_tool_selection_without_linking() {
    let dir = TempDir::new("verbose");
    let input = dir.file("hello.c", "int main(void) { return 0; }\n");
    let output = dir.path.join("hello.ast");
    let fake_clang = dir.file("clang", "");

    let result = Command::new(rcc_bin())
        .arg("-v")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input)
        .env("RCC_LINKER_DRIVER", &fake_clang)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("rcc version"), "{stderr}");
    assert!(stderr.contains("target:"), "{stderr}");
    assert!(stderr.contains("linker driver:"), "{stderr}");
    assert!(stderr.contains(fake_clang.to_string_lossy().as_ref()), "{stderr}");
    assert!(stderr.contains("lld:"), "{stderr}");
}
