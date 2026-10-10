use crate::Planner;
use crate::definitions::structs::SynthesizedMethod;
use crate::names::go_name;
use crate::plan::values::GoExpression;
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
        let enum_id = self.facts.qualified_current(name);

        let layout = self.facts.enum_layout(&enum_id)?;

        let generics_string = self.generics_to_string(generics);
        let receiver_generics = self.receiver_generics_string(generics);
        let has_iterate = attributes.iter().any(|a| a.name == "iterate");

        let mut result = layout.emit_definition(self, &generics_string);
        for method in self.enum_synthesized_methods(name, attributes) {
            let code = match method {
                SynthesizedMethod::Format(format) => {
                    let (code, requirements) =
                        layout.emit_format_method(&receiver_generics, format);
                    self.require_packages(&requirements);
                    code
                }
                SynthesizedMethod::Json => {
                    self.require_fmt();
                    self.require_errors();
                    if !layout.variants.is_empty() {
                        self.require_json();
                    }
                    layout.emit_json_methods(&receiver_generics)
                }
                SynthesizedMethod::ToString => self.to_string_method(name, &receiver_generics),
                SynthesizedMethod::Equals => {
                    let Some(code) = self.enum_equals_method(name, &enum_id, &receiver_generics)
                    else {
                        continue;
                    };
                    code
                }
                SynthesizedMethod::StringerShadow => {
                    unreachable!("only structs embed a stringer to shadow")
                }
            };
            result.push_str("\n\n");
            result.push_str(&code);
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

        Some(result)
    }

    fn enum_equals_method(
        &mut self,
        name: &str,
        enum_id: &str,
        receiver_generics: &str,
    ) -> Option<String> {
        let Some(Definition {
            body:
                DefinitionBody::Enum {
                    generics: sem_generics,
                    variants: sem_variants,
                    ..
                },
            ..
        }) = self.facts.definition(enum_id)
        else {
            return None;
        };
        let sem_generics = sem_generics.clone();
        let sem_variants = sem_variants.clone();
        let layout = self
            .facts
            .enum_layout(enum_id)
            .expect("enum layout should exist");

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
                    let field = |base: &str| {
                        let access = GoExpression::selector(
                            GoExpression::name(base.to_string()),
                            layout_field.go_name.clone(),
                        );
                        if layout_field.is_recursive() {
                            GoExpression::dereference(access)
                        } else {
                            access
                        }
                    };
                    let comparison = self.equality_expression(
                        field(&receiver),
                        field(&other),
                        &sem_field.ty,
                        &sem_generics,
                    );
                    self.render_expression(&comparison)
                })
                .collect();
            cases.push(format!(
                "case {}:\nreturn {}",
                layout_variant.tag_constant,
                comparisons.join(" && ")
            ));
        }

        if cases.is_empty() {
            return Some(format!(
                "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nreturn {receiver}.Tag == {other}.Tag\n}}"
            ));
        }
        let mut method = format!(
            "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nif {receiver}.Tag != {other}.Tag {{\nreturn false\n}}\nswitch {receiver}.Tag {{\n"
        );
        for case in &cases {
            method.push_str(case);
            method.push('\n');
        }
        method.push_str("default:\nreturn true\n}\n}");
        Some(method)
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
        let layout = self
            .facts
            .enum_layout(enum_id)
            .expect("enum layout should exist");
        let variant = layout
            .get_variant(variant_name)
            .expect("variant should exist in layout");

        let enum_name = layout.enum_name.clone();
        let generics = layout.generics.clone();
        let func_name = go_name::enum_make_function(&enum_name, &variant.name);
        let go_type_name = go_name::escape_type_name(&enum_name);
        let tag_constant = variant.tag_constant.clone();

        let (fields, params): (Vec<_>, Vec<_>) = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let argument = format!("arg{}", index);
                let param = format!("{} {}", argument, self.use_go_type(&field.ty));
                let value = if field.is_recursive() { "&" } else { "" };
                let field_assignment = format!("{}: {value}{}", field.go_name, argument);
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
                .map(|g| self.generic_go_name(&g.name))
                .collect::<Vec<_>>()
                .join(", ");
            let generics_string = self.generics_to_string(&generics);
            (generics_string, format!("[{}]", args))
        };

        let return_type = Type::Nominal {
            id: Symbol::from_raw(enum_name.clone()),
            writable: false,
            params: generics
                .iter()
                .map(|g| Type::Parameter(g.name.clone()))
                .collect(),
        };

        let return_type = self.use_go_type(&return_type);

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
