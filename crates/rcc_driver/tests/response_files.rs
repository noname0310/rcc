use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{Cli, ExitCode};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rcc-driver-rsp-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn file(&self, name: &str, src: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, src).expect("write file");
        path
    }

    fn bytes(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, bytes).expect("write bytes");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn parse(args: &[String]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(args)
}

fn at(path: &Path) -> String {
    format!("@{}", path.display())
}

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

#[test]
fn response_file_expands_like_direct_arguments() {
    let dir = TempDir::new("basic");
    let input = dir.file("hello world.c", "int main(void) { return 0; }\n");
    let output = dir.path.join("hello ast.txt");
    let rsp = dir.file(
        "args.rsp",
        &format!(
            "--emit=ast\n-o \"{}\"\n\"{}\"\n# trailing comment\n",
            output.display(),
            input.display()
        ),
    );

    let cli = parse(&["rcc".to_owned(), at(&rsp)]).expect("parse response file");

    assert_eq!(cli.emit.len(), 1);
    assert_eq!(cli.output.as_deref(), Some(output.as_path()));
    assert_eq!(cli.input, [input]);
}

#[test]
fn nested_response_file_references_are_relative_to_parent_file() {
    let dir = TempDir::new("nested");
    let sub = dir.path.join("sub");
    fs::create_dir_all(&sub).expect("create subdir");
    let leaf = sub.join("leaf.rsp");
    fs::write(&leaf, "nested.c").expect("write leaf");
    let inner = sub.join("inner.rsp");
    fs::write(&inner, "@leaf.rsp").expect("write inner");
    let outer = dir.file("outer.rsp", "--emit=ast @sub/inner.rsp");

    let cli = parse(&["rcc".to_owned(), at(&outer)]).expect("parse nested response files");

    assert_eq!(cli.input, [PathBuf::from("nested.c")]);
}

#[test]
fn response_file_preserves_windows_paths_and_escaped_quotes() {
    let dir = TempDir::new("escaping");
    let rsp = dir.file("args.rsp", "-DMSG=\\\"hello\\\" \"C:\\\\src dir\\\\hello.c\"");

    let cli = parse(&["rcc".to_owned(), at(&rsp)]).expect("parse escaping response file");

    assert_eq!(cli.defines, [("MSG".to_owned(), Some("\"hello\"".to_owned()))]);
    assert_eq!(cli.input, [PathBuf::from("C:\\\\src dir\\\\hello.c")]);
}

#[test]
fn response_file_cycle_is_rejected() {
    let dir = TempDir::new("cycle");
    let a = dir.path.join("a.rsp");
    let b = dir.path.join("b.rsp");
    fs::write(&a, format!("@{}", b.display())).expect("write a");
    fs::write(&b, format!("@{}", a.display())).expect("write b");

    let err = parse(&["rcc".to_owned(), at(&a)]).unwrap_err().to_string();

    assert!(err.contains("response file cycle"), "{err}");
    assert!(err.contains("a.rsp"), "{err}");
    assert!(err.contains("b.rsp"), "{err}");
}

#[test]
fn missing_response_file_is_rejected_with_path() {
    let dir = TempDir::new("missing");
    let missing = dir.path.join("missing.rsp");

    let err = parse(&["rcc".to_owned(), at(&missing)]).unwrap_err().to_string();

    assert!(err.contains("cannot read"), "{err}");
    assert!(err.contains("missing.rsp"), "{err}");
}

#[test]
fn non_utf8_response_file_is_rejected() {
    let dir = TempDir::new("utf8");
    let rsp = dir.bytes("bad.rsp", &[0xff, 0xfe, b'\n']);

    let err = parse(&["rcc".to_owned(), at(&rsp)]).unwrap_err().to_string();

    assert!(err.contains("cannot decode UTF-8"), "{err}");
    assert!(err.contains("bad.rsp"), "{err}");
}

#[test]
fn unterminated_quote_reports_line_and_column() {
    let dir = TempDir::new("quote");
    let rsp = dir.file("bad.rsp", "--emit=ast\n\"unterminated.c");

    let err = parse(&["rcc".to_owned(), at(&rsp)]).unwrap_err().to_string();

    assert!(err.contains("bad.rsp:2:1"), "{err}");
    assert!(err.contains("unterminated \" quote"), "{err}");
}

#[test]
fn cyclic_response_file_exits_with_usage_failure() {
    let dir = TempDir::new("binary-cycle");
    let rsp = dir.file("self.rsp", "@self.rsp");

    let output = Command::new(rcc_bin()).arg(at(&rsp)).output().expect("run rcc");

    assert_eq!(output.status.code(), Some(ExitCode::Usage.code()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("response file cycle"), "{stderr}");
}
