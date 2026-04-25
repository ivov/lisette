use crate::Emitter;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::names::generics::receiver_generics_string;
use crate::names::go_name;
use syntax::ast::{Attribute, Generic, StructFieldDefinition, StructKind};
use syntax::program::Definition;
use syntax::types::{SimpleKind, Type};

impl Emitter<'_> {
    pub(crate) fn emit_struct_definition(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        kind: &StructKind,
        struct_attrs: &[Attribute],
    ) -> String {
        let generics = self.merge_impl_bounds(name, generics);
        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let map_key_generics =
            Self::collect_map_key_generics(fields.iter().map(|f| &f.ty), &generic_names);
        let generics_string = self.generics_to_string_with_map_keys(&generics, &map_key_generics);

        if *kind == StructKind::Tuple {
            return self.emit_tuple_struct(name, &generics_string, fields, &generics);
        }

        let (field_strings, go_field_names): (Vec<String>, Vec<(String, String)>) = fields
            .iter()
            .map(|f| self.emit_struct_field(f, name, struct_attrs))
            .unzip();

        let receiver_generics = receiver_generics_string(&generics);
        let go_type_name = go_name::escape_keyword(name);

        let definition = if field_strings.is_empty() {
            format!("type {}{} struct{{}}", go_type_name, generics_string)
        } else {
            format!(
                "type {}{} struct {{\n{}\n}}",
                go_type_name,
                generics_string,
                field_strings.join("\n")
            )
        };

        if let Some(stringer_name) = self.stringer_method_name(name) {
            let string_method = self.emit_struct_stringer_method(
                name,
                &receiver_generics,
                &go_field_names,
                stringer_name,
            );
            if !go_field_names.is_empty() {
                self.ensure_imported.insert("fmt".to_string());
            }
            format!("{definition}\n\n{string_method}")
        } else {
            definition
        }
    }

    /// Emit a tuple struct along with its optional Stringer implementation.
    /// Zero-field structs and tuple structs that return a literal without
    /// `fmt.Sprintf` don't require the `fmt` import.
    fn emit_tuple_struct(
        &mut self,
        name: &str,
        generics_string: &str,
        fields: &[StructFieldDefinition],
        generics: &[Generic],
    ) -> String {
        let definition = self.emit_tuple_struct_definition(name, generics_string, fields);
        let Some(stringer_name) = self.stringer_method_name(name) else {
            return definition;
        };
        let receiver_generics = receiver_generics_string(generics);
        let is_type_alias = fields.len() == 1 && generics_string.is_empty();
        let underlying_go_type = is_type_alias.then(|| self.go_type_as_string(&fields[0].ty));
        let string_method = self.emit_tuple_struct_stringer_method(
            name,
            &receiver_generics,
            fields.len(),
            underlying_go_type.as_deref(),
            stringer_name,
        );
        if string_method.is_empty() {
            return definition;
        }
        if string_method.contains("fmt.") {
            self.ensure_imported.insert("fmt".to_string());
        }
        format!("{definition}\n\n{string_method}")
    }

    /// Emit one Go struct field, returning the field source code paired with
    /// the (source-name, Go-name) mapping used by the stringer and tag lookups.
    fn emit_struct_field(
        &mut self,
        f: &StructFieldDefinition,
        struct_name: &str,
        struct_attrs: &[Attribute],
    ) -> (String, (String, String)) {
        let tag_configs = interpret_field_attributes(f, struct_attrs);
        let needs_omitzero = is_option_type(&f.ty);
        let tag_string = format_tag_string(&f.name, &tag_configs, needs_omitzero);

        let has_tags = !tag_configs.is_empty();
        let needs_export = f.visibility.is_public() || has_tags;
        let field_name = if needs_export {
            go_name::make_exported(&f.name)
        } else {
            go_name::escape_keyword(&f.name).into_owned()
        };

        if has_tags && !f.visibility.is_public() {
            let key = format!("{}.{}.{}", self.current_module, struct_name, f.name);
            self.module.tag_exported_fields.insert(key);
        }

        let field_definition = if let Some(tags) = tag_string {
            format!("{} {} {}", field_name, self.go_type_as_string(&f.ty), tags)
        } else {
            format!("{} {}", field_name, self.go_type_as_string(&f.ty))
        };

        let field_with_doc = format!("{}{}", self.emit_doc(&f.doc), field_definition);

        (field_with_doc, (f.name.to_string(), field_name))
    }

    fn emit_tuple_struct_definition(
        &mut self,
        name: &str,
        generics_string: &str,
        fields: &[StructFieldDefinition],
    ) -> String {
        let go_type_name = go_name::escape_keyword(name);

        if fields.is_empty() {
            return format!("type {}{} struct{{}}", go_type_name, generics_string);
        }

        if fields.len() == 1 && generics_string.is_empty() {
            let underlying = self.go_type_as_string(&fields[0].ty);
            return format!("type {} {}", go_type_name, underlying);
        }

        let field_strings: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| format!("F{} {}", i, self.go_type_as_string(&f.ty)))
            .collect();

        format!(
            "type {}{} struct {{\n{}\n}}",
            go_type_name,
            generics_string,
            field_strings.join("\n")
        )
    }

    fn emit_struct_stringer_method(
        &self,
        name: &str,
        receiver_generics: &str,
        fields: &[(String, String)],
        method_name: &str,
    ) -> String {
        let receiver = crate::utils::receiver_name(name);
        let go_type_name = go_name::escape_keyword(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        if fields.is_empty() {
            return format!(
                "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn \"{name}\"\n}}"
            );
        }
        let format_parts: Vec<String> =
            fields.iter().map(|(src, _)| format!("{src}: %v")).collect();
        let args: Vec<String> = fields
            .iter()
            .map(|(_, go)| format!("{receiver}.{go}"))
            .collect();
        format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name} {{ {} }}\", {})\n}}",
            format_parts.join(", "),
            args.join(", ")
        )
    }

    fn emit_tuple_struct_stringer_method(
        &self,
        name: &str,
        receiver_generics: &str,
        field_count: usize,
        underlying_go_type: Option<&str>,
        method_name: &str,
    ) -> String {
        let receiver = crate::utils::receiver_name(name);
        let go_type_name = go_name::escape_keyword(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        if field_count == 0 {
            return format!(
                "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn \"{name}\"\n}}"
            );
        }
        if let Some(underlying) = underlying_go_type {
            if underlying.starts_with('*') {
                return String::new();
            }
            return format!(
                "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name}(%v)\", {underlying}({receiver}))\n}}"
            );
        }
        let placeholders: Vec<&str> = (0..field_count).map(|_| "%v").collect();
        let args: Vec<String> = (0..field_count)
            .map(|i| format!("{receiver}.F{i}"))
            .collect();
        format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {})\n}}",
            placeholders.join(", "),
            args.join(", ")
        )
    }

    /// Returns the auto-stringer method name to emit, or `None` to skip.
    /// A user method only suppresses auto-emission when it matches
    /// `fn NAME(self) -> string` *and* emits as a real Go receiver method.
    /// UFCS-emitted methods (specialized impls, extra type params, mixed
    /// impl blocks) become free functions in Go and do not satisfy Go
    /// interfaces, so they do not count.
    pub(super) fn stringer_method_name(&self, name: &str) -> Option<&'static str> {
        let qualified = format!("{}.{}", self.current_module, name);
        let methods = self
            .ctx
            .definitions
            .get(qualified.as_str())
            .and_then(|def| match def {
                Definition::Struct { methods, .. }
                | Definition::Enum { methods, .. }
                | Definition::ValueEnum { methods, .. }
                | Definition::TypeAlias { methods, .. } => Some(methods),
                _ => None,
            });

        let is_user_stringer = |method_name: &str| {
            methods.is_some_and(|m| is_stringer_signature(m.get(method_name)))
                && !self
                    .ctx
                    .ufcs_methods
                    .contains(&(qualified.clone(), method_name.to_string()))
        };

        let has_stringer = is_user_stringer("string") || is_user_stringer(ENUM_STRINGER_METHOD);

        let has_go_stringer =
            is_user_stringer("goString") || is_user_stringer(ENUM_GO_STRINGER_METHOD);
        match (has_stringer, has_go_stringer) {
            (true, true) => None,
            (true, false) => Some(ENUM_GO_STRINGER_METHOD),
            _ => Some(ENUM_STRINGER_METHOD),
        }
    }
}

fn is_stringer_signature(method_ty: Option<&Type>) -> bool {
    let Some(ty) = method_ty else {
        return false;
    };
    let func = match ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let Type::Function {
        params,
        return_type,
        ..
    } = func
    else {
        return false;
    };
    params.len() == 1 && matches!(return_type.as_ref(), Type::Simple(SimpleKind::String))
}

fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Nominal {
            id, underlying_ty, ..
        } => {
            if id == "Option" || id.ends_with(".Option") {
                return true;
            }
            underlying_ty.as_deref().is_some_and(is_option_type)
        }
        _ => false,
    }
}
