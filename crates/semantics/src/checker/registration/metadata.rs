use super::*;

impl TaskState {
    pub(super) fn populate_module_generic_bounds(&self, store: &mut Store, module_id: &str) {
        let Some(module) = store.get_module_mut(module_id) else {
            return;
        };
        for file in module.files.values_mut() {
            for item in &mut file.items {
                populate_expression_generic_bounds(item, &self.facts.bound_types);
            }
        }
    }

    /// Resolve each item's generic bounds from the per-module pass results.
    /// Test harnesses that emit a typed AST directly bypass that pass.
    pub fn populate_item_generic_bounds(&self, items: &mut [Expression]) {
        for item in items {
            populate_expression_generic_bounds(item, &self.facts.bound_types);
        }
    }
}

pub(super) fn declaration_value_position_types(definition: &Definition) -> Vec<(Type, Span)> {
    match &definition.body {
        DefinitionBody::Struct { fields, .. } => fields
            .iter()
            .map(|field| (field.ty.clone(), field.annotation.get_span()))
            .collect(),
        DefinitionBody::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| variant_field_types(&variant.fields))
            .collect(),
        DefinitionBody::TypeAlias { alias, .. } => alias_body_types(alias),
        _ => Vec::new(),
    }
}

pub(super) fn function_signature_pairs(
    fn_ty: &Type,
    params: &[Binding],
    fallback: Span,
) -> (Vec<(Type, Span)>, Vec<Bound>) {
    let Type::Function(function) = fn_ty.unwrap_forall() else {
        return (Vec::new(), Vec::new());
    };
    let pairs: Vec<(Type, Span)> = function
        .params
        .iter()
        .enumerate()
        .map(|(index, param_ty)| {
            let span = params
                .get(index)
                .and_then(|binding| binding.annotation.as_ref())
                .map_or(fallback, Annotation::get_span);
            (param_ty.ty.clone(), span)
        })
        .collect();
    (pairs, function.bounds.clone())
}

fn variant_field_types(fields: &VariantFields) -> Vec<(Type, Span)> {
    match fields {
        VariantFields::Unit => Vec::new(),
        VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields
            .iter()
            .map(|field| (field.ty.clone(), field.annotation.get_span()))
            .collect(),
    }
}

fn alias_body_types(alias: &AliasKind) -> Vec<(Type, Span)> {
    match alias {
        AliasKind::Opaque(_) => Vec::new(),
        AliasKind::Transparent { annotation, target } => {
            vec![(target.clone(), annotation.get_span())]
        }
    }
}

fn populate_expression_generic_bounds(
    expression: &mut Expression,
    bound_types: &rustc_hash::FxHashMap<Span, Type>,
) {
    match expression {
        Expression::Function { generics, .. }
        | Expression::Struct { generics, .. }
        | Expression::Enum { generics, .. }
        | Expression::TypeAlias { generics, .. } => populate_generic_bounds(generics, bound_types),
        Expression::ImplBlock {
            generics, methods, ..
        } => {
            populate_generic_bounds(generics, bound_types);
            for method in methods {
                populate_expression_generic_bounds(method, bound_types);
            }
        }
        Expression::Interface {
            generics,
            method_signatures,
            ..
        } => {
            populate_generic_bounds(generics, bound_types);
            for method in method_signatures {
                populate_expression_generic_bounds(method, bound_types);
            }
        }
        _ => {}
    }
}

fn populate_generic_bounds(
    generics: &mut [Generic],
    bound_types: &rustc_hash::FxHashMap<Span, Type>,
) {
    for generic in generics {
        generic.resolve_bounds_with(|bound| {
            bound_types
                .get(&bound.get_span())
                .cloned()
                .unwrap_or(Type::Error)
        });
    }
}

pub(super) fn enum_variant_constructor_type(
    enum_variant: &EnumVariant,
    enum_ty: &Type,
    generics: &[Generic],
) -> Type {
    if enum_variant.fields.is_empty() {
        return enum_ty.clone();
    }

    let return_type = match enum_ty {
        Type::Forall { body, .. } => body.as_ref().clone(),
        _ => enum_ty.clone(),
    };

    let fn_ty = Type::function(
        enum_variant
            .fields
            .iter()
            .map(|field| FunctionParameter::new(field.ty.clone(), false))
            .collect(),
        Default::default(),
        return_type.into(),
    );

    if generics.is_empty() {
        fn_ty
    } else {
        Type::Forall {
            vars: generics.iter().map(|g| g.name.clone()).collect(),
            body: Box::new(fn_ty),
        }
    }
}

pub(super) fn wrap_with_impl_generics(
    fn_ty: &Type,
    generics: &[Generic],
    impl_bounds: &[syntax::types::Bound],
) -> Type {
    if generics.is_empty() {
        return fn_ty.clone();
    }

    let impl_vars: Vec<syntax::EcoString> = generics.iter().map(|g| g.name.clone()).collect();

    let add_impl_bounds = |existing_bounds: &[syntax::types::Bound]| -> Vec<syntax::types::Bound> {
        impl_bounds
            .iter()
            .cloned()
            .chain(existing_bounds.iter().cloned())
            .collect()
    };

    match fn_ty {
        Type::Forall { vars, body } => {
            let new_body = match body.as_ref() {
                Type::Function(f) => f.rebuild(
                    f.params.clone(),
                    add_impl_bounds(&f.bounds),
                    f.return_type.clone(),
                ),
                _ => *body.clone(),
            };
            Type::Forall {
                vars: impl_vars.into_iter().chain(vars.clone()).collect(),
                body: Box::new(new_body),
            }
        }
        Type::Function(f) => Type::Forall {
            vars: impl_vars,
            body: Box::new(f.rebuild(
                f.params.clone(),
                add_impl_bounds(&f.bounds),
                f.return_type.clone(),
            )),
        },
        _ => Type::Forall {
            vars: impl_vars,
            body: Box::new(fn_ty.clone()),
        },
    }
}

fn type_contains_constructor(target_id: &str, ty: &Type) -> bool {
    walk_type(ty, &|id, _| id == target_id)
}

/// Check if a type contains a recursive generic instantiation.
/// E.g., a method on `Box<T>` returning `Box<Box<T>>` creates a Go instantiation cycle.
/// Returns true if `ty` contains `target_id` nested within itself (e.g. `Box<Box<T>>`).
pub(super) fn has_recursive_instantiation(target_id: &str, ty: &Type) -> bool {
    walk_type(ty, &|id, params| {
        id == target_id
            && params
                .iter()
                .any(|p| type_contains_constructor(target_id, p))
    })
}

fn walk_type(ty: &Type, predicate: &dyn Fn(&str, &[Type]) -> bool) -> bool {
    if let Type::Nominal { id, params, .. } = ty
        && predicate(id, params)
    {
        return true;
    }
    ty.children().iter().any(|c| walk_type(c, predicate))
}
