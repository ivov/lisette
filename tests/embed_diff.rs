//! Differential tests for Go-style struct and interface embedding. Each test
//! builds a small graph of embedded types, renders it to both Go and Lisette,
//! and checks that Lisette resolves member access and interface satisfaction
//! the same way Go does. Lisette may reject what Go accepts as promotion is
//! yet to be built, but Lisette must never accept what Go rejects.

mod _embed_harness;

use _embed_harness::corpus::differential_scenarios;
use _embed_harness::go_answerer::{GoAnswerer, go_available};
use _embed_harness::random_scenarios::generate;
use _embed_harness::runner::{run_scenario, summarize};

/// Override with `EMBED_DIFF_N` for a deeper sweep.
const DEFAULT_SEEDS: u64 = 200;

#[test]
fn embed_differential() {
    if !go_available() {
        eprintln!("SKIP embed_differential: the `go` toolchain is required but was not found");
        return;
    }

    let answerer = GoAnswerer::build().expect("the go/types answerer builds");
    let seeds: u64 = std::env::var("EMBED_DIFF_N")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_SEEDS);

    let mut reports = Vec::new();
    for scenario in differential_scenarios() {
        reports.push(run_scenario(&answerer, &scenario));
    }
    for seed in 0..seeds {
        reports.push(run_scenario(&answerer, &generate(seed)));
    }

    let summary = summarize(&reports);

    eprintln!("embed differential: {} scenarios", reports.len());
    eprintln!(
        "  match={} incomplete={} not_comparable={}",
        summary.matches, summary.incompletes, summary.not_comparable
    );
    eprintln!("  agreement with Go: {:.1}%", summary.acceptance() * 100.0);

    // Lisette rejecting what Go accepts (Incomplete) is expected while promotion
    // is unimplemented, but accepting what Go rejects is a real bug. Gate
    // failures and build divergences are faults in the harness or codegen.
    assert!(
        summary.over_accepts.is_empty(),
        "OVER-ACCEPTANCE: Lisette accepts what Go rejects (each reproducible from its seed):\n{}",
        summary.over_accepts.join("\n"),
    );
    assert!(
        summary.gate_failures.is_empty(),
        "harness gate failures (renderer/answerer integrity):\n{}",
        summary.gate_failures.join("\n"),
    );
    assert!(
        summary.build_divergences.is_empty(),
        "emit-and-build divergences (emitted Go misbehaves vs hand-written):\n{}",
        summary.build_divergences.join("\n"),
    );
}
