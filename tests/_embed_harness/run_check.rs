use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use deps::TypedefLocator;
use emit::{EmitOptions, Planner};
use passes::analyze;
use semantics::inference::{AnalyzeInput, CompilePhase, SemanticConfig};
use semantics::loader::MemoryLoader;
use semantics::store::ENTRY_MODULE_ID;

use super::PrintedQuestion;
use super::go_answerer::{GoAnswer, GoAnswers};
use super::lisette_answer::{LisetteAnswer, LisetteAnswers};
use super::render_go::{GoMode, render_go};
use super::render_lis::render_lis_run;
use super::scenario::{
    EdgeKind, MemberType, NodeId, NodeKind, Question, Receiver, Scenario, Visibility,
};

const ENTRY_FILE_ID: u32 = 0;
const PRELUDE_IMPORT_PATH: &str = "github.com/ivov/lisette/prelude";

#[derive(Debug)]
pub enum RunOutcome {
    /// No question was eligible to run on this scenario yet.
    Skipped,
    /// Emitted Go and hand-written Go produced identical stdout.
    Match,
    /// A divergence or failure; the message explains.
    Divergence(String),
}

pub fn run_check(scenario: &Scenario, go: &GoAnswers, lisette: &LisetteAnswers) -> RunOutcome {
    let printed = runnable_questions(scenario, go, lisette);
    if printed.is_empty() {
        return RunOutcome::Skipped;
    }

    let work_dir = match tempfile::TempDir::new() {
        Ok(dir) => dir,
        Err(err) => return RunOutcome::Divergence(format!("temp dir: {err}")),
    };
    let work = work_dir.path();

    let emitted = match emit_and_write(scenario, &printed, work) {
        Ok(Emit::Done(out)) => out,
        Ok(Emit::NotRunnable(_codes)) => return RunOutcome::Skipped,
        Err(err) => return RunOutcome::Divergence(format!("emit io: {err}")),
    };
    let handwritten = match write_handwritten(scenario, &printed, work) {
        Ok(out) => out,
        Err(err) => return RunOutcome::Divergence(format!("handwritten: {err}")),
    };

    let emitted_out = match go_run(&emitted) {
        Ok(out) => out,
        Err(err) => return RunOutcome::Divergence(format!("`go run` (emitted): {err}")),
    };
    let handwritten_out = match go_run(&handwritten) {
        Ok(out) => out,
        Err(err) => return RunOutcome::Divergence(format!("`go run` (handwritten): {err}")),
    };

    if emitted_out == handwritten_out {
        RunOutcome::Match
    } else {
        RunOutcome::Divergence(format!(
            "stdout differs:\n  emitted:     {emitted_out:?}\n  handwritten: {handwritten_out:?}"
        ))
    }
}

/// Selector questions whose method both sides resolve and that are safe to call
/// on a zero-valued receiver (see `safe_to_run`).
fn runnable_questions(
    scenario: &Scenario,
    go: &GoAnswers,
    lisette: &LisetteAnswers,
) -> Vec<PrintedQuestion> {
    let mut printed = Vec::new();
    for (i, question) in scenario.questions.iter().enumerate() {
        let Question::Selector { root, member, .. } = question else {
            continue;
        };
        let expected = &go.results[i];
        let lisette_resolves =
            matches!(&lisette.questions[i], LisetteAnswer::Selector(o) if o.resolves());
        if expected.resolves
            && expected.member_kind == "method"
            && safe_to_run(scenario, expected)
            && lisette_resolves
            && is_zero_constructible(scenario, *root)
        {
            printed.push(PrintedQuestion {
                root: *root,
                member: member.clone(),
                method_expression: runs_as_method_expression(scenario, *root, expected, member),
            });
        }
    }
    printed
}

/// Whether the `Root.member` method-expression form is safe to render and run:
/// a public value-receiver method whose root and declaring type are non-generic,
/// so `Root.member` needs no type arguments and `g(r)` needs no addressing.
fn runs_as_method_expression(
    scenario: &Scenario,
    root: NodeId,
    expected: &GoAnswer,
    member: &str,
) -> bool {
    if !scenario.node(root).type_params.is_empty() {
        return false;
    }
    scenario.nodes.iter().any(|node| {
        node.name == expected.declaring_type
            && node.type_params.is_empty()
            && matches!(&node.kind, NodeKind::Struct { methods, .. }
            if methods.iter().any(|method| {
                method.name == member
                    && matches!(method.receiver, Receiver::Value)
                    && matches!(method.visibility, Visibility::Public)
            }))
    })
}

/// A promoted (depth > 0) method runs only with no pointer indirection and a
/// struct declaring type, so dispatch never hits a nil pointer or interface.
fn safe_to_run(scenario: &Scenario, expected: &GoAnswer) -> bool {
    if expected.depth == 0 {
        return true;
    }
    !expected.indirect
        && scenario.nodes.iter().any(|node| {
            node.name == expected.declaring_type && matches!(node.kind, NodeKind::Struct { .. })
        })
}

/// Whether this node be built as a zero value.
fn is_zero_constructible(scenario: &Scenario, id: NodeId) -> bool {
    match &scenario.node(id).kind {
        NodeKind::NamedBasic { .. } => true,
        NodeKind::Interface { .. } => false,
        NodeKind::Struct { fields, embeds, .. } => {
            fields.iter().all(|field| zeroable(&field.member_type))
                && embeds.iter().all(|embed| match embed.edge {
                    EdgeKind::Pointer => false,
                    EdgeKind::Value => {
                        matches!(scenario.node(embed.target).kind, NodeKind::Interface { .. })
                            || is_zero_constructible(scenario, embed.target)
                    }
                })
        }
    }
}

fn zeroable(member_type: &MemberType) -> bool {
    match member_type {
        MemberType::Ref(_) | MemberType::Node(_) | MemberType::TypeParam(_) => false,
        MemberType::Basic(_) | MemberType::Slice(_) | MemberType::Option(_) => true,
    }
}

struct GoDir {
    path: PathBuf,
}

enum Emit {
    Done(GoDir),
    /// The rendered Lisette did not type-check, with the error codes seen.
    NotRunnable(Vec<String>),
}

fn emit_and_write(
    scenario: &Scenario,
    printed: &[PrintedQuestion],
    work: &Path,
) -> Result<Emit, String> {
    let module = format!("lisette/embed_emitted_{}", scenario.name);
    let lisette = render_lis_run(scenario, printed);
    let build = syntax::build_ast(&lisette, ENTRY_FILE_ID);
    if build.failed() {
        return Ok(Emit::NotRunnable(vec!["parse".to_string()]));
    }

    let mut loader = MemoryLoader::new();
    loader.add_file(ENTRY_MODULE_ID, "main.lis", &lisette);
    let output = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: false,
            standalone_mode: false,
            load_siblings: true,
        },
        loader: &loader,
        source: lisette.clone(),
        filename: "main.lis".to_string(),
        display_path: "main.lis".to_string(),
        ast: build.ast,
        project_root: None,
        locator: TypedefLocator::default(),
        compile_phase: CompilePhase::Emit,
        go_module: module.clone(),
        disable_cache: true,
    });
    let result = output.result;
    if !result.errors.is_empty() {
        let codes = result
            .errors
            .iter()
            .filter_map(|e| e.code_str().map(str::to_string))
            .collect();
        return Ok(Emit::NotRunnable(codes));
    }

    let files = Planner::emit(
        &result.into_emit_input(),
        &module,
        EmitOptions { sourcemap: false },
    );
    let dir = work.join("emitted");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for file in &files {
        fs::write(dir.join(&file.name), file.to_go()).map_err(|e| e.to_string())?;
    }
    write_go_mod(&dir, &module, true)?;
    Ok(Emit::Done(GoDir { path: dir }))
}

fn write_handwritten(
    scenario: &Scenario,
    printed: &[PrintedQuestion],
    work: &Path,
) -> Result<GoDir, String> {
    let module = format!("lisette/embed_hand_{}", scenario.name);
    let go = render_go(scenario, GoMode::RunMain(printed));
    let dir = work.join("handwritten");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("main.go"), go).map_err(|e| e.to_string())?;
    write_go_mod(&dir, &module, false)?;
    Ok(GoDir { path: dir })
}

fn go_run(dir: &GoDir) -> Result<String, String> {
    let output = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir.path)
        .env("NO_COLOR", "1")
        .output()
        .map_err(|e| format!("spawn `go run`: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn write_go_mod(dir: &Path, module: &str, with_prelude: bool) -> Result<(), String> {
    let go_version = go_version();
    let mut content = format!("module {module}\n\ngo {go_version}\n");
    if with_prelude {
        let prelude = prelude_dir()
            .canonicalize()
            .map_err(|e| format!("canonicalize prelude: {e}"))?;
        content.push_str(&format!(
            "\nrequire {PRELUDE_IMPORT_PATH} v0.0.0\n\nreplace {PRELUDE_IMPORT_PATH} => {}\n",
            prelude.display()
        ));
    }
    fs::write(dir.join("go.mod"), content).map_err(|e| e.to_string())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate has a parent")
        .to_path_buf()
}

fn prelude_dir() -> PathBuf {
    repo_root().join("prelude")
}

fn go_version() -> String {
    let raw = fs::read_to_string(repo_root().join("go-version")).unwrap_or_else(|_| "1.25".into());
    let trimmed = raw.trim();
    let mut parts = trimmed.split('.');
    let major = parts.next().unwrap_or("1");
    let minor = parts.next().unwrap_or("25");
    format!("{major}.{minor}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;
    use crate::_embed_harness::go_answerer::{GoAnswerer, go_available};
    use crate::_embed_harness::lisette_answer::lisette_answers;

    #[test]
    fn direct_method_runs_identically() {
        if !go_available() {
            eprintln!("skipping build-arm test: `go` toolchain not found");
            return;
        }
        let answerer = GoAnswerer::build().expect("answerer builds");
        let scenario = fixtures::direct_method();
        let go = answerer.answer(&scenario).unwrap();
        let lisette = lisette_answers(&scenario);
        match run_check(&scenario, &go, &lisette) {
            RunOutcome::Match => {}
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn promoted_value_method_runs_identically() {
        if !go_available() {
            return;
        }
        let answerer = GoAnswerer::build().expect("answerer builds");
        let scenario = fixtures::value_embed_method();
        let go = answerer.answer(&scenario).unwrap();
        let lisette = lisette_answers(&scenario);
        match run_check(&scenario, &go, &lisette) {
            RunOutcome::Match => {}
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn pointer_embed_method_skips() {
        if !go_available() {
            return;
        }
        let answerer = GoAnswerer::build().expect("answerer builds");
        let scenario = fixtures::pointer_embed_method();
        let go = answerer.answer(&scenario).unwrap();
        let lisette = lisette_answers(&scenario);
        assert!(matches!(
            run_check(&scenario, &go, &lisette),
            RunOutcome::Skipped
        ));
    }
}
