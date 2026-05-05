use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rcc_span::FileId;

const SMALL_FIXTURE: &str = r#"
#define MAX(a, b) ((a) > (b) ? (a) : (b))
int sum(int *xs, int n) {
    int acc = 0;
    for (int i = 0; i < n; i++) {
        acc += MAX(xs[i], i);
    }
    return acc;
}
"#;

const LITERAL_FIXTURE: &str = r#"
static const char *names[] = {
    "alpha", "beta\n", "gamma\x20delta", L"wide"
};
unsigned long long mask = 0xff00aa55ULL;
double coeff = 0x1.8p+2;
"#;

fn bench_lexer(c: &mut Criterion) {
    let mut group = c.benchmark_group("lexer");
    group.bench_function("small-c99-unit", |b| {
        b.iter(|| {
            let count = rcc_lexer::tokenize(FileId(0), black_box(SMALL_FIXTURE)).count();
            black_box(count);
        });
    });
    group.bench_function("literal-heavy-unit", |b| {
        b.iter(|| {
            let count = rcc_lexer::tokenize(FileId(0), black_box(LITERAL_FIXTURE)).count();
            black_box(count);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_lexer);
criterion_main!(benches);
