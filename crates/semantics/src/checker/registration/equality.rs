use rustc_hash::FxHashSet as HashSet;

use syntax::EcoString;
use syntax::ast::{Generic, Span, StructFields, VariantFields};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{FunctionParameter, Symbol, Type};

use super::{TaskState, resolved_generic_bounds, wrap_with_impl_generics};
use crate::checker::infer::expressions::comparison::{
    check_not_equatable, param_is_comparable, param_is_equatable,
};
use crate::checker::registration::derived_attributes::{
    DerivedAttribute, DerivedAttributeContext, DerivedAttributeTarget,
};
use crate::store::Store;

fn equals_visibility(store: &Store, id: &str) -> Option<String> {
    match store.get_method(id, "equals") {
        Some(method) if method.visibility.is_public() => None,
        _ => store.package_for_qualified_name(id).map(str::to_string),
    }
}

/// How a hand-written `equals` on a `#[equality]` type bears on synthesis.
enum UserEquals {
    /// No `equals`: gate the fields and synthesize.
    None,
    /// A valid full-type override.
    ValidReceiver,
    /// A partial or concrete receiver (`impl Box<int>`, `impl<T> Pair<T, T>`) or extra
    /// generics: does not cover every instantiation, so it cannot derive equality.
    Specialized,
    /// Wrong shape: cannot be the derived method and would collide with one.
    Conflict,
}

impl TaskState {
    fn process_equality_candidate(
        &mut self,
        store: &mut Store,
        context: &DerivedAttributeContext,
        candidate: &DerivedAttribute,
    ) -> Option<Symbol> {
        let name = match &candidate.target {
            DerivedAttributeTarget::Misplaced => {
                self.sink
                    .push(diagnostics::attribute::equality_not_a_struct_or_enum(
                        &candidate.span,
                    ));
                return None;
            }
            DerivedAttributeTarget::Struct { name } | DerivedAttributeTarget::Enum { name, .. } => {
                name
            }
        };

        if candidate.has_args {
            self.sink
                .push(diagnostics::attribute::equality_with_arguments(
                    &candidate.span,
                ));
            return None;
        }
        if context.is_d_lis {
            self.sink
                .push(diagnostics::attribute::equality_in_typedef(&candidate.span));
            return None;
        }

        let qualified = Symbol::from_parts(&context.package_id, name);
        if is_tuple_struct(store, &qualified) {
            self.sink
                .push(diagnostics::attribute::equality_on_tuple_struct(
                    &candidate.span,
                ));
            return None;
        }

        match user_equals(store, &qualified) {
            UserEquals::ValidReceiver => {
                if store.is_ufcs_method(qualified.as_str(), "equals") {
                    self.sink
                        .push(diagnostics::attribute::equality_specialized_equals(
                            &candidate.span,
                        ));
                    return None;
                }
                return None;
            }
            UserEquals::Conflict => {
                self.sink
                    .push(diagnostics::attribute::equality_conflicting_equals(
                        &candidate.span,
                    ));
                return None;
            }
            UserEquals::Specialized => {
                self.sink
                    .push(diagnostics::attribute::equality_specialized_equals(
                        &candidate.span,
                    ));
                return None;
            }
            UserEquals::None => {}
        }

        if has_hidden_user_equals(store, &qualified) {
            self.sink
                .push(diagnostics::attribute::equality_conflicting_equals(
                    &candidate.span,
                ));
            return None;
        }

        self.synthesize_equals(store, &context.package_id, &qualified);
        Some(qualified)
    }

    /// Synthesize queued equality methods, build the verdict, and gate derivations.
    /// Run once after registration has completed every type definition.
    pub(super) fn finalize_equality(&mut self, store: &mut Store) {
        let batches = std::mem::take(&mut self.pending.equality_attributes);
        let mut derivations = Vec::new();
        for batch in &batches {
            for candidate in &batch.candidates {
                if let Some(derivation) =
                    self.process_equality_candidate(store, &batch.context, candidate)
                {
                    derivations.push(derivation);
                }
            }
        }
        self.record_equality_index(store, &derivations);
        self.validate_equality_derivations(store, &derivations);
    }

    fn validate_equality_derivations(&mut self, store: &Store, derivations: &[Symbol]) {
        for qualified in derivations {
            let id = qualified.as_str();
            let package_id = store
                .package_for_qualified_name(id)
                .map(str::to_string)
                .unwrap_or_default();
            let name = syntax::types::unqualified_name(id).to_string();
            self.gate_equality_derivation(store, &name, qualified, &package_id);
        }
    }

    fn gate_equality_derivation(
        &mut self,
        store: &Store,
        type_name: &str,
        qualified: &Symbol,
        package_id: &str,
    ) {
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return;
        };
        let (generics, fields): (Vec<Generic>, Vec<(EcoString, Span, Type)>) = match &definition
            .body
        {
            DefinitionBody::Struct {
                generics, fields, ..
            } => (
                generics.clone(),
                fields
                    .iter()
                    .map(|f| (f.name.clone(), f.name_span, f.ty.clone()))
                    .collect(),
            ),
            DefinitionBody::Enum {
                generics, variants, ..
            } => {
                let mut specs: Vec<(EcoString, Span, Type)> = Vec::new();
                for variant in variants {
                    match &variant.fields {
                        VariantFields::Tuple(fields) => {
                            for field in fields {
                                specs.push((
                                    variant.name.clone(),
                                    variant.name_span,
                                    field.ty.clone(),
                                ));
                            }
                        }
                        VariantFields::Struct(fields) => {
                            for field in fields {
                                specs.push((field.name.clone(), field.name_span, field.ty.clone()));
                            }
                        }
                        VariantFields::Unit => {}
                    }
                }
                (generics.clone(), specs)
            }
            _ => return,
        };

        self.with_scope(|this| {
            this.put_in_scope(&generics);
            this.record_resolved_generic_bounds(&generics);

            for (field_name, field_span, field_ty) in &fields {
                let reason = check_not_equatable(
                    &this.env,
                    store,
                    field_ty,
                    package_id,
                    &|name| param_is_equatable(&this.scopes, &this.env, store, name),
                    &|name| param_is_comparable(&this.scopes, &this.env, name),
                );
                if let Some(reason) = reason {
                    this.sink
                        .push(diagnostics::attribute::cannot_derive_equality(
                            type_name, field_name, field_span, reason,
                        ));
                }
            }
        });
    }

    fn record_equality_index(&mut self, store: &mut Store, derivations: &[Symbol]) {
        let synthesized: HashSet<&str> = derivations.iter().map(Symbol::as_str).collect();

        let ids: Vec<Symbol> = store
            .packages
            .values()
            .flat_map(|package| package.definitions.iter())
            .filter_map(|(qualified, definition)| match &definition.body {
                DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. }
                    if methods.contains_key("equals") =>
                {
                    Some(qualified.clone())
                }
                _ => None,
            })
            .collect();

        for id in ids {
            let id_str = id.as_str();
            let visibility = equals_visibility(store, id_str);
            let classification = user_equals(store, &id);
            if store.is_ufcs_method(id_str, "equals")
                && matches!(
                    classification,
                    UserEquals::ValidReceiver | UserEquals::Specialized
                )
            {
                store
                    .equality_index
                    .insert_ufcs_lowered(id.to_string(), visibility);
            } else if matches!(classification, UserEquals::ValidReceiver) {
                if synthesized.contains(id_str) {
                    store
                        .equality_index
                        .insert_synthesized_method(id.to_string(), visibility);
                } else {
                    store
                        .equality_index
                        .insert_declared_method(id.to_string(), visibility);
                }
            }
        }
    }

    fn synthesize_equals(&mut self, store: &mut Store, package_id: &str, qualified: &Symbol) {
        let Some(scheme) = store.get_type(qualified.as_str()).cloned() else {
            return;
        };
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return;
        };
        let Some(generics) = type_generics(definition) else {
            return;
        };
        let visibility = definition.visibility.clone();
        let name_span = definition.name_span;

        let receiver_ty = match scheme {
            Type::Forall { body, .. } => *body,
            other => other,
        };
        let fn_ty = Type::function(
            vec![
                FunctionParameter::new(receiver_ty.clone(), false),
                FunctionParameter::new(receiver_ty, false),
            ],
            Default::default(),
            Box::new(Type::bool()),
        );
        let impl_bounds = resolved_generic_bounds(&generics);
        let method_ty = wrap_with_impl_generics(&fn_ty, &generics, &impl_bounds);

        let package = store
            .get_package_mut(package_id)
            .expect("package must exist");
        if let Some(methods) = package
            .definitions
            .get_mut(qualified.as_str())
            .and_then(Definition::methods_mut)
        {
            methods.insert(
                "equals".into(),
                syntax::program::Method {
                    source_name: "equals".into(),
                    ty: method_ty,
                    visibility,
                    name_span,
                    doc: None,
                    allowed_lints: vec![],
                    go_hints: vec![],
                },
            );
        }
    }
}

fn is_tuple_struct(store: &Store, qualified: &Symbol) -> bool {
    matches!(
        store.get_definition(qualified.as_str()).map(|d| &d.body),
        Some(DefinitionBody::Struct {
            fields: StructFields::Tuple(_),
            ..
        })
    )
}

fn has_hidden_user_equals(store: &Store, qualified: &Symbol) -> bool {
    let in_method_set = matches!(
        store.get_definition(qualified.as_str()).map(|d| &d.body),
        Some(DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. })
            if methods.contains_key("equals")
    );
    if in_method_set {
        return false;
    }
    store
        .get_method(qualified, "equals")
        .is_some_and(|method| method.name_span.is_some())
}

fn user_equals(store: &Store, qualified: &Symbol) -> UserEquals {
    let Some(definition) = store.get_definition(qualified.as_str()) else {
        return UserEquals::None;
    };
    let (methods, generics_len) = match &definition.body {
        DefinitionBody::Struct {
            methods, generics, ..
        }
        | DefinitionBody::Enum {
            methods, generics, ..
        } => (methods, generics.len()),
        _ => return UserEquals::None,
    };
    let Some(method_ty) = methods.get("equals") else {
        return UserEquals::None;
    };
    if method_ty
        .ty
        .equals_receiver_vars(qualified.as_str(), generics_len)
        .is_some()
    {
        UserEquals::ValidReceiver
    } else if method_ty.ty.is_equals_signature() {
        UserEquals::Specialized
    } else {
        UserEquals::Conflict
    }
}

fn type_generics(definition: &Definition) -> Option<Vec<Generic>> {
    match &definition.body {
        DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
            Some(generics.clone())
        }
        _ => None,
    }
}
