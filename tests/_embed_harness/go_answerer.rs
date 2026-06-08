//! Asks Go's own type checker (the `go/types` package, via a small Go program
//! in `tests/embed_go_answerer`) for the correct answer to each question. The rest
//! of the harness compares Lisette against these answers.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::render_go::{GoMode, render_go};
use super::scenario::{Question, Scenario};

#[derive(Serialize)]
struct Request<'a> {
    #[serde(rename = "goSource")]
    go_source: &'a str,
    questions: Vec<GoQuestion>,
}

/// A flat question matching the Go side; irrelevant fields are sent empty.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoQuestion {
    pub kind: String,
    pub root: String,
    pub member: String,
    pub type_name: String,
    pub interface: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GoAnswers {
    #[serde(default)]
    pub fatal_error: Option<String>,
    /// Type-checker errors (e.g. a deliberate `duplicate method`).
    #[serde(default)]
    pub type_errors: Vec<String>,
    pub results: Vec<GoAnswer>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GoAnswer {
    #[serde(default)]
    pub resolves: bool,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub member_kind: String,
    #[serde(default)]
    pub declaring_type: String,
    #[serde(default)]
    pub depth: i64,
    #[serde(default)]
    pub indirect: bool,
    #[serde(default)]
    pub resolved_type: Option<CanonicalType>,
    #[serde(default)]
    pub satisfies_value: bool,
    #[serde(default)]
    pub satisfies_pointer: bool,
}

/// The answerer's language-neutral type token (mirrors the Go `CanonicalType`).
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalType {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub element: Option<Box<CanonicalType>>,
    #[serde(default)]
    pub parameters: Vec<CanonicalType>,
    #[serde(default)]
    pub return_type: Option<Box<CanonicalType>>,
}

impl CanonicalType {
    /// A compact, human-readable rendering for triage reports.
    pub fn display(&self) -> String {
        match self.kind.as_str() {
            "basic" | "named" => self.name.clone(),
            "ref" => format!("*{}", child(&self.element)),
            "slice" => format!("[]{}", child(&self.element)),
            "func" => {
                let parameters: Vec<String> =
                    self.parameters.iter().map(CanonicalType::display).collect();
                format!(
                    "fn({}) -> {}",
                    parameters.join(", "),
                    child(&self.return_type)
                )
            }
            other => format!("<{other}>"),
        }
    }
}

fn child(node: &Option<Box<CanonicalType>>) -> String {
    node.as_ref().map(|c| c.display()).unwrap_or_default()
}

/// Is the Go toolchain available? Tests and the runner skip gracefully if not.
pub fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub struct GoAnswerer {
    binary: PathBuf,
}

impl GoAnswerer {
    /// Build the answerer binary and reuse it across queries. The `go build`
    /// runs exactly once per process even when many tests call this in parallel,
    /// so they do not race on the same output path.
    pub fn build() -> Result<GoAnswerer, String> {
        static BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();
        let binary = BINARY.get_or_init(build_binary).clone()?;
        Ok(GoAnswerer { binary })
    }

    pub fn query(&self, go_source: &str, questions: Vec<GoQuestion>) -> Result<GoAnswers, String> {
        let payload = serde_json::to_vec(&Request {
            go_source,
            questions,
        })
        .map_err(|e| format!("serialize answerer request: {e}"))?;

        let mut child = Command::new(&self.binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn answerer: {e}"))?;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(&payload)
            .map_err(|e| format!("write answerer stdin: {e}"))?;
        let output = child
            .wait_with_output()
            .map_err(|e| format!("wait for answerer: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "answerer exited non-zero: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|e| {
            format!(
                "parse answerer response: {e}\nstdout: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    /// Render the scenario to Go and ask the answerer about each of its questions.
    pub fn answer(&self, scenario: &Scenario) -> Result<GoAnswers, String> {
        let go = render_go(scenario, GoMode::TypeDefs);
        self.query(&go, go_questions(scenario))
    }
}

pub fn go_questions(scenario: &Scenario) -> Vec<GoQuestion> {
    scenario
        .questions
        .iter()
        .map(|question| match question {
            Question::Selector { root, member, .. } => GoQuestion {
                kind: "selector".into(),
                root: scenario.node_name(*root).into(),
                member: member.clone(),
                type_name: String::new(),
                interface: String::new(),
            },
            Question::Satisfies { type_id, interface } => GoQuestion {
                kind: "satisfies".into(),
                root: String::new(),
                member: String::new(),
                type_name: scenario.node_name(*type_id).into(),
                interface: scenario.node_name(*interface).into(),
            },
        })
        .collect()
}

fn build_binary() -> Result<PathBuf, String> {
    let binary = answerer_binary_path();
    if let Some(parent) = binary.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create answerer target dir: {e}"))?;
    }
    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&binary)
        .arg(".")
        .current_dir(answerer_src_dir())
        .output()
        .map_err(|e| format!("spawn `go build`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "answerer `go build` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(binary)
}

fn answerer_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("embed_go_answerer")
}

fn answerer_binary_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate has a parent")
        .join("target/embed_go_answerer/answerer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;

    fn answerer_or_skip() -> Option<GoAnswerer> {
        if !go_available() {
            eprintln!("skipping answerer test: `go` toolchain not found");
            return None;
        }
        Some(GoAnswerer::build().expect("answerer builds"))
    }

    #[test]
    fn direct_method_resolves_at_depth_zero() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let response = answerer.answer(&fixtures::direct_method()).unwrap();
        let sel = &response.results[0];
        assert!(sel.resolves, "direct method should resolve");
        assert_eq!(sel.member_kind, "method");
        assert_eq!(sel.depth, 0);
        assert_eq!(sel.declaring_type, "N0");
    }

    #[test]
    fn value_embed_promotes_at_depth_one() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let response = answerer.answer(&fixtures::value_embed_method()).unwrap();
        let sel = &response.results[0];
        assert!(sel.resolves);
        assert_eq!(sel.depth, 1);
        assert_eq!(sel.declaring_type, "N0");
    }

    #[test]
    fn diamond_selector_is_ambiguous() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let response = answerer.answer(&fixtures::diamond()).unwrap();
        let sel = &response.results[0];
        assert!(!sel.resolves, "an ambiguous selector does not resolve");
        assert!(sel.ambiguous, "diamond `N3.M` is ambiguous in Go");
    }

    #[test]
    fn pointer_embed_promotes() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let response = answerer.answer(&fixtures::pointer_embed_method()).unwrap();
        let sel = &response.results[0];
        assert!(sel.resolves);
        assert_eq!(sel.depth, 1);
    }

    #[test]
    fn satisfaction_direct_and_promoted() {
        let Some(answerer) = answerer_or_skip() else {
            return;
        };
        let direct = answerer
            .answer(&fixtures::interface_direct_satisfaction())
            .unwrap();
        assert!(direct.results[0].satisfies_value, "direct satisfaction");

        let promoted = answerer
            .answer(&fixtures::interface_promoted_satisfaction())
            .unwrap();
        assert!(
            promoted.results[0].satisfies_value,
            "satisfaction through a promoted method"
        );
    }
}
