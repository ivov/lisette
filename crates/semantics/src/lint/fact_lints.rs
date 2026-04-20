use crate::facts::DiscardedTailKind;
use crate::lint::{LintContext, LintRule};
use diagnostics::LisetteDiagnostic;

pub struct FactLintGroup;

impl LintRule for FactLintGroup {
    fn check(&self, ctx: &LintContext) -> Vec<LisetteDiagnostic> {
        let mut diagnostics = Vec::new();

        check_unused_variables(ctx, &mut diagnostics);
        check_unused_parameters(ctx, &mut diagnostics);
        check_unused_mut(ctx, &mut diagnostics);
        check_written_but_not_read(ctx, &mut diagnostics);
        check_dead_code(ctx, &mut diagnostics);
        check_pattern_issues(ctx, &mut diagnostics);
        check_unused_expressions(ctx, &mut diagnostics);
        check_discarded_tail_expressions(ctx, &mut diagnostics);
        check_overused_references(ctx, &mut diagnostics);
        check_unused_type_params(ctx, &mut diagnostics);
        check_type_params_only_in_bound(ctx, &mut diagnostics);
        check_always_failing_try_blocks(ctx, &mut diagnostics);
        check_expression_only_fstrings(ctx, &mut diagnostics);

        diagnostics
    }
}

fn check_unused_variables(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for b in ctx.facts.bindings.values() {
        if b.name.starts_with('_') || b.used || b.kind.is_param() {
            continue;
        }
        if b.kind.is_match_arm() && !b.is_as_alias {
            continue;
        }
        diagnostics.push(diagnostics::lint::unused_variable(
            &b.span,
            &b.name,
            b.is_struct_field,
        ));
    }
}

fn check_unused_parameters(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for b in ctx.facts.bindings.values() {
        if !b.is_typedef
            && !b.name.starts_with('_')
            && !b.used
            && b.kind.is_param()
            && b.name != "self"
        {
            diagnostics.push(diagnostics::lint::unused_parameter(&b.span, &b.name));
        }
    }
}

fn check_unused_mut(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for b in ctx.facts.bindings.values() {
        if b.kind.is_mutable() && !b.mutated {
            diagnostics.push(diagnostics::lint::unused_mut(&b.span));
        }
    }
}

fn check_dead_code(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for dc in &ctx.facts.dead_code {
        diagnostics.push(diagnostics::lint::dead_code(&dc.span, dc.cause));
    }
}

fn check_pattern_issues(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for issue in ctx.facts.pattern_issues.iter() {
        diagnostics.push(diagnostics::lint::pattern_issue(&issue.span, issue.kind));
    }
}

fn check_unused_expressions(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for fact in &ctx.facts.unused_expressions {
        diagnostics.push(diagnostics::lint::unused_expression(&fact.span, fact.kind));
    }
}

fn check_discarded_tail_expressions(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for fact in &ctx.facts.discarded_tail_expressions {
        match fact.kind {
            DiscardedTailKind::Partial => {
                diagnostics.push(diagnostics::lint::discarded_partial_in_tail(
                    &fact.span,
                    &fact.return_type,
                ));
            }
            DiscardedTailKind::Result => {
                diagnostics.push(diagnostics::lint::discarded_result_in_tail(
                    &fact.span,
                    &fact.return_type,
                ));
            }
            DiscardedTailKind::Option => {
                diagnostics.push(diagnostics::lint::discarded_option_in_tail(
                    &fact.span,
                    &fact.return_type,
                ));
            }
        }
    }
}

fn check_overused_references(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for fact in &ctx.facts.overused_references {
        diagnostics.push(diagnostics::lint::unnecessary_reference(
            &fact.span,
            fact.name.as_deref(),
        ));
    }
}

fn check_unused_type_params(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for fact in &ctx.facts.unused_type_params {
        if fact.is_typedef {
            continue;
        }
        diagnostics.push(diagnostics::lint::unused_type_parameter(&fact.span));
    }
}

fn check_type_params_only_in_bound(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for fact in &ctx.facts.type_params_only_in_bound {
        if fact.is_typedef {
            continue;
        }
        diagnostics.push(diagnostics::lint::type_param_only_in_bound(
            &fact.span, &fact.name,
        ));
    }
}

fn check_always_failing_try_blocks(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for span in &ctx.facts.always_failing_try_blocks {
        diagnostics.push(diagnostics::lint::ineffective_try_block(span));
    }
}

fn check_expression_only_fstrings(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for span in &ctx.facts.expression_only_fstrings {
        diagnostics.push(diagnostics::lint::expression_only_fstring(span));
    }
}

fn check_written_but_not_read(ctx: &LintContext, diagnostics: &mut Vec<LisetteDiagnostic>) {
    for b in ctx.facts.bindings.values() {
        // Check for mutable variables that are mutated but never read
        if b.kind.is_mutable() && b.mutated && !b.used && !b.name.starts_with('_') {
            diagnostics.push(diagnostics::lint::written_but_not_read(&b.span, &b.name));
        }
    }
}
