//! The Go expression tree behind `GoExpression`.

use crate::names::packages::PackageUse;
use crate::plan::bodies::LoweredBlock;
use crate::plan::local::GoIdentifier;
use crate::render::Renderer;
use crate::types::go_type::render_conversion;
use crate::utils::group_params;
use std::slice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoExpressionNode {
    Identifier(GoIdentifier),
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
        may_panic: bool,
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
        parameters: Vec<GoParameter>,
        result: String,
        body: LoweredBlock,
        layout: FunctionLiteralLayout,
    },
    Empty,
    /// Go source the program supplied through `@rawgo`.
    Verbatim(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoParameter {
    pub(crate) name: GoIdentifier,
    pub(crate) go_type: String,
}

impl GoParameter {
    pub(crate) fn new(name: impl Into<String>, go_type: impl Into<String>) -> Self {
        Self {
            name: GoIdentifier::name(name.into()),
            go_type: go_type.into(),
        }
    }

    fn pair(&self) -> (String, String) {
        (self.name.to_string(), self.go_type.clone())
    }
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

#[derive(Default)]
struct EvaluationObligations {
    observable: bool,
    may_panic_without_call: bool,
    may_block_without_call: bool,
    staging_work: bool,
}

impl EvaluationObligations {
    fn combine(&mut self, other: Self) {
        self.observable |= other.observable;
        self.may_panic_without_call |= other.may_panic_without_call;
        self.may_block_without_call |= other.may_block_without_call;
        self.staging_work |= other.staging_work;
    }

    fn must_evaluate(&self) -> bool {
        self.observable || self.may_panic_without_call
    }
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
    /// A function literal does not run its body when evaluated.
    fn evaluation_obligations(&self) -> EvaluationObligations {
        let mut obligations = EvaluationObligations::default();
        self.visit_children(&mut |child| {
            obligations.combine(child.evaluation_obligations());
        });
        match self {
            Self::Call { .. } | Self::Verbatim(_) => {
                obligations.observable = true;
                obligations.staging_work = true;
            }
            Self::Index { .. } | Self::Slice { .. } | Self::Dereference(_) => {
                obligations.may_panic_without_call = true
            }
            Self::Selector { may_panic, .. } => {
                obligations.may_panic_without_call |= *may_panic;
            }
            Self::TypeAssertion { .. } => {
                obligations.may_panic_without_call = true;
                obligations.staging_work = true;
            }
            Self::Unary { operator, .. } if operator == "<-" => {
                obligations.observable = true;
                obligations.may_block_without_call = true;
                obligations.staging_work = true;
            }
            Self::Binary { operator, .. }
                if matches!(operator.as_str(), "/" | "%" | "<<" | ">>") =>
            {
                obligations.may_panic_without_call = true;
            }
            Self::Binary {
                operator,
                left,
                right,
            } if matches!(operator.as_str(), "==" | "!=")
                && !matches!(left.as_ref(), Self::Literal(_))
                && !matches!(right.as_ref(), Self::Literal(_))
                && !(left.is_unit_struct_literal() && right.is_unit_struct_literal()) =>
            {
                obligations.may_panic_without_call = true;
            }
            Self::Conversion { go_type, .. }
                if go_type.starts_with("*[") || go_type.starts_with('[') =>
            {
                obligations.may_panic_without_call = true;
            }
            Self::CompositeLiteral { go_type, .. }
                if go_type.as_deref().is_some_and(|ty| ty.starts_with("map[")) =>
            {
                obligations.may_panic_without_call = true;
            }
            Self::Identifier(_)
            | Self::Qualified { .. }
            | Self::Literal(_)
            | Self::Type(_)
            | Self::Instantiation { .. }
            | Self::AddressOf(_)
            | Self::Spread(_)
            | Self::FunctionLiteral { .. }
            | Self::Empty
            | Self::Conversion { .. }
            | Self::CompositeLiteral { .. } => {}
            Self::Unary { operator, .. } => {
                if !matches!(operator.as_str(), "+" | "-" | "!" | "^" | "&") {
                    obligations.observable = true;
                    obligations.may_panic_without_call = true;
                    obligations.staging_work = true;
                }
            }
            Self::Binary { operator, .. } => {
                if !matches!(
                    operator.as_str(),
                    "+" | "-"
                        | "*"
                        | "&"
                        | "|"
                        | "^"
                        | "&^"
                        | "&&"
                        | "||"
                        | "=="
                        | "!="
                        | "<"
                        | "<="
                        | ">"
                        | ">="
                ) {
                    obligations.observable = true;
                    obligations.may_panic_without_call = true;
                    obligations.staging_work = true;
                }
            }
        }
        obligations
    }

    pub(crate) fn does_work(&self) -> bool {
        self.evaluation_obligations().staging_work
    }

    fn is_unit_struct_literal(&self) -> bool {
        matches!(
            self,
            Self::CompositeLiteral {
                go_type: Some(go_type),
                elements,
                ..
            } if go_type == "struct{}" && elements.is_empty()
        )
    }

    pub(crate) fn can_erase(&self) -> bool {
        !self.evaluation_obligations().must_evaluate()
    }

    pub(crate) fn requires_ordering_without_call(&self) -> bool {
        let obligations = self.evaluation_obligations();
        obligations.may_panic_without_call || obligations.may_block_without_call
    }

    pub(crate) fn mentions(&self, name: &str) -> bool {
        let mut found = false;
        self.visit(&mut |inner| {
            if let Self::Identifier(read) = inner
                && read == name
            {
                found = true;
            }
        });
        found
    }

    pub(crate) fn visit(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        visit(self);
        if let Self::FunctionLiteral { body, .. } = self {
            body.visit_expressions(visit);
        } else {
            self.visit_children(&mut |child| child.visit(visit));
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

    pub(super) fn visit_children_mut(&mut self, visit: &mut impl FnMut(&mut GoExpressionNode)) {
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
            Self::Identifier(text) => output.push_str(text.spelling()),
            Self::Literal(text) | Self::Type(text) | Self::Verbatim(text) => output.push_str(text),
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
                let pairs: Vec<(String, String)> =
                    parameters.iter().map(GoParameter::pair).collect();
                output.push_str(&group_params(&pairs));
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
        GoExpressionNode::Identifier(text.to_string().into())
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
            may_panic: false,
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
