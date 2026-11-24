use crate::EmitFacts;
use crate::ir::bodies::AsyncKind;
use crate::ir::bodies::{LoweredBlock, Statement};
use crate::ir::evaluation::Effects;
use crate::ir::go_expression::{
    BinaryOp, CallPurity, CompositeElement, CompositeKey, CompositeLayout, FunctionLiteralLayout,
    GoExpressionNode, GoParameter, Operand, Position, SourcePosition, UnaryOp,
};
use crate::ir::local::GoIdentifier;
use crate::names::go_name::GeneratedPackage;
use crate::names::packages::PackageUse;
use crate::passes::cleanup::drop_unreachable_statements;
use crate::passes::verify::verify_final_function_body;
use crate::plan::never::runs_as_never_statement;
use crate::render::writer::GoWriter;
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use crate::{Planner, written_span};
use std::fmt::{self, Display, Formatter};
use std::mem;
use syntax::ast::{Expression, Span};

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
    pub(crate) fn is_default_go_type(self, go_type: &GoType) -> bool {
        match self {
            Self::Int => go_type.is_named("int"),
            Self::Rune => go_type.is_named("rune") || go_type.is_named("int32"),
            Self::Float => go_type.is_named("float64"),
            Self::Complex => go_type.is_named("complex128"),
            Self::Bool => go_type.is_named("bool"),
            Self::String => go_type.is_named("string"),
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
    operator: BinaryOp,
    right: Option<ConstantKind>,
) -> Option<ConstantKind> {
    let left = left?;
    if matches!(operator, BinaryOp::Shl | BinaryOp::Shr) {
        return left.is_numeric().then_some(left);
    }
    let right = right?;
    match operator {
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::And
        | BinaryOp::Or => Some(ConstantKind::Bool),
        BinaryOp::Add if left == ConstantKind::String && right == ConstantKind::String => {
            Some(ConstantKind::String)
        }
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Rem
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::BitAndNot => left.join(right),
        BinaryOp::Shl | BinaryOp::Shr => unreachable!("shifts return above"),
    }
}

fn unary_constant(operator: UnaryOp, value: Option<ConstantKind>) -> Option<ConstantKind> {
    let value = value?;
    match operator {
        UnaryOp::Negate | UnaryOp::Complement => value.is_numeric().then_some(value),
        UnaryOp::Not => (value == ConstantKind::Bool).then_some(value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoExpression {
    node: GoExpressionNode,
    constant: Option<ConstantKind>,
    position: SourcePosition,
}

impl GoExpression {
    fn new(node: GoExpressionNode) -> Self {
        Self {
            node,
            constant: None,
            position: SourcePosition::default(),
        }
    }

    pub(crate) fn into_operand(self) -> Operand {
        Operand::new(self.position, self.node)
    }

    pub(crate) fn from_operand(operand: Operand) -> Self {
        let (position, node) = operand.into_parts();
        Self {
            position,
            ..Self::new(node)
        }
    }

    pub(crate) fn peeled(&self, operand: &Operand) -> Self {
        let mut peeled = Self::from_operand(operand.clone());
        if !peeled.position.is_set() {
            peeled.position = self.position.clone();
        }
        peeled
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

    pub(crate) fn identifier(value: GoIdentifier) -> Self {
        assert!(
            !value.is_pending(),
            "identifier expressions need an explicit target"
        );
        Self::new(GoExpressionNode::Identifier(value))
    }

    pub(crate) fn external_name(value: String) -> Self {
        Self::new(GoExpressionNode::Identifier(GoIdentifier::external(value)))
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

    pub(crate) fn empty() -> Self {
        Self::new(GoExpressionNode::Empty)
    }

    pub(crate) fn verbatim(source: String) -> Self {
        Self::new(GoExpressionNode::Verbatim(source))
    }

    pub(crate) fn literal(rendered: String) -> Self {
        Self::new(GoExpressionNode::Literal(rendered))
    }

    pub(crate) fn type_name(go_type: impl Into<GoType>) -> Self {
        Self::new(GoExpressionNode::Type(go_type.into()))
    }

    pub(crate) fn composite(
        go_type: Option<impl Into<GoType>>,
        elements: Vec<(Option<GoExpression>, GoExpression)>,
        layout: CompositeLayout,
    ) -> Self {
        let node = GoExpressionNode::CompositeLiteral {
            go_type: go_type.map(|go_type| Box::new(go_type.into())),
            elements: elements
                .into_iter()
                .map(|(key, value)| CompositeElement {
                    key: key.map(|key| CompositeKey::Value(key.into_operand())),
                    value: value.into_operand(),
                    position: SourcePosition::default(),
                })
                .collect(),
            layout,
            end: SourcePosition::default(),
        };
        Self::new(node)
    }

    pub(crate) fn struct_literal(
        go_type: impl Into<GoType>,
        fields: Vec<(String, GoExpression)>,
        layout: CompositeLayout,
    ) -> Self {
        let node = GoExpressionNode::CompositeLiteral {
            go_type: Some(Box::new(go_type.into())),
            elements: fields
                .into_iter()
                .map(|(name, value)| CompositeElement {
                    key: Some(CompositeKey::Field(name)),
                    value: value.into_operand(),
                    position: SourcePosition::default(),
                })
                .collect(),
            layout,
            end: SourcePosition::default(),
        };
        Self::new(node)
    }

    pub(crate) fn unit() -> Self {
        Self::empty_composite(GoType::new("struct{}"))
    }

    pub(crate) fn empty_composite(go_type: impl Into<GoType>) -> Self {
        Self::composite(
            Some(go_type),
            Vec::new(),
            CompositeLayout::Inline { padded: true },
        )
    }

    pub(crate) fn without_instantiation(self) -> Self {
        match &self.node {
            GoExpressionNode::Instantiation { base, .. } => self.peeled(base),
            _ => self,
        }
    }

    pub(crate) fn elide_composite_type(mut self, element_type: &GoType) -> Self {
        if let GoExpressionNode::CompositeLiteral { go_type, .. } = &mut self.node
            && go_type.as_deref() == Some(element_type)
        {
            *go_type = None;
        }
        self
    }

    pub(crate) fn instantiation(base: GoExpression, type_arguments: impl Into<GoType>) -> Self {
        let type_arguments = type_arguments.into();
        if type_arguments.is_empty() {
            return base;
        }
        let node = GoExpressionNode::Instantiation {
            base: base.into_operand(),
            type_arguments,
        };
        Self::new(node)
    }

    pub(crate) fn type_assertion(base: GoExpression, go_type: impl Into<GoType>) -> Self {
        let node = GoExpressionNode::TypeAssertion {
            base: base.into_operand(),
            go_type: go_type.into(),
        };
        Self::new(node)
    }

    pub(crate) fn address_of(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::AddressOf(operand.into_operand()))
    }

    pub(crate) fn dereference(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::Dereference(operand.into_operand()))
    }

    pub(crate) fn new_zero(go_type: GoType) -> Self {
        Self::dereference(Self::call(
            Self::external_name("new".to_string()),
            vec![Self::type_name(go_type)],
        ))
    }

    pub(crate) fn spread(operand: GoExpression) -> Self {
        Self::new(GoExpressionNode::Spread(operand.into_operand()))
    }

    pub(crate) fn function_literal(
        parameters: Vec<GoParameter>,
        result: impl Into<GoType>,
        mut body: LoweredBlock,
        layout: FunctionLiteralLayout,
    ) -> Self {
        let result = result.into();
        drop_unreachable_statements(&mut body.statements);
        if !result.is_empty() {
            body.ensure_go_termination();
        }
        if cfg!(debug_assertions) {
            verify_final_function_body(&mut body, !result.is_empty())
                .unwrap_or_else(|error| panic!("{error}"));
        }
        Self::new(GoExpressionNode::FunctionLiteral {
            parameters,
            result: Box::new(result),
            body,
            layout,
            end: SourcePosition::default(),
        })
    }

    pub(crate) fn immediate_call(
        result: impl Into<GoType>,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    ) -> Self {
        Self::call(
            Self::function_literal(Vec::new(), result, body, layout),
            Vec::new(),
        )
    }

    pub(crate) fn call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        Self::call_node(callee, arguments, CallPurity::Effectful)
    }

    pub(crate) fn pure_call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        Self::call_node(callee, arguments, CallPurity::Constructor)
    }

    pub(crate) fn type_name_call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        Self::call_node(callee, arguments, CallPurity::Conversion)
    }

    pub(crate) fn read_only_call(callee: GoExpression, arguments: Vec<GoExpression>) -> Self {
        Self::call_node(callee, arguments, CallPurity::ReadOnly)
    }

    pub(crate) fn into_read_only_call(mut self) -> Self {
        if let GoExpressionNode::Call { purity, .. } = &mut self.node
            && *purity == CallPurity::Effectful
        {
            *purity = CallPurity::ReadOnly;
        }
        self
    }

    fn call_node(callee: GoExpression, arguments: Vec<GoExpression>, purity: CallPurity) -> Self {
        let node = GoExpressionNode::Call {
            callee: callee.into_operand(),
            arguments: arguments
                .into_iter()
                .map(GoExpression::into_operand)
                .collect(),
            purity,
            paren: SourcePosition::default(),
        };
        Self::new(node)
    }

    pub(crate) fn binary(left: GoExpression, operator: BinaryOp, right: GoExpression) -> Self {
        let constant = binary_constant(left.constant, operator, right.constant);
        let node = GoExpressionNode::Binary {
            operator,
            left: left.into_operand(),
            right: right.into_operand(),
        };
        Self::new(node).with_constant(constant)
    }

    pub(crate) fn selector(base: GoExpression, field: String) -> Self {
        let node = GoExpressionNode::Selector {
            base: base.into_operand(),
            field,
            may_panic: true,
        };
        Self::new(node)
    }

    pub(crate) fn value_field(base: GoExpression, field: String) -> Self {
        let node = GoExpressionNode::Selector {
            base: base.into_operand(),
            field,
            may_panic: false,
        };
        Self::new(node)
    }

    pub(crate) fn index(base: GoExpression, index: GoExpression) -> Self {
        let node = GoExpressionNode::Index {
            base: base.into_operand(),
            index: index.into_operand(),
            in_bounds: false,
        };
        Self::new(node)
    }

    pub(crate) fn conversion(go_type: impl Into<GoType>, value: GoExpression) -> Self {
        let node = GoExpressionNode::Conversion {
            go_type: go_type.into(),
            operand: value.into_operand(),
        };
        Self::new(node)
    }

    pub(crate) fn unary(operator: UnaryOp, value: GoExpression) -> Self {
        let constant = unary_constant(operator, value.constant);
        let node = GoExpressionNode::Unary {
            operator,
            operand: value.into_operand(),
        };
        Self::new(node).with_constant(constant)
    }

    pub(crate) fn slice(
        base: GoExpression,
        start: Option<&GoExpression>,
        end: Option<&GoExpression>,
        capacity: Option<&GoExpression>,
    ) -> Self {
        let bound = |bound: Option<&GoExpression>| bound.map(|bound| bound.clone().into_operand());
        let node = GoExpressionNode::Slice {
            base: base.into_operand(),
            low: bound(start),
            high: bound(end),
            max: bound(capacity),
            in_bounds: false,
        };
        Self::new(node)
    }

    pub(crate) fn proven_in_bounds(mut self) -> Self {
        if let GoExpressionNode::Index { in_bounds, .. }
        | GoExpressionNode::Slice { in_bounds, .. } = &mut self.node
        {
            *in_bounds = true;
        }
        self
    }

    pub(crate) fn node(&self) -> &GoExpressionNode {
        &self.node
    }

    pub(crate) fn with_node(&self, node: GoExpressionNode) -> Self {
        Self {
            position: self.position.clone(),
            ..Self::new(node)
        }
    }

    pub(crate) fn position_elements(
        mut self,
        mut position: impl FnMut(usize, &CompositeElement) -> Option<Position>,
    ) -> Self {
        if let GoExpressionNode::CompositeLiteral {
            elements,
            layout: CompositeLayout::MultiLine { .. },
            ..
        } = &mut self.node
        {
            for (index, element) in elements.iter_mut().enumerate() {
                element.position.fill(position(index, element));
            }
        }
        self
    }

    pub(crate) fn without_positions(mut self) -> Self {
        self.node.drop_positions();
        self.position.clear();
        self
    }

    pub(crate) fn position(&self) -> Option<&Position> {
        self.position.get()
    }

    pub(crate) fn positioned(mut self, position: Position) -> Self {
        if !self.is_empty() {
            self.position.fill(Some(position));
        }
        self
    }

    pub(crate) fn node_mut(&mut self) -> &mut GoExpressionNode {
        &mut self.node
    }

    pub(crate) fn rendered(&self) -> String {
        self.node.print()
    }

    pub(crate) fn write(&self, output: &mut GoWriter) {
        output.push_position(&self.position);
        self.node.write_into(output);
    }

    pub(crate) fn write_header(&self, output: &mut GoWriter) {
        output.push_position(&self.position);
        self.node.write_header_into(output);
    }

    pub(crate) fn write_list(values: &[GoExpression], output: &mut GoWriter) {
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            value.write(output);
        }
    }

    pub(crate) fn as_identifier(&self) -> Option<&GoIdentifier> {
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

    pub(crate) fn stands_as_a_statement(&self) -> bool {
        match &self.node {
            GoExpressionNode::Verbatim(_) => true,
            GoExpressionNode::Call { callee, purity, .. } => {
                *purity != CallPurity::Conversion && !is_value_only_callee(callee)
            }
            _ => false,
        }
    }

    pub(crate) fn effects(&self) -> Effects {
        self.node.effects()
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stability {
    #[default]
    Fixed,
    StableAcrossCalls,
    Observable,
}

impl Stability {
    pub(crate) fn is_fixed(self) -> bool {
        matches!(self, Stability::Fixed)
    }

    fn is_observable(self) -> bool {
        !self.is_fixed()
    }

    pub(crate) fn is_stable_across_calls(self) -> bool {
        matches!(self, Stability::StableAcrossCalls)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CaptureBoundary {
    #[default]
    SiblingSequence,
    DirectDelayedCall,
    Delayed,
}

impl CaptureBoundary {
    pub(crate) fn delays_reads(self) -> bool {
        !matches!(
            self,
            CaptureBoundary::SiblingSequence | CaptureBoundary::DirectDelayedCall
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValuePlan {
    setup: Vec<Statement>,
    expression: GoExpression,
    stability: Stability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequencedValues {
    pub setup: Vec<Statement>,
    pub values: Vec<GoExpression>,
    pub stability: Stability,
}

impl ValuePlan {
    pub(crate) fn setup(&self) -> &[Statement] {
        &self.setup
    }

    pub(crate) fn expression(&self) -> &GoExpression {
        &self.expression
    }

    pub(crate) fn stability(&self) -> Stability {
        self.stability
    }

    pub(crate) fn into_parts_with_stability(self) -> (Vec<Statement>, GoExpression, Stability) {
        (self.setup, self.expression, self.stability)
    }

    pub(crate) fn split_setup(self) -> (Vec<Statement>, Self) {
        let Self {
            setup,
            expression,
            stability,
        } = self;
        (setup, Self::new(Vec::new(), expression, stability))
    }

    fn new(setup: Vec<Statement>, expression: GoExpression, stability: Stability) -> Self {
        Self {
            setup,
            expression,
            stability,
        }
    }

    pub(crate) fn constant(rendered: String, kind: ConstantKind) -> Self {
        Self::new(
            Vec::new(),
            GoExpression::constant(rendered, kind),
            Stability::Fixed,
        )
    }

    pub(crate) fn literal(rendered: String) -> Self {
        Self::new(
            Vec::new(),
            GoExpression::literal(rendered),
            Stability::Fixed,
        )
    }

    pub(crate) fn captured(setup: Vec<Statement>, name: GoIdentifier) -> Self {
        Self::new(setup, GoExpression::identifier(name), Stability::Fixed)
    }

    pub(crate) fn computed(setup: Vec<Statement>, expression: GoExpression) -> Self {
        Self::new(setup, expression, Stability::Observable)
    }

    pub(crate) fn from_identifier_expression(
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        let stability = match expression.syntax_form() {
            OperandForm::Literal => Stability::Fixed,
            OperandForm::Call => Stability::StableAcrossCalls,
            OperandForm::Name => stability,
            OperandForm::Other if stability.is_observable() => Stability::Observable,
            OperandForm::Other => Stability::StableAcrossCalls,
        };
        Self::new(Vec::new(), expression, stability)
    }

    pub(crate) fn plain_call(setup: Vec<Statement>, expression: GoExpression) -> Self {
        Self::new(setup, expression, Stability::StableAcrossCalls)
    }

    pub(crate) fn built_from(
        setup: Vec<Statement>,
        expression: GoExpression,
        stability: Stability,
    ) -> Self {
        Self::new(setup, expression, stability)
    }

    pub(crate) fn verbatim(source: String) -> Self {
        Self::computed(Vec::new(), GoExpression::verbatim(source))
    }

    pub(crate) fn map_expression(
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

    pub(crate) fn map_observable_expression(
        self,
        transform: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoExpression,
    ) -> Self {
        let mut plan = self.map_expression(transform);
        plan.make_observable();
        plan
    }

    pub(crate) fn make_observable(&mut self) {
        self.stability = Stability::Observable;
    }

    pub(crate) fn pin(
        &mut self,
        bind: impl FnOnce(&mut Vec<Statement>, GoExpression) -> GoIdentifier,
    ) {
        let value = mem::replace(&mut self.expression, GoExpression::empty());
        let name = bind(&mut self.setup, value);
        self.expression = GoExpression::identifier(name);
    }

    pub(crate) fn with_constructor_stability(mut self) -> Self {
        self.stability = if matches!(self.expression.syntax_form(), OperandForm::Call) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        };
        self
    }

    pub(crate) fn with_stability(mut self, stability: Stability) -> Self {
        self.stability = stability;
        self
    }

    pub(crate) fn binds_local(&self, local: &GoIdentifier) -> bool {
        self.setup
            .iter()
            .any(|statement| statement.kind.binds_local(local))
    }

    pub(crate) fn effects(&self) -> Effects {
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

    pub(crate) fn can_delay(&self) -> bool {
        self.stability.is_fixed() && !self.effects().panics_or_blocks()
    }

    pub(crate) fn rests_in_fixed_name(&self) -> bool {
        matches!(self.expression.syntax_form(), OperandForm::Name) && self.stability.is_fixed()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.expression.is_empty()
    }

    pub(crate) fn into_parts(self) -> (Vec<Statement>, GoExpression) {
        (self.setup, self.expression)
    }

    pub(crate) fn conversion(mut self, go_type: impl Into<GoType>) -> Self {
        self.expression = GoExpression::conversion(go_type, self.expression);
        if !self.stability.is_stable_across_calls() {
            self.stability = Stability::Observable;
        }
        self
    }

    pub(crate) fn unary(mut self, operator: UnaryOp) -> Self {
        self.expression = GoExpression::unary(operator, self.expression);
        self.stability = Stability::Observable;
        self
    }

    pub(crate) fn dereference(mut self) -> Self {
        self.expression = GoExpression::dereference(self.expression);
        self.stability = Stability::Observable;
        self
    }
}

impl Planner<'_> {
    pub(crate) fn plan_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self.plan_unpositioned_operand(expression, ctx);
        self.facts
            .with_source_positions(&written_span(expression), plan)
    }

    fn plan_unpositioned_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if self.facts.is_test_log_call(expression) {
            let (setup, call) = self.plan_test_log_call(expression);
            return ValuePlan::plain_call(setup, call);
        }
        if runs_as_never_statement(expression) && self.facts.is_never(&expression.get_type()) {
            return self.plan_never_value(expression);
        }
        match expression {
            Expression::Paren { .. } => {
                unreachable!("internal error: emit received a parenthesized expression")
            }
            Expression::Cast {
                expression: operand,
                ty,
                ..
            } => self.plan_cast(expression, operand, ty, ctx),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.plan_index_access(expression, index),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => {
                let conversion = self.facts.checked(expression).operand_conversion();
                self.plan_binary(operator, left, right, conversion, ctx)
            }
            Expression::Unary {
                operator,
                expression,
                ..
            } => self.plan_unary(operator, expression, ctx),
            Expression::Tuple { .. } => self.plan_tuple_value(expression, false),
            Expression::Range { start, end, ty, .. } => self.plan_range_value(start, end, ty),
            Expression::StructCall { .. } => self.plan_struct_call(expression),
            Expression::Reference {
                expression: inner, ..
            } => self.plan_reference(inner),
            Expression::DotAccess { .. } => self.plan_dot_access(expression, ctx),
            Expression::Task {
                expression: inner, ..
            } => self.plan_async_wrapper(AsyncKind::Task, inner),
            Expression::Defer {
                expression: inner, ..
            } => self.plan_async_wrapper(AsyncKind::Defer, inner),
            Expression::TryBlock { items, ty, .. } => {
                let ty = self.fallible_block_type(expression, ty);
                self.plan_try_block(items, &ty)
            }
            Expression::RecoverBlock { items, ty, .. } => {
                let ty = self.fallible_block_type(expression, ty);
                self.plan_recover_block(items, &ty)
            }
            Expression::Propagate { expression, .. } => {
                let (setup, value) = self.plan_propagate(expression);
                let stability = self.path_read_stability(&value);
                ValuePlan::built_from(setup, value, stability)
            }
            Expression::Loop { ty, .. } => self.plan_loop_as_operand_temp(expression, ty),
            Expression::If { ty, .. }
            | Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. } => self.plan_branching_as_operand_temp(expression, ty),
            _ => self.plan_operand_leaf(expression, ctx),
        }
    }
}

fn is_value_only_callee(callee: &GoExpressionNode) -> bool {
    match callee {
        GoExpressionNode::Identifier(name) => name.id().is_none() && is_value_only_builtin(name),
        GoExpressionNode::Type(_) => true,
        GoExpressionNode::Instantiation { base, .. } => matches!(**base, GoExpressionNode::Type(_)),
        _ => false,
    }
}

fn is_value_only_builtin(name: &str) -> bool {
    matches!(
        name,
        "append" | "cap" | "complex" | "imag" | "len" | "make" | "max" | "min" | "new" | "real"
    )
}

impl EmitFacts<'_> {
    pub(crate) fn with_source_positions(&self, span: &Span, plan: ValuePlan) -> ValuePlan {
        if !self.sourcemap() {
            return plan;
        }
        let (Some(start), Some(end)) = (self.source_position(span), self.end_position(span)) else {
            return plan;
        };
        plan.map_expression(|_, mut value| {
            value.node_mut().fill_positions(&start, &end);
            value.positioned(start.clone())
        })
    }
}

impl GoExpressionNode {
    pub(crate) fn new_zero_type(&self) -> Option<&GoType> {
        if let Self::Dereference(operand) = self
            && let Self::Call {
                callee, arguments, ..
            } = operand.as_ref()
            && let Self::Identifier(name) = callee.as_ref()
            && name.id().is_none()
            && name.spelling() == "new"
            && let [argument] = arguments.as_slice()
            && let Self::Type(go_type) = argument.as_ref()
        {
            return Some(go_type);
        }
        None
    }
}
