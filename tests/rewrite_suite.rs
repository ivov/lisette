//! Rewrites each emit snapshot program in ways that keep its meaning, and
//! checks that each variant compiles to Go that builds and vets whenever the
//! original does. A variant that only adds parentheses or a block around one
//! expression must compile to the Go of the original. A variant that replaces
//! an annotation with an alias of it must compile to the Go of the original,
//! after the alias is replaced by its target. Lisette errors on a variant are
//! ignored. Runs nightly.

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
    paren: Vec<Range<usize>>,
    alias: Vec<Range<usize>>,
}

impl Sites {
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

enum Compiled {
    Written(String),
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

    let programs: Vec<Program> = harvested
        .par_iter()
        .filter(|test| skip_reason_for_imports(&test.input, &test.snap_body).is_none())
        .filter_map(|test| {
            let source = wrap(&test.input);
            let Compiled::Written(go_code) = compile(&target, &test.name, &source) else {
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

    let mut variant_count = 0;
    let mut rejected = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();
    for Program {
        name,
        go_code,
        variants,
    } in &programs
    {
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
                Compiled::Written(variant_go_code) => {
                    let variant_go_code = variant_go_code
                        .replace(&format!("test_{}", variant.name), &format!("test_{name}"));
                    match go_errors.get(&format!("test_{}", variant.name)) {
                        Some(line) => format!("go: {line}"),
                        None if matches!(variant.rewrite, "paren" | "wrap")
                            && variant_go_code != *go_code =>
                        {
                            "Go differs from the Go of the original program".to_string()
                        }
                        None if variant.rewrite == "alias"
                            && !same_go_after_alias(go_code, &variant_go_code) =>
                        {
                            "Go differs from the Go of the original program, with the alias replaced by its target".to_string()
                        }
                        None => continue,
                    }
                }
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
        "{} variant(s) crashed the compiler, failed go build or vet, or changed the Go of a parenthesized, wrapped or aliased program (listed above)",
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
            Compiled::Written(emitted.go_code)
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
            }
            if let Some(annotation) = &binding.annotation {
                add_alias_site(annotation, source, generics, sites);
            }
        }
        Expression::Return { expression, .. } => sites.add_value(expression),
        Expression::Assignment {
            value,
            compound_operator: None,
            ..
        } => sites.add_value(value),
        Expression::Call { args, .. } => {
            for argument in args {
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

fn same_go_after_alias(original: &str, variant: &str) -> bool {
    let declaration = variant
        .lines()
        .find_map(|line| line.strip_prefix("type MmAlias = "));
    let variant = match declaration {
        Some(target) => replace_word(
            &variant.replacen(&format!("type MmAlias = {target}\n"), "", 1),
            "MmAlias",
            target,
        ),
        None => variant.to_string(),
    };
    let (original_imports, original_body) = split_imports(&ungroup_parameters(original));
    let (variant_imports, variant_body) = split_imports(&ungroup_parameters(&variant));
    let target = declaration.unwrap_or_default();
    original_body == variant_body
        && original_imports
            .iter()
            .all(|import| variant_imports.contains(import))
        && variant_imports.iter().all(|import| {
            original_imports.contains(import)
                || target.contains(&format!("{}.", import_qualifier(import)))
        })
}

fn replace_word(text: &str, word: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(word) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + word.len()..].chars().next();
        let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..at]);
        out.push_str(if is_word(before) || is_word(after) {
            word
        } else {
            replacement
        });
        rest = &rest[at + word.len()..];
    }
    out.push_str(rest);
    out
}

fn split_imports(code: &str) -> (Vec<String>, Vec<String>) {
    let mut imports = Vec::new();
    let mut body = Vec::new();
    let mut in_block = false;
    for line in code.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if in_block {
            if line == ")" {
                in_block = false;
            } else {
                imports.push(line.to_string());
            }
        } else if line == "import (" {
            in_block = true;
        } else if let Some(import) = line.strip_prefix("import ") {
            imports.push(import.to_string());
        } else {
            body.push(line.to_string());
        }
    }
    (imports, body)
}

fn import_qualifier(import: &str) -> &str {
    match import.split_once(' ') {
        Some((qualifier, _)) => qualifier,
        None => import
            .trim_matches('"')
            .rsplit('/')
            .next()
            .unwrap_or_default(),
    }
}

fn ungroup_parameters(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some(found) = code[at..].find("func") {
        let start = at + found;
        at = start + 4;
        if start > 0 && is_identifier_byte(bytes[start - 1]) {
            continue;
        }
        let Some(open) = parameter_list_start(code, at) else {
            continue;
        };
        let Some(close) = matching_close(code, open) else {
            continue;
        };
        out.push_str(&code[copied..=open]);
        out.push_str(&ungroup_list(&ungroup_parameters(&code[open + 1..close])));
        copied = close;
        at = close;
    }
    out.push_str(&code[copied..]);
    out
}

fn parameter_list_start(code: &str, mut at: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    if code[at..].starts_with(" (") {
        at = matching_close(code, at + 1)? + 1;
    }
    if code[at..].starts_with(' ') {
        at += 1;
        while at < bytes.len() && is_identifier_byte(bytes[at]) {
            at += 1;
        }
    }
    if code[at..].starts_with('[') {
        at = matching_close(code, at)? + 1;
    }
    code[at..].starts_with('(').then_some(at)
}

fn matching_close(code: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in code.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn ungroup_list(list: &str) -> String {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in list.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c == ',' && depth == 0 {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    let named = |part: &String| {
        let name_end = part
            .bytes()
            .position(|byte| !is_identifier_byte(byte))
            .unwrap_or(part.len());
        name_end > 0 && (name_end == part.len() || part.as_bytes()[name_end] == b' ')
    };
    if !parts.iter().all(named) || parts.iter().all(|part| !part.contains(' ')) {
        return list.to_string();
    }
    let mut result = Vec::new();
    let mut pending = Vec::new();
    for part in parts {
        match part.split_once(' ') {
            Some((name, ty)) => {
                result.extend(pending.drain(..).map(|pending| format!("{pending} {ty}")));
                result.push(format!("{name} {ty}"));
            }
            None => pending.push(part),
        }
    }
    result.extend(pending);
    result.join(", ")
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
