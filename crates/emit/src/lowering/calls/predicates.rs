use crate::Planner;
use crate::lowering::abi::fallible_producer::FallibleProducer;
use crate::lowering::calls::bound_value::{InExpression, InIfHeader};
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

    fn relative_to(self, requested: Self) -> Self {
        if self == requested {
            Self::Positive
        } else {
            Self::Negated
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
    pub(in crate::lowering) fn try_plan_fused_predicate_condition(
        &mut self,
        condition: &Expression,
    ) -> Option<(Vec<Statement>, PairCondition)> {
        let (target, polarity) = strip_negations(condition);
        let predicate = self.queries().fused_predicate(target)?;
        let polarity = predicate.polarity.relative_to(polarity);
        Some(predicate.producer.bind_test::<InIfHeader>(self, polarity))
    }

    pub(in crate::lowering) fn try_plan_fused_predicate_value(
        &mut self,
        expression: &Expression,
        polarity: Polarity,
    ) -> Option<ValuePlan> {
        let predicate = self.queries().fused_predicate(expression)?;
        let polarity = predicate.polarity.relative_to(polarity);
        let (setup, condition) = predicate.producer.bind_test::<InExpression>(self, polarity);
        Some(ValuePlan::plain_call(setup, condition))
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

        let method = self.facts.checked(callee).callee().method()?;
        let polarity = match (method.owner.as_str(), method.name.as_str()) {
            ("prelude.Result", "is_ok") | ("prelude.Option", "is_some") => Polarity::Positive,
            ("prelude.Result", "is_err") | ("prelude.Option", "is_none") => Polarity::Negated,
            _ => return None,
        };
        Some(FusedPredicate {
            producer: self.fallible_producer(receiver)?,
            polarity,
        })
    }
}
