//! The Go expression tree behind `GoExpression`.

use crate::names::packages::PackageUse;
use crate::plan::bodies::{LoweredBlock, LoweredStatement};
use crate::plan::local::GoIdentifier;
use crate::render::Renderer;
use crate::render::writer::GoWriter;
use crate::types::go_type::{GoType, conversion_prefix};
use crate::utils::group_params;
use std::ops::{Deref, DerefMut};
use std::slice;
use std::sync::Arc;
use syntax::ast::BinaryOperator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Negate,
    Complement,
    Not,
}

impl UnaryOp {
    fn sign(self) -> char {
        match self {
            Self::Negate => '-',
            Self::Complement => '^',
            Self::Not => '!',
        }
    }
}

/// `|>` has no variant, inference lowers it away.
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

    fn level(self) -> u8 {
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

/// What a call does besides evaluating its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallPurity {
    /// Runs no code of its own.
    Constructor,
    /// Reads memory, but writes none and runs no caller code.
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
    /// A Go type in operand position, such as the element type of `make`.
    Type(GoType),
    CompositeLiteral {
        /// `None` inside an enclosing literal that already names the element type.
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
    /// A generic function or type with its type arguments, `f[T, U]`.
    Instantiation {
        base: Operand,
        /// Bracketed Go type list, `[T, U]`.
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
    },
    Slice {
        base: Operand,
        low: Option<Operand>,
        high: Option<Operand>,
        max: Option<Operand>,
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
    /// A variadic argument, `values...`.
    Spread(Operand),
    FunctionLiteral {
        parameters: Vec<GoParameter>,
        result: Box<GoType>,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
        end: SourcePosition,
    },
    Empty,
    /// Go source the program supplied through `@rawgo`.
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

    fn pair(&self) -> (String, String) {
        (self.name.to_string(), self.go_type.code.clone())
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
    /// A struct field name, which never reads a local.
    Field(String),
    /// A map key or an array index.
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

    pub(crate) fn from_line_directive(directive: &str) -> Option<Self> {
        let directive = directive.strip_prefix("//line ")?.trim_end();
        let (rest, column) = directive.rsplit_once(':')?;
        let (path, line) = rest.rsplit_once(':')?;
        Some(Self {
            path: path.into(),
            line: line.parse().ok()?,
            column: column.parse().ok()?,
        })
    }
}

/// Trees that differ only in their positions are equal.
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

    fn write(&self, output: &mut GoWriter, slot: Slot) {
        output.push_position(&self.0.position);
        self.0.node.write(output, slot);
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

/// The whitespace forms match the text the string emitters produced, because
/// element widths feed the layout choice of an enclosing literal. `gofmt`
/// erases the difference in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompositeLayout {
    /// `{ a, b }`, or `{a, b}` when not padded.
    Inline { padded: bool },
    /// One element per line, each behind a tab when indented.
    MultiLine { indented: bool },
}

impl CompositeLayout {
    /// One element per line once several wide elements would crowd one line.
    pub(crate) fn for_elements(count: usize, widest: usize) -> Self {
        if count > 1 && widest > 30 {
            Self::MultiLine { indented: true }
        } else {
            Self::Inline { padded: true }
        }
    }
}

/// A walk over a lowered body. A closure visits only the expressions.
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
        matches!(
            self,
            Self::CompositeLiteral {
                go_type: Some(go_type),
                elements,
                ..
            } if go_type.code == "struct{}" && elements.is_empty()
        )
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

    /// The Go types this node spells itself, not those of its children.
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

    pub(super) fn visit_children_mut(&mut self, visit: &mut impl FnMut(&mut GoExpressionNode)) {
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
            Self::Index { base, index } => {
                visit(base);
                visit(index);
            }
            Self::Slice {
                base,
                low,
                high,
                max,
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
            Self::Index { base, index } => {
                visit(base);
                visit(index);
            }
            Self::Slice {
                base,
                low,
                high,
                max,
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

    pub(crate) fn print(&self) -> String {
        let mut output = GoWriter::default();
        self.write_into(&mut output);
        output.into_text()
    }

    pub(crate) fn write_into(&self, output: &mut GoWriter) {
        self.write(output, Slot::FREE);
    }

    pub(crate) fn write_header_into(&self, output: &mut GoWriter) {
        self.write(output, Slot::HEADER);
    }

    fn binding(&self) -> Binding {
        match self {
            Self::Literal(text) if text.starts_with(['-', '+']) => {
                Binding::Prefix(text.chars().next().unwrap_or(' '))
            }
            Self::Identifier(_)
            | Self::Qualified { .. }
            | Self::Literal(_)
            | Self::Type(_)
            | Self::CompositeLiteral { .. }
            | Self::Call { .. }
            | Self::Instantiation { .. }
            | Self::Selector { .. }
            | Self::Index { .. }
            | Self::Slice { .. }
            | Self::TypeAssertion { .. }
            | Self::Conversion { .. }
            | Self::Spread(_)
            | Self::FunctionLiteral { .. }
            | Self::Empty => Binding::Primary,
            Self::Unary { operator, .. } => Binding::Prefix(operator.sign()),
            Self::AddressOf(_) => Binding::Prefix('&'),
            Self::Dereference(_) => Binding::Prefix('*'),
            Self::Binary { operator, .. } => Binding::Binary(operator.level()),
            Self::Verbatim(_) => Binding::Unknown,
        }
    }

    fn needs_parens(&self, slot: Slot) -> bool {
        if slot.header
            && matches!(
                self,
                Self::CompositeLiteral {
                    go_type: Some(_),
                    ..
                }
            )
        {
            return true;
        }
        match (self.binding(), slot.kind) {
            (Binding::Unknown, SlotKind::Free) => false,
            (Binding::Unknown, _) => true,
            (Binding::Primary, SlotKind::Postfix) => {
                matches!(self, Self::Literal(_) | Self::FunctionLiteral { .. })
            }
            (Binding::Primary, _) => false,
            (Binding::Prefix(sign), SlotKind::Prefix(operator)) => {
                sign == operator && matches!(sign, '-' | '+' | '&')
            }
            (Binding::Prefix(_), SlotKind::Callee | SlotKind::Postfix) => true,
            (Binding::Prefix(_), _) => false,
            (Binding::Binary(level), SlotKind::Left(parent)) => level < parent,
            (Binding::Binary(level), SlotKind::Right(parent)) => level <= parent,
            (Binding::Binary(_), SlotKind::Free) => false,
            (Binding::Binary(_), _) => true,
        }
    }

    fn write_child(child: &Operand, slot: Slot, output: &mut GoWriter) {
        if child.needs_parens(slot) {
            output.push('(');
            child.write(output, Slot::FREE);
            output.push(')');
        } else {
            child.write(output, slot);
        }
    }

    fn write(&self, output: &mut GoWriter, slot: Slot) {
        match self {
            Self::Identifier(text) => output.push_str(text.spelling()),
            Self::Literal(text) => output.push_str(text),
            Self::Verbatim(text) => output.push_foreign(text),
            Self::Type(go_type) => output.push_str(&go_type.code),
            Self::Qualified { package, name } => {
                output.push_str(package.qualifier());
                output.push('.');
                output.push_str(name);
            }
            Self::Empty => {}
            Self::CompositeLiteral {
                go_type,
                elements,
                layout,
                end,
            } => {
                if let Some(go_type) = go_type {
                    output.push_str(&go_type.code);
                }
                match layout {
                    _ if elements.is_empty() => output.push_str("{}"),
                    CompositeLayout::Inline { padded } => {
                        let padding = if *padded { " " } else { "" };
                        output.push('{');
                        output.push_str(padding);
                        for (index, element) in elements.iter().enumerate() {
                            if index > 0 {
                                output.push_str(", ");
                            }
                            element.write(output);
                        }
                        output.push_str(padding);
                        output.push('}');
                    }
                    CompositeLayout::MultiLine { indented } => {
                        let start = output.line();
                        output.push_str("{\n");
                        for element in elements {
                            if *indented {
                                output.push('\t');
                            }
                            element.write(output);
                            output.push_str(",\n");
                        }
                        output.push('}');
                        output.close_literal(end, start);
                    }
                }
            }
            Self::Call {
                callee,
                arguments,
                paren,
                ..
            } => {
                Self::write_child(callee, slot.with(SlotKind::Callee), output);
                output.push_position(paren);
                output.push('(');
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    argument.write(output, Slot::FREE);
                }
                output.push(')');
            }
            Self::Instantiation {
                base,
                type_arguments,
            } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push_str(&type_arguments.code);
            }
            Self::Selector { base, field, .. } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push('.');
                output.push_str(field);
            }
            Self::Index { base, index } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push('[');
                index.write(output, Slot::FREE);
                output.push(']');
            }
            Self::Slice {
                base,
                low,
                high,
                max,
            } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push('[');
                if let Some(low) = low {
                    low.write(output, Slot::FREE);
                }
                output.push(':');
                if let Some(high) = high {
                    high.write(output, Slot::FREE);
                }
                if let Some(max) = max {
                    output.push(':');
                    max.write(output, Slot::FREE);
                }
                output.push(']');
            }
            Self::TypeAssertion { base, go_type } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push_str(".(");
                output.push_str(&go_type.code);
                output.push(')');
            }
            Self::Unary { operator, operand } => {
                let sign = operator.sign();
                output.push(sign);
                Self::write_child(operand, slot.with(SlotKind::Prefix(sign)), output);
            }
            Self::AddressOf(operand) => {
                output.push('&');
                Self::write_child(operand, slot.with(SlotKind::Prefix('&')), output);
            }
            Self::Dereference(operand) => {
                output.push('*');
                Self::write_child(operand, slot.with(SlotKind::Prefix('*')), output);
            }
            Self::Binary {
                operator,
                left,
                right,
            } => {
                let level = operator.level();
                Self::write_child(left, slot.with(SlotKind::Left(level)), output);
                output.push(' ');
                output.push_str(operator.symbol());
                output.push(' ');
                Self::write_child(right, slot.with(SlotKind::Right(level)), output);
            }
            Self::Conversion { go_type, operand } => {
                output.push_str(&conversion_prefix(&go_type.code));
                operand.write(output, Slot::FREE);
                output.push(')');
            }
            Self::Spread(operand) => {
                Self::write_child(operand, slot.with(SlotKind::Postfix), output);
                output.push_str("...");
            }
            Self::FunctionLiteral {
                parameters,
                result,
                body,
                layout,
                end,
            } => {
                output.push_str("func(");
                let pairs: Vec<(String, String)> =
                    parameters.iter().map(GoParameter::pair).collect();
                output.push_str(&group_params(&pairs));
                output.push(')');
                if !result.code.is_empty() {
                    output.push(' ');
                    output.push_str(&result.code);
                }
                output.push_str(" {");
                let lines: Vec<String> = body
                    .statements
                    .iter()
                    .map(|statement| Renderer.render_setup(slice::from_ref(statement), false))
                    .collect();
                let single_line = lines
                    .iter()
                    .all(|line| line.trim_end_matches('\n').lines().count() <= 1);
                let start = output.line();
                if matches!(layout, FunctionLiteralLayout::Inline) && single_line {
                    let joined = lines
                        .iter()
                        .map(|line| line.trim_end_matches('\n'))
                        .collect::<Vec<_>>()
                        .join("; ");
                    if !joined.is_empty() {
                        output.push(' ');
                        if output.directives() {
                            let outer = output.set_inline(true);
                            Renderer.render_lowered_block(output, body);
                            output.set_inline(outer);
                        } else {
                            output.push_str(&joined);
                        }
                        output.push(' ');
                    }
                    output.push('}');
                    output.close_literal(&SourcePosition::default(), start);
                } else {
                    output.push('\n');
                    if output.directives() {
                        Renderer.render_lowered_block(output, body);
                    } else {
                        for line in &lines {
                            output.push_str(line);
                        }
                    }
                    output.push('}');
                    output.close_literal(end, start);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Slot {
    kind: SlotKind,
    header: bool,
}

impl Slot {
    const FREE: Self = Self {
        kind: SlotKind::Free,
        header: false,
    };
    const HEADER: Self = Self {
        kind: SlotKind::Free,
        header: true,
    };

    fn with(self, kind: SlotKind) -> Self {
        Self {
            kind,
            header: self.header,
        }
    }
}

#[derive(Clone, Copy)]
enum SlotKind {
    Free,
    Left(u8),
    Right(u8),
    Prefix(char),
    Callee,
    Postfix,
}

enum Binding {
    Primary,
    Prefix(char),
    Binary(u8),
    Unknown,
}

impl CompositeElement {
    fn write(&self, output: &mut GoWriter) {
        output.push_position(&self.position);
        if let Some(key) = &self.key {
            match key {
                CompositeKey::Field(name) => output.push_str(name),
                CompositeKey::Value(key) => key.write(output, Slot::FREE),
            }
            output.push_str(": ");
        }
        self.value.write(output, Slot::FREE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> GoExpressionNode {
        GoExpressionNode::Identifier(text.to_string().into())
    }

    fn binary(
        left: GoExpressionNode,
        operator: BinaryOp,
        right: GoExpressionNode,
    ) -> GoExpressionNode {
        GoExpressionNode::Binary {
            operator,
            left: Operand::from(left),
            right: Operand::from(right),
        }
    }

    fn header(node: &GoExpressionNode) -> String {
        let mut output = GoWriter::default();
        node.write_header_into(&mut output);
        output.into_text()
    }

    fn literal_struct(go_type: &str) -> GoExpressionNode {
        GoExpressionNode::CompositeLiteral {
            go_type: Some(Box::new(GoType::new(go_type))),
            elements: Vec::new(),
            layout: CompositeLayout::Inline { padded: true },
            end: SourcePosition::default(),
        }
    }

    #[test]
    fn source_operators_keep_their_symbol_and_compound_form() {
        use BinaryOperator::*;
        for operator in [
            Addition,
            Subtraction,
            Multiplication,
            Division,
            Remainder,
            BitwiseAnd,
            BitwiseOr,
            BitwiseXor,
            BitwiseAndNot,
            ShiftLeft,
            ShiftRight,
            Equal,
            NotEqual,
            LessThan,
            LessThanOrEqual,
            GreaterThan,
            GreaterThanOrEqual,
            And,
            Or,
        ] {
            let lowered = BinaryOp::from(&operator);
            assert_eq!(lowered.symbol(), operator.to_string());
            assert_eq!(
                lowered.has_compound_form(),
                operator.compound_assignment_symbol().is_some()
            );
        }
    }

    #[test]
    fn looser_operand_takes_parentheses() {
        let shifted = binary(
            binary(name("a"), BinaryOp::Add, name("b")),
            BinaryOp::Shl,
            name("c"),
        );
        assert_eq!(shifted.print(), "(a + b) << c");
        let sum = binary(
            binary(name("a"), BinaryOp::Mul, name("b")),
            BinaryOp::Add,
            name("c"),
        );
        assert_eq!(sum.print(), "a * b + c");
    }

    #[test]
    fn equal_operand_on_the_right_takes_parentheses() {
        let nested = binary(
            name("a"),
            BinaryOp::Sub,
            binary(name("b"), BinaryOp::Sub, name("c")),
        );
        assert_eq!(nested.print(), "a - (b - c)");
        let flat = binary(
            binary(name("a"), BinaryOp::Sub, name("b")),
            BinaryOp::Sub,
            name("c"),
        );
        assert_eq!(flat.print(), "a - b - c");
    }

    #[test]
    fn repeated_sign_takes_parentheses() {
        let negate = |operand| GoExpressionNode::Unary {
            operator: UnaryOp::Negate,
            operand: Operand::from(operand),
        };
        assert_eq!(negate(negate(name("x"))).print(), "-(-x)");
        assert_eq!(
            negate(GoExpressionNode::Literal("-1".to_string())).print(),
            "-(-1)"
        );
        let not = |operand| GoExpressionNode::Unary {
            operator: UnaryOp::Not,
            operand: Operand::from(operand),
        };
        assert_eq!(not(not(name("x"))).print(), "!!x");
    }

    #[test]
    fn postfix_on_prefix_takes_parentheses() {
        let field = GoExpressionNode::Selector {
            base: Operand::from(GoExpressionNode::Dereference(Operand::from(name("node")))),
            field: "Child".to_string(),
            may_panic: false,
        };
        assert_eq!(field.print(), "(*node).Child");
        let call = GoExpressionNode::Call {
            callee: Operand::from(GoExpressionNode::Dereference(Operand::from(name("f")))),
            arguments: vec![name("x").into()],
            purity: CallPurity::Effectful,
            paren: SourcePosition::default(),
        };
        assert_eq!(call.print(), "(*f)(x)");
    }

    #[test]
    fn composite_literal_takes_parentheses_in_a_header() {
        let compared = binary(name("x"), BinaryOp::Eq, literal_struct("Point"));
        assert_eq!(compared.print(), "x == Point{}");
        assert_eq!(header(&compared), "x == (Point{})");
        let called = GoExpressionNode::Call {
            callee: Operand::from(name("f")),
            arguments: vec![literal_struct("Point").into()],
            purity: CallPurity::Effectful,
            paren: SourcePosition::default(),
        };
        assert_eq!(header(&called), "f(Point{})");
    }

    #[test]
    fn a_directive_after_a_function_literal_body_is_written_again() {
        use crate::plan::bodies::expression_statement;
        use crate::plan::values::GoExpression;

        let position = |line| Position {
            path: "a.lis".into(),
            line,
            column: 1,
        };
        let at = |line| SourcePosition(Some(Box::new(position(line))));
        let call = |callee: &str, arguments: Vec<Operand>| GoExpressionNode::Call {
            callee: name(callee).into(),
            arguments,
            purity: CallPurity::Effectful,
            paren: SourcePosition::default(),
        };
        let body = LoweredBlock {
            statements: vec![expression_statement(
                GoExpression::from_operand(call("g", Vec::new()).into()).positioned(position(3)),
            )],
        };
        let literal = GoExpressionNode::FunctionLiteral {
            parameters: Vec::new(),
            result: Box::default(),
            body,
            layout: FunctionLiteralLayout::Inline,
            end: SourcePosition::default(),
        };
        let outer = call(
            "f",
            vec![literal.into(), Operand::new(at(2), call("h", Vec::new()))],
        );
        assert_eq!(
            {
                let mut output = GoWriter::new(true);
                output.push_position(&at(2));
                outer.write_into(&mut output);
                output.into_text()
            },
            "/*line a.lis:2:1*/ f(func() { /*line a.lis:3:1*/ g() }, /*line a.lis:2:1*/ h())"
        );
    }
}
