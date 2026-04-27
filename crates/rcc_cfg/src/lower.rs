//! HIR expression -> CFG (`Operand` / `Place`) lowering.
//!
//! Two entry points:
//!
//! * [`lower_as_rvalue`] — value-position. Walks the HIR expression tree,
//!   emits intermediate `place := rvalue` assignments into the current
//!   block, and returns an [`Operand`] that names the final value.
//! * [`lower_as_place`] — lvalue-position. Returns a [`Place`] (base local
//!   plus projection chain). Calling this on a non-lvalue HIR node is a
//!   lowering bug and panics in debug builds.
//!
//! The acceptance tests in this file exercise the canonical shape:
//!
//! * `a + b * c` ⇒ a single intermediate temporary `_t<N>` for the
//!   inner multiply, fed into the outer add.
//! * `*p` ⇒ a [`Place`] with a single `Projection::Deref` step.
//!
//! Out of scope (deferred): short-circuit `&&` / `||`, the ternary
//! `a ? b : c`, calls (those terminate a block), `++` / `--`, compound
//! literals, `sizeof` over an expression. Each such arm panics with a
//! `todo!` carrying the task id that owns it.

use rcc_hir::{
    rcc_hir_binop::{BinOp as HirBinOp, UnOp as HirUnOp},
    Body as HirBody, ConvertKind, FloatKind, HirExprId, HirExprKind, IntRank, Local as HirLocal,
    Ty, TyCtxt, TyId,
};
use rcc_span::Span;

use crate::{
    BinOp, BodyBuilder, CastKind, Const, ConstKind, Local, Operand, Place, Projection, Rvalue,
    Statement, StatementKind, UnOp,
};

/// Translation table from HIR local ids to CFG local ids.
///
/// The CFG owns its own local index space (return slot at `Local(0)`,
/// parameters at `Local(1..)`, then user locals and temporaries). HIR
/// uses a parallel space with no implicit return slot. The body-builder
/// task (08-04 / future glue) populates this map when it allocates the
/// per-body CFG slots; expression lowering only needs the lookup.
#[derive(Debug, Clone, Default)]
pub struct LocalMap {
    /// `hir_to_cfg[hir_local.0 as usize]` = the CFG local for that HIR
    /// local. Holes are not expected during normal lowering; if a HIR
    /// local has no CFG counterpart `lookup` panics.
    hir_to_cfg: Vec<Option<Local>>,
}

impl LocalMap {
    /// Create an empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `hir -> cfg`. Grows the underlying vector with `None`
    /// holes as needed.
    pub fn insert(&mut self, hir: HirLocal, cfg: Local) {
        let idx = hir.0 as usize;
        if idx >= self.hir_to_cfg.len() {
            self.hir_to_cfg.resize(idx + 1, None);
        }
        self.hir_to_cfg[idx] = Some(cfg);
    }

    /// Look up a HIR local. Panics if no CFG local has been registered
    /// — every HIR local that is reachable in expression lowering must
    /// have been allocated by the body builder first.
    #[must_use]
    pub fn lookup(&self, hir: HirLocal) -> Local {
        let idx = hir.0 as usize;
        self.hir_to_cfg
            .get(idx)
            .copied()
            .flatten()
            .unwrap_or_else(|| panic!("LocalMap: no CFG local registered for HIR {hir:?}"))
    }
}

/// Read-only context bundle threaded through the lowering recursion.
///
/// Holds references the lowering routines need but never mutate: the
/// HIR body (statement / expression arenas), the type context (signed
/// vs unsigned vs float decisions), and the local-id translation
/// table.
pub struct LowerCx<'a> {
    /// HIR body being lowered.
    pub body: &'a HirBody,
    /// Type interner used to classify operand types (signed-int vs
    /// float vs pointer) when picking the typed CFG `BinOp` variant.
    pub tcx: &'a TyCtxt,
    /// HIR-local -> CFG-local translation.
    pub locals: &'a LocalMap,
}

impl<'a> LowerCx<'a> {
    /// Create a new context.
    #[must_use]
    pub fn new(body: &'a HirBody, tcx: &'a TyCtxt, locals: &'a LocalMap) -> Self {
        Self { body, tcx, locals }
    }
}

/// Lower a HIR expression in *value* position. Returns an [`Operand`]
/// that names the result; intermediate computations are emitted as
/// `Statement::Assign` against the current block of `builder`.
///
/// # Panics
/// Panics on HIR shapes that this task explicitly defers to a later
/// task (short-circuit, ternary, calls, etc.).
pub fn lower_as_rvalue(builder: &mut BodyBuilder, cx: &LowerCx<'_>, expr_id: HirExprId) -> Operand {
    let expr = &cx.body.exprs[expr_id];
    let span = expr.span;
    let ty = expr.ty;

    match &expr.kind {
        HirExprKind::IntConst(n) => Operand::Const(Const { kind: ConstKind::Int(*n), ty }),
        HirExprKind::FloatConst(f) => Operand::Const(Const { kind: ConstKind::Float(*f), ty }),
        HirExprKind::StringRef(def) | HirExprKind::DefRef(def) => {
            // Both refer to a global symbol. The CFG `Const::Global`
            // form already encodes "address of <DefId>"; type carries
            // the pointer-to-... type computed by typeck.
            Operand::Const(Const { kind: ConstKind::Global(*def), ty })
        }
        HirExprKind::LocalRef(hir_local) => {
            let local = cx.locals.lookup(*hir_local);
            Operand::Copy(Place { base: local, projection: Vec::new() })
        }
        HirExprKind::Binary { op, lhs, rhs } => {
            let lhs_op = lower_as_rvalue(builder, cx, *lhs);
            let rhs_op = lower_as_rvalue(builder, cx, *rhs);
            // Pick the typed CFG op. The typed kind is determined by
            // the *operand* type after usual arithmetic conversions
            // (typeck has already promoted both sides).
            let lhs_ty = cx.body.exprs[*lhs].ty;
            let rhs_ty = cx.body.exprs[*rhs].ty;
            let cfg_op = pick_binop(*op, lhs_ty, rhs_ty, cx.tcx);
            let temp = builder.alloc_temp(ty, span);
            push_assign(builder, span, temp, Rvalue::BinaryOp(cfg_op, lhs_op, rhs_op));
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::Unary { op, operand } => match op {
            HirUnOp::Plus => {
                // Unary `+` is a no-op after integer promotion (which
                // typeck already inserted as a Convert wrapper).
                lower_as_rvalue(builder, cx, *operand)
            }
            HirUnOp::Neg | HirUnOp::BitNot | HirUnOp::LogNot => {
                let inner = lower_as_rvalue(builder, cx, *operand);
                let cfg_op = pick_unop(*op, cx.body.exprs[*operand].ty, cx.tcx);
                let temp = builder.alloc_temp(ty, span);
                push_assign(builder, span, temp, Rvalue::UnaryOp(cfg_op, inner));
                Operand::Copy(Place { base: temp, projection: Vec::new() })
            }
            HirUnOp::PreInc | HirUnOp::PreDec | HirUnOp::PostInc | HirUnOp::PostDec => {
                // Increment/decrement is a read-modify-write on an
                // lvalue; the canonical lowering threads through a
                // temp and a couple of assignments. Deferred until
                // we have the wider statement-lowering scaffolding.
                todo!("inc/dec lowering — deferred to a follow-up task in 08-cfg")
            }
        },
        HirExprKind::Convert { operand, kind } => {
            let inner = lower_as_rvalue(builder, cx, *operand);
            let from_ty = cx.body.exprs[*operand].ty;
            // No-op convert kinds we just pass through (decay /
            // identity). Everything else materialises a `Cast` rvalue
            // with the appropriate `CastKind`.
            let cast_kind = convert_to_cast_kind(*kind, from_ty, ty, cx.tcx);
            match cast_kind {
                None => inner,
                Some(kind) => {
                    let temp = builder.alloc_temp(ty, span);
                    push_assign(builder, span, temp, Rvalue::Cast { op: inner, to: ty, kind });
                    Operand::Copy(Place { base: temp, projection: Vec::new() })
                }
            }
        }
        HirExprKind::Cast { operand, to } => {
            let inner = lower_as_rvalue(builder, cx, *operand);
            let from_ty = cx.body.exprs[*operand].ty;
            let kind = explicit_cast_kind(from_ty, *to, cx.tcx);
            let temp = builder.alloc_temp(*to, span);
            push_assign(builder, span, temp, Rvalue::Cast { op: inner, to: *to, kind });
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::AddressOf(operand) => {
            let place = lower_as_place(builder, cx, *operand);
            let temp = builder.alloc_temp(ty, span);
            push_assign(builder, span, temp, Rvalue::AddressOf(place));
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::Deref(_) | HirExprKind::Field { .. } | HirExprKind::Index { .. } => {
            // These are lvalues; in value position emit a Copy of the
            // computed Place. The compute itself is delegated to
            // `lower_as_place` so the rules live in exactly one spot.
            let place = lower_as_place(builder, cx, expr_id);
            Operand::Copy(place)
        }
        HirExprKind::Comma { lhs, rhs } => {
            // Sequence point: evaluate lhs for its side effects, drop
            // the value, then evaluate rhs and use that.
            let _ = lower_as_rvalue(builder, cx, *lhs);
            lower_as_rvalue(builder, cx, *rhs)
        }
        HirExprKind::Assign { lhs, rhs } => {
            let dest = lower_as_place(builder, cx, *lhs);
            let value = lower_as_rvalue(builder, cx, *rhs);
            // Emit through the explicit Place form so projections on
            // `dest` (e.g. `*p = v`, `s.f = v`) are honoured.
            builder.push(Statement {
                kind: StatementKind::Assign { place: dest.clone(), rvalue: Rvalue::Use(value) },
                span,
            });
            // The value of an assignment is the *new* value of the
            // lvalue, viewed as an rvalue (C99 §6.5.16p3).
            Operand::Copy(dest)
        }
        HirExprKind::Call { .. } => {
            // Call lowering needs a terminator + fresh continuation
            // block; that is the territory of task 08-10.
            todo!("call lowering — see tasks/08-cfg/10-call-lowering.md")
        }
        HirExprKind::Cond { .. } => {
            // Ternary is handled by the short-circuit pass.
            todo!("ternary lowering — see tasks/08-cfg/05-short-circuit-lowering.md")
        }
    }
}

/// Lower a HIR expression in *lvalue* position. Returns the [`Place`]
/// it names. Panics in debug builds if `expr` is not an lvalue.
pub fn lower_as_place(builder: &mut BodyBuilder, cx: &LowerCx<'_>, expr_id: HirExprId) -> Place {
    let expr = &cx.body.exprs[expr_id];

    match &expr.kind {
        HirExprKind::LocalRef(hir_local) => {
            let local = cx.locals.lookup(*hir_local);
            Place { base: local, projection: Vec::new() }
        }
        HirExprKind::Deref(operand) => {
            // `*p` — evaluate `p` as an rvalue (it is a pointer
            // value), spill into a temp if it is not already a Place,
            // and return Place { base: <p>, projection: [Deref] }.
            let pointer = lower_as_rvalue(builder, cx, *operand);
            let base = operand_to_place(builder, cx, pointer, expr.span);
            let mut projection = base.projection;
            projection.push(Projection::Deref);
            Place { base: base.base, projection }
        }
        HirExprKind::Field { base, field_index } => {
            let base_place = lower_as_place(builder, cx, *base);
            let mut projection = base_place.projection;
            projection.push(Projection::Field(*field_index));
            Place { base: base_place.base, projection }
        }
        HirExprKind::Index { base, index } => {
            // C99: `a[i]` ≡ `*(a + i)`. After typeck the `base` has
            // already decayed array-to-pointer; we treat it as a
            // pointer here, materialise the index as an Operand, and
            // emit a single Place with a `Projection::Index`.
            let pointer = lower_as_rvalue(builder, cx, *base);
            let index_op = lower_as_rvalue(builder, cx, *index);
            let base = operand_to_place(builder, cx, pointer, expr.span);
            let mut projection = base.projection;
            projection.push(Projection::Index(index_op));
            Place { base: base.base, projection }
        }
        // A `Convert { kind: LvalueToRvalue, .. }` only ever wraps an
        // lvalue subexpression; if some upstream pass calls
        // `lower_as_place` on the wrapper we just step through it.
        HirExprKind::Convert { operand, kind: ConvertKind::LvalueToRvalue } => {
            lower_as_place(builder, cx, *operand)
        }
        _ => panic!(
            "lower_as_place: HIR expression {expr_id:?} is not an lvalue (kind = {:?})",
            std::mem::discriminant(&expr.kind),
        ),
    }
}

/// Helper: emit `Statement::Assign { place: <local>, rvalue: rv }` in
/// the current block. The destination is always a bare local (no
/// projections) — this is the shape used for lowering temporaries.
fn push_assign(builder: &mut BodyBuilder, span: Span, local: Local, rvalue: Rvalue) {
    builder.push(Statement {
        kind: StatementKind::Assign {
            place: Place { base: local, projection: Vec::new() },
            rvalue,
        },
        span,
    });
}

/// Materialise an [`Operand`] as a [`Place`]. If the operand is already
/// a `Copy`/`Move` of a place, return that place; otherwise allocate
/// a temporary, write the operand into it, and return a Place naming
/// the temp.
fn operand_to_place(
    builder: &mut BodyBuilder,
    _cx: &LowerCx<'_>,
    op: Operand,
    span: Span,
) -> Place {
    match op {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Const(c) => {
            let ty = c.ty;
            let temp = builder.alloc_temp(ty, span);
            push_assign(builder, span, temp, Rvalue::Use(Operand::Const(c)));
            Place { base: temp, projection: Vec::new() }
        }
    }
}

/// Pick the right typed CFG `BinOp` for a HIR `BinOp`. Uses the
/// operand types (which after usual-arithmetic-conversion are equal
/// for the arithmetic and comparison cases) to choose between
/// signed / unsigned / float / pointer variants.
fn pick_binop(op: HirBinOp, lhs_ty: TyId, rhs_ty: TyId, tcx: &TyCtxt) -> BinOp {
    let lhs_class = classify(lhs_ty, tcx);
    let rhs_class = classify(rhs_ty, tcx);
    match (op, lhs_class, rhs_class) {
        // Pointer arithmetic.
        (HirBinOp::Add, TyClass::Ptr, _) | (HirBinOp::Add, _, TyClass::Ptr) => BinOp::PtrAdd,
        (HirBinOp::Sub, TyClass::Ptr, TyClass::Ptr) => BinOp::PtrDiff,
        (HirBinOp::Sub, TyClass::Ptr, _) => BinOp::PtrSub,

        // Float arithmetic.
        (HirBinOp::Add, TyClass::Float, _) => BinOp::FAdd,
        (HirBinOp::Sub, TyClass::Float, _) => BinOp::FSub,
        (HirBinOp::Mul, TyClass::Float, _) => BinOp::FMul,
        (HirBinOp::Div, TyClass::Float, _) => BinOp::FDiv,
        (HirBinOp::Lt, TyClass::Float, _) => BinOp::FLt,
        (HirBinOp::Le, TyClass::Float, _) => BinOp::FLe,
        (HirBinOp::Gt, TyClass::Float, _) => BinOp::FGt,
        (HirBinOp::Ge, TyClass::Float, _) => BinOp::FGe,

        // Integer arithmetic, pure form.
        (HirBinOp::Add, _, _) => BinOp::Add,
        (HirBinOp::Sub, _, _) => BinOp::Sub,
        (HirBinOp::Mul, _, _) => BinOp::Mul,

        // Signedness-sensitive.
        (HirBinOp::Div, TyClass::SignedInt, _) => BinOp::SDiv,
        (HirBinOp::Div, _, _) => BinOp::UDiv,
        (HirBinOp::Rem, TyClass::SignedInt, _) => BinOp::SRem,
        (HirBinOp::Rem, _, _) => BinOp::URem,

        // Shifts: lhs decides arithmetic vs logical.
        (HirBinOp::Shl, _, _) => BinOp::Shl,
        (HirBinOp::Shr, TyClass::SignedInt, _) => BinOp::AShr,
        (HirBinOp::Shr, _, _) => BinOp::LShr,

        (HirBinOp::BitAnd, _, _) => BinOp::BitAnd,
        (HirBinOp::BitXor, _, _) => BinOp::BitXor,
        (HirBinOp::BitOr, _, _) => BinOp::BitOr,

        (HirBinOp::Eq, _, _) => BinOp::Eq,
        (HirBinOp::Ne, _, _) => BinOp::Ne,
        (HirBinOp::Lt, TyClass::SignedInt, _) => BinOp::SLt,
        (HirBinOp::Le, TyClass::SignedInt, _) => BinOp::SLe,
        (HirBinOp::Gt, TyClass::SignedInt, _) => BinOp::SGt,
        (HirBinOp::Ge, TyClass::SignedInt, _) => BinOp::SGe,
        (HirBinOp::Lt, _, _) => BinOp::ULt,
        (HirBinOp::Le, _, _) => BinOp::ULe,
        (HirBinOp::Gt, _, _) => BinOp::UGt,
        (HirBinOp::Ge, _, _) => BinOp::UGe,

        // Logical and / or are short-circuit; deferred to task 05.
        (HirBinOp::LogAnd, _, _) | (HirBinOp::LogOr, _, _) => {
            unreachable!(
                "pick_binop: short-circuit `&&`/`||` should not reach the straight-line \
                 expression lowering — see tasks/08-cfg/05-short-circuit-lowering.md"
            )
        }
    }
}

/// Pick the right typed CFG `UnOp` for a HIR `UnOp`. `Plus` is a
/// no-op (handled at the call site), `PreInc` etc. are deferred.
fn pick_unop(op: HirUnOp, operand_ty: TyId, tcx: &TyCtxt) -> UnOp {
    match op {
        HirUnOp::Neg => match classify(operand_ty, tcx) {
            TyClass::Float => UnOp::FNeg,
            _ => UnOp::Neg,
        },
        HirUnOp::BitNot => UnOp::BitNot,
        HirUnOp::LogNot => UnOp::LogNot,
        HirUnOp::Plus | HirUnOp::PreInc | HirUnOp::PreDec | HirUnOp::PostInc | HirUnOp::PostDec => {
            unreachable!("pick_unop: caller filtered this case")
        }
    }
}

/// Lightweight type classification used by `pick_binop` / `pick_unop`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TyClass {
    SignedInt,
    UnsignedInt,
    Float,
    Ptr,
    Other,
}

fn classify(id: TyId, tcx: &TyCtxt) -> TyClass {
    match tcx.get(id) {
        Ty::Int { signed: true, .. } => TyClass::SignedInt,
        Ty::Int { signed: false, .. } => TyClass::UnsignedInt,
        Ty::Float(_) | Ty::Complex(_) => TyClass::Float,
        Ty::Ptr(_) => TyClass::Ptr,
        // Arrays decay to pointers (typeck normally inserts the
        // Convert wrapper) but if we see one raw treat it as pointer.
        Ty::Array { .. } => TyClass::Ptr,
        Ty::Func { .. } => TyClass::Ptr,
        Ty::Void | Ty::Record(_) | Ty::Enum(_) | Ty::Error => TyClass::Other,
    }
}

/// Map a HIR [`ConvertKind`] to a CFG [`CastKind`], or `None` when the
/// conversion is structurally a no-op (decay, lvalue-to-rvalue,
/// integer-promotion that does not change width, …) and need not
/// materialise a temp.
fn convert_to_cast_kind(
    kind: ConvertKind,
    from_ty: TyId,
    to_ty: TyId,
    tcx: &TyCtxt,
) -> Option<CastKind> {
    use ConvertKind::*;
    match kind {
        // Decays produce a pointer value at the same address as the
        // source; in the CFG we model them as a no-op (no Cast),
        // because the operand already has a Place.
        ArrayToPtr | FuncToPtr | LvalueToRvalue => None,

        // Pointer-to-pointer (compatible / void*) — bitcast.
        Pointer => Some(CastKind::PtrToPtr),

        // Real-to-complex / complex-to-real are not representable as a
        // single CastKind; they are codegen-visible and need their own
        // sequence of stores. Keep them as no-op markers for now and
        // let codegen-llvm split them out (task 09).
        RealToComplex | ComplexToReal => None,

        // Integer promotion / usual arithmetic are real width changes
        // unless from/to are already identical.
        IntegerPromotion | UsualArithmetic => {
            if from_ty == to_ty {
                None
            } else {
                Some(arithmetic_cast(from_ty, to_ty, tcx))
            }
        }
    }
}

/// Pick the cast kind for an explicit `(T)expr`. Drives the same
/// classifier as `arithmetic_cast` plus pointer/integer hops.
fn explicit_cast_kind(from_ty: TyId, to_ty: TyId, tcx: &TyCtxt) -> CastKind {
    let from = classify(from_ty, tcx);
    let to = classify(to_ty, tcx);
    match (from, to) {
        (TyClass::Ptr, TyClass::Ptr) => CastKind::PtrToPtr,
        (TyClass::Ptr, TyClass::SignedInt | TyClass::UnsignedInt) => CastKind::PtrToInt,
        (TyClass::SignedInt | TyClass::UnsignedInt, TyClass::Ptr) => CastKind::IntToPtr,
        _ => arithmetic_cast(from_ty, to_ty, tcx),
    }
}

/// Pick the int-vs-float cast kind for two arithmetic types. Assumes
/// at least one of `from`/`to` is integer-or-float (caller handles
/// pointer pairs).
fn arithmetic_cast(from_ty: TyId, to_ty: TyId, tcx: &TyCtxt) -> CastKind {
    let from = classify(from_ty, tcx);
    let to = classify(to_ty, tcx);
    match (from, to) {
        (TyClass::Float, TyClass::Float) => CastKind::FloatToFloat,
        (TyClass::Float, TyClass::SignedInt | TyClass::UnsignedInt) => CastKind::FloatToInt,
        (TyClass::SignedInt | TyClass::UnsignedInt, TyClass::Float) => CastKind::IntToFloat,
        // Integer<->integer (also covers Bool, Char, etc.).
        _ => CastKind::IntToInt,
    }
}

// Suppress the unused-import warning when the test cfg module is
// disabled — these names are referenced only inside `tests::*`.
#[allow(dead_code)]
fn _unused_imports() {
    let _ = (FloatKind::F32, IntRank::Int);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LocalDecl, Terminator, TerminatorKind};
    use rcc_data_structures::IndexVec;
    use rcc_hir::{HirExpr, ValueCat};
    use rcc_span::DUMMY_SP;

    /// Build a minimal HIR `Body` plus matching CFG `BodyBuilder`
    /// pre-seeded with three locals (`a`, `b`, `c`) of type `int`.
    /// Returns the builder, the HIR body, the type context, the
    /// local-map, and the (HIR) ids for the three locals.
    fn three_int_locals() -> (BodyBuilder, HirBody, TyCtxt, LocalMap, [HirLocal; 3]) {
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;

        // HIR body with three int locals.
        let mut hir_body = HirBody::default();
        let ha = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            is_param: false,
            span: DUMMY_SP,
        });
        let hb = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            is_param: false,
            span: DUMMY_SP,
        });
        let hc = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            is_param: false,
            span: DUMMY_SP,
        });

        // CFG builder with return slot + 3 user locals.
        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let cb = builder.alloc_user_local(rcc_span::Symbol(2), int_ty, DUMMY_SP);
        let cc = builder.alloc_user_local(rcc_span::Symbol(3), int_ty, DUMMY_SP);

        let mut map = LocalMap::new();
        map.insert(ha, ca);
        map.insert(hb, cb);
        map.insert(hc, cc);

        (builder, hir_body, tcx, map, [ha, hb, hc])
    }

    /// Helper: push an expression into `body.exprs` and return its id.
    fn push_expr(body: &mut HirBody, ty: TyId, cat: ValueCat, kind: HirExprKind) -> HirExprId {
        let id = HirExprId(u32::try_from(body.exprs.len()).expect("HirExprId overflow"));
        body.exprs.push(HirExpr { id, ty, value_cat: cat, span: DUMMY_SP, kind })
    }

    /// Helper: finish the builder safely after running the lowering
    /// (always terminate the entry block first).
    fn finish(mut b: BodyBuilder) -> crate::Body {
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        b.finish()
    }

    /// `IntConst(42)` lowers to `Operand::Const(Int(42))`, no temps,
    /// no statements.
    #[test]
    fn int_const_is_pure_const() {
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let mut hir_body = HirBody::default();
        let id = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(42));

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);

        let map = LocalMap::new();
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, id);
        match op {
            Operand::Const(Const { kind: ConstKind::Int(v), .. }) => assert_eq!(v, 42),
            other => panic!("expected Int const, got {other:?}"),
        }
        let body = finish(builder);
        // Only the seeded ret slot — no temp allocated for the const.
        assert_eq!(body.locals.len(), 1);
        // Entry block: no statements.
        assert!(body.blocks[crate::BasicBlockId(0)].statements.is_empty());
    }

    /// `LocalRef(a)` lowers to `Copy(Place { base: a, projection: [] })`.
    #[test]
    fn local_ref_is_copy_of_place() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let id = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, id);
        let cfg_a = map.lookup(ha);
        match op {
            Operand::Copy(Place { base, ref projection }) if projection.is_empty() => {
                assert_eq!(base, cfg_a);
            }
            other => panic!("expected Copy(Place(a)), got {other:?}"),
        }
        let body = finish(builder);
        // No new temps.
        assert_eq!(body.locals.len(), 4);
    }

    /// Acceptance: `a + b * c` emits a single temp for `b*c`, then an add.
    #[test]
    fn acceptance_a_plus_b_times_c() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let c = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hc));
        let bc = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::Mul, lhs: b, rhs: c },
        );
        let abc = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::Add, lhs: a, rhs: bc },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, abc);
        // The outer add also allocates a temp; that temp is what we
        // get back.
        let body = finish(builder);

        // 4 base locals (ret + a + b + c) + 1 mul temp + 1 add temp = 6.
        assert_eq!(
            body.locals.len(),
            6,
            "expected exactly two lowering temps (mul + add), got {} locals total",
            body.locals.len()
        );

        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // Two assigns: tmp_mul = b * c; tmp_add = a + tmp_mul.
        assert_eq!(stmts.len(), 2);

        // Statement 0: `_t<mul> = b * c`.
        let (mul_dest, mul_lhs, mul_rhs) = match &stmts[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::BinaryOp(BinOp::Mul, lhs, rhs),
            } => {
                assert!(projection.is_empty());
                (*base, lhs.clone(), rhs.clone())
            }
            other => panic!("expected `_t = Mul`, got {other:?}"),
        };
        let cfg_b = map.lookup(hb);
        let cfg_c = map.lookup(hc);
        assert!(matches!(mul_lhs, Operand::Copy(Place { base, ref projection })
                if projection.is_empty() && base == cfg_b));
        assert!(matches!(mul_rhs, Operand::Copy(Place { base, ref projection })
                if projection.is_empty() && base == cfg_c));

        // Statement 1: `_t<add> = a + _t<mul>`.
        let (add_dest, add_lhs, add_rhs) = match &stmts[1].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::BinaryOp(BinOp::Add, lhs, rhs),
            } => {
                assert!(projection.is_empty());
                (*base, lhs.clone(), rhs.clone())
            }
            other => panic!("expected `_t = Add`, got {other:?}"),
        };
        let cfg_a = map.lookup(ha);
        assert!(matches!(add_lhs, Operand::Copy(Place { base, ref projection })
                if projection.is_empty() && base == cfg_a));
        // The add's rhs is the mul's destination temp.
        assert!(matches!(add_rhs, Operand::Copy(Place { base, ref projection })
                if projection.is_empty() && base == mul_dest));

        // The returned operand names the *outer* add's destination temp.
        match result {
            Operand::Copy(Place { base, ref projection }) if projection.is_empty() => {
                assert_eq!(base, add_dest);
            }
            other => panic!("expected Copy of add's temp, got {other:?}"),
        }

        // Sanity: the two temps are distinct.
        assert_ne!(mul_dest, add_dest);

        // Silence the unused-binding lint on locals we only use inside
        // assertions above.
        let _ = (mul_lhs, mul_rhs, add_lhs, add_rhs);
    }

    /// Acceptance: `*p` lowers to a Place with a single `Deref` step.
    #[test]
    fn acceptance_deref_lvalue() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty)));

        // A single HIR local `p` of type `int*`.
        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ptr_ty,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), int_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        // `p` (lvalue, int*); `*p` (lvalue, int).
        let p = push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let star_p = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::Deref(p));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, star_p);
        assert_eq!(place.base, cp);
        assert_eq!(place.projection.len(), 1);
        assert!(matches!(place.projection[0], Projection::Deref));

        let body = finish(builder);
        // No temps for `*p` in lvalue position — the lookup of `p`
        // alone is a Place, no spilling needed.
        assert_eq!(body.locals.len(), 2);
        assert!(body.blocks[crate::BasicBlockId(0)].statements.is_empty());
    }

    /// `&x` lowers to `Rvalue::AddressOf(<x>)` stored in a temp.
    #[test]
    fn address_of_local() {
        let (mut builder, mut hir_body, mut tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let int_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty)));

        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let amp =
            push_expr(&mut hir_body, int_ptr_ty, ValueCat::RValue, HirExprKind::AddressOf(a_lv));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, amp);
        let body = finish(builder);
        // One AddressOf temp.
        assert_eq!(body.locals.len(), 5);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StatementKind::Assign { place: _, rvalue: Rvalue::AddressOf(p) } => {
                assert_eq!(p.base, map.lookup(ha));
                assert!(p.projection.is_empty());
            }
            other => panic!("expected AddressOf, got {other:?}"),
        }
        // Returned operand is a Copy of that AddressOf temp.
        assert!(matches!(op, Operand::Copy(_)));
    }

    /// Unary `-x` lowers to `UnaryOp(Neg, Copy(x))` for an integer.
    #[test]
    fn unary_neg_int() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let neg = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::Neg, operand: a_lv },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, neg);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1);
        assert!(matches!(
            &stmts[0].kind,
            StatementKind::Assign { rvalue: Rvalue::UnaryOp(UnOp::Neg, _), .. }
        ));
    }

    /// Comma drops the lhs value but keeps lhs side effects, returns rhs.
    #[test]
    fn comma_returns_rhs() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let comma = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Comma { lhs: a_lv, rhs: b_lv },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, comma);
        let body = finish(builder);
        // No temps — both sides are bare local refs.
        assert_eq!(body.locals.len(), 4);
        match op {
            Operand::Copy(Place { base, .. }) => assert_eq!(base, map.lookup(hb)),
            other => panic!("comma should yield rhs (b), got {other:?}"),
        }
    }

    /// `a = b` returns the dest as an Operand, and emits `a := Copy(b)`.
    #[test]
    fn assignment_emits_use() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let assign = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Assign { lhs: a_lv, rhs: b_lv },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, assign);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Copy(Place { base: src_base, .. })),
            } => {
                assert!(projection.is_empty());
                assert_eq!(*base, map.lookup(ha));
                assert_eq!(*src_base, map.lookup(hb));
            }
            other => panic!("expected `a = Copy(b)`, got {other:?}"),
        }
        // Result of an assignment is the new value of the lhs, viewed
        // as Copy of the destination Place.
        match op {
            Operand::Copy(Place { base, .. }) => assert_eq!(base, map.lookup(ha)),
            other => panic!("expected Copy of dest, got {other:?}"),
        }
    }

    /// `Convert { kind: IntegerPromotion, .. }` from `int` to `int`
    /// should be a no-op (no Cast emitted).
    #[test]
    fn convert_same_type_is_no_op() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cv = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Convert { operand: a_lv, kind: ConvertKind::IntegerPromotion },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, cv);
        let body = finish(builder);
        // No statements, no extra locals.
        assert_eq!(body.locals.len(), 4);
        assert!(body.blocks[crate::BasicBlockId(0)].statements.is_empty());
    }

    /// `Convert { kind: UsualArithmetic, .. }` from `int` to `long`
    /// emits a single `IntToInt` Cast.
    #[test]
    fn convert_int_to_long_emits_cast() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let long_ty = tcx.long;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cv = push_expr(
            &mut hir_body,
            long_ty,
            ValueCat::RValue,
            HirExprKind::Convert { operand: a_lv, kind: ConvertKind::UsualArithmetic },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, cv);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StatementKind::Assign {
                rvalue: Rvalue::Cast { kind: CastKind::IntToInt, to, .. },
                ..
            } => assert_eq!(*to, long_ty),
            other => panic!("expected IntToInt cast to long, got {other:?}"),
        }
    }

    /// Explicit `(double)i` lowers to `Rvalue::Cast { IntToFloat }`.
    #[test]
    fn explicit_int_to_float_cast() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let dbl_ty = tcx.double;
        let a_lv = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cast = push_expr(
            &mut hir_body,
            dbl_ty,
            ValueCat::RValue,
            HirExprKind::Cast { operand: a_lv, to: dbl_ty },
        );
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, cast);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StatementKind::Assign {
                rvalue: Rvalue::Cast { kind: CastKind::IntToFloat, to, .. },
                ..
            } => assert_eq!(*to, dbl_ty),
            other => panic!("expected IntToFloat cast, got {other:?}"),
        }
    }

    /// `s.f` (Field) projects through a Place.
    #[test]
    fn field_projection() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        // Pretend we have a record DefId(0).
        let rec_ty = tcx.intern(Ty::Record(rcc_hir::DefId(0)));

        let mut hir_body = HirBody::default();
        let hs = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: rec_ty,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cs = builder.alloc_user_local(rcc_span::Symbol(1), rec_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hs, cs);

        let s = push_expr(&mut hir_body, rec_ty, ValueCat::LValue, HirExprKind::LocalRef(hs));
        let s_dot_f = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Field { base: s, field_index: 2 },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, s_dot_f);
        assert_eq!(place.base, cs);
        assert_eq!(place.projection.len(), 1);
        assert!(matches!(place.projection[0], Projection::Field(2)));
        let _body = finish(builder);
    }

    /// Calling `lower_as_place` on a non-lvalue HIR shape panics. The
    /// IntConst case is the simplest non-lvalue.
    #[test]
    #[should_panic(expected = "is not an lvalue")]
    fn lower_as_place_rejects_rvalue() {
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let mut hir_body = HirBody::default();
        let id = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(7));
        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let map = LocalMap::new();
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_place(&mut builder, &cx, id);
    }

    // Suppress `IndexVec` unused-import lint when no test path
    // references the type directly.
    #[allow(dead_code)]
    fn _suppress_unused_imports() {
        let _ = (LocalDecl { name: None, ty: TyId(0), is_param: false, span: DUMMY_SP },);
        let _: IndexVec<crate::BasicBlockId, crate::BasicBlock> = IndexVec::new();
    }
}
