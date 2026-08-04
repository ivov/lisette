use syntax::ast::Span;
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{FunctionParameter, Symbol, Type};

use super::{TaskState, wrap_with_impl_generics};
use crate::checker::registration::derived_attributes::{
    DerivedAttribute, DerivedAttributeContext, DerivedAttributeTarget,
};
use crate::store::Store;

impl TaskState {
    pub(super) fn process_display_candidate(
        &mut self,
        store: &mut Store,
        context: &DerivedAttributeContext,
        candidate: &DerivedAttribute,
    ) {
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
        if context.is_d_lis {
            self.sink
                .push(diagnostics::attribute::display_in_typedef(&candidate.span));
            return;
        }

        let qualified = Symbol::from_parts(&context.package_id, name);
        if is_struct
            && let Some(definition) = store.get_definition(qualified.as_str())
            && definition.is_pointer_backed_newtype(|id| store.get_definition(id))
        {
            self.sink
                .push(diagnostics::attribute::display_on_pointer_newtype(
                    &candidate.span,
                ));
            return;
        }

        self.synthesize_to_string(store, &context.package_id, &candidate.span, &qualified);
    }

    fn synthesize_to_string(
        &mut self,
        store: &mut Store,
        package_id: &str,
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

        if let Some(user_ty) = definition
            .methods()
            .and_then(|methods| methods.get("to_string"))
            .cloned()
        {
            if definition.is_ufcs_method("to_string") {
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
            vec![FunctionParameter::new(receiver_ty, false)],
            Default::default(),
            Box::new(Type::string()),
        );
        let method_ty = wrap_with_impl_generics(&fn_ty, &generics, &[]);

        let to_string_key = qualified.with_segment("to_string");
        let package = store
            .get_package_mut(package_id)
            .expect("package must exist");
        if let Some(methods) = package
            .definitions
            .get_mut(qualified.as_str())
            .and_then(Definition::methods_mut)
        {
            methods.insert("to_string".into(), method_ty.clone());
        }
        package
            .definitions
            .entry(to_string_key)
            .or_insert_with(|| Definition {
                visibility,
                ty: method_ty,
                name_span,
                doc: None,
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            });
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
