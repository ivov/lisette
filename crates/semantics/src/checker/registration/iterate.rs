use syntax::program::{Definition, DefinitionBody, Visibility};
use syntax::types::{CompoundKind, Symbol, Type};

use super::TaskState;
use crate::checker::registration::derived_attributes::{
    DerivedAttribute, DerivedAttributeContext, DerivedAttributeTarget,
};
use crate::store::Store;

impl TaskState {
    pub(super) fn process_iterate_candidate(
        &mut self,
        store: &mut Store,
        context: &DerivedAttributeContext,
        candidate: &DerivedAttribute,
    ) {
        let DerivedAttributeTarget::Enum {
            name,
            name_span,
            is_generic,
            payload_variant_span,
        } = &candidate.target
        else {
            self.sink
                .push(diagnostics::attribute::iterate_not_an_enum(&candidate.span));
            return;
        };

        if context.is_d_lis {
            self.sink
                .push(diagnostics::attribute::iterate_in_typedef(&candidate.span));
            return;
        }
        if *is_generic {
            self.sink.push(diagnostics::attribute::iterate_generic_enum(
                &candidate.span,
            ));
            return;
        }
        if let Some(variant_span) = payload_variant_span {
            self.sink
                .push(diagnostics::attribute::iterate_non_unit_variant(
                    &candidate.span,
                    variant_span,
                ));
            return;
        }

        let qualified = Symbol::from_parts(&context.module_id, name);
        let variants_key = qualified.with_segment("variants");

        // A static method or a variant literally named `variants` both register
        // a `Value` at `Enum.variants`; an instance method lands in the enum's
        // method map. Any of the three collides.
        let existing_span = store
            .get_definition(variants_key.as_str())
            .and_then(|definition| definition.name_span);
        let (has_instance_variants, visibility) = match store.get_definition(qualified.as_str()) {
            Some(definition) => (
                matches!(&definition.body, DefinitionBody::Enum { methods, .. } if methods.contains_key("variants")),
                definition.visibility.clone(),
            ),
            None => (false, Visibility::Private),
        };
        if existing_span.is_some() || has_instance_variants {
            self.sink
                .push(diagnostics::attribute::iterate_variants_conflict(
                    &candidate.span,
                    existing_span.as_ref(),
                ));
            return;
        }

        let Some(enum_ty) = store.get_type(qualified.as_str()).cloned() else {
            return;
        };

        let slice_ty = Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![enum_ty],
        };
        let fn_ty = Type::function(vec![], Default::default(), Box::new(slice_ty));

        let module = store
            .get_module_mut(&context.module_id)
            .expect("module must exist");
        module.definitions.insert(
            variants_key,
            Definition {
                visibility,
                ty: fn_ty,
                name_span: Some(*name_span),
                doc: None,
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            },
        );
    }
}
