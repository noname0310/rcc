//! Integration tests for `rcc_typeck` (task 07-11).
//!
//! Centralised truth tables and diagnostic fixtures for everything the
//! type checker exposes. The tests are split into thematic blocks:
//!
//! 1. `usual_arithmetic` — C99 §6.3.1.8 truth table across the 13 scalar
//!    types, exercising every rule (steps 1-3 floats, 4a same-type,
//!    4b same-signedness, 4c.i/4c.ii signed/unsigned mixing).
//! 2. `integer_promotion` — C99 §6.3.1.1 promotion of every sub-`int`
//!    integer rank (signed and unsigned) plus bitfield-width variants.
//! 3. `is_assignable` — C99 §6.5.16.1 simple-assignment matrix:
//!    arithmetic ↔ arithmetic, null pointer constant, void* ↔ T*,
//!    qualifier additions, struct compatibility.
//! 4. `is_compatible_type` — interning-backed type compatibility.
//! 5. `is_null_pointer_constant` — recognition through Cast/Convert wrappers.
//! 6. `pointer_convert` — C99 §6.3.2.3 pointer conversion outcomes.
//! 7. `value_category` — every `HirExprKind` arm's lvalue/rvalue mapping.
//! 8. `decay_if_needed` / `lvalue_to_rvalue_if_needed` — implicit
//!    conversion insertion primitives.
//! 9. Diagnostic fixtures — E0080..E0084 and W0008..W0011 covered via
//!    inline tests that build a hand-crafted `Body` and inspect the
//!    captured diagnostics.

use rcc_data_structures::IndexVec;
use rcc_errors::codes;
use rcc_hir::rcc_hir_binop::{BinOp, UnOp};
use rcc_hir::{
    Body, ConvertKind, Def, DefId, DefKind, FloatKind, HirCrate, HirExpr, HirExprId, HirExprKind,
    HirStmt, HirStmtId, HirStmtKind, IntRank, Linkage, Local, LocalDecl, Qual, Ty, TyCtxt, TyId,
    ValueCat,
};
use rcc_session::{Options, Session};
use rcc_span::{Symbol, DUMMY_SP};
use rcc_typeck::const_eval::{ConstEval, ConstScalar, ConstValue};
use rcc_typeck::{
    check_assignment_lhs, check_body, check_body_with_defs, check_init_const, decay_if_needed,
    integer_promotion, is_assignable, is_compatible_type, is_const_init_expr,
    is_null_pointer_constant, lvalue_to_rvalue_if_needed, pointer_convert, usual_arithmetic,
    value_category, verify_typed_hir, AssignError, ConvertError, DecayContext, DefSnapshot,
    FieldSnapshot,
};

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Push a leaf `IntConst(0)` expression with the given type and category.
/// The kind itself is incidental; helpers that classify by *type* (decay,
/// lvalue-to-rvalue) only inspect `ty` and `value_cat`.
fn push_leaf(body: &mut Body, ty: TyId, cat: ValueCat) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty,
        value_cat: cat,
        span: DUMMY_SP,
        kind: HirExprKind::IntConst(0),
    });
    body.exprs[id].id = id;
    id
}

/// Push an expression with the given kind and type. Default category is
/// `RValue` — `value_category` derives the right answer from `kind` so
/// the stored category is intentionally a sentinel.
fn push_kind(body: &mut Body, ty: TyId, kind: HirExprKind) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty,
        value_cat: ValueCat::RValue,
        span: DUMMY_SP,
        kind,
    });
    body.exprs[id].id = id;
    id
}

fn push_local(body: &mut Body, ty: TyId) -> Local {
    body.locals.push(LocalDecl {
        name: None,
        ty,
        quals: rcc_hir::ObjectQuals::none(),
        vla_len: None,
        is_param: false,
        span: DUMMY_SP,
    })
}

fn push_local_with_quals(body: &mut Body, ty: TyId, quals: rcc_hir::ObjectQuals) -> Local {
    body.locals.push(LocalDecl {
        name: None,
        ty,
        quals,
        vla_len: None,
        is_param: false,
        span: DUMMY_SP,
    })
}

fn push_local_ref(body: &mut Body, ty: TyId) -> HirExprId {
    let local = push_local(body, ty);
    push_kind(body, ty, HirExprKind::LocalRef(local))
}

/// Wrap `expr_id` as the root statement of `body`.
fn root_stmt(body: &mut Body, expr_id: HirExprId) {
    let stmt_id = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::Expr(expr_id),
    });
    body.stmts[stmt_id].id = stmt_id;
    body.root = Some(stmt_id);
}

fn push_null_stmt(body: &mut Body) -> HirStmtId {
    let stmt_id =
        body.stmts.push(HirStmt { id: HirStmtId(0), span: DUMMY_SP, kind: HirStmtKind::Null });
    body.stmts[stmt_id].id = stmt_id;
    stmt_id
}

fn ptr_to(tcx: &mut TyCtxt, pointee: TyId) -> TyId {
    tcx.intern(Ty::Ptr(Qual::plain(pointee)))
}

fn function_ty(tcx: &mut TyCtxt, ret: TyId, params: Vec<TyId>) -> TyId {
    tcx.intern(Ty::Func { ret, params, variadic: false, proto: true })
}

fn function_snapshot(name: Symbol, ty: TyId) -> DefSnapshot {
    DefSnapshot {
        name,
        ty: Some(ty),
        value_cat: ValueCat::LValue,
        enumerator_value: None,
        object_quals: rcc_hir::ObjectQuals::none(),
        record_fields: None,
    }
}

fn unwrap_callee_def(body: &Body, expr: HirExprId) -> Option<DefId> {
    match body.exprs[expr].kind {
        HirExprKind::DefRef(def) => Some(def),
        HirExprKind::Convert { operand, .. } => unwrap_callee_def(body, operand),
        _ => None,
    }
}

fn hir_with_function_body(tcx: &mut TyCtxt, body: Body) -> HirCrate {
    let ret = tcx.int;
    let fn_ty = tcx.intern(Ty::Func { ret, params: Vec::new(), variadic: false, proto: true });
    let mut hir = HirCrate::default();
    let def_id = hir.defs.push(Def {
        id: DefId(0),
        name: Symbol(0),
        span: DUMMY_SP,
        kind: DefKind::Function {
            ty: fn_ty,
            has_body: true,
            is_static: false,
            is_inline: false,
            is_extern_inline: false,
            no_instrument_function: false,
            variadic: false,
        },
    });
    hir.defs[def_id].id = def_id;
    hir.bodies.insert(def_id, body);
    hir
}

#[test]
fn tgmath_sqrt_dispatches_by_real_and_complex_argument_type() {
    let mut tcx = TyCtxt::new();
    let (mut session, cap) = Session::for_test();
    let sqrt = session.interner.intern("sqrt");
    let sqrtf = session.interner.intern("sqrtf");
    let sqrtl = session.interner.intern("sqrtl");
    let csqrt = session.interner.intern("csqrt");

    let sqrt_def = DefId(10);
    let sqrtf_def = DefId(11);
    let sqrtl_def = DefId(12);
    let csqrt_def = DefId(13);
    let mut def_info = rcc_data_structures::FxHashMap::default();
    let double = tcx.double;
    let float = tcx.float;
    let long_double = tcx.long_double;
    let complex_double = tcx.complex_double;
    def_info.insert(sqrt_def, function_snapshot(sqrt, function_ty(&mut tcx, double, vec![double])));
    def_info.insert(sqrtf_def, function_snapshot(sqrtf, function_ty(&mut tcx, float, vec![float])));
    def_info.insert(
        sqrtl_def,
        function_snapshot(sqrtl, function_ty(&mut tcx, long_double, vec![long_double])),
    );
    def_info.insert(
        csqrt_def,
        function_snapshot(csqrt, function_ty(&mut tcx, complex_double, vec![complex_double])),
    );

    for (arg_ty, expected_def) in [
        (float, sqrtf_def),
        (double, sqrt_def),
        (long_double, sqrtl_def),
        (complex_double, csqrt_def),
    ] {
        let mut body = Body::default();
        let arg = push_local_ref(&mut body, arg_ty);
        let call = push_kind(
            &mut body,
            tcx.error,
            HirExprKind::BuiltinTgmath { name: sqrt, args: vec![arg] },
        );
        root_stmt(&mut body, call);

        check_body_with_defs(&mut body, &mut tcx, &mut session, &def_info);

        let HirExprKind::Call { callee, .. } = body.exprs[call].kind else {
            panic!("tgmath builtin should rewrite to a call, got {:?}", body.exprs[call].kind);
        };
        assert_eq!(unwrap_callee_def(&body, callee), Some(expected_def));
    }
    assert!(cap.diagnostics().is_empty());
}

fn const_ptr_to(tcx: &mut TyCtxt, pointee: TyId) -> TyId {
    tcx.intern(Ty::Ptr(Qual {
        ty: pointee,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }))
}

fn record(tcx: &mut TyCtxt, def: u32) -> TyId {
    tcx.intern(Ty::Record(DefId(def)))
}

/// Convenience: pointer-to-int via a temporary capture of `tcx.int` so the
/// borrow checker doesn't see overlapping borrows.
fn ptr_to_int(tcx: &mut TyCtxt) -> TyId {
    let int = tcx.int;
    ptr_to(tcx, int)
}

fn ptr_to_char(tcx: &mut TyCtxt) -> TyId {
    let c = tcx.char_;
    ptr_to(tcx, c)
}

fn ptr_to_void(tcx: &mut TyCtxt) -> TyId {
    let v = tcx.void;
    ptr_to(tcx, v)
}

fn ptr_to_float(tcx: &mut TyCtxt) -> TyId {
    let f = tcx.float;
    ptr_to(tcx, f)
}

fn const_ptr_to_int(tcx: &mut TyCtxt) -> TyId {
    let int = tcx.int;
    const_ptr_to(tcx, int)
}

// ─── 1. usual_arithmetic — C99 §6.3.1.8 ──────────────────────────────────

/// Cover every step of C99 §6.3.1.8 across the 13 scalar types. The table
/// is symmetric so each row is verified twice (lhs,rhs) and (rhs,lhs).
#[test]
fn usual_arithmetic_truth_table() {
    let tcx = TyCtxt::new();

    let cases: &[(&str, TyId, TyId, TyId)] = &[
        // Step 1: long double dominates everything.
        ("ld / ld", tcx.long_double, tcx.long_double, tcx.long_double),
        ("ld / d", tcx.long_double, tcx.double, tcx.long_double),
        ("ld / f", tcx.long_double, tcx.float, tcx.long_double),
        ("ld / int", tcx.long_double, tcx.int, tcx.long_double),
        ("ld / ull", tcx.long_double, tcx.ulong_long, tcx.long_double),
        ("ld / _Bool", tcx.long_double, tcx.bool_, tcx.long_double),
        // Step 2: double beats float and integer.
        ("d / d", tcx.double, tcx.double, tcx.double),
        ("d / f", tcx.double, tcx.float, tcx.double),
        ("d / ull", tcx.double, tcx.ulong_long, tcx.double),
        ("d / char", tcx.double, tcx.char_, tcx.double),
        // Step 3: float beats integer.
        ("f / f", tcx.float, tcx.float, tcx.float),
        ("f / ll", tcx.float, tcx.long_long, tcx.float),
        ("f / uint", tcx.float, tcx.uint, tcx.float),
        ("f / _Bool", tcx.float, tcx.bool_, tcx.float),
        // Step 4a: integer promotion brings both to same type.
        ("_Bool / _Bool", tcx.bool_, tcx.bool_, tcx.int),
        ("char / char", tcx.char_, tcx.char_, tcx.int),
        ("schar / schar", tcx.schar, tcx.schar, tcx.int),
        ("uchar / uchar", tcx.uchar, tcx.uchar, tcx.int),
        ("short / short", tcx.short, tcx.short, tcx.int),
        ("ushort / ushort", tcx.ushort, tcx.ushort, tcx.int),
        ("char / short", tcx.char_, tcx.short, tcx.int),
        ("_Bool / ushort", tcx.bool_, tcx.ushort, tcx.int),
        ("int / int", tcx.int, tcx.int, tcx.int),
        ("uint / uint", tcx.uint, tcx.uint, tcx.uint),
        ("long / long", tcx.long, tcx.long, tcx.long),
        ("ulong / ulong", tcx.ulong, tcx.ulong, tcx.ulong),
        ("ll / ll", tcx.long_long, tcx.long_long, tcx.long_long),
        ("ull / ull", tcx.ulong_long, tcx.ulong_long, tcx.ulong_long),
        // Step 4b: same signedness, different rank.
        ("int / long", tcx.int, tcx.long, tcx.long),
        ("int / ll", tcx.int, tcx.long_long, tcx.long_long),
        ("long / ll", tcx.long, tcx.long_long, tcx.long_long),
        ("uint / ulong", tcx.uint, tcx.ulong, tcx.ulong),
        ("ulong / ull", tcx.ulong, tcx.ulong_long, tcx.ulong_long),
        ("uint / ull", tcx.uint, tcx.ulong_long, tcx.ulong_long),
        // Step 4c.i: equal rank, mixed signedness → unsigned wins.
        ("int / uint", tcx.int, tcx.uint, tcx.uint),
        ("long / ulong", tcx.long, tcx.ulong, tcx.ulong),
        ("ll / ull", tcx.long_long, tcx.ulong_long, tcx.ulong_long),
        // Step 4c.i: unsigned rank > signed rank → unsigned wins.
        ("int / ulong", tcx.int, tcx.ulong, tcx.ulong),
        ("int / ull", tcx.int, tcx.ulong_long, tcx.ulong_long),
        ("long / ull", tcx.long, tcx.ulong_long, tcx.ulong_long),
        // Step 4c.ii: signed rank > unsigned rank, signed represents all
        // values of unsigned → signed wins (LP64).
        ("long / uint", tcx.long, tcx.uint, tcx.long),
        ("ll / uint", tcx.long_long, tcx.uint, tcx.long_long),
        // Sub-int operands promote to int/uint first, then re-enter step 4.
        ("long / ushort", tcx.long, tcx.ushort, tcx.long),
        ("ll / ushort", tcx.long_long, tcx.ushort, tcx.long_long),
        ("long / char", tcx.long, tcx.char_, tcx.long),
        ("long / _Bool", tcx.long, tcx.bool_, tcx.long),
        // Sub-int signed/unsigned mixes: promote to int/uint.
        ("char / uint", tcx.char_, tcx.uint, tcx.uint),
        ("short / uint", tcx.short, tcx.uint, tcx.uint),
        ("ushort / int", tcx.ushort, tcx.int, tcx.int),
        ("uchar / int", tcx.uchar, tcx.int, tcx.int),
        ("_Bool / int", tcx.bool_, tcx.int, tcx.int),
        ("_Bool / uint", tcx.bool_, tcx.uint, tcx.uint),
    ];

    for (desc, a, b, expected) in cases {
        assert_eq!(usual_arithmetic(&tcx, *a, *b), *expected, "(a,b): {desc}");
        assert_eq!(usual_arithmetic(&tcx, *b, *a), *expected, "(b,a): {desc} (symmetry)");
    }
}

// ─── 2. integer_promotion — C99 §6.3.1.1 ─────────────────────────────────

#[test]
fn integer_promotion_table_non_bitfield() {
    let tcx = TyCtxt::new();
    // Sub-int integer ranks promote to int (signed) or int (unsigned, on
    // a 32-bit-int target every value of unsigned char/short fits in int).
    assert_eq!(integer_promotion(&tcx, tcx.bool_, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.char_, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.schar, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.uchar, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.short, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.ushort, None), tcx.int);
    // Int and wider pass through unchanged.
    assert_eq!(integer_promotion(&tcx, tcx.int, None), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.uint, None), tcx.uint);
    assert_eq!(integer_promotion(&tcx, tcx.long, None), tcx.long);
    assert_eq!(integer_promotion(&tcx, tcx.ulong, None), tcx.ulong);
    assert_eq!(integer_promotion(&tcx, tcx.long_long, None), tcx.long_long);
    assert_eq!(integer_promotion(&tcx, tcx.ulong_long, None), tcx.ulong_long);
}

#[test]
fn integer_promotion_passes_through_non_integers() {
    let tcx = TyCtxt::new();
    for ty in [tcx.void, tcx.float, tcx.double, tcx.long_double, tcx.error] {
        assert_eq!(integer_promotion(&tcx, ty, None), ty);
    }
}

#[test]
fn integer_promotion_bitfield_widths() {
    let tcx = TyCtxt::new();
    // Unsigned bitfield of width < 32 fits in signed int → int.
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(1)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(3)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(16)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(31)), tcx.int);
    // Width 32 unsigned bitfield must be unsigned int.
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(32)), tcx.uint);
    // Signed bitfield of width <= 32 fits in int.
    assert_eq!(integer_promotion(&tcx, tcx.int, Some(1)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.int, Some(15)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.int, Some(32)), tcx.int);
    // Storage-rank governs signedness, not the natural rank.
    assert_eq!(integer_promotion(&tcx, tcx.uchar, Some(4)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.schar, Some(4)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.ushort, Some(16)), tcx.int);
    assert_eq!(integer_promotion(&tcx, tcx.bool_, Some(1)), tcx.int);
    // Width 0 sentinel maps to int for safety.
    assert_eq!(integer_promotion(&tcx, tcx.uint, Some(0)), tcx.int);
}

// ─── 3. is_assignable — C99 §6.5.16.1 ────────────────────────────────────

#[test]
fn assignable_arith_same_type_ok() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_leaf(&mut body, tcx.int, ValueCat::RValue);
    assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.int, src), Ok(()));
    assert_eq!(is_assignable(&tcx, &body, tcx.double, tcx.double, src), Ok(()));
}

#[test]
fn assignable_arith_widening_ok() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_leaf(&mut body, tcx.char_, ValueCat::RValue);
    assert_eq!(is_assignable(&tcx, &body, tcx.long, tcx.char_, src), Ok(()));
    assert_eq!(is_assignable(&tcx, &body, tcx.double, tcx.float, src), Ok(()));
    assert_eq!(is_assignable(&tcx, &body, tcx.long, tcx.uint, src), Ok(()));
}

#[test]
fn assignable_arith_narrowing_classified() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_leaf(&mut body, tcx.double, ValueCat::RValue);
    // double → int loses fractional part.
    assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.double, src), Err(AssignError::Narrowing));
    // long → int truncates.
    assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.long, src), Err(AssignError::Narrowing));
    // signed → unsigned of same width loses negatives.
    assert_eq!(is_assignable(&tcx, &body, tcx.uint, tcx.int, src), Err(AssignError::Narrowing));
    // double → float drops mantissa bits.
    assert_eq!(is_assignable(&tcx, &body, tcx.float, tcx.double, src), Err(AssignError::Narrowing));
}

#[test]
fn assignable_null_pointer_constant_ok() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    assert_eq!(is_assignable(&tcx, &body, int_ptr, tcx.int, zero), Ok(()));
}

#[test]
fn assignable_void_ptr_object_ptr_ok() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let void_ptr = ptr_to_void(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(is_assignable(&tcx, &body, void_ptr, int_ptr, src), Ok(()));
    assert_eq!(is_assignable(&tcx, &body, int_ptr, void_ptr, src), Ok(()));
}

#[test]
fn assignable_qualifier_addition_ok() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let const_int_ptr = const_ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    // int* → const int* widens qualifiers — accepted.
    assert_eq!(is_assignable(&tcx, &body, const_int_ptr, int_ptr, src), Ok(()));
}

#[test]
fn assignable_qualifier_loss_classified() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let const_int_ptr = const_ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));
    // const int* → int* drops `const` — qualifier loss.
    assert_eq!(
        is_assignable(&tcx, &body, int_ptr, const_int_ptr, src),
        Err(AssignError::QualifierLoss),
    );
}

#[test]
fn assignable_unrelated_pointers_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let float_ptr = ptr_to_float(&mut tcx);
    let src = push_kind(&mut body, float_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(is_assignable(&tcx, &body, int_ptr, float_ptr, src), Err(AssignError::Incompatible),);
}

#[test]
fn assignable_records_same_defid_ok_different_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rec_a = record(&mut tcx, 0);
    let rec_b = record(&mut tcx, 1);
    let src = push_kind(&mut body, rec_a, HirExprKind::LocalRef(Local(0)));
    // struct A = struct A — accepted.
    assert_eq!(is_assignable(&tcx, &body, rec_a, rec_a, src), Ok(()));
    // struct B = struct A — incompatible (different DefIds).
    assert_eq!(is_assignable(&tcx, &body, rec_b, rec_a, src), Err(AssignError::Incompatible));
}

#[test]
fn assignable_bool_from_pointer_ok() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(is_assignable(&tcx, &body, tcx.bool_, int_ptr, src), Ok(()));
}

#[test]
fn assignable_nonzero_integer_to_pointer_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    assert_eq!(is_assignable(&tcx, &body, int_ptr, tcx.int, one), Err(AssignError::Incompatible));
}

// ─── 4. is_compatible_type ────────────────────────────────────────────────

#[test]
fn compatible_type_identity_and_inequality() {
    let mut tcx = TyCtxt::new();
    assert!(is_compatible_type(&tcx, tcx.int, tcx.int));
    assert!(!is_compatible_type(&tcx, tcx.int, tcx.uint));
    let p1 = ptr_to_int(&mut tcx);
    let p2 = ptr_to_int(&mut tcx);
    // Interning yields the same id for structurally-equal types.
    assert_eq!(p1, p2);
    assert!(is_compatible_type(&tcx, p1, p2));
}

// ─── 5. is_null_pointer_constant ─────────────────────────────────────────

#[test]
fn null_pointer_constant_recognises_zero_through_wrappers() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    // Bare `0`.
    let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    assert!(is_null_pointer_constant(&body, zero));
    // `(void *)0`.
    let void_ptr = ptr_to_void(&mut tcx);
    let cast = push_kind(&mut body, void_ptr, HirExprKind::Cast { operand: zero, to: void_ptr });
    assert!(is_null_pointer_constant(&body, cast));
    // Wrapped in a Convert as well.
    let wrapped = push_kind(
        &mut body,
        void_ptr,
        HirExprKind::Convert { operand: cast, kind: ConvertKind::Pointer },
    );
    assert!(is_null_pointer_constant(&body, wrapped));
    // Non-zero literal is not.
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    assert!(!is_null_pointer_constant(&body, one));
}

#[test]
fn switch_condition_undergoes_integer_promotion() {
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();

    let cond = push_kind(&mut body, tcx.schar, HirExprKind::IntConst(-1));
    let case_body = push_null_stmt(&mut body);
    let switch_stmt = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::Switch {
            cond,
            body: case_body,
            cases: vec![rcc_hir::SwitchCase { value: Some(255), target: case_body }],
        },
    });
    body.stmts[switch_stmt].id = switch_stmt;
    body.root = Some(switch_stmt);

    check_body(&mut body, &mut tcx, &mut sess);

    let HirStmtKind::Switch { cond: promoted, cases, .. } = &body.stmts[switch_stmt].kind else {
        panic!("expected switch root");
    };
    assert_eq!(body.exprs[*promoted].ty, tcx.int);
    assert_eq!(cases[0].value, Some(255), "case value remains in promoted int domain");
    match body.exprs[*promoted].kind {
        HirExprKind::Convert { operand, kind: ConvertKind::IntegerPromotion } => {
            assert_eq!(operand, cond);
        }
        ref other => panic!("expected IntegerPromotion wrapper, got {other:?}"),
    }
}

#[test]
fn subscript_index_undergoes_integer_promotion() {
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();

    let ptr_ty = ptr_to_int(&mut tcx);
    let p_local = push_local(&mut body, ptr_ty);
    let i_local = push_local(&mut body, tcx.uchar);
    let base = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(p_local));
    let index = push_kind(&mut body, tcx.uchar, HirExprKind::LocalRef(i_local));
    let subscript = push_kind(&mut body, tcx.int, HirExprKind::Index { base, index });
    root_stmt(&mut body, subscript);

    check_body(&mut body, &mut tcx, &mut sess);

    let HirExprKind::Index { index: promoted, .. } = body.exprs[subscript].kind else {
        panic!("expected subscript expression");
    };
    assert_eq!(body.exprs[promoted].ty, tcx.int);
    match body.exprs[promoted].kind {
        HirExprKind::Convert { operand, kind: ConvertKind::IntegerPromotion } => {
            assert_eq!(body.exprs[operand].ty, tcx.uchar);
            assert!(
                matches!(
                    body.exprs[operand].kind,
                    HirExprKind::Convert {
                        operand: original,
                        kind: ConvertKind::LvalueToRvalue,
                    } if original == index
                ),
                "promotion should wrap the lvalue-to-rvalue index"
            );
        }
        ref other => panic!("expected IntegerPromotion wrapper, got {other:?}"),
    }
}

// ─── 6. pointer_convert — C99 §6.3.2.3 ───────────────────────────────────

#[test]
fn pointer_convert_object_to_void_ok() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let void_ptr = ptr_to_void(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    let new_id = pointer_convert(&mut tcx, &mut body, src, void_ptr).expect("ok");
    assert_ne!(new_id, src, "wrapper inserted");
    assert_eq!(body.exprs[new_id].ty, void_ptr);
}

#[test]
fn pointer_convert_unrelated_pointers_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let char_ptr = ptr_to_char(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, src, char_ptr),
        Err(ConvertError::Incompatible),
    );
}

#[test]
fn pointer_convert_qualifier_loss_classified() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let const_int_ptr = const_ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));
    // const int* → int* drops `const`.
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, src, int_ptr),
        Err(ConvertError::QualifierLoss),
    );
}

#[test]
fn pointer_convert_npc_widens() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let new_id = pointer_convert(&mut tcx, &mut body, zero, int_ptr).expect("npc accepted");
    assert_ne!(new_id, zero);
    assert_eq!(body.exprs[new_id].ty, int_ptr);
}

#[test]
fn pointer_convert_integer_pointer_mix_classified() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let seven = push_kind(&mut body, tcx.int, HirExprKind::IntConst(7));
    // Non-zero integer to pointer — implementation-defined, requires cast.
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, seven, int_ptr),
        Err(ConvertError::IntegerPointerMix),
    );
}

#[test]
fn pointer_convert_function_object_pointer_mix_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let func_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
    let func_ptr = ptr_to(&mut tcx, func_ty);
    let void_ptr = ptr_to_void(&mut tcx);
    let src = push_kind(&mut body, func_ptr, HirExprKind::LocalRef(Local(0)));
    // Function pointer can't be assigned to void* (C99 §6.3.2.3p8).
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, src, void_ptr),
        Err(ConvertError::Incompatible),
    );
}

// ─── 7. value_category — every HirExprKind arm ───────────────────────────

#[test]
fn value_category_constants_are_rvalues() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let i = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let f = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(0.0));
    assert_eq!(value_category(&body, i), ValueCat::RValue);
    assert_eq!(value_category(&body, f), ValueCat::RValue);
}

#[test]
fn value_category_designators_are_lvalues() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let local = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let def = push_kind(&mut body, tcx.int, HirExprKind::DefRef(DefId(0)));
    let arr_ty =
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
    let s = push_kind(&mut body, arr_ty, HirExprKind::StringRef(DefId(0)));
    let int_ptr = ptr_to_int(&mut tcx);
    let ptr = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(1)));
    let deref = push_kind(&mut body, tcx.int, HirExprKind::Deref(ptr));
    let idx = push_kind(&mut body, tcx.int, HirExprKind::Index { base: ptr, index: local });
    assert_eq!(value_category(&body, local), ValueCat::LValue);
    assert_eq!(value_category(&body, def), ValueCat::LValue);
    assert_eq!(value_category(&body, s), ValueCat::LValue);
    assert_eq!(value_category(&body, deref), ValueCat::LValue);
    assert_eq!(value_category(&body, idx), ValueCat::LValue);
}

#[test]
fn value_category_operators_are_rvalues() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let bin = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });
    let un = push_kind(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::Neg, operand: l });
    let call = push_kind(&mut body, tcx.int, HirExprKind::Call { callee: l, args: Vec::new() });
    let cast = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand: l, to: tcx.int });
    let addr = push_kind(&mut body, tcx.int, HirExprKind::AddressOf(l));
    let cond =
        push_kind(&mut body, tcx.int, HirExprKind::Cond { cond: l, then_expr: l, else_expr: r });
    let comma = push_kind(&mut body, tcx.int, HirExprKind::Comma { lhs: l, rhs: r });
    let assign = push_kind(&mut body, tcx.int, HirExprKind::Assign { lhs: l, rhs: r });
    for id in [bin, un, call, cast, addr, cond, comma, assign] {
        assert_eq!(value_category(&body, id), ValueCat::RValue, "{id:?}");
    }
}

#[test]
fn value_category_field_inherits_from_base() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    // s.f where s is an lvalue → field is lvalue.
    let s_lvalue = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let f_lvalue =
        push_kind(&mut body, tcx.int, HirExprKind::Field { base: s_lvalue, field_index: 0 });
    assert_eq!(value_category(&body, f_lvalue), ValueCat::LValue);
    // s.f where s is an rvalue → field is rvalue.
    let s_rvalue = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let f_rvalue =
        push_kind(&mut body, tcx.int, HirExprKind::Field { base: s_rvalue, field_index: 0 });
    assert_eq!(value_category(&body, f_rvalue), ValueCat::RValue);
}

// ─── 8. decay_if_needed / lvalue_to_rvalue_if_needed ─────────────────────

#[test]
fn decay_array_to_ptr_normal_context() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let arr_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(10), is_vla: false });
    let arr = push_leaf(&mut body, arr_ty, ValueCat::LValue);
    let decayed = decay_if_needed(&mut tcx, &mut body, arr, DecayContext::Normal);
    assert_ne!(decayed, arr);
    let HirExprKind::Convert { kind, .. } = body.exprs[decayed].kind else {
        panic!("expected Convert wrapper")
    };
    assert_eq!(kind, ConvertKind::ArrayToPtr);
}

#[test]
fn decay_function_to_ptr_normal_context() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
    let f = push_leaf(&mut body, fn_ty, ValueCat::LValue);
    let decayed = decay_if_needed(&mut tcx, &mut body, f, DecayContext::Normal);
    assert_ne!(decayed, f);
    let HirExprKind::Convert { kind, .. } = body.exprs[decayed].kind else {
        panic!("expected Convert wrapper")
    };
    assert_eq!(kind, ConvertKind::FuncToPtr);
}

#[test]
fn decay_skipped_in_sizeof_and_addrof_and_init_contexts() {
    let mut tcx = TyCtxt::new();
    let arr_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
    for ctx in [
        DecayContext::SizeofOperand,
        DecayContext::AddrOfOperand,
        DecayContext::CharArrayInitializer,
    ] {
        let mut body = Body::default();
        let arr = push_leaf(&mut body, arr_ty, ValueCat::LValue);
        let result = decay_if_needed(&mut tcx, &mut body, arr, ctx);
        assert_eq!(result, arr, "no decay in {ctx:?}");
    }
}

#[test]
fn lvalue_to_rvalue_inserts_wrapper_for_lvalue_scalar() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let lv = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let rv = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, lv);
    assert_ne!(rv, lv);
    let HirExprKind::Convert { kind, .. } = body.exprs[rv].kind else { panic!() };
    assert_eq!(kind, ConvertKind::LvalueToRvalue);
    assert_eq!(body.exprs[rv].value_cat, ValueCat::RValue);
}

#[test]
fn lvalue_to_rvalue_unwraps_atomic_object_type() {
    let mut tcx = TyCtxt::new();
    let atomic_int = tcx.intern(Ty::Atomic(tcx.int));
    let mut body = Body::default();
    let lv = push_kind(&mut body, atomic_int, HirExprKind::LocalRef(Local(0)));
    let rv = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, lv);
    assert_ne!(rv, lv);
    assert_eq!(body.exprs[rv].ty, tcx.int);
    let HirExprKind::Convert { kind, .. } = body.exprs[rv].kind else { panic!() };
    assert_eq!(kind, ConvertKind::LvalueToRvalue);
}

#[test]
fn atomic_wrapped_arithmetic_is_assignment_compatible_with_inner_type() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let atomic_int = tcx.intern(Ty::Atomic(tcx.int));
    assert_eq!(is_assignable(&tcx, &body, atomic_int, tcx.int, src), Ok(()));
    assert!(is_compatible_type(&tcx, atomic_int, tcx.int));
}

#[test]
fn lvalue_to_rvalue_passthrough_for_rvalue() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rv = push_kind(&mut body, tcx.int, HirExprKind::IntConst(7));
    assert_eq!(value_category(&body, rv), ValueCat::RValue);
    let result = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, rv);
    assert_eq!(result, rv);
}

// ─── 9a. E0080 — assignment to rvalue ─────────────────────────────────────

#[test]
fn fixture_e0080_int_literal_lhs() {
    // `1 = x;` — int literal as the LHS of an assignment is an rvalue.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let (mut session, cap) = Session::for_test();
    assert!(!check_assignment_lhs(&mut session, &body, lhs));
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, Some(codes::E0080));
}

#[test]
fn fixture_e0080_cast_lhs() {
    // `(int)x = 1;` — the result of a cast is an rvalue.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let lhs = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand: inner, to: tcx.int });
    let (mut session, cap) = Session::for_test();
    assert!(!check_assignment_lhs(&mut session, &body, lhs));
    assert_eq!(cap.diagnostics()[0].code, Some(codes::E0080));
}

#[test]
fn fixture_e0080_binary_lhs() {
    // `(a + b) = 1;` — binary result is an rvalue.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let lhs = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });
    let (mut session, cap) = Session::for_test();
    assert!(!check_assignment_lhs(&mut session, &body, lhs));
    assert_eq!(cap.diagnostics()[0].code, Some(codes::E0080));
}

#[test]
fn assignment_to_const_local_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let local = push_local_with_quals(
        &mut body,
        tcx.int,
        rcc_hir::ObjectQuals { is_const: true, is_volatile: false, is_restrict: false },
    );
    let lhs = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(local));
    let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_const_global_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let def_id = DefId(4);
    let mut def_info = rcc_data_structures::FxHashMap::default();
    def_info.insert(
        def_id,
        DefSnapshot {
            name: Symbol(0),
            ty: Some(tcx.int),
            value_cat: ValueCat::LValue,
            enumerator_value: None,
            object_quals: rcc_hir::ObjectQuals {
                is_const: true,
                is_volatile: false,
                is_restrict: false,
            },
            record_fields: None,
        },
    );
    let lhs = push_kind(&mut body, tcx.int, HirExprKind::DefRef(def_id));
    let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body_with_defs(&mut body, &mut tcx, &mut session, &def_info);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_const_field_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let record_def = DefId(7);
    let rec_ty = record(&mut tcx, record_def.0);
    let mut def_info = rcc_data_structures::FxHashMap::default();
    def_info.insert(
        record_def,
        DefSnapshot {
            name: Symbol(0),
            ty: None,
            value_cat: ValueCat::RValue,
            enumerator_value: None,
            object_quals: rcc_hir::ObjectQuals::none(),
            record_fields: Some(vec![FieldSnapshot {
                name: Some(Symbol(1)),
                ty: tcx.int,
                bit_width: None,
                ms_bitfields: false,
                quals: rcc_hir::ObjectQuals {
                    is_const: true,
                    is_volatile: false,
                    is_restrict: false,
                },
            }]),
        },
    );
    let base = push_local_ref(&mut body, rec_ty);
    let lhs = push_kind(&mut body, tcx.error, HirExprKind::Field { base, field_index: 0 });
    let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body_with_defs(&mut body, &mut tcx, &mut session, &def_info);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn member_access_on_aggregate_rvalue_is_typed_as_rvalue_field() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let record_def = DefId(7);
    let rec_ty = record(&mut tcx, record_def.0);
    let fn_def = DefId(8);
    let fn_ty =
        tcx.intern(Ty::Func { ret: rec_ty, params: Vec::new(), variadic: false, proto: true });

    let mut def_info = rcc_data_structures::FxHashMap::default();
    def_info.insert(
        record_def,
        DefSnapshot {
            name: Symbol(0),
            ty: None,
            value_cat: ValueCat::RValue,
            enumerator_value: None,
            object_quals: rcc_hir::ObjectQuals::none(),
            record_fields: Some(vec![FieldSnapshot {
                name: Some(Symbol(1)),
                ty: tcx.int,
                bit_width: None,
                ms_bitfields: false,
                quals: rcc_hir::ObjectQuals::none(),
            }]),
        },
    );
    def_info.insert(
        fn_def,
        DefSnapshot {
            name: Symbol(0),
            ty: Some(fn_ty),
            value_cat: ValueCat::LValue,
            enumerator_value: None,
            object_quals: rcc_hir::ObjectQuals::none(),
            record_fields: None,
        },
    );

    let callee = push_kind(&mut body, fn_ty, HirExprKind::DefRef(fn_def));
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: Vec::new() });
    let field = push_kind(
        &mut body,
        tcx.error,
        HirExprKind::UnresolvedField { base: call, field: Symbol(1), field_span: DUMMY_SP },
    );
    root_stmt(&mut body, field);

    let (mut session, cap) = Session::for_test();
    check_body_with_defs(&mut body, &mut tcx, &mut session, &def_info);

    assert!(cap.diagnostics().is_empty());
    assert_eq!(body.exprs[field].ty, tcx.int);
    assert_eq!(body.exprs[field].value_cat, ValueCat::RValue);
    assert!(matches!(body.exprs[field].kind, HirExprKind::Field { field_index: 0, .. }));
}

#[test]
fn assignment_through_pointer_to_const_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ty = tcx.int;
    let ptr_to_const_int = tcx.intern(Ty::Ptr(Qual {
        ty: int_ty,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let ptr = push_local_ref(&mut body, ptr_to_const_int);
    let lhs = push_kind(&mut body, tcx.error, HirExprKind::Deref(ptr));
    let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_pointer_to_const_pointer_local_is_modifiable() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let char_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: tcx.char_,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let ptr_to_const_char_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: char_ptr,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let local =
        push_local_with_quals(&mut body, ptr_to_const_char_ptr, rcc_hir::ObjectQuals::none());
    let rhs_local = push_local(&mut body, ptr_to_const_char_ptr);
    let lhs = push_kind(&mut body, ptr_to_const_char_ptr, HirExprKind::LocalRef(local));
    let rhs = push_kind(&mut body, ptr_to_const_char_ptr, HirExprKind::LocalRef(rhs_local));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(
        !cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)),
        "assigning the pointer object itself must remain modifiable: {:?}",
        cap.diagnostics()
    );
}

#[test]
fn assignment_through_pointer_to_const_pointer_local_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let char_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: tcx.char_,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let ptr_to_const_char_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: char_ptr,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let ptr = push_local_ref(&mut body, ptr_to_const_char_ptr);
    let lhs = push_kind(&mut body, char_ptr, HirExprKind::Deref(ptr));
    let rhs = push_kind(&mut body, char_ptr, HirExprKind::IntConst(0));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_const_pointer_object_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let ptr_ty = ptr_to_int(&mut tcx);
    let local = push_local_with_quals(
        &mut body,
        ptr_ty,
        rcc_hir::ObjectQuals { is_const: true, is_volatile: false, is_restrict: false },
    );
    let lhs = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(local));
    let rhs = push_kind(&mut body, ptr_ty, HirExprKind::IntConst(0));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_pointer_to_const_local_is_modifiable() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let ptr_ty = const_ptr_to_int(&mut tcx);
    let local = push_local_with_quals(&mut body, ptr_ty, rcc_hir::ObjectQuals::none());
    let lhs = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(local));
    let rhs = push_kind(&mut body, ptr_ty, HirExprKind::IntConst(0));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(
        !cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)),
        "assigning a pointer-to-const object must be allowed: {:?}",
        cap.diagnostics()
    );
}

#[test]
fn increment_const_local_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let local = push_local_with_quals(
        &mut body,
        tcx.int,
        rcc_hir::ObjectQuals { is_const: true, is_volatile: false, is_restrict: false },
    );
    let operand = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(local));
    let inc = push_kind(&mut body, tcx.error, HirExprKind::Unary { op: UnOp::PreInc, operand });
    root_stmt(&mut body, inc);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn assignment_to_array_object_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let arr_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
    let lhs = push_local_ref(&mut body, arr_ty);
    let rhs = push_kind(&mut body, arr_ty, HirExprKind::StringRef(DefId(10)));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    root_stmt(&mut body, assign);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

#[test]
fn const_array_initializer_store_does_not_emit_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let arr_ty =
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(3), is_vla: false });
    let local = push_local_with_quals(
        &mut body,
        arr_ty,
        rcc_hir::ObjectQuals { is_const: true, is_volatile: false, is_restrict: false },
    );
    let decl = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::LocalDecl { local, init: None },
    });
    body.stmts[decl].id = decl;

    let base = push_kind(&mut body, arr_ty, HirExprKind::LocalRef(local));
    let index = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let lhs = push_kind(&mut body, tcx.char_, HirExprKind::Index { base, index });
    let rhs = push_kind(&mut body, tcx.char_, HirExprKind::IntConst(b'h' as i128));
    let init = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::InitAssign { lhs, rhs },
    });
    body.stmts[init].id = init;
    let block = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::Block(vec![decl, init]),
    });
    body.stmts[block].id = block;
    body.root = Some(block);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(
        !cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)),
        "initializer stores into const arrays must not be checked as assignments: {:?}",
        cap.diagnostics()
    );
}

#[test]
fn ordinary_assignment_to_const_array_element_still_emits_e0080() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let elem = Qual { ty: tcx.char_, is_const: true, is_volatile: false, is_restrict: false };
    let arr_ty = tcx.intern(Ty::Array { elem, len: Some(3), is_vla: false });
    let local = push_local(&mut body, arr_ty);
    let decl = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::LocalDecl { local, init: None },
    });
    body.stmts[decl].id = decl;

    let base = push_kind(&mut body, arr_ty, HirExprKind::LocalRef(local));
    let index = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let lhs = push_kind(&mut body, tcx.char_, HirExprKind::Index { base, index });
    let rhs = push_kind(&mut body, tcx.char_, HirExprKind::IntConst(b'x' as i128));
    let assign = push_kind(&mut body, tcx.error, HirExprKind::Assign { lhs, rhs });
    let assign_stmt = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::Expr(assign),
    });
    body.stmts[assign_stmt].id = assign_stmt;
    let block = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::Block(vec![decl, assign_stmt]),
    });
    body.stmts[block].id = block;
    body.root = Some(block);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0080)));
}

// ─── 9b. E0081 / E0082 — surfaced via is_assignable / pointer_convert ─────

// E0081 (incompatible types in assignment) and E0082 (incompatible
// pointer conversion) are emitted by the surface coercion helper as of
// 07-15. The fixtures below still pin the lower-level classifier
// outcomes so the diagnostic path keeps using the right reason.

#[test]
fn fixture_e0081_struct_a_assigned_struct_b() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rec_a = record(&mut tcx, 0);
    let rec_b = record(&mut tcx, 1);
    let src = push_kind(&mut body, rec_a, HirExprKind::LocalRef(Local(0)));
    assert_eq!(is_assignable(&tcx, &body, rec_b, rec_a, src), Err(AssignError::Incompatible));
}

#[test]
fn fixture_e0081_char_pointer_assigned_int() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let char_ptr = ptr_to_char(&mut tcx);
    let two = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    assert_eq!(is_assignable(&tcx, &body, char_ptr, tcx.int, two), Err(AssignError::Incompatible),);
}

#[test]
fn fixture_e0082_int_ptr_to_char_ptr_incompatible() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let char_ptr = ptr_to_char(&mut tcx);
    let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, src, char_ptr),
        Err(ConvertError::Incompatible),
    );
}

#[test]
fn fixture_e0082_drop_const_is_qualifier_loss() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let const_int_ptr = const_ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(
        pointer_convert(&mut tcx, &mut body, src, int_ptr),
        Err(ConvertError::QualifierLoss),
    );
}

#[test]
fn fixture_e0082_function_object_pointer_mix() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let func_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
    let func_ptr = ptr_to(&mut tcx, func_ty);
    let int_ptr = ptr_to_int(&mut tcx);
    let src = push_kind(&mut body, func_ptr, HirExprKind::LocalRef(Local(0)));
    assert_eq!(pointer_convert(&mut tcx, &mut body, src, int_ptr), Err(ConvertError::Incompatible),);
}

// ─── 9c. E0083 — invalid operands to a binary operator ───────────────────

#[test]
fn fixture_e0083_bitand_on_float() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let r = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
    let bin =
        push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::BitAnd, lhs: l, rhs: r });
    root_stmt(&mut body, bin);
    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    let diags = cap.diagnostics();
    assert!(diags.iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn fixture_e0083_rem_on_floats() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let l = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(1.0));
    let r = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
    let bin =
        push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::Rem, lhs: l, rhs: r });
    root_stmt(&mut body, bin);
    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn fixture_e0083_shift_with_float_count() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let r = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
    let bin =
        push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::Shl, lhs: l, rhs: r });
    root_stmt(&mut body, bin);
    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn fixture_e0083_if_condition_rejects_record() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rec_ty = record(&mut tcx, 0);
    let cond = push_local_ref(&mut body, rec_ty);
    let then_branch = push_null_stmt(&mut body);
    let if_stmt = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span: DUMMY_SP,
        kind: HirStmtKind::If { cond, then_branch, else_branch: None },
    });
    body.stmts[if_stmt].id = if_stmt;
    body.root = Some(if_stmt);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn fixture_e0083_logical_operator_rejects_record_operand() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rec_ty = record(&mut tcx, 0);
    let lhs = push_local_ref(&mut body, rec_ty);
    let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::LogAnd, lhs, rhs });
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn conditional_pointer_null_arm_converts_to_pointer_type() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let then_expr = push_local_ref(&mut body, int_ptr);
    let else_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Cond { cond, then_expr, else_expr });
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().is_empty());
    assert_eq!(body.exprs[expr].ty, int_ptr);
    let HirExprKind::Cond { else_expr, .. } = body.exprs[expr].kind else {
        panic!("expected conditional expression");
    };
    assert!(matches!(
        body.exprs[else_expr].kind,
        HirExprKind::Convert { kind: ConvertKind::Pointer, .. }
    ));
}

#[test]
fn conditional_void_arms_yield_void() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let two = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let then_expr =
        push_kind(&mut body, tcx.void, HirExprKind::Cast { operand: one, to: tcx.void });
    let else_expr =
        push_kind(&mut body, tcx.void, HirExprKind::Cast { operand: two, to: tcx.void });
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Cond { cond, then_expr, else_expr });
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().is_empty());
    assert_eq!(body.exprs[expr].ty, tcx.void);
}

#[test]
fn gnu_conditional_one_void_arm_yields_void_with_warning() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let then_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let else_expr =
        push_kind(&mut body, tcx.void, HirExprKind::Cast { operand: one, to: tcx.void });
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Cond { cond, then_expr, else_expr });
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert_eq!(body.exprs[expr].ty, tcx.void);
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "strict mode should emit W0018, got {diags:?}");
    assert_eq!(diags[0].code, Some(codes::W0018));
}

#[test]
fn gnu_conditional_one_void_arm_flag_suppresses_warning() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let then_expr =
        push_kind(&mut body, tcx.void, HirExprKind::Cast { operand: one, to: tcx.void });
    let else_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(3));
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Cond { cond, then_expr, else_expr });
    root_stmt(&mut body, expr);

    let cap = rcc_errors::CaptureEmitter::new();
    let opts = Options { gnu_conditional_void_operand: true, ..Options::default() };
    let handler = rcc_errors::Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(opts, handler);
    check_body(&mut body, &mut tcx, &mut session);

    assert_eq!(body.exprs[expr].ty, tcx.void);
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn conditional_incompatible_pointer_arms_emit_e0083() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let int_ptr = ptr_to_int(&mut tcx);
    let float_ptr = ptr_to_float(&mut tcx);
    let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let then_expr = push_local_ref(&mut body, int_ptr);
    let else_expr = push_local_ref(&mut body, float_ptr);
    let expr = push_kind(&mut body, tcx.error, HirExprKind::Cond { cond, then_expr, else_expr });
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert_eq!(body.exprs[expr].ty, tcx.error);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn call_prototype_converts_fixed_argument() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: vec![tcx.long], variadic: false, proto: true });
    let callee = push_local_ref(&mut body, fn_ty);
    let arg = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: vec![arg] });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().is_empty());
    let HirExprKind::Call { args, .. } = &body.exprs[call].kind else {
        panic!("expected call");
    };
    assert_eq!(body.exprs[args[0]].ty, tcx.long);
    assert!(matches!(
        body.exprs[args[0]].kind,
        HirExprKind::Convert { kind: ConvertKind::UsualArithmetic, .. }
    ));
}

#[test]
fn call_prototype_too_many_arguments_emits_e0083() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: vec![tcx.int], variadic: false, proto: true });
    let callee = push_local_ref(&mut body, fn_ty);
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let b = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: vec![a, b] });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn call_prototype_too_few_arguments_emits_e0083() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: vec![tcx.int], variadic: false, proto: true });
    let callee = push_local_ref(&mut body, fn_ty);
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: Vec::new() });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn call_variadic_trailing_char_is_promoted_to_int() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let char_ptr = ptr_to_char(&mut tcx);
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: vec![char_ptr], variadic: true, proto: true });
    let callee = push_local_ref(&mut body, fn_ty);
    let fmt = push_local_ref(&mut body, char_ptr);
    let ch = push_local_ref(&mut body, tcx.char_);
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: vec![fmt, ch] });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().is_empty());
    let HirExprKind::Call { args, .. } = &body.exprs[call].kind else {
        panic!("expected call");
    };
    assert_eq!(body.exprs[args[1]].ty, tcx.int);
    assert!(matches!(
        body.exprs[args[1]].kind,
        HirExprKind::Convert { kind: ConvertKind::IntegerPromotion, .. }
    ));
}

#[test]
fn call_unprototyped_float_promotes_to_double() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let fn_ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: false });
    let callee = push_local_ref(&mut body, fn_ty);
    let arg = push_kind(&mut body, tcx.float, HirExprKind::FloatConst(1.0));
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: vec![arg] });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert!(cap.diagnostics().is_empty());
    let HirExprKind::Call { args, .. } = &body.exprs[call].kind else {
        panic!("expected call");
    };
    assert_eq!(body.exprs[args[0]].ty, tcx.double);
}

#[test]
fn call_non_function_callee_emits_e0083() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let callee = push_local_ref(&mut body, tcx.int);
    let call = push_kind(&mut body, tcx.error, HirExprKind::Call { callee, args: Vec::new() });
    root_stmt(&mut body, call);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);

    assert_eq!(body.exprs[call].ty, tcx.error);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0083)));
}

#[test]
fn verify_typed_hir_accepts_clean_checked_body() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let expr = push_kind(&mut body, tcx.error, HirExprKind::IntConst(1));
    root_stmt(&mut body, expr);

    let (mut session, cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    let hir = hir_with_function_body(&mut tcx, body);

    assert!(verify_typed_hir(&mut session, &tcx, &hir));
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn verify_typed_hir_reports_silent_error_type() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let expr = push_kind(&mut body, tcx.error, HirExprKind::IntConst(1));
    root_stmt(&mut body, expr);
    let hir = hir_with_function_body(&mut tcx, body);

    let (mut session, cap) = Session::for_test();
    assert!(!verify_typed_hir(&mut session, &tcx, &hir));
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0088)));
}

#[test]
fn verify_typed_hir_reports_unresolved_field_placeholder() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let rec = record(&mut tcx, 0);
    let base = push_local_ref(&mut body, rec);
    let expr = push_kind(
        &mut body,
        tcx.int,
        HirExprKind::UnresolvedField { base, field: Symbol(1), field_span: DUMMY_SP },
    );
    root_stmt(&mut body, expr);
    let hir = hir_with_function_body(&mut tcx, body);

    let (mut session, cap) = Session::for_test();
    assert!(!verify_typed_hir(&mut session, &tcx, &hir));
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::E0088)));
}

// ─── 9d. E0084 — non-constant in static init ─────────────────────────────

#[test]
fn fixture_e0084_call_in_static_init() {
    // `static int x = foo();` — call expressions are not constant.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let callee = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let call = push_kind(&mut body, tcx.int, HirExprKind::Call { callee, args: Vec::new() });
    assert!(!is_const_init_expr(&body, call, None, &tcx));
    let (mut session, cap) = Session::for_test();
    let ok = check_init_const(&body, &[call], None, &tcx, &mut session);
    assert!(!ok);
    assert_eq!(cap.diagnostics()[0].code, Some(codes::E0084));
}

#[test]
fn fixture_e0084_local_ref_in_static_init() {
    // `static int x = local;` — local objects are not constant.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let lref = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let (mut session, cap) = Session::for_test();
    let ok = check_init_const(&body, &[lref], None, &tcx, &mut session);
    assert!(!ok);
    assert_eq!(cap.diagnostics()[0].code, Some(codes::E0084));
}

#[test]
fn fixture_e0084_constant_init_silent() {
    // `static int x = 2 + 3;` — accepted silently.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let two = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let three = push_kind(&mut body, tcx.int, HirExprKind::IntConst(3));
    let add =
        push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: two, rhs: three });
    assert!(is_const_init_expr(&body, add, None, &tcx));
    let (mut session, cap) = Session::for_test();
    assert!(check_init_const(&body, &[add], None, &tcx, &mut session));
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn fixture_e0084_address_of_global_silent() {
    // `static int *p = &g;` — address constant, accepted silently.
    let mut tcx = TyCtxt::new();
    let ptr_ty = ptr_to_int(&mut tcx);
    let mut body = Body::default();
    let mut defs: IndexVec<DefId, Def> = IndexVec::new();
    let g = defs.push(Def {
        id: DefId(0),
        name: rcc_span::Symbol(0),
        span: DUMMY_SP,
        kind: DefKind::Global {
            ty: tcx.int,
            quals: rcc_hir::ObjectQuals::none(),
            thread_local: false,
            linkage: Linkage::External,
            init: None,
        },
    });
    defs[g].id = g;
    let dref = push_kind(&mut body, tcx.int, HirExprKind::DefRef(g));
    let addr = push_kind(&mut body, ptr_ty, HirExprKind::AddressOf(dref));
    assert!(is_const_init_expr(&body, addr, Some(&defs), &tcx));
}

// ─── 9e. W0008 — narrowing conversion ────────────────────────────────────

// W0008 is not yet emitted by the surface checker (07-07 defers to 07-11);
// is_assignable already classifies the cases. Pin them down so the future
// emit path has a stable contract to wire up.

#[test]
fn fixture_w0008_double_to_int_narrowing() {
    // `int x = 1.5;` would emit W0008 at the Convert site.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(1.5));
    assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.double, src), Err(AssignError::Narrowing));
}

#[test]
fn fixture_w0008_int_to_char_narrowing() {
    // `char b = 300;` — int → char of 300 narrows.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let src = push_kind(&mut body, tcx.int, HirExprKind::IntConst(300));
    assert_eq!(is_assignable(&tcx, &body, tcx.char_, tcx.int, src), Err(AssignError::Narrowing));
}

// ─── 9f. W0009 — overflow in const expr ──────────────────────────────────

#[test]
fn fixture_w0009_int_overflow() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let lhs = push_kind(&mut body, tcx.long_long, HirExprKind::IntConst(i128::MAX));
    let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let add =
        push_kind(&mut body, tcx.long_long, HirExprKind::Binary { op: BinOp::Add, lhs, rhs: one });
    let (mut session, cap) = Session::for_test();
    let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
    assert_eq!(ce.eval_int(add), None);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::W0009)));
}

// ─── 9g. W0010 — division by zero ────────────────────────────────────────

#[test]
fn fixture_w0010_division_by_zero() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let b = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
    let div = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Div, lhs: a, rhs: b });
    let (mut session, cap) = Session::for_test();
    let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
    assert_eq!(ce.eval_int(div), None);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::W0010)));
}

// ─── 9h. W0011 — shift count out of range ────────────────────────────────

#[test]
fn fixture_w0011_shift_too_far() {
    // `1 << 200`: shift count exceeds the i128 evaluator's maximum.
    // The evaluator works in i128, so any count >= 128 trips W0011.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let b = push_kind(&mut body, tcx.int, HirExprKind::IntConst(200));
    let shl = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Shl, lhs: a, rhs: b });
    let (mut session, cap) = Session::for_test();
    let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
    assert_eq!(ce.eval_int(shl), None);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::W0011)));
}

#[test]
fn fixture_w0011_negative_shift_count() {
    // `1 << -1` — negative shift is also out of range.
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
    let b = push_kind(&mut body, tcx.int, HirExprKind::IntConst(-1));
    let shl = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Shl, lhs: a, rhs: b });
    let (mut session, cap) = Session::for_test();
    let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
    assert_eq!(ce.eval_int(shl), None);
    assert!(cap.diagnostics().iter().any(|d| d.code == Some(codes::W0011)));
}

// ─── 10. ConstEval — additional smoke checks ────────────────────────────

#[test]
fn const_eval_smoke_arithmetic() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
    let b = push_kind(&mut body, tcx.int, HirExprKind::IntConst(3));
    let add = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: a, rhs: b });
    let mul = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Mul, lhs: a, rhs: b });
    let mut ce = ConstEval::new(&tcx, Some(&body));
    assert_eq!(ce.eval_int(add), Some(5));
    assert_eq!(ce.eval_int(mul), Some(6));
}

#[test]
fn const_eval_eval_returns_constvalue_int() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let lit = push_kind(&mut body, tcx.int, HirExprKind::IntConst(42));
    let mut ce = ConstEval::new(&tcx, Some(&body));
    assert_eq!(ce.eval(lit), Some(ConstValue::Int(42)));
}

#[test]
fn const_eval_eval_scalar_integer_path() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let a = push_kind(&mut body, tcx.int, HirExprKind::IntConst(7));
    let mut ce = ConstEval::new(&tcx, Some(&body));
    match ce.eval_scalar(a) {
        Some(ConstScalar::Int(v)) => assert_eq!(v, 7),
        other => panic!("expected ConstScalar::Int, got {other:?}"),
    }
}

#[test]
fn const_eval_legacy_eval_returns_float_for_floatconst() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let f = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(3.5));
    let mut ce = ConstEval::new(&tcx, Some(&body));
    match ce.eval(f) {
        Some(ConstValue::Float(v)) => assert!((v - 3.5).abs() < f64::EPSILON),
        other => panic!("expected ConstValue::Float, got {other:?}"),
    }
}

#[test]
fn const_eval_localref_is_not_constant() {
    let tcx = TyCtxt::new();
    let mut body = Body::default();
    let lr = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
    let mut ce = ConstEval::new(&tcx, Some(&body));
    assert_eq!(ce.eval_int(lr), None);
    assert_eq!(ce.eval_arith(lr), None);
}

// ─── 11. check_body — implicit conversion insertion smoke tests ──────────

#[test]
fn check_body_int_plus_double_inserts_convert() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let lhs = push_kind(&mut body, tcx.error, HirExprKind::IntConst(1));
    let rhs = push_kind(&mut body, tcx.error, HirExprKind::FloatConst(2.0));
    let bin = push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::Add, lhs, rhs });
    root_stmt(&mut body, bin);
    let (mut session, _cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    // Common type: double.
    assert_eq!(body.exprs[bin].ty, tcx.double);
    let HirExprKind::Binary { lhs: nlhs, rhs: nrhs, .. } = body.exprs[bin].kind.clone() else {
        panic!()
    };
    // The int side is wrapped in a Convert with destination type double.
    match body.exprs[nlhs].kind {
        HirExprKind::Convert { .. } => assert_eq!(body.exprs[nlhs].ty, tcx.double),
        ref other => panic!("expected Convert wrapper, got {other:?}"),
    }
    // The double side is unchanged.
    assert_eq!(nrhs, rhs);
    assert!(!session.handler.has_errors());
}

#[test]
fn check_body_int_plus_int_no_wrapper() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let lhs = push_kind(&mut body, tcx.error, HirExprKind::IntConst(1));
    let rhs = push_kind(&mut body, tcx.error, HirExprKind::IntConst(2));
    let bin = push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::Add, lhs, rhs });
    root_stmt(&mut body, bin);
    let (mut session, _cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert_eq!(body.exprs[bin].ty, tcx.int);
    let HirExprKind::Binary { lhs: nlhs, rhs: nrhs, .. } = body.exprs[bin].kind.clone() else {
        panic!()
    };
    assert_eq!(nlhs, lhs);
    assert_eq!(nrhs, rhs);
}

#[test]
fn check_body_comparison_yields_int() {
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let lhs = push_kind(&mut body, tcx.error, HirExprKind::IntConst(1));
    let rhs = push_kind(&mut body, tcx.error, HirExprKind::FloatConst(2.0));
    let bin = push_kind(&mut body, tcx.error, HirExprKind::Binary { op: BinOp::Lt, lhs, rhs });
    root_stmt(&mut body, bin);
    let (mut session, _cap) = Session::for_test();
    check_body(&mut body, &mut tcx, &mut session);
    assert_eq!(body.exprs[bin].ty, tcx.int);
}

// ─── 12. Sanity: FloatKind / IntRank coverage ────────────────────────────

#[test]
fn floatkind_and_intrank_are_distinguishable() {
    // The `Ty::Float`/`Ty::Int` payloads are how the public API detects
    // float/integer types — pin down enum variants so an accidental
    // re-ordering surfaces as a test failure rather than a silent miscall.
    assert_ne!(FloatKind::F32 as u32, FloatKind::F64 as u32);
    assert_ne!(FloatKind::F64 as u32, FloatKind::F80 as u32);
    assert_ne!(IntRank::Char as u32, IntRank::Int as u32);
    assert_ne!(IntRank::Long as u32, IntRank::LongLong as u32);
}
