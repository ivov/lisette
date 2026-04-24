pub(crate) mod attributes;
mod casing;
mod checks;
mod visitor;

use std::cell::RefCell;

use super::lints::LintContext;
use diagnostics::LisetteDiagnostic;

use attributes::{check_attributes, check_struct_attributes};
use checks::{
    check_bool_literal_comparison, check_double_negation, check_duplicate_logical_operand,
    check_empty_match_arm, check_excess_parens_on_condition, check_expression_naming,
    check_identical_if_branches, check_match_literal_collection, check_pattern_naming,
    check_rest_only_slice_pattern, check_self_assignment, check_self_comparison,
    check_single_arm_match, check_uninterpolated_fstring,
};
use visitor::visit_ast;

pub struct AstLintGroup;

impl AstLintGroup {
    pub fn check(&self, ctx: &LintContext) -> Vec<LisetteDiagnostic> {
        let diagnostics = RefCell::new(Vec::new());
        let is_d_lis = ctx.is_d_lis;
        let files = ctx.files;

        visit_ast(
            ctx.ast,
            &mut |expression| {
                check_double_negation(expression, &mut diagnostics.borrow_mut());
                check_self_comparison(expression, &mut diagnostics.borrow_mut());
                check_self_assignment(expression, &mut diagnostics.borrow_mut());
                check_duplicate_logical_operand(expression, files, &mut diagnostics.borrow_mut());
                check_bool_literal_comparison(expression, &mut diagnostics.borrow_mut());
                check_identical_if_branches(expression, &mut diagnostics.borrow_mut());
                check_empty_match_arm(expression, &mut diagnostics.borrow_mut());
                check_excess_parens_on_condition(expression, &mut diagnostics.borrow_mut());
                check_match_literal_collection(expression, &mut diagnostics.borrow_mut());
                check_single_arm_match(expression, &mut diagnostics.borrow_mut());
                check_uninterpolated_fstring(expression, &mut diagnostics.borrow_mut());
                check_expression_naming(expression, is_d_lis, &mut diagnostics.borrow_mut());
                check_struct_attributes(expression, &mut diagnostics.borrow_mut());
                check_attributes(expression, &mut diagnostics.borrow_mut());
            },
            &mut |pattern| {
                check_rest_only_slice_pattern(pattern, &mut diagnostics.borrow_mut());
                check_pattern_naming(pattern, is_d_lis, &mut diagnostics.borrow_mut());
            },
        );

        diagnostics.into_inner()
    }
}
