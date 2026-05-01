//! Integration tests for `rcc_hir_lower` (task 06-12).
//!
//! Walks every feature added during phase 06: declarator-table folding,
//! the three-namespace name resolution, composite (struct/union/enum)
//! lowering, initializer expansion, and statement / expression
//! lowering. Each row is one `#[test]` so the failure point is
//! pin-pointed; the helper [`lower_snippet`] feeds the full
//! lex → preprocess → parse → `lower` pipeline and returns the
//! resulting [`HirCrate`] together with its [`TyCtxt`] for assertions
//! on top-level definitions and the resolver tables.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rcc_ast::{
    ArrayDeclarator, Block, BlockItem, Decl, DeclSpecs, Declarator, DerivedDeclarator, EnumSpec,
    Expr, ExprKind, FieldDecl, FieldDeclarator, FunctionDeclarator, InitDeclarator, Initializer,
    NodeId, ParamDecl, RecordSpec, Stmt, StmtKind, TranslationUnit, TypeQuals, TypeSpec,
};
use rcc_errors::{CaptureEmitter, Handler};
use rcc_hir::ty::{Qual, Ty};
use rcc_hir::{
    Body, DefId, DefKind, GlobalInitDesignator, GlobalInitValue, HirCrate, HirExprKind,
    HirStmtKind, Linkage, Local, LocalDecl, ObjectQuals, RecordKind, TyCtxt, TyId, ValueCat,
};
use rcc_hir_lower::{
    apply_declarator, lower, lower_enum, lower_expr, lower_initializer, lower_record, lower_stmt,
    lower_typedef_name, resolve_expr_ident, resolve_labels, resolve_tag, Binding, DeclScope,
    Resolver, ScopeStack, TagKind,
};
use rcc_session::{Options, Session};
use rcc_span::{Symbol, DUMMY_SP};

// ── Helpers ────────────────────────────────────────────────────────────

/// Drive `src` through `rcc_lexer → rcc_preprocess → rcc_parse →
/// rcc_hir_lower::lower` and return the resulting `HirCrate` together
/// with the freshly-built `TyCtxt`. The returned `Session` is dropped
/// because higher-level assertions only need the HIR shape; tests that
/// inspect diagnostics use `parse_to_ast` instead.
///
/// This is the helper the task's *Deliverables* line names verbatim:
///     `lower_snippet(src: &str) -> (HirCrate, TyCtxt)`.
pub fn lower_snippet(src: &str) -> (HirCrate, TyCtxt) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid =
        sess.source_map.write().unwrap().add_file(PathBuf::from("<lower_snippet>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let hir = lower(&ast, &mut tcx, &mut sess);
    (hir, tcx)
}

fn lower_snippet_with_diagnostics(src: &str) -> (HirCrate, TyCtxt, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid =
        sess.source_map.write().unwrap().add_file(PathBuf::from("<lower_snippet>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let hir = lower(&ast, &mut tcx, &mut sess);
    (hir, tcx, cap)
}

fn checked_snippet_with_diagnostics(src: &str) -> (HirCrate, TyCtxt, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<checked>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut sess);
    rcc_typeck::check(&mut sess, &mut tcx, &mut hir);
    (hir, tcx, cap)
}

/// Like `lower_snippet`, but keeps the `Session` (with capture emitter)
/// around so callers can inspect diagnostics. Returns the parsed AST,
/// not the HIR — this is the entry point for hand-driven lowering of
/// individual passes (declarators, records, statements, ...).
fn parse_to_ast(src: &str) -> (TranslationUnit, Session, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<test>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    (ast, sess, cap)
}

fn intern(sess: &mut Session, s: &str) -> Symbol {
    sess.interner.intern(s)
}

fn named(name: Symbol, derived: Vec<DerivedDeclarator>) -> Declarator {
    Declarator { name: Some((name, DUMMY_SP)), derived, span: DUMMY_SP, attrs: Vec::new() }
}

fn ptr() -> DerivedDeclarator {
    DerivedDeclarator::Pointer(TypeQuals::default())
}

fn const_ptr() -> DerivedDeclarator {
    DerivedDeclarator::Pointer(TypeQuals { const_: true, volatile: false, restrict: false })
}

fn int_lit(text: &str, sess: &mut Session) -> Expr {
    let s = intern(sess, text);
    Expr {
        id: NodeId(0),
        kind: ExprKind::IntLit(rcc_ast::IntLiteral {
            text: s,
            value: text.parse::<u128>().unwrap(),
            suffix: rcc_ast::IntSuffix::None,
        }),
        span: DUMMY_SP,
    }
}

fn array_size(size: u64, sess: &mut Session) -> DerivedDeclarator {
    DerivedDeclarator::Array(ArrayDeclarator {
        quals: TypeQuals::default(),
        has_static: false,
        star: false,
        size: Some(int_lit(&size.to_string(), sess)),
    })
}

fn array_runtime_size(name: &str, sess: &mut Session) -> DerivedDeclarator {
    DerivedDeclarator::Array(ArrayDeclarator {
        quals: TypeQuals::default(),
        has_static: false,
        star: false,
        size: Some(ident_expr(sess, name)),
    })
}

fn array_unsized() -> DerivedDeclarator {
    DerivedDeclarator::Array(ArrayDeclarator {
        quals: TypeQuals::default(),
        has_static: false,
        star: false,
        size: None,
    })
}

fn func_decl(params: Vec<ParamDecl>, is_void: bool, variadic: bool) -> DerivedDeclarator {
    DerivedDeclarator::Function(FunctionDeclarator {
        params,
        is_void,
        variadic,
        kr_names: Vec::new(),
    })
}

fn func_no_params() -> DerivedDeclarator {
    func_decl(Vec::new(), false, false)
}

fn func_void_params() -> DerivedDeclarator {
    func_decl(Vec::new(), true, false)
}

fn param_int() -> ParamDecl {
    ParamDecl {
        specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
        declarator: Declarator {
            name: None,
            derived: Vec::new(),
            span: DUMMY_SP,
            attrs: Vec::new(),
        },
        span: DUMMY_SP,
    }
}

fn ident_expr(sess: &mut Session, name: &str) -> Expr {
    let s = intern(sess, name);
    Expr { id: NodeId(0), kind: ExprKind::Ident(s), span: DUMMY_SP }
}

fn stmt_of(kind: StmtKind) -> Stmt {
    Stmt { id: NodeId(0), kind, span: DUMMY_SP }
}

fn block_of(stmts: Vec<Stmt>) -> Block {
    Block {
        id: NodeId(0),
        items: stmts.into_iter().map(|s| BlockItem::Stmt(Box::new(s))).collect(),
        span: DUMMY_SP,
    }
}

fn record_spec(
    kind: rcc_ast::RecordKind,
    tag: Option<Symbol>,
    fields: Option<Vec<FieldDecl>>,
) -> RecordSpec {
    RecordSpec { id: NodeId(0), kind, tag, fields, span: DUMMY_SP, attrs: Vec::new() }
}

fn named_field(name: Symbol, type_specs: Vec<TypeSpec>) -> FieldDecl {
    FieldDecl {
        specs: DeclSpecs { type_specs, ..DeclSpecs::default() },
        declarators: vec![FieldDeclarator {
            declarator: Some(Declarator {
                name: Some((name, DUMMY_SP)),
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            }),
            bit_width: None,
        }],
        span: DUMMY_SP,
    }
}

fn bitfield_field(name: Option<Symbol>, type_specs: Vec<TypeSpec>, width: Expr) -> FieldDecl {
    FieldDecl {
        specs: DeclSpecs { type_specs, ..DeclSpecs::default() },
        declarators: vec![FieldDeclarator {
            declarator: name.map(|n| Declarator {
                name: Some((n, DUMMY_SP)),
                derived: Vec::new(),
                span: DUMMY_SP,
                attrs: Vec::new(),
            }),
            bit_width: Some(width),
        }],
        span: DUMMY_SP,
    }
}

fn enum_spec(tag: Option<Symbol>, variants: Vec<(Symbol, Option<Expr>)>) -> EnumSpec {
    EnumSpec {
        id: NodeId(0),
        tag,
        enumerators: Some(
            variants
                .into_iter()
                .map(|(name, value)| rcc_ast::Enumerator {
                    name,
                    value,
                    span: DUMMY_SP,
                    attrs: Vec::new(),
                })
                .collect(),
        ),
        span: DUMMY_SP,
        attrs: Vec::new(),
    }
}

/// Push a synthetic LocalRef expression so initializer / assignment
/// helpers have an lvalue target. Returns the `HirExprId` of the new
/// node and the `Local` that backs it.
fn push_local_lvalue(
    body: &mut Body,
    scope: &mut ScopeStack,
    name: Symbol,
    ty: TyId,
) -> (Local, rcc_hir::HirExprId) {
    let local = body.locals.push(LocalDecl {
        name: Some(name),
        ty,
        quals: ObjectQuals::none(),
        vla_len: None,
        is_param: false,
        span: DUMMY_SP,
    });
    scope.insert(name, Binding::Local(local));
    let id = body.exprs.push(rcc_hir::HirExpr {
        id: rcc_hir::HirExprId(0),
        ty,
        value_cat: ValueCat::LValue,
        span: DUMMY_SP,
        kind: HirExprKind::LocalRef(local),
    });
    body.exprs[id].id = id;
    (local, id)
}

// ═══════════════════════════════════════════════════════════════════════
// Section A — C99 §6.7.5 declarator examples
//
// One row per canonical declarator from §6.7.5. Built programmatically
// so that the chain shape is unambiguous; `apply_declarator` is the
// system under test.
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn s6_7_5_int_x() {
    // int x;
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, Vec::new());
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    assert_eq!(ty, tcx.int);
}

#[test]
fn s6_7_5_pointer_to_int() {
    // int *x;
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![ptr()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    assert_eq!(ty, tcx.intern(Ty::Ptr(Qual::plain(tcx.int))));
}

#[test]
fn s6_7_5_incomplete_array() {
    // int x[];
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![array_unsized()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let expected = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_sized_array() {
    // int x[10];
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![array_size(10, &mut sess)]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let expected =
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(10), is_vla: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_block_runtime_array_bound_is_vla() {
    // int x[n]; at block scope is a VLA: the constant length is unknown,
    // but the type records that it must be dynamically sized.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![array_runtime_size("n", &mut sess)]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::Block, &mut tcx, &mut sess);
    let expected = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: true });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_array_of_pointers() {
    // int *x[10]; — array of 10 pointers to int.
    // Right-left: x → [10] (innermost) → * → int.
    // Stored outermost-to-innermost: [Pointer, Array(10)].
    // Forward iteration: int → Ptr(int) → Array[10](Ptr(int)).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![ptr(), array_size(10, &mut sess)]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let ptr_int = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
    let expected =
        tcx.intern(Ty::Array { elem: Qual::plain(ptr_int), len: Some(10), is_vla: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_pointer_to_array() {
    // int (*x)[10]; — pointer to array of 10 ints.
    // Right-left: x → * (innermost) → [10] → int.
    // Stored outermost-to-innermost: [Array(10), Pointer].
    // Forward iteration: int → Array[10](int) → Ptr(Array[10](int)).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![array_size(10, &mut sess), ptr()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let arr = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(10), is_vla: false });
    let expected = tcx.intern(Ty::Ptr(Qual::plain(arr)));
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_function_returning_int() {
    // int x();  — old-style function (no prototype).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![func_no_params()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let expected =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_function_returning_pointer() {
    // int *x();  — function returning a pointer to int.
    // Right-left: x → () (innermost) → * → int.
    // Stored outermost-to-innermost: [Pointer, Function].
    // Forward iteration: int → Ptr(int) → Func()->Ptr(int).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![ptr(), func_no_params()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let ptr_int = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
    let expected =
        tcx.intern(Ty::Func { ret: ptr_int, params: Vec::new(), variadic: false, proto: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_pointer_to_function() {
    // int (*x)(); — pointer to function returning int.
    // Right-left: x → * (innermost) → () → int.
    // Stored outermost-to-innermost: [Function, Pointer].
    // Forward iteration: int → Func()->int → Ptr(Func()->int).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let d = named(x, vec![func_no_params(), ptr()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let func =
        tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: false });
    let expected = tcx.intern(Ty::Ptr(Qual::plain(func)));
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_array_of_pointers_to_function() {
    // int (*fp[3])(int);  — the canonical "spiral" example.
    // Right-left: fp → [3] (innermost) → * → (int) → int.
    // Stored outermost-to-innermost: [Function([int]), Pointer, Array(3)].
    // Forward iteration:
    //   int → Func(int)->int → Ptr(Func) → Array[3](Ptr(Func)).
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let fp = intern(&mut sess, "fp");
    let d = named(
        fp,
        vec![func_decl(vec![param_int()], false, false), ptr(), array_size(3, &mut sess)],
    );
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let func =
        tcx.intern(Ty::Func { ret: tcx.int, params: vec![tcx.int], variadic: false, proto: true });
    let ptr_func = tcx.intern(Ty::Ptr(Qual::plain(func)));
    let expected =
        tcx.intern(Ty::Array { elem: Qual::plain(ptr_func), len: Some(3), is_vla: false });
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_void_pointer() {
    // void *p;  — pointer to void is legal even though `void p;` is not.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let p = intern(&mut sess, "p");
    let d = named(p, vec![ptr()]);
    let ty = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);
    assert_eq!(ty, tcx.intern(Ty::Ptr(Qual::plain(tcx.void))));
}

#[test]
fn s6_7_5_const_pointer_qualifier_does_not_qualify_pointee() {
    // int * const p;
    // The `const` after `*` qualifies the pointer object, not the pointee.
    // The low-level type builder has no object metadata return slot, so it
    // leaves the pointee unqualified; full declaration lowering records the
    // final pointer qualifier in `ObjectQuals`.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let p = intern(&mut sess, "p");
    let d = named(p, vec![const_ptr()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    assert_eq!(ty, tcx.intern(Ty::Ptr(Qual::plain(tcx.int))));
}

#[test]
fn s6_7_5_pointer_to_pointer() {
    // int **pp;  — double pointer.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let pp = intern(&mut sess, "pp");
    let d = named(pp, vec![ptr(), ptr()]);
    let ty = apply_declarator(tcx.int, &d, DeclScope::File, &mut tcx, &mut sess);
    let inner = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
    let expected = tcx.intern(Ty::Ptr(Qual::plain(inner)));
    assert_eq!(ty, expected);
}

#[test]
fn s6_7_5_function_with_void_param() {
    // void f(void); — prototype with explicit `(void)` parameter.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let f = intern(&mut sess, "f");
    let d = named(f, vec![func_void_params()]);
    let ty = apply_declarator(tcx.void, &d, DeclScope::File, &mut tcx, &mut sess);
    let expected =
        tcx.intern(Ty::Func { ret: tcx.void, params: Vec::new(), variadic: false, proto: true });
    assert_eq!(ty, expected);
}

// ═══════════════════════════════════════════════════════════════════════
// Section B — Name resolution (ordinary / tag / label namespaces)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn resolution_undeclared_emits_e0071() {
    let (mut sess, cap) = Session::for_test();
    let unknown = intern(&mut sess, "z");
    let resolver = Resolver::default();
    let scope = ScopeStack::new();
    let result = resolve_expr_ident(unknown, DUMMY_SP, &scope, &resolver, &mut sess);
    assert!(result.is_none());
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, Some("E0071"));
}

#[test]
fn resolution_inner_block_shadows_outer() {
    // void f(){ int x; { int x; /*inner*/ } /*outer*/ }
    let (mut sess, _cap) = Session::for_test();
    let x = intern(&mut sess, "x");
    let resolver = Resolver::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    scope.insert(x, Binding::Local(Local(0)));
    scope.push_scope();
    scope.insert(x, Binding::Local(Local(1)));
    match resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess) {
        Some(HirExprKind::LocalRef(l)) => assert_eq!(l, Local(1)),
        other => panic!("expected LocalRef(1), got {other:?}"),
    }
    scope.pop_scope();
    match resolve_expr_ident(x, DUMMY_SP, &scope, &resolver, &mut sess) {
        Some(HirExprKind::LocalRef(l)) => assert_eq!(l, Local(0)),
        other => panic!("expected LocalRef(0), got {other:?}"),
    }
}

#[test]
fn resolution_tags_and_ordinary_are_independent() {
    // `struct S { int x; }; int S = 1;` — tag `S` and ordinary `S`
    // live in different namespaces, no clash.
    let (hir, _tcx) = lower_snippet("struct S { int x; }; int S = 1;");
    // Two definitions: the struct tag and the global int.
    assert_eq!(hir.defs.len(), 2);
    let kinds: Vec<_> = hir
        .defs
        .iter()
        .map(|d| match d.kind {
            DefKind::Record { .. } => "record",
            DefKind::Global { .. } => "global",
            _ => "other",
        })
        .collect();
    assert!(kinds.contains(&"record"));
    assert!(kinds.contains(&"global"));
}

#[test]
fn resolution_tag_kind_mismatch_e0072() {
    // `struct S {}; union S;` → E0072
    let (mut sess, cap) = Session::for_test();
    let s = intern(&mut sess, "S");
    let tcx = TyCtxt::new();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();

    let id = resolve_tag(s, DUMMY_SP, TagKind::Struct, &mut crate_, &tcx, &mut resolver, &mut sess);
    assert!(id.is_some());

    let result =
        resolve_tag(s, DUMMY_SP, TagKind::Union, &mut crate_, &tcx, &mut resolver, &mut sess);
    assert!(result.is_none());
    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, Some("E0072"));
}

#[test]
fn resolution_label_undefined_emits_e0073() {
    // void f(){ goto missing; }
    let (mut sess, cap) = Session::for_test();
    let missing = intern(&mut sess, "missing");
    let body = block_of(vec![stmt_of(StmtKind::Goto(missing))]);
    let mut resolver = Resolver::default();
    resolve_labels(&body, &mut resolver, &mut sess);
    let diags = cap.diagnostics();
    assert!(diags.iter().any(|d| d.code == Some("E0073")), "expected E0073, got {diags:?}");
}

#[test]
fn resolution_label_duplicate_emits_e0074() {
    // void f(){ x:; x:; }
    let (mut sess, cap) = Session::for_test();
    let x = intern(&mut sess, "x");
    let body = block_of(vec![
        stmt_of(StmtKind::Label { name: x, body: Box::new(stmt_of(StmtKind::Null)) }),
        stmt_of(StmtKind::Label { name: x, body: Box::new(stmt_of(StmtKind::Null)) }),
    ]);
    let mut resolver = Resolver::default();
    resolve_labels(&body, &mut resolver, &mut sess);
    let diags = cap.diagnostics();
    assert!(diags.iter().any(|d| d.code == Some("E0074")), "expected E0074, got {diags:?}");
}

#[test]
fn resolution_typedef_via_lower_typedef_name() {
    // `typedef int T;` — T resolves to tcx.int (interned singleton).
    let (mut sess, _cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let t = intern(&mut sess, "T");
    let id = crate_.defs.push(rcc_hir::Def {
        id: DefId(0),
        name: t,
        span: DUMMY_SP,
        kind: DefKind::Typedef(tcx.int),
    });
    crate_.defs[id].id = id;
    resolver.ordinary.insert(t, id);

    let mut expanding = rcc_data_structures::FxHashSet::default();
    let resolved =
        lower_typedef_name(t, DUMMY_SP, &mut expanding, &resolver, &crate_, &tcx, &mut sess);
    assert_eq!(resolved, tcx.int);
}

// ═══════════════════════════════════════════════════════════════════════
// Section C — Composite (struct / union / enum) lowering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn composite_struct_two_named_fields() {
    // struct S { int a; int b; }
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let a = intern(&mut sess, "a");
    let b = intern(&mut sess, "b");
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
            assert_eq!(fields[1].name, Some(b));
        }
        other => panic!("expected struct, got {other:?}"),
    }
}

#[test]
fn composite_struct_with_named_bitfield() {
    // struct S { int x : 3; }
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let x = intern(&mut sess, "x");
    let spec = record_spec(
        rcc_ast::RecordKind::Struct,
        None,
        Some(vec![bitfield_field(Some(x), vec![TypeSpec::Int], int_lit("3", &mut sess))]),
    );
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
    match kind {
        DefKind::Record { fields, .. } => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].name, Some(x));
            assert_eq!(fields[0].bit_width, Some(3));
        }
        other => panic!("expected record, got {other:?}"),
    }
}

#[test]
fn composite_union_kind_propagates() {
    // union U { int a; long b; }
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let a = intern(&mut sess, "a");
    let b = intern(&mut sess, "b");
    let spec = record_spec(
        rcc_ast::RecordKind::Union,
        None,
        Some(vec![named_field(a, vec![TypeSpec::Int]), named_field(b, vec![TypeSpec::Long])]),
    );
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let kind = lower_record(&spec, &mut tcx, &mut resolver, &mut crate_, &mut sess);
    assert!(matches!(kind, DefKind::Record { kind: RecordKind::Union, .. }));
}

#[test]
fn composite_anonymous_struct_member_flattens() {
    // struct Outer { struct { int a; int b; }; int c; }
    // Field list seen from outside: [a, b, c].
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let a = intern(&mut sess, "a");
    let b = intern(&mut sess, "b");
    let c = intern(&mut sess, "c");

    // Anonymous inner struct declared as the entire field's specifier.
    let inner = record_spec(
        rcc_ast::RecordKind::Struct,
        None,
        Some(vec![named_field(a, vec![TypeSpec::Int]), named_field(b, vec![TypeSpec::Int])]),
    );
    let inner_field = FieldDecl {
        specs: DeclSpecs { type_specs: vec![TypeSpec::Record(inner)], ..DeclSpecs::default() },
        declarators: vec![FieldDeclarator { declarator: None, bit_width: None }],
        span: DUMMY_SP,
    };
    let outer = record_spec(
        rcc_ast::RecordKind::Struct,
        None,
        Some(vec![inner_field, named_field(c, vec![TypeSpec::Int])]),
    );
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let kind = lower_record(&outer, &mut tcx, &mut resolver, &mut crate_, &mut sess);
    match kind {
        DefKind::Record { fields, .. } => {
            let names: Vec<_> = fields.iter().map(|f| f.name).collect();
            assert_eq!(names, vec![Some(a), Some(b), Some(c)]);
        }
        other => panic!("expected record, got {other:?}"),
    }
}

#[test]
fn composite_enum_default_values() {
    // enum { A, B, C } — values 0, 1, 2.
    let (mut sess, _cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let a = intern(&mut sess, "A");
    let b = intern(&mut sess, "B");
    let c = intern(&mut sess, "C");
    let spec = enum_spec(None, vec![(a, None), (b, None), (c, None)]);
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
    match kind {
        DefKind::Enum { variants, .. } => {
            let values: Vec<i128> = variants.iter().map(|v| v.value).collect();
            assert_eq!(values, vec![0, 1, 2]);
        }
        other => panic!("expected enum, got {other:?}"),
    }
}

#[test]
fn composite_enum_explicit_resets_counter() {
    // enum { A = 5, B, C = 10, D } — values 5, 6, 10, 11.
    let (mut sess, _cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let a = intern(&mut sess, "A");
    let b = intern(&mut sess, "B");
    let c = intern(&mut sess, "C");
    let d = intern(&mut sess, "D");
    let spec = enum_spec(
        None,
        vec![
            (a, Some(int_lit("5", &mut sess))),
            (b, None),
            (c, Some(int_lit("10", &mut sess))),
            (d, None),
        ],
    );
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);
    match kind {
        DefKind::Enum { variants, .. } => {
            let values: Vec<i128> = variants.iter().map(|v| v.value).collect();
            assert_eq!(values, vec![5, 6, 10, 11]);
        }
        other => panic!("expected enum, got {other:?}"),
    }
}

#[test]
fn composite_enum_duplicate_emits_e0078() {
    // enum { A }; enum { A = 1 };  — second `A` is duplicate.
    let (mut sess, cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let a = intern(&mut sess, "A");
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();
    let s1 = enum_spec(None, vec![(a, None)]);
    let _ = lower_enum(&s1, &tcx, &mut resolver, &mut crate_, &mut sess);
    let s2 = enum_spec(None, vec![(a, Some(int_lit("1", &mut sess)))]);
    let _ = lower_enum(&s2, &tcx, &mut resolver, &mut crate_, &mut sess);
    let diags = cap.diagnostics();
    assert!(diags.iter().any(|d| d.code == Some("E0078")), "expected E0078, got {diags:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Section D — Initializer lowering
// ═══════════════════════════════════════════════════════════════════════

fn local_array_int_writes(body: &Body, local: Local) -> Vec<(i128, i128)> {
    let mut writes = Vec::new();
    for stmt in body.stmts.iter() {
        let HirStmtKind::Expr(assign) = stmt.kind else { continue };
        let HirExprKind::Assign { lhs, rhs } = &body.exprs[assign].kind else {
            continue;
        };
        let HirExprKind::Index { base, index } = &body.exprs[*lhs].kind else {
            continue;
        };
        if !matches!(&body.exprs[*base].kind, HirExprKind::LocalRef(l) if *l == local) {
            continue;
        }
        let HirExprKind::IntConst(i) = &body.exprs[*index].kind else {
            continue;
        };
        let HirExprKind::IntConst(v) = &body.exprs[*rhs].kind else {
            continue;
        };
        writes.push((*i, *v));
    }
    writes
}

fn local_field_array_int_writes(body: &Body, local: Local, field_index: u32) -> Vec<(i128, i128)> {
    let mut writes = Vec::new();
    for stmt in body.stmts.iter() {
        let HirStmtKind::Expr(assign) = stmt.kind else { continue };
        let HirExprKind::Assign { lhs, rhs } = &body.exprs[assign].kind else {
            continue;
        };
        let HirExprKind::Index { base, index } = &body.exprs[*lhs].kind else {
            continue;
        };
        let HirExprKind::Field { base: field_base, field_index: field } = &body.exprs[*base].kind
        else {
            continue;
        };
        if *field != field_index {
            continue;
        }
        if !matches!(&body.exprs[*field_base].kind, HirExprKind::LocalRef(l) if *l == local) {
            continue;
        }
        let HirExprKind::IntConst(i) = &body.exprs[*index].kind else {
            continue;
        };
        let HirExprKind::IntConst(v) = &body.exprs[*rhs].kind else {
            continue;
        };
        writes.push((*i, *v));
    }
    writes
}

fn last_values_by_index(writes: &[(i128, i128)]) -> BTreeMap<i128, i128> {
    let mut values = BTreeMap::new();
    for (idx, value) in writes {
        values.insert(*idx, *value);
    }
    values
}

#[test]
fn init_scalar_assigns_rhs_to_target() {
    // int x; x = 7;  — scalar init produces a single Assign.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let x = intern(&mut sess, "x");
    let (_local, target) = push_local_lvalue(&mut body, &mut scope, x, tcx.int);

    let init = Initializer::Expr(int_lit("7", &mut sess));
    let mut out = Vec::new();
    lower_initializer(
        target,
        tcx.int,
        &init,
        DUMMY_SP,
        &mut body,
        &scope,
        &mut crate_,
        &mut tcx,
        &mut resolver,
        &mut sess,
        &mut out,
    );
    assert_eq!(out.len(), 1);
    let assign_expr = match body.stmts[out[0]].kind {
        HirStmtKind::Expr(eid) => eid,
        ref other => panic!("expected Expr stmt, got {other:?}"),
    };
    let HirExprKind::Assign { rhs, .. } = body.exprs[assign_expr].kind else {
        panic!("expected Assign, got {:?}", body.exprs[assign_expr].kind);
    };
    assert!(matches!(body.exprs[rhs].kind, HirExprKind::IntConst(7)));
}

#[test]
fn init_array_partial_zero_fills_tail() {
    // int a[3] = {1};  — a[1] and a[2] zero-filled.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let a = intern(&mut sess, "a");
    let arr_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
    let (_local, target) = push_local_lvalue(&mut body, &mut scope, a, arr_ty);

    let init = Initializer::List(vec![(Vec::new(), Initializer::Expr(int_lit("1", &mut sess)))]);
    let mut out = Vec::new();
    lower_initializer(
        target,
        arr_ty,
        &init,
        DUMMY_SP,
        &mut body,
        &scope,
        &mut crate_,
        &mut tcx,
        &mut resolver,
        &mut sess,
        &mut out,
    );
    // Three assignments — a[0]=1, a[1]=0, a[2]=0.
    assert_eq!(out.len(), 3);
    let mut idx_value: Vec<(i128, i128)> = Vec::new();
    for sid in &out {
        let HirStmtKind::Expr(eid) = body.stmts[*sid].kind else { continue };
        let HirExprKind::Assign { lhs, rhs } = body.exprs[eid].kind else { continue };
        let HirExprKind::Index { index, .. } = body.exprs[lhs].kind else { continue };
        let HirExprKind::IntConst(i) = body.exprs[index].kind else { continue };
        let HirExprKind::IntConst(v) = body.exprs[rhs].kind else { continue };
        idx_value.push((i, v));
    }
    idx_value.sort();
    assert_eq!(idx_value, vec![(0, 1), (1, 0), (2, 0)]);
}

#[test]
fn init_array_designator_resets_cursor() {
    // int a[3] = { [2] = 7 };  — a[2]=7, a[0]=0, a[1]=0.
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let a = intern(&mut sess, "a");
    let arr_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
    let (_local, target) = push_local_lvalue(&mut body, &mut scope, a, arr_ty);

    let init = Initializer::List(vec![(
        vec![rcc_ast::Designator::Index(int_lit("2", &mut sess))],
        Initializer::Expr(int_lit("7", &mut sess)),
    )]);
    let mut out = Vec::new();
    lower_initializer(
        target,
        arr_ty,
        &init,
        DUMMY_SP,
        &mut body,
        &scope,
        &mut crate_,
        &mut tcx,
        &mut resolver,
        &mut sess,
        &mut out,
    );
    assert_eq!(out.len(), 3);
    let mut idx_value: Vec<(i128, i128)> = Vec::new();
    for sid in &out {
        let HirStmtKind::Expr(eid) = body.stmts[*sid].kind else { continue };
        let HirExprKind::Assign { lhs, rhs } = body.exprs[eid].kind else { continue };
        let HirExprKind::Index { index, .. } = body.exprs[lhs].kind else { continue };
        let HirExprKind::IntConst(i) = body.exprs[index].kind else { continue };
        let HirExprKind::IntConst(v) = body.exprs[rhs].kind else { continue };
        idx_value.push((i, v));
    }
    idx_value.sort();
    assert_eq!(idx_value, vec![(0, 0), (1, 0), (2, 7)]);
}

#[test]
fn snippet_gnu_range_designator_lowers_local_array_writes() {
    let (hir, _tcx) = lower_snippet("void f(void) { int a[8] = { [1 ... 5] = 9 }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let mut writes = local_array_int_writes(body, Local(0));
    writes.sort();
    assert_eq!(writes, vec![(0, 0), (1, 9), (2, 9), (3, 9), (4, 9), (5, 9), (6, 0), (7, 0)]);
}

#[test]
fn snippet_gnu_range_designator_later_initializer_overrides() {
    let (hir, _tcx) = lower_snippet("void f(void) { int a[4] = { [1 ... 3] = 1, [2] = 9 }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let writes = local_array_int_writes(body, Local(0));
    assert_eq!(
        writes,
        vec![(1, 1), (2, 1), (3, 1), (2, 9), (0, 0)],
        "writes must preserve source order so later entries override earlier ones"
    );
    let final_values = last_values_by_index(&writes);
    assert_eq!(final_values.get(&2), Some(&9));
}

#[test]
fn snippet_gnu_range_designator_lowers_nested_array_field() {
    let (hir, _tcx) = lower_snippet(
        "struct S { int a[4]; int b; }; void f(void) { struct S s = { .a[1 ... 2] = 7 }; }",
    );
    let body = hir.bodies.values().next().expect("missing function body");
    let mut writes = local_field_array_int_writes(body, Local(0), 0);
    writes.sort();
    assert_eq!(writes, vec![(0, 0), (1, 7), (2, 7), (3, 0)]);
}

#[test]
fn init_record_per_field_assign() {
    // struct S { int a; int b; }; struct S s = { 1, 2 };
    let (mut sess, _cap) = Session::for_test();
    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();

    // Hand-build a Record def + matching TyId for the test target.
    let a = intern(&mut sess, "a");
    let b = intern(&mut sess, "b");
    let rec_id = crate_.defs.push(rcc_hir::Def {
        id: DefId(0),
        name: intern(&mut sess, "S"),
        span: DUMMY_SP,
        kind: DefKind::Record {
            kind: RecordKind::Struct,
            layout: None,
            fields: vec![
                rcc_hir::Field {
                    name: Some(a),
                    ty: tcx.int,
                    quals: ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                rcc_hir::Field {
                    name: Some(b),
                    ty: tcx.int,
                    quals: ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
            ],
        },
    });
    crate_.defs[rec_id].id = rec_id;
    let rec_ty = tcx.intern(Ty::Record(rec_id));
    let s = intern(&mut sess, "s");
    let (_local, target) = push_local_lvalue(&mut body, &mut scope, s, rec_ty);

    let init = Initializer::List(vec![
        (Vec::new(), Initializer::Expr(int_lit("1", &mut sess))),
        (Vec::new(), Initializer::Expr(int_lit("2", &mut sess))),
    ]);
    let mut out = Vec::new();
    lower_initializer(
        target,
        rec_ty,
        &init,
        DUMMY_SP,
        &mut body,
        &scope,
        &mut crate_,
        &mut tcx,
        &mut resolver,
        &mut sess,
        &mut out,
    );
    // Two assigns — one per field.
    assert!(out.len() >= 2, "expected at least two field assigns, got {}", out.len());
    let mut field_values: Vec<i128> = Vec::new();
    for sid in &out {
        let HirStmtKind::Expr(eid) = body.stmts[*sid].kind else { continue };
        let HirExprKind::Assign { rhs, .. } = body.exprs[eid].kind else { continue };
        if let HirExprKind::IntConst(v) = body.exprs[rhs].kind {
            field_values.push(v);
        }
    }
    field_values.sort();
    assert_eq!(field_values, vec![1, 2]);
}

// ═══════════════════════════════════════════════════════════════════════
// Section E — Statement and expression lowering
// ═══════════════════════════════════════════════════════════════════════

fn fresh_lower_ctx() -> (Body, ScopeStack, HirCrate, TyCtxt, Resolver) {
    let body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    (body, scope, HirCrate::default(), TyCtxt::new(), Resolver::default())
}

#[test]
fn stmt_for_init_declaration_creates_local() {
    // for (int i = 0; i < 3; i++) ;
    let (mut sess, _cap) = Session::for_test();
    let (mut body, mut scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();

    let i = intern(&mut sess, "i");
    let init_decl = Decl {
        id: NodeId(0),
        span: DUMMY_SP,
        specs: DeclSpecs { type_specs: vec![TypeSpec::Int], ..DeclSpecs::default() },
        inits: vec![InitDeclarator {
            declarator: named(i, Vec::new()),
            init: Some(Initializer::Expr(int_lit("0", &mut sess))),
        }],
    };
    let cond = Expr {
        id: NodeId(0),
        kind: ExprKind::Binary {
            op: rcc_ast::BinOp::Lt,
            lhs: Box::new(ident_expr(&mut sess, "i")),
            rhs: Box::new(int_lit("3", &mut sess)),
        },
        span: DUMMY_SP,
    };
    let step = Expr {
        id: NodeId(0),
        kind: ExprKind::Unary {
            op: rcc_ast::UnOp::PostInc,
            operand: Box::new(ident_expr(&mut sess, "i")),
        },
        span: DUMMY_SP,
    };
    let s = stmt_of(StmtKind::For {
        init: Some(Box::new(BlockItem::Decl(init_decl))),
        cond: Some(Box::new(cond)),
        step: Some(Box::new(step)),
        body: Box::new(stmt_of(StmtKind::Null)),
    });
    let id = lower_stmt(&s, &mut body, &mut scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
    let HirStmtKind::For { init, cond, step, .. } = &body.stmts[id].kind else {
        panic!("expected For, got {:?}", body.stmts[id].kind);
    };
    assert!(init.is_some());
    assert!(cond.is_some());
    assert!(step.is_some());
    // i must have been added as a local.
    assert!(body.locals.iter().any(|l| l.name == Some(i)));
}

#[test]
fn stmt_return_value_lowers_to_return_some() {
    let (mut sess, _cap) = Session::for_test();
    let (mut body, mut scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();
    let s = stmt_of(StmtKind::Return(Some(int_lit("42", &mut sess))));
    let id = lower_stmt(&s, &mut body, &mut scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
    let HirStmtKind::Return(Some(eid)) = body.stmts[id].kind else {
        panic!("expected Return(Some)");
    };
    assert!(matches!(body.exprs[eid].kind, HirExprKind::IntConst(42)));
}

#[test]
fn expr_ternary_creates_cond_node() {
    // 1 ? 2 : 3
    let (mut sess, _cap) = Session::for_test();
    let (mut body, scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();
    let e = Expr {
        id: NodeId(0),
        kind: ExprKind::Cond {
            cond: Box::new(int_lit("1", &mut sess)),
            then_expr: Box::new(int_lit("2", &mut sess)),
            else_expr: Box::new(int_lit("3", &mut sess)),
        },
        span: DUMMY_SP,
    };
    let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
    let HirExprKind::Cond { cond, then_expr, else_expr } = body.exprs[id].kind else {
        panic!("expected Cond");
    };
    assert!(matches!(body.exprs[cond].kind, HirExprKind::IntConst(1)));
    assert!(matches!(body.exprs[then_expr].kind, HirExprKind::IntConst(2)));
    assert!(matches!(body.exprs[else_expr].kind, HirExprKind::IntConst(3)));
}

#[test]
fn expr_member_access_preserves_requested_field_name() {
    // Ad-hoc: synthesise a struct local + `s.a` member access.
    let (mut sess, _cap) = Session::for_test();
    let (mut body, mut scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();
    let a = intern(&mut sess, "a");
    let rec_id = crate_.defs.push(rcc_hir::Def {
        id: DefId(0),
        name: intern(&mut sess, "S"),
        span: DUMMY_SP,
        kind: DefKind::Record {
            kind: RecordKind::Struct,
            layout: None,
            fields: vec![rcc_hir::Field {
                name: Some(a),
                ty: tcx.int,
                quals: ObjectQuals::none(),
                offset: None,
                bit_width: None,
                span: DUMMY_SP,
            }],
        },
    });
    crate_.defs[rec_id].id = rec_id;
    let rec_ty = tcx.intern(Ty::Record(rec_id));
    let s = intern(&mut sess, "s");
    let _ = push_local_lvalue(&mut body, &mut scope, s, rec_ty);

    let e = Expr {
        id: NodeId(0),
        kind: ExprKind::Member { base: Box::new(ident_expr(&mut sess, "s")), field: a },
        span: DUMMY_SP,
    };
    let id = lower_expr(&e, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
    match body.exprs[id].kind {
        HirExprKind::UnresolvedField { field, .. } => assert_eq!(field, a),
        ref other => {
            panic!("expected unresolved member access preserving field name, got {other:?}")
        }
    }
}

#[test]
fn expr_compound_assign_desugars_to_simple_assign() {
    // x += 1   should lower to   x = x + 1   (i.e. an Assign whose RHS
    // is a Binary { Add, x, 1 }).
    let (mut sess, _cap) = Session::for_test();
    let (mut body, mut scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();
    let x = intern(&mut sess, "x");
    let _ = push_local_lvalue(&mut body, &mut scope, x, tcx.int);

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
    let HirExprKind::Assign { rhs, .. } = body.exprs[id].kind else {
        panic!("expected Assign");
    };
    assert!(matches!(
        body.exprs[rhs].kind,
        HirExprKind::Binary { op: rcc_hir::rcc_hir_binop::BinOp::Add, .. }
    ));
}

#[test]
fn expr_paren_does_not_create_extra_node() {
    // (((42))) — paren wrappers are transparent.
    let (mut sess, _cap) = Session::for_test();
    let (mut body, scope, mut crate_, mut tcx, mut resolver) = fresh_lower_ctx();
    let lit = int_lit("42", &mut sess);
    let paren = Expr {
        id: NodeId(0),
        kind: ExprKind::Paren(Box::new(Expr {
            id: NodeId(0),
            kind: ExprKind::Paren(Box::new(Expr {
                id: NodeId(0),
                kind: ExprKind::Paren(Box::new(lit)),
                span: DUMMY_SP,
            })),
            span: DUMMY_SP,
        })),
        span: DUMMY_SP,
    };
    let before = body.exprs.len();
    let id = lower_expr(&paren, &mut body, &scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);
    assert_eq!(body.exprs.len() - before, 1, "paren should not add nodes");
    assert!(matches!(body.exprs[id].kind, HirExprKind::IntConst(42)));
}

// ═══════════════════════════════════════════════════════════════════════
// Section F — End-to-end smoke (parse → lower)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn snippet_empty_translation_unit_has_no_defs() {
    let (hir, _tcx) = lower_snippet("");
    assert_eq!(hir.defs.len(), 0);
}

#[test]
fn snippet_function_prototype_yields_one_def() {
    let (hir, _tcx) = lower_snippet("int add(int a, int b);");
    assert_eq!(hir.defs.len(), 1);
    let def = &hir.defs[DefId(0)];
    assert!(matches!(def.kind, DefKind::Global { .. } | DefKind::Function { .. }));
}

#[test]
fn snippet_function_definition_has_body_flag() {
    let (hir, _tcx) = lower_snippet("int main(void) { return 0; }");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Function { .. }))
        .expect("missing function def");
    let DefKind::Function { has_body, .. } = def.kind else {
        unreachable!();
    };
    assert!(has_body, "function definition should have has_body = true");
}

#[test]
fn snippet_vla_decl_preserves_runtime_bound_expr() {
    let (ast, mut sess, _cap) = parse_to_ast("void f(int n) { int a[n]; }");
    let func = match &ast.decls[0] {
        rcc_ast::ExternalDecl::Function(func) => func,
        other => panic!("expected function definition, got {other:?}"),
    };

    let mut tcx = TyCtxt::new();
    let mut body = Body::default();
    let mut scope = ScopeStack::new();
    scope.push_scope();
    let n = intern(&mut sess, "n");
    let n_local = body.locals.push(LocalDecl {
        name: Some(n),
        ty: tcx.int,
        quals: ObjectQuals::none(),
        vla_len: None,
        is_param: true,
        span: DUMMY_SP,
    });
    scope.insert(n, Binding::Local(n_local));

    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let stmt = Stmt { id: NodeId(0), kind: StmtKind::Compound(func.body.clone()), span: DUMMY_SP };
    let _root =
        lower_stmt(&stmt, &mut body, &mut scope, &mut crate_, &mut tcx, &mut resolver, &mut sess);

    let vla = body
        .locals
        .iter()
        .find(|local| matches!(tcx.get(local.ty), Ty::Array { is_vla: true, .. }))
        .expect("expected a VLA local");
    let len_expr = vla.vla_len.expect("VLA local should carry runtime bound expression");
    match body.exprs[len_expr].kind {
        HirExprKind::LocalRef(local) => {
            assert!(body.locals[local].is_param, "VLA bound should refer to parameter n");
        }
        ref other => panic!("expected VLA bound LocalRef(n), got {other:?}"),
    }
}

#[test]
fn snippet_typedef_chain_resolves() {
    // Acceptance: `typedef int T; typedef T U; U x;`
    // x's type is the interned tcx.int singleton.
    let (mut sess, _cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    let t = intern(&mut sess, "T");
    let u = intern(&mut sess, "U");

    // Register T -> int directly, then U -> T's type.
    let t_id = crate_.defs.push(rcc_hir::Def {
        id: DefId(0),
        name: t,
        span: DUMMY_SP,
        kind: DefKind::Typedef(tcx.int),
    });
    crate_.defs[t_id].id = t_id;
    resolver.ordinary.insert(t, t_id);
    let mut exp = rcc_data_structures::FxHashSet::default();
    let t_ty = lower_typedef_name(t, DUMMY_SP, &mut exp, &resolver, &crate_, &tcx, &mut sess);
    let u_id = crate_.defs.push(rcc_hir::Def {
        id: DefId(0),
        name: u,
        span: DUMMY_SP,
        kind: DefKind::Typedef(t_ty),
    });
    crate_.defs[u_id].id = u_id;
    resolver.ordinary.insert(u, u_id);
    let mut exp2 = rcc_data_structures::FxHashSet::default();
    let u_ty = lower_typedef_name(u, DUMMY_SP, &mut exp2, &resolver, &crate_, &tcx, &mut sess);
    assert_eq!(u_ty, tcx.int);
}

#[test]
fn snippet_file_scope_typedef_and_global_types_are_finalized() {
    let (hir, tcx) = lower_snippet("typedef int T; T g;");
    assert_eq!(hir.defs.len(), 2);
    assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Typedef(ty) if ty == tcx.int));
    assert!(matches!(hir.defs[DefId(1)].kind, DefKind::Global { ty, .. } if ty == tcx.int));
}

#[test]
fn snippet_file_scope_multiple_declarators_get_distinct_types() {
    let (hir, tcx) = lower_snippet("int *p, a[3];");
    assert_eq!(hir.defs.len(), 2);

    let DefKind::Global { ty: p_ty, .. } = hir.defs[DefId(0)].kind else {
        panic!("expected p global");
    };
    match tcx.get(p_ty) {
        Ty::Ptr(pointee) => assert_eq!(pointee.ty, tcx.int),
        other => panic!("expected p to be pointer-to-int, got {other:?}"),
    }

    let DefKind::Global { ty: a_ty, .. } = hir.defs[DefId(1)].kind else {
        panic!("expected a global");
    };
    match tcx.get(a_ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected a to be int[3], got {other:?}"),
    }
}

#[test]
fn snippet_extern_then_definition_globals_are_both_typed() {
    let (hir, tcx) = lower_snippet("extern int x; int x;");
    let tys: Vec<TyId> = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .collect();
    assert_eq!(tys.len(), 2);
    assert!(tys.iter().all(|ty| *ty == tcx.int));
}

#[test]
fn snippet_function_declaration_global_has_function_type() {
    let (hir, tcx) = lower_snippet("int f(int);");
    let DefKind::Global { ty, .. } = hir.defs[DefId(0)].kind else {
        panic!("expected file-scope function declaration as ordinary global def");
    };
    match tcx.get(ty) {
        Ty::Func { ret, params, variadic: false, proto: true } => {
            assert_eq!(*ret, tcx.int);
            assert_eq!(params.as_slice(), &[tcx.int]);
        }
        other => panic!("expected function type, got {other:?}"),
    }
}

#[test]
fn snippet_block_typedef_lowers_later_local_type() {
    let (hir, tcx) = lower_snippet("void f(void) { typedef long T; T x; }");
    assert!(
        hir.defs.iter().any(|def| matches!(def.kind, DefKind::Typedef(ty) if ty == tcx.long)),
        "block typedef should be materialised as a HIR typedef def"
    );
    let body = hir.bodies.values().next().expect("missing function body");
    assert_eq!(body.locals.len(), 1, "typedef should not create a runtime local");
    assert_eq!(body.locals[Local(0)].ty, tcx.long);
    let local_decl_count =
        body.stmts.iter().filter(|stmt| matches!(stmt.kind, HirStmtKind::LocalDecl { .. })).count();
    assert_eq!(local_decl_count, 1, "typedef should not emit a LocalDecl statement");
}

#[test]
fn snippet_block_typedef_shadows_file_scope_typedef() {
    let (hir, tcx) = lower_snippet("typedef int T; void f(void) { typedef long T; T x; }");
    let body = hir.bodies.values().next().expect("missing function body");
    assert_eq!(body.locals.len(), 1);
    assert_eq!(body.locals[Local(0)].ty, tcx.long);
    assert_eq!(
        hir.defs.iter().filter(|def| matches!(def.kind, DefKind::Typedef(_))).count(),
        2,
        "file-scope and block-scope typedef defs should both exist"
    );
}

#[test]
fn snippet_local_object_shadows_outer_typedef_in_expressions() {
    let (hir, tcx) = lower_snippet("typedef int T; void f(void) { int T; T = 1; }");
    let body = hir.bodies.values().next().expect("missing function body");
    assert_eq!(body.locals.len(), 1);
    assert_eq!(body.locals[Local(0)].ty, tcx.int);
    assert!(
        body.stmts.iter().any(|stmt| matches!(stmt.kind, HirStmtKind::Expr(_))),
        "assignment to local T should lower as an expression statement"
    );
}

#[test]
fn snippet_initializer_self_reference_uses_new_local_scope() {
    let (hir, _tcx) = lower_snippet("int x; void f(void) { int x = x; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let init = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt.kind {
            HirStmtKind::LocalDecl { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .expect("missing local initializer");
    assert!(matches!(body.exprs[init].kind, HirExprKind::LocalRef(Local(0))));
}

#[test]
fn snippet_later_declarator_initializer_sees_earlier_local() {
    let (hir, _tcx) = lower_snippet("void f(void) { int a = 1, b = a; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let inits: Vec<_> = body
        .stmts
        .iter()
        .filter_map(|stmt| match stmt.kind {
            HirStmtKind::LocalDecl { init, .. } => init,
            _ => None,
        })
        .collect();
    assert_eq!(inits.len(), 2);
    assert!(matches!(body.exprs[inits[1]].kind, HirExprKind::LocalRef(Local(0))));
}

#[test]
fn snippet_sizeof_initializer_sees_declared_local() {
    let (hir, _tcx) = lower_snippet("void f(void) { int a = sizeof a; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let init = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt.kind {
            HirStmtKind::LocalDecl { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .expect("missing local initializer");
    let HirExprKind::SizeofExpr(inner) = body.exprs[init].kind else {
        panic!("expected sizeof expression");
    };
    assert!(matches!(body.exprs[inner].kind, HirExprKind::LocalRef(Local(0))));
}

#[test]
fn snippet_cast_type_name_preserves_destination_type() {
    let (hir, tcx) = lower_snippet("void f(int x) { (long)x; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let cast_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::Cast { to, .. } => Some(to),
            _ => None,
        })
        .expect("missing cast");
    assert_eq!(cast_ty, tcx.long);
}

#[test]
fn snippet_cast_type_name_resolves_typedef_pointer() {
    let (hir, tcx) = lower_snippet("typedef int T; void f(void) { (T *)0; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let cast_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::Cast { to, .. } => Some(to),
            _ => None,
        })
        .expect("missing cast");
    match tcx.get(cast_ty) {
        Ty::Ptr(pointee) => assert_eq!(pointee.ty, tcx.int),
        other => panic!("expected pointer-to-typedef target, got {other:?}"),
    }
}

#[test]
fn snippet_sizeof_type_preserves_type_name() {
    let (hir, tcx) = lower_snippet("void f(void) { sizeof(int); }");
    let body = hir.bodies.values().next().expect("missing function body");
    let size_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::SizeofType(ty) => Some(ty),
            HirExprKind::IntConst(0) => panic!("sizeof(type) must not lower to zero placeholder"),
            _ => None,
        })
        .expect("missing sizeof(type)");
    assert_eq!(size_ty, tcx.int);
}

#[test]
fn snippet_sizeof_record_type_preserves_completed_record() {
    let (hir, tcx) = lower_snippet("struct S { int x; }; void f(void) { sizeof(struct S); }");
    let record_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match def.kind {
            DefKind::Record { .. } => Some(id),
            _ => None,
        })
        .expect("missing record def");
    let body = hir.bodies.values().next().expect("missing function body");
    let size_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::SizeofType(ty) => Some(ty),
            _ => None,
        })
        .expect("missing sizeof(record)");
    assert!(matches!(tcx.get(size_ty), Ty::Record(id) if *id == record_id));
}

#[test]
fn snippet_sizeof_array_type_preserves_bound() {
    let (hir, tcx) = lower_snippet("void f(void) { sizeof(int[3]); }");
    let body = hir.bodies.values().next().expect("missing function body");
    let size_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::SizeofType(ty) => Some(ty),
            _ => None,
        })
        .expect("missing sizeof(array type)");
    match tcx.get(size_ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected int[3], got {other:?}"),
    }
}

#[test]
fn snippet_sizeof_enum_type_preserves_completed_enum() {
    let (hir, tcx) = lower_snippet("enum E { A }; void f(void) { sizeof(enum E); }");
    let enum_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match def.kind {
            DefKind::Enum { .. } => Some(id),
            _ => None,
        })
        .expect("missing enum def");
    let body = hir.bodies.values().next().expect("missing function body");
    let size_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::SizeofType(ty) => Some(ty),
            _ => None,
        })
        .expect("missing sizeof(enum)");
    assert!(matches!(tcx.get(size_ty), Ty::Enum(id) if *id == enum_id));
}

#[test]
fn snippet_compound_literal_preserves_type_part() {
    let (hir, tcx) = lower_snippet("void f(void) { (int){1}; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let (literal_ty, literal_local, init_stmts) = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { ty, local, ref init_stmts } => {
                Some((ty, local, init_stmts))
            }
            _ => None,
        })
        .expect("missing compound literal");
    assert_eq!(literal_ty, tcx.int);
    assert_eq!(body.locals[literal_local].ty, tcx.int);
    assert_eq!(init_stmts.len(), 1);
    let HirStmtKind::Expr(assign) = body.stmts[init_stmts[0]].kind else {
        panic!("compound literal init must be an assignment expression statement");
    };
    let HirExprKind::Assign { lhs, rhs } = body.exprs[assign].kind else {
        panic!("compound literal init statement must assign");
    };
    assert!(matches!(body.exprs[lhs].kind, HirExprKind::LocalRef(l) if l == literal_local));
    assert!(matches!(body.exprs[rhs].kind, HirExprKind::IntConst(1)));
}

#[test]
fn snippet_compound_literal_address_uses_synthetic_local() {
    let (hir, tcx) = lower_snippet("void f(void) { int *p = &(int){3}; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let (literal_local, init_stmts) = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { local, ref init_stmts, .. } => Some((local, init_stmts)),
            _ => None,
        })
        .expect("missing compound literal");
    assert_eq!(body.locals[literal_local].name, None);
    assert_eq!(body.locals[literal_local].ty, tcx.int);
    assert_eq!(init_stmts.len(), 1);

    let pointer_init = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt.kind {
            HirStmtKind::LocalDecl { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .expect("missing pointer initializer");
    let HirExprKind::AddressOf(operand) = body.exprs[pointer_init].kind else {
        panic!("pointer initializer should be address-of compound literal");
    };
    assert!(
        matches!(body.exprs[operand].kind, HirExprKind::CompoundLiteral { local, .. } if local == literal_local)
    );
}

#[test]
fn snippet_compound_literal_record_initializer_reuses_initializer_walker() {
    let (hir, tcx) =
        lower_snippet("struct S { int x; int y; }; void f(void) { ((struct S){ .x = 1 }).x; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let (literal_ty, literal_local, init_stmts) = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { ty, local, ref init_stmts } => {
                Some((ty, local, init_stmts))
            }
            _ => None,
        })
        .expect("missing record compound literal");
    assert!(matches!(tcx.get(literal_ty), Ty::Record(_)));
    assert_eq!(body.locals[literal_local].ty, literal_ty);
    assert!(
        init_stmts.iter().any(|stmt| {
            let HirStmtKind::Expr(assign) = body.stmts[*stmt].kind else { return false };
            let HirExprKind::Assign { lhs, rhs } = body.exprs[assign].kind else {
                return false;
            };
            matches!(body.exprs[lhs].kind, HirExprKind::Field { base, field_index: 0 }
                if matches!(body.exprs[base].kind, HirExprKind::LocalRef(l) if l == literal_local))
                && matches!(body.exprs[rhs].kind, HirExprKind::IntConst(1))
        }),
        "record compound literal should initialise field x via lower_initializer"
    );
}

#[test]
fn snippet_compound_literal_array_initializer_is_indexable() {
    let (hir, tcx) = lower_snippet("void f(void) { (int[3]){1,2,3}[1]; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let (literal_local, init_stmts) = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { local, ref init_stmts, .. } => Some((local, init_stmts)),
            _ => None,
        })
        .expect("missing array compound literal");
    match tcx.get(body.locals[literal_local].ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected int[3] synthetic local, got {other:?}"),
    }
    assert!(
        init_stmts.iter().any(|stmt| {
            let HirStmtKind::Expr(assign) = body.stmts[*stmt].kind else { return false };
            let HirExprKind::Assign { lhs, rhs } = body.exprs[assign].kind else {
                return false;
            };
            matches!(body.exprs[lhs].kind, HirExprKind::Index { base, index }
                if matches!(body.exprs[base].kind, HirExprKind::LocalRef(l) if l == literal_local)
                    && matches!(body.exprs[index].kind, HirExprKind::IntConst(1)))
                && matches!(body.exprs[rhs].kind, HirExprKind::IntConst(2))
        }),
        "array compound literal should initialise element [1]"
    );
}

#[test]
fn snippet_char_array_string_initializer_completes_length_and_writes_chars() {
    let (hir, tcx) = lower_snippet("void f(void) { char s[] = \"hi\"; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.char_),
        other => panic!("expected completed char[3], got {other:?}"),
    }
    let mut elems = Vec::new();
    for stmt in body.stmts.iter() {
        let HirStmtKind::Expr(assign) = stmt.kind else { continue };
        let HirExprKind::Assign { lhs, rhs } = body.exprs[assign].kind else { continue };
        let HirExprKind::Index { base, index } = body.exprs[lhs].kind else { continue };
        if !matches!(body.exprs[base].kind, HirExprKind::LocalRef(Local(0))) {
            continue;
        }
        let HirExprKind::IntConst(i) = body.exprs[index].kind else { continue };
        let HirExprKind::IntConst(v) = body.exprs[rhs].kind else { continue };
        elems.push((i, v));
    }
    elems.sort();
    assert_eq!(elems, vec![(0, 104), (1, 105), (2, 0)]);
}

#[test]
fn snippet_incomplete_array_list_completes_from_element_count() {
    let (hir, tcx) = lower_snippet("void f(void) { int a[] = {1,2,3}; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected completed int[3], got {other:?}"),
    }
}

#[test]
fn snippet_incomplete_array_designator_completes_from_max_index() {
    let (hir, tcx) = lower_snippet("void f(void) { int a[] = { [4] = 1 }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(5), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected completed int[5], got {other:?}"),
    }
}

#[test]
fn snippet_incomplete_array_range_completes_from_upper_bound() {
    let (hir, tcx) = lower_snippet("void f(void) { int a[] = { [1 ... 4] = 7 }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(5), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected completed int[5], got {other:?}"),
    }
}

#[test]
fn snippet_incomplete_block_array_without_initializer_still_errors() {
    let (_hir, _tcx, cap) = lower_snippet_with_diagnostics("void f(void) { int a[]; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0076)),
        "incomplete block array without initializer should still emit E0076"
    );
}

#[test]
fn snippet_bad_initializer_designator_reports_e0079() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("void f(void) { int a[2] = { .x = 1 }; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0079)),
        "bad initializer designator should emit E0079"
    );
}

#[test]
fn snippet_reversed_range_designator_reports_e0079() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("void f(void) { int a[4] = { [3 ... 1] = 7 }; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0079)),
        "reversed range designator should emit E0079"
    );
}

#[test]
fn snippet_nonconstant_range_designator_reports_e0079() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("void f(void) { int i; int a[4] = { [i ... 2] = 7 }; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0079)),
        "non-constant range bound should emit E0079"
    );
}

#[test]
fn snippet_out_of_bounds_range_designator_reports_e0079() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("void f(void) { int a[2] = { [1 ... 2] = 7 }; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0079)),
        "out-of-bounds range designator should emit E0079"
    );
}

#[test]
fn snippet_global_array_initializer_has_static_payload() {
    let (hir, tcx) = lower_snippet("int g[] = {1,2,3};");
    let def = hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).unwrap();
    let DefKind::Global { ty, init: Some(init), .. } = &def.kind else {
        panic!("expected global with initializer, got {:?}", def.kind);
    };
    match tcx.get(*ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected completed int[3], got {other:?}"),
    }
    assert_eq!(init.ty, *ty);
    let values: Vec<_> = init
        .entries
        .iter()
        .map(|entry| {
            let [GlobalInitDesignator::Index(i)] = entry.path.as_slice() else {
                panic!("expected index path, got {:?}", entry.path);
            };
            let GlobalInitValue::Int(v) = entry.value else {
                panic!("expected int value, got {:?}", entry.value);
            };
            (*i, v)
        })
        .collect();
    assert_eq!(values, vec![(0, 1), (1, 2), (2, 3)]);
}

#[test]
fn snippet_global_array_range_initializer_has_static_payload() {
    let (hir, tcx) = lower_snippet("int g[4] = { [1 ... 2] = 7, [2] = 9 };");
    let def = hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).unwrap();
    let DefKind::Global { ty, init: Some(init), .. } = &def.kind else {
        panic!("expected global with initializer, got {:?}", def.kind);
    };
    match tcx.get(*ty) {
        Ty::Array { elem, len: Some(4), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected int[4], got {other:?}"),
    }
    let values: Vec<_> = init
        .entries
        .iter()
        .map(|entry| {
            let [GlobalInitDesignator::Index(i)] = entry.path.as_slice() else {
                panic!("expected index path, got {:?}", entry.path);
            };
            let GlobalInitValue::Int(v) = entry.value else {
                panic!("expected int value, got {:?}", entry.value);
            };
            (*i, v)
        })
        .collect();
    assert_eq!(values, vec![(1, 7), (2, 7), (2, 9)]);
}

#[test]
fn snippet_global_char_array_string_initializer_has_static_payload() {
    let (hir, tcx) = lower_snippet("char s[] = \"hi\";");
    let def = hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).unwrap();
    let DefKind::Global { ty, init: Some(init), .. } = &def.kind else {
        panic!("expected global with initializer, got {:?}", def.kind);
    };
    match tcx.get(*ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.char_),
        other => panic!("expected completed char[3], got {other:?}"),
    }
    let values: Vec<_> = init
        .entries
        .iter()
        .map(|entry| match entry.value {
            GlobalInitValue::Int(v) => v,
            ref other => panic!("expected int byte, got {other:?}"),
        })
        .collect();
    assert_eq!(values, vec![104, 105, 0]);
}

#[test]
fn snippet_typeck_folds_global_integer_initializer_expr() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("int x = 2 + 3;");
    let (def_id, def) = hir
        .defs
        .iter_enumerated()
        .find(|(_, d)| matches!(d.kind, DefKind::Global { init: Some(_), .. }))
        .expect("missing initialized global");
    let DefKind::Global { init: Some(init), .. } = &def.kind else {
        panic!("expected initialized global");
    };
    assert!(hir.global_init_bodies.contains_key(&def_id));
    assert_eq!(init.entries.len(), 1);
    assert!(init.entries[0].expr.is_some());
    assert!(matches!(init.entries[0].value, GlobalInitValue::Int(5)));
    assert!(
        cap.diagnostics().iter().all(|d| d.code != Some(rcc_errors::codes::E0084)),
        "constant initializer should not emit E0084"
    );
}

#[test]
fn snippet_typeck_folds_global_address_initializer() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("int x; int *p = &x;");
    let globals: Vec<_> = hir
        .defs
        .iter_enumerated()
        .filter(|(_, d)| matches!(d.kind, DefKind::Global { .. }))
        .collect();
    let (x_def, _) = globals[0];
    let (_, p_def) = globals
        .into_iter()
        .find(|(_, d)| matches!(d.kind, DefKind::Global { init: Some(_), .. }))
        .expect("missing pointer initializer");
    let DefKind::Global { init: Some(init), .. } = &p_def.kind else {
        panic!("expected initialized pointer global");
    };
    assert_eq!(init.entries.len(), 1);
    assert!(matches!(
        init.entries[0].value,
        GlobalInitValue::Address { def: Some(base), offset: 0 } if base == x_def
    ));
    assert!(
        cap.diagnostics().iter().all(|d| d.code != Some(rcc_errors::codes::E0084)),
        "address initializer should not emit E0084"
    );
}

#[test]
fn snippet_typeck_preserves_global_string_pointer_initializer() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("char *p = \"hi\";");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Global { init: Some(_), .. }))
        .expect("missing initialized global");
    let DefKind::Global { init: Some(init), .. } = &def.kind else {
        panic!("expected initialized global");
    };
    assert_eq!(init.entries.len(), 1);
    assert!(matches!(init.entries[0].value, GlobalInitValue::StringLiteral(_)));
    assert!(
        cap.diagnostics().iter().all(|d| d.code != Some(rcc_errors::codes::E0084)),
        "string literal pointer initializer should not emit E0084"
    );
}

#[test]
fn snippet_typeck_reports_nonconstant_global_initializer() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("int f(void); int y = f();");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Global { init: Some(_), .. }))
        .expect("missing initialized global");
    let DefKind::Global { init: Some(init), .. } = &def.kind else {
        panic!("expected initialized global");
    };
    assert!(matches!(init.entries[0].value, GlobalInitValue::Error));
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0084)),
        "non-constant initializer should emit E0084"
    );
}

#[test]
fn snippet_typeck_folds_aggregate_range_initializer_leaves() {
    let (hir, _tcx, cap) =
        checked_snippet_with_diagnostics("int a[4] = { [1 ... 2] = 1 + 2, [3] = 4 };");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Global { init: Some(_), .. }))
        .expect("missing initialized global");
    let DefKind::Global { init: Some(init), .. } = &def.kind else {
        panic!("expected initialized global");
    };
    let values: Vec<_> = init
        .entries
        .iter()
        .map(|entry| {
            let [GlobalInitDesignator::Index(i)] = entry.path.as_slice() else {
                panic!("expected index path, got {:?}", entry.path);
            };
            let GlobalInitValue::Int(v) = entry.value else {
                panic!("expected int value, got {:?}", entry.value);
            };
            (*i, v)
        })
        .collect();
    assert_eq!(values, vec![(1, 3), (2, 3), (3, 4)]);
    assert!(
        cap.diagnostics().iter().all(|d| d.code != Some(rcc_errors::codes::E0084)),
        "constant aggregate initializer should not emit E0084"
    );
}

#[test]
fn snippet_switch_collects_case_table_from_source() {
    let (hir, _tcx) =
        lower_snippet("int f(int x) { switch (x) { case 1: return 2; default: return 3; } }");
    let body = hir.bodies.values().next().expect("missing function body");
    let cases = body
        .stmts
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStmtKind::Switch { cases, .. } => Some(cases),
            _ => None,
        })
        .expect("missing switch");
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].value, Some(1));
    assert_eq!(cases[1].value, None);
    assert!(matches!(body.stmts[cases[0].target].kind, HirStmtKind::Case { .. }));
    assert!(matches!(body.stmts[cases[1].target].kind, HirStmtKind::Default { .. }));
}

#[test]
fn snippet_nested_switch_cases_do_not_leak_to_outer_switch() {
    let (hir, _tcx) = lower_snippet(
        "int f(int x) { switch (x) { case 1: switch (x) { case 2: return 2; } default: return 0; } }",
    );
    let body = hir.bodies.values().next().expect("missing function body");
    let switch_cases: Vec<Vec<Option<i128>>> = body
        .stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStmtKind::Switch { cases, .. } => {
                Some(cases.iter().map(|case| case.value).collect())
            }
            _ => None,
        })
        .collect();
    assert_eq!(switch_cases.len(), 2);
    assert!(switch_cases.contains(&vec![Some(1), None]));
    assert!(switch_cases.contains(&vec![Some(2)]));
}

#[test]
fn snippet_case_outside_switch_reports_e0086() {
    let (_hir, _tcx, cap) = lower_snippet_with_diagnostics("int f(void) { case 1: return 0; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0086)),
        "case outside switch should emit E0086"
    );
}

#[test]
fn snippet_duplicate_default_reports_e0086() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("int f(int x) { switch (x) { default: ; default: ; } }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0086)),
        "duplicate default should emit E0086"
    );
}

#[test]
fn snippet_duplicate_block_declarator_diagnoses_without_overwriting_binding() {
    let (hir, _tcx, cap) = lower_snippet_with_diagnostics("void f(void) { int a, a = a; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0070)),
        "duplicate block declarator should emit E0070"
    );
    let body = hir.bodies.values().next().expect("missing function body");
    let second_init = body
        .stmts
        .iter()
        .filter_map(|stmt| match stmt.kind {
            HirStmtKind::LocalDecl { init, .. } => init,
            _ => None,
        })
        .next()
        .expect("missing duplicate declarator initializer");
    assert!(matches!(body.exprs[second_init].kind, HirExprKind::LocalRef(Local(0))));
}

#[test]
fn snippet_duplicate_file_scope_declarator_diagnoses() {
    let (_hir, _tcx, cap) = lower_snippet_with_diagnostics("int a, a;");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0070)),
        "duplicate file-scope declarator in one declaration should emit E0070"
    );
}

#[test]
fn snippet_forward_record_completion_uses_one_def_id() {
    let (hir, tcx) = lower_snippet("struct S; struct S { int a; }; struct S s;");
    let record_ids: Vec<DefId> = hir
        .defs
        .iter_enumerated()
        .filter_map(|(id, def)| match &def.kind {
            DefKind::Record { .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(record_ids.len(), 1);
    let record_id = record_ids[0];
    let DefKind::Record { fields, .. } = &hir.defs[record_id].kind else {
        unreachable!();
    };
    assert_eq!(fields.len(), 1);
    let global_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .expect("missing global");
    assert!(matches!(tcx.get(global_ty), Ty::Record(id) if *id == record_id));
}

#[test]
fn snippet_record_pointer_uses_completed_tag() {
    let (hir, tcx) = lower_snippet("struct S { int a; }; struct S *p;");
    let record_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match def.kind {
            DefKind::Record { .. } => Some(id),
            _ => None,
        })
        .expect("missing record");
    let global_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .expect("missing pointer global");
    match tcx.get(global_ty) {
        Ty::Ptr(pointee) => {
            assert!(matches!(tcx.get(pointee.ty), Ty::Record(id) if *id == record_id))
        }
        other => panic!("expected pointer-to-record, got {other:?}"),
    }
}

#[test]
fn snippet_mutual_record_pointer_references_complete_later() {
    let (hir, tcx) = lower_snippet("struct A { struct B *b; }; struct B { int x; };");
    let records: Vec<_> = hir
        .defs
        .iter_enumerated()
        .filter_map(|(id, def)| match &def.kind {
            DefKind::Record { fields, .. } => Some((id, fields)),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 2);
    let (a_id, a_fields) = records[0];
    let (b_id, b_fields) = records[1];
    assert_eq!(a_fields.len(), 1);
    assert_eq!(b_fields.len(), 1);
    match tcx.get(a_fields[0].ty) {
        Ty::Ptr(pointee) => assert!(matches!(tcx.get(pointee.ty), Ty::Record(id) if *id == b_id)),
        other => panic!("expected A.b to be pointer-to-B, got {other:?}"),
    }
    assert_ne!(a_id, b_id);
}

#[test]
fn snippet_enum_completion_exposes_enumerator_and_global_type() {
    let (hir, tcx) = lower_snippet("enum E { A = 1 }; enum E e;");
    let enum_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match def.kind {
            DefKind::Enum { .. } => Some(id),
            _ => None,
        })
        .expect("missing enum");
    let DefKind::Enum { variants, .. } = &hir.defs[enum_id].kind else {
        unreachable!();
    };
    assert_eq!(variants.len(), 1);
    assert!(
        hir.defs.iter().any(|def| matches!(def.kind, DefKind::Enumerator { value: 1, .. })),
        "enumerator A should be in the ordinary namespace"
    );
    let global_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .expect("missing enum global");
    assert!(matches!(tcx.get(global_ty), Ty::Enum(id) if *id == enum_id));
}

#[test]
fn snippet_struct_global_yields_two_defs() {
    // `struct P { int x; int y; } origin;` — one tag def + one global.
    let (hir, _tcx) = lower_snippet("struct P { int x; int y; } origin;");
    assert_eq!(hir.defs.len(), 2);
    let kinds: Vec<&'static str> = hir
        .defs
        .iter()
        .map(|d| match d.kind {
            DefKind::Record { .. } => "record",
            DefKind::Global { .. } => "global",
            _ => "other",
        })
        .collect();
    assert!(kinds.contains(&"record"));
    assert!(kinds.contains(&"global"));
}

#[test]
fn snippet_static_function_marked_internal() {
    let (hir, _tcx) = lower_snippet("static int helper(void) { return 1; }");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Function { .. }))
        .expect("missing function");
    let DefKind::Function { is_static, .. } = def.kind else {
        unreachable!();
    };
    assert!(is_static);
}

#[test]
fn snippet_inline_function_marked_inline() {
    // Plain `inline` (no storage class): inline definition without
    // providing the external definition (C99 §6.7.4).
    let (hir, _tcx) = lower_snippet("inline int square(int x) { return x * x; }");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Function { .. }))
        .expect("missing function");
    let DefKind::Function { is_inline, is_extern_inline, is_static, .. } = def.kind else {
        unreachable!();
    };
    assert!(is_inline);
    assert!(!is_extern_inline);
    assert!(!is_static);
}

#[test]
fn snippet_extern_inline_function_marked_extern_inline() {
    // `extern inline` provides the external definition (C99 §6.7.4).
    let (hir, _tcx) = lower_snippet("extern inline int square(int x) { return x * x; }");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Function { .. }))
        .expect("missing function");
    let DefKind::Function { is_inline, is_extern_inline, is_static, .. } = def.kind else {
        unreachable!();
    };
    assert!(is_inline);
    assert!(is_extern_inline);
    assert!(!is_static);
}

#[test]
fn snippet_static_inline_function_marked_inline_and_static() {
    // `static inline` always emits with internal linkage; not an
    // `extern inline` definition.
    let (hir, _tcx) = lower_snippet("static inline int square(int x) { return x * x; }");
    let def = hir
        .defs
        .iter()
        .find(|d| matches!(d.kind, DefKind::Function { .. }))
        .expect("missing function");
    let DefKind::Function { is_inline, is_extern_inline, is_static, .. } = def.kind else {
        unreachable!();
    };
    assert!(is_inline);
    assert!(!is_extern_inline);
    assert!(is_static);
}

#[test]
fn snippet_extern_global_has_external_linkage() {
    // Sanity: at least one def should have External linkage.
    let (hir, _tcx) = lower_snippet("extern int errno;");
    let def =
        hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).expect("missing global");
    let DefKind::Global { linkage, .. } = def.kind else {
        unreachable!();
    };
    assert_eq!(linkage, Linkage::External);
}

#[test]
fn snippet_static_global_has_internal_linkage() {
    let (hir, _tcx) = lower_snippet("static int counter;");
    let def =
        hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).expect("missing global");
    let DefKind::Global { linkage, .. } = def.kind else {
        unreachable!();
    };
    assert_eq!(linkage, Linkage::Internal);
}

#[test]
fn snippet_does_not_emit_diagnostics_on_clean_code() {
    let (_ast, _sess, cap) = parse_to_ast("int main(void) { return 0; }");
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean source should not emit errors: {:?}",
        cap.diagnostics()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Section G — task 06-23 HIR placeholder regression gate
//
// These tests deliberately use source snippets, not hand-built HIR. The
// goal is to lock down the real parse -> HIR lower path that previously
// leaked `tcx.error`, `tcx.int`, or `IntConst(0)` placeholders into CFG.
// ═══════════════════════════════════════════════════════════════════════

fn lower_and_typeck_snippet(src: &str) -> (HirCrate, TyCtxt, CaptureEmitter, Session) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<gate>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut sess);
    rcc_typeck::check(&mut sess, &mut tcx, &mut hir);
    (hir, tcx, cap, sess)
}

fn assert_ty_has_no_error(tcx: &TyCtxt, ty: TyId, context: &str) {
    match tcx.get(ty) {
        Ty::Error => panic!("{context} unexpectedly contains tcx.error"),
        Ty::Ptr(pointee) => assert_ty_has_no_error(tcx, pointee.ty, context),
        Ty::Array { elem, .. } => assert_ty_has_no_error(tcx, elem.ty, context),
        Ty::Func { ret, params, .. } => {
            assert_ty_has_no_error(tcx, *ret, context);
            for param in params {
                assert_ty_has_no_error(tcx, *param, context);
            }
        }
        Ty::Void | Ty::Int { .. } | Ty::Float(_) | Ty::Complex(_) | Ty::Record(_) | Ty::Enum(_) => {
        }
    }
}

fn assert_no_def_or_local_error_types(hir: &HirCrate, tcx: &TyCtxt) {
    for def in hir.defs.iter() {
        match &def.kind {
            DefKind::Function { ty, .. } => assert_ty_has_no_error(tcx, *ty, "function def"),
            DefKind::Global { ty, init, .. } => {
                assert_ty_has_no_error(tcx, *ty, "global def");
                if let Some(init) = init {
                    assert_ty_has_no_error(tcx, init.ty, "global initializer");
                }
            }
            DefKind::Typedef(ty) => assert_ty_has_no_error(tcx, *ty, "typedef def"),
            DefKind::Enum { repr, .. } => assert_ty_has_no_error(tcx, *repr, "enum repr"),
            DefKind::Enumerator { ty, .. } => assert_ty_has_no_error(tcx, *ty, "enumerator"),
            DefKind::Record { fields, .. } => {
                for field in fields {
                    assert_ty_has_no_error(tcx, field.ty, "record field");
                }
            }
        }
    }

    for body in hir.bodies.values() {
        for local in body.locals.iter() {
            assert_ty_has_no_error(tcx, local.ty, "local decl");
        }
    }
}

fn assert_no_body_expr_error_types_after_typeck(hir: &HirCrate, tcx: &TyCtxt) {
    for body in hir.bodies.values() {
        for expr in body.exprs.iter() {
            assert_ty_has_no_error(tcx, expr.ty, "typechecked expression");
        }
    }
}

fn def_named<'a>(hir: &'a HirCrate, sess: &Session, name: &str) -> &'a rcc_hir::Def {
    hir.defs
        .iter()
        .find(|def| sess.interner.get(def.name) == name)
        .unwrap_or_else(|| panic!("missing def named {name}"))
}

#[test]
fn regression_gate_struct_sizeof_source_survives_lower_and_typeck() {
    let (hir, tcx, cap, sess) = lower_and_typeck_snippet(
        "struct S { char c; int i; }; unsigned long f(void) { struct S s; return sizeof s; }",
    );
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );
    assert_no_def_or_local_error_types(&hir, &tcx);
    assert_no_body_expr_error_types_after_typeck(&hir, &tcx);

    let record_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| matches!(def.kind, DefKind::Record { .. }).then_some(id))
        .expect("missing struct S record def");
    let f = def_named(&hir, &sess, "f");
    let DefKind::Function { ty, .. } = f.kind else {
        panic!("f should be a function def");
    };
    match tcx.get(ty) {
        Ty::Func { ret, .. } => assert_eq!(*ret, tcx.ulong),
        other => panic!("expected function type, got {other:?}"),
    }

    let body = hir.bodies.values().next().expect("missing function body");
    let s_local = body
        .locals
        .iter()
        .find(|local| local.name.is_some_and(|sym| sess.interner.get(sym) == "s"))
        .expect("missing local s");
    assert!(matches!(tcx.get(s_local.ty), Ty::Record(id) if *id == record_id));
    let sizeof_expr = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, HirExprKind::SizeofExpr(_)))
        .expect("missing sizeof expression");
    assert_eq!(sizeof_expr.ty, tcx.ulong);
}

#[test]
fn regression_gate_member_access_non_record_reports_typeck_error() {
    let (_hir, _tcx, cap, _sess) = lower_and_typeck_snippet("void f(void) { int x; x.y; }");
    assert!(
        cap.diagnostics().iter().any(|diag| diag.code == Some(rcc_errors::codes::E0087)),
        "x.y should emit E0087, got {:?}",
        cap.diagnostics()
    );
}

#[test]
fn regression_gate_return_constraints_report_typeck_errors() {
    for (name, src) in [
        ("void_value_return", "void f(void) { return 1; }"),
        ("nonvoid_bare_return", "int f(void) { return; }"),
        (
            "incompatible_record_return",
            "struct A { int x; }; struct B { int x; }; struct A f(struct B b) { return b; }",
        ),
    ] {
        let (_hir, _tcx, cap, _sess) = lower_and_typeck_snippet(src);
        assert!(
            cap.diagnostics().iter().any(|diag| diag.code == Some(rcc_errors::codes::E0081)),
            "{name} should emit E0081, got {:?}",
            cap.diagnostics()
        );
    }
}

#[test]
fn regression_gate_coercion_failures_report_typeck_errors_before_cfg() {
    for (name, src) in [
        ("incompatible_pointer_assignment", "void f(void) { char *p; int *q; p = q; }"),
        ("integer_pointer_initializer", "void f(void) { int *p = 42; }"),
    ] {
        let (_hir, _tcx, cap, _sess) = lower_and_typeck_snippet(src);
        assert!(
            cap.diagnostics().iter().any(|diag| diag.code == Some(rcc_errors::codes::E0082)),
            "{name} should emit E0082, got {:?}",
            cap.diagnostics()
        );
    }
}

#[test]
fn regression_gate_typedef_record_enum_globals_keep_resolved_types() {
    let (hir, tcx, cap, _sess) = lower_and_typeck_snippet(
        "typedef unsigned long Size; struct S { int x; }; typedef struct S S; S sg; enum E { A = 7 }; enum E eg; Size sz;",
    );
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );
    assert_no_def_or_local_error_types(&hir, &tcx);

    let typedef_tys: Vec<_> = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Typedef(ty) => Some(ty),
            _ => None,
        })
        .collect();
    assert!(typedef_tys.contains(&tcx.ulong), "typedef Size should preserve unsigned long");
    assert!(
        typedef_tys.iter().any(|ty| matches!(tcx.get(*ty), Ty::Record(_))),
        "typedef S should preserve the record type, not fall back to int"
    );

    let global_tys: Vec<_> = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .collect();
    assert!(global_tys.contains(&tcx.ulong), "Size sz should be unsigned long");
    assert!(
        global_tys.iter().any(|ty| matches!(tcx.get(*ty), Ty::Record(_))),
        "S sg should be record-typed"
    );
    assert!(
        global_tys.iter().any(|ty| matches!(tcx.get(*ty), Ty::Enum(_))),
        "enum E eg should be enum-typed"
    );
}

#[test]
fn regression_gate_volatile_global_preserves_object_qualifier() {
    let (hir, tcx) = lower_snippet("volatile int g;");
    let def = hir.defs.iter().find(|def| matches!(def.kind, DefKind::Global { .. })).unwrap();
    let DefKind::Global { ty, quals, .. } = def.kind else {
        unreachable!();
    };
    assert_eq!(ty, tcx.int);
    assert_eq!(quals, ObjectQuals { is_const: false, is_volatile: true, is_restrict: false });
}

#[test]
fn regression_gate_const_local_preserves_object_qualifier() {
    let (hir, tcx) = lower_snippet("void f(void) { const int local = 1; }");
    let body = hir.bodies.values().next().expect("missing function body");
    assert_eq!(body.locals.len(), 1);
    let local = &body.locals[Local(0)];
    assert_eq!(local.ty, tcx.int);
    assert_eq!(local.quals, ObjectQuals { is_const: true, is_volatile: false, is_restrict: false });
}

#[test]
fn regression_gate_volatile_record_field_preserves_object_qualifier() {
    let (hir, tcx) = lower_snippet("struct S { volatile int x; };");
    let record = hir.defs.iter().find(|def| matches!(def.kind, DefKind::Record { .. })).unwrap();
    let DefKind::Record { fields, .. } = &record.kind else {
        unreachable!();
    };
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].ty, tcx.int);
    assert_eq!(
        fields[0].quals,
        ObjectQuals { is_const: false, is_volatile: true, is_restrict: false }
    );
}

#[test]
fn regression_gate_distinguishes_const_pointer_object_from_pointer_to_const() {
    let (hir, tcx) = lower_snippet("int * const p; const int *q;");
    let globals: Vec<_> = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Global { ty, quals, .. } => Some((ty, quals)),
            _ => None,
        })
        .collect();
    assert_eq!(globals.len(), 2);

    let (p_ty, p_quals) = globals[0];
    assert_eq!(
        p_quals,
        ObjectQuals { is_const: true, is_volatile: false, is_restrict: false },
        "`int * const p` must preserve const on the pointer object"
    );
    match tcx.get(p_ty) {
        Ty::Ptr(pointee) => assert_eq!(
            *pointee,
            Qual::plain(tcx.int),
            "`int * const p` must not turn pointer const into pointee const"
        ),
        other => panic!("expected p to be pointer-to-int, got {other:?}"),
    }

    let (q_ty, q_quals) = globals[1];
    assert_eq!(q_quals, ObjectQuals::none(), "`const int *q` does not qualify the object q");
    match tcx.get(q_ty) {
        Ty::Ptr(pointee) => {
            assert_eq!(pointee.ty, tcx.int);
            assert!(pointee.is_const, "`const int *q` must keep const on the pointee");
            assert!(!pointee.is_volatile);
            assert!(!pointee.is_restrict);
        }
        other => panic!("expected q to be pointer-to-const-int, got {other:?}"),
    }
}

#[test]
fn regression_gate_sizeof_type_and_compound_literal_keep_type_names() {
    let (hir, tcx) = lower_snippet(
        "struct S { int x; }; typedef struct S S; void f(void) { sizeof(S); (S){ .x = 1 }; }",
    );
    let record_id = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| matches!(def.kind, DefKind::Record { .. }).then_some(id))
        .expect("missing record def");
    let body = hir.bodies.values().next().expect("missing function body");

    let sizeof_ty = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::SizeofType(ty) => Some(ty),
            HirExprKind::IntConst(0) => {
                panic!("sizeof(type-name) must not lower to IntConst(0) placeholder")
            }
            _ => None,
        })
        .expect("missing sizeof(type-name)");
    assert!(matches!(tcx.get(sizeof_ty), Ty::Record(id) if *id == record_id));

    let (compound_ty, compound_local) = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { ty, local, .. } => Some((ty, local)),
            _ => None,
        })
        .expect("missing compound literal");
    assert!(matches!(tcx.get(compound_ty), Ty::Record(id) if *id == record_id));
    assert_eq!(body.locals[compound_local].ty, compound_ty);
}

#[test]
fn regression_gate_source_switches_have_case_tables() {
    let (hir, _tcx) = lower_snippet(
        "int f(int x) { switch (x) { case 4: return 1; case 9: return 2; default: return 3; } }",
    );
    let body = hir.bodies.values().next().expect("missing function body");
    let cases = body
        .stmts
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStmtKind::Switch { cases, .. } => Some(cases),
            _ => None,
        })
        .expect("missing switch");
    assert_eq!(
        cases.iter().map(|case| case.value).collect::<Vec<_>>(),
        vec![Some(4), Some(9), None,]
    );
}

#[test]
fn regression_gate_invalid_supported_boundary_reports_diagnostic() {
    let (_hir, _tcx, cap) =
        lower_snippet_with_diagnostics("void f(void) { int a[2] = { .bad = 1 }; }");
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0079)),
        "invalid designator should remain a diagnosed boundary, not a silent placeholder"
    );
}
