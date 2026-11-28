#[allow(dead_code, unused_imports)]
mod _harness;

use _harness::go::go_available;

#[path = "go_equivalents/widened_subjects.rs"]
mod widened_subjects;

use std::any::Any;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::Command;

use rayon::prelude::*;
use syntax::ast::{Annotation, Expression, LetMode, Span, UnaryOperator};
use syntax::program::{DotAccessResolution, ReceiverCoercion, strip_go_prefix};
use syntax::types::Type;

use _harness::go::{
    CompileError, compile_go_test, harvest_snapshots, prelude_dir, read_go_toolchain, repo_root,
    run_go_build, run_go_vet, skip_reason_for_imports, snapshots_dir, write_go_mod,
    write_subpackage,
};
use _harness::go_types::{build_go_helper, fresh_dir};
use _harness::markdown::{backticks_around, code_span};
use _harness::pipeline::TestPipeline;
use _harness::wrap::wrap;

const SITES_PER_REWRITE: usize = 3;
const REPORT_LIMIT: usize = 15_000;

#[derive(Default)]
struct Sites {
    wrap: Vec<Range<usize>>,
    paren: Vec<Range<usize>>,
    alias: Vec<Range<usize>>,
    reference: Vec<(Range<usize>, String, bool)>,
}

impl Sites {
    fn add_place_read(&mut self, value: &Expression, source: &str) {
        if is_place(value) {
            let range = range_of(value.get_span());
            if let Some(text) = source.get(range.clone()) {
                let replacement = format!("(&{text}).*");
                self.reference.push((range, replacement, true));
            }
        }
    }

    fn add_value(&mut self, value: &Expression) {
        let range = range_of(value.get_span());
        if !range.is_empty() {
            self.wrap.push(range.clone());
            self.paren.push(range);
        }
    }
}

struct Variant {
    name: String,
    rewrite: &'static str,
    change: String,
    source: String,
}

struct Program {
    name: String,
    go_code: String,
    variants: Vec<(Variant, Compiled)>,
}

struct Failure<'a> {
    program: &'a str,
    variant: &'a Variant,
    problem: Problem,
    repro: PathBuf,
}

enum Problem {
    Compiler(String),
    Go(String),
    DifferentGo { original: String, variant: String },
}

impl Problem {
    fn summary(&self) -> String {
        match self {
            Self::Compiler(message) => format!("compiler: {message}"),
            Self::Go(line) => format!("go: {line}"),
            Self::DifferentGo { original, variant } => format!(
                "Go differs from the Go of the original program\n    original: {original}\n    variant:  {variant}"
            ),
        }
    }

    fn markdown(&self) -> (&'static str, String) {
        match self {
            Self::Compiler(message) => (
                "Panic",
                code_span(
                    message
                        .strip_prefix("panic: ")
                        .unwrap_or(message)
                        .lines()
                        .next()
                        .unwrap_or_default(),
                ),
            ),
            Self::Go(line) => ("Build error", code_span(go_error_message(line))),
            Self::DifferentGo { original, variant } => {
                let lines = format!("- {original}\n+ {variant}");
                let fence = backticks_around(&lines, 3);
                ("Changed Go", format!("{fence}diff\n{lines}\n{fence}"))
            }
        }
    }
}

fn go_error_message(line: &str) -> &str {
    line.find(".go:")
        .and_then(|at| line[at..].split_once(": "))
        .map_or(line, |(_, message)| message)
}

enum Compiled {
    Emitted(String),
    Rejected,
    Failed(String),
}

#[test]
fn equivalent_programs_give_valid_go() {
    if !go_available() {
        eprintln!("skipping equivalent_programs_give_valid_go: `go` not found");
        return;
    }

    let target = repo_root().join("target/go_emit_equivalents");
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create target/go_emit_equivalents");
    let go_toolchain = read_go_toolchain().expect("read go-toolchain");
    write_go_mod(&target, &prelude_dir(), &go_toolchain).expect("write go.mod");

    let harvested = harvest_snapshots(&snapshots_dir("emit"));
    assert!(!harvested.is_empty(), "no snapshots harvested");

    let programs: Vec<Program> = harvested
        .par_iter()
        .filter(|test| skip_reason_for_imports(&test.input, &test.snap_body).is_none())
        .filter_map(|test| {
            let source = wrap(&test.input);
            let Compiled::Emitted(go_code) = compile(&test.name, &source) else {
                return None;
            };
            let variants = variants(&test.name, &source)
                .into_iter()
                .map(|variant| {
                    let compiled = compile(&variant.name, &variant.source);
                    if let Compiled::Emitted(variant_go_code) = &compiled
                        && !gives_original_go(&variant, variant_go_code, &test.name, &go_code)
                    {
                        write_subpackage(&target, &variant.name, variant_go_code, None)
                            .expect("write subpackage");
                    }
                    (variant, compiled)
                })
                .collect();
            Some(Program {
                name: test.name.clone(),
                go_code,
                variants,
            })
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

    let alias_differences = alias_differences(&programs);
    let mut variant_count = 0;
    let mut rejected = 0;
    let mut failures = Vec::new();
    for Program {
        name,
        go_code,
        variants,
    } in &programs
    {
        for (variant, compiled) in variants {
            variant_count += 1;
            let problem = match compiled {
                Compiled::Rejected => {
                    rejected += 1;
                    continue;
                }
                Compiled::Failed(message) => Problem::Compiler(message.clone()),
                Compiled::Emitted(variant_go_code) => {
                    let variant_go_code = with_program_name(variant_go_code, variant, name);
                    if let Some(line) = go_errors.get(&format!("test_{}", variant.name)) {
                        Problem::Go(line.clone())
                    } else {
                        let difference =
                            go_difference(variant, go_code, &variant_go_code, &alias_differences);
                        let Some((original, changed)) = difference else {
                            continue;
                        };
                        Problem::DifferentGo {
                            original,
                            variant: changed,
                        }
                    }
                }
            };
            let repro = target.join(format!("test_{}", variant.name));
            fs::create_dir_all(&repro).expect("create package dir");
            let repro = repro.join("test.lis");
            fs::write(&repro, &variant.source).expect("write test.lis");
            failures.push(Failure {
                program: name,
                variant,
                problem,
                repro,
            });
        }
    }

    eprintln!(
        "{} programs, {} variants, {} rejected by Lisette, {} failures",
        programs.len(),
        variant_count,
        rejected,
        failures.len(),
    );
    for Failure {
        variant,
        problem,
        repro,
        ..
    } in &failures
    {
        eprintln!(
            "{} ({}): {}\n  {}\n  repro: {}",
            variant.name,
            variant.rewrite,
            variant.change,
            problem.summary(),
            repro.display(),
        );
    }
    if let Ok(path) = env::var("GO_EQUIVALENTS_REPORT")
        && !failures.is_empty()
    {
        fs::write(path, report(&failures)).expect("write report");
    }
    assert!(
        failures.is_empty(),
        "{} variant(s) crashed the compiler, failed go build or vet, or changed the Go of a parenthesized, wrapped or aliased program (listed above)",
        failures.len()
    );
}

fn report(failures: &[Failure]) -> String {
    let mut groups: Vec<(&str, (&str, String), Vec<String>)> = Vec::new();
    for Failure {
        program,
        variant,
        problem,
        ..
    } in failures
    {
        let problem = problem.markdown();
        let mut change = variant.change.clone();
        if let Some(first) = change.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        match groups
            .iter_mut()
            .find(|(name, text, _)| name == program && *text == problem)
        {
            Some((_, _, changes)) if changes.contains(&change) => {}
            Some((_, _, changes)) => changes.push(change),
            None => groups.push((program, problem, vec![change])),
        }
    }
    let mut body = String::new();
    let mut shown = 0;
    let mut omitted = 0;
    for (program, (kind, detail), changes) in &groups {
        let bullets: String = changes
            .iter()
            .map(|change| format!("- {change}\n"))
            .collect();
        let entry = format!(
            "\n### {}. {kind} in {}\n\n{detail}\n\n{bullets}",
            shown + 1,
            code_span(program)
        );
        if body.len() + entry.len() > REPORT_LIMIT {
            omitted += 1;
            continue;
        }
        shown += 1;
        body.push_str(&entry);
    }
    if omitted > 0 {
        body.push_str(&format!(
            "\n{omitted} more failures do not fit in this issue. See the run log.\n"
        ));
    }
    body.trim_start().to_string()
}

fn compile(name: &str, source: &str) -> Compiled {
    let result = catch_unwind(AssertUnwindSafe(|| {
        compile_go_test(source, &format!("test_{name}"))
    }));
    match result {
        Ok(Ok(emitted)) => Compiled::Emitted(emitted.go_code),
        Ok(Err(CompileError::Inference(_))) => Compiled::Rejected,
        Ok(Err(CompileError::Emit(message))) => Compiled::Failed(message),
        Err(payload) => Compiled::Failed(panic_message(payload)),
    }
}

/// Whether the rewrite keeps the Go of the original program.
fn must_give_original_go(variant: &Variant) -> bool {
    matches!(variant.rewrite, "paren" | "wrap" | "ref" | "call")
}

/// Whether the variant gives the Go of the original program. Such a variant needs no `go build`,
/// because the original already gets one.
fn gives_original_go(variant: &Variant, variant_go_code: &str, name: &str, go_code: &str) -> bool {
    must_give_original_go(variant) && with_program_name(variant_go_code, variant, name) == go_code
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
    for (i, range) in pick(&sites.paren).enumerate() {
        let expression = &source[range.clone()];
        let replacement = format!("({expression})");
        variants.push(Variant {
            name: format!("{name}__paren{}", i + 1),
            rewrite: "paren",
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
    let count = sites.reference.len().min(SITES_PER_REWRITE);
    for i in 0..count {
        let (range, replacement, reads_through) =
            &sites.reference[i * sites.reference.len() / count];
        let variant_source = splice(source, range, replacement);
        if *reads_through && reads_a_reference(source, range) {
            continue;
        }
        variants.push(Variant {
            name: format!("{name}__ref{}", i + 1),
            rewrite: "ref",
            change: describe(source, range, replacement),
            source: variant_source,
        });
    }
    let calls = call_sites(source);
    let count = calls.len().min(SITES_PER_REWRITE);
    for i in 0..count {
        let (range, replacement) = &calls[i * calls.len() / count];
        variants.push(Variant {
            name: format!("{name}__call{}", i + 1),
            rewrite: "call",
            change: describe(source, range, replacement),
            source: splice(source, range, replacement),
        });
    }
    variants.retain(|variant| !syntax::build_ast(&variant.source, 0).has_errors());
    variants
}

fn call_sites(source: &str) -> Vec<(Range<usize>, String)> {
    let inferred = TestPipeline::new(source)
        .wrapped()
        .go_test_mode()
        .compile()
        .run_inference();
    let mut sites = Vec::new();
    for item in &inferred.ast {
        collect_calls(item, source, &mut sites);
    }
    sites
}

fn collect_calls(expression: &Expression, source: &str, sites: &mut Vec<(Range<usize>, String)>) {
    if let Expression::Call {
        expression: callee,
        args,
        spread: None,
        type_arguments,
        span,
        ..
    } = expression
        && type_arguments.annotations().len() == 0
        && let Some(arguments) = args
            .iter()
            .map(|argument| source.get(range_of(argument.get_span())))
            .collect::<Option<Vec<_>>>()
        && let Some(replacement) = call_rewrite(callee, &arguments, source)
    {
        sites.push((range_of(*span), replacement));
    }
    for child in expression.children() {
        collect_calls(child, source, sites);
    }
}

fn call_rewrite(callee: &Expression, arguments: &[&str], source: &str) -> Option<String> {
    match callee {
        Expression::DotAccess {
            expression: receiver,
            member,
            resolution:
                DotAccessResolution::InstanceMethod {
                    receiver_coercion,
                    method: Some(method),
                    ..
                },
            ..
        } => {
            let owner_id = method.owner.as_str();
            if let Type::Nominal { id, .. } = receiver.get_type().strip_refs()
                && id.as_str() != owner_id
            {
                return None;
            }
            let owner = match strip_go_prefix(owner_id) {
                Some(path) => path.rsplit('/').next()?.to_string(),
                None => owner_id.rsplit('.').next()?.to_string(),
            };
            let receiver = source.get(range_of(receiver.get_span()))?;
            let receiver = match receiver_coercion {
                Some(ReceiverCoercion::AutoAddress) => format!("&{receiver}"),
                Some(ReceiverCoercion::AutoDeref) => format!("({receiver}).*"),
                None => receiver.to_string(),
            };
            let mut all = vec![receiver];
            all.extend(arguments.iter().map(|argument| argument.to_string()));
            Some(format!("{owner}.{member}({})", all.join(", ")))
        }
        Expression::Identifier { .. }
        | Expression::DotAccess {
            resolution: DotAccessResolution::PackageMember { .. },
            ..
        } => {
            let (first, rest) = arguments.split_first()?;
            let callee = source.get(range_of(callee.get_span()))?;
            let rest = if rest.is_empty() {
                String::new()
            } else {
                format!("({})", rest.join(", "))
            };
            Some(format!("(({first}) |> {callee}{rest})"))
        }
        _ => None,
    }
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
            body,
            ..
        } => {
            generics.extend(own.iter().map(|generic| generic.name.as_str()));
            for annotation in params.iter().filter_map(|param| param.annotation.as_ref()) {
                add_alias_site(annotation, source, generics, sites);
            }
            add_alias_site(return_annotation, source, generics, sites);
            let returns_value = source
                .get(range_of(return_annotation.get_span()))
                .is_some_and(|text| !text.is_empty());
            if returns_value
                && let Some(Expression::Block { items, .. }) = body.definition()
                && let Some(tail) = items.last()
                && !matches!(
                    tail,
                    Expression::Let { .. }
                        | Expression::Return { .. }
                        | Expression::Assignment { .. }
                )
            {
                sites.add_value(tail);
            }
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
                sites.add_value(value);
                sites.add_place_read(value, source);
            }
            if let Some(annotation) = &binding.annotation {
                add_alias_site(annotation, source, generics, sites);
            }
        }
        Expression::Return { expression, .. } => {
            sites.add_value(expression);
            sites.add_place_read(expression, source);
        }
        Expression::Assignment {
            value,
            compound_operator: None,
            ..
        } => {
            sites.add_value(value);
            sites.add_place_read(value, source);
        }
        Expression::Binary { left, right, .. } => {
            sites.add_place_read(left, source);
            sites.add_place_read(right, source);
        }
        Expression::DotAccess {
            expression: receiver,
            ..
        } => {
            let range = range_of(receiver.get_span());
            match &**receiver {
                Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } => {
                    if let Some(text) = source.get(range_of(inner.get_span())) {
                        sites.reference.push((range, text.to_string(), false));
                    }
                }
                place if is_place(place) => {
                    if let Some(text) = source.get(range.clone()) {
                        let replacement = format!("(&{text})");
                        sites.reference.push((range, replacement, false));
                    }
                }
                _ => {}
            }
        }
        Expression::Call { args, .. } => {
            for argument in args {
                sites.add_place_read(argument, source);
                let range = range_of(argument.get_span());
                if !range.is_empty() {
                    sites.paren.push(range);
                }
            }
        }
        _ => {}
    }
    for child in expression.children() {
        visit(child, source, generics, sites);
    }
    generics.truncate(mark);
}

fn is_place(expression: &Expression) -> bool {
    match expression {
        Expression::Identifier { .. } => true,
        Expression::DotAccess { expression, .. } | Expression::IndexedAccess { expression, .. } => {
            is_place(expression)
        }
        _ => false,
    }
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

fn go_difference(
    variant: &Variant,
    original: &str,
    changed: &str,
    alias_differences: &HashMap<String, (String, String)>,
) -> Option<(String, String)> {
    if must_give_original_go(variant) {
        if changed == original {
            return None;
        }
        first_difference(original.split('\n'), changed.split('\n'))
    } else if variant.rewrite == "alias" {
        alias_differences.get(&variant.name).cloned()
    } else {
        None
    }
}

fn alias_differences(programs: &[Program]) -> HashMap<String, (String, String)> {
    let directory = repo_root().join("target/go_emit_equivalents_aliases");
    fresh_dir(&directory);
    let mut pairs = String::new();
    for program in programs {
        let original_path = directory.join(format!("{}.go", program.name));
        let mut original_written = false;
        for (variant, compiled) in &program.variants {
            let Compiled::Emitted(variant_go_code) = compiled else {
                continue;
            };
            if variant.rewrite != "alias" {
                continue;
            }
            if !original_written {
                fs::write(&original_path, &program.go_code).expect("write original Go");
                original_written = true;
            }
            let variant_path = directory.join(format!("{}.variant.go", variant.name));
            fs::write(
                &variant_path,
                with_program_name(variant_go_code, variant, &program.name),
            )
            .expect("write variant Go");
            pairs.push_str(&format!(
                "{}\t{}\t{}\n",
                variant.name,
                original_path.display(),
                variant_path.display()
            ));
        }
    }
    let pairs_path = directory.join("pairs.tsv");
    fs::write(&pairs_path, pairs).expect("write alias pairs");
    let output = Command::new(build_go_helper(
        "go_equivalents/alias_comparison",
        "alias_comparison",
    ))
    .arg(&pairs_path)
    .output()
    .expect("spawn alias_comparison");
    assert!(
        output.status.success(),
        "alias_comparison failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let name = fields.next()?;
            let original = fields.next()?;
            let variant = fields.next()?;
            Some((
                name.to_string(),
                (original.to_string(), variant.to_string()),
            ))
        })
        .collect()
}

fn with_program_name(go_code: &str, variant: &Variant, name: &str) -> String {
    go_code.replace(&format!("test_{}", variant.name), &format!("test_{name}"))
}

fn first_difference<'a>(
    mut original: impl Iterator<Item = &'a str>,
    mut variant: impl Iterator<Item = &'a str>,
) -> Option<(String, String)> {
    let shown = |line: Option<&str>| {
        line.map_or("(end of file)".to_string(), |line| line.trim().to_string())
    };
    loop {
        match (original.next(), variant.next()) {
            (None, None) => return None,
            (left, right) if left == right => {}
            (left, right) => return Some((shown(left), shown(right))),
        }
    }
}

fn reads_a_reference(source: &str, range: &Range<usize>) -> bool {
    if syntax::build_ast(source, 0).has_errors() {
        return false;
    }
    let inferred = TestPipeline::new(source)
        .wrapped()
        .go_test_mode()
        .compile()
        .run_inference();
    inferred
        .ast
        .iter()
        .any(|item| reference_place_at(item, range))
}

fn reference_place_at(expression: &Expression, range: &Range<usize>) -> bool {
    let span = expression.get_span();
    if span.byte_offset as usize == range.start
        && span.byte_length as usize == range.len()
        && expression.get_type().is_ref()
    {
        return true;
    }
    expression
        .children()
        .into_iter()
        .any(|child| reference_place_at(child, range))
}
