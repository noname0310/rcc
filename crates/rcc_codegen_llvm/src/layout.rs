//! ABI / type layout. System V x86-64 baseline.

use rcc_hir::{FloatKind, IntRank, Layout, Ty, TyCtxt, TyId};

/// Layout context. Stateless for now; holds the `TyCtxt` reference so layout
/// queries can recurse through pointers/records.
pub struct LayoutCx<'tcx> {
    /// Backing type context.
    pub tcx: &'tcx TyCtxt,
}

impl<'tcx> LayoutCx<'tcx> {
    /// Build a new layout context.
    pub fn new(tcx: &'tcx TyCtxt) -> Self {
        Self { tcx }
    }

    /// Compute the layout of `ty` on System V x86-64.
    ///
    /// Aggregates (record/array) and VLA are partial pending M3/M4 impl;
    /// scalar sizes are correct.
    pub fn layout_of(&self, ty: TyId) -> Layout {
        match self.tcx.get(ty) {
            Ty::Void => Layout { size: 0, align: 1 },
            Ty::Int { rank, .. } => match rank {
                IntRank::Bool => Layout { size: 1, align: 1 },
                IntRank::Char => Layout { size: 1, align: 1 },
                IntRank::Short => Layout { size: 2, align: 2 },
                IntRank::Int => Layout { size: 4, align: 4 },
                IntRank::Long => Layout { size: 8, align: 8 },
                IntRank::LongLong => Layout { size: 8, align: 8 },
            },
            Ty::Float(k) => match k {
                FloatKind::F32 => Layout { size: 4, align: 4 },
                FloatKind::F64 => Layout { size: 8, align: 8 },
                FloatKind::F80 => Layout { size: 16, align: 16 },
            },
            Ty::Complex(k) => {
                let base = Layout { size: 0, align: 1 };
                match k {
                    FloatKind::F32 => Layout { size: 8, align: 4 },
                    FloatKind::F64 => Layout { size: 16, align: 8 },
                    FloatKind::F80 => Layout { size: 32, align: 16 },
                    #[allow(unreachable_patterns)]
                    _ => base,
                }
            }
            Ty::Ptr(_) | Ty::Func { .. } => Layout { size: 8, align: 8 },
            Ty::Array { elem, len, is_vla } => {
                if *is_vla {
                    Layout { size: 0, align: 1 }
                } else {
                    let per = self.layout_of(elem.ty);
                    Layout { size: per.size * len.unwrap_or(0), align: per.align }
                }
            }
            Ty::Record(_) | Ty::Enum(_) => Layout { size: 0, align: 1 },
            Ty::Error => Layout { size: 0, align: 1 },
        }
    }
}
