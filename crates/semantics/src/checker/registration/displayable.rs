use syntax::ast::{Attribute, Expression};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::Symbol;

use super::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    pub(super) fn register_displayable(&mut self, store: &Store, items: &[Expression]) {
        let module_id = self.cursor.module_id.clone();
        let is_d_lis = self.is_d_lis(store);
        for item in items {
            self.check_displayable_item(store, &module_id, item, is_d_lis);
        }
    }

    pub(super) fn register_module_displayable(&mut self, store: &Store, module_id: &str) {
        let module = store.get_module(module_id).expect("module must exist");
        for file in module.files.values() {
            let is_d_lis = file.is_d_lis();
            for item in &file.items {
                self.check_displayable_item(store, module_id, item, is_d_lis);
            }
        }
    }

    fn check_displayable_item(
        &mut self,
        store: &Store,
        module_id: &str,
        item: &Expression,
        is_d_lis: bool,
    ) {
        match item {
            Expression::Struct {
                attributes,
                name,
                fields,
                ..
            } => {
                if let Some(attribute) = displayable_attribute(attributes) {
                    self.validate_displayable(store, module_id, attribute, is_d_lis, Some(name));
                }
                for field in fields {
                    self.reject_misplaced_displayable(&field.attributes);
                }
            }
            Expression::Enum { attributes, .. } => {
                if let Some(attribute) = displayable_attribute(attributes) {
                    self.validate_displayable(store, module_id, attribute, is_d_lis, None);
                }
            }
            Expression::Function { attributes, .. } => {
                self.reject_misplaced_displayable(attributes);
            }
            Expression::ImplBlock { methods, .. } => {
                self.reject_misplaced_displayable_methods(methods)
            }
            Expression::Interface {
                method_signatures, ..
            } => self.reject_misplaced_displayable_methods(method_signatures),
            _ => {}
        }
    }

    fn validate_displayable(
        &mut self,
        store: &Store,
        module_id: &str,
        attribute: &Attribute,
        is_d_lis: bool,
        struct_name: Option<&str>,
    ) {
        if !attribute.args.is_empty() {
            self.sink
                .push(diagnostics::attribute::displayable_with_arguments(
                    &attribute.span,
                ));
            return;
        }
        if is_d_lis {
            self.sink
                .push(diagnostics::attribute::displayable_in_typedef(
                    &attribute.span,
                ));
            return;
        }
        if let Some(name) = struct_name {
            let qualified = Symbol::from_parts(module_id, name);
            if let Some(definition) = store.get_definition(qualified.as_str())
                && is_pointer_backed_newtype(definition)
            {
                self.sink
                    .push(diagnostics::attribute::displayable_on_pointer_newtype(
                        &attribute.span,
                    ));
            }
        }
    }

    fn reject_misplaced_displayable(&mut self, attributes: &[Attribute]) {
        if let Some(attribute) = displayable_attribute(attributes) {
            self.sink
                .push(diagnostics::attribute::displayable_not_a_struct_or_enum(
                    &attribute.span,
                ));
        }
    }

    fn reject_misplaced_displayable_methods(&mut self, methods: &[Expression]) {
        for method in methods {
            if let Expression::Function { attributes, .. } = method {
                self.reject_misplaced_displayable(attributes);
            }
        }
    }
}

fn displayable_attribute(attributes: &[Attribute]) -> Option<&Attribute> {
    attributes.iter().find(|a| a.name == "displayable")
}

fn is_pointer_backed_newtype(definition: &Definition) -> bool {
    definition.is_newtype()
        && matches!(
            &definition.body,
            DefinitionBody::Struct { fields, .. } if fields[0].ty.is_ref()
        )
}
