use crate::Planner;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::return_abi;
use crate::target::ir::declarations::{DeclarationKind, GoDeclaration, InterfaceMethod};
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name::{self, GoExport};
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, Generic, ParentInterface};
use syntax::program::{Definition, PRELUDE_PACKAGE_ID};
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn emit_interface(
        &self,
        name: &str,
        items: &[Expression],
        parents: &[ParentInterface],
        generics: &[Generic],
        export: GoExport,
    ) -> GoDeclaration {
        let name_go = go_name::escape_type_name(name).into_owned();
        if self.facts.is_current_package(PRELUDE_PACKAGE_ID) {
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
                &generics
                    .iter()
                    .map(|generic| {
                        let (name, bounds) = generic_bounds(generic);
                        let bounds = bounds
                            .into_iter()
                            .filter(|bound| bound.get_qualified_id() != Some(&interface_id))
                            .collect();
                        (name, bounds)
                    })
                    .collect::<Vec<_>>(),
            ),
            parents: parents
                .iter()
                .map(|parent| self.go_type(&parent.ty))
                .collect(),
            methods: items
                .iter()
                .map(|item| self.emit_interface_method(name, item, export))
                .collect(),
        })
    }

    fn emit_interface_method(
        &self,
        interface_name: &str,
        item: &Expression,
        export: GoExport,
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

        let return_abi = return_abi(
            &self.facts,
            method.ty.get_function_ret().unwrap_or(&Type::Never),
            ReturnSlot::InterfaceMethod {
                name: func.name,
                method,
            },
        );

        let method_name = self.facts.interface_method_go_name(func.name, export);

        InterfaceMethod {
            name: method_name,
            parameters: arguments,
            result: self.return_go_type(&return_abi, &raw_return_ty),
        }
    }
}
