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
    let input = TempCFile::new("gnu-stmt-expr", "int main(void) { return ({ 1; }); }\n");
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
