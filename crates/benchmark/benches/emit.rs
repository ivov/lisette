use criterion::{Criterion, black_box, criterion_group, criterion_main};

use emit::{EmitOptions, Planner};
use lisette_benchmark::{analyze_input, single_fixture, stress_project};
use semantics::analyze::{CompilePhase, analyze};
use semantics::loader::MemoryLoader;

fn bench_small(c: &mut Criterion) {
    let source = single_fixture("small.lis");
    let filename = "small.lis".to_string();

    let mut loader = MemoryLoader::new();
    loader.add_file("_entry_", &filename, &source);

    let ast = syntax::build_ast(&source, 0).ast;
    let analyze_output = analyze(analyze_input(
        source,
        filename,
        ast,
        &loader,
        CompilePhase::Emit,
    ));
    let emit_input = analyze_output.result.into_emit_input();

    c.bench_function("emit/small", |b| {
        b.iter(|| {
            Planner::emit(
                black_box(&emit_input),
                black_box("bench"),
                EmitOptions { sourcemap: false },
            )
        });
    });
}

fn bench_stress(c: &mut Criterion, n_modules: usize) {
    let (loader, entry_source) = stress_project(n_modules, 30);

    let ast = syntax::build_ast(&entry_source, 0).ast;
    let analyze_output = analyze(analyze_input(
        entry_source,
        "main.lis".to_string(),
        ast,
        &loader,
        CompilePhase::Emit,
    ));
    let emit_input = analyze_output.result.into_emit_input();

    c.bench_function(&format!("emit/stress{n_modules}"), |b| {
        b.iter(|| {
            Planner::emit(
                black_box(&emit_input),
                black_box("bench"),
                EmitOptions { sourcemap: false },
            )
        });
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
