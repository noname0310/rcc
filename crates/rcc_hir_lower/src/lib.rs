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
    AlignSpec, AlignSpecKind, BlockItem, Declarator, DerivedDeclarator, EnumSpec, ExternalDecl,
    OffsetofDesignator, RecordSpec, StaticAssert, Stmt, StmtKind, StorageClass, TranslationUnit,
    TypeSpec,
};
use rcc_data_structures::FxHashMap;
use rcc_data_structures::FxHashSet;
use rcc_hir::ty::{IntRank, Qual, Ty};
use rcc_hir::OverflowOp;
use rcc_hir::{
    Body, CommonAttrs, Def, DefId, DefKind, Enumerator, Field, GenericAssociation, GlobalInit,
    GlobalInitDesignator, GlobalInitEntry, GlobalInitValue, HirCrate, HirExpr, HirExprId,
    HirExprKind, HirInlineAsm, HirInlineAsmOperand, HirInlineAsmQuals, HirStmt, HirStmtId,
    HirStmtKind, IntLiteralBase, IntLiteralSuffix, LayoutCx, Linkage, Local, LocalDecl,
    ObjectQuals, RecordKind, ScalarStorageOrder, SwitchCase, SymbolVisibility, TyCtxt, TyId,
    ValueCat,
};
use rcc_session::Session;
use rcc_span::{Span, Symbol};

/// Entry point: lower an AST into a fresh `HirCrate`.
///
pub fn lower(ast: &TranslationUnit, tcx: &mut TyCtxt, session: &mut Session) -> HirCrate {
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    assign_def_ids(ast, tcx, session, &mut crate_, &mut resolver);
    if session.opts.gnu_builtin_libcalls {
        install_gnu_builtin_libcalls(tcx, session, &mut crate_, &mut resolver);
    }
    finalize_file_scope_typedef_def_types(ast, tcx, session, &mut crate_, &mut resolver);
    finalize_file_scope_tag_definitions(ast, tcx, session, &mut crate_, &mut resolver);
    finalize_file_scope_def_types(ast, tcx, session, &mut crate_, &mut resolver);
    check_file_scope_static_asserts(ast, tcx, session, &mut crate_, &mut resolver);
    lower_function_bodies(ast, tcx, session, &mut crate_, &mut resolver);
    crate_
}

fn install_gnu_builtin_libcalls(
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    let void_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.void)));
    let const_void_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: tcx.void,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let const_char_ptr = tcx.intern(Ty::Ptr(Qual {
        ty: tcx.char_,
        is_const: true,
        is_volatile: false,
        is_restrict: false,
    }));
    let char_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
    let va_list_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.builtin_va_list)));

    let builtins = [
        ("abort", tcx.void, Vec::new(), false),
        ("exit", tcx.void, vec![tcx.int], false),
        ("printf", tcx.int, vec![const_char_ptr], true),
        ("fprintf", tcx.int, vec![void_ptr, const_char_ptr], true),
        ("sprintf", tcx.int, vec![char_ptr, const_char_ptr], true),
        ("snprintf", tcx.int, vec![char_ptr, tcx.ulong, const_char_ptr], true),
        ("vprintf", tcx.int, vec![const_char_ptr, va_list_ptr], false),
        ("vfprintf", tcx.int, vec![void_ptr, const_char_ptr, va_list_ptr], false),
        ("malloc", void_ptr, vec![tcx.ulong], false),
        ("alloca", void_ptr, vec![tcx.ulong], false),
        ("memcpy", void_ptr, vec![void_ptr, const_void_ptr, tcx.ulong], false),
        ("memset", void_ptr, vec![void_ptr, tcx.int, tcx.ulong], false),
        ("memcmp", tcx.int, vec![const_void_ptr, const_void_ptr, tcx.ulong], false),
        ("strcmp", tcx.int, vec![const_char_ptr, const_char_ptr], false),
        ("strcpy", char_ptr, vec![char_ptr, const_char_ptr], false),
        ("strncpy", char_ptr, vec![char_ptr, const_char_ptr, tcx.ulong], false),
        ("strchr", char_ptr, vec![const_char_ptr, tcx.int], false),
        ("strlen", tcx.ulong, vec![const_char_ptr], false),
        ("tmpnam", char_ptr, vec![char_ptr], false),
        ("__builtin_clz", tcx.int, vec![tcx.uint], false),
        ("__builtin_clzll", tcx.int, vec![tcx.ulong_long], false),
        ("__builtin_ctz", tcx.int, vec![tcx.uint], false),
        ("__builtin_ctzll", tcx.int, vec![tcx.ulong_long], false),
        ("__builtin_frame_address", void_ptr, vec![tcx.uint], false),
    ];

    for (name, ret, params, variadic) in builtins {
        let sym = session.interner.intern(name);
        if resolver.ordinary.contains_key(&sym) {
            continue;
        }
        let ty = tcx.intern(Ty::Func { ret, params, variadic, proto: true });
        let def = crate_.defs.push(Def {
            id: DefId(0),
            name: sym,
            span: rcc_span::DUMMY_SP,
            kind: DefKind::Function {
                ty,
                has_body: false,
                is_static: false,
                is_inline: false,
                is_extern_inline: false,
                no_instrument_function: false,
                variadic,
            },
        });
        crate_.defs[def].id = def;
        resolver.ordinary.insert(sym, def);
    }
}

fn lower_function_bodies(
    ast: &TranslationUnit,
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    for ext_decl in &ast.decls {
        let ExternalDecl::Function(func_def) = ext_decl else {
            continue;
        };
        let Some((name, _)) = func_def.declarator.name else {
            continue;
        };
        let Some(def_id) = resolver.ordinary.get(&name).copied() else {
            continue;
        };

        let mut fn_ty = lower_type_from_parts(
            &func_def.specs,
            &func_def.declarator,
            DeclScope::File,
            tcx,
            resolver,
            crate_,
            session,
        );
        fn_ty = lower_kr_function_type(func_def, fn_ty, tcx, resolver, crate_, session);
        let variadic = match tcx.get(fn_ty) {
            Ty::Func { variadic, .. } => *variadic,
            _ => false,
        };
        if let DefKind::Function { ty, variadic: def_variadic, .. } = &mut crate_.defs[def_id].kind
        {
            *ty = fn_ty;
            *def_variadic = variadic;
        }

        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        resolver.push_tag_scope();
        let param_bound_stmts = lower_function_params(
            &func_def.declarator,
            &func_def.kr_decls,
            &mut body,
            &mut scope,
            tcx,
            resolver,
            crate_,
            session,
        );

        resolver.current_function = Some(def_id);
        resolver.labels.clear();
        resolve_labels(&func_def.body, resolver, session);
        let root_stmt = Stmt {
            id: func_def.id,
            kind: StmtKind::Compound(func_def.body.clone()),
            span: func_def.body.span,
        };
        let root = lower_stmt(&root_stmt, &mut body, &mut scope, crate_, tcx, resolver, session);
        let root = prepend_function_entry_stmts(&mut body, root, param_bound_stmts);
        populate_switch_case_tables(&mut body, root, session);
        body.root = Some(root);
        resolver.pop_tag_scope();
        scope.pop_scope();
        resolver.labels.clear();
        resolver.current_function = None;

        crate_.bodies.insert(def_id, body);
    }
}

fn check_file_scope_static_asserts(
    ast: &TranslationUnit,
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    for ext_decl in &ast.decls {
        if let ExternalDecl::StaticAssert(assertion) = ext_decl {
            check_static_assert(assertion, DeclScope::File, None, tcx, resolver, crate_, session);
        }
    }
}

fn check_static_assert(
    assertion: &StaticAssert,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) {
    let Some(value) = eval_array_bound_as_i128(
        &assertion.expr,
        scope,
        typedef_scope,
        None,
        tcx,
        resolver,
        crate_,
        session,
    ) else {
        session
            .handler
            .struct_err(
                assertion.expr.span,
                "static assertion expression is not an integer constant expression",
            )
            .code(rcc_errors::codes::E0089)
            .emit();
        return;
    };

    if value == 0 {
        let message = String::from_utf8_lossy(&assertion.message.bytes);
        session
            .handler
            .struct_err(assertion.expr.span, format!("static assertion failed: {message}"))
            .code(rcc_errors::codes::E0089)
            .emit();
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_function_params(
    declarator: &Declarator,
    kr_decls: &[rcc_ast::Decl],
    body: &mut Body,
    scope: &mut ScopeStack,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Vec<HirStmtId> {
    // The body belongs to the function declarator closest to the
    // declared identifier. In `int (*f(int a))(int c)`, the outer
    // `(int c)` describes the returned function type; only `a` is a
    // body parameter.
    let Some(func_decl) = declarator.derived.iter().rev().find_map(|d| match d {
        DerivedDeclarator::Function(f) => Some(f),
        _ => None,
    }) else {
        return Vec::new();
    };

    if !func_decl.kr_names.is_empty() {
        return lower_kr_function_params(
            func_decl, kr_decls, body, scope, tcx, resolver, crate_, session,
        );
    }

    let mut entry_stmts = Vec::new();
    for param in &func_decl.params {
        let raw_ty = lower_type_from_parts(
            &param.specs,
            &param.declarator,
            DeclScope::Param,
            tcx,
            resolver,
            crate_,
            session,
        );
        let ty = adjust_param_type(raw_ty, tcx);
        lower_param_bound_side_effects(
            &param.declarator,
            body,
            scope,
            crate_,
            tcx,
            resolver,
            session,
            &mut entry_stmts,
        );
        let name = param.declarator.name.map(|(sym, _)| sym);
        let local = body.locals.push(LocalDecl {
            name,
            ty,
            quals: parameter_object_quals(&param.specs, &param.declarator),
            vla_len: None,
            is_param: true,
            span: param.span,
        });
        let attrs = lower_common_attrs(&param.specs, &param.declarator, session);
        merge_local_attrs(body, local, attrs);
        if let Some(sym) = name {
            scope.insert(sym, Binding::Local(local));
        }
    }
    entry_stmts
}

#[derive(Copy, Clone)]
struct KrParamInfo {
    ty: TyId,
    quals: ObjectQuals,
    span: Span,
}

#[allow(clippy::too_many_arguments)]
fn lower_kr_function_params(
    func_decl: &rcc_ast::FunctionDeclarator,
    kr_decls: &[rcc_ast::Decl],
    body: &mut Body,
    scope: &mut ScopeStack,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Vec<HirStmtId> {
    let declared = lower_kr_param_info_map(kr_decls, tcx, resolver, crate_, session);
    for (name, span) in &func_decl.kr_names {
        let info = declared.get(name).copied().unwrap_or(KrParamInfo {
            ty: tcx.int,
            quals: ObjectQuals::none(),
            span: *span,
        });
        let local = body.locals.push(LocalDecl {
            name: Some(*name),
            ty: info.ty,
            quals: info.quals,
            vla_len: None,
            is_param: true,
            span: info.span,
        });
        scope.insert(*name, Binding::Local(local));
    }
    Vec::new()
}

fn lower_kr_function_type(
    func_def: &rcc_ast::FunctionDef,
    fn_ty: TyId,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    let Some(func_decl) = func_def.declarator.derived.iter().rev().find_map(|d| match d {
        DerivedDeclarator::Function(f) => Some(f),
        _ => None,
    }) else {
        return fn_ty;
    };
    if func_decl.kr_names.is_empty() {
        return fn_ty;
    }

    let declared = lower_kr_param_info_map(&func_def.kr_decls, tcx, resolver, crate_, session);
    let params = func_decl
        .kr_names
        .iter()
        .map(|(name, _)| declared.get(name).map_or(tcx.int, |info| info.ty))
        .collect::<Vec<_>>();
    match tcx.get(fn_ty).clone() {
        Ty::Func { ret, variadic, .. } => {
            tcx.intern(Ty::Func { ret, params, variadic, proto: false })
        }
        _ => fn_ty,
    }
}

fn lower_kr_param_info_map(
    kr_decls: &[rcc_ast::Decl],
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> FxHashMap<Symbol, KrParamInfo> {
    let mut map = FxHashMap::default();
    for decl in kr_decls {
        for init_decl in &decl.inits {
            let Some((name, span)) = init_decl.declarator.name else {
                continue;
            };
            let raw_ty = lower_type_from_parts(
                &decl.specs,
                &init_decl.declarator,
                DeclScope::Param,
                tcx,
                resolver,
                crate_,
                session,
            );
            map.insert(
                name,
                KrParamInfo {
                    ty: adjust_param_type(raw_ty, tcx),
                    quals: declaration_object_quals(&decl.specs, &init_decl.declarator),
                    span,
                },
            );
        }
    }
    map
}

fn prepend_function_entry_stmts(
    body: &mut Body,
    root: HirStmtId,
    mut entry_stmts: Vec<HirStmtId>,
) -> HirStmtId {
    if entry_stmts.is_empty() {
        return root;
    }
    match &mut body.stmts[root].kind {
        HirStmtKind::Block(stmts) => {
            entry_stmts.extend(std::mem::take(stmts));
            *stmts = entry_stmts;
            root
        }
        _ => {
            entry_stmts.push(root);
            let block = body.stmts.push(HirStmt {
                id: HirStmtId(0),
                span: body.stmts[root].span,
                kind: HirStmtKind::Block(entry_stmts),
            });
            body.stmts[block].id = block;
            block
        }
    }
}

/// C99 adjusts array parameters to pointers, but a non-constant array bound in
/// a function definition still has entry-time runtime semantics. Lower those
/// bound expressions as ordinary expression statements before the user body.
#[allow(clippy::too_many_arguments)]
fn lower_param_bound_side_effects(
    declarator: &Declarator,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    for derived in &declarator.derived {
        let DerivedDeclarator::Array(arr) = derived else {
            continue;
        };
        let Some(size_expr) = &arr.size else {
            continue;
        };
        if eval_array_bound_as_u64(
            size_expr,
            DeclScope::Param,
            Some(scope),
            Some(body),
            tcx,
            resolver,
            crate_,
            session,
        )
        .is_some()
        {
            continue;
        }
        let expr = lower_expr(size_expr, body, scope, crate_, tcx, resolver, session);
        let stmt = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: size_expr.span,
            kind: HirStmtKind::Expr(expr),
        });
        body.stmts[stmt].id = stmt;
        out.push(stmt);
    }
}

/// Per-crate resolution tables built while lowering.
#[derive(Default, Debug)]
pub struct Resolver {
    /// Ordinary namespace: (name) -> `DefId`.
    pub ordinary: FxHashMap<Symbol, DefId>,
    /// File-scope tag namespace: `struct`/`union`/`enum` tags.
    pub tags: FxHashMap<Symbol, DefId>,
    tag_scopes: Vec<FxHashMap<Symbol, DefId>>,
    /// Labels are strictly per-function; populated then flushed per body.
    pub labels: FxHashMap<Symbol, rcc_hir::HirStmtId>,
    /// Function currently being lowered, if inside a function body.
    pub current_function: Option<DefId>,
    /// Interned string literals (content symbol -> generated `Global`
    /// `DefId`). Used by [`lower_expr`] so that repeated identical
    /// string literals reuse the same internally-linked global.
    pub strings: FxHashMap<Symbol, DefId>,
}

impl Resolver {
    fn push_tag_scope(&mut self) {
        self.tag_scopes.push(FxHashMap::default());
    }

    fn pop_tag_scope(&mut self) {
        self.tag_scopes.pop();
    }

    fn lookup_tag(&self, tag: Symbol) -> Option<DefId> {
        for scope in self.tag_scopes.iter().rev() {
            if let Some(&def) = scope.get(&tag) {
                return Some(def);
            }
        }
        self.tags.get(&tag).copied()
    }

    fn lookup_current_tag(&self, tag: Symbol) -> Option<DefId> {
        if let Some(scope) = self.tag_scopes.last() {
            scope.get(&tag).copied()
        } else {
            self.tags.get(&tag).copied()
        }
    }

    fn insert_tag(&mut self, tag: Symbol, def: DefId) {
        if let Some(scope) = self.tag_scopes.last_mut() {
            scope.insert(tag, def);
        } else {
            self.tags.insert(tag, def);
        }
    }
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
#[derive(Clone, Debug)]
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

    /// Lookup a name only in the innermost active scope frame.
    pub fn lookup_current(&self, name: Symbol) -> Option<Binding> {
        self.frames.last().and_then(|frame| frame.get(&name).copied())
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

fn lower_implicit_function_callee(
    ident: Symbol,
    span: Span,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<HirExprKind> {
    if !session.opts.gnu_implicit_function_declaration {
        return None;
    }
    if scope.lookup(ident).is_some() || resolver.ordinary.contains_key(&ident) {
        return None;
    }

    let ident_str = session.interner.get(ident).to_owned();
    session
        .handler
        .struct_warn(
            span,
            format!(
                "implicit declaration of function `{ident_str}` [-Wimplicit-function-declaration]"
            ),
        )
        .code(rcc_errors::codes::W0029)
        .note("synthesizing a prototype-less `extern int` declaration for GNU/C89 compatibility")
        .emit();

    let ty =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: false });
    let def = crate_.defs.push(Def {
        id: DefId(0),
        name: ident,
        span,
        kind: DefKind::Function {
            ty,
            has_body: false,
            is_static: false,
            is_inline: false,
            is_extern_inline: false,
            no_instrument_function: false,
            variadic: false,
        },
    });
    crate_.defs[def].id = def;
    resolver.ordinary.insert(ident, def);

    Some(HirExprKind::DefRef(def))
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
/// Looks up `tag` in the nearest visible tag scope. If found, checks
/// that the stored definition has the same `TagKind` as
/// `expected_kind`. On mismatch, emits `E0072` and returns `None`.
///
/// If the tag is not yet visible (forward declaration), creates a new
/// incomplete `Def` in the current tag scope, registers it, and
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
    if let Some(existing_id) = resolver.lookup_tag(tag) {
        return validate_tag_kind(tag, tag_span, existing_id, expected_kind, crate_, session);
    }
    Some(create_incomplete_tag(tag, tag_span, expected_kind, crate_, tcx, resolver))
}

fn resolve_tag_definition(
    tag: Symbol,
    tag_span: Span,
    expected_kind: TagKind,
    crate_: &mut HirCrate,
    tcx: &TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<DefId> {
    if let Some(existing_id) = resolver.lookup_current_tag(tag) {
        return validate_tag_kind(tag, tag_span, existing_id, expected_kind, crate_, session);
    }
    Some(create_incomplete_tag(tag, tag_span, expected_kind, crate_, tcx, resolver))
}

fn validate_tag_kind(
    tag: Symbol,
    tag_span: Span,
    existing_id: DefId,
    expected_kind: TagKind,
    crate_: &HirCrate,
    session: &mut Session,
) -> Option<DefId> {
    let def = &crate_.defs[existing_id];
    let actual_kind = match &def.kind {
        DefKind::Record { kind, .. } => TagKind::from(*kind),
        DefKind::Enum { .. } => TagKind::Enum,
        _ => {
            // Should not happen — tag scopes only hold Record/Enum.
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
}

fn create_incomplete_tag(
    tag: Symbol,
    tag_span: Span,
    expected_kind: TagKind,
    crate_: &mut HirCrate,
    tcx: &TyCtxt,
    resolver: &mut Resolver,
) -> DefId {
    let kind = match expected_kind {
        TagKind::Struct => DefKind::Record {
            kind: RecordKind::Struct,
            packed: false,
            ms_bitfields: false,
            align_override: None,
            scalar_storage_order: None,
            layout: None,
            fields: Vec::new(),
        },
        TagKind::Union => DefKind::Record {
            kind: RecordKind::Union,
            packed: false,
            ms_bitfields: false,
            align_override: None,
            scalar_storage_order: None,
            layout: None,
            fields: Vec::new(),
        },
        TagKind::Enum => DefKind::Enum { repr: tcx.int, variants: Vec::new() },
    };
    let id = crate_.defs.push(Def { id: DefId(0), name: tag, span: tag_span, kind });
    crate_.defs[id].id = id;
    resolver.insert_tag(tag, id);
    id
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
        walk_block_item_labels(item, resolver, session, LabelPass::Collect);
    }
    // Pass 2: check all gotos.
    for item in &body.items {
        walk_block_item_labels(item, resolver, session, LabelPass::Check);
    }
}

#[derive(Copy, Clone, Debug)]
enum LabelPass {
    Collect,
    Check,
}

fn walk_block_item_labels(
    item: &BlockItem,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    match item {
        BlockItem::Stmt(stmt) => walk_stmt_labels(stmt, resolver, session, pass),
        BlockItem::Decl(decl) => walk_decl_labels(decl, resolver, session, pass),
        BlockItem::StaticAssert(assertion) => {
            walk_expr_labels(&assertion.expr, resolver, session, pass);
        }
    }
}

fn walk_decl_labels(
    decl: &rcc_ast::Decl,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    walk_decl_specs_labels(&decl.specs, resolver, session, pass);
    for init in &decl.inits {
        walk_declarator_labels(&init.declarator, resolver, session, pass);
        if let Some(init) = &init.init {
            walk_initializer_labels(init, resolver, session, pass);
        }
    }
}

fn walk_decl_specs_labels(
    specs: &rcc_ast::DeclSpecs,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    for spec in &specs.type_specs {
        match spec {
            TypeSpec::Record(record) => {
                if let Some(fields) = &record.fields {
                    for field in fields {
                        walk_decl_specs_labels(&field.specs, resolver, session, pass);
                        for declarator in &field.declarators {
                            if let Some(decl) = &declarator.declarator {
                                walk_declarator_labels(decl, resolver, session, pass);
                            }
                            if let Some(width) = &declarator.bit_width {
                                walk_expr_labels(width, resolver, session, pass);
                            }
                        }
                    }
                }
                for assertion in &record.static_asserts {
                    walk_expr_labels(&assertion.expr, resolver, session, pass);
                }
            }
            TypeSpec::Enum(en) => {
                if let Some(enumerators) = &en.enumerators {
                    for enumerator in enumerators {
                        if let Some(value) = &enumerator.value {
                            walk_expr_labels(value, resolver, session, pass);
                        }
                    }
                }
            }
            TypeSpec::TypeofExpr(expr) => walk_expr_labels(expr, resolver, session, pass),
            TypeSpec::TypeofType(ty) | TypeSpec::Atomic(ty) => {
                walk_type_name_labels(ty, resolver, session, pass);
            }
            _ => {}
        }
    }
}

fn walk_declarator_labels(
    declarator: &Declarator,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    for derived in &declarator.derived {
        match derived {
            DerivedDeclarator::Array(array) => {
                if let Some(size) = &array.size {
                    walk_expr_labels(size, resolver, session, pass);
                }
            }
            DerivedDeclarator::Pointer(_) | DerivedDeclarator::Function(_) => {}
        }
    }
}

fn walk_initializer_labels(
    init: &rcc_ast::Initializer,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    match init {
        rcc_ast::Initializer::Expr(expr) => walk_expr_labels(expr, resolver, session, pass),
        rcc_ast::Initializer::List(items) => {
            for (designators, nested) in items {
                for designator in designators {
                    match designator {
                        rcc_ast::Designator::Index(expr) => {
                            walk_expr_labels(expr, resolver, session, pass);
                        }
                        rcc_ast::Designator::Range { lo, hi } => {
                            walk_expr_labels(lo, resolver, session, pass);
                            walk_expr_labels(hi, resolver, session, pass);
                        }
                        rcc_ast::Designator::Field(_) => {}
                    }
                }
                walk_initializer_labels(nested, resolver, session, pass);
            }
        }
    }
}

fn walk_stmt_labels(stmt: &Stmt, resolver: &mut Resolver, session: &mut Session, pass: LabelPass) {
    match &stmt.kind {
        StmtKind::Label { name, body } => {
            if matches!(pass, LabelPass::Collect) {
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
            }
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::Compound(block) => {
            for item in &block.items {
                walk_block_item_labels(item, resolver, session, pass);
            }
        }
        StmtKind::If { cond, then_branch, else_branch } => {
            walk_expr_labels(cond, resolver, session, pass);
            walk_stmt_labels(then_branch, resolver, session, pass);
            if let Some(else_) = else_branch {
                walk_stmt_labels(else_, resolver, session, pass);
            }
        }
        StmtKind::While { cond, body } => {
            walk_expr_labels(cond, resolver, session, pass);
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::DoWhile { body, cond } => {
            walk_stmt_labels(body, resolver, session, pass);
            walk_expr_labels(cond, resolver, session, pass);
        }
        StmtKind::For { init, cond, step, body } => {
            if let Some(init) = init {
                walk_block_item_labels(init, resolver, session, pass);
            }
            if let Some(cond) = cond {
                walk_expr_labels(cond, resolver, session, pass);
            }
            if let Some(step) = step {
                walk_expr_labels(step, resolver, session, pass);
            }
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::Switch { cond, body } => {
            walk_expr_labels(cond, resolver, session, pass);
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::Case { value, range_end, body } => {
            walk_expr_labels(value, resolver, session, pass);
            if let Some(end) = range_end {
                walk_expr_labels(end, resolver, session, pass);
            }
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::Default { body } => {
            walk_stmt_labels(body, resolver, session, pass);
        }
        StmtKind::Attributed { stmt, .. } => {
            walk_stmt_labels(stmt, resolver, session, pass);
        }
        StmtKind::Expr(expr) => {
            if let Some(expr) = expr {
                walk_expr_labels(expr, resolver, session, pass);
            }
        }
        StmtKind::InlineAsm(asm) => {
            for operand in asm.outputs.iter().chain(asm.inputs.iter()) {
                walk_expr_labels(&operand.expr, resolver, session, pass);
            }
        }
        StmtKind::Return(expr) => {
            if let Some(expr) = expr {
                walk_expr_labels(expr, resolver, session, pass);
            }
        }
        StmtKind::Goto(name) => {
            if matches!(pass, LabelPass::Check) && !resolver.labels.contains_key(name) {
                let name_str = session.interner.get(*name);
                session
                    .handler
                    .struct_err(stmt.span, format!("use of undeclared label `{name_str}`"))
                    .code(rcc_errors::codes::E0073)
                    .emit();
            }
        }
        StmtKind::GotoComputed(expr) => {
            walk_expr_labels(expr, resolver, session, pass);
        }
        StmtKind::Break | StmtKind::Continue | StmtKind::Null => {}
    }
}

fn walk_type_name_labels(
    ty: &rcc_ast::TypeName,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    walk_decl_specs_labels(&ty.specs, resolver, session, pass);
    walk_declarator_labels(&ty.declarator, resolver, session, pass);
}

fn walk_expr_labels(
    expr: &rcc_ast::Expr,
    resolver: &mut Resolver,
    session: &mut Session,
    pass: LabelPass,
) {
    match &expr.kind {
        rcc_ast::ExprKind::Binary { lhs, rhs, .. }
        | rcc_ast::ExprKind::Assign { lhs, rhs, .. }
        | rcc_ast::ExprKind::Comma { lhs, rhs } => {
            walk_expr_labels(lhs, resolver, session, pass);
            walk_expr_labels(rhs, resolver, session, pass);
        }
        rcc_ast::ExprKind::Unary { operand, .. }
        | rcc_ast::ExprKind::SizeofExpr(operand)
        | rcc_ast::ExprKind::AlignofExpr(operand)
        | rcc_ast::ExprKind::Paren(operand) => {
            walk_expr_labels(operand, resolver, session, pass);
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            walk_expr_labels(cond, resolver, session, pass);
            walk_expr_labels(then_expr, resolver, session, pass);
            walk_expr_labels(else_expr, resolver, session, pass);
        }
        rcc_ast::ExprKind::GenericSelection { control, associations } => {
            walk_expr_labels(control, resolver, session, pass);
            for assoc in associations {
                if let Some(ty) = &assoc.ty {
                    walk_type_name_labels(ty, resolver, session, pass);
                }
                walk_expr_labels(&assoc.expr, resolver, session, pass);
            }
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            walk_expr_labels(cond, resolver, session, pass);
            walk_expr_labels(else_expr, resolver, session, pass);
        }
        rcc_ast::ExprKind::LabelAddr(name) => {
            if matches!(pass, LabelPass::Check) && !resolver.labels.contains_key(name) {
                let name_str = session.interner.get(*name);
                session
                    .handler
                    .struct_err(expr.span, format!("use of undeclared label `{name_str}`"))
                    .code(rcc_errors::codes::E0073)
                    .emit();
            }
        }
        rcc_ast::ExprKind::Call { callee, args } => {
            walk_expr_labels(callee, resolver, session, pass);
            for arg in args {
                walk_expr_labels(arg, resolver, session, pass);
            }
        }
        rcc_ast::ExprKind::BuiltinOffsetof { ty, designators } => {
            walk_type_name_labels(ty, resolver, session, pass);
            for designator in designators {
                if let rcc_ast::OffsetofDesignator::Index(idx) = designator {
                    walk_expr_labels(idx, resolver, session, pass);
                }
            }
        }
        rcc_ast::ExprKind::BuiltinTypesCompatible { lhs, rhs } => {
            walk_type_name_labels(lhs, resolver, session, pass);
            walk_type_name_labels(rhs, resolver, session, pass);
        }
        rcc_ast::ExprKind::BuiltinVaArg { ap, ty } => {
            walk_expr_labels(ap, resolver, session, pass);
            walk_type_name_labels(ty, resolver, session, pass);
        }
        rcc_ast::ExprKind::StmtExpr(block) => {
            for item in &block.items {
                walk_block_item_labels(item, resolver, session, pass);
            }
        }
        rcc_ast::ExprKind::Member { base, .. }
        | rcc_ast::ExprKind::Arrow { base, .. }
        | rcc_ast::ExprKind::Index { base, index: _ } => {
            walk_expr_labels(base, resolver, session, pass);
            if let rcc_ast::ExprKind::Index { index, .. } = &expr.kind {
                walk_expr_labels(index, resolver, session, pass);
            }
        }
        rcc_ast::ExprKind::Cast { ty, expr } => {
            walk_type_name_labels(ty, resolver, session, pass);
            walk_expr_labels(expr, resolver, session, pass);
        }
        rcc_ast::ExprKind::SizeofType(ty) | rcc_ast::ExprKind::AlignofType(ty) => {
            walk_type_name_labels(ty, resolver, session, pass);
        }
        rcc_ast::ExprKind::CompoundLiteral { ty, init } => {
            walk_type_name_labels(ty, resolver, session, pass);
            walk_initializer_labels(init, resolver, session, pass);
        }
        rcc_ast::ExprKind::Ident(_)
        | rcc_ast::ExprKind::IntLit(_)
        | rcc_ast::ExprKind::FloatLit(_)
        | rcc_ast::ExprKind::CharLit(_)
        | rcc_ast::ExprKind::StringLit(_) => {}
    }
}

/// Lower a single AST statement into the body, returning its `HirStmtId`.
///
/// This is the statement half of AST→HIR lowering (task 06-09). Every
/// [`StmtKind`] variant produces a matching [`HirStmtKind`] entry in
/// `body.stmts`. Local variables encountered along the way are pushed
/// into `body.locals` and registered as [`Binding::Local`] in `scope`
/// so subsequent expression references resolve correctly. Scope
/// management:
///
/// - A `Compound` block pushes fresh ordinary and tag scope frames on
///   entry and pops them on exit.
/// - A `For` statement pushes fresh ordinary and tag scope frames around the init /
///   condition / step / body because C99 §6.8.5p5 gives the init
///   declaration the same scope as the loop body.
///
/// Expression lowering is delegated to [`lower_expr`] (task 06-10),
/// which covers every AST expression variant and handles string-
/// literal interning into `resolver.strings`.
#[allow(clippy::too_many_arguments)]
pub fn lower_stmt(
    stmt: &Stmt,
    body: &mut Body,
    scope: &mut ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> HirStmtId {
    let kind = match &stmt.kind {
        StmtKind::Null => HirStmtKind::Null,
        StmtKind::Expr(None) => HirStmtKind::Null,
        StmtKind::Expr(Some(e)) => {
            let id = lower_expr(e, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Expr(id)
        }
        StmtKind::InlineAsm(asm) => {
            lower_inline_asm_stmt(asm, body, scope, crate_, tcx, resolver, session)
        }
        StmtKind::Compound(block) => {
            let ids = lower_block_items(&block.items, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Block(ids)
        }
        StmtKind::If { cond, then_branch, else_branch } => {
            let cond_id = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            let then_id = lower_stmt(then_branch, body, scope, crate_, tcx, resolver, session);
            let else_id = else_branch
                .as_deref()
                .map(|e| lower_stmt(e, body, scope, crate_, tcx, resolver, session));
            HirStmtKind::If { cond: cond_id, then_branch: then_id, else_branch: else_id }
        }
        StmtKind::While { cond, body: body_stmt } => {
            let cond_id = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::While { cond: cond_id, body: body_id }
        }
        StmtKind::DoWhile { body: body_stmt, cond } => {
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            let cond_id = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::DoWhile { body: body_id, cond: cond_id }
        }
        StmtKind::For { init, cond, step, body: body_stmt } => {
            // C99 §6.8.5p5: the for-init declaration has the same scope
            // as the loop body. Push fresh ordinary and tag frames so
            // declared identifiers are visible to cond / step / body
            // but not outside.
            scope.push_scope();
            resolver.push_tag_scope();
            let init_id = init.as_deref().and_then(|item| match item {
                BlockItem::Stmt(s) => {
                    Some(lower_stmt(s, body, scope, crate_, tcx, resolver, session))
                }
                BlockItem::Decl(d) => {
                    lower_for_init_decl(d, stmt.span, body, scope, crate_, tcx, resolver, session)
                }
                BlockItem::StaticAssert(assertion) => {
                    check_static_assert(
                        assertion,
                        DeclScope::Block,
                        Some(&*scope),
                        tcx,
                        resolver,
                        crate_,
                        session,
                    );
                    None
                }
            });
            let cond_id =
                cond.as_ref().map(|e| lower_expr(e, body, scope, crate_, tcx, resolver, session));
            let step_id =
                step.as_ref().map(|e| lower_expr(e, body, scope, crate_, tcx, resolver, session));
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            resolver.pop_tag_scope();
            scope.pop_scope();
            HirStmtKind::For { init: init_id, cond: cond_id, step: step_id, body: body_id }
        }
        StmtKind::Switch { cond, body: body_stmt } => {
            let cond_id = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            // Case/default collection into `cases` runs as a separate
            // pass in task 06-10 / typeck; keep it empty here so the
            // statement still has the canonical `Switch` shape.
            HirStmtKind::Switch { cond: cond_id, body: body_id, cases: Vec::new() }
        }
        StmtKind::Case { value, range_end, body: body_stmt } => {
            // Try to fold the case value at lowering time using the
            // existing integer-constant evaluator. Non-foldable
            // expressions fall through as `None`; the switch-collection
            // pass (task 06-10 / typeck) can emit its own diagnostic.
            let folded = eval_enum_value_as_i128(value, resolver, crate_);
            let folded_end =
                range_end.as_ref().map(|end| eval_enum_value_as_i128(end, resolver, crate_));
            let folded_end = folded_end.flatten();
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Case { value: folded, range_end: folded_end, body: body_id }
        }
        StmtKind::Default { body: body_stmt } => {
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Default { body: body_id }
        }
        StmtKind::Attributed { stmt, .. } => {
            return lower_stmt(stmt, body, scope, crate_, tcx, resolver, session);
        }
        StmtKind::Label { name, body: body_stmt } => {
            let body_id = lower_stmt(body_stmt, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Label { name: *name, body: body_id }
        }
        StmtKind::Goto(name) => HirStmtKind::Goto(*name),
        StmtKind::GotoComputed(expr) => {
            let id = lower_expr(expr, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::GotoComputed(id)
        }
        StmtKind::Break => HirStmtKind::Break,
        StmtKind::Continue => HirStmtKind::Continue,
        StmtKind::Return(None) => HirStmtKind::Return(None),
        StmtKind::Return(Some(e)) => {
            let id = lower_expr(e, body, scope, crate_, tcx, resolver, session);
            HirStmtKind::Return(Some(id))
        }
    };

    let stmt_id = body.stmts.push(HirStmt { id: HirStmtId(0), span: stmt.span, kind });
    body.stmts[stmt_id].id = stmt_id;
    stmt_id
}

#[allow(clippy::too_many_arguments)]
fn lower_inline_asm_stmt(
    asm: &rcc_ast::InlineAsm,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> HirStmtKind {
    validate_inline_asm(asm, session);

    let template = inline_asm_string_text(&asm.template, asm.span, session, "inline asm template")
        .unwrap_or_default()
        .to_owned();
    let outputs = asm
        .outputs
        .iter()
        .map(|operand| {
            let expr = lower_expr(&operand.expr, body, scope, crate_, tcx, resolver, session);
            HirInlineAsmOperand {
                name: operand.name.map(|(name, _)| name),
                constraint: asm_constraint_text(&operand.constraint).unwrap_or_default().to_owned(),
                expr,
            }
        })
        .collect();
    let inputs = asm
        .inputs
        .iter()
        .map(|operand| {
            let expr = lower_expr(&operand.expr, body, scope, crate_, tcx, resolver, session);
            HirInlineAsmOperand {
                name: operand.name.map(|(name, _)| name),
                constraint: asm_constraint_text(&operand.constraint).unwrap_or_default().to_owned(),
                expr,
            }
        })
        .collect();
    let clobbers = asm
        .clobbers
        .iter()
        .filter_map(|clobber| {
            inline_asm_string_text(clobber, asm.span, session, "inline asm clobber")
                .map(str::to_owned)
        })
        .collect();

    HirStmtKind::InlineAsm(HirInlineAsm {
        quals: HirInlineAsmQuals { volatile: asm.quals.volatile, inline: asm.quals.inline },
        template,
        outputs,
        inputs,
        clobbers,
    })
}

fn validate_inline_asm(asm: &rcc_ast::InlineAsm, session: &mut Session) {
    if asm.quals.goto {
        session
            .handler
            .struct_err(asm.span, "GNU `asm goto` is parsed but not supported yet")
            .code(rcc_errors::codes::E0032)
            .help("remove `goto` or wait for the LLVM inline-asm codegen task")
            .emit();
    }

    validate_inline_asm_operand_names(asm, session);

    for (idx, output) in asm.outputs.iter().enumerate() {
        validate_inline_asm_constraint(
            &output.constraint,
            output.span,
            InlineAsmOperandRole::Output { index: idx },
            asm.outputs.len(),
            session,
        );
    }
    for input in &asm.inputs {
        validate_inline_asm_constraint(
            &input.constraint,
            input.span,
            InlineAsmOperandRole::Input,
            asm.outputs.len(),
            session,
        );
    }
    for clobber in &asm.clobbers {
        validate_inline_asm_clobber(clobber, asm.span, session);
    }
}

fn validate_inline_asm_operand_names(asm: &rcc_ast::InlineAsm, session: &mut Session) {
    let mut seen = FxHashMap::default();
    for operand in asm.outputs.iter().chain(&asm.inputs) {
        let Some((name, span)) = operand.name else {
            continue;
        };
        if let Some(previous) = seen.insert(name, span) {
            let name = session.interner.get(name);
            session
                .handler
                .struct_err(span, format!("duplicate inline asm operand name `{name}`"))
                .code(rcc_errors::codes::E0032)
                .label(previous, "first operand with this name")
                .emit();
        }
    }
}

#[derive(Copy, Clone)]
enum InlineAsmOperandRole {
    Output { index: usize },
    Input,
}

fn validate_inline_asm_constraint(
    lit: &rcc_ast::StringLiteral,
    span: Span,
    role: InlineAsmOperandRole,
    output_count: usize,
    session: &mut Session,
) {
    let Some(text) = inline_asm_string_text(lit, span, session, "inline asm constraint") else {
        return;
    };
    if text.is_empty() {
        emit_inline_asm_semantic_error(span, "inline asm constraint cannot be empty", session);
        return;
    }

    for alternative in text.split(',') {
        if alternative.is_empty() {
            emit_inline_asm_semantic_error(
                span,
                "inline asm constraint contains an empty alternative",
                session,
            );
            continue;
        }
        validate_inline_asm_constraint_alternative(alternative, span, role, output_count, session);
    }
}

fn validate_inline_asm_constraint_alternative(
    alternative: &str,
    span: Span,
    role: InlineAsmOperandRole,
    output_count: usize,
    session: &mut Session,
) {
    let (marker, mut rest) = match role {
        InlineAsmOperandRole::Output { .. } => match alternative.as_bytes().first().copied() {
            Some(b'=') | Some(b'+') => (alternative.as_bytes()[0] as char, &alternative[1..]),
            _ => {
                emit_inline_asm_semantic_error(
                    span,
                    "output inline asm constraint must start with `=` or `+`",
                    session,
                );
                return;
            }
        },
        InlineAsmOperandRole::Input => {
            if matches!(alternative.as_bytes().first(), Some(b'=') | Some(b'+')) {
                emit_inline_asm_semantic_error(
                    span,
                    "input inline asm constraint cannot start with `=` or `+`",
                    session,
                );
                return;
            }
            ('\0', alternative)
        }
    };

    while matches!(rest.as_bytes().first(), Some(b'&' | b'%' | b'?' | b'!' | b'*')) {
        rest = &rest[1..];
    }

    if rest.is_empty() {
        emit_inline_asm_semantic_error(
            span,
            "inline asm constraint is missing a register, memory, immediate, or matching operand",
            session,
        );
        return;
    }

    if rest.chars().all(|ch| ch.is_ascii_digit()) {
        validate_inline_asm_matching_constraint(rest, span, role, output_count, session);
        return;
    }

    let Some(first) = rest.chars().next() else {
        return;
    };
    if inline_asm_constraint_class(first).is_some() {
        return;
    }

    let kind = match role {
        InlineAsmOperandRole::Output { .. } => "output",
        InlineAsmOperandRole::Input => "input",
    };
    let hint = if marker == '+' {
        "supported read/write output constraints include `+r` and `+m`"
    } else {
        "supported constraints include generic `r`, `m`, `g`, `i`, `n`, `X`, matching digits, and common x86-64 `a/b/c/d/S/D` registers"
    };
    session
        .handler
        .struct_err(span, format!("unsupported {kind} inline asm constraint `{alternative}`"))
        .code(rcc_errors::codes::E0032)
        .help(hint)
        .emit();
}

fn validate_inline_asm_matching_constraint(
    digits: &str,
    span: Span,
    role: InlineAsmOperandRole,
    output_count: usize,
    session: &mut Session,
) {
    let Ok(index) = digits.parse::<usize>() else {
        emit_inline_asm_semantic_error(
            span,
            "inline asm matching constraint index is too large",
            session,
        );
        return;
    };
    match role {
        InlineAsmOperandRole::Output { index: output_index } => {
            emit_inline_asm_semantic_error(
                span,
                &format!("output operand {output_index} cannot use matching constraint `{digits}`"),
                session,
            );
        }
        InlineAsmOperandRole::Input if index >= output_count => {
            emit_inline_asm_semantic_error(
                span,
                &format!("matching constraint `{digits}` does not name an output operand"),
                session,
            );
        }
        InlineAsmOperandRole::Input => {}
    }
}

#[derive(Copy, Clone)]
enum InlineAsmConstraintClass {
    Register,
    Memory,
    Immediate,
    Generic,
}

fn inline_asm_constraint_class(ch: char) -> Option<InlineAsmConstraintClass> {
    match ch {
        'r' | 'a' | 'b' | 'c' | 'd' | 'S' | 'D' => Some(InlineAsmConstraintClass::Register),
        'm' => Some(InlineAsmConstraintClass::Memory),
        'i' | 'n' => Some(InlineAsmConstraintClass::Immediate),
        'g' | 'X' => Some(InlineAsmConstraintClass::Generic),
        _ => None,
    }
}

fn validate_inline_asm_clobber(lit: &rcc_ast::StringLiteral, span: Span, session: &mut Session) {
    let Some(text) = inline_asm_string_text(lit, span, session, "inline asm clobber") else {
        return;
    };
    if text.is_empty() {
        emit_inline_asm_semantic_error(span, "inline asm clobber cannot be empty", session);
    }
}

fn inline_asm_string_text<'a>(
    lit: &'a rcc_ast::StringLiteral,
    span: Span,
    session: &mut Session,
    what: &str,
) -> Option<&'a str> {
    if lit.bytes.contains(&0) {
        emit_inline_asm_semantic_error(span, &format!("{what} cannot contain NUL bytes"), session);
        return None;
    }
    match std::str::from_utf8(&lit.bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            emit_inline_asm_semantic_error(span, &format!("{what} must be UTF-8 text"), session);
            None
        }
    }
}

fn emit_inline_asm_semantic_error(span: Span, message: &str, session: &mut Session) {
    session.handler.struct_err(span, message).code(rcc_errors::codes::E0032).emit();
}

fn asm_constraint_text(lit: &rcc_ast::StringLiteral) -> Option<&str> {
    std::str::from_utf8(&lit.bytes).ok()
}

fn populate_switch_case_tables(body: &mut Body, root: HirStmtId, session: &mut Session) {
    populate_switch_case_tables_in_stmt(body, root, 0, session);
}

fn populate_switch_case_tables_in_stmt(
    body: &mut Body,
    stmt_id: HirStmtId,
    switch_depth: usize,
    session: &mut Session,
) {
    let kind = body.stmts[stmt_id].kind.clone();
    match kind {
        HirStmtKind::Block(stmts) => {
            for stmt in stmts {
                populate_switch_case_tables_in_stmt(body, stmt, switch_depth, session);
            }
        }
        HirStmtKind::If { cond, then_branch, else_branch } => {
            populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            populate_switch_case_tables_in_stmt(body, then_branch, switch_depth, session);
            if let Some(else_branch) = else_branch {
                populate_switch_case_tables_in_stmt(body, else_branch, switch_depth, session);
            }
        }
        HirStmtKind::While { cond, body: inner } | HirStmtKind::DoWhile { body: inner, cond } => {
            populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            populate_switch_case_tables_in_stmt(body, inner, switch_depth, session);
        }
        HirStmtKind::For { init, cond, step, body: inner } => {
            if let Some(init) = init {
                populate_switch_case_tables_in_stmt(body, init, switch_depth, session);
            }
            if let Some(cond) = cond {
                populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            }
            if let Some(step) = step {
                populate_switch_case_tables_in_expr(body, step, switch_depth, session);
            }
            populate_switch_case_tables_in_stmt(body, inner, switch_depth, session);
        }
        HirStmtKind::Switch { cond, body: switch_body, .. } => {
            populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            let mut state = SwitchCaseCollection::default();
            collect_cases_for_switch(body, switch_body, session, &mut state);
            body.stmts[stmt_id].kind =
                HirStmtKind::Switch { cond, body: switch_body, cases: state.cases };
            populate_switch_case_tables_in_stmt(body, switch_body, switch_depth + 1, session);
        }
        HirStmtKind::Case { body: inner, .. } => {
            if switch_depth == 0 {
                emit_invalid_switch_label(
                    body.stmts[stmt_id].span,
                    "case label outside switch",
                    session,
                );
            }
            populate_switch_case_tables_in_stmt(body, inner, switch_depth, session);
        }
        HirStmtKind::Default { body: inner } => {
            if switch_depth == 0 {
                emit_invalid_switch_label(
                    body.stmts[stmt_id].span,
                    "default label outside switch",
                    session,
                );
            }
            populate_switch_case_tables_in_stmt(body, inner, switch_depth, session);
        }
        HirStmtKind::Label { body: inner, .. } => {
            populate_switch_case_tables_in_stmt(body, inner, switch_depth, session);
        }
        HirStmtKind::Expr(expr) => {
            populate_switch_case_tables_in_expr(body, expr, switch_depth, session);
        }
        HirStmtKind::InitAssign { lhs, rhs } => {
            populate_switch_case_tables_in_expr(body, lhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, rhs, switch_depth, session);
        }
        HirStmtKind::InlineAsm(asm) => {
            for operand in asm.outputs.iter().chain(&asm.inputs) {
                populate_switch_case_tables_in_expr(body, operand.expr, switch_depth, session);
            }
        }
        HirStmtKind::GotoComputed(expr) => {
            populate_switch_case_tables_in_expr(body, expr, switch_depth, session);
        }
        HirStmtKind::Return(Some(expr)) => {
            populate_switch_case_tables_in_expr(body, expr, switch_depth, session);
        }
        HirStmtKind::LocalDecl { init: Some(expr), .. } => {
            populate_switch_case_tables_in_expr(body, expr, switch_depth, session);
        }
        HirStmtKind::Goto(_)
        | HirStmtKind::Break
        | HirStmtKind::Continue
        | HirStmtKind::Return(None)
        | HirStmtKind::LocalDecl { init: None, .. }
        | HirStmtKind::Null => {}
    }
}

fn populate_switch_case_tables_in_expr(
    body: &mut Body,
    expr_id: HirExprId,
    switch_depth: usize,
    session: &mut Session,
) {
    let kind = body.exprs[expr_id].kind.clone();
    match kind {
        HirExprKind::Binary { lhs, rhs, .. } | HirExprKind::Comma { lhs, rhs } => {
            populate_switch_case_tables_in_expr(body, lhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, rhs, switch_depth, session);
        }
        HirExprKind::Unary { operand, .. }
        | HirExprKind::UnresolvedField { base: operand, .. }
        | HirExprKind::Field { base: operand, .. }
        | HirExprKind::Convert { operand, .. }
        | HirExprKind::Cast { operand, .. }
        | HirExprKind::SizeofExpr(operand)
        | HirExprKind::AlignofExpr(operand)
        | HirExprKind::AddressOf(operand)
        | HirExprKind::Deref(operand)
        | HirExprKind::BuiltinVaEnd { ap: operand } => {
            populate_switch_case_tables_in_expr(body, operand, switch_depth, session);
        }
        HirExprKind::Index { base, index } => {
            populate_switch_case_tables_in_expr(body, base, switch_depth, session);
            populate_switch_case_tables_in_expr(body, index, switch_depth, session);
        }
        HirExprKind::Call { callee, args } => {
            populate_switch_case_tables_in_expr(body, callee, switch_depth, session);
            for arg in args {
                populate_switch_case_tables_in_expr(body, arg, switch_depth, session);
            }
        }
        HirExprKind::StmtExpr { stmts, result } => {
            for stmt in stmts {
                populate_switch_case_tables_in_stmt(body, stmt, switch_depth, session);
            }
            if let Some(result) = result {
                populate_switch_case_tables_in_expr(body, result, switch_depth, session);
            }
        }
        HirExprKind::CompoundLiteral { init_stmts, .. } => {
            for stmt in init_stmts {
                populate_switch_case_tables_in_stmt(body, stmt, switch_depth, session);
            }
        }
        HirExprKind::VectorInit { lanes, .. } => {
            for lane in lanes {
                populate_switch_case_tables_in_expr(body, lane, switch_depth, session);
            }
        }
        HirExprKind::Cond { cond, then_expr, else_expr } => {
            populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            populate_switch_case_tables_in_expr(body, then_expr, switch_depth, session);
            populate_switch_case_tables_in_expr(body, else_expr, switch_depth, session);
        }
        HirExprKind::GenericSelection { selected: Some(selected), .. } => {
            populate_switch_case_tables_in_expr(body, selected, switch_depth, session);
        }
        HirExprKind::GenericSelection { selected: None, .. } => {}
        HirExprKind::OmittedCond { cond, else_expr } => {
            populate_switch_case_tables_in_expr(body, cond, switch_depth, session);
            populate_switch_case_tables_in_expr(body, else_expr, switch_depth, session);
        }
        HirExprKind::Assign { lhs, rhs } => {
            populate_switch_case_tables_in_expr(body, lhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, rhs, switch_depth, session);
        }
        HirExprKind::BuiltinVaArg { ap, .. } => {
            populate_switch_case_tables_in_expr(body, ap, switch_depth, session);
        }
        HirExprKind::BuiltinVaStart { ap, last_param }
        | HirExprKind::BuiltinVaCopy { dst: ap, src: last_param } => {
            populate_switch_case_tables_in_expr(body, ap, switch_depth, session);
            populate_switch_case_tables_in_expr(body, last_param, switch_depth, session);
        }
        HirExprKind::BuiltinExpect { value, expected } => {
            populate_switch_case_tables_in_expr(body, value, switch_depth, session);
            populate_switch_case_tables_in_expr(body, expected, switch_depth, session);
        }
        HirExprKind::BuiltinUnreachable => {}
        HirExprKind::BuiltinConstantP { expr } => {
            populate_switch_case_tables_in_expr(body, expr, switch_depth, session);
        }
        HirExprKind::BuiltinBswap { value, .. } => {
            populate_switch_case_tables_in_expr(body, value, switch_depth, session);
        }
        HirExprKind::BuiltinComplex { real, imag } => {
            populate_switch_case_tables_in_expr(body, real, switch_depth, session);
            populate_switch_case_tables_in_expr(body, imag, switch_depth, session);
        }
        HirExprKind::BuiltinTgmath { args, .. } => {
            for arg in args {
                populate_switch_case_tables_in_expr(body, arg, switch_depth, session);
            }
        }
        HirExprKind::BuiltinOverflow { lhs, rhs, dst, .. } => {
            populate_switch_case_tables_in_expr(body, lhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, rhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, dst, switch_depth, session);
        }
        HirExprKind::BuiltinOverflowP { lhs, rhs, probe, .. } => {
            populate_switch_case_tables_in_expr(body, lhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, rhs, switch_depth, session);
            populate_switch_case_tables_in_expr(body, probe, switch_depth, session);
        }
        HirExprKind::IntLiteral { .. }
        | HirExprKind::IntConst(_)
        | HirExprKind::FloatConst(_)
        | HirExprKind::StringRef(_)
        | HirExprKind::LocalRef(_)
        | HirExprKind::DefRef(_)
        | HirExprKind::BuiltinVaArea
        | HirExprKind::SizeofType(_)
        | HirExprKind::AlignofType(_)
        | HirExprKind::LabelAddr(_) => {}
    }
}

#[derive(Default)]
struct SwitchCaseCollection {
    cases: Vec<SwitchCase>,
    seen_values: FxHashSet<i128>,
    seen_default: bool,
}

fn collect_cases_for_switch(
    body: &Body,
    stmt_id: HirStmtId,
    session: &mut Session,
    state: &mut SwitchCaseCollection,
) {
    match body.stmts[stmt_id].kind.clone() {
        HirStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_cases_for_switch(body, stmt, session, state);
            }
        }
        HirStmtKind::If { then_branch, else_branch, .. } => {
            collect_cases_for_switch(body, then_branch, session, state);
            if let Some(else_branch) = else_branch {
                collect_cases_for_switch(body, else_branch, session, state);
            }
        }
        HirStmtKind::While { body: inner, .. }
        | HirStmtKind::DoWhile { body: inner, .. }
        | HirStmtKind::Label { body: inner, .. } => {
            collect_cases_for_switch(body, inner, session, state);
        }
        HirStmtKind::For { init, body: inner, .. } => {
            if let Some(init) = init {
                collect_cases_for_switch(body, init, session, state);
            }
            collect_cases_for_switch(body, inner, session, state);
        }
        HirStmtKind::Switch { .. } => {
            // A nested switch owns its own case/default labels. Do not
            // leak them into the enclosing switch table.
        }
        HirStmtKind::Case { value, range_end, body: inner } => {
            let Some(v) = value else {
                emit_invalid_switch_label(
                    body.stmts[stmt_id].span,
                    "case label is not an integer constant expression",
                    session,
                );
                collect_cases_for_switch(body, inner, session, state);
                return;
            };
            if let Some(end) = range_end {
                if end < v {
                    emit_invalid_switch_label(
                        body.stmts[stmt_id].span,
                        "case range lower bound is greater than upper bound",
                        session,
                    );
                } else if end - v > 4096 {
                    emit_invalid_switch_label(
                        body.stmts[stmt_id].span,
                        "case range is too large to expand",
                        session,
                    );
                } else {
                    for value in v..=end {
                        push_switch_case_value(body, stmt_id, value, session, state);
                    }
                }
            } else {
                push_switch_case_value(body, stmt_id, v, session, state);
            }
            collect_cases_for_switch(body, inner, session, state);
        }
        HirStmtKind::Default { body: inner } => {
            if state.seen_default {
                emit_invalid_switch_label(
                    body.stmts[stmt_id].span,
                    "duplicate default label in switch",
                    session,
                );
            }
            state.seen_default = true;
            state.cases.push(SwitchCase { value: None, target: stmt_id });
            collect_cases_for_switch(body, inner, session, state);
        }
        HirStmtKind::Expr(_)
        | HirStmtKind::InitAssign { .. }
        | HirStmtKind::InlineAsm(_)
        | HirStmtKind::Goto(_)
        | HirStmtKind::GotoComputed(_)
        | HirStmtKind::Break
        | HirStmtKind::Continue
        | HirStmtKind::Return(_)
        | HirStmtKind::LocalDecl { .. }
        | HirStmtKind::Null => {}
    }
}

fn push_switch_case_value(
    body: &Body,
    stmt_id: HirStmtId,
    value: i128,
    session: &mut Session,
    state: &mut SwitchCaseCollection,
) {
    if !state.seen_values.insert(value) {
        emit_invalid_switch_label(
            body.stmts[stmt_id].span,
            "duplicate case value in switch",
            session,
        );
    }
    state.cases.push(SwitchCase { value: Some(value), target: stmt_id });
}

fn emit_invalid_switch_label(span: Span, message: &str, session: &mut Session) {
    session.handler.struct_err(span, message.to_string()).code(rcc_errors::codes::E0086).emit();
}

/// Lower a list of [`BlockItem`]s (from a compound statement or a
/// function body) into a flat `Vec<HirStmtId>`, pushing a new scope
/// frame on entry and popping it on exit.
///
/// Declarations (non-typedef) are materialised as [`LocalDecl`] entries
/// in `body.locals`, registered in `scope` as [`Binding::Local`], and
/// recorded as [`HirStmtKind::LocalDecl`] statements in the returned
/// list so later CFG construction sees the initialisation point.
/// Typedef-storage declarations are materialised as scoped
/// [`DefKind::Typedef`] entries and contribute no runtime statements.
#[allow(clippy::too_many_arguments)]
fn lower_block_items(
    items: &[BlockItem],
    body: &mut Body,
    scope: &mut ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Vec<HirStmtId> {
    scope.push_scope();
    resolver.push_tag_scope();
    let mut out: Vec<HirStmtId> = Vec::with_capacity(items.len());
    for item in items {
        match item {
            BlockItem::Stmt(s) => {
                let id = lower_stmt(s, body, scope, crate_, tcx, resolver, session);
                out.push(id);
            }
            BlockItem::Decl(decl) => {
                lower_block_decl(decl, body, scope, crate_, tcx, resolver, session, &mut out);
            }
            BlockItem::StaticAssert(assertion) => {
                check_static_assert(
                    assertion,
                    DeclScope::Block,
                    Some(&*scope),
                    tcx,
                    resolver,
                    crate_,
                    session,
                );
            }
        }
    }
    resolver.pop_tag_scope();
    scope.pop_scope();
    out
}

/// Lower a GNU statement-expression block.
///
/// The final expression statement, when present, is *not* emitted as a
/// `HirStmtKind::Expr`; it becomes the `result` expression instead. That is
/// required for CFG lowering to execute side effects exactly once while
/// keeping block-scoped locals live until after the result value is read.
#[allow(clippy::too_many_arguments)]
fn lower_stmt_expr_block(
    block: &rcc_ast::Block,
    body: &mut Body,
    outer_scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> (Vec<HirStmtId>, Option<HirExprId>) {
    let mut scope = outer_scope.clone();
    scope.push_scope();
    resolver.push_tag_scope();

    let mut out = Vec::with_capacity(block.items.len());
    let mut result = None;
    let last_idx = block.items.len().checked_sub(1);

    for (idx, item) in block.items.iter().enumerate() {
        match item {
            BlockItem::Stmt(stmt)
                if Some(idx) == last_idx && matches!(&stmt.kind, StmtKind::Expr(Some(_))) =>
            {
                let StmtKind::Expr(Some(expr)) = &stmt.kind else {
                    unreachable!("guarded by matches! above");
                };
                result = Some(lower_expr(expr, body, &scope, crate_, tcx, resolver, session));
            }
            BlockItem::Stmt(stmt) => {
                let id = lower_stmt(stmt, body, &mut scope, crate_, tcx, resolver, session);
                out.push(id);
            }
            BlockItem::Decl(decl) => {
                lower_block_decl(decl, body, &mut scope, crate_, tcx, resolver, session, &mut out);
            }
            BlockItem::StaticAssert(assertion) => {
                check_static_assert(
                    assertion,
                    DeclScope::Block,
                    Some(&scope),
                    tcx,
                    resolver,
                    crate_,
                    session,
                );
            }
        }
    }

    resolver.pop_tag_scope();
    (out, result)
}

/// Lower a block-scope declaration, pushing one [`HirStmtKind::LocalDecl`]
/// per init-declarator into `out`.
///
/// Typedef-storage declarations still affect name lookup: the typedef
/// name is added to the current scope frame as a placeholder binding so
/// subsequent declarators in the same scope can resolve through it.
/// Because typedefs carry no runtime semantics, they do not contribute
/// a statement to `out`.
#[allow(clippy::too_many_arguments)]
fn lower_block_decl(
    decl: &rcc_ast::Decl,
    body: &mut Body,
    scope: &mut ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    let is_typedef = decl.specs.storage == Some(StorageClass::Typedef);
    if decl.specs.thread_local
        && !matches!(decl.specs.storage, Some(StorageClass::Static | StorageClass::Extern))
    {
        session
            .handler
            .struct_err(
                decl.span,
                "`_Thread_local` block-scope declarations must also use `static` or `extern`",
            )
            .code(rcc_errors::codes::E0060)
            .emit();
    }

    if decl.inits.is_empty() {
        materialize_tag_definitions_in_specs(&decl.specs, tcx, resolver, crate_, session);
        return;
    }

    for init_decl in &decl.inits {
        let Some((name, _name_span)) = init_decl.declarator.name else {
            continue;
        };

        if is_typedef {
            let duplicate = scope.lookup_current(name).is_some();
            if duplicate {
                emit_duplicate_ordinary(name, decl.span, session);
            }
            let ty = lower_type_from_parts_in_scope(
                &decl.specs,
                &init_decl.declarator,
                DeclScope::Block,
                Some(scope),
                Some(body),
                tcx,
                resolver,
                crate_,
                session,
            );
            let id = crate_.defs.push(Def {
                id: DefId(0),
                name,
                span: decl.span,
                kind: DefKind::Typedef(ty),
            });
            crate_.defs[id].id = id;
            if !duplicate {
                scope.insert(name, Binding::Def(id));
            }
            continue;
        }

        let mut ty = lower_type_from_parts_in_scope(
            &decl.specs,
            &init_decl.declarator,
            DeclScope::Block,
            Some(scope),
            Some(body),
            tcx,
            resolver,
            crate_,
            session,
        );
        if matches!(tcx.get(ty), Ty::Func { .. }) {
            let def_id = lower_block_scope_function_decl(
                name,
                decl.span,
                ty,
                &decl.specs,
                &init_decl.declarator,
                scope,
                crate_,
                resolver,
                tcx,
                session,
            );
            if let Some(def_id) = def_id {
                scope.insert(name, Binding::Def(def_id));
            }
            continue;
        }
        if decl.specs.storage == Some(StorageClass::Extern) {
            let def_id = lower_block_scope_extern_object_decl(
                name,
                decl.span,
                ty,
                &decl.specs,
                &init_decl.declarator,
                scope,
                crate_,
                resolver,
                tcx,
                session,
            );
            scope.insert(name, Binding::Def(def_id));
            continue;
        }
        if let Some(init) = &init_decl.init {
            ty = complete_initializer_type(ty, init, tcx, crate_);
        }
        if is_incomplete_array_ty(ty, tcx) {
            emit_incomplete_array_at_block_scope(init_decl.declarator.span, session);
            ty = tcx.error;
        }
        if decl.specs.storage == Some(StorageClass::Static) {
            let duplicate = scope.lookup_current(name).is_some();
            if duplicate {
                emit_duplicate_ordinary(name, decl.span, session);
            }
            let def_id = lower_block_scope_static_object_decl(
                name,
                decl.span,
                ty,
                &decl.specs,
                init_decl,
                scope,
                crate_,
                tcx,
                resolver,
                session,
            );
            if !duplicate {
                scope.insert(name, Binding::Def(def_id));
            }
            continue;
        }
        let vla_len = lower_top_level_vla_len(
            &init_decl.declarator,
            ty,
            body,
            scope,
            crate_,
            tcx,
            resolver,
            session,
        );

        let duplicate = scope.lookup_current(name).is_some();
        if duplicate {
            emit_duplicate_ordinary(name, decl.span, session);
        }

        // C99 §6.2.1p7: the identifier's scope starts just after its
        // declarator. VLA bounds above are part of the declarator and use
        // the old scope; initializers below use this new binding.
        let local = body.locals.push(LocalDecl {
            name: Some(name),
            ty,
            quals: declaration_object_quals(&decl.specs, &init_decl.declarator),
            vla_len,
            is_param: false,
            span: decl.span,
        });
        let attrs = lower_common_attrs_with_align(
            &decl.specs,
            &init_decl.declarator,
            DeclScope::Block,
            Some(scope),
            tcx,
            resolver,
            crate_,
            session,
        );
        merge_local_attrs(body, local, attrs);
        if !duplicate {
            scope.insert(name, Binding::Local(local));
        }

        // Scalar (single-expression) initialiser: keep the value inline
        // on the LocalDecl so simple `int x = 5;` declarations still
        // produce exactly one statement. Aggregate (brace-enclosed)
        // initialisers are handled by `lower_initializer` below, which
        // flattens them into a sequence of initializer stores per
        // C99 §6.7.8.
        let (init_expr, list_init) = match &init_decl.init {
            Some(init @ rcc_ast::Initializer::Expr(e))
                if is_string_array_initializer(ty, e, tcx) =>
            {
                (None, Some(init))
            }
            Some(rcc_ast::Initializer::Expr(e)) => {
                (Some(lower_expr(e, body, scope, crate_, tcx, resolver, session)), None)
            }
            Some(init @ rcc_ast::Initializer::List(_)) => (None, Some(init)),
            None => (None, None),
        };

        let stmt_id = body.stmts.push(HirStmt {
            id: HirStmtId(0),
            span: decl.span,
            kind: HirStmtKind::LocalDecl { local, init: init_expr },
        });
        body.stmts[stmt_id].id = stmt_id;
        out.push(stmt_id);

        // For brace-enclosed initialisers, flatten the list into a
        // sequence of initializer store statements appended to the current
        // block. The target lvalue is a fresh `LocalRef` expression so
        // every assignment has its own HIR node.
        if let Some(init) = list_init {
            let target = push_local_ref(local, ty, decl.span, body);
            lower_initializer(
                target, ty, init, decl.span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_block_scope_extern_object_decl(
    name: Symbol,
    span: Span,
    ty: TyId,
    specs: &rcc_ast::DeclSpecs,
    declarator: &Declarator,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> DefId {
    let attrs = lower_common_attrs_with_align(
        specs,
        declarator,
        DeclScope::Block,
        Some(scope),
        tcx,
        resolver,
        crate_,
        session,
    );
    if let Some(def_id) = resolver
        .ordinary
        .get(&name)
        .copied()
        .filter(|id| matches!(crate_.defs[*id].kind, DefKind::Global { .. }))
    {
        merge_def_attrs(crate_, def_id, attrs);
        return def_id;
    }

    let def_id = crate_.defs.push(Def {
        id: DefId(0),
        name,
        span,
        kind: DefKind::Global {
            ty,
            quals: declaration_object_quals(specs, declarator),
            thread_local: specs.thread_local,
            linkage: Linkage::External,
            init: None,
        },
    });
    crate_.defs[def_id].id = def_id;
    merge_def_attrs(crate_, def_id, attrs);
    def_id
}

#[allow(clippy::too_many_arguments)]
fn lower_block_scope_static_object_decl(
    name: Symbol,
    span: Span,
    ty: TyId,
    specs: &rcc_ast::DeclSpecs,
    init_decl: &rcc_ast::InitDeclarator,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> DefId {
    let unique_name = block_static_symbol(name, crate_, session);
    let global_init = if let Some(init) = &init_decl.init {
        let mut init_body = Body::default();
        let global_init = lower_global_initializer(
            ty,
            init,
            init_decl.declarator.span,
            &mut init_body,
            scope,
            crate_,
            tcx,
            resolver,
            session,
        );
        Some((global_init, init_body))
    } else {
        Some((GlobalInit { ty, entries: Vec::new() }, Body::default()))
    };

    let def_id = crate_.defs.push(Def {
        id: DefId(0),
        name: unique_name,
        span,
        kind: DefKind::Global {
            ty,
            quals: declaration_object_quals(specs, &init_decl.declarator),
            thread_local: specs.thread_local,
            linkage: Linkage::Internal,
            init: global_init.as_ref().map(|(init, _)| init.clone()),
        },
    });
    crate_.defs[def_id].id = def_id;
    let attrs = lower_common_attrs_with_align(
        specs,
        &init_decl.declarator,
        DeclScope::Block,
        Some(scope),
        tcx,
        resolver,
        crate_,
        session,
    );
    merge_def_attrs(crate_, def_id, attrs);
    if let Some((_init, init_body)) = global_init {
        if !init_body.exprs.is_empty() {
            crate_.global_init_bodies.insert(def_id, init_body);
        }
    }
    def_id
}

fn block_static_symbol(name: Symbol, crate_: &HirCrate, session: &mut Session) -> Symbol {
    let original = session.interner.get(name).to_owned();
    let ordinal = crate_.defs.len();
    session.interner.intern(&format!("__rcc_block_static_{original}_{ordinal}"))
}

#[allow(clippy::too_many_arguments)]
fn lower_block_scope_function_decl(
    name: Symbol,
    span: Span,
    ty: TyId,
    specs: &rcc_ast::DeclSpecs,
    declarator: &rcc_ast::Declarator,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
    tcx: &TyCtxt,
    session: &mut Session,
) -> Option<DefId> {
    match scope.lookup_current(name) {
        Some(Binding::Def(def)) if matches!(crate_.defs[def].kind, DefKind::Function { .. }) => {
            return Some(def);
        }
        Some(_) => {
            emit_duplicate_ordinary(name, span, session);
            return None;
        }
        None => {}
    }

    if let Some(def) = resolver
        .ordinary
        .get(&name)
        .copied()
        .filter(|def| matches!(crate_.defs[*def].kind, DefKind::Function { .. }))
    {
        if let DefKind::Function { ty: slot, variadic, .. } = &mut crate_.defs[def].kind {
            *slot = ty;
            *variadic = match tcx.get(ty) {
                Ty::Func { variadic, .. } => *variadic,
                _ => false,
            };
        }
        let attrs = lower_common_attrs(specs, declarator, session);
        merge_def_attrs(crate_, def, attrs);
        return Some(def);
    }

    let flags = function_decl_flags(specs, declarator, session);
    let variadic = match tcx.get(ty) {
        Ty::Func { variadic, .. } => *variadic,
        _ => false,
    };
    let def = crate_.defs.push(Def {
        id: DefId(0),
        name,
        span,
        kind: DefKind::Function {
            ty,
            has_body: false,
            is_static: flags.is_static,
            is_inline: flags.is_inline,
            is_extern_inline: flags.is_extern_inline,
            no_instrument_function: flags.no_instrument_function,
            variadic,
        },
    });
    crate_.defs[def].id = def;
    let attrs = lower_common_attrs(specs, declarator, session);
    merge_def_attrs(crate_, def, attrs);
    Some(def)
}

/// Lower the runtime bound for a block-scope VLA declaration.
///
/// The declarator-to-type fold keeps the final type shape, but the bound
/// expression itself must be stored on the HIR local so CFG lowering can
/// evaluate it at the declaration point. We only attach the expression when
/// the declared object itself is a top-level VLA array; pointer-to-VLA
/// declarations do not allocate a VLA local.
#[allow(clippy::too_many_arguments)]
fn lower_top_level_vla_len(
    declarator: &Declarator,
    ty: TyId,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<HirExprId> {
    if !matches!(tcx.get(ty), Ty::Array { is_vla: true, .. }) {
        return None;
    }

    let Some(DerivedDeclarator::Array(arr_decl)) = declarator.derived.last() else {
        return None;
    };
    let size_expr = arr_decl.size.as_ref()?;
    if eval_array_bound_as_u64(
        size_expr,
        DeclScope::Block,
        Some(scope),
        Some(body),
        tcx,
        resolver,
        crate_,
        session,
    )
    .is_some()
    {
        return None;
    }

    Some(lower_expr(size_expr, body, scope, crate_, tcx, resolver, session))
}

/// Push an `HirExprKind::LocalRef` into `body.exprs` and return its id.
///
/// Used by [`lower_initializer`] to materialise the "root" lvalue for
/// an aggregate local whenever it needs a fresh HIR node (each
/// component assignment requires its own tree of `Field` / `Index`
/// nodes referring back to this root).
fn push_local_ref(local: Local, ty: TyId, span: Span, body: &mut Body) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty,
        value_cat: ValueCat::LValue,
        span,
        kind: HirExprKind::LocalRef(local),
    });
    body.exprs[id].id = id;
    id
}

fn complete_initializer_type(
    ty: TyId,
    init: &rcc_ast::Initializer,
    tcx: &mut TyCtxt,
    crate_: &HirCrate,
) -> TyId {
    let Ty::Array { elem, len: None, is_vla: false } = tcx.get(ty).clone() else {
        return ty;
    };

    let completed_len = match init {
        rcc_ast::Initializer::Expr(e) if is_string_array_initializer(ty, e, tcx) => {
            string_initializer_len(e)
        }
        rcc_ast::Initializer::List(items)
            if braced_string_array_initializer_expr(ty, items, tcx).is_some() =>
        {
            braced_string_array_initializer_expr(ty, items, tcx).and_then(string_initializer_len)
        }
        rcc_ast::Initializer::List(items) => {
            Some(array_initializer_list_len(items, elem.ty, tcx, crate_))
        }
        _ => None,
    };

    completed_len
        .map(|len| tcx.intern(Ty::Array { elem, len: Some(len), is_vla: false }))
        .unwrap_or(ty)
}

fn is_incomplete_array_ty(ty: TyId, tcx: &TyCtxt) -> bool {
    matches!(tcx.get(ty), Ty::Array { len: None, is_vla: false, .. })
}

fn emit_incomplete_array_at_block_scope(span: Span, session: &mut Session) {
    session
        .handler
        .struct_err(span, "incomplete array type at block scope".to_string())
        .code(rcc_errors::codes::E0076)
        .emit();
}

fn array_initializer_list_len(
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    elem_ty: TyId,
    tcx: &TyCtxt,
    crate_: &HirCrate,
) -> u64 {
    if flat_scalar_initializer_items(items) {
        let leaves_per_elem = aggregate_leaf_count(elem_ty, tcx, crate_).max(1);
        let item_count = items.len() as u64;
        return item_count.saturating_add(leaves_per_elem - 1) / leaves_per_elem;
    }

    let mut cursor = 0u64;
    let mut max_len = 0u64;
    for (desigs, _) in items {
        let idx = match desigs.first() {
            Some(rcc_ast::Designator::Index(e)) => eval_const_expr_as_u64(e).unwrap_or(cursor),
            Some(rcc_ast::Designator::Range { hi, .. }) => {
                eval_const_expr_as_u64(hi).unwrap_or(cursor)
            }
            _ => cursor,
        };
        max_len = max_len.max(idx.saturating_add(1));
        cursor = idx.saturating_add(1);
    }
    max_len
}

fn flat_scalar_initializer_items(
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
) -> bool {
    !items.is_empty()
        && items.iter().all(|(desigs, init)| {
            desigs.is_empty()
                && matches!(init, rcc_ast::Initializer::Expr(expr) if is_flat_elidable_scalar_initializer_expr(expr))
        })
}

fn is_flat_elidable_scalar_initializer_expr(expr: &rcc_ast::Expr) -> bool {
    !is_string_literal_expr(expr) && !matches!(expr.kind, rcc_ast::ExprKind::CompoundLiteral { .. })
}

fn initializer_list_contains_whole_record_item(
    target_ty: TyId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    body: &Body,
    scope: &ScopeStack,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    resolver: &Resolver,
) -> bool {
    match tcx.get(target_ty).clone() {
        Ty::Array { elem, .. } => items.iter().any(|(desigs, init)| {
            desigs.is_empty()
                && record_expr_initializes_ty(init, elem.ty, body, scope, crate_, tcx, resolver)
        }),
        Ty::Record(def_id) => {
            let DefKind::Record { fields, kind, .. } = &crate_.defs[def_id].kind else {
                return false;
            };
            let field_pairs = fields.iter().map(|field| (field.name, field.ty)).collect::<Vec<_>>();
            let mut cursor = 0_u32;
            for (desigs, init) in items {
                if !desigs.is_empty() {
                    continue;
                }
                let Some(field_idx) = next_initializable_field(&field_pairs, cursor) else {
                    return false;
                };
                let field_ty = fields[field_idx as usize].ty;
                if record_expr_initializes_ty(init, field_ty, body, scope, crate_, tcx, resolver) {
                    return true;
                }
                cursor = field_idx + 1;
                if matches!(kind, RecordKind::Union) {
                    break;
                }
            }
            false
        }
        _ => false,
    }
}

fn record_expr_initializes_ty(
    init: &rcc_ast::Initializer,
    target_ty: TyId,
    body: &Body,
    scope: &ScopeStack,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    resolver: &Resolver,
) -> bool {
    if !matches!(tcx.get(target_ty), Ty::Record(_)) {
        return false;
    }
    let rcc_ast::Initializer::Expr(expr) = init else {
        return false;
    };
    expr_known_ty(expr, body, scope, crate_, tcx, resolver).is_some_and(|ty| ty == target_ty)
}

fn expr_known_ty(
    expr: &rcc_ast::Expr,
    body: &Body,
    scope: &ScopeStack,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    resolver: &Resolver,
) -> Option<TyId> {
    match &expr.kind {
        rcc_ast::ExprKind::Paren(inner) => expr_known_ty(inner, body, scope, crate_, tcx, resolver),
        rcc_ast::ExprKind::Ident(name) => binding_expr_ty(*name, body, scope, crate_, resolver),
        rcc_ast::ExprKind::Call { callee, .. } => {
            let callee_ty = expr_known_ty(callee, body, scope, crate_, tcx, resolver)?;
            match tcx.get(callee_ty) {
                Ty::Func { ret, .. } => Some(*ret),
                Ty::Ptr(q) => match tcx.get(q.ty) {
                    Ty::Func { ret, .. } => Some(*ret),
                    _ => None,
                },
                _ => None,
            }
        }
        rcc_ast::ExprKind::Index { base, .. } => {
            let base_ty = expr_known_ty(base, body, scope, crate_, tcx, resolver)?;
            match tcx.get(base_ty) {
                Ty::Array { elem, .. } => Some(elem.ty),
                Ty::Ptr(q) => Some(q.ty),
                _ => None,
            }
        }
        _ => None,
    }
}

fn binding_expr_ty(
    name: Symbol,
    body: &Body,
    scope: &ScopeStack,
    crate_: &HirCrate,
    resolver: &Resolver,
) -> Option<TyId> {
    let binding =
        scope.lookup(name).or_else(|| resolver.ordinary.get(&name).copied().map(Binding::Def))?;
    match binding {
        Binding::Local(local) => Some(body.locals[local].ty),
        Binding::Def(def_id) => match crate_.defs[def_id].kind {
            DefKind::Global { ty, .. } | DefKind::Function { ty, .. } | DefKind::Typedef(ty) => {
                Some(ty)
            }
            _ => None,
        },
    }
}

fn aggregate_leaf_count(ty: TyId, tcx: &TyCtxt, crate_: &HirCrate) -> u64 {
    match tcx.get(ty).clone() {
        Ty::Array { elem, len: Some(len), .. } => {
            len.saturating_mul(aggregate_leaf_count(elem.ty, tcx, crate_).max(1))
        }
        Ty::Record(def_id) => {
            let DefKind::Record { fields, kind, .. } = &crate_.defs[def_id].kind else {
                return 1;
            };
            let mut count = 0_u64;
            for field in fields.iter().filter(|field| is_initializable_record_field(field)) {
                count = count.saturating_add(aggregate_leaf_count(field.ty, tcx, crate_).max(1));
                if matches!(kind, RecordKind::Union) {
                    break;
                }
            }
            count
        }
        _ => 1,
    }
}

fn is_initializable_record_field(field: &Field) -> bool {
    field.name.is_some()
}

fn next_initializable_field(fields: &[(Option<Symbol>, TyId)], start: u32) -> Option<u32> {
    fields
        .iter()
        .enumerate()
        .skip(start as usize)
        .find_map(|(idx, (name, _))| name.is_some().then_some(idx as u32))
}

fn aggregate_leaf_paths(
    ty: TyId,
    tcx: &TyCtxt,
    crate_: &HirCrate,
) -> Vec<(Vec<GlobalInitDesignator>, TyId)> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    collect_aggregate_leaf_paths(ty, tcx, crate_, &mut path, &mut out);
    out
}

fn collect_aggregate_leaf_paths(
    ty: TyId,
    tcx: &TyCtxt,
    crate_: &HirCrate,
    path: &mut Vec<GlobalInitDesignator>,
    out: &mut Vec<(Vec<GlobalInitDesignator>, TyId)>,
) {
    match tcx.get(ty).clone() {
        Ty::Array { elem, len: Some(len), .. } => {
            for idx in 0..len {
                path.push(GlobalInitDesignator::Index(idx));
                collect_aggregate_leaf_paths(elem.ty, tcx, crate_, path, out);
                path.pop();
            }
        }
        Ty::Record(def_id) => {
            let DefKind::Record { fields, kind, .. } = &crate_.defs[def_id].kind else {
                out.push((path.clone(), ty));
                return;
            };
            let mut emitted = 0_u32;
            for (field_idx, field) in fields.iter().enumerate() {
                if !is_initializable_record_field(field) {
                    continue;
                }
                path.push(GlobalInitDesignator::Field(field_idx as u32));
                collect_aggregate_leaf_paths(field.ty, tcx, crate_, path, out);
                path.pop();
                emitted += 1;
                if matches!(kind, RecordKind::Union) && emitted == 1 {
                    break;
                }
            }
        }
        _ => out.push((path.clone(), ty)),
    }
}

fn is_string_array_initializer(ty: TyId, expr: &rcc_ast::Expr, tcx: &TyCtxt) -> bool {
    let rcc_ast::ExprKind::StringLit(lit) = &expr.kind else {
        return false;
    };
    match lit.encoding {
        rcc_ast::LiteralEncoding::None | rcc_ast::LiteralEncoding::Utf8 => {
            is_char_array_ty(ty, tcx)
        }
        rcc_ast::LiteralEncoding::Wide
        | rcc_ast::LiteralEncoding::Utf16
        | rcc_ast::LiteralEncoding::Utf32 => is_integer_array_ty(ty, tcx),
    }
}

fn braced_string_array_initializer_expr<'a>(
    ty: TyId,
    items: &'a [(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    tcx: &TyCtxt,
) -> Option<&'a rcc_ast::Expr> {
    let [(desigs, rcc_ast::Initializer::Expr(expr))] = items else {
        return None;
    };
    if desigs.is_empty() && is_string_array_initializer(ty, expr, tcx) {
        Some(expr)
    } else {
        None
    }
}

fn is_string_literal_expr(expr: &rcc_ast::Expr) -> bool {
    matches!(expr.kind, rcc_ast::ExprKind::StringLit(_))
}

fn is_char_array_ty(ty: TyId, tcx: &TyCtxt) -> bool {
    match tcx.get(ty) {
        Ty::Array { elem, .. } => is_char_like_ty(elem.ty, tcx),
        _ => false,
    }
}

fn is_char_like_ty(ty: TyId, tcx: &TyCtxt) -> bool {
    matches!(tcx.get(ty), Ty::Int { rank: IntRank::Char, .. })
}

fn is_integer_array_ty(ty: TyId, tcx: &TyCtxt) -> bool {
    match tcx.get(ty) {
        Ty::Array { elem, .. } => matches!(tcx.get(elem.ty), Ty::Int { .. } | Ty::Enum(_)),
        _ => false,
    }
}

fn string_initializer_len(expr: &rcc_ast::Expr) -> Option<u64> {
    let rcc_ast::ExprKind::StringLit(lit) = &expr.kind else {
        return None;
    };
    Some((string_literal_elements(lit).len() + 1) as u64)
}

fn string_literal_element_ty(lit: &rcc_ast::StringLiteral, tcx: &TyCtxt) -> TyId {
    match lit.encoding {
        rcc_ast::LiteralEncoding::None | rcc_ast::LiteralEncoding::Utf8 => tcx.char_,
        rcc_ast::LiteralEncoding::Utf16 => tcx.ushort,
        rcc_ast::LiteralEncoding::Utf32 => tcx.uint,
        rcc_ast::LiteralEncoding::Wide => tcx.int,
    }
}

fn char_literal_ty(enc: rcc_ast::LiteralEncoding, tcx: &TyCtxt) -> TyId {
    match enc {
        rcc_ast::LiteralEncoding::Utf16 => tcx.ushort,
        rcc_ast::LiteralEncoding::Utf32 => tcx.uint,
        rcc_ast::LiteralEncoding::None
        | rcc_ast::LiteralEncoding::Utf8
        | rcc_ast::LiteralEncoding::Wide => tcx.int,
    }
}

fn string_literal_elements(lit: &rcc_ast::StringLiteral) -> Vec<i128> {
    match lit.encoding {
        rcc_ast::LiteralEncoding::None | rcc_ast::LiteralEncoding::Utf8 => {
            lit.bytes.iter().map(|&b| i128::from(b)).collect()
        }
        rcc_ast::LiteralEncoding::Utf16 => match std::str::from_utf8(&lit.bytes) {
            Ok(text) => text.encode_utf16().map(i128::from).collect(),
            Err(_) => lit.bytes.iter().map(|&b| i128::from(b)).collect(),
        },
        rcc_ast::LiteralEncoding::Wide | rcc_ast::LiteralEncoding::Utf32 => {
            match std::str::from_utf8(&lit.bytes) {
                Ok(text) => text.chars().map(|ch| i128::from(ch as u32)).collect(),
                Err(_) => lit.bytes.iter().map(|&b| i128::from(b)).collect(),
            }
        }
    }
}

/// Flatten a brace-enclosed initialiser (`Initializer::List`) into a
/// sequence of `HirStmtKind::InitAssign` statements.
///
/// Semantics (C99 §6.7.8):
/// - **Scalar target + `{ v }`** — treated as `target = v;` (§6.7.8p11).
/// - **Array target** — walks elements left-to-right. Each sub-initialiser
///   binds to the "current element index", which advances after each
///   item and can be reset by a `[N]` designator (§6.7.8p6, §6.7.8p17).
///   After the last explicit item, every remaining element is
///   zero-filled (§6.7.8p21).
/// - **Struct target** — same walker, but the cursor is a field index,
///   and `.name` designators reset it (§6.7.8p7, §6.7.8p17).
/// - **Union target** — only one member is initialised: the first one,
///   unless a designator selects a different member (§6.7.8p15-17).
///
/// Zero-fill policy: rather than emit an explicit assignment-to-zero for
/// every unset scalar component (which would explode on large arrays),
/// this pass only zero-fills *components that the walker actually
/// visits*. For `int a[3] = {1}` the walker visits all three slots
/// (because the explicit list still advances the cursor until the array
/// is covered on the zero-fill pass) so the acceptance test sees three
/// stores `a[0]=1, a[1]=0, a[2]=0`. Tail zero-fill for huge arrays is
/// deferred to codegen (BSS / static-init constant data); an aggregate
/// zero-fill marker expression is not introduced at this stage.
///
/// Deferred (not handled here):
/// - Compound-literal lowering (`(int[]){1,2,3}`).
/// - Initialising a struct/union with a non-brace aggregate (GNU ext).
#[allow(clippy::too_many_arguments)]
pub fn lower_initializer(
    target: HirExprId,
    target_ty: TyId,
    init: &rcc_ast::Initializer,
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    match init {
        rcc_ast::Initializer::Expr(e) => {
            if is_string_array_initializer(target_ty, e, tcx) {
                lower_string_array_initializer(target, target_ty, e, span, body, tcx, out);
                return;
            }
            // Scalar initialiser.
            let rhs = lower_expr(e, body, scope, crate_, tcx, resolver, session);
            emit_assign_stmt(target, rhs, span, body, out);
        }
        rcc_ast::Initializer::List(items) => {
            if let Some(expr) = braced_string_array_initializer_expr(target_ty, items, tcx) {
                lower_string_array_initializer(target, target_ty, expr, span, body, tcx, out);
                return;
            }
            if let Ty::Vector { elem, lanes, .. } = tcx.get(target_ty).clone() {
                lower_vector_initializer(
                    target, target_ty, elem, lanes, items, span, body, scope, crate_, tcx,
                    resolver, session, out,
                );
                return;
            }
            lower_initializer_list(
                target, target_ty, items, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_string_array_initializer(
    target: HirExprId,
    target_ty: TyId,
    expr: &rcc_ast::Expr,
    span: Span,
    body: &mut Body,
    tcx: &mut TyCtxt,
    out: &mut Vec<HirStmtId>,
) {
    let rcc_ast::ExprKind::StringLit(lit) = &expr.kind else {
        return;
    };
    let Ty::Array { elem, len, .. } = tcx.get(target_ty).clone() else {
        return;
    };
    let mut values = string_literal_elements(lit);
    values.push(0);
    let write_len = len.unwrap_or(values.len() as u64);
    for i in 0..write_len {
        let value = values.get(i as usize).copied().unwrap_or(0);
        let index_expr = push_index_expr(target, i, elem.ty, span, body, tcx);
        let rhs = push_int_const(value, elem.ty, span, body);
        emit_assign_stmt(index_expr, rhs, span, body, out);
    }
}

fn emit_invalid_initializer_designator(span: Span, message: &str, session: &mut Session) {
    session.handler.struct_err(span, message.to_string()).code(rcc_errors::codes::E0079).emit();
}

#[allow(clippy::too_many_arguments)]
fn eval_range_designator(
    lo: &rcc_ast::Expr,
    hi: &rcc_ast::Expr,
    len: Option<u64>,
    span: Span,
    scope: DeclScope,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<(u64, u64)> {
    let Some(lo) = eval_array_bound_as_i128(lo, scope, None, None, tcx, resolver, crate_, session)
    else {
        emit_invalid_initializer_designator(
            span,
            "initializer range lower bound must be an integer constant",
            session,
        );
        return None;
    };
    let Some(hi) = eval_array_bound_as_i128(hi, scope, None, None, tcx, resolver, crate_, session)
    else {
        emit_invalid_initializer_designator(
            span,
            "initializer range upper bound must be an integer constant",
            session,
        );
        return None;
    };
    if lo < 0 || hi < 0 {
        emit_invalid_initializer_designator(
            span,
            "initializer range designator bound cannot be negative",
            session,
        );
        return None;
    }
    if lo > hi {
        emit_invalid_initializer_designator(
            span,
            "initializer range lower bound is greater than upper bound",
            session,
        );
        return None;
    }
    let Ok(lo) = u64::try_from(lo) else {
        emit_invalid_initializer_designator(
            span,
            "initializer range lower bound is too large",
            session,
        );
        return None;
    };
    let Ok(hi) = u64::try_from(hi) else {
        emit_invalid_initializer_designator(
            span,
            "initializer range upper bound is too large",
            session,
        );
        return None;
    };
    if len.is_some_and(|n| hi >= n) {
        emit_invalid_initializer_designator(
            span,
            "array initializer range designator exceeds the declared bound",
            session,
        );
        return None;
    }
    Some((lo, hi))
}

#[allow(clippy::too_many_arguments)]
fn lower_global_initializer(
    target_ty: TyId,
    init: &rcc_ast::Initializer,
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> GlobalInit {
    let mut entries = Vec::new();
    lower_global_initializer_into(
        target_ty,
        init,
        span,
        Vec::new(),
        body,
        scope,
        crate_,
        tcx,
        resolver,
        session,
        &mut entries,
    );
    GlobalInit { ty: target_ty, entries }
}

#[allow(clippy::too_many_arguments)]
fn lower_global_initializer_into(
    target_ty: TyId,
    init: &rcc_ast::Initializer,
    span: Span,
    path: Vec<GlobalInitDesignator>,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<GlobalInitEntry>,
) {
    match init {
        rcc_ast::Initializer::Expr(e) if is_string_array_initializer(target_ty, e, tcx) => {
            lower_global_string_array_initializer(target_ty, e, span, path, tcx, out);
        }
        rcc_ast::Initializer::Expr(e) => {
            if let Some(value) =
                lower_file_scope_compound_literal_address(e, crate_, tcx, resolver, session)
            {
                out.push(GlobalInitEntry { path, ty: target_ty, expr: None, value, span: e.span });
                return;
            }
            let expr = lower_expr(e, body, scope, crate_, tcx, resolver, session);
            let value = lower_global_init_expr(e, crate_, tcx, resolver);
            out.push(GlobalInitEntry {
                path,
                ty: target_ty,
                expr: Some(expr),
                value,
                span: e.span,
            });
        }
        rcc_ast::Initializer::List(items) => {
            if let Some(expr) = braced_string_array_initializer_expr(target_ty, items, tcx) {
                lower_global_string_array_initializer(target_ty, expr, span, path, tcx, out);
                return;
            }
            if let Ty::Vector { elem, lanes, .. } = tcx.get(target_ty).clone() {
                lower_global_vector_initializer(
                    target_ty, elem, lanes, items, span, path, body, scope, crate_, tcx, resolver,
                    session, out,
                );
                return;
            }
            if lower_global_flat_brace_elision_initializer(
                target_ty,
                items,
                span,
                path.clone(),
                body,
                scope,
                crate_,
                tcx,
                resolver,
                session,
                out,
            ) {
                return;
            }
            lower_global_initializer_list(
                target_ty, items, span, path, body, scope, crate_, tcx, resolver, session, out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_global_flat_brace_elision_initializer(
    target_ty: TyId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    base_path: Vec<GlobalInitDesignator>,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<GlobalInitEntry>,
) -> bool {
    if !flat_scalar_initializer_items(items) {
        return false;
    }
    if initializer_list_contains_whole_record_item(
        target_ty, items, body, scope, crate_, tcx, resolver,
    ) {
        return false;
    }
    let leaves = aggregate_leaf_paths(target_ty, tcx, crate_);
    if leaves.is_empty() {
        return false;
    }

    for (idx, (_, sub_init)) in items.iter().enumerate() {
        let Some((leaf_path, leaf_ty)) = leaves.get(idx) else {
            emit_invalid_initializer_designator(
                span,
                "initializer has more scalar leaves than the target aggregate",
                session,
            );
            break;
        };
        let rcc_ast::Initializer::Expr(expr) = sub_init else {
            unreachable!("flat_scalar_initializer_items only accepts expression leaves")
        };
        let mut path = base_path.clone();
        path.extend(leaf_path.iter().copied());
        if let Some(value) =
            lower_file_scope_compound_literal_address(expr, crate_, tcx, resolver, session)
        {
            out.push(GlobalInitEntry { path, ty: *leaf_ty, expr: None, value, span: expr.span });
            continue;
        }
        let expr_id = lower_expr(expr, body, scope, crate_, tcx, resolver, session);
        let value = lower_global_init_expr(expr, crate_, tcx, resolver);
        out.push(GlobalInitEntry {
            path,
            ty: *leaf_ty,
            expr: Some(expr_id),
            value,
            span: expr.span,
        });
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn lower_global_initializer_list(
    target_ty: TyId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    path: Vec<GlobalInitDesignator>,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<GlobalInitEntry>,
) {
    match tcx.get(target_ty).clone() {
        Ty::Array { elem, len, .. } => {
            let mut cursor = 0u64;
            for (desigs, sub_init) in items {
                let (idx_range, nested_desigs) = match desigs.split_first() {
                    Some((rcc_ast::Designator::Index(e), rest)) => {
                        let idx = eval_const_expr_as_u64(e).unwrap_or(cursor);
                        (Some((idx, idx)), rest)
                    }
                    Some((rcc_ast::Designator::Range { lo, hi }, rest)) => (
                        eval_range_designator(
                            lo,
                            hi,
                            len,
                            span,
                            DeclScope::File,
                            tcx,
                            resolver,
                            crate_,
                            session,
                        ),
                        rest,
                    ),
                    Some((rcc_ast::Designator::Field(_), _)) => {
                        emit_invalid_initializer_designator(
                            span,
                            "field designator cannot initialize an array object",
                            session,
                        );
                        continue;
                    }
                    None => (Some((cursor, cursor)), &[][..]),
                };
                let Some((first_idx, last_idx)) = idx_range else {
                    continue;
                };
                if len.is_some_and(|n| last_idx >= n) {
                    emit_invalid_initializer_designator(
                        span,
                        "array initializer designator exceeds the declared bound",
                        session,
                    );
                    continue;
                }
                for idx in first_idx..=last_idx {
                    let mut next_path = path.clone();
                    next_path.push(GlobalInitDesignator::Index(idx));
                    if nested_desigs.is_empty() {
                        lower_global_initializer_into(
                            elem.ty, sub_init, span, next_path, body, scope, crate_, tcx, resolver,
                            session, out,
                        );
                    } else {
                        let nested = vec![(nested_desigs.to_vec(), sub_init.clone())];
                        lower_global_initializer_list(
                            elem.ty, &nested, span, next_path, body, scope, crate_, tcx, resolver,
                            session, out,
                        );
                    }
                }
                cursor = last_idx.saturating_add(1);
            }
        }
        Ty::Record(def_id) => {
            let (fields, is_union): (Vec<(Option<Symbol>, TyId)>, bool) =
                match &crate_.defs[def_id].kind {
                    DefKind::Record { fields, kind, .. } => (
                        fields.iter().map(|f| (f.name, f.ty)).collect(),
                        matches!(kind, RecordKind::Union),
                    ),
                    _ => return,
                };
            let mut cursor = 0u32;
            for (desigs, sub_init) in items {
                let (field_idx, nested_desigs) = match desigs.split_first() {
                    Some((rcc_ast::Designator::Field(name), rest)) => {
                        let Some((i, _)) =
                            fields.iter().enumerate().find(|(_, (fname, _))| *fname == Some(*name))
                        else {
                            emit_invalid_initializer_designator(
                                span,
                                "field designator does not name a member of the target record",
                                session,
                            );
                            continue;
                        };
                        (i as u32, rest)
                    }
                    Some((rcc_ast::Designator::Index(_), _)) => {
                        emit_invalid_initializer_designator(
                            span,
                            "array designator cannot initialize a record object",
                            session,
                        );
                        continue;
                    }
                    Some((rcc_ast::Designator::Range { .. }, _)) => {
                        emit_invalid_initializer_designator(
                            span,
                            "range designator cannot initialize a record object",
                            session,
                        );
                        continue;
                    }
                    None => {
                        let Some(next_idx) = next_initializable_field(&fields, cursor) else {
                            emit_invalid_initializer_designator(
                                span,
                                "record initializer has more elements than fields",
                                session,
                            );
                            continue;
                        };
                        (next_idx, &[][..])
                    }
                };
                if field_idx as usize >= fields.len() {
                    emit_invalid_initializer_designator(
                        span,
                        "record initializer has more elements than fields",
                        session,
                    );
                    continue;
                }
                let (_, field_ty) = fields[field_idx as usize];
                let mut next_path = path.clone();
                next_path.push(GlobalInitDesignator::Field(field_idx));
                if nested_desigs.is_empty() {
                    lower_global_initializer_into(
                        field_ty, sub_init, span, next_path, body, scope, crate_, tcx, resolver,
                        session, out,
                    );
                } else {
                    let nested = vec![(nested_desigs.to_vec(), sub_init.clone())];
                    lower_global_initializer_list(
                        field_ty, &nested, span, next_path, body, scope, crate_, tcx, resolver,
                        session, out,
                    );
                }
                cursor = field_idx.saturating_add(1);
                if is_union {
                    break;
                }
            }
        }
        _ => {
            if let Some((desigs, nested)) = items.first() {
                if !desigs.is_empty() {
                    emit_invalid_initializer_designator(
                        span,
                        "designator cannot initialize a scalar object",
                        session,
                    );
                }
                lower_global_initializer_into(
                    target_ty, nested, span, path, body, scope, crate_, tcx, resolver, session, out,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_vector_initializer(
    target: HirExprId,
    target_ty: TyId,
    elem_ty: TyId,
    lanes: u32,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    let lane_exprs = lower_vector_lane_exprs(
        elem_ty, lanes, items, span, body, scope, crate_, tcx, resolver, session,
    );
    let init_expr = push_vector_init_expr(target_ty, lane_exprs, span, body);
    emit_assign_stmt(target, init_expr, span, body, out);
}

#[allow(clippy::too_many_arguments)]
fn lower_vector_lane_exprs(
    _elem_ty: TyId,
    lanes: u32,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Vec<HirExprId> {
    let mut exprs = Vec::with_capacity(lanes as usize);
    for (desigs, sub_init) in items {
        if !desigs.is_empty() {
            emit_invalid_initializer_designator(
                span,
                "designator cannot initialize a vector object",
                session,
            );
            continue;
        }
        let Some(expr) = scalar_initializer_expr(sub_init) else {
            emit_invalid_initializer_designator(
                span,
                "vector initializer lane must be a scalar expression",
                session,
            );
            continue;
        };
        if exprs.len() >= lanes as usize {
            emit_invalid_initializer_designator(
                span,
                "vector initializer has more elements than lanes",
                session,
            );
            break;
        }
        exprs.push(lower_expr(expr, body, scope, crate_, tcx, resolver, session));
    }
    while exprs.len() < lanes as usize {
        // C zero-initialization starts from the integer constant `0` and then
        // applies the destination conversion. Keeping the synthetic lane as
        // `int` lets typeck insert the same conversion wrapper as it would for
        // an explicit `{ 0 }` initializer, including float vector lanes.
        exprs.push(push_int_const(0, tcx.int, span, body));
    }
    exprs
}

fn scalar_initializer_expr(init: &rcc_ast::Initializer) -> Option<&rcc_ast::Expr> {
    match init {
        rcc_ast::Initializer::Expr(expr) => Some(expr),
        rcc_ast::Initializer::List(items) if items.len() == 1 && items[0].0.is_empty() => {
            scalar_initializer_expr(&items[0].1)
        }
        rcc_ast::Initializer::List(_) => None,
    }
}

fn push_vector_init_expr(
    ty: TyId,
    lanes: Vec<HirExprId>,
    span: Span,
    body: &mut Body,
) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::VectorInit { ty, lanes },
    });
    body.exprs[id].id = id;
    id
}

#[allow(clippy::too_many_arguments)]
fn lower_file_scope_compound_literal_address(
    expr: &rcc_ast::Expr,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<GlobalInitValue> {
    let rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::AddrOf, operand } = &expr.kind else {
        return None;
    };
    let rcc_ast::ExprKind::CompoundLiteral { ty, init } = &operand.kind else {
        return None;
    };

    let object_ty =
        lower_type_name_in_scope(ty, DeclScope::File, None, tcx, resolver, crate_, session);
    let literal_def = materialize_file_scope_compound_literal(
        object_ty,
        init,
        operand.span,
        crate_,
        tcx,
        resolver,
        session,
    );
    Some(GlobalInitValue::Address { def: Some(literal_def), offset: 0 })
}

#[allow(clippy::too_many_arguments)]
fn materialize_file_scope_compound_literal(
    ty: TyId,
    init: &rcc_ast::Initializer,
    span: Span,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> DefId {
    let name = compound_literal_symbol(crate_, session);
    let def_id = crate_.defs.push(Def {
        id: DefId(0),
        name,
        span,
        kind: DefKind::Global {
            ty,
            quals: ObjectQuals::none(),
            thread_local: false,
            linkage: Linkage::Internal,
            init: None,
        },
    });
    crate_.defs[def_id].id = def_id;

    let mut init_body = Body::default();
    let scope = ScopeStack::new();
    let global_init = lower_global_initializer(
        ty,
        init,
        span,
        &mut init_body,
        &scope,
        crate_,
        tcx,
        resolver,
        session,
    );
    if let DefKind::Global { init, .. } = &mut crate_.defs[def_id].kind {
        *init = Some(global_init);
    }
    if !init_body.exprs.is_empty() {
        crate_.global_init_bodies.insert(def_id, init_body);
    }
    def_id
}

#[allow(clippy::too_many_arguments)]
fn lower_global_vector_initializer(
    target_ty: TyId,
    elem_ty: TyId,
    lanes: u32,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    path: Vec<GlobalInitDesignator>,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<GlobalInitEntry>,
) {
    let lane_exprs = lower_vector_lane_exprs(
        elem_ty, lanes, items, span, body, scope, crate_, tcx, resolver, session,
    );
    let expr = push_vector_init_expr(target_ty, lane_exprs, span, body);
    out.push(GlobalInitEntry {
        path,
        ty: target_ty,
        expr: Some(expr),
        value: GlobalInitValue::Vector(vec![GlobalInitValue::Error; lanes as usize]),
        span,
    });
}

fn compound_literal_symbol(crate_: &HirCrate, session: &mut Session) -> Symbol {
    session.interner.intern(&format!("__rcc_compound_literal_{}", crate_.defs.len()))
}

fn lower_global_string_array_initializer(
    target_ty: TyId,
    expr: &rcc_ast::Expr,
    span: Span,
    path: Vec<GlobalInitDesignator>,
    tcx: &TyCtxt,
    out: &mut Vec<GlobalInitEntry>,
) {
    let rcc_ast::ExprKind::StringLit(lit) = &expr.kind else {
        return;
    };
    let Ty::Array { elem, len, .. } = tcx.get(target_ty).clone() else {
        return;
    };
    let mut values = string_literal_elements(lit);
    values.push(0);
    let write_len = len.unwrap_or(values.len() as u64);
    for i in 0..write_len {
        let mut next_path = path.clone();
        next_path.push(GlobalInitDesignator::Index(i));
        let value = values
            .get(i as usize)
            .copied()
            .map(GlobalInitValue::Int)
            .unwrap_or(GlobalInitValue::Zero);
        out.push(GlobalInitEntry { path: next_path, ty: elem.ty, expr: None, value, span });
    }
}

fn lower_global_init_expr(
    expr: &rcc_ast::Expr,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
) -> GlobalInitValue {
    match &expr.kind {
        rcc_ast::ExprKind::IntLit(lit) => {
            i128::try_from(lit.value).map(GlobalInitValue::Int).unwrap_or(GlobalInitValue::Error)
        }
        rcc_ast::ExprKind::CharLit(lit) => GlobalInitValue::Int(i128::from(lit.value)),
        rcc_ast::ExprKind::FloatLit(lit) => GlobalInitValue::Float(lit.value),
        rcc_ast::ExprKind::StringLit(lit) => {
            let def_id = intern_string_literal(lit, expr.span, crate_, tcx, resolver);
            GlobalInitValue::StringLiteral(def_id)
        }
        rcc_ast::ExprKind::LabelAddr(label) => resolver
            .current_function
            .map(|function| GlobalInitValue::LabelAddress { function, label: *label })
            .unwrap_or(GlobalInitValue::Error),
        _ => GlobalInitValue::Error,
    }
}

/// Core of [`lower_initializer`] for brace-enclosed lists. See that
/// function's docs for the overall semantics.
#[allow(clippy::too_many_arguments)]
fn lower_initializer_list(
    target: HirExprId,
    target_ty: TyId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    if let Some(expr) = braced_string_array_initializer_expr(target_ty, items, tcx) {
        lower_string_array_initializer(target, target_ty, expr, span, body, tcx, out);
        return;
    }

    // Scalar target with a `{ v }` wrapper: unwrap and assign. Empty
    // `{}` on a scalar is malformed C but we emit nothing here; typeck
    // will diagnose.
    let target_ty_kind = tcx.get(target_ty).clone();
    if !matches!(target_ty_kind, Ty::Array { .. } | Ty::Record(_)) {
        if let Some((desigs, nested)) = items.first() {
            if !desigs.is_empty() {
                emit_invalid_initializer_designator(
                    span,
                    "designator cannot initialize a scalar object",
                    session,
                );
            }
            // Recurse so nested `{{ 1 }}` still works, but use the same
            // scalar target lvalue.
            lower_initializer(
                target, target_ty, nested, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
        return;
    }

    match target_ty_kind {
        Ty::Array { elem, len, .. } => {
            if lower_flat_brace_elision_initializer(
                target, target_ty, items, span, body, scope, crate_, tcx, resolver, session, out,
            ) {
                return;
            }
            lower_array_initializer(
                target, elem.ty, len, items, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
        Ty::Record(def_id) => {
            if lower_flat_brace_elision_initializer(
                target, target_ty, items, span, body, scope, crate_, tcx, resolver, session, out,
            ) {
                return;
            }
            lower_record_initializer(
                target, def_id, items, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
        _ => unreachable!("scalar case handled above"),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_flat_brace_elision_initializer(
    target: HirExprId,
    target_ty: TyId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) -> bool {
    if !flat_scalar_initializer_items(items) {
        return false;
    }
    if initializer_list_contains_whole_record_item(
        target_ty, items, body, scope, crate_, tcx, resolver,
    ) {
        return false;
    }
    let leaves = aggregate_leaf_paths(target_ty, tcx, crate_);
    if leaves.is_empty() {
        return false;
    }

    let mut written = FxHashSet::default();
    for (idx, (_, sub_init)) in items.iter().enumerate() {
        let Some((path, leaf_ty)) = leaves.get(idx) else {
            emit_invalid_initializer_designator(
                span,
                "initializer has more scalar leaves than the target aggregate",
                session,
            );
            break;
        };
        let leaf_target = push_lvalue_path(target, target_ty, path, span, body, tcx, crate_);
        lower_initializer(
            leaf_target,
            *leaf_ty,
            sub_init,
            span,
            body,
            scope,
            crate_,
            tcx,
            resolver,
            session,
            out,
        );
        written.insert(path.clone());
    }

    for (path, leaf_ty) in leaves {
        if written.contains(&path) {
            continue;
        }
        let leaf_target = push_lvalue_path(target, target_ty, &path, span, body, tcx, crate_);
        emit_zero_init(
            leaf_target,
            leaf_ty,
            span,
            body,
            scope,
            crate_,
            tcx,
            resolver,
            session,
            out,
        );
    }
    true
}

/// Walker for array initialiser lists.
#[allow(clippy::too_many_arguments)]
fn lower_array_initializer(
    target: HirExprId,
    elem_ty: TyId,
    len: Option<u64>,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    // Track which indices have been explicitly written; zero-fill the
    // remaining slots of a known-length array at the end.
    let mut written: FxHashSet<u64> = FxHashSet::default();
    let mut cursor: u64 = 0;

    for (desigs, sub_init) in items {
        // A designator list resets the cursor. `[N]` for arrays, then
        // subsequent designators drill into the element's aggregate.
        // Here we only support a leading `[N]` designator for the
        // array level; deeper designators are recursed into via the
        // element-level initialiser walker.
        let (idx_range, nested_desigs) = match desigs.split_first() {
            Some((rcc_ast::Designator::Index(e), rest)) => {
                let i = eval_const_expr_as_u64(e).unwrap_or(cursor);
                (Some((i, i)), rest)
            }
            Some((rcc_ast::Designator::Range { lo, hi }, rest)) => (
                eval_range_designator(
                    lo,
                    hi,
                    len,
                    span,
                    DeclScope::Block,
                    tcx,
                    resolver,
                    crate_,
                    session,
                ),
                rest,
            ),
            Some((rcc_ast::Designator::Field(_), _)) => {
                emit_invalid_initializer_designator(
                    span,
                    "field designator cannot initialize an array object",
                    session,
                );
                continue;
            }
            None => (Some((cursor, cursor)), &[][..]),
        };
        let Some((first_idx, last_idx)) = idx_range else {
            continue;
        };
        if len.is_some_and(|n| last_idx >= n) {
            emit_invalid_initializer_designator(
                span,
                "array initializer designator exceeds the declared bound",
                session,
            );
            continue;
        }

        // Emit the assignment for this sub-initialiser, nested inside
        // each selected target[idx] lvalue.
        for idx in first_idx..=last_idx {
            let index_expr = push_index_expr(target, idx, elem_ty, span, body, tcx);
            if nested_desigs.is_empty() {
                lower_initializer(
                    index_expr, elem_ty, sub_init, span, body, scope, crate_, tcx, resolver,
                    session, out,
                );
            } else {
                // Nested designators like `{ [1].x = 5 }` — feed the
                // remaining designators through a synthetic single-item
                // list so the element-level walker can consume them.
                let nested_items = vec![(nested_desigs.to_vec(), sub_init.clone())];
                lower_initializer_list(
                    index_expr,
                    elem_ty,
                    &nested_items,
                    span,
                    body,
                    scope,
                    crate_,
                    tcx,
                    resolver,
                    session,
                    out,
                );
            }
            written.insert(idx);
        }
        cursor = last_idx.saturating_add(1);
    }

    // Zero-fill the tail (and any gaps, for designated inits) when the
    // array length is known. For the classic `int a[3] = {1}` case this
    // emits `a[1]=0; a[2]=0;` after the explicit `a[0]=1;`.
    let _ = cursor; // only used to advance during the walk above.
    if let Some(n) = len {
        for i in 0..n {
            if written.contains(&i) {
                continue;
            }
            let index_expr = push_index_expr(target, i, elem_ty, span, body, tcx);
            emit_zero_init(
                index_expr, elem_ty, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
    }
}

/// Walker for struct/union initialiser lists.
#[allow(clippy::too_many_arguments)]
fn lower_record_initializer(
    target: HirExprId,
    def_id: DefId,
    items: &[(Vec<rcc_ast::Designator>, rcc_ast::Initializer)],
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    // Pull the record's fields out. If the def is incomplete (still
    // under lowering) or isn't a Record at all, bail.
    let (fields, is_union): (Vec<(Option<Symbol>, TyId)>, bool) = match &crate_.defs[def_id].kind {
        DefKind::Record { fields, kind, .. } => {
            (fields.iter().map(|f| (f.name, f.ty)).collect(), matches!(kind, RecordKind::Union))
        }
        _ => return,
    };

    let mut written: FxHashSet<u32> = FxHashSet::default();
    let mut cursor: u32 = 0;

    for (desigs, sub_init) in items {
        let (field_idx, nested_desigs) = match desigs.split_first() {
            Some((rcc_ast::Designator::Field(name), rest)) => {
                let Some((i, _)) =
                    fields.iter().enumerate().find(|(_, (fname, _))| *fname == Some(*name))
                else {
                    emit_invalid_initializer_designator(
                        span,
                        "field designator does not name a member of the target record",
                        session,
                    );
                    continue;
                };
                (i as u32, rest)
            }
            Some((rcc_ast::Designator::Index(_), _)) => {
                emit_invalid_initializer_designator(
                    span,
                    "array designator cannot initialize a record object",
                    session,
                );
                continue;
            }
            Some((rcc_ast::Designator::Range { .. }, _)) => {
                emit_invalid_initializer_designator(
                    span,
                    "range designator cannot initialize a record object",
                    session,
                );
                continue;
            }
            None => {
                let Some(next_idx) = next_initializable_field(&fields, cursor) else {
                    emit_invalid_initializer_designator(
                        span,
                        "record initializer has more elements than fields",
                        session,
                    );
                    continue;
                };
                (next_idx, &[][..])
            }
        };

        if field_idx as usize >= fields.len() {
            emit_invalid_initializer_designator(
                span,
                "record initializer has more elements than fields",
                session,
            );
            continue;
        }
        let (_, field_ty) = fields[field_idx as usize];

        let field_expr = push_field_expr(target, field_idx, field_ty, span, body);
        if nested_desigs.is_empty() {
            lower_initializer(
                field_expr, field_ty, sub_init, span, body, scope, crate_, tcx, resolver, session,
                out,
            );
        } else {
            let nested_items = vec![(nested_desigs.to_vec(), sub_init.clone())];
            lower_initializer_list(
                field_expr,
                field_ty,
                &nested_items,
                span,
                body,
                scope,
                crate_,
                tcx,
                resolver,
                session,
                out,
            );
        }
        written.insert(field_idx);
        cursor = field_idx + 1;

        // C99 §6.7.8p15-17: a union initialiser sets exactly one member.
        if is_union {
            break;
        }
    }

    // Zero-fill any un-initialised struct fields (not for unions — the
    // member selection already picks exactly one member to live).
    if !is_union {
        for (i, (name, field_ty)) in fields.iter().enumerate() {
            if name.is_none() {
                continue;
            }
            let i = i as u32;
            if written.contains(&i) {
                continue;
            }
            let field_expr = push_field_expr(target, i, *field_ty, span, body);
            emit_zero_init(
                field_expr, *field_ty, span, body, scope, crate_, tcx, resolver, session, out,
            );
        }
    }
}

/// Recursively zero-initialise a component whose type may itself be an
/// aggregate. Scalar components become `comp = 0;`; nested aggregates
/// recurse so every leaf scalar is written.
#[allow(clippy::too_many_arguments)]
fn emit_zero_init(
    target: HirExprId,
    target_ty: TyId,
    span: Span,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
    out: &mut Vec<HirStmtId>,
) {
    let ty_kind = tcx.get(target_ty).clone();
    match ty_kind {
        Ty::Array { elem, len, .. } => {
            if let Some(n) = len {
                for i in 0..n {
                    let index_expr = push_index_expr(target, i, elem.ty, span, body, tcx);
                    emit_zero_init(
                        index_expr, elem.ty, span, body, scope, crate_, tcx, resolver, session, out,
                    );
                }
            }
            // Unknown-length arrays can't be zero-filled here; rely on
            // the declaration emitting a BSS slot at codegen time.
        }
        Ty::Record(def_id) => {
            let fields: Vec<(Option<Symbol>, TyId)> = match &crate_.defs[def_id].kind {
                DefKind::Record { fields, .. } => fields.iter().map(|f| (f.name, f.ty)).collect(),
                _ => return,
            };
            for (i, (_, fty)) in fields.iter().enumerate() {
                let field_expr = push_field_expr(target, i as u32, *fty, span, body);
                emit_zero_init(
                    field_expr, *fty, span, body, scope, crate_, tcx, resolver, session, out,
                );
            }
        }
        _ => {
            // Scalar leaf: emit the canonical integer zero and let typeck
            // insert the destination conversion. `scope` is unused for
            // zero-init since we synthesise the RHS directly.
            let _ = (scope, resolver, session);
            let zero = push_int_const(0, tcx.int, span, body);
            emit_assign_stmt(target, zero, span, body, out);
        }
    }
}

fn push_lvalue_path(
    root: HirExprId,
    root_ty: TyId,
    path: &[GlobalInitDesignator],
    span: Span,
    body: &mut Body,
    tcx: &TyCtxt,
    crate_: &HirCrate,
) -> HirExprId {
    let mut current = root;
    let mut current_ty = root_ty;
    for component in path {
        match *component {
            GlobalInitDesignator::Index(idx) => {
                let Ty::Array { elem, .. } = tcx.get(current_ty).clone() else {
                    break;
                };
                current = push_index_expr(current, idx, elem.ty, span, body, tcx);
                current_ty = elem.ty;
            }
            GlobalInitDesignator::Field(field_idx) => {
                let Ty::Record(def_id) = tcx.get(current_ty).clone() else {
                    break;
                };
                let field_ty = match &crate_.defs[def_id].kind {
                    DefKind::Record { fields, .. } => {
                        fields.get(field_idx as usize).map(|field| field.ty).unwrap_or(current_ty)
                    }
                    _ => current_ty,
                };
                current = push_field_expr(current, field_idx, field_ty, span, body);
                current_ty = field_ty;
            }
        }
    }
    current
}

/// Build a `target[idx]` lvalue expression and push it into `body.exprs`.
fn push_index_expr(
    base: HirExprId,
    idx: u64,
    elem_ty: TyId,
    span: Span,
    body: &mut Body,
    tcx: &TyCtxt,
) -> HirExprId {
    let idx_id = push_int_const(i128::from(idx), tcx.int, span, body);
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: elem_ty,
        value_cat: ValueCat::LValue,
        span,
        kind: HirExprKind::Index { base, index: idx_id },
    });
    body.exprs[id].id = id;
    id
}

/// Build a `target.<field_index>` lvalue expression.
fn push_field_expr(
    base: HirExprId,
    field_index: u32,
    field_ty: TyId,
    span: Span,
    body: &mut Body,
) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: field_ty,
        value_cat: ValueCat::LValue,
        span,
        kind: HirExprKind::Field { base, field_index },
    });
    body.exprs[id].id = id;
    id
}

/// Push an `IntConst` expression and return its id.
fn push_int_const(value: i128, ty: TyId, span: Span, body: &mut Body) -> HirExprId {
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::IntConst(value),
    });
    body.exprs[id].id = id;
    id
}

/// Build an initializer store statement appended to `out`.
fn emit_assign_stmt(
    lhs: HirExprId,
    rhs: HirExprId,
    span: Span,
    body: &mut Body,
    out: &mut Vec<HirStmtId>,
) {
    let stmt_id = body.stmts.push(HirStmt {
        id: HirStmtId(0),
        span,
        kind: HirStmtKind::InitAssign { lhs, rhs },
    });
    body.stmts[stmt_id].id = stmt_id;
    out.push(stmt_id);
}

/// Lower a `for`-init declaration (C99 §6.8.5p3 allows a declaration in
/// place of the first expression). Returns the id of the first
/// resulting `HirStmt` (normally there's exactly one declarator, but in
/// the rare `for (int a, b; ...)` case we still want a single statement
/// to feed into `HirStmtKind::For::init` — the remaining declarators
/// become trailing statements in the enclosing block if any; here we
/// collapse them into a single `Block` for simplicity).
#[allow(clippy::too_many_arguments)]
fn lower_for_init_decl(
    decl: &rcc_ast::Decl,
    stmt_span: Span,
    body: &mut Body,
    scope: &mut ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<HirStmtId> {
    let mut ids: Vec<HirStmtId> = Vec::new();
    lower_block_decl(decl, body, scope, crate_, tcx, resolver, session, &mut ids);
    match ids.len() {
        0 => None,
        1 => Some(ids[0]),
        _ => {
            // Multi-declarator init: wrap them in a synthetic Block
            // so `HirStmtKind::For::init` can still point at a single
            // statement.
            let block_id = body.stmts.push(HirStmt {
                id: HirStmtId(0),
                span: stmt_span,
                kind: HirStmtKind::Block(ids),
            });
            body.stmts[block_id].id = block_id;
            Some(block_id)
        }
    }
}

/// Lower an AST expression into an `HirExprId` in `body.exprs`.
///
/// Task 06-10: maps every [`rcc_ast::ExprKind`] variant to a
/// [`HirExprKind`] entry, resolving identifiers against `scope` +
/// `resolver.ordinary`. Types are left as placeholders (`tcx.error`)
/// and value categories as `RValue`; the typeck phase (phase 07)
/// fills in real types, lvalue/rvalue classification, and any
/// implicit `Convert` nodes.
///
/// ## Lowering rules
///
/// | AST shape                         | HIR shape                                            |
/// |-----------------------------------|------------------------------------------------------|
/// | `IntLit`                          | `IntLiteral`                                         |
/// | `FloatLit`                        | `FloatConst`                                         |
/// | `CharLit`                         | `IntConst` (first character's code point)            |
/// | `StringLit "s"`                   | `StringRef(def_id)` pointing at a synthesised global |
/// | `Ident`                           | `LocalRef` / `DefRef` via [`resolve_expr_ident`]     |
/// | `Paren(e)`                        | no-op: the inner expression's id is returned          |
/// | `Binary { op, .. }`               | `Binary { op, .. }`                                  |
/// | `Unary { Plus/Neg/Bit/Log/… }`    | `Unary { op, .. }`                                   |
/// | `Unary { AddrOf }`                | `AddressOf`                                          |
/// | `Unary { Deref }`                 | `Deref`                                              |
/// | `Unary { Pre/PostInc/Dec }`       | `Unary { PreInc/PreDec/PostInc/PostDec }`            |
/// | `Cond`                            | `Cond`                                               |
/// | `Assign { =, .. }`                | `Assign { lhs, rhs }`                                |
/// | `Assign { += / -= / …, .. }`      | `Assign { lhs, Binary { op, lhs, rhs } }` (desugared)|
/// | `Comma`                           | `Comma`                                              |
/// | `Call`                            | `Call`                                               |
/// | `Member { a.b }`                  | `UnresolvedField { base, field: b }`                 |
/// | `Arrow  { a->b }`                 | `UnresolvedField { base: Deref(a), field: b }`       |
/// | `Index`                           | `Index`                                              |
/// | `Cast`                            | `Cast { operand, to }`                               |
/// | `SizeofExpr`                      | `SizeofExpr` (typed / folded in typeck + CFG)        |
/// | `SizeofType`                      | `SizeofType(ty)`                                     |
/// | `CompoundLiteral`                 | synthetic local + initializer statements             |
///
/// The returned id is always the id of the last node pushed for this
/// expression, so `body.exprs[id].span == expr.span` except for the
/// `Paren` passthrough, where the inner expression's span is kept
/// verbatim.
///
/// String literal interning: each distinct `StringLit` creates a
/// `DefKind::Global` with `linkage: Internal` and an
/// `array-of-char` type sized to the raw byte count (quotes stripped,
/// escape-decoded character count + 1 for the trailing NUL). The
/// generated `DefId` is memoised in `resolver.strings` so that
/// repeated identical literals reuse the same global.
#[allow(clippy::too_many_arguments)]
pub fn lower_expr(
    expr: &rcc_ast::Expr,
    body: &mut Body,
    scope: &ScopeStack,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> HirExprId {
    let mut initial_ty = tcx.error;
    let kind: HirExprKind = match &expr.kind {
        rcc_ast::ExprKind::IntLit(lit) => HirExprKind::IntLiteral {
            value: i128::try_from(lit.value).unwrap_or(i128::MAX),
            base: hir_int_base(lit.base),
            suffix: hir_int_suffix(lit.suffix),
        },
        rcc_ast::ExprKind::FloatLit(lit) => {
            let component_ty = hir_float_literal_ty(lit.suffix, tcx);
            if lit.imaginary {
                let real = body.exprs.push(HirExpr {
                    id: HirExprId(0),
                    ty: component_ty,
                    value_cat: ValueCat::RValue,
                    span: expr.span,
                    kind: HirExprKind::FloatConst(0.0),
                });
                body.exprs[real].id = real;
                let imag = body.exprs.push(HirExpr {
                    id: HirExprId(0),
                    ty: component_ty,
                    value_cat: ValueCat::RValue,
                    span: expr.span,
                    kind: HirExprKind::FloatConst(lit.value),
                });
                body.exprs[imag].id = imag;
                HirExprKind::BuiltinComplex { real, imag }
            } else {
                initial_ty = component_ty;
                HirExprKind::FloatConst(lit.value)
            }
        }
        rcc_ast::ExprKind::CharLit(lit) => {
            initial_ty = char_literal_ty(lit.encoding, tcx);
            HirExprKind::IntConst(i128::from(lit.value))
        }
        rcc_ast::ExprKind::StringLit(lit) => {
            let def_id = intern_string_literal(lit, expr.span, crate_, tcx, resolver);
            HirExprKind::StringRef(def_id)
        }
        rcc_ast::ExprKind::Paren(inner) => {
            // Reuse the wrapped expression's id so the node count in
            // HIR matches the number of AST nodes with semantic shape.
            // Parentheses are a grouping syntax — not a value shape.
            return lower_expr(inner, body, scope, crate_, tcx, resolver, session);
        }
        rcc_ast::ExprKind::Ident(sym) => {
            if let Some(kind) = lower_va_area(*sym, expr.span, crate_, resolver, session) {
                kind
            } else if let Some(kind) =
                lower_predefined_function_name(*sym, expr.span, crate_, tcx, resolver, session)
            {
                kind
            } else {
                resolve_expr_ident(*sym, expr.span, scope, resolver, session)
                    .unwrap_or(HirExprKind::IntConst(0))
            }
        }
        rcc_ast::ExprKind::Binary { op, lhs, rhs } => {
            let lhs_id = lower_expr(lhs, body, scope, crate_, tcx, resolver, session);
            let rhs_id = lower_expr(rhs, body, scope, crate_, tcx, resolver, session);
            HirExprKind::Binary { op: ast_binop_to_hir(*op), lhs: lhs_id, rhs: rhs_id }
        }
        rcc_ast::ExprKind::Unary { op, operand } => {
            let op_id = lower_expr(operand, body, scope, crate_, tcx, resolver, session);
            match ast_unop_to_hir(*op) {
                Some(hop) => HirExprKind::Unary { op: hop, operand: op_id },
                None => {
                    // `&` / `*` are modelled by dedicated HirExprKind
                    // variants, not by `UnOp`.
                    match op {
                        rcc_ast::UnOp::AddrOf => HirExprKind::AddressOf(op_id),
                        rcc_ast::UnOp::Deref => HirExprKind::Deref(op_id),
                        _ => unreachable!("ast_unop_to_hir only returns None for &/ *"),
                    }
                }
            }
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            let c = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            let t = lower_expr(then_expr, body, scope, crate_, tcx, resolver, session);
            let e = lower_expr(else_expr, body, scope, crate_, tcx, resolver, session);
            HirExprKind::Cond { cond: c, then_expr: t, else_expr: e }
        }
        rcc_ast::ExprKind::GenericSelection { control, associations } => {
            let control = lower_expr(control, body, scope, crate_, tcx, resolver, session);
            let associations = associations
                .iter()
                .map(|assoc| {
                    let ty = assoc.ty.as_ref().map(|ty| {
                        lower_type_name_in_scope(
                            ty,
                            DeclScope::Block,
                            Some(scope),
                            tcx,
                            resolver,
                            crate_,
                            session,
                        )
                    });
                    let expr = lower_expr(&assoc.expr, body, scope, crate_, tcx, resolver, session);
                    GenericAssociation { ty, expr }
                })
                .collect();
            HirExprKind::GenericSelection { control, associations, selected: None }
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            let c = lower_expr(cond, body, scope, crate_, tcx, resolver, session);
            let e = lower_expr(else_expr, body, scope, crate_, tcx, resolver, session);
            HirExprKind::OmittedCond { cond: c, else_expr: e }
        }
        rcc_ast::ExprKind::LabelAddr(name) => HirExprKind::LabelAddr(*name),
        rcc_ast::ExprKind::Assign { op, lhs, rhs } => {
            let l = lower_expr(lhs, body, scope, crate_, tcx, resolver, session);
            let r = lower_expr(rhs, body, scope, crate_, tcx, resolver, session);
            match assign_op_to_binop(*op) {
                // Simple `a = b`.
                None => HirExprKind::Assign { lhs: l, rhs: r },
                // Compound `a op= b` desugars to `a = a op b`. We push
                // a fresh `Binary` node so the HIR carries the
                // arithmetic explicitly; the LHS is referenced twice
                // by id (there is no side-effect duplication because
                // `lower_expr` already evaluated it once into `l`).
                Some(binop) => {
                    let rhs_expr = HirExpr {
                        id: HirExprId(0),
                        ty: tcx.error,
                        value_cat: ValueCat::RValue,
                        span: expr.span,
                        kind: HirExprKind::Binary { op: binop, lhs: l, rhs: r },
                    };
                    let binop_id = body.exprs.push(rhs_expr);
                    body.exprs[binop_id].id = binop_id;
                    HirExprKind::Assign { lhs: l, rhs: binop_id }
                }
            }
        }
        rcc_ast::ExprKind::Comma { lhs, rhs } => {
            let l = lower_expr(lhs, body, scope, crate_, tcx, resolver, session);
            let r = lower_expr(rhs, body, scope, crate_, tcx, resolver, session);
            HirExprKind::Comma { lhs: l, rhs: r }
        }
        rcc_ast::ExprKind::Call { callee, args } => {
            // Intercept __builtin_va_start/end/copy calls before callee
            // resolution, since these are not declared functions.
            if let rcc_ast::ExprKind::Ident(sym) = &callee.kind {
                let name = session.interner.get(*sym).to_owned();
                match name.as_str() {
                    "__builtin_va_start" => {
                        if args.len() != 2 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_va_start` requires exactly 2 arguments",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let ap_id =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let last_id =
                            lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.void,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinVaStart { ap: ap_id, last_param: last_id },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_va_end" => {
                        if args.len() != 1 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_va_end` requires exactly 1 argument",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let ap_id =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.void,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinVaEnd { ap: ap_id },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_va_copy" => {
                        if args.len() != 2 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_va_copy` requires exactly 2 arguments",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let dst_id =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let src_id =
                            lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.void,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinVaCopy { dst: dst_id, src: src_id },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_expect" => {
                        if args.len() != 2 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_expect` requires exactly 2 arguments",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let value_id =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let expected_id =
                            lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.error,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinExpect {
                                value: value_id,
                                expected: expected_id,
                            },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_unreachable" => {
                        if args.is_empty() {
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.void,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::BuiltinUnreachable,
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        session
                            .handler
                            .struct_err(
                                expr.span,
                                "`__builtin_unreachable` requires exactly 0 arguments",
                            )
                            .emit();
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.error,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::IntConst(0),
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_constant_p" => {
                        if args.len() != 1 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_constant_p` requires exactly 1 argument",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let inner =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.int,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinConstantP { expr: inner },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_bswap16" | "__builtin_bswap32" | "__builtin_bswap64" => {
                        let bits = match name.as_str() {
                            "__builtin_bswap16" => 16,
                            "__builtin_bswap32" => 32,
                            "__builtin_bswap64" => 64,
                            _ => unreachable!("filtered by match arm"),
                        };
                        if args.len() != 1 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    format!("{name} requires exactly 1 argument"),
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let value =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.error,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinBswap { bits, value },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_complex" => {
                        if args.len() != 2 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    "`__builtin_complex` requires exactly 2 arguments",
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let real =
                            lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let imag =
                            lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.error,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinComplex { real, imag },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    name if name.starts_with("__builtin_tgmath_") => {
                        let family =
                            name.strip_prefix("__builtin_tgmath_").expect("prefix checked above");
                        let family = session.interner.intern(family);
                        let args = args
                            .iter()
                            .map(|arg| lower_expr(arg, body, scope, crate_, tcx, resolver, session))
                            .collect();
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.error,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinTgmath { name: family, args },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_add_overflow" | "__builtin_mul_overflow"
                        if session.opts.gnu_builtin_libcalls =>
                    {
                        let op = if name == "__builtin_add_overflow" {
                            OverflowOp::Add
                        } else {
                            OverflowOp::Mul
                        };
                        if args.len() != 3 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    format!("{name} requires exactly 3 arguments"),
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let lhs = lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let rhs = lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let dst = lower_expr(&args[2], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.int,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinOverflow {
                                op,
                                lhs,
                                rhs,
                                dst,
                                result_ty: tcx.error,
                            },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    "__builtin_add_overflow_p" | "__builtin_mul_overflow_p"
                        if session.opts.gnu_builtin_libcalls =>
                    {
                        let op = if name == "__builtin_add_overflow_p" {
                            OverflowOp::Add
                        } else {
                            OverflowOp::Mul
                        };
                        if args.len() != 3 {
                            session
                                .handler
                                .struct_err(
                                    expr.span,
                                    format!("{name} requires exactly 3 arguments"),
                                )
                                .emit();
                            let id = body.exprs.push(HirExpr {
                                id: HirExprId(0),
                                ty: tcx.error,
                                value_cat: ValueCat::RValue,
                                span: expr.span,
                                kind: HirExprKind::IntConst(0),
                            });
                            body.exprs[id].id = id;
                            return id;
                        }
                        let lhs = lower_expr(&args[0], body, scope, crate_, tcx, resolver, session);
                        let rhs = lower_expr(&args[1], body, scope, crate_, tcx, resolver, session);
                        let probe =
                            lower_expr(&args[2], body, scope, crate_, tcx, resolver, session);
                        let id = body.exprs.push(HirExpr {
                            id: HirExprId(0),
                            ty: tcx.int,
                            value_cat: ValueCat::RValue,
                            span: expr.span,
                            kind: HirExprKind::BuiltinOverflowP {
                                op,
                                lhs,
                                rhs,
                                probe,
                                result_ty: tcx.error,
                            },
                        });
                        body.exprs[id].id = id;
                        return id;
                    }
                    _ => {}
                }
            }
            let implicit_callee = match &callee.kind {
                rcc_ast::ExprKind::Ident(sym) => lower_implicit_function_callee(
                    *sym,
                    callee.span,
                    scope,
                    crate_,
                    tcx,
                    resolver,
                    session,
                ),
                _ => None,
            };
            if let Some(kind) = implicit_callee {
                let callee_id = body.exprs.push(HirExpr {
                    id: HirExprId(0),
                    ty: tcx.error,
                    value_cat: ValueCat::RValue,
                    span: callee.span,
                    kind,
                });
                body.exprs[callee_id].id = callee_id;
                let arg_ids: Vec<HirExprId> = args
                    .iter()
                    .map(|a| lower_expr(a, body, scope, crate_, tcx, resolver, session))
                    .collect();
                HirExprKind::Call { callee: callee_id, args: arg_ids }
            } else {
                let callee_id = lower_expr(callee, body, scope, crate_, tcx, resolver, session);
                let arg_ids: Vec<HirExprId> = args
                    .iter()
                    .map(|a| lower_expr(a, body, scope, crate_, tcx, resolver, session))
                    .collect();
                HirExprKind::Call { callee: callee_id, args: arg_ids }
            }
        }
        rcc_ast::ExprKind::BuiltinVaArg { ap, ty } => {
            let ap_id = lower_expr(ap, body, scope, crate_, tcx, resolver, session);
            let ty_id = lower_type_name_in_scope(
                ty,
                DeclScope::Block,
                Some(scope),
                tcx,
                resolver,
                crate_,
                session,
            );
            HirExprKind::BuiltinVaArg { ap: ap_id, ty: ty_id }
        }
        rcc_ast::ExprKind::BuiltinOffsetof { ty, designators } => {
            let value =
                lower_builtin_offsetof(ty, designators, expr.span, crate_, tcx, resolver, session);
            HirExprKind::IntConst(i128::from(value))
        }
        rcc_ast::ExprKind::BuiltinTypesCompatible { lhs, rhs } => {
            let lhs_ty = lower_type_name(lhs, DeclScope::Block, tcx, resolver, crate_, session);
            let rhs_ty = lower_type_name(rhs, DeclScope::Block, tcx, resolver, crate_, session);
            HirExprKind::IntConst(if lhs_ty == rhs_ty { 1 } else { 0 })
        }
        rcc_ast::ExprKind::StmtExpr(block) => {
            let (stmts, result) =
                lower_stmt_expr_block(block, body, scope, crate_, tcx, resolver, session);
            HirExprKind::StmtExpr { stmts, result }
        }
        rcc_ast::ExprKind::Member { base, field } => {
            let base_id = lower_expr(base, body, scope, crate_, tcx, resolver, session);
            // Field-index resolution happens in typeck; keep the source
            // member name here so the resolver can choose the correct field.
            // The AST currently stores only the whole member expression span,
            // so this is the best available member-token span.
            HirExprKind::UnresolvedField { base: base_id, field: *field, field_span: expr.span }
        }
        rcc_ast::ExprKind::Arrow { base, field } => {
            // `a->b` lowers to `(*a).b`. Emit the Deref as its own
            // HIR node so the indirection is explicit.
            let base_id = lower_expr(base, body, scope, crate_, tcx, resolver, session);
            let deref_expr = HirExpr {
                id: HirExprId(0),
                ty: tcx.error,
                value_cat: ValueCat::LValue,
                span: base.span,
                kind: HirExprKind::Deref(base_id),
            };
            let deref_id = body.exprs.push(deref_expr);
            body.exprs[deref_id].id = deref_id;
            HirExprKind::UnresolvedField { base: deref_id, field: *field, field_span: expr.span }
        }
        rcc_ast::ExprKind::Index { base, index } => {
            let base_id = lower_expr(base, body, scope, crate_, tcx, resolver, session);
            let index_id = lower_expr(index, body, scope, crate_, tcx, resolver, session);
            HirExprKind::Index { base: base_id, index: index_id }
        }
        rcc_ast::ExprKind::Cast { ty, expr: inner } => {
            let to = lower_type_name_in_scope(
                ty,
                DeclScope::Block,
                Some(scope),
                tcx,
                resolver,
                crate_,
                session,
            );
            let inner_id = lower_expr(inner, body, scope, crate_, tcx, resolver, session);
            HirExprKind::Cast { operand: inner_id, to }
        }
        rcc_ast::ExprKind::SizeofExpr(inner) => {
            let inner_id = lower_expr(inner, body, scope, crate_, tcx, resolver, session);
            HirExprKind::SizeofExpr(inner_id)
        }
        rcc_ast::ExprKind::SizeofType(ty) => {
            let ty = lower_type_name_in_scope(
                ty,
                DeclScope::Block,
                Some(scope),
                tcx,
                resolver,
                crate_,
                session,
            );
            HirExprKind::SizeofType(ty)
        }
        rcc_ast::ExprKind::AlignofExpr(inner) => {
            let inner_id = lower_expr(inner, body, scope, crate_, tcx, resolver, session);
            HirExprKind::AlignofExpr(inner_id)
        }
        rcc_ast::ExprKind::AlignofType(ty) => {
            let ty = lower_type_name_in_scope(
                ty,
                DeclScope::Block,
                Some(scope),
                tcx,
                resolver,
                crate_,
                session,
            );
            HirExprKind::AlignofType(ty)
        }
        rcc_ast::ExprKind::CompoundLiteral { ty, init } => {
            let ty = lower_type_name_in_scope(
                ty,
                DeclScope::Block,
                Some(scope),
                tcx,
                resolver,
                crate_,
                session,
            );
            let local = body.locals.push(LocalDecl {
                name: None,
                ty,
                quals: ObjectQuals::none(),
                vla_len: None,
                is_param: false,
                span: expr.span,
            });
            let target = push_local_ref(local, ty, expr.span, body);
            let mut init_stmts = Vec::new();
            lower_initializer(
                target,
                ty,
                init,
                expr.span,
                body,
                scope,
                crate_,
                tcx,
                resolver,
                session,
                &mut init_stmts,
            );
            HirExprKind::CompoundLiteral { ty, local, init_stmts }
        }
    };

    let expr_id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: initial_ty,
        value_cat: ValueCat::RValue,
        span: expr.span,
        kind,
    });
    body.exprs[expr_id].id = expr_id;
    expr_id
}

fn lower_builtin_offsetof(
    ty: &rcc_ast::TypeName,
    designators: &[OffsetofDesignator],
    span: Span,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> u64 {
    let mut current_ty = lower_type_name(ty, DeclScope::Block, tcx, resolver, crate_, session);
    let layouts = LayoutCx::with_defs_for_target(tcx, &crate_.defs, session.opts.target.clone());
    let mut offset = 0_u64;

    for designator in designators {
        match designator {
            OffsetofDesignator::Field(name) => {
                let Ty::Record(def) = tcx.get(current_ty) else {
                    emit_offsetof_error(session, span, "offsetof field designator needs a record");
                    return 0;
                };
                let Ok(record_layout) = layouts.record_layout_of(current_ty) else {
                    emit_offsetof_error(session, span, "could not compute record layout");
                    return 0;
                };
                let DefKind::Record { fields, .. } = &crate_.defs[*def].kind else {
                    emit_offsetof_error(session, span, "offsetof record definition is malformed");
                    return 0;
                };
                let Some(field_idx) = fields.iter().position(|field| field.name == Some(*name))
                else {
                    let field = session.interner.get(*name);
                    emit_offsetof_error(session, span, format!("record has no field `{field}`"));
                    return 0;
                };
                offset = match offset.checked_add(record_layout.fields[field_idx].offset) {
                    Some(value) => value,
                    None => {
                        emit_offsetof_error(session, span, "offsetof computation overflowed");
                        return 0;
                    }
                };
                current_ty = fields[field_idx].ty;
            }
            OffsetofDesignator::Index(index) => {
                let idx = match eval_offsetof_index(index) {
                    Some(value) if value >= 0 => value as u64,
                    _ => {
                        emit_offsetof_error(
                            session,
                            span,
                            "offsetof array designator needs a non-negative integer constant",
                        );
                        return 0;
                    }
                };
                let Ty::Array { elem, .. } = tcx.get(current_ty).clone() else {
                    emit_offsetof_error(
                        session,
                        span,
                        "offsetof subscript designator needs an array",
                    );
                    return 0;
                };
                let Ok(elem_layout) = layouts.layout_of(elem.ty) else {
                    emit_offsetof_error(session, span, "could not compute array element layout");
                    return 0;
                };
                let Some(delta) = elem_layout.size.checked_mul(idx) else {
                    emit_offsetof_error(session, span, "offsetof array index overflowed");
                    return 0;
                };
                offset = match offset.checked_add(delta) {
                    Some(value) => value,
                    None => {
                        emit_offsetof_error(session, span, "offsetof computation overflowed");
                        return 0;
                    }
                };
                current_ty = elem.ty;
            }
        }
    }

    offset
}

fn emit_offsetof_error(session: &mut Session, span: Span, msg: impl Into<String>) {
    session.handler.struct_err(span, msg.into()).code(rcc_errors::codes::E0084).emit();
}

fn eval_offsetof_index(expr: &rcc_ast::Expr) -> Option<i128> {
    match &expr.kind {
        rcc_ast::ExprKind::IntLit(lit) => i128::try_from(lit.value).ok(),
        rcc_ast::ExprKind::CharLit(lit) => Some(i128::from(lit.value)),
        rcc_ast::ExprKind::Paren(inner) => eval_offsetof_index(inner),
        rcc_ast::ExprKind::Unary { op, operand } => {
            let value = eval_offsetof_index(operand)?;
            match op {
                rcc_ast::UnOp::Plus => Some(value),
                rcc_ast::UnOp::Neg => value.checked_neg(),
                _ => None,
            }
        }
        rcc_ast::ExprKind::Binary { op, lhs, rhs } => {
            let lhs = eval_offsetof_index(lhs)?;
            let rhs = eval_offsetof_index(rhs)?;
            match op {
                rcc_ast::BinOp::Add => lhs.checked_add(rhs),
                rcc_ast::BinOp::Sub => lhs.checked_sub(rhs),
                rcc_ast::BinOp::Mul => lhs.checked_mul(rhs),
                _ => None,
            }
        }
        _ => None,
    }
}

fn hir_int_base(base: rcc_ast::IntBase) -> IntLiteralBase {
    match base {
        rcc_ast::IntBase::Decimal => IntLiteralBase::Decimal,
        rcc_ast::IntBase::Octal => IntLiteralBase::Octal,
        rcc_ast::IntBase::Hex => IntLiteralBase::Hex,
        rcc_ast::IntBase::Binary => IntLiteralBase::Binary,
    }
}

fn hir_int_suffix(suffix: rcc_ast::IntSuffix) -> IntLiteralSuffix {
    match suffix {
        rcc_ast::IntSuffix::None => IntLiteralSuffix::None,
        rcc_ast::IntSuffix::U => IntLiteralSuffix::U,
        rcc_ast::IntSuffix::L => IntLiteralSuffix::L,
        rcc_ast::IntSuffix::UL => IntLiteralSuffix::UL,
        rcc_ast::IntSuffix::LL => IntLiteralSuffix::LL,
        rcc_ast::IntSuffix::ULL => IntLiteralSuffix::ULL,
    }
}

fn hir_float_literal_ty(suffix: rcc_ast::FloatSuffix, tcx: &TyCtxt) -> TyId {
    match suffix {
        rcc_ast::FloatSuffix::None => tcx.double,
        rcc_ast::FloatSuffix::F => tcx.float,
        rcc_ast::FloatSuffix::L => tcx.long_double,
    }
}

fn hir_float_literal_ty_for_ast(lit: &rcc_ast::FloatLiteral, tcx: &TyCtxt) -> TyId {
    let real = hir_float_literal_ty(lit.suffix, tcx);
    if !lit.imaginary {
        return real;
    }
    match lit.suffix {
        rcc_ast::FloatSuffix::F => tcx.complex_float,
        rcc_ast::FloatSuffix::None => tcx.complex_double,
        rcc_ast::FloatSuffix::L => tcx.complex_long_double,
    }
}

fn lower_predefined_function_name(
    ident: Symbol,
    span: Span,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    session: &mut Session,
) -> Option<HirExprKind> {
    let spelling = session.interner.get(ident).to_owned();
    let is_gnu_alias = match spelling.as_str() {
        "__func__" => false,
        "__FUNCTION__" => true,
        _ => return None,
    };
    let function = resolver.current_function?;

    if is_gnu_alias && !session.opts.gnu_function_names {
        session
            .handler
            .struct_warn(span, "GNU `__FUNCTION__` is not part of C99")
            .code(rcc_errors::codes::W0022)
            .note("lowering it as an alias for C99 `__func__`")
            .emit();
    }

    let fn_name = session.interner.get(crate_.defs[function].name).to_owned();
    let text = session.interner.intern(&format!("\"{fn_name}\""));
    let lit = rcc_ast::StringLiteral {
        text,
        bytes: fn_name.into_bytes(),
        encoding: rcc_ast::LiteralEncoding::None,
    };
    let def_id = intern_string_literal(&lit, span, crate_, tcx, resolver);
    Some(HirExprKind::StringRef(def_id))
}

fn lower_va_area(
    ident: Symbol,
    span: Span,
    crate_: &HirCrate,
    resolver: &Resolver,
    session: &mut Session,
) -> Option<HirExprKind> {
    if session.interner.get(ident) != "__va_area__" {
        return None;
    }

    let Some(function) = resolver.current_function else {
        invalid_va_area_use(
            span,
            session,
            "`__va_area__` is only available inside variadic functions",
        );
        return Some(HirExprKind::IntConst(0));
    };
    let variadic = matches!(crate_.defs[function].kind, DefKind::Function { variadic: true, .. });
    if !variadic {
        invalid_va_area_use(
            span,
            session,
            "`__va_area__` is only available inside variadic functions",
        );
        return Some(HirExprKind::IntConst(0));
    }

    if !session.opts.gnu_va_area {
        session
            .handler
            .struct_warn(span, "GNU `__va_area__` is not part of C99")
            .code(rcc_errors::codes::W0023)
            .note("lowering it as a pointer to the current function's varargs save area")
            .emit();
    }

    Some(HirExprKind::BuiltinVaArea)
}

fn invalid_va_area_use(span: Span, session: &mut Session, msg: &str) {
    session.handler.struct_err(span, msg).code(rcc_errors::codes::E0071).emit();
}

/// Intern a string literal into the global table.
///
/// Looks up the (already deduplicated) `Symbol` of the literal's
/// source text in `resolver.strings`. If present, returns the cached
/// `DefId`. Otherwise creates a new `DefKind::Global` with
/// `linkage: Internal` and an array type whose element type follows the
/// string-literal encoding and whose length includes the trailing NUL required
/// by C99/C11 §6.4.5p6.
///
/// The type is interned via `TyCtxt::intern` so string literals with identical
/// text share the same type id.
fn intern_string_literal(
    lit: &rcc_ast::StringLiteral,
    span: Span,
    crate_: &mut HirCrate,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
) -> DefId {
    let text = lit.text;
    if let Some(&existing) = resolver.strings.get(&text) {
        return existing;
    }

    let elements = string_literal_elements(lit);
    let len = elements.len() + 1; // +1 for NUL

    let elem_ty = string_literal_element_ty(lit, tcx);
    let array_ty =
        tcx.intern(Ty::Array { elem: Qual::plain(elem_ty), len: Some(len as u64), is_vla: false });

    // Build element-level GlobalInit entries from the already-decoded parser
    // payload. This avoids re-parsing source spelling in codegen, which would
    // break adjacent string concatenation (`"a" "b"` → single `ab` payload).
    let mut entries = Vec::with_capacity(len);
    for (i, value) in elements.into_iter().enumerate() {
        entries.push(GlobalInitEntry {
            path: vec![GlobalInitDesignator::Index(i as u64)],
            ty: elem_ty,
            expr: None,
            value: GlobalInitValue::Int(value),
            span,
        });
    }
    // Trailing NUL.
    entries.push(GlobalInitEntry {
        path: vec![GlobalInitDesignator::Index((len - 1) as u64)],
        ty: elem_ty,
        expr: None,
        value: GlobalInitValue::Int(0),
        span,
    });
    let init = GlobalInit { ty: array_ty, entries };

    // Create a synthetic anonymous global for this literal. The
    // name is the interned string itself so diagnostics can surface
    // something sensible; codegen emits it as an internal-linkage
    // constant (C99 §6.4.5p5 "static storage duration").
    let def_id = crate_.defs.push(Def {
        id: DefId(0),
        name: text,
        span,
        kind: DefKind::Global {
            ty: array_ty,
            quals: ObjectQuals::none(),
            thread_local: false,
            linkage: Linkage::Internal,
            init: Some(init),
        },
    });
    crate_.defs[def_id].id = def_id;
    resolver.strings.insert(text, def_id);
    def_id
}

/// Strip the surrounding quotes and any encoding prefix (`L`, `u`,
/// `U`, `u8`) from a C string literal's source text. Returns the
/// slice between the quotes or `""` if the input is malformed.
#[cfg(test)]
fn strip_string_literal_quotes(s: &str) -> &str {
    // Drop any encoding prefix before the opening quote.
    let after_prefix = s.strip_prefix("u8").or_else(|| s.strip_prefix('u')).unwrap_or(s);
    let after_prefix = after_prefix
        .strip_prefix('L')
        .or_else(|| after_prefix.strip_prefix('U'))
        .unwrap_or(after_prefix);
    // Remove the surrounding `"..."`.
    let inner = after_prefix.strip_prefix('"').unwrap_or(after_prefix);
    inner.strip_suffix('"').unwrap_or(inner)
}

#[cfg(test)]
fn decode_string_literal_values(content: &str) -> Vec<i128> {
    let bytes = content.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            out.push(bytes[i] as i128);
            i += 1;
            continue;
        }

        let next = bytes[i + 1];
        match next {
            b'n' => {
                out.push(10);
                i += 2;
            }
            b't' => {
                out.push(9);
                i += 2;
            }
            b'r' => {
                out.push(13);
                i += 2;
            }
            b'\\' => {
                out.push(b'\\' as i128);
                i += 2;
            }
            b'"' => {
                out.push(b'"' as i128);
                i += 2;
            }
            b'\'' => {
                out.push(b'\'' as i128);
                i += 2;
            }
            b'a' => {
                out.push(7);
                i += 2;
            }
            b'b' => {
                out.push(8);
                i += 2;
            }
            b'f' => {
                out.push(12);
                i += 2;
            }
            b'v' => {
                out.push(11);
                i += 2;
            }
            b'?' => {
                out.push(b'?' as i128);
                i += 2;
            }
            b'x' | b'X' => {
                let mut val = 0i128;
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    let digit = (bytes[i] as char).to_digit(16).unwrap_or(0) as i128;
                    val = (val << 4) | digit;
                    i += 1;
                }
                out.push(val);
            }
            d if d.is_ascii_digit() && d < b'8' => {
                let mut val = (d - b'0') as i128;
                i += 2;
                let mut consumed = 1;
                while consumed < 3
                    && i < bytes.len()
                    && bytes[i].is_ascii_digit()
                    && bytes[i] < b'8'
                {
                    val = (val << 3) | (bytes[i] - b'0') as i128;
                    i += 1;
                    consumed += 1;
                }
                out.push(val);
            }
            _ => {
                out.push(next as i128);
                i += 2;
            }
        }
    }
    out
}

/// Decode the numeric value of the first character in a C char
/// constant's source text (e.g. `'a'` → 97, `'\n'` → 10, `'\x41'`
/// → 65). Ignores multi-character constants beyond the first char.
///
/// Returns `None` if the input does not look like a char literal;
/// the caller then substitutes 0 so later passes keep working.
#[cfg(test)]
fn decode_first_char_value(s: &str) -> Option<i32> {
    // Drop any encoding prefix.
    let after_prefix = s
        .strip_prefix('L')
        .or_else(|| s.strip_prefix('u'))
        .or_else(|| s.strip_prefix('U'))
        .unwrap_or(s);
    let inner = after_prefix.strip_prefix('\'')?;
    let bytes = inner.as_bytes();
    if bytes.is_empty() {
        return Some(0);
    }
    if bytes[0] != b'\\' {
        return Some(bytes[0] as i32);
    }
    // Escape sequence.
    let next = *bytes.get(1)?;
    Some(match next {
        b'n' => 10,
        b't' => 9,
        b'r' => 13,
        b'0' => 0,
        b'\\' => b'\\' as i32,
        b'\'' => b'\'' as i32,
        b'"' => b'"' as i32,
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'v' => 11,
        b'?' => b'?' as i32,
        b'x' | b'X' => {
            // Hex escape: read up to 2 hex digits for a char.
            let mut val: i32 = 0;
            let mut j = 2;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() && j - 2 < 2 {
                let d = (bytes[j] as char).to_digit(16)? as i32;
                val = (val << 4) | d;
                j += 1;
            }
            val
        }
        d if d.is_ascii_digit() => {
            // Octal escape: up to 3 octal digits.
            let mut val: i32 = (d - b'0') as i32;
            let mut j = 2;
            while j < bytes.len() && bytes[j].is_ascii_digit() && bytes[j] < b'8' && j - 1 < 3 {
                val = (val << 3) | (bytes[j] - b'0') as i32;
                j += 1;
            }
            val
        }
        _ => next as i32,
    })
}

/// Map an AST compound-assignment op to its arithmetic binary
/// operator. Returns `None` for `=` (plain assignment, no desugar).
fn assign_op_to_binop(op: rcc_ast::AssignOp) -> Option<rcc_hir::rcc_hir_binop::BinOp> {
    use rcc_ast::AssignOp as A;
    use rcc_hir::rcc_hir_binop::BinOp as H;
    Some(match op {
        A::Eq => return None,
        A::AddEq => H::Add,
        A::SubEq => H::Sub,
        A::MulEq => H::Mul,
        A::DivEq => H::Div,
        A::RemEq => H::Rem,
        A::ShlEq => H::Shl,
        A::ShrEq => H::Shr,
        A::AndEq => H::BitAnd,
        A::XorEq => H::BitXor,
        A::OrEq => H::BitOr,
    })
}

/// Translate an [`rcc_ast::BinOp`] to the matching [`rcc_hir_binop::BinOp`].
fn ast_binop_to_hir(op: rcc_ast::BinOp) -> rcc_hir::rcc_hir_binop::BinOp {
    use rcc_ast::BinOp as A;
    use rcc_hir::rcc_hir_binop::BinOp as H;
    match op {
        A::Add => H::Add,
        A::Sub => H::Sub,
        A::Mul => H::Mul,
        A::Div => H::Div,
        A::Rem => H::Rem,
        A::Shl => H::Shl,
        A::Shr => H::Shr,
        A::Lt => H::Lt,
        A::Le => H::Le,
        A::Gt => H::Gt,
        A::Ge => H::Ge,
        A::Eq => H::Eq,
        A::Ne => H::Ne,
        A::BitAnd => H::BitAnd,
        A::BitXor => H::BitXor,
        A::BitOr => H::BitOr,
        A::LogAnd => H::LogAnd,
        A::LogOr => H::LogOr,
    }
}

/// Translate an [`rcc_ast::UnOp`] to the matching [`rcc_hir_binop::UnOp`].
///
/// Returns `None` for `&` and `*`, which are represented as dedicated
/// [`HirExprKind::AddressOf`] / [`HirExprKind::Deref`] variants rather
/// than as `UnOp` cases.
fn ast_unop_to_hir(op: rcc_ast::UnOp) -> Option<rcc_hir::rcc_hir_binop::UnOp> {
    use rcc_ast::UnOp as A;
    use rcc_hir::rcc_hir_binop::UnOp as H;
    Some(match op {
        A::Plus => H::Plus,
        A::Neg => H::Neg,
        A::BitNot => H::BitNot,
        A::LogNot => H::LogNot,
        A::PreInc => H::PreInc,
        A::PreDec => H::PreDec,
        A::PostInc => H::PostInc,
        A::PostDec => H::PostDec,
        A::AddrOf | A::Deref => return None,
    })
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

fn emit_duplicate_ordinary(name: Symbol, span: Span, session: &mut Session) {
    let sym_str = session.interner.get(name);
    session
        .handler
        .struct_err(span, format!("redeclaration of `{sym_str}` in the same scope"))
        .code(rcc_errors::codes::E0070)
        .emit();
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
    lower_typedef_name_in_scope(sym, span, expanding, None, resolver, crate_, tcx, session)
}

#[allow(clippy::too_many_arguments)]
fn lower_typedef_name_in_scope(
    sym: Symbol,
    span: Span,
    expanding: &mut FxHashSet<DefId>,
    scope: Option<&ScopeStack>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    session: &mut Session,
) -> TyId {
    // Look up the symbol in the ordinary namespace.
    let def_id = match scope.and_then(|s| s.lookup(sym)) {
        Some(Binding::Def(id)) => id,
        Some(Binding::Local(_)) => {
            let sym_str = session.interner.get(sym);
            session
                .handler
                .struct_err(span, format!("`{sym_str}` is not a typedef"))
                .code(rcc_errors::codes::E0071)
                .emit();
            return tcx.error;
        }
        None => match resolver.ordinary.get(&sym) {
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
        },
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

#[allow(clippy::too_many_arguments)]
fn lower_typeof_expr_to_ty(
    expr: &rcc_ast::Expr,
    scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> TyId {
    let expr = peel_ast_parens(expr);
    match &expr.kind {
        rcc_ast::ExprKind::Ident(sym) => lower_typeof_ident_to_ty(
            *sym, expr.span, scope, local_body, resolver, crate_, tcx, session,
        ),
        rcc_ast::ExprKind::IntLit(lit) => match lit.suffix {
            rcc_ast::IntSuffix::None => tcx.int,
            rcc_ast::IntSuffix::U => tcx.uint,
            rcc_ast::IntSuffix::L => tcx.long,
            rcc_ast::IntSuffix::UL => tcx.ulong,
            rcc_ast::IntSuffix::LL => tcx.long_long,
            rcc_ast::IntSuffix::ULL => tcx.ulong_long,
        },
        rcc_ast::ExprKind::FloatLit(lit) => hir_float_literal_ty_for_ast(lit, tcx),
        rcc_ast::ExprKind::CharLit(lit) => char_literal_ty(lit.encoding, tcx),
        rcc_ast::ExprKind::StringLit(lit) => {
            let elem = string_literal_element_ty(lit, tcx);
            let len = string_literal_elements(lit).len().saturating_add(1) as u64;
            tcx.intern(Ty::Array { elem: Qual::plain(elem), len: Some(len), is_vla: false })
        }
        rcc_ast::ExprKind::Index { base, .. } => {
            let base_ty =
                lower_typeof_expr_to_ty(base, scope, local_body, resolver, crate_, tcx, session);
            match tcx.get(base_ty) {
                Ty::Array { elem, .. } => elem.ty,
                Ty::Ptr(q) => q.ty,
                _ => {
                    session
                        .handler
                        .struct_err(expr.span, "subscripted expression is not an array or pointer")
                        .code(rcc_errors::codes::E0061)
                        .emit();
                    tcx.error
                }
            }
        }
        rcc_ast::ExprKind::SizeofExpr(_)
        | rcc_ast::ExprKind::SizeofType(_)
        | rcc_ast::ExprKind::AlignofExpr(_)
        | rcc_ast::ExprKind::AlignofType(_) => tcx.ulong,
        _ => {
            session
                .handler
                .struct_err(
                    expr.span,
                    "unsupported GNU `typeof` expression; only identifiers and literals are currently lowered",
                )
                .code(rcc_errors::codes::E0061)
                .emit();
            tcx.error
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_sizeof_operand_to_ty(
    expr: &rcc_ast::Expr,
    scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> TyId {
    let expr = peel_ast_parens(expr);
    if let Some(ty) =
        lower_sizeof_expr_operand_to_ty(expr, scope, local_body, resolver, crate_, tcx, session)
    {
        return ty;
    }
    lower_typeof_expr_to_ty(expr, scope, local_body, resolver, crate_, tcx, session)
}

#[allow(clippy::too_many_arguments)]
fn lower_sizeof_expr_operand_to_ty(
    expr: &rcc_ast::Expr,
    scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    session: &mut Session,
) -> Option<TyId> {
    let expr = peel_ast_parens(expr);
    match &expr.kind {
        rcc_ast::ExprKind::Ident(sym) => {
            if let Some(Binding::Local(local)) = scope.and_then(|scope| scope.lookup(*sym)) {
                return local_body.map(|body| body.locals[local].ty);
            }
            Some(lower_typeof_ident_to_ty(
                *sym, expr.span, scope, local_body, resolver, crate_, tcx, session,
            ))
        }
        rcc_ast::ExprKind::Member { base, field } => {
            let base_ty = lower_sizeof_expr_operand_to_ty(
                base, scope, local_body, resolver, crate_, tcx, session,
            )?;
            record_field_ty(base_ty, *field, crate_, tcx)
        }
        rcc_ast::ExprKind::Arrow { base, field } => {
            let base_ty = lower_sizeof_expr_operand_to_ty(
                base, scope, local_body, resolver, crate_, tcx, session,
            )?;
            match tcx.get(base_ty) {
                Ty::Ptr(q) => record_field_ty(q.ty, *field, crate_, tcx),
                _ => None,
            }
        }
        rcc_ast::ExprKind::Index { base, .. } => {
            let base_ty = lower_sizeof_expr_operand_to_ty(
                base, scope, local_body, resolver, crate_, tcx, session,
            )?;
            match tcx.get(base_ty) {
                Ty::Array { elem, .. } => Some(elem.ty),
                Ty::Ptr(q) => Some(q.ty),
                _ => None,
            }
        }
        _ => None,
    }
}

fn record_field_ty(
    record_ty: TyId,
    field: Symbol,
    crate_: &HirCrate,
    tcx: &TyCtxt,
) -> Option<TyId> {
    let Ty::Record(record) = tcx.get(record_ty) else {
        return None;
    };
    let DefKind::Record { fields, .. } = &crate_.defs[*record].kind else {
        return None;
    };
    fields.iter().find(|f| f.name == Some(field)).map(|f| f.ty)
}

fn peel_ast_parens(mut expr: &rcc_ast::Expr) -> &rcc_ast::Expr {
    while let rcc_ast::ExprKind::Paren(inner) = &expr.kind {
        expr = inner;
    }
    expr
}

#[allow(clippy::too_many_arguments)]
fn lower_typeof_ident_to_ty(
    sym: Symbol,
    span: Span,
    scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    resolver: &Resolver,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    session: &mut Session,
) -> TyId {
    if let Some(binding) = scope.and_then(|scope| scope.lookup(sym)) {
        return match binding {
            Binding::Def(def) => def_type_for_typeof(def, span, crate_, tcx, session),
            Binding::Local(local) => local_body
                .and_then(|body| body.locals.get(local).map(|decl| decl.ty))
                .unwrap_or_else(|| {
                    let name = session.interner.get(sym);
                    session
                        .handler
                        .struct_err(
                            span,
                            format!(
                                "cannot determine type of local identifier `{name}` for GNU `typeof`"
                            ),
                        )
                        .code(rcc_errors::codes::E0061)
                        .emit();
                    tcx.error
                }),
        };
    }

    if let Some(&def) = resolver.ordinary.get(&sym) {
        return def_type_for_typeof(def, span, crate_, tcx, session);
    }

    let name = session.interner.get(sym);
    session
        .handler
        .struct_err(span, format!("use of undeclared identifier `{name}` in GNU `typeof`"))
        .code(rcc_errors::codes::E0071)
        .emit();
    tcx.error
}

fn def_type_for_typeof(
    def: DefId,
    span: Span,
    crate_: &HirCrate,
    tcx: &TyCtxt,
    session: &mut Session,
) -> TyId {
    let ty = match &crate_.defs[def].kind {
        DefKind::Function { ty, .. }
        | DefKind::Global { ty, .. }
        | DefKind::Typedef(ty)
        | DefKind::Enumerator { ty, .. } => *ty,
        DefKind::Record { .. } | DefKind::Enum { .. } => tcx.error,
    };
    if ty == tcx.error {
        let name = session.interner.get(crate_.defs[def].name);
        session
            .handler
            .struct_err(span, format!("cannot determine type of `{name}` for GNU `typeof`"))
            .code(rcc_errors::codes::E0061)
            .emit();
    }
    ty
}

/// Lower declaration specifiers plus a declarator into a complete HIR type.
///
/// This is the canonical type-construction entry point for HIR lowering.
/// It first resolves the declaration-specifier base type (builtin scalar,
/// typedef, record, enum, or complex type), then folds the declarator chain
/// over that base. Every source declaration path should route through this
/// helper instead of open-coding specifier resolution.
#[allow(clippy::too_many_arguments)]
pub fn lower_type_from_parts(
    specs: &rcc_ast::DeclSpecs,
    declarator: &Declarator,
    scope: DeclScope,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    lower_type_from_parts_in_scope(
        specs, declarator, scope, None, None, tcx, resolver, crate_, session,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_type_from_parts_in_scope(
    specs: &rcc_ast::DeclSpecs,
    declarator: &Declarator,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    let base = lower_specs_to_base_ty_in_scope(
        specs,
        typedef_scope,
        local_body,
        tcx,
        resolver,
        crate_,
        session,
    );
    if base == tcx.error {
        return tcx.error;
    }
    apply_aligned_attr_override_to_record(base, &specs.attrs, tcx, crate_, session);
    apply_packed_attr_to_record(base, &specs.attrs, tcx, crate_, session);
    apply_scalar_storage_order_attr_to_record(base, &specs.attrs, tcx, crate_, session);
    let base = lower_atomic_qual(base, &specs.quals, tcx);
    let mut ty = apply_declarator_with_base_quals_in_scope(
        base,
        strip_atomic_qual(specs.quals),
        declarator,
        scope,
        typedef_scope,
        local_body,
        tcx,
        resolver,
        crate_,
        session,
        specs.storage == Some(StorageClass::Typedef),
    );
    ty = apply_vector_size_attrs_to_type(ty, &specs.attrs, tcx, session);
    ty = apply_vector_size_attrs_to_type(ty, &declarator.attrs, tcx, session);
    apply_aligned_attr_override_to_record(ty, &declarator.attrs, tcx, crate_, session);
    apply_packed_attr_to_record(ty, &declarator.attrs, tcx, crate_, session);
    apply_scalar_storage_order_attr_to_record(ty, &declarator.attrs, tcx, crate_, session);
    ty
}

fn apply_aligned_attr_override_to_record(
    ty: TyId,
    attrs: &[rcc_ast::Attribute],
    tcx: &TyCtxt,
    crate_: &mut HirCrate,
    session: &Session,
) {
    let Some(align) = aligned_attr_override(attrs, session) else {
        return;
    };
    let Ty::Record(def_id) = *tcx.get(ty) else {
        return;
    };
    let DefKind::Record { align_override, .. } = &mut crate_.defs[def_id].kind else {
        return;
    };
    *align_override = Some(align_override.map_or(align, |existing| existing.max(align)));
}

fn aligned_attr_override(attrs: &[rcc_ast::Attribute], session: &Session) -> Option<u32> {
    attrs.iter().filter_map(|attr| aligned_attr_value(attr, session)).max()
}

fn apply_packed_attr_to_record(
    ty: TyId,
    attrs: &[rcc_ast::Attribute],
    tcx: &TyCtxt,
    crate_: &mut HirCrate,
    session: &Session,
) {
    if !packed_attr_present(attrs, session) {
        return;
    }
    let Ty::Record(def_id) = *tcx.get(ty) else {
        return;
    };
    let DefKind::Record { packed, .. } = &mut crate_.defs[def_id].kind else {
        return;
    };
    *packed = true;
}

fn packed_attr_present(attrs: &[rcc_ast::Attribute], session: &Session) -> bool {
    attrs
        .iter()
        .any(|attr| matches!(session.interner.get(attr.name), "packed" | "__packed__" | "packed__"))
}

fn ms_struct_attr_present(attrs: &[rcc_ast::Attribute], session: &Session) -> bool {
    attrs.iter().any(|attr| {
        matches!(session.interner.get(attr.name), "ms_struct" | "__ms_struct__" | "ms_struct__")
    })
}

fn aligned_attr_value(attr: &rcc_ast::Attribute, session: &Session) -> Option<u32> {
    let name = session.interner.get(attr.name);
    if !matches!(name, "aligned" | "__aligned__" | "aligned__") {
        return None;
    }
    let [arg] = attr.args.as_slice() else {
        return None;
    };
    let [token] = arg.tokens.as_slice() else {
        return None;
    };
    let rcc_ast::AttributeTokenKind::Int(raw) = token.kind else {
        return None;
    };
    let align = u32::try_from(raw).ok()?;
    (align != 0 && align.is_power_of_two()).then_some(align)
}

fn apply_scalar_storage_order_attr_to_record(
    ty: TyId,
    attrs: &[rcc_ast::Attribute],
    tcx: &TyCtxt,
    crate_: &mut HirCrate,
    session: &Session,
) {
    let Some(order) = scalar_storage_order_attr(attrs, session) else {
        return;
    };
    let Ty::Record(def_id) = *tcx.get(ty) else {
        return;
    };
    let DefKind::Record { scalar_storage_order, .. } = &mut crate_.defs[def_id].kind else {
        return;
    };
    *scalar_storage_order = Some(order);
}

fn scalar_storage_order_attr(
    attrs: &[rcc_ast::Attribute],
    session: &Session,
) -> Option<ScalarStorageOrder> {
    attrs.iter().filter_map(|attr| scalar_storage_order_attr_value(attr, session)).next_back()
}

fn scalar_storage_order_attr_value(
    attr: &rcc_ast::Attribute,
    session: &Session,
) -> Option<ScalarStorageOrder> {
    let name = session.interner.get(attr.name);
    if !matches!(
        name,
        "scalar_storage_order" | "__scalar_storage_order__" | "scalar_storage_order__"
    ) {
        return None;
    }
    let [arg] = attr.args.as_slice() else {
        return None;
    };
    let [token] = arg.tokens.as_slice() else {
        return None;
    };
    let rcc_ast::AttributeTokenKind::String(bytes) = &token.kind else {
        return None;
    };
    match bytes.as_slice() {
        b"little-endian" => Some(ScalarStorageOrder::LittleEndian),
        b"big-endian" => Some(ScalarStorageOrder::BigEndian),
        _ => None,
    }
}

fn apply_vector_size_attrs_to_type(
    mut ty: TyId,
    attrs: &[rcc_ast::Attribute],
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> TyId {
    for attr in attrs {
        if !is_vector_size_attr(attr, session) {
            continue;
        }
        let Some(bytes) = vector_size_attr_bytes(attr, tcx, session) else {
            emit_invalid_vector_size(
                attr.span,
                "vector_size requires an integer byte size",
                session,
            );
            return tcx.error;
        };
        ty = make_vector_type(ty, bytes, attr.span, tcx, session);
        if ty == tcx.error {
            return ty;
        }
    }
    ty
}

fn is_vector_size_attr(attr: &rcc_ast::Attribute, session: &Session) -> bool {
    matches!(session.interner.get(attr.name), "vector_size" | "__vector_size__" | "vector_size__")
}

fn vector_size_attr_bytes(
    attr: &rcc_ast::Attribute,
    tcx: &TyCtxt,
    session: &Session,
) -> Option<u64> {
    let [arg] = attr.args.as_slice() else {
        return None;
    };
    AttrExprParser::new(&arg.tokens, tcx, session).parse()
}

fn make_vector_type(
    elem: TyId,
    bytes: u64,
    span: Span,
    tcx: &mut TyCtxt,
    session: &mut Session,
) -> TyId {
    if bytes == 0 || !bytes.is_power_of_two() {
        emit_invalid_vector_size(
            span,
            "vector_size must be a non-zero power-of-two byte size",
            session,
        );
        return tcx.error;
    }
    if !matches!(tcx.get(elem), Ty::Int { .. } | Ty::Float(_)) {
        emit_invalid_vector_size(
            span,
            "vector_size currently requires an integer or floating element type",
            session,
        );
        return tcx.error;
    }
    let elem_layout = match LayoutCx::new(tcx).layout_of(elem) {
        Ok(layout) => layout,
        Err(_) => {
            emit_invalid_vector_size(
                span,
                "vector_size element type has no compile-time layout",
                session,
            );
            return tcx.error;
        }
    };
    if elem_layout.size == 0 || bytes % elem_layout.size != 0 {
        emit_invalid_vector_size(
            span,
            "vector_size byte size must be a multiple of the element size",
            session,
        );
        return tcx.error;
    }
    let lanes = bytes / elem_layout.size;
    let Ok(lanes) = u32::try_from(lanes) else {
        emit_invalid_vector_size(span, "vector_size lane count is too large", session);
        return tcx.error;
    };
    if lanes == 0 {
        emit_invalid_vector_size(span, "vector_size must contain at least one lane", session);
        return tcx.error;
    }
    tcx.intern(Ty::Vector { elem, lanes, bytes })
}

fn emit_invalid_vector_size(span: Span, message: &str, session: &mut Session) {
    session.handler.struct_err(span, message).code(rcc_errors::codes::E0061).emit();
}

struct AttrExprParser<'a> {
    tokens: &'a [rcc_ast::AttributeToken],
    pos: usize,
    tcx: &'a TyCtxt,
    session: &'a Session,
}

impl<'a> AttrExprParser<'a> {
    fn new(tokens: &'a [rcc_ast::AttributeToken], tcx: &'a TyCtxt, session: &'a Session) -> Self {
        Self { tokens, pos: 0, tcx, session }
    }

    fn parse(mut self) -> Option<u64> {
        let value = self.parse_add()?;
        (self.pos == self.tokens.len()).then_some(value)
    }

    fn parse_add(&mut self) -> Option<u64> {
        let mut lhs = self.parse_mul()?;
        loop {
            if self.eat_punct("+") {
                lhs = lhs.checked_add(self.parse_mul()?)?;
            } else if self.eat_punct("-") {
                lhs = lhs.checked_sub(self.parse_mul()?)?;
            } else {
                return Some(lhs);
            }
        }
    }

    fn parse_mul(&mut self) -> Option<u64> {
        let mut lhs = self.parse_primary()?;
        loop {
            if self.eat_punct("*") {
                lhs = lhs.checked_mul(self.parse_primary()?)?;
            } else if self.eat_punct("/") {
                let rhs = self.parse_primary()?;
                if rhs == 0 {
                    return None;
                }
                lhs /= rhs;
            } else {
                return Some(lhs);
            }
        }
    }

    fn parse_primary(&mut self) -> Option<u64> {
        if self.eat_punct("(") {
            let value = self.parse_add()?;
            return self.eat_punct(")").then_some(value);
        }
        if self.eat_symbol("sizeof") {
            return self.parse_sizeof();
        }
        let token = self.tokens.get(self.pos)?;
        if let rcc_ast::AttributeTokenKind::Int(value) = token.kind {
            self.pos += 1;
            return u64::try_from(value).ok();
        }
        None
    }

    fn parse_sizeof(&mut self) -> Option<u64> {
        if !self.eat_punct("(") {
            return None;
        }
        let start = self.pos;
        let mut depth = 1_u32;
        while let Some(token) = self.tokens.get(self.pos) {
            if self.token_is_punct(token, "(") {
                depth += 1;
            } else if self.token_is_punct(token, ")") {
                depth -= 1;
                if depth == 0 {
                    let ty = self.builtin_type_from_tokens(&self.tokens[start..self.pos])?;
                    self.pos += 1;
                    return LayoutCx::new(self.tcx).layout_of(ty).ok().map(|layout| layout.size);
                }
            }
            self.pos += 1;
        }
        None
    }

    fn builtin_type_from_tokens(&self, tokens: &[rcc_ast::AttributeToken]) -> Option<TyId> {
        let mut words = Vec::new();
        for token in tokens {
            let rcc_ast::AttributeTokenKind::Symbol(sym) = token.kind else {
                return None;
            };
            words.push(self.session.interner.get(sym));
        }
        match words.as_slice() {
            ["char"] | ["signed", "char"] => Some(self.tcx.char_),
            ["unsigned", "char"] => Some(self.tcx.uchar),
            ["short"] | ["short", "int"] | ["signed", "short"] | ["signed", "short", "int"] => {
                Some(self.tcx.short)
            }
            ["unsigned", "short"] | ["unsigned", "short", "int"] => Some(self.tcx.ushort),
            ["int"] | ["signed"] | ["signed", "int"] => Some(self.tcx.int),
            ["unsigned"] | ["unsigned", "int"] => Some(self.tcx.uint),
            ["long"] | ["long", "int"] | ["signed", "long"] | ["signed", "long", "int"] => {
                Some(self.tcx.long)
            }
            ["unsigned", "long"] | ["unsigned", "long", "int"] => Some(self.tcx.ulong),
            ["long", "long"]
            | ["long", "long", "int"]
            | ["signed", "long", "long"]
            | ["signed", "long", "long", "int"] => Some(self.tcx.long_long),
            ["unsigned", "long", "long"] | ["unsigned", "long", "long", "int"] => {
                Some(self.tcx.ulong_long)
            }
            ["float"] => Some(self.tcx.float),
            ["double"] => Some(self.tcx.double),
            ["long", "double"] => Some(self.tcx.long_double),
            _ => None,
        }
    }

    fn eat_symbol(&mut self, expected: &str) -> bool {
        let Some(token) = self.tokens.get(self.pos) else {
            return false;
        };
        let rcc_ast::AttributeTokenKind::Symbol(sym) = token.kind else {
            return false;
        };
        if self.session.interner.get(sym) == expected {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_punct(&mut self, expected: &str) -> bool {
        let Some(token) = self.tokens.get(self.pos) else {
            return false;
        };
        if self.token_is_punct(token, expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn token_is_punct(&self, token: &rcc_ast::AttributeToken, expected: &str) -> bool {
        let rcc_ast::AttributeTokenKind::Punct(sym) = token.kind else {
            return false;
        };
        self.session.interner.get(sym) == expected
    }
}

/// Lower an AST `type-name` (used by casts, `sizeof(type)`, and compound
/// literals) into a `TyId`.
///
/// Expression lowering still wires the result into dedicated HIR shapes in
/// later tasks; this helper exists now so every type-name lowering path uses
/// the same service as declarations.
#[allow(clippy::too_many_arguments)]
pub fn lower_type_name(
    ty: &rcc_ast::TypeName,
    scope: DeclScope,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    lower_type_name_in_scope(ty, scope, None, tcx, resolver, crate_, session)
}

#[allow(clippy::too_many_arguments)]
fn lower_type_name_in_scope(
    ty: &rcc_ast::TypeName,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    lower_type_from_parts_in_scope(
        &ty.specs,
        &ty.declarator,
        scope,
        typedef_scope,
        None,
        tcx,
        resolver,
        crate_,
        session,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_specs_to_base_ty(
    specs: &rcc_ast::DeclSpecs,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    lower_specs_to_base_ty_in_scope(specs, None, None, tcx, resolver, crate_, session)
}

#[allow(clippy::too_many_arguments)]
fn lower_specs_to_base_ty_in_scope(
    specs: &rcc_ast::DeclSpecs,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    for ts in &specs.type_specs {
        match ts {
            TypeSpec::TypedefName(sym) => {
                let mut expanding = FxHashSet::default();
                return lower_typedef_name_in_scope(
                    *sym,
                    specs.span,
                    &mut expanding,
                    typedef_scope,
                    resolver,
                    crate_,
                    tcx,
                    session,
                );
            }
            TypeSpec::Record(spec) => {
                return lower_record_spec_to_ty(spec, tcx, resolver, crate_, session);
            }
            TypeSpec::Enum(spec) => {
                return lower_enum_spec_to_ty(spec, tcx, resolver, crate_, session);
            }
            TypeSpec::TypeofType(ty) => {
                return lower_type_name_in_scope(
                    ty,
                    DeclScope::File,
                    typedef_scope,
                    tcx,
                    resolver,
                    crate_,
                    session,
                );
            }
            TypeSpec::TypeofExpr(expr) => {
                return lower_typeof_expr_to_ty(
                    expr,
                    typedef_scope,
                    local_body,
                    resolver,
                    crate_,
                    tcx,
                    session,
                );
            }
            TypeSpec::Atomic(ty) => {
                let inner = lower_type_name_in_scope(
                    ty,
                    DeclScope::File,
                    typedef_scope,
                    tcx,
                    resolver,
                    crate_,
                    session,
                );
                return if inner == tcx.error { inner } else { tcx.intern(Ty::Atomic(inner)) };
            }
            _ => {}
        }
    }

    lower_builtin_specs_to_base_ty(specs, tcx, session)
}

#[allow(clippy::too_many_arguments)]
fn lower_record_spec_to_ty(
    spec: &RecordSpec,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    let expected = match spec.kind {
        rcc_ast::RecordKind::Struct => TagKind::Struct,
        rcc_ast::RecordKind::Union => TagKind::Union,
    };
    let hir_kind = match spec.kind {
        rcc_ast::RecordKind::Struct => RecordKind::Struct,
        rcc_ast::RecordKind::Union => RecordKind::Union,
    };

    let def_id = if let Some(tag) = spec.tag {
        let resolver_fn = if spec.fields.is_some() { resolve_tag_definition } else { resolve_tag };
        let Some(id) = resolver_fn(tag, spec.span, expected, crate_, tcx, resolver, session) else {
            return tcx.error;
        };
        id
    } else {
        let name = session.interner.intern("<anonymous record>");
        let id = crate_.defs.push(Def {
            id: DefId(0),
            name,
            span: spec.span,
            kind: DefKind::Record {
                kind: hir_kind,
                packed: false,
                ms_bitfields: false,
                align_override: None,
                scalar_storage_order: None,
                layout: None,
                fields: Vec::new(),
            },
        });
        crate_.defs[id].id = id;
        id
    };

    if spec.fields.is_some() && can_complete_record_tag(def_id, spec.span, crate_, session) {
        let lowered = lower_record(spec, tcx, resolver, crate_, session);
        crate_.defs[def_id].span = spec.span;
        crate_.defs[def_id].kind = lowered;
    }

    tcx.intern(Ty::Record(def_id))
}

#[allow(clippy::too_many_arguments)]
fn lower_enum_spec_to_ty(
    spec: &EnumSpec,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    let def_id = if let Some(tag) = spec.tag {
        let resolver_fn =
            if spec.enumerators.is_some() { resolve_tag_definition } else { resolve_tag };
        let Some(id) = resolver_fn(tag, spec.span, TagKind::Enum, crate_, tcx, resolver, session)
        else {
            return tcx.error;
        };
        id
    } else {
        let name = session.interner.intern("<anonymous enum>");
        let id = crate_.defs.push(Def {
            id: DefId(0),
            name,
            span: spec.span,
            kind: DefKind::Enum { repr: tcx.int, variants: Vec::new() },
        });
        crate_.defs[id].id = id;
        id
    };

    if spec.enumerators.is_some() && can_complete_enum_tag(def_id, spec.span, crate_, session) {
        let lowered = lower_enum(spec, tcx, resolver, crate_, session);
        crate_.defs[def_id].span = spec.span;
        crate_.defs[def_id].kind = lowered;
    }

    tcx.intern(Ty::Enum(def_id))
}

fn can_complete_record_tag(
    def_id: DefId,
    span: Span,
    crate_: &HirCrate,
    session: &mut Session,
) -> bool {
    let def = &crate_.defs[def_id];
    let DefKind::Record { fields, .. } = &def.kind else {
        return true;
    };
    if !fields.is_empty() && def.span != span {
        emit_duplicate_ordinary(def.name, span, session);
        return false;
    }
    true
}

fn can_complete_enum_tag(
    def_id: DefId,
    span: Span,
    crate_: &HirCrate,
    session: &mut Session,
) -> bool {
    let def = &crate_.defs[def_id];
    let DefKind::Enum { variants, .. } = &def.kind else {
        return true;
    };
    if !variants.is_empty() && def.span != span {
        emit_duplicate_ordinary(def.name, span, session);
        return false;
    }
    true
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
fn lower_atomic_qual(base: TyId, q: &rcc_ast::TypeQuals, tcx: &mut TyCtxt) -> TyId {
    if q.atomic {
        tcx.intern(Ty::Atomic(base))
    } else {
        base
    }
}

fn strip_atomic_qual(mut q: rcc_ast::TypeQuals) -> rcc_ast::TypeQuals {
    q.atomic = false;
    q
}

fn quals_to_hir(base: TyId, q: &rcc_ast::TypeQuals, tcx: &mut TyCtxt) -> Qual {
    let ty = lower_atomic_qual(base, q, tcx);
    Qual { ty, is_const: q.const_, is_volatile: q.volatile, is_restrict: q.restrict }
}

fn object_quals_from_type_quals(q: &rcc_ast::TypeQuals) -> ObjectQuals {
    ObjectQuals { is_const: q.const_, is_volatile: q.volatile, is_restrict: q.restrict }
}

fn merge_type_quals(
    base: rcc_ast::TypeQuals,
    component: &rcc_ast::TypeQuals,
) -> rcc_ast::TypeQuals {
    rcc_ast::TypeQuals {
        const_: base.const_ || component.const_,
        volatile: base.volatile || component.volatile,
        restrict: base.restrict || component.restrict,
        atomic: base.atomic || component.atomic,
    }
}

fn declaration_object_quals(specs: &rcc_ast::DeclSpecs, declarator: &Declarator) -> ObjectQuals {
    if declarator.derived.iter().all(|derived| matches!(derived, DerivedDeclarator::Pointer(_))) {
        return match declarator.derived.first() {
            Some(DerivedDeclarator::Pointer(quals)) => object_quals_from_type_quals(quals),
            Some(_) => unreachable!("all derived declarators are pointers"),
            None => object_quals_from_type_quals(&specs.quals),
        };
    }

    match declarator.derived.last() {
        Some(DerivedDeclarator::Pointer(quals)) => object_quals_from_type_quals(quals),
        Some(_) => ObjectQuals::none(),
        None => object_quals_from_type_quals(&specs.quals),
    }
}

fn parameter_object_quals(specs: &rcc_ast::DeclSpecs, declarator: &Declarator) -> ObjectQuals {
    match declarator.derived.last() {
        Some(DerivedDeclarator::Array(arr)) => object_quals_from_type_quals(&arr.quals),
        _ => declaration_object_quals(specs, declarator),
    }
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
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    apply_declarator_with_context(base, d, scope, tcx, &mut resolver, &mut crate_, session)
}

#[allow(clippy::too_many_arguments)]
fn apply_declarator_with_context(
    base: TyId,
    d: &Declarator,
    scope: DeclScope,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    apply_declarator_with_context_in_scope(
        base, d, scope, None, None, tcx, resolver, crate_, session,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_declarator_with_context_in_scope(
    base: TyId,
    d: &Declarator,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> TyId {
    apply_declarator_with_base_quals_in_scope(
        base,
        rcc_ast::TypeQuals::default(),
        d,
        scope,
        typedef_scope,
        local_body,
        tcx,
        resolver,
        crate_,
        session,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_declarator_with_base_quals_in_scope(
    base: TyId,
    base_quals: rcc_ast::TypeQuals,
    d: &Declarator,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
    allow_void_declarator: bool,
) -> TyId {
    let mut ty = base;
    let mut pending_component_quals = base_quals;
    let derived = derived_chain_for_type_construction(d);

    // Iterate the derived chain in forward order (outermost-to-innermost).
    for dd in derived.iter() {
        match dd {
            DerivedDeclarator::Pointer(quals) => {
                // Build a pointer to the current qualified component.
                // The pointer's own qualifiers become pending metadata for
                // the newly constructed pointer type: an outer pointer/array
                // can consume them as component qualifiers, while the final
                // declarator object records them in `ObjectQuals`.
                let qual = quals_to_hir(ty, &pending_component_quals, tcx);
                ty = tcx.intern(Ty::Ptr(qual));
                pending_component_quals = *quals;
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
                // literal constants for now). Non-constant block-scope
                // bounds are VLAs; the HIR local stores the lowered bound
                // expression so CFG can evaluate it at the declaration.
                let (len, is_vla) = if arr_decl.star {
                    // [*] — VLA of unspecified size.
                    (None, true)
                } else if let Some(ref size_expr) = arr_decl.size {
                    // Try to evaluate as a constant integer.
                    match eval_array_bound_as_u64(
                        size_expr,
                        scope,
                        typedef_scope,
                        local_body,
                        tcx,
                        resolver,
                        crate_,
                        session,
                    ) {
                        Some(n) => (Some(n), false),
                        None => (None, scope != DeclScope::File),
                    }
                } else {
                    // No size — incomplete array. The declaration-lowering
                    // caller completes it from an initializer when one exists
                    // (`int a[] = {1,2,3}` / `char s[] = "hi"`), then emits
                    // the block-scope incomplete-array diagnostic if no
                    // initializer could complete the type.
                    (None, false)
                };

                let merged = if scope == DeclScope::Param {
                    // C99 §6.7.5.3p7 adjusts array parameters to
                    // pointers. Qualifiers written inside the brackets
                    // qualify that adjusted pointer object, not the
                    // element type.
                    pending_component_quals
                } else {
                    merge_type_quals(pending_component_quals, &arr_decl.quals)
                };
                let elem = quals_to_hir(ty, &merged, tcx);
                ty = tcx.intern(Ty::Array { elem, len, is_vla });
                pending_component_quals = rcc_ast::TypeQuals::default();
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
                    let param_ty = lower_type_from_parts_in_scope(
                        &param.specs,
                        &param.declarator,
                        DeclScope::Param,
                        typedef_scope,
                        None,
                        tcx,
                        resolver,
                        crate_,
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

    ty = lower_atomic_qual(ty, &pending_component_quals, tcx);

    // Final check: if after all derivations the type is still void
    // and the declarator has a name (i.e. it's an object, not a
    // return type or parameter), reject it.
    // But only if there were no derivations — if there were
    // derivations, void was either wrapped in a pointer (legal) or
    // caught above.
    if !allow_void_declarator
        && d.derived.is_empty()
        && *tcx.get(ty) == Ty::Void
        && d.name.is_some()
    {
        session
            .handler
            .struct_err(d.span, "cannot declare variable of type `void`".to_string())
            .code(rcc_errors::codes::E0076)
            .emit();
        return tcx.error;
    }

    ty
}

fn derived_chain_for_type_construction(d: &Declarator) -> Vec<DerivedDeclarator> {
    let mut out = Vec::with_capacity(d.derived.len());
    let mut i = 0;
    while i < d.derived.len() {
        if matches!(d.derived[i], DerivedDeclarator::Pointer(_)) {
            let start = i;
            while i < d.derived.len() && matches!(d.derived[i], DerivedDeclarator::Pointer(_)) {
                i += 1;
            }
            out.extend(d.derived[start..i].iter().rev().cloned());
        } else {
            out.push(d.derived[i].clone());
            i += 1;
        }
    }
    out
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
        Ty::BuiltinVaList => {
            // On the SysV/glibc target model, `__builtin_va_list` is
            // represented as the object payload of the real single-element
            // array typedef. Function parameters therefore adjust to a
            // pointer, matching clang's libc ABI for `vprintf`.
            tcx.intern(Ty::Ptr(Qual::plain(ty)))
        }
        Ty::Func { .. } => {
            // Decay to pointer to function.
            tcx.intern(Ty::Ptr(Qual::plain(ty)))
        }
        _ => ty,
    }
}

/// Lower builtin scalar declaration specifiers to a base type.
///
/// Typedef names, records, and enums are handled before this helper by
/// [`lower_specs_to_base_ty`]. This function must therefore never silently
/// swallow non-builtin specifiers and turn them into `int`.
fn lower_builtin_specs_to_base_ty(
    specs: &rcc_ast::DeclSpecs,
    tcx: &mut TyCtxt,
    session: &mut Session,
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
    let mut has_complex = false;
    let mut saw_builtin = false;
    let mut saw_unsupported = false;

    for ts in &specs.type_specs {
        match ts {
            TypeSpec::Void => {
                has_void = true;
                saw_builtin = true;
            }
            TypeSpec::Char => {
                has_char = true;
                saw_builtin = true;
            }
            TypeSpec::Short => {
                has_short = true;
                saw_builtin = true;
            }
            TypeSpec::Int => {
                has_int = true;
                saw_builtin = true;
            }
            TypeSpec::Long => {
                long_count += 1;
                saw_builtin = true;
            }
            TypeSpec::Float => {
                has_float = true;
                saw_builtin = true;
            }
            TypeSpec::Double => {
                has_double = true;
                saw_builtin = true;
            }
            TypeSpec::Signed => {
                has_signed = true;
                saw_builtin = true;
            }
            TypeSpec::Unsigned => {
                has_unsigned = true;
                saw_builtin = true;
            }
            TypeSpec::Bool => {
                has_bool = true;
                saw_builtin = true;
            }
            TypeSpec::Complex => {
                has_complex = true;
                saw_builtin = true;
            }
            TypeSpec::Imaginary => {
                saw_unsupported = true;
            }
            TypeSpec::BuiltinVaList => {
                return tcx.builtin_va_list;
            }
            TypeSpec::TypedefName(_)
            | TypeSpec::Record(_)
            | TypeSpec::Enum(_)
            | TypeSpec::TypeofExpr(_)
            | TypeSpec::TypeofType(_)
            | TypeSpec::Atomic(_) => {
                saw_unsupported = true;
            }
        }
    }

    if saw_unsupported {
        session
            .handler
            .struct_err(specs.span, "unsupported type specifier in HIR lowering".to_string())
            .code(rcc_errors::codes::E0061)
            .emit();
        return tcx.error;
    }

    if has_complex {
        if has_float {
            return tcx.complex_float;
        }
        if has_double && long_count >= 1 {
            return tcx.complex_long_double;
        }
        return tcx.complex_double;
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

    if saw_builtin {
        session
            .handler
            .struct_err(specs.span, "invalid builtin type specifier combination".to_string())
            .code(rcc_errors::codes::E0061)
            .emit();
    }
    tcx.error
}

/// Stub constant-expression evaluator for array sizes.
///
/// Handles only integer literals for now. A full `ConstEval` lives in
/// `rcc_typeck` and will be wired in later.
fn eval_const_expr_as_u64(expr: &rcc_ast::Expr) -> Option<u64> {
    let value = eval_const_expr_as_i128(expr)?;
    u64::try_from(value).ok()
}

fn eval_const_expr_as_i128(expr: &rcc_ast::Expr) -> Option<i128> {
    match &expr.kind {
        rcc_ast::ExprKind::IntLit(lit) => i128::try_from(lit.value).ok(),
        rcc_ast::ExprKind::CharLit(lit) => Some(i128::from(lit.value)),
        rcc_ast::ExprKind::Paren(inner) => eval_const_expr_as_i128(inner),
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Plus, operand } => {
            eval_const_expr_as_i128(operand)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Neg, operand } => {
            eval_const_expr_as_i128(operand).and_then(i128::checked_neg)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::BitNot, operand } => {
            eval_const_expr_as_i128(operand).map(|v| !v)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::LogNot, operand } => {
            eval_const_expr_as_i128(operand).map(|v| i128::from(v == 0))
        }
        rcc_ast::ExprKind::Binary { op, lhs, rhs } => {
            let l = eval_const_expr_as_i128(lhs)?;
            let r = eval_const_expr_as_i128(rhs)?;
            match op {
                rcc_ast::BinOp::Add => l.checked_add(r),
                rcc_ast::BinOp::Sub => l.checked_sub(r),
                rcc_ast::BinOp::Mul => l.checked_mul(r),
                rcc_ast::BinOp::Div => (r != 0).then(|| l.checked_div(r)).flatten(),
                rcc_ast::BinOp::Rem => (r != 0).then(|| l.checked_rem(r)).flatten(),
                rcc_ast::BinOp::Shl => u32::try_from(r).ok().and_then(|shift| l.checked_shl(shift)),
                rcc_ast::BinOp::Shr => u32::try_from(r).ok().and_then(|shift| l.checked_shr(shift)),
                rcc_ast::BinOp::Lt => Some(i128::from(l < r)),
                rcc_ast::BinOp::Le => Some(i128::from(l <= r)),
                rcc_ast::BinOp::Gt => Some(i128::from(l > r)),
                rcc_ast::BinOp::Ge => Some(i128::from(l >= r)),
                rcc_ast::BinOp::Eq => Some(i128::from(l == r)),
                rcc_ast::BinOp::Ne => Some(i128::from(l != r)),
                rcc_ast::BinOp::BitAnd => Some(l & r),
                rcc_ast::BinOp::BitXor => Some(l ^ r),
                rcc_ast::BinOp::BitOr => Some(l | r),
                rcc_ast::BinOp::LogAnd => Some(i128::from(l != 0 && r != 0)),
                rcc_ast::BinOp::LogOr => Some(i128::from(l != 0 || r != 0)),
            }
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            if eval_const_expr_as_i128(cond)? != 0 {
                eval_const_expr_as_i128(then_expr)
            } else {
                eval_const_expr_as_i128(else_expr)
            }
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            let c = eval_const_expr_as_i128(cond)?;
            if c != 0 {
                Some(c)
            } else {
                eval_const_expr_as_i128(else_expr)
            }
        }
        _ => None,
    }
}

/// Constant-evaluate an array bound while HIR lowering declarators.
///
/// This intentionally stays narrower than full HIR `ConstEval`: it covers the
/// AST expression shapes that can appear after preprocessing in C99 array
/// bounds before type checking, plus `sizeof(type-name)` because that is a
/// first-class integer constant expression in C99. Expressions that need
/// ordinary identifier lookup still return `None`, making block-scope arrays
/// true VLAs.
#[allow(clippy::too_many_arguments)]
fn eval_array_bound_as_u64(
    expr: &rcc_ast::Expr,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<u64> {
    let value = eval_array_bound_as_i128(
        expr,
        scope,
        typedef_scope,
        local_body,
        tcx,
        resolver,
        crate_,
        session,
    )?;
    u64::try_from(value).ok()
}

#[allow(clippy::too_many_arguments)]
fn eval_array_bound_as_i128(
    expr: &rcc_ast::Expr,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    local_body: Option<&Body>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<i128> {
    match &expr.kind {
        rcc_ast::ExprKind::SizeofType(type_name) => {
            let ty = lower_type_name_in_scope(
                type_name,
                scope,
                typedef_scope,
                tcx,
                resolver,
                crate_,
                session,
            );
            let layout = LayoutCx::with_defs(tcx, &crate_.defs).layout_of(ty).ok()?;
            Some(i128::from(layout.size))
        }
        rcc_ast::ExprKind::AlignofType(type_name) => {
            let ty = lower_type_name_in_scope(
                type_name,
                scope,
                typedef_scope,
                tcx,
                resolver,
                crate_,
                session,
            );
            let layout = LayoutCx::with_defs(tcx, &crate_.defs).layout_of(ty).ok()?;
            Some(i128::from(layout.align))
        }
        rcc_ast::ExprKind::SizeofExpr(operand) => {
            let ty = lower_sizeof_operand_to_ty(
                operand,
                typedef_scope,
                local_body,
                resolver,
                crate_,
                tcx,
                session,
            );
            let layout = LayoutCx::with_defs(tcx, &crate_.defs).layout_of(ty).ok()?;
            Some(i128::from(layout.size))
        }
        rcc_ast::ExprKind::AlignofExpr(operand) => {
            let ty = lower_sizeof_operand_to_ty(
                operand,
                typedef_scope,
                local_body,
                resolver,
                crate_,
                tcx,
                session,
            );
            let layout = LayoutCx::with_defs(tcx, &crate_.defs).layout_of(ty).ok()?;
            Some(i128::from(layout.align))
        }
        rcc_ast::ExprKind::Ident(name) => {
            let def_id = *resolver.ordinary.get(name)?;
            match &crate_.defs.get(def_id)?.kind {
                DefKind::Enumerator { value, .. } => Some(*value),
                _ => None,
            }
        }
        rcc_ast::ExprKind::IntLit(lit) => i128::try_from(lit.value).ok(),
        rcc_ast::ExprKind::CharLit(lit) => Some(i128::from(lit.value)),
        rcc_ast::ExprKind::BuiltinOffsetof { ty, designators } => Some(i128::from(
            lower_builtin_offsetof(ty, designators, expr.span, crate_, tcx, resolver, session),
        )),
        rcc_ast::ExprKind::Cast { expr: operand, .. } => eval_array_bound_as_i128(
            operand,
            scope,
            typedef_scope,
            local_body,
            tcx,
            resolver,
            crate_,
            session,
        ),
        rcc_ast::ExprKind::Paren(inner) => eval_array_bound_as_i128(
            inner,
            scope,
            typedef_scope,
            local_body,
            tcx,
            resolver,
            crate_,
            session,
        ),
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Plus, operand } => eval_array_bound_as_i128(
            operand,
            scope,
            typedef_scope,
            local_body,
            tcx,
            resolver,
            crate_,
            session,
        ),
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Neg, operand } => eval_array_bound_as_i128(
            operand,
            scope,
            typedef_scope,
            local_body,
            tcx,
            resolver,
            crate_,
            session,
        )
        .and_then(i128::checked_neg),
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::BitNot, operand } => {
            eval_array_bound_as_i128(
                operand,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )
            .map(|v| !v)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::LogNot, operand } => {
            eval_array_bound_as_i128(
                operand,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )
            .map(|v| i128::from(v == 0))
        }
        rcc_ast::ExprKind::Binary { op, lhs, rhs } => {
            let l = eval_array_bound_as_i128(
                lhs,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )?;
            let r = eval_array_bound_as_i128(
                rhs,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )?;
            match op {
                rcc_ast::BinOp::Add => l.checked_add(r),
                rcc_ast::BinOp::Sub => l.checked_sub(r),
                rcc_ast::BinOp::Mul => l.checked_mul(r),
                rcc_ast::BinOp::Div => (r != 0).then(|| l.checked_div(r)).flatten(),
                rcc_ast::BinOp::Rem => (r != 0).then(|| l.checked_rem(r)).flatten(),
                rcc_ast::BinOp::Shl => u32::try_from(r).ok().and_then(|shift| l.checked_shl(shift)),
                rcc_ast::BinOp::Shr => u32::try_from(r).ok().and_then(|shift| l.checked_shr(shift)),
                rcc_ast::BinOp::Lt => Some(i128::from(l < r)),
                rcc_ast::BinOp::Le => Some(i128::from(l <= r)),
                rcc_ast::BinOp::Gt => Some(i128::from(l > r)),
                rcc_ast::BinOp::Ge => Some(i128::from(l >= r)),
                rcc_ast::BinOp::Eq => Some(i128::from(l == r)),
                rcc_ast::BinOp::Ne => Some(i128::from(l != r)),
                rcc_ast::BinOp::BitAnd => Some(l & r),
                rcc_ast::BinOp::BitXor => Some(l ^ r),
                rcc_ast::BinOp::BitOr => Some(l | r),
                rcc_ast::BinOp::LogAnd => Some(i128::from(l != 0 && r != 0)),
                rcc_ast::BinOp::LogOr => Some(i128::from(l != 0 || r != 0)),
            }
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            if eval_array_bound_as_i128(
                cond,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )? != 0
            {
                eval_array_bound_as_i128(
                    then_expr,
                    scope,
                    typedef_scope,
                    local_body,
                    tcx,
                    resolver,
                    crate_,
                    session,
                )
            } else {
                eval_array_bound_as_i128(
                    else_expr,
                    scope,
                    typedef_scope,
                    local_body,
                    tcx,
                    resolver,
                    crate_,
                    session,
                )
            }
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            let c = eval_array_bound_as_i128(
                cond,
                scope,
                typedef_scope,
                local_body,
                tcx,
                resolver,
                crate_,
                session,
            )?;
            if c != 0 {
                Some(c)
            } else {
                eval_array_bound_as_i128(
                    else_expr,
                    scope,
                    typedef_scope,
                    local_body,
                    tcx,
                    resolver,
                    crate_,
                    session,
                )
            }
        }
        _ => None,
    }
}

/// Return the width in bits of an integer type for bit-field validation.
///
/// The numbers follow the rcc target-independent defaults used by
/// `rcc_typeck` / `rcc_codegen_llvm` (LP64-ish): `_Bool` is 1 bit,
/// `char` 8, `short` 16, `int` 32, `long` / `long long` 64. Returns
/// `None` when the type is not an integer (bit-fields shall have an
/// integer type per C99 §6.7.2.1p5).
fn int_type_bit_width(ty: TyId, tcx: &TyCtxt) -> Option<u32> {
    match tcx.get(ty) {
        Ty::Int { rank, .. } => Some(match rank {
            IntRank::Bool => 1,
            IntRank::Char => 8,
            IntRank::Short => 16,
            IntRank::Int => 32,
            IntRank::Long => 64,
            IntRank::LongLong => 64,
        }),
        _ => None,
    }
}

/// Constant-evaluate a bit-field width expression to a signed integer.
///
/// Bit-field widths are integer constant expressions, so reuse the same
/// pre-typeck evaluator that array bounds use. This covers common hosted
/// headers and projects that spell widths as `sizeof(type) * CHAR_BIT - n`.
fn eval_bit_width(
    expr: &rcc_ast::Expr,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<i64> {
    let value = eval_array_bound_as_i128(
        expr,
        DeclScope::Block,
        None,
        None,
        tcx,
        resolver,
        crate_,
        session,
    )?;
    i64::try_from(value).ok()
}

/// Fold an enumerator's `= expr` initializer into an `i128`.
///
/// Scope is deliberately narrow: enumerator values are constant
/// expressions (C99 §6.7.2.2p3), but the full `ConstEval` on HIR is
/// not wired up at this point of the pipeline yet (it runs on HIR
/// expressions and we are still lowering the AST). We cover the same
/// literal shapes as [`eval_const_expr_as_u64`] but keep the result
/// signed so a negative explicit value like `-1` round-trips.
///
/// Returns `None` on unrecognised shapes; the caller then emits a
/// diagnostic on behalf of the broken enumerator.
fn eval_enum_value_as_i128(
    expr: &rcc_ast::Expr,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> Option<i128> {
    match &expr.kind {
        rcc_ast::ExprKind::IntLit(lit) => i128::try_from(lit.value).ok(),
        rcc_ast::ExprKind::CharLit(lit) => Some(i128::from(lit.value)),
        rcc_ast::ExprKind::Ident(name) => {
            let def_id = *resolver.ordinary.get(name)?;
            match &crate_.defs.get(def_id)?.kind {
                DefKind::Enumerator { value, .. } => Some(*value),
                _ => None,
            }
        }
        rcc_ast::ExprKind::Paren(inner) => eval_enum_value_as_i128(inner, resolver, crate_),
        rcc_ast::ExprKind::Cast { expr: operand, .. } => {
            eval_enum_value_as_i128(operand, resolver, crate_)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Neg, operand } => {
            eval_enum_value_as_i128(operand, resolver, crate_).and_then(i128::checked_neg)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::Plus, operand } => {
            eval_enum_value_as_i128(operand, resolver, crate_)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::BitNot, operand } => {
            eval_enum_value_as_i128(operand, resolver, crate_).map(|v| !v)
        }
        rcc_ast::ExprKind::Unary { op: rcc_ast::UnOp::LogNot, operand } => {
            eval_enum_value_as_i128(operand, resolver, crate_).map(|v| i128::from(v == 0))
        }
        rcc_ast::ExprKind::Binary { op, lhs, rhs } => {
            let l = eval_enum_value_as_i128(lhs, resolver, crate_)?;
            let r = eval_enum_value_as_i128(rhs, resolver, crate_)?;
            match op {
                rcc_ast::BinOp::Add => l.checked_add(r),
                rcc_ast::BinOp::Sub => l.checked_sub(r),
                rcc_ast::BinOp::Mul => l.checked_mul(r),
                rcc_ast::BinOp::Div => (r != 0).then(|| l.checked_div(r)).flatten(),
                rcc_ast::BinOp::Rem => (r != 0).then(|| l.checked_rem(r)).flatten(),
                rcc_ast::BinOp::Shl => u32::try_from(r).ok().and_then(|shift| l.checked_shl(shift)),
                rcc_ast::BinOp::Shr => u32::try_from(r).ok().and_then(|shift| l.checked_shr(shift)),
                rcc_ast::BinOp::Lt => Some(i128::from(l < r)),
                rcc_ast::BinOp::Le => Some(i128::from(l <= r)),
                rcc_ast::BinOp::Gt => Some(i128::from(l > r)),
                rcc_ast::BinOp::Ge => Some(i128::from(l >= r)),
                rcc_ast::BinOp::Eq => Some(i128::from(l == r)),
                rcc_ast::BinOp::Ne => Some(i128::from(l != r)),
                rcc_ast::BinOp::BitAnd => Some(l & r),
                rcc_ast::BinOp::BitXor => Some(l ^ r),
                rcc_ast::BinOp::BitOr => Some(l | r),
                rcc_ast::BinOp::LogAnd => Some(i128::from(l != 0 && r != 0)),
                rcc_ast::BinOp::LogOr => Some(i128::from(l != 0 || r != 0)),
            }
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            if eval_enum_value_as_i128(cond, resolver, crate_)? != 0 {
                eval_enum_value_as_i128(then_expr, resolver, crate_)
            } else {
                eval_enum_value_as_i128(else_expr, resolver, crate_)
            }
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            let c = eval_enum_value_as_i128(cond, resolver, crate_)?;
            if c != 0 {
                Some(c)
            } else {
                eval_enum_value_as_i128(else_expr, resolver, crate_)
            }
        }
        _ => None,
    }
}

/// Materialise an `enum` specifier into a `DefKind::Enum` value.
///
/// For each enumerator:
/// - If an explicit `= expr` is present, fold it to an `i128` via
///   [`eval_enum_value_as_i128`]. Non-foldable expressions emit `E0077`
///   (reusing the invalid-constant diagnostic code) and the enumerator
///   is dropped.
/// - Otherwise the value is `previous + 1`, starting at `0` for the
///   first enumerator (C99 §6.7.2.2p3). Overflow on the implicit
///   `prev + 1` step is pinned at `i128::MAX` and still warned below.
/// - Values outside `[INT_MIN, INT_MAX]` emit `W0007` (M4 simplifies
///   the §6.7.2.2p4 type-selection algorithm to "always `int`"). The
///   enumerator is still recorded so later passes see a stable binding.
///
/// Registration:
/// - Each enumerator name is inserted into `resolver.ordinary` as a
///   fresh `DefKind::Enumerator { ty: tcx.int, value }` (C99 §6.4.4.3).
/// - A duplicate name in the same ordinary namespace emits `E0078` and
///   the new binding is dropped (the first definition wins, matching
///   the convention of the label and typedef passes).
pub fn lower_enum(
    spec: &EnumSpec,
    tcx: &TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> DefKind {
    let Some(enumerators) = spec.enumerators.as_ref() else {
        // Bare tag reference (`enum E;`) — nothing to define.
        return DefKind::Enum { repr: tcx.int, variants: Vec::new() };
    };

    let mut variants: Vec<Enumerator> = Vec::with_capacity(enumerators.len());
    let mut next_value: i128 = 0;

    for enumerator in enumerators {
        let value = if let Some(value_expr) = &enumerator.value {
            match eval_enum_value_as_i128(value_expr, resolver, crate_) {
                Some(v) => v,
                None => {
                    session
                        .handler
                        .struct_err(
                            enumerator.span,
                            "enumerator value is not an integer constant expression".to_string(),
                        )
                        .code(rcc_errors::codes::E0077)
                        .emit();
                    // Fall back to the implicit-continuation counter so
                    // later enumerators in the same list stay sensible.
                    next_value
                }
            }
        } else {
            next_value
        };

        // In M4 the underlying type is fixed to `int`. The §6.7.2.2p4
        // algorithm is deferred to M6; until then we warn when a value
        // does not fit so users see the simplification bite.
        if value < i128::from(i32::MIN) || value > i128::from(i32::MAX) {
            let name_str = session.interner.get(enumerator.name);
            session
                .handler
                .struct_warn(
                    enumerator.span,
                    format!(
                        "value {value} of enumerator `{name_str}` is outside the range of `int`"
                    ),
                )
                .code(rcc_errors::codes::W0007)
                .emit();
        }

        // Register the enumerator in the ordinary namespace. Duplicate
        // names at the same scope are an E0078 constraint violation;
        // the first binding wins.
        if let Some(&_existing) = resolver.ordinary.get(&enumerator.name) {
            let name_str = session.interner.get(enumerator.name);
            session
                .handler
                .struct_err(enumerator.span, format!("duplicate enumerator name `{name_str}`"))
                .code(rcc_errors::codes::E0078)
                .emit();
        } else {
            let id = crate_.defs.push(Def {
                id: DefId(0),
                name: enumerator.name,
                span: enumerator.span,
                kind: DefKind::Enumerator { ty: tcx.int, value },
            });
            crate_.defs[id].id = id;
            resolver.ordinary.insert(enumerator.name, id);
        }

        variants.push(Enumerator { name: enumerator.name, value, span: enumerator.span });
        next_value = value.saturating_add(1);
    }

    DefKind::Enum { repr: tcx.int, variants }
}

/// Materialise a `struct` / `union` specifier (with fields) into a
/// `DefKind::Record` value.
///
/// Field lowering:
/// - Shared `DeclSpecs` are lowered once per `FieldDecl` group to a
///   base `TyId` via [`lower_specs_to_base_ty`] (including typedef,
///   record, and enum references).
/// - Each `FieldDeclarator` is folded over the base with
///   [`apply_declarator`] in `DeclScope::Block` (fields require
///   complete types — no incomplete arrays like `int x[];`).
/// - Anonymous fields (declarator `None`) fall into two cases:
///   - With a `bit_width` → anonymous bit-field (padding separator).
///     The field is emitted with `name = None` so name lookup skips it.
///   - Without a `bit_width` → anonymous `struct`/`union` member
///     (C11 §6.7.2.1p13). The anonymous record itself remains a real
///     field so layout preserves tail padding and union overlay; typeck
///     performs recursive promoted-member lookup for `parent.inner_field`.
/// - Bit-field widths are validated per C99 §6.7.2.1p4: the width must
///   be a non-negative integer constant expression and shall not
///   exceed the width of the underlying type. Named zero-width
///   bit-fields are rejected (only anonymous zero-width bit-fields are
///   the legal "alignment separator" form). Violations emit `E0077`
///   and the offending bit-field is dropped from the field list.
pub fn lower_record(
    spec: &RecordSpec,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> DefKind {
    let kind = match spec.kind {
        rcc_ast::RecordKind::Struct => RecordKind::Struct,
        rcc_ast::RecordKind::Union => RecordKind::Union,
    };

    let mut out_fields: Vec<Field> = Vec::new();

    let Some(field_decls) = spec.fields.as_ref() else {
        // Bare tag reference; nothing to lower. Caller shouldn't normally
        // pass a non-defining spec, but stay defensive.
        return DefKind::Record {
            kind,
            packed: packed_attr_present(&spec.attrs, session),
            ms_bitfields: session.opts.ms_bitfields || ms_struct_attr_present(&spec.attrs, session),
            align_override: aligned_attr_override(&spec.attrs, session),
            scalar_storage_order: scalar_storage_order_attr(&spec.attrs, session),
            layout: None,
            fields: Vec::new(),
        };
    };

    for assertion in &spec.static_asserts {
        check_static_assert(assertion, DeclScope::Block, None, tcx, resolver, crate_, session);
    }

    for fd in field_decls {
        // Lower shared specifiers once per field-decl group.
        let base = lower_specs_to_base_ty(&fd.specs, tcx, resolver, crate_, session);

        for fdd in &fd.declarators {
            match (&fdd.declarator, &fdd.bit_width) {
                // ── Anonymous bit-field: `int : 0;` separator. ──────
                (None, Some(width_expr)) => {
                    let (ok_width, bit_width) = validate_bit_width(
                        base, width_expr, fd.span, /*is_named=*/ false, tcx, resolver, crate_,
                        session,
                    );
                    if ok_width {
                        out_fields.push(Field {
                            name: None,
                            ty: base,
                            quals: object_quals_from_type_quals(&fd.specs.quals),
                            align_override: field_align_override(
                                &fd.specs,
                                &[],
                                DeclScope::Block,
                                None,
                                tcx,
                                resolver,
                                crate_,
                                session,
                            ),
                            offset: None,
                            bit_width,
                            span: fd.span,
                        });
                    }
                }
                // ── Anonymous struct/union member. ─────────────────
                (None, None) => {
                    // The field specifier describes an anonymous record
                    // (no tag, defined inline). Keep it as a real unnamed
                    // field for layout; typeck recursively promotes the
                    // member names during `.` / `->` lookup.
                    if let Some(inner_spec) = find_anon_record_in_specs(&fd.specs) {
                        let ty =
                            lower_record_spec_to_ty(inner_spec, tcx, resolver, crate_, session);
                        out_fields.push(Field {
                            name: None,
                            ty,
                            quals: object_quals_from_type_quals(&fd.specs.quals),
                            align_override: field_align_override(
                                &fd.specs,
                                &[],
                                DeclScope::Block,
                                None,
                                tcx,
                                resolver,
                                crate_,
                                session,
                            ),
                            offset: None,
                            bit_width: None,
                            span: fd.span,
                        });
                    }
                    // Any other shape (e.g. `int;` with no declarator) is
                    // a malformed field declaration; the parser already
                    // rejects it, so we silently drop here.
                }
                // ── Named field (with or without bit-width). ────────
                (Some(decl), bw) => {
                    let ty = apply_declarator_with_context(
                        base,
                        decl,
                        DeclScope::Block,
                        tcx,
                        resolver,
                        crate_,
                        session,
                    );
                    let name = decl.name.map(|(n, _)| n);
                    let bit_width = if let Some(width_expr) = bw {
                        let (ok, bw_val) = validate_bit_width(
                            ty,
                            width_expr,
                            fd.span,
                            /*is_named=*/ name.is_some(),
                            tcx,
                            resolver,
                            crate_,
                            session,
                        );
                        if !ok {
                            continue;
                        }
                        bw_val
                    } else {
                        None
                    };
                    out_fields.push(Field {
                        name,
                        ty,
                        quals: declaration_object_quals(&fd.specs, decl),
                        align_override: field_align_override(
                            &fd.specs,
                            &decl.attrs,
                            DeclScope::Block,
                            None,
                            tcx,
                            resolver,
                            crate_,
                            session,
                        ),
                        offset: None,
                        bit_width,
                        span: decl.span,
                    });
                }
            }
        }
    }

    DefKind::Record {
        kind,
        packed: packed_attr_present(&spec.attrs, session),
        ms_bitfields: session.opts.ms_bitfields || ms_struct_attr_present(&spec.attrs, session),
        align_override: aligned_attr_override(&spec.attrs, session),
        scalar_storage_order: scalar_storage_order_attr(&spec.attrs, session),
        layout: None,
        fields: out_fields,
    }
}

/// Return the inline anonymous `RecordSpec` inside a field's
/// `DeclSpecs`, if any.
fn find_anon_record_in_specs(specs: &rcc_ast::DeclSpecs) -> Option<&RecordSpec> {
    for ts in &specs.type_specs {
        if let TypeSpec::Record(rs) = ts {
            if rs.tag.is_none() && rs.fields.is_some() {
                return Some(rs);
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn field_align_override(
    specs: &rcc_ast::DeclSpecs,
    declarator_attrs: &[rcc_ast::Attribute],
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<u32> {
    let attr_align = specs
        .attrs
        .iter()
        .chain(declarator_attrs)
        .filter_map(|attr| aligned_attr_value(attr, session))
        .max();
    let mut align = attr_align;
    for spec in &specs.align_specs {
        if let Some(value) =
            align_spec_value(spec, scope, typedef_scope, tcx, resolver, crate_, session)
        {
            align = Some(align.map_or(value, |existing| existing.max(value)));
        }
    }
    align
}

/// Validate a bit-field width expression against the field type.
///
/// Returns `(ok, bit_width)`:
/// - `ok == false` means the field should be dropped (width was
///   invalid; a diagnostic has been emitted).
/// - `bit_width == Some(n)` is the accepted bit-field width for the
///   `Field` record; `None` means the expression failed to evaluate
///   to an integer constant (diagnostic already emitted).
#[allow(clippy::too_many_arguments)]
fn validate_bit_width(
    field_ty: TyId,
    width_expr: &rcc_ast::Expr,
    span: Span,
    is_named: bool,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> (bool, Option<u32>) {
    let value = match eval_bit_width(width_expr, tcx, resolver, crate_, session) {
        Some(v) => v,
        None => {
            session
                .handler
                .struct_err(span, "bit-field width is not an integer constant".to_string())
                .code(rcc_errors::codes::E0077)
                .emit();
            return (false, None);
        }
    };
    if value < 0 {
        session
            .handler
            .struct_err(span, format!("bit-field width cannot be negative (got {value})"))
            .code(rcc_errors::codes::E0077)
            .emit();
        return (false, None);
    }
    // A zero-width bit-field is only legal on an *anonymous* bit-field
    // (C99 §6.7.2.1p4): it forces the next field to be laid out on the
    // next storage-unit boundary. A named zero-width bit-field is a
    // constraint violation.
    if value == 0 && is_named {
        session
            .handler
            .struct_err(span, "named bit-field must have a non-zero width".to_string())
            .code(rcc_errors::codes::E0077)
            .emit();
        return (false, None);
    }
    // Type width check. If the field's type is not a plain integer we
    // skip the upper-bound check (error sentinel or typedef chain that
    // didn't resolve); the base diagnostic would already have fired.
    if let Some(type_bits) = int_type_bit_width(field_ty, tcx) {
        if value > i64::from(type_bits) {
            session
                .handler
                .struct_err(
                    span,
                    format!(
                        "bit-field width {value} exceeds width of underlying type ({type_bits} bits)"
                    ),
                )
                .code(rcc_errors::codes::E0077)
                .emit();
            return (false, None);
        }
    }
    (true, Some(value as u32))
}

/// First-pass: walk the AST top-level and assign a `DefId` to every
/// function definition, global variable, typedef, and struct/union/enum tag.
///
/// Populates `crate_.defs`, `resolver.ordinary`, and `resolver.tags`.
/// Conflict detection is deferred to task 02.
fn assign_def_ids(
    ast: &TranslationUnit,
    tcx: &TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    let mut function_typedefs = FxHashSet::default();

    for ext_decl in &ast.decls {
        match ext_decl {
            ExternalDecl::Function(func_def) => {
                // Function definition — extract name from declarator.
                if let Some((name, _span)) = func_def.declarator.name {
                    let flags = function_decl_flags(&func_def.specs, &func_def.declarator, session);
                    let attrs = lower_common_attrs(&func_def.specs, &func_def.declarator, session);
                    let id = if let Some(existing) = resolver
                        .ordinary
                        .get(&name)
                        .copied()
                        .filter(|id| matches!(crate_.defs[*id].kind, DefKind::Function { .. }))
                    {
                        if let DefKind::Function {
                            has_body,
                            is_static,
                            is_inline,
                            is_extern_inline,
                            no_instrument_function,
                            ..
                        } = &mut crate_.defs[existing].kind
                        {
                            *has_body = true;
                            *is_static = flags.is_static;
                            *is_inline = flags.is_inline;
                            *is_extern_inline = flags.is_extern_inline;
                            *no_instrument_function |= flags.no_instrument_function;
                        }
                        crate_.defs[existing].span = func_def.span;
                        merge_def_attrs(crate_, existing, attrs);
                        existing
                    } else {
                        let id = crate_.defs.push(Def {
                            id: DefId(0), // patched below
                            name,
                            span: func_def.span,
                            kind: DefKind::Function {
                                ty: tcx.error,
                                has_body: true,
                                is_static: flags.is_static,
                                is_inline: flags.is_inline,
                                is_extern_inline: flags.is_extern_inline,
                                no_instrument_function: flags.no_instrument_function,
                                variadic: false,
                            },
                        });
                        crate_.defs[id].id = id;
                        merge_def_attrs(crate_, id, attrs);
                        resolver.ordinary.insert(name, id);
                        id
                    };
                    resolver.ordinary.insert(name, id);
                }
            }
            ExternalDecl::Decl(decl) => {
                let is_typedef = decl.specs.storage == Some(StorageClass::Typedef);
                let mut names_in_decl = FxHashSet::default();

                // Scan type specifiers for tag definitions (struct/union/enum).
                for ts in &decl.specs.type_specs {
                    match ts {
                        TypeSpec::Record(rec) => {
                            if let Some(tag) = rec.tag {
                                let kind = match rec.kind {
                                    rcc_ast::RecordKind::Struct => TagKind::Struct,
                                    rcc_ast::RecordKind::Union => TagKind::Union,
                                };
                                let _ = resolve_tag(
                                    tag, rec.span, kind, crate_, tcx, resolver, session,
                                );
                            }
                        }
                        TypeSpec::Enum(en) => {
                            if let Some(tag) = en.tag {
                                let _ = resolve_tag(
                                    tag,
                                    en.span,
                                    TagKind::Enum,
                                    crate_,
                                    tcx,
                                    resolver,
                                    session,
                                );
                            }
                        }
                        _ => {}
                    }
                }

                // Process each init-declarator.
                for init_decl in &decl.inits {
                    if let Some((name, _span)) = init_decl.declarator.name {
                        let duplicate_in_decl = !names_in_decl.insert(name);
                        let declares_function_type = declaration_base_is_function_type(
                            &decl.specs,
                            &init_decl.declarator,
                            &function_typedefs,
                            resolver,
                            crate_,
                        );
                        let is_function_decl = !is_typedef
                            && (is_file_scope_function_declarator(&init_decl.declarator)
                                || declares_function_type);
                        let is_global_object_decl = !is_typedef && !is_function_decl;
                        if duplicate_in_decl && !is_global_object_decl {
                            emit_duplicate_ordinary(name, decl.span, session);
                        }
                        if is_typedef {
                            let is_function_typedef =
                                is_file_scope_function_declarator(&init_decl.declarator)
                                    || declares_function_type;
                            let id = crate_.defs.push(Def {
                                id: DefId(0),
                                name,
                                span: decl.span,
                                kind: DefKind::Typedef(tcx.error),
                            });
                            crate_.defs[id].id = id;
                            // Preserve the earliest file-scope typedef binding in the
                            // resolver: cross-decl redefinitions (e.g. stdint.h followed
                            // by netinet/in.h both providing `typedef ... uint8_t`) must
                            // not invalidate uses of the name made between the two
                            // declarations. Pass 2 fills typedef slots in source order,
                            // so uses interleaved with later redefs would otherwise
                            // resolve to a still-unfinalised slot (Ty::Error).
                            let existing_typedef = !duplicate_in_decl
                                && resolver.ordinary.get(&name).is_some_and(|&existing| {
                                    matches!(crate_.defs[existing].kind, DefKind::Typedef(_))
                                });
                            if !duplicate_in_decl && !existing_typedef {
                                resolver.ordinary.insert(name, id);
                            }
                            if is_function_typedef {
                                function_typedefs.insert(name);
                            }
                        } else if is_function_decl {
                            let flags =
                                function_decl_flags(&decl.specs, &init_decl.declarator, session);
                            let attrs =
                                lower_common_attrs(&decl.specs, &init_decl.declarator, session);
                            let id = if let Some(existing) =
                                resolver.ordinary.get(&name).copied().filter(|id| {
                                    matches!(crate_.defs[*id].kind, DefKind::Function { .. })
                                }) {
                                if let DefKind::Function {
                                    no_instrument_function,
                                    is_static,
                                    is_inline,
                                    is_extern_inline,
                                    ..
                                } = &mut crate_.defs[existing].kind
                                {
                                    *no_instrument_function |= flags.no_instrument_function;
                                    *is_static = flags.is_static;
                                    *is_inline = flags.is_inline;
                                    *is_extern_inline = flags.is_extern_inline;
                                }
                                merge_def_attrs(crate_, existing, attrs);
                                existing
                            } else {
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name,
                                    span: decl.span,
                                    kind: DefKind::Function {
                                        ty: tcx.error,
                                        has_body: false,
                                        is_static: flags.is_static,
                                        is_inline: flags.is_inline,
                                        is_extern_inline: flags.is_extern_inline,
                                        no_instrument_function: flags.no_instrument_function,
                                        variadic: false,
                                    },
                                });
                                crate_.defs[id].id = id;
                                merge_def_attrs(crate_, id, attrs);
                                id
                            };
                            if !duplicate_in_decl {
                                resolver.ordinary.insert(name, id);
                            }
                        } else {
                            // Global variable (or extern declaration).
                            let attrs =
                                lower_common_attrs(&decl.specs, &init_decl.declarator, session);
                            let linkage = match decl.specs.storage {
                                Some(StorageClass::Static) => Linkage::Internal,
                                Some(StorageClass::Extern) => Linkage::External,
                                _ => Linkage::External,
                            };
                            let id = if let Some(existing) =
                                resolver.ordinary.get(&name).copied().filter(|id| {
                                    matches!(crate_.defs[*id].kind, DefKind::Global { .. })
                                }) {
                                merge_def_attrs(crate_, existing, attrs);
                                existing
                            } else {
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name,
                                    span: decl.span,
                                    kind: DefKind::Global {
                                        ty: tcx.error,
                                        quals: declaration_object_quals(
                                            &decl.specs,
                                            &init_decl.declarator,
                                        ),
                                        thread_local: decl.specs.thread_local,
                                        linkage,
                                        init: None,
                                    },
                                });
                                crate_.defs[id].id = id;
                                merge_def_attrs(crate_, id, attrs);
                                id
                            };
                            if !duplicate_in_decl || is_global_object_decl {
                                resolver.ordinary.insert(name, id);
                            }
                        }
                    }
                }
            }
            ExternalDecl::StaticAssert(_) => {}
        }
    }
}

#[derive(Copy, Clone)]
struct FunctionDeclFlags {
    is_static: bool,
    is_inline: bool,
    is_extern_inline: bool,
    no_instrument_function: bool,
}

fn function_decl_flags(
    specs: &rcc_ast::DeclSpecs,
    declarator: &rcc_ast::Declarator,
    session: &Session,
) -> FunctionDeclFlags {
    let is_inline = specs.func_specs.inline;
    let is_static = specs.storage == Some(StorageClass::Static);
    let is_extern_inline = is_inline && specs.storage == Some(StorageClass::Extern);
    let no_instrument_function = attrs_contain_no_instrument_function(&specs.attrs, session)
        || attrs_contain_no_instrument_function(&declarator.attrs, session);
    FunctionDeclFlags { is_static, is_inline, is_extern_inline, no_instrument_function }
}

fn attrs_contain_no_instrument_function(attrs: &[rcc_ast::Attribute], session: &Session) -> bool {
    attrs.iter().any(|attr| {
        matches!(
            session.interner.get(attr.name),
            "no_instrument_function" | "__no_instrument_function__"
        )
    })
}

fn lower_common_attrs(
    specs: &rcc_ast::DeclSpecs,
    declarator: &rcc_ast::Declarator,
    session: &mut Session,
) -> CommonAttrs {
    let mut out = CommonAttrs::default();
    if specs.func_specs.noreturn {
        out.noreturn = true;
    }
    for attr in specs.attrs.iter().chain(&declarator.attrs) {
        lower_common_attr(attr, session, &mut out);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn lower_common_attrs_with_align(
    specs: &rcc_ast::DeclSpecs,
    declarator: &rcc_ast::Declarator,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> CommonAttrs {
    let mut out = lower_common_attrs(specs, declarator, session);
    for spec in &specs.align_specs {
        if let Some(align) =
            align_spec_value(spec, scope, typedef_scope, tcx, resolver, crate_, session)
        {
            out.align_override =
                Some(out.align_override.map_or(align, |existing| existing.max(align)));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn align_spec_value(
    spec: &AlignSpec,
    scope: DeclScope,
    typedef_scope: Option<&ScopeStack>,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) -> Option<u32> {
    let value = match &spec.kind {
        AlignSpecKind::Type(type_name) => {
            let ty = lower_type_name_in_scope(
                type_name,
                scope,
                typedef_scope,
                tcx,
                resolver,
                crate_,
                session,
            );
            let layout = match LayoutCx::with_defs(tcx, &crate_.defs).layout_of(ty) {
                Ok(layout) => layout,
                Err(err) => {
                    session
                        .handler
                        .struct_err(
                            spec.span,
                            format!("cannot apply `_Alignas` to type without layout: {err}"),
                        )
                        .code(rcc_errors::codes::E0061)
                        .emit();
                    return None;
                }
            };
            i128::from(layout.align)
        }
        AlignSpecKind::Expr(expr) => {
            match eval_array_bound_as_i128(
                expr,
                scope,
                typedef_scope,
                None,
                tcx,
                resolver,
                crate_,
                session,
            ) {
                Some(value) => value,
                None => {
                    session
                        .handler
                        .struct_err(
                            spec.span,
                            "`_Alignas` requires an integer constant expression".to_owned(),
                        )
                        .code(rcc_errors::codes::E0061)
                        .emit();
                    return None;
                }
            }
        }
    };
    validate_alignas_value(value, spec.span, session)
}

fn validate_alignas_value(value: i128, span: Span, session: &mut Session) -> Option<u32> {
    let Ok(value) = u32::try_from(value) else {
        session
            .handler
            .struct_err(span, format!("invalid `_Alignas` alignment {value}"))
            .code(rcc_errors::codes::E0061)
            .emit();
        return None;
    };
    if value == 0 {
        return None;
    }
    if !value.is_power_of_two() {
        session
            .handler
            .struct_err(
                span,
                format!("invalid `_Alignas` alignment {value}: expected a power of two"),
            )
            .code(rcc_errors::codes::E0061)
            .emit();
        return None;
    }
    Some(value)
}

fn merge_def_attrs(crate_: &mut HirCrate, def: DefId, attrs: CommonAttrs) {
    if attrs.is_empty() {
        return;
    }
    crate_.def_attrs.entry(def).or_default().merge(attrs);
}

fn merge_local_attrs(body: &mut Body, local: Local, attrs: CommonAttrs) {
    if attrs.is_empty() {
        return;
    }
    body.local_attrs.entry(local).or_default().merge(attrs);
}

fn lower_common_attr(attr: &rcc_ast::Attribute, session: &mut Session, out: &mut CommonAttrs) {
    let name = session.interner.get(attr.name).to_owned();
    if attr_name_eq(&name, "noreturn") {
        if expect_attr_arg_count(attr, 0, session) {
            out.noreturn = true;
        }
    } else if attr_name_eq(&name, "unused") {
        if expect_attr_arg_count(attr, 0, session) {
            out.unused = true;
        }
    } else if attr_name_eq(&name, "deprecated") {
        if expect_attr_arg_count(attr, 0, session) {
            out.deprecated = true;
        }
    } else if attr_name_eq(&name, "weak") {
        if expect_attr_arg_count(attr, 0, session) {
            out.weak = true;
        }
    } else if attr_name_eq(&name, "visibility") {
        if let Some(value) = attr_single_string_arg(attr, session) {
            match value.as_slice() {
                b"default" => out.visibility = Some(SymbolVisibility::Default),
                b"hidden" => out.visibility = Some(SymbolVisibility::Hidden),
                _ => emit_invalid_common_attr(
                    attr.span,
                    "visibility attribute expects \"default\" or \"hidden\"",
                    session,
                ),
            }
        }
    } else if attr_name_eq(&name, "section") {
        if let Some(value) = attr_single_string_arg(attr, session) {
            match std::str::from_utf8(&value) {
                Ok(section) if !section.is_empty() && !section.as_bytes().contains(&0) => {
                    out.section = Some(session.interner.intern(section));
                }
                _ => emit_invalid_common_attr(
                    attr.span,
                    "section attribute expects a non-empty string literal without NUL bytes",
                    session,
                ),
            }
        }
    }
}

fn attr_name_eq(name: &str, expected: &str) -> bool {
    name == expected
        || (name.starts_with("__")
            && name.ends_with("__")
            && name.len() == expected.len() + 4
            && &name[2..name.len() - 2] == expected)
        || (name.ends_with("__")
            && name.len() == expected.len() + 2
            && &name[..name.len() - 2] == expected)
}

fn expect_attr_arg_count(
    attr: &rcc_ast::Attribute,
    expected: usize,
    session: &mut Session,
) -> bool {
    if attr.args.len() == expected {
        return true;
    }
    emit_invalid_common_attr(
        attr.span,
        &format!("attribute expects {expected} argument(s)"),
        session,
    );
    false
}

fn attr_single_string_arg(attr: &rcc_ast::Attribute, session: &mut Session) -> Option<Vec<u8>> {
    if !expect_attr_arg_count(attr, 1, session) {
        return None;
    }
    let [arg] = attr.args.as_slice() else {
        return None;
    };
    let [token] = arg.tokens.as_slice() else {
        emit_invalid_common_attr(attr.span, "attribute expects one string literal", session);
        return None;
    };
    let rcc_ast::AttributeTokenKind::String(bytes) = &token.kind else {
        emit_invalid_common_attr(attr.span, "attribute expects one string literal", session);
        return None;
    };
    Some(bytes.clone())
}

fn emit_invalid_common_attr(span: Span, message: &str, session: &mut Session) {
    session.handler.struct_err(span, message).code(rcc_errors::codes::E0061).emit();
}

fn is_file_scope_function_declarator(declarator: &Declarator) -> bool {
    matches!(declarator.derived.last(), Some(DerivedDeclarator::Function(_)))
}

fn declaration_base_is_function_type(
    specs: &rcc_ast::DeclSpecs,
    declarator: &Declarator,
    function_typedefs: &FxHashSet<Symbol>,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    if !declarator.derived.is_empty() {
        return false;
    }
    specs_name_function_type(specs, function_typedefs, resolver, crate_)
}

fn specs_name_function_type(
    specs: &rcc_ast::DeclSpecs,
    function_typedefs: &FxHashSet<Symbol>,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    specs.type_specs.iter().any(|spec| match spec {
        TypeSpec::TypedefName(sym) => function_typedefs.contains(sym),
        TypeSpec::TypeofExpr(expr) => typeof_expr_names_function(expr, resolver, crate_),
        TypeSpec::TypeofType(ty) | TypeSpec::Atomic(ty) => {
            type_name_names_function_type(ty, function_typedefs, resolver, crate_)
        }
        _ => false,
    })
}

fn type_name_names_function_type(
    ty: &rcc_ast::TypeName,
    function_typedefs: &FxHashSet<Symbol>,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    is_file_scope_function_declarator(&ty.declarator)
        || declaration_base_is_function_type(
            &ty.specs,
            &ty.declarator,
            function_typedefs,
            resolver,
            crate_,
        )
}

fn typeof_expr_names_function(
    expr: &rcc_ast::Expr,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    let rcc_ast::ExprKind::Ident(sym) = &peel_ast_parens(expr).kind else {
        return false;
    };
    resolver
        .ordinary
        .get(sym)
        .is_some_and(|def| matches!(crate_.defs[*def].kind, DefKind::Function { .. }))
}

fn finalize_file_scope_tag_definitions(
    ast: &TranslationUnit,
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    for ext_decl in &ast.decls {
        match ext_decl {
            ExternalDecl::Decl(decl) if decl.inits.is_empty() => {
                materialize_tag_definitions_in_specs(&decl.specs, tcx, resolver, crate_, session);
            }
            _ => {}
        }
    }
}

fn materialize_tag_definitions_in_specs(
    specs: &rcc_ast::DeclSpecs,
    tcx: &mut TyCtxt,
    resolver: &mut Resolver,
    crate_: &mut HirCrate,
    session: &mut Session,
) {
    for ts in &specs.type_specs {
        match ts {
            TypeSpec::Record(spec) if spec.fields.is_some() => {
                let ty = lower_record_spec_to_ty(spec, tcx, resolver, crate_, session);
                apply_aligned_attr_override_to_record(ty, &specs.attrs, tcx, crate_, session);
                apply_scalar_storage_order_attr_to_record(ty, &specs.attrs, tcx, crate_, session);
            }
            TypeSpec::Enum(spec) if spec.enumerators.is_some() => {
                let _ = lower_enum_spec_to_ty(spec, tcx, resolver, crate_, session);
            }
            _ => {}
        }
    }
}

fn finalize_file_scope_typedef_def_types(
    ast: &TranslationUnit,
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    let mut seen_ordinary_defs = FxHashMap::default();

    for ext_decl in &ast.decls {
        let ExternalDecl::Decl(decl) = ext_decl else { continue };
        if decl.specs.storage != Some(StorageClass::Typedef) {
            continue;
        }

        for init_decl in &decl.inits {
            let Some((name, _span)) = init_decl.declarator.name else {
                continue;
            };
            let ordinal = seen_file_scope_ordinary_def(&mut seen_ordinary_defs, name, true);
            let Some(def_id) = find_file_scope_ordinary_def(crate_, name, true, ordinal) else {
                continue;
            };
            if matches!(&crate_.defs[def_id].kind, DefKind::Typedef(ty) if *ty != tcx.error) {
                continue;
            }
            if typedef_array_bound_needs_source_order(init_decl, tcx, resolver, crate_) {
                continue;
            }
            let ty = lower_type_from_parts(
                &decl.specs,
                &init_decl.declarator,
                DeclScope::File,
                tcx,
                resolver,
                crate_,
                session,
            );
            if let DefKind::Typedef(slot) = &mut crate_.defs[def_id].kind {
                *slot = ty;
            }
            if ordinal > 0 {
                if let Some(prev_def_id) =
                    find_file_scope_ordinary_def(crate_, name, true, ordinal - 1)
                {
                    let prev_ty = match crate_.defs[prev_def_id].kind {
                        DefKind::Typedef(prev_ty) => prev_ty,
                        _ => tcx.error,
                    };
                    if prev_ty != tcx.error
                        && ty != tcx.error
                        && !typedef_types_are_compatible(tcx, prev_ty, ty)
                    {
                        emit_incompatible_typedef_redefinition(
                            name,
                            init_decl.declarator.span,
                            prev_def_id,
                            crate_,
                            session,
                        );
                    }
                }
            }
        }
    }
}

fn typedef_types_are_compatible(tcx: &TyCtxt, a: TyId, b: TyId) -> bool {
    strip_atomic_ty(tcx, a) == strip_atomic_ty(tcx, b)
}

fn strip_atomic_ty(tcx: &TyCtxt, mut ty: TyId) -> TyId {
    while let Ty::Atomic(inner) = *tcx.get(ty) {
        ty = inner;
    }
    ty
}

fn emit_incompatible_typedef_redefinition(
    name: Symbol,
    span: Span,
    prev_def_id: DefId,
    crate_: &HirCrate,
    session: &mut Session,
) {
    let name_str = session.interner.get(name);
    let mut diag = session
        .handler
        .struct_err(span, format!("incompatible typedef redefinition of `{name_str}`"));
    diag = diag.code(rcc_errors::codes::E0078);
    diag = diag.label(span, "this typedef names a different type");
    if let Some(prev) = crate_.defs.get(prev_def_id) {
        diag = diag.label(prev.span, "previous typedef with the same name is here");
    }
    diag.emit();
}

fn typedef_array_bound_needs_source_order(
    init_decl: &rcc_ast::InitDeclarator,
    tcx: &TyCtxt,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    init_decl.declarator.derived.iter().any(|derived| match derived {
        DerivedDeclarator::Array(array) => array
            .size
            .as_ref()
            .is_some_and(|size| expr_sizeof_unfinalized_global(size, tcx, resolver, crate_)),
        _ => false,
    })
}

fn expr_sizeof_unfinalized_global(
    expr: &rcc_ast::Expr,
    tcx: &TyCtxt,
    resolver: &Resolver,
    crate_: &HirCrate,
) -> bool {
    match &expr.kind {
        rcc_ast::ExprKind::SizeofExpr(operand) | rcc_ast::ExprKind::AlignofExpr(operand) => {
            let operand = peel_ast_parens(operand);
            if let rcc_ast::ExprKind::Ident(sym) = operand.kind {
                if let Some(&def) = resolver.ordinary.get(&sym) {
                    return matches!(
                        crate_.defs[def].kind,
                        DefKind::Global { ty, .. } if ty == tcx.error
                    );
                }
            }
            expr_sizeof_unfinalized_global(operand, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::Paren(inner)
        | rcc_ast::ExprKind::Unary { operand: inner, .. }
        | rcc_ast::ExprKind::Cast { expr: inner, .. } => {
            expr_sizeof_unfinalized_global(inner, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::Binary { lhs, rhs, .. }
        | rcc_ast::ExprKind::Assign { lhs, rhs, .. }
        | rcc_ast::ExprKind::Comma { lhs, rhs }
        | rcc_ast::ExprKind::Index { base: lhs, index: rhs } => {
            expr_sizeof_unfinalized_global(lhs, tcx, resolver, crate_)
                || expr_sizeof_unfinalized_global(rhs, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::Cond { cond, then_expr, else_expr } => {
            expr_sizeof_unfinalized_global(cond, tcx, resolver, crate_)
                || expr_sizeof_unfinalized_global(then_expr, tcx, resolver, crate_)
                || expr_sizeof_unfinalized_global(else_expr, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::OmittedCond { cond, else_expr } => {
            expr_sizeof_unfinalized_global(cond, tcx, resolver, crate_)
                || expr_sizeof_unfinalized_global(else_expr, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::Call { callee, args } => {
            expr_sizeof_unfinalized_global(callee, tcx, resolver, crate_)
                || args.iter().any(|arg| expr_sizeof_unfinalized_global(arg, tcx, resolver, crate_))
        }
        rcc_ast::ExprKind::Member { base, .. } | rcc_ast::ExprKind::Arrow { base, .. } => {
            expr_sizeof_unfinalized_global(base, tcx, resolver, crate_)
        }
        rcc_ast::ExprKind::GenericSelection { control, associations } => {
            expr_sizeof_unfinalized_global(control, tcx, resolver, crate_)
                || associations
                    .iter()
                    .any(|assoc| expr_sizeof_unfinalized_global(&assoc.expr, tcx, resolver, crate_))
        }
        _ => false,
    }
}

fn finalize_file_scope_def_types(
    ast: &TranslationUnit,
    tcx: &mut TyCtxt,
    session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    let mut seen_ordinary_defs = FxHashMap::default();

    for ext_decl in &ast.decls {
        let ExternalDecl::Decl(decl) = ext_decl else {
            continue;
        };
        let is_typedef = decl.specs.storage == Some(StorageClass::Typedef);

        for init_decl in &decl.inits {
            let Some((name, _span)) = init_decl.declarator.name else {
                continue;
            };
            let is_function_decl =
                !is_typedef && is_file_scope_function_declarator(&init_decl.declarator);
            let def_id = if is_function_decl {
                let Some(def_id) = find_file_scope_function_def(crate_, name) else {
                    continue;
                };
                def_id
            } else if !is_typedef {
                let Some(def_id) = resolver
                    .ordinary
                    .get(&name)
                    .copied()
                    .filter(|id| matches!(crate_.defs[*id].kind, DefKind::Global { .. }))
                else {
                    continue;
                };
                def_id
            } else {
                let ordinal =
                    seen_file_scope_ordinary_def(&mut seen_ordinary_defs, name, is_typedef);
                let Some(def_id) = find_file_scope_ordinary_def(crate_, name, is_typedef, ordinal)
                else {
                    continue;
                };
                if matches!(&crate_.defs[def_id].kind, DefKind::Typedef(ty) if *ty != tcx.error) {
                    continue;
                }
                def_id
            };

            let mut ty = lower_type_from_parts(
                &decl.specs,
                &init_decl.declarator,
                DeclScope::File,
                tcx,
                resolver,
                crate_,
                session,
            );
            if let Some(init) = &init_decl.init {
                ty = complete_initializer_type(ty, init, tcx, crate_);
            }
            let attrs = lower_common_attrs_with_align(
                &decl.specs,
                &init_decl.declarator,
                DeclScope::File,
                None,
                tcx,
                resolver,
                crate_,
                session,
            );
            merge_def_attrs(crate_, def_id, attrs);
            let has_explicit_init = init_decl.init.is_some();
            let global_init = if let Some(init) = &init_decl.init {
                let mut init_body = Body::default();
                let scope = ScopeStack::new();
                let global_init = lower_global_initializer(
                    ty,
                    init,
                    init_decl.declarator.span,
                    &mut init_body,
                    &scope,
                    crate_,
                    tcx,
                    resolver,
                    session,
                );
                if !init_body.exprs.is_empty() {
                    crate_.global_init_bodies.insert(def_id, init_body);
                }
                Some(global_init)
            } else if decl.specs.storage != Some(StorageClass::Extern)
                && matches!(crate_.defs[def_id].kind, DefKind::Global { .. })
            {
                // C99 §6.9.2: a file-scope declaration without `extern`
                // and without an initializer is a tentative definition.
                // If no real initializer appears, it behaves as though it
                // had a zero initializer at the end of the translation unit.
                Some(GlobalInit { ty, entries: Vec::new() })
            } else {
                None
            };

            match &mut crate_.defs[def_id].kind {
                DefKind::Typedef(slot) => *slot = ty,
                DefKind::Function {
                    ty: slot,
                    has_body,
                    is_static,
                    is_inline,
                    is_extern_inline,
                    no_instrument_function,
                    variadic,
                } => {
                    *slot = ty;
                    *variadic = match tcx.get(ty) {
                        Ty::Func { variadic, .. } => *variadic,
                        _ => false,
                    };
                    if !*has_body {
                        let flags =
                            function_decl_flags(&decl.specs, &init_decl.declarator, session);
                        *is_static = flags.is_static;
                        *is_inline = flags.is_inline;
                        *is_extern_inline = flags.is_extern_inline;
                        *no_instrument_function |= flags.no_instrument_function;
                    }
                }
                DefKind::Global { ty: slot, quals, thread_local, init, .. } => {
                    *slot = ty;
                    *quals = declaration_object_quals(&decl.specs, &init_decl.declarator);
                    *thread_local = decl.specs.thread_local;
                    if has_explicit_init || init.is_none() {
                        *init = global_init;
                    }
                }
                _ => {}
            }
        }
    }
}

fn seen_file_scope_ordinary_def(
    seen: &mut FxHashMap<(Symbol, bool), usize>,
    name: Symbol,
    is_typedef: bool,
) -> usize {
    let entry = seen.entry((name, is_typedef)).or_insert(0);
    let ordinal = *entry;
    *entry += 1;
    ordinal
}

fn find_file_scope_ordinary_def(
    crate_: &HirCrate,
    name: Symbol,
    is_typedef: bool,
    ordinal: usize,
) -> Option<DefId> {
    let mut seen = 0;
    for (id, def) in crate_.defs.iter_enumerated() {
        if def.name != name || !matches_file_scope_ordinary_kind(&def.kind, is_typedef) {
            continue;
        }
        if seen == ordinal {
            return Some(id);
        }
        seen += 1;
    }
    None
}

fn find_file_scope_function_def(crate_: &HirCrate, name: Symbol) -> Option<DefId> {
    crate_.defs.iter_enumerated().find_map(|(id, def)| {
        (def.name == name && matches!(def.kind, DefKind::Function { .. })).then_some(id)
    })
}

fn matches_file_scope_ordinary_kind(kind: &DefKind, is_typedef: bool) -> bool {
    matches!((kind, is_typedef), (DefKind::Typedef(_), true) | (DefKind::Global { .. }, false))
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
        Declarator {
            name: Some((name, DUMMY_SP)),
            derived: Vec::new(),
            span: DUMMY_SP,
            attrs: Vec::new(),
        }
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
                    static_asserts: Vec::new(),
                    span: DUMMY_SP,
                    attrs: Vec::new(),
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
                    attrs: Vec::new(),
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
    fn file_scope_typedef_placeholder_is_finalized() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "uint32");
        let ast = TranslationUnit { decls: vec![make_typedef(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Typedef(ty) if ty == tcx.int));
    }

    #[test]
    fn forward_typedef_name_stays_error() {
        let (mut sess, cap) = Session::for_test();
        let t = sym(&mut sess, "T");
        let u = sym(&mut sess, "U");
        let ast = TranslationUnit {
            decls: vec![ExternalDecl::Decl(Decl {
                id: NodeId(0),
                span: DUMMY_SP,
                specs: {
                    let mut s = default_specs();
                    s.storage = Some(StorageClass::Typedef);
                    s.type_specs.push(TypeSpec::TypedefName(t));
                    s
                },
                inits: vec![InitDeclarator { declarator: named_declarator(u), init: None }],
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Typedef(ty) if ty == tcx.error));
        assert!(
            cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
            "unresolved typedef-name should emit an error"
        );
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
                        static_asserts: Vec::new(),
                        span: DUMMY_SP,
                        attrs: Vec::new(),
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
    fn bare_struct_ref_creates_incomplete_def() {
        // `struct S;` (forward declaration, no field body) creates one incomplete tag def.
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
                        static_asserts: Vec::new(),
                        span: DUMMY_SP,
                        attrs: Vec::new(),
                    }));
                    s
                },
                inits: Vec::new(),
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1, "bare struct ref should create one incomplete def");
        assert!(matches!(
            hir.defs[DefId(0)].kind,
            DefKind::Record { kind: RecordKind::Struct, ref fields, .. } if fields.is_empty()
        ));
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

    #[test]
    fn labels_inside_statement_expression_are_visible_to_gotos_in_the_same_function() {
        // void f(){ ({ goto x; x:; }); }
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let stmt_expr = rcc_ast::Expr {
            id: NodeId(0),
            kind: ExprKind::StmtExpr(Box::new(make_block(vec![
                make_goto(x),
                make_label(x, make_null()),
            ]))),
            span: DUMMY_SP,
        };
        let body = make_block(vec![make_stmt(StmtKind::Expr(Some(stmt_expr)))]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "statement-expression labels should resolve: {diags:?}");
        assert!(resolver.labels.contains_key(&x));
    }

    #[test]
    fn missing_label_inside_statement_expression_emits_e0073() {
        // void f(){ ({ goto missing; }); }
        let (mut sess, cap) = Session::for_test();
        let missing = sym(&mut sess, "missing");
        let stmt_expr = rcc_ast::Expr {
            id: NodeId(0),
            kind: ExprKind::StmtExpr(Box::new(make_block(vec![make_goto(missing)]))),
            span: DUMMY_SP,
        };
        let body = make_block(vec![make_stmt(StmtKind::Expr(Some(stmt_expr)))]);

        let mut resolver = Resolver::default();
        resolve_labels(&body, &mut resolver, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0073"));
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
            kind: DefKind::Global {
                ty: tcx.int,
                quals: ObjectQuals::none(),
                thread_local: false,
                linkage: Linkage::External,
                init: None,
            },
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
        Declarator { name: Some((name, DUMMY_SP)), derived, span: DUMMY_SP, attrs: Vec::new() }
    }

    /// Helper: make a pointer derived declarator with no qualifiers.
    fn ptr() -> DerivedDeclarator {
        DerivedDeclarator::Pointer(TypeQuals::default())
    }

    /// Helper: make a pointer derived declarator with const qualifier.
    fn const_ptr() -> DerivedDeclarator {
        DerivedDeclarator::Pointer(TypeQuals {
            const_: true,
            volatile: false,
            restrict: false,
            atomic: false,
        })
    }

    /// Helper: make an array derived declarator with a constant size.
    fn array(size: u64, sess: &mut Session) -> DerivedDeclarator {
        DerivedDeclarator::Array(ArrayDeclarator {
            quals: TypeQuals::default(),
            has_static: false,
            star: false,
            size: Some(int_lit(&size.to_string(), sess)),
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
            declarator: Declarator {
                name: None,
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            },
            span: DUMMY_SP,
        }
    }

    // ── Central type-spec service (task 06-14) tests ───────────────────

    #[test]
    fn type_service_typedef_name_lowers_to_alias() {
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let t = sym(&mut sess, "T");
        let x = sym(&mut sess, "x");
        register_typedef(t, tcx.long, &mut crate_, &mut resolver);

        let specs =
            DeclSpecs { type_specs: vec![TypeSpec::TypedefName(t)], ..DeclSpecs::default() };
        let d = make_declarator(x, Vec::new());
        let ty = lower_type_from_parts(
            &specs,
            &d,
            DeclScope::Block,
            &mut tcx,
            &mut resolver,
            &mut crate_,
            &mut sess,
        );

        assert_eq!(ty, tcx.long);
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn type_service_record_spec_returns_record_ty() {
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let s = sym(&mut sess, "S");
        let a = sym(&mut sess, "a");
        let obj = sym(&mut sess, "obj");
        let rec = record_spec(
            rcc_ast::RecordKind::Struct,
            Some(s),
            Some(vec![named_field(a, vec![TypeSpec::Int])]),
        );
        let specs = DeclSpecs { type_specs: vec![TypeSpec::Record(rec)], ..DeclSpecs::default() };
        let d = make_declarator(obj, Vec::new());

        let ty = lower_type_from_parts(
            &specs,
            &d,
            DeclScope::Block,
            &mut tcx,
            &mut resolver,
            &mut crate_,
            &mut sess,
        );

        let Ty::Record(def_id) = *tcx.get(ty) else {
            panic!("expected record type, got {:?}", tcx.get(ty));
        };
        match &crate_.defs[def_id].kind {
            DefKind::Record { kind: RecordKind::Struct, fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, Some(a));
                assert_eq!(fields[0].ty, tcx.int);
            }
            other => panic!("expected record def, got {other:?}"),
        }
        assert_eq!(resolver.tags.get(&s).copied(), Some(def_id));
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn type_service_enum_spec_returns_enum_ty() {
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let e = sym(&mut sess, "E");
        let a = sym(&mut sess, "A");
        let obj = sym(&mut sess, "obj");
        let en = enum_spec(Some(e), Some(vec![(a, None)]));
        let specs = DeclSpecs { type_specs: vec![TypeSpec::Enum(en)], ..DeclSpecs::default() };
        let d = make_declarator(obj, Vec::new());

        let ty = lower_type_from_parts(
            &specs,
            &d,
            DeclScope::Block,
            &mut tcx,
            &mut resolver,
            &mut crate_,
            &mut sess,
        );

        let Ty::Enum(def_id) = *tcx.get(ty) else {
            panic!("expected enum type, got {:?}", tcx.get(ty));
        };
        match &crate_.defs[def_id].kind {
            DefKind::Enum { repr, variants } => {
                assert_eq!(*repr, tcx.int);
                assert_eq!(variants.len(), 1);
                assert_eq!(variants[0].name, a);
            }
            other => panic!("expected enum def, got {other:?}"),
        }
        assert_eq!(resolver.tags.get(&e).copied(), Some(def_id));
        assert!(resolver.ordinary.contains_key(&a), "enumerator must enter ordinary namespace");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn type_service_unsigned_long_pointer_still_lowers() {
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let p = sym(&mut sess, "p");
        let specs = DeclSpecs {
            type_specs: vec![TypeSpec::Unsigned, TypeSpec::Long],
            ..DeclSpecs::default()
        };
        let d = make_declarator(p, vec![ptr()]);

        let ty = lower_type_from_parts(
            &specs,
            &d,
            DeclScope::Block,
            &mut tcx,
            &mut resolver,
            &mut crate_,
            &mut sess,
        );

        let expected = tcx.intern(Ty::Ptr(Qual::plain(tcx.ulong)));
        assert_eq!(ty, expected);
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn type_service_imaginary_is_error_not_int() {
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let x = sym(&mut sess, "x");
        let specs = DeclSpecs { type_specs: vec![TypeSpec::Imaginary], ..DeclSpecs::default() };
        let d = make_declarator(x, Vec::new());

        let ty = lower_type_from_parts(
            &specs,
            &d,
            DeclScope::Block,
            &mut tcx,
            &mut resolver,
            &mut crate_,
            &mut sess,
        );

        assert_eq!(ty, tcx.error);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0061));
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
        // `int * const cp;` means cp is a const pointer to int. The
        // const qualifies the pointer object, not the pointee; the full
        // declaration lowering path records that in `ObjectQuals`.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let cp = sym(&mut sess, "cp");
        let d = make_declarator(cp, vec![const_ptr()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
        let expected = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
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
    fn declarator_incomplete_array_block_scope_deferred_to_decl_lowering() {
        // The declarator fold cannot know whether an initializer will
        // complete `int arr[]`, so it preserves the incomplete array. The
        // block-declaration lowering layer emits E0076 if no initializer
        // completes it.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let arr = sym(&mut sess, "arr");
        let d = make_declarator(arr, vec![incomplete_array()]);
        let result = apply_declarator(tcx.int, &d, DeclScope::Block, &mut tcx, &mut sess);
        let expected =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: false });
        assert_eq!(result, expected);
        assert!(cap.diagnostics().is_empty());
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
                attrs: Vec::new(),
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

    // ── Composite lowering (task 06-07) tests ──────────────────────────

    use rcc_ast::{FieldDecl, FieldDeclarator};

    /// Helper: build a `FieldDecl` from a single type spec list and a
    /// vector of `(name, derived, bit_width)` tuples.
    fn field_decl(
        type_specs: Vec<TypeSpec>,
        decls: Vec<(Option<Symbol>, Vec<DerivedDeclarator>, Option<Expr>)>,
    ) -> FieldDecl {
        FieldDecl {
            specs: DeclSpecs { type_specs, ..DeclSpecs::default() },
            declarators: decls
                .into_iter()
                .map(|(name, derived, bit_width)| FieldDeclarator {
                    declarator: name.map(|n| Declarator {
                        name: Some((n, DUMMY_SP)),
                        derived,
                        span: DUMMY_SP,
                        attrs: Vec::new(),
                    }),
                    bit_width,
                })
                .collect(),
            span: DUMMY_SP,
        }
    }

    /// Helper: build a named field with a given type spec, no derivations.
    fn named_field(name: Symbol, type_specs: Vec<TypeSpec>) -> FieldDecl {
        field_decl(type_specs, vec![(Some(name), Vec::new(), None)])
    }

    /// Helper: int literal constant expression with given text.
    fn int_lit(text: &str, sess: &mut Session) -> Expr {
        let s = sym(sess, text);
        let value = parse_int_lit_value(text);
        Expr {
            id: NodeId(0),
            kind: ExprKind::IntLit(rcc_ast::IntLiteral {
                text: s,
                value,
                base: parse_int_lit_base(text),
                suffix: rcc_ast::IntSuffix::None,
            }),
            span: DUMMY_SP,
        }
    }

    fn parse_int_lit_value(text: &str) -> u128 {
        let s = text.trim_end_matches(['u', 'U', 'l', 'L']);
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u128::from_str_radix(hex, 16).unwrap()
        } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
            u128::from_str_radix(bin, 2).unwrap()
        } else if s.starts_with('0') && s.len() > 1 {
            u128::from_str_radix(s, 8).unwrap()
        } else {
            s.parse::<u128>().unwrap()
        }
    }

    fn parse_int_lit_base(text: &str) -> rcc_ast::IntBase {
        let s = text.trim_end_matches(['u', 'U', 'l', 'L']);
        if s.starts_with("0x") || s.starts_with("0X") {
            rcc_ast::IntBase::Hex
        } else if s.starts_with("0b") || s.starts_with("0B") {
            rcc_ast::IntBase::Binary
        } else if s.starts_with('0') {
            rcc_ast::IntBase::Octal
        } else {
            rcc_ast::IntBase::Decimal
        }
    }

    /// Helper: unary `-n` constant expression.
    fn neg_lit(text: &str, sess: &mut Session) -> Expr {
        Expr {
            id: NodeId(0),
            kind: ExprKind::Unary {
                op: rcc_ast::UnOp::Neg,
                operand: Box::new(int_lit(text, sess)),
            },
            span: DUMMY_SP,
        }
    }

    /// Helper: build a `RecordSpec` from a list of field decls.
    fn record_spec(
        kind: rcc_ast::RecordKind,
        tag: Option<Symbol>,
        fields: Option<Vec<FieldDecl>>,
    ) -> RecordSpec {
        RecordSpec {
            id: NodeId(0),
            kind,
            tag,
            fields,
            static_asserts: Vec::new(),
            span: DUMMY_SP,
            attrs: Vec::new(),
        }
    }

    #[test]
    fn record_lower_simple_two_fields() {
        // struct S { int a; int b; }
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");
        let spec = record_spec(
            rcc_ast::RecordKind::Struct,
            None,
            Some(vec![named_field(a, vec![TypeSpec::Int]), named_field(b, vec![TypeSpec::Int])]),
        );
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Record { kind: RecordKind::Struct, fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, Some(a));
                assert_eq!(fields[0].ty, tcx.int);
                assert_eq!(fields[0].bit_width, None);
                assert_eq!(fields[1].name, Some(b));
                assert_eq!(fields[1].ty, tcx.int);
            }
            other => panic!("expected Record, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn record_lower_shared_specs_multiple_declarators() {
        // struct S { int a, b; } — one FieldDecl with two declarators.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");
        let fd = field_decl(
            vec![TypeSpec::Int],
            vec![(Some(a), Vec::new(), None), (Some(b), Vec::new(), None)],
        );
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Record { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, Some(a));
                assert_eq!(fields[1].name, Some(b));
                assert_eq!(fields[0].ty, tcx.int);
                assert_eq!(fields[1].ty, tcx.int);
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_lower_union_kind() {
        // union U { int a; } — kind is Union.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");
        let spec = record_spec(
            rcc_ast::RecordKind::Union,
            None,
            Some(vec![named_field(a, vec![TypeSpec::Int])]),
        );
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Record { kind: RecordKind::Union, .. } => {}
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn record_bitfield_width_zero_separator() {
        // struct { int : 0; int a; } — anonymous zero-width separator.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");

        let zero_width = int_lit("0", &mut sess);
        let sep = field_decl(vec![TypeSpec::Int], vec![(None, Vec::new(), Some(zero_width))]);
        let named = named_field(a, vec![TypeSpec::Int]);

        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![sep, named]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        let diags = cap.diagnostics();
        assert!(diags.is_empty(), "zero-width anonymous bit-field should be accepted: {diags:?}");
        match kind {
            DefKind::Record { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, None, "separator has no name");
                assert_eq!(fields[0].bit_width, Some(0));
                assert_eq!(fields[1].name, Some(a));
                assert_eq!(fields[1].bit_width, None);
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_bitfield_width_negative_errors() {
        // struct { int x : -1; } → E0077.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");

        let neg = neg_lit("1", &mut sess);
        let fd = field_decl(vec![TypeSpec::Int], vec![(Some(x), Vec::new(), Some(neg))]);
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0077"));
        assert!(diags[0].message.contains("negative"));

        // The invalid field is dropped from the field list.
        match kind {
            DefKind::Record { fields, .. } => {
                assert!(fields.is_empty(), "invalid bit-field should be dropped");
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_bitfield_width_exceeds_type_errors() {
        // struct { int x : 64; } — 64 > 32 (width of int) → E0077.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");

        let big = int_lit("64", &mut sess);
        let fd = field_decl(vec![TypeSpec::Int], vec![(Some(x), Vec::new(), Some(big))]);
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let _ = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0077"));
        assert!(diags[0].message.contains("exceeds"));
    }

    #[test]
    fn record_named_bitfield_zero_width_errors() {
        // struct { int x : 0; } — named zero-width bit-field → E0077.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");

        let zero = int_lit("0", &mut sess);
        let fd = field_decl(vec![TypeSpec::Int], vec![(Some(x), Vec::new(), Some(zero))]);
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let _ = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0077"));
        assert!(diags[0].message.contains("non-zero width"), "got: {}", diags[0].message);
    }

    #[test]
    fn record_bitfield_width_equal_to_type_ok() {
        // struct { int x : 32; } — exactly the type width is legal.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let x = sym(&mut sess, "x");

        let width = int_lit("32", &mut sess);
        let fd = field_decl(vec![TypeSpec::Int], vec![(Some(x), Vec::new(), Some(width))]);
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        assert!(cap.diagnostics().is_empty());
        match kind {
            DefKind::Record { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].bit_width, Some(32));
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_anonymous_struct_preserves_layout_field() {
        // Acceptance: struct { int a; struct { int b; }; } — outer struct
        // exposes both `a` and `b` for lookup.
        let (mut sess, cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");

        // Inner anonymous struct: `struct { int b; }`
        let inner_spec = record_spec(
            rcc_ast::RecordKind::Struct,
            None,
            Some(vec![named_field(b, vec![TypeSpec::Int])]),
        );

        // Field declaring the anonymous inner struct (no declarator, no bit-width).
        let anon_member =
            field_decl(vec![TypeSpec::Record(inner_spec)], vec![(None, Vec::new(), None)]);

        let outer_spec = record_spec(
            rcc_ast::RecordKind::Struct,
            None,
            Some(vec![named_field(a, vec![TypeSpec::Int]), anon_member]),
        );
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&outer_spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        assert!(cap.diagnostics().is_empty(), "anonymous record lowering should not error");

        match kind {
            DefKind::Record { fields, .. } => {
                // Preserved: `a` and one unnamed record field appear in the outer list.
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, Some(a));
                assert_eq!(fields[1].name, None);
                assert!(matches!(tcx.get(fields[1].ty), Ty::Record(_)));
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_anonymous_union_preserves_union_layout_field() {
        // struct { int a; union { int b; int c; }; }
        // Preserved: a, anonymous union field.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");
        let c = sym(&mut sess, "c");

        let inner_spec = record_spec(
            rcc_ast::RecordKind::Union,
            None,
            Some(vec![named_field(b, vec![TypeSpec::Int]), named_field(c, vec![TypeSpec::Int])]),
        );

        let anon_member =
            field_decl(vec![TypeSpec::Record(inner_spec)], vec![(None, Vec::new(), None)]);

        let outer_spec = record_spec(
            rcc_ast::RecordKind::Struct,
            None,
            Some(vec![named_field(a, vec![TypeSpec::Int]), anon_member]),
        );
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&outer_spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Record { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, Some(a));
                assert_eq!(fields[1].name, None);
                let Ty::Record(union_def) = *tcx.get(fields[1].ty) else {
                    panic!("anonymous union should remain a record field");
                };
                let DefKind::Record { kind: RecordKind::Union, fields: union_fields, .. } =
                    &crate_.defs[union_def].kind
                else {
                    panic!("expected anonymous union definition");
                };
                let names: Vec<_> = union_fields.iter().filter_map(|f| f.name).collect();
                assert_eq!(names, vec![b, c]);
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_bare_tag_ref_no_fields() {
        // `struct S;` — no defining fields. `lower_record` shouldn't
        // normally be called for this, but should handle it defensively.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let s = sym(&mut sess, "S");
        let spec = record_spec(rcc_ast::RecordKind::Struct, Some(s), None);
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Record { fields, .. } => assert!(fields.is_empty()),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_pointer_field_type() {
        // struct { int *p; } — pointer declarator on a field.
        let (mut sess, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let p = sym(&mut sess, "p");

        let fd = field_decl(vec![TypeSpec::Int], vec![(Some(p), vec![ptr()], None)]);
        let spec = record_spec(rcc_ast::RecordKind::Struct, None, Some(vec![fd]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
        let ptr_int = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        match kind {
            DefKind::Record { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].ty, ptr_int);
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    // ── Enum lowering (task 06-08) tests ───────────────────────────────

    /// Helper: build an `EnumSpec` from a list of `(name, optional value expr)`.
    fn enum_spec(tag: Option<Symbol>, variants: Option<Vec<(Symbol, Option<Expr>)>>) -> EnumSpec {
        EnumSpec {
            id: NodeId(0),
            tag,
            enumerators: variants.map(|vs| {
                vs.into_iter()
                    .map(|(name, value)| rcc_ast::Enumerator {
                        name,
                        value,
                        span: DUMMY_SP,
                        attrs: Vec::new(),
                    })
                    .collect()
            }),
            span: DUMMY_SP,
            attrs: Vec::new(),
        }
    }

    #[test]
    fn enum_default_only_values_are_sequential_from_zero() {
        // enum { A, B, C } — A=0, B=1, C=2.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let b = sym(&mut sess, "B");
        let c = sym(&mut sess, "C");
        let spec = enum_spec(None, Some(vec![(a, None), (b, None), (c, None)]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        assert!(cap.diagnostics().is_empty(), "default enum: {:?}", cap.diagnostics());
        match kind {
            DefKind::Enum { repr, variants } => {
                assert_eq!(repr, tcx.int);
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0].value, 0);
                assert_eq!(variants[1].value, 1);
                assert_eq!(variants[2].value, 2);
                assert_eq!(variants[0].name, a);
                assert_eq!(variants[1].name, b);
                assert_eq!(variants[2].name, c);
            }
            other => panic!("expected Enum, got {other:?}"),
        }

        // Each enumerator is now a def in `ordinary`.
        assert_eq!(resolver.ordinary.len(), 3);
        for name in [a, b, c] {
            let def_id = resolver.ordinary.get(&name).copied().expect("enumerator registered");
            match &crate_.defs[def_id].kind {
                DefKind::Enumerator { ty, .. } => assert_eq!(*ty, tcx.int),
                other => panic!("expected Enumerator def, got {other:?}"),
            }
        }
    }

    #[test]
    fn enum_explicit_values_override_and_continue() {
        // enum { A, B = 5, C } — A=0, B=5, C=6.  (Task acceptance.)
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let b = sym(&mut sess, "B");
        let c = sym(&mut sess, "C");
        let five = int_lit("5", &mut sess);
        let spec = enum_spec(None, Some(vec![(a, None), (b, Some(five)), (c, None)]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        assert!(cap.diagnostics().is_empty(), "explicit enum: {:?}", cap.diagnostics());
        match kind {
            DefKind::Enum { variants, .. } => {
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0].value, 0);
                assert_eq!(variants[1].value, 5);
                assert_eq!(variants[2].value, 6);
            }
            other => panic!("expected Enum, got {other:?}"),
        }
    }

    #[test]
    fn enum_negative_explicit_value_is_supported() {
        // enum { A = -1, B } — A=-1, B=0.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let b = sym(&mut sess, "B");
        let neg_one = neg_lit("1", &mut sess);
        let spec = enum_spec(None, Some(vec![(a, Some(neg_one)), (b, None)]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        assert!(
            cap.diagnostics().is_empty(),
            "-1 is representable as int: {:?}",
            cap.diagnostics()
        );
        match kind {
            DefKind::Enum { variants, .. } => {
                assert_eq!(variants[0].value, -1);
                assert_eq!(variants[1].value, 0);
            }
            other => panic!("expected Enum, got {other:?}"),
        }
    }

    #[test]
    fn enum_value_out_of_int_range_warns_w0007() {
        // enum { HUGE = 0xFFFFFFFFFF }; // > INT_MAX.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let huge_name = sym(&mut sess, "HUGE");
        let huge_lit = int_lit("0xFFFFFFFFFF", &mut sess);
        let spec = enum_spec(None, Some(vec![(huge_name, Some(huge_lit))]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let _ = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected one W0007 diagnostic, got {diags:?}");
        assert_eq!(diags[0].code, Some("W0007"));
        assert!(
            diags[0].message.contains("outside the range of `int`"),
            "unexpected message: {}",
            diags[0].message
        );
    }

    #[test]
    fn enum_duplicate_enumerator_name_errors_e0078() {
        // enum { A, A } — second A is a duplicate.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let spec = enum_spec(None, Some(vec![(a, None), (a, None)]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let _ = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected one E0078, got {diags:?}");
        assert_eq!(diags[0].code, Some("E0078"));
        assert!(diags[0].message.contains("duplicate enumerator"));

        // First binding wins: resolver.ordinary should still have exactly one
        // entry for `A`.
        assert_eq!(resolver.ordinary.len(), 1);
    }

    #[test]
    fn enum_duplicate_against_earlier_ordinary_decl_errors_e0078() {
        // typedef int A; enum { A }; — the enumerator conflicts with the
        // existing ordinary-namespace binding.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();
        // Pre-populate an existing ordinary binding (e.g. a prior typedef).
        resolver.ordinary.insert(a, DefId(0));

        let spec = enum_spec(None, Some(vec![(a, None)]));
        let _ = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0078"));
    }

    #[test]
    fn enum_bare_tag_ref_produces_empty_variants() {
        // enum E; — no enumerators yet, lower_enum still produces a valid
        // (empty) Enum definition.
        let (mut sess, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let e = sym(&mut sess, "E");
        let spec = enum_spec(Some(e), None);
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        match kind {
            DefKind::Enum { variants, repr } => {
                assert_eq!(repr, tcx.int);
                assert!(variants.is_empty());
            }
            other => panic!("expected Enum, got {other:?}"),
        }
        assert!(resolver.ordinary.is_empty());
    }

    #[test]
    fn enum_non_constant_value_errors_e0077() {
        // enum { A = x } — `x` is not a constant expression.
        let (mut sess, cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let a = sym(&mut sess, "A");
        let x = sym(&mut sess, "x");
        let non_const = Expr { id: NodeId(0), kind: ExprKind::Ident(x), span: DUMMY_SP };
        let spec = enum_spec(None, Some(vec![(a, Some(non_const))]));
        let mut resolver = Resolver::default();
        let mut crate_ = HirCrate::default();

        let _ = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some("E0077"));
    }

    // ── Statement lowering (task 06-09) tests ──────────────────────────

    use rcc_hir::{Body, HirExprKind, HirStmtKind, LocalDecl};

    fn hir_int_value(body: &Body, id: HirExprId) -> Option<i128> {
        match body.exprs[id].kind {
            HirExprKind::IntLiteral { value, .. } | HirExprKind::IntConst(value) => Some(value),
            _ => None,
        }
    }

    /// Helper: wrap a statement kind into an `rcc_ast::Stmt`.
    fn stmt(kind: StmtKind) -> Stmt {
        Stmt { id: NodeId(0), kind, span: DUMMY_SP }
    }

    /// Helper: build a simple expression statement wrapping an int
    /// literal with the given numeric text.
    fn expr_stmt_int(sess: &mut Session, text: &str) -> Stmt {
        stmt(StmtKind::Expr(Some(int_lit(text, sess))))
    }

    /// Helper: build an ident-reference expression.
    fn ident_expr(sess: &mut Session, name: &str) -> Expr {
        let s = sym(sess, name);
        Expr { id: NodeId(0), kind: ExprKind::Ident(s), span: DUMMY_SP }
    }

    /// Helper: build a binary expression `lhs op rhs`.
    fn binop(op: rcc_ast::BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr {
            id: NodeId(0),
            kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) },
            span: DUMMY_SP,
        }
    }

    /// Helper: lowering harness — returns (Body, root stmt id).
    fn lower_single_stmt(sess: &mut Session, s: Stmt) -> (Body, HirStmtId) {
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope(); // function scope
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();
        let id = lower_stmt(&s, &mut body, &mut scope, &mut crate_, &mut tcx, &mut resolver, sess);
        (body, id)
    }

    #[test]
    fn stmt_null_lowers_to_null() {
        let (mut sess, _cap) = Session::for_test();
        let (body, id) = lower_single_stmt(&mut sess, stmt(StmtKind::Null));
        assert!(matches!(body.stmts[id].kind, HirStmtKind::Null));
    }

    #[test]
    fn stmt_expr_some_lowers_to_expr() {
        let (mut sess, _cap) = Session::for_test();
        let s = expr_stmt_int(&mut sess, "42");
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Expr(eid) => {
                assert_eq!(hir_int_value(&body, *eid), Some(42));
            }
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    #[test]
    fn stmt_expr_none_lowers_to_null() {
        let (mut sess, _cap) = Session::for_test();
        let (body, id) = lower_single_stmt(&mut sess, stmt(StmtKind::Expr(None)));
        assert!(matches!(body.stmts[id].kind, HirStmtKind::Null));
    }

    #[test]
    fn stmt_break_continue_goto() {
        let (mut sess, _cap) = Session::for_test();
        let label = sym(&mut sess, "L");

        let (b1, id1) = lower_single_stmt(&mut sess, stmt(StmtKind::Break));
        assert!(matches!(b1.stmts[id1].kind, HirStmtKind::Break));

        let (b2, id2) = lower_single_stmt(&mut sess, stmt(StmtKind::Continue));
        assert!(matches!(b2.stmts[id2].kind, HirStmtKind::Continue));

        let (b3, id3) = lower_single_stmt(&mut sess, stmt(StmtKind::Goto(label)));
        match &b3.stmts[id3].kind {
            HirStmtKind::Goto(n) => assert_eq!(*n, label),
            other => panic!("expected Goto, got {other:?}"),
        }
    }

    #[test]
    fn stmt_return_void_and_value() {
        let (mut sess, _cap) = Session::for_test();
        let (b1, id1) = lower_single_stmt(&mut sess, stmt(StmtKind::Return(None)));
        assert!(matches!(b1.stmts[id1].kind, HirStmtKind::Return(None)));

        let val = int_lit("7", &mut sess);
        let (b2, id2) = lower_single_stmt(&mut sess, stmt(StmtKind::Return(Some(val))));
        match &b2.stmts[id2].kind {
            HirStmtKind::Return(Some(eid)) => {
                assert_eq!(hir_int_value(&b2, *eid), Some(7));
            }
            other => panic!("expected Return(Some), got {other:?}"),
        }
    }

    #[test]
    fn stmt_if_with_else() {
        let (mut sess, _cap) = Session::for_test();
        let cond = int_lit("1", &mut sess);
        let then_s = expr_stmt_int(&mut sess, "10");
        let else_s = expr_stmt_int(&mut sess, "20");
        let s = stmt(StmtKind::If {
            cond,
            then_branch: Box::new(then_s),
            else_branch: Some(Box::new(else_s)),
        });
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::If { cond, then_branch, else_branch } => {
                assert_eq!(hir_int_value(&body, *cond), Some(1));
                assert!(matches!(body.stmts[*then_branch].kind, HirStmtKind::Expr(_)));
                let else_id = else_branch.expect("expected else branch");
                assert!(matches!(body.stmts[else_id].kind, HirStmtKind::Expr(_)));
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn stmt_if_without_else() {
        let (mut sess, _cap) = Session::for_test();
        let cond = int_lit("0", &mut sess);
        let then_s = stmt(StmtKind::Null);
        let s = stmt(StmtKind::If { cond, then_branch: Box::new(then_s), else_branch: None });
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::If { else_branch, .. } => assert!(else_branch.is_none()),
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn stmt_while_and_do_while() {
        let (mut sess, _cap) = Session::for_test();

        let w_cond = int_lit("1", &mut sess);
        let w_body = stmt(StmtKind::Null);
        let w = stmt(StmtKind::While { cond: w_cond, body: Box::new(w_body) });
        let (body, id) = lower_single_stmt(&mut sess, w);
        assert!(matches!(body.stmts[id].kind, HirStmtKind::While { .. }));

        let d_cond = int_lit("0", &mut sess);
        let d_body = stmt(StmtKind::Null);
        let d = stmt(StmtKind::DoWhile { body: Box::new(d_body), cond: d_cond });
        let (body, id) = lower_single_stmt(&mut sess, d);
        assert!(matches!(body.stmts[id].kind, HirStmtKind::DoWhile { .. }));
    }

    #[test]
    fn stmt_label_and_goto_preserve_name() {
        let (mut sess, _cap) = Session::for_test();
        let l = sym(&mut sess, "end");
        let inner = stmt(StmtKind::Null);
        let s = stmt(StmtKind::Label { name: l, body: Box::new(inner) });
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Label { name, body: inner_id } => {
                assert_eq!(*name, l);
                assert!(matches!(body.stmts[*inner_id].kind, HirStmtKind::Null));
            }
            other => panic!("expected Label, got {other:?}"),
        }
    }

    #[test]
    fn stmt_compound_pushes_and_pops_scope() {
        // { int x; } — declares an x local, visible inside the block only.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator { declarator: named_declarator(x), init: None }],
        };
        let block = Block { id: NodeId(0), items: vec![BlockItem::Decl(decl)], span: DUMMY_SP };
        let s = stmt(StmtKind::Compound(block));
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Block(ids) => {
                assert_eq!(ids.len(), 1, "one LocalDecl statement");
                match &body.stmts[ids[0]].kind {
                    HirStmtKind::LocalDecl { local, init } => {
                        assert!(init.is_none());
                        assert_eq!(body.locals[*local].name, Some(x));
                    }
                    other => panic!("expected LocalDecl, got {other:?}"),
                }
            }
            other => panic!("expected Block, got {other:?}"),
        }
        assert_eq!(body.locals.len(), 1);
    }

    #[test]
    fn stmt_compound_with_initializer_creates_localdecl_with_init() {
        // { int x = 5; }
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let five = int_lit("5", &mut sess);

        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator: named_declarator(x),
                init: Some(rcc_ast::Initializer::Expr(five)),
            }],
        };
        let block = Block { id: NodeId(0), items: vec![BlockItem::Decl(decl)], span: DUMMY_SP };
        let s = stmt(StmtKind::Compound(block));
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Block(ids) => match &body.stmts[ids[0]].kind {
                HirStmtKind::LocalDecl { local, init } => {
                    let init_id = init.expect("expected init expr");
                    assert_eq!(hir_int_value(&body, init_id), Some(5));
                    assert_eq!(body.locals[*local].name, Some(x));
                }
                other => panic!("expected LocalDecl, got {other:?}"),
            },
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn stmt_compound_ident_reference_resolves_to_local() {
        // { int x; x; } — the `x` expression should resolve to LocalRef(0).
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator { declarator: named_declarator(x), init: None }],
        };
        let use_expr = stmt(StmtKind::Expr(Some(ident_expr(&mut sess, "x"))));
        let block = Block {
            id: NodeId(0),
            items: vec![BlockItem::Decl(decl), BlockItem::Stmt(Box::new(use_expr))],
            span: DUMMY_SP,
        };
        let s = stmt(StmtKind::Compound(block));
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Block(ids) => {
                assert_eq!(ids.len(), 2);
                let use_id = ids[1];
                match &body.stmts[use_id].kind {
                    HirStmtKind::Expr(eid) => match &body.exprs[*eid].kind {
                        HirExprKind::LocalRef(l) => assert_eq!(body.locals[*l].name, Some(x)),
                        other => panic!("expected LocalRef, got {other:?}"),
                    },
                    other => panic!("expected Expr, got {other:?}"),
                }
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn stmt_case_and_default_preserve_structure() {
        let (mut sess, _cap) = Session::for_test();
        let case_body = stmt(StmtKind::Break);
        let c = stmt(StmtKind::Case {
            value: int_lit("3", &mut sess),
            range_end: None,
            body: Box::new(case_body),
        });
        let (body, id) = lower_single_stmt(&mut sess, c);
        match &body.stmts[id].kind {
            HirStmtKind::Case { value, body: inner, .. } => {
                assert_eq!(*value, Some(3));
                assert!(matches!(body.stmts[*inner].kind, HirStmtKind::Break));
            }
            other => panic!("expected Case, got {other:?}"),
        }

        let d_body = stmt(StmtKind::Break);
        let d = stmt(StmtKind::Default { body: Box::new(d_body) });
        let (body, id) = lower_single_stmt(&mut sess, d);
        match &body.stmts[id].kind {
            HirStmtKind::Default { body: inner } => {
                assert!(matches!(body.stmts[*inner].kind, HirStmtKind::Break));
            }
            other => panic!("expected Default, got {other:?}"),
        }
    }

    #[test]
    fn stmt_case_char_literal_folds_to_integer_value() {
        let (mut sess, _cap) = Session::for_test();
        let case_body = stmt(StmtKind::Break);
        let c = stmt(StmtKind::Case {
            value: char_lit(&mut sess, "'^'"),
            range_end: None,
            body: Box::new(case_body),
        });
        let (body, id) = lower_single_stmt(&mut sess, c);
        match &body.stmts[id].kind {
            HirStmtKind::Case { value, .. } => {
                assert_eq!(*value, Some(i128::from(b'^')));
            }
            other => panic!("expected Case, got {other:?}"),
        }
    }

    #[test]
    fn stmt_switch_with_body() {
        let (mut sess, _cap) = Session::for_test();
        let cond = int_lit("1", &mut sess);
        let body_stmt = stmt(StmtKind::Break);
        let s = stmt(StmtKind::Switch { cond, body: Box::new(body_stmt) });
        let (body, id) = lower_single_stmt(&mut sess, s);
        match &body.stmts[id].kind {
            HirStmtKind::Switch { cond, body: body_id, cases } => {
                assert_eq!(hir_int_value(&body, *cond), Some(1));
                assert!(matches!(body.stmts[*body_id].kind, HirStmtKind::Break));
                assert!(cases.is_empty(), "cases collected in a later pass");
            }
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    /// Acceptance: `for (int i = 0; i < n; ++i) body` lowers to a `For`
    /// with `init = LocalDecl { local: i, init: 0 }`.
    #[test]
    fn stmt_for_with_init_declaration_acceptance() {
        let (mut sess, _cap) = Session::for_test();
        let i = sym(&mut sess, "i");
        let n = sym(&mut sess, "n");

        // File-scope: `n` is a global int so ident lookup succeeds.
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope(); // function scope
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();
        let n_def = crate_.defs.push(Def {
            id: DefId(0),
            name: n,
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.int,
                quals: ObjectQuals::none(),
                thread_local: false,
                linkage: Linkage::External,
                init: None,
            },
        });
        crate_.defs[n_def].id = n_def;
        resolver.ordinary.insert(n, n_def);

        // init: `int i = 0` as a BlockItem::Decl.
        let zero = int_lit("0", &mut sess);
        let init_decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator: named_declarator(i),
                init: Some(rcc_ast::Initializer::Expr(zero)),
            }],
        };
        let init_item = Box::new(BlockItem::Decl(init_decl));

        // cond: `i < n`
        let cond =
            binop(rcc_ast::BinOp::Lt, ident_expr(&mut sess, "i"), ident_expr(&mut sess, "n"));

        // step: `++i`
        let step = Expr {
            id: NodeId(0),
            kind: ExprKind::Unary {
                op: rcc_ast::UnOp::PreInc,
                operand: Box::new(ident_expr(&mut sess, "i")),
            },
            span: DUMMY_SP,
        };

        // body: `;` (null statement placeholder for "body")
        let body_stmt = stmt(StmtKind::Null);

        let for_stmt = stmt(StmtKind::For {
            init: Some(init_item),
            cond: Some(Box::new(cond)),
            step: Some(Box::new(step)),
            body: Box::new(body_stmt),
        });

        let for_id = lower_stmt(
            &for_stmt,
            &mut body,
            &mut scope,
            &mut crate_,
            &mut tcx,
            &mut resolver,
            &mut sess,
        );

        match &body.stmts[for_id].kind {
            HirStmtKind::For { init, cond, step, body: body_id } => {
                // init must be a LocalDecl with `init = 0`.
                let init_id = init.expect("for-init should produce a stmt id");
                match &body.stmts[init_id].kind {
                    HirStmtKind::LocalDecl { local, init: init_expr } => {
                        assert_eq!(body.locals[*local].name, Some(i), "local's name should be `i`");
                        let init_expr_id = init_expr.expect("init expr present");
                        assert!(
                            hir_int_value(&body, init_expr_id) == Some(0),
                            "init expr should be integer 0, got {:?}",
                            body.exprs[init_expr_id].kind
                        );
                    }
                    other => panic!("expected LocalDecl in for-init, got {other:?}"),
                }

                // cond should be a Binary Lt whose lhs is LocalRef(i) and rhs is DefRef(n).
                let cond_id = cond.expect("cond present");
                match &body.exprs[cond_id].kind {
                    HirExprKind::Binary { op, lhs, rhs } => {
                        assert!(matches!(op, rcc_hir::rcc_hir_binop::BinOp::Lt));
                        match &body.exprs[*lhs].kind {
                            HirExprKind::LocalRef(l) => {
                                assert_eq!(body.locals[*l].name, Some(i));
                            }
                            other => panic!("expected LocalRef for i, got {other:?}"),
                        }
                        match &body.exprs[*rhs].kind {
                            HirExprKind::DefRef(id) => assert_eq!(*id, n_def),
                            other => panic!("expected DefRef for n, got {other:?}"),
                        }
                    }
                    other => panic!("expected Binary Lt for cond, got {other:?}"),
                }

                // step should be a Unary PreInc on LocalRef(i).
                let step_id = step.expect("step present");
                match &body.exprs[step_id].kind {
                    HirExprKind::Unary { op, operand } => {
                        assert!(matches!(op, rcc_hir::rcc_hir_binop::UnOp::PreInc));
                        match &body.exprs[*operand].kind {
                            HirExprKind::LocalRef(l) => {
                                assert_eq!(body.locals[*l].name, Some(i));
                            }
                            other => panic!("expected LocalRef(i) under PreInc, got {other:?}"),
                        }
                    }
                    other => panic!("expected Unary PreInc for step, got {other:?}"),
                }

                // body is a null statement.
                assert!(matches!(body.stmts[*body_id].kind, HirStmtKind::Null));
            }
            other => panic!("expected For, got {other:?}"),
        }

        // The for-loop's `i` local must go out of scope after the for
        // statement: its binding must not leak into file scope.
        assert!(scope.lookup(i).is_none(), "for-init local should be popped after for");
    }

    #[test]
    fn stmt_for_init_expression_lowers() {
        // for (i = 0; ; ) ;  — init is an expression statement, not a decl.
        let (mut sess, _cap) = Session::for_test();
        let i = sym(&mut sess, "i");

        // Pre-declare `i` as a local before lowering the for-stmt so the
        // ident reference resolves.
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let local = body.locals.push(LocalDecl {
            name: Some(i),
            ty: TyCtxt::new().int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        scope.insert(i, Binding::Local(local));
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();

        // init: `i = 0` as an expression-statement.
        let zero = int_lit("0", &mut sess);
        let init_expr = Expr {
            id: NodeId(0),
            kind: ExprKind::Assign {
                op: rcc_ast::AssignOp::Eq,
                lhs: Box::new(ident_expr(&mut sess, "i")),
                rhs: Box::new(zero),
            },
            span: DUMMY_SP,
        };
        let init_stmt = stmt(StmtKind::Expr(Some(init_expr)));
        let init_item = Box::new(BlockItem::Stmt(Box::new(init_stmt)));

        let for_stmt = stmt(StmtKind::For {
            init: Some(init_item),
            cond: None,
            step: None,
            body: Box::new(stmt(StmtKind::Null)),
        });

        let for_id = lower_stmt(
            &for_stmt,
            &mut body,
            &mut scope,
            &mut crate_,
            &mut tcx,
            &mut resolver,
            &mut sess,
        );
        match &body.stmts[for_id].kind {
            HirStmtKind::For { init, cond, step, .. } => {
                let init_id = init.expect("init should lower to an expression stmt");
                assert!(matches!(body.stmts[init_id].kind, HirStmtKind::Expr(_)));
                assert!(cond.is_none());
                assert!(step.is_none());
            }
            other => panic!("expected For, got {other:?}"),
        }
    }

    #[test]
    fn stmt_nested_compound_inner_scope_lost_on_exit() {
        // { { int x; } x; } — the `x` in the outer block is NOT the
        // inner block's `x` (it should fail to resolve).
        let (mut sess, cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let inner_decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator { declarator: named_declarator(x), init: None }],
        };
        let inner_block =
            Block { id: NodeId(0), items: vec![BlockItem::Decl(inner_decl)], span: DUMMY_SP };
        let inner_stmt = stmt(StmtKind::Compound(inner_block));
        let outer_use = stmt(StmtKind::Expr(Some(ident_expr(&mut sess, "x"))));
        let outer_block = Block {
            id: NodeId(0),
            items: vec![
                BlockItem::Stmt(Box::new(inner_stmt)),
                BlockItem::Stmt(Box::new(outer_use)),
            ],
            span: DUMMY_SP,
        };
        let s = stmt(StmtKind::Compound(outer_block));
        let (_body, _id) = lower_single_stmt(&mut sess, s);

        // The outer reference to `x` must emit E0071 (undeclared).
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some("E0071")),
            "expected E0071 for outer `x` after inner scope popped, got {diags:?}"
        );
    }

    // ── Expression lowering (task 06-10) tests ─────────────────────────

    /// Helper: lower a single expression in an empty body and return
    /// its `(Body, HirExprId, HirCrate, Resolver)`.
    fn lower_single_expr(sess: &mut Session, e: Expr) -> (Body, HirExprId, HirCrate, Resolver) {
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, sess);
        (body, id, crate_, resolver)
    }

    #[test]
    fn stmt_expr_lowers_block_and_splits_final_expr_result() {
        let (mut sess, _cap) = Session::for_test();
        let i = sym(&mut sess, "i");
        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator: named_declarator(i),
                init: Some(rcc_ast::Initializer::Expr(int_lit("2", &mut sess))),
            }],
        };
        let add_assign = Expr {
            id: NodeId(0),
            kind: ExprKind::Assign {
                op: rcc_ast::AssignOp::AddEq,
                lhs: Box::new(Expr { id: NodeId(0), kind: ExprKind::Ident(i), span: DUMMY_SP }),
                rhs: Box::new(int_lit("5", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let final_use = Expr { id: NodeId(0), kind: ExprKind::Ident(i), span: DUMMY_SP };
        let expr = Expr {
            id: NodeId(0),
            kind: ExprKind::StmtExpr(Box::new(Block {
                id: NodeId(0),
                items: vec![
                    BlockItem::Decl(decl),
                    BlockItem::Stmt(Box::new(stmt(StmtKind::Expr(Some(add_assign))))),
                    BlockItem::Stmt(Box::new(stmt(StmtKind::Expr(Some(final_use))))),
                ],
                span: DUMMY_SP,
            })),
            span: DUMMY_SP,
        };

        let (body, id, _crate, _resolver) = lower_single_expr(&mut sess, expr);
        let HirExprKind::StmtExpr { stmts, result: Some(result) } = &body.exprs[id].kind else {
            panic!("expected statement expression with value result");
        };
        assert_eq!(stmts.len(), 2, "decl and add-assign remain runtime statements");
        let HirStmtKind::LocalDecl { local, init: Some(_) } = body.stmts[stmts[0]].kind else {
            panic!("expected first statement-expression item to be a local decl");
        };
        assert!(matches!(body.stmts[stmts[1]].kind, HirStmtKind::Expr(_)));
        assert!(
            matches!(body.exprs[*result].kind, HirExprKind::LocalRef(got) if got == local),
            "final expression should be kept separately as the result"
        );
    }

    #[test]
    fn stmt_expr_without_final_expr_has_void_result_marker() {
        let (mut sess, _cap) = Session::for_test();
        let i = sym(&mut sess, "i");
        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator { declarator: named_declarator(i), init: None }],
        };
        let expr = Expr {
            id: NodeId(0),
            kind: ExprKind::StmtExpr(Box::new(Block {
                id: NodeId(0),
                items: vec![BlockItem::Decl(decl), BlockItem::Stmt(Box::new(stmt(StmtKind::Null)))],
                span: DUMMY_SP,
            })),
            span: DUMMY_SP,
        };

        let (body, id, _crate, _resolver) = lower_single_expr(&mut sess, expr);
        let HirExprKind::StmtExpr { stmts, result: None } = &body.exprs[id].kind else {
            panic!("expected statement expression without value result");
        };
        assert_eq!(stmts.len(), 2);
    }

    /// Helper: build a `StringLit` expression from the raw source text
    /// (quotes included). Example: `string_lit(&mut sess, "\"hi\"")`.
    fn string_lit(sess: &mut Session, raw: &str) -> Expr {
        let s = sym(sess, raw);
        let bytes = decode_string_literal_values(strip_string_literal_quotes(raw))
            .into_iter()
            .map(|v| v as u8)
            .collect();
        Expr {
            id: NodeId(0),
            kind: ExprKind::StringLit(rcc_ast::StringLiteral {
                text: s,
                bytes,
                encoding: rcc_ast::LiteralEncoding::None,
            }),
            span: DUMMY_SP,
        }
    }

    /// Helper: build a `CharLit` expression from the raw source text
    /// (quotes included). Example: `char_lit(&mut sess, "'a'")`.
    fn char_lit(sess: &mut Session, raw: &str) -> Expr {
        let s = sym(sess, raw);
        Expr {
            id: NodeId(0),
            kind: ExprKind::CharLit(rcc_ast::CharLiteral {
                text: s,
                value: decode_first_char_value(raw).unwrap_or(0) as u32,
                encoding: rcc_ast::LiteralEncoding::None,
            }),
            span: DUMMY_SP,
        }
    }

    /// Helper: build a `FloatLit` expression.
    fn float_lit(sess: &mut Session, raw: &str) -> Expr {
        let s = sym(sess, raw);
        let suffix = if raw.ends_with('f') || raw.ends_with('F') {
            rcc_ast::FloatSuffix::F
        } else if raw.ends_with('l') || raw.ends_with('L') {
            rcc_ast::FloatSuffix::L
        } else {
            rcc_ast::FloatSuffix::None
        };
        let value = raw.trim_end_matches(['f', 'F', 'l', 'L']).parse::<f64>().unwrap();
        Expr {
            id: NodeId(0),
            kind: ExprKind::FloatLit(rcc_ast::FloatLiteral {
                text: s,
                value,
                suffix,
                imaginary: false,
            }),
            span: DUMMY_SP,
        }
    }

    #[test]
    fn expr_int_literal_lowers_with_base_and_suffix_metadata() {
        let (mut sess, _cap) = Session::for_test();
        let e = int_lit("42", &mut sess);
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert_eq!(hir_int_value(&body, id), Some(42));
        assert!(matches!(
            body.exprs[id].kind,
            HirExprKind::IntLiteral {
                value: 42,
                base: IntLiteralBase::Decimal,
                suffix: IntLiteralSuffix::None
            }
        ));
    }

    #[test]
    fn expr_int_literal_uses_ast_payload_not_source_text() {
        let (mut sess, _cap) = Session::for_test();
        let misleading_text = sym(&mut sess, "0");
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::IntLit(rcc_ast::IntLiteral {
                text: misleading_text,
                value: 42,
                base: rcc_ast::IntBase::Decimal,
                suffix: rcc_ast::IntSuffix::None,
            }),
            span: DUMMY_SP,
        };
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert_eq!(hir_int_value(&body, id), Some(42));
    }

    #[test]
    fn expr_float_literal_lowers_to_float_const() {
        let (mut sess, _cap) = Session::for_test();
        let e = float_lit(&mut sess, "2.5");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::FloatConst(v) => assert!((v - 2.5).abs() < 1e-9),
            ref other => panic!("expected FloatConst, got {other:?}"),
        }

        // Float suffix should be stripped.
        let e = float_lit(&mut sess, "1.5f");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::FloatConst(v) => assert!((v - 1.5).abs() < 1e-9),
            ref other => panic!("expected FloatConst, got {other:?}"),
        }
    }

    #[test]
    fn expr_char_literal_decodes_value() {
        let (mut sess, _cap) = Session::for_test();
        // 'a' = 97
        let e = char_lit(&mut sess, "'a'");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert!(matches!(body.exprs[id].kind, HirExprKind::IntConst(97)));

        // '\n' = 10
        let e = char_lit(&mut sess, "'\\n'");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert!(matches!(body.exprs[id].kind, HirExprKind::IntConst(10)));

        // '\x41' = 65
        let e = char_lit(&mut sess, "'\\x41'");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert!(matches!(body.exprs[id].kind, HirExprKind::IntConst(65)));

        // '\0' = 0
        let e = char_lit(&mut sess, "'\\0'");
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
        assert!(matches!(body.exprs[id].kind, HirExprKind::IntConst(0)));
    }

    /// Acceptance: `"hi"` → `HirExprKind::StringRef(def_id)` referring
    /// to a new `DefKind::Global { ty: [char; 3], linkage: Internal }`.
    #[test]
    fn expr_string_literal_creates_global_of_correct_length() {
        let (mut sess, _cap) = Session::for_test();
        let e = string_lit(&mut sess, "\"hi\"");
        let (body, id, crate_, resolver) = lower_single_expr(&mut sess, e);

        let def_id = match body.exprs[id].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };

        // Exactly one global was created.
        assert_eq!(crate_.defs.len(), 1);
        match &crate_.defs[def_id].kind {
            DefKind::Global { linkage, .. } => {
                assert_eq!(*linkage, Linkage::Internal, "string literal linkage");
            }
            other => panic!("expected DefKind::Global, got {other:?}"),
        }

        // Resolver cache populated.
        assert_eq!(resolver.strings.len(), 1);
        // (Type shape verified in expr_string_literal_type_is_array_of_char_with_nul.)
    }

    #[test]
    fn expr_string_literal_type_is_array_of_char_with_nul() {
        // Lower a string inside a real session so we can inspect TyCtxt.
        let (mut sess, _cap) = Session::for_test();
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();

        let e = string_lit(&mut sess, "\"hi\"");
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);

        let def_id = match body.exprs[id].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };

        match &crate_.defs[def_id].kind {
            DefKind::Global { ty, linkage: Linkage::Internal, .. } => match tcx.get(*ty) {
                Ty::Array { elem, len, is_vla } => {
                    assert_eq!(elem.ty, tcx.char_, "element should be char");
                    assert_eq!(*len, Some(3), "\"hi\" is 2 chars + NUL = 3");
                    assert!(!is_vla);
                }
                other => panic!("expected Array type, got {other:?}"),
            },
            other => panic!("expected Global Internal, got {other:?}"),
        }
    }

    #[test]
    fn expr_string_literal_dedup_reuses_def_id() {
        // Two occurrences of the same literal should reuse one global.
        let (mut sess, _cap) = Session::for_test();
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();

        let e1 = string_lit(&mut sess, "\"hi\"");
        let e2 = string_lit(&mut sess, "\"hi\"");
        let id1 =
            lower_expr(&e1, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        let id2 =
            lower_expr(&e2, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        let d1 = match body.exprs[id1].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };
        let d2 = match body.exprs[id2].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };
        assert_eq!(d1, d2);
        assert_eq!(crate_.defs.len(), 1, "both literals share one global");
        assert_eq!(resolver.strings.len(), 1);
    }

    #[test]
    fn expr_string_literal_distinct_strings_get_distinct_globals() {
        let (mut sess, _cap) = Session::for_test();
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut crate_ = HirCrate::default();
        let mut tcx = TyCtxt::new();
        let mut resolver = Resolver::default();

        let e1 = string_lit(&mut sess, "\"a\"");
        let e2 = string_lit(&mut sess, "\"bb\"");
        let id1 =
            lower_expr(&e1, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        let id2 =
            lower_expr(&e2, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        let d1 = match body.exprs[id1].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };
        let d2 = match body.exprs[id2].kind {
            HirExprKind::StringRef(d) => d,
            ref other => panic!("expected StringRef, got {other:?}"),
        };
        assert_ne!(d1, d2);
        assert_eq!(crate_.defs.len(), 2);

        // Each has its own [char; N] type with correct length.
        match tcx.get(match &crate_.defs[d1].kind {
            DefKind::Global { ty, .. } => *ty,
            _ => unreachable!(),
        }) {
            Ty::Array { len: Some(2), .. } => {}
            other => panic!("expected [char; 2] for \"a\", got {other:?}"),
        }
        match tcx.get(match &crate_.defs[d2].kind {
            DefKind::Global { ty, .. } => *ty,
            _ => unreachable!(),
        }) {
            Ty::Array { len: Some(3), .. } => {}
            other => panic!("expected [char; 3] for \"bb\", got {other:?}"),
        }
    }

    #[test]
    fn expr_paren_returns_inner_id_without_extra_node() {
        let (mut sess, _cap) = Session::for_test();
        let inner = int_lit("7", &mut sess);
        let paren = Expr { id: NodeId(0), kind: ExprKind::Paren(Box::new(inner)), span: DUMMY_SP };
        let (body, id, _crate, _res) = lower_single_expr(&mut sess, paren);
        // Paren doesn't add a wrapper — the id is the inner int-const node.
        assert_eq!(hir_int_value(&body, id), Some(7));
        // Exactly one HIR expression node was created, not two.
        assert_eq!(body.exprs.len(), 1);
    }

    #[test]
    fn expr_binary_all_arith_cmp_log_bit_ops_lower() {
        // Sweep every BinOp variant; each must produce HirExprKind::Binary.
        let ops: &[(rcc_ast::BinOp, rcc_hir::rcc_hir_binop::BinOp)] = &[
            (rcc_ast::BinOp::Add, rcc_hir::rcc_hir_binop::BinOp::Add),
            (rcc_ast::BinOp::Sub, rcc_hir::rcc_hir_binop::BinOp::Sub),
            (rcc_ast::BinOp::Mul, rcc_hir::rcc_hir_binop::BinOp::Mul),
            (rcc_ast::BinOp::Div, rcc_hir::rcc_hir_binop::BinOp::Div),
            (rcc_ast::BinOp::Rem, rcc_hir::rcc_hir_binop::BinOp::Rem),
            (rcc_ast::BinOp::Shl, rcc_hir::rcc_hir_binop::BinOp::Shl),
            (rcc_ast::BinOp::Shr, rcc_hir::rcc_hir_binop::BinOp::Shr),
            (rcc_ast::BinOp::Lt, rcc_hir::rcc_hir_binop::BinOp::Lt),
            (rcc_ast::BinOp::Le, rcc_hir::rcc_hir_binop::BinOp::Le),
            (rcc_ast::BinOp::Gt, rcc_hir::rcc_hir_binop::BinOp::Gt),
            (rcc_ast::BinOp::Ge, rcc_hir::rcc_hir_binop::BinOp::Ge),
            (rcc_ast::BinOp::Eq, rcc_hir::rcc_hir_binop::BinOp::Eq),
            (rcc_ast::BinOp::Ne, rcc_hir::rcc_hir_binop::BinOp::Ne),
            (rcc_ast::BinOp::BitAnd, rcc_hir::rcc_hir_binop::BinOp::BitAnd),
            (rcc_ast::BinOp::BitXor, rcc_hir::rcc_hir_binop::BinOp::BitXor),
            (rcc_ast::BinOp::BitOr, rcc_hir::rcc_hir_binop::BinOp::BitOr),
            (rcc_ast::BinOp::LogAnd, rcc_hir::rcc_hir_binop::BinOp::LogAnd),
            (rcc_ast::BinOp::LogOr, rcc_hir::rcc_hir_binop::BinOp::LogOr),
        ];
        for (ast_op, hir_op) in ops {
            let (mut sess, _cap) = Session::for_test();
            let e = binop(*ast_op, int_lit("1", &mut sess), int_lit("2", &mut sess));
            let (body, id, _crate, _res) = lower_single_expr(&mut sess, e);
            match body.exprs[id].kind {
                HirExprKind::Binary { op, .. } => assert_eq!(op, *hir_op, "ast op {ast_op:?}"),
                ref other => panic!("expected Binary for {ast_op:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn expr_unary_all_ops_lower() {
        // Regular UnOps.
        let (mut sess, _cap) = Session::for_test();
        for (ast_op, hir_op) in [
            (rcc_ast::UnOp::Plus, rcc_hir::rcc_hir_binop::UnOp::Plus),
            (rcc_ast::UnOp::Neg, rcc_hir::rcc_hir_binop::UnOp::Neg),
            (rcc_ast::UnOp::BitNot, rcc_hir::rcc_hir_binop::UnOp::BitNot),
            (rcc_ast::UnOp::LogNot, rcc_hir::rcc_hir_binop::UnOp::LogNot),
            (rcc_ast::UnOp::PreInc, rcc_hir::rcc_hir_binop::UnOp::PreInc),
            (rcc_ast::UnOp::PreDec, rcc_hir::rcc_hir_binop::UnOp::PreDec),
            (rcc_ast::UnOp::PostInc, rcc_hir::rcc_hir_binop::UnOp::PostInc),
            (rcc_ast::UnOp::PostDec, rcc_hir::rcc_hir_binop::UnOp::PostDec),
        ] {
            let e = Expr {
                id: NodeId(0),
                kind: ExprKind::Unary { op: ast_op, operand: Box::new(int_lit("3", &mut sess)) },
                span: DUMMY_SP,
            };
            let mut body = Body::default();
            let mut scope = ScopeStack::new();
            scope.push_scope();
            let mut crate_ = HirCrate::default();
            let mut tcx = TyCtxt::new();
            let mut resolver = Resolver::default();
            let id =
                lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
            match body.exprs[id].kind {
                HirExprKind::Unary { op, .. } => assert_eq!(op, hir_op),
                ref other => panic!("expected Unary for {ast_op:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn expr_unary_addr_of_and_deref_have_dedicated_variants() {
        let (mut sess, _cap) = Session::for_test();

        let addr = Expr {
            id: NodeId(0),
            kind: ExprKind::Unary {
                op: rcc_ast::UnOp::AddrOf,
                operand: Box::new(int_lit("1", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, addr);
        assert!(matches!(body.exprs[id].kind, HirExprKind::AddressOf(_)));

        let deref = Expr {
            id: NodeId(0),
            kind: ExprKind::Unary {
                op: rcc_ast::UnOp::Deref,
                operand: Box::new(int_lit("1", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, deref);
        assert!(matches!(body.exprs[id].kind, HirExprKind::Deref(_)));
    }

    #[test]
    fn expr_conditional_ternary() {
        let (mut sess, _cap) = Session::for_test();
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Cond {
                cond: Box::new(int_lit("1", &mut sess)),
                then_expr: Box::new(int_lit("2", &mut sess)),
                else_expr: Box::new(int_lit("3", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::Cond { cond, then_expr, else_expr } => {
                assert_eq!(hir_int_value(&body, cond), Some(1));
                assert_eq!(hir_int_value(&body, then_expr), Some(2));
                assert_eq!(hir_int_value(&body, else_expr), Some(3));
            }
            ref other => panic!("expected Cond, got {other:?}"),
        }
    }

    #[test]
    fn expr_gnu_omitted_conditional_preserves_single_operand_shape() {
        let (mut sess, _cap) = Session::for_test();
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::OmittedCond {
                cond: Box::new(int_lit("1", &mut sess)),
                else_expr: Box::new(int_lit("3", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::OmittedCond { cond, else_expr } => {
                assert_eq!(hir_int_value(&body, cond), Some(1));
                assert_eq!(hir_int_value(&body, else_expr), Some(3));
            }
            ref other => panic!("expected OmittedCond, got {other:?}"),
        }
    }

    #[test]
    fn expr_comma_preserves_lhs_and_rhs() {
        let (mut sess, _cap) = Session::for_test();
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Comma {
                lhs: Box::new(int_lit("10", &mut sess)),
                rhs: Box::new(int_lit("20", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::Comma { lhs, rhs } => {
                assert_eq!(hir_int_value(&body, lhs), Some(10));
                assert_eq!(hir_int_value(&body, rhs), Some(20));
            }
            ref other => panic!("expected Comma, got {other:?}"),
        }
    }

    #[test]
    fn expr_simple_assign_lowers_to_assign() {
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut tcx = TyCtxt::new();
        let local = body.locals.push(LocalDecl {
            name: Some(x),
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        scope.insert(x, Binding::Local(local));
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Assign {
                op: rcc_ast::AssignOp::Eq,
                lhs: Box::new(ident_expr(&mut sess, "x")),
                rhs: Box::new(int_lit("5", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        match body.exprs[id].kind {
            HirExprKind::Assign { lhs, rhs } => {
                assert!(matches!(body.exprs[lhs].kind, HirExprKind::LocalRef(_)));
                assert_eq!(hir_int_value(&body, rhs), Some(5));
            }
            ref other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn expr_compound_assign_desugars_to_assign_with_binop_rhs() {
        // `x += 1` → `x = x + 1`.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");

        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut tcx = TyCtxt::new();
        let local = body.locals.push(LocalDecl {
            name: Some(x),
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        scope.insert(x, Binding::Local(local));
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Assign {
                op: rcc_ast::AssignOp::AddEq,
                lhs: Box::new(ident_expr(&mut sess, "x")),
                rhs: Box::new(int_lit("1", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        match &body.exprs[id].kind {
            HirExprKind::Assign { lhs, rhs } => {
                assert!(matches!(body.exprs[*lhs].kind, HirExprKind::LocalRef(_)));
                match &body.exprs[*rhs].kind {
                    HirExprKind::Binary { op, lhs: bl, rhs: br } => {
                        assert_eq!(*op, rcc_hir::rcc_hir_binop::BinOp::Add);
                        assert!(matches!(body.exprs[*bl].kind, HirExprKind::LocalRef(_)));
                        assert_eq!(hir_int_value(&body, *br), Some(1));
                    }
                    other => panic!("expected Binary inside Assign rhs, got {other:?}"),
                }
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn expr_call_lowers_callee_and_args() {
        let (mut sess, _cap) = Session::for_test();
        let f = sym(&mut sess, "f");

        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        // Register `f` as a file-scope global so the callee resolves.
        let f_def = crate_.defs.push(Def {
            id: DefId(0),
            name: f,
            span: DUMMY_SP,
            kind: DefKind::Function {
                ty: tcx.int,
                has_body: false,
                is_static: false,
                is_inline: false,
                is_extern_inline: false,
                no_instrument_function: false,
                variadic: false,
            },
        });
        crate_.defs[f_def].id = f_def;
        resolver.ordinary.insert(f, f_def);

        let call = Expr {
            id: NodeId(0),
            kind: ExprKind::Call {
                callee: Box::new(ident_expr(&mut sess, "f")),
                args: vec![int_lit("1", &mut sess), int_lit("2", &mut sess)],
            },
            span: DUMMY_SP,
        };
        let id =
            lower_expr(&call, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        match &body.exprs[id].kind {
            HirExprKind::Call { callee, args } => {
                assert!(matches!(body.exprs[*callee].kind, HirExprKind::DefRef(_)));
                assert_eq!(args.len(), 2);
                assert_eq!(hir_int_value(&body, args[0]), Some(1));
                assert_eq!(hir_int_value(&body, args[1]), Some(2));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn expr_builtin_expect_preserves_hint_side_effect_operand() {
        let (mut sess, _cap) = Session::for_test();
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Call {
                callee: Box::new(ident_expr(&mut sess, "__builtin_expect")),
                args: vec![int_lit("7", &mut sess), int_lit("0", &mut sess)],
            },
            span: DUMMY_SP,
        };

        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);

        match body.exprs[id].kind {
            HirExprKind::BuiltinExpect { value, expected } => {
                assert_eq!(hir_int_value(&body, value), Some(7));
                assert_eq!(hir_int_value(&body, expected), Some(0));
            }
            ref other => panic!("expected BuiltinExpect, got {other:?}"),
        }
    }

    #[test]
    fn expr_member_dot_preserves_requested_field_name() {
        let (mut sess, _cap) = Session::for_test();
        let field = sym(&mut sess, "x");
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Member { base: Box::new(int_lit("0", &mut sess)), field },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::UnresolvedField { base, field: member, field_span } => {
                assert_eq!(member, field);
                assert_eq!(field_span, DUMMY_SP);
                assert_eq!(hir_int_value(&body, base), Some(0));
            }
            ref other => panic!("expected UnresolvedField, got {other:?}"),
        }
    }

    #[test]
    fn expr_arrow_preserves_requested_field_name_over_deref() {
        // `p->x` → UnresolvedField { base: Deref(p), field: x }.
        let (mut sess, _cap) = Session::for_test();
        let field = sym(&mut sess, "x");
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Arrow { base: Box::new(int_lit("0", &mut sess)), field },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::UnresolvedField { base, field: member, field_span } => {
                assert_eq!(member, field);
                assert_eq!(field_span, DUMMY_SP);
                // The base should itself be a Deref node.
                match body.exprs[base].kind {
                    HirExprKind::Deref(inner) => {
                        assert_eq!(hir_int_value(&body, inner), Some(0));
                    }
                    ref other => panic!("expected Deref under Field, got {other:?}"),
                }
            }
            ref other => panic!("expected UnresolvedField, got {other:?}"),
        }
    }

    #[test]
    fn expr_index_lowers_base_and_index() {
        let (mut sess, _cap) = Session::for_test();
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Index {
                base: Box::new(int_lit("100", &mut sess)),
                index: Box::new(int_lit("2", &mut sess)),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::Index { base, index } => {
                assert_eq!(hir_int_value(&body, base), Some(100));
                assert_eq!(hir_int_value(&body, index), Some(2));
            }
            ref other => panic!("expected Index, got {other:?}"),
        }
    }

    #[test]
    fn expr_cast_wraps_operand() {
        let (mut sess, _cap) = Session::for_test();
        let ty_name = rcc_ast::TypeName {
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            declarator: Declarator {
                name: None,
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            },
            span: DUMMY_SP,
        };
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::Cast { ty: ty_name, expr: Box::new(int_lit("1", &mut sess)) },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        match body.exprs[id].kind {
            HirExprKind::Cast { operand, to } => {
                assert_eq!(hir_int_value(&body, operand), Some(1));
                assert_ne!(to, TyCtxt::new().error);
            }
            ref other => panic!("expected Cast, got {other:?}"),
        }
    }

    #[test]
    fn expr_sizeof_expr_preserves_operand_and_type_placeholder() {
        let (mut sess, _cap) = Session::for_test();

        let se = Expr {
            id: NodeId(0),
            kind: ExprKind::SizeofExpr(Box::new(int_lit("1", &mut sess))),
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, se);
        match body.exprs[id].kind {
            HirExprKind::SizeofExpr(inner) => {
                assert_eq!(hir_int_value(&body, inner), Some(1));
            }
            ref other => panic!("expected SizeofExpr, got {other:?}"),
        }

        let ty_name = rcc_ast::TypeName {
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            declarator: Declarator {
                name: None,
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            },
            span: DUMMY_SP,
        };
        let st = Expr { id: NodeId(0), kind: ExprKind::SizeofType(ty_name), span: DUMMY_SP };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, st);
        assert!(matches!(body.exprs[id].kind, HirExprKind::SizeofType(_)));
    }

    #[test]
    fn expr_compound_literal_preserves_type() {
        let (mut sess, _cap) = Session::for_test();
        let ty_name = rcc_ast::TypeName {
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            declarator: Declarator {
                name: None,
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            },
            span: DUMMY_SP,
        };
        let e = Expr {
            id: NodeId(0),
            kind: ExprKind::CompoundLiteral {
                ty: ty_name,
                init: Box::new(rcc_ast::Initializer::Expr(int_lit("0", &mut sess))),
            },
            span: DUMMY_SP,
        };
        let (body, id, _c, _r) = lower_single_expr(&mut sess, e);
        assert!(matches!(body.exprs[id].kind, HirExprKind::CompoundLiteral { .. }));
    }

    #[test]
    fn expr_ident_resolves_to_local_or_def_ref() {
        // Local.
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut tcx = TyCtxt::new();
        let local = body.locals.push(LocalDecl {
            name: Some(x),
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        scope.insert(x, Binding::Local(local));
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();

        let e = ident_expr(&mut sess, "x");
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        assert!(matches!(body.exprs[id].kind, HirExprKind::LocalRef(_)));

        // Global def.
        let (mut sess, _cap) = Session::for_test();
        let g = sym(&mut sess, "g");
        let mut body = Body::default();
        let mut scope = ScopeStack::new();
        scope.push_scope();
        let mut tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        let g_def = crate_.defs.push(Def {
            id: DefId(0),
            name: g,
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: tcx.int,
                quals: ObjectQuals::none(),
                thread_local: false,
                linkage: Linkage::External,
                init: None,
            },
        });
        crate_.defs[g_def].id = g_def;
        resolver.ordinary.insert(g, g_def);

        let e = ident_expr(&mut sess, "g");
        let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
        match body.exprs[id].kind {
            HirExprKind::DefRef(d) => assert_eq!(d, g_def),
            ref other => panic!("expected DefRef, got {other:?}"),
        }
    }

    /// Task deliverable: "assert AST → HIR node count is preserved".
    ///
    /// For a tree with no `Paren` wrappers and no compound-assign
    /// desugaring (which adds a synthetic Binary node) the count of
    /// HIR expression nodes equals the count of AST expression nodes.
    #[test]
    fn expr_tree_node_count_is_preserved() {
        let (mut sess, _cap) = Session::for_test();

        // AST: (1 + 2) * 3  — without Paren wrapper, 4 AST expr nodes.
        // Shape: Binary { Mul, Binary { Add, IntLit 1, IntLit 2 }, IntLit 3 }
        let ast = binop(
            rcc_ast::BinOp::Mul,
            binop(rcc_ast::BinOp::Add, int_lit("1", &mut sess), int_lit("2", &mut sess)),
            int_lit("3", &mut sess),
        );
        // Count AST nodes recursively.
        fn count_ast(e: &Expr) -> usize {
            match &e.kind {
                ExprKind::IntLit(_)
                | ExprKind::FloatLit(_)
                | ExprKind::CharLit(_)
                | ExprKind::StringLit(_)
                | ExprKind::Ident(_) => 1,
                ExprKind::Binary { lhs, rhs, .. } => 1 + count_ast(lhs) + count_ast(rhs),
                ExprKind::Unary { operand, .. } => 1 + count_ast(operand),
                ExprKind::Paren(inner) => count_ast(inner),
                _ => 1,
            }
        }
        let n_ast = count_ast(&ast);
        let (body, _id, _c, _r) = lower_single_expr(&mut sess, ast);
        assert_eq!(body.exprs.len(), n_ast, "one HIR expr node per AST expr node");
        assert_eq!(n_ast, 5); // Binary + Binary + 3 literals
    }

    // ── Initializer lowering (task 06-11) ───────────────────────────────

    /// Helper: extract the initializer stores that follow a `LocalDecl`
    /// inside a freshly-lowered Compound block. Returns (local id, vec of
    /// (lhs HirExprId, rhs HirExprId)) for assert convenience.
    fn collect_init_assigns(
        body: &Body,
        block_root: HirStmtId,
    ) -> (Local, Vec<(HirExprId, HirExprId)>) {
        let HirStmtKind::Block(ids) = &body.stmts[block_root].kind else {
            panic!("expected block at root");
        };
        let mut local = None;
        let mut out = Vec::new();
        for sid in ids {
            match &body.stmts[*sid].kind {
                HirStmtKind::LocalDecl { local: l, .. } => local = Some(*l),
                HirStmtKind::InitAssign { lhs, rhs } => out.push((*lhs, *rhs)),
                _ => {}
            }
        }
        (local.expect("LocalDecl missing"), out)
    }

    /// Build a `int a[3] = {1};` decl-block and lower it through the
    /// statement pipeline so we exercise lower_block_decl + lower_initializer
    /// end-to-end.
    fn lower_array_init_block(
        sess: &mut Session,
        name: Symbol,
        len_text: &str,
        items: Vec<rcc_ast::Initializer>,
    ) -> (Body, HirStmtId) {
        let derived = vec![rcc_ast::DerivedDeclarator::Array(rcc_ast::ArrayDeclarator {
            quals: rcc_ast::TypeQuals::default(),
            has_static: false,
            star: false,
            size: Some(int_lit(len_text, sess)),
        })];
        let declarator = rcc_ast::Declarator {
            name: Some((name, DUMMY_SP)),
            derived,
            span: DUMMY_SP,
            attrs: Vec::new(),
        };
        let init_items: Vec<(Vec<rcc_ast::Designator>, rcc_ast::Initializer)> =
            items.into_iter().map(|i| (Vec::new(), i)).collect();
        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator,
                init: Some(rcc_ast::Initializer::List(init_items)),
            }],
        };
        let block = Block { id: NodeId(0), items: vec![BlockItem::Decl(decl)], span: DUMMY_SP };
        let s = stmt(StmtKind::Compound(block));
        lower_single_stmt(sess, s)
    }

    #[test]
    fn init_array_partial_zero_fills_tail_acceptance() {
        // int a[3] = {1};  ⇒  a[0]=1; a[1]=0; a[2]=0; (acceptance bullet 1)
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let one = rcc_ast::Initializer::Expr(int_lit("1", &mut sess));
        let (body, root) = lower_array_init_block(&mut sess, a, "3", vec![one]);
        let (_local, assigns) = collect_init_assigns(&body, root);
        assert_eq!(assigns.len(), 3, "expected one assign per element, got {}", assigns.len());

        // Pair (index, value) so order is deterministic for assert.
        let mut paired: Vec<(i128, i128)> = assigns
            .iter()
            .map(|(lid, rid)| {
                let i = match &body.exprs[*lid].kind {
                    HirExprKind::Index { index, .. } => {
                        hir_int_value(&body, *index).expect("non-integer index")
                    }
                    other => panic!("expected Index lhs, got {other:?}"),
                };
                let v = hir_int_value(&body, *rid).expect("non-integer rhs");
                (i, v)
            })
            .collect();
        paired.sort_by_key(|(i, _)| *i);
        assert_eq!(paired, vec![(0, 1), (1, 0), (2, 0)]);
    }

    #[test]
    fn init_array_designator_resets_cursor() {
        // int a[3] = { [2] = 7 };  ⇒  a[2]=7; a[0]=0; a[1]=0;
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let derived = vec![rcc_ast::DerivedDeclarator::Array(rcc_ast::ArrayDeclarator {
            quals: rcc_ast::TypeQuals::default(),
            has_static: false,
            star: false,
            size: Some(int_lit("3", &mut sess)),
        })];
        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator: rcc_ast::Declarator {
                    name: Some((a, DUMMY_SP)),
                    derived,
                    span: DUMMY_SP,
                    attrs: Vec::new(),
                },
                init: Some(rcc_ast::Initializer::List(vec![(
                    vec![rcc_ast::Designator::Index(int_lit("2", &mut sess))],
                    rcc_ast::Initializer::Expr(int_lit("7", &mut sess)),
                )])),
            }],
        };
        let block = Block { id: NodeId(0), items: vec![BlockItem::Decl(decl)], span: DUMMY_SP };
        let s = stmt(StmtKind::Compound(block));
        let (body, root) = lower_single_stmt(&mut sess, s);
        let (_local, assigns) = collect_init_assigns(&body, root);
        assert_eq!(assigns.len(), 3);

        let mut paired: Vec<(i128, i128)> = assigns
            .iter()
            .map(|(lid, rid)| {
                let i = match &body.exprs[*lid].kind {
                    HirExprKind::Index { index, .. } => {
                        hir_int_value(&body, *index).expect("non-integer index")
                    }
                    _ => panic!(),
                };
                let v = hir_int_value(&body, *rid).expect("non-integer rhs");
                (i, v)
            })
            .collect();
        paired.sort_by_key(|(i, _)| *i);
        assert_eq!(paired, vec![(0, 0), (1, 0), (2, 7)]);
    }

    #[test]
    fn init_scalar_brace_wrapper_unwraps() {
        // int x = { 5 };  ⇒  x = 5;  (single Assign statement)
        let (mut sess, _cap) = Session::for_test();
        let x = sym(&mut sess, "x");
        let decl = Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
            inits: vec![InitDeclarator {
                declarator: named_declarator(x),
                init: Some(rcc_ast::Initializer::List(vec![(
                    Vec::new(),
                    rcc_ast::Initializer::Expr(int_lit("5", &mut sess)),
                )])),
            }],
        };
        let block = Block { id: NodeId(0), items: vec![BlockItem::Decl(decl)], span: DUMMY_SP };
        let s = stmt(StmtKind::Compound(block));
        let (body, root) = lower_single_stmt(&mut sess, s);
        let (_local, assigns) = collect_init_assigns(&body, root);
        assert_eq!(assigns.len(), 1);
        assert_eq!(hir_int_value(&body, assigns[0].1), Some(5));
    }

    #[test]
    fn init_array_full_explicit_no_zero_fill() {
        // int a[3] = {10, 20, 30};  ⇒  exactly three assigns, no zero-fill.
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let items = vec![
            rcc_ast::Initializer::Expr(int_lit("10", &mut sess)),
            rcc_ast::Initializer::Expr(int_lit("20", &mut sess)),
            rcc_ast::Initializer::Expr(int_lit("30", &mut sess)),
        ];
        let (body, root) = lower_array_init_block(&mut sess, a, "3", items);
        let (_local, assigns) = collect_init_assigns(&body, root);
        assert_eq!(assigns.len(), 3);
        let mut values: Vec<i128> = assigns
            .iter()
            .map(|(_, rid)| hir_int_value(&body, *rid).expect("non-integer rhs"))
            .collect();
        values.sort();
        assert_eq!(values, vec![10, 20, 30]);
    }
}
