use crate::lookup::facts::definition_key::DeclaredName;
use crate::lookup::facts::return_abi_rules::interface_method_return_abi;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::declarations::{DeclarationKind, GoDeclaration, InterfaceMethod};
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name::{self, GoExport};
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, Generic, ParentInterface};
use syntax::program::PRELUDE_PACKAGE_ID;

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn emit_interface(
        &self,
        name: DeclaredName<'_>,
        items: &[Expression],
        parents: &[ParentInterface],
        generics: &[Generic],
        export: GoExport,
    ) -> GoDeclaration {
        let name_go = go_name::escape_type_name(name.as_str()).into_owned();
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
        interface_name: DeclaredName<'_>,
        item: &Expression,
        export: GoExport,
    ) -> InterfaceMethod {
        let function = item.function_definition_view();
        let ty = item.get_type();
        let signature = ty
            .as_function_type()
            .expect("interface method must have function type");

        let arguments: Vec<GoType> = signature
            .params
            .iter()
            .map(|parameter| self.go_type(&parameter.ty))
            .collect();
        let raw_return_ty = signature.return_type.as_ref().clone();
        let qualified_id = self.facts.qualified_current(interface_name);
        let method = self
            .facts
            .definition(&qualified_id)
            .methods()
            .and_then(|methods| {
                methods
                    .values()
                    .find(|method| method.source_name == *function.name)
            })
            .expect("interface method must be registered");

        let return_abi = interface_method_return_abi(self.facts, method);

        let method_name = self
            .facts
            .interface_method_go_name(function.name, &method.ty, export);

        InterfaceMethod {
            name: method_name,
            parameters: arguments,
            result: self.return_go_type(&return_abi, &raw_return_ty),
        }
    }
}
