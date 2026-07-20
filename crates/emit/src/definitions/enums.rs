use crate::Planner;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD, EnumLayout};
use crate::definitions::structs::should_synthesize_stringer;
use crate::names::go_name::{self, prelude_qualifier};
use crate::utils::{synthesized_local_name, synthesized_receiver_name};
use syntax::ast::{Attribute, Generic};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(crate) fn emit_enum(
        &mut self,
        name: &str,
        generics: &[Generic],
        attributes: &[Attribute],
    ) -> Option<String> {
        if matches!(name, "Option" | "Result" | "Partial") {
            return None;
        }

        let enum_id = self.facts.qualified_current(name);

        let layout = self.enum_layout(&enum_id)?;

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
            let _ = self.go_type_string(ty);
        }

        let generics_string = self.generics_to_string(generics);
        let receiver_generics = self.receiver_generics_string(generics);
        let has_json = attributes.iter().any(|a| a.name == "json");
        let has_iterate = attributes.iter().any(|a| a.name == "iterate");

        let synthesize = should_synthesize_stringer(attributes);
        let (has_user_string, has_user_go_string) = self.stringer_overrides(name);
        let emit_string = synthesize && !has_user_string;
        let emit_go_string = synthesize && !has_user_go_string;
        let needs_fmt = emit_string || emit_go_string || has_json;
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
                .is_some_and(|definition| definition.visibility.is_public());
            let fn_name = self.variants_go_name(name, is_public);
            result.push_str("\n\n");
            result.push_str(&layout.emit_variants_function(&fn_name));
        }
        self.append_enum_debug_method(&mut result, name, &receiver_generics, &layout);
        self.append_to_string_method(&mut result, name, &receiver_generics, attributes);
        self.append_enum_equals_method(&mut result, name, &enum_id, &receiver_generics);
        if needs_fmt {
            self.require_fmt();
        }
        if has_json {
            self.require_json();
        }

        Some(result)
    }

    fn append_enum_debug_method(
        &self,
        out: &mut String,
        name: &str,
        receiver_generics: &str,
        layout: &EnumLayout,
    ) {
        if !self.synthesizes_debug_string(name) {
            return;
        }
        out.push_str("\n\n");
        out.push_str(&layout.emit_debug_method(receiver_generics, prelude_qualifier()));
        self.require_fmt();
        if layout.debug_uses_prelude() {
            self.require_stdlib();
        }
    }

    fn append_enum_equals_method(
        &mut self,
        out: &mut String,
        name: &str,
        enum_id: &str,
        receiver_generics: &str,
    ) {
        if !self.should_synthesize_equals(name) {
            return;
        }
        let Some(Definition {
            body:
                DefinitionBody::Enum {
                    variants: sem_variants,
                    ..
                },
            ..
        }) = self.facts.definition(enum_id)
        else {
            return;
        };
        let sem_variants = sem_variants.clone();
        let layout = self.enum_layout(enum_id).expect("enum layout should exist");

        let receiver = synthesized_receiver_name(name, receiver_generics);
        let other = synthesized_local_name("other", &receiver, receiver_generics);
        let go_type_name = go_name::escape_type_name(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        let go_method = self.equals_method_go_name();

        let mut cases: Vec<String> = Vec::new();
        for (sem_variant, layout_variant) in sem_variants.iter().zip(layout.variants.iter()) {
            if sem_variant.fields.is_empty() {
                continue;
            }
            let comparisons: Vec<String> = sem_variant
                .fields
                .iter()
                .zip(layout_variant.fields.iter())
                .map(|(sem_field, layout_field)| {
                    let mut lhs = format!("{receiver}.{}", layout_field.go_name);
                    let mut rhs = format!("{other}.{}", layout_field.go_name);
                    if layout_field.is_recursive {
                        lhs = format!("(*{lhs})");
                        rhs = format!("(*{rhs})");
                    }
                    self.render_equality(&lhs, &rhs, &sem_field.ty)
                })
                .collect();
            cases.push(format!(
                "case {}:\nreturn {}",
                layout_variant.tag_constant,
                comparisons.join(" && ")
            ));
        }

        out.push_str("\n\n");
        if cases.is_empty() {
            out.push_str(&format!(
                "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nreturn {receiver}.Tag == {other}.Tag\n}}"
            ));
            return;
        }
        let mut method = format!(
            "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nif {receiver}.Tag != {other}.Tag {{\nreturn false\n}}\nswitch {receiver}.Tag {{\n"
        );
        for case in &cases {
            method.push_str(case);
            method.push('\n');
        }
        method.push_str("default:\nreturn true\n}\n}");
        out.push_str(&method);
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
    ) -> String {
        let layout = self.enum_layout(enum_id).expect("enum layout should exist");
        let variant = layout
            .get_variant(variant_name)
            .expect("variant should exist in layout");

        let enum_name = layout.enum_name.clone();
        let generics = layout.generics.clone();
        let go_type_name = go_name::escape_type_name(&enum_name);
        let func_name = format!("Make{}{}", go_type_name, variant.name);
        let tag_constant = variant.tag_constant.clone();

        let (fields, params): (Vec<_>, Vec<_>) = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let argument = format!("arg{}", index);
                if field.is_recursive {
                    let pointee = field.go_type.trim_start_matches('*');
                    let param = format!("{} {}", argument, pointee);
                    let field_assignment = format!("{}: &{}", field.go_name, argument);
                    (field_assignment, param)
                } else {
                    let param = format!("{} {}", argument, field.go_type);
                    let field_assignment = format!("{}: {}", field.go_name, argument);
                    (field_assignment, param)
                }
            })
            .unzip();
        let fields = fields.join(", ");
        let params = params.join(", ");

        let (generic_params, generic_args) = if generics.is_empty() {
            (String::new(), String::new())
        } else {
            let args = generics
                .iter()
                .map(|g| self.generic_go_name(&g.name))
                .collect::<Vec<_>>()
                .join(", ");
            let generics_string = self.generics_to_string(&generics);
            (generics_string, format!("[{}]", args))
        };

        let return_type = Type::Nominal {
            id: Symbol::from_raw(enum_name.clone()),
            params: generics
                .iter()
                .map(|g| Type::Parameter(g.name.clone()))
                .collect(),
            underlying_ty: None,
        };

        let return_type = self.go_type_string(&return_type);

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
