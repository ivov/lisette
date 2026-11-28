use crate::output::render::bodies::{can_inline_block, render_lowered_block};
use crate::output::render::declarations::group_params;
use crate::output::render::types::{go_type_text, needs_conversion_parentheses};
use crate::output::render::writer::GoWriter;
use crate::target::ir::bodies::LoweredBlock;
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::go_expression::{
    CompositeElement, CompositeKey, CompositeLayout, FunctionLiteralLayout, GoExpressionNode,
    GoParameter, Operand, SourcePosition,
};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::{GoType, GoTypeShape};

impl GoExpression {
    pub(in crate::output::render) fn write(&self, output: &mut GoWriter) {
        output.push_position(self.source_position());
        self.node().write_into(output);
    }

    pub(in crate::output::render) fn write_header(&self, output: &mut GoWriter) {
        output.push_position(self.source_position());
        self.node().write_header_into(output);
    }

    pub(in crate::output::render) fn write_list(values: &[GoExpression], output: &mut GoWriter) {
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            value.write(output);
        }
    }
}

pub(crate) fn rendered(expression: &GoExpression) -> String {
    expression.node().print()
}

impl GoExpressionNode {
    pub(in crate::output::render) fn print(&self) -> String {
        let mut output = GoWriter::default();
        self.write_into(&mut output);
        output.into_text()
    }

    pub(in crate::output::render) fn write_into(&self, output: &mut GoWriter) {
        self.write(output, Slot::FREE);
    }

    pub(in crate::output::render) fn write_header_into(&self, output: &mut GoWriter) {
        self.write(output, Slot::HEADER);
    }

    fn binding(&self) -> Binding {
        match self {
            Self::Literal(GoLiteral::Int(value)) if *value < 0 => Binding::Prefix('-'),
            Self::Literal(GoLiteral::Token(text)) if text.starts_with(['-', '+']) => {
                Binding::Prefix(
                    text.chars()
                        .next()
                        .expect("a signed token starts with its sign"),
                )
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
            | Self::Unreachable => Binding::Primary,
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
            (Binding::Primary, SlotKind::Postfix) => {
                matches!(self, Self::Literal(_) | Self::FunctionLiteral { .. })
                    || matches!(self, Self::Type(go_type) if matches!(go_type.shape.as_ref(), GoTypeShape::Pointer(_)))
            }
            (Binding::Prefix(sign), SlotKind::Prefix(operator)) => {
                sign == operator && matches!(sign, '-' | '+' | '&')
            }
            (Binding::Binary(level), SlotKind::Left(parent)) => level < parent,
            (Binding::Binary(level), SlotKind::Right(parent)) => level <= parent,
            (Binding::Unknown | Binding::Binary(_), SlotKind::Free)
            | (
                Binding::Primary,
                SlotKind::Free
                | SlotKind::Left(_)
                | SlotKind::Right(_)
                | SlotKind::Prefix(_)
                | SlotKind::Callee,
            )
            | (Binding::Prefix(_), SlotKind::Free | SlotKind::Left(_) | SlotKind::Right(_)) => {
                false
            }
            (
                Binding::Unknown,
                SlotKind::Left(_)
                | SlotKind::Right(_)
                | SlotKind::Prefix(_)
                | SlotKind::Callee
                | SlotKind::Postfix,
            )
            | (Binding::Prefix(_), SlotKind::Callee | SlotKind::Postfix)
            | (Binding::Binary(_), SlotKind::Prefix(_) | SlotKind::Callee | SlotKind::Postfix) => {
                true
            }
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
            Self::Literal(GoLiteral::Nil) => output.push_str("nil"),
            Self::Literal(GoLiteral::Bool(value)) => {
                output.push_str(if *value { "true" } else { "false" })
            }
            Self::Literal(GoLiteral::Int(value)) => output.push_str(&value.to_string()),
            Self::Literal(GoLiteral::Token(text)) => output.push_str(text),
            Self::Verbatim(text) => output.push_foreign(text),
            Self::Unreachable => {
                panic!("internal error: an unreachable value reaches the Go output")
            }
            Self::Type(go_type) => output.push_str(&go_type_text(go_type)),
            Self::Qualified { package, name } => {
                output.push_str(package.qualifier());
                output.push('.');
                output.push_str(name);
            }
            Self::CompositeLiteral {
                go_type,
                elements,
                layout,
                end,
            } => Self::write_composite_literal(output, go_type.as_deref(), elements, layout, end),
            Self::Call {
                callee,
                arguments,
                paren,
                ..
            } => Self::write_call(output, slot, callee, arguments, paren),
            Self::Instantiation {
                base,
                type_arguments,
            } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push_str(&go_type_text(type_arguments));
            }
            Self::Selector { base, field, .. } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push('.');
                output.push_str(field);
            }
            Self::Index { base, index, .. } => {
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
                ..
            } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                Self::write_slice_bounds(output, low, high, max);
            }
            Self::TypeAssertion { base, go_type } => {
                Self::write_child(base, slot.with(SlotKind::Postfix), output);
                output.push_str(".(");
                output.push_str(&go_type_text(go_type));
                output.push(')');
            }
            Self::Spread(operand) => {
                Self::write_child(operand, slot.with(SlotKind::Postfix), output);
                output.push_str("...");
            }
            Self::Unary { operator, operand } => {
                Self::write_prefixed(output, slot, operator.sign(), operand)
            }
            Self::AddressOf(operand) => Self::write_prefixed(output, slot, '&', operand),
            Self::Dereference(operand) => Self::write_prefixed(output, slot, '*', operand),
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
                Self::write_conversion(output, go_type, operand)
            }
            Self::FunctionLiteral {
                parameters,
                result,
                body,
                layout,
                end,
            } => Self::write_function_literal(output, parameters, result, body, layout, end),
        }
    }

    fn write_prefixed(output: &mut GoWriter, slot: Slot, sign: char, operand: &Operand) {
        output.push(sign);
        Self::write_child(operand, slot.with(SlotKind::Prefix(sign)), output);
    }

    fn write_call(
        output: &mut GoWriter,
        slot: Slot,
        callee: &Operand,
        arguments: &[Operand],
        paren: &SourcePosition,
    ) {
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

    fn write_slice_bounds(
        output: &mut GoWriter,
        low: &Option<Operand>,
        high: &Option<Operand>,
        max: &Option<Operand>,
    ) {
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

    fn write_conversion(output: &mut GoWriter, go_type: &GoType, operand: &Operand) {
        if needs_conversion_parentheses(go_type) {
            output.push('(');
            output.push_str(&go_type_text(go_type));
            output.push_str(")(");
        } else {
            output.push_str(&go_type_text(go_type));
            output.push('(');
        }
        operand.write(output, Slot::FREE);
        output.push(')');
    }

    fn write_composite_literal(
        output: &mut GoWriter,
        go_type: Option<&GoType>,
        elements: &[CompositeElement],
        layout: &CompositeLayout,
        end: &SourcePosition,
    ) {
        if let Some(go_type) = go_type {
            output.push_str(&go_type_text(go_type));
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

    fn write_function_literal(
        output: &mut GoWriter,
        parameters: &[GoParameter],
        result: &Option<Box<GoType>>,
        body: &LoweredBlock,
        layout: &FunctionLiteralLayout,
        end: &SourcePosition,
    ) {
        output.push_str("func(");
        let pairs: Vec<(String, String)> = parameters.iter().map(GoParameter::pair).collect();
        output.push_str(&group_params(&pairs));
        output.push(')');
        if let Some(result) = result {
            output.push(' ');
            output.push_str(&go_type_text(result));
        }
        output.push_str(" {");
        let single_line = can_inline_block(body);
        let start = output.line();
        if matches!(layout, FunctionLiteralLayout::Inline) && single_line {
            if !body.renders_empty() {
                output.push(' ');
                output.render_inline(|output| render_lowered_block(output, body));
                output.push(' ');
            }
            output.push('}');
            output.close_literal(&SourcePosition::default(), start);
        } else {
            output.push('\n');
            render_lowered_block(output, body);
            output.push('}');
            output.close_literal(end, start);
        }
    }
}

impl GoParameter {
    fn pair(&self) -> (String, String) {
        (self.name.to_string(), go_type_text(&self.go_type))
    }
}

impl Operand {
    fn write(&self, output: &mut GoWriter, slot: Slot) {
        output.push_position(self.position());
        (**self).write(output, slot);
    }
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
