use crate::Planner;
use crate::abi::ReturnSlot;
use crate::ir::declarations::{DeclarationKind, GoDeclaration, InterfaceMethod};
use crate::names::generics::generic_bounds;
use crate::names::go_name;
use crate::types::go_type::GoType;
use syntax::ast::{Expression, Generic, ParentInterface};
use syntax::program::Definition;
use syntax::types::Type;

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
        let interface_id = self.facts.qualified_current(name);
        GoDeclaration::new(DeclarationKind::Interface {
            name: name_go,
            parameters: self.plan_generics(
                &generics.iter().map(generic_bounds).collect::<Vec<_>>(),
                Some(&interface_id),
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
        let return_abi = self.facts.return_abi(
            method.ty.get_function_ret().unwrap_or(&Type::Never),
            ReturnSlot::InterfaceMethod {
                name: func.name,
                method,
            },
        );

        let method_name = self.facts.interface_method_go_name(func.name, is_public);

        InterfaceMethod {
            name: method_name,
            parameters: arguments,
            result: self.return_go_type(&return_abi, &raw_return_ty),
        }
    }
}
