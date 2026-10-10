use crate::Planner;
use crate::names::go_name;
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
    ) -> String {
        if self.facts.is_current_package(go_name::PRELUDE_PACKAGE) {
            return format!("type {} struct{{}}", name);
        }

        let filtered = strip_self_referential_bounds(generics, name);
        let generics_str = self.generics_to_string(&filtered);

        let mut output = Vec::new();
        output.push(format!(
            "type {}{} interface {{",
            go_name::escape_type_name(name),
            generics_str
        ));

        for parent in parents {
            output.push(self.use_go_type(&parent.ty));
        }

        for item in items {
            output.push(self.emit_interface_method(name, item, is_public));
        }

        output.push("}".to_string());

        output.join("\n")
    }

    /// Emit one interface method signature.
    fn emit_interface_method(
        &mut self,
        interface_name: &str,
        item: &Expression,
        is_public: bool,
    ) -> String {
        let func = item.function_definition_view();
        let ty = item.get_type();
        let all_args = ty
            .get_function_params()
            .expect("interface method must have function type");

        let args: Vec<String> = all_args
            .iter()
            .map(|param| self.use_go_type(&param.ty))
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
        let return_type = match return_abi.lowered() {
            Some(lowered) => self.render_lowered_return_ty(lowered, &raw_return_ty),
            None => self.use_go_type(&raw_return_ty),
        };

        let method_name = self.method_go_name(func.name, is_public);

        if self.interface_method_returns_void(&return_abi, &raw_return_ty) {
            format!("{}({})", method_name, args.join(", "))
        } else {
            format!("{}({}) {}", method_name, args.join(", "), return_type)
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
