use crate::EmitEffects;
use crate::Planner;
use crate::names::go_name;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface, Pattern};
use syntax::types::unqualified_name;

impl Planner<'_> {
    pub(crate) fn emit_interface(
        &mut self,
        name: &str,
        items: &[Expression],
        parents: &[ParentInterface],
        generics: &[Generic],
        is_public: bool,
        fx: &mut EmitEffects,
    ) -> String {
        if self.facts.is_current_module(go_name::PRELUDE_MODULE) {
            return format!("type {} struct{{}}", name);
        }

        let filtered = strip_self_referential_bounds(generics, name);
        let symbol = self.facts.qualified_current(name);
        let generics_str = self.generics_to_string_for_symbol(&symbol, &filtered, fx);

        let mut output = Vec::new();
        output.push(format!(
            "type {}{} interface {{",
            go_name::escape_keyword(name),
            generics_str
        ));

        for parent in parents {
            output.push(self.go_type_string(&parent.ty, fx));
        }

        for item in items {
            output.push(self.emit_interface_method(name, item, is_public, fx));
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
        fx: &mut EmitEffects,
    ) -> String {
        let func = item.function_definition_view();
        let ty = item.get_type();
        let all_args = ty
            .get_function_params()
            .expect("interface method must have function type");

        let has_self_receiver = func.params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
                && p.annotation.is_none()
        });
        let args: Vec<String> = all_args
            .iter()
            .skip(if has_self_receiver { 1 } else { 0 })
            .map(|a| self.go_type_string(a, fx))
            .collect();
        let raw_return_ty = ty
            .get_function_ret()
            .expect("interface method must have return type")
            .clone();
        let qualified_id = self.facts.qualified_current(interface_name);
        let hints = self.go_interface_method_hints(&qualified_id, func.name);
        let return_type = match self.classify_with_go_hints(&raw_return_ty, &hints) {
            Some(shape) => self.render_lowered_return_ty(&shape, &raw_return_ty, fx),
            None => self.go_type_string(&raw_return_ty, fx),
        };

        let method_name = if is_public || self.method_needs_export(func.name) {
            go_name::snake_to_camel(func.name)
        } else {
            go_name::escape_keyword(func.name).into_owned()
        };

        if return_type == "struct{}" {
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
        .map(|g| Generic {
            name: g.name.clone(),
            bounds: g
                .bounds
                .iter()
                .filter(|ann| !bound_references_interface(ann, interface_name))
                .cloned()
                .collect(),
            span: g.span,
        })
        .collect()
}
