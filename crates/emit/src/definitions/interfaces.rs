use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::names::go_name;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface, Pattern};
use syntax::types::{Type, unqualified_name};

impl Emitter<'_> {
    pub(crate) fn emit_interface(
        &mut self,
        name: &str,
        items: &[Expression],
        parents: &[ParentInterface],
        generics: &[Generic],
        is_public: bool,
    ) -> String {
        if self.current_module == go_name::PRELUDE_MODULE {
            return format!("type {} struct{{}}", name);
        }

        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let method_types: Vec<Type> = items.iter().map(|item| item.get_type()).collect();
        let mut map_key_generics =
            Self::collect_map_key_generics(method_types.iter(), &generic_names);

        let mut visited = HashSet::default();
        for parent in parents {
            if let Type::Nominal { id, params, .. } = &parent.ty {
                for position in self.map_key_positions(id, &mut visited) {
                    if let Some(Type::Parameter(name)) = params.get(position)
                        && generic_names.contains(&name.as_ref())
                    {
                        map_key_generics.insert(name.to_string());
                    }
                }
            }
        }

        let filtered = strip_self_referential_bounds(generics, name);
        let generics_str = self.generics_to_string_with_map_keys(&filtered, &map_key_generics);

        let mut output = Vec::new();
        output.push(format!(
            "type {}{} interface {{",
            go_name::escape_keyword(name),
            generics_str
        ));

        for parent in parents {
            output.push(self.go_type_as_string(&parent.ty));
        }

        for item in items {
            output.push(self.emit_interface_method(name, item, is_public));
        }

        output.push("}".to_string());

        output.join("\n")
    }

    /// Emit one interface method signature: drop the implicit `self`,
    /// elide unit returns, and apply hint-aware boundary lowering so
    /// user interface methods emit the same Go shape struct impls do.
    fn emit_interface_method(
        &mut self,
        interface_name: &str,
        item: &Expression,
        is_public: bool,
    ) -> String {
        let func = item.to_function_definition();
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
            .map(|a| self.go_type_as_string(a))
            .collect();
        let raw_return_ty = ty
            .get_function_ret()
            .expect("interface method must have return type")
            .clone();
        let qualified_id = format!("{}.{}", self.current_module, interface_name);
        let hints = self.go_interface_method_hints(&qualified_id, &func.name);
        let return_type = match self.classify_with_go_hints(&raw_return_ty, &hints) {
            Some(shape) => self.render_lowered_return_ty(&shape, &raw_return_ty),
            None => self.go_type_as_string(&raw_return_ty),
        };

        let method_name = if is_public || self.method_needs_export(&func.name) {
            go_name::snake_to_camel(&func.name)
        } else {
            go_name::escape_keyword(&func.name).into_owned()
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
