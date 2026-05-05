use std::fs;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rcc_driver::pipeline::compile;
use rcc_session::{EmitKind, Options, Session};

const PIPELINE_FIXTURE: &str = r#"
#define SCALE(x) ((x) * 3)
typedef struct Pair { int a; int b; } Pair;
static Pair pairs[3] = { {1, 2}, {3, 4}, {5, 6} };

int total(void) {
    int acc = 0;
    for (int i = 0; i < 3; i++) {
        acc += SCALE(pairs[i].a + pairs[i].b);
    }
    return acc;
}
"#;

fn bench_frontend_pipeline(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("pipeline.c");
    let output = dir.path().join("pipeline.ast");
    fs::write(&input, PIPELINE_FIXTURE).expect("write benchmark input");

    c.bench_function("driver-front-end-through-ast", |b| {
        b.iter(|| {
            let opts = Options {
                emit: vec![EmitKind::Ast],
                output: Some(output.clone()),
                ..Options::default()
            };
            let mut session = Session::new(opts);
            compile(&mut session, &input).expect("driver AST pipeline");
            assert!(!session.handler.has_errors(), "pipeline benchmark emitted diagnostics");
            black_box(fs::metadata(&output).expect("AST output metadata").len());
        });
    });
}

criterion_group!(benches, bench_frontend_pipeline);
criterion_main!(benches);
