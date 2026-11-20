use std::borrow::Cow;

use syntax::ast::{Literal, Pattern, RestPattern, expand_or_patterns, outermost_or_patterns};

use crate::Planner;
use crate::expressions::literals::{convert_escape_sequences, raw_string_literal};

pub(crate) fn pattern_literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Integer { value, text } => {
            if let Some(original) = text {
                original.clone()
            } else {
                value.to_string()
            }
        }
        Literal::Float { value, text } => text.clone().unwrap_or_else(|| value.to_string()),
        Literal::Boolean(b) => b.to_string(),
        Literal::String { value, raw: false } => {
            format!("\"{}\"", convert_escape_sequences(value))
        }
        Literal::String { value, raw: true } => raw_string_literal(value),
        Literal::Char(c) => {
            format!("'{}'", convert_escape_sequences(c))
        }
        Literal::Imaginary(_) | Literal::FormatString(_) | Literal::Slice(_) => {
            unreachable!("FormatString, Slice, and Imaginary are not valid pattern literals")
        }
    }
}

pub(crate) fn is_catchall_pattern(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Identifier { .. } | Pattern::Unit { .. } => true,
        Pattern::Literal { .. } | Pattern::EnumVariant { .. } => false,
        Pattern::Struct { fields, rest, .. } => {
            *rest && fields.iter().all(|f| is_catchall_pattern(&f.value))
        }
        Pattern::Tuple { elements, .. } => elements.iter().all(is_catchall_pattern),
        Pattern::Slice { prefix, rest, .. } => prefix.is_empty() && rest.is_present(),
        Pattern::Or { patterns, .. } => patterns.iter().any(is_catchall_pattern),
        Pattern::AsBinding { pattern, .. } => is_catchall_pattern(pattern),
    }
}

pub(crate) fn is_unconditional_catchall(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Or { patterns, .. } => patterns.iter().all(is_catchall_pattern),
        other => is_catchall_pattern(other),
    }
}

pub(crate) fn pattern_binds_name(pattern: &Pattern, name: &str) -> bool {
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

impl Planner<'_> {
    pub(crate) fn pattern_has_binding_collisions(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { .. } => false,
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
                        !self.facts.is_unused_rest_binding(rest) && self.shadows_declaration(name)
                    } else {
                        false
                    }
            }
            Pattern::Or { patterns, .. } => patterns
                .iter()
                .any(|p| self.pattern_has_binding_collisions(p)),
            p @ Pattern::AsBinding {
                pattern: inner,
                name,
                ..
            } => {
                self.pattern_has_binding_collisions(inner)
                    || (!self.facts.is_unused_binding(p) && self.shadows_declaration(name))
            }
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
        }
    }
}

pub(crate) fn pattern_has_bindings(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Identifier { .. } => true,
        Pattern::Tuple { elements, .. } => elements.iter().any(pattern_has_bindings),
        Pattern::EnumVariant { fields, .. } => fields.iter().any(pattern_has_bindings),
        Pattern::Struct { fields, .. } => fields.iter().any(|f| pattern_has_bindings(&f.value)),
        Pattern::Slice { prefix, rest, .. } => {
            prefix.iter().any(pattern_has_bindings) || matches!(rest, RestPattern::Bind { .. })
        }
        Pattern::Or { patterns, .. } => patterns.iter().any(pattern_has_bindings),
        Pattern::AsBinding { .. } => true,
        Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
    }
}

pub(crate) fn has_binding_or_pattern(pattern: &Pattern) -> bool {
    outermost_or_patterns(pattern)
        .into_iter()
        .any(pattern_has_bindings)
}

pub(crate) fn split_binding_or_patterns(pattern: &Pattern) -> Vec<Cow<'_, Pattern>> {
    expand_or_patterns(pattern, &pattern_has_bindings)
        .into_iter()
        .map(|expansion| expansion.pattern)
        .collect()
}
