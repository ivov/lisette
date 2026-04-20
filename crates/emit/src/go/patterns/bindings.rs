use syntax::ast::{Literal, Pattern, RestPattern, StructKind, TypedPattern};
use syntax::program::Definition;
use syntax::types::Type;

use crate::Emitter;
use crate::go::expressions::values::convert_escape_sequences;
use crate::go::names::generics;
use crate::go::patterns::decision_tree::{collect_pattern_info, emit_tree_bindings};
use crate::go::write_line;

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
        Literal::String(s) => {
            format!("\"{}\"", convert_escape_sequences(s))
        }
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
        let resolved = ty.resolve();

        match (pattern, typed) {
            (Pattern::Identifier { identifier, .. }, _) => {
                if let Some(go_name) = self.go_name_for_binding(pattern) {
                    let go_name = if self.is_declared(&go_name) {
                        self.fresh_var(Some(identifier))
                    } else {
                        go_name
                    };
                    let go_name = self.scope.bindings.add(identifier, go_name);
                    self.declare(&go_name);
                    let go_ty = self.go_type_as_string(&resolved);
                    write_line!(output, "var {} {}", go_name, go_ty);
                } else {
                    self.scope.bindings.add(identifier, "_");
                }
            }

            (Pattern::Tuple { elements, .. }, _) => {
                let typed_elems = match typed {
                    Some(TypedPattern::Tuple { elements: te, .. }) => te.as_slice(),
                    _ => &[],
                };
                let elem_types: Option<&[Type]> = match &resolved {
                    Type::Constructor { params, .. } => Some(params),
                    Type::Tuple(elems) => Some(elems),
                    _ => None,
                };
                if let Some(types) = elem_types {
                    for (i, (elem, elem_ty)) in elements.iter().zip(types.iter()).enumerate() {
                        self.emit_binding_declarations_with_type(
                            output,
                            elem,
                            elem_ty,
                            typed_elems.get(i),
                        );
                    }
                }
            }

            (
                Pattern::Struct { fields, .. },
                Some(TypedPattern::Struct {
                    struct_name,
                    struct_fields,
                    pattern_fields: typed_pf,
                    ..
                }),
            ) => {
                if let Type::Constructor { params, .. } = &resolved
                    && let Some(Definition::Struct { generics, .. }) =
                        self.ctx.definitions.get(struct_name.as_str())
                {
                    for field_pattern in fields {
                        if let Some(field_definition) =
                            struct_fields.iter().find(|f| f.name == field_pattern.name)
                        {
                            let field_ty = generics::resolve_field_type(
                                generics,
                                params,
                                &field_definition.ty,
                            );
                            let typed_child = typed_pf
                                .iter()
                                .find(|(n, _)| n == &field_pattern.name)
                                .map(|(_, tp)| tp);
                            self.emit_binding_declarations_with_type(
                                output,
                                &field_pattern.value,
                                &field_ty,
                                typed_child,
                            );
                        }
                    }
                }
            }

            (
                Pattern::Struct { fields, .. },
                Some(TypedPattern::EnumStructVariant {
                    enum_name,
                    variant_fields,
                    pattern_fields: typed_pf,
                    ..
                }),
            ) => {
                if let Type::Constructor { params, .. } = &resolved
                    && let Some(Definition::Enum { generics, .. }) =
                        self.ctx.definitions.get(enum_name.as_str())
                {
                    for field_pattern in fields {
                        if let Some(field_definition) =
                            variant_fields.iter().find(|f| f.name == field_pattern.name)
                        {
                            let field_ty = generics::resolve_field_type(
                                generics,
                                params,
                                &field_definition.ty,
                            );
                            let typed_child = typed_pf
                                .iter()
                                .find(|(n, _)| n == &field_pattern.name)
                                .map(|(_, tp)| tp);
                            self.emit_binding_declarations_with_type(
                                output,
                                &field_pattern.value,
                                &field_ty,
                                typed_child,
                            );
                        }
                    }
                }
            }

            (
                Pattern::Struct {
                    fields, identifier, ..
                },
                _,
            ) => {
                if let Type::Constructor { id, params, .. } = &resolved {
                    if let Some(Definition::Struct {
                        fields: field_defs,
                        generics,
                        ..
                    }) = self.ctx.definitions.get(id.as_str())
                    {
                        for field_pattern in fields {
                            if let Some(field_definition) =
                                field_defs.iter().find(|f| f.name == field_pattern.name)
                            {
                                let field_ty = generics::resolve_field_type(
                                    generics,
                                    params,
                                    &field_definition.ty,
                                );
                                self.emit_binding_declarations_with_type(
                                    output,
                                    &field_pattern.value,
                                    &field_ty,
                                    None,
                                );
                            }
                        }
                    } else if let Some(Definition::Enum {
                        variants, generics, ..
                    }) = self.ctx.definitions.get(id.as_str())
                    {
                        let variant_name = identifier.split('.').next_back().unwrap_or(identifier);
                        if let Some(variant_definition) =
                            variants.iter().find(|v| v.name == variant_name)
                        {
                            for field_pattern in fields {
                                if let Some(field_definition) = variant_definition
                                    .fields
                                    .iter()
                                    .find(|f| f.name == field_pattern.name)
                                {
                                    let field_ty = generics::resolve_field_type(
                                        generics,
                                        params,
                                        &field_definition.ty,
                                    );
                                    self.emit_binding_declarations_with_type(
                                        output,
                                        &field_pattern.value,
                                        &field_ty,
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            (
                Pattern::EnumVariant {
                    fields,
                    ty: pattern_ty,
                    ..
                },
                Some(TypedPattern::EnumVariant {
                    enum_name,
                    variant_fields,
                    fields: typed_fields,
                    ..
                }),
            ) => {
                if self.is_tuple_struct_type(pattern_ty) {
                    if let Type::Constructor { id, params, .. } = &resolved
                        && let Some(Definition::Struct {
                            fields: field_defs,
                            generics,
                            kind: StructKind::Tuple,
                            ..
                        }) = self.ctx.definitions.get(id.as_str())
                    {
                        for (i, (field_pattern, field_definition)) in
                            fields.iter().zip(field_defs.iter()).enumerate()
                        {
                            let field_ty = generics::resolve_field_type(
                                generics,
                                params,
                                &field_definition.ty,
                            );
                            self.emit_binding_declarations_with_type(
                                output,
                                field_pattern,
                                &field_ty,
                                typed_fields.get(i),
                            );
                        }
                    }
                    return;
                }

                if let Type::Constructor { params, .. } = &resolved
                    && let Some(Definition::Enum { generics, .. }) =
                        self.ctx.definitions.get(enum_name.as_str())
                {
                    for (i, (field_pattern, field_definition)) in
                        fields.iter().zip(variant_fields.iter()).enumerate()
                    {
                        let field_ty =
                            generics::resolve_field_type(generics, params, &field_definition.ty);
                        self.emit_binding_declarations_with_type(
                            output,
                            field_pattern,
                            &field_ty,
                            typed_fields.get(i),
                        );
                    }
                }
            }

            (
                Pattern::EnumVariant {
                    identifier,
                    fields,
                    ty: pattern_ty,
                    ..
                },
                _,
            ) => {
                if self.is_tuple_struct_type(pattern_ty) {
                    if let Type::Constructor { id, params, .. } = &resolved
                        && let Some(Definition::Struct {
                            fields: field_defs,
                            generics,
                            kind: StructKind::Tuple,
                            ..
                        }) = self.ctx.definitions.get(id.as_str())
                    {
                        for (field_pattern, field_definition) in
                            fields.iter().zip(field_defs.iter())
                        {
                            let field_ty = generics::resolve_field_type(
                                generics,
                                params,
                                &field_definition.ty,
                            );
                            self.emit_binding_declarations_with_type(
                                output,
                                field_pattern,
                                &field_ty,
                                None,
                            );
                        }
                    }
                    return;
                }

                if let Type::Constructor { id, params, .. } = &resolved
                    && let Some(Definition::Enum {
                        variants, generics, ..
                    }) = self.ctx.definitions.get(id.as_str())
                {
                    let variant_name = identifier.split('.').next_back().unwrap_or(identifier);
                    if let Some(variant_definition) =
                        variants.iter().find(|v| v.name == variant_name)
                    {
                        for (field_pattern, field_definition) in
                            fields.iter().zip(variant_definition.fields.iter())
                        {
                            let field_ty = generics::resolve_field_type(
                                generics,
                                params,
                                &field_definition.ty,
                            );
                            self.emit_binding_declarations_with_type(
                                output,
                                field_pattern,
                                &field_ty,
                                None,
                            );
                        }
                    }
                }
            }

            (
                Pattern::Slice { prefix, rest, .. },
                Some(TypedPattern::Slice {
                    prefix: typed_prefix,
                    element_type,
                    ..
                }),
            ) => {
                for (i, elem) in prefix.iter().enumerate() {
                    self.emit_binding_declarations_with_type(
                        output,
                        elem,
                        element_type,
                        typed_prefix.get(i),
                    );
                }
                if let RestPattern::Bind { name, .. } = rest
                    && let Some(go_name) = self.go_name_for_rest_binding(rest)
                {
                    let go_name = self.scope.bindings.add(name, go_name);
                    let go_ty = self.go_type_as_string(&resolved);
                    write_line!(output, "var {} {}", go_name, go_ty);
                }
            }

            (Pattern::Slice { prefix, rest, .. }, _) => {
                if let Type::Constructor { params, .. } = &resolved
                    && let Some(elem_ty) = params.first()
                {
                    for elem in prefix {
                        self.emit_binding_declarations_with_type(output, elem, elem_ty, None);
                    }
                    if let RestPattern::Bind { name, .. } = rest
                        && let Some(go_name) = self.go_name_for_rest_binding(rest)
                    {
                        let go_name = self.scope.bindings.add(name, go_name);
                        let go_ty = self.go_type_as_string(&resolved);
                        write_line!(output, "var {} {}", go_name, go_ty);
                    }
                }
            }

            (Pattern::Or { patterns, .. }, Some(TypedPattern::Or { alternatives })) => {
                if let Some(first) = patterns.first() {
                    self.emit_binding_declarations_with_type(
                        output,
                        first,
                        ty,
                        alternatives.first(),
                    );
                }
            }

            (Pattern::Or { patterns, .. }, _) => {
                if let Some(first) = patterns.first() {
                    self.emit_binding_declarations_with_type(output, first, ty, None);
                }
            }

            (
                p @ Pattern::AsBinding {
                    pattern: inner,
                    name,
                    ..
                },
                _,
            ) => {
                self.emit_binding_declarations_with_type(output, inner, ty, typed);
                if let Some(go_name) = self.go_name_for_binding(p) {
                    let go_name = if self.is_declared(&go_name) {
                        self.fresh_var(Some(name))
                    } else {
                        go_name
                    };
                    let go_name = self.scope.bindings.add(name, go_name);
                    self.declare(&go_name);
                    let go_ty = self.go_type_as_string(&resolved);
                    write_line!(output, "var {} {}", go_name, go_ty);
                } else {
                    self.scope.bindings.add(name, "_");
                }
            }

            (Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. }, _) => {}
        }
    }
}
