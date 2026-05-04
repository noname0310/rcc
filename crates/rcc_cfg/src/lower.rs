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
//! Short-circuit `&&` / `||` and the ternary `a ? b : c` are handled
//! by [`lower_short_circuit`] / [`lower_ternary`] (task 08-05): these
//! terminate the current block with a `SwitchInt` and continue lowering
//! at a fresh join block.
//!
use rcc_data_structures::{FxHashMap, IndexVec};
use rcc_hir::{
    rcc_hir_binop::{BinOp as HirBinOp, UnOp as HirUnOp},
    Body as HirBody, ConvertKind, Def, DefId, FloatKind, HirExprId, HirExprKind, HirStmtId,
    HirStmtKind, IntRank, LayoutCx, Local as HirLocal, OverflowOp, Ty, TyCtxt, TyId, ValueCat,
};
use rcc_span::Span;

use crate::{
    BasicBlockId, BinOp, BodyBuilder, CastKind, Const, ConstKind, Local, Operand, Place,
    Projection, Rvalue, Statement, StatementKind, Terminator, TerminatorKind, UnOp,
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
    /// Shared target layout service used by `sizeof` lowering.
    pub layout: LayoutCx<'a>,
    /// Enclosing function return type, when this context is lowering a
    /// full function body.
    pub ret_ty: Option<TyId>,
}

impl<'a> LowerCx<'a> {
    /// Create a new context.
    #[must_use]
    pub fn new(body: &'a HirBody, tcx: &'a TyCtxt, locals: &'a LocalMap) -> Self {
        Self { body, tcx, locals, layout: LayoutCx::new(tcx), ret_ty: None }
    }

    /// Create a context with access to top-level definitions so record
    /// and enum layout queries can be resolved.
    #[must_use]
    pub fn with_defs(
        body: &'a HirBody,
        tcx: &'a TyCtxt,
        locals: &'a LocalMap,
        defs: &'a IndexVec<DefId, Def>,
    ) -> Self {
        Self { body, tcx, locals, layout: LayoutCx::with_defs(tcx, defs), ret_ty: None }
    }

    /// Create a context with top-level definitions and the enclosing
    /// function return type.
    #[must_use]
    pub fn with_defs_and_return(
        body: &'a HirBody,
        tcx: &'a TyCtxt,
        locals: &'a LocalMap,
        defs: &'a IndexVec<DefId, Def>,
        ret_ty: TyId,
    ) -> Self {
        Self { body, tcx, locals, layout: LayoutCx::with_defs(tcx, defs), ret_ty: Some(ret_ty) }
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
        HirExprKind::IntLiteral { value, .. } | HirExprKind::IntConst(value) => {
            Operand::Const(Const { kind: ConstKind::Int(*value), ty })
        }
        HirExprKind::FloatConst(f) => Operand::Const(Const { kind: ConstKind::Float(*f), ty }),
        HirExprKind::StringRef(def) | HirExprKind::DefRef(def) => {
            // Both refer to a global symbol. The CFG `Const::Global`
            // form already encodes "address of <DefId>"; type carries
            // the pointer-to-... type computed by typeck.
            Operand::Const(Const { kind: ConstKind::Global(*def), ty })
        }
        HirExprKind::LabelAddr(name) => {
            let target = builder.label_block(*name);
            Operand::Const(Const { kind: ConstKind::BlockAddress(target), ty })
        }
        HirExprKind::BuiltinVaArea => {
            let temp = builder.alloc_temp(ty, span);
            push_assign(builder, span, temp, Rvalue::BuiltinVaArea);
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::LocalRef(hir_local) => {
            let local = cx.locals.lookup(*hir_local);
            Operand::Copy(Place { base: local, projection: Vec::new() })
        }
        HirExprKind::Binary { op: HirBinOp::LogAnd, lhs, rhs } => {
            lower_short_circuit(builder, cx, ty, span, /* is_and */ true, *lhs, *rhs)
        }
        HirExprKind::Binary { op: HirBinOp::LogOr, lhs, rhs } => {
            lower_short_circuit(builder, cx, ty, span, /* is_and */ false, *lhs, *rhs)
        }
        HirExprKind::Binary { op, lhs, rhs } => {
            // CFG policy: choose left-to-right evaluation for
            // non-short-circuit binary operands. C99 leaves many of
            // these orders unspecified; fixing one order keeps MIR
            // dumps and downstream codegen deterministic.
            let lhs_op = lower_as_rvalue(builder, cx, *lhs);
            let rhs_op = lower_as_rvalue(builder, cx, *rhs);
            // Pick the typed CFG op. The typed kind is determined by
            // the *operand* type after usual arithmetic conversions
            // (typeck has already promoted both sides).
            let lhs_ty = cx.body.exprs[*lhs].ty;
            let rhs_ty = cx.body.exprs[*rhs].ty;
            let cfg_op = pick_binop(*op, lhs_ty, rhs_ty, cx.tcx);
            let rhs_op =
                if matches!(cfg_op, BinOp::Shl | BinOp::AShr | BinOp::LShr) && rhs_ty != lhs_ty {
                    let rhs_temp = builder.alloc_temp(lhs_ty, span);
                    push_assign(
                        builder,
                        span,
                        rhs_temp,
                        Rvalue::Cast { op: rhs_op, to: lhs_ty, kind: CastKind::IntToInt },
                    );
                    Operand::Copy(Place { base: rhs_temp, projection: Vec::new() })
                } else {
                    rhs_op
                };
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
                lower_inc_dec(builder, cx, ty, span, *op, *operand)
            }
        },
        HirExprKind::Convert { operand, kind } => {
            if *kind == ConvertKind::LvalueToRvalue {
                if let Some(def) = direct_global_ref(cx, *operand) {
                    let operand_ty = cx.body.exprs[*operand].ty;
                    if !matches!(cx.tcx.get(operand_ty), Ty::Array { .. } | Ty::Func { .. }) {
                        let temp = builder.alloc_temp(ty, span);
                        push_assign(builder, span, temp, Rvalue::LoadGlobal { def, ty });
                        return Operand::Copy(Place { base: temp, projection: Vec::new() });
                    }
                }
            }
            if matches!(kind, ConvertKind::ArrayToPtr | ConvertKind::FuncToPtr) {
                if let Some(def) = direct_global_ref(cx, *operand) {
                    return Operand::Const(Const { kind: ConstKind::Global(def), ty });
                }
            }
            if *kind == ConvertKind::FuncToPtr
                && matches!(cx.tcx.get(cx.body.exprs[*operand].ty), Ty::Func { .. })
            {
                let place = lower_as_place(builder, cx, *operand);
                let temp = builder.alloc_temp(ty, span);
                push_assign(builder, span, temp, Rvalue::AddressOf(place));
                return Operand::Copy(Place { base: temp, projection: Vec::new() });
            }
            if *kind == ConvertKind::ArrayToPtr
                && matches!(cx.tcx.get(cx.body.exprs[*operand].ty), Ty::Array { .. })
            {
                let mut place = lower_as_place(builder, cx, *operand);
                place.projection.push(Projection::Index(Operand::Const(Const {
                    kind: ConstKind::Int(0),
                    ty: cx.tcx.long,
                })));
                let temp = builder.alloc_temp(ty, span);
                push_assign(builder, span, temp, Rvalue::AddressOf(place));
                return Operand::Copy(Place { base: temp, projection: Vec::new() });
            }
            if *kind == ConvertKind::ArrayToPtr
                && matches!(cx.tcx.get(cx.body.exprs[*operand].ty), Ty::BuiltinVaList)
            {
                let place = lower_as_place(builder, cx, *operand);
                let temp = builder.alloc_temp(ty, span);
                push_assign(builder, span, temp, Rvalue::AddressOf(place));
                return Operand::Copy(Place { base: temp, projection: Vec::new() });
            }
            let inner = lower_as_rvalue(builder, cx, *operand);
            let from_ty = cx.body.exprs[*operand].ty;
            if matches!(kind, ConvertKind::RealToComplex | ConvertKind::ComplexToReal) {
                let temp = builder.alloc_temp(ty, span);
                let rvalue = match kind {
                    ConvertKind::RealToComplex => Rvalue::ComplexFromReal { real: inner, to: ty },
                    ConvertKind::ComplexToReal => {
                        Rvalue::RealFromComplex { complex: inner, to: ty }
                    }
                    _ => unreachable!(),
                };
                push_assign(builder, span, temp, rvalue);
                return Operand::Copy(Place { base: temp, projection: Vec::new() });
            }
            if let ConvertKind::BitfieldPrecision { width, signed } = *kind {
                let temp = builder.alloc_temp(ty, span);
                push_assign(
                    builder,
                    span,
                    temp,
                    Rvalue::BitfieldPrecision { op: inner, to: ty, width, signed },
                );
                return Operand::Copy(Place { base: temp, projection: Vec::new() });
            }
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
            if *to == cx.tcx.void {
                let _ = lower_as_rvalue(builder, cx, *operand);
                return Operand::Const(Const { kind: ConstKind::ZeroInit, ty: *to });
            }
            let inner = lower_as_rvalue(builder, cx, *operand);
            let from_ty = cx.body.exprs[*operand].ty;
            let kind = explicit_cast_kind(from_ty, *to, cx.tcx);
            let temp = builder.alloc_temp(*to, span);
            push_assign(builder, span, temp, Rvalue::Cast { op: inner, to: *to, kind });
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::SizeofExpr(operand) => lower_sizeof_expr(builder, cx, expr_id, *operand),
        HirExprKind::SizeofType(ty) => lower_sizeof_type(cx, expr_id, *ty),
        HirExprKind::AddressOf(operand) => {
            if let Some(def) = direct_global_ref(cx, *operand) {
                return Operand::Const(Const { kind: ConstKind::Global(def), ty });
            }
            let place = lower_as_place(builder, cx, *operand);
            let temp = builder.alloc_temp(ty, span);
            push_assign(builder, span, temp, Rvalue::AddressOf(place));
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::UnresolvedField { .. } => {
            panic!("unresolved member access reached CFG lowering; run rcc_typeck first")
        }
        HirExprKind::Field { base, field_index } if expr.value_cat == ValueCat::RValue => {
            lower_field_as_rvalue(builder, cx, *base, *field_index, span)
        }
        HirExprKind::CompoundLiteral { .. } | HirExprKind::Deref(_) | HirExprKind::Index { .. } => {
            // These are lvalues; in value position emit a Copy of the
            // computed Place. The compute itself is delegated to
            // `lower_as_place` so the rules live in exactly one spot.
            let place = lower_as_place(builder, cx, expr_id);
            Operand::Copy(place)
        }
        HirExprKind::Field { .. } => {
            // `s.f` is an lvalue iff `s` is. Typeck writes the final
            // category onto the field node; rvalue fields are handled
            // above by materialising their base value first.
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
            if let Some(parts) = compound_assign_parts(cx, *lhs, *rhs) {
                return lower_compound_assign(builder, cx, span, *lhs, parts);
            }
            // CFG policy: compute the destination place before the
            // source value. For assignment expressions with side
            // effects this is the single order represented in MIR.
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
        HirExprKind::Call { callee, args } => {
            // CFG policy: evaluate the callee first, then arguments
            // left-to-right in source order. C99 does not require a
            // particular argument order; conformance tests whose
            // stdout depends on another valid order must be skipped or
            // demoted by `rcc_conformance::metadata`.
            let callee_op = lower_as_rvalue(builder, cx, *callee);
            let arg_ops: Vec<Operand> =
                args.iter().map(|a| lower_as_rvalue(builder, cx, *a)).collect();

            // Void calls produce no value.
            let is_void = matches!(cx.tcx.get(ty), Ty::Void);

            let (destination, result) = if is_void {
                (None, Operand::Const(Const { kind: ConstKind::Int(0), ty }))
            } else {
                let dest_local = builder.alloc_temp(ty, span);
                let dest = Place { base: dest_local, projection: Vec::new() };
                (Some(dest.clone()), Operand::Copy(dest))
            };

            // Create successor block and emit the Call terminator.
            let successor = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::Call {
                    callee: callee_op,
                    args: arg_ops,
                    destination,
                    target: Some(successor),
                },
                span,
            });
            builder.switch_to(successor);
            result
        }
        HirExprKind::StmtExpr { stmts, result } => {
            lower_statement_expression(builder, cx, span, ty, stmts, *result)
        }
        HirExprKind::Cond { cond, then_expr, else_expr } => {
            lower_ternary(builder, cx, ty, span, *cond, *then_expr, *else_expr)
        }
        HirExprKind::OmittedCond { cond, else_expr } => {
            lower_omitted_ternary(builder, cx, ty, span, *cond, *else_expr)
        }
        HirExprKind::BuiltinVaArg { ap, .. } => {
            let ap_op = lower_as_rvalue(builder, cx, *ap);
            let dest_local = builder.alloc_temp(ty, span);
            let dest = Place { base: dest_local, projection: Vec::new() };
            builder.push(Statement {
                kind: StatementKind::Assign {
                    place: dest.clone(),
                    rvalue: Rvalue::BuiltinVaArg { ap: ap_op, ty },
                },
                span,
            });
            Operand::Copy(dest)
        }
        HirExprKind::BuiltinVaStart { ap, last_param } => {
            let ap_op = lower_as_rvalue(builder, cx, *ap);
            let last_op = lower_as_rvalue(builder, cx, *last_param);
            let successor = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::BuiltinVaStart {
                    ap: ap_op,
                    last_param: last_op,
                    target: successor,
                },
                span,
            });
            builder.switch_to(successor);
            Operand::Const(Const { kind: ConstKind::Int(0), ty })
        }
        HirExprKind::BuiltinVaEnd { ap } => {
            let ap_op = lower_as_rvalue(builder, cx, *ap);
            let successor = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::BuiltinVaEnd { ap: ap_op, target: successor },
                span,
            });
            builder.switch_to(successor);
            Operand::Const(Const { kind: ConstKind::Int(0), ty })
        }
        HirExprKind::BuiltinVaCopy { dst, src } => {
            let dst_op = lower_as_rvalue(builder, cx, *dst);
            let src_op = lower_as_rvalue(builder, cx, *src);
            let successor = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::BuiltinVaCopy { dst: dst_op, src: src_op, target: successor },
                span,
            });
            builder.switch_to(successor);
            Operand::Const(Const { kind: ConstKind::Int(0), ty })
        }
        HirExprKind::BuiltinOverflow { op, lhs, rhs, dst, result_ty } => {
            let lhs_op = lower_as_rvalue(builder, cx, *lhs);
            let rhs_op = lower_as_rvalue(builder, cx, *rhs);
            let dst_op = lower_as_rvalue(builder, cx, *dst);
            let temp = builder.alloc_temp(ty, span);
            push_assign(
                builder,
                span,
                temp,
                Rvalue::CheckedOverflow {
                    op: checked_overflow_op(*op),
                    lhs: lhs_op,
                    rhs: rhs_op,
                    dst: Some(dst_op),
                    ty: *result_ty,
                },
            );
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
        HirExprKind::BuiltinOverflowP { op, lhs, rhs, probe, result_ty } => {
            let lhs_op = lower_as_rvalue(builder, cx, *lhs);
            let rhs_op = lower_as_rvalue(builder, cx, *rhs);
            let _probe_op = lower_as_rvalue(builder, cx, *probe);
            let temp = builder.alloc_temp(ty, span);
            push_assign(
                builder,
                span,
                temp,
                Rvalue::CheckedOverflow {
                    op: checked_overflow_op(*op),
                    lhs: lhs_op,
                    rhs: rhs_op,
                    dst: None,
                    ty: *result_ty,
                },
            );
            Operand::Copy(Place { base: temp, projection: Vec::new() })
        }
    }
}

fn lower_field_as_rvalue(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    base: HirExprId,
    field_index: u32,
    span: Span,
) -> Operand {
    let base_op = lower_as_rvalue(builder, cx, base);
    let mut place = operand_to_place(builder, cx, base_op, span);
    place.projection.push(Projection::Field(field_index));
    Operand::Copy(place)
}

fn checked_overflow_op(op: OverflowOp) -> BinOp {
    match op {
        OverflowOp::Add => BinOp::Add,
        OverflowOp::Mul => BinOp::Mul,
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
        HirExprKind::DefRef(def) => {
            Place { base: Local(u32::MAX), projection: vec![Projection::Global(*def)] }
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
            let base_place = if cx.body.exprs[*base].value_cat == ValueCat::LValue {
                lower_as_place(builder, cx, *base)
            } else {
                let base_value = lower_as_rvalue(builder, cx, *base);
                operand_to_place(builder, cx, base_value, expr.span)
            };
            let mut projection = base_place.projection;
            projection.push(Projection::Field(*field_index));
            Place { base: base_place.base, projection }
        }
        HirExprKind::UnresolvedField { .. } => {
            panic!("unresolved member access reached CFG lowering; run rcc_typeck first")
        }
        HirExprKind::Index { base, index } => {
            // C99: `a[i]` ≡ `*(a + i)`. After typeck the `base` has
            // already decayed array-to-pointer; we treat it as a
            // pointer here, materialise the index as an Operand, and
            // emit a single Place with a `Projection::Index`.
            let index_op = lower_as_rvalue(builder, cx, *index);
            let base = match cx.body.exprs[*base].kind {
                HirExprKind::Convert { operand, kind: ConvertKind::ArrayToPtr }
                    if matches!(cx.tcx.get(cx.body.exprs[operand].ty), Ty::Array { .. }) =>
                {
                    lower_as_place(builder, cx, operand)
                }
                _ => {
                    let pointer = lower_as_rvalue(builder, cx, *base);
                    operand_to_place(builder, cx, pointer, expr.span)
                }
            };
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
        HirExprKind::CompoundLiteral { local, init_stmts, .. } => {
            lower_compound_literal_place(builder, cx, expr.span, *local, init_stmts)
        }
        HirExprKind::Comma { lhs, rhs } => {
            let _ = lower_as_rvalue(builder, cx, *lhs);
            lower_as_place(builder, cx, *rhs)
        }
        _ => panic!(
            "lower_as_place: HIR expression {expr_id:?} is not an lvalue (kind = {:?})",
            std::mem::discriminant(&expr.kind),
        ),
    }
}

fn lower_compound_literal_place(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    hir_local: HirLocal,
    init_stmts: &[HirStmtId],
) -> Place {
    let cfg_local = cx.locals.lookup(hir_local);
    let ty = cx.body.locals[hir_local].ty;
    builder.storage_live(cfg_local, span);
    if is_aggregate_ty(cx.tcx, ty) && !is_vla_ty(cx.tcx, ty) {
        builder.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: cfg_local, projection: Vec::new() },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::ZeroInit, ty })),
            },
            span,
        });
    }
    for stmt in init_stmts {
        lower_stmt(builder, cx, *stmt);
    }
    Place { base: cfg_local, projection: Vec::new() }
}

fn lower_inc_dec(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    result_ty: TyId,
    span: Span,
    op: HirUnOp,
    operand: HirExprId,
) -> Operand {
    let place = lower_as_place(builder, cx, operand);
    let operand_ty = cx.body.exprs[operand].ty;
    let is_postfix = matches!(op, HirUnOp::PostInc | HirUnOp::PostDec);
    let is_decrement = matches!(op, HirUnOp::PreDec | HirUnOp::PostDec);

    let old_value = if is_postfix {
        let old_temp = builder.alloc_temp(result_ty, span);
        push_assign(builder, span, old_temp, Rvalue::Use(Operand::Copy(place.clone())));
        Operand::Copy(Place { base: old_temp, projection: Vec::new() })
    } else {
        Operand::Copy(place.clone())
    };

    let new_temp = builder.alloc_temp(operand_ty, span);
    let binop = inc_dec_binop(cx.tcx, operand_ty, is_decrement);
    let step = inc_dec_step(cx.tcx, operand_ty);
    push_assign(builder, span, new_temp, Rvalue::BinaryOp(binop, old_value.clone(), step));

    let new_value = Operand::Copy(Place { base: new_temp, projection: Vec::new() });
    builder.push(Statement {
        kind: StatementKind::Assign { place, rvalue: Rvalue::Use(new_value.clone()) },
        span,
    });

    if is_postfix {
        old_value
    } else {
        new_value
    }
}

#[derive(Copy, Clone)]
struct CompoundAssignParts {
    op: HirBinOp,
    rhs: HirExprId,
    lhs_value_ty: TyId,
    result_ty: TyId,
    final_ty: TyId,
    final_convert: Option<ConvertKind>,
}

fn compound_assign_parts(
    cx: &LowerCx<'_>,
    lhs: HirExprId,
    rhs: HirExprId,
) -> Option<CompoundAssignParts> {
    let (core_rhs, final_convert) = match cx.body.exprs[rhs].kind {
        HirExprKind::Convert { operand, kind }
            if matches!(kind, ConvertKind::IntegerPromotion | ConvertKind::UsualArithmetic) =>
        {
            (operand, Some(kind))
        }
        _ => (rhs, None),
    };
    let HirExprKind::Binary { op, lhs: binary_lhs, rhs: binary_rhs } = cx.body.exprs[core_rhs].kind
    else {
        return None;
    };
    if !same_lvalue_expr(cx.body, lhs, binary_lhs) {
        return None;
    }
    Some(CompoundAssignParts {
        op,
        rhs: binary_rhs,
        lhs_value_ty: cx.body.exprs[binary_lhs].ty,
        result_ty: cx.body.exprs[core_rhs].ty,
        final_ty: cx.body.exprs[rhs].ty,
        final_convert,
    })
}

fn same_lvalue_expr(body: &HirBody, lhs: HirExprId, candidate: HirExprId) -> bool {
    if lhs == candidate {
        return true;
    }
    match body.exprs[candidate].kind {
        HirExprKind::Convert { operand, .. } => same_lvalue_expr(body, lhs, operand),
        _ => false,
    }
}

fn lower_compound_assign(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    lhs: HirExprId,
    parts: CompoundAssignParts,
) -> Operand {
    let dest = lower_as_place(builder, cx, lhs);
    let dest_ty = cx.body.exprs[lhs].ty;
    let mut lhs_value = Operand::Copy(dest.clone());
    if dest_ty != parts.lhs_value_ty {
        let lhs_temp = builder.alloc_temp(parts.lhs_value_ty, span);
        push_assign(
            builder,
            span,
            lhs_temp,
            Rvalue::Cast { op: lhs_value, to: parts.lhs_value_ty, kind: CastKind::IntToInt },
        );
        lhs_value = Operand::Copy(Place { base: lhs_temp, projection: Vec::new() });
    }

    let rhs_value = lower_as_rvalue(builder, cx, parts.rhs);
    let rhs_ty = cx.body.exprs[parts.rhs].ty;
    let cfg_op = pick_binop(parts.op, parts.lhs_value_ty, rhs_ty, cx.tcx);
    let rhs_value = if matches!(cfg_op, BinOp::Shl | BinOp::AShr | BinOp::LShr)
        && rhs_ty != parts.lhs_value_ty
    {
        let rhs_temp = builder.alloc_temp(parts.lhs_value_ty, span);
        push_assign(
            builder,
            span,
            rhs_temp,
            Rvalue::Cast { op: rhs_value, to: parts.lhs_value_ty, kind: CastKind::IntToInt },
        );
        Operand::Copy(Place { base: rhs_temp, projection: Vec::new() })
    } else {
        rhs_value
    };

    let result_temp = builder.alloc_temp(parts.result_ty, span);
    push_assign(builder, span, result_temp, Rvalue::BinaryOp(cfg_op, lhs_value, rhs_value));
    let mut value = Operand::Copy(Place { base: result_temp, projection: Vec::new() });

    if let Some(kind) = parts.final_convert {
        if let Some(cast_kind) = convert_to_cast_kind(kind, parts.result_ty, parts.final_ty, cx.tcx)
        {
            let final_temp = builder.alloc_temp(parts.final_ty, span);
            push_assign(
                builder,
                span,
                final_temp,
                Rvalue::Cast { op: value, to: parts.final_ty, kind: cast_kind },
            );
            value = Operand::Copy(Place { base: final_temp, projection: Vec::new() });
        }
    }

    builder.push(Statement {
        kind: StatementKind::Assign { place: dest.clone(), rvalue: Rvalue::Use(value) },
        span,
    });
    Operand::Copy(dest)
}

fn inc_dec_binop(tcx: &TyCtxt, operand_ty: TyId, is_decrement: bool) -> BinOp {
    match classify(operand_ty, tcx) {
        TyClass::Ptr if is_decrement => BinOp::PtrSub,
        TyClass::Ptr => BinOp::PtrAdd,
        TyClass::Float if is_decrement => BinOp::FSub,
        TyClass::Float => BinOp::FAdd,
        _ if is_decrement => BinOp::Sub,
        _ => BinOp::Add,
    }
}

fn inc_dec_step(tcx: &TyCtxt, operand_ty: TyId) -> Operand {
    match tcx.get(operand_ty) {
        Ty::Float(_) | Ty::Complex(_) => {
            Operand::Const(Const { kind: ConstKind::Float(1.0), ty: operand_ty })
        }
        Ty::Ptr(_) => Operand::Const(Const { kind: ConstKind::Int(1), ty: tcx.long }),
        _ => Operand::Const(Const { kind: ConstKind::Int(1), ty: operand_ty }),
    }
}

/// Lower a HIR statement into the current CFG block.
///
/// Task 08-06 introduces the statement scaffolding needed for `if`/`else`
/// control flow. Statement forms owned by later 08-cfg tasks remain explicit
/// `todo!` arms.
///
/// # Panics
/// Panics on statement kinds owned by later tasks.
pub fn lower_stmt(builder: &mut BodyBuilder, cx: &LowerCx<'_>, stmt_id: HirStmtId) {
    let stmt = &cx.body.stmts[stmt_id];

    // `case` / `default` are labels inside the nearest enclosing switch,
    // not lexical sub-bodies. Switch lowering installs the label->block map
    // and then lowers the whole switch body in source order so statements
    // following a label, including sibling `break` statements, stay in the
    // same case block until another label transfers the cursor.
    if let HirStmtKind::Case { body, .. } | HirStmtKind::Default { body } = &stmt.kind {
        if let Some(case_bb) = builder.switch_case_block(stmt_id) {
            if !builder.is_current_terminated() {
                builder.goto(case_bb, stmt.span);
            }
            builder.switch_to(case_bb);
            lower_stmt(builder, cx, *body);
            return;
        }
    }

    // Labels are always reachable via goto, even if the current block
    // is already terminated.  If we fall through to the label, emit a
    // Goto so the predecessor block is properly terminated.
    if let HirStmtKind::Label { name, body } = &stmt.kind {
        let label_bb = builder.label_block(*name);
        let had_fallthrough = !builder.is_current_terminated();
        if had_fallthrough {
            builder.goto(label_bb, stmt.span);
        }
        builder.switch_to(label_bb);
        if !had_fallthrough {
            builder.emit_label_storage_lives(*name, stmt.span);
        }
        lower_stmt(builder, cx, *body);
        return;
    }

    if builder.is_current_terminated() {
        lower_stmt_in_dead_code(builder, cx, stmt_id);
        return;
    }
    match &stmt.kind {
        HirStmtKind::Block(stmts) => {
            lower_block_stmts(builder, cx, stmt.span, stmts);
        }
        HirStmtKind::Expr(expr) => {
            let _ = lower_as_rvalue(builder, cx, *expr);
        }
        HirStmtKind::If { cond, then_branch, else_branch } => {
            lower_if(builder, cx, stmt.span, *cond, *then_branch, *else_branch);
        }
        HirStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                let value = lower_as_rvalue(builder, cx, *expr);
                let value = if let Some(ret_ty) = cx.ret_ty {
                    lower_complex_coercion_if_needed(
                        builder,
                        cx,
                        stmt.span,
                        value,
                        cx.body.exprs[*expr].ty,
                        ret_ty,
                    )
                } else {
                    value
                };
                builder.push(Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(0), projection: Vec::new() },
                        rvalue: Rvalue::Use(value),
                    },
                    span: stmt.span,
                });
            }
            // `return` exits every open lexical scope: emit StorageDead
            // for all currently-live block-scoped locals in reverse
            // declaration order before the `Return` terminator.
            builder.emit_storage_deads_to_depth(0, stmt.span);
            builder.terminate(Terminator { kind: TerminatorKind::Return, span: stmt.span });
        }
        HirStmtKind::Null => {}
        HirStmtKind::LocalDecl { local, init } => {
            lower_local_decl(builder, cx, stmt.span, *local, *init);
        }
        HirStmtKind::While { cond, body } => {
            lower_while(builder, cx, stmt.span, *cond, *body);
        }
        HirStmtKind::DoWhile { body, cond } => {
            lower_do_while(builder, cx, stmt.span, *body, *cond);
        }
        HirStmtKind::For { init, cond, step, body } => {
            lower_for(builder, cx, stmt.span, *init, *cond, *step, *body);
        }
        HirStmtKind::Switch { cond, body, cases } => {
            lower_switch(builder, cx, stmt.span, *cond, *body, cases);
        }
        HirStmtKind::Case { body, .. } | HirStmtKind::Default { body } => {
            // Top-level Case/Default outside a switch is invalid HIR, but
            // leave the body lowering tolerant so earlier diagnostics can
            // keep collecting errors.
            lower_stmt(builder, cx, *body);
        }
        HirStmtKind::Goto(name) => {
            builder.goto_label(*name, stmt.span);
        }
        HirStmtKind::GotoComputed(expr) => {
            let target = lower_as_rvalue(builder, cx, *expr);
            let targets = builder.label_targets();
            builder.emit_storage_deads_to_depth(0, stmt.span);
            builder.terminate(Terminator {
                kind: TerminatorKind::IndirectGoto { target, targets },
                span: stmt.span,
            });
        }
        HirStmtKind::Label { .. } => {
            unreachable!("Label is handled before the match")
        }
        HirStmtKind::Break => {
            // `break` exits the innermost enclosing breakable construct
            // (loop or switch).  The break stack preserves nesting order.
            let ctx =
                builder.current_break_ctx().expect("break statement outside of a loop or switch");
            // Flush every scope opened *inside* the breakable construct.
            builder.emit_storage_deads_to_depth(ctx.scope_depth, stmt.span);
            builder.goto(ctx.target, stmt.span);
        }
        HirStmtKind::Continue => {
            let loop_ctx = *builder.current_loop().expect("continue statement outside of a loop");
            // Flush every scope opened *inside* the loop body.
            builder.emit_storage_deads_to_depth(loop_ctx.scope_depth, stmt.span);
            builder.goto(loop_ctx.cont_target, stmt.span);
        }
    }
}

/// When the current block is already terminated, scan the statement
/// tree for labels that may be goto targets and lower them.
/// Containers (Block, If, loops, switch) are recursed into; everything
/// else is skipped because it is truly unreachable.
fn lower_stmt_in_dead_code(builder: &mut BodyBuilder, cx: &LowerCx<'_>, stmt_id: HirStmtId) {
    let stmt = &cx.body.stmts[stmt_id];
    match &stmt.kind {
        HirStmtKind::Label { name, body } => {
            let label_bb = builder.label_block(*name);
            // In dead code, no predecessor needs a goto to the label.
            builder.switch_to(label_bb);
            builder.emit_label_storage_lives(*name, stmt.span);
            lower_stmt(builder, cx, *body);
        }
        HirStmtKind::Block(stmts) => {
            builder.enter_scope();
            for child in stmts {
                if builder.is_current_terminated() {
                    lower_stmt_in_dead_code(builder, cx, *child);
                } else {
                    lower_stmt(builder, cx, *child);
                }
            }
            builder.exit_scope(stmt.span);
        }
        HirStmtKind::If { then_branch, else_branch, .. } => {
            lower_stmt_in_dead_code(builder, cx, *then_branch);
            if let Some(else_b) = else_branch {
                lower_stmt_in_dead_code(builder, cx, *else_b);
            }
        }
        HirStmtKind::While { body, .. } | HirStmtKind::DoWhile { body, .. } => {
            lower_stmt_in_dead_code(builder, cx, *body);
        }
        HirStmtKind::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                lower_stmt_in_dead_code(builder, cx, *init_stmt);
            }
            lower_stmt_in_dead_code(builder, cx, *body);
        }
        HirStmtKind::Switch { body, .. } => {
            lower_stmt_in_dead_code(builder, cx, *body);
        }
        HirStmtKind::Case { body, .. } | HirStmtKind::Default { body } => {
            if let Some(case_bb) = builder.switch_case_block(stmt_id) {
                builder.switch_to(case_bb);
                lower_stmt(builder, cx, *body);
            } else {
                lower_stmt_in_dead_code(builder, cx, *body);
            }
        }
        _ => {}
    }
}

fn lower_if(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    cond: HirExprId,
    then_branch: HirStmtId,
    else_branch: Option<HirStmtId>,
) {
    let cond_op = lower_condition_operand(builder, cx, span, cond);
    let then_block = builder.new_block();

    match else_branch {
        None => {
            let join_block = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Some(0), join_block), (None, then_block)],
                },
                span,
            });

            builder.switch_to(then_block);
            lower_stmt(builder, cx, then_branch);
            if !builder.is_current_terminated() {
                builder.goto(join_block, span);
            }

            builder.switch_to(join_block);
        }
        Some(else_branch) => {
            let else_block = builder.new_block();
            builder.terminate(Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Some(0), else_block), (None, then_block)],
                },
                span,
            });

            builder.switch_to(then_block);
            lower_stmt(builder, cx, then_branch);
            let then_end = builder.current();
            let then_terminated = builder.is_current_terminated();

            builder.switch_to(else_block);
            lower_stmt(builder, cx, else_branch);
            let else_end = builder.current();
            let else_terminated = builder.is_current_terminated();

            match (then_terminated, else_terminated) {
                (true, true) => {}
                (false, false) => {
                    let join_block = builder.new_block();
                    builder.switch_to(then_end);
                    builder.goto(join_block, span);
                    builder.switch_to(else_end);
                    builder.goto(join_block, span);
                    builder.switch_to(join_block);
                }
                (false, true) => {
                    let join_block = builder.new_block();
                    builder.switch_to(then_end);
                    builder.goto(join_block, span);
                    builder.switch_to(join_block);
                }
                (true, false) => {
                    let join_block = builder.new_block();
                    builder.switch_to(else_end);
                    builder.goto(join_block, span);
                    builder.switch_to(join_block);
                }
            }
        }
    }
}

/// Lower a short-circuit `&&` (`is_and == true`) or `||` operator.
///
/// Emits the canonical 3-block diamond:
///
/// ```text
/// current:
///   result_temp := <short-circuit answer>   ; 0 for &&, 1 for ||
///   discr = lower(lhs)
///   switch_int discr {
///     case 0:  short_circuit_target,    ; join for &&, rhs for ||
///     default: long_path_target,        ; rhs for &&, join for ||
///   }
///
/// rhs:
///   rhs_op = lower(rhs)
///   result_temp := rhs_op != 0           ; normalise to 0/1
///   goto join
///
/// join:
///   ; cursor lands here; subsequent statements append to this block
/// ```
///
/// The pre-initialisation in `current` is what makes the short-circuit
/// path correct: `&&` yields `0` when `lhs == 0`, `||` yields `1` when
/// `lhs != 0`, and in both cases the join is reached without re-writing
/// the temp.
fn lower_short_circuit(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    ty: rcc_hir::TyId,
    span: Span,
    is_and: bool,
    lhs: rcc_hir::HirExprId,
    rhs: rcc_hir::HirExprId,
) -> Operand {
    // Allocate the result temp and pre-initialise it to the short-circuit
    // answer (`0` for `&&`, `1` for `||`).
    let result_local = builder.alloc_temp(ty, span);
    let init_value: i128 = if is_and { 0 } else { 1 };
    push_assign(
        builder,
        span,
        result_local,
        Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(init_value), ty })),
    );

    // Evaluate lhs. The recursion may itself emit blocks (e.g. nested
    // short-circuit / ternary); the cursor afterwards is wherever lhs's
    // evaluation ended, which is the block we terminate with the branch.
    let lhs_op = lower_condition_operand(builder, cx, span, lhs);

    // Allocate the rhs and join blocks (cursor unchanged).
    let rhs_block = builder.new_block();
    let join_block = builder.new_block();

    // Branch on lhs.
    //   &&: zero -> join (skip rhs, keep result = 0); non-zero -> rhs.
    //   ||: zero -> rhs (need to inspect rhs);        non-zero -> join (keep 1).
    let (zero_target, default_target) =
        if is_and { (join_block, rhs_block) } else { (rhs_block, join_block) };
    builder.terminate(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: lhs_op,
            targets: vec![(Some(0), zero_target), (None, default_target)],
        },
        span,
    });

    // Lower rhs in rhs_block, normalise to 0/1 via `rhs_op != 0`, and join.
    builder.switch_to(rhs_block);
    let rhs_op = lower_as_rvalue(builder, cx, rhs);
    let rhs_ty = cx.body.exprs[rhs].ty;
    let rhs_zero = scalar_zero(cx.tcx, rhs_ty);
    push_assign(builder, span, result_local, Rvalue::BinaryOp(BinOp::Ne, rhs_op, rhs_zero));
    // Use the cursor (might differ from rhs_block if rhs itself emitted
    // sub-blocks); the goto terminates whatever current is.
    builder.goto(join_block, span);

    // Continue lowering at the join block.
    builder.switch_to(join_block);

    Operand::Copy(Place { base: result_local, projection: Vec::new() })
}

/// Lower a ternary `cond ? then_expr : else_expr`.
///
/// Emits 4 blocks (current + then + else + join):
///
/// ```text
/// current:
///   cond_op = lower(cond)
///   switch_int cond_op {
///     case 0:  else_block,
///     default: then_block,
///   }
///
/// then_block:
///   result_temp := lower(then_expr)
///   goto join
///
/// else_block:
///   result_temp := lower(else_expr)
///   goto join
///
/// join:
///   ; cursor
/// ```
fn lower_ternary(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    ty: rcc_hir::TyId,
    span: Span,
    cond: rcc_hir::HirExprId,
    then_expr: rcc_hir::HirExprId,
    else_expr: rcc_hir::HirExprId,
) -> Operand {
    // Lower the controlling expression in the current block.
    let cond_op = lower_condition_operand(builder, cx, span, cond);

    let is_void_result = matches!(cx.tcx.get(ty), Ty::Void);
    let result_local = (!is_void_result).then(|| builder.alloc_temp(ty, span));
    let then_block = builder.new_block();
    let else_block = builder.new_block();
    let join_block = builder.new_block();

    // Branch on cond: zero -> else, non-zero -> then.
    builder.terminate(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Some(0), else_block), (None, then_block)],
        },
        span,
    });

    // Then arm.
    builder.switch_to(then_block);
    let then_op = lower_as_rvalue(builder, cx, then_expr);
    if let Some(result_local) = result_local {
        push_assign(builder, span, result_local, Rvalue::Use(then_op));
    }
    builder.goto(join_block, span);

    builder.switch_to(else_block);
    let else_op = lower_as_rvalue(builder, cx, else_expr);
    if let Some(result_local) = result_local {
        push_assign(builder, span, result_local, Rvalue::Use(else_op));
    }
    builder.goto(join_block, span);

    builder.switch_to(join_block);

    if let Some(result_local) = result_local {
        return Operand::Copy(Place { base: result_local, projection: Vec::new() });
    }

    Operand::Const(Const { kind: ConstKind::Int(0), ty })
}

/// Lower a GNU omitted-middle conditional `cond ?: else_expr`.
///
/// This is not equivalent to lowering a synthetic `cond ? cond : else_expr`:
/// GNU C requires the first operand to be evaluated exactly once. We
/// materialize it into the result slot before branching, test that slot, and
/// only overwrite it on the false arm.
fn lower_omitted_ternary(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    ty: rcc_hir::TyId,
    span: Span,
    cond: rcc_hir::HirExprId,
    else_expr: rcc_hir::HirExprId,
) -> Operand {
    if matches!(cx.tcx.get(ty), Ty::Void) {
        let cond_ty = cx.body.exprs[cond].ty;
        let cond_local = builder.alloc_temp(cond_ty, span);
        let cond_op = lower_as_rvalue(builder, cx, cond);
        push_assign(builder, span, cond_local, Rvalue::Use(cond_op));
        let cond_place = Place { base: cond_local, projection: Vec::new() };
        let discr =
            normalize_condition_operand(builder, cx, span, Operand::Copy(cond_place), cond_ty);

        let then_block = builder.new_block();
        let else_block = builder.new_block();
        let join_block = builder.new_block();

        builder.terminate(Terminator {
            kind: TerminatorKind::SwitchInt {
                discr,
                targets: vec![(Some(0), else_block), (None, then_block)],
            },
            span,
        });

        builder.switch_to(then_block);
        builder.goto(join_block, span);

        builder.switch_to(else_block);
        let _else_op = lower_as_rvalue(builder, cx, else_expr);
        builder.goto(join_block, span);

        builder.switch_to(join_block);
        return Operand::Const(Const { kind: ConstKind::Int(0), ty });
    }

    let result_local = builder.alloc_temp(ty, span);
    let result_place = Place { base: result_local, projection: Vec::new() };

    let cond_op = lower_as_rvalue(builder, cx, cond);
    push_assign(builder, span, result_local, Rvalue::Use(cond_op));
    let discr =
        normalize_condition_operand(builder, cx, span, Operand::Copy(result_place.clone()), ty);

    let then_block = builder.new_block();
    let else_block = builder.new_block();
    let join_block = builder.new_block();

    builder.terminate(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr,
            targets: vec![(Some(0), else_block), (None, then_block)],
        },
        span,
    });

    builder.switch_to(then_block);
    builder.goto(join_block, span);

    builder.switch_to(else_block);
    let else_op = lower_as_rvalue(builder, cx, else_expr);
    push_assign(builder, span, result_local, Rvalue::Use(else_op));
    builder.goto(join_block, span);

    builder.switch_to(join_block);

    Operand::Copy(result_place)
}

/// Lower a `while (cond) body` loop.
///
/// Emits the canonical loop structure:
///
/// ```text
/// current:
///   goto header
///
/// header:
///   cond_op = lower(cond)
///   switch_int cond_op {
///     case 0:  exit,
///     default: body_bb,
///   }
///
/// body_bb:
///   lower(body)
///   goto header    ; back edge
///
/// exit:
///   ; cursor lands here
/// ```
///
/// `continue` targets the header; `break` targets the exit block.
fn lower_while(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    cond: HirExprId,
    body: HirStmtId,
) {
    let header = builder.new_block();
    let exit = builder.new_block();

    // Current block → header.
    builder.goto(header, span);

    // Loop context: continue → header, break → exit.
    builder.push_loop(header, exit);

    // Header: evaluate condition and branch.
    builder.switch_to(header);
    let cond_op = lower_condition_operand(builder, cx, span, cond);
    let body_bb = builder.new_block();
    builder.terminate(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Some(0), exit), (None, body_bb)],
        },
        span,
    });

    // Body.
    builder.switch_to(body_bb);
    lower_stmt(builder, cx, body);
    if !builder.is_current_terminated() {
        builder.goto(header, span); // back edge
    }

    builder.pop_loop();
    builder.switch_to(exit);
}

/// Lower a `do body while (cond)` loop.
///
/// Emits the canonical loop structure:
///
/// ```text
/// current:
///   goto body_bb
///
/// body_bb:
///   lower(body)
///   goto cond_bb    ; fall-through to condition
///
/// cond_bb:
///   cond_op = lower(cond)
///   switch_int cond_op {
///     case 0:  exit,
///     default: body_bb,    ; back edge
///   }
///
/// exit:
///   ; cursor lands here
/// ```
///
/// `continue` targets the condition block; `break` targets the exit block.
fn lower_do_while(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    body: HirStmtId,
    cond: HirExprId,
) {
    let body_bb = builder.new_block();
    let cond_bb = builder.new_block();
    let exit = builder.new_block();

    // Current block → body.
    builder.goto(body_bb, span);

    // Loop context: continue → cond_bb, break → exit.
    builder.push_loop(cond_bb, exit);

    // Body.
    builder.switch_to(body_bb);
    lower_stmt(builder, cx, body);
    if !builder.is_current_terminated() {
        builder.goto(cond_bb, span); // fall-through to condition
    }

    // Condition.
    builder.switch_to(cond_bb);
    let cond_op = lower_condition_operand(builder, cx, span, cond);
    builder.terminate(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Some(0), exit), (None, body_bb)],
        },
        span,
    });

    builder.pop_loop();
    builder.switch_to(exit);
}

/// Lower a `for (init; cond; step) body` loop.
///
/// Emits the canonical loop structure:
///
/// ```text
/// current:
///   lower(init)      ; if present
///   goto header
///
/// header:
///   cond_op = lower(cond)    ; if present
///   switch_int cond_op {     ; or goto body_bb if no condition
///     case 0:  exit,
///     default: body_bb,
///   }
///
/// body_bb:
///   lower(body)
///   goto step_bb     ; fall-through to step
///
/// step_bb:
///   lower(step)      ; if present
///   goto header      ; back edge
///
/// exit:
///   ; cursor lands here
/// ```
///
/// `continue` targets the step block; `break` targets the exit block.
fn lower_for(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    init: Option<HirStmtId>,
    cond: Option<HirExprId>,
    step: Option<HirExprId>,
    body: HirStmtId,
) {
    // C99 §6.8.5p5: the for-loop is its own block whose scope encloses
    // the optional declaration in `init`. Open that scope first so a
    // declaration like `for (int i = 0; ...; ...)` is bracketed by
    // `StorageLive(i)` / `StorageDead(i)` covering the whole loop —
    // including the exit block. For an expression init the frame is
    // empty and `exit_scope` is a no-op.
    builder.enter_scope();

    // Lower init if present (runs in the current block).
    if let Some(init_stmt) = init {
        lower_stmt(builder, cx, init_stmt);
    }

    let header = builder.new_block();
    let step_bb = builder.new_block();
    let exit = builder.new_block();

    // Current block → header.
    builder.goto(header, span);

    // Loop context: continue → step_bb, break → exit. Pushed *after*
    // `enter_scope` so the for-loop's own frame stays live across
    // break/continue (those must not StorageDead the loop variable).
    builder.push_loop(step_bb, exit);

    // Header: evaluate condition (if present) and branch.
    builder.switch_to(header);
    let body_bb = builder.new_block();
    match cond {
        Some(cond_expr) => {
            let cond_op = lower_condition_operand(builder, cx, span, cond_expr);
            builder.terminate(Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Some(0), exit), (None, body_bb)],
                },
                span,
            });
        }
        None => {
            // Infinite loop: no condition, unconditionally jump to body.
            builder.goto(body_bb, span);
        }
    }

    // Body.
    builder.switch_to(body_bb);
    lower_stmt(builder, cx, body);
    if !builder.is_current_terminated() {
        builder.goto(step_bb, span); // fall-through to step
    }

    // Step.
    builder.switch_to(step_bb);
    if let Some(step_expr) = step {
        let _ = lower_as_rvalue(builder, cx, step_expr);
    }
    builder.goto(header, span); // back edge

    builder.pop_loop();
    builder.switch_to(exit);

    // Close the for-loop's own scope. If `init` declared a local, this
    // emits the matching `StorageDead` in the exit block.
    builder.exit_scope(span);
}

/// Lower a `switch (cond) { case ...: ... default: ... }` statement.
///
/// The HIR `Switch` node carries a pre-collected `cases` table that maps
/// each `case` value (or `None` for `default`) to the corresponding
/// [`Case`](`/`Default`) statement id.
///
/// Emits the canonical switch structure:
///
/// ```text
/// current:
///   discr = lower(cond)
///   goto dispatch
///
/// dispatch:
///   switch_int discr {
///     case val_0: bb_case_0,
///     case val_1: bb_case_1,
///     ...
///     default:    bb_default,   ; or join if no default
///   }
///
/// body_scan:              ; intentionally unreachable from dispatch
///   lower(full switch body in source order)
///   case/default labels switch the cursor to their dispatch blocks
///
/// join:
///   ; cursor lands here
/// ```
///
/// `break` inside a switch targets the join block.
fn lower_switch(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    cond: HirExprId,
    body: HirStmtId,
    cases: &[rcc_hir::SwitchCase],
) {
    // Evaluate the discriminant.
    let discr = lower_as_rvalue(builder, cx, cond);

    // Current block → dispatch block.
    let dispatch = builder.new_block();
    builder.goto(dispatch, span);

    builder.switch_to(dispatch);

    // Create a block for each case/default label, and remember which HIR
    // statement id owns each block. Lowering the full switch body then
    // treats those statements as labels.
    let mut case_map: FxHashMap<HirStmtId, BasicBlockId> = FxHashMap::default();
    for case in cases {
        case_map.entry(case.target).or_insert_with(|| builder.new_block());
    }

    // Join block (break target).
    let join = builder.new_block();

    // Push switch context so `break` targets the join block.
    builder.push_switch(join);
    builder.push_switch_cases(case_map.clone());

    // Build dispatch targets: (value, block) pairs.
    // C99 allows `default:` anywhere; we normalise it to the last
    // position so codegen can always treat `targets.last()` as the
    // default arm.  `cases` is assumed to be in source order (typeck
    // guarantees this).
    let mut targets: Vec<(Option<i128>, BasicBlockId)> = Vec::with_capacity(cases.len() + 1);
    let mut default_block: Option<BasicBlockId> = None;
    for case in cases {
        let block = case_map[&case.target];
        match case.value {
            Some(v) => targets.push((Some(v), block)),
            None => default_block = Some(block), // multiple defaults rejected by typeck
        }
    }
    targets.push((None, default_block.unwrap_or(join)));

    builder.terminate(Terminator { kind: TerminatorKind::SwitchInt { discr, targets }, span });

    // Lower the whole switch body in source order from a synthetic
    // unreachable scanner block. Case/default labels jump the lowering
    // cursor to their dispatch-selected blocks; all sibling statements after
    // a label remain in that same block until another label is seen.
    let body_scan = builder.new_block();
    builder.switch_to(body_scan);
    lower_stmt(builder, cx, body);
    if !builder.is_current_terminated() {
        builder.goto(join, span);
    }

    builder.pop_switch_cases();
    builder.pop_switch();
    builder.switch_to(join);
}

/// Build a typed scalar zero suitable for `BinOp::Ne` against `ty`.
/// `Float` / `Complex` types get `ConstKind::Float(0.0)`; everything
/// else (integers, pointers — null is `0` in C99) gets
/// `ConstKind::Int(0)`. The returned const carries `ty` so codegen can
/// pick `icmp ne` vs `fcmp` etc. from the operand classifier.
fn scalar_zero(tcx: &rcc_hir::TyCtxt, ty: rcc_hir::TyId) -> Operand {
    match tcx.get(ty) {
        rcc_hir::Ty::Float(_) | rcc_hir::Ty::Complex(_) => {
            Operand::Const(Const { kind: ConstKind::Float(0.0), ty })
        }
        rcc_hir::Ty::Ptr(_) => Operand::Const(Const { kind: ConstKind::ZeroInit, ty }),
        _ => Operand::Const(Const { kind: ConstKind::Int(0), ty }),
    }
}

fn lower_condition_operand(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    expr: HirExprId,
) -> Operand {
    let value = lower_as_rvalue(builder, cx, expr);
    let ty = cx.body.exprs[expr].ty;
    normalize_condition_operand(builder, cx, span, value, ty)
}

fn normalize_condition_operand(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    value: Operand,
    ty: rcc_hir::TyId,
) -> Operand {
    if matches!(cx.tcx.get(ty), rcc_hir::Ty::Int { .. }) {
        return value;
    }

    let result = builder.alloc_temp(cx.tcx.int, span);
    push_assign(builder, span, result, Rvalue::BinaryOp(BinOp::Ne, value, scalar_zero(cx.tcx, ty)));
    Operand::Copy(Place { base: result, projection: Vec::new() })
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

fn lower_complex_coercion_if_needed(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    value: Operand,
    from_ty: TyId,
    to_ty: TyId,
) -> Operand {
    if from_ty == to_ty {
        return value;
    }

    let rvalue = match (is_complex_ty(cx.tcx, from_ty), is_complex_ty(cx.tcx, to_ty)) {
        (false, true) if is_real_arithmetic_ty(cx.tcx, from_ty) => {
            Rvalue::ComplexFromReal { real: value, to: to_ty }
        }
        (true, false) if is_real_arithmetic_ty(cx.tcx, to_ty) => {
            Rvalue::RealFromComplex { complex: value, to: to_ty }
        }
        _ => return value,
    };

    let temp = builder.alloc_temp(to_ty, span);
    push_assign(builder, span, temp, rvalue);
    Operand::Copy(Place { base: temp, projection: Vec::new() })
}

fn is_complex_ty(tcx: &TyCtxt, ty: TyId) -> bool {
    matches!(tcx.get(ty), Ty::Complex(_))
}

fn is_real_arithmetic_ty(tcx: &TyCtxt, ty: TyId) -> bool {
    matches!(tcx.get(ty), Ty::Int { .. } | Ty::Float(_))
}

/// Lower a `Block(stmts)` statement list, applying the aggregate-init
/// recogniser: a `LocalDecl` of an array/record type without an inline
/// initialiser is materialised as a single `place := ZeroInit` store,
/// and the immediately-following run of zero-leaf assignments emitted
/// by `rcc_hir_lower::lower_initializer` is dropped (those leaves are
/// already covered by the zero-fill).
///
/// `int a[1000] = {0};` therefore lowers to one `a := ZeroInit` and no
/// per-element stores; `int a[5] = {1, 2};` lowers to a `ZeroInit`
/// followed by two non-zero leaf stores `a[0] = 1; a[1] = 2;`. Any
/// other statement is forwarded to `lower_stmt`.
fn lower_block_stmts(builder: &mut BodyBuilder, cx: &LowerCx<'_>, span: Span, stmts: &[HirStmtId]) {
    builder.enter_scope();
    let mut i = 0;
    while i < stmts.len() {
        let cur_id = stmts[i];
        lower_stmt(builder, cx, cur_id);
        i += 1;

        // After lowering a `LocalDecl(aggregate, init=None)` we know
        // its CFG side has just emitted a `ZeroInit` store. The HIR
        // initialiser walker emits one leaf assignment per array slot
        // / record field that follows immediately; zero-valued leaves
        // are subsumed by the `ZeroInit` and can be dropped. Non-zero
        // leaves still need to be lowered. The recogniser only spans
        // the contiguous run of leaf assignments rooted at this local
        // — anything else (including the user's own subsequent stores
        // at a different statement) flows through the outer loop.
        if let HirStmtKind::LocalDecl { local: hir_local, init: None } = cx.body.stmts[cur_id].kind
        {
            let ty = cx.body.locals[hir_local].ty;
            if is_aggregate_ty(cx.tcx, ty) {
                while i < stmts.len() && is_leaf_assign_for_hir_local(cx, stmts[i], hir_local) {
                    if !is_zero_const_assign(cx, stmts[i]) {
                        lower_stmt(builder, cx, stmts[i]);
                    }
                    i += 1;
                }
            }
        }
    }
    builder.exit_scope(span);
}

fn lower_statement_expression(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    ty: TyId,
    stmts: &[HirStmtId],
    result: Option<HirExprId>,
) -> Operand {
    builder.enter_scope();
    for stmt in stmts {
        lower_stmt(builder, cx, *stmt);
    }
    let value = if let Some(expr) = result {
        let value = lower_as_rvalue(builder, cx, expr);
        let temp = builder.alloc_temp(ty, span);
        builder.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: temp, projection: Vec::new() },
                rvalue: Rvalue::Use(value),
            },
            span,
        });
        Operand::Copy(Place { base: temp, projection: Vec::new() })
    } else {
        Operand::Const(Const { kind: ConstKind::ZeroInit, ty })
    };
    builder.exit_scope(span);
    value
}

/// Lower a `HirStmtKind::LocalDecl { local, init }`.
///
/// The CFG local has already been allocated by the body-builder pre-pass;
/// `LocalMap::lookup` resolves the HIR id. Always emits `StorageLive(L)`
/// first (it must dominate every use of `L`) and records `L` in the
/// innermost scope frame so the matching `StorageDead` is emitted on
/// every reachable scope-exit path. After the live marker, three cases:
///
/// 1. `init = Some(expr)` — emit a single `cfg_local := <expr>` assign.
/// 2. `init = None` and `ty` is an aggregate (array / struct / union) —
///    emit `cfg_local := ZeroInit`. The trailing per-leaf stores from
///    `rcc_hir_lower::lower_initializer` are recognised and dropped at
///    the `Block` level (see [`lower_block_stmts`]).
/// 3. `init = None` and `ty` is scalar — emit nothing else; the slot
///    is `StorageLive` but its value is indeterminate, matching C99
///    §6.7.8p10.
fn lower_local_decl(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    hir_local: HirLocal,
    init: Option<HirExprId>,
) {
    let cfg_local = cx.locals.lookup(hir_local);
    let ty = cx.body.locals[hir_local].ty;
    if let Some(len_expr) = cx.body.locals[hir_local].vla_len {
        lower_vla_len(builder, cx, span, cfg_local, len_expr);
    }
    builder.storage_live(cfg_local, span);
    if let Some(init_expr) = init {
        // CFG policy: initializer expressions are evaluated at the
        // declaration point after StorageLive. Aggregate leaf stores
        // produced by HIR lowering remain in source/walk order unless
        // they are zero leaves subsumed by the aggregate ZeroInit.
        let value = lower_as_rvalue(builder, cx, init_expr);
        push_assign(builder, span, cfg_local, Rvalue::Use(value));
    } else if is_vla_ty(cx.tcx, ty) {
        // A plain VLA declaration has indeterminate element values. Its
        // runtime allocation is represented by StorageLive + the saved
        // length metadata, not by ZeroInit.
    } else if is_aggregate_ty(cx.tcx, ty) {
        builder.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: cfg_local, projection: Vec::new() },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::ZeroInit, ty })),
            },
            span,
        });
    }
}

fn lower_vla_len(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    span: Span,
    cfg_local: Local,
    len_expr: HirExprId,
) {
    let len_operand = lower_as_rvalue(builder, cx, len_expr);
    let len_ty = cx.body.exprs[len_expr].ty;
    let len_local = builder.alloc_temp(cx.tcx.ulong, span);
    if len_ty == cx.tcx.ulong {
        push_assign(builder, span, len_local, Rvalue::Use(len_operand));
    } else {
        let kind = explicit_cast_kind(len_ty, cx.tcx.ulong, cx.tcx);
        push_assign(
            builder,
            span,
            len_local,
            Rvalue::Cast { op: len_operand, to: cx.tcx.ulong, kind },
        );
    }
    builder.set_vla_len(cfg_local, len_local);
}

fn lower_sizeof_expr(
    builder: &mut BodyBuilder,
    cx: &LowerCx<'_>,
    expr_id: HirExprId,
    operand: HirExprId,
) -> Operand {
    let expr = &cx.body.exprs[expr_id];
    let operand_ty = cx.body.exprs[operand].ty;

    let Ty::Array { elem, is_vla: true, .. } = cx.tcx.get(operand_ty) else {
        let size = layout_size_as_i128(
            cx.layout
                .layout_of(operand_ty)
                .unwrap_or_else(|err| panic!("sizeof layout failed for {operand_ty:?}: {err}")),
        );
        return Operand::Const(Const { kind: ConstKind::Int(size), ty: expr.ty });
    };

    let place = lower_as_place(builder, cx, operand);
    let len_local = builder.alloc_temp(expr.ty, expr.span);
    push_assign(builder, expr.span, len_local, Rvalue::Len(place));

    let elem_size = layout_size_as_i128(cx.layout.layout_of(elem.ty).unwrap_or_else(|err| {
        panic!("sizeof(VLA) element layout failed for {:?}: {err}", elem.ty)
    }));
    if elem_size == 1 {
        return Operand::Copy(Place { base: len_local, projection: Vec::new() });
    }

    let size_local = builder.alloc_temp(expr.ty, expr.span);
    push_assign(
        builder,
        expr.span,
        size_local,
        Rvalue::BinaryOp(
            BinOp::Mul,
            Operand::Copy(Place { base: len_local, projection: Vec::new() }),
            Operand::Const(Const { kind: ConstKind::Int(elem_size), ty: expr.ty }),
        ),
    );
    Operand::Copy(Place { base: size_local, projection: Vec::new() })
}

fn lower_sizeof_type(cx: &LowerCx<'_>, expr_id: HirExprId, ty: TyId) -> Operand {
    let expr = &cx.body.exprs[expr_id];
    let size = layout_size_as_i128(
        cx.layout
            .layout_of(ty)
            .unwrap_or_else(|err| panic!("sizeof(type) layout failed for {ty:?}: {err}")),
    );
    Operand::Const(Const { kind: ConstKind::Int(size), ty: expr.ty })
}

fn layout_size_as_i128(layout: rcc_hir::Layout) -> i128 {
    i128::from(layout.size)
}

/// `true` for C aggregate types (array / struct / union). Enums are
/// scalars at the CFG level (their underlying integer type is what is
/// stored), so they do not count.
fn is_aggregate_ty(tcx: &TyCtxt, ty: TyId) -> bool {
    matches!(tcx.get(ty), Ty::Array { .. } | Ty::Record(_))
}

fn is_vla_ty(tcx: &TyCtxt, ty: TyId) -> bool {
    matches!(tcx.get(ty), Ty::Array { is_vla: true, .. })
}

/// `true` if `stmt_id` is `HirStmtKind::Expr(Assign { lhs, .. })` whose
/// `lhs` is an lvalue chain rooted at `hir_local`. Used by
/// [`lower_block_stmts`] to bound the run of HIR-walker-emitted leaf
/// assignments associated with the preceding aggregate `LocalDecl`.
fn is_leaf_assign_for_hir_local(cx: &LowerCx<'_>, stmt_id: HirStmtId, hir_local: HirLocal) -> bool {
    let HirStmtKind::Expr(expr_id) = cx.body.stmts[stmt_id].kind else {
        return false;
    };
    let HirExprKind::Assign { lhs, .. } = cx.body.exprs[expr_id].kind else {
        return false;
    };
    is_lvalue_rooted_at(cx, lhs, hir_local)
}

/// `true` if `stmt_id` is `Assign { rhs: <zero> }` (any lvalue). The
/// caller has already verified the lvalue root with
/// [`is_leaf_assign_for_hir_local`].
fn is_zero_const_assign(cx: &LowerCx<'_>, stmt_id: HirStmtId) -> bool {
    let HirStmtKind::Expr(expr_id) = cx.body.stmts[stmt_id].kind else {
        return false;
    };
    let HirExprKind::Assign { rhs, .. } = cx.body.exprs[expr_id].kind else {
        return false;
    };
    is_zero_const_expr(cx, rhs)
}

/// `true` if `expr_id` is an lvalue chain (`Field` / `Index` / `Deref`,
/// transparently passing through `Convert` wrappers inserted by typeck)
/// whose root is `LocalRef(hir_local)`.
fn is_lvalue_rooted_at(cx: &LowerCx<'_>, expr_id: HirExprId, hir_local: HirLocal) -> bool {
    let mut cur = expr_id;
    loop {
        match &cx.body.exprs[cur].kind {
            HirExprKind::LocalRef(l) => return *l == hir_local,
            HirExprKind::Field { base, .. } | HirExprKind::Index { base, .. } => {
                cur = *base;
            }
            HirExprKind::Deref(operand) | HirExprKind::Convert { operand, .. } => {
                cur = *operand;
            }
            _ => return false,
        }
    }
}

/// `true` if `expr_id` evaluates to numeric zero. `Convert` and `Cast`
/// wrappers around the constant are stepped through so the recogniser
/// still fires after typeck has inserted implicit conversions.
fn is_zero_const_expr(cx: &LowerCx<'_>, expr_id: HirExprId) -> bool {
    let mut cur = expr_id;
    loop {
        match &cx.body.exprs[cur].kind {
            HirExprKind::IntConst(0) => return true,
            HirExprKind::FloatConst(f) if *f == 0.0 => return true,
            HirExprKind::Convert { operand, .. } | HirExprKind::Cast { operand, .. } => {
                cur = *operand;
            }
            _ => return false,
        }
    }
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

fn direct_global_ref(cx: &LowerCx<'_>, expr_id: HirExprId) -> Option<DefId> {
    match cx.body.exprs[expr_id].kind {
        HirExprKind::DefRef(def) | HirExprKind::StringRef(def) => Some(def),
        _ => None,
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
/// no-op (handled at the call site), `PreInc` etc. are lowered as
/// read-modify-write operations before this helper is reached.
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
        Ty::Void | Ty::Vector { .. } | Ty::Record(_) | Ty::Enum(_) | Ty::Error => TyClass::Other,
        Ty::BuiltinVaList => TyClass::Other,
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

        // Pointer conversions include compatible pointer bitcasts and
        // null-pointer constants that typeck already wrapped with a pointer
        // destination type. Pick the concrete CFG cast from source/target
        // types so `0 -> T *` becomes `IntToPtr`, not `PtrToPtr(0)`.
        Pointer => Some(explicit_cast_kind(from_ty, to_ty, tcx)),

        // Handled directly in `lower_as_rvalue` because these are not
        // casts. They construct/extract complex components and must stay
        // visible to codegen.
        RealToComplex | ComplexToReal => unreachable!("complex conversions are Rvalue nodes"),
        BitfieldPrecision { .. } => unreachable!("bitfield precision conversion is an Rvalue node"),

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
    use rcc_data_structures::Idx;
    use rcc_data_structures::IndexVec;
    use rcc_hir::{HirExpr, HirStmt, ValueCat};
    use rcc_span::{Symbol, DUMMY_SP};

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
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let hb = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let hc = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
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

    /// Helper: push a statement into `body.stmts` and return its id.
    fn push_stmt(body: &mut HirBody, kind: HirStmtKind) -> HirStmtId {
        let id = HirStmtId(u32::try_from(body.stmts.len()).expect("HirStmtId overflow"));
        body.stmts.push(HirStmt { id, span: DUMMY_SP, kind })
    }

    fn block_stmt(body: &mut HirBody, stmts: Vec<HirStmtId>) -> HirStmtId {
        push_stmt(body, HirStmtKind::Block(stmts))
    }

    fn assign_local_stmt(body: &mut HirBody, ty: TyId, local: HirLocal, value: i128) -> HirStmtId {
        let assign = assign_local_expr(body, ty, local, value);
        push_stmt(body, HirStmtKind::Expr(assign))
    }

    fn assign_local_expr(body: &mut HirBody, ty: TyId, local: HirLocal, value: i128) -> HirExprId {
        let lhs = push_expr(body, ty, ValueCat::LValue, HirExprKind::LocalRef(local));
        let rhs = push_expr(body, ty, ValueCat::RValue, HirExprKind::IntConst(value));
        push_expr(body, ty, ValueCat::RValue, HirExprKind::Assign { lhs, rhs })
    }

    fn return_const_stmt(body: &mut HirBody, ty: TyId, value: i128) -> HirStmtId {
        let expr = push_expr(body, ty, ValueCat::RValue, HirExprKind::IntConst(value));
        push_stmt(body, HirStmtKind::Return(Some(expr)))
    }

    fn if_stmt(
        body: &mut HirBody,
        cond: HirExprId,
        then_branch: HirStmtId,
        else_branch: Option<HirStmtId>,
    ) -> HirStmtId {
        push_stmt(body, HirStmtKind::If { cond, then_branch, else_branch })
    }

    fn switch_zero_default(
        block: &crate::BasicBlock,
    ) -> (crate::BasicBlockId, crate::BasicBlockId) {
        match &block.terminator.kind {
            TerminatorKind::SwitchInt { targets, .. } => {
                assert_eq!(targets.len(), 2, "SwitchInt should have zero/default targets");
                assert_eq!(targets[0].0, Some(0), "target[0] should be the zero case");
                assert_eq!(targets[1].0, None, "target[1] should be the default case");
                (targets[0].1, targets[1].1)
            }
            other => panic!("expected SwitchInt, got {other:?}"),
        }
    }

    fn goto_target(block: &crate::BasicBlock) -> crate::BasicBlockId {
        match block.terminator.kind {
            TerminatorKind::Goto(target) => target,
            ref other => panic!("expected Goto, got {other:?}"),
        }
    }

    fn assert_assign_const(block: &crate::BasicBlock, local: Local, value: i128) {
        assert_eq!(block.statements.len(), 1, "expected one assignment statement");
        match &block.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, local);
                assert!(projection.is_empty());
                assert_eq!(*v, value);
            }
            other => panic!("expected `{local:?} = {value}`, got {other:?}"),
        }
    }

    fn assert_switch_discr_local(block: &crate::BasicBlock, local: Local) {
        match &block.terminator.kind {
            TerminatorKind::SwitchInt { discr, .. } => {
                assert!(matches!(
                    discr,
                    Operand::Copy(Place { base, projection })
                        if *base == local && projection.is_empty()
                ));
            }
            other => panic!("expected SwitchInt, got {other:?}"),
        }
    }

    fn assert_return_const(block: &crate::BasicBlock, value: i128) {
        assert_eq!(block.statements.len(), 1, "return value should assign return slot");
        match &block.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, Local(0));
                assert!(projection.is_empty());
                assert_eq!(*v, value);
            }
            other => panic!("expected return-slot assignment, got {other:?}"),
        }
        assert!(matches!(block.terminator.kind, TerminatorKind::Return));
    }

    fn assigned_int_constants(block: &crate::BasicBlock) -> Vec<i128> {
        block
            .statements
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StatementKind::Assign {
                    rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
                    ..
                } => Some(*v),
                _ => None,
            })
            .collect()
    }

    /// Helper: finish the builder safely after running lowering. If the
    /// current block is still open, terminate it with a synthetic return.
    fn finish(mut b: BodyBuilder) -> crate::Body {
        if !b.is_current_terminated() {
            b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        }
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

    #[test]
    fn stmt_expr_materializes_value_before_inner_locals_die() {
        let (mut builder, mut hir_body, tcx, map, [hi, _, _]) = three_int_locals();
        let init = push_expr(&mut hir_body, tcx.int, ValueCat::RValue, HirExprKind::IntConst(2));
        let decl = push_stmt(&mut hir_body, HirStmtKind::LocalDecl { local: hi, init: Some(init) });
        let result = push_expr(&mut hir_body, tcx.int, ValueCat::LValue, HirExprKind::LocalRef(hi));
        let stmt_expr = push_expr(
            &mut hir_body,
            tcx.int,
            ValueCat::RValue,
            HirExprKind::StmtExpr { stmts: vec![decl], result: Some(result) },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, stmt_expr);
        let Operand::Copy(Place { base: result_temp, projection }) = op else {
            panic!("statement expression value should be materialized into a temp");
        };
        assert!(projection.is_empty());

        let cfg_i = map.lookup(hi);
        assert_ne!(result_temp, cfg_i, "result must outlive the statement-expression scope");
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(
            stmts.len(),
            4,
            "expected StorageLive, init, result materialization, StorageDead"
        );
        assert_storage_live(&stmts[0], cfg_i);
        match &stmts[1].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(2), .. })),
            } => {
                assert_eq!(*base, cfg_i);
                assert!(projection.is_empty());
            }
            other => panic!("expected `i := 2`, got {other:?}"),
        }
        match &stmts[2].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue:
                    Rvalue::Use(Operand::Copy(Place { base: copied, projection: copied_projection })),
            } => {
                assert_eq!(*base, result_temp);
                assert!(projection.is_empty());
                assert_eq!(*copied, cfg_i);
                assert!(copied_projection.is_empty());
            }
            other => panic!("expected temp materialization from `i`, got {other:?}"),
        }
        assert_storage_dead(&stmts[3], cfg_i);
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
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
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
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
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

    #[test]
    fn field_projection_from_call_result_uses_call_destination_temp() {
        let (mut builder, mut hir_body, mut tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let rec_ty = tcx.intern(Ty::Record(rcc_hir::DefId(0)));
        let func_ty =
            tcx.intern(Ty::Func { ret: rec_ty, params: Vec::new(), variadic: false, proto: true });
        let func_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(func_ty)));
        let callee = def_ref_expr(&mut hir_body, func_ptr_ty, rcc_hir::DefId::new(55));
        let call = call_expr(&mut hir_body, rec_ty, callee, Vec::new());
        let field = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Field { base: call, field_index: 1 },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let op = lower_as_rvalue(&mut builder, &cx, field);
        let Operand::Copy(Place { base: field_base, projection }) = op else {
            panic!("expected field copy from materialized call result, got {op:?}");
        };
        assert!(matches!(projection.as_slice(), [Projection::Field(1)]));

        let body = finish(builder);
        let call_dest = body.blocks.iter().find_map(|bb| match &bb.terminator.kind {
            TerminatorKind::Call { destination: Some(dest), .. } => Some(dest),
            _ => None,
        });
        let Some(dest) = call_dest else {
            panic!("aggregate call should have a destination temp");
        };
        assert_eq!(dest.base, field_base);
        assert!(dest.projection.is_empty());
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

    // ── Task 08-04: place projection tests ──────────────────────────────

    /// 1. Local variable: `lower_as_place` on `LocalRef` returns
    ///    `Place { base, projection: [] }` with no temporaries.
    #[test]
    fn place_local_variable() {
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;

        let mut hir_body = HirBody::default();
        let hx = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cx_local = builder.alloc_user_local(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hx, cx_local);

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hx));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, x);
        assert_eq!(place.base, cx_local);
        assert!(place.projection.is_empty(), "local variable must have no projections");
        let _body = finish(builder);
    }

    /// 2. Pointer dereference: `*p` → `Place { base: p, proj: [Deref] }`.
    ///    (More targeted variant of `acceptance_deref_lvalue`.)
    #[test]
    fn place_deref_pointer() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty)));

        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ptr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), int_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        let p = push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let star_p = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::Deref(p));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, star_p);

        assert_eq!(place.base, cp);
        assert_eq!(place.projection.len(), 1);
        assert!(matches!(place.projection[0], Projection::Deref));
        let _body = finish(builder);
    }

    /// 3. Struct field: `s.x` → `Place { base: s, proj: [Field(0)] }`.
    ///    (Dedicated test distinct from `field_projection` which uses
    ///    field_index 2.)
    #[test]
    fn place_struct_field() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let rec_ty = tcx.intern(Ty::Record(rcc_hir::DefId(0)));

        let mut hir_body = HirBody::default();
        let hs = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: rec_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cs = builder.alloc_user_local(rcc_span::Symbol(1), rec_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hs, cs);

        let s = push_expr(&mut hir_body, rec_ty, ValueCat::LValue, HirExprKind::LocalRef(hs));
        let s_dot_x = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Field { base: s, field_index: 0 },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, s_dot_x);

        assert_eq!(place.base, cs);
        assert_eq!(place.projection.len(), 1);
        assert!(matches!(place.projection[0], Projection::Field(0)));
        let _body = finish(builder);
    }

    /// 4. Pointer field access: `p->x` lowers to
    ///    `Place { base: p, proj: [Deref, Field(0)] }`.
    ///
    ///    In HIR `p->x` is represented as `Field { base: Deref(p), field_index: 0 }`.
    ///    The lowering chains the Deref projection from the inner expression
    ///    and appends the Field projection, producing a single Place.
    #[test]
    fn place_pointer_field() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let rec_ty = tcx.intern(Ty::Record(rcc_hir::DefId(0)));
        let rec_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(rec_ty)));

        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: rec_ptr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), rec_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        // HIR: `*p` (lvalue, Record)
        let p = push_expr(&mut hir_body, rec_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let deref_p = push_expr(&mut hir_body, rec_ty, ValueCat::LValue, HirExprKind::Deref(p));
        // HIR: `(*p).x` → Field { base: Deref(p), field_index: 0 }
        let arrow_x = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Field { base: deref_p, field_index: 0 },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, arrow_x);

        // Single Place: base = p, projection = [Deref, Field(0)].
        assert_eq!(place.base, cp, "base must be the pointer local");
        assert_eq!(place.projection.len(), 2, "p->x needs exactly two projections");
        assert!(matches!(place.projection[0], Projection::Deref), "first projection must be Deref");
        assert!(
            matches!(place.projection[1], Projection::Field(0)),
            "second projection must be Field(0)"
        );
        let _body = finish(builder);
    }

    /// 5. Array index: `a[i]` → `Place { base: a, proj: [Index(i)] }`.
    ///
    ///    In HIR, after array-to-pointer decay, this is
    ///    `Index { base: Convert(ArrayToPtr, LocalRef(a)), index: LocalRef(i) }`.
    ///    The `Convert` is a no-op for the Place lowering; the base stays
    ///    as the array local and the index becomes an `Operand`.
    #[test]
    fn place_array_index() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let arr_ty = tcx.intern(Ty::Array {
            elem: rcc_hir::Qual::plain(int_ty),
            len: Some(3),
            is_vla: false,
        });

        let mut hir_body = HirBody::default();
        // `a` — array of 3 ints.
        let ha = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: arr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        // `i` — index integer.
        let hi = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(1), arr_ty, DUMMY_SP);
        let ci = builder.alloc_user_local(rcc_span::Symbol(2), int_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(ha, ca);
        map.insert(hi, ci);

        // HIR: `a` (lvalue) wrapped in ArrayToPtr decay, then indexed.
        let a_ref = push_expr(&mut hir_body, arr_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let a_decayed = push_expr(
            &mut hir_body,
            tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty))),
            ValueCat::RValue,
            HirExprKind::Convert { operand: a_ref, kind: ConvertKind::ArrayToPtr },
        );
        let i_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hi));
        let a_sub_i = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Index { base: a_decayed, index: i_ref },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, a_sub_i);

        assert_eq!(place.base, ca, "base must be the array local");
        assert_eq!(place.projection.len(), 1, "a[i] needs exactly one projection");
        match &place.projection[0] {
            Projection::Index(Operand::Copy(Place { base, projection })) => {
                assert!(projection.is_empty(), "index operand must be a bare local");
                assert_eq!(*base, ci, "index operand must be local i");
            }
            other => panic!("expected Projection::Index(Copy(i)), got {other:?}"),
        }
        let _body = finish(builder);
    }

    /// 6. Nested projection: `p->field[2].x` lowers to a single
    ///    `Place { base: p, proj: [Deref, Field(0), Index(2), Field(1)] }`.
    ///
    ///    HIR shape (simplified, no Convert wrappers):
    ///    ```text
    ///    Field {
    ///        base: Index {
    ///            base: Field {
    ///                base: Deref(LocalRef(p)),  // p->field
    ///                field_index: 0,
    ///            },
    ///            index: IntConst(2),            // [2]
    ///        },
    ///        field_index: 1,                     // .x
    ///    }
    ///    ```
    #[test]
    fn place_nested_projection() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let rec_ty = tcx.intern(Ty::Record(rcc_hir::DefId(0)));
        let rec_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(rec_ty)));
        // field 0 of the record is `int[4]`.
        let arr_ty = tcx.intern(Ty::Array {
            elem: rcc_hir::Qual::plain(int_ty),
            len: Some(4),
            is_vla: false,
        });

        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: rec_ptr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), rec_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        // Build HIR bottom-up:
        // `p`
        let p_ref =
            push_expr(&mut hir_body, rec_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        // `*p`
        let deref_p = push_expr(&mut hir_body, rec_ty, ValueCat::LValue, HirExprKind::Deref(p_ref));
        // `(*p).field` (field 0, type arr_ty)
        let p_arrow_field = push_expr(
            &mut hir_body,
            arr_ty,
            ValueCat::LValue,
            HirExprKind::Field { base: deref_p, field_index: 0 },
        );
        // `2` (constant index)
        let idx = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(2));
        // `(*p).field[2]` (element type int)
        let subscript = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Index { base: p_arrow_field, index: idx },
        );
        // `(*p).field[2].x` (field 1)
        let final_access = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Field { base: subscript, field_index: 1 },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let place = lower_as_place(&mut builder, &cx, final_access);

        // Must be a single Place with four chained projections.
        assert_eq!(place.base, cp, "base must be the pointer local");
        assert_eq!(place.projection.len(), 4, "p->field[2].x needs exactly four projections");
        assert!(matches!(place.projection[0], Projection::Deref), "proj[0] must be Deref");
        assert!(matches!(place.projection[1], Projection::Field(0)), "proj[1] must be Field(0)");
        match &place.projection[2] {
            Projection::Index(Operand::Const(Const { kind: ConstKind::Int(v), .. })) => {
                assert_eq!(*v, 2, "proj[2] index must be constant 2");
            }
            other => panic!("expected Projection::Index(Const(2)), got {other:?}"),
        }
        assert!(matches!(place.projection[3], Projection::Field(1)), "proj[3] must be Field(1)");

        let body = finish(builder);
        // Index(2) is a constant operand — no temp needed. Only the
        // return slot + `p` local.
        assert_eq!(body.locals.len(), 2, "no temps for constant-index projection");
    }

    /// 7. Assignment LHS with projection: `*p = 42` emits
    ///    `Assign { place: Place { p, [Deref] }, rvalue: Use(Const(42)) }`.
    #[test]
    fn place_assign_lhs_deref() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty)));

        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ptr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), int_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        // LHS: `*p`.
        let p_ref =
            push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let star_p = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::Deref(p_ref));
        // RHS: `42`.
        let rhs = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(42));
        // `*p = 42`
        let assign = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Assign { lhs: star_p, rhs },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, assign);
        let body = finish(builder);

        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 1, "assignment must emit exactly one statement");
        match &stmts[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, cp, "LHS base must be the pointer local");
                assert_eq!(projection.len(), 1, "LHS must have one projection");
                assert!(matches!(projection[0], Projection::Deref), "LHS projection must be Deref");
                assert_eq!(*v, 42, "RHS must be constant 42");
            }
            other => panic!("expected `*p = Const(42)`, got {other:?}"),
        }
    }

    #[test]
    fn inc_dec_prefix_and_postfix_ints_lower_without_panic() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a_pre = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let pre = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PreInc, operand: a_pre },
        );
        let b_post = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let post_inc = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PostInc, operand: b_post },
        );
        let c_pre = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hc));
        let pre_dec = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PreDec, operand: c_pre },
        );
        let a_post = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let post_dec = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PostDec, operand: a_post },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let pre_op = lower_as_rvalue(&mut builder, &cx, pre);
        let post_inc_op = lower_as_rvalue(&mut builder, &cx, post_inc);
        let pre_dec_op = lower_as_rvalue(&mut builder, &cx, pre_dec);
        let post_dec_op = lower_as_rvalue(&mut builder, &cx, post_dec);

        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 10, "prefix emits 2 statements; postfix emits 3 statements");
        assert!(matches!(pre_op, Operand::Copy(Place { .. })));
        assert!(matches!(post_inc_op, Operand::Copy(Place { .. })));
        assert!(matches!(pre_dec_op, Operand::Copy(Place { .. })));
        assert!(matches!(post_dec_op, Operand::Copy(Place { .. })));
        let add_count = stmts
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    StatementKind::Assign { rvalue: Rvalue::BinaryOp(BinOp::Add, _, _), .. }
                )
            })
            .count();
        let sub_count = stmts
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    StatementKind::Assign { rvalue: Rvalue::BinaryOp(BinOp::Sub, _, _), .. }
                )
            })
            .count();
        assert_eq!(add_count, 2, "`++i` and `i++` must each emit Add");
        assert_eq!(sub_count, 2, "`--i` and `i--` must each emit Sub");
    }

    #[test]
    fn inc_dec_pointer_and_deref_lvalues_lower_without_panic() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_ptr_ty = tcx.intern(Ty::Ptr(rcc_hir::Qual::plain(int_ty)));

        let mut hir_body = HirBody::default();
        let hp = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ptr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let cp = builder.alloc_user_local(rcc_span::Symbol(1), int_ptr_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hp, cp);

        // `p++` — pointer post-increment.
        let p_ref =
            push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let post_ptr = push_expr(
            &mut hir_body,
            int_ptr_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PostInc, operand: p_ref },
        );

        // `(*p)++` — dereferenced int lvalue post-increment.
        let p_ref =
            push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let star_p = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::Deref(p_ref));
        let post_deref = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PostInc, operand: star_p },
        );

        // `*p++` — dereference the old pointer value while incrementing `p`.
        let p_ref =
            push_expr(&mut hir_body, int_ptr_ty, ValueCat::LValue, HirExprKind::LocalRef(hp));
        let post_ptr_for_deref = push_expr(
            &mut hir_body,
            int_ptr_ty,
            ValueCat::RValue,
            HirExprKind::Unary { op: HirUnOp::PostInc, operand: p_ref },
        );
        let star_post_ptr = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::LValue,
            HirExprKind::Deref(post_ptr_for_deref),
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, post_ptr);
        let _ = lower_as_rvalue(&mut builder, &cx, post_deref);
        let _ = lower_as_rvalue(&mut builder, &cx, star_post_ptr);

        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        let ptr_add_count = stmts
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    StatementKind::Assign { rvalue: Rvalue::BinaryOp(BinOp::PtrAdd, _, _), .. }
                )
            })
            .count();
        assert_eq!(ptr_add_count, 2, "`p++` and `*p++` must both increment the pointer");
        assert!(
            stmts.iter().any(|s| matches!(
                s.kind,
                StatementKind::Assign {
                    place: Place { base, ref projection },
                    rvalue: Rvalue::Use(_),
                } if base == cp && matches!(projection.as_slice(), [Projection::Deref])
            )),
            "(*p)++ must store through the dereferenced pointer place"
        );
    }

    /// 8. Non-lvalue expression in place position panics.
    ///    `Binary { Add, a, b }` is an rvalue; `lower_as_place` must
    ///    reject it.
    #[test]
    #[should_panic(expected = "is not an lvalue")]
    fn place_rejects_binary_rvalue() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let sum = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::Add, lhs: a, rhs: b },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_place(&mut builder, &cx, sum);
    }

    // Suppress `IndexVec` unused-import lint when no test path
    // references the type directly.
    #[allow(dead_code)]
    fn _suppress_unused_imports() {
        let _ = (LocalDecl {
            name: None,
            ty: TyId(0),
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        },);
        let _: IndexVec<crate::BasicBlockId, crate::BasicBlock> = IndexVec::new();
    }

    // ── Task 08-05: short-circuit + ternary lowering ────────────────────

    /// Acceptance: `a && b` lowers to a 3-block diamond.
    ///
    /// Layout:
    ///   bb0 (entry): `result := 0; switch a { 0 -> join, _ -> rhs }`
    ///   bb1 (rhs):   `result := b != 0; goto join`
    ///   bb2 (join):  current cursor after lowering
    ///
    /// Verifies the spec acceptance:
    /// > Lowered CFG visits `rhs` block only when `a` is non-zero for `&&`.
    #[test]
    fn short_circuit_and_diamond() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let aandb = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogAnd, lhs: a, rhs: b },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, aandb);
        let body = finish(builder);

        // 3 blocks total: entry + rhs + join.
        assert_eq!(body.blocks.len(), 3, "&& must produce exactly 3 blocks");

        // Returned operand names the result temp.
        let result_local = match result {
            Operand::Copy(Place { base, projection }) if projection.is_empty() => base,
            other => panic!("expected Copy of result temp, got {other:?}"),
        };

        // Entry: pre-init `result := 0`, then SwitchInt on `a`.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_eq!(entry.statements.len(), 1, "entry: pre-init result := 0");
        match &entry.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(0), .. })),
            } => {
                assert!(projection.is_empty());
                assert_eq!(*base, result_local);
            }
            other => panic!("expected `result := 0`, got {other:?}"),
        }

        let (zero_target, default_target) = match &entry.terminator.kind {
            TerminatorKind::SwitchInt { discr, targets } => {
                assert!(
                    matches!(discr, Operand::Copy(Place { base, .. }) if *base == map.lookup(ha))
                );
                assert_eq!(targets.len(), 2, "SwitchInt should have (0, ...) and default");
                assert_eq!(targets[0].0, Some(0), "first target is the zero case");
                assert_eq!(targets[1].0, None, "second target is the default");
                (targets[0].1, targets[1].1)
            }
            other => panic!("expected SwitchInt, got {other:?}"),
        };

        // For `&&`: zero-case is the join (short-circuit), default is rhs.
        let join_block = zero_target;
        let rhs_block = default_target;
        assert_ne!(join_block, rhs_block, "join and rhs must be distinct blocks");

        // RHS block: `result := b != 0; goto join`.
        let rhs_bb = &body.blocks[rhs_block];
        assert_eq!(rhs_bb.statements.len(), 1);
        match &rhs_bb.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::BinaryOp(BinOp::Ne, lhs_op, rhs_op),
            } => {
                assert!(projection.is_empty());
                assert_eq!(*base, result_local);
                assert!(
                    matches!(lhs_op, Operand::Copy(Place { base, .. }) if *base == map.lookup(hb))
                );
                assert!(matches!(rhs_op, Operand::Const(Const { kind: ConstKind::Int(0), .. })));
            }
            other => panic!("expected `result := b != 0`, got {other:?}"),
        }
        assert!(
            matches!(rhs_bb.terminator.kind, TerminatorKind::Goto(t) if t == join_block),
            "rhs block must goto join"
        );

        // Join: cursor lands here after lowering; `finish()` emits the
        // synthetic Return so the test helper is happy. No statements
        // are added by the lowering itself.
        let join_bb = &body.blocks[join_block];
        assert!(join_bb.statements.is_empty(), "join must be empty after lowering");
    }

    /// `a || b` lowers to the mirror diamond: pre-init = 1, zero -> rhs,
    /// non-zero -> join.
    #[test]
    fn short_circuit_or_diamond() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let aorb = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogOr, lhs: a, rhs: b },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _result = lower_as_rvalue(&mut builder, &cx, aorb);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 3, "|| must produce exactly 3 blocks");

        let entry = &body.blocks[crate::BasicBlockId(0)];

        // Entry: `result := 1` (the short-circuit answer for ||).
        match &entry.statements[0].kind {
            StatementKind::Assign {
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(1), .. })),
                ..
            } => {}
            other => panic!("expected `result := 1`, got {other:?}"),
        }

        // SwitchInt: 0 -> rhs, default -> join (mirror of &&).
        match &entry.terminator.kind {
            TerminatorKind::SwitchInt { targets, .. } => {
                assert_eq!(targets[0].0, Some(0));
                assert_eq!(targets[1].0, None);
                let rhs_block = targets[0].1;
                let join_block = targets[1].1;
                assert_ne!(rhs_block, join_block);
                let rhs_bb = &body.blocks[rhs_block];
                assert!(
                    matches!(rhs_bb.terminator.kind, TerminatorKind::Goto(t) if t == join_block),
                    "rhs block must goto join"
                );
            }
            other => panic!("expected SwitchInt, got {other:?}"),
        }
    }

    /// Acceptance: ternary `a ? b : c` lowers to 4 blocks
    /// (entry + then + else + join).
    #[test]
    fn ternary_lowers_to_four_blocks() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let c = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hc));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Cond { cond: a, then_expr: b, else_expr: c },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, cond);
        let body = finish(builder);

        // 4 blocks: entry + then + else + join.
        assert_eq!(body.blocks.len(), 4, "?: must produce exactly 4 blocks");

        let result_local = match result {
            Operand::Copy(Place { base, projection }) if projection.is_empty() => base,
            other => panic!("expected Copy(result_temp), got {other:?}"),
        };

        // Entry: SwitchInt on cond, with case 0 -> else and default -> then.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let (else_block, then_block) = match &entry.terminator.kind {
            TerminatorKind::SwitchInt { discr, targets } => {
                assert!(
                    matches!(discr, Operand::Copy(Place { base, .. }) if *base == map.lookup(ha))
                );
                assert_eq!(targets.len(), 2);
                assert_eq!(targets[0].0, Some(0), "case 0 routes to else branch");
                assert_eq!(targets[1].0, None, "default routes to then branch");
                (targets[0].1, targets[1].1)
            }
            other => panic!("expected SwitchInt, got {other:?}"),
        };
        assert_ne!(else_block, then_block);

        // Then block: `result := Copy(b); goto join`.
        let then_bb = &body.blocks[then_block];
        assert_eq!(then_bb.statements.len(), 1);
        match &then_bb.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Copy(Place { base: src_base, .. })),
            } => {
                assert!(projection.is_empty());
                assert_eq!(*base, result_local);
                assert_eq!(*src_base, map.lookup(hb));
            }
            other => panic!("expected `result := Copy(b)`, got {other:?}"),
        }
        let join_block = match then_bb.terminator.kind {
            TerminatorKind::Goto(t) => t,
            ref other => panic!("expected goto join, got {other:?}"),
        };

        // Else block: `result := Copy(c); goto join` (same join as then).
        let else_bb = &body.blocks[else_block];
        assert_eq!(else_bb.statements.len(), 1);
        match &else_bb.statements[0].kind {
            StatementKind::Assign {
                rvalue: Rvalue::Use(Operand::Copy(Place { base: src_base, .. })),
                ..
            } => {
                assert_eq!(*src_base, map.lookup(hc));
            }
            other => panic!("expected `result := Copy(c)`, got {other:?}"),
        }
        assert!(
            matches!(else_bb.terminator.kind, TerminatorKind::Goto(t) if t == join_block),
            "else block must goto same join as then"
        );

        // Join is the cursor block — empty after lowering.
        assert!(body.blocks[join_block].statements.is_empty());
    }

    // Task 08-06: if/else statement lowering.

    #[test]
    fn if_without_else_branches_then_to_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let root = if_stmt(&mut hir_body, cond, then_stmt, None);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 3, "if without else uses entry + then + join");
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_switch_discr_local(entry, map.lookup(ha));
        let (join_block, then_block) = switch_zero_default(entry);

        let then_bb = &body.blocks[then_block];
        assert_assign_const(then_bb, map.lookup(hb), 1);
        assert_eq!(goto_target(then_bb), join_block);

        let join_bb = &body.blocks[join_block];
        assert!(join_bb.statements.is_empty());
        assert!(matches!(join_bb.terminator.kind, TerminatorKind::Return));
    }

    #[test]
    fn if_with_else_branches_rejoin() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let else_stmt = assign_local_stmt(&mut hir_body, int_ty, hc, 2);
        let root = if_stmt(&mut hir_body, cond, then_stmt, Some(else_stmt));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 4, "if/else uses entry + then + else + join");
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_switch_discr_local(entry, map.lookup(ha));
        let (else_block, then_block) = switch_zero_default(entry);

        let then_bb = &body.blocks[then_block];
        let else_bb = &body.blocks[else_block];
        assert_assign_const(then_bb, map.lookup(hb), 1);
        assert_assign_const(else_bb, map.lookup(hc), 2);

        let then_join = goto_target(then_bb);
        let else_join = goto_target(else_bb);
        assert_eq!(then_join, else_join, "both branches must rejoin");
        assert!(body.blocks[then_join].statements.is_empty());
        assert!(matches!(body.blocks[then_join].terminator.kind, TerminatorKind::Return));
    }

    #[test]
    fn nested_if_preserves_inner_join_before_outer_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let outer_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let inner_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let inner_then = assign_local_stmt(&mut hir_body, int_ty, hc, 2);
        let inner_if = if_stmt(&mut hir_body, inner_cond, inner_then, None);
        let root = if_stmt(&mut hir_body, outer_cond, inner_if, None);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 5);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let (outer_join, outer_then) = switch_zero_default(entry);
        assert_switch_discr_local(entry, map.lookup(ha));

        let outer_then_bb = &body.blocks[outer_then];
        assert_switch_discr_local(outer_then_bb, map.lookup(hb));
        let (inner_join, inner_then_block) = switch_zero_default(outer_then_bb);

        let inner_then_bb = &body.blocks[inner_then_block];
        assert_assign_const(inner_then_bb, map.lookup(hc), 2);
        assert_eq!(goto_target(inner_then_bb), inner_join);

        assert_eq!(goto_target(&body.blocks[inner_join]), outer_join);
        assert!(matches!(body.blocks[outer_join].terminator.kind, TerminatorKind::Return));
    }

    #[test]
    fn else_if_chain_rejoins_after_inner_if() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let outer_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let inner_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let inner_then = assign_local_stmt(&mut hir_body, int_ty, hc, 2);
        let inner_if = if_stmt(&mut hir_body, inner_cond, inner_then, None);
        let root = if_stmt(&mut hir_body, outer_cond, then_stmt, Some(inner_if));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 6);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let (outer_else, outer_then) = switch_zero_default(entry);

        let outer_then_bb = &body.blocks[outer_then];
        assert_assign_const(outer_then_bb, map.lookup(hb), 1);

        let outer_else_bb = &body.blocks[outer_else];
        assert_switch_discr_local(outer_else_bb, map.lookup(hb));
        let (inner_join, inner_then_block) = switch_zero_default(outer_else_bb);
        assert_assign_const(&body.blocks[inner_then_block], map.lookup(hc), 2);
        assert_eq!(goto_target(&body.blocks[inner_then_block]), inner_join);

        let outer_join = goto_target(outer_then_bb);
        assert_eq!(goto_target(&body.blocks[inner_join]), outer_join);
        assert!(matches!(body.blocks[outer_join].terminator.kind, TerminatorKind::Return));
    }

    #[test]
    fn empty_then_block_falls_through_to_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let empty_then = block_stmt(&mut hir_body, Vec::new());
        let root = if_stmt(&mut hir_body, cond, empty_then, None);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let (join_block, then_block) = switch_zero_default(&body.blocks[crate::BasicBlockId(0)]);
        let then_bb = &body.blocks[then_block];
        assert!(then_bb.statements.is_empty());
        assert_eq!(goto_target(then_bb), join_block);
    }

    #[test]
    fn empty_else_block_falls_through_to_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let empty_else = block_stmt(&mut hir_body, Vec::new());
        let root = if_stmt(&mut hir_body, cond, then_stmt, Some(empty_else));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let (else_block, then_block) = switch_zero_default(&body.blocks[crate::BasicBlockId(0)]);
        let then_bb = &body.blocks[then_block];
        let else_bb = &body.blocks[else_block];
        assert_assign_const(then_bb, map.lookup(hb), 1);
        assert!(else_bb.statements.is_empty());
        assert_eq!(goto_target(then_bb), goto_target(else_bb));
    }

    #[test]
    fn if_condition_logical_and_uses_short_circuit_blocks() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogAnd, lhs: a, rhs: b },
        );
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hc, 3);
        let root = if_stmt(&mut hir_body, cond, then_stmt, None);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 5);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let result_local = match &entry.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(0), .. })),
            } => {
                assert!(projection.is_empty());
                *base
            }
            other => panic!("expected && preinit, got {other:?}"),
        };
        let (short_circuit_join, rhs_block) = switch_zero_default(entry);

        let rhs_bb = &body.blocks[rhs_block];
        assert_eq!(goto_target(rhs_bb), short_circuit_join);

        let sc_join_bb = &body.blocks[short_circuit_join];
        assert_switch_discr_local(sc_join_bb, result_local);
        let (if_join, then_block) = switch_zero_default(sc_join_bb);
        assert_assign_const(&body.blocks[then_block], map.lookup(hc), 3);
        assert_eq!(goto_target(&body.blocks[then_block]), if_join);
    }

    #[test]
    fn if_condition_logical_or_uses_short_circuit_blocks() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogOr, lhs: a, rhs: b },
        );
        let then_stmt = assign_local_stmt(&mut hir_body, int_ty, hc, 4);
        let root = if_stmt(&mut hir_body, cond, then_stmt, None);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 5);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let result_local = match &entry.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(1), .. })),
            } => {
                assert!(projection.is_empty());
                *base
            }
            other => panic!("expected || preinit, got {other:?}"),
        };
        let (rhs_block, short_circuit_join) = switch_zero_default(entry);

        let rhs_bb = &body.blocks[rhs_block];
        assert_eq!(goto_target(rhs_bb), short_circuit_join);

        let sc_join_bb = &body.blocks[short_circuit_join];
        assert_switch_discr_local(sc_join_bb, result_local);
        let (if_join, then_block) = switch_zero_default(sc_join_bb);
        assert_assign_const(&body.blocks[then_block], map.lookup(hc), 4);
        assert_eq!(goto_target(&body.blocks[then_block]), if_join);
    }

    #[test]
    fn if_else_both_arms_return_omits_join_block() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let then_ret = return_const_stmt(&mut hir_body, int_ty, 1);
        let else_ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let root = if_stmt(&mut hir_body, cond, then_ret, Some(else_ret));

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 3, "both returning arms must not allocate a join");
        let (else_block, then_block) = switch_zero_default(&body.blocks[crate::BasicBlockId(0)]);
        assert_return_const(&body.blocks[then_block], 1);
        assert_return_const(&body.blocks[else_block], 0);
    }

    // ── Task 08-07: loop lowering ──────────────────────────────────────

    fn while_stmt(body: &mut HirBody, cond: HirExprId, loop_body: HirStmtId) -> HirStmtId {
        push_stmt(body, HirStmtKind::While { cond, body: loop_body })
    }

    fn do_while_stmt(body: &mut HirBody, loop_body: HirStmtId, cond: HirExprId) -> HirStmtId {
        push_stmt(body, HirStmtKind::DoWhile { body: loop_body, cond })
    }

    fn for_stmt(
        body: &mut HirBody,
        init: Option<HirStmtId>,
        cond: Option<HirExprId>,
        step: Option<HirExprId>,
        loop_body: HirStmtId,
    ) -> HirStmtId {
        push_stmt(body, HirStmtKind::For { init, cond, step, body: loop_body })
    }

    fn break_stmt(body: &mut HirBody) -> HirStmtId {
        push_stmt(body, HirStmtKind::Break)
    }

    fn continue_stmt(body: &mut HirBody) -> HirStmtId {
        push_stmt(body, HirStmtKind::Continue)
    }

    fn label_stmt(body: &mut HirBody, name: &str, stmt: HirStmtId) -> HirStmtId {
        // Labels are compared by Symbol equality; we use hard-coded ids
        // because the tests don't need real string interning.
        let sym = match name {
            "end" => Symbol(0),
            "middle" => Symbol(1),
            "start" => Symbol(2),
            "loop" => Symbol(3),
            _ => Symbol(name.as_bytes()[0] as u32),
        };
        push_stmt(body, HirStmtKind::Label { name: sym, body: stmt })
    }

    fn goto_stmt(body: &mut HirBody, name: &str) -> HirStmtId {
        let sym = match name {
            "end" => Symbol(0),
            "middle" => Symbol(1),
            "start" => Symbol(2),
            "loop" => Symbol(3),
            _ => Symbol(name.as_bytes()[0] as u32),
        };
        push_stmt(body, HirStmtKind::Goto(sym))
    }

    /// `while (a) { b = 1; }` lowers to entry → header → body → exit.
    ///
    /// Expected layout:
    ///   bb0 (entry):  goto bb1 (header)
    ///   bb1 (header): switch a { 0 → exit, default → body }
    ///   bb2 (body):   b := 1; goto bb1 (back edge)
    ///   bb3 (exit):   (cursor)
    #[test]
    fn simple_while_loop() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let root = while_stmt(&mut hir_body, cond, body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 4, "while uses entry + header + body + exit");

        // bb0 (entry): goto header.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header = goto_target(entry);

        // bb1 (header): SwitchInt on a → exit or body.
        let header_bb = &body.blocks[header];
        assert_switch_discr_local(header_bb, map.lookup(ha));
        let (exit_block, body_block) = switch_zero_default(header_bb);

        // bb2 (body): b := 1; goto header (back edge).
        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hb), 1);
        assert_eq!(goto_target(body_bb), header, "body must back-edge to header");

        // bb3 (exit): cursor.
        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// `while (a) {}` — empty body must still form a valid back edge.
    #[test]
    fn while_loop_empty_body() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let empty_body = block_stmt(&mut hir_body, Vec::new());
        let root = while_stmt(&mut hir_body, cond, empty_body);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 4);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header = goto_target(entry);
        let header_bb = &body.blocks[header];
        let (exit_block, body_block) = switch_zero_default(header_bb);

        let body_bb = &body.blocks[body_block];
        assert!(body_bb.statements.is_empty(), "empty body must have no statements");
        assert_eq!(goto_target(body_bb), header, "empty body must back-edge to header");
        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// `do { b = 1; } while (a);` — body executes before condition.
    ///
    /// Expected layout:
    ///   bb0 (entry):  goto bb1 (body)
    ///   bb1 (body):   b := 1; goto bb2 (cond)
    ///   bb2 (cond):   switch a { 0 → exit, default → body (back edge) }
    ///   bb3 (exit):   (cursor)
    #[test]
    fn simple_do_while_loop() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let root = do_while_stmt(&mut hir_body, body_stmt, cond);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 4, "do-while uses entry + body + cond + exit");

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let body_block = goto_target(entry);

        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hb), 1);
        let cond_block = goto_target(body_bb);

        let cond_bb = &body.blocks[cond_block];
        assert_switch_discr_local(cond_bb, map.lookup(ha));
        let (exit_block, back_target) = switch_zero_default(cond_bb);
        assert_eq!(back_target, body_block, "cond true must loop back to body");

        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// `for (i = 0; i < 10; i = i + 1) { b = 1; }` — full for loop.
    ///
    /// Expected layout:
    ///   bb0 (entry + init): i := 0; goto header
    ///   bb1 (header):       switch (i < 10) { 0 → exit, default → body }
    ///   bb2 (body):         b := 1; goto step
    ///   bb3 (step):         i := i + 1; goto header (back edge)
    ///   bb4 (exit):         (cursor)
    #[test]
    fn for_loop_with_init_cond_step() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // init: i = 0
        let init_stmt = assign_local_stmt(&mut hir_body, int_ty, ha, 0);

        // cond: i < 10
        let i_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let ten = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(10));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::Lt, lhs: i_ref, rhs: ten },
        );

        // step: i = i + 1 (a simple assignment for testing)
        let step_lhs =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let step_rhs = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(1));
        let step_expr = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Assign { lhs: step_lhs, rhs: step_rhs },
        );

        // body: b = 1
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);

        let root = for_stmt(&mut hir_body, Some(init_stmt), Some(cond), Some(step_expr), body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // 5 blocks: entry+init, header, body, step, exit.
        assert_eq!(body.blocks.len(), 5, "for loop uses entry + header + body + step + exit");

        let entry = &body.blocks[crate::BasicBlockId(0)];
        // Entry should have init code (i := 0) and goto header.
        assert!(!entry.statements.is_empty(), "entry must contain init code");
        let header = goto_target(entry);

        // Header: SwitchInt on i < 10.
        let header_bb = &body.blocks[header];
        assert!(matches!(header_bb.terminator.kind, TerminatorKind::SwitchInt { .. }));
        let (exit_block, body_block) = switch_zero_default(header_bb);

        // Body: b := 1; goto step.
        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hb), 1);
        let step_block = goto_target(body_bb);

        // Step: has step code and back edge to header.
        let step_bb = &body.blocks[step_block];
        assert!(!step_bb.statements.is_empty(), "step block must contain step code");
        assert_eq!(goto_target(step_bb), header, "step must back-edge to header");

        // Exit.
        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// `for (;;) { b = 1; }` — infinite loop (no init, cond, or step).
    ///
    /// Expected layout:
    ///   bb0 (entry):  goto header
    ///   bb1 (header): goto body (no condition)
    ///   bb2 (body):   b := 1; goto step
    ///   bb3 (step):   goto header (back edge)
    ///   bb4 (exit):   (cursor — only reachable via break)
    #[test]
    fn for_infinite_loop() {
        let (mut builder, mut hir_body, tcx, map, [_ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let root = for_stmt(&mut hir_body, None, None, None, body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 5, "for(;;) uses entry + header + body + step + exit");

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header = goto_target(entry);

        // Header: unconditional goto body (no SwitchInt).
        let header_bb = &body.blocks[header];
        let body_block = goto_target(header_bb);
        assert!(
            matches!(header_bb.terminator.kind, TerminatorKind::Goto(_)),
            "infinite loop header must be unconditional Goto"
        );

        // Body: b := 1; goto step.
        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hb), 1);
        let step_block = goto_target(body_bb);

        // Step: goto header (back edge).
        let step_bb = &body.blocks[step_block];
        assert_eq!(goto_target(step_bb), header, "step must back-edge to header");
    }

    /// Nested loops: `while (a) { while (b) { body; } }`
    ///
    /// Verifies that the inner loop's exit flows into the outer loop's
    /// body continuation and the outer back edge is correct.
    #[test]
    fn nested_while_loops() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        // Inner: while (b) { c = 1; }
        let inner_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let inner_body = assign_local_stmt(&mut hir_body, int_ty, hc, 1);
        let inner_while = while_stmt(&mut hir_body, inner_cond, inner_body);

        // Outer: while (a) { <inner> }
        let outer_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let root = while_stmt(&mut hir_body, outer_cond, inner_while);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Blocks: entry + outer_header + outer_body(=inner) + inner_header + inner_body +
        //         inner_exit + outer_exit = 6 or 7 depending on structure.
        // entry → outer_header → (outer_exit | outer_body)
        // outer_body → inner_header → (inner_exit | inner_body)
        // inner_body → inner_header (back edge)
        // inner_exit → outer_header (back edge)
        // outer_exit → cursor
        assert!(body.blocks.len() >= 6, "nested loops must produce at least 6 blocks");

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let outer_header_id = goto_target(entry);
        let outer_header = &body.blocks[outer_header_id];
        assert_switch_discr_local(outer_header, map.lookup(ha));
        let (outer_exit, outer_body_id) = switch_zero_default(outer_header);

        let outer_body = &body.blocks[outer_body_id];
        // Outer body → inner header.
        let inner_header_id = goto_target(outer_body);
        let inner_header = &body.blocks[inner_header_id];
        assert_switch_discr_local(inner_header, map.lookup(hb));
        let (inner_exit, inner_body_id) = switch_zero_default(inner_header);

        // Inner body → inner header (back edge).
        let inner_body = &body.blocks[inner_body_id];
        assert_assign_const(inner_body, map.lookup(hc), 1);
        assert_eq!(goto_target(inner_body), inner_header_id, "inner body must loop back");

        // Inner exit → outer header (back edge).
        let inner_exit_bb = &body.blocks[inner_exit];
        assert_eq!(
            goto_target(inner_exit_bb),
            outer_header_id,
            "inner exit must loop back to outer header"
        );

        // Outer exit.
        assert!(body.blocks[outer_exit].statements.is_empty());
    }

    /// `while (a && b) { body; }` — loop condition with short-circuit &&.
    ///
    /// Verifies that the short-circuit diamond is nested inside the
    /// header and the loop structure remains correct.
    #[test]
    fn while_condition_short_circuit_and() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogAnd, lhs: a, rhs: b },
        );
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hc, 1);
        let root = while_stmt(&mut hir_body, cond, body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Blocks: entry + sc_init + sc_rhs + sc_join(=header with SwitchInt) + body + exit = 6
        assert_eq!(body.blocks.len(), 6, "while(a && b) must produce 6 blocks");

        // Entry → short-circuit init block.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let sc_init = goto_target(entry);

        // Short-circuit init: result := 0, SwitchInt on a.
        let sc_init_bb = &body.blocks[sc_init];
        let (sc_join, sc_rhs) = switch_zero_default(sc_init_bb);

        // SC rhs: result := b != 0, goto sc_join.
        let sc_rhs_bb = &body.blocks[sc_rhs];
        assert_eq!(goto_target(sc_rhs_bb), sc_join);

        // SC join (= inner condition): SwitchInt on result → exit or body.
        let sc_join_bb = &body.blocks[sc_join];
        assert!(matches!(sc_join_bb.terminator.kind, TerminatorKind::SwitchInt { .. }));
        let (exit_block, body_block) = switch_zero_default(sc_join_bb);

        // Body: c := 1; back edge to sc_init (= the while header).
        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hc), 1);
        assert_eq!(goto_target(body_bb), sc_init, "body must back-edge to the while header");

        // Exit.
        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// `while (a || b) { body; }` — loop condition with short-circuit ||.
    #[test]
    fn while_condition_short_circuit_or() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let a = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let b = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let cond = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::LogOr, lhs: a, rhs: b },
        );
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hc, 1);
        let root = while_stmt(&mut hir_body, cond, body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert_eq!(body.blocks.len(), 6, "while(a || b) must produce 6 blocks");

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let sc_init = goto_target(entry);
        let sc_init_bb = &body.blocks[sc_init];
        // For ||: 0 → rhs, default → join.
        let (sc_rhs, sc_join) = switch_zero_default(sc_init_bb);

        let sc_rhs_bb = &body.blocks[sc_rhs];
        assert_eq!(goto_target(sc_rhs_bb), sc_join);

        let sc_join_bb = &body.blocks[sc_join];
        let (exit_block, body_block) = switch_zero_default(sc_join_bb);

        let body_bb = &body.blocks[body_block];
        assert_assign_const(body_bb, map.lookup(hc), 1);
        assert_eq!(goto_target(body_bb), sc_init, "body must back-edge to the while header");

        assert!(body.blocks[exit_block].statements.is_empty());
    }

    /// Back edge structure: verify that every loop body's terminator
    /// points back to the header (not to itself or to the exit).
    #[test]
    fn back_edge_points_to_header() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 42);
        let root = while_stmt(&mut hir_body, cond, body_stmt);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (_exit, body_id) = switch_zero_default(header_bb);

        let body_bb = &body.blocks[body_id];
        let back_target = goto_target(body_bb);
        assert_eq!(back_target, header_id, "back edge must target the header");
        assert_ne!(back_target, body_id, "back edge must not target itself");
    }

    /// Loop exit block is the join point after the loop.
    #[test]
    fn loop_exit_block_is_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let body_stmt = assign_local_stmt(&mut hir_body, int_ty, hb, 1);
        let while_s = while_stmt(&mut hir_body, cond, body_stmt);

        // After the loop, assign b = 2 in the exit block.
        let after_assign = assign_local_stmt(&mut hir_body, int_ty, hb, 2);
        let root = block_stmt(&mut hir_body, vec![while_s, after_assign]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // The exit block must contain the post-loop assignment.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (exit_block, _body_block) = switch_zero_default(header_bb);

        let exit_bb = &body.blocks[exit_block];
        assert_assign_const(exit_bb, map.lookup(hb), 2);
    }

    /// `break` in a while loop targets the exit block.
    #[test]
    fn break_targets_exit_block() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let brk = break_stmt(&mut hir_body);
        let root = while_stmt(&mut hir_body, cond, brk);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (exit_block, body_block) = switch_zero_default(header_bb);

        // Body block: break → exit.
        let body_bb = &body.blocks[body_block];
        assert_eq!(goto_target(body_bb), exit_block, "break must target exit");
    }

    /// `continue` in a while loop targets the header block.
    #[test]
    fn continue_targets_header_in_while() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cont = continue_stmt(&mut hir_body);
        let root = while_stmt(&mut hir_body, cond, cont);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (_exit, body_block) = switch_zero_default(header_bb);

        let body_bb = &body.blocks[body_block];
        assert_eq!(goto_target(body_bb), header_id, "continue must target header in while");
    }

    /// `continue` in a for loop targets the step block.
    #[test]
    fn continue_targets_step_in_for() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let step_expr =
            push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(1));
        let cont = continue_stmt(&mut hir_body);
        let root = for_stmt(&mut hir_body, None, Some(cond), Some(step_expr), cont);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (exit_block, body_block) = switch_zero_default(header_bb);

        let body_bb = &body.blocks[body_block];
        let step_id = goto_target(body_bb);

        // Continue from body must target the step block.
        // But in our implementation, body → step via goto (always).
        // The continue_stmt is a bare stmt, so the body block has:
        //   goto step (from continue)
        // and we need to verify the step → header back edge.
        let step_bb = &body.blocks[step_id];
        assert_eq!(goto_target(step_bb), header_id, "step must back-edge to header");

        // Also verify break in for targets exit.
        // We already tested continue; let's also verify body → step path.
        // For a for loop body with only `continue`, the body block's
        // terminator should be goto step.
        assert_eq!(
            step_id,
            goto_target(body_bb),
            "for-loop body with continue must goto step block"
        );

        let _ = exit_block;
    }

    /// `break` in a for loop targets the exit block.
    #[test]
    fn break_targets_exit_in_for() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let brk = break_stmt(&mut hir_body);
        let root = for_stmt(&mut hir_body, None, Some(cond), None, brk);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (exit_block, body_block) = switch_zero_default(header_bb);

        let body_bb = &body.blocks[body_block];
        assert_eq!(goto_target(body_bb), exit_block, "break in for must target exit");
    }

    /// Nested loop break/continue: break targets the inner exit, not outer.
    #[test]
    fn nested_loop_break_targets_inner_exit() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        // Inner: while (b) { break; }
        let inner_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));
        let brk = break_stmt(&mut hir_body);
        let inner_while = while_stmt(&mut hir_body, inner_cond, brk);

        // Outer: while (a) { <inner>; c = 2; }
        let outer_cond =
            push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let after_inner = assign_local_stmt(&mut hir_body, int_ty, hc, 2);
        let outer_body = block_stmt(&mut hir_body, vec![inner_while, after_inner]);
        let root = while_stmt(&mut hir_body, outer_cond, outer_body);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Find blocks.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let outer_header_id = goto_target(entry);
        let outer_header = &body.blocks[outer_header_id];
        let (outer_exit, outer_body_id) = switch_zero_default(outer_header);

        // Outer body → inner header.
        let outer_body_bb = &body.blocks[outer_body_id];
        let inner_header_id = goto_target(outer_body_bb);
        let inner_header = &body.blocks[inner_header_id];
        let (inner_exit, inner_body_id) = switch_zero_default(inner_header);

        // Inner body: break → inner_exit (not outer_exit).
        let inner_body_bb = &body.blocks[inner_body_id];
        assert_eq!(
            goto_target(inner_body_bb),
            inner_exit,
            "break in inner loop must target inner exit, not outer exit"
        );
        assert_ne!(inner_exit, outer_exit, "inner and outer exit must be different blocks");
    }

    // ── Task 08-08: switch lowering ─────────────────────────────────────

    /// Helper: push a `Case { value, body }` statement.
    fn case_stmt(body: &mut HirBody, value: i128, case_body: HirStmtId) -> HirStmtId {
        push_stmt(body, HirStmtKind::Case { value: Some(value), range_end: None, body: case_body })
    }

    /// Helper: push a `Default { body }` statement.
    fn default_stmt(body: &mut HirBody, default_body: HirStmtId) -> HirStmtId {
        push_stmt(body, HirStmtKind::Default { body: default_body })
    }

    /// Helper: push a `Switch { cond, body, cases }` statement.
    fn switch_stmt(
        body: &mut HirBody,
        cond: HirExprId,
        switch_body: HirStmtId,
        cases: Vec<rcc_hir::SwitchCase>,
    ) -> HirStmtId {
        push_stmt(body, HirStmtKind::Switch { cond, body: switch_body, cases })
    }

    /// Helper: extract SwitchInt targets as a Vec<(Option<i128>, BasicBlockId)>.
    fn switch_targets(block: &crate::BasicBlock) -> &[(Option<i128>, crate::BasicBlockId)] {
        match &block.terminator.kind {
            TerminatorKind::SwitchInt { targets, .. } => targets,
            other => panic!("expected SwitchInt, got {other:?}"),
        }
    }

    /// `switch (x) { case 1: a=10; break; case 2: b=20; default: c=30; }`
    ///
    /// Expected layout:
    ///   bb0 (entry):    goto bb1 (dispatch)
    ///   bb1 (dispatch): switch x { 1→bb2, 2→bb3, None→bb4 }
    ///   bb2 (case 1):   a=10; goto bb5 (join, via break)
    ///   bb3 (case 2):   b=20; goto bb4 (fallthrough to default)
    ///   bb4 (default):  c=30; goto bb5 (fallthrough to join)
    ///   bb5 (join):     (cursor)
    #[test]
    fn switch_basic_with_default() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        // case 1: a = 10; break;
        let case1_body = assign_local_stmt(&mut hir_body, int_ty, ha, 10);
        let brk = break_stmt(&mut hir_body);
        let case1_inner = block_stmt(&mut hir_body, vec![case1_body, brk]);
        let case1 = case_stmt(&mut hir_body, 1, case1_inner);

        // case 2: b = 20;
        let case2_body = assign_local_stmt(&mut hir_body, int_ty, hb, 20);
        let case2 = case_stmt(&mut hir_body, 2, case2_body);

        // default: c = 30;
        let def_body = assign_local_stmt(&mut hir_body, int_ty, hc, 30);
        let def = default_stmt(&mut hir_body, def_body);

        let switch_body = block_stmt(&mut hir_body, vec![case1, case2, def]);
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(1), target: case1 },
            rcc_hir::SwitchCase { value: Some(2), target: case2 },
            rcc_hir::SwitchCase { value: None, target: def },
        ];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // bb0 (entry) → dispatch.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);

        // bb1 (dispatch): SwitchInt with 3 targets.
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);
        assert_eq!(tgts.len(), 3, "dispatch must have 3 targets");
        assert_eq!(tgts[0], (Some(1), crate::BasicBlockId(2)), "target[0] = case 1");
        assert_eq!(tgts[1], (Some(2), crate::BasicBlockId(3)), "target[1] = case 2");
        assert_eq!(tgts[2], (None, crate::BasicBlockId(4)), "target[2] = default");

        // bb2 (case 1): a=10; break → join.
        let case1_bb = &body.blocks[crate::BasicBlockId(2)];
        assert_assign_const(case1_bb, map.lookup(ha), 10);
        let join_id = goto_target(case1_bb);

        // bb3 (case 2): b=20; fallthrough → default.
        let case2_bb = &body.blocks[crate::BasicBlockId(3)];
        assert_assign_const(case2_bb, map.lookup(hb), 20);
        assert_eq!(
            goto_target(case2_bb),
            crate::BasicBlockId(4),
            "case 2 must fallthrough to default"
        );

        // bb4 (default): c=30; fallthrough → join.
        let default_bb = &body.blocks[crate::BasicBlockId(4)];
        assert_assign_const(default_bb, map.lookup(hc), 30);
        assert_eq!(goto_target(default_bb), join_id, "default must fallthrough to join");

        // bb5 (join): empty, cursor lands here.
        assert!(body.blocks[join_id].statements.is_empty());
    }

    /// Fallthrough: `case 1: case 2: a=5; break;` — case 1 falls through
    /// to case 2's body.
    #[test]
    fn switch_case_fallthrough() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        // case 1: (empty body — falls through)
        let case1_inner = push_stmt(&mut hir_body, HirStmtKind::Null);
        let case1 = case_stmt(&mut hir_body, 1, case1_inner);

        // case 2: a=5; break;
        let case2_body = assign_local_stmt(&mut hir_body, int_ty, ha, 5);
        let brk = break_stmt(&mut hir_body);
        let case2_inner = block_stmt(&mut hir_body, vec![case2_body, brk]);
        let case2 = case_stmt(&mut hir_body, 2, case2_inner);

        let switch_body = block_stmt(&mut hir_body, vec![case1, case2]);
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(1), target: case1 },
            rcc_hir::SwitchCase { value: Some(2), target: case2 },
        ];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);
        assert_eq!(tgts.len(), 3, "dispatch must have 3 targets (1, 2, default→join)");

        // case 1 block: empty body, falls through to case 2.
        let case1_bb = &body.blocks[tgts[0].1];
        assert!(case1_bb.statements.is_empty(), "case 1 body is empty");
        assert_eq!(goto_target(case1_bb), tgts[1].1, "case 1 must fallthrough to case 2");

        // case 2 block: a=5; break → join.
        let case2_bb = &body.blocks[tgts[1].1];
        assert_assign_const(case2_bb, map.lookup(ha), 5);
        let join_id = goto_target(case2_bb);
        assert_eq!(join_id, crate::BasicBlockId(4), "break must target join");
    }

    /// Real C parse shape: `case 1: stmt; break; case 2: ...` leaves the
    /// `break` as a sibling statement after the `Case` node, not inside the
    /// case label body. Switch lowering must therefore lower the whole switch
    /// body in source order, treating case/default as labels.
    #[test]
    fn switch_sibling_break_stays_in_case_block() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        let case1_body = assign_local_stmt(&mut hir_body, int_ty, ha, 10);
        let case1 = case_stmt(&mut hir_body, 1, case1_body);
        let break1 = break_stmt(&mut hir_body);

        let case2_body = assign_local_stmt(&mut hir_body, int_ty, hb, 20);
        let case2 = case_stmt(&mut hir_body, 2, case2_body);
        let break2 = break_stmt(&mut hir_body);

        let default_body = assign_local_stmt(&mut hir_body, int_ty, hc, 30);
        let def = default_stmt(&mut hir_body, default_body);
        let break3 = break_stmt(&mut hir_body);

        let switch_body =
            block_stmt(&mut hir_body, vec![case1, break1, case2, break2, def, break3]);
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(1), target: case1 },
            rcc_hir::SwitchCase { value: Some(2), target: case2 },
            rcc_hir::SwitchCase { value: None, target: def },
        ];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch = &body.blocks[goto_target(entry)];
        let tgts = switch_targets(dispatch);
        let case1_bb = &body.blocks[tgts[0].1];
        let case2_bb = &body.blocks[tgts[1].1];
        let default_bb = &body.blocks[tgts[2].1];

        assert_assign_const(case1_bb, map.lookup(ha), 10);
        assert_assign_const(case2_bb, map.lookup(hb), 20);
        assert_assign_const(default_bb, map.lookup(hc), 30);

        let join = goto_target(default_bb);
        assert_eq!(goto_target(case1_bb), join, "case 1 break must not fall through");
        assert_eq!(goto_target(case2_bb), join, "case 2 break must not fall through");
    }

    /// Nested switch: inner `break` targets inner join, not outer join.
    #[test]
    fn switch_nested_inner_break_targets_inner_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;

        // Outer switch discriminant.
        let outer_x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        // Inner switch discriminant.
        let inner_x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hb));

        // Inner case 1: c=99; break; (break targets inner join)
        let inner_case_body = assign_local_stmt(&mut hir_body, int_ty, hc, 99);
        let inner_brk = break_stmt(&mut hir_body);
        let inner_case_inner = block_stmt(&mut hir_body, vec![inner_case_body, inner_brk]);
        let inner_case = case_stmt(&mut hir_body, 1, inner_case_inner);

        // Inner switch body.
        let inner_body = block_stmt(&mut hir_body, vec![inner_case]);
        let inner_cases = vec![rcc_hir::SwitchCase { value: Some(1), target: inner_case }];
        let inner_switch = switch_stmt(&mut hir_body, inner_x, inner_body, inner_cases);

        // Outer case 1: contains the inner switch.
        let outer_case = case_stmt(&mut hir_body, 1, inner_switch);

        // Outer switch body.
        let outer_body = block_stmt(&mut hir_body, vec![outer_case]);
        let outer_cases = vec![rcc_hir::SwitchCase { value: Some(1), target: outer_case }];
        let root = switch_stmt(&mut hir_body, outer_x, outer_body, outer_cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Outer dispatch.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let outer_dispatch_id = goto_target(entry);
        let outer_dispatch_bb = &body.blocks[outer_dispatch_id];
        let outer_tgts = switch_targets(outer_dispatch_bb);
        assert_eq!(outer_tgts.len(), 2, "outer dispatch: 1 target + default→join");

        // Outer case 1 block → inner dispatch.
        let outer_case1_bb = &body.blocks[outer_tgts[0].1];
        let inner_dispatch_id = goto_target(outer_case1_bb);
        let inner_dispatch_bb = &body.blocks[inner_dispatch_id];
        let inner_tgts = switch_targets(inner_dispatch_bb);
        assert_eq!(inner_tgts.len(), 2, "inner dispatch: 1 target + default→join");

        // Inner case 1: c=99; break → inner join (not outer join).
        let inner_case1_bb = &body.blocks[inner_tgts[0].1];
        assert_assign_const(inner_case1_bb, map.lookup(hc), 99);
        let inner_join_id = goto_target(inner_case1_bb);

        // Inner join is NOT the outer join.
        let outer_join_id = outer_tgts[1].1; // default → outer join
        assert_ne!(
            inner_join_id, outer_join_id,
            "inner break must target inner join, not outer join"
        );
    }

    /// Default-only switch: `switch (x) { default: a=1; }`
    #[test]
    fn switch_default_only() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        let def_body = assign_local_stmt(&mut hir_body, int_ty, ha, 1);
        let def = default_stmt(&mut hir_body, def_body);

        let switch_body = block_stmt(&mut hir_body, vec![def]);
        let cases = vec![rcc_hir::SwitchCase { value: None, target: def }];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);

        // Default-only: dispatch has 1 target (None → default block).
        assert_eq!(tgts.len(), 1, "default-only switch should have 1 target");
        assert_eq!(tgts[0].0, None, "target should be default");

        // Default block: a=1; fallthrough to join.
        let default_bb = &body.blocks[tgts[0].1];
        assert_assign_const(default_bb, map.lookup(ha), 1);
        let join_id = goto_target(default_bb);
        assert!(body.blocks[join_id].statements.is_empty());
    }

    /// No default: `switch (x) { case 1: a=10; }` — unmatched → join.
    #[test]
    fn switch_no_default_unmatched_goes_to_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        let case1_body = assign_local_stmt(&mut hir_body, int_ty, ha, 10);
        let case1 = case_stmt(&mut hir_body, 1, case1_body);

        let switch_body = block_stmt(&mut hir_body, vec![case1]);
        let cases = vec![rcc_hir::SwitchCase { value: Some(1), target: case1 }];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);

        // 1 explicit case + auto-added default → join.
        assert_eq!(tgts.len(), 2, "dispatch must have 2 targets");
        assert_eq!(tgts[0].0, Some(1), "target[0] = case 1");
        assert_eq!(tgts[1].0, None, "target[1] = default (→join)");

        // Case 1 block: a=10; fallthrough to join.
        let case1_bb = &body.blocks[tgts[0].1];
        assert_assign_const(case1_bb, map.lookup(ha), 10);
        let join_id = goto_target(case1_bb);

        // Default target is the join block.
        assert_eq!(tgts[1].1, join_id, "default target must be the join block");
    }

    /// `break` inside a switch targets the switch join, not a loop exit.
    #[test]
    fn break_in_switch_targets_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        // case 1: b=42; break;
        let case_body = assign_local_stmt(&mut hir_body, int_ty, hb, 42);
        let brk = break_stmt(&mut hir_body);
        let case_inner = block_stmt(&mut hir_body, vec![case_body, brk]);
        let case1 = case_stmt(&mut hir_body, 1, case_inner);

        let switch_body = block_stmt(&mut hir_body, vec![case1]);
        let cases = vec![rcc_hir::SwitchCase { value: Some(1), target: case1 }];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);

        // case 1 block: b=42; break → join.
        let case1_bb = &body.blocks[tgts[0].1];
        assert_assign_const(case1_bb, map.lookup(hb), 42);
        let break_target = goto_target(case1_bb);
        // break_target must be the join block (default target in dispatch).
        assert_eq!(break_target, tgts[1].1, "break in switch must target the join block");
    }

    /// Regression: `default:` in the middle of the case list.
    /// `switch (x) { case 1: ; default: a=1; case 2: b=2; }`
    ///
    /// targets must be `[(Some(1), c1), (Some(2), c2), (None, def)]`
    /// — default is always the last entry.
    /// fallthrough: c1 → def → c2 → join.
    #[test]
    fn switch_default_in_middle_is_last_target() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let x = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        // case 1: (empty)
        let case1_inner = push_stmt(&mut hir_body, HirStmtKind::Null);
        let case1 = case_stmt(&mut hir_body, 1, case1_inner);

        // default: a = 1;
        let def_body = assign_local_stmt(&mut hir_body, int_ty, ha, 1);
        let def = default_stmt(&mut hir_body, def_body);

        // case 2: b = 2;
        let case2_body = assign_local_stmt(&mut hir_body, int_ty, hb, 2);
        let case2 = case_stmt(&mut hir_body, 2, case2_body);

        // Source order: case 1, default, case 2.
        let switch_body = block_stmt(&mut hir_body, vec![case1, def, case2]);
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(1), target: case1 },
            rcc_hir::SwitchCase { value: None, target: def },
            rcc_hir::SwitchCase { value: Some(2), target: case2 },
        ];
        let root = switch_stmt(&mut hir_body, x, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);

        // default must be last: [(1, c1), (2, c2), (None, def)].
        assert_eq!(tgts.len(), 3, "dispatch must have 3 targets");
        assert_eq!(tgts[0].0, Some(1), "target[0] = case 1");
        assert_eq!(tgts[1].0, Some(2), "target[1] = case 2");
        assert_eq!(tgts[2].0, None, "target[2] = default (must be last)");

        // Fallthrough order follows source: case 1 → default → case 2 → join.
        let case1_bb = &body.blocks[tgts[0].1];
        assert!(case1_bb.statements.is_empty(), "case 1 body is empty");
        let def_block_id = tgts[2].1;
        assert_eq!(goto_target(case1_bb), def_block_id, "case 1 must fallthrough to default");

        let def_bb = &body.blocks[def_block_id];
        assert_assign_const(def_bb, map.lookup(ha), 1);
        let case2_block_id = tgts[1].1;
        assert_eq!(goto_target(def_bb), case2_block_id, "default must fallthrough to case 2");

        let case2_bb = &body.blocks[case2_block_id];
        assert_assign_const(case2_bb, map.lookup(hb), 2);
        let join_id = goto_target(case2_bb);
        assert!(body.blocks[join_id].statements.is_empty(), "join must be empty");
    }

    /// `break` inside a switch that is inside a loop must target the
    /// switch join, NOT the loop exit.
    #[test]
    fn break_in_switch_inside_loop_targets_switch_join() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // Loop condition: a != 0 (arbitrary).
        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        // switch (a) { case 1: b=42; break; }
        let discr = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let case_body = assign_local_stmt(&mut hir_body, int_ty, hb, 42);
        let brk = break_stmt(&mut hir_body);
        let case_inner = block_stmt(&mut hir_body, vec![case_body, brk]);
        let case1 = case_stmt(&mut hir_body, 1, case_inner);

        let switch_body = block_stmt(&mut hir_body, vec![case1]);
        let cases = vec![rcc_hir::SwitchCase { value: Some(1), target: case1 }];
        let switch_stmt = switch_stmt(&mut hir_body, discr, switch_body, cases);

        // while (a) { switch(a) { case 1: b=42; break; } }
        let loop_body = block_stmt(&mut hir_body, vec![switch_stmt]);
        let root = while_stmt(&mut hir_body, cond, loop_body);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Expected blocks:
        // bb0 entry → bb1 header
        // bb1 header: switch a { 0→bb2 exit, default→bb3 body }
        // bb3 body: switch(a) dispatch → ...
        //   dispatch: switch a { 1→bb4 case1, default→bb5 switch_join }
        // bb4 case1: b=42; break → bb5 (switch join, NOT bb2 loop exit)
        // bb5 switch_join: goto bb1 (back edge to loop header)
        // bb2 exit: (cursor)

        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (loop_exit, loop_body_id) = switch_zero_default(header_bb);

        // Loop body contains the switch.
        let loop_body_bb = &body.blocks[loop_body_id];
        let dispatch_id = goto_target(loop_body_bb);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);
        assert_eq!(tgts.len(), 2, "dispatch: 1 case + default→join");

        // case 1 block: b=42; break.
        let case1_bb = &body.blocks[tgts[0].1];
        assert_assign_const(case1_bb, map.lookup(hb), 42);
        let break_target = goto_target(case1_bb);

        // break_target must be the SWITCH join (tgts[1].1), not the loop exit.
        let switch_join_id = tgts[1].1;
        assert_eq!(
            break_target, switch_join_id,
            "break inside switch must target switch join, not loop exit"
        );
        assert_ne!(break_target, loop_exit, "break inside switch must NOT target loop exit");

        // Switch join falls through back to loop header.
        let switch_join_bb = &body.blocks[switch_join_id];
        assert_eq!(
            goto_target(switch_join_bb),
            header_id,
            "switch join must back-edge to loop header"
        );
    }

    // ------------------------------------------------------------------
    // Goto / label lowering (task 08-09)
    // ------------------------------------------------------------------

    /// `goto end; end: return;`
    /// Forward goto: the target block is created by the pre-pass.
    #[test]
    fn forward_goto() {
        let (mut builder, mut hir_body, tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let end_label = label_stmt(&mut hir_body, "end", ret);
        let g = goto_stmt(&mut hir_body, "end");
        let root = block_stmt(&mut hir_body, vec![g, end_label]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // bb0 entry → bb1 (goto target)
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let goto_target_id = goto_target(entry);
        // goto target should be the label block.
        let label_bb = &body.blocks[goto_target_id];
        // label block contains the return.
        assert!(
            matches!(&label_bb.terminator.kind, TerminatorKind::Return),
            "label block must terminate with Return"
        );
    }

    /// `start: a = 1; goto middle; middle: b = 2; goto end; end: return;`
    #[test]
    fn multiple_labels_and_gotos() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let end_label = label_stmt(&mut hir_body, "end", ret);
        let g2 = goto_stmt(&mut hir_body, "end");
        let mid_body = assign_local_stmt(&mut hir_body, int_ty, hb, 2);
        let mid_label = label_stmt(&mut hir_body, "middle", mid_body);
        let g1 = goto_stmt(&mut hir_body, "middle");
        let start_body = assign_local_stmt(&mut hir_body, int_ty, ha, 1);
        let start_label = label_stmt(&mut hir_body, "start", start_body);
        let root = block_stmt(&mut hir_body, vec![start_label, g1, mid_label, g2, end_label]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // Entry goto start label.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let start_id = goto_target(entry);
        let start_bb = &body.blocks[start_id];
        assert_assign_const(start_bb, map.lookup(ha), 1);

        // start block's terminator is goto middle (g1).
        let middle_id = goto_target(start_bb);
        let middle_bb = &body.blocks[middle_id];
        assert_assign_const(middle_bb, map.lookup(hb), 2);

        // middle block's terminator is goto end (g2).
        let end_id = goto_target(middle_bb);
        let end_bb = &body.blocks[end_id];
        assert!(
            matches!(&end_bb.terminator.kind, TerminatorKind::Return),
            "end block must be Return"
        );
    }

    /// Duff's device (goto crossing switch labels).
    ///
    /// Simplified version that exercises switch + do-while interleaving.
    /// The key invariant is that the outer pre-pass creates blocks for
    /// any `Label` nested inside the switch body before `lower_switch`
    /// runs.
    #[test]
    fn duffs_device_fixture() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // do-while body: a = 0; a = 7; a = 6;
        let s6 = assign_local_stmt(&mut hir_body, int_ty, ha, 6);
        let s7 = assign_local_stmt(&mut hir_body, int_ty, ha, 7);
        let s0 = assign_local_stmt(&mut hir_body, int_ty, ha, 0);
        let loop_body = block_stmt(&mut hir_body, vec![s0, s7, s6]);

        // do { ... } while (a)
        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let do_while = do_while_stmt(&mut hir_body, loop_body, cond);

        // switch (a) { case 0: do_while; case 7: ...; case 6: ...; }
        let discr = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));

        let case6 = case_stmt(&mut hir_body, 6, s6);
        let case7 = case_stmt(&mut hir_body, 7, s7);
        let case0 = case_stmt(&mut hir_body, 0, do_while);

        let switch_body = block_stmt(&mut hir_body, vec![case0, case7, case6]);
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(0), target: case0 },
            rcc_hir::SwitchCase { value: Some(7), target: case7 },
            rcc_hir::SwitchCase { value: Some(6), target: case6 },
        ];
        let switch = switch_stmt(&mut hir_body, discr, switch_body, cases);

        let root = block_stmt(&mut hir_body, vec![switch]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let _body = finish(builder);

        // Main assertion: compiles without panic.  Switch/do-while
        // shapes are validated by their own task tests.
    }

    /// `goto L` inside a switch case body where `L` is a label inside
    /// a *different* case body.  This exercises `collect_labels`
    /// recursing into `Case`/`Default` bodies.
    #[test]
    fn switch_case_body_with_label_and_goto() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // label L inside case 0 body
        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let lbl = label_stmt(&mut hir_body, "L", ret);
        let case0_body = block_stmt(&mut hir_body, vec![lbl]);
        let case0 = case_stmt(&mut hir_body, 0, case0_body);

        // goto L inside case 1 body (forward goto)
        let g = goto_stmt(&mut hir_body, "L");
        let case1_body = block_stmt(&mut hir_body, vec![g]);
        let case1 = case_stmt(&mut hir_body, 1, case1_body);

        let switch_body = block_stmt(&mut hir_body, vec![case0, case1]);
        let discr = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cases = vec![
            rcc_hir::SwitchCase { value: Some(0), target: case0 },
            rcc_hir::SwitchCase { value: Some(1), target: case1 },
        ];
        let root = switch_stmt(&mut hir_body, discr, switch_body, cases);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // dispatch → case 1 → goto L → label L → return
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let dispatch_id = goto_target(entry);
        let dispatch_bb = &body.blocks[dispatch_id];
        let tgts = switch_targets(dispatch_bb);

        let case1_bb = &body.blocks[tgts[1].1];
        let goto_target_id = goto_target(case1_bb);

        let label_bb = &body.blocks[goto_target_id];
        assert!(
            matches!(&label_bb.terminator.kind, TerminatorKind::Return),
            "label block must terminate with Return"
        );
    }

    /// `goto L; { L: return; }` — label nested inside a Block that
    /// follows a goto.  The label body must still be lowered even
    /// though the Block itself is in dead code.
    #[test]
    fn goto_then_block_with_nested_label() {
        let (mut builder, mut hir_body, tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let lbl = label_stmt(&mut hir_body, "L", ret);
        let inner_block = block_stmt(&mut hir_body, vec![lbl]);
        let g = goto_stmt(&mut hir_body, "L");
        let root = block_stmt(&mut hir_body, vec![g, inner_block]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // bb0 entry → bb1 (goto target = label block)
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let label_id = goto_target(entry);
        let label_bb = &body.blocks[label_id];
        assert!(
            matches!(&label_bb.terminator.kind, TerminatorKind::Return),
            "label block must terminate with Return"
        );
    }

    /// `goto L; { L: a=1; a=2; return; }` — label inside a dead block
    /// with fallthrough statements after the label.  The label body
    /// and its fallthrough must all be lowered, not just the label.
    #[test]
    fn goto_then_block_with_label_and_fallthrough() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let a2 = assign_local_stmt(&mut hir_body, int_ty, hb, 2);
        let a1 = assign_local_stmt(&mut hir_body, int_ty, ha, 1);
        let lbl = label_stmt(&mut hir_body, "L", a1);
        let inner_block = block_stmt(&mut hir_body, vec![lbl, a2, ret]);
        let g = goto_stmt(&mut hir_body, "L");
        let root = block_stmt(&mut hir_body, vec![g, inner_block]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        // bb0 entry → bb1 (goto target = label block)
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let label_id = goto_target(entry);
        let label_bb = &body.blocks[label_id];

        // label block must contain a=1, a=2, ret slot, then terminate with Return.
        assert_eq!(label_bb.statements.len(), 3, "label block must have 3 statements");
        // Check first statement is a=1.
        match &label_bb.statements[0].kind {
            StatementKind::Assign {
                place: Place { base, .. },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, map.lookup(ha));
                assert_eq!(*v, 1);
            }
            other => panic!("expected `ha = 1`, got {other:?}"),
        }
        // Check second statement is a=2.
        match &label_bb.statements[1].kind {
            StatementKind::Assign {
                place: Place { base, .. },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, map.lookup(hb));
                assert_eq!(*v, 2);
            }
            other => panic!("expected `hb = 2`, got {other:?}"),
        }
        assert!(
            matches!(&label_bb.terminator.kind, TerminatorKind::Return),
            "label block must terminate with Return"
        );
    }

    #[test]
    fn goto_into_dead_block_with_ordinary_local_emits_storage_live_at_label() {
        let (mut builder, mut hir_body, tcx, map, [_ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let decl = local_decl_stmt(&mut hir_body, hb);
        let assign = assign_local_stmt(&mut hir_body, int_ty, hb, 1234);
        let lbl = label_stmt(&mut hir_body, "L", assign);
        let inner_block = block_stmt(&mut hir_body, vec![decl, lbl]);
        let g = goto_stmt(&mut hir_body, "L");
        let root = block_stmt(&mut hir_body, vec![g, inner_block]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        builder.collect_labels(&hir_body, root, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let label_id = goto_target(&body.blocks[crate::BasicBlockId(0)]);
        let label_bb = &body.blocks[label_id];
        let cfg_b = map.lookup(hb);
        assert_storage_live(&label_bb.statements[0], cfg_b);
        match &label_bb.statements[1].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(v), .. })),
            } => {
                assert_eq!(*base, cfg_b);
                assert!(projection.is_empty());
                assert_eq!(*v, 1234);
            }
            other => panic!("expected assignment to ordinary block local, got {other:?}"),
        }
        assert!(
            label_bb.statements.iter().any(
                |stmt| matches!(stmt.kind, StatementKind::StorageDead(local) if local == cfg_b)
            ),
            "label block should close the ordinary local scope"
        );
    }

    // ------------------------------------------------------------------
    // Call lowering helpers (task 08-10)
    // ------------------------------------------------------------------

    /// Create a `DefRef` expression pointing at an arbitrary [`DefId`].
    fn def_ref_expr(body: &mut HirBody, ty: TyId, def: rcc_hir::DefId) -> HirExprId {
        push_expr(body, ty, ValueCat::RValue, HirExprKind::DefRef(def))
    }

    /// Create a `Call` expression.
    fn call_expr(
        body: &mut HirBody,
        ret_ty: TyId,
        callee: HirExprId,
        args: Vec<HirExprId>,
    ) -> HirExprId {
        push_expr(body, ret_ty, ValueCat::RValue, HirExprKind::Call { callee, args })
    }

    /// Non-void call: `f(1)` returns a value into a fresh temporary.
    #[test]
    fn call_non_void_basic() {
        let (mut builder, mut hir_body, mut tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // callee: arbitrary DefId (say a function f) with a function type
        let func_ty = tcx.intern(rcc_hir::Ty::Func {
            ret: int_ty,
            params: Vec::new(),
            variadic: false,
            proto: true,
        });
        let func_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(func_ty)));
        let callee_def = rcc_hir::DefId::new(42);
        let callee = def_ref_expr(&mut hir_body, func_ptr_ty, callee_def);

        // arg: literal 1
        let arg = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(1));

        // f(1)
        let call = call_expr(&mut hir_body, int_ty, callee, vec![arg]);

        // Lower as rvalue — must produce an Operand without panicking.
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, call);

        // The result should be Copy(dest) where dest is a new temporary.
        match result {
            Operand::Copy(Place { .. }) => {
                // Finish the builder to get the Body.
                let body = finish(builder);
                // Find the block that has the Call terminator.
                let has_call = body
                    .blocks
                    .iter()
                    .any(|bb| matches!(&bb.terminator.kind, TerminatorKind::Call { .. }));
                assert!(has_call, "body must contain a Call terminator");
            }
            other => panic!("expected Copy(Place), got {other:?}"),
        }
    }

    /// Void call: `g(42)` where g returns void — destination is None.
    #[test]
    fn call_void_basic() {
        let (mut builder, mut hir_body, mut tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        let func_ty = tcx.intern(rcc_hir::Ty::Func {
            ret: tcx.void,
            params: Vec::new(),
            variadic: false,
            proto: true,
        });
        let func_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(func_ty)));
        let callee_def = rcc_hir::DefId::new(99);
        let callee = def_ref_expr(&mut hir_body, func_ptr_ty, callee_def);

        let arg = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(42));
        let call = call_expr(&mut hir_body, tcx.void, callee, vec![arg]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, call);

        // Void calls return a dummy Int(0) operand.
        match result {
            Operand::Const(Const { kind: ConstKind::Int(0), .. }) => {}
            other => panic!("expected dummy Int(0), got {other:?}"),
        }

        let body = finish(builder);
        // The Call terminator must have destination = None.
        let call_term = body.blocks.iter().find_map(|bb| match &bb.terminator.kind {
            TerminatorKind::Call { destination, .. } => Some(destination),
            _ => None,
        });
        assert!(call_term.is_some(), "body must contain a Call terminator");
        assert!(call_term.unwrap().is_none(), "void call must have None destination");
    }

    /// Variadic call: `printf(fmt, x)` — args passed through as-is.
    #[test]
    fn call_variadic() {
        let (mut builder, mut hir_body, mut tcx, map, [_ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // printf type: int(const char*, ...) (simplified)
        let char_ty = tcx.intern(rcc_hir::Ty::Int { rank: IntRank::Char, signed: true });
        let char_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(char_ty)));
        let func_ty = tcx.intern(rcc_hir::Ty::Func {
            ret: int_ty,
            params: vec![char_ptr_ty],
            variadic: true,
            proto: true,
        });
        let func_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(func_ty)));
        let callee_def = rcc_hir::DefId::new(7);
        let callee = def_ref_expr(&mut hir_body, func_ptr_ty, callee_def);

        // args: a string literal (as DefRef) and an int
        let fmt_arg = push_expr(
            &mut hir_body,
            char_ptr_ty,
            ValueCat::RValue,
            HirExprKind::StringRef(rcc_hir::DefId::new(100)),
        );
        let int_arg = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(7));

        let call = call_expr(&mut hir_body, int_ty, callee, vec![fmt_arg, int_arg]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let result = lower_as_rvalue(&mut builder, &cx, call);

        match result {
            Operand::Copy(Place { .. }) => {}
            other => panic!("expected Copy(Place), got {other:?}"),
        }

        let body = finish(builder);
        let call_term = body.blocks.iter().find_map(|bb| match &bb.terminator.kind {
            TerminatorKind::Call { args, .. } => Some(args.len()),
            _ => None,
        });
        assert_eq!(call_term, Some(2), "variadic call must pass both args");
    }

    #[test]
    fn eval_order_call_callee_then_args_left_to_right() {
        let (mut builder, mut hir_body, mut tcx, map, [ha, hb, hc]) = three_int_locals();
        let int_ty = tcx.int;
        let func_ty = tcx.intern(rcc_hir::Ty::Func {
            ret: int_ty,
            params: vec![int_ty, int_ty],
            variadic: false,
            proto: true,
        });
        let func_ptr_ty = tcx.intern(rcc_hir::Ty::Ptr(rcc_hir::Qual::plain(func_ty)));

        let callee_side_effect = assign_local_expr(&mut hir_body, int_ty, ha, 10);
        let callee_def = def_ref_expr(&mut hir_body, func_ptr_ty, rcc_hir::DefId::new(42));
        let callee = push_expr(
            &mut hir_body,
            func_ptr_ty,
            ValueCat::RValue,
            HirExprKind::Comma { lhs: callee_side_effect, rhs: callee_def },
        );
        let arg0 = assign_local_expr(&mut hir_body, int_ty, hb, 20);
        let arg1 = assign_local_expr(&mut hir_body, int_ty, hc, 30);
        let call = call_expr(&mut hir_body, int_ty, callee, vec![arg0, arg1]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, call);
        let body = finish(builder);
        let entry = &body.blocks[BasicBlockId(0)];

        assert_eq!(
            assigned_int_constants(entry),
            vec![10, 20, 30],
            "callee side effect must precede argument side effects, and arguments must follow source order"
        );
        match &entry.terminator.kind {
            TerminatorKind::Call { args, .. } => {
                assert!(matches!(
                    args.as_slice(),
                    [
                        Operand::Copy(Place { base, projection }),
                        Operand::Copy(Place { base: base1, projection: projection1 }),
                    ] if *base == map.lookup(hb)
                        && projection.is_empty()
                        && *base1 == map.lookup(hc)
                        && projection1.is_empty()
                ));
            }
            other => panic!("expected call terminator, got {other:?}"),
        }
    }

    #[test]
    fn eval_order_binary_operands_left_to_right() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let lhs = assign_local_expr(&mut hir_body, int_ty, ha, 1);
        let rhs = assign_local_expr(&mut hir_body, int_ty, hb, 2);
        let add = push_expr(
            &mut hir_body,
            int_ty,
            ValueCat::RValue,
            HirExprKind::Binary { op: HirBinOp::Add, lhs, rhs },
        );

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        let _ = lower_as_rvalue(&mut builder, &cx, add);
        let body = finish(builder);
        let entry = &body.blocks[BasicBlockId(0)];

        assert_eq!(
            assigned_int_constants(entry),
            vec![1, 2],
            "binary operands must be materialized left-to-right before the BinaryOp temp"
        );
        assert!(matches!(
            entry.statements.last().map(|stmt| &stmt.kind),
            Some(StatementKind::Assign { rvalue: Rvalue::BinaryOp(BinOp::Add, _, _), .. })
        ));
    }

    // ------------------------------------------------------------------
    // Aggregate-init lowering (task 08-cfg/11)
    // ------------------------------------------------------------------

    /// Push an `HirExprKind::LocalRef` lvalue.
    fn local_ref_expr(body: &mut HirBody, ty: TyId, local: HirLocal) -> HirExprId {
        push_expr(body, ty, ValueCat::LValue, HirExprKind::LocalRef(local))
    }

    /// Push an `HirExprKind::Index` lvalue: `base[idx]`.
    fn index_expr(body: &mut HirBody, elem_ty: TyId, base: HirExprId, idx: i128) -> HirExprId {
        let int_ty = body.exprs[base].ty; // for the integer index node only
        let _ = int_ty;
        let index =
            push_expr(body, body.exprs[base].ty, ValueCat::RValue, HirExprKind::IntConst(idx));
        push_expr(body, elem_ty, ValueCat::LValue, HirExprKind::Index { base, index })
    }

    /// Push `target = value` (as `HirStmtKind::Expr(Assign)`).
    fn assign_index_const_stmt(
        body: &mut HirBody,
        elem_ty: TyId,
        local_ty: TyId,
        local: HirLocal,
        idx: i128,
        value: i128,
    ) -> HirStmtId {
        let base = local_ref_expr(body, local_ty, local);
        let lhs = index_expr(body, elem_ty, base, idx);
        let rhs = push_expr(body, elem_ty, ValueCat::RValue, HirExprKind::IntConst(value));
        let assign = push_expr(body, elem_ty, ValueCat::RValue, HirExprKind::Assign { lhs, rhs });
        push_stmt(body, HirStmtKind::Expr(assign))
    }

    /// Push a `LocalDecl { local, init: None }` statement.
    fn local_decl_stmt(body: &mut HirBody, local: HirLocal) -> HirStmtId {
        push_stmt(body, HirStmtKind::LocalDecl { local, init: None })
    }

    /// Push a `LocalDecl { local, init: Some(expr) }` statement.
    fn local_decl_with_init_stmt(
        body: &mut HirBody,
        local: HirLocal,
        init: HirExprId,
    ) -> HirStmtId {
        push_stmt(body, HirStmtKind::LocalDecl { local, init: Some(init) })
    }

    /// Build a fresh CFG body with one `int[N]` user-local already
    /// allocated (`Local(1)`). Returns the builder, the HIR body, the
    /// type context, the local-map (HIR `a` ↦ CFG `Local(1)`), the HIR
    /// local id, the array `TyId`, and the element `TyId`.
    fn one_array_local(len: u64) -> (BodyBuilder, HirBody, TyCtxt, LocalMap, HirLocal, TyId, TyId) {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let arr_ty = tcx.intern(Ty::Array {
            elem: rcc_hir::Qual::plain(int_ty),
            len: Some(len),
            is_vla: false,
        });

        let mut hir_body = HirBody::default();
        let ha = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: arr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(1), arr_ty, DUMMY_SP);

        let mut map = LocalMap::new();
        map.insert(ha, ca);

        (builder, hir_body, tcx, map, ha, arr_ty, int_ty)
    }

    fn assert_zero_init_at(stmt: &Statement, expected_local: Local) {
        match &stmt.kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::ZeroInit, .. })),
            } => {
                assert_eq!(*base, expected_local);
                assert!(projection.is_empty(), "ZeroInit must store to the bare aggregate place");
            }
            other => panic!("expected `local := ZeroInit`, got {other:?}"),
        }
    }

    fn assert_storage_live(stmt: &Statement, expected_local: Local) {
        match &stmt.kind {
            StatementKind::StorageLive(l) => assert_eq!(*l, expected_local),
            other => panic!("expected `StorageLive({expected_local:?})`, got {other:?}"),
        }
    }

    fn assert_storage_dead(stmt: &Statement, expected_local: Local) {
        match &stmt.kind {
            StatementKind::StorageDead(l) => assert_eq!(*l, expected_local),
            other => panic!("expected `StorageDead({expected_local:?})`, got {other:?}"),
        }
    }

    /// Acceptance: `int a[5] = {1, 2};` lowers to one `a := ZeroInit`
    /// and exactly two leaf stores (`a[0]=1`, `a[1]=2`); the
    /// trailing zero-fill leaves emitted by the HIR initialiser
    /// walker are dropped.
    #[test]
    fn aggregate_dense_partial_init() {
        let (mut builder, mut hir_body, tcx, map, ha, arr_ty, int_ty) = one_array_local(5);

        // Build the HIR sequence the initializer walker emits for
        // `int a[5] = {1, 2}`: LocalDecl + a[0]=1 + a[1]=2 + a[2]=0
        // + a[3]=0 + a[4]=0.
        let decl = local_decl_stmt(&mut hir_body, ha);
        let s0 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 0, 1);
        let s1 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 1, 2);
        let s2 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 2, 0);
        let s3 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 3, 0);
        let s4 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 4, 0);
        let root = block_stmt(&mut hir_body, vec![decl, s0, s1, s2, s3, s4]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // StorageLive(a), ZeroInit, a[0]=1, a[1]=2, StorageDead(a).
        assert_eq!(
            stmts.len(),
            5,
            "expected `StorageLive + ZeroInit + 2 leaf stores + StorageDead`, got {} statements",
            stmts.len()
        );
        assert_storage_live(&stmts[0], cfg_a);
        assert_zero_init_at(&stmts[1], cfg_a);
        // `a[0] = 1`
        match &stmts[2].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(1), .. })),
            } => {
                assert_eq!(*base, cfg_a);
                assert_eq!(projection.len(), 1);
                assert!(matches!(
                    projection[0],
                    Projection::Index(Operand::Const(Const { kind: ConstKind::Int(0), .. }))
                ));
            }
            other => panic!("expected `a[0] = 1`, got {other:?}"),
        }
        // `a[1] = 2`
        match &stmts[3].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(2), .. })),
            } => {
                assert_eq!(*base, cfg_a);
                assert!(matches!(
                    projection[0],
                    Projection::Index(Operand::Const(Const { kind: ConstKind::Int(1), .. }))
                ));
            }
            other => panic!("expected `a[1] = 2`, got {other:?}"),
        }
        assert_storage_dead(&stmts[4], cfg_a);
    }

    /// Acceptance: `int a[1000] = {0};` lowers to one `a := ZeroInit`
    /// and zero leaf stores (per task 08-cfg/11).
    #[test]
    fn aggregate_all_zero_init() {
        let (mut builder, mut hir_body, tcx, map, ha, arr_ty, int_ty) = one_array_local(1000);

        let decl = local_decl_stmt(&mut hir_body, ha);
        let mut leaves: Vec<HirStmtId> = (0..1000)
            .map(|i| assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, i, 0))
            .collect();
        let mut all = vec![decl];
        all.append(&mut leaves);
        let root = block_stmt(&mut hir_body, all);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // StorageLive(a), ZeroInit, StorageDead(a).
        assert_eq!(
            stmts.len(),
            3,
            "expected `StorageLive + ZeroInit + StorageDead`, got {} statements",
            stmts.len()
        );
        assert_storage_live(&stmts[0], cfg_a);
        assert_zero_init_at(&stmts[1], cfg_a);
        assert_storage_dead(&stmts[2], cfg_a);
    }

    /// Acceptance: `int a[5] = { [2] = 5 };` (designated) lowers to
    /// one `ZeroInit` and a single non-zero store at index `2`.
    #[test]
    fn aggregate_designated_single_store() {
        let (mut builder, mut hir_body, tcx, map, ha, arr_ty, int_ty) = one_array_local(5);

        // The HIR walker emits zeros at indices 0,1,3,4 plus the
        // explicit `a[2] = 5`.
        let decl = local_decl_stmt(&mut hir_body, ha);
        let s0 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 0, 0);
        let s1 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 1, 0);
        let s2 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 2, 5);
        let s3 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 3, 0);
        let s4 = assign_index_const_stmt(&mut hir_body, int_ty, arr_ty, ha, 4, 0);
        let root = block_stmt(&mut hir_body, vec![decl, s0, s1, s2, s3, s4]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // StorageLive(a), ZeroInit, a[2]=5, StorageDead(a).
        assert_eq!(
            stmts.len(),
            4,
            "expected `StorageLive + ZeroInit + 1 leaf store + StorageDead`, got {} statements",
            stmts.len()
        );
        assert_storage_live(&stmts[0], cfg_a);
        assert_zero_init_at(&stmts[1], cfg_a);
        match &stmts[2].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(5), .. })),
            } => {
                assert_eq!(*base, cfg_a);
                assert_eq!(projection.len(), 1);
                assert!(matches!(
                    projection[0],
                    Projection::Index(Operand::Const(Const { kind: ConstKind::Int(2), .. }))
                ));
            }
            other => panic!("expected `a[2] = 5`, got {other:?}"),
        }
        assert_storage_dead(&stmts[3], cfg_a);
    }

    /// `int x = 42;` (scalar with init) lowers to `StorageLive(x);
    /// x := 42; StorageDead(x);` once block-scope bracketing is in
    /// place; no aggregate machinery kicks in.
    #[test]
    fn scalar_local_decl_with_init() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;
        let value = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(42));
        let decl = local_decl_with_init_stmt(&mut hir_body, ha, value);
        let root = block_stmt(&mut hir_body, vec![decl]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 3);
        assert_storage_live(&stmts[0], cfg_a);
        match &stmts[1].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(42), .. })),
            } => {
                assert_eq!(*base, cfg_a);
                assert!(projection.is_empty());
            }
            other => panic!("expected `x := 42`, got {other:?}"),
        }
        assert_storage_dead(&stmts[2], cfg_a);
    }

    /// `int x;` (scalar, no init) emits no value-producing statement —
    /// the slot's value remains indeterminate — but is still bracketed
    /// by `StorageLive` / `StorageDead`.
    #[test]
    fn scalar_local_decl_uninit() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let decl = local_decl_stmt(&mut hir_body, ha);
        let root = block_stmt(&mut hir_body, vec![decl]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(
            stmts.len(),
            2,
            "scalar uninit decl must emit only `StorageLive` + `StorageDead`",
        );
        assert_storage_live(&stmts[0], cfg_a);
        assert_storage_dead(&stmts[1], cfg_a);
    }

    /// `int a[5];` (aggregate, no init list) is conservatively
    /// materialised as `a := ZeroInit`. C99 §6.7.8p10 makes the value
    /// indeterminate, but zero is a valid refinement and matches the
    /// behaviour required when a later initialiser walker leaves no
    /// trailing leaves.
    #[test]
    fn aggregate_local_decl_uninit_zeroes() {
        let (mut builder, mut hir_body, tcx, map, ha, _arr_ty, _int_ty) = one_array_local(3);
        let decl = local_decl_stmt(&mut hir_body, ha);
        let root = block_stmt(&mut hir_body, vec![decl]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // StorageLive(a), ZeroInit, StorageDead(a).
        assert_eq!(stmts.len(), 3);
        assert_storage_live(&stmts[0], cfg_a);
        assert_zero_init_at(&stmts[1], cfg_a);
        assert_storage_dead(&stmts[2], cfg_a);
    }

    // ── Task 08-12: StorageLive / StorageDead bracketing ────────────────

    /// Acceptance: `{ int x; { int y; } }` emits
    /// `StorageLive(x); StorageLive(y); StorageDead(y); StorageDead(x);`
    /// in that order — inner scope dies before the outer scope, in
    /// reverse declaration order within each frame.
    #[test]
    fn nested_blocks_emit_storage_in_lifo_order() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();

        // { int x; { int y; } }
        let decl_y = local_decl_stmt(&mut hir_body, hb);
        let inner = block_stmt(&mut hir_body, vec![decl_y]);
        let decl_x = local_decl_stmt(&mut hir_body, ha);
        let outer = block_stmt(&mut hir_body, vec![decl_x, inner]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, outer);
        let body = finish(builder);

        let cfg_x = map.lookup(ha);
        let cfg_y = map.lookup(hb);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        // x is `int` (scalar): StorageLive only, no value-store.
        // y is `int` (scalar): StorageLive only, no value-store.
        // Inner exit: StorageDead(y).
        // Outer exit: StorageDead(x).
        assert_eq!(stmts.len(), 4, "expected 4 storage statements, got {}", stmts.len());
        assert_storage_live(&stmts[0], cfg_x);
        assert_storage_live(&stmts[1], cfg_y);
        assert_storage_dead(&stmts[2], cfg_y);
        assert_storage_dead(&stmts[3], cfg_x);
    }

    /// Two siblings in the same scope die in *reverse declaration*
    /// order: `{ int a; int b; }` emits `Live(a); Live(b); Dead(b);
    /// Dead(a);`.
    #[test]
    fn sibling_locals_die_in_reverse_declaration_order() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();

        let decl_a = local_decl_stmt(&mut hir_body, ha);
        let decl_b = local_decl_stmt(&mut hir_body, hb);
        let outer = block_stmt(&mut hir_body, vec![decl_a, decl_b]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, outer);
        let body = finish(builder);

        let cfg_a = map.lookup(ha);
        let cfg_b = map.lookup(hb);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        assert_eq!(stmts.len(), 4);
        assert_storage_live(&stmts[0], cfg_a);
        assert_storage_live(&stmts[1], cfg_b);
        assert_storage_dead(&stmts[2], cfg_b);
        assert_storage_dead(&stmts[3], cfg_a);
    }

    /// `return` in a nested scope flushes every open scope in reverse
    /// before the `Return` terminator: every reachable path through a
    /// `StorageLive(L)` reaches exactly one `StorageDead(L)`.
    #[test]
    fn return_emits_storage_dead_for_all_open_scopes() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // { int x; { int y; return 0; } }
        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let decl_y = local_decl_stmt(&mut hir_body, hb);
        let inner = block_stmt(&mut hir_body, vec![decl_y, ret]);
        let decl_x = local_decl_stmt(&mut hir_body, ha);
        let outer = block_stmt(&mut hir_body, vec![decl_x, inner]);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, outer);
        let body = finish(builder);

        let cfg_x = map.lookup(ha);
        let cfg_y = map.lookup(hb);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        // StorageLive(x); StorageLive(y); ret-slot := 0;
        // StorageDead(y); StorageDead(x); Return.
        assert_eq!(entry.statements.len(), 5, "{:#?}", entry.statements);
        assert_storage_live(&entry.statements[0], cfg_x);
        assert_storage_live(&entry.statements[1], cfg_y);
        // ret-slot assign at index 2.
        match &entry.statements[2].kind {
            StatementKind::Assign { place: Place { base, .. }, .. } => {
                assert_eq!(*base, Local(0), "return value goes to ret slot");
            }
            other => panic!("expected ret-slot assign, got {other:?}"),
        }
        assert_storage_dead(&entry.statements[3], cfg_y);
        assert_storage_dead(&entry.statements[4], cfg_x);
        assert!(matches!(entry.terminator.kind, TerminatorKind::Return));
    }

    /// `break` flushes every scope opened *inside* the loop body
    /// before the `goto` to the loop exit. The loop variable from a
    /// `for (int i = 0; ...; ...)` init stays live across the break.
    #[test]
    fn break_emits_intervening_storage_dead() {
        // Build a HIR body with two locals: i (loop var, int) and y
        // (inner-block local, int).
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let mut hir_body = HirBody::default();
        let hi = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let hy = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ci = builder.alloc_user_local(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let cy = builder.alloc_user_local(rcc_span::Symbol(2), int_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hi, ci);
        map.insert(hy, cy);

        // for (int i = 0; ; ) { { int y; break; } }
        let zero = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(0));
        let init = local_decl_with_init_stmt(&mut hir_body, hi, zero);

        let decl_y = local_decl_stmt(&mut hir_body, hy);
        let brk = break_stmt(&mut hir_body);
        let inner_block = block_stmt(&mut hir_body, vec![decl_y, brk]);

        let for_stmt_id = for_stmt(&mut hir_body, Some(init), None, None, inner_block);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, for_stmt_id);
        let body = finish(builder);

        // Find the body block and verify it ends with:
        //   StorageLive(y); StorageDead(y); Goto(exit).
        // The exit block ends with StorageDead(i).
        let entry = &body.blocks[crate::BasicBlockId(0)];
        // entry: StorageLive(i); i := 0; Goto(header).
        assert_storage_live(&entry.statements[0], ci);
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        // Infinite loop ⇒ header is a plain `Goto(body_bb)`.
        let body_bb_id = goto_target(header_bb);
        let body_bb = &body.blocks[body_bb_id];

        // Body block: StorageLive(y); StorageDead(y); goto exit.
        assert!(body_bb.statements.len() >= 2);
        assert_storage_live(&body_bb.statements[0], cy);
        let last = body_bb.statements.last().expect("body must end with StorageDead(y)");
        assert_storage_dead(last, cy);
        let exit_id = goto_target(body_bb);

        // Exit block: StorageDead(i) (the for's frame).
        let exit_bb = &body.blocks[exit_id];
        assert!(!exit_bb.statements.is_empty(), "exit must hold StorageDead(i)");
        assert_storage_dead(&exit_bb.statements[0], ci);
    }

    /// `continue` flushes scopes opened inside the loop body too.
    #[test]
    fn continue_emits_intervening_storage_dead() {
        let (mut builder, mut hir_body, tcx, map, [ha, _hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // while (a) { { int b? actually any local in inner block } continue; }
        // For simplicity: while (a) { int x; continue; } where x = ha.
        let cond = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let cont = continue_stmt(&mut hir_body);
        let decl = local_decl_stmt(&mut hir_body, ha);
        let body_block = block_stmt(&mut hir_body, vec![decl, cont]);
        let root = while_stmt(&mut hir_body, cond, body_block);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_x = map.lookup(ha);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        let header_id = goto_target(entry);
        let header_bb = &body.blocks[header_id];
        let (_exit, body_bb_id) = switch_zero_default(header_bb);
        let body_bb = &body.blocks[body_bb_id];
        // Body: StorageLive(x); StorageDead(x); goto header.
        assert!(body_bb.statements.len() >= 2);
        assert_storage_live(&body_bb.statements[0], cfg_x);
        let last = body_bb.statements.last().unwrap();
        assert_storage_dead(last, cfg_x);
        // Continue jumps to header, not to exit.
        assert_eq!(goto_target(body_bb), header_id);
    }

    #[test]
    fn goto_out_of_block_emits_storage_dead() {
        let (mut builder, mut hir_body, tcx, map, [_ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // { { int y; goto L; } L: return 0; }
        let decl_y = local_decl_stmt(&mut hir_body, hb);
        let go_l = goto_stmt(&mut hir_body, "L");
        let inner = block_stmt(&mut hir_body, vec![decl_y, go_l]);
        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let label = label_stmt(&mut hir_body, "L", ret);
        let root = block_stmt(&mut hir_body, vec![inner, label]);

        builder.collect_labels(&hir_body, root, &map);
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_y = map.lookup(hb);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_storage_live(&entry.statements[0], cfg_y);
        assert_storage_dead(&entry.statements[1], cfg_y);
        let label_id = goto_target(entry);
        assert!(matches!(body.blocks[label_id].terminator.kind, TerminatorKind::Return));
    }

    #[test]
    fn goto_out_of_nested_blocks_emits_storage_dead_in_lifo_order() {
        let (mut builder, mut hir_body, tcx, map, [ha, hb, _hc]) = three_int_locals();
        let int_ty = tcx.int;

        // { { int x; { int y; goto L; } } L: return 0; }
        let decl_x = local_decl_stmt(&mut hir_body, ha);
        let decl_y = local_decl_stmt(&mut hir_body, hb);
        let go_l = goto_stmt(&mut hir_body, "L");
        let inner = block_stmt(&mut hir_body, vec![decl_y, go_l]);
        let outer = block_stmt(&mut hir_body, vec![decl_x, inner]);
        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let label = label_stmt(&mut hir_body, "L", ret);
        let root = block_stmt(&mut hir_body, vec![outer, label]);

        builder.collect_labels(&hir_body, root, &map);
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        let cfg_x = map.lookup(ha);
        let cfg_y = map.lookup(hb);
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_storage_live(&entry.statements[0], cfg_x);
        assert_storage_live(&entry.statements[1], cfg_y);
        assert_storage_dead(&entry.statements[2], cfg_y);
        assert_storage_dead(&entry.statements[3], cfg_x);
    }

    /// `for (int i = 0; ...; ...)` brackets the loop variable's
    /// lifetime around the entire loop: `StorageLive(i)` in the
    /// pre-header, `StorageDead(i)` in the post-loop exit block.
    #[test]
    fn for_init_decl_lives_across_loop() {
        let tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let mut hir_body = HirBody::default();
        let hi = hir_body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty: int_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ci = builder.alloc_user_local(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hi, ci);

        // for (int i = 0; ; ) ;     (empty body)
        let zero = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(0));
        let init = local_decl_with_init_stmt(&mut hir_body, hi, zero);
        let empty_body = block_stmt(&mut hir_body, vec![]);
        let for_stmt_id = for_stmt(&mut hir_body, Some(init), None, None, empty_body);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, for_stmt_id);
        let body = finish(builder);

        // entry: StorageLive(i); i := 0; goto header.
        let entry = &body.blocks[crate::BasicBlockId(0)];
        assert_storage_live(&entry.statements[0], ci);

        // The exit block is wherever the cursor landed after lower_for.
        // Find it by following: entry → header → body_bb → step → header.
        // Since the loop is infinite, the only exit-style block is one
        // whose only statement is StorageDead(i) (the for's
        // `exit_scope`). The reachability audit allows it as orphan.
        let exit_with_dead = body.blocks.iter().find(|bb| {
            !bb.statements.is_empty()
                && matches!(bb.statements[0].kind, StatementKind::StorageDead(l) if l == ci)
        });
        assert!(exit_with_dead.is_some(), "expected an exit block with StorageDead(i)");
    }

    // ── Task 08-13: VLA lowering ────────────────────────────────────────

    fn add_hir_local(
        body: &mut HirBody,
        ty: TyId,
        vla_len: Option<HirExprId>,
        is_param: bool,
    ) -> HirLocal {
        body.locals.push(rcc_hir::LocalDecl {
            name: None,
            ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len,
            is_param,
            span: DUMMY_SP,
        })
    }

    fn int_vla_ty(tcx: &mut TyCtxt) -> TyId {
        tcx.intern(Ty::Array { elem: rcc_hir::Qual::plain(tcx.int), len: None, is_vla: true })
    }

    fn char_int_record_ty(
        tcx: &mut TyCtxt,
        defs: &mut rcc_data_structures::IndexVec<rcc_hir::DefId, rcc_hir::Def>,
    ) -> TyId {
        let record_def = defs.push(rcc_hir::Def {
            id: rcc_hir::DefId(0),
            name: Symbol(99),
            span: DUMMY_SP,
            kind: rcc_hir::DefKind::Record {
                kind: rcc_hir::RecordKind::Struct,
                align_override: None,
                layout: None,
                fields: vec![
                    rcc_hir::Field {
                        name: None,
                        ty: tcx.char_,
                        quals: rcc_hir::ObjectQuals::none(),
                        align_override: None,
                        offset: None,
                        bit_width: None,
                        span: DUMMY_SP,
                    },
                    rcc_hir::Field {
                        name: None,
                        ty: tcx.int,
                        quals: rcc_hir::ObjectQuals::none(),
                        align_override: None,
                        offset: None,
                        bit_width: None,
                        span: DUMMY_SP,
                    },
                ],
            },
        });
        defs[record_def].id = record_def;
        tcx.intern(Ty::Record(record_def))
    }

    #[test]
    fn vla_decl_saves_runtime_bound_before_storage_live() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let vla_ty = int_vla_ty(&mut tcx);
        let mut hir_body = HirBody::default();
        let hn = add_hir_local(&mut hir_body, int_ty, None, true);
        let n_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hn));
        let ha = add_hir_local(&mut hir_body, vla_ty, Some(n_ref), false);

        let decl = local_decl_stmt(&mut hir_body, ha);
        let root = block_stmt(&mut hir_body, vec![decl]);

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(tcx.void, DUMMY_SP);
        let cn = builder.alloc_param(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(2), vla_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hn, cn);
        map.insert(ha, ca);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);
        let saved_len = body.locals[ca].vla_len.expect("VLA local should record saved len local");
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;

        match &stmts[0].kind {
            StatementKind::Assign {
                place: Place { base, projection },
                rvalue:
                    Rvalue::Cast {
                        op: Operand::Copy(Place { base: source, projection: source_proj }),
                        to,
                        kind,
                    },
            } => {
                assert_eq!(*base, saved_len);
                assert!(projection.is_empty());
                assert_eq!(*source, cn);
                assert!(source_proj.is_empty());
                assert_eq!(*to, tcx.ulong);
                assert_eq!(*kind, CastKind::IntToInt);
            }
            other => panic!("expected saved VLA length cast, got {other:?}"),
        }
        assert_storage_live(&stmts[1], ca);
        assert_storage_dead(stmts.last().expect("block should end with StorageDead(a)"), ca);
        assert!(
            !stmts.iter().any(|s| matches!(
                s.kind,
                StatementKind::Assign {
                    rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::ZeroInit, .. })),
                    ..
                }
            )),
            "plain VLA declarations must not be aggregate-zero-initialized"
        );
    }

    #[test]
    #[should_panic(expected = "goto into VLA scope")]
    fn goto_into_vla_scope_is_rejected() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let vla_ty = int_vla_ty(&mut tcx);
        let mut hir_body = HirBody::default();

        let len_expr = push_expr(&mut hir_body, int_ty, ValueCat::RValue, HirExprKind::IntConst(4));
        let ha = add_hir_local(&mut hir_body, vla_ty, Some(len_expr), false);

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(1), vla_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(ha, ca);

        // { goto L; { int a[n]; L: return 0; } }
        //
        // Jumping from outside the block to `L` would bypass the VLA
        // runtime allocation and saved length, so CFG must reject it
        // instead of lowering a valid-looking edge into the label block.
        let go_l = goto_stmt(&mut hir_body, "L");
        let decl_a = local_decl_stmt(&mut hir_body, ha);
        let ret = return_const_stmt(&mut hir_body, int_ty, 0);
        let label = label_stmt(&mut hir_body, "L", ret);
        let inner = block_stmt(&mut hir_body, vec![decl_a, label]);
        let root = block_stmt(&mut hir_body, vec![go_l, inner]);

        builder.collect_labels(&hir_body, root, &map);
        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
    }

    #[test]
    fn sizeof_vla_lowers_to_len_times_element_size() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let vla_ty = int_vla_ty(&mut tcx);
        let mut hir_body = HirBody::default();
        let hn = add_hir_local(&mut hir_body, int_ty, None, true);
        let n_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hn));
        let ha = add_hir_local(&mut hir_body, vla_ty, Some(n_ref), false);

        let decl = local_decl_stmt(&mut hir_body, ha);
        let a_ref = push_expr(&mut hir_body, vla_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let sizeof_a =
            push_expr(&mut hir_body, tcx.ulong, ValueCat::RValue, HirExprKind::SizeofExpr(a_ref));
        let expr_stmt = push_stmt(&mut hir_body, HirStmtKind::Expr(sizeof_a));
        let root = block_stmt(&mut hir_body, vec![decl, expr_stmt]);

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(tcx.void, DUMMY_SP);
        let cn = builder.alloc_param(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(2), vla_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hn, cn);
        map.insert(ha, ca);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;

        let len_assign = stmts
            .iter()
            .find_map(|stmt| match &stmt.kind {
                StatementKind::Assign {
                    place: Place { base, .. },
                    rvalue: Rvalue::Len(Place { base: len_base, projection }),
                } if *len_base == ca && projection.is_empty() => Some(*base),
                _ => None,
            })
            .expect("sizeof(vla) should emit Rvalue::Len(a)");

        assert!(
            stmts.iter().any(|stmt| matches!(
                &stmt.kind,
                StatementKind::Assign {
                    rvalue:
                        Rvalue::BinaryOp(
                            BinOp::Mul,
                            Operand::Copy(Place { base, projection }),
                            Operand::Const(Const { kind: ConstKind::Int(4), .. }),
                        ),
                    ..
                } if *base == len_assign && projection.is_empty()
            )),
            "sizeof(int vla) should multiply Len(a) by sizeof(int)"
        );
    }

    #[test]
    fn sizeof_record_and_nested_array_lower_to_checked_constants() {
        let mut tcx = TyCtxt::new();
        let mut defs = rcc_data_structures::IndexVec::new();
        let record_ty = char_int_record_ty(&mut tcx, &mut defs);
        let record_array_ty = tcx.intern(Ty::Array {
            elem: rcc_hir::Qual::plain(record_ty),
            len: Some(2),
            is_vla: false,
        });

        let mut hir_body = HirBody::default();
        let hs = add_hir_local(&mut hir_body, record_ty, None, false);
        let ha = add_hir_local(&mut hir_body, record_array_ty, None, false);
        let s_ref =
            push_expr(&mut hir_body, record_ty, ValueCat::LValue, HirExprKind::LocalRef(hs));
        let a_ref =
            push_expr(&mut hir_body, record_array_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let sizeof_s =
            push_expr(&mut hir_body, tcx.ulong, ValueCat::RValue, HirExprKind::SizeofExpr(s_ref));
        let sizeof_a =
            push_expr(&mut hir_body, tcx.ulong, ValueCat::RValue, HirExprKind::SizeofExpr(a_ref));

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(tcx.void, DUMMY_SP);
        let cs = builder.alloc_user_local(Symbol(1), record_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(Symbol(2), record_array_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hs, cs);
        map.insert(ha, ca);

        let cx = LowerCx::with_defs(&hir_body, &tcx, &map, &defs);
        let s_size = lower_as_rvalue(&mut builder, &cx, sizeof_s);
        let a_size = lower_as_rvalue(&mut builder, &cx, sizeof_a);

        assert!(matches!(s_size, Operand::Const(Const { kind: ConstKind::Int(8), .. })));
        assert!(matches!(a_size, Operand::Const(Const { kind: ConstKind::Int(16), .. })));
    }

    #[test]
    fn sizeof_record_vla_uses_checked_record_element_layout() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let mut defs = rcc_data_structures::IndexVec::new();
        let record_ty = char_int_record_ty(&mut tcx, &mut defs);
        let record_vla_ty = tcx.intern(Ty::Array {
            elem: rcc_hir::Qual::plain(record_ty),
            len: None,
            is_vla: true,
        });

        let mut hir_body = HirBody::default();
        let hn = add_hir_local(&mut hir_body, int_ty, None, true);
        let n_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hn));
        let ha = add_hir_local(&mut hir_body, record_vla_ty, Some(n_ref), false);

        let decl = local_decl_stmt(&mut hir_body, ha);
        let a_ref =
            push_expr(&mut hir_body, record_vla_ty, ValueCat::LValue, HirExprKind::LocalRef(ha));
        let sizeof_a =
            push_expr(&mut hir_body, tcx.ulong, ValueCat::RValue, HirExprKind::SizeofExpr(a_ref));
        let expr_stmt = push_stmt(&mut hir_body, HirStmtKind::Expr(sizeof_a));
        let root = block_stmt(&mut hir_body, vec![decl, expr_stmt]);

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(tcx.void, DUMMY_SP);
        let cn = builder.alloc_param(Symbol(1), int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(Symbol(2), record_vla_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hn, cn);
        map.insert(ha, ca);

        let cx = LowerCx::with_defs(&hir_body, &tcx, &map, &defs);
        lower_stmt(&mut builder, &cx, root);
        let body = finish(builder);

        assert!(
            body.blocks.iter().any(|block| block.statements.iter().any(|stmt| matches!(
                &stmt.kind,
                StatementKind::Assign {
                    rvalue: Rvalue::BinaryOp(
                        BinOp::Mul,
                        Operand::Copy(_),
                        Operand::Const(Const { kind: ConstKind::Int(8), .. }),
                    ),
                    ..
                }
            ))),
            "sizeof(struct S[n]) should multiply Len(a) by sizeof(struct S), not by 0"
        );
    }

    #[test]
    fn nested_vlas_emit_storage_dead_in_scope_order() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let vla_ty = int_vla_ty(&mut tcx);
        let mut hir_body = HirBody::default();
        let hn = add_hir_local(&mut hir_body, int_ty, None, true);
        let hm = add_hir_local(&mut hir_body, int_ty, None, true);
        let n_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hn));
        let m_ref = push_expr(&mut hir_body, int_ty, ValueCat::LValue, HirExprKind::LocalRef(hm));
        let ha = add_hir_local(&mut hir_body, vla_ty, Some(n_ref), false);
        let hb = add_hir_local(&mut hir_body, vla_ty, Some(m_ref), false);

        let decl_a = local_decl_stmt(&mut hir_body, ha);
        let decl_b = local_decl_stmt(&mut hir_body, hb);
        let inner = block_stmt(&mut hir_body, vec![decl_b]);
        let outer = block_stmt(&mut hir_body, vec![decl_a, inner]);

        let mut builder = BodyBuilder::new();
        let _ret = builder.alloc_return_slot(tcx.void, DUMMY_SP);
        let cn = builder.alloc_param(rcc_span::Symbol(1), int_ty, DUMMY_SP);
        let cm = builder.alloc_param(rcc_span::Symbol(2), int_ty, DUMMY_SP);
        let ca = builder.alloc_user_local(rcc_span::Symbol(3), vla_ty, DUMMY_SP);
        let cb = builder.alloc_user_local(rcc_span::Symbol(4), vla_ty, DUMMY_SP);
        let mut map = LocalMap::new();
        map.insert(hn, cn);
        map.insert(hm, cm);
        map.insert(ha, ca);
        map.insert(hb, cb);

        let cx = LowerCx::new(&hir_body, &tcx, &map);
        lower_stmt(&mut builder, &cx, outer);
        let body = finish(builder);
        let stmts = &body.blocks[crate::BasicBlockId(0)].statements;
        let dead_b = stmts
            .iter()
            .position(|s| matches!(s.kind, StatementKind::StorageDead(l) if l == cb))
            .expect("inner VLA should be dead at inner scope exit");
        let dead_a = stmts
            .iter()
            .position(|s| matches!(s.kind, StatementKind::StorageDead(l) if l == ca))
            .expect("outer VLA should be dead at outer scope exit");

        assert!(dead_b < dead_a, "inner VLA must be deallocated before outer VLA");
        assert!(body.locals[ca].vla_len.is_some());
        assert!(body.locals[cb].vla_len.is_some());
    }
}
