//! `rcc_typeck`: type checking + implicit conversion insertion.
//!
//! Implements C99 §6.3 (conversions), §6.5 (expression typing), and
//! §6.6 (constant expressions). Mutates the HIR in place by inserting
//! `HirExprKind::Convert { .. }` nodes where an implicit conversion applies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_hir::{
    rcc_hir_binop::{BinOp, UnOp},
    Body, ConvertKind, DefKind, FloatKind, HirCrate, HirExpr, HirExprId, HirExprKind, HirStmtId,
    HirStmtKind, IntRank, Qual, Ty, TyCtxt, TyId, ValueCat,
};
use rcc_session::Session;

pub mod const_eval;

pub use const_eval::{ConstEval, ConstValue};

/// Width in bits of `int` assumed by the type checker.
///
/// Target abstraction will land in phase 15; until then every backend in the
/// workspace assumes a 32-bit `int`, matching the assumption other phases
/// have already baked in (see e.g. enumerator value selection in
/// `rcc_hir_lower`).
const INT_BITS: u32 = 32;

/// Run full type checking over `hir`. After this call every `HirExpr` has a
/// resolved `ty` and every mandatory implicit conversion has been inserted.
///
/// Iterates over every function body in `hir` and dispatches to
/// [`check_body`]. Read-only data (top-level `Def`s and their kinds) is
/// captured up-front so each per-body walk does not need shared `&hir`
/// access concurrently with the `&mut Body` it edits.
pub fn check(session: &mut Session, tcx: &mut TyCtxt, hir: &mut HirCrate) {
    // We need to look up `Def::kind` while typing `DefRef` nodes. Snapshot
    // the (DefId, ty/value-cat-relevant) info up front so the per-body
    // walk does not have to borrow `hir.defs` while it holds `&mut hir.bodies[id]`.
    let def_info: rcc_data_structures::FxHashMap<rcc_hir::DefId, DefSnapshot> =
        hir.defs.iter_enumerated().map(|(id, def)| (id, def_snapshot(&def.kind))).collect();

    // `hir.bodies` is a HashMap; iterate over its keys via a snapshot to
    // avoid alias trouble between the keys-iterator and the per-body
    // `get_mut` that follows.
    let body_keys: Vec<_> = hir.bodies.keys().copied().collect();
    for def_id in body_keys {
        if let Some(body) = hir.bodies.get_mut(&def_id) {
            check_body_with_defs(body, tcx, session, &def_info);
        }
    }
}

/// Per-`DefId` snapshot of the information `check_body` needs about a
/// top-level definition referenced via `HirExprKind::DefRef`. We only
/// keep what the walker reads, so the snapshot stays compact and the
/// borrow shape stays simple.
#[derive(Copy, Clone, Debug)]
pub struct DefSnapshot {
    /// Type of the referenced object/function, or `None` if the kind has
    /// no associated type (records, enums — never the target of `DefRef`).
    pub ty: Option<TyId>,
    /// Value category produced by referencing this def. Functions are
    /// lvalues that decay to pointer-to-function; globals/enumerators are
    /// lvalues for globals and rvalues for enumerators.
    pub value_cat: ValueCat,
    /// Folded enumerator value, when the `DefRef` resolves to an
    /// enumerator. Enumerators are rvalue integer constants (C99
    /// §6.4.4.3p2 + §6.7.2.2p3) — we materialise them as `IntConst` so
    /// later passes do not need to chase a `DefId` to evaluate one.
    pub enumerator_value: Option<i128>,
}

/// Read what we need from a `DefKind` for `DefRef` typing.
fn def_snapshot(kind: &DefKind) -> DefSnapshot {
    match kind {
        DefKind::Function { ty, .. } => DefSnapshot {
            ty: Some(*ty),
            // Function designator is an lvalue (C99 §6.3.2.1p4 says it
            // converts to a pointer-to-function — that conversion is the
            // `FuncToPtr` decay, applied by `decay_if_needed`).
            value_cat: ValueCat::LValue,
            enumerator_value: None,
        },
        DefKind::Global { ty, .. } => {
            DefSnapshot { ty: Some(*ty), value_cat: ValueCat::LValue, enumerator_value: None }
        }
        DefKind::Typedef(ty) => {
            // Should never appear as a `DefRef` operand (typedefs live in
            // a different namespace). Pass through with the typedef's
            // alias type so the walker is total.
            DefSnapshot { ty: Some(*ty), value_cat: ValueCat::RValue, enumerator_value: None }
        }
        DefKind::Enumerator { ty, value } => DefSnapshot {
            ty: Some(*ty),
            value_cat: ValueCat::RValue,
            enumerator_value: Some(*value),
        },
        DefKind::Record { .. } | DefKind::Enum { .. } => {
            DefSnapshot { ty: None, value_cat: ValueCat::RValue, enumerator_value: None }
        }
    }
}

/// Type-check every expression in `body`. After this call every reachable
/// expression carries a non-`Error` type and the value-category required
/// by its position, with every mandatory conversion (integer promotion,
/// usual-arithmetic, lvalue-to-rvalue, array/function decay, pointer
/// conversion) materialised as a fresh `Convert` wrapper.
///
/// Walks the statement tree top-down so each expression is visited in
/// the position it appears, then drives the per-expression typing
/// bottom-up: `visit_expr` types every child first, then folds the
/// children's types into the parent.
///
/// This three-argument form is the public API listed in the task
/// spec; callers that have access to a containing [`HirCrate`] should
/// prefer [`check_body_with_defs`] so `DefRef` nodes can resolve to
/// the type / value-category of the referenced definition.
pub fn check_body(body: &mut Body, tcx: &mut TyCtxt, session: &mut Session) {
    let empty: rcc_data_structures::FxHashMap<rcc_hir::DefId, DefSnapshot> =
        rcc_data_structures::FxHashMap::default();
    check_body_with_defs(body, tcx, session, &empty);
}

/// Internal entry point used by [`check`] when the full crate is
/// available so `DefRef` nodes can be typed against the referenced
/// definition.
pub fn check_body_with_defs(
    body: &mut Body,
    tcx: &mut TyCtxt,
    session: &mut Session,
    def_info: &rcc_data_structures::FxHashMap<rcc_hir::DefId, DefSnapshot>,
) {
    // Walk every statement in the body, visiting whichever expressions
    // each statement points at. The traversal is rooted at `body.root`
    // when present (top-level functions); free-standing test bodies that
    // build an isolated expression set their `root` to `None` and rely
    // on the caller to drive `visit_expr` directly.
    if let Some(root) = body.root {
        visit_stmt(root, body, tcx, session, def_info);
    }
}

/// Type-check the statement at `stmt_id`, recursing into nested
/// statements and expressions. Updates child expression ids in-place so
/// any `Convert` wrappers inserted by the per-expression walker remain
/// reachable from their parent statement.
fn visit_stmt(
    stmt_id: HirStmtId,
    body: &mut Body,
    tcx: &mut TyCtxt,
    session: &mut Session,
    def_info: &rcc_data_structures::FxHashMap<rcc_hir::DefId, DefSnapshot>,
) {
    // Clone the kind so we can mutate child ids without holding a borrow
    // on `body.stmts` while we recurse into `body.exprs`.
    let kind = body.stmts[stmt_id].kind.clone();
    let new_kind = match kind {
        HirStmtKind::Block(stmts) => {
            for s in &stmts {
                visit_stmt(*s, body, tcx, session, def_info);
            }
            HirStmtKind::Block(stmts)
        }
        HirStmtKind::Expr(e) => {
            let e2 = visit_expr(e, body, tcx, session, def_info);
            HirStmtKind::Expr(e2)
        }
        HirStmtKind::If { cond, then_branch, else_branch } => {
            let cond2 = visit_expr(cond, body, tcx, session, def_info);
            // Controlling expression must be scalar; convert lvalue-to-rvalue
            // and decay arrays/functions so the resulting node is a plain
            // scalar rvalue.
            let cond2 = scalar_rvalue(cond2, body, tcx);
            visit_stmt(then_branch, body, tcx, session, def_info);
            if let Some(eb) = else_branch {
                visit_stmt(eb, body, tcx, session, def_info);
            }
            HirStmtKind::If { cond: cond2, then_branch, else_branch }
        }
        HirStmtKind::While { cond, body: b } => {
            let cond2 = visit_expr(cond, body, tcx, session, def_info);
            let cond2 = scalar_rvalue(cond2, body, tcx);
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::While { cond: cond2, body: b }
        }
        HirStmtKind::DoWhile { body: b, cond } => {
            visit_stmt(b, body, tcx, session, def_info);
            let cond2 = visit_expr(cond, body, tcx, session, def_info);
            let cond2 = scalar_rvalue(cond2, body, tcx);
            HirStmtKind::DoWhile { body: b, cond: cond2 }
        }
        HirStmtKind::For { init, cond, step, body: b } => {
            if let Some(i) = init {
                visit_stmt(i, body, tcx, session, def_info);
            }
            let cond2 = cond.map(|c| {
                let c2 = visit_expr(c, body, tcx, session, def_info);
                scalar_rvalue(c2, body, tcx)
            });
            let step2 = step.map(|s| visit_expr(s, body, tcx, session, def_info));
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::For { init, cond: cond2, step: step2, body: b }
        }
        HirStmtKind::Switch { cond, body: b, cases } => {
            let cond2 = visit_expr(cond, body, tcx, session, def_info);
            let cond2 = scalar_rvalue(cond2, body, tcx);
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::Switch { cond: cond2, body: b, cases }
        }
        HirStmtKind::Label { name, body: b } => {
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::Label { name, body: b }
        }
        HirStmtKind::Case { value, body: b } => {
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::Case { value, body: b }
        }
        HirStmtKind::Default { body: b } => {
            visit_stmt(b, body, tcx, session, def_info);
            HirStmtKind::Default { body: b }
        }
        HirStmtKind::Return(opt_e) => {
            // The return-value's expected type is the enclosing function's
            // return type. We don't have that here without threading more
            // context; for now just type-check the expression and fall
            // through. Task 07-11 will tighten this with `is_assignable`
            // against the declared return type.
            let opt_e2 = opt_e.map(|e| {
                let e2 = visit_expr(e, body, tcx, session, def_info);
                rvalue_decayed(e2, body, tcx)
            });
            HirStmtKind::Return(opt_e2)
        }
        HirStmtKind::LocalDecl { local, init } => {
            let init2 = init.map(|e| {
                let e2 = visit_expr(e, body, tcx, session, def_info);
                let e2 = rvalue_decayed(e2, body, tcx);
                // Coerce the initializer to the declared local type.
                let want = body.locals[local].ty;
                coerce_to(e2, want, body, tcx, session)
            });
            HirStmtKind::LocalDecl { local, init: init2 }
        }
        HirStmtKind::Goto(_) | HirStmtKind::Break | HirStmtKind::Continue | HirStmtKind::Null => {
            kind
        }
    };
    body.stmts[stmt_id].kind = new_kind;
}

/// Type-check the expression at `expr_id`, recursing into its children
/// first. Returns the id the parent should now reference — typically
/// `expr_id` itself (the type was filled in place), but can be a fresh
/// id when the walker wrapped the expression in a `Convert` node.
pub fn visit_expr(
    expr_id: HirExprId,
    body: &mut Body,
    tcx: &mut TyCtxt,
    session: &mut Session,
    def_info: &rcc_data_structures::FxHashMap<rcc_hir::DefId, DefSnapshot>,
) -> HirExprId {
    // Clone the kind to break the borrow on `body.exprs` for the
    // recursive calls below. After computing the new kind we write it
    // back, along with the resolved type and value category.
    let kind = body.exprs[expr_id].kind.clone();
    let span = body.exprs[expr_id].span;

    match kind {
        // ---- Leaves --------------------------------------------------
        HirExprKind::IntConst(_) => {
            body.exprs[expr_id].ty = tcx.int;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            expr_id
        }
        HirExprKind::FloatConst(_) => {
            body.exprs[expr_id].ty = tcx.double;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            expr_id
        }
        HirExprKind::StringRef(def_id) => {
            // String literal: the `Def` carries the array-of-char type
            // built by lowering. `value_cat` is lvalue (string literals
            // designate static-storage objects).
            if let Some(snap) = def_info.get(&def_id) {
                if let Some(ty) = snap.ty {
                    body.exprs[expr_id].ty = ty;
                }
            }
            body.exprs[expr_id].value_cat = ValueCat::LValue;
            expr_id
        }
        HirExprKind::LocalRef(local) => {
            body.exprs[expr_id].ty = body.locals[local].ty;
            body.exprs[expr_id].value_cat = ValueCat::LValue;
            expr_id
        }
        HirExprKind::DefRef(def_id) => {
            let snap = def_info.get(&def_id).copied().unwrap_or(DefSnapshot {
                ty: None,
                value_cat: ValueCat::RValue,
                enumerator_value: None,
            });
            // Enumerator references rewrite to a typed `IntConst` so
            // const-eval and the CFG never need to look up enumerators.
            if let Some(value) = snap.enumerator_value {
                body.exprs[expr_id].ty = snap.ty.unwrap_or(tcx.int);
                body.exprs[expr_id].value_cat = ValueCat::RValue;
                body.exprs[expr_id].kind = HirExprKind::IntConst(value);
                return expr_id;
            }
            if let Some(ty) = snap.ty {
                body.exprs[expr_id].ty = ty;
            }
            body.exprs[expr_id].value_cat = snap.value_cat;
            expr_id
        }

        // ---- Compound forms -----------------------------------------
        HirExprKind::Binary { op, lhs, rhs } => {
            let lhs2 = visit_expr(lhs, body, tcx, session, def_info);
            let rhs2 = visit_expr(rhs, body, tcx, session, def_info);
            type_binary(expr_id, op, lhs2, rhs2, span, body, tcx, session)
        }
        HirExprKind::Unary { op, operand } => {
            let op2 = visit_expr(operand, body, tcx, session, def_info);
            type_unary(expr_id, op, op2, body, tcx, session)
        }
        HirExprKind::AddressOf(operand) => {
            let op2 = visit_expr(operand, body, tcx, session, def_info);
            // Address-of: operand of `&` does not decay (DecayContext::AddrOfOperand).
            // We still need to record the operand id (no l-to-r conversion either).
            let inner_ty = body.exprs[op2].ty;
            let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(inner_ty)));
            body.exprs[expr_id].ty = ptr_ty;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::AddressOf(op2);
            expr_id
        }
        HirExprKind::Deref(operand) => {
            let op2 = visit_expr(operand, body, tcx, session, def_info);
            // The pointer needs to be an rvalue (a value to dereference);
            // if it's an lvalue (e.g. a pointer-typed local), apply
            // lvalue-to-rvalue. Arrays decay to pointers.
            let op2 = rvalue_decayed(op2, body, tcx);
            let pointee = match *tcx.get(body.exprs[op2].ty) {
                Ty::Ptr(q) => q.ty,
                _ => tcx.error,
            };
            body.exprs[expr_id].ty = pointee;
            body.exprs[expr_id].value_cat = ValueCat::LValue;
            body.exprs[expr_id].kind = HirExprKind::Deref(op2);
            expr_id
        }
        HirExprKind::Index { base, index } => {
            let base2 = visit_expr(base, body, tcx, session, def_info);
            let index2 = visit_expr(index, body, tcx, session, def_info);
            // `a[i]` is `*(a + i)`: base decays to pointer, index is an
            // integer rvalue. Result type is the pointee of the decayed
            // base; result is an lvalue.
            let base2 = rvalue_decayed(base2, body, tcx);
            let index2 = rvalue_decayed(index2, body, tcx);
            let elem = match *tcx.get(body.exprs[base2].ty) {
                Ty::Ptr(q) => q.ty,
                _ => tcx.error,
            };
            body.exprs[expr_id].ty = elem;
            body.exprs[expr_id].value_cat = ValueCat::LValue;
            body.exprs[expr_id].kind = HirExprKind::Index { base: base2, index: index2 };
            expr_id
        }
        HirExprKind::Field { base, field_index } => {
            let base2 = visit_expr(base, body, tcx, session, def_info);
            // We don't yet have struct field-resolution machinery in HIR
            // typeck (task 07-11 covers fields). Best we can do today:
            // preserve the placeholder type from lowering. We at least
            // get the value category right so assignment LHS checks work.
            let cat = value_category(body, base2);
            body.exprs[expr_id].value_cat = cat;
            body.exprs[expr_id].kind = HirExprKind::Field { base: base2, field_index };
            expr_id
        }
        HirExprKind::Call { callee, args } => {
            let callee2 = visit_expr(callee, body, tcx, session, def_info);
            // Function designator decays to pointer-to-function.
            let callee2 = rvalue_decayed(callee2, body, tcx);
            let mut new_args = Vec::with_capacity(args.len());
            for a in args {
                let a2 = visit_expr(a, body, tcx, session, def_info);
                let a2 = rvalue_decayed(a2, body, tcx);
                // Argument promotion is handled per-parameter when we
                // know the prototype below; for unprototyped / variadic
                // arguments we apply default argument promotions.
                new_args.push(a2);
            }
            type_call(expr_id, callee2, new_args, body, tcx, session)
        }
        HirExprKind::Convert { operand, kind } => {
            let op2 = visit_expr(operand, body, tcx, session, def_info);
            // Preserve the convert kind; just rewire the operand id and
            // leave the type untouched (the wrapper was inserted with a
            // deliberate destination type at construction time).
            body.exprs[expr_id].kind = HirExprKind::Convert { operand: op2, kind };
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            // If the wrapper still has a placeholder type, fall back to
            // the operand's type — this is the common shape for the
            // string-literal global case where lowering pre-built a
            // Convert with a known destination type.
            if body.exprs[expr_id].ty == tcx.error {
                body.exprs[expr_id].ty = body.exprs[op2].ty;
            }
            expr_id
        }
        HirExprKind::Cast { operand, to } => {
            let op2 = visit_expr(operand, body, tcx, session, def_info);
            // Cast operand becomes an rvalue, with arrays/functions decayed.
            let op2 = rvalue_decayed(op2, body, tcx);
            // The `to` field is the placeholder `tcx.error` from lowering;
            // task 07-11 will resolve the source type-name. For now we
            // leave the destination as `to` if it is not the error sentinel,
            // otherwise fall back to the operand's type so we do not poison
            // the IR.
            let dst = if to == tcx.error { body.exprs[op2].ty } else { to };
            body.exprs[expr_id].ty = dst;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Cast { operand: op2, to: dst };
            expr_id
        }
        HirExprKind::Assign { lhs, rhs } => {
            let lhs2 = visit_expr(lhs, body, tcx, session, def_info);
            let rhs2 = visit_expr(rhs, body, tcx, session, def_info);
            // C99 §6.5.16p2: LHS must be a modifiable lvalue. We check
            // the lvalue requirement here; the modifiable subset is task
            // 07-05's job (already in tree).
            check_assignment_lhs(session, body, lhs2);
            // RHS is an rvalue, decayed.
            let rhs2 = rvalue_decayed(rhs2, body, tcx);
            // Coerce RHS to LHS's type.
            let lhs_ty = body.exprs[lhs2].ty;
            let rhs2 = coerce_to(rhs2, lhs_ty, body, tcx, session);
            body.exprs[expr_id].ty = lhs_ty;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Assign { lhs: lhs2, rhs: rhs2 };
            expr_id
        }
        HirExprKind::Cond { cond, then_expr, else_expr } => {
            let cond2 = visit_expr(cond, body, tcx, session, def_info);
            let cond2 = scalar_rvalue(cond2, body, tcx);
            let then2 = visit_expr(then_expr, body, tcx, session, def_info);
            let else2 = visit_expr(else_expr, body, tcx, session, def_info);
            let then2 = rvalue_decayed(then2, body, tcx);
            let else2 = rvalue_decayed(else2, body, tcx);
            let then_ty = body.exprs[then2].ty;
            let else_ty = body.exprs[else2].ty;
            // Common case: both arithmetic — apply usual arithmetic.
            let result_ty = if is_arithmetic(tcx, then_ty) && is_arithmetic(tcx, else_ty) {
                let common = usual_arithmetic(tcx, then_ty, else_ty);
                // No diagnostics on narrowing here — usual_arithmetic
                // is a widening conversion by construction.
                let then2_w = if body.exprs[then2].ty != common {
                    push_arith_convert(body, then2, common)
                } else {
                    then2
                };
                let else2_w = if body.exprs[else2].ty != common {
                    push_arith_convert(body, else2, common)
                } else {
                    else2
                };
                body.exprs[expr_id].kind =
                    HirExprKind::Cond { cond: cond2, then_expr: then2_w, else_expr: else2_w };
                common
            } else if then_ty == else_ty {
                body.exprs[expr_id].kind =
                    HirExprKind::Cond { cond: cond2, then_expr: then2, else_expr: else2 };
                then_ty
            } else {
                // Pointer / mixed cases: pick `then`'s type as a
                // best-effort placeholder. Task 07-11 expands this.
                body.exprs[expr_id].kind =
                    HirExprKind::Cond { cond: cond2, then_expr: then2, else_expr: else2 };
                then_ty
            };
            body.exprs[expr_id].ty = result_ty;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            expr_id
        }
        HirExprKind::Comma { lhs, rhs } => {
            let lhs2 = visit_expr(lhs, body, tcx, session, def_info);
            // LHS is evaluated for side effects and discarded — apply
            // lvalue-to-rvalue + decay so the discard is on the value.
            let lhs2 = rvalue_decayed(lhs2, body, tcx);
            let rhs2 = visit_expr(rhs, body, tcx, session, def_info);
            let rhs2 = rvalue_decayed(rhs2, body, tcx);
            body.exprs[expr_id].ty = body.exprs[rhs2].ty;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Comma { lhs: lhs2, rhs: rhs2 };
            expr_id
        }
    }
}

/// Apply lvalue-to-rvalue + array/function decay to `expr`. Returns the
/// id callers should reference. Common helper for "I want a value here".
fn rvalue_decayed(expr: HirExprId, body: &mut Body, tcx: &mut TyCtxt) -> HirExprId {
    let after_decay = decay_if_needed(tcx, body, expr, DecayContext::Normal);
    lvalue_to_rvalue_if_needed(tcx, body, after_decay)
}

/// Same as [`rvalue_decayed`] but used in scalar-controlling positions
/// (`if`/`while`/`?:` first operand). C99 §6.8.4 / §6.5.15 require the
/// controlling expression to have scalar type; for us "scalar rvalue"
/// suffices structurally. Diagnostic enforcement of "must be scalar" is
/// task 07-11.
fn scalar_rvalue(expr: HirExprId, body: &mut Body, tcx: &mut TyCtxt) -> HirExprId {
    rvalue_decayed(expr, body, tcx)
}

/// Wrap `expr` in a `Convert { kind: UsualArithmetic }` whose type is
/// `dst`. Used by binary arithmetic / conditional to bring an operand
/// up to the common type. Returns the new expression id.
fn push_arith_convert(body: &mut Body, expr: HirExprId, dst: TyId) -> HirExprId {
    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: dst,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: ConvertKind::UsualArithmetic },
    });
    body.exprs[id].id = id;
    id
}

/// Wrap `expr` in a `Convert { kind: IntegerPromotion }` whose type is
/// `dst`. Used by unary `+`/`-`/`~`/`!` and shift/bitwise operands.
fn push_int_promote(body: &mut Body, expr: HirExprId, dst: TyId) -> HirExprId {
    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: dst,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: ConvertKind::IntegerPromotion },
    });
    body.exprs[id].id = id;
    id
}

/// Coerce `expr` to type `dst` for a context that requires assignment-
/// compatibility (initializer, simple assignment RHS, function call
/// argument, return). Inserts the appropriate `Convert` wrapper.
///
/// Diagnostics for the constraint-violating cases of [`AssignError`] are
/// emitted here; the bare-conversion case (arithmetic widening, pointer
/// adjustments, null-pointer-constant, `_Bool` from a pointer) is
/// silent. Narrowing is currently silent too — W0008 is the future
/// home for that warning (task 07-11).
fn coerce_to(
    expr: HirExprId,
    dst: TyId,
    body: &mut Body,
    tcx: &mut TyCtxt,
    _session: &mut Session,
) -> HirExprId {
    let src_ty = body.exprs[expr].ty;
    if src_ty == dst {
        return expr;
    }
    // Skip coercion when either side is the error sentinel — there is
    // already a diagnostic upstream.
    if src_ty == tcx.error || dst == tcx.error {
        return expr;
    }
    // Arithmetic ↔ arithmetic: pick a UsualArithmetic-style widening
    // wrapper. Narrowing diagnostics deferred to 07-11.
    if is_arithmetic(tcx, src_ty) && is_arithmetic(tcx, dst) {
        return push_arith_convert(body, expr, dst);
    }
    // Pointer-shaped destination: try `pointer_convert`. Errors are
    // silent here (07-11 wires up E0081/E0082). On error fall through
    // and leave the expr untouched.
    if matches!(*tcx.get(dst), Ty::Ptr(_)) {
        if let Ok(new_id) = pointer_convert(tcx, body, expr, dst) {
            return new_id;
        }
    }
    // `_Bool` ← pointer / arithmetic. We emit a UsualArithmetic-flavoured
    // convert for now; the dedicated `BoolFromPtr` ConvertKind is task
    // 07-11.
    if dst == tcx.bool_ {
        return push_arith_convert(body, expr, dst);
    }
    expr
}

/// Diagnostic-emitting type-checker for `HirExprKind::Binary`. Updates
/// `body.exprs[expr_id]` in place with the resolved type and rewires
/// the lhs/rhs references to whatever `Convert` wrappers the conversion
/// rules required.
#[allow(clippy::too_many_arguments)]
fn type_binary(
    expr_id: HirExprId,
    op: BinOp,
    lhs: HirExprId,
    rhs: HirExprId,
    span: rcc_span::Span,
    body: &mut Body,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> HirExprId {
    // Both operands undergo lvalue-to-rvalue + decay before any further
    // typing (C99 §6.3.2.1 + §6.5.* operand rules).
    let lhs = rvalue_decayed(lhs, body, tcx);
    let rhs = rvalue_decayed(rhs, body, tcx);
    let lhs_ty = body.exprs[lhs].ty;
    let rhs_ty = body.exprs[rhs].ty;

    let (result_ty, lhs_final, rhs_final) = match op {
        // Arithmetic: usual arithmetic conversions, integer-only for `%`.
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            // `+`/`-` accept pointer arithmetic; the others demand
            // arithmetic operands. We handle pointer arithmetic in a
            // best-effort fashion here: result type is the pointer side.
            if matches!(op, BinOp::Add | BinOp::Sub)
                && (matches!(*tcx.get(lhs_ty), Ty::Ptr(_))
                    || matches!(*tcx.get(rhs_ty), Ty::Ptr(_)))
            {
                let result_ty =
                    if matches!(*tcx.get(lhs_ty), Ty::Ptr(_)) { lhs_ty } else { rhs_ty };
                (result_ty, lhs, rhs)
            } else if op == BinOp::Rem {
                if !is_integer(tcx, lhs_ty) || !is_integer(tcx, rhs_ty) {
                    invalid_operands(session, span, "%");
                    (tcx.error, lhs, rhs)
                } else {
                    let common = usual_arithmetic(tcx, lhs_ty, rhs_ty);
                    let l =
                        if lhs_ty != common { push_arith_convert(body, lhs, common) } else { lhs };
                    let r =
                        if rhs_ty != common { push_arith_convert(body, rhs, common) } else { rhs };
                    (common, l, r)
                }
            } else if !is_arithmetic(tcx, lhs_ty) || !is_arithmetic(tcx, rhs_ty) {
                invalid_operands(session, span, binop_symbol(op));
                (tcx.error, lhs, rhs)
            } else {
                let common = usual_arithmetic(tcx, lhs_ty, rhs_ty);
                let l = if lhs_ty != common { push_arith_convert(body, lhs, common) } else { lhs };
                let r = if rhs_ty != common { push_arith_convert(body, rhs, common) } else { rhs };
                (common, l, r)
            }
        }
        // Bitwise & shift: integer-only operands. Shifts apply integer
        // promotion to each side independently (the result type is the
        // promoted LHS, per §6.5.7p3).
        BinOp::Shl | BinOp::Shr => {
            if !is_integer(tcx, lhs_ty) || !is_integer(tcx, rhs_ty) {
                invalid_operands(session, span, binop_symbol(op));
                (tcx.error, lhs, rhs)
            } else {
                let lhs_p = integer_promotion(tcx, lhs_ty, None);
                let rhs_p = integer_promotion(tcx, rhs_ty, None);
                let l = if lhs_ty != lhs_p { push_int_promote(body, lhs, lhs_p) } else { lhs };
                let r = if rhs_ty != rhs_p { push_int_promote(body, rhs, rhs_p) } else { rhs };
                (lhs_p, l, r)
            }
        }
        BinOp::BitAnd | BinOp::BitXor | BinOp::BitOr => {
            if !is_integer(tcx, lhs_ty) || !is_integer(tcx, rhs_ty) {
                invalid_operands(session, span, binop_symbol(op));
                (tcx.error, lhs, rhs)
            } else {
                let common = usual_arithmetic(tcx, lhs_ty, rhs_ty);
                let l = if lhs_ty != common { push_arith_convert(body, lhs, common) } else { lhs };
                let r = if rhs_ty != common { push_arith_convert(body, rhs, common) } else { rhs };
                (common, l, r)
            }
        }
        // Comparisons: result is `int` (0 or 1). Apply usual arithmetic
        // when both sides are arithmetic; otherwise leave the operands
        // alone (pointer comparisons are valid but we don't materialise
        // additional Converts at this stage).
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            if is_arithmetic(tcx, lhs_ty) && is_arithmetic(tcx, rhs_ty) {
                let common = usual_arithmetic(tcx, lhs_ty, rhs_ty);
                let l = if lhs_ty != common { push_arith_convert(body, lhs, common) } else { lhs };
                let r = if rhs_ty != common { push_arith_convert(body, rhs, common) } else { rhs };
                (tcx.int, l, r)
            } else {
                (tcx.int, lhs, rhs)
            }
        }
        // Logical && / ||: scalar operands, result is `int`.
        BinOp::LogAnd | BinOp::LogOr => (tcx.int, lhs, rhs),
    };

    body.exprs[expr_id].ty = result_ty;
    body.exprs[expr_id].value_cat = ValueCat::RValue;
    body.exprs[expr_id].kind = HirExprKind::Binary { op, lhs: lhs_final, rhs: rhs_final };
    expr_id
}

/// Diagnostic for E0083: invalid operands to a binary operator. The
/// operator spelling is included so the diagnostic carries the offending
/// token.
fn invalid_operands(session: &mut Session, span: rcc_span::Span, op_spelling: &str) {
    session
        .handler
        .struct_err(span, format!("invalid operands to binary `{op_spelling}`"))
        .code(rcc_errors::codes::E0083)
        .emit();
}

/// Source spelling of a `BinOp` for diagnostics.
fn binop_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::BitAnd => "&",
        BinOp::BitXor => "^",
        BinOp::BitOr => "|",
        BinOp::LogAnd => "&&",
        BinOp::LogOr => "||",
    }
}

/// Type-check a unary operator. C99 §6.5.3 cases:
///
/// * `+x` / `-x` / `~x`: arithmetic operand, integer promotion applied;
///   result is the promoted type.
/// * `!x`: scalar operand, result is `int`.
/// * `++x` / `--x` / `x++` / `x--`: scalar operand (real or pointer),
///   modifiable lvalue; result is the operand's type as an rvalue.
fn type_unary(
    expr_id: HirExprId,
    op: UnOp,
    operand: HirExprId,
    body: &mut Body,
    tcx: &mut TyCtxt,
    _session: &mut Session,
) -> HirExprId {
    match op {
        UnOp::Plus | UnOp::Neg | UnOp::BitNot => {
            let operand = rvalue_decayed(operand, body, tcx);
            let op_ty = body.exprs[operand].ty;
            let promoted =
                if is_integer(tcx, op_ty) { integer_promotion(tcx, op_ty, None) } else { op_ty };
            let operand = if op_ty != promoted && is_integer(tcx, op_ty) {
                push_int_promote(body, operand, promoted)
            } else {
                operand
            };
            body.exprs[expr_id].ty = promoted;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Unary { op, operand };
            expr_id
        }
        UnOp::LogNot => {
            let operand = rvalue_decayed(operand, body, tcx);
            body.exprs[expr_id].ty = tcx.int;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Unary { op, operand };
            expr_id
        }
        UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => {
            // The operand of ++/-- is a modifiable lvalue. We do NOT apply
            // lvalue-to-rvalue (the read+modify+write is emitted at the
            // CFG layer). Decay similarly does not apply (operand must be
            // a scalar lvalue). The result is the operand's type as an
            // rvalue (post-forms produce the original value, pre-forms
            // produce the new value; both are rvalues per §6.5.3.1p2).
            body.exprs[expr_id].ty = body.exprs[operand].ty;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Unary { op, operand };
            expr_id
        }
    }
}

/// Type-check a `Call` expression. Picks the result type from the
/// callee's function signature (after function-to-pointer decay) and
/// coerces each argument to its declared parameter type. Variadic /
/// unprototyped trailing arguments go through default argument
/// promotions.
fn type_call(
    expr_id: HirExprId,
    callee: HirExprId,
    mut args: Vec<HirExprId>,
    body: &mut Body,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> HirExprId {
    // After `rvalue_decayed`, `callee` should have type `Ptr(Func {..})`.
    let callee_ty = body.exprs[callee].ty;
    let pointee = match *tcx.get(callee_ty) {
        Ty::Ptr(q) => Some(q.ty),
        Ty::Func { .. } => Some(callee_ty),
        _ => None,
    };
    let (ret, params, variadic, proto) = match pointee.map(|p| tcx.get(p).clone()) {
        Some(Ty::Func { ret, params, variadic, proto }) => (ret, params, variadic, proto),
        _ => {
            body.exprs[expr_id].ty = tcx.error;
            body.exprs[expr_id].value_cat = ValueCat::RValue;
            body.exprs[expr_id].kind = HirExprKind::Call { callee, args };
            return expr_id;
        }
    };

    if proto {
        // Coerce each prototyped argument to its parameter type. Surplus
        // args go through default argument promotion (only valid when the
        // function is variadic; the diagnostic for arity mismatch is
        // task 07-11's job).
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(param_ty) = params.get(i) {
                *arg = coerce_to(*arg, *param_ty, body, tcx, session);
            } else if variadic {
                *arg = default_arg_promote(*arg, body, tcx);
            }
        }
    } else {
        // K&R-style prototype-less function: every argument goes through
        // default argument promotions (C99 §6.5.2.2p6).
        for arg in args.iter_mut() {
            *arg = default_arg_promote(*arg, body, tcx);
        }
    }

    body.exprs[expr_id].ty = ret;
    body.exprs[expr_id].value_cat = ValueCat::RValue;
    body.exprs[expr_id].kind = HirExprKind::Call { callee, args };
    expr_id
}

/// Apply default argument promotions to `expr` (C99 §6.5.2.2p6 +
/// §6.3.1.1p2): integers go through integer promotion, and `float`
/// promotes to `double`.
fn default_arg_promote(expr: HirExprId, body: &mut Body, tcx: &mut TyCtxt) -> HirExprId {
    let ty = body.exprs[expr].ty;
    if is_integer(tcx, ty) {
        let promoted = integer_promotion(tcx, ty, None);
        if promoted != ty {
            return push_int_promote(body, expr, promoted);
        }
        return expr;
    }
    if let Ty::Float(FloatKind::F32) = *tcx.get(ty) {
        return push_arith_convert(body, expr, tcx.double);
    }
    expr
}

/// Integer promotion (C99 §6.3.1.1).
///
/// Applied to a value of integer type. The return value is the type the
/// operand should be converted to before further evaluation:
///
/// * For non-bitfield operands (`bit_width == None`):
///   - Any integer type whose conversion rank is **less than** that of `int`
///     (`_Bool`, `char`, `signed char`, `unsigned char`, `short`,
///     `unsigned short`) promotes to `int` if every value of the original
///     type is representable in `int`, otherwise `unsigned int`.
///   - All other integer types are unchanged.
///
/// * For bitfield operands (`bit_width == Some(n)`):
///   - Promotion is governed by the bitfield's value range, not its declared
///     storage type's range. A bitfield of width `n` declared with a signed
///     integer type holds `[-2^(n-1), 2^(n-1) - 1]`; one declared with an
///     unsigned integer type holds `[0, 2^n - 1]`.
///   - If `int` can represent every value of the bitfield → `int`,
///     otherwise → `unsigned int`. By the time a bitfield with rank greater
///     than `int` matters, `n` has already exceeded `INT_BITS`, so the rule
///     "every value representable" still produces the right answer.
///
/// Non-integer types pass through unchanged so callers can chain this with
/// the usual arithmetic conversions blindly.
pub fn integer_promotion(tcx: &TyCtxt, ty: TyId, bit_width: Option<u32>) -> TyId {
    let Ty::Int { signed, rank } = *tcx.get(ty) else {
        return ty;
    };

    if let Some(width) = bit_width {
        // C99 §6.3.1.1p2: a bitfield is promoted based on the values it can
        // actually hold. A zero-width bitfield is not an lvalue and therefore
        // never reaches integer promotion, but we treat it as fitting in `int`
        // for safety (range is the empty set, trivially a subset of `int`).
        return promote_bitfield(tcx, signed, width);
    }

    // Non-bitfield: lookup by rank.
    match rank {
        IntRank::Bool | IntRank::Char | IntRank::Short => {
            // Every value of these types fits in a 32-bit signed `int` on
            // every target rcc cares about, so the answer is always `int`.
            // (`unsigned short` on a 16-bit-int target would map to
            // `unsigned int`; that branch is dead today but kept explicit
            // below for clarity once `INT_BITS` becomes target-dependent.)
            if signed || sub_int_unsigned_fits_in_int(rank) {
                tcx.int
            } else {
                tcx.uint
            }
        }
        IntRank::Int | IntRank::Long | IntRank::LongLong => ty,
    }
}

/// `unsigned char` / `unsigned short` always fit in `int` when
/// `INT_BITS == 32`. Helper exists so the day-`INT_BITS`-becomes-16 edit
/// touches one place.
fn sub_int_unsigned_fits_in_int(rank: IntRank) -> bool {
    match rank {
        // `_Bool` has range {0, 1}; `unsigned char` is at most 8 bits;
        // `unsigned short` is at least 16 bits, but on every modern target
        // (and on every target rcc compiles for) <= INT_BITS - 1.
        IntRank::Bool | IntRank::Char | IntRank::Short => true,
        _ => false,
    }
}

fn promote_bitfield(tcx: &TyCtxt, signed: bool, width: u32) -> TyId {
    // Width 0 is special: non-promotable named bitfields have width >= 1, and
    // unnamed zero-width bitfields are never read. Map to `int` for safety.
    if width == 0 {
        return tcx.int;
    }

    if signed {
        // Signed bitfield value range is [-2^(w-1), 2^(w-1) - 1]. Any width up
        // to `INT_BITS` fits in `int`; widths greater than `INT_BITS` cannot
        // occur in well-formed C99 (bitfield width must not exceed the
        // declared type's width), but if they did the value would still fit
        // when the storage type rank is > Int — and the storage-type rank
        // would already exceed `int`, so falling through to `int` is wrong;
        // however, integer_promotion's contract for storage rank > int is
        // "stay unchanged", which is handled by the early rank check above
        // for non-bitfields. For bitfields of rank > int, the user asked for
        // a sub-int promotion of a wider value; treat as `unsigned int` if
        // it doesn't fit in signed int.
        if width <= INT_BITS {
            tcx.int
        } else {
            tcx.uint
        }
    } else {
        // Unsigned bitfield value range is [0, 2^w - 1]. Fits in signed `int`
        // (which holds [0, 2^(INT_BITS-1) - 1] on the non-negative side) iff
        // `w < INT_BITS`.
        if width < INT_BITS {
            tcx.int
        } else {
            tcx.uint
        }
    }
}

/// Width in bits of an `IntRank` on the LP64 model rcc currently targets.
///
/// Phase 15 (`TargetInfo`) replaces this hard-coded table with a
/// target-driven one. Until then, every backend rcc supports is LP64
/// (`int` = 32, `long` = `long long` = 64, plus 8-bit `char` and 16-bit
/// `short`). Values match what `rcc_codegen_llvm` already emits.
fn int_rank_bits(rank: IntRank) -> u32 {
    match rank {
        IntRank::Bool => 1,
        IntRank::Char => 8,
        IntRank::Short => 16,
        IntRank::Int => INT_BITS,
        IntRank::Long => 64,
        IntRank::LongLong => 64,
    }
}

/// "Unsigned counterpart" of an `IntRank`. For C99 §6.3.1.8 step 4 we may
/// need `unsigned long` from `long`, etc. Helper returns the matching
/// pre-interned `TyId` from the context.
fn unsigned_counterpart(tcx: &TyCtxt, rank: IntRank) -> TyId {
    match rank {
        // `_Bool` is already unsigned; `char`'s unsigned counterpart is
        // `unsigned char`. Neither path is reachable for the §6.3.1.8 rule
        // (their integer-promoted form is `int`/`unsigned int`), but we
        // keep the entries so the helper is total.
        IntRank::Bool => tcx.bool_,
        IntRank::Char => tcx.uchar,
        IntRank::Short => tcx.ushort,
        IntRank::Int => tcx.uint,
        IntRank::Long => tcx.ulong,
        IntRank::LongLong => tcx.ulong_long,
    }
}

/// Usual arithmetic conversions (C99 §6.3.1.8). Returns the common real type.
///
/// Implements the spec ladder verbatim:
///
/// 1. If either operand has `long double` type, the other is converted to
///    `long double`.
/// 2. Otherwise, if either has `double` type, the other → `double`.
/// 3. Otherwise, if either has `float` type, the other → `float`.
/// 4. Otherwise, integer promotions are performed on both operands, then
///    one of the following sub-rules applies:
///    - (4a) If both have the same type, no further conversion is needed.
///    - (4b) If both are signed or both are unsigned, the operand of lesser
///      rank is converted to the type of the operand of greater rank.
///    - (4c.i) Otherwise (exactly one operand is signed, the other
///      unsigned), if the unsigned operand has rank ≥ signed operand's
///      rank, convert the signed operand to the unsigned type.
///    - (4c.ii) Else if the signed type can represent every value of the
///      unsigned type (signed has more value bits), convert the unsigned
///      operand to the signed type.
///    - (4c.iii) Otherwise, both operands are converted to the unsigned
///      counterpart of the signed operand's type.
///
/// `_Complex` arithmetic (C99 §6.3.1.8 second paragraph) is deferred to
/// task 07-12; we only handle real arithmetic here.
///
/// The caller is responsible for actually inserting `Convert` nodes on
/// each operand to bring it to the returned common type.
pub fn usual_arithmetic(tcx: &TyCtxt, a: TyId, b: TyId) -> TyId {
    // Steps 1-3: floating types dominate, in long-double / double / float order.
    match (tcx.get(a), tcx.get(b)) {
        (Ty::Float(FloatKind::F80), _) | (_, Ty::Float(FloatKind::F80)) => return tcx.long_double,
        (Ty::Float(FloatKind::F64), _) | (_, Ty::Float(FloatKind::F64)) => return tcx.double,
        (Ty::Float(FloatKind::F32), _) | (_, Ty::Float(FloatKind::F32)) => return tcx.float,
        _ => {}
    }

    // Step 4: apply integer promotion to both operands.
    let a = integer_promotion(tcx, a, None);
    let b = integer_promotion(tcx, b, None);

    // Decompose both promoted operands into (signed, rank). Non-integer
    // operands (`Ty::Error`, pointers, records) reach this function only
    // through a malformed call; keep it lossy by returning the first
    // operand unchanged so downstream passes can keep going on already
    // poisoned input.
    //
    // `Ty` is not `Copy` (some variants carry a `Vec`), so we destructure
    // through a reference; `signed`/`rank` are themselves `Copy` and bind
    // by value via the default-binding-mode rules.
    let Ty::Int { signed: sa, rank: ra } = tcx.get(a) else { return a };
    let Ty::Int { signed: sb, rank: rb } = tcx.get(b) else { return a };
    let (sa, ra, sb, rb) = (*sa, *ra, *sb, *rb);

    // Step 4a: same type after promotion → done.
    if a == b {
        return a;
    }

    // Step 4b: same signedness → operand of greater rank wins.
    if sa == sb {
        return if ra >= rb { a } else { b };
    }

    // Step 4c: mixed signedness. Identify the signed and unsigned operands.
    let (signed_ty, signed_rank, unsigned_ty, unsigned_rank) =
        if sa { (a, ra, b, rb) } else { (b, rb, a, ra) };

    // Step 4c.i: unsigned rank ≥ signed rank → result is the unsigned type.
    if unsigned_rank >= signed_rank {
        return unsigned_ty;
    }

    // Step 4c.ii: signed rank > unsigned rank. The signed type can represent
    // every value of the unsigned type iff it has strictly more value bits
    // (signed-bits − 1 > unsigned-bits, i.e. signed-bits ≥ unsigned-bits + 2).
    // On LP64 this is true for `long`/`long long` paired with `unsigned int`
    // (64 ≥ 32 + 2) and for any wider signed paired with a strictly narrower
    // unsigned. On a hypothetical LLP64 target where `long` is 32 bits this
    // helper would correctly fall through to step 4c.iii.
    let signed_bits = int_rank_bits(signed_rank);
    let unsigned_bits = int_rank_bits(unsigned_rank);
    if signed_bits >= unsigned_bits + 2 {
        return signed_ty;
    }

    // Step 4c.iii: convert both to the unsigned counterpart of the signed
    // operand's type. (Reached today only on hypothetical non-LP64 targets;
    // included for spec completeness so phase-15 retargeting is one edit.)
    unsigned_counterpart(tcx, signed_rank)
}

/// Syntactic context in which an expression appears, for the purposes of
/// C99 §6.3.2.1p3 / p4 array-and-function decay.
///
/// The default context (`Normal`) decays array lvalues to a pointer to the
/// first element and function designators to a pointer to function. The
/// other variants are the spec's enumerated exceptions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecayContext {
    /// Ordinary use: arrays decay to `&arr[0]`, functions decay to `&func`.
    Normal,
    /// Operand of `sizeof` (C99 §6.3.2.1p3: array case) /
    /// `sizeof` of a function-designator is a constraint violation but we
    /// still decline to decay so the diagnostic can spot the function type.
    SizeofOperand,
    /// Operand of unary `&` (C99 §6.3.2.1p3 array case + p4 function case).
    /// Address-of an array yields a pointer to the array, not to its first
    /// element; address-of a function yields a pointer to the function
    /// (semantically identical to the decayed form, but no `Convert` is
    /// inserted because `&f` and `f` are interchangeable per p4).
    AddrOfOperand,
    /// String literal used to initialise a `char[]` array (C99 §6.7.8p14):
    /// the array initialiser keeps its array type rather than decaying to
    /// `char *`.
    CharArrayInitializer,
}

/// Apply C99 §6.3.2.1p3 (array → pointer) and §6.3.2.1p4 (function →
/// pointer) decay to `expr` if `ctx` permits it. Returns the id of either:
///
/// * the original expression (no decay needed or context forbids it), or
/// * a freshly-pushed `HirExprKind::Convert { kind: ArrayToPtr | FuncToPtr }`
///   wrapper whose `ty` is the decayed pointer type.
///
/// The wrapper's `value_cat` is always `RValue` — both decays produce a
/// non-modifiable rvalue per the spec ("which is not an lvalue").
///
/// `ctx == Normal` is the rule; the other variants encode the three
/// enumerated exceptions in p3/p4. Callers should pass the more specific
/// variant whenever the syntactic position is known. Unknown positions
/// default to `Normal` (the conservative choice — failing to decay where
/// the spec requires decay is a soundness bug; decaying where it isn't
/// required is at worst a missed diagnostic).
pub fn decay_if_needed(
    tcx: &mut TyCtxt,
    body: &mut Body,
    expr: HirExprId,
    ctx: DecayContext,
) -> HirExprId {
    // Look up the operand's type; clone the relevant variants so we can
    // hand `tcx` back as `&mut` for `intern`.
    let (decay_kind, new_ty) = match tcx.get(body.exprs[expr].ty).clone() {
        Ty::Array { elem, .. } if ctx == DecayContext::Normal => {
            (ConvertKind::ArrayToPtr, tcx.intern(Ty::Ptr(elem)))
        }
        Ty::Func { .. } if ctx == DecayContext::Normal => {
            // `func -> &func` is type "pointer to function", with no
            // qualifiers (functions cannot be qualified).
            let func_ty = body.exprs[expr].ty;
            (ConvertKind::FuncToPtr, tcx.intern(Ty::Ptr(Qual::plain(func_ty))))
        }
        // Either the operand is not a candidate for decay, or `ctx` forbids
        // the conversion in this position. In both cases the spec says the
        // expression keeps its original type, so we hand `expr` back
        // verbatim — no Convert wrapper is inserted.
        _ => return expr,
    };

    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: new_ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: decay_kind },
    });
    body.exprs[id].id = id;
    id
}

/// Compute the value category of `expr` per C99 §6.3.2.1.
///
/// An *lvalue* is an expression that designates an object; an *rvalue*
/// (or, in the standard's wording, the value of an expression that is
/// not an lvalue) is everything else — including the value-producing
/// result of a cast, a function call, a binary operator, address-of,
/// etc. C99 §6.3.2.1p1 enumerates the lvalue-producing forms; this
/// function is the canonical encoder of that table for HIR.
///
/// The classification is computed *from the kind*, not read from
/// `HirExpr::value_cat`: lowering writes a best-guess category as the
/// nodes are produced, but the type-checker must own the final answer
/// because lowering does not have full type information yet (e.g. the
/// distinction between a function designator and a regular identifier
/// depends on the resolved `DefKind`).
///
/// The rules implemented here are:
///
/// | HIR kind                                        | Category |
/// |-------------------------------------------------|----------|
/// | `IntConst`, `FloatConst`                         | rvalue   |
/// | `StringRef`                                      | lvalue   |
/// | `LocalRef`, `DefRef`                             | lvalue   |
/// | `Deref(p)` (i.e. `*p`)                           | lvalue   |
/// | `Index { base, .. }` (`a[i]` lowered to `*(a+i)`)| lvalue   |
/// | `Field { base, .. }` (`s.f`, `p->f`)             | inherits from `base` |
/// | `Convert { kind: LvalueToRvalue }`              | rvalue   |
/// | `Convert { kind: ArrayToPtr | FuncToPtr }`      | rvalue   |
/// | other `Convert { .. }`                          | rvalue   |
/// | `Cast { .. }`                                   | rvalue   |
/// | `Binary`, `Unary`, `Call`                       | rvalue   |
/// | `AddressOf`                                     | rvalue   |
/// | `Cond`, `Comma`, `Assign`                       | rvalue   |
///
/// Notes:
/// - `Field` follows the base because C99 §6.5.2.3p3 says `s.f` is an
///   lvalue iff `s` is. The `p->f` case is always an lvalue and is
///   already represented as `Field { base: Deref(p), .. }` in HIR, so
///   the recursive rule produces the right answer without a special
///   case.
/// - Pre/post increment and decrement are *rvalues*: they produce the
///   updated (or original) value, not an lvalue designating the
///   modified object (C99 §6.5.3.1p2 and §6.5.2.4p2). They're carried
///   in `Unary`, which uniformly returns rvalue.
/// - Assignment expressions (`a = b`) are rvalues per C99 §6.5.16p3.
pub fn value_category(body: &Body, expr: HirExprId) -> ValueCat {
    match body.exprs[expr].kind {
        // Constants and arithmetic / pointer-producing operators.
        HirExprKind::IntConst(_)
        | HirExprKind::FloatConst(_)
        | HirExprKind::Binary { .. }
        | HirExprKind::Unary { .. }
        | HirExprKind::Call { .. }
        | HirExprKind::Cast { .. }
        | HirExprKind::AddressOf(_)
        | HirExprKind::Cond { .. }
        | HirExprKind::Comma { .. }
        | HirExprKind::Assign { .. }
        | HirExprKind::Convert { .. } => ValueCat::RValue,

        // Identifier-style designators are lvalues. String literals are
        // arrays of `char` (with static storage duration) and §6.4.5p6
        // makes them lvalues that decay to pointers in most contexts.
        HirExprKind::LocalRef(_)
        | HirExprKind::DefRef(_)
        | HirExprKind::StringRef(_)
        | HirExprKind::Deref(_)
        | HirExprKind::Index { .. } => ValueCat::LValue,

        // `s.f` is an lvalue iff `s` is. `p->f` is lowered as
        // `Field { base: Deref(p), .. }`, so this also covers it.
        HirExprKind::Field { base, .. } => value_category(body, base),
    }
}

/// Apply the C99 §6.3.2.1p2 lvalue-to-rvalue conversion to `expr` if
/// needed. Returns the id of either:
///
/// * the original expression, or
/// * a freshly-pushed `Convert { kind: LvalueToRvalue }` wrapper whose
///   type strips top-level qualifiers (§6.3.2.1p2: "the value has the
///   unqualified version of the type of the lvalue") and whose
///   `value_cat` is `RValue`.
///
/// The conversion is *not* applied to:
///
/// * expressions of array type — those decay via `decay_if_needed`
///   (§6.3.2.1p3) and the lvalue-to-rvalue rule explicitly excludes
///   them ("except when it is the operand of … or is an array");
/// * expressions that are already rvalues (no-op);
/// * function designators — handled by `decay_if_needed`.
///
/// Callers responsible for context-specific exemptions (operand of
/// `sizeof`, `&`, the LHS of `=` / `op=`, `++`/`--`) must simply not
/// call this helper in those positions. The helper is the unconditional
/// "force this position to an rvalue" primitive; the calling-side
/// decision of whether to force is in task 07-07.
pub fn lvalue_to_rvalue_if_needed(tcx: &mut TyCtxt, body: &mut Body, expr: HirExprId) -> HirExprId {
    if value_category(body, expr) == ValueCat::RValue {
        return expr;
    }

    let orig_ty = body.exprs[expr].ty;

    // Arrays and functions don't take this path (they decay first).
    // We're conservative here: if the operand still has array/function
    // type by the time we're invoked, leave it alone — `decay_if_needed`
    // is the right tool.
    match tcx.get(orig_ty) {
        Ty::Array { .. } | Ty::Func { .. } => return expr,
        _ => {}
    }

    // C99 §6.3.2.1p2: the converted value has the *unqualified* version
    // of the lvalue's type. For our `Ty` model qualifiers ride on the
    // pointee inside `Ptr` / `Array::elem`; the top-level `TyId` for an
    // ordinary scalar already has no qualifiers, so no rewrite is
    // required. Pointer-to-qualified-T stays pointer-to-qualified-T:
    // the qualifier belongs to the pointee, not the pointer value.
    let new_ty = orig_ty;
    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: new_ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: ConvertKind::LvalueToRvalue },
    });
    body.exprs[id].id = id;
    id
}

/// Verify that `lhs` is an lvalue, suitable as the destination of an
/// assignment (`=` or any compound `op=`). Emits E0080 ("assignment to
/// rvalue") when the LHS is not an lvalue and returns `false`. The
/// caller is then free to either keep going (the typechecker will paper
/// over the constraint violation downstream) or skip further checks on
/// the offending statement.
///
/// This helper covers C99 §6.5.16p2's *lvalue* requirement only. The
/// orthogonal *modifiable*-lvalue requirement (no const-qualified
/// objects, no array types, no incomplete types, no const-qualified
/// member of a struct/union, …) lives in task 07-05.
pub fn check_assignment_lhs(session: &mut Session, body: &Body, lhs: HirExprId) -> bool {
    if value_category(body, lhs) == ValueCat::LValue {
        return true;
    }

    let span = body.exprs[lhs].span;
    session
        .handler
        .struct_err(span, "assignment to rvalue: left operand must designate an object")
        .code(rcc_errors::codes::E0080)
        .emit();
    false
}

// ---------------------------------------------------------------------
// Assignment compatibility (C99 §6.5.16.1)
// ---------------------------------------------------------------------

/// Outcome of [`is_assignable`]. The two non-`Incompatible` variants
/// flag the conversion as well-formed *but worth a warning* — the
/// caller is expected to forward `Narrowing` to W0008 and
/// `QualifierLoss` to E0081 (the spec treats discarding qualifiers as
/// a constraint violation, not a warning, but we keep them as separate
/// cases so downstream callers can choose finer-grained messaging in
/// task 07-07).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssignError {
    /// The two types do not match any §6.5.16.1p1 bullet.
    Incompatible,
    /// Arithmetic-to-arithmetic conversion that loses range or precision.
    /// The assignment is still legal (C99 silently allows it); the
    /// caller emits W0008.
    Narrowing,
    /// Pointer-to-pointer assignment where the destination's pointee
    /// type does not include every qualifier on the source's pointee.
    /// C99 §6.5.16.1p1 third bullet treats this as a constraint
    /// violation, but we report it separately so callers can produce a
    /// more specific diagnostic (\"discards `const` qualifier\").
    QualifierLoss,
}

/// True if `expr` is a *null pointer constant* per C99 §6.3.2.3p3:
/// "An integer constant expression with the value 0, or such an
/// expression cast to type `void *`".
///
/// In HIR a literal `0` lowers to [`HirExprKind::IntConst(0)`]; an
/// explicit `(void *)0` lowers to a [`HirExprKind::Cast`] over the
/// same; the type-checker's own [`ConvertKind::Pointer`] /
/// [`ConvertKind::IntegerPromotion`] / [`ConvertKind::UsualArithmetic`]
/// wrappers may also sit on top once task 07-07 lands. We unwrap
/// `Cast` and `Convert` recursively so the test stays robust as more
/// implicit conversions accrue.
pub fn is_null_pointer_constant(body: &Body, expr: HirExprId) -> bool {
    match body.exprs[expr].kind {
        HirExprKind::IntConst(0) => true,
        HirExprKind::Cast { operand, .. } => is_null_pointer_constant(body, operand),
        HirExprKind::Convert { operand, .. } => is_null_pointer_constant(body, operand),
        _ => false,
    }
}

/// True iff `a` and `b` denote *compatible types* per C99 §6.2.7.
///
/// In our HIR every type is interned via [`TyCtxt::intern`], so equal
/// `TyId`s always denote the same `Ty` and are therefore compatible.
/// The non-trivial cases — pointer / array / function compatibility,
/// tagged-type definitions across translation units — all bottom out
/// in `TyId` equality once interning has done its job:
///
/// * Pointers are compatible iff their pointees are compatible
///   (qualifiers must match).
/// * Arrays are compatible iff their element types are compatible and
///   any specified lengths agree.
/// * Function types are compatible iff return types match and every
///   parameter pair matches (after default argument promotions).
/// * `Ty::Record(DefId)` carries a single `DefId`, so two records
///   compare equal iff they refer to the same definition — which is
///   what §6.2.7 demands within a single translation unit.
///
/// The interner gives us all of this for free: `tcx.intern(ty)` is
/// idempotent, so any two structurally-identical `Ty` values share a
/// single `TyId`. We expose the helper as a named function anyway so
/// callers (and future cross-TU compatibility logic) have a stable
/// extension point.
pub fn is_compatible_type(_tcx: &TyCtxt, a: TyId, b: TyId) -> bool {
    a == b
}

/// Width in bits of an integer rank. Re-exported helper so the
/// narrowing classifier can share it with [`usual_arithmetic`].
fn integer_bits(rank: IntRank) -> u32 {
    int_rank_bits(rank)
}

/// Width in bits of a float kind's mantissa (number of value bits the
/// significand can represent exactly). Used to decide when an integer
/// → float conversion loses precision.
fn float_significand_bits(kind: FloatKind) -> u32 {
    match kind {
        // IEEE 754 binary32: 1 implicit + 23 explicit fraction bits.
        FloatKind::F32 => 24,
        // IEEE 754 binary64: 1 implicit + 52 explicit fraction bits.
        FloatKind::F64 => 53,
        // x87 extended precision: 64 explicit bits, no implicit bit.
        // For our LP64 / x86_64 target this is the long double layout.
        FloatKind::F80 => 64,
    }
}

/// "Width" of a float kind for descending-precision narrowing checks
/// (`double → float` is narrowing, `float → double` is not).
fn float_rank(kind: FloatKind) -> u32 {
    match kind {
        FloatKind::F32 => 0,
        FloatKind::F64 => 1,
        FloatKind::F80 => 2,
    }
}

/// True iff converting a value of type `src` to type `dst` may lose
/// information at run time. Both types are assumed to be arithmetic
/// (integer or float); records, pointers, void, and the error sentinel
/// must be filtered out before calling this.
fn is_narrowing_arithmetic(tcx: &TyCtxt, src: TyId, dst: TyId) -> bool {
    if src == dst {
        return false;
    }
    let src_ty = tcx.get(src);
    let dst_ty = tcx.get(dst);
    match (src_ty, dst_ty) {
        // Integer → integer.
        (Ty::Int { signed: ss, rank: sr }, Ty::Int { signed: ds, rank: dr }) => {
            let sb = integer_bits(*sr);
            let db = integer_bits(*dr);
            match (*ss, *ds) {
                // Same signedness: narrowing iff dst is strictly narrower.
                (true, true) | (false, false) => sb > db,
                // Signed src → unsigned dst: negatives become huge values, so
                // *every* such conversion can lose information regardless of width.
                (true, false) => true,
                // Unsigned src → signed dst: dst must have at least one extra
                // bit to hold every unsigned value (sign bit). Otherwise narrowing.
                (false, true) => sb >= db,
            }
        }
        // Float → float: narrowing iff dst rank is lower.
        (Ty::Float(s), Ty::Float(d)) => float_rank(*s) > float_rank(*d),
        // Integer → float: narrowing iff the integer's value bits exceed
        // the float's significand width.
        (Ty::Int { signed, rank }, Ty::Float(f)) => {
            let int_bits = integer_bits(*rank);
            let value_bits = if *signed { int_bits.saturating_sub(1) } else { int_bits };
            value_bits > float_significand_bits(*f)
        }
        // Float → integer: always narrowing (the fractional part is lost,
        // and the integer range may not cover the float's range either).
        (Ty::Float(_), Ty::Int { .. }) => true,
        // Anything else (including `_Complex` — task 07-12) we conservatively
        // call non-narrowing; the caller will already have rejected the
        // assignment via `is_assignable` if the types are otherwise
        // incompatible.
        _ => false,
    }
}

/// True iff `a` is an arithmetic type per C99 §6.2.5p18 (integer or
/// floating, real or complex). `_Complex` is included because §6.5.16.1
/// treats every arithmetic type uniformly.
fn is_arithmetic(tcx: &TyCtxt, a: TyId) -> bool {
    matches!(tcx.get(a), Ty::Int { .. } | Ty::Float(_) | Ty::Complex(_))
}

/// True iff `a` is a pointer type.
fn is_pointer(tcx: &TyCtxt, a: TyId) -> bool {
    matches!(tcx.get(a), Ty::Ptr(_))
}

/// Pointee `Qual` of a pointer type, or `None` for non-pointers.
fn pointee_qual(tcx: &TyCtxt, a: TyId) -> Option<Qual> {
    match *tcx.get(a) {
        Ty::Ptr(q) => Some(q),
        _ => None,
    }
}

/// True iff every qualifier set on `inner` is also set on `outer`
/// (i.e. `outer ⊇ inner`). C99 §6.5.16.1p1 third bullet requires the
/// destination pointee's qualifiers to be a *superset* of the source
/// pointee's, so writing through the destination cannot drop a qualifier
/// the source promised.
fn qualifiers_superset(outer: Qual, inner: Qual) -> bool {
    (!inner.is_const || outer.is_const)
        && (!inner.is_volatile || outer.is_volatile)
        && (!inner.is_restrict || outer.is_restrict)
}

/// True iff `t` is `void` (possibly via a `Qual` wrapper at the call
/// site; this helper takes the bare `TyId`).
fn is_void(tcx: &TyCtxt, t: TyId) -> bool {
    matches!(tcx.get(t), Ty::Void)
}

/// True iff `t` is an "object type" or incomplete type in the sense of
/// §6.5.16.1p1 fourth bullet — anything that is not a function type.
fn is_object_or_incomplete(tcx: &TyCtxt, t: TyId) -> bool {
    !matches!(tcx.get(t), Ty::Func { .. })
}

/// Check the C99 §6.5.16.1 simple-assignment constraint. Returns
/// `Ok(())` when the assignment, function-call argument, return
/// statement, or initializer is well-formed; otherwise yields an
/// [`AssignError`] describing how the constraint is violated.
///
/// The six cases of §6.5.16.1p1 are matched in spec order:
///
/// 1. Both operands have arithmetic type — accepted; flagged as
///    [`AssignError::Narrowing`] when the source's value range or
///    precision does not fit in the destination.
/// 2. Both operands have a compatible struct or union type — accepted.
/// 3. Both operands are pointers to compatible (possibly differently
///    qualified) types, with the destination pointee carrying every
///    qualifier of the source pointee. Flagged as
///    [`AssignError::QualifierLoss`] when the pointee types are
///    compatible but qualifiers narrow.
/// 4. One operand is a pointer to an object/incomplete type and the
///    other is a pointer to (qualified or unqualified) `void`, with
///    the same qualifier-superset rule on the destination side.
/// 5. The destination is a pointer and the source expression is a
///    *null pointer constant* (see [`is_null_pointer_constant`]).
/// 6. The destination has type `_Bool` and the source has any pointer
///    type.
///
/// All other shapes are [`AssignError::Incompatible`]. The function is
/// intentionally pure — it does *not* emit diagnostics; the caller
/// (task 07-07) decides how to surface the result.
pub fn is_assignable(
    tcx: &TyCtxt,
    body: &Body,
    dst: TyId,
    src_ty: TyId,
    src_expr: HirExprId,
) -> Result<(), AssignError> {
    // Same TyId: trivially assignable. Catches `int = int`, `T* = T*`,
    // `struct S = struct S`. No conversion is required.
    if dst == src_ty {
        return Ok(());
    }

    // Bullet 1: arithmetic ↔ arithmetic.
    if is_arithmetic(tcx, dst) && is_arithmetic(tcx, src_ty) {
        if is_narrowing_arithmetic(tcx, src_ty, dst) {
            return Err(AssignError::Narrowing);
        }
        return Ok(());
    }

    // Bullet 6: _Bool ← any pointer (C99 §6.3.1.2 + §6.5.16.1p1 last
    // bullet). Match this *before* the pointer rules so a pointer source
    // doesn't accidentally fall into bullet 5 via the dst-is-pointer
    // shortcut. Note: dst != src_ty by the early-return above, so this
    // only fires when one side is a real pointer and the other is _Bool.
    if dst == tcx.bool_ && is_pointer(tcx, src_ty) {
        return Ok(());
    }

    // Bullet 2: struct / union of compatible types. Records are interned
    // by `DefId`, so compatibility reduces to TyId equality — already
    // handled by the early-return at the top. We keep an explicit branch
    // here so a future cross-TU "compatible record" rule has a hook.
    if let (Ty::Record(da), Ty::Record(db)) = (tcx.get(dst), tcx.get(src_ty)) {
        if da == db {
            return Ok(());
        }
        return Err(AssignError::Incompatible);
    }

    // Bullets 3 + 4 + 5: pointer-shaped destination.
    if let Some(dst_pointee) = pointee_qual(tcx, dst) {
        // Bullet 5: null pointer constant.
        if is_null_pointer_constant(body, src_expr) {
            return Ok(());
        }

        // Both operands must be pointers from here on; otherwise it's
        // incompatible. (Integer-to-pointer assignment of a non-null
        // constant is a constraint violation in C99.)
        let Some(src_pointee) = pointee_qual(tcx, src_ty) else {
            return Err(AssignError::Incompatible);
        };

        // Bullet 4: void* ↔ object-pointer.
        let dst_is_void_ptr = is_void(tcx, dst_pointee.ty);
        let src_is_void_ptr = is_void(tcx, src_pointee.ty);
        if (dst_is_void_ptr && is_object_or_incomplete(tcx, src_pointee.ty))
            || (src_is_void_ptr && is_object_or_incomplete(tcx, dst_pointee.ty))
        {
            // Qualifier rule still applies: writing through `dst` must
            // not drop a qualifier the source promised.
            if !qualifiers_superset(dst_pointee, src_pointee) {
                return Err(AssignError::QualifierLoss);
            }
            return Ok(());
        }

        // Bullet 3: pointer-to-compatible-types, qualifier superset.
        if is_compatible_type(tcx, dst_pointee.ty, src_pointee.ty) {
            if !qualifiers_superset(dst_pointee, src_pointee) {
                return Err(AssignError::QualifierLoss);
            }
            return Ok(());
        }

        return Err(AssignError::Incompatible);
    }

    Err(AssignError::Incompatible)
}

// ---------------------------------------------------------------------
// Pointer conversions (C99 §6.3.2.3)
// ---------------------------------------------------------------------

/// Outcome of [`pointer_convert`]: a structural reason why the implicit
/// conversion is rejected. Successful conversions return the converted
/// `HirExprId` directly; this enum catalogues only the failure modes
/// callers may want to surface as different diagnostics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConvertError {
    /// Source and destination are pointers but not interchangeable per
    /// any §6.3.2.3 bullet — most commonly two object pointers with
    /// unrelated pointee types (`int *` ↔ `char *`), or two function
    /// pointers with incompatible signatures. Caller emits **E0082**.
    Incompatible,
    /// Source and destination are pointer-compatible (or one is
    /// `void *`), but the destination's pointee qualifier set drops a
    /// qualifier the source promised. C99 §6.3.2.3p2 / §6.5.16.1p1
    /// third bullet treat this as a constraint violation; an explicit
    /// cast is required to suppress it. Caller emits **E0082**.
    QualifierLoss,
    /// One operand is a pointer and the other is an integer that is
    /// *not* a null pointer constant. C99 §6.3.2.3p5 / §6.3.2.3p6 say
    /// the conversion is implementation-defined and requires an
    /// explicit cast. Caller emits **E0082**.
    IntegerPointerMix,
}

/// Apply a C99 §6.3.2.3 pointer conversion to `src` so its value can
/// stand in for an expression of type `dst_ty`. The helper covers
/// every bullet of §6.3.2.3:
///
/// 1. **Null pointer constant ↔ pointer.** An integer constant
///    expression with value `0` (or such an expression cast to
///    `void *`) converts to any pointer type as the null pointer.
///    The resulting wrapper has type `dst_ty` and `kind: Pointer`.
/// 2. **`void *` ↔ object pointer.** Any pointer to an
///    object/incomplete type may be converted to/from a pointer to
///    `void`, with qualifier additions on the destination pointee
///    permitted. Function pointers do *not* qualify (§6.3.2.3p8).
/// 3. **Pointer to compatible-pointee object types.** Two object
///    pointers are interchangeable when their pointee types are
///    compatible (here: identical, since `is_compatible_type`
///    bottoms out in `TyId` equality after interning) and the
///    destination's pointee qualifier set is a superset of the
///    source's.
/// 4. **Pointer to function ↔ pointer to function.** Two function
///    pointers are interchangeable iff their pointee function types
///    are *compatible* — same return type, same parameter list (after
///    default argument promotions), same variadicity.
/// 5. **Integer ↔ pointer.** Implementation-defined per §6.3.2.3p5/6;
///    rcc demands an explicit cast and rejects the implicit form
///    (except for the null-pointer-constant case in bullet 1).
///
/// Returns:
///
/// * `Ok(src)` — types already match (`dst_ty == src_ty`); no wrapper
///   inserted. The caller can treat the result as identical to the
///   input.
/// * `Ok(new_id)` — a freshly pushed `HirExprKind::Convert { kind:
///   ConvertKind::Pointer }` wrapper with type `dst_ty` and
///   `value_cat: RValue`.
/// * `Err(ConvertError::*)` — the conversion is ill-formed; the caller
///   chooses the diagnostic (E0082 in every case for now).
///
/// The helper is purely structural — it inspects `tcx`/`body` but
/// emits no diagnostics. Diagnostics are routed through task 07-07,
/// which is the central caller of this helper for assignment / call
/// argument / return / initializer positions.
pub fn pointer_convert(
    tcx: &mut TyCtxt,
    body: &mut Body,
    src: HirExprId,
    dst_ty: TyId,
) -> Result<HirExprId, ConvertError> {
    let src_ty = body.exprs[src].ty;

    // Trivial: types already equal (after interning). No conversion
    // needed; keep the original id so callers can reason about
    // identity.
    if src_ty == dst_ty {
        return Ok(src);
    }

    // Destination must be a pointer for §6.3.2.3 to apply at all. If
    // it isn't, fall through to the IntegerPointerMix / Incompatible
    // distinction so the caller can produce a precise diagnostic.
    let Some(dst_pointee) = pointee_qual(tcx, dst_ty) else {
        // Source is a pointer, dest is not — it's the "pointer to
        // integer" half of §6.3.2.3p6. Without an explicit cast, this
        // is rejected. Pointer to non-pointer non-integer (struct,
        // float, …) bottoms out at Incompatible.
        if is_pointer(tcx, src_ty) {
            if is_integer(tcx, dst_ty) {
                return Err(ConvertError::IntegerPointerMix);
            }
            return Err(ConvertError::Incompatible);
        }
        // Neither side is a pointer: not our problem. Caller is
        // misusing the helper; report Incompatible so it still gets
        // an error code.
        return Err(ConvertError::Incompatible);
    };

    // Bullet 1: null pointer constant → any pointer type.
    if is_null_pointer_constant(body, src) {
        return Ok(push_pointer_convert(body, src, dst_ty));
    }

    // Source must be a pointer from here on. If it's an integer,
    // we're in §6.3.2.3p5 territory ("integer to pointer"); rcc
    // requires an explicit cast.
    let Some(src_pointee) = pointee_qual(tcx, src_ty) else {
        if is_integer(tcx, src_ty) {
            return Err(ConvertError::IntegerPointerMix);
        }
        return Err(ConvertError::Incompatible);
    };

    let dst_is_func = matches!(tcx.get(dst_pointee.ty), Ty::Func { .. });
    let src_is_func = matches!(tcx.get(src_pointee.ty), Ty::Func { .. });

    // Bullet 4: function-pointer ↔ function-pointer. Both sides must
    // be function pointers, and the pointee function types must be
    // compatible. A function-pointer / object-pointer mix is
    // explicitly disallowed (§6.3.2.3p8) — fall through to
    // Incompatible below if exactly one side is a function pointer.
    if src_is_func && dst_is_func {
        if is_compatible_type(tcx, src_pointee.ty, dst_pointee.ty) {
            // Function types are unqualified — qualifiers on a
            // function-pointer's pointee are not meaningful, so
            // qualifier-superset is vacuously satisfied. We still
            // emit the wrapper so type-equality at the use site
            // matches the destination.
            return Ok(push_pointer_convert(body, src, dst_ty));
        }
        return Err(ConvertError::Incompatible);
    }
    if src_is_func || dst_is_func {
        return Err(ConvertError::Incompatible);
    }

    // From here on both sides are object/incomplete pointers.
    let dst_is_void_ptr = is_void(tcx, dst_pointee.ty);
    let src_is_void_ptr = is_void(tcx, src_pointee.ty);

    // Bullet 2: `void *` ↔ object pointer. Permit either direction
    // when the *other* side is an object/incomplete pointer (we
    // already excluded function pointers above, so any non-void
    // pointee is an object/incomplete pointee).
    if dst_is_void_ptr || src_is_void_ptr {
        if !qualifiers_superset(dst_pointee, src_pointee) {
            return Err(ConvertError::QualifierLoss);
        }
        return Ok(push_pointer_convert(body, src, dst_ty));
    }

    // Bullet 3: two object pointers, pointee types must be
    // compatible. After interning, that's TyId equality on the
    // (unqualified) pointee.
    if is_compatible_type(tcx, src_pointee.ty, dst_pointee.ty) {
        if !qualifiers_superset(dst_pointee, src_pointee) {
            return Err(ConvertError::QualifierLoss);
        }
        return Ok(push_pointer_convert(body, src, dst_ty));
    }

    Err(ConvertError::Incompatible)
}

/// Push a `ConvertKind::Pointer` wrapper around `src` with destination
/// type `dst_ty`. The wrapper inherits `src`'s span and is always an
/// rvalue (a converted pointer is the value of the conversion, not
/// an lvalue designating the original object).
fn push_pointer_convert(body: &mut Body, src: HirExprId, dst_ty: TyId) -> HirExprId {
    let span = body.exprs[src].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: dst_ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: src, kind: ConvertKind::Pointer },
    });
    body.exprs[id].id = id;
    id
}

/// True iff `t` is an integer type per C99 §6.2.5p17.
fn is_integer(tcx: &TyCtxt, t: TyId) -> bool {
    matches!(tcx.get(t), Ty::Int { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Truth table for non-bitfield integer promotion.
    /// (input alias getter, expected output alias getter)
    #[test]
    fn integer_promotion_truth_table_non_bitfield() {
        let tcx = TyCtxt::new();

        // (description, input, expected)
        let cases: &[(&str, TyId, TyId)] = &[
            ("_Bool -> int", tcx.bool_, tcx.int),
            ("signed char -> int", tcx.schar, tcx.int),
            ("char -> int", tcx.char_, tcx.int),
            ("unsigned char -> int", tcx.uchar, tcx.int),
            ("short -> int", tcx.short, tcx.int),
            ("unsigned short -> int", tcx.ushort, tcx.int),
            ("int -> int (unchanged)", tcx.int, tcx.int),
            ("unsigned int -> unsigned int (unchanged)", tcx.uint, tcx.uint),
            ("long -> long (unchanged)", tcx.long, tcx.long),
            ("unsigned long -> unsigned long (unchanged)", tcx.ulong, tcx.ulong),
            ("long long -> long long (unchanged)", tcx.long_long, tcx.long_long),
            ("unsigned long long -> unsigned long long", tcx.ulong_long, tcx.ulong_long),
        ];

        for (desc, input, expected) in cases {
            let got = integer_promotion(&tcx, *input, None);
            assert_eq!(got, *expected, "{desc}");
        }
    }

    #[test]
    fn integer_promotion_passes_through_non_integer_types() {
        let tcx = TyCtxt::new();
        // void / float / double / long double / error all pass through.
        for ty in [tcx.void, tcx.float, tcx.double, tcx.long_double, tcx.error] {
            assert_eq!(integer_promotion(&tcx, ty, None), ty);
        }
    }

    #[test]
    fn integer_promotion_unsigned_int_bitfield_3bit_to_int() {
        // Acceptance criterion from the task: a 3-bit unsigned int bitfield
        // promotes to `int`, since its range [0, 7] fits in int.
        let tcx = TyCtxt::new();
        let got = integer_promotion(&tcx, tcx.uint, Some(3));
        assert_eq!(got, tcx.int);
    }

    #[test]
    fn integer_promotion_bitfield_unsigned_widths() {
        let tcx = TyCtxt::new();

        // Unsigned bitfield: fits in signed int iff width <= INT_BITS - 1 = 31.
        for width in 1..=31u32 {
            let got = integer_promotion(&tcx, tcx.uint, Some(width));
            assert_eq!(got, tcx.int, "unsigned int : {width} should promote to int");
        }
        // 32-bit unsigned bitfield exceeds signed int range -> unsigned int.
        let got = integer_promotion(&tcx, tcx.uint, Some(32));
        assert_eq!(got, tcx.uint);
    }

    #[test]
    fn integer_promotion_bitfield_signed_widths() {
        let tcx = TyCtxt::new();

        // Signed bitfield always fits in signed int up to INT_BITS = 32 bits.
        for width in 1..=32u32 {
            let got = integer_promotion(&tcx, tcx.int, Some(width));
            assert_eq!(got, tcx.int, "signed int : {width} should promote to int");
        }
    }

    #[test]
    fn integer_promotion_bitfield_storage_rank_governs_signedness() {
        // The C99 rule says rank/signedness is "determined by the declared
        // type" — so an `unsigned char : 4` bitfield is treated with the
        // unsigned-range formula even though the natural promotion of
        // `unsigned char` (no bitfield) is also `int`.
        let tcx = TyCtxt::new();

        // unsigned char : 4 -> [0, 15] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.uchar, Some(4)), tcx.int);
        // signed char : 4 -> [-8, 7] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.schar, Some(4)), tcx.int);
        // unsigned short : 16 -> [0, 65535] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.ushort, Some(16)), tcx.int);
        // _Bool : 1 -> {0, 1} fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.bool_, Some(1)), tcx.int);
    }

    #[test]
    fn integer_promotion_bitfield_zero_width_maps_to_int() {
        // Width-0 bitfields are never read, but if integer_promotion is
        // accidentally invoked on one we must not panic and we must produce
        // something sensible.
        let tcx = TyCtxt::new();
        assert_eq!(integer_promotion(&tcx, tcx.uint, Some(0)), tcx.int);
        assert_eq!(integer_promotion(&tcx, tcx.int, Some(0)), tcx.int);
    }

    #[test]
    fn usual_arithmetic_still_works_after_signature_change() {
        // Smoke-test: usual_arithmetic was the in-tree caller that needed
        // updating. Make sure char + char still yields int.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.char_, tcx.char_), tcx.int);
        assert_eq!(usual_arithmetic(&tcx, tcx.short, tcx.uint), tcx.uint);
        assert_eq!(usual_arithmetic(&tcx, tcx.long, tcx.int), tcx.long);
    }

    /// Acceptance criteria spelled out in the task file.
    #[test]
    fn usual_arithmetic_acceptance_signed_int_op_unsigned_int() {
        // Step 4c.i (equal rank, mixed signedness): result is `unsigned int`.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.int, tcx.uint), tcx.uint);
        assert_eq!(usual_arithmetic(&tcx, tcx.uint, tcx.int), tcx.uint);
    }

    #[test]
    fn usual_arithmetic_acceptance_long_op_unsigned_int_lp64() {
        // Step 4c.ii: on LP64, `long` has 64 bits and can represent every
        // value of 32-bit `unsigned int`, so the result is `long`.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.long, tcx.uint), tcx.long);
        assert_eq!(usual_arithmetic(&tcx, tcx.uint, tcx.long), tcx.long);
    }

    /// Truth-table for §6.3.1.8 across the 13 scalar types. Checks every
    /// rule (steps 1-9) at least twice with both orderings (a,b) and (b,a)
    /// to make sure the implementation is symmetric.
    ///
    /// The 13 types per the spec are:
    ///   long double, double, float,
    ///   long long, unsigned long long,
    ///   long, unsigned long,
    ///   int, unsigned int,
    ///   short, unsigned short,
    ///   char, _Bool.
    ///
    /// We do not literally enumerate 169 pairs — instead the table encodes
    /// representative cases for every C99 sub-rule.
    #[test]
    fn usual_arithmetic_truth_table_13_scalars() {
        let tcx = TyCtxt::new();

        // Each row: (description, lhs, rhs, expected common type).
        // The implementation must be symmetric, so we feed each row twice
        // (a,b) and (b,a). Cells where lhs == rhs are not duplicated.
        let cases: &[(&str, TyId, TyId, TyId)] = &[
            // ---- Step 1: long double dominates everything. ----
            ("long double / long double", tcx.long_double, tcx.long_double, tcx.long_double),
            ("long double / double", tcx.long_double, tcx.double, tcx.long_double),
            ("long double / float", tcx.long_double, tcx.float, tcx.long_double),
            ("long double / int", tcx.long_double, tcx.int, tcx.long_double),
            ("long double / unsigned long long", tcx.long_double, tcx.ulong_long, tcx.long_double),
            ("long double / _Bool", tcx.long_double, tcx.bool_, tcx.long_double),
            // ---- Step 2: double beats float and any integer. ----
            ("double / double", tcx.double, tcx.double, tcx.double),
            ("double / float", tcx.double, tcx.float, tcx.double),
            ("double / unsigned long", tcx.double, tcx.ulong, tcx.double),
            ("double / char", tcx.double, tcx.char_, tcx.double),
            // ---- Step 3: float beats any integer. ----
            ("float / float", tcx.float, tcx.float, tcx.float),
            ("float / long long", tcx.float, tcx.long_long, tcx.float),
            ("float / unsigned int", tcx.float, tcx.uint, tcx.float),
            ("float / _Bool", tcx.float, tcx.bool_, tcx.float),
            // ---- Step 4a: integer promotion brings both to the same type. ----
            ("_Bool / _Bool -> int", tcx.bool_, tcx.bool_, tcx.int),
            ("char / char -> int", tcx.char_, tcx.char_, tcx.int),
            ("short / short -> int", tcx.short, tcx.short, tcx.int),
            ("unsigned short / unsigned short -> int", tcx.ushort, tcx.ushort, tcx.int),
            ("char / short -> int (both promote to int)", tcx.char_, tcx.short, tcx.int),
            ("_Bool / unsigned short -> int", tcx.bool_, tcx.ushort, tcx.int),
            ("int / int", tcx.int, tcx.int, tcx.int),
            ("unsigned int / unsigned int", tcx.uint, tcx.uint, tcx.uint),
            ("long / long", tcx.long, tcx.long, tcx.long),
            ("unsigned long / unsigned long", tcx.ulong, tcx.ulong, tcx.ulong),
            ("long long / long long", tcx.long_long, tcx.long_long, tcx.long_long),
            (
                "unsigned long long / unsigned long long",
                tcx.ulong_long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4b: same signedness, different rank. ----
            ("int / long -> long (both signed)", tcx.int, tcx.long, tcx.long),
            ("int / long long -> long long (both signed)", tcx.int, tcx.long_long, tcx.long_long),
            ("long / long long -> long long (both signed)", tcx.long, tcx.long_long, tcx.long_long),
            ("unsigned int / unsigned long -> unsigned long", tcx.uint, tcx.ulong, tcx.ulong),
            (
                "unsigned long / unsigned long long -> unsigned long long",
                tcx.ulong,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            (
                "unsigned int / unsigned long long -> unsigned long long",
                tcx.uint,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.i: equal rank, mixed signedness → unsigned wins. ----
            ("int / unsigned int -> unsigned int", tcx.int, tcx.uint, tcx.uint),
            ("long / unsigned long -> unsigned long", tcx.long, tcx.ulong, tcx.ulong),
            (
                "long long / unsigned long long -> unsigned long long",
                tcx.long_long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.i: unsigned rank > signed rank → unsigned wins. ----
            ("int / unsigned long -> unsigned long", tcx.int, tcx.ulong, tcx.ulong),
            (
                "int / unsigned long long -> unsigned long long",
                tcx.int,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            (
                "long / unsigned long long -> unsigned long long",
                tcx.long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.ii: signed rank > unsigned rank, signed type can
            //                  represent every value of the unsigned type
            //                  (LP64: long has 64 bits, unsigned int has 32).
            ("long / unsigned int -> long (LP64)", tcx.long, tcx.uint, tcx.long),
            (
                "long long / unsigned int -> long long (LP64)",
                tcx.long_long,
                tcx.uint,
                tcx.long_long,
            ),
            // After integer promotion, `unsigned short` becomes `int` (every
            // value of unsigned short fits in int on a 32-bit-int target),
            // so pairing it with `long` falls through to step 4b after
            // promotion, not 4c. Same for char/_Bool.
            ("long / unsigned short -> long", tcx.long, tcx.ushort, tcx.long),
            ("long long / unsigned short -> long long", tcx.long_long, tcx.ushort, tcx.long_long),
            ("long / char -> long", tcx.long, tcx.char_, tcx.long),
            ("long / _Bool -> long", tcx.long, tcx.bool_, tcx.long),
            // ---- Sub-int signed/unsigned mixes promote to int/unsigned int
            //      via §6.3.1.1, then re-enter §6.3.1.8 step 4. ----
            ("char / unsigned int -> unsigned int", tcx.char_, tcx.uint, tcx.uint),
            ("short / unsigned int -> unsigned int", tcx.short, tcx.uint, tcx.uint),
            ("unsigned short / int -> int (promotes to int)", tcx.ushort, tcx.int, tcx.int),
            ("unsigned char / int -> int", tcx.uchar, tcx.int, tcx.int),
            ("_Bool / int -> int", tcx.bool_, tcx.int, tcx.int),
            ("_Bool / unsigned int -> unsigned int", tcx.bool_, tcx.uint, tcx.uint),
        ];

        for (desc, a, b, expected) in cases {
            let got_ab = usual_arithmetic(&tcx, *a, *b);
            assert_eq!(got_ab, *expected, "(a,b): {desc}");
            let got_ba = usual_arithmetic(&tcx, *b, *a);
            assert_eq!(got_ba, *expected, "(b,a): {desc} (symmetry)");
        }
    }

    /// Direct white-box test for step 4c.iii: when the signed type cannot
    /// represent every value of the unsigned type, both convert to the
    /// unsigned counterpart of the signed type. This branch is unreachable
    /// on LP64 with the current scalar set (every signed type whose rank
    /// strictly exceeds an unsigned operand's rank also has at least 2
    /// extra bits over it). We exercise it indirectly via the helper.
    #[test]
    fn usual_arithmetic_step_4c_iii_helpers() {
        let tcx = TyCtxt::new();
        assert_eq!(unsigned_counterpart(&tcx, IntRank::Int), tcx.uint);
        assert_eq!(unsigned_counterpart(&tcx, IntRank::Long), tcx.ulong);
        assert_eq!(unsigned_counterpart(&tcx, IntRank::LongLong), tcx.ulong_long);
        assert_eq!(int_rank_bits(IntRank::Int), 32);
        assert_eq!(int_rank_bits(IntRank::Long), 64);
        assert_eq!(int_rank_bits(IntRank::LongLong), 64);
        assert_eq!(int_rank_bits(IntRank::Short), 16);
        assert_eq!(int_rank_bits(IntRank::Char), 8);
        assert_eq!(int_rank_bits(IntRank::Bool), 1);
    }

    // ------------------------------------------------------------------
    // Array/function decay (C99 §6.3.2.1p3-4) — decay_if_needed.
    // ------------------------------------------------------------------
    //
    // These tests exercise the helper directly against a hand-built `Body`
    // rather than driving lowering end-to-end; the helper's contract is
    // purely "given an expr id whose type is array/function, return the
    // decayed wrapper unless the context forbids it". End-to-end coverage
    // arrives in task 07-07 once `check()` actually runs.

    use rcc_span::DUMMY_SP;

    /// Build a minimal `IntConst`-shaped leaf expression of type `ty` and
    /// category `cat` and return its id. The constant payload is a stand-in
    /// — the decay helper inspects `ty`/`value_cat` only, never the kind.
    fn push_leaf_expr(body: &mut Body, ty: TyId, cat: ValueCat) -> HirExprId {
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

    fn intern_int_array(tcx: &mut TyCtxt, len: u64) -> TyId {
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(len), is_vla: false })
    }

    fn intern_int_func_no_args(tcx: &mut TyCtxt) -> TyId {
        let int = tcx.int;
        tcx.intern(Ty::Func { ret: int, params: Vec::new(), variadic: false, proto: true })
    }

    /// Acceptance: `int arr[10]; int *p = arr;` inserts ArrayToPtr around `arr`.
    /// We model this as `decay_if_needed(arr, Normal)` and check the wrapper.
    #[test]
    fn decay_array_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::Normal);

        // A new wrapper expression must have been pushed.
        assert_ne!(decayed, arr_id, "decay must allocate a fresh expr id");
        let wrapper = &body.exprs[decayed];

        // Wrapper kind: Convert { operand: arr_id, kind: ArrayToPtr }.
        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, arr_id);
                assert_eq!(kind, ConvertKind::ArrayToPtr);
            }
            ref other => panic!("expected Convert wrapper, got {other:?}"),
        }

        // Wrapper type: `int *` (Ptr to plain int).
        match tcx.get(wrapper.ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, tcx.int),
            other => panic!("expected Ptr(int), got {other:?}"),
        }

        // Decayed expression is an rvalue (C99 §6.3.2.1p3).
        assert_eq!(wrapper.value_cat, ValueCat::RValue);
    }

    /// Function designator → pointer-to-function (C99 §6.3.2.1p4).
    #[test]
    fn decay_function_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::Normal);

        assert_ne!(decayed, fn_id);
        let wrapper = &body.exprs[decayed];

        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, fn_id);
                assert_eq!(kind, ConvertKind::FuncToPtr);
            }
            ref other => panic!("expected Convert wrapper, got {other:?}"),
        }

        // Wrapper type: pointer to the original function type.
        match tcx.get(wrapper.ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, fn_ty),
            other => panic!("expected Ptr(func_ty), got {other:?}"),
        }

        assert_eq!(wrapper.value_cat, ValueCat::RValue);
    }

    /// Acceptance: `int arr[10]; sizeof arr;` does NOT decay — sizeof returns
    /// 40. We assert the array type is preserved (size is a codegen concern).
    #[test]
    fn decay_array_skipped_inside_sizeof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::SizeofOperand);

        // Same id, same type — no Convert wrapper inserted.
        assert_eq!(result, arr_id, "sizeof operand must not decay");
        assert_eq!(body.exprs[result].ty, arr_ty);
        match tcx.get(body.exprs[result].ty) {
            Ty::Array { len, .. } => assert_eq!(*len, Some(10)),
            other => panic!("expected Array preserved, got {other:?}"),
        }
    }

    /// `&arr` — the operand of unary `&` does not decay (C99 §6.3.2.1p3).
    #[test]
    fn decay_array_skipped_inside_addrof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::AddrOfOperand);

        assert_eq!(result, arr_id);
        assert_eq!(body.exprs[result].ty, arr_ty);
    }

    /// `char a[] = "abc";` — the string literal initialiser keeps array type.
    #[test]
    fn decay_skipped_inside_char_array_initializer() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let lit_id = push_leaf_expr(&mut body, char_arr_ty, ValueCat::LValue);

        let result =
            decay_if_needed(&mut tcx, &mut body, lit_id, DecayContext::CharArrayInitializer);

        assert_eq!(result, lit_id);
        assert_eq!(body.exprs[result].ty, char_arr_ty);
    }

    /// Function designator under `sizeof` — sizeof of a function is a
    /// constraint violation in C99, but the helper still declines to decay
    /// so the diagnostic pass can spot the function type. (No diagnostic is
    /// emitted by decay_if_needed itself.)
    #[test]
    fn decay_function_skipped_inside_sizeof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::SizeofOperand);

        assert_eq!(result, fn_id);
        assert_eq!(body.exprs[result].ty, fn_ty);
    }

    /// Function designator under `&` — `&f` and `f` (decayed) are
    /// interchangeable per §6.3.2.1p4, so we leave the operand alone and let
    /// the AddressOf node carry the same pointer-to-function type itself.
    #[test]
    fn decay_function_skipped_inside_addrof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::AddrOfOperand);

        assert_eq!(result, fn_id);
        assert_eq!(body.exprs[result].ty, fn_ty);
    }

    /// Non-array, non-function operands pass through untouched in every
    /// context. Run the rule across the four context variants.
    #[test]
    fn decay_passthrough_for_non_decaying_types() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        for ctx in [
            DecayContext::Normal,
            DecayContext::SizeofOperand,
            DecayContext::AddrOfOperand,
            DecayContext::CharArrayInitializer,
        ] {
            let mut body = Body::default();
            let id = push_leaf_expr(&mut body, int_ty, ValueCat::RValue);
            let result = decay_if_needed(&mut tcx, &mut body, id, ctx);
            assert_eq!(result, id, "non-array/func passthrough in {ctx:?}");
            assert_eq!(body.exprs[result].ty, int_ty);
        }
    }

    /// Pointer-typed operands are not "arrays" — they must pass through
    /// even in `Normal` context (no double-decay).
    #[test]
    fn decay_pointer_does_not_decay() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let id = push_leaf_expr(&mut body, ptr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        assert_eq!(result, id);
        assert_eq!(body.exprs[result].ty, ptr_ty);
    }

    /// VLAs (`int v[n]`) decay too — `len: None, is_vla: true` is still an
    /// `Array` and its element type is well-defined.
    #[test]
    fn decay_vla_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let vla_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: true });
        let id = push_leaf_expr(&mut body, vla_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        assert_ne!(decayed, id);
        match body.exprs[decayed].kind {
            HirExprKind::Convert { kind, .. } => assert_eq!(kind, ConvertKind::ArrayToPtr),
            ref other => panic!("expected Convert/ArrayToPtr, got {other:?}"),
        }
        match tcx.get(body.exprs[decayed].ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, tcx.int),
            other => panic!("expected Ptr(int), got {other:?}"),
        }
    }

    /// Qualified element type (e.g. `const int arr[3]`) decays to a pointer
    /// whose pointee carries the same qualifiers (C99 §6.3.2.1p3 + §6.7.3).
    #[test]
    fn decay_preserves_element_qualifiers() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let elem = Qual { ty: tcx.int, is_const: true, is_volatile: false, is_restrict: false };
        let arr_ty = tcx.intern(Ty::Array { elem, len: Some(3), is_vla: false });
        let id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        match tcx.get(body.exprs[decayed].ty) {
            Ty::Ptr(q) => {
                assert_eq!(q.ty, tcx.int);
                assert!(q.is_const, "const-ness of element type must survive decay");
                assert!(!q.is_volatile);
            }
            other => panic!("expected Ptr(const int), got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // value_category — every HirExprKind arm.
    // ------------------------------------------------------------------

    use rcc_hir::{rcc_hir_binop::BinOp, rcc_hir_binop::UnOp, DefId, Local};

    /// Push a fully-typed `HirExpr` with the given `kind` and return its id.
    /// `value_cat` here is the *lowering-time guess* that lib.rs writes; the
    /// type-checker is supposed to override it via `value_category`. We
    /// deliberately set it to the WRONG category in some of these tests so
    /// that any accidental "read it back from value_cat" implementation gets
    /// caught.
    fn push_kind(body: &mut Body, ty: TyId, kind: HirExprKind) -> HirExprId {
        let id = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty,
            // Sentinel: the unit under test must derive the answer from
            // `kind`, not echo this back.
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind,
        });
        body.exprs[id].id = id;
        id
    }

    /// Acceptance row: literals are rvalues.
    #[test]
    fn value_category_int_const_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    #[test]
    fn value_category_float_const_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(0.0));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// String literal is an array-typed lvalue (C99 §6.4.5p6).
    #[test]
    fn value_category_string_ref_is_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let id = push_kind(&mut body, arr, HirExprKind::StringRef(DefId(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Identifier resolving to a local object → lvalue (C99 §6.5.1p2).
    #[test]
    fn value_category_local_ref_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Identifier resolving to a top-level def (global / function) → lvalue.
    #[test]
    fn value_category_def_ref_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::DefRef(DefId(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Binary op result is always an rvalue.
    #[test]
    fn value_category_binary_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// Unary op (including pre/post inc/dec) is rvalue per §6.5.3.1p2.
    #[test]
    fn value_category_unary_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let operand = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::Neg, operand });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// Function call result is rvalue (C99 §6.5.2.2p10 — the value of a
    /// function call is not an lvalue).
    #[test]
    fn value_category_call_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let callee = push_kind(&mut body, tcx.int, HirExprKind::DefRef(DefId(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Call { callee, args: Vec::new() });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `s.f` follows the base. Lvalue base → lvalue field.
    #[test]
    fn value_category_field_inherits_lvalue_from_base() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Field { base, field_index: 0 });
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// `(a + b).f` (rvalue base) → rvalue field. Synthetic but covers the
    /// inheritance rule when the base is not itself an lvalue.
    #[test]
    fn value_category_field_inherits_rvalue_from_base() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let base =
            push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });
        let id = push_kind(&mut body, tcx.int, HirExprKind::Field { base, field_index: 0 });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `a[i]` → lvalue (lowered to `*(a + i)` semantically).
    #[test]
    fn value_category_index_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let index = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Index { base, index });
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Convert wrappers always produce rvalues — the whole point of an
    /// LvalueToRvalue / ArrayToPtr / FuncToPtr / Pointer / IntegerPromotion
    /// / UsualArithmetic conversion is to *yield a value*.
    #[test]
    fn value_category_convert_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        for kind in [
            ConvertKind::IntegerPromotion,
            ConvertKind::UsualArithmetic,
            ConvertKind::ArrayToPtr,
            ConvertKind::FuncToPtr,
            ConvertKind::LvalueToRvalue,
            ConvertKind::Pointer,
        ] {
            let id = push_kind(&mut body, tcx.int, HirExprKind::Convert { operand: inner, kind });
            assert_eq!(value_category(&body, id), ValueCat::RValue, "Convert {kind:?}");
        }
    }

    /// Cast expression is an rvalue per §6.5.4p4.
    #[test]
    fn value_category_cast_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let operand = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand, to: tcx.int });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `&x` produces a pointer rvalue.
    #[test]
    fn value_category_address_of_is_rvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let id = push_kind(&mut body, ptr_ty, HirExprKind::AddressOf(inner));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `*p` is an lvalue (C99 §6.5.3.2p4).
    #[test]
    fn value_category_deref_is_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let inner = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Deref(inner));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Conditional `a ? b : c` is an rvalue (§6.5.15p4).
    #[test]
    fn value_category_cond_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let then_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let else_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(3));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Cond { cond, then_expr, else_expr });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `,` is an rvalue.
    #[test]
    fn value_category_comma_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Comma { lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `a = b` is an rvalue (§6.5.16p3 — "An assignment expression has the
    /// value of the left operand after the assignment, but is not an
    /// lvalue").
    #[test]
    fn value_category_assign_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Assign { lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    // ------------------------------------------------------------------
    // lvalue_to_rvalue_if_needed
    // ------------------------------------------------------------------

    /// LValue scalar → wrapped in `Convert { kind: LvalueToRvalue }`.
    #[test]
    fn l_to_r_wraps_scalar_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, inner);
        assert_ne!(after, inner, "scalar lvalue must allocate a Convert wrapper");

        let wrapper = &body.exprs[after];
        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, inner);
                assert_eq!(kind, ConvertKind::LvalueToRvalue);
            }
            ref other => panic!("expected Convert/LvalueToRvalue, got {other:?}"),
        }
        assert_eq!(wrapper.value_cat, ValueCat::RValue);
        assert_eq!(wrapper.ty, tcx.int);
    }

    /// Already-rvalue → no wrapper, returns same id.
    #[test]
    fn l_to_r_passthrough_rvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id);
    }

    /// Array-typed lvalue → no wrapper (decay is a separate conversion).
    #[test]
    fn l_to_r_passthrough_array_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 3);
        let id = push_kind(&mut body, arr_ty, HirExprKind::LocalRef(Local(0)));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id, "array lvalue must not get LvalueToRvalue wrapper");
    }

    /// Function-designator lvalue → no wrapper.
    #[test]
    fn l_to_r_passthrough_function_designator() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let id = push_kind(&mut body, fn_ty, HirExprKind::DefRef(DefId(0)));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id, "function designator must not get LvalueToRvalue wrapper");
    }

    /// Idempotent: applying the helper twice does not stack wrappers.
    #[test]
    fn l_to_r_is_idempotent() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let once = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, inner);
        let twice = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, once);
        assert_eq!(once, twice, "second application must be a no-op");
    }

    // ------------------------------------------------------------------
    // check_assignment_lhs (E0080).
    // ------------------------------------------------------------------

    /// Acceptance: `x = 1;` — `x` is an lvalue, no diagnostic.
    #[test]
    fn assignment_lhs_lvalue_local_accepted() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok, "LocalRef LHS must be accepted as lvalue");
        assert!(cap.diagnostics().is_empty(), "no E0080 expected");
    }

    /// Acceptance: `(int)x = 1;` — cast result is an rvalue → E0080.
    #[test]
    fn assignment_lhs_cast_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand: inner, to: tcx.int });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok, "cast LHS must be rejected as rvalue");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0080));
    }

    /// `1 = x;` — int literal LHS is an rvalue → E0080.
    #[test]
    fn assignment_lhs_int_const_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0080));
    }

    /// `(a + b) = 1;` — binary-op result LHS rejected.
    #[test]
    fn assignment_lhs_binary_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let lhs =
            push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok);
        assert_eq!(cap.diagnostics().len(), 1);
    }

    /// `*p = 1;` — deref LHS is an lvalue, accepted.
    #[test]
    fn assignment_lhs_deref_accepted() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let p = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(Local(0)));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Deref(p));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }

    /// `a[i] = 1;` — subscript LHS is an lvalue, accepted.
    #[test]
    fn assignment_lhs_index_accepted() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let idx = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Index { base, index: idx });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }

    // ------------------------------------------------------------------
    // Assignment compatibility (C99 §6.5.16.1) — is_assignable.
    // ------------------------------------------------------------------

    use rcc_hir::DefId as RecDefId;

    /// Bullet 1: arithmetic ↔ arithmetic. Same type → Ok, no narrowing.
    #[test]
    fn assignable_arith_same_type_ok() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let src = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.int, src), Ok(()));
    }

    /// Bullet 1: widening (`char → long`, `float → double`) accepted with no warning.
    #[test]
    fn assignable_arith_widening_ok() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let src = push_kind(&mut body, tcx.char_, HirExprKind::IntConst(1));
        assert_eq!(is_assignable(&tcx, &body, tcx.long, tcx.char_, src), Ok(()));
        assert_eq!(is_assignable(&tcx, &body, tcx.double, tcx.float, src), Ok(()));
        // unsigned-narrower → signed-wider holds the value range:
        assert_eq!(is_assignable(&tcx, &body, tcx.long, tcx.uint, src), Ok(()));
    }

    /// Acceptance: `int x = 1.5;` is accepted but flags Narrowing → caller emits W0008.
    #[test]
    fn assignable_double_to_int_is_narrowing() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let src = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(1.5));
        assert_eq!(
            is_assignable(&tcx, &body, tcx.int, tcx.double, src),
            Err(AssignError::Narrowing),
        );
    }

    /// Bullet 1: signed → unsigned of same width is narrowing (negatives lost).
    #[test]
    fn assignable_signed_to_unsigned_is_narrowing() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let src = push_kind(&mut body, tcx.int, HirExprKind::IntConst(-1));
        assert_eq!(is_assignable(&tcx, &body, tcx.uint, tcx.int, src), Err(AssignError::Narrowing),);
    }

    /// Bullet 1: `long → int` (truncation) is narrowing.
    #[test]
    fn assignable_long_to_int_is_narrowing() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let src = push_kind(&mut body, tcx.long, HirExprKind::IntConst(0));
        assert_eq!(is_assignable(&tcx, &body, tcx.int, tcx.long, src), Err(AssignError::Narrowing),);
    }

    /// Acceptance: `int *p = 0;` accepted (null pointer constant).
    #[test]
    fn assignable_null_pointer_constant_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        // Source: literal `0` of type int.
        let src = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        assert_eq!(is_assignable(&tcx, &body, int_ptr, tcx.int, src), Ok(()));
    }

    /// Null pointer constant survives Cast / Convert wrappers (`(void*)0`).
    #[test]
    fn assignable_null_pointer_through_cast() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let cast =
            push_kind(&mut body, void_ptr, HirExprKind::Cast { operand: zero, to: void_ptr });
        assert_eq!(is_assignable(&tcx, &body, int_ptr, void_ptr, cast), Ok(()));
    }

    /// Non-zero integer to pointer is a constraint violation
    /// (C99 §6.5.16.1p1 — only the *integer constant 0* is a null pointer
    /// constant).
    #[test]
    fn assignable_nonzero_int_to_pointer_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let one = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        assert_eq!(
            is_assignable(&tcx, &body, int_ptr, tcx.int, one),
            Err(AssignError::Incompatible),
        );
    }

    /// Bullet 4: `void *p = &x;` — object pointer → void* (and reverse) accepted.
    #[test]
    fn assignable_void_ptr_from_object_ptr_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, void_ptr, int_ptr, src), Ok(()));
        assert_eq!(is_assignable(&tcx, &body, int_ptr, void_ptr, src), Ok(()));
    }

    /// Bullet 4: function pointer is *not* an object pointer, so
    /// `void* = &func` is a constraint violation per §6.3.2.3p8.
    #[test]
    fn assignable_void_ptr_from_function_ptr_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let func_ty =
            tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
        let func_ptr = tcx.intern(Ty::Ptr(Qual::plain(func_ty)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let src = push_kind(&mut body, func_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(
            is_assignable(&tcx, &body, void_ptr, func_ptr, src),
            Err(AssignError::Incompatible),
        );
    }

    /// Bullet 3: `const int *p = &c_i;` accepted — both pointee types are
    /// `int`, dst pointee is `const`, src pointee is unqualified, dst's
    /// qualifier set is a superset.
    #[test]
    fn assignable_qualifier_widen_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let const_int_ptr = tcx.intern(Ty::Ptr(Qual {
            ty: tcx.int,
            is_const: true,
            is_volatile: false,
            is_restrict: false,
        }));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, const_int_ptr, int_ptr, src), Ok(()));
    }

    /// Bullet 3: `int *p = &c_i;` (dropping `const`) is a qualifier-loss
    /// constraint violation.
    #[test]
    fn assignable_qualifier_drop_loss() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let const_int_ptr = tcx.intern(Ty::Ptr(Qual {
            ty: tcx.int,
            is_const: true,
            is_volatile: false,
            is_restrict: false,
        }));
        let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(
            is_assignable(&tcx, &body, int_ptr, const_int_ptr, src),
            Err(AssignError::QualifierLoss),
        );
    }

    /// Acceptance: `struct A; struct B; struct A a; struct B *p = &a;` → E0081.
    /// Different record `DefId`s → not compatible.
    #[test]
    fn assignable_different_records_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let rec_a = tcx.intern(Ty::Record(RecDefId(0)));
        let rec_b = tcx.intern(Ty::Record(RecDefId(1)));
        let ptr_a = tcx.intern(Ty::Ptr(Qual::plain(rec_a)));
        let ptr_b = tcx.intern(Ty::Ptr(Qual::plain(rec_b)));
        let src = push_kind(&mut body, ptr_a, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, ptr_b, ptr_a, src), Err(AssignError::Incompatible),);
    }

    /// Bullet 2: same-DefId record ↔ record assignment accepted.
    #[test]
    fn assignable_same_record_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let rec = tcx.intern(Ty::Record(RecDefId(0)));
        let src = push_kind(&mut body, rec, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, rec, rec, src), Ok(()));
    }

    /// Bullet 6: `_Bool b = p;` for any pointer `p` is well-formed
    /// (C99 §6.3.1.2 — pointer-to-bool is the standard "is non-null?" idiom).
    #[test]
    fn assignable_bool_from_pointer_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, tcx.bool_, int_ptr, src), Ok(()));
    }

    /// Mismatched non-void pointers (e.g. `int* = float*`) reject as Incompatible.
    #[test]
    fn assignable_unrelated_pointers_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let float_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.float)));
        let src = push_kind(&mut body, float_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(
            is_assignable(&tcx, &body, int_ptr, float_ptr, src),
            Err(AssignError::Incompatible),
        );
    }

    /// Pointer LHS, struct RHS — incompatible (no §6.5.16.1p1 bullet matches).
    #[test]
    fn assignable_pointer_from_record_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let rec = tcx.intern(Ty::Record(RecDefId(0)));
        let src = push_kind(&mut body, rec, HirExprKind::LocalRef(Local(0)));
        assert_eq!(is_assignable(&tcx, &body, int_ptr, rec, src), Err(AssignError::Incompatible),);
    }

    /// Arithmetic LHS, pointer RHS (other than `_Bool`) — incompatible.
    #[test]
    fn assignable_int_from_pointer_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(
            is_assignable(&tcx, &body, tcx.int, int_ptr, src),
            Err(AssignError::Incompatible),
        );
    }

    /// `is_null_pointer_constant` — IntConst(0) is the canonical match.
    #[test]
    fn npc_int_const_zero() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        assert!(is_null_pointer_constant(&body, id));
    }

    /// `is_null_pointer_constant` — IntConst(7) is not a null pointer constant.
    #[test]
    fn npc_int_const_nonzero_rejected() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(7));
        assert!(!is_null_pointer_constant(&body, id));
    }

    /// `is_null_pointer_constant` recurses through Cast and Convert wrappers.
    #[test]
    fn npc_through_cast_and_convert() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let cast =
            push_kind(&mut body, tcx.long, HirExprKind::Cast { operand: zero, to: tcx.long });
        assert!(is_null_pointer_constant(&body, cast));

        let convert = push_kind(
            &mut body,
            tcx.long,
            HirExprKind::Convert { operand: zero, kind: ConvertKind::IntegerPromotion },
        );
        assert!(is_null_pointer_constant(&body, convert));

        // Nested wrappers still bottom out in IntConst(0).
        let nested = push_kind(
            &mut body,
            tcx.long,
            HirExprKind::Convert { operand: cast, kind: ConvertKind::IntegerPromotion },
        );
        assert!(is_null_pointer_constant(&body, nested));
    }

    /// Float literal is not a null pointer constant — only *integer*
    /// constant expressions with value 0 qualify (§6.3.2.3p3).
    #[test]
    fn npc_float_const_rejected() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(0.0));
        assert!(!is_null_pointer_constant(&body, id));
    }

    /// `is_compatible_type` — interned `TyId` equality covers the standard
    /// in-translation-unit cases.
    #[test]
    fn compatible_type_basic() {
        let mut tcx = TyCtxt::new();
        let p1 = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let p2 = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        assert!(is_compatible_type(&tcx, p1, p2));
        assert!(is_compatible_type(&tcx, tcx.int, tcx.int));
        assert!(!is_compatible_type(&tcx, tcx.int, tcx.long));
    }

    // ------------------------------------------------------------------
    // Pointer conversions (C99 §6.3.2.3) — pointer_convert.
    // ------------------------------------------------------------------

    /// Helper: build a `const`-qualified `Qual` over `ty`.
    fn const_qual(ty: TyId) -> Qual {
        Qual { ty, is_const: true, is_volatile: false, is_restrict: false }
    }

    /// Assert the most-recently pushed expression in `body` is a
    /// `Convert { kind: Pointer }` wrapper around `expected_operand`
    /// with type `expected_ty`. Returns the wrapper id for callers
    /// that want to chain checks.
    fn assert_pointer_wrapper(
        body: &Body,
        wrapper: HirExprId,
        expected_operand: HirExprId,
        expected_ty: TyId,
    ) {
        let expr = &body.exprs[wrapper];
        assert_eq!(expr.ty, expected_ty, "wrapper type");
        assert_eq!(expr.value_cat, ValueCat::RValue, "wrapper value cat");
        match expr.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, expected_operand, "wrapped operand");
                assert_eq!(kind, ConvertKind::Pointer, "convert kind");
            }
            ref other => panic!("expected Convert::Pointer wrapper, got {other:?}"),
        }
    }

    /// Acceptance: `void *p = &x;` — `int *` source, `void *` dest is
    /// accepted and a `ConvertKind::Pointer` wrapper is inserted.
    #[test]
    fn pointer_convert_object_ptr_to_void_ptr_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));

        let result = pointer_convert(&mut tcx, &mut body, src, void_ptr).expect("must succeed");
        assert_ne!(result, src, "must allocate a wrapper");
        assert_pointer_wrapper(&body, result, src, void_ptr);
    }

    /// `void *` → `int *` accepted (the symmetric case of the void*
    /// rule, exercised by e.g. `int *p = malloc(n);`).
    #[test]
    fn pointer_convert_void_ptr_to_object_ptr_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let src = push_kind(&mut body, void_ptr, HirExprKind::LocalRef(Local(0)));

        let result = pointer_convert(&mut tcx, &mut body, src, int_ptr).expect("must succeed");
        assert_pointer_wrapper(&body, result, src, int_ptr);
    }

    /// Acceptance: `int *p = &x; char *q = p;` is rejected — `int *`
    /// and `char *` have unrelated pointee types.
    #[test]
    fn pointer_convert_unrelated_object_ptrs_incompatible() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let char_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, char_ptr),
            Err(ConvertError::Incompatible),
        );
    }

    /// Identical pointer types (`int *` ↔ `int *`) need no wrapper —
    /// the helper returns the source id unchanged.
    #[test]
    fn pointer_convert_identical_ptr_no_wrapper() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));
        let len_before = body.exprs.len();

        let result = pointer_convert(&mut tcx, &mut body, src, int_ptr).expect("trivial ok");
        assert_eq!(result, src, "no wrapper needed");
        assert_eq!(body.exprs.len(), len_before, "no allocation");
    }

    /// Bullet 1: literal `0` (a null pointer constant) converts to any
    /// pointer type. Source type happens to be `int`, but the
    /// integer-to-pointer rejection path must not fire because the
    /// expression is a null pointer constant.
    #[test]
    fn pointer_convert_null_pointer_constant_to_int_ptr() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));

        let result = pointer_convert(&mut tcx, &mut body, zero, int_ptr).expect("npc ok");
        assert_pointer_wrapper(&body, result, zero, int_ptr);
    }

    /// `(void *)0` is also a null pointer constant — it survives the
    /// `Cast` wrapper inside `is_null_pointer_constant`.
    #[test]
    fn pointer_convert_null_pointer_constant_via_cast() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let zero = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let void_zero =
            push_kind(&mut body, void_ptr, HirExprKind::Cast { operand: zero, to: void_ptr });

        let result = pointer_convert(&mut tcx, &mut body, void_zero, int_ptr).expect("npc ok");
        assert_pointer_wrapper(&body, result, void_zero, int_ptr);
    }

    /// Bullet 1 negative: `int x = 7; int *p = x;` — non-zero integer
    /// to pointer is *not* a null pointer constant, so it requires an
    /// explicit cast.
    #[test]
    fn pointer_convert_nonzero_int_to_ptr_requires_cast() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let seven = push_kind(&mut body, tcx.int, HirExprKind::IntConst(7));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, seven, int_ptr),
            Err(ConvertError::IntegerPointerMix),
        );
    }

    /// Bullet 1 negative: pointer-to-integer assignment requires an
    /// explicit cast (regardless of source pointer's value).
    #[test]
    fn pointer_convert_ptr_to_int_requires_cast() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let int_ty = tcx.int;
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, int_ty),
            Err(ConvertError::IntegerPointerMix),
        );
    }

    /// Bullet 2 / 3 qualifier rule: `const int *q = p;` with `int *p`
    /// adds `const` on the pointee — accepted.
    #[test]
    fn pointer_convert_qualifier_addition_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let const_int_ptr = tcx.intern(Ty::Ptr(const_qual(tcx.int)));
        let src = push_kind(&mut body, int_ptr, HirExprKind::LocalRef(Local(0)));

        let result = pointer_convert(&mut tcx, &mut body, src, const_int_ptr).expect("widen qual");
        assert_pointer_wrapper(&body, result, src, const_int_ptr);
    }

    /// Bullet 3 negative: `int *q = cp;` with `const int *cp` drops
    /// `const` — must be rejected as `QualifierLoss`.
    #[test]
    fn pointer_convert_qualifier_drop_loss() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let const_int_ptr = tcx.intern(Ty::Ptr(const_qual(tcx.int)));
        let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, int_ptr),
            Err(ConvertError::QualifierLoss),
        );
    }

    /// Bullet 2 with qualifiers: `void *p = cp;` where `cp` is
    /// `const int *` drops `const` — qualifier loss.
    #[test]
    fn pointer_convert_void_ptr_qualifier_drop_loss() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let const_int_ptr = tcx.intern(Ty::Ptr(const_qual(tcx.int)));
        let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, void_ptr),
            Err(ConvertError::QualifierLoss),
        );
    }

    /// Bullet 2 with qualifiers OK: `const void *p = cp;` carries the
    /// `const` through — accepted.
    #[test]
    fn pointer_convert_const_void_ptr_from_const_object_ok() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let const_void_ptr = tcx.intern(Ty::Ptr(const_qual(tcx.void)));
        let const_int_ptr = tcx.intern(Ty::Ptr(const_qual(tcx.int)));
        let src = push_kind(&mut body, const_int_ptr, HirExprKind::LocalRef(Local(0)));

        let result = pointer_convert(&mut tcx, &mut body, src, const_void_ptr).expect("qual ok");
        assert_pointer_wrapper(&body, result, src, const_void_ptr);
    }

    /// Bullet 4: function pointers with the *same* signature are
    /// interchangeable — `int (*)(int) = int (*)(int)`.
    #[test]
    fn pointer_convert_compatible_function_ptrs_ok() {
        // Two structurally-identical Func types intern to the same
        // TyId, so this path actually goes through the trivial
        // "src_ty == dst_ty" branch. Use intermediate shapes to make
        // sure we exercise the function-pointer branch when types
        // differ but pointees are interned-equal at point of call.
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_func = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let int_func_ptr = tcx.intern(Ty::Ptr(Qual::plain(int_func)));
        // Re-intern the same Func; we expect the same TyId because of
        // dedup. This means the helper takes the trivial-equal
        // shortcut, returning `src` unchanged.
        let int_func_ptr_dup = tcx.intern(Ty::Ptr(Qual::plain(int_func)));
        assert_eq!(int_func_ptr, int_func_ptr_dup);

        let src = push_kind(&mut body, int_func_ptr, HirExprKind::LocalRef(Local(0)));
        let result =
            pointer_convert(&mut tcx, &mut body, src, int_func_ptr_dup).expect("trivial ok");
        assert_eq!(result, src);
    }

    /// Bullet 4 negative: function pointers with different parameter
    /// lists are *not* compatible — `int (*)(int)` ↔ `int (*)(double)`
    /// must be rejected. This is the explicit acceptance scenario in
    /// the task spec.
    #[test]
    fn pointer_convert_incompatible_function_ptrs_e0082() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_double = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.double],
            variadic: false,
            proto: true,
        });
        let int_int = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let src_ptr = tcx.intern(Ty::Ptr(Qual::plain(int_double)));
        let dst_ptr = tcx.intern(Ty::Ptr(Qual::plain(int_int)));
        let src = push_kind(&mut body, src_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, dst_ptr),
            Err(ConvertError::Incompatible),
        );
    }

    /// Bullet 4 / 8: function pointers with *different return types*
    /// are also incompatible.
    #[test]
    fn pointer_convert_function_ptrs_different_return_e0082() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let int_func = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let void_func = tcx.intern(Ty::Func {
            ret: tcx.void,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let src_ptr = tcx.intern(Ty::Ptr(Qual::plain(int_func)));
        let dst_ptr = tcx.intern(Ty::Ptr(Qual::plain(void_func)));
        let src = push_kind(&mut body, src_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, dst_ptr),
            Err(ConvertError::Incompatible),
        );
    }

    /// §6.3.2.3p8: a function pointer is *not* an object pointer, so
    /// `void* = func_ptr` is rejected (no implicit conversion between
    /// function pointers and `void*`).
    #[test]
    fn pointer_convert_function_ptr_to_void_ptr_rejected() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let func_ty =
            tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
        let func_ptr = tcx.intern(Ty::Ptr(Qual::plain(func_ty)));
        let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        let src = push_kind(&mut body, func_ptr, HirExprKind::LocalRef(Local(0)));

        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src, void_ptr),
            Err(ConvertError::Incompatible),
        );
        // And the reverse direction.
        let src2 = push_kind(&mut body, void_ptr, HirExprKind::LocalRef(Local(0)));
        assert_eq!(
            pointer_convert(&mut tcx, &mut body, src2, func_ptr),
            Err(ConvertError::Incompatible),
        );
    }

    /// §6.3.2.3p4 records "qualifiers must be added, not removed" for
    /// the function-pointer case (functions cannot be qualified, so
    /// the only meaningful test is that compatibility is decided on
    /// the function type itself, not the surrounding qualifiers).
    /// Already covered by the compatible/incompatible tests above.
    /// Truth table: walk every §6.3.2.3 bullet at least once with the
    /// expected outcome.
    #[test]
    fn pointer_convert_truth_table() {
        // (description, src_ty_builder, dst_ty_builder, src_kind_builder,
        //  expected) — encoded as closures so each row gets a fresh tcx
        // / body and we don't cross-pollute interned ids.
        #[derive(Debug)]
        enum Outcome {
            Trivial,
            Wrap,
            Err(ConvertError),
        }

        // Helper: run one row.
        fn run(
            label: &str,
            build_src: impl FnOnce(&mut TyCtxt) -> TyId,
            build_dst: impl FnOnce(&mut TyCtxt) -> TyId,
            build_kind: impl FnOnce(&mut TyCtxt) -> HirExprKind,
            expected: Outcome,
        ) {
            let mut tcx = TyCtxt::new();
            let mut body = Body::default();
            let src_ty = build_src(&mut tcx);
            let dst_ty = build_dst(&mut tcx);
            let kind = build_kind(&mut tcx);
            let src = push_kind(&mut body, src_ty, kind);

            let len_before = body.exprs.len();
            let result = pointer_convert(&mut tcx, &mut body, src, dst_ty);
            match (result, expected) {
                (Ok(id), Outcome::Trivial) => {
                    assert_eq!(id, src, "{label}: trivial path must reuse src id");
                    assert_eq!(body.exprs.len(), len_before, "{label}: no allocation");
                }
                (Ok(id), Outcome::Wrap) => {
                    assert_ne!(id, src, "{label}: wrap path must allocate fresh id");
                    let expr = &body.exprs[id];
                    assert_eq!(expr.ty, dst_ty, "{label}: wrapper has dst type");
                    match expr.kind {
                        HirExprKind::Convert { operand, kind: ConvertKind::Pointer } => {
                            assert_eq!(operand, src, "{label}: wrapped operand")
                        }
                        ref other => panic!("{label}: expected Convert::Pointer, got {other:?}"),
                    }
                }
                (Err(e), Outcome::Err(want)) => {
                    assert_eq!(e, want, "{label}");
                }
                (got, want) => panic!("{label}: result={got:?}, want={want:?}"),
            }
        }

        // ---- §6.3.2.3p1: pointer to qualified ↔ unqualified; trivial. ----
        // `int *` → `int *` (same TyId after interning) → no wrapper.
        run(
            "int* -> int* (trivial)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Trivial,
        );

        // ---- §6.3.2.3p3: null pointer constant ↔ pointer. ----
        run(
            "0 -> int* (null pointer constant)",
            |t| t.int,
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::IntConst(0),
            Outcome::Wrap,
        );
        run(
            "0 -> char* (null pointer constant)",
            |t| t.int,
            |t| t.intern(Ty::Ptr(Qual::plain(t.char_))),
            |_| HirExprKind::IntConst(0),
            Outcome::Wrap,
        );
        run(
            "1 -> int* (non-zero int, requires cast)",
            |t| t.int,
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::IntConst(1),
            Outcome::Err(ConvertError::IntegerPointerMix),
        );

        // ---- §6.3.2.3p1 + p7: void* ↔ object pointer (both directions). ----
        run(
            "int* -> void*",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Wrap,
        );
        run(
            "void* -> int*",
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Wrap,
        );
        run(
            "char* -> void* (object pointer)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.char_))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Wrap,
        );

        // ---- §6.3.2.3p1 qualifier rule: addition OK, removal not. ----
        run(
            "int* -> const int* (add const)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.intern(Ty::Ptr(const_qual(t.int))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Wrap,
        );
        run(
            "const int* -> int* (drop const)",
            |t| t.intern(Ty::Ptr(const_qual(t.int))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::QualifierLoss),
        );
        run(
            "void* -> const void* (add const)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |t| t.intern(Ty::Ptr(const_qual(t.void))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Wrap,
        );
        run(
            "const void* -> void* (drop const)",
            |t| t.intern(Ty::Ptr(const_qual(t.void))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::QualifierLoss),
        );

        // ---- §6.3.2.3p1: unrelated object pointers reject. ----
        run(
            "int* -> char* (unrelated pointee)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.char_))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
        run(
            "int* -> float* (unrelated pointee)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.intern(Ty::Ptr(Qual::plain(t.float))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );

        // ---- §6.3.2.3p8: function-pointer compatibility. ----
        run(
            "int(*)(int) -> int(*)(int) (compatible)",
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Trivial,
        );
        run(
            "int(*)(int) -> int(*)(double) (incompatible)",
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.double],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
        run(
            "int(*)(int) -> void(*)(int) (different return)",
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.void,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
        run(
            "int(*)(int) -> int(*)(int, ...) (variadic mismatch)",
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: true,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );

        // ---- §6.3.2.3p8 again: function-pointer / object-pointer mix. ----
        run(
            "int(*)(int) -> void* (function pointer not object pointer)",
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
        run(
            "void* -> int(*)(int) (function pointer not object pointer)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.void))),
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
        run(
            "0 -> int(*)(int) (null pointer constant ok for function ptr)",
            |t| t.int,
            |t| {
                let f = t.intern(Ty::Func {
                    ret: t.int,
                    params: vec![t.int],
                    variadic: false,
                    proto: true,
                });
                t.intern(Ty::Ptr(Qual::plain(f)))
            },
            |_| HirExprKind::IntConst(0),
            Outcome::Wrap,
        );

        // ---- §6.3.2.3p5/p6: integer ↔ pointer requires explicit cast. ----
        run(
            "int* -> int (pointer to integer)",
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |t| t.int,
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::IntegerPointerMix),
        );
        run(
            "int -> int* (non-null integer to pointer)",
            |t| t.int,
            |t| t.intern(Ty::Ptr(Qual::plain(t.int))),
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::IntegerPointerMix),
        );

        // ---- Sanity: non-pointer ↔ non-pointer falls through to
        // Incompatible (caller must not invoke us on this shape, but
        // we keep the helper total). ----
        run(
            "int -> float (caller misuse)",
            |t| t.int,
            |t| t.float,
            |_| HirExprKind::LocalRef(Local(0)),
            Outcome::Err(ConvertError::Incompatible),
        );
    }

    // ------------------------------------------------------------------
    // check_body / visit_expr — implicit conversion insertion (07-07).
    // ------------------------------------------------------------------

    use rcc_hir::HirStmt;

    /// Wrap a single expression as the root statement of a fresh body.
    /// Returns the body and the expression id so the test can drive
    /// `check_body` and then inspect the typed result.
    fn body_with_root_expr(expr_kind: HirExprKind, ty: TyId) -> (Body, HirExprId, HirStmtId) {
        let mut body = Body::default();
        let expr_id = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: expr_kind,
        });
        body.exprs[expr_id].id = expr_id;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(expr_id),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);
        (body, expr_id, stmt_id)
    }

    /// Acceptance: `1 + 2.0` — IntConst is wrapped in `Convert(IntToFloat, f64)`
    /// before the FAdd. The HIR uses `ConvertKind::UsualArithmetic` to label
    /// the wrapper; `IntToFloat` is the target lowering category, not a HIR
    /// kind. We assert: the int side is wrapped in a Convert with destination
    /// type `double`, and the binary op result type is `double`.
    #[test]
    fn check_body_acceptance_int_plus_double() {
        let mut tcx = TyCtxt::new();
        // Build the operands first: IntConst(1) and FloatConst(2.0).
        let mut body = Body::default();
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(2.0),
        });
        body.exprs[rhs].id = rhs;
        let bin = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::Add, lhs, rhs },
        });
        body.exprs[bin].id = bin;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(bin),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        // Binary expression's result type is `double`.
        assert_eq!(body.exprs[bin].ty, tcx.double);

        // The lhs (originally IntConst(1)) must now be referenced via a
        // Convert wrapper whose destination type is `double`.
        let HirExprKind::Binary { lhs: new_lhs, rhs: new_rhs, .. } = body.exprs[bin].kind.clone()
        else {
            panic!("expected Binary kind");
        };
        match body.exprs[new_lhs].kind {
            HirExprKind::Convert { operand, kind: _ } => {
                assert_eq!(operand, lhs, "wrapper must wrap the original IntConst");
                assert_eq!(body.exprs[new_lhs].ty, tcx.double, "wrapper has type double");
            }
            ref other => panic!("expected Convert on lhs, got {other:?}"),
        }
        // The rhs is already double, so no wrapper expected — the id
        // stays the original.
        assert_eq!(new_rhs, rhs, "rhs already double, no wrapper needed");
        assert_eq!(body.exprs[rhs].ty, tcx.double);

        // No errors emitted.
        assert!(!session.handler.has_errors());
    }

    /// Plain `1 + 2` — both IntConst, both already typed `int` after
    /// the leaf typer; no Convert wrappers expected on the operands.
    #[test]
    fn check_body_int_plus_int_no_wrapper() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(2),
        });
        body.exprs[rhs].id = rhs;
        let bin = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::Add, lhs, rhs },
        });
        body.exprs[bin].id = bin;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(bin),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[bin].ty, tcx.int);
        let HirExprKind::Binary { lhs: nl, rhs: nr, .. } = body.exprs[bin].kind.clone() else {
            panic!()
        };
        assert_eq!(nl, lhs);
        assert_eq!(nr, rhs);
    }

    /// Comparison `1 < 2` returns `int` regardless of operand types.
    #[test]
    fn check_body_comparison_yields_int() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(2.0),
        });
        body.exprs[rhs].id = rhs;
        let bin = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::Lt, lhs, rhs },
        });
        body.exprs[bin].id = bin;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(bin),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[bin].ty, tcx.int, "comparison result is int");
    }

    /// Bitwise `&` on a float operand emits E0083.
    #[test]
    fn check_body_bitand_on_float_emits_e0083() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(2.0),
        });
        body.exprs[rhs].id = rhs;
        let bin = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::BitAnd, lhs, rhs },
        });
        body.exprs[bin].id = bin;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(bin),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0083));
    }

    /// Unary `-` on a `char` integer-promotes to `int`.
    #[test]
    fn check_body_unary_neg_promotes_char_to_int() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        // Add a local of type `char` so we have an lvalue to negate.
        let char_local = body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: tcx.char_,
            is_param: false,
            span: DUMMY_SP,
        });
        let operand = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::LocalRef(char_local),
        });
        body.exprs[operand].id = operand;
        let neg = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Unary { op: UnOp::Neg, operand },
        });
        body.exprs[neg].id = neg;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(neg),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[neg].ty, tcx.int, "char promoted to int by unary -");
    }

    /// Unary `!` on a scalar yields `int`.
    #[test]
    fn check_body_unary_lognot_yields_int() {
        let mut tcx = TyCtxt::new();
        let (mut body, _, _) = body_with_root_expr(HirExprKind::IntConst(1), tcx.error);
        let kid = HirExprId(0); // root expr
        let not = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Unary { op: UnOp::LogNot, operand: kid },
        });
        body.exprs[not].id = not;
        // Re-root so the walker visits the LogNot.
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(not),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[not].ty, tcx.int);
    }

    /// `*p` produces an lvalue of the pointee type.
    #[test]
    fn check_body_deref_typed_to_pointee() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let p_local = body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: ptr_ty,
            is_param: false,
            span: DUMMY_SP,
        });
        let p_ref = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::LocalRef(p_local),
        });
        body.exprs[p_ref].id = p_ref;
        let deref = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Deref(p_ref),
        });
        body.exprs[deref].id = deref;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(deref),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[deref].ty, tcx.int);
        assert_eq!(body.exprs[deref].value_cat, ValueCat::LValue);
    }

    /// `&x` for an `int x` produces a value of type `int *` rvalue.
    #[test]
    fn check_body_address_of_yields_pointer() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let x_local = body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: tcx.int,
            is_param: false,
            span: DUMMY_SP,
        });
        let x_ref = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::LocalRef(x_local),
        });
        body.exprs[x_ref].id = x_ref;
        let addr = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::AddressOf(x_ref),
        });
        body.exprs[addr].id = addr;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(addr),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        match *tcx.get(body.exprs[addr].ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, tcx.int),
            ref other => panic!("expected Ptr(int), got {other:?}"),
        }
        assert_eq!(body.exprs[addr].value_cat, ValueCat::RValue);
    }

    /// Assignment `x = 1.5` for an `int x`: RHS is a double, must be
    /// wrapped in a Convert to `int` before the Assign.
    #[test]
    fn check_body_assign_inserts_narrowing_convert() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let x_local = body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: tcx.int,
            is_param: false,
            span: DUMMY_SP,
        });
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::LocalRef(x_local),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(1.5),
        });
        body.exprs[rhs].id = rhs;
        let assign = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Assign { lhs, rhs },
        });
        body.exprs[assign].id = assign;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(assign),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        // The Assign's type is the LHS type (int).
        assert_eq!(body.exprs[assign].ty, tcx.int);
        // The RHS is now a Convert wrapper of type `int`.
        let HirExprKind::Assign { rhs: new_rhs, .. } = body.exprs[assign].kind.clone() else {
            panic!()
        };
        assert_eq!(body.exprs[new_rhs].ty, tcx.int);
        assert!(matches!(body.exprs[new_rhs].kind, HirExprKind::Convert { .. }));
    }

    /// Comma `a, b` has the type of its RHS, evaluating the LHS for side
    /// effects.
    #[test]
    fn check_body_comma_takes_rhs_type() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[lhs].id = lhs;
        let rhs = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(2.0),
        });
        body.exprs[rhs].id = rhs;
        let comma = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Comma { lhs, rhs },
        });
        body.exprs[comma].id = comma;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(comma),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[comma].ty, tcx.double);
    }

    /// Conditional `1 ? 2 : 3.0` — operands taken to common type `double`.
    #[test]
    fn check_body_conditional_unifies_types() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let cond = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[cond].id = cond;
        let t = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(2),
        });
        body.exprs[t].id = t;
        let e = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(3.0),
        });
        body.exprs[e].id = e;
        let qm = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Cond { cond, then_expr: t, else_expr: e },
        });
        body.exprs[qm].id = qm;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(qm),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[qm].ty, tcx.double);
    }

    /// `LocalRef` to an `int` local types as `int` lvalue.
    #[test]
    fn check_body_local_ref_typed_from_local_decl() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let l = body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: tcx.int,
            is_param: false,
            span: DUMMY_SP,
        });
        let r = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::LocalRef(l),
        });
        body.exprs[r].id = r;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(r),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        assert_eq!(body.exprs[r].ty, tcx.int);
        assert_eq!(body.exprs[r].value_cat, ValueCat::LValue);
    }

    /// Integer constants always type as `int`.
    #[test]
    fn check_body_int_const_typed_to_int() {
        let mut tcx = TyCtxt::new();
        let (mut body, eid, _) = body_with_root_expr(HirExprKind::IntConst(42), tcx.error);
        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);
        assert_eq!(body.exprs[eid].ty, tcx.int);
    }

    /// Float constants type as `double`.
    #[test]
    fn check_body_float_const_typed_to_double() {
        let mut tcx = TyCtxt::new();
        let (mut body, eid, _) = body_with_root_expr(HirExprKind::FloatConst(2.5), tcx.error);
        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);
        assert_eq!(body.exprs[eid].ty, tcx.double);
    }

    /// Acceptance: after a clean typeck pass, no `Ty::Error` surfaces in
    /// any expression of the body.
    #[test]
    fn check_body_no_error_type_after_clean_pass() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        // (1 + 2) * 3.5
        let e1 = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(1),
        });
        body.exprs[e1].id = e1;
        let e2 = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(2),
        });
        body.exprs[e2].id = e2;
        let add = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::Add, lhs: e1, rhs: e2 },
        });
        body.exprs[add].id = add;
        let e3 = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::FloatConst(3.5),
        });
        body.exprs[e3].id = e3;
        let mul = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty: tcx.error,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind: HirExprKind::Binary { op: BinOp::Mul, lhs: add, rhs: e3 },
        });
        body.exprs[mul].id = mul;
        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: DUMMY_SP,
            kind: HirStmtKind::Expr(mul),
        });
        body.stmts[stmt_id].id = stmt_id;
        body.root = Some(stmt_id);

        let (mut session, _cap) = Session::for_test();
        check_body(&mut body, &mut tcx, &mut session);

        // Walk every reachable expression and confirm no Ty::Error.
        for expr in body.exprs.iter() {
            assert_ne!(
                expr.ty, tcx.error,
                "expr {:?} of kind {:?} still has Ty::Error",
                expr.id, expr.kind
            );
        }
        // The outer multiply yields double (1 + 2 is int, 3.5 is double,
        // usual arithmetic raises both sides to double).
        assert_eq!(body.exprs[mul].ty, tcx.double);
    }
}
