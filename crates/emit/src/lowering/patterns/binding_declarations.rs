use crate::lowering::expressions::literals::integer_literal;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::go_expression::GoLiteral;
use std::borrow::Cow;

use syntax::ast::{Literal, Pattern, RestPattern, expand_or_patterns, outermost_or_patterns};

use crate::lowering::expressions::literals::{convert_escape_sequences, raw_string_literal};

pub(in crate::lowering) fn pattern_literal(literal: &Literal) -> GoLiteral {
    match literal {
        Literal::Integer { value, text } => integer_literal(*value, text),
        Literal::Float { value, text } => {
            GoLiteral::Token(text.clone().unwrap_or_else(|| value.to_string()))
        }
        Literal::Boolean(b) => GoLiteral::Bool(*b),
        Literal::String { value, raw: false } => {
            GoLiteral::Token(format!("\"{}\"", convert_escape_sequences(value)))
        }
        Literal::String { value, raw: true } => GoLiteral::Token(raw_string_literal(value)),
        Literal::Char(c) => GoLiteral::Token(format!("'{}'", convert_escape_sequences(c))),
        Literal::Imaginary(_) | Literal::FormatString(_) | Literal::Slice(_) => {
            unreachable!("FormatString, Slice, and Imaginary are not valid pattern literals")
        }
    }
}

pub(in crate::lowering) fn pattern_binds_name(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Identifier { identifier, .. } => identifier == name,
        Pattern::Tuple { elements, .. } => elements.iter().any(|e| pattern_binds_name(e, name)),
        Pattern::EnumVariant { fields, .. } => fields.iter().any(|f| pattern_binds_name(f, name)),
        Pattern::Struct { fields, .. } => fields.iter().any(|f| pattern_binds_name(&f.value, name)),
        Pattern::Slice { prefix, rest, .. } => {
            prefix.iter().any(|e| pattern_binds_name(e, name))
                || matches!(rest, RestPattern::Bind { name: n, .. } if n == name)
        }
        Pattern::Or { patterns, .. } => patterns.iter().any(|p| pattern_binds_name(p, name)),
        Pattern::AsBinding {
            pattern,
            name: as_name,
            ..
        } => as_name == name || pattern_binds_name(pattern, name),
        Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
    }
}

pub(in crate::lowering) fn pattern_has_bindings(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Tuple { elements, .. } => elements.iter().any(pattern_has_bindings),
        Pattern::EnumVariant { fields, .. } => fields.iter().any(pattern_has_bindings),
        Pattern::Struct { fields, .. } => fields.iter().any(|f| pattern_has_bindings(&f.value)),
        Pattern::Slice { prefix, rest, .. } => {
            prefix.iter().any(pattern_has_bindings) || matches!(rest, RestPattern::Bind { .. })
        }
        Pattern::Or { patterns, .. } => patterns.iter().any(pattern_has_bindings),
        Pattern::Identifier { .. } | Pattern::AsBinding { .. } => true,
        Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
    }
}

pub(in crate::lowering) fn has_binding_or_pattern(pattern: &Pattern) -> bool {
    outermost_or_patterns(pattern)
        .into_iter()
        .any(pattern_has_bindings)
}

pub(in crate::lowering) fn split_binding_or_patterns(pattern: &Pattern) -> Vec<Cow<'_, Pattern>> {
    expand_or_patterns(pattern, &pattern_has_bindings)
        .into_iter()
        .map(|expansion| expansion.pattern)
        .collect()
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn pattern_has_binding_collisions(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Tuple { elements, .. } => elements
                .iter()
                .any(|e| self.pattern_has_binding_collisions(e)),
            Pattern::EnumVariant { fields, .. } => fields
                .iter()
                .any(|f| self.pattern_has_binding_collisions(f)),
            Pattern::Struct { fields, .. } => fields
                .iter()
                .any(|f| self.pattern_has_binding_collisions(&f.value)),
            Pattern::Slice { prefix, rest, .. } => {
                prefix
                    .iter()
                    .any(|e| self.pattern_has_binding_collisions(e))
                    || if let RestPattern::Bind { name, .. } = rest {
                        self.shadows_declaration(name)
                    } else {
                        false
                    }
            }
            Pattern::Or { patterns, .. } => patterns
                .iter()
                .any(|p| self.pattern_has_binding_collisions(p)),
            Pattern::AsBinding {
                pattern: inner,
                name,
                ..
            } => self.pattern_has_binding_collisions(inner) || self.shadows_declaration(name),
            Pattern::Identifier { .. }
            | Pattern::WildCard { .. }
            | Pattern::Literal { .. }
            | Pattern::Unit { .. } => false,
        }
    }
}
