use criterion::{Criterion, black_box, criterion_group, criterion_main};

use lisette_benchmark::single_fixture;

fn bench_parse(c: &mut Criterion) {
    let source = single_fixture("small.lis");
    c.bench_function("parse/small", |b| {
        b.iter(|| syntax::build_ast(black_box(&source), black_box(0)));
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
