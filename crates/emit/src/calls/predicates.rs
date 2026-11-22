use crate::Planner;
use crate::abi::fallible_producer::FallibleProducer;
use crate::calls::bound_value::PairCondition;
use crate::calls::comma_ok::CommaOkValueSlot;

use crate::ir::bodies::Statement;
use crate::ir::values::ValuePlan;
use syntax::ast::{Expression, UnaryOperator};
use syntax::program::CallOperation;

struct FusedPredicate<'a> {
    producer: FallibleProducer<'a>,
    succeeds: bool,
}

pub(crate) fn strip_negations(mut expression: &Expression) -> (&Expression, bool) {
    let mut negated = false;
    while let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: inner,
        ..
    } = expression
    {
        negated = !negated;
        expression = inner;
    }
    (expression, negated)
}

impl<'p> Planner<'p> {
    fn fused_predicate<'a>(&self, expression: &'a Expression) -> Option<FusedPredicate<'a>>
    where
        'p: 'a,
    {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } = expression
        else {
            return None;
        };
        if !args.is_empty()
            || spread.is_some()
            || !matches!(
                self.facts.call_operation(expression),
                Some(CallOperation::Function | CallOperation::BoundMethod { .. })
            )
        {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver,
            ..
        } = &**callee
        else {
            return None;
        };
        let definition = self.facts.checked(callee).callee().definition.as_deref()?;
        let succeeds = match definition {
            "prelude.Result.is_ok" | "prelude.Option.is_some" => true,
            "prelude.Result.is_err" | "prelude.Option.is_none" => false,
            _ => return None,
        };
        Some(FusedPredicate {
            producer: self.fallible_producer(receiver)?,
            succeeds,
        })
    }

    fn bind_fused_predicate(
        &mut self,
        predicate: FusedPredicate<'_>,
        negated: bool,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, PairCondition) {
        let succeeds = predicate.succeeds;
        let bound = predicate.producer.bind_slot(self, slot);
        let condition = if succeeds != negated {
            bound.success_condition()
        } else {
            bound.failure_condition()
        };
        (bound.statements, condition)
    }

    pub(crate) fn try_plan_fused_predicate_condition(
        &mut self,
        condition: &Expression,
    ) -> Option<(Vec<Statement>, PairCondition)> {
        let (target, negated) = strip_negations(condition);
        let predicate = self.fused_predicate(target)?;
        Some(self.bind_fused_predicate(predicate, negated, CommaOkValueSlot::Unused))
    }

    pub(crate) fn try_plan_fused_predicate_value(
        &mut self,
        expression: &Expression,
        negated: bool,
    ) -> Option<ValuePlan> {
        let predicate = self.fused_predicate(expression)?;
        let (setup, condition) =
            self.bind_fused_predicate(predicate, negated, CommaOkValueSlot::Discarded);
        debug_assert!(condition.initializer.is_none());
        Some(ValuePlan::plain_call(setup, condition.condition))
    }
}
