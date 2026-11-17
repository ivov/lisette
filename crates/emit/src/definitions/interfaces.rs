use crate::Planner;
use crate::abi::callable::CallableReturnAbi;
use crate::names::go_name;
use crate::plan::declarations::{DeclarationKind, GoDeclaration, InterfaceMethod};
use crate::types::go_type::GoType;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface};
use syntax::program::Definition;
use syntax::types::unqualified_name;

impl Planner<'_> {
    pub(crate) fn emit_interface(
        &mut self,
        name: &str,
        items: &[Expression],
        parents: &[ParentInterface],
        generics: &[Generic],
        is_public: bool,
    ) -> GoDeclaration {
        let name_go = go_name::escape_type_name(name).into_owned();
        if self.facts.is_current_package(go_name::PRELUDE_PACKAGE) {
            return GoDeclaration::new(DeclarationKind::Struct {
                name: name_go,
                parameters: Vec::new(),
                fields: Vec::new(),
            });
        }
        let filtered = strip_self_referential_bounds(generics, name);
        GoDeclaration::new(DeclarationKind::Interface {
            name: name_go,
            parameters: self.plan_generics(
                &filtered
                    .iter()
                    .map(|generic| {
                        (
                            generic.name.clone(),
                            generic
                                .resolved_bounds()
                                .expect("generic bounds are resolved before emission")
                                .cloned()
                                .collect(),
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            parents: parents
                .iter()
                .map(|parent| self.go_type(&parent.ty))
                .collect(),
            methods: items
                .iter()
                .map(|item| self.emit_interface_method(name, item, is_public))
                .collect(),
        })
    }

    /// Emit one interface method signature.
    fn emit_interface_method(
        &mut self,
        interface_name: &str,
        item: &Expression,
        is_public: bool,
    ) -> InterfaceMethod {
        let func = item.function_definition_view();
        let ty = item.get_type();
        let all_args = ty
            .get_function_params()
            .expect("interface method must have function type");

        let arguments: Vec<GoType> = all_args
            .iter()
            .map(|parameter| self.go_type(&parameter.ty))
            .collect();
        let raw_return_ty = ty
            .get_function_ret()
            .expect("interface method must have return type")
            .clone();
        let qualified_id = self.facts.qualified_current(interface_name);
        // Looked up by source name: a sealed method is keyed differently.
        let method = self
            .facts
            .definition(&qualified_id)
            .and_then(Definition::methods)
            .and_then(|methods| {
                methods
                    .values()
                    .find(|method| method.source_name == *func.name)
            })
            .expect("interface method must be registered");
        let return_abi = self.interface_method_return_abi(func.name, method);
        let return_type = if return_abi.is_lowered() {
            self.callable_return_go_type(&return_abi, &raw_return_ty)
        } else {
            self.go_type(&raw_return_ty)
        };

        let mut method_name = self.method_go_name(func.name, is_public);
        if self.facts.method_uses_tagged_return(func.name) {
            method_name = go_name::tagged_twin(&method_name);
        }

        InterfaceMethod {
            name: method_name,
            parameters: arguments,
            result: if return_abi == CallableReturnAbi::Void {
                GoType::default()
            } else {
                return_type
            },
        }
    }
}

fn bound_references_interface(annotation: &Annotation, interface_name: &str) -> bool {
    let Annotation::Constructor { name, .. } = annotation else {
        return false;
    };
    unqualified_name(name) == interface_name
}

fn strip_self_referential_bounds(generics: &[Generic], interface_name: &str) -> Vec<Generic> {
    generics
        .iter()
        .cloned()
        .map(|mut generic| {
            generic.retain_bounds(|bound| !bound_references_interface(bound, interface_name));
            generic
        })
        .collect()
}
