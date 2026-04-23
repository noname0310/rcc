//! `rcc_typeck`: type checking + implicit conversion insertion.
//!
//! Implements C99 §6.3 (conversions), §6.5 (expression typing), and
//! §6.6 (constant expressions). Mutates the HIR in place by inserting
//! `HirExprKind::Convert { .. }` nodes where an implicit conversion applies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_hir::{FloatKind, HirCrate, IntRank, Ty, TyCtxt, TyId};
use rcc_session::Session;

pub mod const_eval;

pub use const_eval::{ConstEval, ConstValue};

/// Run full type checking over `hir`. After this call every `HirExpr` has a
/// resolved `ty` and every mandatory implicit conversion has been inserted.
///
/// M2 scope: interface only.
pub fn check(_session: &mut Session, _tcx: &mut TyCtxt, _hir: &mut HirCrate) {
    // Implementation in M2-follow-up.
}

/// Integer promotion (C99 §6.3.1.1): any `int`-rank-lower integer type becomes `int`
/// or `unsigned int`, depending on fit. Returns the destination `TyId`.
pub fn integer_promotion(tcx: &TyCtxt, ty: TyId) -> TyId {
    match tcx.get(ty) {
        Ty::Int { rank, .. } if *rank < IntRank::Int => tcx.int,
        _ => ty,
    }
}

/// Usual arithmetic conversions (C99 §6.3.1.8). Returns the common type.
/// Caller is responsible for inserting conversions on both operands.
pub fn usual_arithmetic(tcx: &TyCtxt, a: TyId, b: TyId) -> TyId {
    // Long double dominates, then double, then float.
    match (tcx.get(a), tcx.get(b)) {
        (Ty::Float(FloatKind::F80), _) | (_, Ty::Float(FloatKind::F80)) => tcx.long_double,
        (Ty::Float(FloatKind::F64), _) | (_, Ty::Float(FloatKind::F64)) => tcx.double,
        (Ty::Float(FloatKind::F32), _) | (_, Ty::Float(FloatKind::F32)) => tcx.float,
        _ => {
            // Integer case: promote then pick higher rank; tie-break by signedness.
            let a = integer_promotion(tcx, a);
            let b = integer_promotion(tcx, b);
            match (tcx.get(a), tcx.get(b)) {
                (Ty::Int { signed: sa, rank: ra }, Ty::Int { signed: sb, rank: rb }) => {
                    if ra == rb && sa == sb {
                        a
                    } else if ra > rb {
                        a
                    } else if rb > ra {
                        b
                    } else {
                        // Same rank, different signedness -> unsigned wins.
                        if !sa {
                            a
                        } else {
                            b
                        }
                    }
                }
                _ => a,
            }
        }
    }
}
