//! Compares Lisette's answer to Go's for each question and classifies the
//! result: they agree (Match), Lisette rejected something Go accepts
//! (Incomplete, fine for now), or Lisette accepted something Go rejects
//! (OverAccept, a failure).

use super::go_answerer::{GoAnswer, GoAnswers};
use super::lisette_answer::{LisetteAnswer, LisetteAnswers, Outcome};
use super::scenario::{Question, Scenario};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Match,
    Incomplete,
    OverAccept(OverAcceptKind),
    /// A harness-integrity failure (parse, Go answer, or shape mismatch). Always
    /// fails the suite.
    Gate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverAcceptKind {
    ResolvesButGoRejects,
    SatisfiesButGoDenies,
    PickedOneWhereGoAmbiguous,
}

impl Verdict {
    pub fn is_over_accept(&self) -> bool {
        matches!(self, Verdict::OverAccept(_))
    }

    pub fn is_gate(&self) -> bool {
        matches!(self, Verdict::Gate(_))
    }
}

#[derive(Debug, Clone)]
pub struct Comparison {
    pub question_index: usize,
    pub label: String,
    pub verdict: Verdict,
    /// Human-readable detail about Go's answer, for the report.
    pub detail: String,
}

/// A satisfaction question contributes two comparisons (value and pointer forms).
pub fn compare(scenario: &Scenario, go: &GoAnswers, lisette: &LisetteAnswers) -> Vec<Comparison> {
    if let Some(fatal) = &go.fatal_error {
        return gate_all(scenario, &format!("Go answer error: {fatal}"));
    }
    if lisette.parse_failed {
        return gate_all(scenario, "rendered Lisette failed to parse");
    }
    if go.results.len() != scenario.questions.len()
        || lisette.questions.len() != scenario.questions.len()
    {
        return gate_all(scenario, "question count mismatch between Go and Lisette");
    }

    let mut out = Vec::new();
    for (i, question) in scenario.questions.iter().enumerate() {
        let expected = &go.results[i];
        match (question, &lisette.questions[i]) {
            (Question::Selector { root, member, .. }, LisetteAnswer::Selector(outcome)) => {
                out.push(Comparison {
                    question_index: i,
                    label: format!("{}.{}", scenario.node_name(*root), member),
                    verdict: compare_selector(expected, outcome),
                    detail: selector_detail(expected),
                });
            }
            (
                Question::Satisfies { type_id, interface },
                LisetteAnswer::Satisfies { value, pointer },
            ) => {
                let type_name = scenario.node_name(*type_id);
                let interface_name = scenario.node_name(*interface);
                out.push(Comparison {
                    question_index: i,
                    label: format!("{type_name} : {interface_name} (value)"),
                    verdict: compare_satisfies(expected.satisfies_value, value),
                    detail: format!("go: value={}", expected.satisfies_value),
                });
                out.push(Comparison {
                    question_index: i,
                    label: format!("{type_name} : {interface_name} (pointer)"),
                    verdict: compare_satisfies(expected.satisfies_pointer, pointer),
                    detail: format!("go: pointer={}", expected.satisfies_pointer),
                });
            }
            _ => out.push(Comparison {
                question_index: i,
                label: "<mismatch>".into(),
                verdict: Verdict::Gate("question/answer kind mismatch".into()),
                detail: String::new(),
            }),
        }
    }
    out
}

fn compare_selector(go: &GoAnswer, lisette: &Outcome) -> Verdict {
    match (go.resolves, go.ambiguous, lisette.resolves()) {
        (true, _, true) => Verdict::Match,
        (true, _, false) => Verdict::Incomplete,
        (false, false, false) => Verdict::Match,
        (false, false, true) => Verdict::OverAccept(OverAcceptKind::ResolvesButGoRejects),
        (false, true, false) => Verdict::Incomplete,
        (false, true, true) => Verdict::OverAccept(OverAcceptKind::PickedOneWhereGoAmbiguous),
    }
}

fn compare_satisfies(go_satisfies: bool, lisette: &Outcome) -> Verdict {
    match (go_satisfies, lisette.resolves()) {
        (true, true) => Verdict::Match,
        (true, false) => Verdict::Incomplete,
        (false, true) => Verdict::OverAccept(OverAcceptKind::SatisfiesButGoDenies),
        (false, false) => Verdict::Match,
    }
}

fn selector_detail(go: &GoAnswer) -> String {
    if go.resolves {
        let type_token = go
            .resolved_type
            .as_ref()
            .map(|t| t.display())
            .unwrap_or_default();
        format!(
            "go: {} on {} depth={} indirect={} : {type_token}",
            go.member_kind, go.declaring_type, go.depth, go.indirect,
        )
    } else if go.ambiguous {
        "go: ambiguous".into()
    } else {
        "go: not found".into()
    }
}

fn gate_all(scenario: &Scenario, reason: &str) -> Vec<Comparison> {
    let count = scenario.questions.len().max(1);
    (0..count)
        .map(|i| Comparison {
            question_index: i,
            label: scenario.name.clone(),
            verdict: Verdict::Gate(reason.to_string()),
            detail: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;
    use crate::_embed_harness::go_answerer::{GoAnswerer, go_available};
    use crate::_embed_harness::lisette_answer::lisette_answers;

    fn answerer_or_skip() -> Option<GoAnswerer> {
        if !go_available() {
            eprintln!("skipping comparator test: `go` toolchain not found");
            return None;
        }
        Some(GoAnswerer::build().expect("answerer builds"))
    }

    fn only(scenario: &Scenario, answerer: &GoAnswerer) -> Verdict {
        let go = answerer.answer(scenario).unwrap();
        let lisette = lisette_answers(scenario);
        let comparisons = compare(scenario, &go, &lisette);
        comparisons[0].verdict.clone()
    }

    #[test]
    fn direct_method_matches() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        assert_eq!(only(&fixtures::direct_method(), &answerer), Verdict::Match);
    }

    #[test]
    fn promoted_method_is_incomplete() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        assert_eq!(
            only(&fixtures::value_embed_method(), &answerer),
            Verdict::Incomplete
        );
    }

    #[test]
    fn diamond_is_incomplete_not_over_accept() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        assert_eq!(only(&fixtures::diamond(), &answerer), Verdict::Incomplete);
    }

    #[test]
    fn direct_satisfaction_matches() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        assert_eq!(
            only(&fixtures::interface_direct_satisfaction(), &answerer),
            Verdict::Match
        );
    }

    #[test]
    fn promoted_satisfaction_is_incomplete() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        assert_eq!(
            only(&fixtures::interface_promoted_satisfaction(), &answerer),
            Verdict::Incomplete
        );
    }

    #[test]
    fn interface_conflict_matches_no_over_accept() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let scenario = fixtures::interface_conflict();
        let go = answerer.answer(&scenario).unwrap();
        let lisette = lisette_answers(&scenario);
        for comparison in compare(&scenario, &go, &lisette) {
            assert!(
                !comparison.verdict.is_over_accept(),
                "{}: unexpected over-accept ({:?})",
                comparison.label,
                comparison.verdict
            );
        }
    }

    #[test]
    fn no_over_acceptance_across_all_fixtures() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        for scenario in fixtures::all() {
            let go = answerer.answer(&scenario).unwrap();
            let lisette = lisette_answers(&scenario);
            for comparison in compare(&scenario, &go, &lisette) {
                assert!(
                    !comparison.verdict.is_over_accept() && !comparison.verdict.is_gate(),
                    "{} / {}: {:?} ({})",
                    scenario.name,
                    comparison.label,
                    comparison.verdict,
                    comparison.detail
                );
            }
        }
    }
}
