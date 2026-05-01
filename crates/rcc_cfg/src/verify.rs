//! Release-profile CFG verifier.
//!
//! This module checks invariants that used to live only in debug assertions
//! or integration-test helpers. It is intentionally structural, not a full
//! dataflow/lifetime analysis.

use std::fmt;

use rcc_hir::{TyCtxt, TyId};

use crate::{
    BasicBlockId, Body, Local, Operand, Place, Projection, Rvalue, StatementKind, TerminatorKind,
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
pub fn verify_body(body: &Body, _tcx: &TyCtxt) -> Result<(), Vec<CfgError>> {
    let mut errors = Vec::new();
    if body.blocks.is_empty() {
        errors.push(CfgError { at: CfgLocation::Body, kind: CfgErrorKind::EmptyBody });
        return Err(errors);
    }

    verify_return_slot(body, &mut errors);
    let reachable = reachable_blocks(body, &mut errors);
    verify_blocks(body, &reachable, &mut errors);
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

fn verify_blocks(body: &Body, reachable: &[bool], errors: &mut Vec<CfgError>) {
    for (bb, block) in body.blocks.iter_enumerated() {
        for (index, stmt) in block.statements.iter().enumerate() {
            let at = CfgLocation::Statement { block: bb, index };
            match &stmt.kind {
                StatementKind::Assign { place, rvalue } => {
                    verify_place(body, place, at.clone(), errors);
                    verify_rvalue(body, rvalue, at, errors);
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
            TerminatorKind::SwitchInt { discr, targets } => {
                verify_operand(body, discr, at.clone(), errors);
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
                verify_operand(body, callee, at.clone(), errors);
                for arg in args {
                    verify_operand(body, arg, at.clone(), errors);
                }
                if let Some(dest) = destination {
                    verify_place(body, dest, at.clone(), errors);
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
        }
    }
}

fn verify_rvalue(body: &Body, rvalue: &Rvalue, at: CfgLocation, errors: &mut Vec<CfgError>) {
    match rvalue {
        Rvalue::Use(op) | Rvalue::UnaryOp(_, op) | Rvalue::Cast { op, .. } => {
            verify_operand(body, op, at, errors);
        }
        Rvalue::ComplexFromReal { real, .. } => verify_operand(body, real, at, errors),
        Rvalue::RealFromComplex { complex, .. } => verify_operand(body, complex, at, errors),
        Rvalue::BinaryOp(_, lhs, rhs) => {
            verify_operand(body, lhs, at.clone(), errors);
            verify_operand(body, rhs, at, errors);
        }
        Rvalue::AddressOf(place) | Rvalue::Len(place) => verify_place(body, place, at, errors),
    }
}

fn verify_operand(body: &Body, operand: &Operand, at: CfgLocation, errors: &mut Vec<CfgError>) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => verify_place(body, place, at, errors),
        Operand::Const(_) => {}
    }
}

fn verify_place(body: &Body, place: &Place, at: CfgLocation, errors: &mut Vec<CfgError>) {
    verify_local(body, place.base, at.clone(), errors);
    for projection in &place.projection {
        match projection {
            Projection::Deref | Projection::Field(_) => {}
            Projection::Index(index) => verify_operand(body, index, at.clone(), errors),
        }
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
        TerminatorKind::SwitchInt { targets, .. } => {
            targets.iter().map(|(_, target)| *target).collect()
        }
        TerminatorKind::Call { target: Some(target), .. } => vec![*target],
        TerminatorKind::Call { target: None, .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
    }
}

fn verify_storage(body: &Body, errors: &mut Vec<CfgError>) {
    let mut live = vec![0usize; body.locals.len()];
    let mut dead = vec![0usize; body.locals.len()];
    for (bb, block) in body.blocks.iter_enumerated() {
        for (index, stmt) in block.statements.iter().enumerate() {
            match stmt.kind {
                StatementKind::StorageLive(local) if (local.0 as usize) < live.len() => {
                    live[local.0 as usize] += 1;
                }
                StatementKind::StorageDead(local) if (local.0 as usize) < dead.len() => {
                    let idx = local.0 as usize;
                    dead[idx] += 1;
                    if live[idx] == 0 {
                        errors.push(CfgError {
                            at: CfgLocation::Statement { block: bb, index },
                            kind: CfgErrorKind::StorageDeadWithoutLive { local },
                        });
                    }
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
    use rcc_hir::{Ty, TyCtxt};
    use rcc_span::DUMMY_SP;

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
        LocalDecl {
            name: name.then_some(rcc_span::Symbol(1)),
            ty: ty(),
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        }
    }

    fn base_body() -> Body {
        let mut locals: IndexVec<Local, LocalDecl> = IndexVec::new();
        locals.push(local_decl(false));
        let mut blocks: IndexVec<BasicBlockId, BasicBlock> = IndexVec::new();
        blocks.push(BasicBlock {
            statements: Vec::new(),
            terminator: Terminator { kind: TerminatorKind::Return, span: DUMMY_SP },
        });
        Body { def: None, locals, blocks, ret_ty: Some(ty()) }
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
}
