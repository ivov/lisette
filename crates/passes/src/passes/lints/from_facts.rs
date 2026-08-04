use rustc_hash::FxHashSet as HashSet;

use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use diagnostics::{Edit, Fix};
use semantics::facts::Facts;
use semantics::store::Store;
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
    RestOnlyPattern,
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
    ReplaceableWithAutofill,
}

pub(crate) fn run(
    store: &Store,
    facts: &Facts,
    pattern_lints: Vec<LisetteDiagnostic>,
    mut diagnostics: Vec<LisetteDiagnostic>,
    sink: &LocalSink,
) -> UnusedInfo {
    let mut unused = UnusedInfo::default();
    let erroring_functions = erroring_function_spans(facts, sink);
    collect_bindings(
        store,
        facts,
        &mut unused,
        &erroring_functions,
        &mut diagnostics,
    );
    collect_shadowed_captures(store, facts, &mut diagnostics);
    collect_dead_code(facts, &mut diagnostics);
    diagnostics.extend(pattern_lints);
    collect_overused_references(facts, &mut diagnostics);
    collect_always_failing_try_blocks(facts, &mut diagnostics);
    collect_expression_only_fstrings(store, facts, &mut diagnostics);
    collect_unprefixed_fstrings(store, facts, &mut diagnostics);

    diagnostics.sort_by(LisetteDiagnostic::sort_key);
    sink.extend(diagnostics);
    unused
}

fn mut_keyword_deletion(store: &Store, name: Span) -> Option<Span> {
    let source = &store.get_file(name.file_id)?.source;
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

fn fstring_inner(store: &Store, span: Span) -> Option<&str> {
    let source = &store.get_file(span.file_id)?.source;
    let text = source.get(span.byte_offset as usize..span.end() as usize)?;
    let inner = text.strip_prefix("f\"")?.strip_suffix('"')?.trim();
    Some(inner.strip_prefix('{')?.strip_suffix('}')?.trim())
}

fn collect_unprefixed_fstrings(store: &Store, facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    let produced: Vec<LisetteDiagnostic> = facts
        .unprefixed_fstrings
        .iter()
        .map(|fact| {
            let before = Span::new(fact.span.file_id, fact.span.byte_offset, 0);
            diagnostics::lint::unprefixed_fstring(&fact.span, &fact.name).with_fix(Fix::new(
                "Add the `f` prefix",
                Edit::replacement(before, "f"),
            ))
        })
        .collect();
    push_suppressible(store, produced, out);
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
                *file_id == function_span.file_id
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
    store: &Store,
    facts: &Facts,
    unused: &mut UnusedInfo,
    erroring_functions: &[Span],
    out: &mut Vec<LisetteDiagnostic>,
) {
    for b in facts.bindings.values() {
        let is_anon = b.name.starts_with('_');
        let written_but_not_read =
            b.kind.is_mutable() && b.mutation.is_some() && !b.used && !is_anon;
        let is_write_only_param = written_but_not_read && b.kind.is_param();

        if !b.used && !is_write_only_param {
            if !is_anon && b.kind.is_param() && !b.origin.is_typedef() && b.name != "self" {
                out.push(diagnostics::lint::unused_parameter(&b.span, &b.name));
            } else if !written_but_not_read
                && !is_anon
                && !b.kind.is_param()
                && (!b.kind.is_pattern_position() || b.origin.is_as_alias())
            {
                out.push(diagnostics::lint::unused_variable(
                    &b.span,
                    &b.name,
                    b.origin.is_struct_field(),
                ));
            }
            unused.mark_binding_unused(b.span);
        }

        if b.kind.is_mutable() && b.mutation.is_none() && !within_any(erroring_functions, b.span) {
            let mut diagnostic = diagnostics::lint::unused_mut(&b.span);
            if let Some(deletion) = mut_keyword_deletion(store, b.span) {
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

fn collect_shadowed_captures(store: &Store, facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    let produced: Vec<LisetteDiagnostic> = facts
        .bindings
        .values()
        .filter_map(|b| {
            let outer = b.shadows?;
            Some(diagnostics::lint::shadowed_capture(
                &b.span,
                &outer,
                &b.name,
                b.kind.is_param(),
                b.origin.is_struct_field(),
            ))
        })
        .collect();
    push_suppressible(store, produced, out);
}

fn push_suppressible(
    store: &Store,
    produced: Vec<LisetteDiagnostic>,
    out: &mut Vec<LisetteDiagnostic>,
) {
    if produced.is_empty() {
        return;
    }
    let reported: HashSet<u32> = produced
        .iter()
        .filter_map(LisetteDiagnostic::file_id)
        .collect();
    let allows: Vec<_> = store
        .packages
        .values()
        .flat_map(|package| package.source_files())
        .filter(|file| reported.contains(&file.id))
        .flat_map(|file| super::suppression::collect_function_allows(&file.items))
        .collect();
    out.extend(super::suppression::filter_allowed(produced, &allows));
}

fn collect_dead_code(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for dc in &facts.dead_code {
        out.push(diagnostics::lint::dead_code(&dc.span, dc.cause));
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

fn collect_always_failing_try_blocks(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for span in &facts.always_failing_try_blocks {
        out.push(diagnostics::lint::ineffective_try_block(span));
    }
}

fn collect_expression_only_fstrings(
    store: &Store,
    facts: &Facts,
    out: &mut Vec<LisetteDiagnostic>,
) {
    for fact in &facts.expression_only_fstrings {
        let mut diagnostic = diagnostics::lint::expression_only_fstring(&fact.span);
        if let Some(inner) = fstring_inner(store, fact.span) {
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
