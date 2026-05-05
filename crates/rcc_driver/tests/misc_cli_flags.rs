use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, run, Cli, ExitCode};
use rcc_session::OptLevel;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcc-driver-misc-cli-{}-{id}", std::process::id()));
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

fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

#[test]
fn std_c99_is_accepted_and_preserves_default_options() {
    let cli = parse(&["rcc", "-std=c99", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert_eq!(cli.standard.as_deref(), Some("c99"));
    assert_eq!(opts.emit, Vec::new());
}

#[test]
fn cli_undefine_spelling_maps_to_session_options() {
    let cli = parse(&["rcc", "-DFOO=1", "-UFOO", "-U", "BAR", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert_eq!(cli.undefines, vec!["FOO".to_string(), "BAR".to_string()]);
    assert_eq!(opts.cli_undefines, vec!["FOO".to_string(), "BAR".to_string()]);
}

#[test]
fn unsupported_std_is_rejected_during_cli_parse() {
    let err = Cli::try_parse_from(["rcc", "-std=c11", "hello.c"]).unwrap_err().to_string();

    assert!(err.contains("unsupported standard 'c11'"), "{err}");
}

#[test]
fn optimization_spelling_maps_to_session_options() {
    for (spelling, expected) in [
        ("-O", OptLevel::Less),
        ("-O0", OptLevel::None),
        ("-O1", OptLevel::Less),
        ("-O2", OptLevel::Default),
        ("-O3", OptLevel::Aggressive),
    ] {
        let cli = parse(&["rcc", spelling, "hello.c"]);
        let opts = options_from_cli(&cli);
        assert_eq!(opts.opt_level, expected, "{spelling}");
    }

    let cli = parse(&["rcc", "--opt-level=aggressive", "hello.c"]);
    assert_eq!(options_from_cli(&cli).opt_level, OptLevel::Aggressive);
}

#[test]
fn ansi_alias_is_parsed_but_rejected_before_compilation() {
    let cli = parse(&["rcc", "-ansi", "does-not-need-to-exist.c"]);

    assert!(cli.ansi);
    assert_eq!(run(cli), ExitCode::Usage.code());
}

#[test]
fn known_f_flags_parse_and_do_not_change_options() {
    let cli = parse(&[
        "rcc",
        "-fPIC",
        "-fno-strict-aliasing",
        "-fwrapv",
        "-fstack-protector",
        "-fno-common",
        "-fvisibility=hidden",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert_eq!(
        cli.feature_flags,
        ["PIC", "no-strict-aliasing", "wrapv", "stack-protector", "no-common", "visibility=hidden"]
    );
    assert_eq!(opts.emit, Vec::new());
}

#[test]
fn gnu_binary_literals_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-binary-literals", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_binary_integer_literals);
}

#[test]
fn gnu_statement_expressions_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-statement-expressions", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_statement_expressions);
}

#[test]
fn gnu_omitted_conditional_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-omitted-conditional-operand", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_omitted_conditional_operand);
}

#[test]
fn gnu_conditional_void_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-conditional-void-operand", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_conditional_void_operand);
}

#[test]
fn gnu_function_names_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-function-names", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_function_names);
}

#[test]
fn gnu_va_area_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-va-area", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_va_area);
}

#[test]
fn gnu89_inline_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu89-inline", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu89_inline);
}

#[test]
fn gnu_extension_flags_added_for_conformance_are_wired() {
    let cli = parse(&[
        "rcc",
        "-fgnu-range-designators",
        "-fgnu-attributes",
        "-fgnu-inline-asm",
        "-fgnu-builtin-libcalls",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_range_designators);
    assert!(opts.gnu_attributes);
    assert!(opts.gnu_inline_asm);
    assert!(opts.gnu_builtin_libcalls);
}

#[test]
fn gnu_implicit_function_declaration_flag_sets_frontend_option() {
    let cli = parse(&["rcc", "-fgnu-implicit-function-declaration", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_implicit_function_declaration);

    let alias = parse(&["rcc", "-fimplicit-function-declaration", "hello.c"]);
    assert!(options_from_cli(&alias).gnu_implicit_function_declaration);
}

#[test]
fn gnu_preprocessor_compat_flags_set_frontend_options() {
    let cli = parse(&[
        "rcc",
        "-fgnu-va-args-elision",
        "-fgnu-permissive-redefinition",
        "-fgnu-named-variadic",
        "-fgnu-permissive-paste",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_va_args_elision);
    assert!(opts.gnu_permissive_redefinition);
    assert!(opts.gnu_named_variadic);
    assert!(opts.gnu_permissive_paste);
}

#[test]
fn isystem_spelling_maps_to_system_include_options() {
    let first = PathBuf::from("first-system-include");
    let second = PathBuf::from("second-system-include");
    let joined = format!("-isystem{}", second.display());
    let cli = Cli::try_parse_from([
        OsString::from("rcc"),
        OsString::from("-isystem"),
        first.clone().into_os_string(),
        OsString::from(joined),
        OsString::from("hello.c"),
    ])
    .unwrap();
    let opts = options_from_cli(&cli);

    assert_eq!(cli.system_include_paths, vec![first.clone(), second.clone()]);
    assert!(opts.system_include_paths.starts_with(&[first, second]));
}

#[test]
fn sysroot_discovers_existing_linux_system_include_dirs_under_root() {
    let root = tempfile::tempdir().unwrap();
    for rel in [
        Path::new("usr/include"),
        Path::new("usr/local/include"),
        Path::new("usr/include/x86_64-unknown-linux-gnu"),
    ] {
        fs::create_dir_all(root.path().join(rel)).unwrap();
    }
    let cli = Cli::try_parse_from([
        OsString::from("rcc"),
        OsString::from("--target=x86_64-unknown-linux-gnu"),
        OsString::from("--sysroot"),
        root.path().as_os_str().to_owned(),
        OsString::from("hello.c"),
    ])
    .unwrap();
    let opts = options_from_cli(&cli);

    assert_eq!(opts.sysroot.as_deref(), Some(root.path()));
    assert!(opts.system_include_paths.starts_with(&[
        root.path().join("usr/include"),
        root.path().join("usr/local/include"),
        root.path().join("usr/include/x86_64-unknown-linux-gnu"),
    ]));
}

#[test]
fn strict_binary_integer_literal_is_rejected() {
    let input = TempCFile::new("strict-binary", "int x = 0b10;\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(!result.status.success(), "strict mode accepted GNU binary literal");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("E0011") || stderr.contains("octal"), "{stderr}");
}

#[test]
fn gnu_binary_integer_literal_frontend_accepts_and_preserves_value() {
    let input = TempCFile::new("gnu-binary", "int x = 0b10011;\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("-fgnu-binary-literals")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!stderr.contains("ignoring compatibility flag -fgnu-binary-literals"), "{stderr}");
    let ast = fs::read_to_string(&output).expect("read ast output");
    assert!(ast.contains("value: 19"), "{ast}");
    assert!(ast.contains("base: Binary"), "{ast}");
}

#[test]
fn fpic_frontend_compile_succeeds_and_reports_ignored_note() {
    let input = TempCFile::new("fpic", "int main(void) { return 0; }\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("-fPIC")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("note: ignoring compatibility flag -fPIC"), "{stderr}");
}

#[test]
fn unknown_f_flag_frontend_compile_succeeds_with_warning() {
    let input = TempCFile::new("unknown-f", "int main(void) { return 0; }\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("-fexperimental-thing")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("warning: ignoring unknown compatibility flag -fexperimental-thing"),
        "{stderr}"
    );
}
