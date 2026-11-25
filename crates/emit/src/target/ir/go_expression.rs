use crate::target::ir::bodies::{LoweredBlock, LoweredStatement};
use crate::target::ir::local::GoIdentifier;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use crate::target::types::go_type::GoTypeShape;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use syntax::ast::BinaryOperator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Negate,
    Complement,
    Not,
}

impl UnaryOp {
    pub(crate) fn sign(self) -> char {
        match self {
            Self::Negate => '-',
            Self::Complement => '^',
            Self::Not => '!',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    BitAndNot,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinaryOp {
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::BitAndNot => "&^",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
        }
    }

    pub(crate) fn level(self) -> u8 {
        match self {
            Self::Mul
            | Self::Div
            | Self::Rem
            | Self::Shl
            | Self::Shr
            | Self::BitAnd
            | Self::BitAndNot => 5,
            Self::Add | Self::Sub | Self::BitOr | Self::BitXor => 4,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 3,
            Self::And => 2,
            Self::Or => 1,
        }
    }

    pub(crate) fn has_compound_form(self) -> bool {
        match self {
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::BitAndNot
            | Self::Shl
            | Self::Shr => true,
            Self::Eq
            | Self::Ne
            | Self::Lt
            | Self::Le
            | Self::Gt
            | Self::Ge
            | Self::And
            | Self::Or => false,
        }
    }
}

impl From<&BinaryOperator> for BinaryOp {
    fn from(operator: &BinaryOperator) -> Self {
        match operator {
            BinaryOperator::Addition => Self::Add,
            BinaryOperator::Subtraction => Self::Sub,
            BinaryOperator::Multiplication => Self::Mul,
            BinaryOperator::Division => Self::Div,
            BinaryOperator::Remainder => Self::Rem,
            BinaryOperator::BitwiseAnd => Self::BitAnd,
            BinaryOperator::BitwiseOr => Self::BitOr,
            BinaryOperator::BitwiseXor => Self::BitXor,
            BinaryOperator::BitwiseAndNot => Self::BitAndNot,
            BinaryOperator::ShiftLeft => Self::Shl,
            BinaryOperator::ShiftRight => Self::Shr,
            BinaryOperator::Equal => Self::Eq,
            BinaryOperator::NotEqual => Self::Ne,
            BinaryOperator::LessThan => Self::Lt,
            BinaryOperator::LessThanOrEqual => Self::Le,
            BinaryOperator::GreaterThan => Self::Gt,
            BinaryOperator::GreaterThanOrEqual => Self::Ge,
            BinaryOperator::And => Self::And,
            BinaryOperator::Or => Self::Or,
            BinaryOperator::Pipeline => {
                unreachable!("pipeline expressions are lowered during inference")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallPurity {
    Constructor,
    Conversion,
    ReadOnly,
    Effectful,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoExpressionNode {
    Identifier(GoIdentifier),
    Qualified {
        package: PackageUse,
        name: String,
    },
    Literal(String),
    Type(GoType),
    CompositeLiteral {
        go_type: Option<Box<GoType>>,
        elements: Vec<CompositeElement>,
        layout: CompositeLayout,
        end: SourcePosition,
    },
    Call {
        callee: Operand,
        arguments: Vec<Operand>,
        purity: CallPurity,
        paren: SourcePosition,
    },
    Instantiation {
        base: Operand,
        type_arguments: GoType,
    },
    Selector {
        base: Operand,
        field: String,
        may_panic: bool,
    },
    Index {
        base: Operand,
        index: Operand,
        in_bounds: bool,
    },
    Slice {
        base: Operand,
        low: Option<Operand>,
        high: Option<Operand>,
        max: Option<Operand>,
        in_bounds: bool,
    },
    TypeAssertion {
        base: Operand,
        go_type: GoType,
    },
    Unary {
        operator: UnaryOp,
        operand: Operand,
    },
    AddressOf(Operand),
    Dereference(Operand),
    Binary {
        operator: BinaryOp,
        left: Operand,
        right: Operand,
    },
    Conversion {
        go_type: GoType,
        operand: Operand,
    },
    Spread(Operand),
    FunctionLiteral {
        parameters: Vec<GoParameter>,
        result: Box<GoType>,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
        end: SourcePosition,
    },
    Empty,
    Verbatim(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoParameter {
    pub(crate) name: GoIdentifier,
    pub(crate) go_type: GoType,
}

impl GoParameter {
    pub(crate) fn new(name: impl Into<GoIdentifier>, go_type: impl Into<GoType>) -> Self {
        Self {
            name: name.into(),
            go_type: go_type.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionLiteralLayout {
    Inline,
    MultiLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompositeElement {
    pub key: Option<CompositeKey>,
    pub value: Operand,
    pub position: SourcePosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompositeKey {
    Field(String),
    Value(Operand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Position {
    pub(crate) path: Arc<str>,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

impl Position {
    pub(crate) fn inline_directive(&self) -> String {
        format!("/*line {}:{}:{}*/ ", self.path, self.line, self.column)
    }

    pub(crate) fn line_directive(&self) -> String {
        format!("//line {}:{}:{}\n", self.path, self.line, self.column)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SourcePosition(Option<Box<Position>>);

impl SourcePosition {
    pub(crate) fn get(&self) -> Option<&Position> {
        self.0.as_deref()
    }

    pub(crate) fn is_set(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn fill(&mut self, position: Option<Position>) {
        if self.0.is_none() {
            self.0 = position.map(Box::new);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
}

impl PartialEq for SourcePosition {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for SourcePosition {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Operand(Box<PositionedNode>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct PositionedNode {
    position: SourcePosition,
    node: GoExpressionNode,
}

impl Operand {
    pub(crate) fn new(position: SourcePosition, node: GoExpressionNode) -> Self {
        Self(Box::new(PositionedNode { position, node }))
    }

    pub(crate) fn into_parts(self) -> (SourcePosition, GoExpressionNode) {
        let PositionedNode { position, node } = *self.0;
        (position, node)
    }

    pub(crate) fn position(&self) -> &SourcePosition {
        &self.0.position
    }

    pub(crate) fn position_mut(&mut self) -> &mut SourcePosition {
        &mut self.0.position
    }
}

impl From<GoExpressionNode> for Operand {
    fn from(node: GoExpressionNode) -> Self {
        Self::new(SourcePosition::default(), node)
    }
}

impl Deref for Operand {
    type Target = GoExpressionNode;

    fn deref(&self) -> &GoExpressionNode {
        &self.0.node
    }
}

impl DerefMut for Operand {
    fn deref_mut(&mut self) -> &mut GoExpressionNode {
        &mut self.0.node
    }
}

impl AsRef<GoExpressionNode> for Operand {
    fn as_ref(&self) -> &GoExpressionNode {
        &self.0.node
    }
}

impl AsMut<GoExpressionNode> for Operand {
    fn as_mut(&mut self) -> &mut GoExpressionNode {
        &mut self.0.node
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompositeLayout {
    Inline { padded: bool },
    MultiLine { indented: bool },
}

impl CompositeLayout {
    pub(crate) fn for_elements(count: usize, widest: usize) -> Self {
        if count > 1 && widest > 30 {
            Self::MultiLine { indented: true }
        } else {
            Self::Inline { padded: true }
        }
    }
}

pub(crate) trait TreeVisitor {
    fn expression(&mut self, node: &GoExpressionNode);

    fn statement(&mut self, _statement: &LoweredStatement) {}
}

pub(crate) struct Expressions<'a, F>(pub(crate) &'a mut F);

impl<F: FnMut(&GoExpressionNode)> TreeVisitor for Expressions<'_, F> {
    fn expression(&mut self, node: &GoExpressionNode) {
        (self.0)(node)
    }
}

pub(crate) fn verbatim_identifiers(source: &str) -> impl Iterator<Item = &str> {
    source
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|token| {
            token
                .chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
        })
}

impl GoExpressionNode {
    pub(super) fn is_unit_struct_literal(&self) -> bool {
        let Self::CompositeLiteral {
            go_type: Some(go_type),
            elements,
            ..
        } = self
        else {
            return false;
        };
        elements.is_empty()
            && matches!(go_type.shape.as_ref(), GoTypeShape::Struct(fields) if fields.is_empty())
    }

    pub(crate) fn mentions_local(&self, name: &GoIdentifier) -> bool {
        let Some(id) = name.id() else {
            return false;
        };
        let mut found = false;
        self.visit(&mut |inner| {
            if let Self::Identifier(read) = inner
                && read.id() == Some(id)
            {
                found = true;
            }
        });
        found
    }

    pub(crate) fn has_call(&self) -> bool {
        let mut found = false;
        self.visit(&mut |inner| {
            if matches!(
                inner,
                Self::Call { .. } | Self::FunctionLiteral { .. } | Self::Verbatim(_)
            ) {
                found = true;
            }
        });
        found
    }

    pub(crate) fn mentions(&self, name: &str) -> bool {
        let mut found = false;
        self.visit(&mut |inner| {
            if let Self::Identifier(read) = inner
                && read.spelling() == name
            {
                found = true;
            }
        });
        found
    }

    pub(crate) fn fill_positions(&mut self, start: &Position, end: &Position) {
        match self {
            Self::Call { paren, .. } => paren.fill(Some(start.clone())),
            Self::FunctionLiteral { end: slot, .. }
            | Self::CompositeLiteral {
                end: slot,
                layout: CompositeLayout::MultiLine { .. },
                ..
            } => slot.fill(Some(end.clone())),
            _ => {}
        }
    }

    pub(crate) fn drop_positions(&mut self) {
        self.visit_operands_mut(&mut |operand| {
            operand.position_mut().clear();
            operand.drop_positions();
        });
    }

    pub(crate) fn visit(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        self.walk(&mut Expressions(visit));
    }

    pub(crate) fn own_types(&self, visit: &mut impl FnMut(&GoType)) {
        match self {
            Self::CompositeLiteral {
                go_type: Some(go_type),
                ..
            } => visit(go_type),
            Self::Type(go_type)
            | Self::TypeAssertion { go_type, .. }
            | Self::Conversion { go_type, .. }
            | Self::Instantiation {
                type_arguments: go_type,
                ..
            } => visit(go_type),
            Self::FunctionLiteral {
                parameters, result, ..
            } => {
                for parameter in parameters {
                    visit(&parameter.go_type);
                }
                visit(result);
            }
            _ => {}
        }
    }

    pub(crate) fn walk(&self, visit: &mut impl TreeVisitor) {
        visit.expression(self);
        if let Self::FunctionLiteral { body, .. } = self {
            body.walk(visit);
        } else {
            self.visit_children(&mut |child| child.walk(visit));
        }
    }

    pub(crate) fn visit_children(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        self.visit_operands(&mut |operand| visit(operand));
    }

    pub(crate) fn visit_children_mut(&mut self, visit: &mut impl FnMut(&mut GoExpressionNode)) {
        self.visit_operands_mut(&mut |operand| visit(operand));
    }

    fn visit_operands(&self, visit: &mut impl FnMut(&Operand)) {
        match self {
            Self::Identifier(_)
            | Self::Qualified { .. }
            | Self::Literal(_)
            | Self::Type(_)
            | Self::Empty
            | Self::Verbatim(_)
            | Self::FunctionLiteral { .. } => {}
            Self::CompositeLiteral { elements, .. } => {
                for element in elements {
                    if let Some(CompositeKey::Value(key)) = &element.key {
                        visit(key);
                    }
                    visit(&element.value);
                }
            }
            Self::Call {
                callee, arguments, ..
            } => {
                visit(callee);
                for argument in arguments {
                    visit(argument);
                }
            }
            Self::Instantiation { base, .. }
            | Self::Selector { base, .. }
            | Self::TypeAssertion { base, .. } => visit(base),
            Self::Index { base, index, .. } => {
                visit(base);
                visit(index);
            }
            Self::Slice {
                base,
                low,
                high,
                max,
                ..
            } => {
                visit(base);
                for bound in [low, high, max].into_iter().flatten() {
                    visit(bound);
                }
            }
            Self::Unary { operand, .. }
            | Self::Conversion { operand, .. }
            | Self::AddressOf(operand)
            | Self::Dereference(operand)
            | Self::Spread(operand) => visit(operand),
            Self::Binary { left, right, .. } => {
                visit(left);
                visit(right);
            }
        }
    }

    fn visit_operands_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        match self {
            Self::Identifier(_)
            | Self::Qualified { .. }
            | Self::Literal(_)
            | Self::Type(_)
            | Self::Empty
            | Self::Verbatim(_)
            | Self::FunctionLiteral { .. } => {}
            Self::CompositeLiteral { elements, .. } => {
                for element in elements {
                    if let Some(CompositeKey::Value(key)) = &mut element.key {
                        visit(key);
                    }
                    visit(&mut element.value);
                }
            }
            Self::Call {
                callee, arguments, ..
            } => {
                visit(callee);
                for argument in arguments {
                    visit(argument);
                }
            }
            Self::Instantiation { base, .. }
            | Self::Selector { base, .. }
            | Self::TypeAssertion { base, .. } => visit(base),
            Self::Index { base, index, .. } => {
                visit(base);
                visit(index);
            }
            Self::Slice {
                base,
                low,
                high,
                max,
                ..
            } => {
                visit(base);
                for bound in [low, high, max].into_iter().flatten() {
                    visit(bound);
                }
            }
            Self::Unary { operand, .. }
            | Self::Conversion { operand, .. }
            | Self::AddressOf(operand)
            | Self::Dereference(operand)
            | Self::Spread(operand) => visit(operand),
            Self::Binary { left, right, .. } => {
                visit(left);
                visit(right);
            }
        }
    }
}
