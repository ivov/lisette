use crate::target::ir::bodies::LoweredBlock;
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::go_expression::{
    CallPurity, CompositeElement, CompositeKey, CompositeLayout, FunctionLiteralLayout,
    GoBinaryOperator, GoExpressionNode, GoParameter, GoUnaryOperator, Operand, Position,
    SourcePosition,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;

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
    operator: GoBinaryOperator,
    right: Option<ConstantKind>,
) -> Option<ConstantKind> {
    let left = left?;
    match (operator, right) {
        (GoBinaryOperator::ShiftLeft | GoBinaryOperator::ShiftRight, _) => {
            left.is_numeric().then_some(left)
        }
        (_, None) => None,
        (
            GoBinaryOperator::Equal
            | GoBinaryOperator::NotEqual
            | GoBinaryOperator::LessThan
            | GoBinaryOperator::LessThanOrEqual
            | GoBinaryOperator::GreaterThan
            | GoBinaryOperator::GreaterThanOrEqual
            | GoBinaryOperator::And
            | GoBinaryOperator::Or,
            Some(_),
        ) => Some(ConstantKind::Bool),
        (GoBinaryOperator::Addition, Some(ConstantKind::String))
            if left == ConstantKind::String =>
        {
            Some(ConstantKind::String)
        }
        (
            GoBinaryOperator::Addition
            | GoBinaryOperator::Subtraction
            | GoBinaryOperator::Multiplication
            | GoBinaryOperator::Division
            | GoBinaryOperator::Remainder
            | GoBinaryOperator::BitwiseAnd
            | GoBinaryOperator::BitwiseOr
            | GoBinaryOperator::BitwiseXor
            | GoBinaryOperator::BitwiseAndNot,
            Some(right),
        ) => left.join(right),
    }
}

fn unary_constant(operator: GoUnaryOperator, value: Option<ConstantKind>) -> Option<ConstantKind> {
    let value = value?;
    match operator {
        GoUnaryOperator::Negate | GoUnaryOperator::Complement => {
            value.is_numeric().then_some(value)
        }
        GoUnaryOperator::Not => (value == ConstantKind::Bool).then_some(value),
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

    pub(crate) fn constant(literal: GoLiteral, kind: ConstantKind) -> Self {
        Self::literal(literal).with_constant(Some(kind))
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
        Self::literal(GoLiteral::Nil)
    }

    pub(crate) fn empty() -> Self {
        Self::new(GoExpressionNode::Empty)
    }

    pub(crate) fn verbatim(source: String) -> Self {
        Self::new(GoExpressionNode::Verbatim(source))
    }

    pub(crate) fn literal(literal: GoLiteral) -> Self {
        Self::new(GoExpressionNode::Literal(literal))
    }

    pub(crate) fn boolean(value: bool) -> Self {
        Self::literal(GoLiteral::Bool(value))
    }

    pub(crate) fn integer(value: i128) -> Self {
        Self::literal(GoLiteral::Int(value))
    }

    pub(crate) fn token(text: String) -> Self {
        Self::literal(GoLiteral::Token(text))
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
        Self::empty_composite(GoType::structure(Vec::new()))
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

    pub(crate) fn unchecked_function_literal(
        parameters: Vec<GoParameter>,
        result: GoType,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    ) -> Self {
        Self::new(GoExpressionNode::FunctionLiteral {
            parameters,
            result: Box::new(result),
            body,
            layout,
            end: SourcePosition::default(),
        })
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

    pub(crate) fn binary(
        left: GoExpression,
        operator: GoBinaryOperator,
        right: GoExpression,
    ) -> Self {
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

    pub(crate) fn unary(operator: GoUnaryOperator, value: GoExpression) -> Self {
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

    pub(crate) fn source_position(&self) -> &SourcePosition {
        &self.position
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

    pub(crate) fn as_identifier(&self) -> Option<&GoIdentifier> {
        match &self.node {
            GoExpressionNode::Identifier(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn as_boolean(&self) -> Option<bool> {
        match &self.node {
            GoExpressionNode::Literal(GoLiteral::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_integer(&self) -> Option<i128> {
        match &self.node {
            GoExpressionNode::Literal(GoLiteral::Int(value)) => Some(*value),
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

    pub(crate) fn is_observable(self) -> bool {
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
