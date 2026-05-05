use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, run, run_status, Cli, ExitCode};
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
        "-fgnu-qualifier-aliases",
        "-fgnu-inline-asm",
        "-fgnu-builtin-libcalls",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert!(opts.gnu_range_designators);
    assert!(opts.gnu_attributes);
    assert!(opts.gnu_qualifier_aliases);
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
fn linux_gnu_hosted_flag_sets_policy_without_gnu_syntax_flags() {
    let cli = parse(&["rcc", "--linux-gnu-hosted", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.linux_gnu_hosted);
    assert!(!opts.gnu_binary_integer_literals);
    assert!(!opts.gnu_statement_expressions);
    assert!(!opts.gnu_attributes);
    assert!(!opts.gnu_inline_asm);
    assert!(!opts.gnu_implicit_function_declaration);
}

#[test]
fn linux_gnu_hosted_installs_feature_test_macros_as_cli_defines() {
    let cli = parse(&[
        "rcc",
        "--linux-gnu-hosted",
        "-D_POSIX_C_SOURCE=199309L",
        "-U_GNU_SOURCE",
        "hello.c",
    ]);
    let opts = options_from_cli(&cli);

    assert_eq!(
        opts.cli_defines,
        vec![
            ("_GNU_SOURCE".to_owned(), Some("1".to_owned())),
            ("_DEFAULT_SOURCE".to_owned(), Some("1".to_owned())),
            ("_POSIX_C_SOURCE".to_owned(), Some("200809L".to_owned())),
            ("_XOPEN_SOURCE".to_owned(), Some("700".to_owned())),
            ("_POSIX_C_SOURCE".to_owned(), Some("199309L".to_owned())),
        ]
    );
    assert_eq!(opts.cli_undefines, vec!["_GNU_SOURCE".to_owned()]);
}

#[test]
fn pthread_flag_installs_reentrant_as_cli_define() {
    let cli = parse(&["rcc", "-pthread", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(cli.pthread);
    assert!(opts.cli_defines.contains(&("_REENTRANT".to_owned(), Some("1".to_owned()))));
    assert!(opts.link.pthread);
}

#[test]
fn pthread_is_rejected_for_windows_targets() {
    let cli = parse(&["rcc", "--target=x86_64-pc-windows-msvc", "-pthread", "hello.c"]);
    let status = run_status(cli);

    assert_eq!(status.exit_code, ExitCode::Usage);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_gnu_hosted_headers_see_feature_test_macros() {
    let input = TempCFile::new(
        "linux-hosted-headers",
        r#"
#include <features.h>
#ifndef _GNU_SOURCE
#error missing _GNU_SOURCE
#endif
#ifndef __USE_GNU
#error missing __USE_GNU
#endif
#include <unistd.h>
#if !defined(_POSIX_C_SOURCE) || _POSIX_C_SOURCE < 200809L
#error missing POSIX.1-2008 feature level
#endif
#include <pthread.h>
#ifndef _REENTRANT
#error missing _REENTRANT
#endif
int marker;
"#,
    );
    let output = input.sibling("i");
    let result = Command::new(rcc_bin())
        .arg("--linux-gnu-hosted")
        .arg("-pthread")
        .arg("-E")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
}

#[test]
fn glibc_cdefs_coreutils_style_annotations_parse_after_expansion() {
    let input = TempCFile::new(
        "glibc-cdefs-annotations",
        r#"
#include <sys/cdefs.h>

__BEGIN_DECLS
extern int one(const char *) __THROW __nonnull ((1)) __wur;
extern void *two(unsigned long)
  __THROW __attribute_malloc__ __attribute_alloc_size__ ((1));
extern int __NTH(three (int));
__END_DECLS

int main(void) { return 0; }
"#,
    );
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("--linux-gnu-hosted")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "AST output should be emitted after successful parsing");
}

#[test]
fn pthread_header_shim_parses_and_typechecks_for_linux_target() {
    let input = TempCFile::new(
        "pthread-header",
        r#"
#include <pthread.h>

static void *worker(void *arg) {
    return arg;
}

int main(void) {
    pthread_t thread;
    if (pthread_create(&thread, 0, worker, 0) != 0)
        return 1;
    if (pthread_join(thread, 0) != 0)
        return 2;
    return 0;
}
"#,
    );
    let output = input.sibling("hir");
    let result = Command::new(rcc_bin())
        .arg("--target=x86_64-unknown-linux-gnu")
        .arg("--linux-gnu-hosted")
        .arg("-pthread")
        .arg("--emit=hir")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "HIR output should be emitted after pthread header typecheck");
}

#[test]
fn posix_core_type_headers_parse_and_lower_for_linux_target() {
    let input = TempCFile::new(
        "posix-core-types",
        r#"
#include <sys/types.h>
#include <unistd.h>
#include <signal.h>
#include <time.h>

static void on_signal(int signo) {
    (void)signo;
}

ssize_t rw_once(int fd, void *buf, size_t n) {
    off_t off = lseek(fd, (off_t)0, SEEK_SET);
    pid_t pid = getpid();
    uid_t uid = getuid();
    gid_t gid = getgid();
    mode_t mode = (mode_t)0;
    time_t now = time(0);
    clockid_t cid = CLOCK_MONOTONIC;
    struct timespec ts;
    sighandler_t handler = on_signal;
    signal(2, handler);
    clock_gettime(cid, &ts);
    return off == (off_t)-1 || pid == (pid_t)-1 || uid == (uid_t)-1 ||
           gid == (gid_t)-1 || mode == (mode_t)-1 || now == (time_t)-1
        ? (ssize_t)-1
        : write(fd, buf, n);
}
"#,
    );
    let output = input.sibling("hir");
    let result = Command::new(rcc_bin())
        .arg("--target=x86_64-unknown-linux-gnu")
        .arg("--linux-gnu-hosted")
        .arg("--emit=hir")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "HIR output should be emitted after POSIX type lowering");
}

#[test]
fn filesystem_posix_headers_parse_and_typecheck_for_linux_target() {
    let input = TempCFile::new(
        "filesystem-posix",
        r#"
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <dirent.h>
#include <unistd.h>

int inspect_path(const char *path) {
    struct stat st;
    struct timeval tv;
    DIR *dir;
    struct dirent *ent;
    int status = 0;
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOCTTY);
    if (fd < 0)
        return 1;
    if (fstat(fd, &st) != 0)
        return 2;
    if (S_ISREG(st.st_mode) && st.st_size < (off_t)0)
        return 3;
    dir = fdopendir(fd);
    if (dir) {
        ent = readdir(dir);
        if (ent && ent->d_ino == (ino_t)0)
            status = ent->d_name[0] == 0;
        closedir(dir);
    }
    gettimeofday(&tv, 0);
    waitpid((pid_t)-1, &status, WNOHANG);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 0;
}
"#,
    );
    let output = input.sibling("hir");
    let result = Command::new(rcc_bin())
        .arg("--target=x86_64-unknown-linux-gnu")
        .arg("--linux-gnu-hosted")
        .arg("--emit=hir")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "HIR output should be emitted after filesystem header typecheck");
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
fn linux_gnu_hosted_keeps_strict_c99_binary_integer_rejection() {
    let input = TempCFile::new("linux-hosted-strict-binary", "int x = 0b10;\n");
    let output = input.sibling("ast");
    let result = Command::new(rcc_bin())
        .arg("--linux-gnu-hosted")
        .arg("--emit=ast")
        .arg("-o")
        .arg(&output)
        .arg(&input.path)
        .output()
        .expect("run rcc");

    assert!(!result.status.success(), "hosted mode accepted GNU binary literal");
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
