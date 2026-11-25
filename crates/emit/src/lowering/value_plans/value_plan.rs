use crate::target::ir::bodies::Statement;
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::go_expression::{GoExpressionNode, GoUnaryOperator};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{ConstantKind, GoExpression, OperandForm, Stability};
use crate::target::types::go_type::GoType;
use std::mem;
use syntax::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::lowering) struct ValuePlan {
    setup: Vec<Statement>,
    value: PlannedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannedValue {
    Go {
        expression: GoExpression,
        stability: Stability,
    },
    Diverges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::lowering) struct SequencedValues {
    pub setup: Vec<Statement>,
    pub values: Vec<GoExpression>,
    pub stability: Stability,
}

impl ValuePlan {
    pub(in crate::lowering) fn diverging(setup: Vec<Statement>) -> Self {
        Self {
            setup,
            value: PlannedValue::Diverges,
        }
    }

    pub(in crate::lowering) fn diverges(&self) -> bool {
        matches!(self.value, PlannedValue::Diverges)
    }

    pub(in crate::lowering) fn setup(&self) -> &[Statement] {
        &self.setup
    }

    pub(in crate::lowering) fn expression(&self) -> Option<&GoExpression> {
        match &self.value {
            PlannedValue::Go { expression, .. } => Some(expression),
            PlannedValue::Diverges => None,
        }
    }

    pub(in crate::lowering) fn stability(&self) -> Stability {
        match &self.value {
            PlannedValue::Go { stability, .. } => *stability,
            PlannedValue::Diverges => Stability::Observable,
        }
    }

    pub(in crate::lowering) fn into_parts_with_stability(
        self,
    ) -> (Vec<Statement>, GoExpression, Stability) {
        let stability = self.stability();
        let (setup, expression) = self.into_parts();
        (setup, expression, stability)
    }

    pub(in crate::lowering) fn split_setup(self) -> (Vec<Statement>, Self) {
        let Self { setup, value } = self;
        (
            setup,
            Self {
                setup: Vec::new(),
                value,
            },
        )
    }

    pub(in crate::lowering) fn new(
        setup: Vec<Statement>,
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        Self {
            setup,
            value: PlannedValue::Go {
                expression,
                stability,
            },
        }
    }

    pub(in crate::lowering) fn constant(literal: GoLiteral, kind: ConstantKind) -> Self {
        Self::new(
            Vec::new(),
            GoExpression::constant(literal, kind),
            Stability::Fixed,
        )
    }

    pub(in crate::lowering) fn nil() -> Self {
        Self::new(Vec::new(), GoExpression::nil(), Stability::Fixed)
    }

    pub(in crate::lowering) fn captured(setup: Vec<Statement>, name: GoIdentifier) -> Self {
        Self::new(setup, GoExpression::identifier(name), Stability::Fixed)
    }

    pub(in crate::lowering) fn computed(setup: Vec<Statement>, expression: GoExpression) -> Self {
        Self::new(setup, expression, Stability::Observable)
    }

    pub(in crate::lowering) fn from_identifier_expression(
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        let stability = match expression.syntax_form() {
            OperandForm::Literal => Stability::Fixed,
            OperandForm::Name => stability,
            OperandForm::Other if stability.is_observable() => Stability::Observable,
            OperandForm::Call | OperandForm::Other => Stability::StableAcrossCalls,
        };

        Self::new(Vec::new(), expression, stability)
    }

    pub(in crate::lowering) fn plain_call(setup: Vec<Statement>, expression: GoExpression) -> Self {
        Self::new(setup, expression, Stability::StableAcrossCalls)
    }

    pub(in crate::lowering) fn verbatim(source: String) -> Self {
        Self::computed(Vec::new(), GoExpression::verbatim(source))
    }

    pub(in crate::lowering) fn map_expression(
        self,
        transform: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoExpression,
    ) -> Self {
        let Self { mut setup, value } = self;
        let value = match value {
            PlannedValue::Go {
                expression,
                stability,
            } => PlannedValue::Go {
                expression: transform(&mut setup, expression),
                stability,
            },
            PlannedValue::Diverges => PlannedValue::Diverges,
        };
        Self { setup, value }
    }

    pub(in crate::lowering) fn map_observable_expression(
        self,
        transform: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_expression(transform);
        plan.make_observable();
        plan
    }

    pub(in crate::lowering) fn make_observable(&mut self) {
        self.set_stability(Stability::Observable);
    }

    fn set_stability(&mut self, stability: Stability) {
        if let PlannedValue::Go {
            stability: current, ..
        } = &mut self.value
        {
            *current = stability;
        }
    }

    pub(in crate::lowering) fn pin(
        &mut self,
        bind: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoIdentifier,
    ) {
        if let PlannedValue::Go { expression, .. } = &mut self.value {
            let value = mem::replace(expression, GoExpression::empty());
            *expression = GoExpression::identifier(bind(&mut self.setup, value));
        }
    }

    pub(in crate::lowering) fn with_constructor_stability(mut self) -> Self {
        let stability = match self.expression().map(GoExpression::syntax_form) {
            Some(OperandForm::Call) => Stability::StableAcrossCalls,
            _ => Stability::Observable,
        };
        self.set_stability(stability);
        self
    }

    pub(in crate::lowering) fn with_stability(mut self, stability: Stability) -> Self {
        self.set_stability(stability);
        self
    }

    pub(in crate::lowering) fn binds_local(&self, local: &GoIdentifier) -> bool {
        self.setup
            .iter()
            .any(|statement| statement.kind.binds_local(local))
    }

    pub(in crate::lowering) fn effects(&self) -> Effects {
        let PlannedValue::Go { expression, .. } = &self.value else {
            return Effects::anything();
        };
        let reads = if self.reads_only_own_setup(expression) {
            Stability::Fixed
        } else {
            self.stability()
        };
        expression.effects().with_known_reads(reads)
    }

    fn reads_only_own_setup(&self, expression: &GoExpression) -> bool {
        if self.setup.is_empty() || expression.effects().reads() == Stability::Observable {
            return false;
        }
        let mut own = true;
        expression.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                own &= self.binds_local(name);
            }
        });
        own
    }

    pub(in crate::lowering) fn can_delay(&self) -> bool {
        self.stability().is_fixed() && !self.effects().panics_or_blocks()
    }

    pub(in crate::lowering) fn must_evaluate_now(&self) -> bool {
        !self.can_delay()
    }

    pub(in crate::lowering) fn rests_in_fixed_name(&self) -> bool {
        self.expression()
            .is_some_and(|expression| matches!(expression.syntax_form(), OperandForm::Name))
            && self.stability().is_fixed()
    }

    pub(in crate::lowering) fn is_empty(&self) -> bool {
        self.expression().is_none_or(GoExpression::is_empty)
    }

    pub(in crate::lowering) fn into_parts(self) -> (Vec<Statement>, GoExpression) {
        match self.value {
            PlannedValue::Go { expression, .. } => (self.setup, expression),
            PlannedValue::Diverges => (self.setup, GoExpression::empty()),
        }
    }

    pub(in crate::lowering) fn conversion(self, go_type: impl Into<GoType>) -> Self {
        let keeps_stability = self.stability().is_stable_across_calls();
        let mut plan =
            self.map_expression(|_, expression| GoExpression::conversion(go_type, expression));
        if !keeps_stability {
            plan.make_observable();
        }
        plan
    }

    pub(in crate::lowering) fn unary(self, operator: GoUnaryOperator) -> Self {
        self.map_observable_expression(|_, expression| GoExpression::unary(operator, expression))
    }

    pub(in crate::lowering) fn dereference(self) -> Self {
        self.map_observable_expression(|_, expression| GoExpression::dereference(expression))
    }
}

pub(in crate::lowering) enum PlacePlan<'a> {
    Statement,
    Return,
    Assign {
        local: &'a GoExpression,
        target_ty: Option<&'a Type>,
    },
}

impl PlacePlan<'_> {
    pub(in crate::lowering) fn is_return(&self) -> bool {
        matches!(self, PlacePlan::Return)
    }
}
