use crate::target::ir::bodies::Statement;
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::{GoExpressionNode, UnaryOp};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{ConstantKind, GoExpression, OperandForm, Stability};
use crate::target::types::go_type::GoType;
use std::mem;
use syntax::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::lowering) struct ValuePlan {
    setup: Vec<Statement>,
    expression: GoExpression,
    stability: Stability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::lowering) struct SequencedValues {
    pub setup: Vec<Statement>,
    pub values: Vec<GoExpression>,
    pub stability: Stability,
}

impl ValuePlan {
    pub(in crate::lowering) fn setup(&self) -> &[Statement] {
        &self.setup
    }

    pub(in crate::lowering) fn expression(&self) -> &GoExpression {
        &self.expression
    }

    pub(in crate::lowering) fn stability(&self) -> Stability {
        self.stability
    }

    pub(in crate::lowering) fn into_parts_with_stability(
        self,
    ) -> (Vec<Statement>, GoExpression, Stability) {
        (self.setup, self.expression, self.stability)
    }

    pub(in crate::lowering) fn split_setup(self) -> (Vec<Statement>, Self) {
        let Self {
            setup,
            expression,
            stability,
        } = self;
        (setup, Self::new(Vec::new(), expression, stability))
    }

    pub(in crate::lowering) fn new(
        setup: Vec<Statement>,
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        Self {
            setup,
            expression,
            stability,
        }
    }

    pub(in crate::lowering) fn constant(rendered: String, kind: ConstantKind) -> Self {
        Self::new(
            Vec::new(),
            GoExpression::constant(rendered, kind),
            Stability::Fixed,
        )
    }

    pub(in crate::lowering) fn literal(rendered: String) -> Self {
        Self::new(
            Vec::new(),
            GoExpression::literal(rendered),
            Stability::Fixed,
        )
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
        let Self {
            mut setup,
            expression,
            stability,
        } = self;
        let expression = transform(&mut setup, expression);
        Self::new(setup, expression, stability)
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
        self.stability = Stability::Observable;
    }

    pub(in crate::lowering) fn pin(
        &mut self,
        bind: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoIdentifier,
    ) {
        let value = mem::replace(&mut self.expression, GoExpression::empty());
        let name = bind(&mut self.setup, value);
        self.expression = GoExpression::identifier(name);
    }

    pub(in crate::lowering) fn with_constructor_stability(mut self) -> Self {
        self.stability = if matches!(self.expression.syntax_form(), OperandForm::Call) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        };
        self
    }

    pub(in crate::lowering) fn with_stability(mut self, stability: Stability) -> Self {
        self.stability = stability;
        self
    }

    pub(in crate::lowering) fn binds_local(&self, local: &GoIdentifier) -> bool {
        self.setup
            .iter()
            .any(|statement| statement.kind.binds_local(local))
    }

    pub(in crate::lowering) fn effects(&self) -> Effects {
        let reads = if self.reads_only_own_setup() {
            Stability::Fixed
        } else {
            self.stability
        };
        self.expression.effects().with_known_reads(reads)
    }

    fn reads_only_own_setup(&self) -> bool {
        if self.setup.is_empty() || self.expression.effects().reads() == Stability::Observable {
            return false;
        }
        let mut own = true;
        self.expression.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                own &= self.binds_local(name);
            }
        });
        own
    }

    pub(in crate::lowering) fn can_delay(&self) -> bool {
        self.stability.is_fixed() && !self.effects().panics_or_blocks()
    }

    pub(in crate::lowering) fn must_evaluate_now(&self) -> bool {
        !self.can_delay()
    }

    pub(in crate::lowering) fn rests_in_fixed_name(&self) -> bool {
        matches!(self.expression.syntax_form(), OperandForm::Name) && self.stability.is_fixed()
    }

    pub(in crate::lowering) fn is_empty(&self) -> bool {
        self.expression.is_empty()
    }

    pub(in crate::lowering) fn into_parts(self) -> (Vec<Statement>, GoExpression) {
        (self.setup, self.expression)
    }

    pub(in crate::lowering) fn conversion(mut self, go_type: impl Into<GoType>) -> Self {
        self.expression = GoExpression::conversion(go_type, self.expression);
        if !self.stability.is_stable_across_calls() {
            self.stability = Stability::Observable;
        }
        self
    }

    pub(in crate::lowering) fn unary(mut self, operator: UnaryOp) -> Self {
        self.expression = GoExpression::unary(operator, self.expression);
        self.stability = Stability::Observable;
        self
    }

    pub(in crate::lowering) fn dereference(mut self) -> Self {
        self.expression = GoExpression::dereference(self.expression);
        self.stability = Stability::Observable;
        self
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
