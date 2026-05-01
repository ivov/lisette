use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::context::AnalysisContext;
use crate::facts::Facts;
use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::program::File;
use syntax::program::Module;
use syntax::program::UnusedInfo;

use super::ast_lints::AstLintGroup;

pub struct LintContext<'a> {
    pub ast: &'a [Expression],
    pub is_d_lis: bool,
    pub files: &'a HashMap<u32, File>,
}

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

pub fn lint_all_modules(
    analysis: &AnalysisContext,
    facts: &Facts,
    sink: &LocalSink,
    unused: &mut UnusedInfo,
) {
    let store = analysis.store;
    let go_package_names = &store.go_package_names;
    let config = LintConfig::default();

    for module in store.modules.values() {
        if module.is_internal() {
            continue;
        }
        lint_module(
            module,
            store,
            go_package_names,
            facts,
            &config,
            sink,
            unused,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn lint_module(
    module: &Module,
    store: &crate::store::Store,
    go_package_names: &HashMap<String, String>,
    facts: &Facts,
    config: &LintConfig,
    sink: &LocalSink,
    unused: &mut UnusedInfo,
) {
    let mut diagnostics: Vec<LisetteDiagnostic> = Vec::new();

    // AST diagnostics extended first so the stable sort keeps them ahead of
    // reference-graph diagnostics on offset ties.
    for file in module.files.values() {
        let ctx = LintContext {
            ast: &file.items,
            is_d_lis: file.is_d_lis(),
            files: &module.files,
        };
        diagnostics.extend(AstLintGroup.check(&ctx));
    }

    let zero_fill_sink = diagnostics::LocalSink::new();
    for file in module.files.values() {
        super::replaceable_with_zero_fill::run(
            &file.items,
            &file.source,
            &module.id,
            store,
            &zero_fill_sink,
        );
    }
    diagnostics.extend(zero_fill_sink.take());

    let ref_result =
        super::ref_lints::run_ref_lints(module, &module.files, go_package_names, config, facts);
    if !ref_result.unused_import_aliases.is_empty() {
        unused.imports_by_module.insert(
            module.id.clone().into(),
            ref_result
                .unused_import_aliases
                .into_iter()
                .map(|s| s.into())
                .collect(),
        );
    }
    for span in ref_result.unused_definition_spans {
        unused.mark_definition_unused(span);
    }
    diagnostics.extend(ref_result.diagnostics);

    diagnostics.sort_by_key(|d| d.primary_offset());
    sink.extend(diagnostics);
}

pub fn lint_all_facts(facts: &Facts, unused: &mut UnusedInfo, sink: &LocalSink) {
    let mut diagnostics: Vec<LisetteDiagnostic> = Vec::new();

    collect_bindings(facts, unused, &mut diagnostics);
    collect_dead_code(facts, &mut diagnostics);
    collect_pattern_issues(facts, &mut diagnostics);
    collect_unused_expressions(facts, &mut diagnostics);
    collect_discarded_tail_expressions(facts, &mut diagnostics);
    collect_overused_references(facts, &mut diagnostics);
    collect_unused_type_params(facts, &mut diagnostics);
    collect_type_params_only_in_bound(facts, &mut diagnostics);
    collect_always_failing_try_blocks(facts, &mut diagnostics);
    collect_expression_only_fstrings(facts, &mut diagnostics);

    diagnostics.sort_by_key(|d| d.primary_offset());
    sink.extend(diagnostics);
}

fn collect_bindings(facts: &Facts, unused: &mut UnusedInfo, out: &mut Vec<LisetteDiagnostic>) {
    for b in facts.bindings.values() {
        let is_anon = b.name.starts_with('_');

        if !b.used {
            if !is_anon && b.kind.is_param() && !b.is_typedef && b.name != "self" {
                out.push(diagnostics::lint::unused_parameter(&b.span, &b.name));
            } else if !is_anon && !b.kind.is_param() && (!b.kind.is_match_arm() || b.is_as_alias) {
                out.push(diagnostics::lint::unused_variable(
                    &b.span,
                    &b.name,
                    b.is_struct_field,
                ));
            }
            unused.mark_binding_unused(b.span);
        }

        if b.kind.is_mutable() && !b.mutated {
            out.push(diagnostics::lint::unused_mut(&b.span));
        }

        if b.kind.is_mutable() && b.mutated && !b.used && !is_anon {
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
        out.push(diagnostics::lint::unnecessary_reference(
            &fact.span,
            fact.name.as_deref(),
        ));
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

fn collect_expression_only_fstrings(facts: &Facts, out: &mut Vec<LisetteDiagnostic>) {
    for span in &facts.expression_only_fstrings {
        out.push(diagnostics::lint::expression_only_fstring(span));
    }
}
