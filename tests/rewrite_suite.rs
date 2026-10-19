//! Rewrites each emit snapshot program in ways that keep its meaning, and
//! checks that each variant compiles to Go that builds and vets whenever the
//! original does. Lisette errors on a variant are ignored. Runs nightly.

#[allow(dead_code, unused_imports)]
mod _harness;
#[allow(dead_code)]
#[path = "e2e_suite/harness.rs"]
mod harness;

use std::any::Any;
use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::process::Command;

use rayon::prelude::*;
use syntax::ast::{Annotation, Expression, LetMode, Span};

use _harness::wrap::wrap;
use harness::{
    CompileError, compile_e2e_suite_test, harvest_snapshots, prelude_dir, read_go_toolchain,
    read_skip_list, repo_root, run_go_build, run_go_vet, skip_reason_for_imports, snapshots_dir,
    write_go_mod, write_subpackage,
};

const SITES_PER_REWRITE: usize = 3;

#[derive(Default)]
struct Sites {
    wrap: Vec<Range<usize>>,
    alias: Vec<Range<usize>>,
}

struct Variant {
    name: String,
    rewrite: &'static str,
    change: String,
    source: String,
}

enum Compiled {
    Written,
    Rejected,
    Failed(String),
}

#[test]
fn rewrite_suite() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping rewrite_suite: `go` not found");
        return;
    }

    let target = repo_root().join("target/rewrite_suite");
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create target/rewrite_suite");
    let go_toolchain = read_go_toolchain().expect("read go-toolchain");
    write_go_mod(&target, &prelude_dir(), &go_toolchain).expect("write go.mod");

    let harvested = harvest_snapshots(&snapshots_dir("emit"));
    assert!(!harvested.is_empty(), "no snapshots harvested");
    let skip_list = read_skip_list("rewrite_suite/skip.txt");

    let programs: Vec<(String, Vec<(Variant, Compiled)>)> = harvested
        .par_iter()
        .filter(|test| skip_reason_for_imports(&test.snap_body).is_none())
        .filter_map(|test| {
            let source = wrap(&test.input);
            let Compiled::Written = compile(&target, &test.name, &source) else {
                return None;
            };
            let variants = variants(&test.name, &source)
                .into_iter()
                .filter(|variant| !skip_list.contains(&variant.name))
                .map(|variant| {
                    let compiled = compile(&target, &variant.name, &variant.source);
                    (variant, compiled)
                })
                .collect();
            Some((test.name.clone(), variants))
        })
        .collect();

    let mut go_errors = HashMap::new();
    for result in [run_go_build(&target), run_go_vet(&target)] {
        if let Err(output) = result {
            assert!(
                record_first_errors(&output, &mut go_errors),
                "go failed outside any package:\n{output}"
            );
        }
    }

    let mut variant_count = 0;
    let mut rejected = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();
    for (name, variants) in &programs {
        if go_errors.contains_key(&format!("test_{name}")) {
            skipped += 1;
            continue;
        }
        for (variant, compiled) in variants {
            variant_count += 1;
            let error = match compiled {
                Compiled::Rejected => {
                    rejected += 1;
                    continue;
                }
                Compiled::Failed(message) => format!("compiler: {message}"),
                Compiled::Written => match go_errors.get(&format!("test_{}", variant.name)) {
                    Some(line) => format!("go: {line}"),
                    None => continue,
                },
            };
            failures.push(format!(
                "{} ({}): {}\n  {error}\n  repro: {}",
                variant.name,
                variant.rewrite,
                variant.change,
                target
                    .join(format!("test_{}", variant.name))
                    .join("test.lis")
                    .display(),
            ));
        }
    }

    eprintln!(
        "{} programs, {} variants, {} on skip list, {} rejected by Lisette, {} programs skipped (original fails go build or vet), {} failures",
        programs.len(),
        variant_count,
        skip_list.len(),
        rejected,
        skipped,
        failures.len(),
    );
    for failure in &failures {
        eprintln!("{failure}");
    }
    assert!(
        failures.is_empty(),
        "{} variant(s) crashed the compiler or failed go build or vet (listed above)",
        failures.len()
    );
}

fn compile(target: &Path, name: &str, source: &str) -> Compiled {
    let package_dir = target.join(format!("test_{name}"));
    fs::create_dir_all(&package_dir).expect("create package dir");
    fs::write(package_dir.join("test.lis"), source).expect("write test.lis");

    let result = catch_unwind(AssertUnwindSafe(|| {
        compile_e2e_suite_test(source, &format!("test_{name}"))
    }));
    match result {
        Ok(Ok(emitted)) => {
            write_subpackage(target, name, &emitted.go_code, None).expect("write subpackage");
            Compiled::Written
        }
        Ok(Err(CompileError::Inference(_))) => Compiled::Rejected,
        Ok(Err(CompileError::Emit(message))) => Compiled::Failed(message),
        Err(payload) => Compiled::Failed(panic_message(payload)),
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => format!("panic: {message}"),
        Err(payload) => match payload.downcast::<&str>() {
            Ok(message) => format!("panic: {message}"),
            Err(_) => "panic".to_string(),
        },
    }
}

fn variants(name: &str, source: &str) -> Vec<Variant> {
    let result = syntax::build_ast(source, 0);
    if result.has_errors() {
        return Vec::new();
    }
    let mut sites = Sites::default();
    for item in &result.ast {
        visit(item, source, &mut Vec::new(), &mut sites);
    }

    let mut variants = Vec::new();
    for (i, range) in pick(&sites.wrap).enumerate() {
        let expression = &source[range.clone()];
        let replacement = format!("{{ {expression} }}");
        variants.push(Variant {
            name: format!("{name}__wrap{}", i + 1),
            rewrite: "wrap",
            change: describe(source, range, &replacement),
            source: splice(source, range, &replacement),
        });
    }
    for (i, range) in pick(&sites.alias).enumerate() {
        let annotation = &source[range.clone()];
        let declaration = format!("type MmAlias = {annotation}");
        variants.push(Variant {
            name: format!("{name}__alias{}", i + 1),
            rewrite: "alias",
            change: format!(
                "{}, plus `{declaration}`",
                describe(source, range, "MmAlias")
            ),
            source: format!("{}\n{declaration}\n", splice(source, range, "MmAlias")),
        });
    }
    variants.retain(|variant| !syntax::build_ast(&variant.source, 0).has_errors());
    variants
}

fn visit<'a>(
    expression: &'a Expression,
    source: &str,
    generics: &mut Vec<&'a str>,
    sites: &mut Sites,
) {
    let mark = generics.len();
    match expression {
        Expression::ImplBlock { generics: own, .. } => {
            generics.extend(own.iter().map(|generic| generic.name.as_str()));
        }
        Expression::Function {
            generics: own,
            params,
            return_annotation,
            ..
        } => {
            generics.extend(own.iter().map(|generic| generic.name.as_str()));
            for annotation in params.iter().filter_map(|param| param.annotation.as_ref()) {
                add_alias_site(annotation, source, generics, sites);
            }
            add_alias_site(return_annotation, source, generics, sites);
        }
        Expression::Lambda {
            params,
            return_annotation,
            ..
        } => {
            for annotation in params.iter().filter_map(|param| param.annotation.as_ref()) {
                add_alias_site(annotation, source, generics, sites);
            }
            add_alias_site(return_annotation, source, generics, sites);
        }
        Expression::Let {
            binding,
            value,
            mode,
            ..
        } => {
            if matches!(mode, LetMode::Plain) {
                sites.wrap.push(range_of(value.get_span()));
            }
            if let Some(annotation) = &binding.annotation {
                add_alias_site(annotation, source, generics, sites);
            }
        }
        _ => {}
    }
    for child in expression.children() {
        visit(child, source, generics, sites);
    }
    generics.truncate(mark);
}

fn add_alias_site(annotation: &Annotation, source: &str, generics: &[&str], sites: &mut Sites) {
    let range = range_of(annotation.get_span());
    let Some(text) = source.get(range.clone()) else {
        return;
    };
    let mentions_generic = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| generics.contains(&word));
    if !text.is_empty() && !text.starts_with("mut ") && !mentions_generic {
        sites.alias.push(range);
    }
}

fn range_of(span: Span) -> Range<usize> {
    let start = span.byte_offset as usize;
    start..start + span.byte_length as usize
}

fn pick(sites: &[Range<usize>]) -> impl Iterator<Item = &Range<usize>> {
    let count = sites.len().min(SITES_PER_REWRITE);
    (0..count).map(move |i| &sites[i * sites.len() / count])
}

fn splice(source: &str, range: &Range<usize>, replacement: &str) -> String {
    format!(
        "{}{replacement}{}",
        &source[..range.start],
        &source[range.end..]
    )
}

fn describe(source: &str, range: &Range<usize>, replacement: &str) -> String {
    let line = source[..range.start].matches('\n').count() + 1;
    format!(
        "line {line}: `{}` became `{replacement}`",
        &source[range.clone()]
    )
}

/// Maps each failing package directory to its first error line in `go` output.
fn record_first_errors(output: &str, errors: &mut HashMap<String, String>) -> bool {
    let mut found = false;
    for line in output.lines() {
        let path = line.strip_prefix("vet: ").unwrap_or(line);
        let path = path.strip_prefix("./").unwrap_or(path);
        let Some((package, _)) = path.split_once('/') else {
            continue;
        };
        if package.starts_with("test_") {
            found = true;
            errors
                .entry(package.to_string())
                .or_insert_with(|| line.to_string());
        }
    }
    found
}
