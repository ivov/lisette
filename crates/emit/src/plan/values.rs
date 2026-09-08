use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::names::packages::PackageUse;
use crate::plan::bodies::{LoweredBlock, LoweredStatement};
use crate::plan::go_expression::{
    CompositeElement, CompositeLayout, FunctionLiteralLayout, GoExpressionNode,
};
use std::fmt::{self, Display, Formatter};
use syntax::ast::Expression;
use syntax::types::SimpleKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandForm {
    Literal,
    Name,
    Call,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConstantKind {
    Int,
    Rune,
    Float,
    Complex,
    Bool,
    String,
}

impl ConstantKind {
    pub(crate) fn default_kind(self) -> SimpleKind {
        match self {
            Self::Int => SimpleKind::Int,
            Self::Rune => SimpleKind::Rune,
            Self::Float => SimpleKind::Float64,
            Self::Complex => SimpleKind::Complex128,
            Self::Bool => SimpleKind::Bool,
            Self::String => SimpleKind::String,
        }
    }

    fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Rune | Self::Float | Self::Complex)
    }

    pub(crate) fn join(self, other: Self) -> Option<Self> {
        (self.is_numeric() && other.is_numeric()).then(|| self.max(other))
    }
}

fn binary_constant(
    left: Option<ConstantKind>,
    operator: &str,
    right: Option<ConstantKind>,
) -> Option<ConstantKind> {
    let (left, right) = (left?, right?);
    match operator {
        "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" => Some(ConstantKind::Bool),
        "<<" | ">>" => left.is_numeric().then_some(left),
        "+" if left == ConstantKind::String && right == ConstantKind::String => {
            Some(ConstantKind::String)
        }
        "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "&^" => left.join(right),
        _ => None,
    }
}

fn unary_constant(operator: &str, value: Option<ConstantKind>) -> Option<ConstantKind> {
    let value = value?;
    match operator {
        "-" | "+" | "^" => value.is_numeric().then_some(value),
        "!" => (value == ConstantKind::Bool).then_some(value),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoExpression {
    node: GoExpressionNode,
    constant: Option<ConstantKind>,
}

impl GoExpression {
    fn new(node: GoExpressionNode) -> Self {
        Self {
            node,
            constant: None,
        }
    }

    pub(crate) fn constant(rendered: String, kind: ConstantKind) -> Self {
        Self::literal(rendered).with_constant(Some(kind))
    }

    pub(crate) fn with_constant(mut self, constant: Option<ConstantKind>) -> Self {
        self.constant = constant;
        self
    }

    pub(crate) fn constant_kind(&self) -> Option<ConstantKind> {
        self.constant
    }

    pub(crate) fn name(value: String) -> Self {
        debug_assert!(
            go_name::is_plain_identifier(&value),
            "an identifier node holds one Go name, got `{value}`"
        );
        Self::new(GoExpressionNode::Identifier(value))
    }

    pub(crate) fn qualified(package: PackageUse, name: impl Into<String>) -> Self {
        Self::new(GoExpressionNode::Qualified {
            package,
            name: name.into(),
        })
    }

    pub(crate) fn generated(package: GeneratedPackage, name: impl Into<String>) -> Self {
        Self::qualified(PackageUse::generated(package), name)
    }

    pub(crate) fn nil() -> Self {
        Self::literal("nil".to_string())
    }

    /// The value of a lowering that produced only statements.
    pub(crate) fn empty() -> Self {
        Self::new(GoExpressionNode::Empty)
    }

    pub(crate) fn verbatim(source: String) -> Self {
        Self::new(GoExpressionNode::Verbatim(source))
    }

    pub(crate) fn literal(rendered: String) -> Self {
        Self::new(GoExpressionNode::Literal(rendered))
    }

    pub(crate) fn type_name(go_type: String) -> Self {
        Self::new(GoExpressionNode::Type(go_type))
    }

    pub(crate) fn composite(
        go_type: Option<String>,
        elements: Vec<(Option<GoExpression>, GoExpression)>,
        layout: CompositeLayout,
    ) -> Self {
        let node = GoExpressionNode::CompositeLiteral {
            go_type,
            elements: elements
                .into_iter()
                .map(|(key, value)| CompositeElement {
                    key: key.map(|key| key.node),
                    value: value.node,
                })
                .collect(),
            layout,
        };
        Self::new(node)
    }

    /// `T{}` with no elements.
    pub(crate) fn empty_composite(go_type: String) -> Self {
        Self::composite(
            Some(go_type),
            Vec::new(),
            CompositeLayout::Inline { padded: true },
        )
    }

    /// The callee without its type arguments, for a call site that re-instantiates.
    pub(crate) fn without_instantiation(self) -> Self {
        match self.node {
            GoExpressionNode::Instantiation { base, .. } => Self::new(*base),
            _ => self,
        }
    }

    /// Drop the type of a composite literal nested in a literal of `element_type`.
    pub(crate) fn elide_composite_type(mut self, element_type: &str) -> Self {
        if let GoExpressionNode::CompositeLiteral { go_type, .. } = &mut self.node
            && go_type.as_deref() == Some(element_type)
        {
            *go_type = None;
        }
        self
    }

    pub(crate) fn instantiation(base: GoExpression, type_arguments: String) -> Self {
        if type_arguments.is_empty() {
            return base;
        }
        let node = GoExpressionNode::Instantiation {
            base: Box::new(base.node),
            type_arguments,
        };
        Self::new(node)
    }

    pub(crate) fn type_assertion(base: GoExpression, go_type: String) -> Self {
        let node = GoExpressionNode::TypeAssertion {
            base: Box::new(base.node),
            go_type,
        };
        Self::new(node)
    }

    pub(crate) fn address_of(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::AddressOf(Box::new(operand.node)))
    }

    pub(crate) fn dereference(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::Dereference(Box::new(operand.node)))
    }

    pub(crate) fn spread(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::Spread(Box::new(operand.node)))
    }

    pub(crate) fn function_literal(
        parameters: String,
        result: String,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    ) -> Self {
        Self::new(GoExpressionNode::FunctionLiteral {
            parameters,
            result,
            body,
            layout,
        })
    }

    pub(crate) fn immediate_call(
        result: String,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    ) -> Self {
        Self::call(
            Self::function_literal(String::new(), result, body, layout),
            Vec::new(),
        )
    }

    pub(crate) fn call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        let node = GoExpressionNode::Call {
            callee: Box::new(callee.node),
            arguments: arguments
                .into_iter()
                .map(|argument| argument.node)
                .collect(),
        };
        Self::new(node)
    }

    pub(crate) fn binary(
        left: GoExpression,
        operator: impl Into<String>,
        right: GoExpression,
    ) -> Self {
        let operator = operator.into();
        let constant = binary_constant(left.constant, &operator, right.constant);
        let node = GoExpressionNode::Binary {
            operator,
            left: Box::new(left.node),
            right: Box::new(right.node),
        };
        Self::new(node).with_constant(constant)
    }

    pub(crate) fn selector(base: GoExpression, field: String) -> Self {
        let node = GoExpressionNode::Selector {
            base: Box::new(base.node),
            field,
        };
        Self::new(node)
    }

    pub(crate) fn index(base: GoExpression, index: GoExpression) -> Self {
        let node = GoExpressionNode::Index {
            base: Box::new(base.node),
            index: Box::new(index.node),
        };
        Self::new(node)
    }

    pub(crate) fn conversion(go_type: String, value: GoExpression) -> Self {
        let node = GoExpressionNode::Conversion {
            go_type,
            operand: Box::new(value.node),
        };
        Self::new(node)
    }

    pub(crate) fn unary(operator: &str, value: GoExpression) -> Self {
        let constant = unary_constant(operator, value.constant);
        let node = GoExpressionNode::Unary {
            operator: operator.to_string(),
            operand: Box::new(value.node),
        };
        Self::new(node).with_constant(constant)
    }

    pub(crate) fn slice(
        base: GoExpression,
        start: Option<&GoExpression>,
        end: Option<&GoExpression>,
        capacity: Option<&GoExpression>,
    ) -> Self {
        let bound = |bound: Option<&GoExpression>| bound.map(|bound| Box::new(bound.node.clone()));
        let node = GoExpressionNode::Slice {
            base: Box::new(base.node),
            low: bound(start),
            high: bound(end),
            max: bound(capacity),
        };
        Self::new(node)
    }

    /// Lift a subtree taken out of another expression.
    pub(crate) fn from_node(node: GoExpressionNode) -> Self {
        Self::new(node)
    }

    pub(crate) fn node(&self) -> &GoExpressionNode {
        &self.node
    }

    pub(crate) fn rendered(&self) -> String {
        self.node.print()
    }

    pub(crate) fn print_header(&self) -> String {
        self.node.print_header()
    }

    pub(crate) fn rename_identifier(&mut self, from: &str, to: &str) {
        self.node.rename_identifier(from, to);
    }

    pub(crate) fn as_identifier(&self) -> Option<&str> {
        match &self.node {
            GoExpressionNode::Identifier(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn as_literal(&self) -> Option<&str> {
        match &self.node {
            GoExpressionNode::Literal(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn is_composite_literal(&self) -> bool {
        matches!(self.node, GoExpressionNode::CompositeLiteral { .. })
    }

    pub(crate) fn is_literal(&self) -> bool {
        matches!(self.syntax_form(), OperandForm::Literal)
    }

    pub(crate) fn syntax_form(&self) -> OperandForm {
        syntax_form(&self.node)
    }

    pub(crate) fn is_empty(&self) -> bool {
        match &self.node {
            GoExpressionNode::Empty => true,
            GoExpressionNode::Verbatim(source) => source.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn does_work(&self) -> bool {
        self.node.does_work()
    }
}

fn syntax_form(node: &GoExpressionNode) -> OperandForm {
    match node {
        GoExpressionNode::Identifier(_) | GoExpressionNode::Qualified { .. } => OperandForm::Name,
        GoExpressionNode::Literal(_) | GoExpressionNode::CompositeLiteral { .. } => {
            OperandForm::Literal
        }
        GoExpressionNode::Call { .. } => OperandForm::Call,
        GoExpressionNode::Instantiation { base, .. } => syntax_form(base),
        _ => OperandForm::Other,
    }
}

impl Display for GoExpression {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.node.print())
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
    DirectDelayedCall,
    DeferSite,
    TaskSite,
    LoopLifetime,
}

impl CaptureBoundary {
    pub(crate) fn requires_value_capture(self, stability: Stability) -> bool {
        !matches!(
            self,
            CaptureBoundary::SiblingSequence | CaptureBoundary::DirectDelayedCall
        ) && stability.is_observable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationFacts {
    pub stability: Stability,
    pub effect: EvaluationEffect,
}

impl EvaluationFacts {
    const fn new(stability: Stability, effect: EvaluationEffect) -> Self {
        Self { stability, effect }
    }

    const fn literal() -> Self {
        Self::new(Stability::Literal, EvaluationEffect::Pure)
    }

    const fn value(effect: EvaluationEffect) -> Self {
        Self::new(Stability::Observable, effect)
    }

    const fn call(effect: EvaluationEffect) -> Self {
        Self::new(Stability::StableAcrossCalls, effect)
    }

    const fn with_stability(self, stability: Stability) -> Self {
        Self { stability, ..self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValuePlan {
    pub setup: Vec<LoweredStatement>,
    pub expression: GoExpression,
    pub evaluation: EvaluationFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequencedValues {
    pub setup: Vec<LoweredStatement>,
    pub values: Vec<GoExpression>,
    pub effect: EvaluationEffect,
}

impl ValuePlan {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        for statement in &self.setup {
            statement.visit_expressions(visit);
        }
        self.expression.node().visit(visit);
    }

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

    pub(crate) fn constant(rendered: String, kind: ConstantKind) -> Self {
        Self::from_facts(
            Vec::new(),
            GoExpression::constant(rendered, kind),
            EvaluationFacts::literal(),
        )
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
            EvaluationFacts::new(Stability::Observable, effect),
        )
    }

    /// A name the setup just bound, or which nothing can rebind.
    pub(crate) fn captured(setup: Vec<LoweredStatement>, name: String) -> Self {
        Self::captured_with_effect(setup, name, EvaluationEffect::Pure)
    }

    pub(crate) fn captured_with_effect(
        setup: Vec<LoweredStatement>,
        name: String,
        effect: EvaluationEffect,
    ) -> Self {
        Self::from_facts(
            setup,
            GoExpression::name(name),
            EvaluationFacts::new(Stability::Fixed, effect),
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
            OperandForm::Name => EvaluationFacts::new(stability, EvaluationEffect::Pure),
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

    pub(crate) fn verbatim(source: String) -> Self {
        Self::computed(
            Vec::new(),
            GoExpression::verbatim(source),
            EvaluationEffect::Pure,
        )
    }

    pub(crate) fn map_expression(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, GoExpression) -> GoExpression,
    ) -> Self {
        let Self {
            mut setup,
            expression,
            evaluation,
        } = self;
        let expression = transform(&mut setup, expression);
        Self::from_facts(setup, expression, evaluation)
    }

    pub(crate) fn map_observable_expression(
        self,
        transform: impl FnOnce(&mut Vec<LoweredStatement>, GoExpression) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_expression(transform);
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

    pub(crate) fn into_addressed_location(mut self) -> Self {
        self.evaluation.stability = Stability::StableAcrossCalls;
        self
    }

    pub(crate) fn replace_with_pinned_name(&mut self, name: String) {
        self.expression = GoExpression::name(name);
        self.evaluation.effect = EvaluationEffect::Pure;
    }

    pub(crate) fn with_pure_constructor_evaluation(mut self) -> Self {
        self.evaluation.effect = EvaluationEffect::PureCall.combine(self.evaluation.effect);
        self.evaluation.stability = if matches!(self.expression.syntax_form(), OperandForm::Call) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        };
        self
    }

    pub(crate) fn with_stability(mut self, stability: Stability) -> Self {
        self.evaluation.stability = stability;
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

    pub(crate) fn into_parts(self) -> (Vec<LoweredStatement>, GoExpression) {
        (self.setup, self.expression)
    }

    pub(crate) fn conversion(mut self, go_type: String) -> Self {
        self.expression = GoExpression::conversion(go_type, self.expression);
        if !self.evaluation.stability.is_stable_across_calls() {
            self.evaluation.stability = Stability::Observable;
        }
        self
    }

    pub(crate) fn unary(mut self, operator: &'static str) -> Self {
        self.expression = GoExpression::unary(operator, self.expression);
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
            return ValuePlan::plain_call(setup, call, EvaluationEffect::EffectfulCall);
        }
        match expression {
            Expression::Paren { expression, .. } => self.plan_operand(expression, ctx),
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
            } => self.plan_struct_call(name, field_assignments, spread, ty),
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
                ValuePlan::computed(setup, value, EvaluationEffect::Pure)
            }
            Expression::If { ty, .. } => self.plan_branching_as_operand_temp(expression, ty),
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
