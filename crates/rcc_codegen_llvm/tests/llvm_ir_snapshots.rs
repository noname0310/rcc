#![cfg(feature = "llvm")]

//! Golden snapshots for source-to-LLVM IR codegen.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::build_bodies;
use rcc_codegen_llvm::codegen;
use rcc_errors::{CaptureEmitter, Handler};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_session::{Options, Session};
use rcc_typeck::{check, verify_typed_hir};

fn render(name: &str, src: &str) -> String {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session
        .source_map
        .write()
        .unwrap()
        .add_file(PathBuf::from(format!("<llvm-ir/{name}>")), Arc::from(src));

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

macro_rules! snap {
    ($name:literal, $src:expr, @$snapshot:literal) => {
        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(render($name, $src), @$snapshot);
        });
    };
}

#[test]
fn function_return() {
    snap!("function_return", "int f(void) { return 42; }", @r###"
    ; ModuleID = '<llvm-ir/function_return>'
    source_filename = "<llvm-ir/function_return>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    define i32 @f() {
    entry:
      %ret.addr = alloca i32, align 4
      store i32 42, ptr %ret.addr, align 4
      %load = load i32, ptr %ret.addr, align 4
      ret i32 %load
    }
    "###);
}

#[test]
fn branch_if_else() {
    snap!("branch_if_else", "int f(int x) { if (x) return 1; else return 2; }", @r###"
    ; ModuleID = '<llvm-ir/branch_if_else>'
    source_filename = "<llvm-ir/branch_if_else>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    define i32 @f(i32 %0) {
    entry:
      %ret.addr = alloca i32, align 4
      %param10.addr = alloca i32, align 4
      %param.unit = getelementptr i8, ptr %param10.addr, i64 0
      store i32 %0, ptr %param.unit, align 4
      %load = load i32, ptr %param10.addr, align 4
      switch i32 %load, label %bb1 [
        i32 0, label %bb2
      ]

    bb1:                                              ; preds = %entry
      store i32 1, ptr %ret.addr, align 4
      %load1 = load i32, ptr %ret.addr, align 4
      ret i32 %load1

    bb2:                                              ; preds = %entry
      store i32 2, ptr %ret.addr, align 4
      %load2 = load i32, ptr %ret.addr, align 4
      ret i32 %load2
    }
    "###);
}

#[test]
fn direct_call() {
    snap!(
        "direct_call",
        "int callee(int x) { return x; } int f(void) { return callee(7); }",
        @r###"
    ; ModuleID = '<llvm-ir/direct_call>'
    source_filename = "<llvm-ir/direct_call>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    define i32 @callee(i32 %0) {
    entry:
      %ret.addr = alloca i32, align 4
      %param10.addr = alloca i32, align 4
      %param.unit = getelementptr i8, ptr %param10.addr, i64 0
      store i32 %0, ptr %param.unit, align 4
      %load = load i32, ptr %param10.addr, align 4
      store i32 %load, ptr %ret.addr, align 4
      %load1 = load i32, ptr %ret.addr, align 4
      ret i32 %load1
    }

    define i32 @f() {
    entry:
      %ret.addr = alloca i32, align 4
      %tmp1.addr = alloca i32, align 4
      %call = call i32 @callee(i32 7)
      %param.unit = getelementptr i8, ptr %tmp1.addr, i64 0
      store i32 %call, ptr %param.unit, align 4
      br label %bb1

    bb1:                                              ; preds = %entry
      %load = load i32, ptr %tmp1.addr, align 4
      store i32 %load, ptr %ret.addr, align 4
      %load1 = load i32, ptr %ret.addr, align 4
      ret i32 %load1
    }
    "###
    );
}

#[test]
fn global_variable() {
    snap!("global_variable", "static int x = 5; int f(void) { return 0; }", @r###"
    ; ModuleID = '<llvm-ir/global_variable>'
    source_filename = "<llvm-ir/global_variable>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    @x = internal global i32 5

    define i32 @f() {
    entry:
      %ret.addr = alloca i32, align 4
      store i32 0, ptr %ret.addr, align 4
      %load = load i32, ptr %ret.addr, align 4
      ret i32 %load
    }
    "###);
}

#[test]
fn aggregate_local_field() {
    snap!(
        "aggregate_local_field",
        "struct Pair { int a; int b; }; int f(void) { struct Pair p = {1, 2}; return p.b; }",
        @r###"
    ; ModuleID = '<llvm-ir/aggregate_local_field>'
    source_filename = "<llvm-ir/aggregate_local_field>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    %rcc.record.0 = type { i32, i32 }

    define i32 @f() {
    entry:
      %ret.addr = alloca i32, align 4
      %local15.addr = alloca %rcc.record.0, align 8
      call void @llvm.lifetime.start.p0(i64 8, ptr %local15.addr)
      call void @llvm.memset.p0.i64(ptr align 4 %local15.addr, i8 0, i64 8, i1 false)
      store i32 1, ptr %local15.addr, align 4
      %field_gep = getelementptr i8, ptr %local15.addr, i64 4
      store i32 2, ptr %field_gep, align 4
      %field_gep1 = getelementptr i8, ptr %local15.addr, i64 4
      %load = load i32, ptr %field_gep1, align 4
      store i32 %load, ptr %ret.addr, align 4
      call void @llvm.lifetime.end.p0(i64 8, ptr %local15.addr)
      %load2 = load i32, ptr %ret.addr, align 4
      ret i32 %load2
    }

    ; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
    declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #0

    ; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
    declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #1

    ; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
    declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #0

    attributes #0 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
    attributes #1 = { nocallback nofree nounwind willreturn memory(argmem: write) }
    "###
    );
}

#[test]
fn vla_sizeof() {
    snap!("vla_sizeof", "unsigned long f(int n) { int a[n]; return sizeof a; }", @r###"
    ; ModuleID = '<llvm-ir/vla_sizeof>'
    source_filename = "<llvm-ir/vla_sizeof>"
    target datalayout = "<normalized>"
    target triple = "x86_64-unknown-linux-gnu"

    define i64 @f(i32 %0) {
    entry:
      %ret.addr = alloca i64, align 8
      %param12.addr = alloca i32, align 4
      %tmp3.addr = alloca i64, align 8
      %tmp4.addr = alloca i64, align 8
      %tmp5.addr = alloca i64, align 8
      %param.unit = getelementptr i8, ptr %param12.addr, i64 0
      store i32 %0, ptr %param.unit, align 4
      %load = load i32, ptr %param12.addr, align 4
      %sext = sext i32 %load to i64
      store i64 %sext, ptr %tmp3.addr, align 8
      %vla_len = load i64, ptr %tmp3.addr, align 8
      %vla_stack = call ptr @llvm.stacksave.p0()
      %local13.addr = alloca i32, i64 %vla_len, align 4
      %len = load i64, ptr %tmp3.addr, align 8
      store i64 %len, ptr %tmp4.addr, align 8
      %load1 = load i64, ptr %tmp4.addr, align 8
      %mul = mul i64 %load1, 4
      store i64 %mul, ptr %tmp5.addr, align 8
      %load2 = load i64, ptr %tmp5.addr, align 8
      store i64 %load2, ptr %ret.addr, align 8
      call void @llvm.stackrestore.p0(ptr %vla_stack)
      %load3 = load i64, ptr %ret.addr, align 8
      ret i64 %load3
    }

    ; Function Attrs: nocallback nofree nosync nounwind willreturn
    declare ptr @llvm.stacksave.p0() #0

    ; Function Attrs: nocallback nofree nosync nounwind willreturn
    declare void @llvm.stackrestore.p0(ptr) #0

    attributes #0 = { nocallback nofree nosync nounwind willreturn }
    "###);
}
