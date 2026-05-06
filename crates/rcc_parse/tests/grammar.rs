//! Grammar-production unit tests (task 05-28).
//!
//! One `#[test]` per C99 §6 grammar production (roughly), organised
//! by chapter: §6.4 lexical, §6.5 expressions, §6.7 declarations,
//! §6.8 statements, §6.9 external definitions, plus negative cases.
//!
//! Each test feeds a C snippet through the full pipeline
//! (lex → preprocess → parse) and asserts structural properties on
//! the resulting AST or on emitted diagnostics.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_ast::*;
use rcc_errors::{CaptureEmitter, Diagnostic, Handler};
use rcc_preprocess::preprocess;
use rcc_session::{LanguageStandard, Options, Session};

// ── Helpers ─────────────────────────────────────────────────────────

/// Parse `src` through lex→preprocess→parse, returning the AST and
/// any captured diagnostics.
fn parse_snippet(src: &str) -> (Option<TranslationUnit>, Vec<Diagnostic>, CaptureEmitter) {
    parse_snippet_with_options(src, Options::default())
}

fn parse_snippet_with_options(
    src: &str,
    opts: Options,
) -> (Option<TranslationUnit>, Vec<Diagnostic>, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(opts, handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<test>"), Arc::from(src));
    let pp_tokens = preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens);
    let diags = cap.diagnostics();
    (ast, diags, cap)
}

fn parse_ok_with_session(src: &str, opts: Options) -> (TranslationUnit, Vec<Diagnostic>, Session) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(opts, handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<test>"), Arc::from(src));
    let pp_tokens = preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens);
    let diags = cap.diagnostics();
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(errors.is_empty(), "expected zero errors, got {errors:#?}\nsource: {src}");
    (ast.expect("parse returned None"), diags, sess)
}

/// Parse `src` and assert it produces a valid AST with zero errors.
fn parse_ok(src: &str) -> TranslationUnit {
    let (ast, diags, _) = parse_snippet(src);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(errors.is_empty(), "expected zero errors, got {errors:#?}\nsource: {src}");
    ast.expect("parse returned None")
}

fn parse_ok_with_options(src: &str, opts: Options) -> (TranslationUnit, Vec<Diagnostic>) {
    let (ast, diags, _) = parse_snippet_with_options(src, opts);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(errors.is_empty(), "expected zero errors, got {errors:#?}\nsource: {src}");
    (ast.expect("parse returned None"), diags)
}

/// Parse `src` and assert at least one error diagnostic was emitted.
fn parse_err(src: &str) -> Vec<Diagnostic> {
    let (_, diags, _) = parse_snippet(src);
    let errors: Vec<_> =
        diags.into_iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected at least one error, got none\nsource: {src}");
    errors
}

/// Shorthand: parse succeeds with N top-level declarations.
fn parse_decl_count(src: &str, n: usize) -> TranslationUnit {
    let tu = parse_ok(src);
    assert_eq!(
        tu.decls.len(),
        n,
        "expected {n} external decls, got {}\nsource: {src}",
        tu.decls.len()
    );
    tu
}

// ═══════════════════════════════════════════════════════════════════
// §6.4 — Lexical elements (phase 7 conversions)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn s6_4_integer_constants() {
    // decimal, hex, octal with suffixes
    let tu = parse_ok("int a = 42; int b = 0xFF; int c = 077; int d = 0UL;");
    assert_eq!(tu.decls.len(), 4);
}

#[test]
fn s6_4_float_constants() {
    let tu = parse_ok("double a = 3.14; float b = 1.0f; double c = 1e10; double d = 0x1.0p4;");
    assert_eq!(tu.decls.len(), 4);
}

#[test]
fn s6_4_char_constants() {
    let tu = parse_ok("int a = 'x'; int b = '\\n'; int c = '\\0';");
    assert_eq!(tu.decls.len(), 3);
}

#[test]
fn s6_4_string_literals() {
    let tu = parse_ok("char *s = \"hello\"; char *t = \"wo\" \"rld\";");
    assert_eq!(tu.decls.len(), 2);
}

#[test]
fn s6_4_keywords_as_specifiers() {
    // Various C99 keywords in specifier position.
    parse_ok("static const volatile int x;");
    parse_ok("extern unsigned long long y;");
    parse_ok("register short z;");
    parse_ok("inline void f(void) {}");
    parse_ok("_Bool b;");
}

// ═══════════════════════════════════════════════════════════════════
// §6.5 — Expressions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn s6_5_1_primary_ident() {
    parse_ok("int x; void f(void) { x; }");
}

#[test]
fn s6_5_1_primary_constant() {
    parse_ok("void f(void) { 42; 3.14; 'a'; }");
}

#[test]
fn s6_5_1_primary_string() {
    parse_ok("void f(void) { \"hello\"; }");
}

#[test]
fn s6_5_1_primary_paren() {
    parse_ok("void f(void) { (1 + 2); }");
}

#[test]
fn s6_5_2_postfix_array_subscript() {
    parse_ok("void f(void) { int a[10]; a[0]; }");
}

#[test]
fn s6_5_2_postfix_function_call() {
    parse_ok("int g(int); void f(void) { g(1); }");
}

#[test]
fn s6_5_2_postfix_member_access() {
    parse_ok("struct S { int x; }; void f(void) { struct S s; s.x; }");
}

#[test]
fn s6_5_2_postfix_arrow() {
    parse_ok("struct S { int x; }; void f(void) { struct S *p; p->x; }");
}

#[test]
fn s6_5_2_postfix_increment_decrement() {
    parse_ok("void f(void) { int x; x++; x--; }");
}

#[test]
fn s6_5_3_unary_address_deref() {
    parse_ok("void f(void) { int x; int *p = &x; *p; }");
}

#[test]
fn s6_5_3_unary_neg_not() {
    parse_ok("void f(void) { int x; -x; +x; ~x; !x; }");
}

#[test]
fn s6_5_3_unary_sizeof_expr() {
    parse_ok("void f(void) { int x; sizeof x; }");
}

#[test]
fn s6_5_3_unary_sizeof_type() {
    parse_ok("void f(void) { sizeof(int); sizeof(double *); }");
}

#[test]
fn s6_5_3_unary_pre_inc_dec() {
    parse_ok("void f(void) { int x; ++x; --x; }");
}

#[test]
fn s6_5_4_cast_expression() {
    parse_ok("void f(void) { (int)3.14; (void *)0; }");
}

#[test]
fn s6_5_5_to_s6_5_12_binary_arithmetic() {
    parse_ok("void f(void) { int a; a * 2; a / 2; a % 2; a + 1; a - 1; }");
}

#[test]
fn s6_5_7_shift_operators() {
    parse_ok("void f(void) { int a; a << 1; a >> 1; }");
}

#[test]
fn s6_5_8_relational_operators() {
    parse_ok("void f(void) { int a; a < 1; a > 1; a <= 1; a >= 1; }");
}

#[test]
fn s6_5_9_equality_operators() {
    parse_ok("void f(void) { int a; a == 1; a != 1; }");
}

#[test]
fn s6_5_10_to_s6_5_12_bitwise_operators() {
    parse_ok("void f(void) { int a; a & 1; a ^ 1; a | 1; }");
}

#[test]
fn s6_5_13_logical_and() {
    parse_ok("void f(void) { int a; int b; a && b; }");
}

#[test]
fn s6_5_14_logical_or() {
    parse_ok("void f(void) { int a; int b; a || b; }");
}

#[test]
fn s6_5_15_conditional_ternary() {
    parse_ok("void f(void) { int a; a ? 1 : 0; }");
}

#[test]
fn s6_5_16_assignment_simple() {
    parse_ok("void f(void) { int a; a = 42; }");
}

#[test]
fn s6_5_16_assignment_compound() {
    parse_ok("void f(void) { int a; a += 1; a -= 1; a *= 2; a /= 2; a %= 2; a <<= 1; a >>= 1; a &= 1; a ^= 1; a |= 1; }");
}

#[test]
fn s6_5_17_comma_expression() {
    parse_ok("void f(void) { int a; (a = 1, a + 2); }");
}

#[test]
fn s6_5_2_6_compound_literal() {
    parse_ok("void f(void) { (int){42}; }");
}

#[test]
fn gnu_statement_expression_warns_in_default_mode() {
    let src = "int f(void) { int x = ({ int y = 1; y; }); }";
    let (_tu, diags) = parse_ok_with_options(src, Options::default());
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0013)),
        "expected W0013 for GNU statement expression in strict mode, got {diags:#?}"
    );
}

#[test]
fn gnu_statement_expression_option_suppresses_warning() {
    let src = "int f(void) { int x = ({ int y = 1; y; }); }";
    let opts = Options { gnu_statement_expressions: true, ..Options::default() };
    let (_tu, diags) = parse_ok_with_options(src, opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0013)),
        "gnu option should suppress W0013, got {diags:#?}"
    );
}

#[test]
fn gnu_typeof_expr_warns_in_default_mode() {
    let src = "int f(void); extern typeof(f) f;";
    let (tu, diags) = parse_ok_with_options(src, Options::default());
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0024)),
        "expected W0024 for GNU typeof in strict mode, got {diags:#?}"
    );
    let ExternalDecl::Decl(decl) = &tu.decls[1] else {
        panic!("expected typeof declaration");
    };
    assert!(matches!(decl.specs.type_specs.as_slice(), [TypeSpec::TypeofExpr(_)]));
}

#[test]
fn gnu_typeof_option_suppresses_warning() {
    let src = "int f(void); extern __typeof__(f) f;";
    let opts = Options { gnu_typeof: true, ..Options::default() };
    let (_tu, diags) = parse_ok_with_options(src, opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0024)),
        "gnu option should suppress W0024, got {diags:#?}"
    );
}

#[test]
fn gnu_typeof_type_name_is_preserved() {
    let src = "extern typeof(int (*)(int)) fp;";
    let opts = Options { gnu_typeof: true, ..Options::default() };
    let (tu, _diags) = parse_ok_with_options(src, opts);
    let ExternalDecl::Decl(decl) = &tu.decls[0] else {
        panic!("expected declaration");
    };
    assert!(matches!(decl.specs.type_specs.as_slice(), [TypeSpec::TypeofType(_)]));
}

#[test]
fn gnu_statement_expression_preserves_labels_and_gotos() {
    let src = "void f(void) { int x = ({ label: 1; goto label; }); }";
    let tu = parse_ok(src);
    let ExternalDecl::Function(func) = &tu.decls[0] else {
        panic!("expected function definition");
    };
    let BlockItem::Decl(decl) = &func.body.items[0] else {
        panic!("expected declaration initialized by statement expression");
    };
    let init = decl.inits[0].init.as_ref().expect("initializer");
    let Initializer::Expr(expr) = init else {
        panic!("expected expression initializer");
    };
    let ExprKind::StmtExpr(block) = &expr.kind else {
        panic!("expected statement-expression AST, got {:?}", expr.kind);
    };
    assert_eq!(block.items.len(), 2);
    assert!(
        matches!(
            &block.items[0],
            BlockItem::Stmt(stmt) if matches!(stmt.kind, StmtKind::Label { .. })
        ),
        "first item should preserve label, got {:?}",
        block.items[0]
    );
    assert!(
        matches!(
            &block.items[1],
            BlockItem::Stmt(stmt) if matches!(stmt.kind, StmtKind::Goto(_))
        ),
        "second item should preserve goto, got {:?}",
        block.items[1]
    );
}

#[test]
fn gnu_statement_expression_malformed_reports_error() {
    parse_err("void f(void) { int x = ({ int y = 1; y; ); }");
}

#[test]
fn ctestsuite_00213_reduced_statement_expression_fixture_parses() {
    parse_ok("void f(void) { int i = 1; (1 ? 0 : ({ while (i--) label: i; goto label; })); }");
}

#[test]
fn ctestsuite_00214_reduced_statement_expression_fixture_parses() {
    parse_ok(
        "void f(void) { int __ret = 42; ({ if (__builtin_expect(!!(0), 0)) { int x = !!(__ret); } __ret; }); }",
    );
}

#[test]
fn gnu_attribute_declaration_specifier_warns_in_default_mode() {
    let src = "__attribute__((noreturn)) void f(void);";
    let (tu, diags, sess) = parse_ok_with_session(src, Options::default());
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0015)),
        "expected W0015 for GNU attribute in strict mode, got {diags:#?}"
    );
    let ExternalDecl::Decl(decl) = &tu.decls[0] else {
        panic!("expected declaration");
    };
    assert_attr_name(&sess, &decl.specs.attrs[0], "noreturn");
}

#[test]
fn gnu_attribute_option_suppresses_warning() {
    let src = "__attribute__((noreturn)) void f(void);";
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let (_tu, diags) = parse_ok_with_options(src, opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0015)),
        "gnu option should suppress W0015, got {diags:#?}"
    );
}

#[test]
fn gnu_attribute_declarator_payloads_are_preserved() {
    let src = "int x __attribute__((aligned(16), section(\"text\"), unused));";
    let (tu, _diags, sess) = parse_ok_with_session(src, Options::default());
    let ExternalDecl::Decl(decl) = &tu.decls[0] else {
        panic!("expected declaration");
    };
    let attrs = &decl.inits[0].declarator.attrs;
    assert_eq!(attrs.len(), 3);
    assert_attr_name(&sess, &attrs[0], "aligned");
    assert_eq!(attrs[0].args.len(), 1);
    assert!(matches!(attrs[0].args[0].tokens[0].kind, AttributeTokenKind::Int(16)));
    assert_attr_name(&sess, &attrs[1], "section");
    assert!(
        matches!(&attrs[1].args[0].tokens[0].kind, AttributeTokenKind::String(bytes) if bytes == b"text")
    );
    assert_attr_name(&sess, &attrs[2], "unused");
}

#[test]
fn gnu_glibc_attribute_table_parses_without_unknown_attribute_warning() {
    let cases = [
        "__nothrow__",
        "__leaf__",
        "__nonnull__(1)",
        "pure",
        "const",
        "__malloc__",
        "format(printf, 1, 2)",
        "warn_unused_result",
        "visibility(\"default\")",
        "deprecated(\"use replacement\")",
        "alloc_size(1)",
        "alloc_align(1)",
        "access(read_only, 1)",
        "copy(target)",
    ];
    let opts = Options { gnu_attributes: true, ..Options::default() };
    for attr in cases {
        let src = format!("extern int f(void) __attribute__(({attr}));");
        let (_tu, diags, _sess) = parse_ok_with_session(&src, opts.clone());
        assert!(
            diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0033)),
            "supported attribute {attr:?} emitted W0033: {diags:#?}"
        );
    }
}

#[test]
fn gnu_unsupported_attribute_warns_but_recovers() {
    let opts = Options { gnu_attributes: true, ..Options::default() };
    let src = "int x __attribute__((vendor_only(1))); int y;";
    let (tu, diags, _sess) = parse_ok_with_session(src, opts);

    assert_eq!(tu.decls.len(), 2);
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0033)),
        "expected W0033 for unsupported attribute, got {diags:#?}"
    );
}

#[test]
fn representative_glibc_attribute_declarations_parse_in_hosted_mode() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    let src = r#"
        extern int clock_gettime(int, void *)
          __attribute__((__nothrow__, __leaf__, __nonnull__(2)));
        extern void *malloc(unsigned long)
          __attribute__((__malloc__, __alloc_size__(1), __warn_unused_result__));
    "#;
    let (tu, diags, _sess) = parse_ok_with_session(src, opts);

    assert_eq!(tu.decls.len(), 2);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0033)),
        "glibc attributes should be recognized: {diags:#?}"
    );
}

#[test]
fn gnulib_funcdecl_and_cxxalias_macro_surface_parse_in_hosted_mode() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    let src = r#"
        #define _GL_EXTERN_C extern
        #define _GL_EXTERN_C_FUNC
        #define _GL_FUNCDECL_SYS_NAME(func) (func)
        #define _GL_FUNCDECL_SYS(func,rettype,parameters,...) \
          _GL_EXTERN_C_FUNC __VA_ARGS__ rettype _GL_FUNCDECL_SYS_NAME (func) parameters
        #define _GL_CXXALIAS_SYS(func,rettype,parameters) \
          _GL_EXTERN_C int _gl_cxxalias_dummy
        #define _GL_ARG_NONNULL(params) __attribute__ ((__nonnull__ params))

        typedef long off64_t;
        typedef struct __rcc_FILE FILE;
        typedef __builtin_va_list va_list;

        _GL_FUNCDECL_SYS (vfzprintf, off64_t,
                          (FILE *restrict fp,
                           const char *restrict format, va_list args),
                          _GL_ARG_NONNULL ((1, 2)));
        _GL_CXXALIAS_SYS (vfzprintf, off64_t,
                          (FILE *restrict fp,
                           const char *restrict format, va_list args));
    "#;
    let (tu, diags, _sess) = parse_ok_with_session(src, opts);

    assert_eq!(tu.decls.len(), 5);
    for forbidden in [rcc_errors::codes::W0005, rcc_errors::codes::E0030, rcc_errors::codes::E0063]
    {
        assert!(
            diags.iter().all(|d| d.code != Some(forbidden)),
            "gnulib declaration helper emitted {forbidden}: {diags:#?}"
        );
    }
}

#[test]
fn malformed_parenthesized_function_decl_does_not_poison_following_prototypes() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    let src = r#"
        typedef struct __rcc_FILE FILE;
        typedef __builtin_va_list va_list;
        __attribute__ ((__nonnull__ ((1, 2))))
        off64_t (vfzprintf) (FILE *restrict fp,
                             const char *restrict format, va_list args);
        extern int _gl_cxxalias_dummy;
        extern int isalnum(int);
    "#;
    let (_ast, diags, _cap) = parse_snippet_with_options(src, opts);

    for forbidden in [rcc_errors::codes::W0005, rcc_errors::codes::E0030, rcc_errors::codes::E0063]
    {
        assert!(
            diags.iter().all(|d| d.code != Some(forbidden)),
            "malformed declaration recovery emitted {forbidden}: {diags:#?}"
        );
    }
}

#[test]
fn gnu_extension_inline_header_function_parses_in_hosted_mode() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    let src = r#"
        typedef unsigned long __uint64_t;
        __extension__ static __inline __uint64_t
        __bswap_64(__uint64_t __bsx)
        {
          return __bsx;
        }
    "#;
    let (tu, diags, _sess) = parse_ok_with_session(src, opts);

    assert_eq!(tu.decls.len(), 2);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0034)),
        "hosted mode should suppress W0034 for glibc __extension__: {diags:#?}"
    );
}

#[test]
fn gnu_extension_declaration_prefix_warns_in_strict_c99() {
    let src = r#"
        typedef unsigned long __uint64_t;
        __extension__ static __inline __uint64_t
        __bswap_64(__uint64_t __bsx)
        {
          return __bsx;
        }
    "#;
    let (_tu, diags) = parse_ok_with_options(src, Options::default());

    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0034)),
        "strict C99 should warn W0034 for GNU __extension__, got {diags:#?}"
    );
}

#[test]
fn gnu_qualifier_aliases_are_rejected_in_strict_c99() {
    parse_err("int *__restrict p;");
    parse_err("void f(int a[__const static 4]);");
}

#[test]
fn hosted_qualifier_aliases_parse_in_pointer_and_array_parameters() {
    let opts = Options { linux_gnu_hosted: true, ..Options::default() };
    parse_ok_with_options(
        "void f(char *__restrict p, char *__const q, char *__volatile r, int a[__restrict_arr static 4]);",
        opts,
    );
}

#[test]
fn explicit_gnu_qualifier_alias_flag_parses_without_hosted_mode() {
    let opts = Options { gnu_qualifier_aliases: true, ..Options::default() };
    parse_ok_with_options("void f(char *__restrict__ p, __const int *q);", opts);
}

#[test]
fn gnu_attribute_record_field_enum_function_and_statement_sites_parse() {
    let src = r#"
        struct __attribute__((packed)) S {
            int x __attribute__((aligned(4)));
        };
        enum __attribute__((packed)) E {
            A __attribute__((unused)) = 1
        };
        int f(void) __attribute__((noreturn)) {
            __attribute__((unused));
            return 0;
        }
    "#;
    let (tu, _diags, sess) = parse_ok_with_session(src, Options::default());
    let ExternalDecl::Decl(record_decl) = &tu.decls[0] else {
        panic!("expected record declaration");
    };
    let TypeSpec::Record(record) = &record_decl.specs.type_specs[0] else {
        panic!("expected record spec");
    };
    assert_attr_name(&sess, &record.attrs[0], "packed");
    let field = &record.fields.as_ref().unwrap()[0];
    let field_decl = field.declarators[0].declarator.as_ref().unwrap();
    assert_attr_name(&sess, &field_decl.attrs[0], "aligned");

    let ExternalDecl::Decl(enum_decl) = &tu.decls[1] else {
        panic!("expected enum declaration");
    };
    let TypeSpec::Enum(en) = &enum_decl.specs.type_specs[0] else {
        panic!("expected enum spec");
    };
    assert_attr_name(&sess, &en.attrs[0], "packed");
    assert_attr_name(&sess, &en.enumerators.as_ref().unwrap()[0].attrs[0], "unused");

    let ExternalDecl::Function(func) = &tu.decls[2] else {
        panic!("expected function definition");
    };
    assert_attr_name(&sess, &func.declarator.attrs[0], "noreturn");
    let BlockItem::Stmt(stmt) = &func.body.items[0] else {
        panic!("expected attributed statement");
    };
    assert!(matches!(stmt.kind, StmtKind::Attributed { .. }));
}

#[test]
fn gnu_attribute_type_name_site_parses() {
    parse_ok("void f(void) { sizeof(int __attribute__((aligned(4)))); }");
}

#[test]
fn gnu_attribute_type_name_before_base_specifier_parses() {
    parse_ok(
        r#"
        #define ATTR __attribute__((__noinline__))
        int f(void) { void *p = 0; return ((ATTR int (*)(void)) p)(); }
        "#,
    );
}

#[test]
fn gnu_attribute_inside_abstract_pointer_declarator_parses() {
    parse_ok(
        r#"
        #define ATTR __attribute__((__noinline__))
        int f(void) { void *p = 0; return ((int (ATTR *)(void)) p)(); }
        "#,
    );
}

#[test]
fn gnu_attribute_malformed_parentheses_reports_error() {
    let errs = parse_err("int x __attribute__((aligned(16));");
    assert!(
        errs.iter().any(|d| d.code == Some(rcc_errors::codes::E0031)),
        "expected E0031, got {errs:#?}"
    );
}

fn assert_attr_name(sess: &Session, attr: &Attribute, expected: &str) {
    assert_eq!(sess.interner.get(attr.name), expected);
}

#[test]
fn gnu_inline_asm_basic_warns_in_default_mode() {
    let src = r#"void f(void) { __asm__ volatile ("nop"); }"#;
    let (tu, diags, _sess) = parse_ok_with_session(src, Options::default());
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0016)),
        "expected W0016 for GNU inline asm in strict mode, got {diags:#?}"
    );
    let asm = inline_asm_stmt(&tu, 0);
    assert!(asm.quals.volatile);
    assert_eq!(asm.template.bytes, b"nop");
    assert!(asm.outputs.is_empty());
    assert!(asm.inputs.is_empty());
    assert!(asm.clobbers.is_empty());
}

#[test]
fn gnu_inline_asm_option_suppresses_warning() {
    let src = r#"void f(void) { asm("nop"); }"#;
    let opts = Options { gnu_inline_asm: true, ..Options::default() };
    let (_tu, diags) = parse_ok_with_options(src, opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0016)),
        "gnu option should suppress W0016, got {diags:#?}"
    );
}

#[test]
fn gnu_inline_asm_extended_operands_and_clobbers_are_preserved() {
    let src = r#"
        void f(void) {
            int out;
            int in;
            asm("mov %1, %0" : "=r"(out) : "r"(in) : "cc", "memory");
        }
    "#;
    let (tu, _diags, _sess) = parse_ok_with_session(src, Options::default());
    let asm = inline_asm_stmt(&tu, 2);
    assert_eq!(asm.template.bytes, b"mov %1, %0");
    assert_eq!(asm.outputs.len(), 1);
    assert_eq!(asm.outputs[0].constraint.bytes, b"=r");
    assert!(matches!(asm.outputs[0].expr.kind, ExprKind::Ident(_)));
    assert_eq!(asm.inputs.len(), 1);
    assert_eq!(asm.inputs[0].constraint.bytes, b"r");
    assert_eq!(asm.clobbers.len(), 2);
    assert_eq!(asm.clobbers[0].bytes, b"cc");
    assert_eq!(asm.clobbers[1].bytes, b"memory");
}

#[test]
fn gnu_inline_asm_symbolic_operands_are_preserved() {
    let src = r#"
        void f(void) {
            int out;
            int in;
            __asm__ __volatile__ ("add %1, %0" : [dst] "+r"(out) : [src] "r"(in));
        }
    "#;
    let (tu, _diags, sess) = parse_ok_with_session(src, Options::default());
    let asm = inline_asm_stmt(&tu, 2);
    assert!(asm.quals.volatile);
    let (dst, _) = asm.outputs[0].name.expect("output symbolic name");
    let (src, _) = asm.inputs[0].name.expect("input symbolic name");
    assert_eq!(sess.interner.get(dst), "dst");
    assert_eq!(sess.interner.get(src), "src");
}

#[test]
fn gnu_inline_asm_malformed_operand_reports_error() {
    let errs = parse_err(r#"void f(void) { asm("nop" : "r"x); }"#);
    assert!(
        errs.iter().any(|d| d.code == Some(rcc_errors::codes::E0032)),
        "expected E0032, got {errs:#?}"
    );
}

#[test]
fn ordinary_asm_call_is_not_reclassified_without_string_template() {
    let (tu, diags, _sess) = parse_ok_with_session("void f(void) { asm(x); }", Options::default());
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0016)),
        "ordinary call should not emit inline-asm warning, got {diags:#?}"
    );
    let ExternalDecl::Function(func) = &tu.decls[0] else {
        panic!("expected function");
    };
    let BlockItem::Stmt(stmt) = &func.body.items[0] else {
        panic!("expected statement");
    };
    assert!(matches!(stmt.kind, StmtKind::Expr(Some(_))));
}

fn inline_asm_stmt(tu: &TranslationUnit, idx: usize) -> &InlineAsm {
    let ExternalDecl::Function(func) = &tu.decls[0] else {
        panic!("expected function");
    };
    let BlockItem::Stmt(stmt) = &func.body.items[idx] else {
        panic!("expected statement at body index {idx}");
    };
    let StmtKind::InlineAsm(asm) = &stmt.kind else {
        panic!("expected inline asm statement, got {:?}", stmt.kind);
    };
    asm
}

// ═══════════════════════════════════════════════════════════════════
// §6.7 — Declarations
// ═══════════════════════════════════════════════════════════════════

#[test]
fn s6_7_basic_int_decl() {
    let tu = parse_decl_count("int x;", 1);
    assert!(matches!(tu.decls[0], ExternalDecl::Decl(_)));
}

#[test]
fn s6_7_multiple_declarators() {
    parse_ok("int x, y, z;");
}

#[test]
fn s6_7_with_initializer() {
    parse_ok("int x = 10;");
}

#[test]
fn s6_7_1_storage_class_static() {
    parse_ok("static int x;");
}

#[test]
fn s6_7_1_storage_class_extern() {
    parse_ok("extern int x;");
}

#[test]
fn s6_7_1_storage_class_typedef() {
    parse_ok("typedef int myint;");
}

#[test]
fn s6_7_3_type_qualifiers() {
    parse_ok("const int x = 0;");
    parse_ok("volatile int y;");
    parse_ok("const volatile int z;");
}

#[test]
fn s6_7_4_function_specifier_inline() {
    parse_ok("inline int f(void) { return 0; }");
}

#[test]
fn c11_noreturn_function_specifier_parses_declarations_and_definitions() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (tu, diags) = parse_ok_with_options(
        r#"
        _Noreturn void f(void);
        static _Noreturn void g(void) { for (;;) {} }
        "#,
        opts,
    );
    assert!(diags.is_empty(), "clean C11 parse: {diags:?}");

    let ExternalDecl::Decl(decl) = &tu.decls[0] else {
        panic!("expected prototype declaration");
    };
    assert!(decl.specs.func_specs.noreturn);

    let ExternalDecl::Function(func) = &tu.decls[1] else {
        panic!("expected function definition");
    };
    assert!(func.specs.func_specs.noreturn);
}

#[test]
fn c99_noreturn_function_specifier_is_diagnosed() {
    let errors = parse_err("_Noreturn void f(void);");
    assert!(errors.iter().any(|d| d.message.contains("requires `-std=c11`")), "{errors:#?}");
}

#[test]
fn c11_static_assert_parses_file_block_and_record_scope() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (tu, diags) = parse_ok_with_options(
        r#"
        _Static_assert(1, "file");
        struct S {
            _Static_assert(sizeof(int) == 4, "field");
            int x;
        };
        void f(void) {
            _Static_assert(1, "block");
        }
        "#,
        opts,
    );
    assert!(diags.is_empty(), "clean C11 parse: {diags:?}");
    assert!(matches!(tu.decls[0], ExternalDecl::StaticAssert(_)));

    let ExternalDecl::Decl(record_decl) = &tu.decls[1] else {
        panic!("expected record declaration");
    };
    let TypeSpec::Record(record) = &record_decl.specs.type_specs[0] else {
        panic!("expected record spec");
    };
    assert_eq!(record.static_asserts.len(), 1);
    assert_eq!(record.fields.as_ref().expect("record fields").len(), 1);

    let ExternalDecl::Function(func) = &tu.decls[2] else {
        panic!("expected function definition");
    };
    assert!(matches!(func.body.items[0], BlockItem::StaticAssert(_)));
}

#[test]
fn c99_static_assert_declaration_is_diagnosed() {
    let errors = parse_err(r#"_Static_assert(1, "needs c11");"#);
    assert!(errors.iter().any(|d| d.message.contains("requires `-std=c11`")), "{errors:#?}");
}

#[test]
fn c11_alignof_and_alignas_parse() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (tu, diags) = parse_ok_with_options(
        r#"
        int a[_Alignof(int)];
        _Alignas(16) int x;
        _Alignas(long double) int y;
        "#,
        opts,
    );
    assert!(diags.is_empty(), "clean C11 parse: {diags:?}");

    let ExternalDecl::Decl(array_decl) = &tu.decls[0] else {
        panic!("expected array declaration");
    };
    let DerivedDeclarator::Array(array) = &array_decl.inits[0].declarator.derived[0] else {
        panic!("expected array declarator");
    };
    match &array.size.as_ref().expect("array size").kind {
        ExprKind::AlignofType(ty) => {
            assert!(matches!(ty.specs.type_specs.as_slice(), [TypeSpec::Int]));
        }
        other => panic!("expected AlignofType, got {other:?}"),
    }

    let ExternalDecl::Decl(x_decl) = &tu.decls[1] else {
        panic!("expected aligned object declaration");
    };
    assert_eq!(x_decl.specs.align_specs.len(), 1);
    assert!(matches!(x_decl.specs.align_specs[0].kind, AlignSpecKind::Expr(_)));

    let ExternalDecl::Decl(y_decl) = &tu.decls[2] else {
        panic!("expected type aligned object declaration");
    };
    assert_eq!(y_decl.specs.align_specs.len(), 1);
    assert!(matches!(y_decl.specs.align_specs[0].kind, AlignSpecKind::Type(_)));
}

#[test]
fn c99_alignof_and_alignas_are_diagnosed() {
    let errors = parse_err("_Alignas(16) int x; int y[_Alignof(int)];");
    assert!(errors.iter().any(|d| d.message.contains("`_Alignas`")), "{errors:#?}");
    assert!(errors.iter().any(|d| d.message.contains("`_Alignof`")), "{errors:#?}");
}

#[test]
fn c11_generic_selection_parses_associations() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (tu, diags) = parse_ok_with_options(
        "int f(void) { return _Generic(1, int: 10, long: 11, default: 20); }",
        opts,
    );
    assert!(diags.is_empty(), "clean C11 parse: {diags:?}");
    let ExternalDecl::Function(func) = &tu.decls[0] else {
        panic!("expected function definition");
    };
    let BlockItem::Stmt(stmt) = &func.body.items[0] else {
        panic!("expected return statement");
    };
    let StmtKind::Return(Some(expr)) = &stmt.kind else {
        panic!("expected return expression");
    };
    let ExprKind::GenericSelection { control, associations } = &expr.kind else {
        panic!("expected GenericSelection, got {:?}", expr.kind);
    };
    assert!(matches!(control.kind, ExprKind::IntLit(_)));
    assert_eq!(associations.len(), 3);
    assert!(associations[0].ty.is_some());
    assert!(associations[1].ty.is_some());
    assert!(associations[2].ty.is_none());
}

#[test]
fn c99_generic_selection_is_diagnosed() {
    let errors = parse_err("int f(void) { return _Generic(1, int: 10, default: 20); }");
    assert!(errors.iter().any(|d| d.message.contains("`_Generic`")), "{errors:#?}");
}

#[test]
fn c11_atomic_type_specifier_and_qualifier_parse() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (tu, _) = parse_ok_with_options("_Atomic(int) x; _Atomic int y; int * _Atomic p;", opts);
    assert_eq!(tu.decls.len(), 3);

    let ExternalDecl::Decl(first) = &tu.decls[0] else { panic!("expected declaration") };
    assert!(matches!(first.specs.type_specs.as_slice(), [TypeSpec::Atomic(_)]));

    let ExternalDecl::Decl(second) = &tu.decls[1] else { panic!("expected declaration") };
    assert!(second.specs.quals.atomic);
    assert!(matches!(second.specs.type_specs.as_slice(), [TypeSpec::Int]));

    let ExternalDecl::Decl(third) = &tu.decls[2] else { panic!("expected declaration") };
    assert!(matches!(
        third.inits[0].declarator.derived.as_slice(),
        [DerivedDeclarator::Pointer(q)] if q.atomic
    ));
}

#[test]
fn c11_atomic_requires_c11_mode() {
    let errors = parse_err("_Atomic(int) x; _Atomic int y;");
    assert!(
        errors.iter().any(|d| d.message.contains("`_Atomic")),
        "expected _Atomic C11 diagnostic, got {errors:#?}"
    );
}

#[test]
fn c11_anonymous_record_member_is_standard() {
    let opts = Options { language_standard: LanguageStandard::C11, ..Options::default() };
    let (_tu, diags) =
        parse_ok_with_options("struct S { union { int x; long y; }; int tail; };", opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0035)),
        "C11 anonymous record member should not warn: {diags:#?}"
    );
}

#[test]
fn c99_anonymous_record_member_warns_as_extension() {
    let (ast, diags, _) = parse_snippet("struct S { union { int x; long y; }; int tail; };");
    assert!(ast.is_some(), "extension should still parse");
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0035)),
        "expected W0035 in C99 mode, got {diags:#?}"
    );
}

#[test]
fn s6_7_5_pointer_declarator() {
    parse_ok("int *p;");
    parse_ok("const int *p;");
    parse_ok("int *const p;");
    parse_ok("int **pp;");
}

#[test]
fn s6_7_5_array_declarator() {
    parse_ok("int a[10];");
    parse_ok("int a[];");
}

#[test]
fn s6_7_5_function_declarator() {
    parse_ok("int f(int a, int b);");
    parse_ok("int f(void);");
    parse_ok("int f(int, char *);");
}

#[test]
fn s6_7_5_function_pointer_declarator() {
    parse_ok("int (*fp)(int, int);");
}

#[test]
fn s6_7_5_array_of_pointers() {
    parse_ok("int *a[10];");
}

#[test]
fn s6_7_5_pointer_to_array() {
    parse_ok("int (*a)[10];");
}

#[test]
fn s6_7_7_typedef_and_usage() {
    parse_ok("typedef int myint; myint x;");
}

#[test]
fn s6_7_7_typedef_pointer() {
    parse_ok("typedef int *intptr; intptr p;");
}

#[test]
fn s6_7_2_1_struct_basic() {
    parse_ok("struct S { int x; int y; };");
}

#[test]
fn s6_7_2_1_struct_with_variable() {
    parse_ok("struct S { int x; } s;");
}

#[test]
fn s6_7_2_1_union_basic() {
    parse_ok("union U { int i; float f; };");
}

#[test]
fn s6_7_2_1_struct_nested() {
    parse_ok("struct Outer { struct Inner { int x; } inner; int y; };");
}

#[test]
fn s6_7_2_1_struct_forward_decl() {
    parse_ok("struct S; struct S *p;");
}

#[test]
fn s6_7_2_2_enum_basic() {
    parse_ok("enum Color { RED, GREEN, BLUE };");
}

#[test]
fn s6_7_2_2_enum_with_values() {
    parse_ok("enum E { A = 0, B = 1, C = 100 };");
}

#[test]
fn s6_7_2_2_enum_trailing_comma() {
    // C99 allows trailing comma in enumerator list.
    parse_ok("enum E { A, B, C, };");
}

#[test]
fn s6_7_8_initializer_simple() {
    parse_ok("int x = 42;");
}

#[test]
fn s6_7_8_initializer_brace() {
    parse_ok("int a[3] = {1, 2, 3};");
}

#[test]
fn s6_7_8_initializer_nested() {
    parse_ok("int a[2][2] = {{1, 2}, {3, 4}};");
}

#[test]
fn s6_7_8_designated_initializer_field() {
    parse_ok("struct S { int x; int y; }; struct S s = { .x = 1, .y = 2 };");
}

#[test]
fn s6_7_8_designated_initializer_index() {
    parse_ok("int a[10] = { [0] = 1, [9] = 10 };");
}

#[test]
fn s6_7_8_trailing_comma_in_init_list() {
    // C99 §6.7.8 permits a trailing comma after the last element.
    parse_ok("int a[3] = {1, 2, 3,};");
}

#[test]
fn s6_7_8_mixed_positional_and_designated() {
    parse_ok("struct S { int a; int b; int c; }; struct S s = { 1, .b = 2, 3 };");
}

#[test]
fn s6_7_8_chained_designator() {
    // Nested designator chain: .field[index] = value
    parse_ok("struct { int x[2]; } s = { .x[1] = 42 };");
}

#[test]
fn gnu_range_designator_warns_in_default_mode() {
    let src = "int a[8] = { [1 ... 5] = 9 };";
    let (_tu, diags) = parse_ok_with_options(src, Options::default());
    assert!(
        diags.iter().any(|d| d.code == Some(rcc_errors::codes::W0014)),
        "expected W0014 for GNU range designator in strict mode, got {diags:#?}"
    );
}

#[test]
fn gnu_range_designator_option_suppresses_warning() {
    let src = "int a[8] = { [1 ... 5] = 9 };";
    let opts = Options { gnu_range_designators: true, ..Options::default() };
    let (_tu, diags) = parse_ok_with_options(src, opts);
    assert!(
        diags.iter().all(|d| d.code != Some(rcc_errors::codes::W0014)),
        "gnu option should suppress W0014, got {diags:#?}"
    );
}

#[test]
fn ctestsuite_00216_reduced_range_initializer_fixture_parses() {
    parse_ok(
        "struct T { unsigned char s[16]; unsigned char a; }; \
         void f(void) { int elt = 0x42; \
         struct T lt2 = { { [1 ... 5] = 9, [6 ... 10] = elt, [4 ... 7] = elt + 1 }, 1 }; }",
    );
}

#[test]
fn s6_7_8_deeply_nested_init() {
    parse_ok("int a[2][2][2] = {{{1,2},{3,4}},{{5,6},{7,8}}};");
}

#[test]
fn s6_7_8_missing_comma_in_init_list() {
    // Missing comma between elements — should produce an error but recover.
    let errs = parse_err("int a[2] = {1 2};");
    assert!(!errs.is_empty(), "should detect missing comma or unexpected token");
}

#[test]
fn s6_7_8_missing_rbracket_in_designator() {
    // [expr without closing ] — should produce error diagnostic.
    let errs = parse_err("int a[2] = { [0 = 1 };");
    assert!(!errs.is_empty(), "should detect missing `]`");
}

#[test]
fn s6_7_8_designator_dot_no_ident() {
    // `.` not followed by an identifier — diagnostic expected.
    let errs = parse_err("struct S { int x; }; struct S s = { . = 1 };");
    assert!(!errs.is_empty(), "should detect missing ident after `.`");
}

#[test]
fn s6_7_8_empty_brace_init() {
    // Empty {} — C99 forbids it, should emit diagnostic.
    let (ast, diags, _) = parse_snippet("int x = {};");
    assert!(
        diags.iter().any(|d| d.message.contains("empty initializer")),
        "should warn about empty init: {diags:?}"
    );
    // Parser should still produce an AST (recovery)
    assert!(ast.is_some());
}

// ═══════════════════════════════════════════════════════════════════
// §6.8 — Statements and blocks
// ═══════════════════════════════════════════════════════════════════

#[test]
fn s6_8_1_labeled_statement() {
    parse_ok("void f(void) { label: ; }");
}

#[test]
fn s6_8_2_compound_statement() {
    parse_ok("void f(void) { { int x; x = 1; } }");
}

#[test]
fn s6_8_3_expression_statement() {
    parse_ok("void f(void) { 42; }");
}

#[test]
fn s6_8_3_null_statement() {
    parse_ok("void f(void) { ; }");
}

#[test]
fn s6_8_4_1_if_statement() {
    parse_ok("void f(void) { if (1) ; }");
}

#[test]
fn s6_8_4_1_if_else_statement() {
    parse_ok("void f(void) { if (1) ; else ; }");
}

#[test]
fn s6_8_4_1_if_else_chain() {
    parse_ok("void f(void) { if (1) ; else if (0) ; else ; }");
}

#[test]
fn s6_8_5_1_while_statement() {
    parse_ok("void f(void) { while (1) ; }");
}

#[test]
fn s6_8_5_2_do_while_statement() {
    parse_ok("void f(void) { do ; while (1); }");
}

#[test]
fn s6_8_5_3_for_statement() {
    parse_ok("void f(void) { int i; for (i = 0; i < 10; i++) ; }");
}

#[test]
fn s6_8_5_3_for_statement_declaration_init() {
    parse_ok("void f(void) { for (int i = 0; i < 10; ++i) ; }");
}

#[test]
fn s6_8_5_3_for_statement_empty_clauses() {
    parse_ok("void f(void) { for (;;) ; }");
}

#[test]
fn s6_8_4_2_switch_case_default() {
    parse_ok("void f(void) { switch (1) { case 0: ; break; case 1: ; break; default: ; } }");
}

#[test]
fn s6_8_6_1_goto_statement() {
    parse_ok("void f(void) { goto end; end: ; }");
}

#[test]
fn s6_8_6_2_continue_statement() {
    parse_ok("void f(void) { while (1) continue; }");
}

#[test]
fn s6_8_6_3_break_statement() {
    parse_ok("void f(void) { while (1) break; }");
}

#[test]
fn s6_8_6_4_return_void() {
    parse_ok("void f(void) { return; }");
}

#[test]
fn s6_8_6_4_return_value() {
    parse_ok("int f(void) { return 42; }");
}

// ═══════════════════════════════════════════════════════════════════
// §6.9 — External definitions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn s6_9_1_function_definition() {
    let tu = parse_ok("int add(int a, int b) { return a + b; }");
    assert_eq!(tu.decls.len(), 1);
    assert!(matches!(tu.decls[0], ExternalDecl::Function(_)));
}

#[test]
fn s6_9_1_function_void_params() {
    parse_ok("void f(void) { }");
}

#[test]
fn s6_9_1_function_no_params() {
    parse_ok("void f() { }");
}

#[test]
fn s6_9_1_kr_function_definition() {
    parse_ok("int add(a, b) int a; int b; { return a + b; }");
}

#[test]
fn s6_9_global_variable() {
    let tu = parse_decl_count("int g;", 1);
    assert!(matches!(tu.decls[0], ExternalDecl::Decl(_)));
}

#[test]
fn s6_9_global_variable_with_init() {
    parse_ok("int g = 100;");
}

#[test]
fn s6_9_multiple_external_declarations() {
    let tu = parse_ok("int x; int y; int z;");
    assert_eq!(tu.decls.len(), 3);
}

#[test]
fn s6_9_mixed_functions_and_decls() {
    let tu = parse_ok("int g; void f(void) { } int h; int main(void) { return 0; }");
    assert_eq!(tu.decls.len(), 4);
    assert!(matches!(tu.decls[0], ExternalDecl::Decl(_)));
    assert!(matches!(tu.decls[1], ExternalDecl::Function(_)));
    assert!(matches!(tu.decls[2], ExternalDecl::Decl(_)));
    assert!(matches!(tu.decls[3], ExternalDecl::Function(_)));
}

#[test]
fn s6_9_variadic_function() {
    parse_ok("int printf(const char *fmt, ...);");
}

#[test]
fn s6_9_function_returning_pointer() {
    parse_ok("int *f(void) { return 0; }");
}

// ═══════════════════════════════════════════════════════════════════
// Negative cases — error diagnostics
// ═══════════════════════════════════════════════════════════════════

#[test]
fn neg_missing_semicolon() {
    let errs = parse_err("int x\nint y;");
    assert!(!errs.is_empty());
}

#[test]
fn neg_unexpected_token_in_expr() {
    let errs = parse_err("void f(void) { int x = ; }");
    assert!(!errs.is_empty());
}

#[test]
fn neg_missing_closing_brace() {
    let errs = parse_err("void f(void) { int x;");
    assert!(!errs.is_empty());
}

#[test]
fn neg_missing_closing_paren() {
    let errs = parse_err("void f(void) { (1 + 2; }");
    assert!(!errs.is_empty());
}

#[test]
fn neg_invalid_cast() {
    // Extra tokens in a type-name context.
    let errs = parse_err("void f(void) { (int int)1; }");
    assert!(!errs.is_empty());
}

#[test]
fn neg_type_name_rejects_storage_class_contexts() {
    let sizeof_errs = parse_err("void f(void) { sizeof(static int); }");
    assert!(!sizeof_errs.is_empty());

    let cast_errs = parse_err("void f(void) { (typedef int)x; }");
    assert!(!cast_errs.is_empty());
}

#[test]
fn neg_type_name_requires_type_specifier() {
    let errs = parse_err("void f(void) { sizeof(const); }");
    assert!(
        errs.iter().any(|d| d.code == Some("E0061")),
        "expected strict type-name E0061, got {errs:#?}"
    );
}

#[test]
fn neg_recovery_still_parses_rest() {
    // After a bad declaration, the next valid one should still parse.
    let (ast, diags, _) = parse_snippet("int x\nint y;");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected an error for missing semicolon");
    let tu = ast.expect("parse should still return Some");
    // At least `int y;` should have been parsed.
    assert!(!tu.decls.is_empty(), "after recovery, at least one decl should survive");
}

#[test]
fn recovery_file_scope_bad_declaration_keeps_following_decl() {
    let (ast, diags, _) = parse_snippet("int *; int y;");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected an error for missing declarator");
    let tu = ast.expect("parse still returns a translation unit");
    assert_eq!(tu.decls.len(), 1, "only the valid following declaration should survive");
    assert!(matches!(tu.decls[0], ExternalDecl::Decl(_)));
}

#[test]
fn recovery_block_bad_declaration_keeps_following_decl() {
    let (ast, diags, _) = parse_snippet("void f(void) { int *; int y; y = 1; }");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected an error for missing block declarator");
    let tu = ast.expect("parse still returns a translation unit");
    let ExternalDecl::Function(f) = &tu.decls[0] else {
        panic!("expected function definition");
    };
    let decls = f.body.items.iter().filter(|item| matches!(item, BlockItem::Decl(_))).count();
    assert_eq!(decls, 1, "the valid `int y;` declaration should survive");
}

#[test]
fn recovery_for_bad_init_does_not_leak_scope() {
    let src = "void f(void) { typedef int T; for (int T = ; ; ) ; T x; }";
    let (ast, diags, _) = parse_snippet(src);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected an initializer error");
    let tu = ast.expect("parse still returns a translation unit");
    let ExternalDecl::Function(f) = &tu.decls[0] else {
        panic!("expected function definition");
    };
    let decls = f.body.items.iter().filter(|item| matches!(item, BlockItem::Decl(_))).count();
    assert_eq!(
        decls, 2,
        "outer typedef and following `T x;` should both be declarations; \
         the `for` init's ordinary `T` must not leak"
    );
}

#[test]
fn recovery_parameter_list_keeps_following_external_decl() {
    let (ast, diags, _) = parse_snippet("int f(int x, int [; int g;");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected a malformed parameter diagnostic");
    let tu = ast.expect("parse still returns a translation unit");
    assert_eq!(tu.decls.len(), 2, "malformed parameter list must not hide `int g;`");
}

#[test]
fn recovery_kr_decl_list_attempts_function_body() {
    let (ast, diags, _) = parse_snippet("int f(a) int *; { return 1; } int g;");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected a malformed K&R parameter declaration diagnostic");
    let tu = ast.expect("parse still returns a translation unit");
    assert_eq!(tu.decls.len(), 2, "bad K&R decl list must not discard the function body");
    assert!(matches!(tu.decls[0], ExternalDecl::Function(_)));
    assert!(matches!(tu.decls[1], ExternalDecl::Decl(_)));
}

// ═══════════════════════════════════════════════════════════════════
// Additional coverage — complex productions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn complex_function_pointer_param() {
    parse_ok("void qsort(void *base, int n, int size, int (*cmp)(const void *, const void *));");
}

#[test]
fn complex_nested_struct() {
    parse_ok("struct A { struct B { int x; } b; int y; }; struct A a;");
}

#[test]
fn complex_typedef_function_pointer() {
    parse_ok("typedef int (*handler)(int, int); handler h;");
}

#[test]
fn complex_array_of_function_pointers() {
    parse_ok("int (*ftable[10])(int);");
}

#[test]
fn complex_multi_level_pointer() {
    parse_ok("int ***p;");
}

#[test]
fn complex_cast_to_void_pointer() {
    parse_ok("void f(void) { void *p = (void *)0; }");
}

#[test]
fn complex_for_with_decl_init() {
    parse_ok("void f(void) { int i; for (i = 0; i < 10; i = i + 1) { } }");
}

#[test]
fn complex_nested_blocks() {
    parse_ok("void f(void) { { { { int x; } } } }");
}

#[test]
fn complex_sizeof_complex_type() {
    parse_ok("void f(void) { sizeof(int *); sizeof(struct { int x; }); }");
}

#[test]
fn complex_compound_literal_struct() {
    parse_ok("struct P { int x; int y; }; void f(void) { (struct P){1, 2}; }");
}

#[test]
fn complex_enum_usage_in_switch() {
    parse_ok(
        "enum E { A, B, C }; void f(enum E e) { switch (e) { case A: break; case B: break; default: break; } }",
    );
}

#[test]
fn complex_chained_member_access() {
    parse_ok(
        "struct Inner { int val; }; struct Outer { struct Inner in; }; void f(void) { struct Outer o; o.in.val; }",
    );
}

#[test]
fn complex_ternary_nested() {
    parse_ok("void f(void) { int a; int b; int c; a ? b ? 1 : 2 : c; }");
}

#[test]
fn complex_string_adjacent_concat() {
    parse_ok("char *s = \"hello\" \" \" \"world\";");
}
