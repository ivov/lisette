use crate::Emitter;
use crate::names::generics::receiver_generics_string;
use crate::names::go_name;
use syntax::ast::{Attribute, EnumVariant, Generic};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

impl Emitter<'_> {
    pub(crate) fn emit_enum(
        &mut self,
        name: &str,
        generics: &[Generic],
        attributes: &[Attribute],
    ) -> Option<String> {
        if matches!(name, "Option" | "Result" | "Partial") {
            return None;
        }

        let enum_id = format!("{}.{}", self.current_module, name);

        if !self.module.enum_layouts.contains_key(&enum_id) {
            return None;
        }

        let variant_field_types: Vec<Type> = if let Some(Definition {
            body: DefinitionBody::Enum { variants, .. },
            ..
        }) = self.ctx.definitions.get(enum_id.as_str())
        {
            variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|f| f.ty.clone()))
                .collect()
        } else {
            Vec::new()
        };
        for ty in &variant_field_types {
            let _ = self.go_type_as_string(ty);
        }

        let generics = self.merge_impl_bounds(name, generics);
        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let map_key_generics = self.enum_map_key_generics(&enum_id, &generic_names);
        let generics_string = self.generics_to_string_with_map_keys(&generics, &map_key_generics);
        let receiver_generics = receiver_generics_string(&generics);
        let has_json = attributes.iter().any(|a| a.name == "json");

        let layout = self.module.enum_layouts.get(&enum_id).unwrap();
        let mut result = layout.emit_definition(&generics_string);
        if let Some(stringer_name) = self.stringer_method_name(name) {
            result.push_str("\n\n");
            result.push_str(&layout.emit_stringer_method(&receiver_generics, stringer_name));
            self.ensure_imported.insert("fmt".to_string());
        }
        if has_json {
            result.push_str("\n\n");
            result.push_str(&layout.emit_json_methods(&receiver_generics));
        }
        if has_json {
            self.ensure_imported.insert("encoding/json".to_string());
        }

        Some(result)
    }

    pub(crate) fn register_prelude_make_functions(&mut self) {
        for prelude_type in crate::PreludeType::enum_types() {
            for (constructor, make_fn) in prelude_type.make_function_entries() {
                self.module.make_functions.insert(constructor, make_fn);
            }
        }
    }

    pub(crate) fn register_make_functions(&mut self, name: &str, variants: &[EnumVariant]) {
        let go_type_name = go_name::escape_keyword(name);
        for variant in variants {
            let constructor = format!("{}.{}", name, variant.name);
            let fn_name = format!("Make{}{}", go_type_name, variant.name);
            self.module.make_functions.insert(constructor, fn_name);
        }
    }

    pub(crate) fn create_make_function_code(
        &mut self,
        enum_id: &str,
        variant_name: &str,
    ) -> String {
        let layout = self
            .module
            .enum_layouts
            .get(enum_id)
            .expect("enum layout should exist");
        let variant = layout
            .get_variant(variant_name)
            .expect("variant should exist in layout");

        let enum_name = layout.enum_name.clone();
        let generics = layout.generics.clone();
        let go_type_name = go_name::escape_keyword(&enum_name);
        let func_name = format!("Make{}{}", go_type_name, variant.name);
        let tag_constant = variant.tag_constant.clone();

        let (fields, params): (Vec<_>, Vec<_>) = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let argument = format!("arg{}", index);
                let param = format!("{} {}", argument, field.go_type);
                let field_assignment = format!("{}: {}", field.go_name, argument);
                (field_assignment, param)
            })
            .unzip();
        let fields = fields.join(", ");
        let params = params.join(", ");

        let (generic_params, generic_args) = if generics.is_empty() {
            (String::new(), String::new())
        } else {
            let args = generics
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
            let map_key_generics = self.enum_map_key_generics(enum_id, &generic_names);
            let generics_string =
                self.generics_to_string_with_map_keys(&generics, &map_key_generics);
            (generics_string, format!("[{}]", args))
        };

        let return_type = Type::Nominal {
            id: Symbol::from_raw(enum_name.clone()),
            params: generics
                .iter()
                .map(|g| Type::Nominal {
                    id: Symbol::from_raw(g.name.clone()),
                    params: vec![],
                    underlying_ty: None,
                })
                .collect(),
            underlying_ty: None,
        };

        let return_type = self.go_type_as_string(&return_type);

        format!(
            "func {} {} ({}) {} {{\n    return {} {} {{ Tag: {}, {} }}\n}}",
            func_name,
            generic_params,
            params,
            return_type,
            go_type_name,
            generic_args,
            tag_constant,
            fields
        )
    }
}
