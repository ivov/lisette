use crate::Planner;
use crate::lowering::abi::fallible_producer::FallibleProducer;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::bodies::PairCondition;

use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::Statement;
use syntax::ast::{Expression, UnaryOperator};
use syntax::program::CallOperation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum Polarity {
    Positive,
    Negated,
}

impl Polarity {
    pub(in crate::lowering) fn flip(self) -> Self {
        match self {
            Self::Positive => Self::Negated,
            Self::Negated => Self::Positive,
        }
    }
}

struct FusedPredicate<'a> {
    producer: FallibleProducer<'a>,
    polarity: Polarity,
}

pub(in crate::lowering) fn strip_negations(mut expression: &Expression) -> (&Expression, Polarity) {
    let mut polarity = Polarity::Positive;
    while let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: inner,
        ..
    } = expression
    {
        polarity = polarity.flip();
        expression = inner;
    }
    (expression, polarity)
}

impl<'p> Planner<'p> {
    fn bind_fused_predicate(
        &mut self,
        predicate: FusedPredicate<'_>,
        polarity: Polarity,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, PairCondition) {
        let predicate_polarity = predicate.polarity;
        let bound = predicate.producer.bind_slot(self, slot);
        let condition = if predicate_polarity == polarity {
            bound.success_condition()
        } else {
            bound.failure_condition()
        };
        (bound.statements, condition)
    }

    pub(in crate::lowering) fn try_plan_fused_predicate_condition(
        &mut self,
        condition: &Expression,
    ) -> Option<(Vec<Statement>, PairCondition)> {
        let (target, polarity) = strip_negations(condition);
        let predicate = self.queries().fused_predicate(target)?;
        Some(self.bind_fused_predicate(predicate, polarity, CommaOkValueSlot::Unused))
    }

    pub(in crate::lowering) fn try_plan_fused_predicate_value(
        &mut self,
        expression: &Expression,
        polarity: Polarity,
    ) -> Option<ValuePlan> {
        let predicate = self.queries().fused_predicate(expression)?;
        let (setup, condition) =
            self.bind_fused_predicate(predicate, polarity, CommaOkValueSlot::Discarded);
        debug_assert!(condition.initializer.is_none());
        Some(ValuePlan::plain_call(setup, condition.condition))
    }
}

impl<'p> PlannerQueries<'_, 'p> {
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
        let polarity = match definition {
            "prelude.Result.is_ok" | "prelude.Option.is_some" => Polarity::Positive,
            "prelude.Result.is_err" | "prelude.Option.is_none" => Polarity::Negated,
            _ => return None,
        };
        Some(FusedPredicate {
            producer: self.fallible_producer(receiver)?,
            polarity,
        })
    }
}
