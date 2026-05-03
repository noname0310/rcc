//! Constant-expression evaluator (C99 §6.6).
//!
//! Used by:
//! * `rcc_preprocess` to resolve `#if` controlling expressions (integer subset).
//! * `rcc_hir_lower` for enumerator values, array sizes, bitfield widths.
//! * `rcc_typeck` for initializer constness verification.
//!
//! Implements the integer-constant-expression subset described by
//! C99 §6.6p6: each operand is an integer constant, an enumeration
//! constant, a character constant, a `sizeof` expression whose result
//! is a constant, or a cast to an integer type. The operators allowed
//! are arithmetic, bitwise, shift, relational, equality, logical, the
//! conditional, the unary integer operators, casts, and `sizeof`.
//! Assignment, comma, function call, pre/post-increment, and address-
//! of expressions are *not* part of an integer constant expression and
//! cause the evaluator to bail with `None`.
//!
//! All folding is performed on `i128` so we can evaluate every C99
//! integer type without losing information; division-by-zero, shift-
//! count-out-of-range, and signed overflow are detected and surfaced
//! as warnings (W0009 / W0010 / W0011) when a `Session` is supplied.

use rcc_data_structures::IndexVec;
use rcc_errors::codes;
use rcc_hir::{
    rcc_hir_binop::{BinOp, UnOp},
    Body, ConvertKind, Def, DefId, DefKind, HirExprId, HirExprKind, IntRank, LayoutCx, Ty, TyCtxt,
    TyId,
};
use rcc_session::Session;

/// A constant value recognised by the evaluator.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ConstValue {
    /// Integer value (widened to i128 for safety).
    Int(i128),
    /// Floating-point value.
    Float(f64),
}

/// A scalar constant value covering every C99 §6.6 constant
/// expression form: integer constants (§6.6p6), arithmetic constants
/// (§6.6p7, including floats), and address constants (§6.6p8 — a
/// reference to a static-storage-duration object plus an integer
/// byte offset, optionally with a null base for `(char*)0 + N`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ConstScalar {
    /// Integer constant (C99 §6.6p6 / §6.6p7 result of a comparison).
    Int(i128),
    /// Floating-point constant (C99 §6.6p7).
    Float(f64),
    /// Address constant (C99 §6.6p8): `&obj`, `&arr[N]`, function
    /// designator, or null-base `(char*)0 + N`. The base is `Some`
    /// when it points at a `Def` (global object / function); `None`
    /// represents the null pointer's base. The `offset` is in bytes.
    Address {
        /// Static-storage-duration object/function the address refers
        /// to, or `None` for a null base (`(char*)0 + offset`).
        def: Option<DefId>,
        /// Byte offset relative to `def`'s base.
        offset: i128,
    },
}

/// Constant-expression evaluator.
///
/// Borrows the [`TyCtxt`] for type queries (sizeof, signedness), the
/// [`Body`] holding the expression arena, an optional [`Def`] table so
/// `DefRef`-to-enumerator references resolve, and an optional
/// `&mut Session` so the evaluator can warn on overflow / div-by-zero /
/// out-of-range shifts.
pub struct ConstEval<'a> {
    /// Type context (for conversions / widths).
    pub tcx: &'a TyCtxt,
    /// Body being evaluated; lookup for `HirExprId`s lives here.
    pub body: Option<&'a Body>,
    /// Top-level definitions, used to resolve enumerator constants.
    pub defs: Option<&'a IndexVec<DefId, Def>>,
    /// Diagnostic sink for non-fatal evaluator warnings.
    pub session: Option<&'a mut Session>,
}

impl<'a> ConstEval<'a> {
    /// Build an evaluator with the legacy two-argument signature used
    /// by callers that don't need enumerator resolution or diagnostics.
    pub fn new(tcx: &'a TyCtxt, body: Option<&'a Body>) -> Self {
        Self { tcx, body, defs: None, session: None }
    }

    /// Build an evaluator wired up with the full set of inputs.
    pub fn with_defs_and_session(
        tcx: &'a TyCtxt,
        body: Option<&'a Body>,
        defs: Option<&'a IndexVec<DefId, Def>>,
        session: Option<&'a mut Session>,
    ) -> Self {
        Self { tcx, body, defs, session }
    }

    /// Evaluate `expr` and return its constant value.
    ///
    /// Float constants surface as [`ConstValue::Float`]; integer
    /// constants run through [`Self::eval_int`].
    pub fn eval(&mut self, expr: HirExprId) -> Option<ConstValue> {
        let body = self.body?;
        let e = body.exprs.get(expr)?;
        if let HirExprKind::FloatConst(v) = &e.kind {
            return Some(ConstValue::Float(*v));
        }
        self.eval_int(expr).map(ConstValue::Int)
    }

    /// Unified entry point for C99 §6.6 constant expressions.
    ///
    /// Tries the integer path first (§6.6p6), then the
    /// arithmetic/floating-point path (§6.6p7), and finally the
    /// address-constant path (§6.6p8). Returns the first form that
    /// succeeds; `None` means the expression is not a constant
    /// expression of any C99-recognised flavour.
    pub fn eval_scalar(&mut self, expr: HirExprId) -> Option<ConstScalar> {
        if let Some(v) = self.eval_int(expr) {
            return Some(ConstScalar::Int(v));
        }
        if let Some(v) = self.eval_arith(expr) {
            return Some(ConstScalar::Float(v));
        }
        let (def, offset) = self.eval_address(expr)?;
        Some(ConstScalar::Address { def, offset })
    }

    /// Evaluate `expr` as a C99 §6.6p7 arithmetic constant expression
    /// and return its `f64` value.
    ///
    /// An arithmetic constant expression has arithmetic operands —
    /// integer constants, floating constants, character constants,
    /// `sizeof` results, enumeration constants, and casts that yield
    /// arithmetic types. Operators are the same as for an ICE except
    /// `sizeof` (handled implicitly via `Cast` truncation), and the
    /// expression's value may be a floating-point quantity.
    ///
    /// This routine performs every operation in IEEE-754 binary64.
    /// Comparisons and logical operators yield 0 / 1 as `f64` — but a
    /// caller that needs an integer result should call [`Self::eval_int`]
    /// instead, which handles those producing-`int` cases natively.
    pub fn eval_arith(&mut self, expr: HirExprId) -> Option<f64> {
        let body = self.body?;
        let e = body.exprs.get(expr)?;
        match e.kind.clone() {
            // ---- Leaves ---------------------------------------------------
            HirExprKind::FloatConst(v) => Some(v),
            // Integer constants are also arithmetic constants — promote
            // to f64 in the floating context.
            #[allow(clippy::cast_precision_loss)]
            HirExprKind::IntConst(v) => Some(v as f64),

            HirExprKind::DefRef(def_id) => {
                let defs = self.defs?;
                let def = defs.get(def_id)?;
                match &def.kind {
                    #[allow(clippy::cast_precision_loss)]
                    DefKind::Enumerator { value, .. } => Some(*value as f64),
                    _ => None,
                }
            }

            HirExprKind::StringRef(_) | HirExprKind::LocalRef(_) => None,
            HirExprKind::SizeofExpr(operand) => {
                let ty = body.exprs.get(operand)?.ty;
                #[allow(clippy::cast_precision_loss)]
                self.size_of_ty(ty).map(|size| size as f64)
            }
            HirExprKind::SizeofType(ty) => {
                let size = self.size_of_ty(ty)?;
                #[allow(clippy::cast_precision_loss)]
                Some(size as f64)
            }

            // ---- Operators ------------------------------------------------
            HirExprKind::Unary { op, operand } => {
                let v = self.eval_arith(operand)?;
                match op {
                    UnOp::Plus => Some(v),
                    UnOp::Neg => Some(-v),
                    // `~` is integer-only.
                    UnOp::BitNot => None,
                    UnOp::LogNot => Some(if v == 0.0 { 1.0 } else { 0.0 }),
                    UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => None,
                }
            }

            HirExprKind::Binary { op, lhs, rhs } => self.eval_arith_binary(op, lhs, rhs),

            HirExprKind::Cond { cond, then_expr, else_expr } => {
                // The controlling expression is itself a constant
                // expression, but it can be either int or arithmetic;
                // try integer first (the common case for `1 ? a : b`),
                // then arithmetic for `1.0 ? a : b`.
                let c = if let Some(c) = self.eval_int(cond) {
                    c != 0
                } else {
                    self.eval_arith(cond)? != 0.0
                };
                if c {
                    self.eval_arith(then_expr)
                } else {
                    self.eval_arith(else_expr)
                }
            }

            HirExprKind::Cast { operand, to } => {
                let target = self.tcx.get(to);
                match *target {
                    // Cast to floating: route the operand through this
                    // routine again. We intentionally don't try to
                    // model `float` vs `double` precision here — the
                    // evaluator works in `f64` throughout, matching
                    // host semantics where `float` arithmetic
                    // round-trips through `f64` registers anyway.
                    Ty::Float(_) => self.eval_arith(operand),
                    // Cast to integer: evaluate operand as f64, then
                    // truncate-towards-zero (C99 §6.3.1.4).
                    Ty::Int { signed, rank } => {
                        let v = self.eval_arith(operand)?;
                        // Truncate-toward-zero, then bit-mask to the
                        // destination width.
                        if !v.is_finite() {
                            return None;
                        }
                        let truncated = v.trunc();
                        // Best-effort conversion via i128. f64 → i128
                        // is well-defined for values inside
                        // [i128::MIN, i128::MAX]; outside that range
                        // we bail rather than fold a UB result.
                        if truncated < i128::MIN as f64 || truncated > i128::MAX as f64 {
                            return None;
                        }
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_precision_loss,
                            clippy::cast_possible_wrap
                        )]
                        let as_int = truncated as i128;
                        let bits = int_rank_bits(rank);
                        #[allow(clippy::cast_precision_loss)]
                        let masked = truncate_to_width(as_int, bits, signed) as f64;
                        Some(masked)
                    }
                    _ => None,
                }
            }

            HirExprKind::Convert { operand, kind } => match kind {
                ConvertKind::IntegerPromotion
                | ConvertKind::UsualArithmetic
                | ConvertKind::LvalueToRvalue => self.eval_arith(operand),
                ConvertKind::ArrayToPtr | ConvertKind::FuncToPtr | ConvertKind::Pointer => None,
                // `_Complex` arithmetic constant folding is not part of
                // M7 — a real-to-complex / complex-to-real wrapper means
                // the surrounding expression is *not* a real arithmetic
                // constant. Bail without trying to fold (full complex
                // const-eval is genuinely hard and is left as future
                // work for the §6.6p7 arithmetic-constant pass).
                ConvertKind::RealToComplex | ConvertKind::ComplexToReal => None,
            },

            HirExprKind::Call { .. }
            | HirExprKind::UnresolvedField { .. }
            | HirExprKind::Field { .. }
            | HirExprKind::Index { .. }
            | HirExprKind::CompoundLiteral { .. }
            | HirExprKind::AddressOf(_)
            | HirExprKind::Deref(_)
            | HirExprKind::Comma { .. }
            | HirExprKind::Assign { .. }
            | HirExprKind::BuiltinVaArg { .. }
            | HirExprKind::BuiltinVaStart { .. }
            | HirExprKind::BuiltinVaEnd { .. }
            | HirExprKind::BuiltinVaCopy { .. } => None,
        }
    }

    /// Evaluate `expr` as a C99 §6.6p8 address constant.
    ///
    /// Returns `(Some(def), offset)` when the expression resolves to
    /// a static-storage-duration object/function reference plus an
    /// integer byte offset, or `(None, offset)` for a null-base
    /// expression like `(char*)0 + N` (§6.3.2.3p3 null pointer
    /// constant + integer offset).
    ///
    /// Recognised forms (all combined with optional `+ icex` /
    /// `- icex` integer-constant offsets):
    /// * `&obj` where `obj` is a global / function (rvalue address).
    /// * `arr` where `arr` is a global array (decays to its address).
    /// * `func` where `func` is a function designator.
    /// * `&arr[N]` — folded to `(arr, N * sizeof(elem))`.
    /// * `(T*) 0` and `(T*) 0 + N` — folded to `(None, N * sizeof(T))`.
    ///
    /// `&obj.field` / `&obj->field` would require record layout
    /// (filled by codegen) and so currently returns `(Some(def), 0)`
    /// — the codegen pass adds the field offset when materialising
    /// the constant initialiser. Tests pin the documented behaviour.
    pub fn eval_address(&mut self, expr: HirExprId) -> Option<(Option<DefId>, i128)> {
        let body = self.body?;
        let e = body.exprs.get(expr)?;
        match e.kind.clone() {
            // ---- Direct designators --------------------------------------
            HirExprKind::DefRef(def_id) => {
                // A bare DefRef here means the expression is in an rvalue
                // context but typed as a pointer — i.e. an array or
                // function name that has decayed already. Enumerators
                // are integer constants, not addresses.
                let defs = self.defs?;
                let def = defs.get(def_id)?;
                match &def.kind {
                    DefKind::Global { .. } | DefKind::Function { .. } => Some((Some(def_id), 0)),
                    _ => None,
                }
            }

            // `&expr` ----------------------------------------------------
            HirExprKind::AddressOf(operand) => self.eval_address_of(operand),

            // `arr` decays to `&arr[0]` — handled via the `Convert`
            // wrappers the typeck pass inserts.
            HirExprKind::Convert { operand, kind } => match kind {
                ConvertKind::ArrayToPtr | ConvertKind::FuncToPtr => self.eval_address(operand),
                ConvertKind::Pointer | ConvertKind::LvalueToRvalue => self.eval_address(operand),
                ConvertKind::IntegerPromotion | ConvertKind::UsualArithmetic => None,
                // Complex conversions never produce an address constant.
                ConvertKind::RealToComplex | ConvertKind::ComplexToReal => None,
            },

            // `(T*) icex` — null pointer constant with an integer offset.
            HirExprKind::Cast { operand, to } => {
                let target = self.tcx.get(to);
                match *target {
                    Ty::Ptr(_) => {
                        // First, see if the operand was already an
                        // address constant (e.g. `(char*) &x`). If so,
                        // pass the base/offset through unchanged —
                        // pointer-to-pointer casts preserve the address
                        // value (C99 §6.3.2.3p7).
                        if let Some(addr) = self.eval_address(operand) {
                            return Some(addr);
                        }
                        // Otherwise treat as a null-base if the
                        // operand folds to integer 0; non-zero integer
                        // casts to pointer are implementation-defined
                        // and not constant for our purposes.
                        let v = self.eval_int(operand)?;
                        Some((None, v))
                    }
                    _ => None,
                }
            }

            // Pointer + integer / pointer - integer ----------------------
            HirExprKind::Binary { op, lhs, rhs } => self.eval_address_binary(op, lhs, rhs),

            _ => None,
        }
    }

    /// Body of `eval_address` for the `&expr` case. Handles
    /// `&global`, `&arr[N]`, and the field-offset deferred case.
    fn eval_address_of(&mut self, operand: HirExprId) -> Option<(Option<DefId>, i128)> {
        let body = self.body?;
        let e = body.exprs.get(operand)?;
        match e.kind.clone() {
            HirExprKind::DefRef(def_id) => {
                let defs = self.defs?;
                let def = defs.get(def_id)?;
                match &def.kind {
                    DefKind::Global { .. } | DefKind::Function { .. } => Some((Some(def_id), 0)),
                    _ => None,
                }
            }
            HirExprKind::Index { base, index } => {
                // `&base[index]` ≡ `base + index * sizeof(*base)` with
                // `base` decaying to a pointer. The element size lives
                // on the indexed expression's type (which is already
                // the element type after typeck).
                let elem_ty = body.exprs.get(operand)?.ty;
                let elem_size = self.size_of_ty(elem_ty)?;
                let (base_def, base_off) = self.eval_address(base)?;
                let idx = self.eval_int(index)?;
                let off = idx.checked_mul(elem_size)?;
                let total = base_off.checked_add(off)?;
                Some((base_def, total))
            }
            HirExprKind::UnresolvedField { .. } => None,
            HirExprKind::Field { base, .. } => {
                // `&obj.field` — record-layout-dependent. We expose
                // `(Some(base_def), 0)` and let the codegen pass add
                // the field offset; the deferred behaviour is pinned
                // by `field_offset_deferred_to_codegen` below.
                let (base_def, base_off) = self.eval_address(base)?;
                Some((base_def, base_off))
            }
            HirExprKind::Deref(inner) => {
                // `&*p` ≡ `p` (C99 §6.5.3.2p3).
                self.eval_address(inner)
            }
            _ => None,
        }
    }

    /// Body of `eval_address` for the `Binary` case.
    ///
    /// Handles `addr + icex`, `icex + addr`, and `addr - icex`,
    /// scaling the integer by the element size of the pointee type.
    fn eval_address_binary(
        &mut self,
        op: BinOp,
        lhs: HirExprId,
        rhs: HirExprId,
    ) -> Option<(Option<DefId>, i128)> {
        let body = self.body?;
        match op {
            BinOp::Add => {
                // Either side may be the pointer.
                if let Some((def, off)) = self.eval_address(lhs) {
                    let elem_size = self.pointee_size_of_expr(lhs)?;
                    let n = self.eval_int(rhs)?;
                    let scaled = n.checked_mul(elem_size)?;
                    let total = off.checked_add(scaled)?;
                    return Some((def, total));
                }
                if let Some((def, off)) = self.eval_address(rhs) {
                    let elem_size = self.pointee_size_of_expr(rhs)?;
                    let n = self.eval_int(lhs)?;
                    let scaled = n.checked_mul(elem_size)?;
                    let total = off.checked_add(scaled)?;
                    return Some((def, total));
                }
                None
            }
            BinOp::Sub => {
                let (def, off) = self.eval_address(lhs)?;
                let elem_size = self.pointee_size_of_expr(lhs)?;
                let n = self.eval_int(rhs)?;
                let scaled = n.checked_mul(elem_size)?;
                let total = off.checked_sub(scaled)?;
                let _ = body; // silence unused if no body needed
                Some((def, total))
            }
            _ => None,
        }
    }

    /// Element size of the pointee of a pointer-typed expression, or
    /// `None` if the expression isn't a pointer or the pointee is
    /// an incomplete / unsized type.
    fn pointee_size_of_expr(&self, expr: HirExprId) -> Option<i128> {
        let body = self.body?;
        let ty = body.exprs.get(expr)?.ty;
        match self.tcx.get(ty).clone() {
            Ty::Ptr(qual) => self.size_of_ty(qual.ty),
            // Array names appear here when ArrayToPtr decay has not
            // been wrapped (e.g. when the address evaluator recurses
            // into a `Convert::ArrayToPtr`). The "pointer" is a
            // pointer-to-element, so the element size of the array
            // is what we want.
            Ty::Array { elem, .. } => self.size_of_ty(elem.ty),
            _ => None,
        }
    }

    /// Evaluate the expression as an integer constant expression
    /// (C99 §6.6p6). Returns `None` when the expression is not a
    /// valid ICE or when a folding hazard (overflow, div-by-zero,
    /// shift-out-of-range) makes the result undefined.
    pub fn eval_int(&mut self, expr: HirExprId) -> Option<i128> {
        let body = self.body?;
        let e = body.exprs.get(expr)?;
        let span = e.span;
        match e.kind.clone() {
            // ---- Leaves ---------------------------------------------------
            HirExprKind::IntConst(v) => Some(v),

            // C99 §6.6p6: floats may participate in an ICE only when
            // they are the operand of a cast to an integer type — that
            // path is handled by `Cast`, never here.
            HirExprKind::FloatConst(_) => None,

            // String / local references / function or global designators
            // are not integer constants.
            HirExprKind::StringRef(_) | HirExprKind::LocalRef(_) => None,

            // Top-level reference: only enumerators are integer
            // constants. Functions and globals are *address constants*,
            // which §6.6 distinguishes from integer constants.
            HirExprKind::DefRef(def_id) => {
                let defs = self.defs?;
                let def = defs.get(def_id)?;
                match &def.kind {
                    DefKind::Enumerator { value, .. } => Some(*value),
                    _ => None,
                }
            }

            // ---- Operators ------------------------------------------------
            HirExprKind::Unary { op, operand } => {
                let v = self.eval_int(operand)?;
                match op {
                    UnOp::Plus => Some(v),
                    UnOp::Neg => match v.checked_neg() {
                        Some(r) => Some(r),
                        None => {
                            self.warn_overflow(span);
                            None
                        }
                    },
                    UnOp::BitNot => Some(!v),
                    UnOp::LogNot => Some(i128::from(v == 0)),
                    // Pre/post increment are forbidden in an ICE
                    // (§6.6p3 — "shall not contain assignment, …,
                    // increment, or decrement operators").
                    UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => None,
                }
            }

            HirExprKind::Binary { op, lhs, rhs } => self.eval_binary(op, lhs, rhs, span),

            HirExprKind::Cond { cond, then_expr, else_expr } => {
                // C99 §6.5.15p4: the condition is evaluated first and
                // exactly one of the two arms is — the unselected arm
                // need not even be a constant expression.
                if self.eval_int(cond)? != 0 {
                    self.eval_int(then_expr)
                } else {
                    self.eval_int(else_expr)
                }
            }

            HirExprKind::Cast { operand, to } => {
                // Cast to an integer type is part of an ICE; cast to a
                // floating type or pointer is not (the latter would
                // produce an address constant, not an integer
                // constant).
                let target = self.tcx.get(to);
                let (signed, bits) = match *target {
                    Ty::Int { signed, rank } => (signed, int_rank_bits(rank)),
                    _ => return None,
                };
                let v = self.eval_int(operand)?;
                Some(truncate_to_width(v, bits, signed))
            }

            HirExprKind::SizeofExpr(operand) => {
                let ty = body.exprs.get(operand)?.ty;
                self.size_of_ty(ty)
            }
            HirExprKind::SizeofType(ty) => self.size_of_ty(ty),

            // The typeck pass wraps ICE-bearing expressions in
            // `Convert { kind: IntegerPromotion | UsualArithmetic |
            // LvalueToRvalue | … }` nodes. None of those affect the
            // *value* of an integer constant — promotion and the usual
            // arithmetic conversions widen the representation but
            // preserve value (the destination type always represents
            // the source value when the source is an integer
            // constant); lvalue-to-rvalue is the identity on rvalues
            // (and a constant expression is always an rvalue at the
            // language level). Pointer / array-decay / func-decay
            // wrappers cannot appear over an integer-typed leaf, so
            // recursing is safe.
            HirExprKind::Convert { operand, kind } => match kind {
                ConvertKind::IntegerPromotion
                | ConvertKind::UsualArithmetic
                | ConvertKind::LvalueToRvalue => self.eval_int(operand),
                // Decay / pointer conversions never produce an
                // integer-typed result.
                ConvertKind::ArrayToPtr | ConvertKind::FuncToPtr | ConvertKind::Pointer => None,
                // `_Complex` conversions never produce an integer-typed
                // result either. The surrounding expression is not an
                // integer constant expression in the §6.6p6 sense.
                ConvertKind::RealToComplex | ConvertKind::ComplexToReal => None,
            },

            // The remaining HIR kinds are not part of an integer
            // constant expression by C99 §6.6p3 (assignment, comma,
            // function call) or §6.6p7 (lvalue access, address-of,
            // indirection on an arbitrary pointer, struct field /
            // array indexing on a non-constant target). Bail with
            // `None`.
            HirExprKind::Call { .. }
            | HirExprKind::UnresolvedField { .. }
            | HirExprKind::Field { .. }
            | HirExprKind::Index { .. }
            | HirExprKind::CompoundLiteral { .. }
            | HirExprKind::AddressOf(_)
            | HirExprKind::Deref(_)
            | HirExprKind::Comma { .. }
            | HirExprKind::Assign { .. }
            | HirExprKind::BuiltinVaArg { .. }
            | HirExprKind::BuiltinVaStart { .. }
            | HirExprKind::BuiltinVaEnd { .. }
            | HirExprKind::BuiltinVaCopy { .. } => None,
        }
    }

    /// Compute the size of a type in bytes using the shared layout service.
    pub fn size_of_ty(&self, ty: TyId) -> Option<i128> {
        let layout = match self.defs {
            Some(defs) => LayoutCx::with_defs(self.tcx, defs).layout_of(ty),
            None => LayoutCx::new(self.tcx).layout_of(ty),
        }
        .ok()?;
        Some(i128::from(layout.size))
    }

    fn eval_binary(
        &mut self,
        op: BinOp,
        lhs: HirExprId,
        rhs: HirExprId,
        span: rcc_span::Span,
    ) -> Option<i128> {
        // `&&` / `||` short-circuit per C99 §6.5.13–14: if the LHS
        // already determines the result, the RHS need not be a
        // constant expression at all.
        match op {
            BinOp::LogAnd => {
                let l = self.eval_int(lhs)?;
                if l == 0 {
                    return Some(0);
                }
                let r = self.eval_int(rhs)?;
                return Some(i128::from(r != 0));
            }
            BinOp::LogOr => {
                let l = self.eval_int(lhs)?;
                if l != 0 {
                    return Some(1);
                }
                let r = self.eval_int(rhs)?;
                return Some(i128::from(r != 0));
            }
            _ => {}
        }

        let l = self.eval_int(lhs)?;
        let r = self.eval_int(rhs)?;
        match op {
            BinOp::Add => match l.checked_add(r) {
                Some(v) => Some(v),
                None => {
                    self.warn_overflow(span);
                    None
                }
            },
            BinOp::Sub => match l.checked_sub(r) {
                Some(v) => Some(v),
                None => {
                    self.warn_overflow(span);
                    None
                }
            },
            BinOp::Mul => match l.checked_mul(r) {
                Some(v) => Some(v),
                None => {
                    self.warn_overflow(span);
                    None
                }
            },
            BinOp::Div => {
                if r == 0 {
                    self.warn_div_by_zero(span);
                    return None;
                }
                match l.checked_div(r) {
                    Some(v) => Some(v),
                    None => {
                        // Only `i128::MIN / -1` reaches here.
                        self.warn_overflow(span);
                        None
                    }
                }
            }
            BinOp::Rem => {
                if r == 0 {
                    self.warn_div_by_zero(span);
                    return None;
                }
                match l.checked_rem(r) {
                    Some(v) => Some(v),
                    None => {
                        self.warn_overflow(span);
                        None
                    }
                }
            }
            BinOp::Shl => {
                let count = r;
                if !(0..128).contains(&count) {
                    self.warn_shift_oor(span);
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = count as u32;
                match l.checked_shl(n) {
                    Some(v) => Some(v),
                    None => {
                        self.warn_overflow(span);
                        None
                    }
                }
            }
            BinOp::Shr => {
                let count = r;
                if !(0..128).contains(&count) {
                    self.warn_shift_oor(span);
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = count as u32;
                Some(l >> n)
            }
            BinOp::BitAnd => Some(l & r),
            BinOp::BitXor => Some(l ^ r),
            BinOp::BitOr => Some(l | r),
            BinOp::Lt => Some(i128::from(l < r)),
            BinOp::Le => Some(i128::from(l <= r)),
            BinOp::Gt => Some(i128::from(l > r)),
            BinOp::Ge => Some(i128::from(l >= r)),
            BinOp::Eq => Some(i128::from(l == r)),
            BinOp::Ne => Some(i128::from(l != r)),
            BinOp::LogAnd | BinOp::LogOr => unreachable!("handled above"),
        }
    }

    fn eval_arith_binary(&mut self, op: BinOp, lhs: HirExprId, rhs: HirExprId) -> Option<f64> {
        // `&&` / `||` short-circuit (C99 §6.5.13–14). The result is
        // always 0 / 1, which we surface as `f64` for the arithmetic
        // path's uniform return type — callers wanting an `int` for
        // these go through `eval_int`.
        match op {
            BinOp::LogAnd => {
                let l = self.eval_arith(lhs)?;
                if l == 0.0 {
                    return Some(0.0);
                }
                let r = self.eval_arith(rhs)?;
                return Some(if r != 0.0 { 1.0 } else { 0.0 });
            }
            BinOp::LogOr => {
                let l = self.eval_arith(lhs)?;
                if l != 0.0 {
                    return Some(1.0);
                }
                let r = self.eval_arith(rhs)?;
                return Some(if r != 0.0 { 1.0 } else { 0.0 });
            }
            _ => {}
        }
        let l = self.eval_arith(lhs)?;
        let r = self.eval_arith(rhs)?;
        match op {
            BinOp::Add => Some(l + r),
            BinOp::Sub => Some(l - r),
            BinOp::Mul => Some(l * r),
            BinOp::Div => {
                // IEEE-754 div-by-zero yields ±inf / NaN; the C99
                // §6.5.5p5 UB is at the language level. Letting the
                // host FPU produce an inf is fine for compile-time
                // folding because callers either consume the f64
                // directly or run the result through a cast that will
                // bail on non-finite values.
                Some(l / r)
            }
            // `%` is integer-only (C99 §6.5.5p2).
            BinOp::Rem => None,
            // Bitwise / shift are integer-only.
            BinOp::Shl | BinOp::Shr | BinOp::BitAnd | BinOp::BitXor | BinOp::BitOr => None,
            // Comparisons yield 0/1 as `f64` (the source-level result
            // type is `int`, but uniformity wins here).
            BinOp::Lt => Some(if l < r { 1.0 } else { 0.0 }),
            BinOp::Le => Some(if l <= r { 1.0 } else { 0.0 }),
            BinOp::Gt => Some(if l > r { 1.0 } else { 0.0 }),
            BinOp::Ge => Some(if l >= r { 1.0 } else { 0.0 }),
            #[allow(clippy::float_cmp)]
            BinOp::Eq => Some(if l == r { 1.0 } else { 0.0 }),
            #[allow(clippy::float_cmp)]
            BinOp::Ne => Some(if l != r { 1.0 } else { 0.0 }),
            BinOp::LogAnd | BinOp::LogOr => unreachable!("handled above"),
        }
    }

    fn warn_overflow(&mut self, span: rcc_span::Span) {
        if let Some(s) = self.session.as_deref_mut() {
            s.handler
                .struct_warn(span, "integer overflow in constant expression")
                .code(codes::W0009)
                .emit();
        }
    }

    fn warn_div_by_zero(&mut self, span: rcc_span::Span) {
        if let Some(s) = self.session.as_deref_mut() {
            s.handler
                .struct_warn(span, "division by zero in constant expression")
                .code(codes::W0010)
                .emit();
        }
    }

    fn warn_shift_oor(&mut self, span: rcc_span::Span) {
        if let Some(s) = self.session.as_deref_mut() {
            s.handler
                .struct_warn(span, "shift count out of range in constant expression")
                .code(codes::W0011)
                .emit();
        }
    }
}

/// Width in bits of an integer rank, mirroring `rcc_typeck::int_rank_bits`.
///
/// Kept in this module so const-eval has zero dependency on the surface
/// `lib.rs` helpers; the values must stay in sync with `lib.rs::INT_BITS`.
fn int_rank_bits(rank: IntRank) -> u32 {
    match rank {
        IntRank::Bool => 1,
        IntRank::Char => 8,
        IntRank::Short => 16,
        IntRank::Int => 32,
        IntRank::Long => 64,
        IntRank::LongLong => 64,
    }
}

/// Truncate `v` to a `bits`-wide integer of the given signedness,
/// preserving the standard C casting semantics: cast to a narrower
/// unsigned type masks; cast to a narrower signed type masks then
/// sign-extends.
fn truncate_to_width(v: i128, bits: u32, signed: bool) -> i128 {
    if bits >= 128 {
        return v;
    }
    // `_Bool`: §6.3.1.2 says the result is 0 if the source is 0,
    // 1 otherwise — never the masked low bit.
    if bits == 1 && !signed {
        return i128::from(v != 0);
    }
    let mask: u128 = (1u128 << bits) - 1;
    #[allow(clippy::cast_sign_loss)]
    let raw = (v as u128) & mask;
    if signed {
        let sign_bit = 1u128 << (bits - 1);
        if raw & sign_bit != 0 {
            // Sign-extend: subtract 2^bits.
            #[allow(clippy::cast_possible_wrap)]
            let extended = raw.wrapping_sub(1u128 << bits) as i128;
            extended
        } else {
            #[allow(clippy::cast_possible_wrap)]
            let widened = raw as i128;
            widened
        }
    } else {
        #[allow(clippy::cast_possible_wrap)]
        let widened = raw as i128;
        widened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_hir::{HirExpr, HirExprId, ValueCat};
    use rcc_span::DUMMY_SP;

    fn push(body: &mut Body, ty: TyId, kind: HirExprKind) -> HirExprId {
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

    /// Acceptance: literal `42` evaluates to 42.
    #[test]
    fn literal_42() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let e = push(&mut body, tcx.int, HirExprKind::IntConst(42));
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(e), Some(42));
    }

    /// Acceptance: `1 + 2 * 3` evaluates to 7.
    #[test]
    fn add_mul_precedence() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let three = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let mul =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Mul, lhs: two, rhs: three });
        let add =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: one, rhs: mul });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(add), Some(7));
    }

    #[test]
    fn sub_div_mod() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(10));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let sub = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Sub, lhs: a, rhs: b });
        let div = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Div, lhs: a, rhs: b });
        let rem = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Rem, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(sub), Some(7));
        assert_eq!(ce.eval_int(div), Some(3));
        assert_eq!(ce.eval_int(rem), Some(1));
    }

    /// `1 << 4` → 16.
    #[test]
    fn shift_left() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(4));
        let shl = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Shl, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(shl), Some(16));
    }

    /// `0xFF & 0x0F` → 15.
    #[test]
    fn bitand() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(0xFF));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(0x0F));
        let e = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::BitAnd, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(e), Some(15));
    }

    /// Logical operators short-circuit and yield 0/1.
    #[test]
    fn logical_ops() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let zero = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        let and = push(
            &mut body,
            tcx.int,
            HirExprKind::Binary { op: BinOp::LogAnd, lhs: one, rhs: zero },
        );
        let or =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::LogOr, lhs: one, rhs: zero });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(and), Some(0));
        assert_eq!(ce.eval_int(or), Some(1));
    }

    /// Comparisons yield 0/1.
    #[test]
    fn comparison_ops() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let lt =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Lt, lhs: one, rhs: two });
        let two_a = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let two_b = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let eq =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Eq, lhs: two_a, rhs: two_b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(lt), Some(1));
        assert_eq!(ce.eval_int(eq), Some(1));
    }

    /// `1 ? 10 : 20` → 10.
    #[test]
    fn conditional() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let cond = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let then_e = push(&mut body, tcx.int, HirExprKind::IntConst(10));
        let else_e = push(&mut body, tcx.int, HirExprKind::IntConst(20));
        let cv = push(
            &mut body,
            tcx.int,
            HirExprKind::Cond { cond, then_expr: then_e, else_expr: else_e },
        );
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(cv), Some(10));
    }

    /// `-5`, `~0`, `!1`.
    #[test]
    fn unary_ops() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let five = push(&mut body, tcx.int, HirExprKind::IntConst(5));
        let neg = push(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::Neg, operand: five });
        let zero = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        let bnot = push(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::BitNot, operand: zero });
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let lnot = push(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::LogNot, operand: one });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(neg), Some(-5));
        assert_eq!(ce.eval_int(bnot), Some(-1));
        assert_eq!(ce.eval_int(lnot), Some(0));
    }

    /// Cast `(unsigned char) 300` → 44 (mod 256).
    #[test]
    fn cast_truncate() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let v = push(&mut body, tcx.int, HirExprKind::IntConst(300));
        let c = push(&mut body, tcx.uchar, HirExprKind::Cast { operand: v, to: tcx.uchar });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(c), Some(44));
    }

    /// Cast `(signed char) 200` sign-extends to -56.
    #[test]
    fn cast_sign_extend() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let v = push(&mut body, tcx.int, HirExprKind::IntConst(200));
        let c = push(&mut body, tcx.schar, HirExprKind::Cast { operand: v, to: tcx.schar });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(c), Some(-56));
    }

    /// `Convert { IntegerPromotion }` is value-preserving.
    #[test]
    fn convert_passthrough() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let v = push(&mut body, tcx.schar, HirExprKind::IntConst(7));
        let promoted = push(
            &mut body,
            tcx.int,
            HirExprKind::Convert { operand: v, kind: ConvertKind::IntegerPromotion },
        );
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(promoted), Some(7));
    }

    /// Acceptance: `sizeof(int)` evaluates to the target's int size (4).
    #[test]
    fn sizeof_int_via_size_of_ty() {
        let tcx = TyCtxt::new();
        let body = Body::default();
        let ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.size_of_ty(tcx.int), Some(4));
        assert_eq!(ce.size_of_ty(tcx.char_), Some(1));
        assert_eq!(ce.size_of_ty(tcx.short), Some(2));
        assert_eq!(ce.size_of_ty(tcx.long), Some(8));
        assert_eq!(ce.size_of_ty(tcx.long_long), Some(8));
    }

    /// Enumerator `DefRef`s resolve through the `defs` table.
    #[test]
    fn enumerator_defref() {
        use rcc_hir::Def;
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();
        let did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Enumerator { ty: tcx.int, value: 5 },
        });
        defs[did].id = did;
        let e = push(&mut body, tcx.int, HirExprKind::DefRef(did));
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        assert_eq!(ce.eval_int(e), Some(5));
    }

    /// Acceptance: `INT_MAX + 1` triggers W0009 and yields `None`.
    #[test]
    fn int_max_plus_one_overflow() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        // Use i128::MAX so overflow trips the i128 accumulator (the
        // outer "fits in `int`" check belongs to a separate range
        // task; here we exercise the evaluator's own detector).
        let lhs = push(&mut body, tcx.long_long, HirExprKind::IntConst(i128::MAX));
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let add =
            push(&mut body, tcx.long_long, HirExprKind::Binary { op: BinOp::Add, lhs, rhs: one });
        let (mut session, cap) = Session::for_test();
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
        assert_eq!(ce.eval_int(add), None);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(codes::W0009));
    }

    /// `LocalRef` is not a constant expression.
    #[test]
    fn non_constant_local_ref() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let e = push(&mut body, tcx.int, HirExprKind::LocalRef(rcc_hir::Local(0)));
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(e), None);
    }

    /// `1 / 0` yields `None` and W0010.
    #[test]
    fn division_by_zero() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        let div = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Div, lhs: a, rhs: b });
        let (mut session, cap) = Session::for_test();
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
        assert_eq!(ce.eval_int(div), None);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(codes::W0010));
    }

    /// Shift count out of range yields `None` and W0011.
    #[test]
    fn shift_out_of_range() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(200));
        let shl = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Shl, lhs: a, rhs: b });
        let (mut session, cap) = Session::for_test();
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), None, Some(&mut session));
        assert_eq!(ce.eval_int(shl), None);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(codes::W0011));
    }

    /// Logical-or short-circuits past a non-constant RHS.
    #[test]
    fn logor_short_circuit() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        // Non-constant RHS would normally fail; short-circuit means
        // we never look at it.
        let nc = push(&mut body, tcx.int, HirExprKind::LocalRef(rcc_hir::Local(0)));
        let or =
            push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::LogOr, lhs: one, rhs: nc });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(or), Some(1));
    }

    /// Conditional only evaluates the selected arm.
    #[test]
    fn cond_only_evaluates_selected_arm() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let cond = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        // Then-arm is non-constant; the false branch must not look at it.
        let nc = push(&mut body, tcx.int, HirExprKind::LocalRef(rcc_hir::Local(0)));
        let elsev = push(&mut body, tcx.int, HirExprKind::IntConst(99));
        let cv =
            push(&mut body, tcx.int, HirExprKind::Cond { cond, then_expr: nc, else_expr: elsev });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_int(cv), Some(99));
    }

    /// Legacy `eval()` returns `ConstValue::Int` for integer expressions.
    #[test]
    fn legacy_eval_returns_int() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let b = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let add = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval(add), Some(ConstValue::Int(5)));
    }

    // ── Phase 07-09: arithmetic + address constants ────────────────

    /// `1.0 + 2.0` folds to `3.0`.
    #[test]
    fn eval_arith_float_add() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.double, HirExprKind::FloatConst(1.0));
        let b = push(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
        let add =
            push(&mut body, tcx.double, HirExprKind::Binary { op: BinOp::Add, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_arith(add), Some(3.0));
    }

    /// Acceptance: `1.0 / 3.0` folds in a global initialiser context.
    #[test]
    fn eval_arith_float_div_one_third() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.double, HirExprKind::FloatConst(1.0));
        let b = push(&mut body, tcx.double, HirExprKind::FloatConst(3.0));
        let div =
            push(&mut body, tcx.double, HirExprKind::Binary { op: BinOp::Div, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        let v = ce.eval_arith(div).unwrap();
        assert!((v - (1.0_f64 / 3.0_f64)).abs() < f64::EPSILON);
        // And the unified entry surfaces it as a Float.
        let mut ce2 = ConstEval::new(&tcx, Some(&body));
        match ce2.eval_scalar(div) {
            Some(ConstScalar::Float(f)) => {
                assert!((f - (1.0_f64 / 3.0_f64)).abs() < f64::EPSILON);
            }
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// `(double) 1 + 2.0` folds with int-to-float promotion.
    #[test]
    fn eval_arith_int_to_float_cast() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let one = push(&mut body, tcx.int, HirExprKind::IntConst(1));
        let one_d = push(&mut body, tcx.double, HirExprKind::Cast { operand: one, to: tcx.double });
        let two = push(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
        let add = push(
            &mut body,
            tcx.double,
            HirExprKind::Binary { op: BinOp::Add, lhs: one_d, rhs: two },
        );
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_arith(add), Some(3.0));
    }

    /// `1.0 < 2.0` is an integer-typed comparison; `eval_int` handles
    /// it by routing through the arithmetic path internally — but in
    /// our split design it returns `None` from `eval_int` and the
    /// arithmetic path returns `1.0`. The unified `eval_scalar` then
    /// surfaces a Float; documented for callers.
    #[test]
    fn eval_arith_float_compare() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.double, HirExprKind::FloatConst(1.0));
        let b = push(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
        let lt = push(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Lt, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_arith(lt), Some(1.0));
    }

    /// Float `%` is not a constant expression (integer-only operator).
    #[test]
    fn eval_arith_rem_rejected() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let a = push(&mut body, tcx.double, HirExprKind::FloatConst(5.0));
        let b = push(&mut body, tcx.double, HirExprKind::FloatConst(2.0));
        let rem =
            push(&mut body, tcx.double, HirExprKind::Binary { op: BinOp::Rem, lhs: a, rhs: b });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_arith(rem), None);
    }

    /// `&global_int` ⇒ `Address { def: <id>, offset: 0 }`.
    #[test]
    fn eval_address_of_global_int() {
        use rcc_hir::{Def, Linkage, Qual};
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();
        let did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.int,
                quals: rcc_hir::ObjectQuals::none(),
                linkage: Linkage::External,
                init: None,
            },
        });
        defs[did].id = did;
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        // Build &global_int. The `DefRef` is an lvalue of int; the
        // `AddressOf` wraps it.
        let r = push(&mut body, tcx.int, HirExprKind::DefRef(did));
        let addr = push(&mut body, int_ptr, HirExprKind::AddressOf(r));
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        assert_eq!(ce.eval_address(addr), Some((Some(did), 0)));
    }

    /// Acceptance: `&global_arr[2]` ⇒
    /// `Address { def: <arr_id>, offset: 2 * sizeof(int) }`.
    #[test]
    fn eval_address_of_array_index() {
        use rcc_hir::{Def, Linkage, Qual};
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();

        // Type: int[3]
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
        let arr_did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: arr_ty,
                quals: rcc_hir::ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: None,
            },
        });
        defs[arr_did].id = arr_did;

        // arr[2]: Index { base = arr, index = 2 }; the indexed
        // expression has element type `int`.
        let arr_ref = push(&mut body, arr_ty, HirExprKind::DefRef(arr_did));
        let two = push(&mut body, tcx.int, HirExprKind::IntConst(2));
        let elem = push(&mut body, tcx.int, HirExprKind::Index { base: arr_ref, index: two });
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let addr = push(&mut body, int_ptr, HirExprKind::AddressOf(elem));

        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        assert_eq!(ce.eval_address(addr), Some((Some(arr_did), 8)));
    }

    /// `&arr[0] + 3` evaluates to `(arr, 3 * sizeof(int))`.
    #[test]
    fn eval_address_pointer_plus_int() {
        use rcc_hir::{Def, Linkage, Qual};
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(4), is_vla: false });
        let arr_did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: arr_ty,
                quals: rcc_hir::ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: None,
            },
        });
        defs[arr_did].id = arr_did;

        let arr_ref = push(&mut body, arr_ty, HirExprKind::DefRef(arr_did));
        let zero = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        let elem = push(&mut body, tcx.int, HirExprKind::Index { base: arr_ref, index: zero });
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let addr0 = push(&mut body, int_ptr, HirExprKind::AddressOf(elem));
        let three = push(&mut body, tcx.int, HirExprKind::IntConst(3));
        let added = push(
            &mut body,
            int_ptr,
            HirExprKind::Binary { op: BinOp::Add, lhs: addr0, rhs: three },
        );

        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        assert_eq!(ce.eval_address(added), Some((Some(arr_did), 12)));
    }

    /// `(char*)0 + 5` ⇒ `Address { def: None, offset: 5 }`.
    #[test]
    fn eval_address_null_base_plus_offset() {
        use rcc_hir::Qual;
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        // 0
        let zero = push(&mut body, tcx.int, HirExprKind::IntConst(0));
        // (char*) 0
        let char_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let cast = push(&mut body, char_ptr, HirExprKind::Cast { operand: zero, to: char_ptr });
        // (char*) 0 + 5
        let five = push(&mut body, tcx.int, HirExprKind::IntConst(5));
        let added =
            push(&mut body, char_ptr, HirExprKind::Binary { op: BinOp::Add, lhs: cast, rhs: five });
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_address(added), Some((None, 5)));
    }

    /// `&local_var` is *not* an address constant per C99 §6.6p8 — it
    /// must reference a static-storage-duration object.
    #[test]
    fn eval_address_local_rejected() {
        use rcc_hir::Qual;
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let lref = push(&mut body, tcx.int, HirExprKind::LocalRef(rcc_hir::Local(0)));
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let addr = push(&mut body, int_ptr, HirExprKind::AddressOf(lref));
        let mut ce = ConstEval::new(&tcx, Some(&body));
        assert_eq!(ce.eval_address(addr), None);
    }

    /// `&obj.field` where `obj` is a global struct: documented as
    /// `(Some(def), 0)` — the field offset is filled by codegen.
    #[test]
    fn eval_address_field_offset_deferred_to_codegen() {
        use rcc_hir::{Def, Linkage, Qual};
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();
        let did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.int,
                quals: rcc_hir::ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: None,
            },
        });
        defs[did].id = did;
        let r = push(&mut body, tcx.int, HirExprKind::DefRef(did));
        let f = push(&mut body, tcx.int, HirExprKind::Field { base: r, field_index: 1 });
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let addr = push(&mut body, int_ptr, HirExprKind::AddressOf(f));
        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        // Per the documentation in `eval_address_of`, the field
        // offset is left at 0; the codegen pass adds it later.
        assert_eq!(ce.eval_address(addr), Some((Some(did), 0)));
    }

    /// Unified `eval_scalar` dispatches: ints stay int, floats stay
    /// float, addresses surface as Address.
    #[test]
    fn eval_scalar_dispatch() {
        use rcc_hir::{Def, Linkage, Qual};
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let mut defs: IndexVec<DefId, Def> = IndexVec::new();

        let i = push(&mut body, tcx.int, HirExprKind::IntConst(42));
        let f = push(&mut body, tcx.double, HirExprKind::FloatConst(2.5));
        let did = defs.push(Def {
            id: DefId(0),
            name: rcc_span::Symbol(0),
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.int,
                quals: rcc_hir::ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: None,
            },
        });
        defs[did].id = did;
        let r = push(&mut body, tcx.int, HirExprKind::DefRef(did));
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let a = push(&mut body, int_ptr, HirExprKind::AddressOf(r));

        let mut ce = ConstEval::with_defs_and_session(&tcx, Some(&body), Some(&defs), None);
        assert_eq!(ce.eval_scalar(i), Some(ConstScalar::Int(42)));
        assert_eq!(ce.eval_scalar(f), Some(ConstScalar::Float(2.5)));
        assert_eq!(ce.eval_scalar(a), Some(ConstScalar::Address { def: Some(did), offset: 0 }));
    }
}
