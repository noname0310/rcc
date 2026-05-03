//! Common driver emit-stage snapshots.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_driver::pipeline;
use rcc_errors::{CaptureEmitter, Handler};
use rcc_session::{EmitKind, Options, Session};

#[macro_use]
mod support;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempOutput {
    path: PathBuf,
}

impl TempOutput {
    fn new(stage: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("rcc-driver-emit-snapshot-{}-{id}-{stage}", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn hello_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.c")
}

fn render_emit_stage(kind: EmitKind) -> String {
    let input = hello_fixture();
    let output = TempOutput::new(stage_name(kind));
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(
        Options { emit: vec![kind], output: Some(output.path.clone()), ..Options::default() },
        handler,
    );

    pipeline::compile(&mut session, &input)
        .unwrap_or_else(|err| panic!("compile {} as {:?}: {err}", input.display(), kind));
    assert!(!session.handler.has_errors(), "unexpected diagnostics: {:?}", cap.diagnostics());
    fs::read_to_string(&output.path)
        .unwrap_or_else(|err| panic!("read {}: {err}", output.path.display()))
}

fn stage_name(kind: EmitKind) -> &'static str {
    match kind {
        EmitKind::Tokens => "tokens",
        EmitKind::Pp => "pp",
        EmitKind::Ast => "ast",
        EmitKind::Hir => "hir",
        EmitKind::Mir => "mir",
        EmitKind::LlvmIr => "ll",
        EmitKind::Asm => "asm",
        EmitKind::Obj => "obj",
    }
}

#[test]
fn common_hello_emit_snapshots() {
    for kind in [EmitKind::Tokens, EmitKind::Pp, EmitKind::Ast, EmitKind::Hir, EmitKind::Mir] {
        assert_emit_snapshot!(stage_name(kind), "common_hello", render_emit_stage(kind));
    }
}
