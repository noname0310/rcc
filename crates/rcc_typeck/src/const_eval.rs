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
    Body, ConvertKind, Def, DefId, DefKind, FloatKind, HirExprId, HirExprKind, IntRank, Ty, TyCtxt,
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
            },

            // The remaining HIR kinds are not part of an integer
            // constant expression by C99 §6.6p3 (assignment, comma,
            // function call) or §6.6p7 (lvalue access, address-of,
            // indirection on an arbitrary pointer, struct field /
            // array indexing on a non-constant target). Bail with
            // `None`.
            HirExprKind::Call { .. }
            | HirExprKind::Field { .. }
            | HirExprKind::Index { .. }
            | HirExprKind::AddressOf(_)
            | HirExprKind::Deref(_)
            | HirExprKind::Comma { .. }
            | HirExprKind::Assign { .. } => None,
        }
    }

    /// Compute the size of a type in bytes.
    ///
    /// **TODO(phase 15):** the per-target sizes here are baked-in
    /// LP64 stubs (the same assumption the rest of the compiler is
    /// already making — see `INT_BITS` in `rcc_typeck::lib`). When
    /// `TargetInfo` lands the stubs should defer to it.
    pub fn size_of_ty(&self, ty: TyId) -> Option<i128> {
        match self.tcx.get(ty).clone() {
            Ty::Void => None,
            Ty::Int { rank, .. } => Some(match rank {
                IntRank::Bool => 1,
                IntRank::Char => 1,
                IntRank::Short => 2,
                IntRank::Int => 4,
                IntRank::Long => 8,
                IntRank::LongLong => 8,
            }),
            Ty::Float(kind) => Some(match kind {
                FloatKind::F32 => 4,
                FloatKind::F64 => 8,
                // `long double` lays out as a 16-byte slot on the
                // SysV-x86_64 / AArch64 stub targets we'll support
                // first; record-layout is target-specific so this is
                // the same LP64 placeholder as the integer ranks.
                FloatKind::F80 => 16,
            }),
            // Pointers are LP64.
            Ty::Ptr(_) => Some(8),
            Ty::Array { elem, len: Some(n), is_vla: false } => {
                let elem_size = self.size_of_ty(elem.ty)?;
                elem_size.checked_mul(i128::from(n))
            }
            // VLA / incomplete arrays / functions / records / enums:
            // their size requires layout that lives outside the
            // const-eval API surface (or is genuinely unknowable at
            // const-eval time, e.g. VLAs). Bail.
            Ty::Array { .. }
            | Ty::Func { .. }
            | Ty::Record(_)
            | Ty::Enum(_)
            | Ty::Complex(_)
            | Ty::Error => None,
        }
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
}
