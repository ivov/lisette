use syntax::EcoString;
use syntax::ast::{
    EnumFieldDefinition, Generic, Literal, Pattern, RestPattern, StructFieldDefinition,
    StructFieldPattern, StructKind, TypedPattern, VariantFields,
};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, unqualified_name};

use crate::Emitter;
use crate::expressions::literals::{convert_escape_sequences, emit_raw_string};
use crate::names::generics;
use crate::patterns::decision_tree::{collect_pattern_info, emit_tree_bindings};
use crate::write_line;

/// Shared access to a named, typed field — implemented for both
/// `StructFieldDefinition` and `EnumFieldDefinition` so the recursion
/// helpers can iterate either shape without duplication.
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

/// View an enum variant's fields as a slice. `Unit` variants yield an empty
/// slice, so callers can treat all variant shapes uniformly.
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

impl Emitter<'_> {
    pub(crate) fn emit_pattern_bindings(
        &mut self,
        output: &mut String,
        subject: &str,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
    ) {
        let (_, bindings) = collect_pattern_info(self, pattern, typed);
        emit_tree_bindings(self, output, &bindings, subject);
    }

    pub(crate) fn fresh_var(&mut self, hint: Option<&str>) -> String {
        loop {
            self.scope.next_var += 1;
            let name = match hint {
                Some(h) => format!("{}_{}", h, self.scope.next_var),
                None => format!("tmp_{}", self.scope.next_var),
            };
            if !self.scope.bindings.has_go_name(&name) && !self.is_declared(&name) {
                return name;
            }
        }
    }

    pub(crate) fn pattern_has_bindings(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { .. } => true,
            Pattern::Tuple { elements, .. } => elements.iter().any(Self::pattern_has_bindings),
            Pattern::EnumVariant { fields, .. } => fields.iter().any(Self::pattern_has_bindings),
            Pattern::Struct { fields, .. } => {
                fields.iter().any(|f| Self::pattern_has_bindings(&f.value))
            }
            Pattern::Slice { prefix, rest, .. } => {
                prefix.iter().any(Self::pattern_has_bindings)
                    || matches!(rest, RestPattern::Bind { .. })
            }
            Pattern::Or { patterns, .. } => patterns.iter().any(Self::pattern_has_bindings),
            Pattern::AsBinding { .. } => true,
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
        }
    }

    pub(crate) fn pattern_binds_name(pattern: &Pattern, name: &str) -> bool {
        match pattern {
            Pattern::Identifier { identifier, .. } => identifier == name,
            Pattern::Tuple { elements, .. } => {
                elements.iter().any(|e| Self::pattern_binds_name(e, name))
            }
            Pattern::EnumVariant { fields, .. } => {
                fields.iter().any(|f| Self::pattern_binds_name(f, name))
            }
            Pattern::Struct { fields, .. } => fields
                .iter()
                .any(|f| Self::pattern_binds_name(&f.value, name)),
            Pattern::Slice { prefix, rest, .. } => {
                prefix.iter().any(|e| Self::pattern_binds_name(e, name))
                    || matches!(rest, RestPattern::Bind { name: n, .. } if n == name)
            }
            Pattern::Or { patterns, .. } => {
                patterns.iter().any(|p| Self::pattern_binds_name(p, name))
            }
            Pattern::AsBinding {
                pattern,
                name: as_name,
                ..
            } => as_name == name || Self::pattern_binds_name(pattern, name),
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
        }
    }

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
                        !self.ctx.unused.is_unused_rest_binding(rest) && self.is_declared(name)
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
                    || (!self.ctx.unused.is_unused_binding(p) && self.is_declared(name))
            }
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => false,
        }
    }

    pub(crate) fn is_catchall_pattern(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::WildCard { .. } | Pattern::Identifier { .. } | Pattern::Unit { .. } => true,
            Pattern::Literal { .. } | Pattern::EnumVariant { .. } => false,
            Pattern::Struct { fields, rest, .. } => {
                *rest && fields.iter().all(|f| Self::is_catchall_pattern(&f.value))
            }
            Pattern::Tuple { elements, .. } => elements.iter().all(Self::is_catchall_pattern),
            Pattern::Slice { prefix, rest, .. } => prefix.is_empty() && rest.is_present(),
            Pattern::Or { patterns, .. } => patterns.iter().any(Self::is_catchall_pattern),
            Pattern::AsBinding { pattern, .. } => Self::is_catchall_pattern(pattern),
        }
    }

    pub(crate) fn emit_binding_declarations_with_type(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        ty: &Type,
        typed: Option<&TypedPattern>,
    ) {
        match pattern {
            Pattern::Identifier { identifier, .. } => {
                self.declare_pattern_var(output, pattern, identifier, ty);
            }
            Pattern::Tuple { elements, .. } => {
                self.emit_tuple_pattern_decls(output, elements, ty, typed);
            }
            Pattern::Struct {
                fields, identifier, ..
            } => {
                self.emit_struct_pattern_decls(output, fields, identifier, ty, typed);
            }
            Pattern::EnumVariant {
                fields,
                identifier,
                ty: pattern_ty,
                ..
            } => {
                self.emit_enum_variant_pattern_decls(
                    output, fields, identifier, pattern_ty, ty, typed,
                );
            }
            Pattern::Slice { prefix, rest, .. } => {
                self.emit_slice_pattern_decls(output, prefix, rest, ty, typed);
            }
            Pattern::Or { patterns, .. } => {
                let Some(first) = patterns.first() else {
                    return;
                };
                let alt = match typed {
                    Some(TypedPattern::Or { alternatives }) => alternatives.first(),
                    _ => None,
                };
                self.emit_binding_declarations_with_type(output, first, ty, alt);
            }
            p @ Pattern::AsBinding {
                pattern: inner,
                name,
                ..
            } => {
                self.emit_binding_declarations_with_type(output, inner, ty, typed);
                self.declare_pattern_var(output, p, name, ty);
            }
            Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => {}
        }
    }

    /// Declare a Go `var X T` binding for an identifier-shaped pattern, falling
    /// back to `_` when the binding is unused and to a fresh name when the
    /// desired Go name is already taken in the current scope.
    fn declare_pattern_var(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        lisette_name: &EcoString,
        resolved: &Type,
    ) {
        let Some(go_name) = self.go_name_for_binding(pattern) else {
            self.scope.bindings.add(lisette_name, "_");
            return;
        };
        let go_name = if self.is_declared(&go_name) {
            self.fresh_var(Some(lisette_name))
        } else {
            go_name
        };
        let go_name = self.scope.bindings.add(lisette_name, go_name);
        self.declare(&go_name);
        let go_ty = self.go_type_as_string(resolved);
        write_line!(output, "var {} {}", go_name, go_ty);
    }

    /// Recurse into tuple-pattern elements, pairing each with the matching
    /// tuple-slot type from the resolved tuple (or constructor with tuple args).
    fn emit_tuple_pattern_decls(
        &mut self,
        output: &mut String,
        elements: &[Pattern],
        resolved: &Type,
        typed: Option<&TypedPattern>,
    ) {
        let typed_elems: &[TypedPattern] = match typed {
            Some(TypedPattern::Tuple { elements: te, .. }) => te.as_slice(),
            _ => &[],
        };
        let types: &[Type] = match resolved {
            Type::Nominal { params, .. } => params,
            Type::Tuple(elems) => elems,
            _ => return,
        };
        for (i, (elem, elem_ty)) in elements.iter().zip(types.iter()).enumerate() {
            self.emit_binding_declarations_with_type(output, elem, elem_ty, typed_elems.get(i));
        }
    }

    /// Recurse into named struct-pattern fields. The pattern may be matching
    /// a plain struct or an enum's struct variant, discovered via the typed
    /// pattern when present and via the definitions table as a fallback.
    fn emit_struct_pattern_decls(
        &mut self,
        output: &mut String,
        fields: &[StructFieldPattern],
        identifier: &EcoString,
        resolved: &Type,
        typed: Option<&TypedPattern>,
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
                }) = self.ctx.definitions.get(struct_name.as_str())
                else {
                    return;
                };
                self.recurse_named_fields(
                    output,
                    fields,
                    struct_fields,
                    generics,
                    params,
                    Some(pattern_fields),
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
                }) = self.ctx.definitions.get(enum_name.as_str())
                else {
                    return;
                };
                self.recurse_named_fields(
                    output,
                    fields,
                    variant_fields,
                    generics,
                    params,
                    Some(pattern_fields),
                );
            }
            _ => self.emit_struct_pattern_fallback(output, fields, identifier, resolved),
        }
    }

    /// Untyped struct-pattern fallback: look up the definition by id, then
    /// dispatch to the same field-recursion helper with `typed_pf = None`.
    fn emit_struct_pattern_fallback(
        &mut self,
        output: &mut String,
        fields: &[StructFieldPattern],
        identifier: &EcoString,
        resolved: &Type,
    ) {
        let Type::Nominal { id, params, .. } = resolved else {
            return;
        };
        match self.ctx.definitions.get(id.as_str()).map(|d| &d.body) {
            Some(DefinitionBody::Struct {
                fields: field_defs,
                generics,
                ..
            }) => {
                self.recurse_named_fields(output, fields, field_defs, generics, params, None);
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
                        generics,
                        params,
                        None,
                    );
                }
            }
            _ => {}
        }
    }

    /// Recurse into positional enum-variant-pattern fields. Tuple-struct
    /// matches route through the struct definition; everything else routes
    /// through the enum variant's positional fields.
    fn emit_enum_variant_pattern_decls(
        &mut self,
        output: &mut String,
        fields: &[Pattern],
        identifier: &EcoString,
        pattern_ty: &Type,
        resolved: &Type,
        typed: Option<&TypedPattern>,
    ) {
        if self.is_tuple_struct_type(pattern_ty) {
            self.emit_tuple_struct_variant_decls(output, fields, resolved, typed);
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
            }) = self.ctx.definitions.get(enum_name.as_str())
            else {
                return;
            };
            self.recurse_positional_fields(
                output,
                fields,
                variant_fields,
                generics,
                params,
                typed_fields,
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
        }) = self.ctx.definitions.get(id.as_str())
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
            generics,
            params,
            None,
        );
    }

    /// Enum-variant-shaped pattern matching a tuple struct (newtype): use the
    /// struct's own positional fields with `Tuple` kind, not the enum path.
    fn emit_tuple_struct_variant_decls(
        &mut self,
        output: &mut String,
        fields: &[Pattern],
        resolved: &Type,
        typed: Option<&TypedPattern>,
    ) {
        let Type::Nominal { id, params, .. } = resolved else {
            return;
        };
        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    fields: field_defs,
                    generics,
                    kind: StructKind::Tuple,
                    ..
                },
            ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
            return;
        };
        let typed_fields = match typed {
            Some(TypedPattern::EnumVariant { fields: tf, .. }) => Some(tf.as_slice()),
            _ => None,
        };
        self.recurse_positional_fields(output, fields, field_defs, generics, params, typed_fields);
    }

    /// Recurse into a slice pattern's prefix elements and (optionally) bind
    /// the rest variable as a slice of the full element type.
    fn emit_slice_pattern_decls(
        &mut self,
        output: &mut String,
        prefix: &[Pattern],
        rest: &RestPattern,
        resolved: &Type,
        typed: Option<&TypedPattern>,
    ) {
        let (elem_ty, typed_prefix): (Type, Option<&[TypedPattern]>) = match typed {
            Some(TypedPattern::Slice {
                prefix: tp,
                element_type,
                ..
            }) => (element_type.clone(), Some(tp.as_slice())),
            _ => {
                let Type::Nominal { params, .. } = resolved else {
                    return;
                };
                let Some(elem) = params.first().cloned() else {
                    return;
                };
                (elem, None)
            }
        };

        for (i, elem) in prefix.iter().enumerate() {
            let typed_child = typed_prefix.and_then(|tp| tp.get(i));
            self.emit_binding_declarations_with_type(output, elem, &elem_ty, typed_child);
        }

        if let RestPattern::Bind { name, .. } = rest
            && let Some(go_name) = self.go_name_for_rest_binding(rest)
        {
            let go_name = self.scope.bindings.add(name, go_name);
            let go_ty = self.go_type_as_string(resolved);
            write_line!(output, "var {} {}", go_name, go_ty);
        }
    }

    /// For each named pattern field, look up its definition, resolve the
    /// field's type against the enclosing type's generics, and recurse with
    /// the matching typed child when available.
    fn recurse_named_fields<F: FieldDef>(
        &mut self,
        output: &mut String,
        patterns: &[StructFieldPattern],
        defs: &[F],
        generics: &[Generic],
        params: &[Type],
        typed_pf: Option<&[(EcoString, TypedPattern)]>,
    ) {
        for pattern in patterns {
            let Some(def) = defs.iter().find(|d| d.name() == &pattern.name) else {
                continue;
            };
            let field_ty = generics::resolve_field_type(generics, params, def.ty());
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
            );
        }
    }

    /// For each positional pattern slot, zip with the definition slots and
    /// recurse. Positional fields short of the definitions list are skipped
    /// silently (parser already validates arity).
    fn recurse_positional_fields<F: FieldDef>(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        defs: &[F],
        generics: &[Generic],
        params: &[Type],
        typed_fields: Option<&[TypedPattern]>,
    ) {
        for (i, (pattern, def)) in patterns.iter().zip(defs.iter()).enumerate() {
            let field_ty = generics::resolve_field_type(generics, params, def.ty());
            let typed_child = typed_fields.and_then(|tf| tf.get(i));
            self.emit_binding_declarations_with_type(output, pattern, &field_ty, typed_child);
        }
    }
}
