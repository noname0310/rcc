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
    Body, ConvertKind, DefId, DefKind, GlobalInitDesignator, GlobalInitValue, HirCrate, HirExprId,
    HirExprKind, HirStmt, HirStmtKind, Layout, LayoutCx, Linkage, Local, LocalDecl, ObjectQuals,
    OverflowOp, RecordKind, SymbolVisibility, TyCtxt, TyId, ValueCat,
};
use rcc_hir_lower::{
    apply_declarator, lower, lower_enum, lower_expr, lower_initializer, lower_record, lower_stmt,
    lower_typedef_name, resolve_expr_ident, resolve_labels, resolve_tag, Binding, DeclScope,
    Resolver, ScopeStack, TagKind,
};
use rcc_session::{LanguageStandard, Options, Session};
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

#[test]
fn block_scope_function_declaration_binds_def_without_local_storage() {
    let src = r#"
        int f1(char *p) { return *p + 1; }
        int main(void) {
            char s = 1;
            int f1(char *);
            return f1(&s);
        }
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let main_def = DefId(1);
    let body = hir.bodies.get(&main_def).expect("main body");

    assert!(
        body.locals.iter().all(|decl| !matches!(tcx.get(decl.ty), Ty::Func { .. })),
        "block-scope function prototype must not allocate local storage: {:?}",
        body.locals
    );

    let call = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            HirExprKind::Call { callee, .. } => Some(*callee),
            _ => None,
        })
        .expect("call expression");
    match body.exprs[call].kind {
        HirExprKind::Convert { operand, .. } => {
            assert!(matches!(body.exprs[operand].kind, HirExprKind::DefRef(DefId(0))));
        }
        HirExprKind::DefRef(DefId(0)) => {}
        ref other => panic!("expected call through DefRef(0), got {other:?}"),
    }
}

#[test]
fn gnu_builtin_libcalls_injects_external_libc_declarations() {
    let src = r#"
        int main(void) {
            char dst[4];
            char src[4];
            __builtin_memcpy(dst, src, 4);
            __builtin_printf("%s", dst);
            __builtin_prefetch(dst, 0, 3);
            if (__builtin_strlen(dst) == 99)
                abort();
            return __CHAR_BIT__ == 8 ? 0 : 1;
        }
    "#;
    let opts = Options { gnu_builtin_libcalls: true, ..Options::default() };
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(opts, handler);
    let fid =
        sess.source_map.write().unwrap().add_file(PathBuf::from("<gnu-builtins>"), Arc::from(src));
    let pp_tokens = rcc_preprocess::preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut sess);
    rcc_typeck::check(&mut sess, &mut tcx, &mut hir);

    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    for name in ["memcpy", "strlen", "abort", "printf", "vprintf", "vfprintf"] {
        let def = hir
            .defs
            .iter()
            .find(|def| sess.interner.get(def.name) == name)
            .unwrap_or_else(|| panic!("missing injected declaration for {name}"));
        assert!(matches!(def.kind, DefKind::Function { has_body: false, .. }));
        if name == "strlen" {
            let DefKind::Function { ty, .. } = def.kind else { unreachable!() };
            assert!(matches!(tcx.get(ty), Ty::Func { ret, .. } if *ret == tcx.ulong));
        }
        if matches!(name, "vprintf" | "vfprintf") {
            let DefKind::Function { ty, .. } = def.kind else { unreachable!() };
            let Ty::Func { params, .. } = tcx.get(ty) else {
                panic!("expected function type for {name}");
            };
            let va_param = params.last().expect("v*printf should have a va_list parameter");
            assert!(
                matches!(tcx.get(*va_param), Ty::Ptr(q) if q.ty == tcx.builtin_va_list),
                "{name} must use pointer-adjusted va_list, got {:?}",
                tcx.get(*va_param)
            );
        }
    }
}

#[test]
fn gnu_overflow_builtins_lower_to_typed_hir_nodes() {
    let src = r#"
        int f(unsigned a, int b) {
            int out;
            return __builtin_add_overflow(a, b, &out)
                + __builtin_mul_overflow_p(a, b, 0);
        }
    "#;
    let opts = Options { gnu_builtin_libcalls: true, ..Options::default() };
    let (hir, tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let body = hir.bodies.values().next().expect("function body");

    let add = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::BuiltinOverflow { op: OverflowOp::Add, lhs, rhs, result_ty, .. } => {
                Some((lhs, rhs, result_ty))
            }
            _ => None,
        })
        .expect("add overflow builtin");
    assert_eq!(add.2, tcx.int);
    assert_eq!(body.exprs[add.0].ty, tcx.uint, "lhs keeps its original unsigned type");
    assert_eq!(body.exprs[add.1].ty, tcx.int, "rhs keeps its original signed type");

    assert!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            HirExprKind::BuiltinOverflowP {
                op: OverflowOp::Mul,
                result_ty,
                ..
            } if result_ty == tcx.int
        )),
        "mul overflow predicate should use the probe expression type"
    );
}

#[test]
fn block_scope_static_object_binds_internal_global_without_local_storage() {
    let src = r#"
        int main(void) {
            static int fred = 4567;
            fred = fred + 1;
            return fred;
        }
    "#;
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let (static_def, init) = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Global { linkage: Linkage::Internal, init: Some(init), .. } => {
                Some((id, init))
            }
            _ => None,
        })
        .expect("block-scope static should lower to an internal global");
    assert!(
        init.entries.iter().any(|entry| matches!(entry.value, GlobalInitValue::Int(4567))),
        "static initializer should be represented as a global initializer: {init:?}"
    );

    let main_body = hir.bodies.get(&DefId(0)).expect("main body");
    assert!(main_body.locals.is_empty(), "static object must not allocate an automatic local");
    assert!(
        main_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, HirExprKind::DefRef(def) if def == static_def)),
        "uses of the block-scope static should resolve to the generated global def"
    );
}

#[test]
fn file_scope_tentative_definitions_merge_with_explicit_initializer() {
    let src = "int x, x = 3, x; int main(void) { return x; }";
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let globals: Vec<_> = hir
        .defs
        .iter()
        .filter_map(|def| match &def.kind {
            DefKind::Global { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .collect();
    assert_eq!(globals.len(), 1, "all file-scope `x` declarations should merge");
    assert!(
        globals[0].entries.iter().any(|entry| matches!(entry.value, GlobalInitValue::Int(3))),
        "explicit initializer must win over tentative zero markers: {:?}",
        globals[0]
    );
}

#[test]
fn file_scope_array_size_accepts_integer_constant_expression() {
    let src = r#"
        #define BASE 0x1e
        int array[BASE + 1];
        int main(void) { return sizeof(array) == sizeof(int) * 31 ? 0 : 1; }
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let array_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .expect("global array");
    match tcx.get(array_ty) {
        Ty::Array { len: Some(31), .. } => {}
        other => panic!("expected complete array[31], got {other:?}"),
    }
}

#[test]
fn block_scope_struct_definition_shadows_file_scope_tag() {
    let src = r#"
        struct T;
        struct T { int x; };

        int main(void) {
            struct T v;
            { struct T { int z; }; }
            v.x = 2;
            return v.x != 2;
        }
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn nested_block_struct_definition_shadows_outer_block_tag() {
    let src = r#"
        int main(void) {
            struct T { int x; } s1;
            s1.x = 1;
            {
                struct T { int y; } s2;
                s2.y = 1;
                if (s1.x - s2.y != 0)
                    return 1;
            }
            return 0;
        }
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn duplicate_block_scope_tag_definition_is_diagnosed() {
    let src = r#"
        int main(void) {
            struct T { int x; };
            struct T { int y; };
            return 0;
        }
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|diag| diag.code == Some(rcc_errors::codes::E0070)),
        "same-scope duplicate tag definition should emit E0070: {diags:?}"
    );
}

#[test]
fn function_pointer_return_definition_lowers_body_parameters_from_final_declarator() {
    let src = r#"
        int f2(int c, int b) {
            return c - b;
        }

        int (*f1(int a, int b))(int c, int b) {
            if (a != b)
                return f2;
            return 0;
        }
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn file_scope_compound_literal_address_materializes_internal_global() {
    let src = r#"
        struct S { int a; int b; };
        struct S *s = &(struct S) { 1, 2 };
    "#;
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let literal_def = hir
        .defs
        .iter()
        .filter_map(|def| match &def.kind {
            DefKind::Global { init: Some(init), .. } => {
                init.entries.iter().find_map(|entry| match entry.value {
                    GlobalInitValue::Address { def: Some(base), offset: 0 } => Some(base),
                    _ => None,
                })
            }
            _ => None,
        })
        .next()
        .expect("pointer initializer should address a synthetic compound-literal global");

    match &hir.defs[literal_def].kind {
        DefKind::Global { linkage: Linkage::Internal, init: Some(init), .. } => {
            let values: Vec<_> = init
                .entries
                .iter()
                .filter_map(|entry| match entry.value {
                    GlobalInitValue::Int(v) => Some(v),
                    _ => None,
                })
                .collect();
            assert_eq!(values, vec![1, 2]);
        }
        other => panic!("expected internal synthetic global, got {other:?}"),
    }
}

#[test]
fn nested_file_scope_compound_literal_address_initializer_is_constant() {
    let src = r#"
        struct S1 { int a; int b; };
        struct S2 { struct S1 s1; struct S1 *ps1; int arr[2]; };
        struct S1 gs1 = { .a = 1, 2 };
        struct S2 *s = &(struct S2) {
            {.b = 2, .a = 1},
            &gs1,
            {[0] = 1, 1 + 1}
        };
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn local_flat_initializer_descends_into_nested_array_field() {
    let src = r#"
        struct PT { long c[4]; long b, e, k; };
        int main(void) {
            struct PT p = { 1, 2, 3, 4, 5, 6, 7 };
            return p.c[0] == 1 && p.c[3] == 4 && p.b == 5 && p.k == 7 ? 0 : 1;
        }
    "#;
    let (_hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
}

#[test]
fn global_flat_initializer_completes_outer_array_by_aggregate_leaf_count() {
    let src = r#"
        typedef long I;
        typedef struct { I c[4]; I b, e, k; } PT;
        PT cases[] = { 1,2,3,4,5,6,7, 8,9,10,11,12,13,14 };
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let def = hir
        .defs
        .iter()
        .find(|def| matches!(def.kind, DefKind::Global { init: Some(_), .. }))
        .expect("expected initialized global cases");
    let DefKind::Global { ty, init: Some(init), .. } = &def.kind else {
        panic!("expected initialized global cases, got {:?}", def.kind);
    };
    match tcx.get(*ty) {
        Ty::Array { len: Some(2), .. } => {}
        other => panic!("expected cases[2], got {other:?}"),
    }

    let leaves: Vec<_> = init
        .entries
        .iter()
        .take(7)
        .map(|entry| {
            let value = match entry.value {
                GlobalInitValue::Int(v) => v,
                ref other => panic!("expected integer leaf, got {other:?}"),
            };
            (entry.path.clone(), value)
        })
        .collect();
    assert_eq!(
        leaves,
        vec![
            vec![
                GlobalInitDesignator::Index(0),
                GlobalInitDesignator::Field(0),
                GlobalInitDesignator::Index(0)
            ],
            vec![
                GlobalInitDesignator::Index(0),
                GlobalInitDesignator::Field(0),
                GlobalInitDesignator::Index(1)
            ],
            vec![
                GlobalInitDesignator::Index(0),
                GlobalInitDesignator::Field(0),
                GlobalInitDesignator::Index(2)
            ],
            vec![
                GlobalInitDesignator::Index(0),
                GlobalInitDesignator::Field(0),
                GlobalInitDesignator::Index(3)
            ],
            vec![GlobalInitDesignator::Index(0), GlobalInitDesignator::Field(1)],
            vec![GlobalInitDesignator::Index(0), GlobalInitDesignator::Field(2)],
            vec![GlobalInitDesignator::Index(0), GlobalInitDesignator::Field(3)],
        ]
        .into_iter()
        .zip(1..=7)
        .collect::<Vec<_>>()
    );
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
    checked_snippet_with_options(src, Options::default())
}

fn checked_snippet_with_options(src: &str, opts: Options) -> (HirCrate, TyCtxt, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(opts, handler);
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
    DerivedDeclarator::Pointer(TypeQuals {
        const_: true,
        volatile: false,
        restrict: false,
        atomic: false,
    })
}

fn int_lit(text: &str, sess: &mut Session) -> Expr {
    let s = intern(sess, text);
    Expr {
        id: NodeId(0),
        kind: ExprKind::IntLit(rcc_ast::IntLiteral {
            text: s,
            value: text.parse::<u128>().unwrap(),
            base: rcc_ast::IntBase::Decimal,
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
fn s6_7_5_sizeof_type_array_bound_is_fixed() {
    let src = r#"
        int foo(void) {
            union {
                char a[sizeof(unsigned)];
                unsigned b;
            } u;
            return 0;
        }
    "#;
    let (hir, tcx) = lower_snippet(src);
    let array_ty = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Record { kind: RecordKind::Union, fields, .. } if fields.len() == 2 => {
                Some(fields[0].ty)
            }
            _ => None,
        })
        .expect("anonymous union fields should be lowered");

    match tcx.get(array_ty) {
        Ty::Array { len: Some(4), is_vla: false, .. } => {}
        other => panic!("sizeof(unsigned) bound should be fixed Array[4], got {other:?}"),
    }
    let layout = LayoutCx::with_defs(&tcx, &hir.defs).layout_of(array_ty).unwrap();
    assert_eq!(layout.size, 4);
}

#[test]
fn s6_7_5_enum_constant_array_bound_is_fixed() {
    let src = r#"
        enum { N = 6 };
        struct S { int a[N]; };
        int f(struct S *s) { return s->a[5]; }
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let array_ty = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Record { fields, .. } if fields.len() == 1 => Some(fields[0].ty),
            _ => None,
        })
        .expect("record field should be lowered");

    match tcx.get(array_ty) {
        Ty::Array { len: Some(6), is_vla: false, .. } => {}
        other => panic!("enum constant bound should be fixed Array[6], got {other:?}"),
    }
    let layout = LayoutCx::with_defs(&tcx, &hir.defs).layout_of(array_ty).unwrap();
    assert_eq!(layout.size, 24);
}

#[test]
fn s6_7_5_cast_and_offsetof_array_bounds_are_fixed() {
    let src = r#"
        struct Base { char c; int value; };
        struct S {
            char padding[__builtin_offsetof(struct Base, value)];
            char scratch[(int)(16 * sizeof(void*))];
        };
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let fields = hir
        .defs
        .iter()
        .filter_map(|def| match &def.kind {
            DefKind::Record { fields, .. } if fields.len() == 2 => Some(fields),
            _ => None,
        })
        .next_back()
        .expect("struct S fields should be lowered");

    match tcx.get(fields[0].ty) {
        Ty::Array { len: Some(4), is_vla: false, .. } => {}
        other => panic!("offsetof bound should be fixed Array[4], got {other:?}"),
    }
    match tcx.get(fields[1].ty) {
        Ty::Array { len: Some(128), is_vla: false, .. } => {}
        other => panic!("cast/sizeof bound should be fixed Array[128], got {other:?}"),
    }
}

#[test]
fn gnu_aligned_attribute_sets_record_layout_override() {
    let src = r#"
        typedef struct x { int a; int b; } __attribute__((aligned(32))) X;
        typedef struct y { X x[32]; int c; } Y;
        Y y[2];
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, mut tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let x_record = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { fields, align_override: Some(32), .. } if fields.len() == 2 => {
                Some(id)
            }
            _ => None,
        })
        .expect("aligned struct x record should carry an override");
    let x_ty = tcx.intern(Ty::Record(x_record));
    let x_layout = LayoutCx::with_defs(&tcx, &hir.defs).layout_of(x_ty).unwrap();
    assert_eq!(x_layout.align, 32);
    assert_eq!(x_layout.size, 32);
}

#[test]
fn gnu_packed_attribute_sets_record_layout_policy() {
    let src = r#"
        struct __attribute__((packed)) s { char c; int i; };
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, mut tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let record = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { packed: true, fields, .. } if fields.len() == 2 => Some(id),
            _ => None,
        })
        .expect("packed record should carry layout policy");
    let record_ty = tcx.intern(Ty::Record(record));
    let layout = LayoutCx::with_defs(&tcx, &hir.defs).record_layout_of(record_ty).unwrap();
    assert_eq!(layout.layout, Layout { size: 5, align: 1 });
    assert_eq!(layout.fields[1].offset, 1);
}

#[test]
fn gnu_aligned_attribute_sets_field_layout_override() {
    let src = r#"
        struct s1 { int __attribute__((aligned(8))) a; };
        struct outer { char c; struct s1 m; };
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, mut tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let s1_record = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { fields, .. }
                if fields.len() == 1 && fields[0].align_override == Some(8) =>
            {
                Some(id)
            }
            _ => None,
        })
        .expect("struct s1 field should carry GNU aligned(8)");
    let s1_ty = tcx.intern(Ty::Record(s1_record));
    let s1_layout = LayoutCx::with_defs(&tcx, &hir.defs).record_layout_of(s1_ty).unwrap();
    assert_eq!(s1_layout.layout.align, 8);
    assert_eq!(s1_layout.layout.size, 8);

    let outer_ty = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { fields, .. } if fields.len() == 2 && fields[1].ty == s1_ty => {
                Some(tcx.intern(Ty::Record(id)))
            }
            _ => None,
        })
        .expect("outer record should reference aligned struct s1");
    let outer_layout = LayoutCx::with_defs(&tcx, &hir.defs).record_layout_of(outer_ty).unwrap();
    assert_eq!(outer_layout.layout.align, 8);
    assert_eq!(outer_layout.fields[1].offset, 8);
}

#[test]
fn gnu_common_function_attrs_lower_to_def_attrs() {
    let src = r#"
        __attribute__((noreturn, deprecated, visibility("hidden"), section(".text.hot"), weak))
        void f(void);
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(!cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error));

    let (def, _) = hir
        .defs
        .iter_enumerated()
        .find(|(_, def)| matches!(def.kind, DefKind::Function { .. }))
        .expect("function def");
    let attrs = hir.def_attrs.get(&def).copied().expect("function attrs");
    assert!(attrs.noreturn);
    assert!(attrs.deprecated);
    assert_eq!(attrs.visibility, Some(SymbolVisibility::Hidden));
    assert!(attrs.section.is_some());
    assert!(attrs.weak);
}

#[test]
fn c11_noreturn_function_specifier_lowers_to_common_attrs() {
    let src = r#"
        _Noreturn void fatal(void) { for (;;) {} }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(!cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error));

    let (def, _) = hir
        .defs
        .iter_enumerated()
        .find(|(_, def)| matches!(def.kind, DefKind::Function { .. }))
        .expect("function def");
    let attrs = hir.def_attrs.get(&def).copied().expect("function attrs");
    assert!(attrs.noreturn);
}

#[test]
fn c11_noreturn_does_not_change_function_pointer_compatibility() {
    let src = r#"
        int (*p)(void);
        _Noreturn int fatal(void) { for (;;) {} }
        void use(void) { p = fatal; }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_static_assert_accepts_file_block_and_sizeof_constant_expression() {
    let src = r#"
        _Static_assert(1, "file");
        _Static_assert(sizeof(int) == 4, "int size");
        void f(void) {
            _Static_assert(1, "block");
        }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_static_assert_false_reports_message_before_codegen() {
    let src = r#"
        _Static_assert(0, "broken assumption");
        int main(void) { return 0; }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0089) && d.message.contains("broken assumption")
        }),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_static_assert_in_record_does_not_create_field() {
    let src = r#"
        struct S {
            _Static_assert(sizeof(int) == 4, "layout");
            int x;
        };
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );

    let record = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Record { fields, .. } => Some(fields),
            _ => None,
        })
        .expect("record definition");
    assert_eq!(record.len(), 1);
}

#[test]
fn c11_alignof_folds_in_static_assert() {
    let src = r#"
        _Static_assert(_Alignof(int) == 4, "int alignment");
        int main(void) { return 0; }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_alignas_lowers_global_local_and_field_overrides() {
    let src = r#"
        _Alignas(16) int g;
        struct S {
            _Alignas(16) int x;
            char y;
        };
        void f(void) {
            _Alignas(16) int x;
        }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, mut tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );

    assert!(
        hir.def_attrs.iter().any(|(id, attrs)| {
            attrs.align_override == Some(16) && matches!(hir.defs[*id].kind, DefKind::Global { .. })
        }),
        "expected aligned global attrs in {:#?}",
        hir.def_attrs
    );

    let body = hir.bodies.values().next().expect("function body");
    assert!(
        body.local_attrs.values().any(|attrs| attrs.align_override == Some(16)),
        "expected aligned local attrs in {:#?}",
        body.local_attrs
    );

    let (record_def, fields) = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { fields, .. } if fields.len() == 2 => Some((id, fields)),
            _ => None,
        })
        .expect("record definition");
    assert_eq!(fields[0].align_override, Some(16));

    let record_ty = tcx.intern(Ty::Record(record_def));
    let layout = LayoutCx::with_defs(&tcx, &hir.defs).layout_of(record_ty).expect("record layout");
    assert!(layout.align >= 16, "layout: {layout:?}");
}

#[test]
fn c11_alignas_type_name_uses_layout_service() {
    let src = "_Alignas(long double) int g;";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );
    let global_attrs = hir.def_attrs.values().next().copied().expect("global attrs");
    assert_eq!(global_attrs.align_override, Some(16));
}

#[test]
fn c11_invalid_alignas_is_diagnosed() {
    let src = "_Alignas(3) int g;";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().any(|d| d.code == Some(rcc_errors::codes::E0061)
            && d.message.contains("invalid `_Alignas` alignment")),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_anonymous_union_member_preserves_layout_and_promoted_lookup() {
    let src = r#"
        struct S {
            union { int x; long y; };
            char tail;
        };
        int f(struct S *s) {
            s->x = 1;
            s->y = 2;
            return s->x;
        }
    "#;
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, mut tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );

    let (record_def, fields) = hir
        .defs
        .iter_enumerated()
        .find_map(|(id, def)| match &def.kind {
            DefKind::Record { kind: RecordKind::Struct, fields, .. }
                if fields.len() == 2 && fields[0].name.is_none() && fields[1].name.is_some() =>
            {
                Some((id, fields))
            }
            _ => None,
        })
        .expect("outer record");
    assert!(matches!(tcx.get(fields[0].ty), Ty::Record(_)));
    let record_ty = tcx.intern(Ty::Record(record_def));
    let layout = LayoutCx::with_defs(&tcx, &hir.defs)
        .record_layout_of(record_ty)
        .expect("outer record layout");
    assert_eq!(layout.fields[0].offset, 0);
    assert_eq!(layout.fields[1].offset, 8, "tail must follow the anonymous union storage");

    let body = hir.bodies.values().next().expect("function body");
    assert!(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                HirExprKind::Field { base, field_index: 0 }
                    if matches!(body.exprs[base].kind, HirExprKind::Field { field_index: 0, .. })
            )
        }),
        "promoted `s->x`/`s->y` should lower through nested anonymous field paths: {:#?}",
        body.exprs
    );
}

#[test]
fn c11_generic_selection_records_selected_association() {
    let src = "int f(int x) { return _Generic(x, int: 10, default: 20); }";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        !cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );
    let body = hir.bodies.values().next().expect("missing function body");
    let (associations, selected) = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            HirExprKind::GenericSelection { associations, selected: Some(selected), .. } => {
                Some((associations, *selected))
            }
            _ => None,
        })
        .expect("expected generic selection");
    assert_eq!(associations.len(), 2);
    assert!(matches!(body.exprs[selected].kind, HirExprKind::IntConst(10)));
}

#[test]
fn c11_generic_selection_duplicate_compatible_type_is_diagnosed() {
    let src = "int f(void) { return _Generic(1, int: 10, signed int: 11, default: 20); }";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics()
            .iter()
            .any(|d| d.message.contains("duplicate compatible `_Generic` association type")),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_generic_selection_missing_match_without_default_is_diagnosed() {
    let src = "int f(void) { return _Generic(1.0, int: 10); }";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().any(|d| d.message.contains("no matching association")),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_atomic_specifier_and_qualifier_lower_to_atomic_ty() {
    let src = "_Atomic(int) x; _Atomic int y; int * _Atomic p;";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );

    let globals = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Global { ty, .. } => Some(ty),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(globals.len(), 3);
    assert!(matches!(tcx.get(globals[0]), Ty::Atomic(inner) if *inner == tcx.int));
    assert!(matches!(tcx.get(globals[1]), Ty::Atomic(inner) if *inner == tcx.int));
    let Ty::Atomic(ptr) = *tcx.get(globals[2]) else {
        panic!("expected atomic-qualified pointer, got {:?}", tcx.get(globals[2]));
    };
    assert!(matches!(tcx.get(ptr), Ty::Ptr(q) if q.ty == tcx.int));
}

#[test]
fn c11_invalid_atomic_object_types_are_diagnosed() {
    let src = "_Atomic(void) v; typedef int F(void); _Atomic(F) f;";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().any(|d| d.message.contains("invalid C11 atomic object type")),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn c11_thread_local_globals_lower_to_tls_defs() {
    let src = "_Thread_local int x; static _Thread_local int y; extern _Thread_local int z;";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "{:#?}",
        cap.diagnostics()
    );

    let globals = hir
        .defs
        .iter()
        .filter_map(|def| match def.kind {
            DefKind::Global { thread_local, linkage, .. } => Some((thread_local, linkage)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        globals,
        vec![(true, Linkage::External), (true, Linkage::Internal), (true, Linkage::External)]
    );
}

#[test]
fn c11_thread_local_block_scope_requires_static_or_extern() {
    let src = "int f(void) { _Thread_local int x; return 0; }";
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics()
            .iter()
            .any(|d| d.message.contains("block-scope declarations must also use")),
        "{:#?}",
        cap.diagnostics()
    );
}

#[test]
fn gnu_common_global_and_local_unused_attrs_lower() {
    let src = r#"
        int g __attribute__((unused, visibility("default"), section(".data.rcc")));
        int main(void) {
            int x __attribute__((unused));
            return 0;
        }
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(!cap.diagnostics().iter().any(|d| d.level == rcc_errors::Level::Error));

    let (global, _) = hir
        .defs
        .iter_enumerated()
        .find(|(_, def)| matches!(def.kind, DefKind::Global { .. }))
        .expect("global def");
    let global_attrs = hir.def_attrs.get(&global).copied().expect("global attrs");
    assert!(global_attrs.unused);
    assert_eq!(global_attrs.visibility, Some(SymbolVisibility::Default));
    assert!(global_attrs.section.is_some());

    let body = hir.bodies.values().next().expect("main body");
    assert!(
        body.local_attrs.values().any(|attrs| attrs.unused),
        "expected local `unused` attribute in {:#?}",
        body.local_attrs
    );
}

#[test]
fn gnu_vector_size_attribute_lowers_typedef_and_sizeof_layout() {
    let src = r#"
        typedef int v4si __attribute__((vector_size(16)));
        int size_check[sizeof(v4si)];
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let vector_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Typedef(ty) if matches!(tcx.get(ty), Ty::Vector { .. }) => Some(ty),
            _ => None,
        })
        .expect("vector typedef should lower to Ty::Vector");

    match tcx.get(vector_ty) {
        Ty::Vector { elem, lanes, bytes } => {
            assert_eq!(*elem, tcx.int);
            assert_eq!(*lanes, 4);
            assert_eq!(*bytes, 16);
        }
        other => panic!("expected vector type, got {other:?}"),
    }
    assert_eq!(
        LayoutCx::with_defs(&tcx, &hir.defs).layout_of(vector_ty).unwrap(),
        rcc_hir::Layout { size: 16, align: 16 }
    );
}

#[test]
fn gnu_vector_size_attribute_evaluates_sizeof_expression() {
    let src = r#"
        #define vector(elcount, type) __attribute__((vector_size((elcount) * sizeof(type)))) type
        typedef vector(4, float) v4sf;
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let vector_ty = hir
        .defs
        .iter()
        .find_map(|def| match def.kind {
            DefKind::Typedef(ty) if matches!(tcx.get(ty), Ty::Vector { .. }) => Some(ty),
            _ => None,
        })
        .expect("vector macro typedef should lower to Ty::Vector");

    assert!(matches!(
        tcx.get(vector_ty),
        Ty::Vector { elem, lanes: 4, bytes: 16 } if *elem == tcx.float
    ));
}

#[test]
fn gnu_vector_size_attribute_rejects_invalid_byte_size() {
    let src = r#"
        typedef int bad __attribute__((vector_size(3)));
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(
        cap.diagnostics().iter().any(|diag| diag.code == Some(rcc_errors::codes::E0061)),
        "expected E0061, got {:?}",
        cap.diagnostics()
    );
}

#[test]
fn gnu_vector_initializer_lowers_to_vector_value_not_aggregate_paths() {
    let src = r#"
        typedef int v4si __attribute__((vector_size(16)));
        v4si g = { 1 + 0, 2 };
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let init = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Global { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .expect("vector global initializer");
    assert_eq!(init.entries.len(), 1);
    assert!(init.entries[0].path.is_empty(), "vector lanes must not use aggregate paths");
    match &init.entries[0].value {
        GlobalInitValue::Vector(lanes) => {
            assert_eq!(lanes.len(), 4);
            assert!(matches!(lanes[0], GlobalInitValue::Int(1)));
            assert!(matches!(lanes[1], GlobalInitValue::Int(2)));
            assert!(matches!(lanes[2], GlobalInitValue::Int(0) | GlobalInitValue::Zero));
            assert!(matches!(lanes[3], GlobalInitValue::Int(0) | GlobalInitValue::Zero));
        }
        other => panic!("expected vector initializer value, got {other:?}"),
    }
}

#[test]
fn gnu_vector_compound_literal_uses_vector_init_expression() {
    let src = r#"
        typedef int v4si __attribute__((vector_size(16)));
        int main(void) {
            v4si x;
            x = (v4si){ 1, 2, 3, 4 };
            return 0;
        }
    "#;
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let body = hir.bodies.values().next().expect("main body");
    let (literal_local, init_stmts) = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            HirExprKind::CompoundLiteral { local, init_stmts, .. } => Some((*local, init_stmts)),
            _ => None,
        })
        .expect("vector compound literal");
    assert!(
        init_stmts.iter().any(|stmt| match body.stmts[*stmt].kind {
            HirStmtKind::InitAssign { lhs, rhs } => {
                matches!(body.exprs[lhs].kind, HirExprKind::LocalRef(local) if local == literal_local)
                    && matches!(&body.exprs[rhs].kind, HirExprKind::VectorInit { lanes, .. } if lanes.len() == 4)
            }
            _ => false,
        }),
        "compound literal initializer should assign one VectorInit to its backing local"
    );
}

#[test]
fn aggregate_initializers_skip_unnamed_bitfields() {
    let src = r#"
        struct S { int a:4; int :4; int b:4; int c:4; } x = { 2, 3, 4 };
    "#;
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let init = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Global { init: Some(init), .. } => Some(init),
            _ => None,
        })
        .expect("global initializer");
    let paths = init.entries.iter().map(|entry| entry.path.as_slice()).collect::<Vec<_>>();

    assert!(matches!(paths[0], [GlobalInitDesignator::Field(0)]));
    assert!(matches!(paths[1], [GlobalInitDesignator::Field(2)]));
    assert!(matches!(paths[2], [GlobalInitDesignator::Field(3)]));
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
fn composite_bitfield_width_accepts_sizeof_integer_constant_expression() {
    let src = r#"
        typedef unsigned long uintptr_t;
        struct StackElemBits {
            uintptr_t val : sizeof(uintptr_t) * 8 - 3;
            uintptr_t type : 3;
        };
    "#;
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let record = hir
        .defs
        .iter()
        .find_map(|def| match &def.kind {
            DefKind::Record { fields, .. } if fields.len() == 2 => Some(fields),
            _ => None,
        })
        .expect("missing StackElemBits record");
    assert_eq!(record[0].bit_width, Some(61));
    assert_eq!(record[1].bit_width, Some(3));
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
fn composite_anonymous_struct_member_preserves_nested_layout_field() {
    // struct Outer { struct { int a; int b; }; int c; }
    // Physical field list: [anonymous struct, c]. Typeck promotes a/b for lookup.
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
            assert_eq!(names, vec![None, Some(c)]);
            let Ty::Record(inner_def) = *tcx.get(fields[0].ty) else {
                panic!("anonymous struct should remain a nested record field");
            };
            let DefKind::Record { fields: inner_fields, .. } = &crate_.defs[inner_def].kind else {
                panic!("expected nested record");
            };
            let inner_names: Vec<_> = inner_fields.iter().map(|f| f.name).collect();
            assert_eq!(inner_names, vec![Some(a), Some(b)]);
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
fn composite_enum_initializer_can_reference_prior_enumerator() {
    let (mut sess, _cap) = Session::for_test();
    let tcx = TyCtxt::new();
    let a = intern(&mut sess, "A");
    let b = intern(&mut sess, "B");
    let c = intern(&mut sess, "C");
    let b_value = Expr {
        id: NodeId(0),
        kind: ExprKind::Binary {
            op: rcc_ast::BinOp::Add,
            lhs: Box::new(ident_expr(&mut sess, "A")),
            rhs: Box::new(int_lit("7", &mut sess)),
        },
        span: DUMMY_SP,
    };
    let spec =
        enum_spec(None, vec![(a, Some(int_lit("148", &mut sess))), (b, Some(b_value)), (c, None)]);
    let mut resolver = Resolver::default();
    let mut crate_ = HirCrate::default();

    let kind = lower_enum(&spec, &tcx, &mut resolver, &mut crate_, &mut sess);

    match kind {
        DefKind::Enum { variants, .. } => {
            let values: Vec<i128> = variants.iter().map(|v| v.value).collect();
            assert_eq!(values, vec![148, 155, 156]);
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

fn hir_int_value(body: &Body, id: HirExprId) -> Option<i128> {
    match body.exprs[id].kind {
        HirExprKind::IntLiteral { value, .. } | HirExprKind::IntConst(value) => Some(value),
        _ => None,
    }
}

fn init_assign_operands(stmt: &HirStmt) -> Option<(HirExprId, HirExprId)> {
    match stmt.kind {
        HirStmtKind::InitAssign { lhs, rhs } => Some((lhs, rhs)),
        _ => None,
    }
}

fn local_array_int_writes(body: &Body, local: Local) -> Vec<(i128, i128)> {
    let mut writes = Vec::new();
    for stmt in body.stmts.iter() {
        let Some((lhs, rhs)) = init_assign_operands(stmt) else { continue };
        let HirExprKind::Index { base, index } = &body.exprs[lhs].kind else {
            continue;
        };
        if !matches!(&body.exprs[*base].kind, HirExprKind::LocalRef(l) if *l == local) {
            continue;
        }
        let Some(i) = hir_int_value(body, *index) else {
            continue;
        };
        let Some(v) = hir_int_value(body, rhs) else {
            continue;
        };
        writes.push((i, v));
    }
    writes
}

fn local_field_array_int_writes(body: &Body, local: Local, field_index: u32) -> Vec<(i128, i128)> {
    let mut writes = Vec::new();
    for stmt in body.stmts.iter() {
        let Some((lhs, rhs)) = init_assign_operands(stmt) else { continue };
        let HirExprKind::Index { base, index } = &body.exprs[lhs].kind else {
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
        let Some(i) = hir_int_value(body, *index) else {
            continue;
        };
        let Some(v) = hir_int_value(body, rhs) else {
            continue;
        };
        writes.push((i, v));
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
    let Some((_lhs, rhs)) = init_assign_operands(&body.stmts[out[0]]) else {
        panic!("expected InitAssign stmt, got {:?}", body.stmts[out[0]].kind);
    };
    assert_eq!(hir_int_value(&body, rhs), Some(7));
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
        let Some((lhs, rhs)) = init_assign_operands(&body.stmts[*sid]) else { continue };
        let HirExprKind::Index { index, .. } = body.exprs[lhs].kind else { continue };
        let Some(i) = hir_int_value(&body, index) else { continue };
        let Some(v) = hir_int_value(&body, rhs) else { continue };
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
        let Some((lhs, rhs)) = init_assign_operands(&body.stmts[*sid]) else { continue };
        let HirExprKind::Index { index, .. } = body.exprs[lhs].kind else { continue };
        let Some(i) = hir_int_value(&body, index) else { continue };
        let Some(v) = hir_int_value(&body, rhs) else { continue };
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
fn snippet_gnu_range_designator_accepts_enum_constant_bounds() {
    let (hir, _tcx, cap) = checked_snippet_with_options(
        "enum { OP_COUNT = 250 }; void f(void) { int table[256] = { [OP_COUNT ... 255] = 7 }; }",
        Options { gnu_range_designators: true, ..Options::default() },
    );
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    assert!(hir.bodies.values().next().is_some(), "snippet should lower a function body");
}

#[test]
fn quickjs_nan_boxing_jsvalueconst_initializers_survive_typeck() {
    let src = r#"
        typedef struct JSContext JSContext;
        typedef long long int64_t;
        typedef unsigned long long uint64_t;
        typedef uint64_t JSValue;
        #define JSValueConst JSValue

        JSValue JS_NewInt64(JSContext *ctx, int64_t val);

        struct array_sort_context {
            JSContext *ctx;
            int exception;
            int has_method;
            JSValueConst method;
        };

        void f(JSContext *ctx, JSValue element, JSValue source, JSValueConst *argv) {
            JSValueConst args[3] = { element, JS_NewInt64(ctx, 1), source };
            struct array_sort_context asc = { ctx, 0, 0, argv[0] };
        }
    "#;
    let (_hir, _tcx, cap, _sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "diagnostics: {:?}",
        cap.diagnostics()
    );
}

#[test]
fn quickjs_struct_jsvalueconst_initializers_survive_typeck() {
    let src = r#"
        typedef struct JSContext JSContext;
        typedef long int64_t;
        typedef union JSValueUnion {
            int int32;
            double float64;
            void *ptr;
            int64_t short_big_int;
        } JSValueUnion;
        typedef struct JSValue {
            JSValueUnion u;
            int64_t tag;
        } JSValue;
        #define JSValueConst JSValue
        #define JS_MKVAL(tag, val) (JSValue){ (JSValueUnion){ .int32 = val }, tag }

        JSValue JS_NewInt64(JSContext *ctx, int64_t val);

        struct array_sort_context {
            JSContext *ctx;
            int exception;
            int has_method;
            JSValueConst method;
        };

        void f(JSContext *ctx, JSValue element, JSValue source, JSValueConst *argv) {
            JSValueConst args[3] = { element, JS_NewInt64(ctx, 1), source };
            struct array_sort_context asc = { ctx, 0, 0, argv[0] };
            JSValue v = JS_MKVAL(0, 1);
        }
    "#;
    let (_hir, _tcx, cap, _sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "diagnostics: {:?}",
        cap.diagnostics()
    );
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
            packed: false,
            ms_bitfields: false,
            align_override: None,
            scalar_storage_order: None,
            layout: None,
            fields: vec![
                rcc_hir::Field {
                    name: Some(a),
                    ty: tcx.int,
                    quals: ObjectQuals::none(),
                    align_override: None,
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                rcc_hir::Field {
                    name: Some(b),
                    ty: tcx.int,
                    quals: ObjectQuals::none(),
                    align_override: None,
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
        let Some((_lhs, rhs)) = init_assign_operands(&body.stmts[*sid]) else { continue };
        if let Some(v) = hir_int_value(&body, rhs) {
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
    assert_eq!(hir_int_value(&body, eid), Some(42));
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
    assert_eq!(hir_int_value(&body, cond), Some(1));
    assert_eq!(hir_int_value(&body, then_expr), Some(2));
    assert_eq!(hir_int_value(&body, else_expr), Some(3));
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
            packed: false,
            ms_bitfields: false,
            align_override: None,
            scalar_storage_order: None,
            layout: None,
            fields: vec![rcc_hir::Field {
                name: Some(a),
                ty: tcx.int,
                quals: ObjectQuals::none(),
                align_override: None,
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
    assert_eq!(hir_int_value(&body, id), Some(42));
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
    assert_eq!(tys.len(), 1);
    assert!(tys.iter().all(|ty| *ty == tcx.int));
}

#[test]
fn snippet_function_declaration_is_function_prototype() {
    let (hir, tcx) = lower_snippet("int f(int);");
    let DefKind::Function { ty, has_body, is_static, is_inline, is_extern_inline, .. } =
        hir.defs[DefId(0)].kind
    else {
        panic!("expected file-scope function declaration as function prototype def");
    };
    assert!(!has_body);
    assert!(!is_static);
    assert!(!is_inline);
    assert!(!is_extern_inline);
    match tcx.get(ty) {
        Ty::Func { ret, params, variadic: false, proto: true } => {
            assert_eq!(*ret, tcx.int);
            assert_eq!(params.as_slice(), &[tcx.int]);
        }
        other => panic!("expected function type, got {other:?}"),
    }
}

#[test]
fn snippet_function_typedef_declaration_merges_with_definition() {
    let src = "typedef int functype(int); extern functype func; int func(int i) { return i + 1; }";
    let (hir, tcx, cap) =
        checked_snippet_with_options(src, Options { gnu_typeof: true, ..Options::default() });
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let function_defs: Vec<_> =
        hir.defs.iter().filter(|def| matches!(def.kind, DefKind::Function { .. })).collect();
    assert_eq!(function_defs.len(), 1, "function typedef declaration should not create a global");
    let DefKind::Function { ty, has_body, .. } = function_defs[0].kind else {
        unreachable!();
    };
    assert!(has_body);
    assert!(
        matches!(tcx.get(ty), Ty::Func { ret, params, .. } if *ret == tcx.int && params == &[tcx.int])
    );
}

#[test]
fn snippet_gnu_typeof_function_redeclaration_merges_with_function_def() {
    let src = r#"
        int set_anon_super(void);
        int set_anon_super(void) { return 42; }
        typedef int sas_type(void);
        extern typeof(set_anon_super) set_anon_super;
        extern sas_type set_anon_super;
    "#;
    let (hir, tcx, cap) =
        checked_snippet_with_options(src, Options { gnu_typeof: true, ..Options::default() });
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let function_defs: Vec<_> =
        hir.defs.iter().filter(|def| matches!(def.kind, DefKind::Function { .. })).collect();
    assert_eq!(function_defs.len(), 1, "typeof redeclarations should reuse the function def");
    let DefKind::Function { ty, has_body, .. } = function_defs[0].kind else {
        unreachable!();
    };
    assert!(has_body);
    assert!(
        matches!(tcx.get(ty), Ty::Func { ret, params, proto: true, .. } if *ret == tcx.int && params.is_empty())
    );
}

#[test]
fn builtin_va_list_function_parameter_adjusts_to_pointer() {
    let (hir, tcx) = lower_snippet("typedef __builtin_va_list va_list; int sink(va_list);");
    let def = hir
        .defs
        .iter()
        .find(|def| matches!(def.kind, DefKind::Function { .. }))
        .expect("expected function declaration");
    let DefKind::Function { ty, .. } = def.kind else { unreachable!() };
    match tcx.get(ty) {
        Ty::Func { params, .. } => {
            assert_eq!(params.len(), 1);
            assert!(
                matches!(tcx.get(params[0]), Ty::Ptr(q) if q.ty == tcx.builtin_va_list),
                "__builtin_va_list parameter should adjust to pointer, got {:?}",
                tcx.get(params[0])
            );
        }
        other => panic!("expected function type, got {other:?}"),
    }
}

#[test]
fn snippet_function_prototype_storage_flags_are_preserved() {
    let (hir, _tcx) = lower_snippet("static int s(int); extern int e(int);");
    let DefKind::Function { has_body, is_static, is_inline, is_extern_inline, .. } =
        hir.defs[DefId(0)].kind
    else {
        panic!("expected static prototype function def");
    };
    assert!(!has_body);
    assert!(is_static);
    assert!(!is_inline);
    assert!(!is_extern_inline);

    let DefKind::Function { has_body, is_static, is_inline, is_extern_inline, .. } =
        hir.defs[DefId(1)].kind
    else {
        panic!("expected extern prototype function def");
    };
    assert!(!has_body);
    assert!(!is_static);
    assert!(!is_inline);
    assert!(!is_extern_inline);
}

#[test]
fn snippet_function_pointer_object_remains_global() {
    let (hir, tcx) = lower_snippet("int (*fp)(int);");
    let DefKind::Global { ty, .. } = hir.defs[DefId(0)].kind else {
        panic!("function pointer object should remain a global object");
    };
    match tcx.get(ty) {
        Ty::Ptr(pointee) => match tcx.get(pointee.ty) {
            Ty::Func { ret, params, variadic: false, proto: true } => {
                assert_eq!(*ret, tcx.int);
                assert_eq!(params.as_slice(), &[tcx.int]);
            }
            other => panic!("expected pointer to function, got pointer to {other:?}"),
        },
        other => panic!("expected pointer-to-function object type, got {other:?}"),
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
            HirExprKind::IntLiteral { value: 0, .. } | HirExprKind::IntConst(0) => {
                panic!("sizeof(type) must not lower to zero placeholder")
            }
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
    let Some((lhs, rhs)) = init_assign_operands(&body.stmts[init_stmts[0]]) else {
        panic!("compound literal init must be an InitAssign statement");
    };
    assert!(matches!(body.exprs[lhs].kind, HirExprKind::LocalRef(l) if l == literal_local));
    assert_eq!(hir_int_value(body, rhs), Some(1));
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
            let Some((lhs, rhs)) = init_assign_operands(&body.stmts[*stmt]) else { return false };
            matches!(body.exprs[lhs].kind, HirExprKind::Field { base, field_index: 0 }
                if matches!(body.exprs[base].kind, HirExprKind::LocalRef(l) if l == literal_local))
                && hir_int_value(body, rhs) == Some(1)
        }),
        "record compound literal should initialise field x via lower_initializer"
    );
}

#[test]
fn snippet_record_initializer_keeps_aggregate_compound_literal_as_subobject() {
    let src = r#"
        typedef union {
            int int32;
            void *ptr;
        } JSValueUnion;
        typedef struct {
            JSValueUnion u;
            long tag;
        } JSValue;

        void f(int val) {
            JSValue v = (JSValue){ (JSValueUnion){ .int32 = val }, 1 };
        }
    "#;
    let (hir, tcx, cap) = checked_snippet_with_diagnostics(src);
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());

    let body = hir.bodies.values().next().expect("missing function body");
    let outer_literal = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::CompoundLiteral { ty, local, ref init_stmts }
                if matches!(tcx.get(ty), Ty::Record(def_id)
                if matches!(
                    hir.defs[*def_id].kind,
                    DefKind::Record { kind: RecordKind::Struct, .. }
                )) =>
            {
                Some((local, init_stmts))
            }
            _ => None,
        })
        .expect("missing JSValue compound literal");
    let saw_whole_union_assignment = outer_literal.1.iter().any(|stmt| {
        let Some((lhs, rhs)) = init_assign_operands(&body.stmts[*stmt]) else { return false };
        matches!(body.exprs[lhs].kind, HirExprKind::Field { base, field_index: 0 }
            if matches!(body.exprs[base].kind, HirExprKind::LocalRef(local) if local == outer_literal.0))
            && body.exprs[lhs].ty == body.exprs[rhs].ty
            && matches!(tcx.get(body.exprs[rhs].ty), Ty::Record(def_id)
                if matches!(
                    hir.defs[*def_id].kind,
                    DefKind::Record { kind: RecordKind::Union, .. }
                ))
    });
    assert!(
        saw_whole_union_assignment,
        "aggregate compound literal must initialize the union subobject as a whole"
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
            let Some((lhs, rhs)) = init_assign_operands(&body.stmts[*stmt]) else { return false };
            matches!(body.exprs[lhs].kind, HirExprKind::Index { base, index }
                if matches!(body.exprs[base].kind, HirExprKind::LocalRef(l) if l == literal_local)
                    && hir_int_value(body, index) == Some(1))
                && hir_int_value(body, rhs) == Some(2)
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
        let Some((lhs, rhs)) = init_assign_operands(stmt) else { continue };
        let HirExprKind::Index { base, index } = body.exprs[lhs].kind else { continue };
        if !matches!(body.exprs[base].kind, HirExprKind::LocalRef(Local(0))) {
            continue;
        }
        let Some(i) = hir_int_value(body, index) else { continue };
        let Some(v) = hir_int_value(body, rhs) else { continue };
        elems.push((i, v));
    }
    elems.sort();
    assert_eq!(elems, vec![(0, 104), (1, 105), (2, 0)]);
}

#[test]
fn snippet_braced_char_array_string_initializer_completes_length() {
    let (hir, tcx) = lower_snippet("void f(void) { char s[] = { \"hi\" }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.char_),
        other => panic!("expected completed char[3], got {other:?}"),
    }
}

#[test]
fn snippet_local_struct_char_array_string_initializer_writes_subobject_chars() {
    let (hir, _tcx) =
        lower_snippet("void f(void) { struct S { char x[3]; }; struct S s = { \"abc\" }; }");
    let body = hir.bodies.values().next().expect("missing function body");
    let mut elems = Vec::new();
    for stmt in body.stmts.iter() {
        let Some((lhs, rhs)) = init_assign_operands(stmt) else { continue };
        let HirExprKind::Index { base, index } = body.exprs[lhs].kind else { continue };
        let HirExprKind::Field { base: field_base, field_index: 0 } = body.exprs[base].kind else {
            continue;
        };
        if !matches!(body.exprs[field_base].kind, HirExprKind::LocalRef(Local(0))) {
            continue;
        }
        let Some(i) = hir_int_value(body, index) else { continue };
        let Some(v) = hir_int_value(body, rhs) else { continue };
        elems.push((i, v));
    }
    elems.sort();
    assert_eq!(elems, vec![(0, 97), (1, 98), (2, 99)]);
}

#[test]
fn snippet_wide_string_initializer_completes_wchar_array_by_codepoint() {
    let (hir, tcx) = lower_snippet("typedef int wchar_t; void f(void) { wchar_t s[] = L\"A世\"; }");
    let body = hir.bodies.values().next().expect("missing function body");
    match tcx.get(body.locals[Local(0)].ty) {
        Ty::Array { elem, len: Some(3), is_vla: false } => assert_eq!(elem.ty, tcx.int),
        other => panic!("expected completed wchar_t[3], got {other:?}"),
    }
    let mut elems = Vec::new();
    for stmt in body.stmts.iter() {
        let Some((lhs, rhs)) = init_assign_operands(stmt) else { continue };
        let HirExprKind::Index { base, index } = body.exprs[lhs].kind else { continue };
        if !matches!(body.exprs[base].kind, HirExprKind::LocalRef(Local(0))) {
            continue;
        }
        let Some(i) = hir_int_value(body, index) else { continue };
        let Some(v) = hir_int_value(body, rhs) else { continue };
        elems.push((i, v));
    }
    elems.sort();
    assert_eq!(elems, vec![(0, 0x41), (1, 0x4e16), (2, 0)]);
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
fn snippet_block_static_label_address_initializer_is_preserved() {
    let (hir, _tcx) =
        lower_snippet("void f(void) { static void *p[] = { &&L }; goto *p[0]; L: ; }");
    let mut found = false;
    for def in hir.defs.iter() {
        let DefKind::Global { init: Some(init), .. } = &def.kind else {
            continue;
        };
        found |= init
            .entries
            .iter()
            .any(|entry| matches!(entry.value, GlobalInitValue::LabelAddress { .. }));
    }
    assert!(found, "block-scope static initializer should preserve LabelAddress");
}

fn global_init_int_bytes(hir: &HirCrate, def_id: DefId) -> Vec<i128> {
    let DefKind::Global { init: Some(init), .. } = &hir.defs[def_id].kind else {
        panic!("expected string global, got {:?}", hir.defs[def_id].kind);
    };
    init.entries
        .iter()
        .map(|entry| match entry.value {
            GlobalInitValue::Int(v) => v,
            ref other => panic!("expected int byte, got {other:?}"),
        })
        .collect()
}

#[test]
fn snippet_func_predefined_identifier_lowers_to_function_name_string() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("char *f(void) { return __func__; }");
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let body = hir.bodies.values().next().expect("function body");
    let def_id = body
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            HirExprKind::StringRef(def_id) => Some(def_id),
            _ => None,
        })
        .expect("__func__ should lower to StringRef");
    assert_eq!(global_init_int_bytes(&hir, def_id), vec![b'f' as i128, 0]);
}

#[test]
fn snippet_function_alias_warns_in_strict_mode() {
    let (_hir, _tcx, cap) =
        checked_snippet_with_diagnostics("char *g(void) { return __FUNCTION__; }");
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0022)),
        "strict mode should warn for __FUNCTION__: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::E0071)),
        "__FUNCTION__ must not be reported as undeclared: {diags:?}"
    );
}

#[test]
fn snippet_function_alias_option_suppresses_warning() {
    let opts = Options { gnu_function_names: true, ..Options::default() };
    let (_hir, _tcx, cap) =
        checked_snippet_with_options("char *g(void) { return __FUNCTION__; }", opts);
    let diags = cap.diagnostics();
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0022)),
        "GNU option should suppress W0022: {diags:?}"
    );
}

#[test]
fn snippet_va_area_lowers_inside_variadic_function() {
    let (hir, _tcx, cap) =
        checked_snippet_with_diagnostics("void f(int n, ...) { void *p = __va_area__; }");
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0023)),
        "strict mode should warn for __va_area__: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::E0071)),
        "__va_area__ must not be reported as undeclared in a variadic function: {diags:?}"
    );
    let body = hir.bodies.values().next().expect("function body");
    assert!(
        body.exprs.iter().any(|expr| matches!(expr.kind, HirExprKind::BuiltinVaArea)),
        "__va_area__ should lower to a dedicated HIR node"
    );
}

#[test]
fn snippet_va_area_option_suppresses_warning() {
    let opts = Options { gnu_va_area: true, ..Options::default() };
    let (_hir, _tcx, cap) =
        checked_snippet_with_options("void f(int n, ...) { void *p = __va_area__; }", opts);
    let diags = cap.diagnostics();
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0023)),
        "GNU option should suppress W0023: {diags:?}"
    );
}

#[test]
fn snippet_va_area_rejects_non_variadic_function() {
    let (_hir, _tcx, cap) =
        checked_snippet_with_diagnostics("void f(void) { void *p = __va_area__; }");
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::E0071)),
        "non-variadic __va_area__ use should be rejected: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0023)),
        "invalid __va_area__ use should not also emit compatibility warning: {diags:?}"
    );
}

#[test]
fn inline_asm_extended_operands_validate_and_lower_side_effects() {
    let src = r#"
        int f(int in) {
            int out;
            asm volatile ("mov %1, %0" : "=r"(out) : "0"(in) : "cc", "memory");
            return out;
        }
    "#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        !diags.iter().any(|d| d.code == Some(rcc_errors::codes::E0032)),
        "valid x86-64 inline asm should pass validation: {diags:?}"
    );

    let body = hir.bodies.values().next().expect("function body");
    let asm = body
        .stmts
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStmtKind::InlineAsm(asm) => Some(asm),
            _ => None,
        })
        .expect("inline asm should be preserved for CFG/codegen");
    assert_eq!(asm.template, "mov %1, %0");
    assert!(asm.quals.volatile);
    assert_eq!(asm.outputs.len(), 1);
    assert_eq!(asm.outputs[0].constraint, "=r");
    assert_eq!(asm.inputs.len(), 1);
    assert_eq!(asm.inputs[0].constraint, "0");
    assert_eq!(asm.clobbers, ["cc", "memory"]);
}

#[test]
fn inline_asm_rejects_output_without_write_marker() {
    let src = r#"int f(int x) { asm("" : "r"(x)); return x; }"#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0032)
                && d.message.contains("output inline asm constraint")
        }),
        "expected output constraint error: {diags:?}"
    );
}

#[test]
fn inline_asm_rejects_missing_matching_output() {
    let src = r#"int f(int x) { asm("" : : "1"(x)); return x; }"#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0032)
                && d.message.contains("does not name an output operand")
        }),
        "expected matching constraint error: {diags:?}"
    );
}

#[test]
fn inline_asm_rejects_duplicate_symbolic_operand_names() {
    let src = r#"
        int f(int a, int b) {
            asm("" : [x] "=r"(a) : [x] "r"(b));
            return a;
        }
    "#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0032)
                && d.message.contains("duplicate inline asm operand name")
        }),
        "expected duplicate operand name error: {diags:?}"
    );
}

#[test]
fn inline_asm_rejects_unsupported_target_constraint() {
    let src = r#"int f(int x) { asm("" : "=foo"(x)); return x; }"#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0032)
                && d.message.contains("unsupported output inline asm constraint")
        }),
        "expected unsupported constraint policy error: {diags:?}"
    );
}

#[test]
fn inline_asm_goto_policy_is_explicit() {
    let src = r#"int f(void) { asm goto ("nop"); return 0; }"#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_hir, _tcx, cap) = checked_snippet_with_options(src, opts);
    let diags = cap.diagnostics();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(rcc_errors::codes::E0032) && d.message.contains("asm goto")
        }),
        "expected asm goto policy error: {diags:?}"
    );
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
fn snippet_global_struct_char_array_string_initializer_has_subobject_payload() {
    let (hir, _tcx) = lower_snippet("struct S { char x[3]; }; struct S s = { \"abc\" };");
    let def = hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).unwrap();
    let DefKind::Global { init: Some(init), .. } = &def.kind else {
        panic!("expected global with initializer, got {:?}", def.kind);
    };
    let values: Vec<_> = init
        .entries
        .iter()
        .map(|entry| {
            let [GlobalInitDesignator::Field(0), GlobalInitDesignator::Index(i)] =
                entry.path.as_slice()
            else {
                panic!("expected .x[index] path, got {:?}", entry.path);
            };
            let GlobalInitValue::Int(v) = entry.value else {
                panic!("expected int byte, got {:?}", entry.value);
            };
            (*i, v)
        })
        .collect();
    assert_eq!(values, vec![(0, 97), (1, 98), (2, 99)]);
}

#[test]
fn snippet_typeck_folds_global_integer_initializer_expr() {
    let (hir, _tcx, cap) = checked_snippet_with_diagnostics("int x = 2 + 3;");
    let (def_id, def) = hir
        .defs
        .iter_enumerated()
        .find(|(_, d)| {
            matches!(&d.kind, DefKind::Global { init: Some(init), .. } if !init.entries.is_empty())
        })
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
        .find(|(_, d)| {
            matches!(&d.kind, DefKind::Global { init: Some(init), .. } if !init.entries.is_empty())
        })
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
    let GlobalInitValue::Address { def: Some(string_def), offset: 0 } = init.entries[0].value
    else {
        panic!("string pointer initializer should fold to the literal address");
    };
    assert!(
        matches!(hir.defs[string_def].kind, DefKind::Global { init: Some(_), .. }),
        "string literal address should target a synthetic initialized global"
    );
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
    let (hir, _tcx, cap) = lower_snippet_with_diagnostics("int a, a;");
    assert!(
        cap.diagnostics().iter().all(|d| d.code != Some(rcc_errors::codes::E0070)),
        "repeated file-scope tentative declarations should merge without E0070"
    );
    assert_eq!(hir.defs.iter().filter(|def| matches!(def.kind, DefKind::Global { .. })).count(), 1);
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
    let DefKind::Global { linkage, init, .. } = &def.kind else {
        unreachable!();
    };
    assert_eq!(*linkage, Linkage::External);
    assert!(init.is_none(), "`extern int errno;` is a declaration, not a tentative definition");
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
fn snippet_tentative_global_gets_zero_initializer_marker() {
    let (hir, tcx) = lower_snippet("int counter;");
    let def =
        hir.defs.iter().find(|d| matches!(d.kind, DefKind::Global { .. })).expect("missing global");
    let DefKind::Global { linkage, init: Some(init), ty, .. } = &def.kind else {
        panic!("expected tentative definition to carry an empty zero-init marker");
    };
    assert_eq!(*linkage, Linkage::External);
    assert_eq!(*ty, tcx.int);
    assert_eq!(init.ty, tcx.int);
    assert!(init.entries.is_empty(), "empty GlobalInit means implicit zero initialization");
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
        Ty::Atomic(inner) => assert_ty_has_no_error(tcx, *inner, context),
        Ty::Array { elem, .. } => assert_ty_has_no_error(tcx, elem.ty, context),
        Ty::Vector { elem, .. } => assert_ty_has_no_error(tcx, *elem, context),
        Ty::Func { ret, params, .. } => {
            assert_ty_has_no_error(tcx, *ret, context);
            for param in params {
                assert_ty_has_no_error(tcx, *param, context);
            }
        }
        Ty::Void
        | Ty::Int { .. }
        | Ty::Float(_)
        | Ty::Complex(_)
        | Ty::Record(_)
        | Ty::Enum(_)
        | Ty::BuiltinVaList => {}
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

fn assert_no_global_direct_function_types(hir: &HirCrate, tcx: &TyCtxt) {
    for def in hir.defs.iter() {
        if let DefKind::Global { ty, .. } = def.kind {
            assert!(
                !matches!(tcx.get(ty), Ty::Func { .. }),
                "global object {:?} carries a direct function type",
                def.id
            );
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
fn regression_gate_file_scope_typedef_record_pointer_field_resolves_before_tag_materialization() {
    let src = r#"
        typedef unsigned int FFelem;
        struct DUPFFstruct {
            int maxdeg;
            int deg;
            FFelem *coeffs;
        };
    "#;
    let (hir, tcx, cap, sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );
    assert_no_def_or_local_error_types(&hir, &tcx);

    let rec = def_named(&hir, &sess, "DUPFFstruct");
    let DefKind::Record { fields, .. } = &rec.kind else {
        panic!("DUPFFstruct should be a record def");
    };
    let coeffs = fields
        .iter()
        .find(|field| field.name.is_some_and(|sym| sess.interner.get(sym) == "coeffs"))
        .expect("missing coeffs field");
    match tcx.get(coeffs.ty) {
        Ty::Ptr(q) => assert_eq!(q.ty, tcx.uint),
        other => panic!("coeffs should be FFelem* / unsigned int*, got {other:?}"),
    }
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
fn regression_gate_function_prototype_call_uses_function_def() {
    let (hir, tcx, cap, sess) =
        lower_and_typeck_snippet("int callee(int); int f(void) { return callee(7); }");
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean prototype call should not emit errors: {:?}",
        cap.diagnostics()
    );
    assert_no_def_or_local_error_types(&hir, &tcx);
    assert_no_body_expr_error_types_after_typeck(&hir, &tcx);
    assert_no_global_direct_function_types(&hir, &tcx);

    let callee = def_named(&hir, &sess, "callee");
    let DefKind::Function { ty, has_body, is_static, .. } = callee.kind else {
        panic!("callee should be a function prototype def");
    };
    assert!(!has_body);
    assert!(!is_static);
    match tcx.get(ty) {
        Ty::Func { ret, params, variadic: false, proto: true } => {
            assert_eq!(*ret, tcx.int);
            assert_eq!(params.as_slice(), &[tcx.int]);
        }
        other => panic!("expected callee function type, got {other:?}"),
    }

    let f = def_named(&hir, &sess, "f");
    let DefKind::Function { has_body, .. } = f.kind else {
        panic!("f should be a function definition");
    };
    assert!(has_body);
}

#[test]
fn regression_gate_function_prototype_and_definition_share_def_id() {
    let (hir, tcx) = lower_snippet("int f(void); int f(void) { return 0; }");
    let defs: Vec<_> = hir
        .defs
        .iter_enumerated()
        .filter(|(_, def)| matches!(def.kind, DefKind::Function { .. }))
        .collect();
    assert_eq!(defs.len(), 1, "prototype and definition should merge to one function def");
    let (def_id, def) = defs[0];
    let DefKind::Function { ty, has_body, .. } = def.kind else {
        unreachable!();
    };
    assert!(has_body);
    assert!(hir.bodies.contains_key(&def_id), "function body should use the merged DefId");
    assert!(matches!(tcx.get(ty), Ty::Func { .. }));
    assert_no_global_direct_function_types(&hir, &tcx);
}

#[test]
fn regression_gate_kr_parameter_declaration_sets_body_param_type() {
    let (hir, tcx, cap, sess) = lower_and_typeck_snippet(
        r#"
        f(a)
             unsigned long a;
        {
          return a == 0xdeadbeefL;
        }
        "#,
    );
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "K&R fixture should only warn, got {:?}",
        cap.diagnostics()
    );

    let f = def_named(&hir, &sess, "f");
    let DefKind::Function { ty, has_body, .. } = f.kind else {
        panic!("f should be a function definition");
    };
    assert!(has_body);
    match tcx.get(ty) {
        Ty::Func { ret, params, proto: false, .. } => {
            assert_eq!(*ret, tcx.int);
            assert_eq!(params.as_slice(), &[tcx.ulong]);
        }
        other => panic!("expected old-style function with unsigned long param, got {other:?}"),
    }

    let body = hir.bodies.get(&f.id).expect("missing f body");
    let a = body
        .locals
        .iter()
        .find(|local| local.name.is_some_and(|sym| sess.interner.get(sym) == "a"))
        .expect("missing K&R parameter local a");
    assert_eq!(a.ty, tcx.ulong);
    assert!(a.is_param);
}

#[test]
fn regression_gate_vla_parameter_bound_side_effects_enter_body() {
    let (hir, _tcx, cap, sess) =
        lower_and_typeck_snippet("int foo(int a, int b[a++], int c, int d[c++]) { return a + c; }");
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let foo = def_named(&hir, &sess, "foo");
    let body = hir.bodies.get(&foo.id).expect("missing foo body");
    assert_eq!(body.locals.len(), 4, "expected four adjusted parameters");
    let root = body.root.expect("missing root statement");
    let HirStmtKind::Block(stmts) = &body.stmts[root].kind else {
        panic!("function body root should be a block");
    };
    assert!(stmts.len() >= 3, "expected two entry side effects before user body");
    assert_post_inc_stmt_targets_local(body, stmts[0], Local(0));
    assert_post_inc_stmt_targets_local(body, stmts[1], Local(2));
}

#[test]
fn regression_gate_block_scope_extern_binds_file_scope_object() {
    let (hir, _tcx, cap, sess) = lower_and_typeck_snippet(
        "int v = 3; int f(void) { int v = 4; { extern int v; return v; } }",
    );
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let global_v = def_named(&hir, &sess, "v");
    let f = def_named(&hir, &sess, "f");
    let body = hir.bodies.get(&f.id).expect("missing f body");
    let root = body.root.expect("missing root");
    let ret = first_return_expr(body, root).expect("missing return expression");
    assert!(
        expr_references_def(body, ret, global_v.id),
        "inner `extern int v` return should load the file-scope object"
    );
    assert!(
        !expr_references_local(body, ret, Local(0)),
        "inner `extern int v` must not resolve to the block local"
    );
}

#[test]
fn regression_gate_float_literal_suffix_survives_typeck() {
    let (hir, tcx, cap, _sess) = lower_and_typeck_snippet(
        "float f(void) { return 1.0f; } long double g(void) { return 2.0L; }",
    );
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );
    let literal_tys = hir
        .bodies
        .values()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| matches!(expr.kind, HirExprKind::FloatConst(_)).then_some(expr.ty))
        .collect::<Vec<_>>();
    assert!(literal_tys.contains(&tcx.float), "`1.0f` should remain float");
    assert!(literal_tys.contains(&tcx.long_double), "`2.0L` should remain long double");
}

#[test]
fn regression_gate_va_list_call_argument_decays_to_pointer() {
    let src = r#"
        typedef __builtin_va_list va_list;
        void sink(va_list);
        void f(int n, ...) {
            va_list ap;
            sink(ap);
        }
    "#;
    let (hir, tcx, cap, sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let f = def_named(&hir, &sess, "f");
    let body = hir.bodies.get(&f.id).expect("missing f body");
    let call = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, HirExprKind::Call { .. }))
        .expect("missing sink call");
    let HirExprKind::Call { args, .. } = &call.kind else { unreachable!() };
    assert_eq!(args.len(), 1);
    let arg = args[0];
    assert!(
        matches!(tcx.get(body.exprs[arg].ty), Ty::Ptr(q) if q.ty == tcx.builtin_va_list),
        "va_list call argument should be pointer-adjusted, got {:?}",
        tcx.get(body.exprs[arg].ty)
    );
    assert!(
        matches!(body.exprs[arg].kind, HirExprKind::Convert { kind: ConvertKind::ArrayToPtr, .. }),
        "va_list call argument should carry array-style decay, got {:?}",
        body.exprs[arg].kind
    );
}

#[test]
fn regression_gate_builtin_offsetof_lowers_field_and_array_path() {
    let src = r#"
        struct U { short a; short b; };
        struct T { int tag; struct U arr[3]; };
        int f(void) { return __builtin_offsetof(struct T, arr[2].b); }
    "#;
    let (hir, _tcx, cap, sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let f = def_named(&hir, &sess, "f");
    let body = hir.bodies.get(&f.id).expect("missing f body");
    assert!(
        body.exprs.iter().any(|expr| matches!(expr.kind, HirExprKind::IntConst(14))),
        "offsetof(struct T, arr[2].b) should lower to byte offset 14; exprs={:?}",
        body.exprs
    );
}

#[test]
fn common_builtins_fold_in_hir_and_typeck() {
    let src = r#"
        typedef int I;
        int f(int x) {
            return __builtin_types_compatible_p(I, int)
                + 10 * __builtin_types_compatible_p(I, long)
                + 100 * __builtin_constant_p(1 + 2)
                + 1000 * __builtin_constant_p(x)
                + __builtin_bswap32(0x01020304U);
        }
    "#;
    let (hir, _tcx, cap, sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let f = def_named(&hir, &sess, "f");
    let body = hir.bodies.get(&f.id).expect("missing f body");
    for expected in [1, 0, 67_305_985] {
        assert!(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, HirExprKind::IntConst(v) if v == expected)),
            "expected folded IntConst({expected}) in exprs={:?}",
            body.exprs
        );
    }
}

#[test]
fn runtime_bswap_survives_typeck_for_cfg_and_codegen() {
    let src = "unsigned f(unsigned x) { return __builtin_bswap32(x); }";
    let (hir, _tcx, cap, sess) = lower_and_typeck_snippet(src);
    assert!(
        cap.diagnostics().iter().all(|d| d.level != rcc_errors::Level::Error),
        "clean fixture should not emit errors: {:?}",
        cap.diagnostics()
    );

    let f = def_named(&hir, &sess, "f");
    let body = hir.bodies.get(&f.id).expect("missing f body");
    assert!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, HirExprKind::BuiltinBswap { bits: 32, .. })),
        "runtime bswap should remain a builtin node for CFG/codegen: {:?}",
        body.exprs
    );
}

fn assert_post_inc_stmt_targets_local(body: &Body, stmt: rcc_hir::HirStmtId, expected: Local) {
    let HirStmtKind::Expr(expr) = body.stmts[stmt].kind else {
        panic!("expected parameter-bound expression statement");
    };
    let HirExprKind::Unary { op: rcc_hir::rcc_hir_binop::UnOp::PostInc, operand } =
        body.exprs[expr].kind
    else {
        panic!("expected parameter-bound post-increment expression");
    };
    assert!(
        matches!(body.exprs[operand].kind, HirExprKind::LocalRef(local) if local == expected),
        "post-increment should target {expected:?}"
    );
}

fn first_return_expr(body: &Body, stmt: rcc_hir::HirStmtId) -> Option<HirExprId> {
    match &body.stmts[stmt].kind {
        HirStmtKind::Return(expr) => *expr,
        HirStmtKind::Block(stmts) => stmts.iter().find_map(|stmt| first_return_expr(body, *stmt)),
        HirStmtKind::If { then_branch, else_branch, .. } => first_return_expr(body, *then_branch)
            .or_else(|| else_branch.and_then(|stmt| first_return_expr(body, stmt))),
        HirStmtKind::Label { body: inner, .. }
        | HirStmtKind::Case { body: inner, .. }
        | HirStmtKind::Default { body: inner }
        | HirStmtKind::While { body: inner, .. }
        | HirStmtKind::DoWhile { body: inner, .. }
        | HirStmtKind::Switch { body: inner, .. } => first_return_expr(body, *inner),
        HirStmtKind::For { init, body: inner, .. } => init
            .and_then(|stmt| first_return_expr(body, stmt))
            .or_else(|| first_return_expr(body, *inner)),
        HirStmtKind::Expr(_)
        | HirStmtKind::InitAssign { .. }
        | HirStmtKind::InlineAsm(_)
        | HirStmtKind::Goto(_)
        | HirStmtKind::GotoComputed(_)
        | HirStmtKind::Break
        | HirStmtKind::Continue
        | HirStmtKind::LocalDecl { .. }
        | HirStmtKind::Null => None,
    }
}

fn expr_references_def(body: &Body, expr: HirExprId, expected: DefId) -> bool {
    match &body.exprs[expr].kind {
        HirExprKind::DefRef(def) => *def == expected,
        HirExprKind::Convert { operand, .. }
        | HirExprKind::Cast { operand, .. }
        | HirExprKind::AddressOf(operand)
        | HirExprKind::Deref(operand)
        | HirExprKind::Unary { operand, .. }
        | HirExprKind::SizeofExpr(operand) => expr_references_def(body, *operand, expected),
        _ => false,
    }
}

fn expr_references_local(body: &Body, expr: HirExprId, expected: Local) -> bool {
    match &body.exprs[expr].kind {
        HirExprKind::LocalRef(local) => *local == expected,
        HirExprKind::Convert { operand, .. }
        | HirExprKind::Cast { operand, .. }
        | HirExprKind::AddressOf(operand)
        | HirExprKind::Deref(operand)
        | HirExprKind::Unary { operand, .. }
        | HirExprKind::SizeofExpr(operand) => expr_references_local(body, *operand, expected),
        _ => false,
    }
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
fn regression_gate_restrict_pointer_parameter_preserves_object_qualifier() {
    let (hir, tcx) = lower_snippet("void f(int * restrict p, int *q) { }");
    let body = hir.bodies.values().next().expect("missing function body");
    let params = body.locals.iter().filter(|local| local.is_param).collect::<Vec<_>>();
    assert_eq!(params.len(), 2);

    assert!(params[0].quals.is_restrict, "`restrict` must qualify the pointer parameter object");
    assert!(!params[1].quals.is_restrict);
    assert!(matches!(tcx.get(params[0].ty), Ty::Ptr(_)));
    assert!(matches!(tcx.get(params[1].ty), Ty::Ptr(_)));
}

#[test]
fn hosted_qualifier_aliases_lower_parameter_object_quals() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    let (hir, tcx, cap) = checked_snippet_with_options(
        "void f(int *__restrict p, int *__const q, int *__volatile r, int a[__restrict_arr static 4]) { }",
        opts,
    );
    assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    let body = hir.bodies.values().next().expect("missing function body");
    let params = body.locals.iter().filter(|local| local.is_param).collect::<Vec<_>>();
    assert_eq!(params.len(), 4);

    assert!(params[0].quals.is_restrict, "__restrict must qualify the pointer parameter object");
    assert!(params[1].quals.is_const, "__const must qualify the pointer parameter object");
    assert!(params[2].quals.is_volatile, "__volatile must qualify the pointer parameter object");
    assert!(
        params[3].quals.is_restrict,
        "__restrict_arr inside [] must qualify the adjusted array parameter pointer"
    );
    match tcx.get(params[3].ty) {
        Ty::Ptr(elem) => {
            assert_eq!(elem.ty, tcx.int);
            assert!(
                !elem.is_restrict,
                "array-parameter restrict belongs to the adjusted pointer, not the element"
            );
        }
        other => panic!("expected adjusted array parameter to be pointer, got {other:?}"),
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
            HirExprKind::IntLiteral { value: 0, .. } | HirExprKind::IntConst(0) => {
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
