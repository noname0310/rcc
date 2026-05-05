use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::{options_from_cli, pipeline, Cli};
use rcc_errors::{codes, CaptureEmitter, Handler};
use rcc_session::Session;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempCFile {
    path: PathBuf,
}

impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcc-driver-warning-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{name}.c"));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
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

fn compile_warning_fixture(args: &[&str]) -> (Session, CaptureEmitter) {
    compile_warning_source("gnu-stmt-expr", "int main(void) { return ({ 1; }); }\n", args)
}

fn compile_warning_source(name: &str, src: &str, args: &[&str]) -> (Session, CaptureEmitter) {
    let input = TempCFile::new(name, src);
    let output = input.path.with_extension("ast");
    let mut argv = vec!["rcc"];
    argv.extend_from_slice(args);
    argv.push("-o");
    argv.push(output.to_str().unwrap());
    argv.push(input.path.to_str().unwrap());
    let cli = parse(&argv);
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(options_from_cli(&cli), handler);
    pipeline::compile(&mut session, &cli.input[0]).expect("compile warning fixture");
    (session, cap)
}

#[test]
fn wall_and_pedantic_flags_are_recorded() {
    let cli = parse(&["rcc", "-Wall", "-Wextra", "-Wpedantic", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.warning_config.wall_enabled());
    assert!(opts.warning_config.extra_enabled());
    assert!(opts.warning_config.pedantic_enabled());
}

#[test]
fn wall_and_extra_enable_documented_warning_names() {
    let wall = options_from_cli(&parse(&["rcc", "-Wall", "hello.c"]));
    assert!(wall.warning_config.warning_enabled("unused-variable"));
    assert!(wall.warning_config.warning_enabled("unused-function"));
    assert!(wall.warning_config.warning_enabled("implicit-function-declaration"));
    assert!(!wall.warning_config.warning_enabled("unused-parameter"));

    let extra = options_from_cli(&parse(&["rcc", "-Wextra", "hello.c"]));
    assert!(extra.warning_config.warning_enabled("unused-variable"));
    assert!(extra.warning_config.warning_enabled("unused-parameter"));
    assert!(extra.warning_config.warning_enabled("sign-compare"));
    assert!(extra.warning_config.warning_enabled("unreachable-code"));
}

#[test]
fn named_warning_controls_override_groups() {
    let opts = options_from_cli(&parse(&["rcc", "-Wall", "-Wno-unused-variable", "hello.c"]));
    assert!(!opts.warning_config.warning_enabled("unused-variable"));
    assert!(opts.warning_config.warning_enabled("unused-function"));

    let opts = options_from_cli(&parse(&["rcc", "-Wunused_parameter", "hello.c"]));
    assert!(opts.warning_config.warning_enabled("unused-parameter"));
}

#[test]
fn named_werror_controls_warning_names() {
    let opts = options_from_cli(&parse(&[
        "rcc",
        "-Werror=unused-variable",
        "-Wno-error=unused-function",
        "hello.c",
    ]));

    assert!(opts.warning_config.named_warning_promoted_to_error("unused-variable"));
    assert!(!opts.warning_config.named_warning_promoted_to_error("unused-function"));
    assert!(!opts.warning_config.named_warning_promoted_to_error("unused-parameter"));
}

#[test]
fn werror_promotes_pipeline_warning_to_error() {
    let (session, cap) = compile_warning_fixture(&["-Werror", "--emit=ast"]);

    assert!(session.handler.has_errors());
    assert_eq!(session.handler.error_count(), 1);
    assert_eq!(session.handler.warning_count(), 0);
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0013));
}

#[test]
fn suppress_all_warnings_drops_pipeline_warning() {
    let (session, cap) = compile_warning_fixture(&["-w", "--emit=ast"]);

    assert!(!session.handler.has_errors());
    assert_eq!(session.handler.warning_count(), 0);
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn named_wno_suppresses_matching_pipeline_warning() {
    let (session, cap) = compile_warning_fixture(&["-Wno-gnu-statement-expression", "--emit=ast"]);

    assert!(!session.handler.has_errors());
    assert_eq!(session.handler.warning_count(), 0);
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn wno_unused_variable_is_stored_as_named_override() {
    let cli = parse(&["rcc", "-Wno-unused-variable", "hello.c"]);
    let opts = options_from_cli(&cli);

    assert!(opts.warning_config.warning_disabled("unused-variable"));
}

#[test]
fn wall_emits_unused_variable_warning() {
    let (session, cap) = compile_warning_source(
        "unused-variable",
        "int main(void) { int x; return 0; }\n",
        &["-Wall", "--emit=hir"],
    );

    assert!(!session.handler.has_errors());
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0026));
    assert!(diags[0].message.contains("[-Wunused-variable]"));
}

#[test]
fn unused_variable_is_quiet_without_group_or_named_flag() {
    let (session, cap) = compile_warning_source(
        "unused-variable-default",
        "int main(void) { int x; return 0; }\n",
        &["--emit=hir"],
    );

    assert!(!session.handler.has_errors());
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn read_or_volatile_local_suppresses_unused_variable_warning() {
    let (_, read_cap) = compile_warning_source(
        "unused-variable-read",
        "int main(void) { int x; return x; }\n",
        &["-Wall", "--emit=hir"],
    );
    assert!(read_cap.diagnostics().is_empty());

    let (_, volatile_cap) = compile_warning_source(
        "unused-variable-volatile",
        "int main(void) { volatile int x; return 0; }\n",
        &["-Wall", "--emit=hir"],
    );
    assert!(volatile_cap.diagnostics().is_empty());
}

#[test]
fn gnu_unused_attribute_suppresses_unused_variable_warning() {
    let (_, cap) = compile_warning_source(
        "unused-variable-gnu-attr",
        "int main(void) { int x __attribute__((unused)); return 0; }\n",
        &["-Wall", "-fgnu-attributes", "--emit=hir"],
    );

    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn writes_only_still_warns_for_unused_variable() {
    let (_, cap) = compile_warning_source(
        "unused-variable-write-only",
        "int main(void) { int x; x = 1; return 0; }\n",
        &["-Wall", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, Some(codes::W0026));
}

#[test]
fn gnu_deprecated_attribute_warns_on_use() {
    let (_, cap) = compile_warning_source(
        "deprecated-gnu-attr",
        "__attribute__((deprecated)) int old_api(void) { return 1; }\nint main(void) { return old_api(); }\n",
        &["-fgnu-attributes", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert_eq!(diags[0].code, Some(codes::W0032));
    assert!(diags[0].message.contains("deprecated declaration `old_api`"));
}

#[test]
fn wno_and_werror_unused_variable_are_honored() {
    let (_, suppressed) = compile_warning_source(
        "unused-variable-wno",
        "int main(void) { int x; return 0; }\n",
        &["-Wall", "-Wno-unused-variable", "--emit=hir"],
    );
    assert!(suppressed.diagnostics().is_empty());

    let (session, promoted) = compile_warning_source(
        "unused-variable-werror",
        "int main(void) { int x; return 0; }\n",
        &["-Werror=unused-variable", "--emit=hir"],
    );
    let diags = promoted.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0026));
}

#[test]
fn wall_emits_unused_static_function_warning() {
    let (_, cap) = compile_warning_source(
        "unused-function",
        "static int helper(void) { return 1; }\nint main(void) { return 0; }\n",
        &["-Wall", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0027));
    assert!(diags[0].message.contains("[-Wunused-function]"));
}

#[test]
fn called_or_external_function_suppresses_unused_function_warning() {
    let (_, called) = compile_warning_source(
        "used-function",
        "static int helper(void) { return 1; }\nint main(void) { return helper(); }\n",
        &["-Wall", "--emit=hir"],
    );
    assert!(called.diagnostics().is_empty());

    let (_, external) = compile_warning_source(
        "external-function",
        "int helper(void) { return 1; }\nint main(void) { return 0; }\n",
        &["-Wall", "--emit=hir"],
    );
    assert!(external.diagnostics().is_empty());
}

#[test]
fn wno_and_werror_unused_function_are_honored() {
    let (_, suppressed) = compile_warning_source(
        "unused-function-wno",
        "static int helper(void) { return 1; }\nint main(void) { return 0; }\n",
        &["-Wall", "-Wno-unused-function", "--emit=hir"],
    );
    assert!(suppressed.diagnostics().is_empty());

    let (session, promoted) = compile_warning_source(
        "unused-function-werror",
        "static int helper(void) { return 1; }\nint main(void) { return 0; }\n",
        &["-Werror=unused-function", "--emit=hir"],
    );
    let diags = promoted.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0027));
}

#[test]
fn wextra_emits_unused_parameter_warning() {
    let (_, cap) = compile_warning_source(
        "unused-parameter",
        "int f(int x) { return 0; }\n",
        &["-Wextra", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0028));
    assert!(diags[0].message.contains("[-Wunused-parameter]"));
}

#[test]
fn wall_alone_does_not_emit_unused_parameter_warning() {
    let (_, cap) = compile_warning_source(
        "unused-parameter-wall",
        "int f(int x) { return 0; }\n",
        &["-Wall", "--emit=hir"],
    );

    assert!(cap.diagnostics().is_empty());
}

#[test]
fn read_or_void_cast_parameter_suppresses_unused_parameter_warning() {
    let (_, read_cap) = compile_warning_source(
        "unused-parameter-read",
        "int f(int x) { return x; }\n",
        &["-Wextra", "--emit=hir"],
    );
    assert!(read_cap.diagnostics().is_empty());

    let (_, void_cast_cap) = compile_warning_source(
        "unused-parameter-void-cast",
        "int f(int x) { (void)x; return 0; }\n",
        &["-Wextra", "--emit=hir"],
    );
    assert!(void_cast_cap.diagnostics().is_empty());
}

#[test]
fn wno_and_werror_unused_parameter_are_honored() {
    let (_, suppressed) = compile_warning_source(
        "unused-parameter-wno",
        "int f(int x) { return 0; }\n",
        &["-Wextra", "-Wno-unused-parameter", "--emit=hir"],
    );
    assert!(suppressed.diagnostics().is_empty());

    let (session, promoted) = compile_warning_source(
        "unused-parameter-werror",
        "int f(int x) { return 0; }\n",
        &["-Werror=unused-parameter", "--emit=hir"],
    );
    let diags = promoted.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0028));
}

#[test]
fn strict_c99_undeclared_call_remains_hard_error() {
    let (session, cap) = compile_warning_source(
        "strict-undeclared-call",
        "int main(void) { return missing(1); }\n",
        &["-Wall", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert!(session.handler.has_errors());
    assert!(diags.iter().any(|d| d.level == rcc_errors::Level::Error));
    assert!(diags.iter().any(|d| d.code == Some(codes::E0071)), "{diags:?}");
    assert!(!diags.iter().any(|d| d.code == Some(codes::W0029)), "{diags:?}");
}

#[test]
fn gnu_implicit_function_declaration_warns_under_wall() {
    let (session, cap) = compile_warning_source(
        "gnu-implicit-call",
        "int main(void) { return missing(1, 2.0f); }\n",
        &["-fgnu-implicit-function-declaration", "-Wall", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert!(!session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0029));
    assert!(diags[0].message.contains("[-Wimplicit-function-declaration]"));
}

#[test]
fn gnu_implicit_function_declaration_can_be_enabled_by_name() {
    let (session, cap) = compile_warning_source(
        "gnu-implicit-call-named",
        "int main(void) { return missing(); }\n",
        &["-fgnu-implicit-function-declaration", "-Wimplicit-function-declaration", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert!(!session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, Some(codes::W0029));
}

#[test]
fn wno_suppresses_gnu_implicit_function_declaration_warning() {
    let (session, cap) = compile_warning_source(
        "gnu-implicit-call-wno",
        "int main(void) { return missing(); }\n",
        &[
            "-fgnu-implicit-function-declaration",
            "-Wall",
            "-Wno-implicit-function-declaration",
            "--emit=hir",
        ],
    );

    assert!(!session.handler.has_errors());
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn werror_promotes_gnu_implicit_function_declaration_warning() {
    let (session, cap) = compile_warning_source(
        "gnu-implicit-call-werror",
        "int main(void) { return missing(); }\n",
        &[
            "-fgnu-implicit-function-declaration",
            "-Werror=implicit-function-declaration",
            "--emit=hir",
        ],
    );

    let diags = cap.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0029));
}

#[test]
fn wextra_emits_sign_compare_warning() {
    let (_, cap) = compile_warning_source(
        "sign-compare",
        "int main(void) { int i = -1; unsigned u = 1; return i < u; }\n",
        &["-Wextra", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0030));
    assert!(diags[0].message.contains("[-Wsign-compare]"));
}

#[test]
fn named_sign_compare_warning_can_be_enabled_without_wextra() {
    let (_, cap) = compile_warning_source(
        "sign-compare-named",
        "int main(void) { int i = -1; unsigned u = 1; return i == u; }\n",
        &["-Wsign-compare", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, Some(codes::W0030));
}

#[test]
fn same_signed_or_explicitly_cast_comparisons_do_not_warn() {
    let (_, same_signed) = compile_warning_source(
        "sign-compare-same-signed",
        "int main(void) { int i = -1; int j = 1; return i < j; }\n",
        &["-Wextra", "--emit=hir"],
    );
    assert!(same_signed.diagnostics().is_empty());

    let (_, explicit_cast) = compile_warning_source(
        "sign-compare-cast",
        "int main(void) { int i = -1; unsigned u = 1; return (unsigned)i < u; }\n",
        &["-Wextra", "--emit=hir"],
    );
    assert!(explicit_cast.diagnostics().is_empty());
}

#[test]
fn sign_compare_suppression_and_promotion_are_honored() {
    let (_, suppressed) = compile_warning_source(
        "sign-compare-wno",
        "int main(void) { int i = -1; unsigned u = 1; return i < u; }\n",
        &["-Wextra", "-Wno-sign-compare", "--emit=hir"],
    );
    assert!(suppressed.diagnostics().is_empty());

    let (session, promoted) = compile_warning_source(
        "sign-compare-werror",
        "int main(void) { int i = -1; unsigned u = 1; return i < u; }\n",
        &["-Werror=sign-compare", "--emit=hir"],
    );
    let diags = promoted.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0030));
}

#[test]
fn wextra_emits_unreachable_code_warning_after_return() {
    let (_, cap) = compile_warning_source(
        "unreachable-return",
        "int x;\nint main(void) { return 0; x = 1; }\n",
        &["-Wextra", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0031));
    assert!(diags[0].message.contains("[-Wunreachable-code]"));
}

#[test]
fn unreachable_detector_does_not_cross_if_branches() {
    let (_, cap) = compile_warning_source(
        "unreachable-if-branch",
        "int x;\nint main(int n) { if (n) return 0; else x = 1; return x; }\n",
        &["-Wextra", "--emit=hir"],
    );

    assert!(cap.diagnostics().is_empty());
}

#[test]
fn preprocessor_disabled_dead_code_is_not_seen_by_unreachable_detector() {
    let (_, cap) = compile_warning_source(
        "unreachable-pp-disabled",
        "int x;\nint main(void) { return 0;\n#if 0\nx = 1;\n#endif\n}\n",
        &["-Wextra", "--emit=hir"],
    );

    assert!(cap.diagnostics().is_empty());
}

#[test]
fn unreachable_code_suppression_and_promotion_are_honored() {
    let (_, suppressed) = compile_warning_source(
        "unreachable-wno",
        "int x;\nint main(void) { return 0; x = 1; }\n",
        &["-Wextra", "-Wno-unreachable-code", "--emit=hir"],
    );
    assert!(suppressed.diagnostics().is_empty());

    let (session, promoted) = compile_warning_source(
        "unreachable-werror",
        "int x;\nint main(void) { goto done; x = 1; done: return x; }\n",
        &["-Werror=unreachable-code", "--emit=hir"],
    );
    let diags = promoted.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0031));
}

#[test]
fn diagnostic_pragma_ignored_suppresses_later_warning() {
    let (_, cap) = compile_warning_source(
        "pragma-ignored",
        "#pragma GCC diagnostic ignored \"-Wunused-variable\"\nint main(void) { int x; return 0; }\n",
        &["-Wall", "--emit=hir"],
    );

    assert!(cap.diagnostics().is_empty());
}

#[test]
fn diagnostic_pragma_push_pop_restores_warning_policy() {
    let (_, cap) = compile_warning_source(
        "pragma-push-pop",
        concat!(
            "#pragma GCC diagnostic ignored \"-Wunused-variable\"\n",
            "#pragma GCC diagnostic push\n",
            "#pragma GCC diagnostic warning \"-Wunused-variable\"\n",
            "int first(void) { int x; return 0; }\n",
            "#pragma GCC diagnostic pop\n",
            "int second(void) { int y; return 0; }\n",
        ),
        &["-Wall", "--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0026));
}

#[test]
fn diagnostic_pragma_error_promotes_later_warning() {
    let (session, cap) = compile_warning_source(
        "pragma-error",
        "#pragma GCC diagnostic error \"-Wunused-variable\"\nint main(void) { int x; return 0; }\n",
        &["--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert!(session.handler.has_errors());
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0026));
}

#[test]
fn malformed_diagnostic_pragma_emits_stable_warning() {
    let (_, cap) = compile_warning_source(
        "pragma-malformed",
        "#pragma GCC diagnostic ignored unused-variable\nint main(void) { return 0; }\n",
        &["--emit=hir"],
    );

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].level, rcc_errors::Level::Warning);
    assert_eq!(diags[0].code, Some(codes::W0001));
    assert!(diags[0].message.contains("malformed #pragma GCC diagnostic"));
}
