//! End-to-end CFG lowering invariants over C snippets.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_cfg::{
    build_bodies, pretty::dump_body, BasicBlockId, Body, Const, ConstKind, Operand, Place,
    Projection, Rvalue, StatementKind, TerminatorKind,
};
use rcc_errors::{CaptureEmitter, Handler};
use rcc_hir::{DefId, Local, TyCtxt};
use rcc_hir_lower::lower;
use rcc_session::{Options, Session};
use rcc_typeck::check;

struct Fixture {
    name: &'static str,
    src: &'static str,
    functions: usize,
}

struct Lowered {
    tcx: TyCtxt,
    bodies: Vec<(DefId, Body)>,
}

const FIXTURES: &[Fixture] = &[
    Fixture { name: "return_zero", src: "int f(void) { return 0; }", functions: 1 },
    Fixture { name: "return_param", src: "int f(int x) { return x; }", functions: 1 },
    Fixture {
        name: "local_arithmetic",
        src: "int f(int a, int b) { int x = a + b * 2; return x; }",
        functions: 1,
    },
    Fixture {
        name: "comma_expression",
        src: "int f(void) { int x = 1; int y = (x = 2, x + 3); return y; }",
        functions: 1,
    },
    Fixture {
        name: "casts",
        src: "int f(double d) { int x = (int)d; return x; }",
        functions: 1,
    },
    Fixture {
        name: "logical_and",
        src: "int f(int a, int b) { return a && b; }",
        functions: 1,
    },
    Fixture {
        name: "logical_or",
        src: "int f(int a, int b) { return a || b; }",
        functions: 1,
    },
    Fixture {
        name: "conditional_expression",
        src: "int f(int a) { return a ? 1 : 2; }",
        functions: 1,
    },
    Fixture {
        name: "if_without_else",
        src: "int f(int a) { int x = 0; if (a) x = 1; return x; }",
        functions: 1,
    },
    Fixture {
        name: "if_else",
        src: "int f(int a) { int x = 0; if (a) x = 1; else x = 2; return x; }",
        functions: 1,
    },
    Fixture {
        name: "nested_if",
        src: "int f(int a, int b) { int x = 0; if (a) { if (b) x = 1; } return x; }",
        functions: 1,
    },
    Fixture {
        name: "while_loop",
        src: "int f(int n) { int i = 0; while (i < n) { i = i + 1; } return i; }",
        functions: 1,
    },
    Fixture {
        name: "do_while_loop",
        src: "int f(int n) { int i = 0; do { i = i + 1; } while (i < n); return i; }",
        functions: 1,
    },
    Fixture {
        name: "for_expression_init",
        src: "int f(int n) { int i = 0; for (i = 0; i < n; i = i + 1) {} return i; }",
        functions: 1,
    },
    Fixture {
        name: "for_prefix_increment_step",
        src: "int f(int n) { int i = 0; for (i = 0; i < n; ++i) {} return i; }",
        functions: 1,
    },
    Fixture {
        name: "postfix_increment_return",
        src: "int f(void) { int i = 0; return i++; }",
        functions: 1,
    },
    Fixture {
        name: "prefix_decrement_return",
        src: "int f(void) { int i = 2; return --i; }",
        functions: 1,
    },
    Fixture {
        name: "deref_postfix_increment",
        src: "int f(int *p) { return (*p)++; }",
        functions: 1,
    },
    Fixture {
        name: "postfix_pointer_deref",
        src: "int f(int *p) { return *p++; }",
        functions: 1,
    },
    Fixture {
        name: "array_index_postfix_increment",
        src: "int f(int i) { int a[2] = {1, 2}; return a[i]++; }",
        functions: 1,
    },
    Fixture {
        name: "for_infinite_with_break",
        src: "int f(void) { int i = 0; for (;;) { i = i + 1; if (i == 3) break; } return i; }",
        functions: 1,
    },
    Fixture {
        name: "break_continue",
        src: "int f(int n) { int i = 0; while (i < n) { i = i + 1; if (i == 2) continue; if (i == 4) break; } return i; }",
        functions: 1,
    },
    Fixture {
        name: "switch_cases",
        src: "int f(int x) { int y = 0; switch (x) { case 1: y = 2; break; case 2: y = 3; default: y = 4; } return y; }",
        functions: 1,
    },
    Fixture {
        name: "goto_label",
        src: "int f(int x) { int y = 0; if (x) goto L; y = 1; L: return y; }",
        functions: 1,
    },
    Fixture {
        name: "array_init_index",
        src: "int f(void) { int a[3] = {1, 2}; return a[1]; }",
        functions: 1,
    },
    Fixture {
        name: "struct_field",
        src: "struct S { int a; int b; }; int f(void) { struct S s = {1, 2}; return s.b; }",
        functions: 1,
    },
    Fixture {
        name: "address_and_deref",
        src: "int f(void) { int x = 1; int *p = &x; return *p; }",
        functions: 1,
    },
    Fixture {
        name: "call_non_void",
        src: "int g(int); int f(int x) { return g(x); }",
        functions: 1,
    },
    Fixture {
        name: "call_void",
        src: "void g(int); int f(int x) { g(x); return x; }",
        functions: 1,
    },
    Fixture {
        name: "variadic_call",
        src: "int printf(char *, ...); int f(void) { printf(\"%d\", 1); return 0; }",
        functions: 1,
    },
    Fixture {
        name: "vla_sizeof",
        src: "unsigned long f(int n) { int a[n]; return sizeof a; }",
        functions: 1,
    },
    Fixture {
        name: "sizeof_int_array",
        src: "unsigned long f(void) { int a[3]; return sizeof a; }",
        functions: 1,
    },
    Fixture {
        name: "real_to_complex_return",
        src: "double _Complex f(double x) { return x; }",
        functions: 1,
    },
    Fixture {
        name: "complex_to_real_return",
        src: "double f(double _Complex z) { return z; }",
        functions: 1,
    },
    Fixture {
        name: "multi_function",
        src: "int a(void) { return 1; } int b(void) { return a(); }",
        functions: 2,
    },
    Fixture {
        name: "block_scope",
        src: "int f(void) { int x = 1; { int y = 2; x = x + y; } return x; }",
        functions: 1,
    },
];

#[test]
fn cfg_fixture_matrix_satisfies_invariants() {
    assert!(FIXTURES.len() >= 25, "task requires at least 25 CFG fixtures");
    for fixture in FIXTURES {
        let lowered = lower_snippet(fixture.name, fixture.src);
        assert_eq!(
            lowered.bodies.len(),
            fixture.functions,
            "{}: unexpected function-body count",
            fixture.name
        );
        for (def, body) in &lowered.bodies {
            assert_body_invariants(fixture.name, *def, body);
        }
    }
}

#[test]
fn cfg_snapshots_are_stable() {
    let cases = [
        ("if_else", "int f(int a) { int x = 0; if (a) x = 1; else x = 2; return x; }"),
        (
            "loop_break_continue",
            "int f(int n) { int i = 0; while (i < n) { i = i + 1; if (i == 3) break; } return i; }",
        ),
        (
            "switch_fallthrough",
            "int f(int x) { int y = 0; switch (x) { case 1: y = 1; case 2: y = 2; break; default: y = 3; } return y; }",
        ),
        ("call_and_pointer", "int g(int *); int f(void) { int x = 1; return g(&x); }"),
        ("vla_sizeof", "unsigned long f(int n) { int a[n]; return sizeof a; }"),
    ];

    for (name, src) in cases {
        insta::with_settings!({
            snapshot_path => "snapshots/cfg",
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(name, render_snippet(name, src));
        });
    }
}

#[test]
fn sizeof_layout_service_lowers_fixed_sizes() {
    let cases = [("sizeof_int_array", "unsigned long f(void) { int a[3]; return sizeof a; }", 12)];

    for (name, src, expected) in cases {
        let lowered = lower_snippet(name, src);
        let body = &lowered.bodies[0].1;
        assert!(
            body_contains_int_const(body, expected),
            "{name}: expected sizeof constant {expected} in CFG:\n{}",
            dump_body(body, &lowered.tcx)
        );
    }
}

#[test]
fn complex_conversion_rvalues_are_explicit() {
    let cases = [
        ("return_real_to_complex", "double _Complex f(double x) { return x; }", 1, 0),
        ("return_complex_to_real", "double f(double _Complex z) { return z; }", 0, 1),
        ("assignment_real_to_complex", "void f(double x) { double _Complex z; z = x; }", 1, 0),
        (
            "call_argument_real_to_complex",
            "void g(double _Complex); void f(double x) { g(x); }",
            1,
            0,
        ),
        (
            "conditional_real_to_complex_arm",
            "double _Complex f(int c, double x, double _Complex z) { return c ? x : z; }",
            1,
            0,
        ),
    ];

    for (name, src, expected_to_complex, expected_to_real) in cases {
        let lowered = lower_snippet(name, src);
        let counts = complex_conversion_counts(&lowered.bodies[0].1);
        assert_eq!(
            counts.0,
            expected_to_complex,
            "{name}: unexpected ComplexFromReal count in CFG:\n{}",
            dump_body(&lowered.bodies[0].1, &lowered.tcx)
        );
        assert_eq!(
            counts.1,
            expected_to_real,
            "{name}: unexpected RealFromComplex count in CFG:\n{}",
            dump_body(&lowered.bodies[0].1, &lowered.tcx)
        );
    }
}

fn lower_snippet(name: &str, src: &str) -> Lowered {
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session
        .source_map
        .write()
        .unwrap()
        .add_file(PathBuf::from(format!("<cfg/{name}>")), Arc::from(src));

    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);
    let ast = rcc_parse::parse(&mut session, pp_tokens).expect("parse returned None");
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, &mut session);
    check(&mut session, &mut tcx, &mut hir);
    assert!(
        !session.handler.has_errors(),
        "{name}: unexpected diagnostics: {:?}",
        cap.diagnostics()
    );

    let mut bodies: Vec<_> = build_bodies(&mut session, &tcx, &hir).into_iter().collect();
    bodies.sort_by_key(|(def, _)| def.0);
    Lowered { tcx, bodies }
}

fn complex_conversion_counts(body: &Body) -> (usize, usize) {
    let mut to_complex = 0usize;
    let mut to_real = 0usize;
    for block in body.blocks.iter() {
        for stmt in &block.statements {
            let StatementKind::Assign { rvalue, .. } = &stmt.kind else { continue };
            match rvalue {
                Rvalue::ComplexFromReal { .. } => to_complex += 1,
                Rvalue::RealFromComplex { .. } => to_real += 1,
                _ => {}
            }
        }
    }
    (to_complex, to_real)
}

fn body_contains_int_const(body: &Body, expected: i128) -> bool {
    body.blocks.iter().any(|block| {
        block.statements.iter().any(|stmt| match &stmt.kind {
            StatementKind::Assign { rvalue, .. } => rvalue_contains_int_const(rvalue, expected),
            _ => false,
        })
    })
}

fn rvalue_contains_int_const(rvalue: &Rvalue, expected: i128) -> bool {
    match rvalue {
        Rvalue::Use(op) | Rvalue::UnaryOp(_, op) | Rvalue::Cast { op, .. } => {
            operand_contains_int_const(op, expected)
        }
        Rvalue::ComplexFromReal { real, .. } => operand_contains_int_const(real, expected),
        Rvalue::RealFromComplex { complex, .. } => operand_contains_int_const(complex, expected),
        Rvalue::BinaryOp(_, lhs, rhs) => {
            operand_contains_int_const(lhs, expected) || operand_contains_int_const(rhs, expected)
        }
        Rvalue::AddressOf(_) | Rvalue::Len(_) => false,
    }
}

fn operand_contains_int_const(operand: &Operand, expected: i128) -> bool {
    matches!(
        operand,
        Operand::Const(Const { kind: ConstKind::Int(value), .. }) if *value == expected
    )
}

fn render_snippet(name: &str, src: &str) -> String {
    let lowered = lower_snippet(name, src);
    let mut out = String::new();
    for (idx, (_, body)) in lowered.bodies.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&dump_body(body, &lowered.tcx));
    }
    out
}

fn assert_body_invariants(name: &str, def: DefId, body: &Body) {
    assert!(!body.blocks.is_empty(), "{name}/def#{}: body has no blocks", def.0);
    assert!(!body.locals.is_empty(), "{name}/def#{}: body has no locals", def.0);
    assert_eq!(body.blocks.iter_enumerated().next().unwrap().0, BasicBlockId(0));

    if let Some(ret_ty) = body.ret_ty {
        assert_eq!(
            body.locals[Local(0)].ty,
            ret_ty,
            "{name}/def#{}: return slot type differs from body ret_ty",
            def.0
        );
    }
    assert!(
        !body.locals[Local(0)].is_param,
        "{name}/def#{}: return slot must not be a parameter",
        def.0
    );

    assert_local_order(name, def, body);
    let reachable = reachable_blocks(name, def, body);
    let mut storage_live = vec![0usize; body.locals.len()];
    let mut storage_dead = vec![0usize; body.locals.len()];

    for (bb, block) in body.blocks.iter_enumerated() {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::Assign { place, rvalue } => {
                    assert_place_valid(name, def, body, place);
                    assert_rvalue_valid(name, def, body, rvalue);
                }
                StatementKind::StorageLive(local) => {
                    assert_local_valid(name, def, body, *local);
                    storage_live[local.0 as usize] += 1;
                }
                StatementKind::StorageDead(local) => {
                    assert_local_valid(name, def, body, *local);
                    storage_dead[local.0 as usize] += 1;
                }
                StatementKind::Nop => {}
            }
        }

        assert_terminator_valid(name, def, body, bb, &block.terminator.kind);
        if reachable[bb.0 as usize] {
            assert!(
                !matches!(block.terminator.kind, TerminatorKind::Unreachable),
                "{name}/def#{}: reachable {bb:?} has default unreachable terminator",
                def.0
            );
        }
    }

    for (local, decl) in body.locals.iter_enumerated() {
        if let Some(vla_len) = decl.vla_len {
            assert_local_valid(name, def, body, vla_len);
            assert_ne!(
                local, vla_len,
                "{name}/def#{}: VLA local uses itself as length local",
                def.0
            );
        }
        if local != Local(0) && !decl.is_param && decl.name.is_some() {
            let live = storage_live[local.0 as usize];
            let dead = storage_dead[local.0 as usize];
            assert!(live > 0, "{name}/def#{}: named local {local:?} is never StorageLive", def.0);
            assert!(dead > 0, "{name}/def#{}: named local {local:?} is never StorageDead", def.0);
        }
    }
}

fn assert_local_order(name: &str, def: DefId, body: &Body) {
    let mut seen_user_local = false;
    for (local, decl) in body.locals.iter_enumerated().skip(1) {
        if decl.is_param {
            assert!(
                !seen_user_local,
                "{name}/def#{}: parameter {local:?} appears after user locals/temps",
                def.0
            );
        } else {
            seen_user_local = true;
        }
    }
}

fn reachable_blocks(name: &str, def: DefId, body: &Body) -> Vec<bool> {
    let mut reachable = vec![false; body.blocks.len()];
    let mut stack = vec![BasicBlockId(0)];
    while let Some(bb) = stack.pop() {
        assert_block_valid(name, def, body, bb);
        let idx = bb.0 as usize;
        if reachable[idx] {
            continue;
        }
        reachable[idx] = true;
        for succ in successors(&body.blocks[bb].terminator.kind) {
            assert_block_valid(name, def, body, succ);
            stack.push(succ);
        }
    }
    reachable
}

fn assert_terminator_valid(
    name: &str,
    def: DefId,
    body: &Body,
    bb: BasicBlockId,
    term: &TerminatorKind,
) {
    match term {
        TerminatorKind::Goto(target) => assert_block_valid(name, def, body, *target),
        TerminatorKind::SwitchInt { discr, targets } => {
            assert_operand_valid(name, def, body, discr);
            assert!(
                !targets.is_empty(),
                "{name}/def#{}: {bb:?} has SwitchInt without targets",
                def.0
            );
            assert!(
                targets.last().is_some_and(|(value, _)| value.is_none()),
                "{name}/def#{}: {bb:?} SwitchInt default target must be last",
                def.0
            );
            for (_, target) in targets {
                assert_block_valid(name, def, body, *target);
            }
        }
        TerminatorKind::Call { callee, args, destination, target } => {
            assert_operand_valid(name, def, body, callee);
            for arg in args {
                assert_operand_valid(name, def, body, arg);
            }
            if let Some(dest) = destination {
                assert_place_valid(name, def, body, dest);
            }
            if let Some(target) = target {
                assert_block_valid(name, def, body, *target);
            }
        }
        TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}

fn assert_rvalue_valid(name: &str, def: DefId, body: &Body, rvalue: &Rvalue) {
    match rvalue {
        Rvalue::Use(op) | Rvalue::UnaryOp(_, op) | Rvalue::Cast { op, .. } => {
            assert_operand_valid(name, def, body, op);
        }
        Rvalue::ComplexFromReal { real, .. } => {
            assert_operand_valid(name, def, body, real);
        }
        Rvalue::RealFromComplex { complex, .. } => {
            assert_operand_valid(name, def, body, complex);
        }
        Rvalue::BinaryOp(_, lhs, rhs) => {
            assert_operand_valid(name, def, body, lhs);
            assert_operand_valid(name, def, body, rhs);
        }
        Rvalue::AddressOf(place) | Rvalue::Len(place) => {
            assert_place_valid(name, def, body, place);
        }
    }
}

fn assert_operand_valid(name: &str, def: DefId, body: &Body, operand: &Operand) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => assert_place_valid(name, def, body, place),
        Operand::Const(Const { .. }) => {}
    }
}

fn assert_place_valid(name: &str, def: DefId, body: &Body, place: &Place) {
    assert_local_valid(name, def, body, place.base);
    for projection in &place.projection {
        match projection {
            Projection::Deref | Projection::Field(_) => {}
            Projection::Index(index) => assert_operand_valid(name, def, body, index),
        }
    }
}

fn assert_local_valid(name: &str, def: DefId, body: &Body, local: Local) {
    assert!(
        (local.0 as usize) < body.locals.len(),
        "{name}/def#{}: local {local:?} out of range 0..{}",
        def.0,
        body.locals.len()
    );
}

fn assert_block_valid(name: &str, def: DefId, body: &Body, bb: BasicBlockId) {
    assert!(
        (bb.0 as usize) < body.blocks.len(),
        "{name}/def#{}: block {bb:?} out of range 0..{}",
        def.0,
        body.blocks.len()
    );
}

fn successors(term: &TerminatorKind) -> Vec<BasicBlockId> {
    match term {
        TerminatorKind::Goto(target) => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => {
            targets.iter().map(|(_, target)| *target).collect()
        }
        TerminatorKind::Call { target: Some(target), .. } => vec![*target],
        TerminatorKind::Call { target: None, .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
    }
}
