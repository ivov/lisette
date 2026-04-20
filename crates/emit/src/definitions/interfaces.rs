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
            if let Type::Constructor { id, params, .. } = &parent.ty {
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
            output.push(self.emit_interface_method(item, is_public));
        }

        output.push("}".to_string());

        output.join("\n")
    }

    /// Emit one interface method signature. Drops a leading `self` receiver
    /// from the param list since interface methods take their receiver
    /// implicitly. Unit returns (`struct{}`) are elided from the Go signature.
    fn emit_interface_method(&mut self, item: &Expression, is_public: bool) -> String {
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
        let return_type = self.go_type_as_string(
            ty.get_function_ret()
                .expect("interface method must have return type"),
        );

        let method_name = if is_public || self.method_needs_export(&func.name) {
            go_name::capitalize_first(&func.name)
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
