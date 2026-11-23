//! Compares `lis check` with Go's type checker, one family of Go rules per test.

#[allow(dead_code, unused_imports)]
mod _harness;

#[path = "go_rejections/constant_contexts.rs"]
mod constant_contexts;
#[path = "go_rejections/constants.rs"]
mod constants;
#[path = "go_rejections/embedded_types/mod.rs"]
mod embedded_types;
#[path = "go_rejections/interfaces.rs"]
mod interfaces;
#[path = "go_rejections/operators.rs"]
mod operators;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;

use _harness::go::emit_program;
use _harness::go::go_available;
use _harness::go::{GO_MODULE, prelude_dir, read_go_toolchain, repo_root, write_go_mod};
use _harness::go_types::{fresh_dir, panic_message, type_check_module};
use _harness::lists::FailingList;
use semantics::AnalysisScope;
use semantics::loader::MemoryLoader;
use semantics::store::ENTRY_PACKAGE_ID;

/// Go files by path relative to the package or project root.
pub type GoFiles = Vec<(String, String)>;

pub struct Case {
    pub id: String,
    /// Lisette files: `main.lis` alone is a script, otherwise `main.lis` and `<package>/<file>.lis`.
    pub files: Vec<(String, String)>,
    pub reference: Option<GoFiles>,
}

pub enum Compiled {
    Accepted(GoFiles),
    Rejected(String),
    Failed(String),
}

/// Placeholder module path in the emitted Go, replaced by the path of each case directory.
pub const CASE_MODULE: &str = "case.invalid/module";

pub fn compile_case(files: &[(String, String)]) -> Compiled {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let locator = deps::TypedefLocator::default();
        let (_, main) = files
            .iter()
            .find(|(path, _)| path == "main.lis")
            .expect("a case has main.lis");
        let mut loader = MemoryLoader::new();
        for (path, source) in files {
            match path.rsplit_once('/') {
                Some((folder, name)) => loader.add_file(folder, name, source),
                None => loader.add_file(ENTRY_PACKAGE_ID, path, source),
            }
        }
        let scope = if files.len() == 1 {
            AnalysisScope::Script {
                inside_project: false,
            }
        } else {
            AnalysisScope::Directory
        };
        emit_program(main, "main.lis", CASE_MODULE, scope, &loader, &locator)
    }));
    match result {
        Err(payload) => Compiled::Failed(format!("panic: {}", panic_message(payload.as_ref()))),
        Ok(result) => {
            if let Some(error) = result.errors.first() {
                let code = error.code_str().unwrap_or("no code").to_string();
                if code.starts_with("emit.") {
                    return Compiled::Failed(format!("emit rejected: {code}"));
                }
                return Compiled::Rejected(code);
            }
            if result.output.is_empty() {
                return Compiled::Failed("emit wrote no files".to_string());
            }
            Compiled::Accepted(
                result
                    .output
                    .iter()
                    .map(|file| (file.name.clone(), file.to_go()))
                    .collect(),
            )
        }
    }
}

pub struct Family {
    pub name: &'static str,
    /// Lisette rejections of programs that Go accepts, by Lisette policy.
    pub intended: &'static [Intended],
}

/// A Lisette code that may reject a case that Go accepts, except in cases whose id starts with one of `except`.
pub struct Intended {
    pub code: &'static str,
    pub except: &'static [&'static str],
}

impl Family {
    fn intends(&self, id: &str, code: &str) -> bool {
        self.intended.iter().any(|intended| {
            intended.code == code && !intended.except.iter().any(|prefix| id.starts_with(prefix))
        })
    }
}

enum Outcome {
    BothAccept,
    BothReject,
    NoReference,
    IntendedStrictness,
    Failure(String),
}

fn run_family(family: &Family, generate: fn() -> Vec<Case>) {
    if !go_available() {
        eprintln!("skipping {}: `go` not found", family.name);
        return;
    }
    let started = Instant::now();
    let cases = generate();
    assert!(!cases.is_empty(), "{}: no cases", family.name);
    let mut ids = BTreeSet::new();
    for case in &cases {
        assert!(
            !case.id.contains(char::is_whitespace) && !case.id.contains('#'),
            "case id `{}` must have no spaces and no `#`",
            case.id
        );
        assert!(
            ids.insert(case.id.clone()),
            "duplicate case id `{}`",
            case.id
        );
    }

    let module = repo_root().join("target/go_rejections").join(family.name);
    fresh_dir(&module);
    write_go_mod(&module, &prelude_dir(), &read_go_toolchain().unwrap()).unwrap();

    let compiled: Vec<Compiled> = cases
        .par_iter()
        .map(|case| compile_case(&case.files))
        .collect();
    let lisette_seconds = started.elapsed().as_secs_f64();

    for (index, (case, compiled)) in cases.iter().zip(&compiled).enumerate() {
        match compiled {
            Compiled::Accepted(go) => write_package(&module, &format!("e{index:05}"), go),
            Compiled::Rejected(_) => {
                if let Some(reference) = &case.reference {
                    write_package(&module, &format!("r{index:05}"), reference);
                }
            }
            Compiled::Failed(_) => {}
        }
    }
    let go_errors = type_check_module(&module);
    let first_error = |directory: &str| {
        go_errors
            .iter()
            .find(|(package, _)| package.split('/').next() == Some(directory))
            .map(|(_, errors)| errors[0].clone())
    };

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures = BTreeMap::new();
    for (index, (case, compiled)) in cases.iter().zip(compiled).enumerate() {
        let outcome = match compiled {
            Compiled::Failed(message) => Outcome::Failure(message),
            Compiled::Accepted(_) => match first_error(&format!("e{index:05}")) {
                None => Outcome::BothAccept,
                Some(error) => Outcome::Failure(format!("Lisette accepts, Go rejects: {error}")),
            },
            Compiled::Rejected(code) => {
                if case.reference.is_none() {
                    Outcome::NoReference
                } else if first_error(&format!("r{index:05}")).is_some() {
                    Outcome::BothReject
                } else if family.intends(&case.id, &code) {
                    Outcome::IntendedStrictness
                } else {
                    Outcome::Failure(format!("Lisette rejects with {code}, Go accepts"))
                }
            }
        };
        let label = match outcome {
            Outcome::BothAccept => "both accept",
            Outcome::BothReject => "both reject",
            Outcome::NoReference => "rejected, no reference Go",
            Outcome::IntendedStrictness => "rejected by Lisette policy",
            Outcome::Failure(reason) => {
                failures.insert(case.id.clone(), reason);
                "failure"
            }
        };
        *counts.entry(label).or_default() += 1;
    }

    eprintln!(
        "{}: {} cases in {:.1}s (Lisette {lisette_seconds:.1}s): {counts:?}",
        family.name,
        cases.len(),
        started.elapsed().as_secs_f64(),
    );
    FailingList::read(&format!("go_rejections/failing_{}.txt", family.name))
        .assert_matches(&ids, &failures, true);
}

/// Writes one Go program in its own directory and points its module paths at that directory.
fn write_package(module: &Path, directory: &str, files: &GoFiles) {
    let root = module.join(directory);
    let module_path = format!("{GO_MODULE}/{directory}");
    for (path, source) in files {
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, source.replace(CASE_MODULE, &module_path)).unwrap();
    }
}

#[test]
fn constants_match_go() {
    run_family(&constants::FAMILY, constants::cases);
}

#[test]
fn constant_contexts_match_go() {
    run_family(&constant_contexts::FAMILY, constant_contexts::cases);
}

#[test]
fn operators_match_go() {
    run_family(&operators::FAMILY, operators::cases);
}

#[test]
fn interfaces_match_go() {
    run_family(&interfaces::FAMILY, interfaces::cases);
}
