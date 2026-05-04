//! HIR -> CFG lowering. Produces one `Body` per function.
//!
//! The [`BodyBuilder`] type mediates block creation, statement emission,
//! and terminator fixup. Lowering routines later in this phase take
//! `&mut BodyBuilder` and append at the *current* block.

use rcc_data_structures::{FxHashMap, IndexVec};
use rcc_errors::{codes, DiagnosticBuilder, Level};
use rcc_hir::{
    Body as HirBody, DefId, DefKind, HirCrate, HirExprKind, LayoutCx, LayoutError, Local,
    ObjectQuals, Ty, TyCtxt, TyId,
};
use rcc_session::Session;
use rcc_span::{Span, Symbol};

use crate::lower::{lower_stmt, LocalMap, LowerCx};
use crate::{
    BasicBlock, BasicBlockId, Body, LocalDecl, Statement, StatementKind, Terminator, TerminatorKind,
};

/// Build CFG bodies for every function in `hir`. Returns a `DefId -> Body` map.
///
pub fn build_bodies(session: &mut Session, tcx: &TyCtxt, hir: &HirCrate) -> FxHashMap<DefId, Body> {
    let mut out = FxHashMap::default();
    let layout = LayoutCx::with_defs(tcx, &hir.defs);
    for (&def_id, hir_body) in &hir.bodies {
        let Some(def) = hir.defs.get(def_id) else {
            continue;
        };
        let DefKind::Function { ty: fn_ty, .. } = def.kind else {
            continue;
        };
        let ret_ty = match tcx.get(fn_ty) {
            Ty::Func { ret, .. } => *ret,
            _ => tcx.void,
        };

        let mut builder = BodyBuilder::new();
        builder.set_def(def_id);
        builder.alloc_return_slot(ret_ty, def.span);

        let mut local_map = LocalMap::new();
        for (hir_local, decl) in hir_body.locals.iter_enumerated().filter(|(_, decl)| decl.is_param)
        {
            let cfg_local =
                builder.alloc_param_decl_with_quals(decl.name, decl.ty, decl.quals, decl.span);
            local_map.insert(hir_local, cfg_local);
        }
        for (hir_local, decl) in
            hir_body.locals.iter_enumerated().filter(|(_, decl)| !decl.is_param)
        {
            let cfg_local = builder.local_with_quals(decl.ty, decl.name, decl.quals, decl.span);
            local_map.insert(hir_local, cfg_local);
        }

        if let Some(root) = hir_body.root {
            if !audit_sizeof_layout(session, hir_body, &layout) {
                continue;
            }
            builder.collect_labels(hir_body, root, &local_map);
            let cx = LowerCx::with_defs_and_return(hir_body, tcx, &local_map, &hir.defs, ret_ty);
            lower_stmt(&mut builder, &cx, root);
        }
        if !builder.is_current_terminated() {
            builder.terminate(Terminator { kind: TerminatorKind::Return, span: def.span });
        }
        let body = builder.finish();
        #[cfg(any(debug_assertions, test))]
        if let Err(errors) = crate::verify::verify_body_with_hir(&body, tcx, hir) {
            emit_cfg_verifier_error(session, def.span, &errors);
        }
        out.insert(def_id, body);
    }
    out
}

fn audit_sizeof_layout(session: &mut Session, hir_body: &HirBody, layout: &LayoutCx<'_>) -> bool {
    let mut ok = true;
    for expr in hir_body.exprs.iter() {
        let result = match expr.kind {
            HirExprKind::SizeofExpr(operand) => {
                let operand_ty = hir_body.exprs[operand].ty;
                match layout.tcx.get(operand_ty) {
                    Ty::Array { elem, is_vla: true, .. } => layout.layout_of(elem.ty),
                    _ => layout.layout_of(operand_ty),
                }
            }
            HirExprKind::SizeofType(ty) => layout.layout_of(ty),
            _ => continue,
        };
        if let Err(err) = result {
            emit_sizeof_layout_error(session, expr.span, err);
            ok = false;
        }
    }
    ok
}

fn emit_sizeof_layout_error(session: &mut Session, span: Span, err: LayoutError) {
    DiagnosticBuilder::new(
        &mut session.handler,
        Level::Error,
        "cannot compute layout for sizeof operand",
    )
    .code(codes::E0085)
    .primary(span, "sizeof requires a complete object layout")
    .note(err.to_string())
    .emit();
}

#[cfg(any(debug_assertions, test))]
fn emit_cfg_verifier_error(session: &mut Session, span: Span, errors: &[crate::verify::CfgError]) {
    let mut diag =
        DiagnosticBuilder::new(&mut session.handler, Level::Error, "invalid CFG produced");
    diag = diag.primary(span, "CFG verifier rejected this function body");
    for err in errors {
        diag = diag.note(err.to_string());
    }
    diag.emit();
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

/// Metadata recorded for a label during the pre-pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelInfo {
    /// Basic block assigned to the label.
    pub block: BasicBlockId,
    /// Lexical scope depth at the label location.
    pub scope_depth: usize,
    /// Deepest active scope depth that already had a runtime local entry
    /// before the label. Jumping from a shallower depth would bypass
    /// `StorageLive` / initializer / VLA-length side effects.
    pub runtime_scope_depth: Option<usize>,
    /// Deepest active scope depth that already had a VLA declaration
    /// before the label. Jumping from a shallower depth violates C99's
    /// variably-modified-type goto constraint and would bypass runtime
    /// allocation metadata.
    pub vla_scope_depth: Option<usize>,
    /// Ordinary automatic locals whose declarations are in scope before
    /// this label. A valid goto may enter these scopes, but the label
    /// block still needs `StorageLive` for the locals that the jump
    /// bypassed.
    pub ordinary_locals_to_live: Vec<crate::Local>,
    /// All locals whose declarations are in scope before this label,
    /// including VLA locals. A backward goto to a label before a later
    /// declaration must end the lifetime of every currently-live local
    /// not present in this set.
    pub locals_live_at_label: Vec<crate::Local>,
    /// VLA locals whose declarations are in scope before this label.
    /// Jumping to such a label is only valid if those VLA locals are
    /// already live on the source edge; otherwise the jump would bypass
    /// their runtime allocation.
    pub vla_locals_live_at_label: Vec<crate::Local>,
}

#[derive(Debug, Default, Clone)]
struct LabelScopeState {
    has_runtime_entry: bool,
    has_vla: bool,
    ordinary_locals: Vec<crate::Local>,
    vla_locals: Vec<crate::Local>,
    live_locals: Vec<crate::Local>,
}

/// Loop context for break/continue target resolution.
///
/// Pushed onto a per-body stack when entering a loop construct;
/// `break` emits `Goto(break_target)`, `continue` emits
/// `Goto(cont_target)`. For `while`/`do-while` the continue target
/// is the header; for `for` it is the step block.
#[derive(Debug, Copy, Clone)]
pub struct LoopCtx {
    /// Block that `continue` jumps to.
    pub cont_target: BasicBlockId,
    /// Block that `break` jumps to (the loop exit).
    pub break_target: BasicBlockId,
    /// Scope depth (i.e. `scopes.len()`) at the time the loop was
    /// entered. `break` / `continue` emit `StorageDead` for every
    /// scope frame opened *since* the loop was entered (i.e. frames
    /// at depth >= this value), then transfer control.
    pub scope_depth: usize,
}

/// Break-only context for resolving `break` inside a loop or switch.
///
/// Loops push both a [`LoopCtx`] and a [`BreakCtx`]; switches push only
/// a [`BreakCtx`]. The split lets `continue` resolve via the loop stack
/// while `break` resolves via the unified break stack (preserving the
/// "break exits the innermost breakable" rule even when a switch is
/// nested inside a loop).
#[derive(Debug, Copy, Clone)]
pub struct BreakCtx {
    /// Block that `break` jumps to.
    pub target: BasicBlockId,
    /// Scope depth (i.e. `scopes.len()`) at the time the breakable
    /// construct was entered. `break` emits `StorageDead` for every
    /// scope frame at depth >= this value.
    pub scope_depth: usize,
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
    /// Stack of enclosing loop contexts. `continue` target is resolved
    /// by peeking the top of this stack.
    loop_stack: Vec<LoopCtx>,
    /// Stack of enclosing breakable constructs (loops and switches).
    /// `break` targets the top of this stack, preserving nesting order.
    break_stack: Vec<BreakCtx>,
    /// Stack of active switch case-label maps.
    ///
    /// A C `case` / `default` label is not a scoped sub-body; it is a
    /// control-flow label inside the enclosing switch body. Lowering
    /// therefore needs to resolve a `HirStmtId` for the label to the block
    /// selected by the dispatch terminator while still lowering the switch
    /// body in source order.
    switch_case_stack: Vec<FxHashMap<rcc_hir::HirStmtId, BasicBlockId>>,
    /// Label name → metadata map. Populated by a pre-pass so forward
    /// `goto` can be resolved in a single lowering pass while preserving
    /// scope-lifetime information.
    label_map: FxHashMap<Symbol, LabelInfo>,
    /// Stack of lexical scope frames. Each frame holds the locals
    /// declared in that scope, in declaration order. Pushed by
    /// [`enter_scope`](Self::enter_scope) on block entry, popped by
    /// [`exit_scope`](Self::exit_scope) on block exit. The matching
    /// `StorageLive` / `StorageDead` statements bracket every
    /// block-scoped local's lifetime so LLVM's `mem2reg` and stack-slot
    /// reuse passes can promote / share allocas.
    scopes: Vec<Vec<Local>>,
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
            break_stack: Vec::new(),
            switch_case_stack: Vec::new(),
            label_map: FxHashMap::default(),
            scopes: Vec::new(),
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
        let local = self.locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span,
        });
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
        self.alloc_param_decl(Some(name), ty, span)
    }

    /// Allocate a parameter slot with an optional source name.
    pub fn alloc_param_decl(&mut self, name: Option<Symbol>, ty: TyId, span: Span) -> Local {
        self.alloc_param_decl_with_quals(name, ty, ObjectQuals::none(), span)
    }

    /// Allocate a parameter slot with object qualifiers preserved from HIR.
    pub fn alloc_param_decl_with_quals(
        &mut self,
        name: Option<Symbol>,
        ty: TyId,
        quals: ObjectQuals,
        span: Span,
    ) -> Local {
        debug_assert_eq!(
            self.phase,
            AllocPhase::Params,
            "alloc_param: phase is {:?}, expected Params (call alloc_return_slot first, \
             and do not interleave alloc_user_local/alloc_temp before parameters)",
            self.phase
        );
        self.locals.push(LocalDecl { name, ty, quals, vla_len: None, is_param: true, span })
    }

    /// Allocate a user-declared local. Closes the parameter run on the first
    /// call.
    pub fn alloc_user_local(&mut self, name: Symbol, ty: TyId, span: Span) -> Local {
        self.alloc_user_local_with_quals(name, ty, ObjectQuals::none(), span)
    }

    /// Allocate a user-declared local with object qualifiers preserved from HIR.
    pub fn alloc_user_local_with_quals(
        &mut self,
        name: Symbol,
        ty: TyId,
        quals: ObjectQuals,
        span: Span,
    ) -> Local {
        debug_assert!(
            self.phase != AllocPhase::ReturnSlot,
            "alloc_user_local: return slot has not been allocated yet"
        );
        self.phase = AllocPhase::Locals;
        self.locals.push(LocalDecl {
            name: Some(name),
            ty,
            quals,
            vla_len: None,
            is_param: false,
            span,
        })
    }

    /// Allocate a lowering-introduced temporary. Closes the parameter run on
    /// the first call.
    pub fn alloc_temp(&mut self, ty: TyId, span: Span) -> Local {
        debug_assert!(
            self.phase != AllocPhase::ReturnSlot,
            "alloc_temp: return slot has not been allocated yet"
        );
        self.phase = AllocPhase::Locals;
        self.locals.push(LocalDecl {
            name: None,
            ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span,
        })
    }

    /// Convenience matching the task spec's `local(ty, name)` signature:
    /// allocate a user-local when `name` is `Some`, a temporary otherwise.
    pub fn local(&mut self, ty: TyId, name: Option<Symbol>, span: Span) -> Local {
        self.local_with_quals(ty, name, ObjectQuals::none(), span)
    }

    /// Allocate a source local with object qualifiers, or an unqualified temp.
    pub fn local_with_quals(
        &mut self,
        ty: TyId,
        name: Option<Symbol>,
        quals: ObjectQuals,
        span: Span,
    ) -> Local {
        match name {
            Some(sym) => self.alloc_user_local_with_quals(sym, ty, quals, span),
            None => self.alloc_temp(ty, span),
        }
    }

    /// Attach the runtime element-count local for a VLA user local.
    pub fn set_vla_len(&mut self, local: Local, len_local: Local) {
        debug_assert!(local.0 < self.locals.len() as u32, "set_vla_len: unknown local {local:?}");
        debug_assert!(
            len_local.0 < self.locals.len() as u32,
            "set_vla_len: unknown len local {len_local:?}"
        );
        self.locals[local].vla_len = Some(len_local);
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
    /// loop exit block. The current [`scope_depth`](Self::scope_depth)
    /// is recorded so `break` / `continue` can emit `StorageDead` for
    /// every intervening scope opened inside the loop body.
    pub fn push_loop(&mut self, cont_target: BasicBlockId, break_target: BasicBlockId) {
        let scope_depth = self.scopes.len();
        self.loop_stack.push(LoopCtx { cont_target, break_target, scope_depth });
        self.break_stack.push(BreakCtx { target: break_target, scope_depth });
    }

    /// Pop the current loop context.
    ///
    /// # Panics
    /// Panics if the loop stack is empty (i.e., not inside a loop).
    pub fn pop_loop(&mut self) {
        self.loop_stack.pop().expect("pop_loop: no loop context to pop");
        self.break_stack.pop().expect("pop_loop: break_stack mismatch");
    }

    /// Get the current loop context (top of stack), or `None` if not
    /// inside a loop.
    #[must_use]
    pub fn current_loop(&self) -> Option<&LoopCtx> {
        self.loop_stack.last()
    }

    /// Push a breakable construct (switch join block) onto the break stack.
    /// The current [`scope_depth`](Self::scope_depth) is recorded so a
    /// `break` inside the switch unwinds only the scopes opened *inside*
    /// the switch body.
    pub fn push_switch(&mut self, join_block: BasicBlockId) {
        let scope_depth = self.scopes.len();
        self.break_stack.push(BreakCtx { target: join_block, scope_depth });
    }

    /// Pop the current switch context from the break stack.
    ///
    /// # Panics
    /// Panics if the break stack is empty.
    pub fn pop_switch(&mut self) {
        self.break_stack.pop().expect("pop_switch: no switch context to pop");
    }

    /// Push the active case/default label map for a switch body.
    pub fn push_switch_cases(&mut self, cases: FxHashMap<rcc_hir::HirStmtId, BasicBlockId>) {
        self.switch_case_stack.push(cases);
    }

    /// Pop the active case/default label map for a switch body.
    ///
    /// # Panics
    /// Panics if no switch case map is active.
    pub fn pop_switch_cases(&mut self) {
        self.switch_case_stack.pop().expect("pop_switch_cases: no switch case map to pop");
    }

    /// Resolve a case/default label statement to the block assigned by the
    /// innermost active switch dispatch.
    #[must_use]
    pub fn switch_case_block(&self, stmt: rcc_hir::HirStmtId) -> Option<BasicBlockId> {
        self.switch_case_stack.last().and_then(|cases| cases.get(&stmt).copied())
    }

    /// Get the current break target (top of stack), or `None` if not
    /// inside a breakable construct.
    #[must_use]
    pub fn current_break_target(&self) -> Option<BasicBlockId> {
        self.break_stack.last().map(|ctx| ctx.target)
    }

    /// Get the current break context (top of stack), or `None` if not
    /// inside a breakable construct. Carries both the jump target and
    /// the scope depth at the time the breakable construct was entered.
    #[must_use]
    pub fn current_break_ctx(&self) -> Option<BreakCtx> {
        self.break_stack.last().copied()
    }

    /// Number of currently-open lexical scope frames. Used by
    /// `push_loop` / `push_switch` to record where break/continue
    /// should unwind to.
    #[must_use]
    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }

    /// Enter a fresh lexical scope frame. Subsequent
    /// [`storage_live`](Self::storage_live) calls associate locals
    /// with this frame; [`exit_scope`](Self::exit_scope) emits the
    /// matching `StorageDead` statements when the frame is popped.
    pub fn enter_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    /// Pop the innermost scope frame. If the current block is still
    /// open (i.e. not yet terminated), emit `StorageDead` for every
    /// local declared in the popped frame in *reverse declaration
    /// order*. The frame is popped regardless: a terminated block
    /// already emitted its `StorageDead`s on the terminating path
    /// (`break` / `continue` / `return`), and the post-block fall-
    /// through is the only path that needs them now.
    ///
    /// # Panics
    /// Panics if there is no scope to exit (i.e. unbalanced
    /// enter/exit calls).
    pub fn exit_scope(&mut self, span: Span) {
        let frame = self.scopes.pop().expect("exit_scope: no scope to exit");
        if self.is_current_terminated() {
            return;
        }
        // Reverse declaration order: the last local declared dies
        // first. Mirrors the order in which their RAII analogues would
        // run if C had any.
        for &local in frame.iter().rev() {
            self.push(Statement { kind: StatementKind::StorageDead(local), span });
        }
    }

    /// Emit `StorageLive(local)` in the current block and record
    /// `local` in the innermost scope frame so the matching
    /// `exit_scope` emits the matching `StorageDead`.
    ///
    /// # Panics
    /// Panics if no scope has been entered yet.
    pub fn storage_live(&mut self, local: Local, span: Span) {
        debug_assert!(
            !self.scopes.is_empty(),
            "storage_live: no current scope (call enter_scope first)"
        );
        self.push(Statement { kind: StatementKind::StorageLive(local), span });
        self.scopes.last_mut().expect("storage_live: no current scope").push(local);
    }

    /// Emit `StorageDead` for every local in scope frames at depth
    /// `>= target_depth`, innermost frame first, reverse declaration
    /// order within each frame. Frames are *not* popped — only
    /// [`exit_scope`](Self::exit_scope) pops them.
    ///
    /// Used by `break` / `continue` / `return` to flush every scope
    /// they jump out of. A no-op when the current block is already
    /// terminated.
    pub fn emit_storage_deads_to_depth(&mut self, target_depth: usize, span: Span) {
        if self.is_current_terminated() {
            return;
        }
        // Collect first to side-step the &mut self borrow on `push`.
        let mut to_dead: Vec<Local> = Vec::new();
        for depth in (target_depth..self.scopes.len()).rev() {
            for &local in self.scopes[depth].iter().rev() {
                to_dead.push(local);
            }
        }
        for local in to_dead {
            self.push(Statement { kind: StatementKind::StorageDead(local), span });
        }
    }

    fn active_locals(&self) -> Vec<Local> {
        self.scopes.iter().flat_map(|scope| scope.iter().copied()).collect()
    }

    fn emit_storage_deads_except(&mut self, keep_live: &[Local], span: Span) {
        if self.is_current_terminated() {
            return;
        }
        let mut to_dead: Vec<Local> = Vec::new();
        for depth in (0..self.scopes.len()).rev() {
            for &local in self.scopes[depth].iter().rev() {
                if !keep_live.contains(&local) {
                    to_dead.push(local);
                }
            }
        }
        for local in to_dead {
            self.push(Statement { kind: StatementKind::StorageDead(local), span });
        }
    }

    /// Convenience: terminate the current block with a plain `Goto(target)`.
    pub fn goto(&mut self, target: BasicBlockId, span: Span) {
        self.terminate(Terminator { kind: TerminatorKind::Goto(target), span });
    }

    /// Register a label → block mapping. Called by the pre-pass that
    /// scans the HIR for `Label` statements before lowering begins.
    pub fn insert_label(&mut self, name: Symbol, block: BasicBlockId) {
        self.label_map.insert(
            name,
            LabelInfo {
                block,
                scope_depth: self.scope_depth(),
                runtime_scope_depth: None,
                vla_scope_depth: None,
                ordinary_locals_to_live: Vec::new(),
                locals_live_at_label: Vec::new(),
                vla_locals_live_at_label: Vec::new(),
            },
        );
    }

    fn insert_label_info(&mut self, name: Symbol, info: LabelInfo) {
        self.label_map.insert(name, info);
    }

    /// Look up the block id for a label name.
    ///
    /// # Panics
    /// Panics if the label was not registered by the pre-pass.
    #[must_use]
    pub fn label_block(&self, name: Symbol) -> BasicBlockId {
        self.label_info(name).block
    }

    /// Look up the metadata for a label name.
    ///
    /// # Panics
    /// Panics if the label was not registered by the pre-pass.
    #[must_use]
    pub fn label_info(&self, name: Symbol) -> LabelInfo {
        self.label_map.get(&name).cloned().unwrap_or_else(|| panic!("unknown label: {name:?}"))
    }

    /// Conservative destination list for a computed goto in this function.
    #[must_use]
    pub fn label_targets(&self) -> Vec<BasicBlockId> {
        self.label_map.values().map(|info| info.block).collect()
    }

    /// Emit a goto to a named label while preserving lexical lifetime
    /// markers for scopes exited by the jump.
    ///
    /// # Panics
    /// Panics if the jump enters a scope whose runtime local setup or VLA
    /// allocation would be bypassed.
    pub fn goto_label(&mut self, name: Symbol, span: Span) {
        let info = self.label_info(name);
        let current_depth = self.scope_depth();
        let active_locals = self.active_locals();
        if let Some(vla_depth) = info.vla_scope_depth {
            assert!(
                current_depth >= vla_depth,
                "goto into VLA scope: current depth {current_depth}, label depth {}, required VLA \
                 depth {vla_depth}",
                info.scope_depth
            );
        }
        for local in &info.vla_locals_live_at_label {
            assert!(
                active_locals.contains(local),
                "goto into VLA scope: label requires live VLA local {local:?}"
            );
        }
        self.emit_storage_deads_except(&info.locals_live_at_label, span);
        self.goto(info.block, span);
    }

    pub fn emit_label_storage_lives(&mut self, name: Symbol, span: Span) {
        let info = self.label_info(name);
        for local in info.ordinary_locals_to_live {
            self.storage_live(local, span);
        }
    }

    /// Pre-pass: scan the HIR body for `Label` statements and create an
    /// empty block for each one.  This lets forward `goto` resolve to a
    /// [`BasicBlockId`] during the single lowering pass that follows.
    pub fn collect_labels(
        &mut self,
        hir_body: &rcc_hir::Body,
        stmt_id: rcc_hir::HirStmtId,
        local_map: &LocalMap,
    ) {
        let mut scopes = Vec::new();
        self.collect_labels_scoped(hir_body, stmt_id, local_map, &mut scopes);
    }

    fn collect_labels_scoped(
        &mut self,
        hir_body: &rcc_hir::Body,
        stmt_id: rcc_hir::HirStmtId,
        local_map: &LocalMap,
        scopes: &mut Vec<LabelScopeState>,
    ) {
        use rcc_hir::HirStmtKind;
        let stmt = &hir_body.stmts[stmt_id];
        match &stmt.kind {
            HirStmtKind::Label { name, body } => {
                let bb = self.new_block();
                self.insert_label_info(
                    *name,
                    LabelInfo {
                        block: bb,
                        scope_depth: scopes.len(),
                        runtime_scope_depth: deepest_scope_depth(scopes, |s| s.has_runtime_entry),
                        vla_scope_depth: deepest_scope_depth(scopes, |s| s.has_vla),
                        ordinary_locals_to_live: scopes
                            .iter()
                            .flat_map(|scope| scope.ordinary_locals.iter().copied())
                            .collect(),
                        locals_live_at_label: scopes
                            .iter()
                            .flat_map(|scope| scope.live_locals.iter().copied())
                            .collect(),
                        vla_locals_live_at_label: scopes
                            .iter()
                            .flat_map(|scope| scope.vla_locals.iter().copied())
                            .collect(),
                    },
                );
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
            }
            HirStmtKind::Block(stmts) => {
                scopes.push(LabelScopeState::default());
                for &s in stmts {
                    self.collect_labels_scoped(hir_body, s, local_map, scopes);
                }
                scopes.pop().expect("collect_labels: block scope stack underflow");
            }
            HirStmtKind::Expr(expr) => {
                self.collect_expr_labels(hir_body, *expr, local_map, scopes);
            }
            HirStmtKind::GotoComputed(expr) => {
                self.collect_expr_labels(hir_body, *expr, local_map, scopes);
            }
            HirStmtKind::If { cond, then_branch, else_branch } => {
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                self.collect_labels_scoped(hir_body, *then_branch, local_map, scopes);
                if let Some(else_b) = else_branch {
                    self.collect_labels_scoped(hir_body, *else_b, local_map, scopes);
                }
            }
            HirStmtKind::While { cond, body } => {
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
            }
            HirStmtKind::DoWhile { body, cond } => {
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
            }
            HirStmtKind::For { init, cond, step, body } => {
                scopes.push(LabelScopeState::default());
                if let Some(init_stmt) = init {
                    self.collect_labels_scoped(hir_body, *init_stmt, local_map, scopes);
                }
                if let Some(cond) = cond {
                    self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                }
                if let Some(step) = step {
                    self.collect_expr_labels(hir_body, *step, local_map, scopes);
                }
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
                scopes.pop().expect("collect_labels: for scope stack underflow");
            }
            HirStmtKind::Switch { cond, body, .. } => {
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
            }
            HirStmtKind::Case { body, .. } | HirStmtKind::Default { body } => {
                self.collect_labels_scoped(hir_body, *body, local_map, scopes);
            }
            HirStmtKind::Return(Some(expr)) => {
                self.collect_expr_labels(hir_body, *expr, local_map, scopes);
            }
            HirStmtKind::Return(None) => {}
            HirStmtKind::LocalDecl { local, init } => {
                if let Some(scope) = scopes.last_mut() {
                    let cfg_local = local_map.lookup(*local);
                    scope.has_runtime_entry = true;
                    if hir_body.locals[*local].vla_len.is_some() {
                        scope.has_vla = true;
                        scope.vla_locals.push(cfg_local);
                    } else {
                        scope.ordinary_locals.push(cfg_local);
                    }
                    scope.live_locals.push(cfg_local);
                }
                if let Some(vla_len) = hir_body.locals[*local].vla_len {
                    self.collect_expr_labels(hir_body, vla_len, local_map, scopes);
                }
                if let Some(init) = init {
                    self.collect_expr_labels(hir_body, *init, local_map, scopes);
                }
            }
            _ => {}
        }
    }

    fn collect_expr_labels(
        &mut self,
        hir_body: &rcc_hir::Body,
        expr_id: rcc_hir::HirExprId,
        local_map: &LocalMap,
        scopes: &mut Vec<LabelScopeState>,
    ) {
        use rcc_hir::HirExprKind;

        match &hir_body.exprs[expr_id].kind {
            HirExprKind::Binary { lhs, rhs, .. }
            | HirExprKind::Comma { lhs, rhs }
            | HirExprKind::Assign { lhs, rhs } => {
                self.collect_expr_labels(hir_body, *lhs, local_map, scopes);
                self.collect_expr_labels(hir_body, *rhs, local_map, scopes);
            }
            HirExprKind::Unary { operand, .. }
            | HirExprKind::Convert { operand, .. }
            | HirExprKind::Cast { operand, .. }
            | HirExprKind::SizeofExpr(operand)
            | HirExprKind::AddressOf(operand)
            | HirExprKind::Deref(operand) => {
                self.collect_expr_labels(hir_body, *operand, local_map, scopes);
            }
            HirExprKind::Call { callee, args } => {
                self.collect_expr_labels(hir_body, *callee, local_map, scopes);
                for arg in args {
                    self.collect_expr_labels(hir_body, *arg, local_map, scopes);
                }
            }
            HirExprKind::StmtExpr { stmts, result } => {
                scopes.push(LabelScopeState::default());
                for &stmt in stmts {
                    self.collect_labels_scoped(hir_body, stmt, local_map, scopes);
                }
                if let Some(result) = result {
                    self.collect_expr_labels(hir_body, *result, local_map, scopes);
                }
                scopes.pop().expect("collect_labels: statement expression scope stack underflow");
            }
            HirExprKind::UnresolvedField { base, .. } | HirExprKind::Field { base, .. } => {
                self.collect_expr_labels(hir_body, *base, local_map, scopes);
            }
            HirExprKind::Index { base, index } => {
                self.collect_expr_labels(hir_body, *base, local_map, scopes);
                self.collect_expr_labels(hir_body, *index, local_map, scopes);
            }
            HirExprKind::CompoundLiteral { init_stmts, .. } => {
                scopes.push(LabelScopeState::default());
                for &stmt in init_stmts {
                    self.collect_labels_scoped(hir_body, stmt, local_map, scopes);
                }
                scopes.pop().expect("collect_labels: compound literal scope stack underflow");
            }
            HirExprKind::VectorInit { lanes, .. } => {
                for lane in lanes {
                    self.collect_expr_labels(hir_body, *lane, local_map, scopes);
                }
            }
            HirExprKind::Cond { cond, then_expr, else_expr } => {
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                self.collect_expr_labels(hir_body, *then_expr, local_map, scopes);
                self.collect_expr_labels(hir_body, *else_expr, local_map, scopes);
            }
            HirExprKind::OmittedCond { cond, else_expr } => {
                self.collect_expr_labels(hir_body, *cond, local_map, scopes);
                self.collect_expr_labels(hir_body, *else_expr, local_map, scopes);
            }
            HirExprKind::BuiltinVaArg { ap, .. } | HirExprKind::BuiltinVaEnd { ap } => {
                self.collect_expr_labels(hir_body, *ap, local_map, scopes);
            }
            HirExprKind::BuiltinVaStart { ap, last_param } => {
                self.collect_expr_labels(hir_body, *ap, local_map, scopes);
                self.collect_expr_labels(hir_body, *last_param, local_map, scopes);
            }
            HirExprKind::BuiltinVaCopy { dst, src } => {
                self.collect_expr_labels(hir_body, *dst, local_map, scopes);
                self.collect_expr_labels(hir_body, *src, local_map, scopes);
            }
            HirExprKind::BuiltinExpect { value, expected } => {
                self.collect_expr_labels(hir_body, *value, local_map, scopes);
                self.collect_expr_labels(hir_body, *expected, local_map, scopes);
            }
            HirExprKind::BuiltinOverflow { lhs, rhs, dst, .. } => {
                self.collect_expr_labels(hir_body, *lhs, local_map, scopes);
                self.collect_expr_labels(hir_body, *rhs, local_map, scopes);
                self.collect_expr_labels(hir_body, *dst, local_map, scopes);
            }
            HirExprKind::BuiltinOverflowP { lhs, rhs, probe, .. } => {
                self.collect_expr_labels(hir_body, *lhs, local_map, scopes);
                self.collect_expr_labels(hir_body, *rhs, local_map, scopes);
                self.collect_expr_labels(hir_body, *probe, local_map, scopes);
            }
            HirExprKind::IntLiteral { .. }
            | HirExprKind::IntConst(_)
            | HirExprKind::FloatConst(_)
            | HirExprKind::StringRef(_)
            | HirExprKind::LocalRef(_)
            | HirExprKind::DefRef(_)
            | HirExprKind::LabelAddr(_)
            | HirExprKind::BuiltinVaArea
            | HirExprKind::SizeofType(_) => {}
        }
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
        let labels = self.label_map.iter().map(|(name, info)| (*name, info.block)).collect();
        Body {
            def: self.def,
            locals: self.locals,
            blocks: self.blocks,
            labels,
            ret_ty: self.ret_ty,
        }
    }
}

fn deepest_scope_depth(
    scopes: &[LabelScopeState],
    pred: impl Fn(&LabelScopeState) -> bool,
) -> Option<usize> {
    scopes.iter().enumerate().rev().find_map(|(idx, scope)| pred(scope).then_some(idx + 1))
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
        TerminatorKind::IndirectGoto { targets, .. } => targets.clone(),
        TerminatorKind::SwitchInt { targets, .. } => targets.iter().map(|(_, t)| *t).collect(),
        TerminatorKind::Call { target: Some(t), .. } => vec![*t],
        TerminatorKind::Call { target: None, .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
        TerminatorKind::BuiltinVaStart { target, .. }
        | TerminatorKind::BuiltinVaEnd { target, .. }
        | TerminatorKind::BuiltinVaCopy { target, .. } => vec![*target],
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
            quals: ObjectQuals::none(),
            vla_len: None,
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
