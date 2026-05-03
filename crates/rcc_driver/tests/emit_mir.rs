//! Golden snapshots for `rcc --emit=mir`.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::{build_bodies, pretty::dump_body};
use rcc_errors::{codes, CaptureEmitter, Handler};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_session::{Options, Session};
use rcc_typeck::{check, verify_typed_hir};

#[macro_use]
mod support;

fn render(src: &str) -> String {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session.source_map.write().unwrap().add_file(PathBuf::from("<mir>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);
    let ast = rcc_parse::parse(&mut session, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut session);
    check(&mut session, &mut tcx, &mut hir);
    verify_typed_hir(&mut session, &tcx, &hir);
    assert!(!session.handler.has_errors(), "unexpected diagnostics: {:?}", cap.diagnostics());
    let bodies = build_bodies(&mut session, &tcx, &hir);

    let mut ids: Vec<_> = bodies.keys().copied().collect();
    ids.sort_by_key(|id| id.0);
    let mut out = String::new();
    for (idx, id) in ids.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&dump_body(&bodies[id], &tcx));
    }
    out
}

fn diagnostics_after_mir_build(src: &str) -> CaptureEmitter {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session.source_map.write().unwrap().add_file(PathBuf::from("<mir>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);
    let ast = rcc_parse::parse(&mut session, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut session);
    check(&mut session, &mut tcx, &mut hir);
    verify_typed_hir(&mut session, &tcx, &hir);
    let _ = build_bodies(&mut session, &tcx, &hir);
    cap
}

#[test]
fn simple_return() {
    assert_emit_snapshot!("mir", "simple_return", render("int main(void) { return 0; }"));
}

#[test]
fn locals_and_expression() {
    assert_emit_snapshot!(
        "mir",
        "locals_and_expression",
        render("int f(int a) { int x = a + 1; return x; }")
    );
}

#[test]
fn if_else_returns() {
    assert_emit_snapshot!(
        "mir",
        "if_else_returns",
        render("int f(int a) { if (a) return 1; else return 2; }")
    );
}

#[test]
fn while_loop() {
    assert_emit_snapshot!(
        "mir",
        "while_loop",
        render("int f(int n) { int i = 0; while (i < n) { i = i + 1; } return i; }")
    );
}

#[test]
fn vla_sizeof() {
    assert_emit_snapshot!(
        "mir",
        "vla_sizeof",
        render("unsigned long f(int n) { int a[n]; return sizeof a; }")
    );
}

#[test]
fn sizeof_type() {
    assert_emit_snapshot!(
        "mir",
        "sizeof_type",
        render("unsigned long f(void) { return sizeof(int); }")
    );
}

#[test]
fn compound_literal_address() {
    assert_emit_snapshot!(
        "mir",
        "compound_literal_address",
        render("int f(void) { int *p = &(int){3}; return *p; }")
    );
}

#[test]
fn switch_from_source() {
    assert_emit_snapshot!(
        "mir",
        "switch_from_source",
        render("int f(int x) { switch (x) { case 1: return 2; default: return 3; } }")
    );
}

#[test]
fn complex_real_to_complex_return() {
    assert_emit_snapshot!(
        "mir",
        "complex_real_to_complex_return",
        render("double _Complex f(double x) { return x; }")
    );
}

#[test]
fn complex_to_real_return() {
    assert_emit_snapshot!(
        "mir",
        "complex_to_real_return",
        render("double f(double _Complex z) { return z; }")
    );
}

#[test]
fn sizeof_incomplete_type_reports_layout_error() {
    let cap =
        diagnostics_after_mir_build("struct S; unsigned long f(void) { return sizeof(struct S); }");
    assert!(
        cap.diagnostics().iter().any(|diag| diag.code == Some(codes::E0085)),
        "expected E0085 for sizeof incomplete record, got {:?}",
        cap.diagnostics()
    );
}
