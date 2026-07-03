use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use diagnostics::{Edit, Fix};
use semantics::context::AnalysisContext;
use semantics::facts::Facts;
use syntax::ast::Span;
use syntax::program::UnusedInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lint {
    UnusedVariable,
    UnusedParameter,
    UnusedMut,
    UnusedImport,
    UnusedType,
    UnusedFunction,
    UnusedConstant,
    UnusedStructField,
    UnusedEnumVariant,
    UnusedLiteral,
    UnusedResult,
    UnusedOption,
    UnusedValue,
    DeadCodeAfterReturn,
    DeadCodeAfterBreak,
    DeadCodeAfterContinue,
    DeadCodeAfterDivergingIf,
    DeadCodeAfterDivergingMatch,
    DeadCodeAfterInfiniteLoop,
    DeadCodeAfterDivergingCall,
    DoubleBoolNegation,
    DoubleIntNegation,
    SelfComparison,
    SelfAssignment,
    MatchLiteralCollection,
    EmptyMatchArm,
    InternalTypeLeak,
    UnnecessaryReference,
    UnusedTypeParameter,
    TypeParamOnlyInBound,
    RestOnlySlicePattern,
    NonPascalCaseType,
    NonPascalCaseTypeParameter,
    NonPascalCaseEnumVariant,
    NonSnakeCaseFunction,
    NonSnakeCaseVariable,
    NonSnakeCaseParameter,
    NonSnakeCaseStructField,
    NonScreamingSnakeCaseConstant,
    RedundantIfLet,
    RedundantLetElse,
    SingleArmMatch,
    RedundantIfLetElse,
    UnreachableIfLetElse,
    TryBlockNoSuccessPath,
    ExcessParensOnCondition,
    ReplaceableWithZeroFill,
}

#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    disabled: HashSet<Lint>,
}

impl LintConfig {
    pub fn is_enabled(&self, lint: Lint) -> bool {
        !self.disabled.contains(&lint)
    }
}

pub(crate) fn run(
    analysis: &AnalysisContext,
    facts: &Facts,
    unused: &mut UnusedInfo,
    sink: &LocalSink,
) {
    let mut diagnostics: Vec<LisetteDiagnostic> = Vec::new();
    let sources = source_by_file(analysis);

    let erroring_functions = erroring_function_spans(facts, sink);
    collect_bindings(
        facts,
        unused,
        &sources,
        &erroring_functions,
        &mut diagnostics,
    );
    collect_dead_code(facts, &mut diagnostics);
    collect_pattern_issues(facts, &mut diagnostics);
    collect_unused_expressions(facts, &mut diagnostics);
    collect_discarded_tail_expressions(facts, &mut diagnostics);
    collect_overused_references(facts, &mut diagnostics);
    collect_unused_type_params(facts, &mut diagnostics);
    collect_type_params_only_in_bound(facts, &mut diagnostics);
    collect_always_failing_try_blocks(facts, &mut diagnostics);
    collect_expression_only_fstrings(facts, &sources, &mut diagnostics);

    diagnostics.sort_by(LisetteDiagnostic::sort_key);
    sink.extend(diagnostics);
}

fn source_by_file<'a>(analysis: &AnalysisContext<'a>) -> HashMap<u32, &'a str> {
    let mut sources = HashMap::default();
    for module in analysis.store.modules.values() {
        for (file_id, file) in &module.files {
            sources.insert(*file_id, file.source.as_str());
        }
    }
    sources
}

fn mut_keyword_deletion(sources: &HashMap<u32, &str>, name: Span) -> Option<Span> {
    let source = sources.get(&name.file_id)?;
    let name_start = name.byte_offset as usize;
    let before = source.get(..name_start)?.trim_end();
    let mut_start = before.strip_suffix("mut")?.len();
    let preceded_by_word = before[..mut_start]
        .bytes()
        .last()
        .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_');
    if preceded_by_word {
        return None;
    }
    Some(Span::new(
        name.file_id,
        mut_start as u32,
        (name_start - mut_start) as u32,
    ))
}

fn fstring_inner<'a>(sources: &HashMap<u32, &'a str>, span: Span) -> Option<&'a str> {
    let source = sources.get(&span.file_id)?;
    let text = source.get(span.byte_offset as usize..span.end() as usize)?;
    let inner = text.strip_prefix("f\"")?.strip_suffix('"')?.trim();
    Some(inner.strip_prefix('{')?.strip_suffix('}')?.trim())
}

fn erroring_function_spans(facts: &Facts, sink: &LocalSink) -> Vec<Span> {
    let error_points = sink.error_label_points();
    if error_points.is_empty() {
        return Vec::new();
    }
    facts
        .function_spans
        .iter()
        .filter(|function_span| {
            error_points.iter().any(|(file_id, offset)| {
                *file_id == Some(function_span.file_id)
                    && function_span.byte_offset as usize <= *offset
                    && *offset < function_span.end() as usize
            })
        })
        .copied()
        .collect()
}

fn within_any(function_spans: &[Span], span: Span) -> bool {
    function_spans.iter().any(|function_span| {
        function_span.file_id == span.file_id
            && function_span.byte_offset <= span.byte_offset
            && span.end() <= function_span.end()
    })
}

fn collect_bindings(
    facts: &Facts,
    unused: &mut UnusedInfo,
    sources: &HashMap<u32, &str>,
    erroring_functions: &[Span],
    out: &mut Vec<LisetteDiagnostic>,
) {
    for b in facts.bindings.values() {
        let is_anon = b.name.starts_with('_');
        let written_but_not_read = b.kind.is_mutable() && b.mutated && !b.used && !is_anon;
        let is_write_only_param = written_but_not_read && b.kind.is_param();

        if !b.used && !is_write_only_param {
            if !is_anon && b.kind.is_param() && !b.is_typedef && b.name != "self" {
                out.push(diagnostics::lint::unused_parameter(&b.span, &b.name));
            } else if !written_but_not_read
                && !is_anon
                && !b.kind.is_param()
                && (!b.kind.is_pattern_position() || b.is_as_alias)
            {
                out.push(diagnostics::lint::unused_variable(
                    &b.span,
                    &b.name,
                    b.is_struct_field,
                ));
            }
            unused.mark_binding_unused(b.span);
        }

        if b.kind.is_mutable() && !b.mutated && !within_any(erroring_functions, b.span) {
            let mut diagnostic = diagnostics::lint::unused_mut(&b.span);
            if let Some(deletion) = mut_keyword_deletion(sources, b.span) {
                diagnostic =
                    diagnostic.with_fix(Fix::new("Remove `mut`", Edit::deletion(deletion)));
            }
            out.push(diagnostic);
        }

        if written_but_not_read {
            out.push(diagnostics::lint::written_but_not_read(&b.span, &b.name));
        }
    }
}

fn collect_dead_code(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for dc in &facts.dead_code {
        out.push(diagnostics::lint::dead_code(&dc.span, dc.cause));
    }
}

fn collect_pattern_issues(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for issue in &facts.pattern_issues {
        out.push(diagnostics::lint::pattern_issue(&issue.span, issue.kind));
    }
}

fn collect_unused_expressions(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for fact in &facts.unused_expressions {
        out.push(diagnostics::lint::unused_expression(&fact.span, fact.kind));
    }
}

fn collect_discarded_tail_expressions(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for fact in &facts.discarded_tail_expressions {
        out.push(diagnostics::infer::mismatched_tail_value(
            &fact.span,
            &fact.return_type,
            &fact.expected_span,
            &fact.expected_type,
        ));
    }
}

fn collect_overused_references(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for fact in &facts.overused_references {
        out.push(
            diagnostics::lint::unnecessary_reference(&fact.span, fact.name.as_deref()).with_fix(
                Fix::new(
                    "Remove the redundant `&`",
                    Edit::deletion(Span::new(fact.span.file_id, fact.span.byte_offset, 1)),
                ),
            ),
        );
    }
}

fn collect_unused_type_params(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for fact in &facts.unused_type_params {
        out.push(diagnostics::lint::unused_type_parameter(&fact.span));
    }
}

fn collect_type_params_only_in_bound(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for fact in &facts.type_params_only_in_bound {
        out.push(diagnostics::lint::type_param_only_in_bound(
            &fact.span, &fact.name,
        ));
    }
}

fn collect_always_failing_try_blocks(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for span in &facts.always_failing_try_blocks {
        out.push(diagnostics::lint::ineffective_try_block(span));
    }
}

fn collect_expression_only_fstrings(
    facts: &Facts,
    sources: &HashMap<u32, &str>,
    out: &mut Vec<LisetteDiagnostic>,
) {
    for fact in &facts.expression_only_fstrings {
        let mut diagnostic = diagnostics::lint::expression_only_fstring(&fact.span);
        if let Some(inner) = fstring_inner(sources, fact.span) {
            let replacement = if fact.needs_parens {
                format!("({inner})")
            } else {
                inner.to_string()
            };
            diagnostic = diagnostic.with_fix(Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(fact.span, replacement),
            ));
        }
        out.push(diagnostic);
    }
}
