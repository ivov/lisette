use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};

use lisette_benchmark::{analyze_input, single_fixture, stress_project};
use semantics::analyze::{CompilePhase, analyze};
use semantics::loader::MemoryLoader;

fn bench_small(c: &mut Criterion) {
    let source = single_fixture("small.lis");
    let filename = "small.lis".to_string();

    let mut loader = MemoryLoader::new();
    loader.add_file("_entry_", &filename, &source);

    c.bench_function("analyze/small", |b| {
        b.iter_batched(
            || syntax::build_ast(&source, 0).ast,
            |ast| {
                analyze(analyze_input(
                    source.clone(),
                    filename.clone(),
                    ast,
                    black_box(&loader),
                    CompilePhase::Check,
                ))
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_stress(c: &mut Criterion, n_modules: usize) {
    let (loader, entry_source) = stress_project(n_modules, 30);

    c.bench_function(&format!("analyze/stress{n_modules}"), |b| {
        b.iter_batched(
            || syntax::build_ast(&entry_source, 0).ast,
            |ast| {
                analyze(analyze_input(
                    entry_source.clone(),
                    "main.lis".to_string(),
                    ast,
                    black_box(&loader),
                    CompilePhase::Check,
                ))
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_stress30(c: &mut Criterion) {
    bench_stress(c, 30);
}

fn bench_stress100(c: &mut Criterion) {
    bench_stress(c, 100);
}

criterion_group!(benches, bench_small, bench_stress30, bench_stress100);
criterion_main!(benches);
