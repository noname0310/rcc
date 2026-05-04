//! Source-to-CFG fixtures that lock the pre-codegen contract.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::{
    build_bodies,
    verify::{verify_body_with_hir, CfgErrorKind},
    Body, ConstKind, Operand, Projection, Rvalue, StatementKind,
};
use rcc_errors::{CaptureEmitter, Diagnostic, Handler};
use rcc_hir::{DefId, DefKind, GlobalInitValue, HirCrate, Local, ObjectQuals, Ty, TyCtxt};
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

fn expect_warnings_and_lower(name: &str, src: &str, expected_codes: &[&'static str]) -> Lowered {
    let mut checked = check_snippet(name, src);
    let diagnostics = checked.cap.diagnostics();
    assert!(
        !checked.session.handler.has_errors(),
        "{name}: warnings should not stop CFG/codegen: {diagnostics:?}"
    );
    for code in expected_codes {
        assert!(
            diagnostics.iter().any(|d| d.code == Some(*code)),
            "{name}: expected warning {code}, got {diagnostics:?}"
        );
    }
    let mut bodies: Vec<_> =
        build_bodies(&mut checked.session, &checked.tcx, &checked.hir).into_iter().collect();
    bodies.sort_by_key(|(def, _)| def.0);
    for (_, body) in &bodies {
        verify_body_with_hir(body, &checked.tcx, &checked.hir)
            .unwrap_or_else(|errors| panic!("{name}: CFG verifier errors: {errors:?}"));
    }
    Lowered { tcx: checked.tcx, hir: checked.hir, bodies }
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
fn global_object_read_lowers_to_explicit_load() {
    let lowered =
        lower_checked("global-object-read", "static int x = 5; int f(void) { return x; }");
    let body = only_body(&lowered);
    let x = find_global(&lowered.hir);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    rvalue: Rvalue::LoadGlobal { def, ty },
                    ..
                } if *def == x && *ty == lowered.tcx.int
            )
        }),
        "return x should load the object value from global x"
    );
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).all(|stmt| {
            !matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Const(c)),
                } if place.base.0 == 0 && matches!(c.kind, ConstKind::Global(def) if def == x)
            )
        }),
        "return x must not store the address constant global#x into the return slot"
    );
}

#[test]
fn global_object_address_stays_address_value() {
    let lowered = lower_checked("global-address", "static int x; int *f(void) { return &x; }");
    let body = only_body(&lowered);
    let x = find_global(&lowered.hir);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Const(c)),
                } if place.base.0 == 0 && matches!(c.kind, ConstKind::Global(def) if def == x)
            )
        }),
        "return &x should pass the global address constant through to the pointer return slot"
    );
}

#[test]
fn global_object_assignment_lowers_to_global_place() {
    let lowered = lower_checked("global-object-assign", "int x; int f(void) { x = 7; return x; }");
    let body = only_body(&lowered);
    let x = find_global(&lowered.hir);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Const(c)),
                } if matches!(place.projection.as_slice(), [Projection::Global(def)] if *def == x)
                    && matches!(c.kind, ConstKind::Int(7))
            )
        }),
        "x = 7 should store through a global place, not panic in lower_as_place"
    );
}

#[test]
fn global_object_field_assignment_lowers_to_projected_global_place() {
    let lowered = lower_checked(
        "global-object-field-assign",
        "typedef struct { int x; int y; } S; S v; int f(void) { v.x = 1; return v.x; }",
    );
    let body = only_body(&lowered);
    let v = find_global(&lowered.hir);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Const(c)),
                } if matches!(
                    place.projection.as_slice(),
                    [Projection::Global(def), Projection::Field(0)] if *def == v
                ) && matches!(c.kind, ConstKind::Int(1))
            )
        }),
        "v.x = 1 should store through a projected global place"
    );
}

#[test]
fn deref_of_global_address_loads_object_value() {
    let lowered =
        lower_checked("global-deref-address", "static int x = 3; int f(void) { return *&x; }");
    let body = only_body(&lowered);
    let x = find_global(&lowered.hir);
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    rvalue: Rvalue::Use(Operand::Const(c)),
                    ..
                } if matches!(c.kind, ConstKind::Global(def) if def == x)
            )
        }),
        "*&x should materialize the global address"
    );
    assert!(
        body.blocks.iter().flat_map(|block| &block.statements).any(|stmt| {
            matches!(
                &stmt.kind,
                StatementKind::Assign {
                    place,
                    rvalue: Rvalue::Use(Operand::Copy(src) | Operand::Move(src)),
                } if place.base.0 == 0
                    && src.projection.iter().any(|p| matches!(p, Projection::Deref))
            )
        }),
        "return *&x should copy from a dereferenced address place into the return slot"
    );
}

#[test]
fn function_designator_return_is_not_global_object_load() {
    let lowered = lower_checked(
        "function-designator-return",
        "int f(void) { return 1; } int (*g(void))(void) { return f; }",
    );
    let g_body = lowered
        .bodies
        .iter()
        .find(|(_, body)| body.ret_ty.is_some_and(|ret| matches!(lowered.tcx.get(ret), Ty::Ptr(_))))
        .map(|(_, body)| body)
        .expect("expected function returning a function pointer");
    assert!(
        g_body.blocks.iter().flat_map(|block| &block.statements).all(|stmt| {
            !matches!(&stmt.kind, StatementKind::Assign { rvalue: Rvalue::LoadGlobal { .. }, .. })
        }),
        "returning function designator f must not become a global object load"
    );
}

#[test]
fn verifier_rejects_global_address_stored_as_scalar() {
    let lowered =
        lower_checked("global-address-old-shape", "static int x = 5; int f(void) { return x; }");
    let mut body = only_body(&lowered).clone();
    let x = find_global(&lowered.hir);
    body.blocks[rcc_cfg::BasicBlockId(0)].statements.insert(
        0,
        rcc_cfg::Statement {
            kind: StatementKind::Assign {
                place: rcc_cfg::Place { base: Local(0), projection: Vec::new() },
                rvalue: Rvalue::Use(Operand::Const(rcc_cfg::Const {
                    kind: ConstKind::Global(x),
                    ty: lowered.tcx.int,
                })),
            },
            span: rcc_span::DUMMY_SP,
        },
    );
    let errors = verify_body_with_hir(&body, &lowered.tcx, &lowered.hir).unwrap_err();
    assert!(errors.iter().any(|err| matches!(
        err.kind,
        CfgErrorKind::InvalidGlobalAddressType { def, ty }
            if def == x && ty == lowered.tcx.int
    )));
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
fn incompatible_pointer_assignment_warns_and_reaches_cfg() {
    let lowered = expect_warnings_and_lower(
        "incompatible-pointer-assignment",
        "int f(char *q) { int *p; p = q; return 0; }",
        &["E0082"],
    );
    assert_eq!(lowered.bodies.len(), 1);
}

#[test]
fn incompatible_call_pointer_coercion_warns_and_reaches_cfg() {
    let lowered = expect_warnings_and_lower(
        "incompatible-call-coercion",
        "int sink(int *p); int f(char *q) { return sink(q); }",
        &["E0082"],
    );
    assert_eq!(lowered.bodies.len(), 1);
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

fn find_global(hir: &HirCrate) -> DefId {
    hir.defs
        .iter_enumerated()
        .find_map(|(def, data)| matches!(data.kind, DefKind::Global { .. }).then_some(def))
        .expect("missing global")
}

fn volatile_quals() -> ObjectQuals {
    ObjectQuals { is_const: false, is_volatile: true, is_restrict: false }
}
