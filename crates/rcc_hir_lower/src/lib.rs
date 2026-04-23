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

use rcc_ast::{ExternalDecl, StorageClass, TranslationUnit, TypeSpec};
use rcc_data_structures::FxHashMap;
use rcc_hir::{Def, DefId, DefKind, HirCrate, HirExprKind, Linkage, Local, RecordKind, TyCtxt};
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
        Block, Decl, DeclSpecs, Declarator, EnumSpec, ExternalDecl, FunctionDef, InitDeclarator,
        NodeId, RecordSpec, TranslationUnit, TypeSpec,
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
}
