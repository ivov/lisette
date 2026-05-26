use std::fmt::Write as _;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use lisette_benchmark::single_fixture;

fn stress_source(n_modules: usize, n_funcs: usize) -> String {
    let mut source = String::new();
    for i in 0..n_modules {
        for j in 0..n_funcs {
            writeln!(
                source,
                "fn m{i:03}_f{j:03}(x: int, y: int) -> int {{\n  let a = x + y;\n  let b = a * 2;\n  if b > 100 {{\n    return b - 1;\n  }}\n  return b + x;\n}}\n"
            )
            .unwrap();
        }
    }
    source
}

fn bench_parse(c: &mut Criterion) {
    let source = single_fixture("small.lis");
    c.bench_function("parse/small", |b| {
        b.iter(|| syntax::build_ast(black_box(&source), black_box(0)));
    });
}

fn bench_parse_stress30(c: &mut Criterion) {
    let source = stress_source(30, 30);
    c.bench_function("parse/stress30", |b| {
        b.iter(|| syntax::build_ast(black_box(&source), black_box(0)));
    });
}

fn bench_parse_stress100(c: &mut Criterion) {
    let source = stress_source(100, 30);
    c.bench_function("parse/stress100", |b| {
        b.iter(|| syntax::build_ast(black_box(&source), black_box(0)));
    });
}

criterion_group!(
    benches,
    bench_parse,
    bench_parse_stress30,
    bench_parse_stress100
);
criterion_main!(benches);
