//! Constant-expression evaluator (C99 §6.6).
//!
//! Used by:
//! * `rcc_preprocess` to resolve `#if` controlling expressions (integer subset).
//! * `rcc_hir_lower` for enumerator values, array sizes, bitfield widths.
//! * `rcc_typeck` for initializer constness verification.

use rcc_hir::{Body, HirExprId, HirExprKind, TyCtxt};

/// A constant value recognised by the evaluator.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ConstValue {
    /// Integer value (widened to i128 for safety).
    Int(i128),
    /// Floating-point value.
    Float(f64),
}

/// Stateless constant-expression evaluator.
pub struct ConstEval<'tcx, 'body> {
    /// Type context (for conversions / widths).
    pub tcx: &'tcx TyCtxt,
    /// Body being evaluated; lookup for `HirExprId`s lives here.
    pub body: Option<&'body Body>,
}

impl<'tcx, 'body> ConstEval<'tcx, 'body> {
    /// Build an evaluator.
    pub fn new(tcx: &'tcx TyCtxt, body: Option<&'body Body>) -> Self {
        Self { tcx, body }
    }

    /// Evaluate an expression id within `body`.
    pub fn eval(&self, expr: HirExprId) -> Option<ConstValue> {
        let body = self.body?;
        let e = body.exprs.get(expr)?;
        match &e.kind {
            HirExprKind::IntConst(v) => Some(ConstValue::Int(*v)),
            HirExprKind::FloatConst(v) => Some(ConstValue::Float(*v)),
            // Other kinds are evaluated in M2 follow-up.
            _ => None,
        }
    }
}
