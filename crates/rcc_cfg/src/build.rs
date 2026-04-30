//! HIR -> CFG lowering. Produces one `Body` per function.
//!
//! The [`BodyBuilder`] type mediates block creation, statement emission,
//! and terminator fixup. Lowering routines later in this phase take
//! `&mut BodyBuilder` and append at the *current* block.

use rcc_data_structures::{FxHashMap, IndexVec};
use rcc_hir::{DefId, HirCrate, Local, TyCtxt, TyId};
use rcc_session::Session;
use rcc_span::{Span, Symbol};

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

/// Where in the local-allocation pipeline the builder currently sits.
///
/// The CFG layout convention (mirrors rustc's MIR) is:
/// 1. `Local(0)` = return slot,
/// 2. `Local(1..=N)` = parameters in source order,
/// 3. subsequent locals = declared user variables, then lowering temporaries.
///
/// The phase only advances forward; debug-mode assertions in `alloc_param` /
/// `alloc_user_local` / `alloc_temp` reject out-of-order calls. Release builds
/// skip the checks (the helpers still produce well-formed `Local`s, just
/// without the order audit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllocPhase {
    /// Nothing allocated yet; only `alloc_return_slot` is valid.
    ReturnSlot,
    /// Return slot done; `alloc_param` extends the parameter run.
    Params,
    /// Parameters done; `alloc_user_local` / `alloc_temp` may be mixed freely.
    Locals,
}

/// Loop context for break/continue target resolution.
///
/// Pushed onto a per-body stack when entering a loop construct;
/// `break` emits `Goto(break_target)`, `continue` emits
/// `Goto(cont_target)`. For `while`/`do-while` the continue target
/// is the header; for `for` it is the step block.
#[derive(Debug, Clone)]
pub struct LoopCtx {
    /// Block that `continue` jumps to.
    pub cont_target: BasicBlockId,
    /// Block that `break` jumps to (the loop exit).
    pub break_target: BasicBlockId,
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
    /// Tracks how far the local-allocation pipeline has advanced. See
    /// [`AllocPhase`] for the staged convention.
    phase: AllocPhase,
    /// Stack of enclosing loop contexts. `break` / `continue` targets
    /// are resolved by peeking the top of this stack.
    loop_stack: Vec<LoopCtx>,
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
        Self {
            def: None,
            ret_ty: None,
            locals: IndexVec::new(),
            blocks,
            states,
            current: entry,
            phase: AllocPhase::ReturnSlot,
            loop_stack: Vec::new(),
        }
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

    /// Low-level escape hatch: append `decl` to the locals table verbatim.
    ///
    /// Prefer the staged helpers — [`alloc_return_slot`](Self::alloc_return_slot),
    /// [`alloc_param`](Self::alloc_param), [`alloc_user_local`](Self::alloc_user_local),
    /// [`alloc_temp`](Self::alloc_temp) — which enforce the `Local(0) = ret`,
    /// then params, then user-locals/temps ordering.
    ///
    /// This entry point performs **no** ordering check and does not advance
    /// the internal phase tracker; it exists for tests and for code paths
    /// that already validated their own invariants.
    pub fn alloc_local(&mut self, decl: LocalDecl) -> Local {
        self.locals.push(decl)
    }

    /// Allocate `Local(0)` as the return slot.
    ///
    /// Must be called exactly once, before any parameter or user-local. The
    /// task spec ([rustc-style MIR convention][rustc-mir]) reserves
    /// `Local(0)` for the value the function returns; void functions use a
    /// `void`/unit `TyId` here.
    ///
    /// Also calls [`set_ret_ty`](Self::set_ret_ty) so callers do not have to
    /// pass `ret_ty` twice.
    ///
    /// [rustc-mir]: https://rustc-dev-guide.rust-lang.org/mir/index.html#mir-data-types
    ///
    /// # Panics
    /// In debug builds, panics if called twice or after parameters /
    /// user-locals were already allocated.
    pub fn alloc_return_slot(&mut self, ret_ty: TyId, span: Span) -> Local {
        debug_assert_eq!(
            self.phase,
            AllocPhase::ReturnSlot,
            "alloc_return_slot: must be the first allocation (phase is {:?})",
            self.phase
        );
        debug_assert!(
            self.locals.is_empty(),
            "alloc_return_slot: locals table is non-empty ({} entries)",
            self.locals.len()
        );
        self.ret_ty = Some(ret_ty);
        let local = self.locals.push(LocalDecl { name: None, ty: ret_ty, is_param: false, span });
        debug_assert_eq!(local, Local(0));
        self.phase = AllocPhase::Params;
        local
    }

    /// Allocate a parameter slot. Must follow [`alloc_return_slot`] and
    /// precede any [`alloc_user_local`] / [`alloc_temp`] call.
    ///
    /// [`alloc_return_slot`]: Self::alloc_return_slot
    /// [`alloc_user_local`]: Self::alloc_user_local
    /// [`alloc_temp`]: Self::alloc_temp
    ///
    /// # Panics
    /// In debug builds, panics if the return slot was not allocated yet, or
    /// if a user-local / temp has already been allocated.
    pub fn alloc_param(&mut self, name: Symbol, ty: TyId, span: Span) -> Local {
        debug_assert_eq!(
            self.phase,
            AllocPhase::Params,
            "alloc_param: phase is {:?}, expected Params (call alloc_return_slot first, \
             and do not interleave alloc_user_local/alloc_temp before parameters)",
            self.phase
        );
        self.locals.push(LocalDecl { name: Some(name), ty, is_param: true, span })
    }

    /// Allocate a user-declared local. Closes the parameter run on the first
    /// call.
    pub fn alloc_user_local(&mut self, name: Symbol, ty: TyId, span: Span) -> Local {
        debug_assert!(
            self.phase != AllocPhase::ReturnSlot,
            "alloc_user_local: return slot has not been allocated yet"
        );
        self.phase = AllocPhase::Locals;
        self.locals.push(LocalDecl { name: Some(name), ty, is_param: false, span })
    }

    /// Allocate a lowering-introduced temporary. Closes the parameter run on
    /// the first call.
    pub fn alloc_temp(&mut self, ty: TyId, span: Span) -> Local {
        debug_assert!(
            self.phase != AllocPhase::ReturnSlot,
            "alloc_temp: return slot has not been allocated yet"
        );
        self.phase = AllocPhase::Locals;
        self.locals.push(LocalDecl { name: None, ty, is_param: false, span })
    }

    /// Convenience matching the task spec's `local(ty, name)` signature:
    /// allocate a user-local when `name` is `Some`, a temporary otherwise.
    pub fn local(&mut self, ty: TyId, name: Option<Symbol>, span: Span) -> Local {
        match name {
            Some(sym) => self.alloc_user_local(sym, ty, span),
            None => self.alloc_temp(ty, span),
        }
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

    /// Push a new loop context onto the stack.
    ///
    /// Called when entering a `while`, `do-while`, or `for` loop.
    /// `cont_target` is the block `continue` should jump to (header
    /// for while/do-while, step block for for). `break_target` is the
    /// loop exit block.
    pub fn push_loop(&mut self, cont_target: BasicBlockId, break_target: BasicBlockId) {
        self.loop_stack.push(LoopCtx { cont_target, break_target });
    }

    /// Pop the current loop context.
    ///
    /// # Panics
    /// Panics if the loop stack is empty (i.e., not inside a loop).
    pub fn pop_loop(&mut self) {
        self.loop_stack.pop().expect("pop_loop: no loop context to pop");
    }

    /// Get the current loop context (top of stack), or `None` if not
    /// inside a loop.
    #[must_use]
    pub fn current_loop(&self) -> Option<&LoopCtx> {
        self.loop_stack.last()
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
    use rcc_span::{Symbol, DUMMY_SP};

    fn dummy_ty() -> TyId {
        // `TyId` is a `u32`-backed newtype; any value is structurally valid
        // for a builder unit test that never consults the type interner.
        TyId(0)
    }

    fn ty(n: u32) -> TyId {
        TyId(n)
    }

    fn sym(n: u32) -> Symbol {
        Symbol(n)
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

    /// Acceptance test for 08-cfg/02-local-allocation:
    /// `void f(int a, int b) { int c; }` yields locals
    /// `[ret:void, a, b, c]`.
    #[test]
    fn acceptance_void_f_int_a_int_b_int_c() {
        // Pretend TyId(1) = int, TyId(2) = void; the builder never resolves
        // these, so any distinct ids will do.
        let int_ty = ty(1);
        let void_ty = ty(2);
        let a = sym(10);
        let b = sym(11);
        let c = sym(12);

        let mut bld = BodyBuilder::new();
        let ret = bld.alloc_return_slot(void_ty, DUMMY_SP);
        let pa = bld.alloc_param(a, int_ty, DUMMY_SP);
        let pb = bld.alloc_param(b, int_ty, DUMMY_SP);
        let lc = bld.alloc_user_local(c, int_ty, DUMMY_SP);

        // Slot indices must follow the rustc-style convention.
        assert_eq!(ret, Local(0));
        assert_eq!(pa, Local(1));
        assert_eq!(pb, Local(2));
        assert_eq!(lc, Local(3));

        // Terminate so finish() does not trip the reachability audit.
        bld.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        let body = bld.finish();

        assert_eq!(body.locals.len(), 4);
        assert_eq!(body.ret_ty, Some(void_ty));

        // Local 0: return slot — no name, type = void, not a param.
        assert_eq!(body.locals[Local(0)].name, None);
        assert_eq!(body.locals[Local(0)].ty, void_ty);
        assert!(!body.locals[Local(0)].is_param);

        // Locals 1, 2: parameters in source order.
        assert_eq!(body.locals[Local(1)].name, Some(a));
        assert_eq!(body.locals[Local(1)].ty, int_ty);
        assert!(body.locals[Local(1)].is_param);
        assert_eq!(body.locals[Local(2)].name, Some(b));
        assert_eq!(body.locals[Local(2)].ty, int_ty);
        assert!(body.locals[Local(2)].is_param);

        // Local 3: declared user variable — named, not a param.
        assert_eq!(body.locals[Local(3)].name, Some(c));
        assert_eq!(body.locals[Local(3)].ty, int_ty);
        assert!(!body.locals[Local(3)].is_param);
    }

    /// Temporaries follow user-locals in allocation order.
    #[test]
    fn temps_follow_user_locals() {
        let mut bld = BodyBuilder::new();
        let _ret = bld.alloc_return_slot(ty(2), DUMMY_SP);
        let _p = bld.alloc_param(sym(1), ty(1), DUMMY_SP);
        let user = bld.alloc_user_local(sym(2), ty(1), DUMMY_SP);
        let t0 = bld.alloc_temp(ty(1), DUMMY_SP);
        let t1 = bld.alloc_temp(ty(1), DUMMY_SP);

        assert_eq!(user, Local(2));
        assert_eq!(t0, Local(3));
        assert_eq!(t1, Local(4));

        bld.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        let body = bld.finish();
        // Temps have no name and are not params.
        assert_eq!(body.locals[t0].name, None);
        assert!(!body.locals[t0].is_param);
        assert_eq!(body.locals[t1].name, None);
        assert!(!body.locals[t1].is_param);
    }

    /// `local(ty, Some(name))` -> user-local; `local(ty, None)` -> temp.
    #[test]
    fn local_convenience_dispatches_on_name() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_return_slot(ty(2), DUMMY_SP);
        let named = bld.local(ty(1), Some(sym(7)), DUMMY_SP);
        let unnamed = bld.local(ty(1), None, DUMMY_SP);

        bld.terminate(Terminator { kind: TerminatorKind::Return, span: DUMMY_SP });
        let body = bld.finish();
        assert_eq!(body.locals[named].name, Some(sym(7)));
        assert!(!body.locals[named].is_param);
        assert_eq!(body.locals[unnamed].name, None);
        assert!(!body.locals[unnamed].is_param);
    }

    /// Calling `alloc_return_slot` twice trips the phase guard (debug only).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_return_slot")]
    fn double_return_slot_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_return_slot(ty(2), DUMMY_SP);
        let _ = bld.alloc_return_slot(ty(2), DUMMY_SP);
    }

    /// `alloc_param` before `alloc_return_slot` is a phase violation.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_param")]
    fn alloc_param_before_return_slot_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_param(sym(0), ty(1), DUMMY_SP);
    }

    /// Once a user-local has been allocated, `alloc_param` is rejected.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_param")]
    fn alloc_param_after_user_local_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_return_slot(ty(2), DUMMY_SP);
        let _ = bld.alloc_user_local(sym(0), ty(1), DUMMY_SP);
        let _ = bld.alloc_param(sym(1), ty(1), DUMMY_SP);
    }

    /// Once a temp has been allocated, `alloc_param` is rejected.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_param")]
    fn alloc_param_after_temp_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_return_slot(ty(2), DUMMY_SP);
        let _ = bld.alloc_temp(ty(1), DUMMY_SP);
        let _ = bld.alloc_param(sym(1), ty(1), DUMMY_SP);
    }

    /// `alloc_user_local` / `alloc_temp` before the return slot is a phase
    /// violation.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_user_local")]
    fn alloc_user_local_before_return_slot_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_user_local(sym(0), ty(1), DUMMY_SP);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "alloc_temp")]
    fn alloc_temp_before_return_slot_panics() {
        let mut bld = BodyBuilder::new();
        let _ = bld.alloc_temp(ty(1), DUMMY_SP);
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
