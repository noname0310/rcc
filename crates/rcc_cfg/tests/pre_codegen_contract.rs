//! Source-to-CFG fixtures that lock the pre-codegen contract.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::{
    build_bodies, verify::verify_body_with_hir, Body, Operand, Projection, Rvalue, StatementKind,
};
use rcc_errors::{CaptureEmitter, Diagnostic, Handler};
use rcc_hir::{DefId, DefKind, GlobalInitValue, HirCrate, ObjectQuals, TyCtxt};
use rcc_hir_lower::lower;
use rcc_session::{Options, Session};
use rcc_typeck::{check, verify_typed_hir};

struct Checked {
    session: Session,
    cap: CaptureEmitter,
    tcx: TyCtxt,
    hir: HirCrate,
}

struct Lowered {
    tcx: TyCtxt,
    hir: HirCrate,
    bodies: Vec<(DefId, Body)>,
}

fn check_snippet(name: &str, src: &str) -> Checked {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session
        .source_map
        .write()
        .unwrap()
        .add_file(PathBuf::from(format!("<pre-codegen/{name}>")), Arc::from(src));

    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);
    let ast = rcc_parse::parse(&mut session, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut session);
    check(&mut session, &mut tcx, &mut hir);
    verify_typed_hir(&mut session, &tcx, &hir);
    Checked { session, cap, tcx, hir }
}

fn lower_checked(name: &str, src: &str) -> Lowered {
    let mut checked = check_snippet(name, src);
    assert!(
        !checked.session.handler.has_errors(),
        "{name}: unexpected diagnostics: {:?}",
        checked.cap.diagnostics()
    );
    let mut bodies: Vec<_> =
        build_bodies(&mut checked.session, &checked.tcx, &checked.hir).into_iter().collect();
    bodies.sort_by_key(|(def, _)| def.0);
    for (_, body) in &bodies {
        verify_body_with_hir(body, &checked.tcx, &checked.hir)
            .unwrap_or_else(|errors| panic!("{name}: CFG verifier errors: {errors:?}"));
    }
    assert!(
        !checked.session.handler.has_errors(),
        "{name}: CFG build emitted diagnostics: {:?}",
        checked.cap.diagnostics()
    );
    Lowered { tcx: checked.tcx, hir: checked.hir, bodies }
}

fn expect_errors(name: &str, src: &str, expected_codes: &[&'static str]) -> Vec<Diagnostic> {
    let checked = check_snippet(name, src);
    let diagnostics = checked.cap.diagnostics();
    assert!(
        checked.session.handler.has_errors(),
        "{name}: expected diagnostics before CFG/codegen"
    );
    for code in expected_codes {
        assert!(
            diagnostics.iter().any(|d| d.code == Some(*code)),
            "{name}: expected diagnostic {code}, got {diagnostics:?}"
        );
    }
    diagnostics
}

#[test]
fn member_access_field_index_survives_to_cfg() {
    let lowered = lower_checked(
        "member-second-field",
        "struct S { int a; int b; }; int f(struct S s) { return s.b; }",
    );
    let body = only_body(&lowered);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Copy(src) | Operand::Move(src)),
                } if place.base.0 == 0
                    && src.projection.iter().any(|p| matches!(p, Projection::Field(1)))
            )
        }),
        "return s.b should store from Projection::Field(1)"
    );
}

#[test]
fn return_coercion_is_explicit_before_codegen() {
    let lowered = lower_checked("return-coercion", "long f(int x) { return x; }");
    let body = only_body(&lowered);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    rvalue: Rvalue::Cast { to, .. },
                    ..
                } if *to == lowered.tcx.long
            )
        }),
        "int-to-long return coercion should be an explicit Cast rvalue"
    );
}

#[test]
fn folded_global_initializer_is_ready_for_codegen() {
    let lowered = lower_checked("global-init-fold", "static int x = 2 + 3;");
    let init = lowered.hir.defs.iter().find_map(|def| match &def.kind {
        DefKind::Global { init: Some(init), .. } => Some(init),
        _ => None,
    });
    let init = init.expect("expected one global initializer");
    assert_eq!(init.entries.len(), 1);
    assert!(matches!(init.entries[0].value, GlobalInitValue::Int(5)));
}

#[test]
fn volatile_local_metadata_survives_to_cfg() {
    let lowered = lower_checked(
        "volatile-metadata",
        "int f(void) { volatile int x; int y = x; x = y; return y; }",
    );
    let body = only_body(&lowered);
    assert!(
        body.locals.iter().any(|decl| decl.quals == volatile_quals()),
        "volatile local qualifier should reach CFG LocalDecl metadata"
    );
}

#[test]
fn invalid_return_stops_before_cfg() {
    let diagnostics = expect_errors(
        "invalid-return",
        "struct A { int x; }; struct B { int x; }; struct A f(struct B b) { return b; }",
        &["E0081"],
    );
    assert!(diagnostics.iter().all(|d| d.code != Some("E0088")));
}

#[test]
fn invalid_pointer_assignment_stops_before_cfg() {
    expect_errors(
        "invalid-pointer-assignment",
        "int f(char *q) { int *p; p = q; return 0; }",
        &["E0082"],
    );
}

#[test]
fn invalid_call_coercion_stops_before_cfg() {
    expect_errors(
        "invalid-call-coercion",
        "int sink(int *p); int f(char *q) { return sink(q); }",
        &["E0082"],
    );
}

#[test]
fn const_assignment_stops_before_cfg() {
    expect_errors(
        "const-assignment",
        "int f(void) { const int x = 0; x = 1; return x; }",
        &["E0080"],
    );
}

fn only_body(lowered: &Lowered) -> &Body {
    assert_eq!(lowered.bodies.len(), 1);
    &lowered.bodies[0].1
}

fn volatile_quals() -> ObjectQuals {
    ObjectQuals { is_const: false, is_volatile: true, is_restrict: false }
}
