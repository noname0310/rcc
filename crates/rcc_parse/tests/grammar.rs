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
use rcc_session::{Options, Session};

// ── Helpers ─────────────────────────────────────────────────────────

/// Parse `src` through lex→preprocess→parse, returning the AST and
/// any captured diagnostics.
fn parse_snippet(src: &str) -> (Option<TranslationUnit>, Vec<Diagnostic>, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut sess = Session::with_handler(Options::default(), handler);
    let fid = sess.source_map.write().unwrap().add_file(PathBuf::from("<test>"), Arc::from(src));
    let pp_tokens = preprocess(&mut sess, fid);
    let ast = rcc_parse::parse(&mut sess, pp_tokens);
    let diags = cap.diagnostics();
    (ast, diags, cap)
}

/// Parse `src` and assert it produces a valid AST with zero errors.
fn parse_ok(src: &str) -> TranslationUnit {
    let (ast, diags, _) = parse_snippet(src);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(errors.is_empty(), "expected zero errors, got {errors:#?}\nsource: {src}");
    ast.expect("parse returned None")
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
fn neg_recovery_still_parses_rest() {
    // After a bad declaration, the next valid one should still parse.
    let (ast, diags, _) = parse_snippet("int x\nint y;");
    let errors: Vec<_> = diags.iter().filter(|d| d.level == rcc_errors::Level::Error).collect();
    assert!(!errors.is_empty(), "expected an error for missing semicolon");
    let tu = ast.expect("parse should still return Some");
    // At least `int y;` should have been parsed.
    assert!(!tu.decls.is_empty(), "after recovery, at least one decl should survive");
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
