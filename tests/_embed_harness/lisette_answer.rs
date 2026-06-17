//! Lisette's side of the comparison. Renders a graph to Lisette, runs the real
//! type checker, and decides for each question whether Lisette accepted it by
//! seeing which error diagnostics land inside that question's function. Reads only
//! public diagnostic codes, so it survives changes to the checker's internals.

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use passes::analyze;
use semantics::inference::{AnalyzeInput, CompilePhase, SemanticConfig};
use semantics::loader::MemoryLoader;
use semantics::store::ENTRY_MODULE_ID;

use super::render_lis::{QuestionSpans, render_lis_declarations, render_lis_questions};
use super::scenario::Scenario;

const ENTRY_FILE_ID: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Resolves,
    Rejects { codes: Vec<String> },
}

impl Outcome {
    pub fn resolves(&self) -> bool {
        matches!(self, Outcome::Resolves)
    }

    pub fn has_code(&self, code: &str) -> bool {
        matches!(self, Outcome::Rejects { codes } if codes.iter().any(|c| c == code))
    }

    pub fn is_ambiguous(&self) -> bool {
        self.has_code("infer.ambiguous_selector")
    }
}

#[derive(Debug, Clone)]
pub enum LisetteAnswer {
    Selector(Outcome),
    Satisfies { value: Outcome, pointer: Outcome },
}

#[derive(Debug, Clone)]
pub struct LisetteAnswers {
    /// The rendered Lisette failed to parse: a renderer bug.
    pub parse_failed: bool,
    /// Error codes outside every question span (declaration- or sink-level);
    /// expected empty.
    pub unattributed: Vec<String>,
    /// Aligned 1:1 with `scenario.questions`.
    pub questions: Vec<LisetteAnswer>,
}

pub fn lisette_answers(scenario: &Scenario) -> LisetteAnswers {
    let rendered = render_lis_questions(scenario);
    let checked = check(&rendered.source);
    if checked.parse_failed {
        return LisetteAnswers {
            parse_failed: true,
            unattributed: vec![],
            questions: vec![],
        };
    }

    let errors: Vec<(usize, String)> = checked
        .errors
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| {
            d.location_offset()
                .map(|offset| (offset, d.code_str().unwrap_or("").to_string()))
        })
        .collect();
    let mut attributed = vec![false; errors.len()];

    let questions = rendered
        .spans
        .iter()
        .map(|span| match span {
            QuestionSpans::Selector { range } => {
                LisetteAnswer::Selector(outcome(codes_in(&errors, *range, &mut attributed)))
            }
            QuestionSpans::Satisfies { value, pointer } => LisetteAnswer::Satisfies {
                value: outcome(codes_in(&errors, *value, &mut attributed)),
                pointer: outcome(codes_in(&errors, *pointer, &mut attributed)),
            },
        })
        .collect();

    let unattributed = errors
        .iter()
        .zip(&attributed)
        .filter(|(_, done)| !**done)
        .map(|((_, code), _)| code.clone())
        .collect();

    LisetteAnswers {
        parse_failed: false,
        unattributed,
        questions,
    }
}

/// Does the declarations-only rendering register without errors? A failure is a
/// renderer bug (a decls-only file has no member accesses to resolve).
pub fn declarations_register_cleanly(scenario: &Scenario) -> Result<(), Vec<String>> {
    let source = render_lis_declarations(scenario);
    let checked = check(&source);
    if checked.parse_failed {
        return Err(vec!["parse failed".to_string()]);
    }
    let codes: Vec<String> = checked
        .errors
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code_str().unwrap_or("<no code>").to_string())
        .collect();
    if codes.is_empty() { Ok(()) } else { Err(codes) }
}

/// Check a raw Lisette source and return its error codes (`None` if it compiles
/// cleanly). Used by the corpus reject cases.
pub fn check_codes(source: &str) -> Option<Vec<String>> {
    let checked = check(source);
    if checked.parse_failed {
        return Some(vec!["parse.failed".to_string()]);
    }
    let codes: Vec<String> = checked
        .errors
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code_str().unwrap_or("<no code>").to_string())
        .collect();
    if codes.is_empty() { None } else { Some(codes) }
}

struct Checked {
    parse_failed: bool,
    errors: Vec<LisetteDiagnostic>,
}

/// Compile a self-contained Lisette source through the real checker.
fn check(source: &str) -> Checked {
    let build = syntax::build_ast(source, ENTRY_FILE_ID);
    if build.failed() {
        return Checked {
            parse_failed: true,
            errors: vec![],
        };
    }

    let mut loader = MemoryLoader::new();
    loader.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let output = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: false,
            standalone_mode: false,
            load_siblings: true,
        },
        loader: &loader,
        source: source.to_string(),
        filename: "main.lis".to_string(),
        display_path: "main.lis".to_string(),
        ast: build.ast,
        project_root: None,
        locator: TypedefLocator::default(),
        compile_phase: CompilePhase::Check,
        go_module: String::new(),
        disable_cache: true,
    });

    Checked {
        parse_failed: false,
        errors: output.result.errors,
    }
}

fn codes_in(
    errors: &[(usize, String)],
    range: (usize, usize),
    attributed: &mut [bool],
) -> Vec<String> {
    let mut codes = Vec::new();
    for (i, (offset, code)) in errors.iter().enumerate() {
        if *offset >= range.0 && *offset < range.1 {
            attributed[i] = true;
            codes.push(code.clone());
        }
    }
    codes
}

fn outcome(codes: Vec<String>) -> Outcome {
    if codes.is_empty() {
        Outcome::Resolves
    } else {
        Outcome::Rejects { codes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;

    fn selector_outcome(scenario: &Scenario) -> Outcome {
        match &lisette_answers(scenario).questions[0] {
            LisetteAnswer::Selector(outcome) => outcome.clone(),
            other => panic!("expected a selector answer, got {other:?}"),
        }
    }

    fn satisfies_value(scenario: &Scenario) -> Outcome {
        match &lisette_answers(scenario).questions[0] {
            LisetteAnswer::Satisfies { value, .. } => value.clone(),
            other => panic!("expected a satisfies answer, got {other:?}"),
        }
    }

    #[test]
    fn direct_method_resolves() {
        assert_eq!(
            selector_outcome(&fixtures::direct_method()),
            Outcome::Resolves
        );
    }

    #[test]
    fn promoted_method_resolves() {
        assert_eq!(
            selector_outcome(&fixtures::value_embed_method()),
            Outcome::Resolves,
            "a value-embedded method must promote"
        );
    }

    #[test]
    fn diamond_selector_is_ambiguous() {
        let outcome = selector_outcome(&fixtures::diamond());
        assert!(
            outcome.is_ambiguous(),
            "diamond promotion must report ambiguity, got {outcome:?}"
        );
    }

    #[test]
    fn direct_satisfaction_resolves() {
        assert_eq!(
            satisfies_value(&fixtures::interface_direct_satisfaction()),
            Outcome::Resolves
        );
    }

    #[test]
    fn promoted_satisfaction_resolves() {
        assert_eq!(
            satisfies_value(&fixtures::interface_promoted_satisfaction()),
            Outcome::Resolves,
            "a value embedding the declarer must satisfy the interface"
        );
    }

    #[test]
    fn all_fixtures_have_clean_decls() {
        for scenario in fixtures::all() {
            declarations_register_cleanly(&scenario).unwrap_or_else(|codes| {
                panic!("{}: decls did not register: {codes:?}", scenario.name)
            });
        }
    }

    #[test]
    fn every_basic_type_is_a_valid_newtype() {
        use crate::_embed_harness::scenario::*;
        for basic in BasicType::ALL {
            let scenario = Scenario {
                name: format!("newtype_{}", basic.lisette()),
                seed: 0,
                nodes: vec![Node {
                    id: 0,
                    name: "N0".into(),
                    type_params: vec![],
                    kind: NodeKind::NamedBasic {
                        underlying: basic,
                        methods: vec![Method {
                            name: "M".into(),
                            receiver: Receiver::Value,
                            signature: Signature {
                                parameters: vec![],
                                return_type: MemberType::Basic(BasicType::String),
                            },
                            visibility: Visibility::Public,
                        }],
                    },
                    origin: Origin::Native,
                }],
                questions: vec![],
            };
            declarations_register_cleanly(&scenario)
                .unwrap_or_else(|codes| panic!("newtype over `{}`: {codes:?}", basic.lisette()));
        }
    }

    /// A duplicate-method interface is rejected at registration, so Lisette does
    /// not over-accept it; Go denies satisfaction too, so they agree.
    #[test]
    fn interface_conflict_is_caught_at_registration() {
        let scenario = fixtures::interface_conflict();
        let decls = declarations_register_cleanly(&scenario);
        assert!(
            matches!(&decls, Err(codes) if codes.iter().any(|c| c == "infer.interface_method_conflict")),
            "expected interface_method_conflict at registration, got {decls:?}"
        );
        match &lisette_answers(&scenario).questions[0] {
            LisetteAnswer::Satisfies { value, .. } => {
                assert!(
                    !value.resolves(),
                    "conflicted interface must not be satisfied"
                );
            }
            other => panic!("expected a satisfies answer, got {other:?}"),
        }
    }

    #[test]
    fn no_unattributed_errors_on_fixtures() {
        for scenario in fixtures::all() {
            let v = lisette_answers(&scenario);
            assert!(!v.parse_failed, "{}: parse failed", scenario.name);
            assert!(
                v.unattributed.is_empty(),
                "{}: unattributed errors {:?}",
                scenario.name,
                v.unattributed
            );
        }
    }
}
