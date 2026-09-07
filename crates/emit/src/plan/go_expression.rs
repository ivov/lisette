//! The Go expression tree behind `GoExpression`.

use crate::plan::bodies::LoweredBlock;
use crate::render::Renderer;
use crate::types::go_type::render_conversion;
use std::slice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoExpressionNode {
    Identifier(String),
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
    Parenthesized(Box<GoExpressionNode>),
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
    pub key: Option<String>,
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
    /// Print the node token for token, since `gofmt` owns the spacing.
    pub(crate) fn print(&self) -> String {
        let mut output = String::new();
        self.write(&mut output);
        output
    }

    fn write(&self, output: &mut String) {
        match self {
            Self::Identifier(text)
            | Self::Literal(text)
            | Self::Type(text)
            | Self::Verbatim(text) => output.push_str(text),
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
                callee.write(output);
                output.push('(');
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    argument.write(output);
                }
                output.push(')');
            }
            Self::Instantiation {
                base,
                type_arguments,
            } => {
                base.write(output);
                output.push_str(type_arguments);
            }
            Self::Selector { base, field } => {
                base.write(output);
                output.push('.');
                output.push_str(field);
            }
            Self::Index { base, index } => {
                base.write(output);
                output.push('[');
                index.write(output);
                output.push(']');
            }
            Self::Slice {
                base,
                low,
                high,
                max,
            } => {
                base.write(output);
                output.push('[');
                if let Some(low) = low {
                    low.write(output);
                }
                output.push(':');
                if let Some(high) = high {
                    high.write(output);
                }
                if let Some(max) = max {
                    output.push(':');
                    max.write(output);
                }
                output.push(']');
            }
            Self::TypeAssertion { base, go_type } => {
                base.write(output);
                output.push_str(".(");
                output.push_str(go_type);
                output.push(')');
            }
            Self::Unary { operator, operand } => {
                let operand = operand.print();
                // Go reads `--x` as a decrement, so a negated negative keeps its parentheses.
                if operator == "-" && operand.starts_with('-') {
                    output.push_str("-(");
                    output.push_str(&operand);
                    output.push(')');
                } else {
                    output.push_str(operator);
                    output.push_str(&operand);
                }
            }
            Self::AddressOf(operand) => {
                output.push('&');
                operand.write(output);
            }
            Self::Dereference(operand) => {
                output.push('*');
                operand.write(output);
            }
            Self::Binary {
                operator,
                left,
                right,
            } => {
                left.write(output);
                output.push(' ');
                output.push_str(operator);
                output.push(' ');
                right.write(output);
            }
            Self::Parenthesized(inner) => {
                output.push('(');
                inner.write(output);
                output.push(')');
            }
            Self::Conversion { go_type, operand } => {
                output.push_str(&render_conversion(go_type, &operand.print()));
            }
            Self::Spread(operand) => {
                operand.write(output);
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

impl CompositeElement {
    fn write(&self, output: &mut String) {
        if let Some(key) = &self.key {
            output.push_str(key);
            output.push_str(": ");
        }
        self.value.write(output);
    }
}
