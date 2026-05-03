#![cfg(feature = "llvm")]

//! FileCheck-lite semantic LLVM IR tests.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::build_bodies;
use rcc_codegen_llvm::codegen;
use rcc_errors::{CaptureEmitter, Handler};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_session::{Options, Session};
use rcc_typeck::{check, verify_typed_hir};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Directive {
    Check(String),
    CheckNot(String),
}

fn assert_checked(name: &str, checked_source: &str) {
    let (source, directives) = parse_checked_source(checked_source);
    let ir = render(name, &source);
    filecheck(&ir, &directives).unwrap_or_else(|message| panic!("{message}\n\nIR:\n{ir}"));
}

fn parse_checked_source(checked_source: &str) -> (String, Vec<Directive>) {
    let mut source = String::new();
    let mut directives = Vec::new();

    for line in checked_source.replace("\r\n", "\n").lines() {
        let trimmed = line.trim_start();
        if let Some(pattern) = trimmed.strip_prefix("// CHECK-NOT:") {
            directives.push(Directive::CheckNot(pattern.trim_start().to_owned()));
            continue;
        }
        if let Some(pattern) = trimmed.strip_prefix("// CHECK:") {
            directives.push(Directive::Check(pattern.trim_start().to_owned()));
            continue;
        }

        source.push_str(line);
        source.push('\n');
    }

    (source, directives)
}

fn filecheck(ir: &str, directives: &[Directive]) -> Result<(), String> {
    let mut cursor = 0;
    let mut pending_not: Vec<&str> = Vec::new();

    for (idx, directive) in directives.iter().enumerate() {
        match directive {
            Directive::Check(pattern) => {
                let Some(offset) = ir[cursor..].find(pattern) else {
                    return Err(format!(
                        "CHECK #{idx} not found after byte {cursor}: `{pattern}`\n{}",
                        excerpt(ir, cursor)
                    ));
                };

                let match_start = cursor + offset;
                let segment = &ir[cursor..match_start];
                verify_not_patterns(idx, segment, &pending_not)?;
                pending_not.clear();
                cursor = match_start + pattern.len();
            }
            Directive::CheckNot(pattern) => pending_not.push(pattern),
        }
    }

    verify_not_patterns(directives.len(), &ir[cursor..], &pending_not)
}

fn verify_not_patterns(idx: usize, segment: &str, patterns: &[&str]) -> Result<(), String> {
    for pattern in patterns {
        if segment.contains(pattern) {
            return Err(format!(
                "CHECK-NOT before directive #{idx} unexpectedly matched `{pattern}`\n{}",
                excerpt(segment, 0)
            ));
        }
    }
    Ok(())
}

fn excerpt(ir: &str, cursor: usize) -> String {
    let start = cursor.saturating_sub(120);
    let end = (cursor + 600).min(ir.len());
    format!("IR excerpt at byte {cursor}:\n{}", &ir[start..end])
}

fn render(name: &str, src: &str) -> String {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session
        .source_map
        .write()
        .unwrap()
        .add_file(PathBuf::from(format!("<llvm-filecheck/{name}>")), Arc::from(src));

    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);
    let ast = rcc_parse::parse(&mut session, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut session);
    check(&mut session, &mut tcx, &mut hir);
    verify_typed_hir(&mut session, &tcx, &hir);
    assert!(!session.handler.has_errors(), "{name}: diagnostics: {:?}", cap.diagnostics());

    let bodies = build_bodies(&mut session, &tcx, &hir);
    let artifact = codegen(&mut session, &tcx, &hir, &bodies).expect("LLVM codegen failed");
    normalize_ir(&artifact.ir_text)
}

fn normalize_ir(ir: &str) -> String {
    let mut out = String::new();
    for line in ir.replace("\r\n", "\n").lines() {
        let line = line.trim_end();
        if line.starts_with("target datalayout = ") {
            out.push_str("target datalayout = \"<normalized>\"");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[test]
fn filecheck_reports_missing_expected_instruction() {
    let err = filecheck(
        "define i32 @f() {\n  ret i32 0\n}\n",
        &[Directive::Check("store i32".to_owned())],
    )
    .unwrap_err();
    assert!(err.contains("not found"));
    assert!(err.contains("store i32"));
    assert!(err.contains("IR excerpt"));
}

#[test]
fn filecheck_reports_negative_match() {
    let directives = [
        Directive::Check("define i32 @f()".to_owned()),
        Directive::CheckNot("load volatile".to_owned()),
        Directive::Check("ret i32 0".to_owned()),
    ];
    let err =
        filecheck("define i32 @f() {\n  load volatile i32, ptr @x\n  ret i32 0\n}\n", &directives)
            .unwrap_err();
    assert!(err.contains("CHECK-NOT"));
    assert!(err.contains("load volatile"));
}

#[test]
fn sret_return_is_explicit_in_function_signature() {
    assert_checked(
        "sret_return",
        r#"
        // CHECK: %rcc.record.0 = type { i64, i64, i64 }
        // CHECK: define void @make(ptr sret(%rcc.record.0)
        // CHECK: call void @llvm.memcpy.p0.p0.i64
        struct Big { long a; long b; long c; };
        struct Big make(void) {
            struct Big out = {1, 2, 3};
            return out;
        }
        "#,
    );
}

#[test]
fn static_linkage_is_internal_for_globals_and_functions() {
    assert_checked(
        "internal_linkage",
        r#"
        // CHECK: @x = internal global i32 5
        // CHECK: define internal i32 @f()
        // CHECK: define i32 @g()
        // CHECK: call i32 @f()
        static int x = 5;
        static int f(void) { return x; }
        int g(void) { return f(); }
        "#,
    );
}

#[test]
fn aggregate_assignment_uses_mem_intrinsics() {
    assert_checked(
        "aggregate_mem_intrinsics",
        r#"
        // CHECK: call void @llvm.memset.p0.i64
        // CHECK: call void @llvm.memcpy.p0.p0.i64
        // CHECK: declare void @llvm.memset.p0.i64
        // CHECK: declare void @llvm.memcpy.p0.p0.i64
        struct Pair { int a; int b; };
        int f(void) {
            struct Pair a = {1, 2};
            struct Pair b;
            b = a;
            return b.b;
        }
        "#,
    );
}

#[test]
fn volatile_loads_and_stores_survive_ir_emission() {
    assert_checked(
        "volatile_ops",
        r#"
        // CHECK: define i32 @f()
        // CHECK: store volatile i32 7
        // CHECK: load volatile i32
        // CHECK-NOT: call void @llvm.memcpy
        int f(void) {
            volatile int x;
            x = 7;
            return x;
        }
        "#,
    );
}

#[test]
fn bitfield_write_masks_neighbor_bits() {
    assert_checked(
        "bitfield_masks",
        r#"
        // CHECK: define i32 @f(ptr
        // CHECK: load i32
        // CHECK: and i32
        // CHECK: shl i32
        // CHECK: or i32
        // CHECK: store i32
        // CHECK: lshr i32
        struct S { unsigned a:3; unsigned b:5; };
        int f(struct S *p) {
            p->b = 17;
            return p->b;
        }
        "#,
    );
}
