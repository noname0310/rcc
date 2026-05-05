use std::path::PathBuf;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rcc_parse::parse;
use rcc_preprocess::preprocess;
use rcc_session::Session;

const DECLARATION_FIXTURE: &str = r#"
typedef struct Node Node;
struct Node {
    int value;
    Node *next;
};
static int table[4] = { [0] = 1, [2] = 3 };
int sum(Node *n) {
    int acc = 0;
    for (; n; n = n->next) {
        acc += n->value;
    }
    return acc;
}
"#;

const EXPRESSION_FIXTURE: &str = r#"
int compute(int a, int b, int c) {
    int x = (a + b * c) << 2;
    x = x ? x + (a, b) : c;
    return sizeof(int[4]) + x;
}
"#;

fn seed(src: &'static str) -> (Session, Vec<rcc_lexer::PpToken>) {
    let (mut session, _) = Session::for_test();
    let id =
        session.source_map.write().unwrap().add_file(PathBuf::from("<bench.c>"), Arc::from(src));
    let tokens = preprocess(&mut session, id);
    (session, tokens)
}

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");
    group.bench_function("declarations-and-statements", |b| {
        let (mut session, tokens) = seed(DECLARATION_FIXTURE);
        b.iter(|| {
            let ast = parse(&mut session, black_box(tokens.clone())).expect("parse benchmark AST");
            black_box(ast.decls.len());
        });
    });
    group.bench_function("expression-heavy", |b| {
        let (mut session, tokens) = seed(EXPRESSION_FIXTURE);
        b.iter(|| {
            let ast = parse(&mut session, black_box(tokens.clone())).expect("parse benchmark AST");
            black_box(ast.decls.len());
        });
    });
    group.finish();
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
