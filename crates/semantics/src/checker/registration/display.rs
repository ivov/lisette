use syntax::ast::Span;
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

use super::{TaskState, wrap_with_impl_generics};
use crate::call_classification::is_ufcs_method_type;
use crate::checker::registration::derived_attributes::{
    DerivedAttribute, DerivedAttributeKind, DerivedAttributeTarget,
};
use crate::store::Store;

impl TaskState {
    pub(super) fn register_display(&mut self, store: &mut Store, candidates: &[DerivedAttribute]) {
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.kind == DerivedAttributeKind::Display)
        {
            self.process_display_candidate(store, candidate);
        }
    }

    fn process_display_candidate(&mut self, store: &mut Store, candidate: &DerivedAttribute) {
        let (name, is_struct) = match &candidate.target {
            DerivedAttributeTarget::Misplaced => {
                self.sink
                    .push(diagnostics::attribute::display_not_a_struct_or_enum(
                        &candidate.span,
                    ));
                return;
            }
            DerivedAttributeTarget::Struct { name } => (name, true),
            DerivedAttributeTarget::Enum { name, .. } => (name, false),
        };

        if candidate.has_args {
            self.sink
                .push(diagnostics::attribute::display_with_arguments(
                    &candidate.span,
                ));
            return;
        }
        if candidate.is_d_lis {
            self.sink
                .push(diagnostics::attribute::display_in_typedef(&candidate.span));
            return;
        }

        let qualified = Symbol::from_parts(&candidate.module_id, name);
        if is_struct
            && let Some(definition) = store.get_definition(qualified.as_str())
            && definition.is_pointer_backed_newtype(|id| {
                store
                    .get_definition(id)
                    .is_some_and(Definition::is_type_alias)
            })
        {
            self.sink
                .push(diagnostics::attribute::display_on_pointer_newtype(
                    &candidate.span,
                ));
            return;
        }

        self.synthesize_to_string(store, &candidate.module_id, &candidate.span, &qualified);
    }

    fn synthesize_to_string(
        &mut self,
        store: &mut Store,
        module_id: &str,
        attribute_span: &Span,
        qualified: &Symbol,
    ) {
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

        if let Some(user_ty) = user_to_string_type(store, qualified) {
            if is_ufcs_method_type(&user_ty, generics.len()) {
                self.sink
                    .push(diagnostics::attribute::display_specialized_to_string(
                        attribute_span,
                    ));
                return;
            }
            if user_ty.is_stringer_signature() {
                return;
            }
        }

        let receiver_ty = match scheme {
            Type::Forall { body, .. } => *body,
            other => other,
        };
        let fn_ty = Type::function(
            vec![receiver_ty],
            vec![false],
            Default::default(),
            Box::new(Type::string()),
        );
        let method_ty = wrap_with_impl_generics(&fn_ty, &generics, &[]);

        let to_string_key = qualified.with_segment("to_string");
        let module = store.get_module_mut(module_id).expect("module must exist");
        if let Some(methods) = module
            .definitions
            .get_mut(qualified.as_str())
            .and_then(Definition::methods_mut)
        {
            methods.insert("to_string".into(), method_ty.clone());
        }
        module
            .definitions
            .entry(to_string_key)
            .or_insert_with(|| Definition {
                visibility,
                ty: method_ty,
                name: None,
                name_span,
                doc: None,
                body: DefinitionBody::Value {
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                    const_value: None,
                },
            });
    }
}

fn user_to_string_type(store: &Store, qualified: &Symbol) -> Option<Type> {
    match &store.get_definition(qualified.as_str())?.body {
        DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. } => {
            methods.get("to_string").cloned()
        }
        _ => None,
    }
}

fn type_generics(definition: &Definition) -> Option<Vec<syntax::ast::Generic>> {
    match &definition.body {
        DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
            Some(generics.clone())
        }
        _ => None,
    }
}
