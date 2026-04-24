//! `rcc_hir_lower`: AST -> HIR lowering.
//!
//! Analogous to `rustc_ast_lowering`. Responsibilities:
//!
//! 1. Resolve identifiers against three *separate* C name spaces
//!    (ordinary / tag / label).
//! 2. Flatten declarators (`int (*fp[3])(int,int)`) into `Ty`.
//! 3. Expand `typedef` references.
//! 4. Assign `DefId`s and `HirId`s.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_ast::{
    BlockItem, Declarator, DerivedDeclarator, ExternalDecl, Stmt, StmtKind, StorageClass,
    TranslationUnit, TypeSpec,
};
use rcc_data_structures::FxHashMap;
use rcc_data_structures::FxHashSet;
use rcc_hir::ty::{Qual, Ty};
use rcc_hir::{
    Def, DefId, DefKind, HirCrate, HirExprKind, Linkage, Local, RecordKind, TyCtxt, TyId,
};
use rcc_session::Session;
use rcc_span::{Span, Symbol};

/// Entry point: lower an AST into a fresh `HirCrate`.
///
/// Currently implements only the first-pass DefId assignment (task 06-01).
/// Further lowering (name resolution, type flattening, etc.) will be added
/// in subsequent tasks.
pub fn lower(ast: &TranslationUnit, tcx: &mut TyCtxt, session: &mut Session) -> HirCrate {
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    assign_def_ids(ast, tcx, session, &mut crate_, &mut resolver);
    crate_
}

/// Per-crate resolution tables built while lowering.
#[derive(Default, Debug)]
pub struct Resolver {
    /// Ordinary namespace: (name) -> `DefId`.
    pub ordinary: FxHashMap<Symbol, DefId>,
    /// Tag namespace: `struct`/`union`/`enum` tags.
    pub tags: FxHashMap<Symbol, DefId>,
    /// Labels are strictly per-function; populated then flushed per body.
    pub labels: FxHashMap<Symbol, rcc_hir::HirStmtId>,
}

/// A binding visible in a particular scope.
#[derive(Copy, Clone, Debug)]
pub enum Binding {
    /// A function-scoped local (parameter or declared variable).
    Local(Local),
    /// A top-level definition (function, global, typedef, etc.).
    Def(DefId),
}

/// Per-body scope stack for ordinary-namespace resolution.
///
/// Each scope frame is a `HashMap<Symbol, Binding>`. The stack is
/// pushed when entering a compound statement and popped on exit.
/// File-scope lookup falls through to the `Resolver::ordinary` table.
#[derive(Debug)]
pub struct ScopeStack {
    /// Stack of scope frames (innermost last).
    frames: Vec<FxHashMap<Symbol, Binding>>,
}

impl ScopeStack {
    /// Build a new, empty scope stack.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Push a new empty scope frame.
    pub fn push_scope(&mut self) {
        self.frames.push(FxHashMap::default());
    }

    /// Pop the innermost scope frame.
    pub fn pop_scope(&mut self) {
        self.frames.pop();
    }

    /// Insert a binding in the innermost scope frame.
    ///
    /// Panics if no scope frame is active.
    pub fn insert(&mut self, name: Symbol, binding: Binding) {
        self.frames.last_mut().expect("no active scope frame").insert(name, binding);
    }

    /// Lookup a name by walking from the innermost scope outward.
    /// Returns `None` if not found in any frame.
    pub fn lookup(&self, name: Symbol) -> Option<Binding> {
        for frame in self.frames.iter().rev() {
            if let Some(&b) = frame.get(&name) {
                return Some(b);
            }
        }
        None
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve an `ExprKind::Ident` to either a `LocalRef` or a `DefRef`.
///
/// Searches the body-local `scope` first (parameters, locals), then
/// falls through to the file-scope `resolver.ordinary` table.
///
/// On failure (undeclared identifier), emits `E0071` with a `help:`
/// suggesting similarly-named symbols if any exist within edit-distance 3.
/// Returns `None` so the caller can decide how to represent the error
/// expression.
pub fn resolve_expr_ident(
    ident: Symbol,
    ident_span: Span,
    scope: &ScopeStack,
    resolver: &Resolver,
    session: &mut Session,
) -> Option<HirExprKind> {
    // 1. Search local scope stack (innermost-first).
    if let Some(binding) = scope.lookup(ident) {
        return Some(match binding {
            Binding::Local(local) => HirExprKind::LocalRef(local),
            Binding::Def(def_id) => HirExprKind::DefRef(def_id),
        });
    }

    // 2. Fall through to file-scope ordinary namespace.
    if let Some(&def_id) = resolver.ordinary.get(&ident) {
        return Some(HirExprKind::DefRef(def_id));
    }

    // 3. Not found — emit E0071.
    let ident_str = session.interner.get(ident);

    // Collect candidates from both file-scope and all scope frames.
    let mut candidates: Vec<(String, u32)> = Vec::new();
    let mut seen = rcc_data_structures::FxHashSet::default();

    // File-scope candidates.
    for &sym in resolver.ordinary.keys() {
        if seen.insert(sym) {
            let name = session.interner.get(sym).to_owned();
            let dist = edit_distance(ident_str, &name);
            if dist <= 3 && dist > 0 {
                candidates.push((name, dist));
            }
        }
    }

    // Local scope candidates.
    for frame in &scope.frames {
        for &sym in frame.keys() {
            if seen.insert(sym) {
                let name = session.interner.get(sym).to_owned();
                let dist = edit_distance(ident_str, &name);
                if dist <= 3 && dist > 0 {
                    candidates.push((name, dist));
                }
            }
        }
    }

    // Sort by distance, then alphabetically.
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut builder = session
        .handler
        .struct_err(ident_span, format!("use of undeclared identifier `{ident_str}`"))
        .code(rcc_errors::codes::E0071);

    if let Some((best, _)) = candidates.first() {
        builder = builder.help(format!("did you mean `{best}`?"));
    }

    builder.emit();

    None
}

/// The kind of a tag: struct, union, or enum.
///
/// Used by [`resolve_tag`] to verify that a tag reference uses the same
/// keyword as the original declaration (C99 §6.7.2.3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TagKind {
    /// `struct`
    Struct,
    /// `union`
    Union,
    /// `enum`
    Enum,
}

impl TagKind {
    /// Human-readable keyword for diagnostics.
    pub fn keyword(self) -> &'static str {
        match self {
            TagKind::Struct => "struct",
            TagKind::Union => "union",
            TagKind::Enum => "enum",
        }
    }
}

impl From<RecordKind> for TagKind {
    fn from(kind: RecordKind) -> Self {
        match kind {
            RecordKind::Struct => TagKind::Struct,
            RecordKind::Union => TagKind::Union,
        }
    }
}

/// Resolve a struct/union/enum tag reference.
///
/// Looks up `tag` in `resolver.tags`. If found, checks that the stored
/// definition has the same `TagKind` as `expected_kind`. On mismatch,
/// emits `E0072` and returns `None`.
///
/// If the tag is not yet in the table (forward declaration), creates a
/// new incomplete `Def` of the appropriate kind, registers it, and
/// returns the fresh `DefId`.
pub fn resolve_tag(
    tag: Symbol,
    tag_span: Span,
    expected_kind: TagKind,
    crate_: &mut HirCrate,
    tcx: &TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<DefId> {
    if let Some(&existing_id) = resolver.tags.get(&tag) {
        // Check kind matches.
        let def = &crate_.defs[existing_id];
        let actual_kind = match &def.kind {
            DefKind::Record { kind, .. } => TagKind::from(*kind),
            DefKind::Enum { .. } => TagKind::Enum,
            _ => {
                // Should not happen — tags table only has Record/Enum.
                return Some(existing_id);
            }
        };
        if actual_kind != expected_kind {
            let tag_str = session.interner.get(tag);
            session
                .handler
                .struct_err(
                    tag_span,
                    format!(
                        "use of `{tag_str}` as `{}` but previously declared as `{}`",
                        expected_kind.keyword(),
                        actual_kind.keyword(),
                    ),
                )
                .code(rcc_errors::codes::E0072)
                .emit();
            return None;
        }
        Some(existing_id)
    } else {
        // Forward declaration — create an incomplete def.
        let kind = match expected_kind {
            TagKind::Struct => {
                DefKind::Record { kind: RecordKind::Struct, layout: None, fields: Vec::new() }
            }
            TagKind::Union => {
                DefKind::Record { kind: RecordKind::Union, layout: None, fields: Vec::new() }
            }
            TagKind::Enum => DefKind::Enum { repr: tcx.int, variants: Vec::new() },
        };
        let id = crate_.defs.push(Def { id: DefId(0), name: tag, span: tag_span, kind });
        crate_.defs[id].id = id;
        resolver.tags.insert(tag, id);
        Some(id)
    }
}

/// Two-pass label resolution for a single function body.
///
/// **Pass 1** — collect: walks every statement in the function body
/// and records each `StmtKind::Label { name, .. }` in
/// `resolver.labels`. Duplicate labels (same name in the same
/// function) emit `E0074`.
///
/// **Pass 2** — check: walks every statement again and verifies that
/// each `StmtKind::Goto(name)` references a label collected in pass 1.
/// Unknown labels emit `E0073`.
///
/// The caller must clear `resolver.labels` before calling this for
/// each function, ensuring labels are strictly per-function.
pub fn resolve_labels(body: &rcc_ast::Block, resolver: &mut Resolver, session: &mut Session) {
    // Pass 1: collect all labels.
    for item in &body.items {
        collect_labels_in_block_item(item, resolver, session);
    }
    // Pass 2: check all gotos.
    for item in &body.items {
        check_gotos_in_block_item(item, resolver, session);
    }
}

/// Recursively collect labels from a block item.
fn collect_labels_in_block_item(item: &BlockItem, resolver: &mut Resolver, session: &mut Session) {
    match item {
        BlockItem::Stmt(stmt) => collect_labels_in_stmt(stmt, resolver, session),
        BlockItem::Decl(_) => {}
    }
}

/// Recursively collect labels from a statement.
fn collect_labels_in_stmt(stmt: &Stmt, resolver: &mut Resolver, session: &mut Session) {
    match &stmt.kind {
        StmtKind::Label { name, body } => {
            // Check for duplicate label.
            if resolver.labels.contains_key(name) {
                let name_str = session.interner.get(*name);
                session
                    .handler
                    .struct_err(stmt.span, format!("duplicate label `{name_str}`"))
                    .code(rcc_errors::codes::E0074)
                    .emit();
            } else {
                // Use HirStmtId(0) as a placeholder; the real id is
                // assigned later during full statement lowering.
                resolver.labels.insert(*name, rcc_hir::HirStmtId(0));
            }
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::Compound(block) => {
            for item in &block.items {
                collect_labels_in_block_item(item, resolver, session);
            }
        }
        StmtKind::If { then_branch, else_branch, .. } => {
            collect_labels_in_stmt(then_branch, resolver, session);
            if let Some(else_) = else_branch {
                collect_labels_in_stmt(else_, resolver, session);
            }
        }
        StmtKind::While { body, .. } => {
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::DoWhile { body, .. } => {
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::For { init, body, .. } => {
            if let Some(init) = init {
                if let BlockItem::Stmt(s) = init.as_ref() {
                    collect_labels_in_stmt(s, resolver, session);
                }
            }
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::Switch { body, .. } => {
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::Case { body, .. } => {
            collect_labels_in_stmt(body, resolver, session);
        }
        StmtKind::Default { body } => {
            collect_labels_in_stmt(body, resolver, session);
        }
        // Terminal statements — no sub-statements to recurse into.
        StmtKind::Expr(_)
        | StmtKind::Goto(_)
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Return(_)
        | StmtKind::Null => {}
    }
}

/// Recursively check gotos in a block item.
fn check_gotos_in_block_item(item: &BlockItem, resolver: &mut Resolver, session: &mut Session) {
    match item {
        BlockItem::Stmt(stmt) => check_gotos_in_stmt(stmt, resolver, session),
        BlockItem::Decl(_) => {}
    }
}

/// Recursively check that every `goto` references a known label.
fn check_gotos_in_stmt(stmt: &Stmt, resolver: &mut Resolver, session: &mut Session) {
    match &stmt.kind {
        StmtKind::Goto(name) => {
            if !resolver.labels.contains_key(name) {
                let name_str = session.interner.get(*name);
                session
                    .handler
                    .struct_err(stmt.span, format!("use of undeclared label `{name_str}`"))
                    .code(rcc_errors::codes::E0073)
                    .emit();
            }
        }
        StmtKind::Label { body, .. } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::Compound(block) => {
            for item in &block.items {
                check_gotos_in_block_item(item, resolver, session);
            }
        }
        StmtKind::If { then_branch, else_branch, .. } => {
            check_gotos_in_stmt(then_branch, resolver, session);
            if let Some(else_) = else_branch {
                check_gotos_in_stmt(else_, resolver, session);
            }
        }
        StmtKind::While { body, .. } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::DoWhile { body, .. } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::For { init, body, .. } => {
            if let Some(init) = init {
                if let BlockItem::Stmt(s) = init.as_ref() {
                    check_gotos_in_stmt(s, resolver, session);
                }
            }
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::Switch { body, .. } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::Case { body, .. } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        StmtKind::Default { body } => {
            check_gotos_in_stmt(body, resolver, session);
        }
        // Terminal statements — no sub-statements to recurse into.
        StmtKind::Expr(_)
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Return(_)
        | StmtKind::Null => {}
    }
}

/// Levenshtein edit distance between two strings.
fn edit_distance(a: &str, b: &str) -> u32 {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let m = a_bytes.len();
    let n = b_bytes.len();

    // Quick exit for length difference > 3.
    if m.abs_diff(n) > 3 {
        return 4; // sentinel > 3
    }

    let mut prev: Vec<u32> = (0..=(n as u32)).collect();
    let mut curr = vec![0u32; n + 1];

    for i in 1..=m {
        curr[0] = i as u32;
        for j in 1..=n {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Resolve a `TypeSpec::TypedefName(sym)` to the underlying `TyId`.
///
/// Looks up `sym` in the ordinary namespace, verifies the binding is a
/// `DefKind::Typedef(ty_id)`, and returns the stored `TyId`. If the
/// typedef's stored type was itself produced by expanding another typedef
/// (e.g. `typedef int T; typedef T U;`), the caller is expected to have
/// already resolved `T` before `U`, so `U`'s stored `TyId` is already
/// the final interned type (`tcx.int`).
///
/// Cycle detection: uses `expanding` to track which `DefId`s are currently
/// being resolved. If a typedef chain revisits a `DefId` already in the
/// set, emits `E0075` and returns `tcx.error`.
///
/// # Arguments
///
/// * `sym` — the typedef name to look up.
/// * `span` — source span for diagnostics.
/// * `expanding` — mutable set of `DefId`s currently being expanded in
///   the caller's resolution pass. The caller must insert IDs before
///   recursing and remove them afterward.
/// * `resolver` — file-scope name tables.
/// * `crate_` — the `HirCrate` holding all `Def` nodes.
/// * `tcx` — the type interner.
/// * `session` — the compilation session (for diagnostics + interner).
pub fn lower_typedef_name(
    sym: Symbol,
    span: Span,
    expanding: &mut FxHashSet<DefId>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    session: &mut Session,
) -> TyId {
    // Look up the symbol in the ordinary namespace.
    let def_id = match resolver.ordinary.get(&sym) {
        Some(&id) => id,
        None => {
            // Not found — emit undeclared identifier. This shouldn't
            // normally happen because the parser only produces
            // TypedefName when the name was seen as a typedef, but
            // handle gracefully.
            let sym_str = session.interner.get(sym);
            session
                .handler
                .struct_err(span, format!("use of undeclared typedef `{sym_str}`"))
                .code(rcc_errors::codes::E0071)
                .emit();
            return tcx.error;
        }
    };

    // Cycle detection: if this DefId is already being expanded, we have
    // a typedef cycle (e.g. `typedef U T; typedef T U;`).
    if expanding.contains(&def_id) {
        let sym_str = session.interner.get(sym);
        session
            .handler
            .struct_err(span, format!("typedef cycle detected for `{sym_str}`"))
            .code(rcc_errors::codes::E0075)
            .emit();
        return tcx.error;
    }

    let def = &crate_.defs[def_id];
    match &def.kind {
        DefKind::Typedef(ty_id) => *ty_id,
        _ => {
            // The symbol is not a typedef — this is unexpected when
            // processing TypeSpec::TypedefName, but handle gracefully.
            let sym_str = session.interner.get(sym);
            session
                .handler
                .struct_err(span, format!("`{sym_str}` is not a typedef"))
                .code(rcc_errors::codes::E0071)
                .emit();
            tcx.error
        }
    }
}

/// Whether the declaration occurs at file scope, function (block) scope,
/// or as a function parameter.
///
/// This distinction matters for incomplete array types: `int arr[]` at
/// file scope is a valid tentative definition (incomplete type with
/// `len = None`), at function scope it is an error because locals
/// must have a complete type, but as a parameter it is legal because
/// C99 §6.7.5.3p7 adjusts array parameters to pointers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeclScope {
    /// File-scope (translation-unit level).
    File,
    /// Function / block scope (inside a function body).
    Block,
    /// Function parameter scope (incomplete arrays are allowed because
    /// of the array-to-pointer adjustment).
    Param,
}

/// Convert AST `TypeQuals` to HIR `Qual` wrapping a given `TyId`.
fn quals_to_hir(base: TyId, q: &rcc_ast::TypeQuals) -> Qual {
    Qual { ty: base, is_const: q.const_, is_volatile: q.volatile, is_restrict: q.restrict }
}

/// Fold a parsed `Declarator` (name + chain of `DerivedDeclarator`) over
/// a base `Ty` obtained from `DeclSpecs`. Produces the final `TyId` for
/// the declared name; applies qualifiers correctly.
///
/// The `derived` chain is stored outermost-to-innermost (as parsed).
/// "Outermost" = the derivation farthest from the identifier in the
/// reading rule; "innermost" = closest to the identifier. We iterate
/// the chain in **forward order** so that the outermost derivation wraps
/// the base type first, building the type inside-out.
///
/// For example, `int (*fp[3])(int)`:
///
/// ```text
/// parsed derived chain: [Function([int]), Pointer, Array(3)]
///                        ^^^^^^^^^^^^^^^^  ^^^^^^^  ^^^^^^^^
///                        outermost         middle   innermost
/// ```
///
/// Forward iteration: Func(int)->int, then Ptr, then Array[3] →
/// `Array[3] of Ptr to Func(int)->int`.
///
/// # Errors
///
/// Emits `E0076` for illegal declarator forms:
/// - `void x;` (object of type void)
/// - function returning array
/// - function returning function
///
/// Returns `tcx.error` after emitting the diagnostic so lowering can
/// continue.
pub fn apply_declarator(
    base: TyId,
    d: &Declarator,
    scope: DeclScope,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> TyId {
    let mut ty = base;

    // Iterate the derived chain in forward order (outermost-to-innermost).
    for dd in d.derived.iter() {
        match dd {
            DerivedDeclarator::Pointer(quals) => {
                // Build a Ptr whose pointee is the current type + qualifiers.
                let qual = quals_to_hir(ty, quals);
                ty = tcx.intern(Ty::Ptr(qual));
            }
            DerivedDeclarator::Array(arr_decl) => {
                // C99 §6.7.5.2: the element type shall be a complete object
                // type. In particular, void arrays are illegal.
                if *tcx.get(ty) == Ty::Void {
                    session
                        .handler
                        .struct_err(d.span, "array element type cannot be `void`".to_string())
                        .code(rcc_errors::codes::E0076)
                        .emit();
                    return tcx.error;
                }

                // Evaluate constant size expression (stub: only integer
                // literal constants for now; VLA deferred).
                let len = if arr_decl.star {
                    // [*] — VLA of unspecified size.
                    None
                } else if let Some(ref size_expr) = arr_decl.size {
                    // Try to evaluate as a constant integer.
                    eval_const_expr_as_u64(size_expr, &session.interner)
                } else {
                    // No size — incomplete array.
                    // At block scope, incomplete arrays without an initializer
                    // are an error. However, the initializer check happens
                    // at a higher level — here we just produce the incomplete
                    // type and let the caller validate.
                    if scope == DeclScope::Block {
                        session
                            .handler
                            .struct_err(d.span, "incomplete array type at block scope".to_string())
                            .code(rcc_errors::codes::E0076)
                            .emit();
                        return tcx.error;
                    }
                    None
                };

                let elem = quals_to_hir(ty, &arr_decl.quals);
                ty = tcx.intern(Ty::Array { elem, len, is_vla: arr_decl.star });
            }
            DerivedDeclarator::Function(func_decl) => {
                // C99 §6.7.5.3p1: the return type shall not be an array
                // or function type.
                match tcx.get(ty) {
                    Ty::Array { .. } => {
                        session
                            .handler
                            .struct_err(d.span, "function cannot return array type".to_string())
                            .code(rcc_errors::codes::E0076)
                            .emit();
                        return tcx.error;
                    }
                    Ty::Func { .. } => {
                        session
                            .handler
                            .struct_err(d.span, "function cannot return function type".to_string())
                            .code(rcc_errors::codes::E0076)
                            .emit();
                        return tcx.error;
                    }
                    _ => {}
                }

                // Lower parameter types.
                let mut param_tys = Vec::new();
                for param in &func_decl.params {
                    let param_base = lower_declspecs_to_base_ty(&param.specs, tcx, session);
                    let param_ty = apply_declarator(
                        param_base,
                        &param.declarator,
                        DeclScope::Param,
                        tcx,
                        session,
                    );
                    // C99 §6.7.5.3p7: array parameter types are
                    // adjusted to pointer-to-element. Function parameter
                    // types are adjusted to pointer-to-function.
                    let adjusted = adjust_param_type(param_ty, tcx);
                    param_tys.push(adjusted);
                }

                let proto = func_decl.is_void || !func_decl.params.is_empty();
                ty = tcx.intern(Ty::Func {
                    ret: ty,
                    params: param_tys,
                    variadic: func_decl.variadic,
                    proto,
                });
            }
        }
    }

    // Final check: if after all derivations the type is still void
    // and the declarator has a name (i.e. it's an object, not a
    // return type or parameter), reject it.
    // But only if there were no derivations — if there were
    // derivations, void was either wrapped in a pointer (legal) or
    // caught above.
    if d.derived.is_empty() && *tcx.get(ty) == Ty::Void && d.name.is_some() {
        session
            .handler
            .struct_err(d.span, "cannot declare variable of type `void`".to_string())
            .code(rcc_errors::codes::E0076)
            .emit();
        return tcx.error;
    }

    ty
}

/// Adjust a parameter type per C99 §6.7.5.3p7-8:
/// - Array of T -> pointer to T
/// - Function -> pointer to function
fn adjust_param_type(ty: TyId, tcx: &mut TyCtxt) -> TyId {
    match tcx.get(ty).clone() {
        Ty::Array { elem, .. } => {
            // Decay to pointer to element type.
            tcx.intern(Ty::Ptr(elem))
        }
        Ty::Func { .. } => {
            // Decay to pointer to function.
            tcx.intern(Ty::Ptr(Qual::plain(ty)))
        }
        _ => ty,
    }
}

/// Minimal DeclSpecs-to-base-type lowering for parameter declarations.
///
/// This is a simplified version that handles the common cases needed by
/// `apply_declarator` when lowering function parameter types. A full
/// implementation lives in a later task; here we cover the basics:
/// `void`, `int`, `char`, `short`, `long`, `long long`, `float`,
/// `double`, `signed`/`unsigned` variants, and `_Bool`.
fn lower_declspecs_to_base_ty(
    specs: &rcc_ast::DeclSpecs,
    tcx: &mut TyCtxt,
    _session: &mut Session,
) -> TyId {
    // Classify the type specifiers.
    let mut has_void = false;
    let mut has_char = false;
    let mut has_short = false;
    let mut has_int = false;
    let mut long_count: u32 = 0;
    let mut has_float = false;
    let mut has_double = false;
    let mut has_signed = false;
    let mut has_unsigned = false;
    let mut has_bool = false;

    for ts in &specs.type_specs {
        match ts {
            TypeSpec::Void => has_void = true,
            TypeSpec::Char => has_char = true,
            TypeSpec::Short => has_short = true,
            TypeSpec::Int => has_int = true,
            TypeSpec::Long => long_count += 1,
            TypeSpec::Float => has_float = true,
            TypeSpec::Double => has_double = true,
            TypeSpec::Signed => has_signed = true,
            TypeSpec::Unsigned => has_unsigned = true,
            TypeSpec::Bool => has_bool = true,
            _ => {
                // TypedefName, Record, Enum, Complex, Imaginary
                // are handled by later tasks.
            }
        }
    }

    if has_void {
        return tcx.void;
    }
    if has_bool {
        return tcx.bool_;
    }
    if has_float {
        return tcx.float;
    }
    if has_double && long_count >= 1 {
        return tcx.long_double;
    }
    if has_double {
        return tcx.double;
    }
    if has_char {
        return if has_unsigned { tcx.uchar } else { tcx.char_ };
    }
    if has_short {
        return if has_unsigned { tcx.ushort } else { tcx.short };
    }
    if long_count >= 2 {
        return if has_unsigned { tcx.ulong_long } else { tcx.long_long };
    }
    if long_count == 1 {
        return if has_unsigned { tcx.ulong } else { tcx.long };
    }
    if has_unsigned {
        return tcx.uint;
    }
    // Default: signed int (covers `int`, `signed`, `signed int`, and
    // empty specifier list which defaults to int).
    if has_int || has_signed || specs.type_specs.is_empty() {
        return tcx.int;
    }

    // Fallback for unrecognised combos — tcx.int is a safe default.
    tcx.int
}

/// Stub constant-expression evaluator for array sizes.
///
/// Handles only integer literals for now. A full `ConstEval` lives in
/// `rcc_typeck` and will be wired in later.
fn eval_const_expr_as_u64(expr: &rcc_ast::Expr, interner: &rcc_span::Interner) -> Option<u64> {
    match &expr.kind {
        rcc_ast::ExprKind::IntLit { text } => {
            // The text is the raw literal string. Parse it.
            // Handle hex (0x), octal (0), and decimal.
            let s = interner.get(*text);
            // Strip any suffix (u, U, l, L, ll, LL, etc.)
            let s = s.trim_end_matches(['u', 'U', 'l', 'L']);
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else if s.starts_with('0') && s.len() > 1 {
                u64::from_str_radix(s, 8).ok()
            } else {
                s.parse::<u64>().ok()
            }
        }
        rcc_ast::ExprKind::Paren(inner) => eval_const_expr_as_u64(inner, interner),
        _ => None,
    }
}

/// First-pass: walk the AST top-level and assign a `DefId` to every
/// function definition, global variable, typedef, and struct/union/enum tag.
///
/// Populates `crate_.defs`, `resolver.ordinary`, and `resolver.tags`.
/// Conflict detection is deferred to task 02.
fn assign_def_ids(
    ast: &TranslationUnit,
    tcx: &TyCtxt,
    _session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    for ext_decl in &ast.decls {
        match ext_decl {
            ExternalDecl::Function(func_def) => {
                // Function definition — extract name from declarator.
                if let Some((name, _span)) = func_def.declarator.name {
                    let id = crate_.defs.push(Def {
                        id: DefId(0), // patched below
                        name,
                        span: func_def.span,
                        kind: DefKind::Function {
                            ty: tcx.error,
                            has_body: true,
                            is_static: func_def.specs.storage == Some(StorageClass::Static),
                            is_inline: func_def.specs.func_specs.inline,
                            variadic: false,
                        },
                    });
                    crate_.defs[id].id = id;
                    resolver.ordinary.insert(name, id);
                }
            }
            ExternalDecl::Decl(decl) => {
                let is_typedef = decl.specs.storage == Some(StorageClass::Typedef);

                // Scan type specifiers for tag definitions (struct/union/enum).
                for ts in &decl.specs.type_specs {
                    match ts {
                        TypeSpec::Record(rec) => {
                            // Only register when defining (fields present) and tag exists.
                            if let (Some(tag), Some(_fields)) = (rec.tag, &rec.fields) {
                                let kind = match rec.kind {
                                    rcc_ast::RecordKind::Struct => RecordKind::Struct,
                                    rcc_ast::RecordKind::Union => RecordKind::Union,
                                };
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name: tag,
                                    span: rec.span,
                                    kind: DefKind::Record {
                                        kind,
                                        layout: None,
                                        fields: Vec::new(),
                                    },
                                });
                                crate_.defs[id].id = id;
                                resolver.tags.insert(tag, id);
                            }
                        }
                        TypeSpec::Enum(en) => {
                            // Only register when defining (enumerators present) and tag exists.
                            if let (Some(tag), Some(_enumerators)) = (en.tag, &en.enumerators) {
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name: tag,
                                    span: en.span,
                                    kind: DefKind::Enum { repr: tcx.int, variants: Vec::new() },
                                });
                                crate_.defs[id].id = id;
                                resolver.tags.insert(tag, id);
                            }
                        }
                        _ => {}
                    }
                }

                // Process each init-declarator.
                for init_decl in &decl.inits {
                    if let Some((name, _span)) = init_decl.declarator.name {
                        if is_typedef {
                            let id = crate_.defs.push(Def {
                                id: DefId(0),
                                name,
                                span: decl.span,
                                kind: DefKind::Typedef(tcx.error),
                            });
                            crate_.defs[id].id = id;
                            resolver.ordinary.insert(name, id);
                        } else {
                            // Global variable (or extern declaration).
                            let linkage = match decl.specs.storage {
                                Some(StorageClass::Static) => Linkage::Internal,
                                Some(StorageClass::Extern) => Linkage::External,
                                _ => Linkage::External,
                            };
                            let id = crate_.defs.push(Def {
                                id: DefId(0),
                                name,
                                span: decl.span,
                                kind: DefKind::Global { ty: tcx.error, linkage },
                            });
                            crate_.defs[id].id = id;
                            resolver.ordinary.insert(name, id);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_ast::{
        Block, BlockItem, Decl, DeclSpecs, Declarator, EnumSpec, ExternalDecl, FunctionDef,
        InitDeclarator, NodeId, RecordSpec, Stmt, StmtKind, TranslationUnit, TypeSpec,
    };
    use rcc_hir::TyCtxt;
    use rcc_session::Session;
    use rcc_span::DUMMY_SP;

    /// Helper: intern a name symbol via the session interner.
    fn sym(sess: &mut Session, s: &str) -> Symbol {
        sess.interner.intern(s)
    }

    /// Helper: build a minimal declarator with just a name.
    fn named_declarator(name: Symbol) -> Declarator {
        Declarator { name: Some((name, DUMMY_SP)), derived: Vec::new(), span: DUMMY_SP }
    }

    /// Helper: default DeclSpecs (no storage class, empty type specs).
    fn default_specs() -> DeclSpecs {
        DeclSpecs::default()
    }

    /// Helper: a minimal empty compound block (function body).
    fn empty_body() -> Block {
        Block { id: NodeId(0), items: Vec::new(), span: DUMMY_SP }
    }

    /// Helper: make a function definition `ExternalDecl`.
    fn make_func(name: Symbol) -> ExternalDecl {
        ExternalDecl::Function(FunctionDef {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: default_specs(),
            declarator: named_declarator(name),
            kr_decls: Vec::new(),
            body: empty_body(),
        })
    }

    /// Helper: make a global variable declaration `ExternalDecl`.
    fn make_global(name: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Int);
                s
            },
            inits: vec![InitDeclarator { declarator: named_declarator(name), init: None }],
        })
    }

    /// Helper: make a typedef declaration `ExternalDecl`.
    fn make_typedef(name: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.storage = Some(StorageClass::Typedef);
                s.type_specs.push(TypeSpec::Int);
                s
            },
            inits: vec![InitDeclarator { declarator: named_declarator(name), init: None }],
        })
    }

    /// Helper: make a `struct tag { ... }` declaration (defining, with empty fields).
    fn make_struct(tag: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Record(RecordSpec {
                    id: NodeId(0),
                    kind: rcc_ast::RecordKind::Struct,
                    tag: Some(tag),
                    fields: Some(Vec::new()),
                    span: DUMMY_SP,
                }));
                s
            },
            inits: Vec::new(),
        })
    }

    /// Helper: make an `enum tag { ... }` declaration (defining, with empty enumerators).
    fn make_enum(tag: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Enum(EnumSpec {
                    id: NodeId(0),
                    tag: Some(tag),
                    enumerators: Some(Vec::new()),
                    span: DUMMY_SP,
                }));
                s
            },
            inits: Vec::new(),
        })
    }

    #[test]
    fn empty_tu_produces_no_defs() {
        let ast = TranslationUnit { decls: Vec::new(), span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let (mut sess, _cap) = Session::for_test();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 0);
    }

    #[test]
    fn single_function_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "main");
        let ast = TranslationUnit { decls: vec![make_func(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert_eq!(hir.defs[DefId(0)].name, name);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Function { .. }));
    }

    #[test]
    fn global_variable_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "counter");
        let ast = TranslationUnit { decls: vec![make_global(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Global { .. }));
    }

    #[test]
    fn typedef_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "uint32");
        let ast = TranslationUnit { decls: vec![make_typedef(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Typedef(_)));
    }

    #[test]
    fn struct_tag_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "point");
        let ast = TranslationUnit { decls: vec![make_struct(tag)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(
            hir.defs[DefId(0)].kind,
            DefKind::Record { kind: RecordKind::Struct, .. }
        ));
    }

    #[test]
    fn enum_tag_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "color");
        let ast = TranslationUnit { decls: vec![make_enum(tag)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Enum { .. }));
    }

    #[test]
    fn mixed_toplevel_assigns_correct_count() {
        // Simulate:  void f(); int g; typedef int T; struct S {}; enum E {};
        // Expected: 5 DefIds total.
        let (mut sess, _cap) = Session::for_test();
        let f = sym(&mut sess, "f");
        let g = sym(&mut sess, "g");
        let t = sym(&mut sess, "T");
        let s = sym(&mut sess, "S");
        let e = sym(&mut sess, "E");

        let ast = TranslationUnit {
            decls: vec![
                make_func(f),
                make_global(g),
                make_typedef(t),
                make_struct(s),
                make_enum(e),
            ],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 5, "expected 5 defs, got {}", hir.defs.len());
    }

    #[test]
    fn def_ids_are_sequential() {
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");
        let c = sym(&mut sess, "c");

        let ast = TranslationUnit {
            decls: vec![make_func(a), make_global(b), make_typedef(c)],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 3);
        for (i, (id, def)) in hir.defs.iter_enumerated().enumerate() {
            assert_eq!(id.0 as usize, i, "DefId should be sequential");
            assert_eq!(def.id, id, "Def.id should match its index");
        }
    }

    #[test]
    fn resolver_ordinary_populated() {
        let (mut sess, _cap) = Session::for_test();
        let f = sym(&mut sess, "f");
        let g = sym(&mut sess, "g");

        let ast = TranslationUnit { decls: vec![make_func(f), make_global(g)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        assert_eq!(resolver.ordinary.len(), 2);
        assert!(resolver.ordinary.contains_key(&f));
        assert!(resolver.ordinary.contains_key(&g));
    }

    #[test]
    fn resolver_tags_populated() {
        let (mut sess, _cap) = Session::for_test();
        let s = sym(&mut sess, "S");
        let e = sym(&mut sess, "E");

        let ast = TranslationUnit { decls: vec![make_struct(s), make_enum(e)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        assert_eq!(resolver.tags.len(), 2);
        assert!(resolver.tags.contains_key(&s));
        assert!(resolver.tags.contains_key(&e));
        // Tags should NOT appear in ordinary namespace.
        assert!(resolver.ordinary.is_empty());
    }

    #[test]
    fn struct_with_tag_variable_produces_two_defs() {
        // `struct S { int x; } s;` — one tag def + one global variable def.
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "S");
        let var = sym(&mut sess, "s");

        let ast = TranslationUnit {
            decls: vec![ExternalDecl::Decl(Decl {
                id: NodeId(0),
                span: DUMMY_SP,
                specs: {
                    let mut s = default_specs();
                    s.type_specs.push(TypeSpec::Record(RecordSpec {
                        id: NodeId(0),
                        kind: rcc_ast::RecordKind::Struct,
                        tag: Some(tag),
                        fields: Some(Vec::new()),
                        span: DUMMY_SP,
                    }));
                    s
                },
                inits: vec![InitDeclarator { declarator: named_declarator(var), init: None }],
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 2, "tag + variable = 2 defs");
    }

    #[test]
    fn bare_struct_ref_no_def() {
        // `struct S;` (forward declaration, no field body) — no tag def created.
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "S");

        let ast = TranslationUnit {
            decls: vec![ExternalDecl::Decl(Decl {
                id: NodeId(0),
                span: DUMMY_SP,
                specs: {
                    let mut s = default_specs();
                    s.type_specs.push(TypeSpec::Record(RecordSpec {
                        id: NodeId(0),
                        kind: rcc_ast::RecordKind::Struct,
                        tag: Some(tag),
                        fields: None, // no definition
                        span: DUMMY_SP,
                    }));
                    s
                },
                inits: Vec::new(),
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 0, "bare struct ref should not create a def");
    }

    // ── Name resolution (task 06-02) tests ──────────────────────────

    #[test]
    fn scope_stack_push_pop_lookup() {
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let y = sym(&mut sess, "y");

        let mut scope = ScopeStack::new();
        scope.push_scope();
        scope.insert(x, Binding::Local(Local(0)));

        // Inner scope shadows x.
        scope.push_scope();
        scope.insert(x, Binding::Local(Local(1)));
        scope.insert(y, Binding::Local(Local(2)));

        // Lookup finds inner x.
        match scope.lookup(x) {
            Some(Binding::Local(l)) => assert_eq!(l, Local(1)),
            other => panic!("expected Local(1), got {other:?}"),
        }

        // Pop inner scope — outer x visible again.
        scope.pop_scope();
        match scope.lookup(x) {
            Some(Binding::Local(l)) => assert_eq!(l, Local(0)),
            other => panic!("expected Local(0), got {other:?}"),
        }

        // y is no longer visible.
        assert!(scope.lookup(y).is_none());
    }

    #[test]
    fn resolve_local_ref() {
        // A local variable in scope should resolve to LocalRef.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let resolver = Resolver::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        scope.insert(x, Binding::Local(Local(42)));

        let result = resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess);
        match result {
            Some(HirExprKind::LocalRef(l)) => assert_eq!(l, Local(42)),
            other => panic!("expected LocalRef(42), got {other:?}"),
        }
    }

    #[test]
    fn resolve_def_ref_file_scope() {
        // File-scope global should resolve to DefRef via the resolver.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let ast = TranslationUnit { decls: vec![make_global(x)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        let scope = ScopeStack::new(); // empty — no function body scope
        let result = resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess);
        match result {
            Some(HirExprKind::DefRef(id)) => assert_eq!(id, DefId(0)),
            other => panic!("expected DefRef(0), got {other:?}"),
        }
    }

    #[test]
    fn resolve_shadowing_local_over_global() {
        // `int x; void f() { int x = 0; x = 1; }` — both references
        // to `x` inside `f` resolve to the function-scope local.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        // Set up file-scope: a global `x`.
        let ast = TranslationUnit { decls: vec![make_global(x)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        // Set up function-scope: a local `x`.
        let mut scope = ScopeStack::new();
        scope.push_scope(); // function scope
        scope.insert(x, Binding::Local(Local(0)));

        // Both uses of `x` in the function should get LocalRef(0).
        let r1 = resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess);
        let r2 = resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess);
        match (r1, r2) {
            (Some(HirExprKind::LocalRef(l1)), Some(HirExprKind::LocalRef(l2))) => {
                assert_eq!(l1, Local(0));
                assert_eq!(l2, Local(0));
            }
            other => panic!("expected two LocalRef(0), got {other:?}"),
        }
    }

    #[test]
    fn resolve_inner_block_shadows_outer() {
        // void f() { int x = 1; { int x = 2; /* inner x */ } /* outer x */ }
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let resolver = Resolver::default();

        let mut scope = ScopeStack::new();
        scope.push_scope(); // outer block
        scope.insert(x, Binding::Local(Local(0)));

        scope.push_scope(); // inner block
        scope.insert(x, Binding::Local(Local(1)));

        // Inside inner block — resolves to Local(1).
        match resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess) {
            Some(HirExprKind::LocalRef(l)) => assert_eq!(l, Local(1)),
            other => panic!("expected LocalRef(1), got {other:?}"),
        }

        scope.pop_scope(); // exit inner block

        // Back in outer block — resolves to Local(0).
        match resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess) {
            Some(HirExprKind::LocalRef(l)) => assert_eq!(l, Local(0)),
            other => panic!("expected LocalRef(0), got {other:?}"),
        }
    }

    #[test]
    fn resolve_undeclared_emits_e0071() {
        // Using an undeclared identifier should emit E0071.
        let (mut sess, cap) = Session::for_test();
        let unknown = sym(&mut sess, "unknown_var");
        let resolver = Resolver::default();
        let scope = ScopeStack::new();

        let result = resolve_expr_ident(unknown, DUMMY_SP, &scope, &resolver, &mut sess);
        assert!(result.is_none(), "undeclared identifier should return None");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0071"));
        assert!(diags[0].message.contains("undeclared identifier"));
        assert!(diags[0].message.contains("unknown_var"));
    }

    #[test]
    fn resolve_undeclared_suggests_similar() {
        // E0071 should include a help suggestion for similarly-named symbols.
        let (mut sess, cap) = Session::for_test();
        let count = sym(&mut sess, "count");
        let conut = sym(&mut sess, "conut"); // typo

        let mut resolver = Resolver::default();
        // Register "count" in file scope.
        resolver.ordinary.insert(count, DefId(0));

        let scope = ScopeStack::new();
        let result = resolve_expr_ident(conut, DUMMY_SP, &scope, &resolver, &mut sess);
        assert!(result.is_none());

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0071"));
        assert!(!diags[0].help.is_empty(), "should have a help suggestion");
        assert!(diags[0].help[0].contains("count"), "help should suggest `count`");
    }

    #[test]
    fn resolve_undeclared_no_suggestion_for_distant_names() {
        // When all candidates are too far (edit distance > 3), no help is shown.
        let (mut sess, cap) = Session::for_test();
        let abcdefg = sym(&mut sess, "abcdefg");
        let xyz = sym(&mut sess, "xyz");

        let mut resolver = Resolver::default();
        resolver.ordinary.insert(xyz, DefId(0));

        let scope = ScopeStack::new();
        let result = resolve_expr_ident(abcdefg, DUMMY_SP, &scope, &resolver, &mut sess);
        assert!(result.is_none());

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].help.is_empty(), "no help for distant names");
    }

    #[test]
    fn resolve_scope_binding_def_in_local_scope() {
        // A DefId binding inserted into the scope stack should resolve as DefRef.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let resolver = Resolver::default();

        let mut scope = ScopeStack::new();
        scope.push_scope();
        scope.insert(x, Binding::Def(DefId(7)));

        match resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess) {
            Some(HirExprKind::DefRef(id)) => assert_eq!(id, DefId(7)),
            other => panic!("expected DefRef(7), got {other:?}"),
        }
    }

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("count", "conut"), 2);
    }

    // ── Tag namespace resolution (task 06-03) tests ──────────────────

    #[test]
    fn resolve_tag_forward_decl_creates_def() {
        // `struct A;` — forward declaration should create an incomplete def.
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "A");
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let result =
            resolve_tag(a, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(result.is_some());
        let id = result.unwrap();
        assert_eq!(crate_.defs.len(), 1);
        assert!(matches!(crate_.defs[id].kind, DefKind::Record { kind: RecordKind::Struct, .. }));
        assert!(resolver.tags.contains_key(&a));
    }

    #[test]
    fn resolve_tag_repeated_forward_returns_same_id() {
        // `struct A; struct A;` — second forward decl returns same DefId.
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "A");
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let id1 =
            resolve_tag(a, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
        let id2 =
            resolve_tag(a, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert_eq!(id1, id2);
        assert_eq!(crate_.defs.len(), 1, "should not create a second def");
    }

    #[test]
    fn resolve_tag_mutual_recursion() {
        // `struct A; struct B { A *a; };`
        // Forward-declare A, then define B which references A via resolve_tag.
        let (mut sess, _cap) = Session::for_test();
        let a_sym = sym(&mut sess, "A");
        let b_sym = sym(&mut sess, "B");

        // Step 1: assign_def_ids for `struct B { ... }` (defines B).
        let ast = TranslationUnit { decls: vec![make_struct(b_sym)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        // B should be in tags.
        assert!(resolver.tags.contains_key(&b_sym));

        // Step 2: resolve_tag for A (forward reference — not yet defined).
        let a_id = resolve_tag(
            a_sym,
            DUMMY_SP,
            TagKind::Struct,
            &mut crate_,
            &tcx,
            &mut resolver,
            &mut sess,
        );
        assert!(a_id.is_some(), "forward reference to A should create a def");

        // Step 3: resolve_tag for A again (should return same id).
        let a_id2 = resolve_tag(
            a_sym,
            DUMMY_SP,
            TagKind::Struct,
            &mut crate_,
            &tcx,
            &mut resolver,
            &mut sess,
        );
        assert_eq!(a_id, a_id2, "repeated resolution should return same DefId");

        // Both A and B should now be in tags.
        assert_eq!(resolver.tags.len(), 2);
    }

    #[test]
    fn resolve_tag_kind_mismatch_struct_then_union() {
        // `struct S { int x; }; union S;` → E0072
        let (mut sess, cap) = Session::for_test();
        let s = sym(&mut sess, "S");

        // Define `struct S`.
        let ast = TranslationUnit { decls: vec![make_struct(s)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        // Now try `union S` — should fail with E0072.
        let result =
            resolve_tag(s, DUMMY_SP, TagKind::Union, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(result.is_none(), "kind mismatch should return None");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0072"));
        assert!(diags[0].message.contains("union"));
        assert!(diags[0].message.contains("struct"));
    }

    #[test]
    fn resolve_tag_kind_mismatch_struct_then_enum() {
        // `struct S { int x; }; enum S;` → E0072
        let (mut sess, cap) = Session::for_test();
        let s = sym(&mut sess, "S");

        let ast = TranslationUnit { decls: vec![make_struct(s)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        let result =
            resolve_tag(s, DUMMY_SP, TagKind::Enum, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(result.is_none());

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0072"));
        assert!(diags[0].message.contains("enum"));
        assert!(diags[0].message.contains("struct"));
    }

    #[test]
    fn resolve_tag_enum_forward_and_match() {
        // `enum E;` then `enum E` again — should return same DefId.
        let (mut sess, _cap) = Session::for_test();
        let e = sym(&mut sess, "E");
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let id1 =
            resolve_tag(e, DUMMY_SP, TagKind::Enum, &mut crate_, &tcx, &mut resolver, &mut sess);
        let id2 =
            resolve_tag(e, DUMMY_SP, TagKind::Enum, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert_eq!(id1, id2);
        assert!(matches!(crate_.defs[id1.unwrap()].kind, DefKind::Enum { .. }));
    }

    #[test]
    fn resolve_tag_union_forward_then_struct_mismatch() {
        // `union U;` then `struct U;` → E0072
        let (mut sess, cap) = Session::for_test();
        let u = sym(&mut sess, "U");
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        // Forward-declare as union.
        let id =
            resolve_tag(u, DUMMY_SP, TagKind::Union, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(id.is_some());

        // Try to use as struct — mismatch.
        let result =
            resolve_tag(u, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(result.is_none());

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0072"));
    }

    #[test]
    fn resolve_tag_does_not_pollute_ordinary_namespace() {
        // Tag resolution should only touch resolver.tags, not resolver.ordinary.
        let (mut sess, _cap) = Session::for_test();
        let s = sym(&mut sess, "S");
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        resolve_tag(s, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
        assert!(resolver.ordinary.is_empty(), "tags should not appear in ordinary namespace");
        assert_eq!(resolver.tags.len(), 1);
    }

    #[test]
    fn resolve_suggests_local_scope_candidates() {
        // Help suggestions should also consider names from local scope frames.
        let (mut sess, cap) = Session::for_test();
        let value = sym(&mut sess, "value");
        let vlue = sym(&mut sess, "vlue"); // typo

        let resolver = Resolver::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        scope.insert(value, Binding::Local(Local(0)));

        let result = resolve_expr_ident(vlue, DUMMY_SP, &scope, &resolver, &mut sess);
        assert!(result.is_none());

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(!diags[0].help.is_empty(), "should suggest from local scope");
        assert!(diags[0].help[0].contains("value"));
    }

    // ── Label resolution (task 06-04) tests ─────────────────────────────

    /// Helper: wrap a statement kind into a `Stmt`.
    fn make_stmt(kind: StmtKind) -> Stmt {
        Stmt { id: NodeId(0), kind, span: DUMMY_SP }
    }

    /// Helper: make a goto statement.
    fn make_goto(name: Symbol) -> Stmt {
        make_stmt(StmtKind::Goto(name))
    }

    /// Helper: make a labeled statement (`name: body`).
    fn make_label(name: Symbol, body: Stmt) -> Stmt {
        make_stmt(StmtKind::Label { name, body: Box::new(body) })
    }

    /// Helper: make a null statement (`;`).
    fn make_null() -> Stmt {
        make_stmt(StmtKind::Null)
    }

    /// Helper: make a block from a list of statements.
    fn make_block(stmts: Vec<Stmt>) -> Block {
        Block {
            id: NodeId(0),
            items: stmts.into_iter().map(|s| BlockItem::Stmt(Box::new(s))).collect(),
            span: DUMMY_SP,
        }
    }

    #[test]
    fn label_forward_goto_resolves() {
        // void f(){ goto x; x:; }
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let body = make_block(vec![make_goto(x), make_label(x, make_null())]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "forward goto should resolve without errors: {diags:?}");
        assert!(resolver.labels.contains_key(&x));
    }

    #[test]
    fn label_backward_goto_resolves() {
        // void f(){ x:; goto x; }
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let body = make_block(vec![make_label(x, make_null()), make_goto(x)]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "backward goto should resolve without errors: {diags:?}");
    }

    #[test]
    fn label_undefined_emits_e0073() {
        // void f(){ goto missing; }
        let (mut sess, cap) = Session::for_test();
        let missing = sym(&mut sess, "missing");

        let body = make_block(vec![make_goto(missing)]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0073"));
        assert!(diags[0].message.contains("undeclared label"));
        assert!(diags[0].message.contains("missing"));
    }

    #[test]
    fn label_duplicate_emits_e0074() {
        // void f(){ a:; a:; }
        let (mut sess, cap) = Session::for_test();
        let a = sym(&mut sess, "a");

        let body = make_block(vec![make_label(a, make_null()), make_label(a, make_null())]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0074"));
        assert!(diags[0].message.contains("duplicate label"));
        assert!(diags[0].message.contains("a"));
    }

    #[test]
    fn label_two_distinct_labels_ok() {
        // void f(){ a: b: goto a; } — two distinct labels, one goto.
        let (mut sess, cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");

        let body = make_block(vec![make_label(a, make_null()), make_label(b, make_goto(a))]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "two distinct labels should be fine: {diags:?}");
        assert!(resolver.labels.contains_key(&a));
        assert!(resolver.labels.contains_key(&b));
    }

    #[test]
    fn label_cleared_per_function() {
        // Simulate two function bodies: labels from the first must not
        // leak into the second.
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        // First function: `x:;`
        let body1 = make_block(vec![make_label(x, make_null())]);
        let mut resolver = Resolver::default();
        resolve_labels(&body1, &mut resolver, &mut sess);
        assert!(resolver.labels.contains_key(&x));

        // Clear labels for second function (as the caller should do).
        resolver.labels.clear();

        // Second function: `goto x;` — x is NOT defined here.
        let body2 = make_block(vec![make_goto(x)]);
        resolve_labels(&body2, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0073"));
    }

    #[test]
    fn label_nested_in_compound_stmt() {
        // void f(){ { x:; } goto x; }
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let inner_block =
            make_stmt(StmtKind::Compound(make_block(vec![make_label(x, make_null())])));
        let body = make_block(vec![inner_block, make_goto(x)]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "label inside nested block should be visible: {diags:?}");
    }

    // ── Typedef expansion (task 06-05) tests ────────────────────────────

    /// Helper: manually register a typedef in the HirCrate + Resolver.
    ///
    /// Returns the `DefId` of the new typedef definition.
    fn register_typedef(
        name: Symbol,
        ty_id: rcc_hir::TyId,
        crate_: &mut HirCrate,
        resolver: &mut Resolver,
    ) -> DefId {
        let id = crate_.defs.push(Def {
            id: DefId(0),
            name,
            span: DUMMY_SP,
            kind: DefKind::Typedef(ty_id),
        });
        crate_.defs[id].id = id;
        resolver.ordinary.insert(name, id);
        id
    }

    #[test]
    fn typedef_simple_int() {
        // typedef int T; — looking up T should return tcx.int.
        let (mut sess, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let t = sym(&mut sess, "T");

        register_typedef(t, tcx.int, &mut crate_, &mut resolver);

        let mut expanding = rcc_data_structures::FxHashSet::default();
        let result =
            lower_typedef_name(t, DUMMY_SP, &mut expanding, &resolver, &crate_, &tcx, &mut sess);
        assert_eq!(result, tcx.int, "typedef T should resolve to tcx.int");
    }

    #[test]
    fn typedef_chain_resolves_to_original() {
        // typedef int T; typedef T U; — looking up U should return tcx.int.
        // After resolving T to tcx.int and storing that in T's Def,
        // when U is defined as `typedef T U`, we call lower_typedef_name
        // for T which returns tcx.int. Then U's Def stores tcx.int.
        // Finally, looking up U returns tcx.int.
        let (mut sess, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let t = sym(&mut sess, "T");
        let u = sym(&mut sess, "U");

        // T -> int
        register_typedef(t, tcx.int, &mut crate_, &mut resolver);
        // Resolve T to get its type.
        let mut expanding = rcc_data_structures::FxHashSet::default();
        let t_ty =
            lower_typedef_name(t, DUMMY_SP, &mut expanding, &resolver, &crate_, &tcx, &mut sess);
        assert_eq!(t_ty, tcx.int);

        // U -> T's resolved type (tcx.int)
        register_typedef(u, t_ty, &mut crate_, &mut resolver);
        let mut expanding2 = rcc_data_structures::FxHashSet::default();
        let u_ty =
            lower_typedef_name(u, DUMMY_SP, &mut expanding2, &resolver, &crate_, &tcx, &mut sess);
        assert_eq!(u_ty, tcx.int, "typedef chain T->U should resolve to tcx.int");
    }

    #[test]
    fn typedef_chain_three_deep() {
        // typedef int A; typedef A B; typedef B C;
        // C should resolve to tcx.int.
        let (mut sess, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let a = sym(&mut sess, "A");
        let b = sym(&mut sess, "B");
        let c = sym(&mut sess, "C");

        // Simulate sequential resolution:
        // A -> int
        register_typedef(a, tcx.int, &mut crate_, &mut resolver);
        let mut exp = rcc_data_structures::FxHashSet::default();
        let a_ty = lower_typedef_name(a, DUMMY_SP, &mut exp, &resolver, &crate_, &tcx, &mut sess);

        // B -> A's type (int)
        register_typedef(b, a_ty, &mut crate_, &mut resolver);
        let mut exp2 = rcc_data_structures::FxHashSet::default();
        let b_ty = lower_typedef_name(b, DUMMY_SP, &mut exp2, &resolver, &crate_, &tcx, &mut sess);

        // C -> B's type (int)
        register_typedef(c, b_ty, &mut crate_, &mut resolver);
        let mut exp3 = rcc_data_structures::FxHashSet::default();
        let c_ty = lower_typedef_name(c, DUMMY_SP, &mut exp3, &resolver, &crate_, &tcx, &mut sess);

        assert_eq!(c_ty, tcx.int, "three-deep typedef chain should resolve to tcx.int");
    }

    #[test]
    fn typedef_cycle_emits_e0075() {
        // Simulate a typedef cycle: T -> U -> T (both point to each other's
        // DefId via error placeholders that would cause re-expansion).
        //
        // We set up the `expanding` set to contain T's DefId before
        // trying to expand T, simulating the cycle detection.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let t = sym(&mut sess, "T");

        let t_id = register_typedef(t, tcx.error, &mut crate_, &mut resolver);

        // Simulate: we're already expanding T, and encounter T again.
        let mut expanding = rcc_data_structures::FxHashSet::default();
        expanding.insert(t_id);

        let result =
            lower_typedef_name(t, DUMMY_SP, &mut expanding, &resolver, &crate_, &tcx, &mut sess);
        assert_eq!(result, tcx.error, "cycle should return tcx.error");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0075"));
        assert!(diags[0].message.contains("typedef cycle"));
        assert!(diags[0].message.contains("T"));
    }

    #[test]
    fn typedef_undeclared_emits_e0071() {
        // Looking up a typedef name that doesn't exist should emit E0071.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let crate_ = HirCrate::default();
        let resolver = Resolver::default();
        let unknown = sym(&mut sess, "NoSuchType");

        let mut expanding = rcc_data_structures::FxHashSet::default();
        let result = lower_typedef_name(
            unknown,
            DUMMY_SP,
            &mut expanding,
            &resolver,
            &crate_,
            &tcx,
            &mut sess,
        );
        assert_eq!(result, tcx.error);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0071"));
        assert!(diags[0].message.contains("NoSuchType"));
    }

    #[test]
    fn typedef_not_a_typedef_emits_e0071() {
        // If the name resolves to a non-typedef (e.g. a global var),
        // emit E0071.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let g = sym(&mut sess, "g");

        // Register g as a global variable, not a typedef.
        let id = crate_.defs.push(Def {
            id: DefId(0),
            name: g,
            span: DUMMY_SP,
            kind: DefKind::Global { ty: tcx.int, linkage: Linkage::External },
        });
        crate_.defs[id].id = id;
        resolver.ordinary.insert(g, id);

        let mut expanding = rcc_data_structures::FxHashSet::default();
        let result =
            lower_typedef_name(g, DUMMY_SP, &mut expanding, &resolver, &crate_, &tcx, &mut sess);
        assert_eq!(result, tcx.error);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0071"));
        assert!(diags[0].message.contains("not a typedef"));
    }

    #[test]
    fn typedef_acceptance_chain() {
        // Acceptance criterion:
        // `typedef int T; typedef T U; U x;`
        // x's type is TyCtxt::int (interned singleton), not a new type.
        let (mut sess, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let t = sym(&mut sess, "T");
        let u = sym(&mut sess, "U");

        // Step 1: process `typedef int T;`
        // T's type is resolved to tcx.int (via DeclSpecs in a future task,
        // but here we simulate the result).
        register_typedef(t, tcx.int, &mut crate_, &mut resolver);

        // Step 2: process `typedef T U;`
        // U's DeclSpecs contains TypeSpec::TypedefName(T).
        // Call lower_typedef_name to get T's type.
        let mut exp = rcc_data_structures::FxHashSet::default();
        let t_resolved =
            lower_typedef_name(t, DUMMY_SP, &mut exp, &resolver, &crate_, &tcx, &mut sess);
        // Store T's resolved type as U's type.
        register_typedef(u, t_resolved, &mut crate_, &mut resolver);

        // Step 3: process `U x;`
        // x's DeclSpecs contains TypeSpec::TypedefName(U).
        let mut exp2 = rcc_data_structures::FxHashSet::default();
        let x_type =
            lower_typedef_name(u, DUMMY_SP, &mut exp2, &resolver, &crate_, &tcx, &mut sess);

        // Verify: x's type must be exactly tcx.int (same interned id).
        assert_eq!(x_type, tcx.int, "x's type must be the interned tcx.int singleton");
    }

    // ── Declarator → Ty (task 06-06) tests ─────────────────────────────

    use rcc_ast::{
        ArrayDeclarator, DerivedDeclarator, Expr, ExprKind, FunctionDeclarator, ParamDecl,
        TypeQuals,
    };
    use rcc_hir::ty::{Qual, Ty};

    /// Helper: make a declarator with a name and a derived chain.
    fn make_declarator(name: Symbol, derived: Vec<DerivedDeclarator>) -> Declarator {
        Declarator { name: Some((name, DUMMY_SP)), derived, span: DUMMY_SP }
    }

    /// Helper: make a pointer derived declarator with no qualifiers.
    fn ptr() -> DerivedDeclarator {
        DerivedDeclarator::Pointer(TypeQuals::default())
    }

    /// Helper: make a pointer derived declarator with const qualifier.
    fn const_ptr() -> DerivedDeclarator {
        DerivedDeclarator::Pointer(TypeQuals { const_: true, volatile: false, restrict: false })
    }

    /// Helper: make an array derived declarator with a constant size.
    fn array(size: u64, sess: &mut Session) -> DerivedDeclarator {
        let text = sym(sess, &size.to_string());
        DerivedDeclarator::Array(ArrayDeclarator {
            quals: TypeQuals::default(),
            has_static: false,
            star: false,
            size: Some(Expr { id: NodeId(0), kind: ExprKind::IntLit { text }, span: DUMMY_SP }),
        })
    }

    /// Helper: make an incomplete array derived declarator (no size).
    fn incomplete_array() -> DerivedDeclarator {
        DerivedDeclarator::Array(ArrayDeclarator {
            quals: TypeQuals::default(),
            has_static: false,
            star: false,
            size: None,
        })
    }

    /// Helper: make a function derived declarator with given param specs.
    fn func_decl(params: Vec<ParamDecl>, is_void: bool, variadic: bool) -> DerivedDeclarator {
        DerivedDeclarator::Function(FunctionDeclarator {
            params,
            is_void,
            variadic,
            kr_names: Vec::new(),
        })
    }

    /// Helper: make a function declarator `(void)`.
    fn func_void() -> DerivedDeclarator {
        func_decl(Vec::new(), true, false)
    }

    /// Helper: make a ParamDecl with a given type spec and no derived.
    fn param(type_specs: Vec<TypeSpec>) -> ParamDecl {
        ParamDecl {
            specs: DeclSpecs { type_specs, ..DeclSpecs::default() },
            declarator: Declarator { name: None, derived: Vec::new(), span: DUMMY_SP },
            span: DUMMY_SP,
        }
    }

    #[test]
    fn declarator_simple_int() {
        // `int x;` — base int, no derivations → int
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");
        let d = make_declarator(x, Vec::new());
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        assert_eq!(result, tcx.int);
    }

    #[test]
    fn declarator_pointer_to_int() {
        // `int *p;` → Ptr(int)
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let p = sym(&mut sess, "p");
        let d = make_declarator(p, vec![ptr()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_const_pointer_to_int() {
        // `int * const cp;` → Ptr(const int)
        // Wait, actually: `int * const cp` means cp is a const pointer
        // to int. The const qualifies the pointer, not the pointee.
        // In our representation: Ptr(Qual { ty: int, is_const: true })
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let cp = sym(&mut sess, "cp");
        let d = make_declarator(cp, vec![const_ptr()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected = tcx.intern(Ty::Ptr(Qual {
            ty: tcx.int,
            is_const: true,
            is_volatile: false,
            is_restrict: false,
        }));
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_array_of_int() {
        // `int arr[10];` → Array[10] of int
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let arr = sym(&mut sess, "arr");
        let d = make_declarator(arr, vec![array(10, &mut sess)]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(10), is_vla: false });
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_pointer_to_array() {
        // `int (*pa)[5];` → Ptr to Array[5] of int
        //
        // Reading from name: pa → * (pointer) → [5] (array) → int
        // Innermost = Pointer, Outermost = Array(5)
        // Stored outermost-to-innermost: [Array(5), Pointer]
        // Forward iteration: Array(5) on int → Array[5] of int,
        //   then Pointer → Ptr(Array[5] of int) ✓
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let pa = sym(&mut sess, "pa");
        let d = make_declarator(pa, vec![array(5, &mut sess), ptr()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        // First: Array[5] of int
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(5), is_vla: false });
        // Then: Ptr to Array[5] of int
        let expected = tcx.intern(Ty::Ptr(Qual::plain(arr_ty)));
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_acceptance_fp_array_of_ptr_to_func() {
        // Acceptance: `int (*fp[3])(int)` → Array[3] of Ptr to Func(int)->int
        //
        // C reading rule (right-left spiral from name):
        //   fp → [3] (array) → * (pointer) → (int) (function) → int
        //
        // Outermost-to-innermost stored: [Function([int]), Pointer, Array(3)]
        // Forward iteration:
        //   base=int → Func(int)->int → Ptr(Func) → Array[3](Ptr(Func))

        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let fp = sym(&mut sess, "fp");
        let d = make_declarator(
            fp,
            vec![
                func_decl(vec![param(vec![TypeSpec::Int])], false, false),
                ptr(),
                array(3, &mut sess),
            ],
        );
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);

        // Expected: Array[3] of Ptr to Func(int)->int
        let func_ty = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(func_ty)));
        let expected =
            tcx.intern(Ty::Array { elem: Qual::plain(ptr_ty), len: Some(3), is_vla: false });
        assert_eq!(
            result, expected,
            "int (*fp[3])(int) should be Array[3] of Ptr to Func(int)->int"
        );
    }

    #[test]
    fn declarator_void_object_error() {
        // `void x;` at file scope → error E0076
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");
        let d = make_declarator(x, Vec::new());
        let result = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);
        assert_eq!(result, tcx.error);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0076"));
        assert!(diags[0].message.contains("void"));
    }

    #[test]
    fn declarator_void_pointer_ok() {
        // `void *p;` → Ptr(void) — legal
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let p = sym(&mut sess, "p");
        let d = make_declarator(p, vec![ptr()]);
        let result = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_func_returning_array_error() {
        // `int f()[10]` → function returning array → error E0076
        // Derived chain outermost-to-innermost: [Array(10), Function]
        // Apply in order: base=int, Array(10) → Array[10] of int,
        //   then Function → func returning Array → ERROR
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let f = sym(&mut sess, "f");
        let d = make_declarator(f, vec![array(10, &mut sess), func_void()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        assert_eq!(result, tcx.error);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0076"));
        assert!(diags[0].message.contains("return array"));
    }

    #[test]
    fn declarator_func_returning_func_error() {
        // `int f()(int)` → function returning function → error E0076
        // Derived chain outermost-to-innermost: [Function([int]), Function(void)]
        // Apply in order: base=int, Function([int]) → Func(int)->int,
        //   then Function(void) → func returning func → ERROR
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let f = sym(&mut sess, "f");
        let d = make_declarator(
            f,
            vec![func_decl(vec![param(vec![TypeSpec::Int])], false, false), func_void()],
        );
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        assert_eq!(result, tcx.error);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0076"));
        assert!(diags[0].message.contains("return function"));
    }

    #[test]
    fn declarator_incomplete_array_file_scope_ok() {
        // `int arr[]` at file scope → incomplete type Array(None)
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let arr = sym(&mut sess, "arr");
        let d = make_declarator(arr, vec![incomplete_array()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: false });
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_incomplete_array_block_scope_error() {
        // `int arr[]` at function scope → error E0076
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let arr = sym(&mut sess, "arr");
        let d = make_declarator(arr, vec![incomplete_array()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::Block, &mut tcx, &mut sess);
        assert_eq!(result, tcx.error);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0076"));
    }

    #[test]
    fn declarator_function_returning_void() {
        // `void f(void)` → Func(void)->void — legal
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let f = sym(&mut sess, "f");
        let d = make_declarator(f, vec![func_void()]);
        let result = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected = tcx.intern(Ty::Func {
            ret: tcx.void,
            params: Vec::new(),
            variadic: false,
            proto: true,
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_pointer_to_pointer() {
        // `int **pp;` → Ptr(Ptr(int))
        // Derived outermost-to-innermost: [Pointer, Pointer]
        // Apply in order: base=int → Ptr(int) → Ptr(Ptr(int))
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let pp = sym(&mut sess, "pp");
        let d = make_declarator(pp, vec![ptr(), ptr()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let inner_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let expected = tcx.intern(Ty::Ptr(Qual::plain(inner_ptr)));
        assert_eq!(result, expected);
    }

    #[test]
    fn declarator_array_param_adjusted_to_pointer() {
        // In a function parameter, `int arr[]` is adjusted to `int *`.
        // Test via a function declarator: `void f(int arr[])`
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let f = sym(&mut sess, "f");

        // Build the parameter: `int arr[]`
        let param_decl = ParamDecl {
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            declarator: Declarator {
                name: Some((sym(&mut sess, "arr"), DUMMY_SP)),
                derived: vec![incomplete_array()],
                span: DUMMY_SP,
            },
            span: DUMMY_SP,
        };
        let d = make_declarator(f, vec![func_decl(vec![param_decl], false, false)]);
        let result = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);

        // The function type should have a pointer parameter (adjusted from array).
        let ptr_int = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let expected = tcx.intern(Ty::Func {
            ret: tcx.void,
            params: vec![ptr_int],
            variadic: false,
            proto: true,
        });
        assert_eq!(result, expected, "array parameter should be adjusted to pointer");
    }
}
