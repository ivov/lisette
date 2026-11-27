use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use syntax::EcoString;
use syntax::ast::{Expression, IdentifierResolution, UnaryOperator};

#[derive(Clone, Copy)]
pub(in crate::lowering) enum Place<'e> {
    Name {
        value: &'e EcoString,
        resolution: &'e IdentifierResolution,
    },
    Field(DotAccessSite<'e>),
    Index {
        base: &'e Expression,
        index: &'e Expression,
    },
    Deref {
        pointee: &'e Expression,
    },
    ReferenceCall,
    Value,
}

impl<'e> Place<'e> {
    pub(in crate::lowering) fn of(facts: &EmitFacts<'_>, expression: &'e Expression) -> Self {
        match expression {
            Expression::Identifier {
                value, resolution, ..
            } => Self::Name { value, resolution },
            Expression::DotAccess {
                expression: receiver,
                member,
                ty,
                resolution,
                ..
            } => Self::Field(DotAccessSite {
                expression,
                receiver,
                member,
                ty,
                resolution,
            }),
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => Self::Index { base, index },
            Expression::Unary {
                operator,
                expression: pointee,
                ..
            } => match operator {
                UnaryOperator::Deref => Self::Deref { pointee },
                UnaryOperator::Negative | UnaryOperator::Not | UnaryOperator::BitwiseNot => {
                    Self::Value
                }
            },
            Expression::Call { .. } if facts.is_reference(&expression.get_type()) => {
                Self::ReferenceCall
            }
            Expression::Call { .. }
            | Expression::Literal { .. }
            | Expression::Lambda { .. }
            | Expression::Block { .. }
            | Expression::Let { .. }
            | Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Tuple { .. }
            | Expression::StructCall { .. }
            | Expression::Assignment { .. }
            | Expression::Return { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
            | Expression::Binary { .. }
            | Expression::Paren { .. }
            | Expression::RawGo { .. }
            | Expression::Loop { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::Reference { .. }
            | Expression::Task { .. }
            | Expression::Defer { .. }
            | Expression::Assert { .. }
            | Expression::Select { .. }
            | Expression::Unit { .. }
            | Expression::Range { .. }
            | Expression::Cast { .. }
            | declaration_expression!() => Self::Value,
        }
    }

    pub(in crate::lowering) fn is_place(self) -> bool {
        match self {
            Self::Name { .. }
            | Self::Field(_)
            | Self::Index { .. }
            | Self::Deref { .. }
            | Self::ReferenceCall => true,
            Self::Value => false,
        }
    }
}
