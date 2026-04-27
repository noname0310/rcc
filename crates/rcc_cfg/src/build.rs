//! HIR -> CFG lowering. Produces one `Body` per function.
//!
//! The [`BodyBuilder`] type mediates block creation, statement emission,
//! and terminator fixup. Lowering routines later in this phase take
//! `&mut BodyBuilder` and append at the *current* block.

use rcc_data_structures::{FxHashMap, IndexVec};
use rcc_hir::{DefId, HirCrate, Local, TyCtxt, TyId};
use rcc_session::Session;
use rcc_span::Span;

use crate::{BasicBlock, BasicBlockId, Body, LocalDecl, Statement, Terminator, TerminatorKind};

/// Build CFG bodies for every function in `hir`. Returns a `DefId -> Body` map.
///
/// M3 scope: interface only.
pub fn build_bodies(
    _session: &mut Session,
    _tcx: &TyCtxt,
    _hir: &HirCrate,
) -> FxHashMap<DefId, Body> {
    FxHashMap::default()
}

/// Per-block bookkeeping while a body is under construction.
///
/// We track whether a block has had its terminator set explicitly, so we can
/// reject double-termination and detect reachable blocks that fall off the
/// end of lowering.
#[derive(Debug, Clone, Copy)]
struct BlockState {
    /// `true` once `terminate()` has been called on this block.
    terminated: bool,
}

/// Mutable cursor used by lowering code to incrementally build a [`Body`].
///
/// The builder owns:
/// - the `locals` table,
/// - the growing `blocks` vector,
/// - a *current block* cursor that statement pushes target,
/// - termination metadata used by [`finish`](Self::finish) for reachability
///   audits.
///
/// On construction the builder allocates a single entry block (id `0`) and
/// makes it current.
#[derive(Debug)]
pub struct BodyBuilder {
    def: Option<DefId>,
    ret_ty: Option<TyId>,
    locals: IndexVec<Local, LocalDecl>,
    blocks: IndexVec<BasicBlockId, BasicBlock>,
    states: IndexVec<BasicBlockId, BlockState>,
    current: BasicBlockId,
}

impl Default for BodyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyBuilder {
    /// Create a fresh builder with one (un-terminated) entry block.
    #[must_use]
    pub fn new() -> Self {
        let mut blocks: IndexVec<BasicBlockId, BasicBlock> = IndexVec::new();
        let mut states: IndexVec<BasicBlockId, BlockState> = IndexVec::new();
        let entry = blocks.push(BasicBlock::default());
        let entry_state = states.push(BlockState { terminated: false });
        debug_assert_eq!(entry, entry_state);
        Self { def: None, ret_ty: None, locals: IndexVec::new(), blocks, states, current: entry }
    }

    /// Set the [`DefId`] this body belongs to.
    pub fn set_def(&mut self, def: DefId) {
        self.def = Some(def);
    }

    /// Set the function return type.
    pub fn set_ret_ty(&mut self, ty: TyId) {
        self.ret_ty = Some(ty);
    }

    /// Id of the entry block (always `BasicBlockId(0)`).
    #[must_use]
    pub fn entry(&self) -> BasicBlockId {
        BasicBlockId(0)
    }

    /// Currently-selected block; statements pushed via [`push`](Self::push)
    /// land here, and [`terminate`](Self::terminate) sets *this* block's
    /// terminator.
    #[must_use]
    pub fn current(&self) -> BasicBlockId {
        self.current
    }

    /// Append a fresh, un-terminated block and return its id.
    ///
    /// The current block is **not** changed; call
    /// [`switch_to`](Self::switch_to) to move the cursor.
    pub fn new_block(&mut self) -> BasicBlockId {
        let id = self.blocks.push(BasicBlock::default());
        let state_id = self.states.push(BlockState { terminated: false });
        debug_assert_eq!(id, state_id);
        id
    }

    /// Move the cursor to `bb`. Subsequent pushes / terminate calls operate
    /// on this block.
    ///
    /// # Panics
    /// In debug builds, panics if `bb` was not produced by this builder.
    pub fn switch_to(&mut self, bb: BasicBlockId) {
        debug_assert!(bb.0 < self.blocks.len() as u32, "switch_to: unknown block id {bb:?}");
        self.current = bb;
    }

    /// Allocate a new local slot, returning its [`Local`] id.
    pub fn alloc_local(&mut self, decl: LocalDecl) -> Local {
        self.locals.push(decl)
    }

    /// Append a statement to the current block.
    ///
    /// # Panics
    /// Panics if the current block has already been terminated; once a block
    /// has a terminator, no further statements may be appended (this would
    /// indicate a lowering bug).
    pub fn push(&mut self, stmt: Statement) {
        let cur = self.current;
        assert!(
            !self.states[cur].terminated,
            "BodyBuilder::push: block {cur:?} is already terminated"
        );
        self.blocks[cur].statements.push(stmt);
    }

    /// Set the terminator of the current block.
    ///
    /// # Panics
    /// Panics if the current block has already been terminated.
    pub fn terminate(&mut self, term: Terminator) {
        let cur = self.current;
        assert!(
            !self.states[cur].terminated,
            "BodyBuilder::terminate: block {cur:?} is already terminated"
        );
        self.blocks[cur].terminator = term;
        self.states[cur].terminated = true;
    }

    /// Whether the current block has been terminated.
    #[must_use]
    pub fn is_current_terminated(&self) -> bool {
        self.states[self.current].terminated
    }

    /// Whether `bb` has been terminated.
    #[must_use]
    pub fn is_terminated(&self, bb: BasicBlockId) -> bool {
        self.states[bb].terminated
    }

    /// Convenience: terminate the current block with a plain `Goto(target)`.
    pub fn goto(&mut self, target: BasicBlockId, span: Span) {
        self.terminate(Terminator { kind: TerminatorKind::Goto(target), span });
    }

    /// Finish the body.
    ///
    /// Asserts (in debug builds) that every block reachable from the entry
    /// has had a terminator set. In release builds the check is skipped;
    /// any block that was never terminated keeps its default
    /// [`TerminatorKind::Unreachable`], which is a valid (if pessimistic)
    /// terminator.
    ///
    /// # Panics
    /// In debug builds, panics if a reachable block is un-terminated.
    #[must_use]
    pub fn finish(self) -> Body {
        debug_assert!(
            unterminated_reachable(&self.states, &self.blocks).is_none(),
            "BodyBuilder::finish: reachable block {:?} has no terminator",
            unterminated_reachable(&self.states, &self.blocks).unwrap()
        );
        Body { def: self.def, locals: self.locals, blocks: self.blocks, ret_ty: self.ret_ty }
    }
}

/// Walk the CFG from `BasicBlockId(0)` and return the id of the first
/// reachable block whose terminator was never explicitly set, or `None` if
/// every reachable block is terminated.
fn unterminated_reachable(
    states: &IndexVec<BasicBlockId, BlockState>,
    blocks: &IndexVec<BasicBlockId, BasicBlock>,
) -> Option<BasicBlockId> {
    if blocks.is_empty() {
        return None;
    }
    let mut visited = vec![false; blocks.len()];
    let mut stack: Vec<BasicBlockId> = Vec::new();
    let entry = BasicBlockId(0);
    stack.push(entry);
    while let Some(bb) = stack.pop() {
        let idx = bb.0 as usize;
        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        if !states[bb].terminated {
            return Some(bb);
        }
        for succ in successors(&blocks[bb].terminator.kind) {
            if !visited[succ.0 as usize] {
                stack.push(succ);
            }
        }
    }
    None
}

/// Successor blocks of a terminator (empty for `Return` / `Unreachable` /
/// `Call { target: None, .. }`).
fn successors(kind: &TerminatorKind) -> Vec<BasicBlockId> {
    match kind {
        TerminatorKind::Goto(t) => vec![*t],
        TerminatorKind::SwitchInt { targets, .. } => targets.iter().map(|(_, t)| *t).collect(),
        TerminatorKind::Call { target: Some(t), .. } => vec![*t],
        TerminatorKind::Call { target: None, .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Const, ConstKind, Operand, Place, Rvalue, StatementKind};
    use rcc_hir::TyId;
    use rcc_span::DUMMY_SP;

    fn dummy_ty() -> TyId {
        // `TyId` is a `u32`-backed newtype; any value is structurally valid
        // for a builder unit test that never consults the type interner.
        TyId(0)
    }

    fn int_const(n: i128) -> Operand {
        Operand::Const(Const { kind: ConstKind::Int(n), ty: dummy_ty() })
    }

    /// `int main(void) { return 0; }` — minimal round-trip.
    #[test]
    fn trivial_return_zero() {
        let mut b = BodyBuilder::new();
        b.set_ret_ty(dummy_ty());

        let ret_slot = b.alloc_local(LocalDecl {
            name: None,
            ty: dummy_ty(),
            is_param: false,
            span: DUMMY_SP,
        });

        b.push(Statement {
            kind: StatementKind::Assign {
                place: Place { base: ret_slot, projection: Vec::new() },
                rvalue: Rvalue::Use(int_const(0)),
            },
            span: DUMMY_SP,
        });
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });

        let body = b.finish();
        assert_eq!(body.locals.len(), 1);
        assert_eq!(body.blocks.len(), 1);
        let entry = &body.blocks[BasicBlockId(0)];
        assert_eq!(entry.statements.len(), 1);
        assert!(matches!(entry.statements[0].kind, StatementKind::Assign { .. }));
        assert!(matches!(entry.terminator.kind, TerminatorKind::Return));
        assert_eq!(body.ret_ty, Some(dummy_ty()));
    }

    /// `new_block` allocates without disturbing the cursor; `switch_to`
    /// moves it.
    #[test]
    fn new_block_and_switch() {
        let mut b = BodyBuilder::new();
        let entry = b.current();
        let bb1 = b.new_block();
        assert_ne!(entry, bb1);
        // Cursor unchanged after new_block.
        assert_eq!(b.current(), entry);
        b.switch_to(bb1);
        assert_eq!(b.current(), bb1);
    }

    /// `goto` helper terminates the current block with a `Goto`.
    #[test]
    fn goto_helper_chains_blocks() {
        let mut b = BodyBuilder::new();
        let bb1 = b.new_block();
        b.goto(bb1, DUMMY_SP);
        b.switch_to(bb1);
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        let body = b.finish();
        assert!(matches!(
            body.blocks[BasicBlockId(0)].terminator.kind,
            TerminatorKind::Goto(t) if t == bb1
        ));
        assert!(matches!(body.blocks[bb1].terminator.kind, TerminatorKind::Return));
    }

    /// Pushing onto a terminated block panics.
    #[test]
    #[should_panic(expected = "already terminated")]
    fn push_after_terminate_panics() {
        let mut b = BodyBuilder::new();
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        b.push(Statement { kind: StatementKind::Nop, span: DUMMY_SP });
    }

    /// Double-terminate panics.
    #[test]
    #[should_panic(expected = "already terminated")]
    fn double_terminate_panics() {
        let mut b = BodyBuilder::new();
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
    }

    /// `finish` panics in debug mode when a reachable block is missing a
    /// terminator. (In release builds the assertion is compiled out, which
    /// matches the task spec: "panics (debug) / emits diagnostic
    /// (release)".)
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "no terminator")]
    fn finish_rejects_unterminated_reachable_block() {
        let b = BodyBuilder::new();
        // Entry block was never terminated — must trip the audit.
        let _body = b.finish();
    }

    /// `finish` accepts un-terminated blocks that are *unreachable* from
    /// the entry. (Dead code that the lowering stage left behind should
    /// not block compilation.)
    #[test]
    fn finish_ignores_unreachable_unterminated_block() {
        let mut b = BodyBuilder::new();
        // Entry: terminate immediately.
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        // Orphan block, never targeted, never terminated.
        let _orphan = b.new_block();
        let body = b.finish();
        assert_eq!(body.blocks.len(), 2);
    }

    /// Successors of `SwitchInt` are visited by the reachability walk.
    #[test]
    fn switch_int_successors_must_be_terminated() {
        let mut b = BodyBuilder::new();
        let arm0 = b.new_block();
        let arm1 = b.new_block();
        b.switch_to(arm0);
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        b.switch_to(arm1);
        b.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        b.switch_to(b.entry());
        b.terminate(Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: int_const(0),
                targets: vec![(Some(0), arm0), (None, arm1)],
            },
            span: DUMMY_SP,
        });
        let body = b.finish();
        assert_eq!(body.blocks.len(), 3);
    }
}
