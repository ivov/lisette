//! Differential tests for Go-style struct and interface embedding. Each test
//! builds a small graph of embedded types, renders it to both Go and Lisette,
//! and checks that Lisette resolves member access and interface satisfaction
//! the same way Go does. Lisette may reject what Go accepts as promotion is
//! yet to be built, but Lisette must never accept what Go rejects.

pub mod compare;
pub mod go_answerer;
pub mod lisette_answer;
pub mod random_scenarios;
pub mod render_go;
pub mod render_lis;
pub mod run_check;
pub mod runner;
pub mod scenario;

#[cfg(test)]
pub mod corpus;
#[cfg(test)]
pub mod fixtures;

use scenario::NodeId;

#[derive(Clone, Debug)]
pub struct PrintedQuestion {
    pub root: NodeId,
    pub member: String,
    /// Also exercise the `Root.member` method-expression form. Set only for a
    /// public, value-receiver method on a non-generic root and declaring type,
    /// where `Root.member` is valid and `g(r)` needs no addressing.
    pub method_expression: bool,
}

use crate::_harness::go::go_available;
use crate::embedded_types::corpus::differential_scenarios;
use crate::embedded_types::go_answerer::GoAnswerer;
use crate::embedded_types::random_scenarios::generate;
use crate::embedded_types::runner::{run_scenario, summarize};

/// The number of random scenarios, after the hand-written ones.
const SEEDS: u64 = 200;

#[test]
fn embedded_types_match_go() {
    if !go_available() {
        eprintln!("skipping embedded_types_match_go: `go` not found");
        return;
    }

    let answerer = GoAnswerer::build().expect("the go/types answerer builds");

    let mut reports = Vec::new();
    for scenario in differential_scenarios() {
        reports.push(run_scenario(&answerer, &scenario));
    }
    for seed in 0..SEEDS {
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
