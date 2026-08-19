use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use std::fmt::{self, Display, Formatter};
use syntax::ast::Expression;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandForm {
    Literal,
    Name,
    Call,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoExpression {
    rendered: String,
    contains_deferred_evaluation: bool,
    composite_literal: bool,
    syntax_form: OperandForm,
}

impl GoExpression {
    fn new(
        rendered: String,
        contains_deferred_evaluation: bool,
        composite_literal: bool,
        syntax_form: OperandForm,
    ) -> Self {
        Self {
            rendered,
            contains_deferred_evaluation,
            composite_literal,
            syntax_form,
        }
    }

    pub(crate) fn name(value: String) -> Self {
        Self::new(value, false, false, OperandForm::Name)
    }

    pub(crate) fn opaque(rendered: String) -> Self {
        Self::opaque_with_deferred_evaluation(rendered, false)
    }

    pub(crate) fn opaque_with_deferred_evaluation(
        rendered: String,
        contains_deferred_evaluation: bool,
    ) -> Self {
        Self::new(
            rendered,
            contains_deferred_evaluation,
            false,
            OperandForm::Other,
        )
    }

    pub(crate) fn literal(rendered: String) -> Self {
        Self::new(rendered, false, false, OperandForm::Literal)
    }

    pub(crate) fn call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        let mut rendered = format!("{callee}(");
        for (index, argument) in arguments.iter().enumerate() {
            if index > 0 {
                rendered.push_str(", ");
            }
            rendered.push_str(argument.as_str());
        }
        rendered.push(')');
        Self::new(rendered, true, false, OperandForm::Call)
    }

    pub(crate) fn receive(channel: GoExpression) -> Self {
        Self::opaque_with_deferred_evaluation(format!("<-{channel}"), true)
    }

    pub(crate) fn binary(
        left: GoExpression,
        operator: impl Into<String>,
        right: GoExpression,
    ) -> Self {
        Self::render_binary(left, operator.into(), right, " ")
    }

    pub(crate) fn compact_binary(
        left: GoExpression,
        operator: impl Into<String>,
        right: GoExpression,
    ) -> Self {
        Self::render_binary(left, operator.into(), right, "")
    }

    fn render_binary(left: Self, operator: String, right: Self, separator: &str) -> Self {
        let deferred = left.contains_deferred_evaluation() || right.contains_deferred_evaluation();
        Self::opaque_with_deferred_evaluation(
            format!("{left}{separator}{operator}{separator}{right}"),
            deferred,
        )
    }

    pub(crate) fn selector(base: GoExpression, field: String) -> Self {
        let deferred = base.contains_deferred_evaluation();
        Self::opaque_with_deferred_evaluation(format!("{base}.{field}"), deferred)
    }

    pub(crate) fn index(base: GoExpression, index: GoExpression) -> Self {
        let deferred = base.contains_deferred_evaluation() || index.contains_deferred_evaluation();
        Self::opaque_with_deferred_evaluation(format!("{base}[{index}]"), deferred)
    }

    pub(crate) fn conversion(go_type: String, value: GoExpression) -> Self {
        Self::opaque_with_deferred_evaluation(format!("{go_type}({value})"), true)
    }

    fn parenthesized(value: GoExpression) -> Self {
        Self::opaque_with_deferred_evaluation(format!("({value})"), true)
    }

    fn unary(operator: &str, value: GoExpression) -> Self {
        let deferred = value.contains_deferred_evaluation();
        Self::opaque_with_deferred_evaluation(render_unary(operator, &value.rendered()), deferred)
    }

    pub(crate) fn slice(
        base: GoExpression,
        start: Option<&GoExpression>,
        end: Option<&GoExpression>,
        capacity: Option<&GoExpression>,
    ) -> Self {
        let mut range = format!(
            "{}:{}",
            start.map(GoExpression::rendered).unwrap_or_default(),
            end.map(GoExpression::rendered).unwrap_or_default()
        );
        if let Some(capacity) = capacity {
            range.push(':');
            range.push_str(&capacity.rendered());
        }
        let contains_deferred_evaluation = start
            .into_iter()
            .chain(end)
            .chain(capacity)
            .any(GoExpression::contains_deferred_evaluation);
        Self::index(
            base,
            Self::opaque_with_deferred_evaluation(range, contains_deferred_evaluation),
        )
    }

    pub(crate) fn composite_literal(rendered: String, contains_deferred_evaluation: bool) -> Self {
        Self::new(
            rendered,
            contains_deferred_evaluation,
            true,
            OperandForm::Literal,
        )
    }

    pub(crate) fn rendered(&self) -> String {
        self.rendered.clone()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.rendered
    }

    pub(crate) fn is_composite_literal(&self) -> bool {
        self.composite_literal
    }

    pub(crate) fn is_literal(&self) -> bool {
        matches!(self.syntax_form, OperandForm::Literal)
    }

    fn syntax_form(&self) -> OperandForm {
        self.syntax_form
    }

    fn is_empty(&self) -> bool {
        self.rendered.is_empty()
    }

    pub(crate) fn contains_deferred_evaluation(&self) -> bool {
        self.contains_deferred_evaluation
    }
}

impl Display for GoExpression {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.rendered)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stability {
    Literal,
    /// A name nothing can rebind before its readers run.
    Fixed,
    Observable,
    StableAcrossCalls,
}

impl Stability {
    pub(crate) fn is_fixed(self) -> bool {
        matches!(self, Stability::Literal | Stability::Fixed)
    }

    pub(crate) fn is_observable(self) -> bool {
        !self.is_fixed()
    }

    pub(crate) fn is_stable_across_calls(self) -> bool {
        matches!(self, Stability::StableAcrossCalls)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EvaluationEffect {
    Pure,
    PureCall,
    EffectfulCall,
}

impl EvaluationEffect {
    pub(crate) fn combine(self, other: Self) -> Self {
        self.max(other)
    }

    pub(crate) fn has_call(self) -> bool {
        !matches!(self, EvaluationEffect::Pure)
    }

    pub(crate) fn has_effectful_call(self) -> bool {
        matches!(self, EvaluationEffect::EffectfulCall)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CaptureBoundary {
    #[default]
    SiblingSequence,
    DeferSite,
    TaskSite,
    LoopLifetime,
    AssignmentRightHandSide,
}

impl CaptureBoundary {
    pub(crate) fn requires_value_capture(self, stability: Stability) -> bool {
        !matches!(self, CaptureBoundary::SiblingSequence) && stability.is_observable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationFacts {
    pub form: OperandForm,
    pub stability: Stability,
    pub effect: EvaluationEffect,
}

impl EvaluationFacts {
    const fn new(form: OperandForm, stability: Stability, effect: EvaluationEffect) -> Self {
        Self {
            form,
            stability,
            effect,
        }
    }

    const fn literal() -> Self {
        Self::new(
            OperandForm::Literal,
            Stability::Literal,
            EvaluationEffect::Pure,
        )
    }

    const fn value(effect: EvaluationEffect) -> Self {
        Self::new(OperandForm::Other, Stability::Observable, effect)
    }

    const fn call(effect: EvaluationEffect) -> Self {
        Self::new(OperandForm::Call, Stability::StableAcrossCalls, effect)
    }

    const fn with_stability(self, stability: Stability) -> Self {
        Self { stability, ..self }
    }
}

pub(crate) struct ValuePlan {
    pub setup: Vec<LoweredStatement>,
    pub expression: GoExpression,
    pub evaluation: EvaluationFacts,
}

pub(crate) struct SequencedValues {
    pub setup: Vec<LoweredStatement>,
    pub values: Vec<GoExpression>,
    pub effect: EvaluationEffect,
}

impl SequencedValues {
    pub(crate) fn into_rendered(self) -> (Vec<LoweredStatement>, Vec<String>) {
        (
            self.setup,
            self.values
                .into_iter()
                .map(|value| value.rendered())
                .collect(),
        )
    }

    pub(crate) fn contains_deferred_evaluation(&self) -> bool {
        self.values
            .iter()
            .any(GoExpression::contains_deferred_evaluation)
    }
}

fn render_unary(op: &str, value: &str) -> String {
    if op == "-" && value.starts_with('-') {
        format!("-({value})")
    } else {
        format!("{op}{value}")
    }
}

impl ValuePlan {
    fn from_facts(
        setup: Vec<LoweredStatement>,
        expression: GoExpression,
        evaluation: EvaluationFacts,
    ) -> Self {
        Self {
            setup,
            expression,
            evaluation,
        }
    }

    pub(crate) fn literal(rendered: String) -> Self {
        Self::from_facts(
            Vec::new(),
            GoExpression::literal(rendered),
            EvaluationFacts::literal(),
        )
    }

    pub(crate) fn evaluated_literal(
        setup: Vec<LoweredStatement>,
        rendered: String,
        effect: EvaluationEffect,
    ) -> Self {
        Self::from_facts(
            setup,
            GoExpression::literal(rendered),
            EvaluationFacts::new(OperandForm::Literal, Stability::Observable, effect),
        )
    }

    /// A name the setup just bound, or which nothing can rebind.
    pub(crate) fn captured(setup: Vec<LoweredStatement>, name: String) -> Self {
        Self::from_facts(
            setup,
            GoExpression::name(name),
            EvaluationFacts::new(OperandForm::Name, Stability::Fixed, EvaluationEffect::Pure),
        )
    }

    pub(crate) fn computed(
        setup: Vec<LoweredStatement>,
        expression: GoExpression,
        effect: EvaluationEffect,
    ) -> Self {
        Self::from_facts(setup, expression, EvaluationFacts::value(effect))
    }

    /// `stability` describes the binding, so only a bare name inherits it
    /// whole: a composite form re-reads whatever it is built from.
    pub(crate) fn from_identifier_expression(
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        let evaluation = match expression.syntax_form() {
            OperandForm::Literal => EvaluationFacts::literal(),
            OperandForm::Call => EvaluationFacts::call(EvaluationEffect::PureCall),
            OperandForm::Name => {
                EvaluationFacts::new(OperandForm::Name, stability, EvaluationEffect::Pure)
            }
            OperandForm::Other => EvaluationFacts::value(EvaluationEffect::Pure).with_stability(
                if stability.is_observable() {
                    Stability::Observable
                } else {
                    Stability::StableAcrossCalls
                },
            ),
        };
        Self::from_facts(Vec::new(), expression, evaluation)
    }

    pub(crate) fn plain_call(
        setup: Vec<LoweredStatement>,
        expression: GoExpression,
        effect: EvaluationEffect,
    ) -> Self {
        Self::from_facts(setup, expression, EvaluationFacts::call(effect))
    }

    pub(crate) fn observable_call(
        setup: Vec<LoweredStatement>,
        expression: GoExpression,
        effect: EvaluationEffect,
    ) -> Self {
        Self::from_facts(
            setup,
            expression,
            EvaluationFacts::call(effect).with_stability(Stability::Observable),
        )
    }

    pub(crate) fn opaque(value: String) -> Self {
        Self::computed(
            Vec::new(),
            GoExpression::opaque(value),
            EvaluationEffect::Pure,
        )
    }

    pub(crate) fn map_rendered(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, String, bool) -> GoExpression,
    ) -> Self {
        let contains_deferred_evaluation = self.expression.contains_deferred_evaluation();
        let Self {
            mut setup,
            expression,
            evaluation,
        } = self;
        let expression = transform(
            &mut setup,
            expression.rendered(),
            contains_deferred_evaluation,
        );
        Self::from_facts(setup, expression, evaluation)
    }

    pub(crate) fn map_rendered_as_computed(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, String, bool) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_rendered(transform);
        plan.evaluation.form = OperandForm::Other;
        plan
    }

    pub(crate) fn map_rendered_as_name(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, String, bool) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_rendered(transform);
        plan.evaluation.form = OperandForm::Name;
        plan
    }

    pub(crate) fn map_rendered_as_observable_computed(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, String, bool) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_rendered_as_computed(transform);
        plan.make_observable();
        plan
    }

    pub(crate) fn make_observable(&mut self) {
        self.evaluation.stability = Stability::Observable;
    }

    pub(crate) fn stable_across_calls_if(mut self, stable_across_calls: bool) -> Self {
        if stable_across_calls {
            self.evaluation.stability = Stability::StableAcrossCalls;
        }
        self
    }

    pub(crate) fn make_observable_computed(&mut self) {
        self.evaluation.form = OperandForm::Other;
        self.make_observable();
    }

    pub(crate) fn into_addressed_location(mut self) -> Self {
        self.evaluation.form = OperandForm::Other;
        self.evaluation.stability = Stability::StableAcrossCalls;
        self
    }

    pub(crate) fn replace_with_pinned_name(&mut self, name: String) {
        self.expression = GoExpression::name(name);
        self.evaluation.form = OperandForm::Name;
        self.evaluation.effect = EvaluationEffect::Pure;
    }

    pub(crate) fn with_pure_constructor_evaluation(mut self) -> Self {
        self.evaluation.effect = EvaluationEffect::PureCall.combine(self.evaluation.effect);
        self.evaluation.stability = if matches!(self.evaluation.form, OperandForm::Call) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        };
        self
    }

    pub(crate) fn rendered(&self) -> String {
        self.expression.rendered()
    }

    /// `rendered` is a name this plan's own setup bound. Callers must also
    /// check that no source binding answers to it.
    pub(crate) fn rests_in_own_temp(&self, rendered: &str) -> bool {
        go_name::is_plain_identifier(rendered)
            && self
                .setup
                .last()
                .is_some_and(|statement| statement.binds_name(rendered))
    }

    pub(crate) fn rests_in_fixed_name(&self) -> bool {
        matches!(self.expression.syntax_form(), OperandForm::Name)
            && self.evaluation.stability.is_fixed()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.expression.is_empty()
    }

    pub(crate) fn into_parts(self) -> (Vec<LoweredStatement>, String) {
        (self.setup, self.expression.rendered())
    }

    pub(crate) fn parenthesized(mut self) -> Self {
        if matches!(self.evaluation.form, OperandForm::Call)
            || (self.evaluation.stability.is_stable_across_calls()
                && !matches!(self.evaluation.form, OperandForm::Name))
        {
            self.evaluation.stability = Stability::Observable;
        }
        self.evaluation.form = OperandForm::Other;
        self.expression = GoExpression::parenthesized(self.expression);
        self
    }

    pub(crate) fn conversion(mut self, go_type: String) -> Self {
        self.expression = GoExpression::conversion(go_type, self.expression);
        self.evaluation.form = OperandForm::Other;
        if !self.evaluation.stability.is_stable_across_calls() {
            self.evaluation.stability = Stability::Observable;
        }
        self
    }

    pub(crate) fn unary(mut self, operator: &'static str) -> Self {
        self.expression = GoExpression::unary(operator, self.expression);
        self.evaluation.form = OperandForm::Other;
        self.evaluation.stability = Stability::Observable;
        self
    }
}

impl Planner<'_> {
    /// Plan a value-position expression into a structured `ValuePlan`. Leaf
    /// kinds route through `plan_operand_leaf`.
    pub(crate) fn plan_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if self.is_test_log_call(expression) {
            let (setup, call) = self.lower_test_log_call(expression);
            return ValuePlan::plain_call(
                setup,
                GoExpression::opaque_with_deferred_evaluation(call, true),
                EvaluationEffect::EffectfulCall,
            );
        }
        match expression {
            Expression::Paren { expression, .. } => {
                self.plan_operand(expression, ctx).parenthesized()
            }
            Expression::Cast { expression, ty, .. } => self.plan_cast(expression, ty, ctx),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.plan_index_access(expression, index),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => self.plan_binary(operator, left, right, ctx),
            Expression::Unary {
                operator,
                expression,
                ..
            } => self.plan_unary(operator, expression, ctx),
            Expression::Tuple { elements, ty, .. } => self.plan_tuple_value(elements, ty, false),
            Expression::Range {
                start,
                end,
                inclusive,
                ty,
                ..
            } => self.plan_range_value(start, end, *inclusive, ty),
            Expression::StructCall {
                name,
                field_assignments,
                spread,
                ty,
                ..
            } => self.plan_struct_call(name, field_assignments, spread, ty, ctx),
            Expression::Reference {
                expression: inner,
                ty,
                ..
            } => self.plan_reference(inner, ty),
            Expression::DotAccess { .. } => self.plan_dot_access(expression, ctx),
            Expression::Task {
                expression: inner, ..
            } => self.plan_async_wrapper("go", inner),
            Expression::Defer {
                expression: inner, ..
            } => self.plan_async_wrapper("defer", inner),
            Expression::TryBlock { items, ty, .. } => self.lower_try_block(items, ty),
            Expression::RecoverBlock { items, ty, .. } => self.lower_recover_block(items, ty),
            Expression::Propagate { expression, .. } => {
                let (setup, value) = self.lower_propagate(expression, None);
                ValuePlan::computed(setup, GoExpression::opaque(value), EvaluationEffect::Pure)
            }
            Expression::If { ty, .. } => self.plan_if_as_operand_temp(expression, ty),
            Expression::Loop { ty, .. } => self.plan_loop_as_operand_temp(expression, ty),
            Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
                if !ty.is_never() =>
            {
                self.plan_branching_as_operand_temp(expression, ty)
            }
            Expression::Call { ty, .. } => self.lower_call_value(expression, ty, ctx),
            _ => self.plan_operand_leaf(expression, ctx),
        }
    }
}
