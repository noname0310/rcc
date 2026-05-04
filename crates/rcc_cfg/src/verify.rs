//! Release-profile CFG verifier.
//!
//! This module checks invariants that used to live only in debug assertions
//! or integration-test helpers. It is intentionally structural, not a full
//! dataflow/lifetime analysis.

use std::fmt;

use rcc_hir::{DefId, DefKind, HirCrate, Ty, TyCtxt, TyId};

use crate::{
    BasicBlockId, Body, ConstKind, Local, Operand, Place, Projection, Rvalue, StatementKind,
    TerminatorKind,
};

/// One CFG verifier error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfgError {
    /// Where the error was found.
    pub at: CfgLocation,
    /// What went wrong.
    pub kind: CfgErrorKind,
}

/// Location in a CFG body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgLocation {
    /// Whole body.
    Body,
    /// A basic block.
    Block(BasicBlockId),
    /// Statement index in a block.
    Statement { block: BasicBlockId, index: usize },
    /// Terminator in a block.
    Terminator(BasicBlockId),
}

/// Structured verifier error kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgErrorKind {
    /// Body has no entry block.
    EmptyBody,
    /// `ret_ty` does not match the return slot type.
    ReturnSlotTypeMismatch { ret_ty: TyId, slot_ty: TyId },
    /// A reachable block still has the default `Unreachable` terminator.
    ReachableUnreachableTerminator { block: BasicBlockId },
    /// Branch/call/switch target points outside `body.blocks`.
    InvalidBlockTarget { target: BasicBlockId },
    /// A place or storage marker references a nonexistent local.
    InvalidLocal { local: Local },
    /// `switchInt` has no targets.
    EmptySwitchTargets,
    /// The default switch target must be last.
    SwitchDefaultNotLast,
    /// A local is marked dead without any corresponding live marker.
    StorageDeadWithoutLive { local: Local },
    /// A straightforward lexical local has an unmatched live/dead pair.
    UnbalancedStorage { local: Local, live: usize, dead: usize },
    /// A produced value does not match the slot or destination it is stored in.
    TypeMismatch { expected: TyId, actual: TyId },
    /// A place projection is not legal for the type it is applied to.
    InvalidProjection { base_ty: TyId, projection: ProjectionKind },
    /// A call callee did not have a function or pointer-to-function type.
    InvalidCalleeType { callee_ty: TyId },
    /// A `ConstKind::Global` address used a non-address, non-function type.
    InvalidGlobalAddressType { def: DefId, ty: TyId },
    /// A global-object load referenced a non-object definition or mismatched type.
    InvalidGlobalObjectLoad { def: DefId, ty: TyId },
    /// A checked-overflow rvalue used an operation other than add/mul.
    InvalidCheckedOverflowOp { op: crate::BinOp },
    /// A checked-overflow rvalue received a non-integer operand/result type.
    NonIntegerOverflowType { ty: TyId },
}

/// Verifier projection category.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProjectionKind {
    /// File-scope global object base.
    Global(DefId),
    /// Pointer dereference.
    Deref,
    /// Record field index.
    Field(u32),
    /// Array or pointer index.
    Index,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum InferredTy {
    Known(TyId),
    AddressOf { pointee: TyId },
    VoidPtr,
}

impl fmt::Display for CfgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.at, self.kind)
    }
}

impl std::error::Error for CfgError {}

/// Verify a CFG body.
///
/// The storage check is deliberately conservative: it catches simple lexical
/// mismatches such as missing `StorageDead` or dead-without-live, but it does
/// not attempt path-sensitive lifetime validation through arbitrary branches.
pub fn verify_body(body: &Body, tcx: &TyCtxt) -> Result<(), Vec<CfgError>> {
    verify_body_inner(body, tcx, None)
}

/// Verify a CFG body with access to the owning HIR crate.
///
/// The extra HIR context lets the verifier validate record field projections,
/// whose field count and field types are intentionally not duplicated in CFG.
pub fn verify_body_with_hir(
    body: &Body,
    tcx: &TyCtxt,
    hir: &HirCrate,
) -> Result<(), Vec<CfgError>> {
    verify_body_inner(body, tcx, Some(hir))
}

fn verify_body_inner(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
) -> Result<(), Vec<CfgError>> {
    let mut errors = Vec::new();
    if body.blocks.is_empty() {
        errors.push(CfgError { at: CfgLocation::Body, kind: CfgErrorKind::EmptyBody });
        return Err(errors);
    }

    verify_return_slot(body, &mut errors);
    let reachable = reachable_blocks(body, &mut errors);
    verify_blocks(body, tcx, hir, &reachable, &mut errors);
    verify_storage(body, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn verify_return_slot(body: &Body, errors: &mut Vec<CfgError>) {
    let Some(ret_ty) = body.ret_ty else { return };
    let Some(slot) = body.locals.get(Local(0)) else {
        errors.push(CfgError {
            at: CfgLocation::Body,
            kind: CfgErrorKind::InvalidLocal { local: Local(0) },
        });
        return;
    };
    if slot.ty != ret_ty {
        errors.push(CfgError {
            at: CfgLocation::Body,
            kind: CfgErrorKind::ReturnSlotTypeMismatch { ret_ty, slot_ty: slot.ty },
        });
    }
}

fn verify_blocks(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    reachable: &[bool],
    errors: &mut Vec<CfgError>,
) {
    for (bb, block) in body.blocks.iter_enumerated() {
        for (index, stmt) in block.statements.iter().enumerate() {
            let at = CfgLocation::Statement { block: bb, index };
            match &stmt.kind {
                StatementKind::Assign { place, rvalue } => {
                    let dst = verify_place_typed(body, tcx, hir, place, at.clone(), errors);
                    let src = verify_rvalue_typed(body, tcx, hir, rvalue, at.clone(), errors);
                    if let (Some(dst), Some(src)) = (dst, src) {
                        verify_type_match(tcx, dst, src, at, errors);
                    }
                }
                StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                    verify_local(body, *local, at, errors);
                }
                StatementKind::Nop => {}
            }
        }

        let at = CfgLocation::Terminator(bb);
        match &block.terminator.kind {
            TerminatorKind::Goto(target) => verify_block_target(body, *target, at, errors),
            TerminatorKind::IndirectGoto { target, targets } => {
                let _ = verify_operand_typed(body, tcx, hir, target, at.clone(), errors);
                for target in targets {
                    verify_block_target(body, *target, at.clone(), errors);
                }
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                let _ = verify_operand_typed(body, tcx, hir, discr, at.clone(), errors);
                if targets.is_empty() {
                    errors
                        .push(CfgError { at: at.clone(), kind: CfgErrorKind::EmptySwitchTargets });
                }
                if !targets.last().is_some_and(|(value, _)| value.is_none()) {
                    errors.push(CfgError {
                        at: at.clone(),
                        kind: CfgErrorKind::SwitchDefaultNotLast,
                    });
                }
                for (_, target) in targets {
                    verify_block_target(body, *target, at.clone(), errors);
                }
            }
            TerminatorKind::Call { callee, args, destination, target } => {
                let callee_ty = verify_operand_typed(body, tcx, hir, callee, at.clone(), errors);
                for arg in args {
                    let _ = verify_operand_typed(body, tcx, hir, arg, at.clone(), errors);
                }
                let dest_ty = destination
                    .as_ref()
                    .and_then(|dest| verify_place_typed(body, tcx, hir, dest, at.clone(), errors));
                if let Some(callee_ty) = callee_ty {
                    if let Some(ret_ty) = callee_return_ty(tcx, callee_ty) {
                        if let Some(dest_ty) = dest_ty {
                            verify_type_match(
                                tcx,
                                ret_ty,
                                InferredTy::Known(dest_ty),
                                at.clone(),
                                errors,
                            );
                        }
                    } else {
                        errors.push(CfgError {
                            at: at.clone(),
                            kind: CfgErrorKind::InvalidCalleeType { callee_ty },
                        });
                    }
                }
                if let Some(target) = target {
                    verify_block_target(body, *target, at, errors);
                }
            }
            TerminatorKind::Return => {}
            TerminatorKind::Unreachable => {
                if reachable.get(bb.0 as usize).copied().unwrap_or(false) {
                    errors.push(CfgError {
                        at,
                        kind: CfgErrorKind::ReachableUnreachableTerminator { block: bb },
                    });
                }
            }
            TerminatorKind::BuiltinVaStart { target, .. }
            | TerminatorKind::BuiltinVaEnd { target, .. }
            | TerminatorKind::BuiltinVaCopy { target, .. } => {
                verify_block_target(body, *target, at, errors);
            }
        }
    }
}

fn verify_rvalue_typed(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    rvalue: &Rvalue,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) -> Option<InferredTy> {
    match rvalue {
        Rvalue::Use(op) => {
            verify_operand_typed(body, tcx, hir, op, at, errors).map(InferredTy::Known)
        }
        Rvalue::UnaryOp(op, operand) => {
            let operand_ty = verify_operand_typed(body, tcx, hir, operand, at, errors)?;
            match op {
                crate::UnOp::LogNot => Some(InferredTy::Known(tcx.int)),
                crate::UnOp::Neg | crate::UnOp::FNeg | crate::UnOp::BitNot => {
                    Some(InferredTy::Known(operand_ty))
                }
            }
        }
        Rvalue::Cast { op, to, .. } => {
            let _ = verify_operand_typed(body, tcx, hir, op, at, errors);
            Some(InferredTy::Known(*to))
        }
        Rvalue::ComplexFromReal { real, to } => {
            let _ = verify_operand_typed(body, tcx, hir, real, at, errors);
            Some(InferredTy::Known(*to))
        }
        Rvalue::RealFromComplex { complex, to } => {
            let _ = verify_operand_typed(body, tcx, hir, complex, at, errors);
            Some(InferredTy::Known(*to))
        }
        Rvalue::BitfieldPrecision { op, to, .. } => {
            let _ = verify_operand_typed(body, tcx, hir, op, at, errors);
            Some(InferredTy::Known(*to))
        }
        Rvalue::VectorInit { ty, lanes } => {
            let elem_ty = match tcx.get(*ty) {
                Ty::Vector { elem, lanes: expected_lanes, .. } => {
                    if lanes.len() != *expected_lanes as usize {
                        errors.push(CfgError {
                            at: at.clone(),
                            kind: CfgErrorKind::TypeMismatch { expected: *ty, actual: *ty },
                        });
                    }
                    *elem
                }
                _ => {
                    errors.push(CfgError {
                        at: at.clone(),
                        kind: CfgErrorKind::TypeMismatch { expected: *ty, actual: *ty },
                    });
                    return Some(InferredTy::Known(*ty));
                }
            };
            for lane in lanes {
                if let Some(actual) = verify_operand_typed(body, tcx, hir, lane, at.clone(), errors)
                {
                    if actual != elem_ty {
                        errors.push(CfgError {
                            at: at.clone(),
                            kind: CfgErrorKind::TypeMismatch { expected: elem_ty, actual },
                        });
                    }
                }
            }
            Some(InferredTy::Known(*ty))
        }
        Rvalue::BinaryOp(op, lhs, rhs) => {
            let lhs_ty = verify_operand_typed(body, tcx, hir, lhs, at.clone(), errors)?;
            let rhs_ty = verify_operand_typed(body, tcx, hir, rhs, at, errors)?;
            Some(InferredTy::Known(binary_result_ty(tcx, *op, lhs_ty, rhs_ty)))
        }
        Rvalue::AddressOf(place) => {
            let pointee = verify_place_typed(body, tcx, hir, place, at.clone(), errors)?;
            Some(InferredTy::AddressOf { pointee })
        }
        Rvalue::LoadGlobal { def, ty } => {
            verify_global_object_load(tcx, hir, *def, *ty, at, errors);
            Some(InferredTy::Known(*ty))
        }
        Rvalue::Len(place) => {
            let _ = verify_place_typed(body, tcx, hir, place, at, errors);
            Some(InferredTy::Known(tcx.ulong))
        }
        Rvalue::BuiltinVaArg { ap, ty } => {
            let _ = verify_operand_typed(body, tcx, hir, ap, at, errors);
            Some(InferredTy::Known(*ty))
        }
        Rvalue::CheckedOverflow { op, lhs, rhs, dst, ty } => {
            if !matches!(op, crate::BinOp::Add | crate::BinOp::Mul) {
                errors.push(CfgError {
                    at: at.clone(),
                    kind: CfgErrorKind::InvalidCheckedOverflowOp { op: *op },
                });
            }
            let lhs_ty = verify_operand_typed(body, tcx, hir, lhs, at.clone(), errors);
            let rhs_ty = verify_operand_typed(body, tcx, hir, rhs, at.clone(), errors);
            if !is_integer_ty(tcx, *ty) {
                errors.push(CfgError {
                    at: at.clone(),
                    kind: CfgErrorKind::NonIntegerOverflowType { ty: *ty },
                });
            }
            if let Some(lhs_ty) = lhs_ty {
                verify_integer_operand(tcx, lhs_ty, at.clone(), errors);
            }
            if let Some(rhs_ty) = rhs_ty {
                verify_integer_operand(tcx, rhs_ty, at.clone(), errors);
            }
            if let Some(dst) = dst {
                let Some(dst_ty) = verify_operand_typed(body, tcx, hir, dst, at.clone(), errors)
                else {
                    return Some(InferredTy::Known(tcx.int));
                };
                match tcx.get(dst_ty) {
                    Ty::Ptr(q) if q.ty == *ty => {}
                    _ => errors.push(CfgError {
                        at: at.clone(),
                        kind: CfgErrorKind::TypeMismatch { expected: *ty, actual: dst_ty },
                    }),
                }
            }
            Some(InferredTy::Known(tcx.int))
        }
        Rvalue::BuiltinVaArea => Some(InferredTy::VoidPtr),
    }
}

fn verify_operand_typed(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    operand: &Operand,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) -> Option<TyId> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            verify_place_typed(body, tcx, hir, place, at, errors)
        }
        Operand::Const(c) => {
            if let ConstKind::Global(def) = c.kind {
                verify_global_address_type(tcx, def, c.ty, at.clone(), errors);
            }
            if let ConstKind::BlockAddress(target) = c.kind {
                verify_block_target(body, target, at, errors);
            }
            Some(c.ty)
        }
    }
}

fn verify_integer_operand(tcx: &TyCtxt, ty: TyId, at: CfgLocation, errors: &mut Vec<CfgError>) {
    if !is_integer_ty(tcx, ty) {
        errors.push(CfgError { at, kind: CfgErrorKind::NonIntegerOverflowType { ty } });
    }
}

fn is_integer_ty(tcx: &TyCtxt, ty: TyId) -> bool {
    matches!(tcx.get(ty), Ty::Int { .. } | Ty::Enum(_))
}

fn verify_global_address_type(
    tcx: &TyCtxt,
    def: DefId,
    ty: TyId,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) {
    if matches!(tcx.get(ty), Ty::Ptr(_) | Ty::Func { .. }) {
        return;
    }
    errors.push(CfgError { at, kind: CfgErrorKind::InvalidGlobalAddressType { def, ty } });
}

fn verify_global_object_load(
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    def: DefId,
    ty: TyId,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) {
    let Some(hir) = hir else {
        return;
    };
    let Some(def_data) = hir.defs.get(def) else {
        errors.push(CfgError { at, kind: CfgErrorKind::InvalidGlobalObjectLoad { def, ty } });
        return;
    };
    let DefKind::Global { ty: global_ty, .. } = &def_data.kind else {
        errors.push(CfgError { at, kind: CfgErrorKind::InvalidGlobalObjectLoad { def, ty } });
        return;
    };
    if *global_ty != ty || matches!(tcx.get(ty), Ty::Func { .. }) {
        errors.push(CfgError { at, kind: CfgErrorKind::InvalidGlobalObjectLoad { def, ty } });
    }
}

fn verify_place_typed(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    place: &Place,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) -> Option<TyId> {
    let mut projections = place.projection.iter();
    let mut ty = match projections.next() {
        Some(Projection::Global(def)) => {
            let Some(hir) = hir else {
                errors.push(CfgError {
                    at: at.clone(),
                    kind: CfgErrorKind::InvalidGlobalObjectLoad { def: *def, ty: TyId(0) },
                });
                return None;
            };
            let Some(def_data) = hir.defs.get(*def) else {
                errors.push(CfgError {
                    at: at.clone(),
                    kind: CfgErrorKind::InvalidGlobalObjectLoad { def: *def, ty: TyId(0) },
                });
                return None;
            };
            let DefKind::Global { ty, .. } = &def_data.kind else {
                errors.push(CfgError {
                    at: at.clone(),
                    kind: CfgErrorKind::InvalidGlobalObjectLoad { def: *def, ty: TyId(0) },
                });
                return None;
            };
            *ty
        }
        Some(first) => {
            verify_local(body, place.base, at.clone(), errors);
            let mut ty = body.locals.get(place.base).map(|decl| decl.ty)?;
            ty = verify_projection_step(body, tcx, hir, ty, first, at.clone(), errors)?;
            ty
        }
        None => {
            verify_local(body, place.base, at.clone(), errors);
            body.locals.get(place.base).map(|decl| decl.ty)?
        }
    };
    for projection in projections {
        if let Projection::Global(def) = projection {
            errors.push(CfgError {
                at: at.clone(),
                kind: CfgErrorKind::InvalidProjection {
                    base_ty: ty,
                    projection: ProjectionKind::Global(*def),
                },
            });
            return None;
        }
        ty = verify_projection_step(body, tcx, hir, ty, projection, at.clone(), errors)?;
    }
    Some(ty)
}

fn verify_projection_step(
    body: &Body,
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    mut ty: TyId,
    projection: &Projection,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) -> Option<TyId> {
    match projection {
        Projection::Global(def) => {
            errors.push(CfgError {
                at,
                kind: CfgErrorKind::InvalidProjection {
                    base_ty: ty,
                    projection: ProjectionKind::Global(*def),
                },
            });
            return None;
        }
        Projection::Deref => match tcx.get(ty) {
            Ty::Ptr(q) => ty = q.ty,
            _ => {
                errors.push(CfgError {
                    at,
                    kind: CfgErrorKind::InvalidProjection {
                        base_ty: ty,
                        projection: ProjectionKind::Deref,
                    },
                });
                return None;
            }
        },
        Projection::Field(index) => {
            ty = verify_field_projection(tcx, hir, ty, *index, at.clone(), errors)?;
        }
        Projection::Index(index) => {
            let _ = verify_operand_typed(body, tcx, hir, index, at.clone(), errors);
            match tcx.get(ty) {
                Ty::Array { elem, .. } => ty = elem.ty,
                Ty::Ptr(q) => ty = q.ty,
                _ => {
                    errors.push(CfgError {
                        at,
                        kind: CfgErrorKind::InvalidProjection {
                            base_ty: ty,
                            projection: ProjectionKind::Index,
                        },
                    });
                    return None;
                }
            }
        }
    }
    Some(ty)
}

fn verify_field_projection(
    tcx: &TyCtxt,
    hir: Option<&HirCrate>,
    base_ty: TyId,
    index: u32,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) -> Option<TyId> {
    let Ty::Record(def_id) = *tcx.get(base_ty) else {
        errors.push(CfgError {
            at,
            kind: CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(index),
            },
        });
        return None;
    };

    let Some(hir) = hir else {
        errors.push(CfgError {
            at,
            kind: CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(index),
            },
        });
        return None;
    };

    let Some(def) = hir.defs.get(def_id) else {
        errors.push(CfgError {
            at,
            kind: CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(index),
            },
        });
        return None;
    };

    let DefKind::Record { fields, .. } = &def.kind else {
        errors.push(CfgError {
            at,
            kind: CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(index),
            },
        });
        return None;
    };

    let Some(field) = fields.get(index as usize) else {
        errors.push(CfgError {
            at,
            kind: CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(index),
            },
        });
        return None;
    };
    Some(field.ty)
}

fn verify_type_match(
    tcx: &TyCtxt,
    expected: TyId,
    actual: InferredTy,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) {
    match actual {
        InferredTy::Known(actual) => {
            if actual != expected {
                errors.push(CfgError { at, kind: CfgErrorKind::TypeMismatch { expected, actual } });
            }
        }
        InferredTy::AddressOf { pointee } => match tcx.get(expected) {
            Ty::Ptr(q) if q.ty == pointee => {}
            _ => {
                errors.push(CfgError {
                    at,
                    kind: CfgErrorKind::TypeMismatch { expected, actual: pointee },
                });
            }
        },
        InferredTy::VoidPtr => match tcx.get(expected) {
            Ty::Ptr(q) if q.ty == tcx.void => {}
            _ => {
                errors.push(CfgError {
                    at,
                    kind: CfgErrorKind::TypeMismatch { expected, actual: tcx.void },
                });
            }
        },
    }
}

fn binary_result_ty(tcx: &TyCtxt, op: crate::BinOp, lhs_ty: TyId, rhs_ty: TyId) -> TyId {
    match op {
        crate::BinOp::Eq
        | crate::BinOp::Ne
        | crate::BinOp::SLt
        | crate::BinOp::SLe
        | crate::BinOp::SGt
        | crate::BinOp::SGe
        | crate::BinOp::ULt
        | crate::BinOp::ULe
        | crate::BinOp::UGt
        | crate::BinOp::UGe
        | crate::BinOp::FLt
        | crate::BinOp::FLe
        | crate::BinOp::FGt
        | crate::BinOp::FGe => tcx.int,
        crate::BinOp::PtrDiff => tcx.long,
        crate::BinOp::PtrAdd | crate::BinOp::PtrSub => {
            if matches!(tcx.get(lhs_ty), Ty::Ptr(_)) {
                lhs_ty
            } else {
                rhs_ty
            }
        }
        crate::BinOp::Add
        | crate::BinOp::Sub
        | crate::BinOp::Mul
        | crate::BinOp::SDiv
        | crate::BinOp::UDiv
        | crate::BinOp::SRem
        | crate::BinOp::URem
        | crate::BinOp::FDiv
        | crate::BinOp::Shl
        | crate::BinOp::AShr
        | crate::BinOp::LShr
        | crate::BinOp::BitAnd
        | crate::BinOp::BitXor
        | crate::BinOp::BitOr
        | crate::BinOp::FAdd
        | crate::BinOp::FSub
        | crate::BinOp::FMul => lhs_ty,
    }
}

fn callee_return_ty(tcx: &TyCtxt, callee_ty: TyId) -> Option<TyId> {
    match tcx.get(callee_ty) {
        Ty::Func { ret, .. } => Some(*ret),
        Ty::Ptr(q) => match tcx.get(q.ty) {
            Ty::Func { ret, .. } => Some(*ret),
            _ => None,
        },
        _ => None,
    }
}

fn verify_local(body: &Body, local: Local, at: CfgLocation, errors: &mut Vec<CfgError>) {
    if (local.0 as usize) >= body.locals.len() {
        errors.push(CfgError { at, kind: CfgErrorKind::InvalidLocal { local } });
    }
}

fn verify_block_target(
    body: &Body,
    target: BasicBlockId,
    at: CfgLocation,
    errors: &mut Vec<CfgError>,
) {
    if (target.0 as usize) >= body.blocks.len() {
        errors.push(CfgError { at, kind: CfgErrorKind::InvalidBlockTarget { target } });
    }
}

fn reachable_blocks(body: &Body, errors: &mut Vec<CfgError>) -> Vec<bool> {
    let mut reachable = vec![false; body.blocks.len()];
    let mut stack = vec![BasicBlockId(0)];
    while let Some(bb) = stack.pop() {
        let idx = bb.0 as usize;
        if idx >= body.blocks.len() {
            errors.push(CfgError {
                at: CfgLocation::Body,
                kind: CfgErrorKind::InvalidBlockTarget { target: bb },
            });
            continue;
        }
        if reachable[idx] {
            continue;
        }
        reachable[idx] = true;
        for succ in successors(&body.blocks[bb].terminator.kind) {
            stack.push(succ);
        }
    }
    reachable
}

fn successors(term: &TerminatorKind) -> Vec<BasicBlockId> {
    match term {
        TerminatorKind::Goto(target) => vec![*target],
        TerminatorKind::IndirectGoto { targets, .. } => targets.clone(),
        TerminatorKind::SwitchInt { targets, .. } => {
            targets.iter().map(|(_, target)| *target).collect()
        }
        TerminatorKind::Call { target: Some(target), .. } => vec![*target],
        TerminatorKind::Call { target: None, .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
        TerminatorKind::BuiltinVaStart { target, .. }
        | TerminatorKind::BuiltinVaEnd { target, .. }
        | TerminatorKind::BuiltinVaCopy { target, .. } => vec![*target],
    }
}

fn verify_storage(body: &Body, errors: &mut Vec<CfgError>) {
    let mut live = vec![0usize; body.locals.len()];
    let mut dead = vec![0usize; body.locals.len()];
    for (_, block) in body.blocks.iter_enumerated() {
        for stmt in &block.statements {
            match stmt.kind {
                StatementKind::StorageLive(local) if (local.0 as usize) < live.len() => {
                    live[local.0 as usize] += 1;
                }
                StatementKind::StorageDead(local) if (local.0 as usize) < dead.len() => {
                    dead[local.0 as usize] += 1;
                }
                _ => {}
            }
        }
    }

    for (local, decl) in body.locals.iter_enumerated() {
        if local == Local(0) || decl.is_param || decl.name.is_none() {
            continue;
        }
        let idx = local.0 as usize;
        let (live_count, dead_count) = (live[idx], dead[idx]);
        if live_count == 0 && dead_count > 0 {
            errors.push(CfgError {
                at: CfgLocation::Body,
                kind: CfgErrorKind::StorageDeadWithoutLive { local },
            });
        }
        if live_count == 0 && dead_count == 0 {
            continue;
        }
        if live_count <= 1 && dead_count <= 1 && live_count != dead_count {
            errors.push(CfgError {
                at: CfgLocation::Body,
                kind: CfgErrorKind::UnbalancedStorage { local, live: live_count, dead: dead_count },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use rcc_data_structures::IndexVec;
    use rcc_hir::{
        Def, DefId, DefKind, Field, HirCrate, ObjectQuals, Qual, RecordKind, Ty, TyCtxt, TyId,
    };
    use rcc_span::{Symbol, DUMMY_SP};

    use super::*;
    use crate::{BasicBlock, Const, ConstKind, LocalDecl, Statement, Terminator, TerminatorKind};

    fn ty() -> TyId {
        TyId(0)
    }

    fn tcx() -> TyCtxt {
        let mut tcx = TyCtxt::new();
        let _ = tcx.intern(Ty::Int { signed: true, rank: rcc_hir::IntRank::Int });
        tcx
    }

    fn local_decl(name: bool) -> LocalDecl {
        local_decl_with_ty(ty(), name)
    }

    fn local_decl_with_ty(ty: TyId, name: bool) -> LocalDecl {
        LocalDecl {
            name: name.then_some(rcc_span::Symbol(1)),
            ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        }
    }

    fn base_body() -> Body {
        body_with_return_ty(ty())
    }

    fn body_with_return_ty(ret_ty: TyId) -> Body {
        let mut locals: IndexVec<Local, LocalDecl> = IndexVec::new();
        locals.push(local_decl_with_ty(ret_ty, false));
        let mut blocks: IndexVec<BasicBlockId, BasicBlock> = IndexVec::new();
        blocks.push(BasicBlock {
            statements: Vec::new(),
            terminator: Terminator { kind: TerminatorKind::Return, span: DUMMY_SP },
        });
        Body {
            def: None,
            locals,
            blocks,
            labels: rcc_data_structures::FxHashMap::default(),
            ret_ty: Some(ret_ty),
        }
    }

    fn record_hir(record: DefId, fields: Vec<TyId>) -> HirCrate {
        let mut hir = HirCrate::default();
        hir.defs.push(Def {
            id: record,
            name: Symbol(1),
            span: DUMMY_SP,
            kind: DefKind::Record {
                kind: RecordKind::Struct,
                align_override: None,
                layout: None,
                fields: fields
                    .into_iter()
                    .enumerate()
                    .map(|(i, ty)| Field {
                        name: Some(Symbol((i + 2) as u32)),
                        ty,
                        quals: ObjectQuals::none(),
                        align_override: None,
                        offset: None,
                        bit_width: None,
                        span: DUMMY_SP,
                    })
                    .collect(),
            },
        });
        hir
    }

    #[test]
    fn verify_accepts_trivial_body() {
        assert!(verify_body(&base_body(), &tcx()).is_ok());
    }

    #[test]
    fn verify_rejects_reachable_default_unreachable() {
        let mut body = base_body();
        body.blocks[BasicBlockId(0)].terminator.kind = TerminatorKind::Unreachable;
        let errors = verify_body(&body, &tcx()).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::ReachableUnreachableTerminator { block: BasicBlockId(0) }
        )));
    }

    #[test]
    fn verify_reports_invalid_block_target() {
        let mut body = base_body();
        body.blocks[BasicBlockId(0)].terminator.kind = TerminatorKind::Goto(BasicBlockId(99));
        let errors = verify_body(&body, &tcx()).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::InvalidBlockTarget { target: BasicBlockId(99) }
        )));
    }

    #[test]
    fn verify_reports_invalid_local_in_place() {
        let mut body = base_body();
        body.blocks[BasicBlockId(0)].statements.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: Local(99), projection: Vec::new() },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(0), ty: ty() })),
            },
            span: DUMMY_SP,
        });
        let errors = verify_body(&body, &tcx()).unwrap_err();
        assert!(errors
            .iter()
            .any(|err| matches!(err.kind, CfgErrorKind::InvalidLocal { local: Local(99) })));
    }

    #[test]
    fn verify_reports_dead_without_live() {
        let mut body = base_body();
        body.locals.push(local_decl(true));
        body.blocks[BasicBlockId(0)]
            .statements
            .push(Statement { kind: StatementKind::StorageDead(Local(1)), span: DUMMY_SP });
        let errors = verify_body(&body, &tcx()).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::StorageDeadWithoutLive { local: Local(1) }
        )));
    }

    #[test]
    fn verify_reports_unbalanced_straightforward_storage() {
        let mut body = base_body();
        body.locals.push(local_decl(true));
        body.blocks[BasicBlockId(0)]
            .statements
            .push(Statement { kind: StatementKind::StorageLive(Local(1)), span: DUMMY_SP });
        let errors = verify_body(&body, &tcx()).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::UnbalancedStorage { local: Local(1), live: 1, dead: 0 }
        )));
    }

    #[test]
    fn verify_accepts_storage_live_in_later_block_order() {
        let mut body = base_body();
        body.locals.push(local_decl(true));
        body.blocks[BasicBlockId(0)].terminator.kind = TerminatorKind::Goto(BasicBlockId(2));
        body.blocks.push(BasicBlock {
            statements: vec![Statement {
                kind: StatementKind::StorageDead(Local(1)),
                span: DUMMY_SP,
            }],
            terminator: Terminator { kind: TerminatorKind::Return, span: DUMMY_SP },
        });
        body.blocks.push(BasicBlock {
            statements: vec![Statement {
                kind: StatementKind::StorageLive(Local(1)),
                span: DUMMY_SP,
            }],
            terminator: Terminator { kind: TerminatorKind::Goto(BasicBlockId(1)), span: DUMMY_SP },
        });

        assert!(verify_body(&body, &tcx()).is_ok());
    }

    #[test]
    fn verify_reports_bad_return_slot_type() {
        let tcx = TyCtxt::new();
        let mut body = body_with_return_ty(tcx.int);
        body.locals[Local(0)].ty = tcx.double;

        let errors = verify_body(&body, &tcx).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::ReturnSlotTypeMismatch { ret_ty, slot_ty }
                if ret_ty == tcx.int && slot_ty == tcx.double
        )));
    }

    #[test]
    fn verify_reports_bad_assignment_type() {
        let tcx = TyCtxt::new();
        let mut body = body_with_return_ty(tcx.int);
        body.locals.push(local_decl_with_ty(tcx.int, true));
        body.blocks[BasicBlockId(0)].statements.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: Local(1), projection: Vec::new() },
                rvalue: Rvalue::Use(Operand::Const(Const {
                    kind: ConstKind::Float(1.0),
                    ty: tcx.double,
                })),
            },
            span: DUMMY_SP,
        });

        let errors = verify_body(&body, &tcx).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::TypeMismatch { expected, actual }
                if expected == tcx.int && actual == tcx.double
        )));
    }

    #[test]
    fn verify_reports_invalid_field_index() {
        let mut tcx = TyCtxt::new();
        let record = DefId(0);
        let rec_ty = tcx.intern(Ty::Record(record));
        let hir = record_hir(record, vec![tcx.int, tcx.int]);
        let mut body = body_with_return_ty(tcx.int);
        body.locals.push(local_decl_with_ty(rec_ty, true));
        body.blocks[BasicBlockId(0)].statements.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: Local(1), projection: vec![Projection::Field(2)] },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(1), ty: tcx.int })),
            },
            span: DUMMY_SP,
        });

        let errors = verify_body_with_hir(&body, &tcx, &hir).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Field(2)
            } if base_ty == rec_ty
        )));
    }

    #[test]
    fn verify_reports_invalid_index_projection() {
        let tcx = TyCtxt::new();
        let mut body = body_with_return_ty(tcx.int);
        body.locals.push(local_decl_with_ty(tcx.int, true));
        body.blocks[BasicBlockId(0)].statements.push(Statement {
            kind: StatementKind::Assign {
                place: Place {
                    base: Local(1),
                    projection: vec![Projection::Index(Operand::Const(Const {
                        kind: ConstKind::Int(0),
                        ty: tcx.int,
                    }))],
                },
                rvalue: Rvalue::Use(Operand::Const(Const { kind: ConstKind::Int(1), ty: tcx.int })),
            },
            span: DUMMY_SP,
        });

        let errors = verify_body(&body, &tcx).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::InvalidProjection {
                base_ty,
                projection: ProjectionKind::Index
            } if base_ty == tcx.int
        )));
    }

    #[test]
    fn verify_reports_call_destination_type_mismatch() {
        let mut tcx = TyCtxt::new();
        let fn_ty = tcx.intern(Ty::Func {
            ret: tcx.double,
            params: Vec::new(),
            variadic: false,
            proto: true,
        });
        let fn_ptr = tcx.intern(Ty::Ptr(Qual::plain(fn_ty)));
        let mut body = body_with_return_ty(tcx.int);
        body.locals.push(local_decl_with_ty(fn_ptr, true));
        body.locals.push(local_decl_with_ty(tcx.int, true));
        body.blocks[BasicBlockId(0)].terminator.kind = TerminatorKind::Call {
            callee: Operand::Copy(Place { base: Local(1), projection: Vec::new() }),
            args: Vec::new(),
            destination: Some(Place { base: Local(2), projection: Vec::new() }),
            target: None,
        };

        let errors = verify_body(&body, &tcx).unwrap_err();
        assert!(errors.iter().any(|err| matches!(
            err.kind,
            CfgErrorKind::TypeMismatch { expected, actual }
                if expected == tcx.double && actual == tcx.int
        )));
    }
}
