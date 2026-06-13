use crate::EmitEffects;
use crate::Planner;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::structs::should_synthesize_stringer;
use crate::names::generics::receiver_generics_string;
use crate::names::go_name;
use syntax::ast::{Attribute, Generic};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(crate) fn emit_enum(
        &mut self,
        name: &str,
        generics: &[Generic],
        attributes: &[Attribute],
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if matches!(name, "Option" | "Result" | "Partial") {
            return None;
        }

        let enum_id = self.facts.qualified_current(name);

        if !self.module.has_enum_layout(&enum_id) {
            return None;
        }

        let variant_field_types: Vec<Type> = if let Some(Definition {
            body: DefinitionBody::Enum { variants, .. },
            ..
        }) = self.facts.definition(enum_id.as_str())
        {
            variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|f| f.ty.clone()))
                .collect()
        } else {
            Vec::new()
        };
        for ty in &variant_field_types {
            let _ = self.go_type_string(ty, fx);
        }

        let generics_string = self.generics_to_string_for_symbol(&enum_id, generics, fx);
        let receiver_generics = receiver_generics_string(generics);
        let has_json = attributes.iter().any(|a| a.name == "json");
        let has_iterate = attributes.iter().any(|a| a.name == "iterate");

        let synthesize = should_synthesize_stringer(attributes);
        let (has_user_string, has_user_go_string) = self.stringer_overrides(name);
        let emit_string = synthesize && !has_user_string;
        let emit_go_string = synthesize && !has_user_go_string;
        let needs_fmt = emit_string || emit_go_string || has_json;
        let layout = self.enum_layout(&enum_id).unwrap();
        let mut result = layout.emit_definition(&generics_string);
        if emit_string {
            result.push_str("\n\n");
            result.push_str(&layout.emit_stringer_method(
                &receiver_generics,
                ENUM_STRINGER_METHOD,
                false,
            ));
        }
        if emit_go_string {
            result.push_str("\n\n");
            result.push_str(&layout.emit_stringer_method(
                &receiver_generics,
                ENUM_GO_STRINGER_METHOD,
                true,
            ));
        }
        if has_json {
            result.push_str("\n\n");
            result.push_str(&layout.emit_json_methods(&receiver_generics));
        }
        if has_iterate {
            let is_public = self
                .facts
                .definition(enum_id.as_str())
                .is_some_and(|definition| definition.visibility().is_public());
            let fn_name = self.variants_go_name(name, is_public);
            result.push_str("\n\n");
            result.push_str(&layout.emit_variants_function(&fn_name));
        }
        self.append_to_string_method(&mut result, name, &receiver_generics, attributes);
        if needs_fmt {
            fx.require_fmt();
        }
        if has_json {
            fx.require_json();
        }

        Some(result)
    }

    /// Export-aware Go name for an `#[iterate]` enum's synthesized `variants`
    /// function. Matches the static-method call-site naming so the definition
    /// and its calls agree.
    pub(crate) fn variants_go_name(&self, enum_name: &str, is_public: bool) -> String {
        go_name::iterate_variants_fn_name(
            enum_name,
            is_public || self.method_needs_export("variants"),
        )
    }

    pub(crate) fn create_make_function_code(
        &mut self,
        enum_id: &str,
        variant_name: &str,
        fx: &mut EmitEffects,
    ) -> String {
        let layout = self
            .module
            .enum_layout(enum_id)
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
            let generics_string = self.generics_to_string_for_symbol(enum_id, &generics, fx);
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

        let return_type = self.go_type_string(&return_type, fx);

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
