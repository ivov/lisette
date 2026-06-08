use super::compare::{Comparison, Verdict, compare};
use super::go_answerer::GoAnswerer;
use super::lisette_answer::{LisetteAnswers, lisette_answers};
use super::run_check::{RunOutcome, run_check};
use super::scenario::Scenario;

pub struct ScenarioReport {
    pub name: String,
    pub seed: u64,
    pub gate_failure: Option<String>,
    pub not_comparable: Option<String>,
    pub comparisons: Vec<Comparison>,
    pub build: RunOutcome,
}

pub fn run_scenario(answerer: &GoAnswerer, scenario: &Scenario) -> ScenarioReport {
    if let Err(err) = scenario.validate() {
        return gate(scenario, format!("invalid scenario: {err}"));
    }

    let go = match answerer.answer(scenario) {
        Ok(answers) => answers,
        Err(err) => return gate(scenario, format!("answerer query failed: {err}")),
    };
    let lisette = lisette_answers(scenario);

    match eligibility(&go, &lisette) {
        Eligibility::RendererFault(reason) => return gate(scenario, reason),
        Eligibility::NotComparable(reason) => return not_comparable(scenario, reason),
        Eligibility::Comparable => {}
    }

    let comparisons = compare(scenario, &go, &lisette);
    let build = run_check(scenario, &go, &lisette);

    ScenarioReport {
        name: scenario.name.clone(),
        seed: scenario.seed,
        gate_failure: None,
        not_comparable: None,
        comparisons,
        build,
    }
}

enum Eligibility {
    Comparable,
    /// A renderer or generator bug: the suite must fail.
    RendererFault(String),
    /// A legitimate Lisette declaration rejection: the scenario is skipped.
    NotComparable(String),
}

/// Decide whether a scenario can be compared per question. The generated Go must
/// type-check and the rendered Lisette must parse, or it is a bug. If Lisette
/// rejects a declaration (e.g. `embed_defined_type`), its answers to unrelated
/// questions cannot be trusted, so the scenario is skipped rather than compared.
/// It is skipped, not forced to Incomplete. Forcing it would let a real
/// over-acceptance hide as a Match.
fn eligibility(go: &super::go_answerer::GoAnswers, lisette: &LisetteAnswers) -> Eligibility {
    if let Some(fatal) = &go.fatal_error {
        return Eligibility::RendererFault(format!(
            "Go could not parse the generated source: {fatal}"
        ));
    }
    let unexpected: Vec<&String> = go
        .type_errors
        .iter()
        .filter(|err| {
            !err.contains("duplicate method") && !err.contains("other declaration of method")
        })
        .collect();
    if !unexpected.is_empty() {
        return Eligibility::RendererFault(format!("unexpected Go type errors: {unexpected:?}"));
    }
    if lisette.parse_failed {
        return Eligibility::RendererFault("rendered Lisette failed to parse".to_string());
    }
    let renderer_faults: Vec<&String> = lisette
        .unattributed
        .iter()
        .filter(|code| code.contains("type_not_found") || code.starts_with("parse."))
        .collect();
    if !renderer_faults.is_empty() {
        return Eligibility::RendererFault(format!(
            "Lisette renderer fault in declarations: {renderer_faults:?}"
        ));
    }
    if !lisette.unattributed.is_empty() {
        return Eligibility::NotComparable(format!(
            "Lisette declaration rejection: {:?}",
            lisette.unattributed
        ));
    }
    Eligibility::Comparable
}

fn gate(scenario: &Scenario, reason: String) -> ScenarioReport {
    ScenarioReport {
        name: scenario.name.clone(),
        seed: scenario.seed,
        gate_failure: Some(reason),
        not_comparable: None,
        comparisons: vec![],
        build: RunOutcome::Skipped,
    }
}

fn not_comparable(scenario: &Scenario, reason: String) -> ScenarioReport {
    ScenarioReport {
        name: scenario.name.clone(),
        seed: scenario.seed,
        gate_failure: None,
        not_comparable: Some(reason),
        comparisons: vec![],
        build: RunOutcome::Skipped,
    }
}

#[derive(Default)]
pub struct Summary {
    pub matches: usize,
    pub incompletes: usize,
    /// Scenarios skipped because Lisette rejected a declaration.
    pub not_comparable: usize,
    pub gate_failures: Vec<String>,
    pub over_accepts: Vec<String>,
    pub build_divergences: Vec<String>,
}

impl Summary {
    /// Fraction of decided comparisons where Lisette matches Go; rises as
    /// promotion is implemented.
    pub fn acceptance(&self) -> f64 {
        let decided = self.matches + self.incompletes;
        if decided == 0 {
            0.0
        } else {
            self.matches as f64 / decided as f64
        }
    }

    pub fn is_clean(&self) -> bool {
        self.gate_failures.is_empty()
            && self.over_accepts.is_empty()
            && self.build_divergences.is_empty()
    }
}

pub fn summarize(reports: &[ScenarioReport]) -> Summary {
    let mut summary = Summary::default();
    for report in reports {
        if let Some(reason) = &report.gate_failure {
            summary
                .gate_failures
                .push(format!("[{} seed={}] {reason}", report.name, report.seed));
            continue;
        }
        if report.not_comparable.is_some() {
            summary.not_comparable += 1;
            continue;
        }
        for comparison in &report.comparisons {
            classify(report, comparison, &mut summary);
        }
        if let RunOutcome::Divergence(message) = &report.build {
            summary
                .build_divergences
                .push(format!("[{} seed={}] {message}", report.name, report.seed));
        }
    }
    summary
}

fn classify(report: &ScenarioReport, comparison: &Comparison, summary: &mut Summary) {
    match &comparison.verdict {
        Verdict::Match => summary.matches += 1,
        Verdict::Incomplete => summary.incompletes += 1,
        Verdict::Gate(reason) => summary
            .gate_failures
            .push(format!("[{} seed={}] {reason}", report.name, report.seed)),
        Verdict::OverAccept(kind) => summary
            .over_accepts
            .push(over_accept_line(report, comparison, *kind)),
    }
}

fn over_accept_line(
    report: &ScenarioReport,
    comparison: &Comparison,
    kind: super::compare::OverAcceptKind,
) -> String {
    format!(
        "[{} seed={}] question #{} `{}`: {:?} ({})",
        report.name,
        report.seed,
        comparison.question_index,
        comparison.label,
        kind,
        comparison.detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;
    use crate::_embed_harness::go_answerer::go_available;
    use crate::_embed_harness::random_scenarios::generate;

    #[test]
    fn generated_sweep_is_sound() {
        if !go_available() {
            eprintln!("skipping runner sweep: `go` toolchain not found");
            return;
        }
        let answerer = GoAnswerer::build().expect("answerer builds");
        let reports: Vec<_> = (0..120)
            .map(|seed| run_scenario(&answerer, &generate(seed)))
            .collect();
        let summary = summarize(&reports);
        assert!(
            summary.is_clean(),
            "generated sweep not clean:\n  gate: {:?}\n  over-accepts: {:?}\n  build: {:?}",
            summary.gate_failures,
            summary.over_accepts,
            summary.build_divergences,
        );
    }

    #[test]
    fn fixtures_run_clean() {
        if !go_available() {
            return;
        }
        let answerer = GoAnswerer::build().expect("answerer builds");
        let reports: Vec<_> = fixtures::all()
            .iter()
            .map(|scenario| run_scenario(&answerer, scenario))
            .collect();
        let summary = summarize(&reports);
        assert!(
            summary.is_clean(),
            "fixtures not clean: {:?}",
            summary.over_accepts
        );
        assert!(summary.matches > 0);
        assert_eq!(
            summary.incompletes, 0,
            "a base fixture regressed to incomplete"
        );
    }
}
