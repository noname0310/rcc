//! Initializer constness check (C99 §6.7.8p4).
//!
//! Every expression in an initializer for an object that has static or
//! thread storage duration shall be a constant expression or a string
//! literal. This module provides the entry points the driver wires up
//! once globals carry their initializer expressions in HIR (a
//! deliberately small first cut — the present module exposes the
//! checking primitive so any pass that already has the list of init
//! expressions for a global can call it).
//!
//! The check delegates the heavy lifting to [`ConstEval`]:
//! * [`is_const_init_expr`] returns `true` iff the expression folds via
//!   `ConstEval::eval_scalar` (covering integer §6.6p6, arithmetic
//!   §6.6p7, and address-constant §6.6p8 forms) **or** is a string
//!   literal designator (§6.7.8p4 explicitly carves these out — they
//!   are not address constants per §6.6p8 but are still legal here).
//! * [`check_init_const`] iterates a flat list of init expressions and
//!   emits [`E0084`] at the offending expression's span on the first
//!   failure. It returns `true` iff every expression in the list is a
//!   constant initializer.
//!
//! [`E0084`]: rcc_errors::codes::E0084

use rcc_data_structures::IndexVec;
use rcc_errors::codes;
use rcc_hir::{Body, ConvertKind, Def, DefId, HirExprId, HirExprKind, TyCtxt};
use rcc_session::Session;

use crate::const_eval::ConstEval;

/// Decide whether a single HIR expression is a legal initializer for
/// an object with static (file-scope / `static`) storage duration per
/// C99 §6.7.8p4.
///
/// "Constant" here is the union of every form C99 §6.6 calls a
/// constant expression — integer (§6.6p6), arithmetic (§6.6p7), and
/// address (§6.6p8) — plus the §6.7.8p4 string-literal carve-out.
///
/// The function does **not** emit diagnostics on its own; it is a pure
/// predicate so callers can run it in tight loops over aggregate
/// initializers without flooding the diagnostic stream. Use
/// [`check_init_const`] when you want the E0084 emission.
#[must_use]
pub fn is_const_init_expr(
    body: &Body,
    expr_id: HirExprId,
    defs: Option<&IndexVec<DefId, Def>>,
    tcx: &TyCtxt,
) -> bool {
    if is_string_literal_init(body, expr_id) {
        return true;
    }
    if let Some(lanes) = vector_init_lanes(body, expr_id) {
        return lanes.iter().all(|lane| is_const_init_expr(body, *lane, defs, tcx));
    }
    let mut ce = ConstEval::with_defs_and_session(tcx, Some(body), defs, None);
    ce.eval_scalar(expr_id).is_some()
}

fn vector_init_lanes(body: &Body, expr_id: HirExprId) -> Option<&[HirExprId]> {
    match &body.exprs.get(expr_id)?.kind {
        HirExprKind::VectorInit { lanes, .. } => Some(lanes.as_slice()),
        HirExprKind::Convert { operand, kind } => match kind {
            ConvertKind::IntegerPromotion | ConvertKind::UsualArithmetic => {
                vector_init_lanes(body, *operand)
            }
            ConvertKind::ArrayToPtr
            | ConvertKind::FuncToPtr
            | ConvertKind::LvalueToRvalue
            | ConvertKind::Pointer
            | ConvertKind::RealToComplex
            | ConvertKind::ComplexToReal
            | ConvertKind::BitfieldPrecision { .. } => None,
        },
        HirExprKind::Cast { operand, .. } => vector_init_lanes(body, *operand),
        _ => None,
    }
}

/// Walk through the §6.3 implicit-conversion wrappers the type
/// checker inserts (lvalue-to-rvalue, array-to-pointer, pointer
/// conversions, plain casts) and report whether the leaf is a
/// `StringRef`.
///
/// C99 §6.7.8p4's "or string literal" allowance is independent of
/// §6.6p8's address-constant rules: a bare `"hello"` initializer for a
/// `char *` global is legal even though `eval_address` does not (yet)
/// fold a `StringRef` into an address constant. Recognising it here
/// keeps the predicate honest until the const evaluator grows that
/// support directly.
fn is_string_literal_init(body: &Body, expr_id: HirExprId) -> bool {
    let mut current = expr_id;
    loop {
        let Some(e) = body.exprs.get(current) else { return false };
        match &e.kind {
            HirExprKind::StringRef(_) => return true,
            HirExprKind::Convert { operand, kind } => match kind {
                ConvertKind::ArrayToPtr
                | ConvertKind::FuncToPtr
                | ConvertKind::Pointer
                | ConvertKind::LvalueToRvalue => current = *operand,
                ConvertKind::IntegerPromotion | ConvertKind::UsualArithmetic => return false,
                // `_Complex` conversions never sit between a
                // string-literal leaf and the surrounding pointer-typed
                // initializer; if they do, it isn't a string-literal
                // initializer in the recognised shape.
                ConvertKind::RealToComplex
                | ConvertKind::ComplexToReal
                | ConvertKind::BitfieldPrecision { .. } => return false,
            },
            HirExprKind::Cast { operand, .. } => current = *operand,
            _ => return false,
        }
    }
}

/// Verify the constness of every expression in `init_exprs`, the flat
/// list of leaf expressions a global's (possibly aggregate)
/// initializer reduces to.
///
/// On the first non-constant leaf, emits [`codes::E0084`] with the
/// offending expression's span as the primary label and a short
/// "non-constant expression" caption. Subsequent expressions are still
/// checked so a single `cargo build` produces the full set of E0084
/// diagnostics in one pass.
///
/// Returns `true` iff every expression in `init_exprs` is a constant
/// initializer.
pub fn check_init_const(
    body: &Body,
    init_exprs: &[HirExprId],
    defs: Option<&IndexVec<DefId, Def>>,
    tcx: &TyCtxt,
    session: &mut Session,
) -> bool {
    let mut all_ok = true;
    for &expr_id in init_exprs {
        if !is_const_init_expr(body, expr_id, defs, tcx) {
            all_ok = false;
            let span = body.exprs.get(expr_id).map_or(rcc_span::DUMMY_SP, |e| e.span);
            session
                .handler
                .struct_err(span, "non-constant expression in static initializer")
                .code(codes::E0084)
                .label(span, "this expression is not a constant")
                .emit();
        }
    }
    all_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_data_structures::IndexVec;
    use rcc_hir::{
        rcc_hir_binop::BinOp, Def, DefKind, HirExpr, HirExprId, HirExprKind, Linkage, Local,
        ValueCat,
    };
    use rcc_span::DUMMY_SP;

    fn push(body: &mut Body, ty: rcc_hir::TyId, kind: HirExprKind) -> HirExprId {
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

    /// Acceptance fixture: `static int x = 2 + 3;` — integer constant
    /// expression, accepted, no diagnostics.
    #[test]
    fn int_const_addition_is_const_init() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let three = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let add =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: two, rhs: three });

        assert!(is_const_init_expr(&body, add, None, &tcx));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[add], None, &tcx, &mut session);
        assert!(ok);
        assert!(cap.diagnostics().is_empty(), "no E0084 expected for `2 + 3`");
    }

    #[test]
    fn vector_init_is_const_when_all_lanes_are_const() {
        let mut tcx = TyCtxt::new();
        let vector_ty = tcx.intern(rcc_hir::Ty::Vector { elem: tcx.int, lanes: 4, bytes: 16 });
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let three = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let four = push(&mut body, tcx.int, HirExprKind::IntConst(4));
        let vector = push(
            &mut body,
            vector_ty,
            HirExprKind::VectorInit { ty: vector_ty, lanes: vec![one, two, three, four] },
        );

        assert!(is_const_init_expr(&body, vector, None, &tcx));
    }

    /// Acceptance fixture: `static int x = foo();` — call expression,
    /// rejected with E0084 at the call's span.
    #[test]
    fn call_expression_in_init_emits_e0084() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        // Build a call whose callee is a LocalRef so we don't depend on
        // a real function definition: `eval_scalar` rejects calls
        // unconditionally regardless of callee shape.
        let callee = push(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let call = push(&mut body, tcx.int, HirExprKind::Call { callee, args: Vec::new() });

        assert!(!is_const_init_expr(&body, call, None, &tcx));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[call], None, &tcx, &mut session);
        assert!(!ok);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0084");
        assert_eq!(diags[0].code, Some(codes::E0084));
    }

    /// `LocalRef` (a non-static local) is not a constant expression.
    #[test]
    fn local_ref_in_init_emits_e0084() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lr = push(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[lr], None, &tcx, &mut session);
        assert!(!ok);
        assert_eq!(cap.diagnostics().len(), 1);
        assert_eq!(cap.diagnostics()[0].code, Some(codes::E0084));
    }

    /// `static int *p = &g;` — address constant, accepted.
    #[test]
    fn address_of_global_is_const_init() {
        let mut tcx = TyCtxt::new();
        let ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(tcx.int)));
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
        let dref = push(&mut body, tcx.int, HirExprKind::DefRef(g));
        let addr = push(&mut body, ptr_ty, HirExprKind::AddressOf(dref));

        assert!(is_const_init_expr(&body, addr, Some(&defs), &tcx));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[addr], Some(&defs), &tcx, &mut session);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }

    /// `static char *p = "hi";` — string literal, accepted via the
    /// §6.7.8p4 carve-out. The `StringRef` is wrapped in
    /// `Convert::ArrayToPtr`, exactly as the typeck pass would emit.
    #[test]
    fn string_literal_init_is_const() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();
        // The synthetic string-literal global is never opened here —
        // the predicate stops at the `StringRef` leaf without needing
        // to resolve the pointee.
        let def = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.char_,
                quals: rcc_hir::ObjectQuals::none(),
                thread_local: false,
                linkage: Linkage::Internal,
                init: None,
            },
        });
        defs[def].id = def;
        let sref = push(&mut body, tcx.char_, HirExprKind::StringRef(def));
        let decayed = push(
            &mut body,
            tcx.char_,
            HirExprKind::Convert { operand: sref, kind: ConvertKind::ArrayToPtr },
        );

        assert!(is_const_init_expr(&body, decayed, Some(&defs), &tcx));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[decayed], Some(&defs), &tcx, &mut session);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }

    /// Aggregate initialiser fixture: `static int a[3] = {1, 2+3, foo()};`
    /// — the first two leaves fold, the third is a call. Exactly one
    /// E0084 fires at the call's span; the surrounding leaves remain
    /// silent.
    #[test]
    fn aggregate_init_emits_e0084_only_on_bad_leaf() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let three = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let add =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: two, rhs: three });
        let callee = push(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let call = push(&mut body, tcx.int, HirExprKind::Call { callee, args: Vec::new() });

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[one, add, call], None, &tcx, &mut session);
        assert!(!ok);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0084 on the bad leaf");
        assert_eq!(diags[0].code, Some(codes::E0084));
    }

    /// Multiple non-const leaves all surface in a single pass — no
    /// short-circuit on the first error.
    #[test]
    fn check_init_const_does_not_short_circuit() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let l1 = push(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let l2 = push(&mut body, tcx.int, HirExprKind::LocalRef(Local(1)));

        let (mut session, cap) = Session::for_test();
        let ok = check_init_const(&body, &[l1, l2], None, &tcx, &mut session);
        assert!(!ok);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 2, "every bad leaf emits its own E0084");
        for d in diags.iter() {
            assert_eq!(d.code, Some(codes::E0084));
        }
    }
}
