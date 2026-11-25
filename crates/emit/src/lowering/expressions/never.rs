use crate::Planner;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::expressions::values::with_source_positions;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::planner::written_span;
use crate::lowering::statements::blocks::directed_at;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::{LoweredBlock, LoweredStatement, Statement, diverging_statement};
use syntax::ast::Expression;
use syntax::types::Type;

fn runs_as_never_statement(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. }
            | Expression::Block { .. }
            | Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Select { .. }
            | Expression::Loop { .. }
            | Expression::Return { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
    )
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_never_value(&mut self, expression: &Expression) -> ValuePlan {
        ValuePlan::diverging(self.lower_never_value(expression))
    }

    pub(in crate::lowering) fn lower_never_value(
        &mut self,
        expression: &Expression,
    ) -> Vec<Statement> {
        let mut block = LoweredBlock {
            statements: vec![self.lower_never_effects(expression)],
        };
        if !block.ends_with_diverge() {
            block
                .statements
                .push(LoweredStatement::UnreachablePanic.into());
        }
        block.statements
    }

    fn lower_never_effects(&mut self, expression: &Expression) -> Statement {
        let Some(site) = CallSite::of(expression) else {
            if runs_as_never_statement(expression) {
                return self.lower_statement(expression);
            }
            let (statements, _) = self
                .plan_operand_shape(expression, ExpressionContext::value())
                .into_parts();
            return LoweredStatement::Body(LoweredBlock { statements }).into();
        };

        let plan = self.plan_call_value(site, site.ty, ExpressionContext::value());
        let (mut statements, call) =
            with_source_positions(&self.facts, &written_span(expression), plan).into_parts();
        if !call.is_empty() {
            statements.push(diverging_statement(call));
        }
        directed_at(
            &self.facts,
            expression,
            LoweredStatement::Body(LoweredBlock { statements }),
        )
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn fallible_block_type(
        &self,
        expression: &Expression,
        ty: &Type,
    ) -> Type {
        if !self.facts.is_never(&ty.ok_type()) {
            return ty.clone();
        }
        self.facts
            .value_use_slot(expression)
            .and_then(|slot| self.facts.peeled_option_or_result(slot))
            .filter(|slot| slot.is_option() == ty.is_option())
            .unwrap_or_else(|| ty.clone())
    }
}
