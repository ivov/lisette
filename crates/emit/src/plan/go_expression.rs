//! The Go expression tree behind `GoExpression`.

use crate::names::packages::PackageUse;
use crate::plan::bodies::LoweredBlock;
use crate::render::Renderer;
use crate::types::go_type::render_conversion;
use std::slice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoExpressionNode {
    Identifier(String),
    Qualified {
        package: PackageUse,
        name: String,
    },
    Literal(String),
    /// A Go type in operand position, such as the element type of `make`.
    Type(String),
    CompositeLiteral {
        /// `None` inside an enclosing literal that already names the element type.
        go_type: Option<String>,
        elements: Vec<CompositeElement>,
        layout: CompositeLayout,
    },
    Call {
        callee: Box<GoExpressionNode>,
        arguments: Vec<GoExpressionNode>,
    },
    /// A generic function or type with its type arguments, `f[T, U]`.
    Instantiation {
        base: Box<GoExpressionNode>,
        /// Bracketed Go type list, `[T, U]`.
        type_arguments: String,
    },
    Selector {
        base: Box<GoExpressionNode>,
        field: String,
    },
    Index {
        base: Box<GoExpressionNode>,
        index: Box<GoExpressionNode>,
    },
    Slice {
        base: Box<GoExpressionNode>,
        low: Option<Box<GoExpressionNode>>,
        high: Option<Box<GoExpressionNode>>,
        max: Option<Box<GoExpressionNode>>,
    },
    TypeAssertion {
        base: Box<GoExpressionNode>,
        go_type: String,
    },
    Unary {
        operator: String,
        operand: Box<GoExpressionNode>,
    },
    AddressOf(Box<GoExpressionNode>),
    Dereference(Box<GoExpressionNode>),
    Binary {
        operator: String,
        left: Box<GoExpressionNode>,
        right: Box<GoExpressionNode>,
    },
    Conversion {
        go_type: String,
        operand: Box<GoExpressionNode>,
    },
    /// A variadic argument, `values...`.
    Spread(Box<GoExpressionNode>),
    FunctionLiteral {
        parameters: String,
        result: String,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    },
    Empty,
    /// Go source the program supplied through `@rawgo`.
    Verbatim(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionLiteralLayout {
    Inline,
    MultiLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompositeElement {
    pub key: Option<GoExpressionNode>,
    pub value: GoExpressionNode,
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

impl GoExpressionNode {
    /// The node holds a call or a type assertion. A function literal only defines code.
    pub(crate) fn does_work(&self) -> bool {
        match self {
            Self::Identifier(_)
            | Self::Qualified { .. }
            | Self::Literal(_)
            | Self::Type(_)
            | Self::Empty
            | Self::Verbatim(_)
            | Self::FunctionLiteral { .. } => false,
            Self::Call { .. } | Self::TypeAssertion { .. } => true,
            Self::CompositeLiteral { elements, .. } => {
                elements.iter().any(|element| element.value.does_work())
            }
            Self::Instantiation { base, .. } | Self::Selector { base, .. } => base.does_work(),
            Self::Index { base, index } => base.does_work() || index.does_work(),
            Self::Slice {
                base,
                low,
                high,
                max,
            } => {
                base.does_work()
                    || [low, high, max]
                        .into_iter()
                        .flatten()
                        .any(|bound| bound.does_work())
            }
            Self::Unary { operand, .. }
            | Self::Conversion { operand, .. }
            | Self::AddressOf(operand)
            | Self::Dereference(operand)
            | Self::Spread(operand) => operand.does_work(),
            Self::Binary { left, right, .. } => left.does_work() || right.does_work(),
        }
    }

    pub(crate) fn visit(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        visit(self);
        if let Self::FunctionLiteral { body, .. } = self {
            body.visit_expressions(visit);
        } else {
            self.visit_children(&mut |child| child.visit(visit));
        }
    }

    pub(crate) fn rename_identifier(&mut self, from: &str, to: &str) {
        match self {
            Self::Identifier(name) if name == from => *name = to.to_string(),
            _ => self.visit_children_mut(&mut |child| child.rename_identifier(from, to)),
        }
    }

    pub(crate) fn visit_children(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
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
                    if let Some(key) = &element.key {
                        visit(key);
                    }
                    visit(&element.value);
                }
            }
            Self::Call { callee, arguments } => {
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

    fn visit_children_mut(&mut self, visit: &mut impl FnMut(&mut GoExpressionNode)) {
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
                    if let Some(key) = &mut element.key {
                        visit(key);
                    }
                    visit(&mut element.value);
                }
            }
            Self::Call { callee, arguments } => {
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
        let mut output = String::new();
        self.write(&mut output, Slot::FREE);
        output
    }

    pub(crate) fn print_header(&self) -> String {
        let mut output = String::new();
        self.write(&mut output, Slot::HEADER);
        output
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
            Self::Unary { operator, .. } => Binding::Prefix(operator.chars().next().unwrap_or(' ')),
            Self::AddressOf(_) => Binding::Prefix('&'),
            Self::Dereference(_) => Binding::Prefix('*'),
            Self::Binary { operator, .. } => Binding::Binary(binary_level(operator)),
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

    fn write_child(child: &Self, slot: Slot, output: &mut String) {
        if child.needs_parens(slot) {
            output.push('(');
            child.write(output, Slot::FREE);
            output.push(')');
        } else {
            child.write(output, slot);
        }
    }

    fn write(&self, output: &mut String, slot: Slot) {
        match self {
            Self::Identifier(text)
            | Self::Literal(text)
            | Self::Type(text)
            | Self::Verbatim(text) => output.push_str(text),
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
            } => {
                if let Some(go_type) = go_type {
                    output.push_str(go_type);
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
                        output.push_str("{\n");
                        for element in elements {
                            if *indented {
                                output.push('\t');
                            }
                            element.write(output);
                            output.push_str(",\n");
                        }
                        output.push('}');
                    }
                }
            }
            Self::Call { callee, arguments } => {
                Self::write_child(callee, slot.with(SlotKind::Callee), output);
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
                output.push_str(type_arguments);
            }
            Self::Selector { base, field } => {
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
                output.push_str(go_type);
                output.push(')');
            }
            Self::Unary { operator, operand } => {
                output.push_str(operator);
                let sign = operator.chars().next().unwrap_or(' ');
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
                let level = binary_level(operator);
                Self::write_child(left, slot.with(SlotKind::Left(level)), output);
                output.push(' ');
                output.push_str(operator);
                output.push(' ');
                Self::write_child(right, slot.with(SlotKind::Right(level)), output);
            }
            Self::Conversion { go_type, operand } => {
                output.push_str(&render_conversion(go_type, &operand.print()));
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
            } => {
                output.push_str("func(");
                output.push_str(parameters);
                output.push(')');
                if !result.is_empty() {
                    output.push(' ');
                    output.push_str(result);
                }
                output.push_str(" {");
                let lines: Vec<String> = body
                    .statements
                    .iter()
                    .map(|statement| Renderer.render_setup(slice::from_ref(statement)))
                    .collect();
                let single_line = lines
                    .iter()
                    .all(|line| line.trim_end_matches('\n').lines().count() <= 1);
                if matches!(layout, FunctionLiteralLayout::Inline) && single_line {
                    let joined = lines
                        .iter()
                        .map(|line| line.trim_end_matches('\n'))
                        .collect::<Vec<_>>()
                        .join("; ");
                    if !joined.is_empty() {
                        output.push(' ');
                        output.push_str(&joined);
                        output.push(' ');
                    }
                    output.push('}');
                } else {
                    output.push('\n');
                    for line in &lines {
                        output.push_str(line);
                    }
                    output.push('}');
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

fn binary_level(operator: &str) -> u8 {
    match operator {
        "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" => 5,
        "+" | "-" | "|" | "^" => 4,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
        "&&" => 2,
        "||" => 1,
        _ => 0,
    }
}

impl CompositeElement {
    fn write(&self, output: &mut String) {
        if let Some(key) = &self.key {
            key.write(output, Slot::FREE);
            output.push_str(": ");
        }
        self.value.write(output, Slot::FREE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> GoExpressionNode {
        GoExpressionNode::Identifier(text.to_string())
    }

    fn binary(left: GoExpressionNode, operator: &str, right: GoExpressionNode) -> GoExpressionNode {
        GoExpressionNode::Binary {
            operator: operator.to_string(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn literal_struct(go_type: &str) -> GoExpressionNode {
        GoExpressionNode::CompositeLiteral {
            go_type: Some(go_type.to_string()),
            elements: Vec::new(),
            layout: CompositeLayout::Inline { padded: true },
        }
    }

    #[test]
    fn looser_operand_takes_parentheses() {
        let shifted = binary(binary(name("a"), "+", name("b")), "<<", name("c"));
        assert_eq!(shifted.print(), "(a + b) << c");
        let sum = binary(binary(name("a"), "*", name("b")), "+", name("c"));
        assert_eq!(sum.print(), "a * b + c");
    }

    #[test]
    fn equal_operand_on_the_right_takes_parentheses() {
        let nested = binary(name("a"), "-", binary(name("b"), "-", name("c")));
        assert_eq!(nested.print(), "a - (b - c)");
        let flat = binary(binary(name("a"), "-", name("b")), "-", name("c"));
        assert_eq!(flat.print(), "a - b - c");
    }

    #[test]
    fn repeated_sign_takes_parentheses() {
        let negate = |operand| GoExpressionNode::Unary {
            operator: "-".to_string(),
            operand: Box::new(operand),
        };
        assert_eq!(negate(negate(name("x"))).print(), "-(-x)");
        assert_eq!(
            negate(GoExpressionNode::Literal("-1".to_string())).print(),
            "-(-1)"
        );
        let not = |operand| GoExpressionNode::Unary {
            operator: "!".to_string(),
            operand: Box::new(operand),
        };
        assert_eq!(not(not(name("x"))).print(), "!!x");
    }

    #[test]
    fn postfix_on_prefix_takes_parentheses() {
        let field = GoExpressionNode::Selector {
            base: Box::new(GoExpressionNode::Dereference(Box::new(name("node")))),
            field: "Child".to_string(),
        };
        assert_eq!(field.print(), "(*node).Child");
        let call = GoExpressionNode::Call {
            callee: Box::new(GoExpressionNode::Dereference(Box::new(name("f")))),
            arguments: vec![name("x")],
        };
        assert_eq!(call.print(), "(*f)(x)");
    }

    #[test]
    fn composite_literal_takes_parentheses_in_a_header() {
        let compared = binary(name("x"), "==", literal_struct("Point"));
        assert_eq!(compared.print(), "x == Point{}");
        assert_eq!(compared.print_header(), "x == (Point{})");
        let called = GoExpressionNode::Call {
            callee: Box::new(name("f")),
            arguments: vec![literal_struct("Point")],
        };
        assert_eq!(called.print_header(), "f(Point{})");
    }
}
