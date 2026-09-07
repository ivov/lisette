//! The Go expression tree behind `GoExpression`.

use crate::types::go_type::render_conversion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoExpressionNode {
    Identifier(String),
    Literal(String),
    CompositeLiteral(String),
    Call {
        callee: Box<GoExpressionNode>,
        arguments: Vec<GoExpressionNode>,
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
    Unary {
        operator: String,
        operand: Box<GoExpressionNode>,
    },
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
    Receive(Box<GoExpressionNode>),
    /// Go text that is not yet a node.
    Raw(String),
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
            | Self::CompositeLiteral(text)
            | Self::Raw(text) => output.push_str(text),
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
            Self::Receive(channel) => {
                output.push_str("<-");
                channel.write(output);
            }
        }
    }
}
