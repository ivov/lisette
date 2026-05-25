use syntax::EcoString;
use syntax::ast::{
    EnumFieldDefinition, Generic, Literal, Pattern, RestPattern, StructFieldDefinition,
    StructFieldPattern, StructKind, TypedPattern, VariantFields,
};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, unqualified_name};

use crate::EmitEffects;
use crate::Planner;
use crate::expressions::literals::{convert_escape_sequences, emit_raw_string};
use crate::names::generics;
use crate::write_line;

/// Generic vars paired with their concrete instantiation arguments; inputs
/// to field-type substitution.
#[derive(Clone, Copy)]
pub(crate) struct GenericArgs<'a> {
    pub(crate) generics: &'a [Generic],
    pub(crate) params: &'a [Type],
}

/// Shared view over `StructFieldDefinition` and `EnumFieldDefinition`.
trait FieldDef {
    fn name(&self) -> &EcoString;
    fn ty(&self) -> &Type;
}

impl FieldDef for StructFieldDefinition {
    fn name(&self) -> &EcoString {
        &self.name
    }
    fn ty(&self) -> &Type {
        &self.ty
    }
}

impl FieldDef for EnumFieldDefinition {
    fn name(&self) -> &EcoString {
        &self.name
    }
    fn ty(&self) -> &Type {
        &self.ty
    }
}

/// `Unit` variants yield an empty slice so callers handle all shapes uniformly.
fn variant_fields_slice(fields: &VariantFields) -> &[EnumFieldDefinition] {
    match fields {
        VariantFields::Unit => &[],
        VariantFields::Tuple(f) | VariantFields::Struct(f) => f,
    }
}

pub(crate) fn emit_pattern_literal(literal: &Literal) -> String {
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
        Literal::String { value, raw: true } => emit_raw_string(value),
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

/// Like `is_catchall_pattern`, but Or-patterns require EVERY alternative
/// to be catchall (rather than ANY).
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
                        !self.facts.is_unused_rest_binding(rest) && self.is_declared(name)
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
                    || (!self.facts.is_unused_binding(p) && self.is_declared(name))
            }
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
        }
    }

    pub(crate) fn emit_binding_declarations_with_type(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        ty: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        match pattern {
            Pattern::Identifier { identifier, .. } => {
                self.declare_pattern_var(output, pattern, identifier, ty, fx);
            }
            Pattern::Tuple { elements, .. } => {
                self.emit_tuple_pattern_declarations(output, elements, ty, typed, fx);
            }
            Pattern::Struct {
                fields, identifier, ..
            } => {
                self.emit_struct_pattern_declarations(output, fields, identifier, ty, typed, fx);
            }
            Pattern::EnumVariant {
                fields,
                identifier,
                ty: pattern_ty,
                ..
            } => {
                self.emit_enum_variant_pattern_declarations(
                    output, fields, identifier, pattern_ty, ty, typed, fx,
                );
            }
            Pattern::Slice { prefix, rest, .. } => {
                self.emit_slice_pattern_declarations(output, prefix, rest, ty, typed, fx);
            }
            Pattern::Or { patterns, .. } => {
                let Some(first) = patterns.first() else {
                    return;
                };
                let alt = match typed {
                    Some(TypedPattern::Or { alternatives }) => alternatives.first(),
                    _ => None,
                };
                self.emit_binding_declarations_with_type(output, first, ty, alt, fx);
            }
            p @ Pattern::AsBinding {
                pattern: inner,
                name,
                ..
            } => {
                self.emit_binding_declarations_with_type(output, inner, ty, typed, fx);
                self.declare_pattern_var(output, p, name, ty, fx);
            }
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => {}
        }
    }

    /// Declare a `var X T` for an identifier pattern; binds `_` when unused.
    fn declare_pattern_var(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        lisette_name: &EcoString,
        resolved: &Type,
        fx: &mut EmitEffects,
    ) {
        let Some(go_name) = self.go_name_for_binding(pattern) else {
            self.scope.bind(lisette_name, "_");
            return;
        };
        self.declare_var_declaration(output, lisette_name, go_name, resolved, fx);
    }

    /// Freshen `go_name`, register the binding, emit `var X T`.
    fn declare_var_declaration(
        &mut self,
        output: &mut String,
        lisette_name: &EcoString,
        go_name: String,
        resolved: &Type,
        fx: &mut EmitEffects,
    ) {
        let go_name = if self.is_declared(&go_name) {
            self.fresh_var(Some(lisette_name))
        } else {
            go_name
        };
        let go_name = self.scope.bind(lisette_name, go_name);
        self.declare(&go_name);
        let go_ty = self.go_type_string(resolved, fx);
        write_line!(output, "var {} {}", go_name, go_ty);
    }

    /// Recurse into tuple-pattern elements paired with their slot types.
    fn emit_tuple_pattern_declarations(
        &mut self,
        output: &mut String,
        elements: &[Pattern],
        resolved: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        let typed_elements: &[TypedPattern] = match typed {
            Some(TypedPattern::Tuple { elements: te, .. }) => te.as_slice(),
            _ => &[],
        };
        let types: &[Type] = match resolved {
            Type::Nominal { params, .. } => params,
            Type::Tuple(elements) => elements,
            _ => return,
        };
        for (i, (element, element_ty)) in elements.iter().zip(types.iter()).enumerate() {
            self.emit_binding_declarations_with_type(
                output,
                element,
                element_ty,
                typed_elements.get(i),
                fx,
            );
        }
    }

    /// Recurse into named struct-pattern fields (plain struct or enum's
    /// struct variant, resolved via the typed pattern or definitions table).
    fn emit_struct_pattern_declarations(
        &mut self,
        output: &mut String,
        fields: &[StructFieldPattern],
        identifier: &EcoString,
        resolved: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        match typed {
            Some(TypedPattern::Struct {
                struct_name,
                struct_fields,
                pattern_fields,
                ..
            }) => {
                let Type::Nominal { params, .. } = resolved else {
                    return;
                };
                let Some(Definition {
                    body: DefinitionBody::Struct { generics, .. },
                    ..
                }) = self.facts.definition(struct_name.as_str())
                else {
                    return;
                };
                self.recurse_named_fields(
                    output,
                    fields,
                    struct_fields,
                    GenericArgs { generics, params },
                    Some(pattern_fields),
                    fx,
                );
            }
            Some(TypedPattern::EnumStructVariant {
                enum_name,
                variant_fields,
                pattern_fields,
                ..
            }) => {
                let Type::Nominal { params, .. } = resolved else {
                    return;
                };
                let Some(Definition {
                    body: DefinitionBody::Enum { generics, .. },
                    ..
                }) = self.facts.definition(enum_name.as_str())
                else {
                    return;
                };
                self.recurse_named_fields(
                    output,
                    fields,
                    variant_fields,
                    GenericArgs { generics, params },
                    Some(pattern_fields),
                    fx,
                );
            }
            _ => self.emit_struct_pattern_fallback(output, fields, identifier, resolved, fx),
        }
    }

    /// Untyped struct-pattern fallback via the definitions table.
    fn emit_struct_pattern_fallback(
        &mut self,
        output: &mut String,
        fields: &[StructFieldPattern],
        identifier: &EcoString,
        resolved: &Type,
        fx: &mut EmitEffects,
    ) {
        let Type::Nominal { id, params, .. } = resolved else {
            return;
        };
        match self.facts.definition(id.as_str()).map(|d| &d.body) {
            Some(DefinitionBody::Struct {
                fields: field_definitions,
                generics,
                ..
            }) => {
                self.recurse_named_fields(
                    output,
                    fields,
                    field_definitions,
                    GenericArgs { generics, params },
                    None,
                    fx,
                );
            }
            Some(DefinitionBody::Enum {
                variants, generics, ..
            }) => {
                let variant_name = unqualified_name(identifier);
                if let Some(variant) = variants.iter().find(|v| v.name == variant_name) {
                    self.recurse_named_fields(
                        output,
                        fields,
                        variant_fields_slice(&variant.fields),
                        GenericArgs { generics, params },
                        None,
                        fx,
                    );
                }
            }
            _ => {}
        }
    }

    /// Recurse into positional enum-variant fields (tuple-struct matches
    /// route through the struct definition).
    #[allow(clippy::too_many_arguments)]
    fn emit_enum_variant_pattern_declarations(
        &mut self,
        output: &mut String,
        fields: &[Pattern],
        identifier: &EcoString,
        pattern_ty: &Type,
        resolved: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        if self.is_tuple_struct_type(pattern_ty) {
            self.emit_tuple_struct_variant_declarations(output, fields, resolved, typed, fx);
            return;
        }

        let typed_fields = match typed {
            Some(TypedPattern::EnumVariant { fields: tf, .. }) => Some(tf.as_slice()),
            _ => None,
        };

        if let Some(TypedPattern::EnumVariant {
            enum_name,
            variant_fields,
            ..
        }) = typed
        {
            let Type::Nominal { params, .. } = resolved else {
                return;
            };
            let Some(Definition {
                body: DefinitionBody::Enum { generics, .. },
                ..
            }) = self.facts.definition(enum_name.as_str())
            else {
                return;
            };
            self.recurse_positional_fields(
                output,
                fields,
                variant_fields,
                GenericArgs { generics, params },
                typed_fields,
                fx,
            );
            return;
        }

        let Type::Nominal { id, params, .. } = resolved else {
            return;
        };
        let Some(Definition {
            body: DefinitionBody::Enum {
                variants, generics, ..
            },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return;
        };
        let variant_name = unqualified_name(identifier);
        let Some(variant) = variants.iter().find(|v| v.name == variant_name) else {
            return;
        };
        self.recurse_positional_fields(
            output,
            fields,
            variant_fields_slice(&variant.fields),
            GenericArgs { generics, params },
            None,
            fx,
        );
    }

    /// Newtype-tuple-struct match via the struct's own positional fields.
    fn emit_tuple_struct_variant_declarations(
        &mut self,
        output: &mut String,
        fields: &[Pattern],
        resolved: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        let Type::Nominal { id, params, .. } = resolved else {
            return;
        };
        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    fields: field_definitions,
                    generics,
                    kind: StructKind::Tuple,
                    ..
                },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return;
        };
        let typed_fields = match typed {
            Some(TypedPattern::EnumVariant { fields: tf, .. }) => Some(tf.as_slice()),
            _ => None,
        };
        self.recurse_positional_fields(
            output,
            fields,
            field_definitions,
            GenericArgs { generics, params },
            typed_fields,
            fx,
        );
    }

    /// Recurse into a slice pattern's prefix and bind any rest variable.
    fn emit_slice_pattern_declarations(
        &mut self,
        output: &mut String,
        prefix: &[Pattern],
        rest: &RestPattern,
        resolved: &Type,
        typed: Option<&TypedPattern>,
        fx: &mut EmitEffects,
    ) {
        let (element_ty, typed_prefix): (Type, Option<&[TypedPattern]>) = match typed {
            Some(TypedPattern::Slice {
                prefix: tp,
                element_type,
                ..
            }) => (element_type.clone(), Some(tp.as_slice())),
            _ => {
                let Type::Nominal { params, .. } = resolved else {
                    return;
                };
                let Some(element) = params.first().cloned() else {
                    return;
                };
                (element, None)
            }
        };

        for (i, element) in prefix.iter().enumerate() {
            let typed_child = typed_prefix.and_then(|tp| tp.get(i));
            self.emit_binding_declarations_with_type(output, element, &element_ty, typed_child, fx);
        }

        if let RestPattern::Bind { name, .. } = rest
            && let Some(go_name) = self.go_name_for_rest_binding(rest)
        {
            self.declare_var_declaration(output, name, go_name, resolved, fx);
        }
    }

    /// For each named field, resolve its type against the enclosing generics
    /// and recurse with the matching typed child.
    fn recurse_named_fields<F: FieldDef>(
        &mut self,
        output: &mut String,
        patterns: &[StructFieldPattern],
        definitions: &[F],
        generic_args: GenericArgs,
        typed_pf: Option<&[(EcoString, TypedPattern)]>,
        fx: &mut EmitEffects,
    ) {
        let GenericArgs { generics, params } = generic_args;
        for pattern in patterns {
            let Some(definition) = definitions.iter().find(|d| d.name() == &pattern.name) else {
                continue;
            };
            let field_ty = generics::resolve_field_type(generics, params, definition.ty());
            let typed_child = typed_pf.and_then(|pf| {
                pf.iter()
                    .find(|(n, _)| n == &pattern.name)
                    .map(|(_, tp)| tp)
            });
            self.emit_binding_declarations_with_type(
                output,
                &pattern.value,
                &field_ty,
                typed_child,
                fx,
            );
        }
    }

    /// Zip positional pattern slots with definition slots and recurse.
    fn recurse_positional_fields<F: FieldDef>(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        definitions: &[F],
        generic_args: GenericArgs,
        typed_fields: Option<&[TypedPattern]>,
        fx: &mut EmitEffects,
    ) {
        let GenericArgs { generics, params } = generic_args;
        for (i, (pattern, definition)) in patterns.iter().zip(definitions.iter()).enumerate() {
            let field_ty = generics::resolve_field_type(generics, params, definition.ty());
            let typed_child = typed_fields.and_then(|tf| tf.get(i));
            self.emit_binding_declarations_with_type(output, pattern, &field_ty, typed_child, fx);
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
