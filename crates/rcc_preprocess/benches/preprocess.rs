use std::path::PathBuf;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rcc_preprocess::preprocess;
use rcc_session::Session;

const MACRO_FIXTURE: &str = r#"
#define CAT_(a, b) a ## b
#define CAT(a, b) CAT_(a, b)
#define STR_(x) #x
#define STR(x) STR_(x)
#define MAX(a, b) ((a) > (b) ? (a) : (b))
#define VEC4(T) struct { T x; T y; T z; T w; }
typedef VEC4(int) vec4i;
int CAT(value, _0) = MAX(1, 2);
const char *name = STR(CAT(value, _0));
"#;

const CONDITIONAL_FIXTURE: &str = r#"
#define FEATURE 1
#define LIMIT 16
#if FEATURE && LIMIT > 8
int enabled = LIMIT;
#else
int disabled = 0;
#endif
#ifndef ONCE
#define ONCE
int once = 1;
#endif
"#;

fn seed(src: &'static str) -> (Session, rcc_span::FileId) {
    let (session, _) = Session::for_test();
    let id =
        session.source_map.write().unwrap().add_file(PathBuf::from("<bench.c>"), Arc::from(src));
    (session, id)
}

fn bench_preprocess(c: &mut Criterion) {
    let mut group = c.benchmark_group("preprocess");
    group.bench_function("macro-expansion", |b| {
        b.iter(|| {
            let (mut session, id) = seed(MACRO_FIXTURE);
            let tokens = preprocess(&mut session, id);
            black_box(tokens.len());
        });
    });
    group.bench_function("conditionals", |b| {
        b.iter(|| {
            let (mut session, id) = seed(CONDITIONAL_FIXTURE);
            let tokens = preprocess(&mut session, id);
            black_box(tokens.len());
        });
    });
    group.finish();
}

criterion_group!(benches, bench_preprocess);
criterion_main!(benches);
