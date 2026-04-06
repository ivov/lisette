use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::go::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::go::names::go_name;
use crate::go::types::native::NativeGoType;
use crate::go::utils::{group_params, optimize_function_body, receiver_name};
use syntax::ast::{
    Annotation, Attribute, Binding, EnumVariant, Expression, FunctionDefinition, Generic,
    ParentInterface, Pattern, Span, StructFieldDefinition, StructKind, TypedPattern, UnaryOperator,
    Visibility,
};
use syntax::types::Type;

impl Emitter<'_> {
    fn change_go_builtin_methods(
        &mut self,
        function_definition: &FunctionDefinition,
        receiver: Option<(String, Type)>,
    ) -> (FunctionDefinition, Option<(String, Type)>) {
        let Some((receiver_name, receiver_type)) = receiver else {
            return (function_definition.clone(), None);
        };

        let Some(native) = NativeGoType::from_type(&receiver_type) else {
            return (
                function_definition.clone(),
                Some((receiver_name, receiver_type)),
            );
        };

        let mut new_function_definition = function_definition.clone();
        new_function_definition.name =
            format!("{}.{}", native.lisette_name(), function_definition.name).into();

        let self_binding = Binding {
            pattern: Pattern::Identifier {
                identifier: receiver_name.into(),
                span: Span::dummy(),
            },
            annotation: Some(Annotation::Unknown),
            typed_pattern: None,
            ty: receiver_type,
            mutable: false,
        };

        new_function_definition.params.insert(0, self_binding);
        (new_function_definition, None)
    }

    fn emit_receiver_part(
        &mut self,
        params_to_process: &[Binding],
        receiver: &Option<(String, Type)>,
        receiver_override: Option<&Type>,
    ) -> (Option<String>, Option<String>) {
        let Some((_, receiver_ty)) = receiver else {
            return (None, None);
        };

        let param_names: Vec<String> = params_to_process
            .iter()
            .filter_map(|param| {
                if let Pattern::Identifier { identifier, .. } = &param.pattern {
                    Some(identifier.to_string())
                } else {
                    None
                }
            })
            .collect();

        let actual_ty = receiver_override.unwrap_or(receiver_ty);
        let ty_string = self.go_type_as_string(actual_ty);
        let mut receiver_var = receiver_name(&ty_string);

        if param_names.contains(&receiver_var) {
            receiver_var = format!("{}{}", receiver_var, receiver_var);
            let mut counter = 2;
            while param_names.contains(&receiver_var) {
                receiver_var = format!("{}{}", receiver_name(&ty_string), counter);
                counter += 1;
            }
        }

        let receiver_part = format!("({} {})", receiver_var, ty_string);

        self.scope.bindings.add("self", receiver_var.clone());
        self.declare(&receiver_var);

        (Some(receiver_var), Some(receiver_part))
    }

    /// Detect Ref<T> parameters where T is a bounded generic and populate
    /// absorbed_ref_generics. Returns the previous value for restoration.
    fn detect_absorbed_ref_generics(
        &mut self,
        params: &[Binding],
        generics: &[Generic],
    ) -> HashSet<String> {
        let saved = self.module.absorbed_ref_generics.clone();
        self.module.absorbed_ref_generics.clear();
        let bounded_generics: HashSet<&str> = generics
            .iter()
            .filter(|g| !g.bounds.is_empty())
            .map(|g| g.name.as_ref())
            .collect();
        for param in params.iter() {
            let resolved = param.ty.resolve();
            if resolved.is_ref()
                && let Some(inner) = resolved.inner()
                && let Type::Parameter(name) = inner.resolve()
                && bounded_generics.contains(name.as_ref())
            {
                self.module.absorbed_ref_generics.insert(name.to_string());
            }
        }
        saved
    }

    fn emit_function_params(
        &mut self,
        params_to_process: &[Binding],
    ) -> (String, Vec<(String, Pattern, Option<TypedPattern>)>) {
        let mut deferred_patterns = Vec::new();
        let mut params = Vec::new();
        for param in params_to_process {
            let name = match &param.pattern {
                Pattern::Identifier { identifier, .. } => {
                    if let Some(go_name) = self.go_name_for_binding(&param.pattern) {
                        let go_id = self.scope.bindings.add(identifier, go_name);
                        self.declare(&go_id);
                        go_id
                    } else {
                        "_".to_string()
                    }
                }
                Pattern::WildCard { .. } => "_".to_string(),
                _ => {
                    let var = self.fresh_var(Some("arg"));
                    self.declare(&var);
                    deferred_patterns.push((
                        var.clone(),
                        param.pattern.clone(),
                        param.typed_pattern.clone(),
                    ));
                    var
                }
            };

            let param_type = {
                let resolved = param.ty.resolve();
                if resolved.is_ref()
                    && let Some(inner) = resolved.inner()
                    && let Type::Parameter(name) = inner.resolve()
                    && self.module.absorbed_ref_generics.contains(name.as_ref())
                {
                    inner
                } else {
                    param.ty.clone()
                }
            };
            params.push((name, self.go_type_as_string(&param_type)));
        }
        (format!("({})", group_params(&params)), deferred_patterns)
    }

    pub(crate) fn emit_function(
        &mut self,
        function_definition: &FunctionDefinition,
        receiver: Option<(String, Type)>,
        is_public: bool,
    ) -> String {
        if matches!(*function_definition.body, Expression::NoOp) {
            return String::new();
        }

        let directive = self.maybe_line_directive(&function_definition.name_span);

        let saved_return_context = self.current_return_context.clone();
        self.current_return_context = Some(function_definition.return_type.clone());

        let (function_definition, receiver) =
            self.change_go_builtin_methods(function_definition, receiver);

        let (params_to_process, receiver_override) =
            self.extract_receiver(&function_definition, receiver.is_some());

        let mut parts = vec!["func".to_string()];

        let (_, receiver_part) =
            self.emit_receiver_part(params_to_process, &receiver, receiver_override.as_ref());
        if let Some(part) = receiver_part {
            parts.push(part);
        }

        let function_name = if is_public {
            go_name::capitalize_first(&function_definition.name)
        } else if receiver.is_some() {
            go_name::escape_keyword(&function_definition.name).into_owned()
        } else {
            go_name::escape_reserved(&function_definition.name).into_owned()
        };
        parts.push(function_name);

        let generic_names: Vec<&str> = function_definition
            .generics
            .iter()
            .map(|g| g.name.as_ref())
            .collect();
        let sig_types = params_to_process
            .iter()
            .map(|p| &p.ty)
            .chain(std::iter::once(&function_definition.return_type));
        let mut map_key_generics = Self::collect_map_key_generics(sig_types, &generic_names);
        for name in &generic_names {
            if !map_key_generics.contains(*name)
                && Self::body_has_map_key_generic(&function_definition.body, name)
            {
                map_key_generics.insert(name.to_string());
            }
        }

        let generics_str =
            self.generics_to_string_with_map_keys(&function_definition.generics, &map_key_generics);
        if !generics_str.is_empty() {
            parts.push(generics_str);
        }

        let saved_absorbed =
            self.detect_absorbed_ref_generics(params_to_process, &function_definition.generics);

        let (params_string, deferred_patterns) = self.emit_function_params(params_to_process);
        parts.push(params_string);

        let return_ty = if function_definition.return_type.is_unit() {
            String::new()
        } else {
            self.go_type_as_string(&function_definition.return_type)
        };

        if !return_ty.is_empty() {
            parts.push(return_ty);
        }

        let signature = parts.join(" ");

        let mut body = String::new();

        // Emit bindings for complex patterns using the unified pattern visitor
        for (var_name, pattern, typed) in deferred_patterns {
            self.emit_pattern_bindings(&mut body, &var_name, &pattern, typed.as_ref());
        }

        self.emit_function_body(
            &mut body,
            &function_definition.body,
            !function_definition.return_type.is_unit(),
        );
        optimize_function_body(&mut body);

        self.current_return_context = saved_return_context;
        self.module.absorbed_ref_generics = saved_absorbed;

        let trimmed_body = body.trim_end();
        if trimmed_body.is_empty() {
            format!("{}{} {{}}", directive, signature)
        } else {
            format!("{}{} {{\n{}\n}}", directive, signature, trimmed_body)
        }
    }

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
            let definition = self.emit_tuple_struct_definition(name, &generics_string, fields);
            let receiver_generics = receiver_generics_string(&generics);
            let is_type_alias = fields.len() == 1 && generics_string.is_empty();
            let underlying_go_type = if is_type_alias {
                Some(self.go_type_as_string(&fields[0].ty))
            } else {
                None
            };
            let string_method = self.emit_tuple_struct_string_method(
                name,
                &receiver_generics,
                fields.len(),
                underlying_go_type.as_deref(),
            );
            // Zero-field structs return a literal without fmt.Sprintf, so also skip fmt.
            if !string_method.is_empty() {
                if string_method.contains("fmt.") {
                    self.ensure_imported.insert("fmt".to_string());
                }
                return format!("{definition}\n\n{string_method}");
            }
            return definition;
        }

        let mut go_field_names: Vec<(String, String)> = Vec::new();

        let field_strings: Vec<String> = fields
            .iter()
            .map(|f| {
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

                go_field_names.push((f.name.to_string(), field_name.clone()));

                if has_tags && !f.visibility.is_public() {
                    let key = format!("{}.{}.{}", self.current_module, name, f.name);
                    self.module.tag_exported_fields.insert(key);
                }

                let field_definition = if let Some(tags) = tag_string {
                    format!("{} {} {}", field_name, self.go_type_as_string(&f.ty), tags)
                } else {
                    format!("{} {}", field_name, self.go_type_as_string(&f.ty))
                };

                if let Some(doc) = &f.doc {
                    let doc_lines: Vec<String> = doc
                        .lines()
                        .map(|line| {
                            if line.is_empty() {
                                "//".to_string()
                            } else {
                                format!("// {}", line)
                            }
                        })
                        .collect();
                    format!("{}\n{}", doc_lines.join("\n"), field_definition)
                } else {
                    field_definition
                }
            })
            .collect();

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

        let string_method =
            self.emit_struct_string_method(name, &receiver_generics, &go_field_names);
        if !go_field_names.is_empty() {
            self.ensure_imported.insert("fmt".to_string());
        }

        format!("{definition}\n\n{string_method}")
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

    fn emit_struct_string_method(
        &self,
        name: &str,
        receiver_generics: &str,
        fields: &[(String, String)],
    ) -> String {
        let receiver = crate::go::utils::receiver_name(name);
        let go_type_name = go_name::escape_keyword(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        if fields.is_empty() {
            return format!(
                "func ({receiver} {receiver_type}) String() string {{\nreturn \"{name}\"\n}}"
            );
        }
        let format_parts: Vec<String> =
            fields.iter().map(|(src, _)| format!("{src}: %v")).collect();
        let args: Vec<String> = fields
            .iter()
            .map(|(_, go)| format!("{receiver}.{go}"))
            .collect();
        format!(
            "func ({receiver} {receiver_type}) String() string {{\nreturn fmt.Sprintf(\"{name} {{ {} }}\", {})\n}}",
            format_parts.join(", "),
            args.join(", ")
        )
    }

    fn emit_tuple_struct_string_method(
        &self,
        name: &str,
        receiver_generics: &str,
        field_count: usize,
        underlying_go_type: Option<&str>,
    ) -> String {
        let receiver = crate::go::utils::receiver_name(name);
        let go_type_name = go_name::escape_keyword(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        if field_count == 0 {
            return format!(
                "func ({receiver} {receiver_type}) String() string {{\nreturn \"{name}\"\n}}"
            );
        }
        if let Some(underlying) = underlying_go_type {
            if underlying.starts_with('*') {
                return String::new();
            }
            return format!(
                "func ({receiver} {receiver_type}) String() string {{\nreturn fmt.Sprintf(\"{name}(%v)\", {underlying}({receiver}))\n}}"
            );
        }
        let placeholders: Vec<&str> = (0..field_count).map(|_| "%v").collect();
        let args: Vec<String> = (0..field_count)
            .map(|i| format!("{receiver}.F{i}"))
            .collect();
        format!(
            "func ({receiver} {receiver_type}) String() string {{\nreturn fmt.Sprintf(\"{name}({})\", {})\n}}",
            placeholders.join(", "),
            args.join(", ")
        )
    }

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
        let generics = self.merge_impl_bounds(name, generics);
        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let map_key_generics = self.enum_map_key_generics(&enum_id, &generic_names);
        let generics_string = self.generics_to_string_with_map_keys(&generics, &map_key_generics);
        let receiver_generics = receiver_generics_string(&generics);
        let has_json = attributes.iter().any(|a| a.name == "json");

        let layout = self.module.enum_layouts.get(&enum_id).unwrap();
        let mut result = layout.emit_definition(&generics_string);
        result.push_str("\n\n");
        result.push_str(&layout.emit_string_method(&receiver_generics));
        if has_json {
            result.push_str("\n\n");
            result.push_str(&layout.emit_json_methods(&receiver_generics));
        }
        self.ensure_imported.insert("fmt".to_string());
        if has_json {
            self.ensure_imported.insert("encoding/json".to_string());
        }

        Some(result)
    }

    pub(crate) fn emit_type_alias(
        &mut self,
        name: &str,
        generics: &[Generic],
        ty: &Type,
    ) -> String {
        let underlying = match ty {
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Constructor {
                    underlying_ty: Some(inner),
                    ..
                } if matches!(inner.as_ref(), Type::Function { .. }) => inner.as_ref(),
                other => other,
            },
            Type::Constructor {
                underlying_ty: Some(inner),
                ..
            } if matches!(inner.as_ref(), Type::Function { .. }) => inner.as_ref(),
            _ => ty,
        };
        let ty_string = self.go_type_as_string(underlying);

        if let Type::Constructor { id, .. } = underlying
            && let Some((module, _)) = id.split_once('.')
            && module != self.current_module
            && module != go_name::PRELUDE_MODULE
            && !go_name::is_go_import(module)
        {
            self.require_module_import(module);
        }

        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let map_key_generics =
            Self::collect_map_key_generics(std::iter::once(underlying), &generic_names);
        let generics_string = self.generics_to_string_with_map_keys(generics, &map_key_generics);

        format!(
            "type {}{} = {}",
            go_name::escape_keyword(name),
            generics_string,
            ty_string
        )
    }

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

        let generics_str = self.generics_to_string_with_map_keys(generics, &map_key_generics);

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
                output.push(format!("{}({})", method_name, args.join(", ")));
            } else {
                output.push(format!(
                    "{}({}) {}",
                    method_name,
                    args.join(", "),
                    return_type
                ));
            }
        }

        output.push("}".to_string());

        output.join("\n")
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
    ) -> String {
        let go_identifier = self.scope.bindings.add(identifier, identifier);
        let ty_str = self.go_type_as_string(ty);

        let mut output = String::new();
        let expression_string = self.emit_operand(&mut output, expression);
        let value = if expression_string.is_empty() {
            "struct{}{}"
        } else {
            &expression_string
        };
        let keyword = if Self::is_go_const_eligible(expression) {
            "const"
        } else {
            "var"
        };
        format!("{} {} {} = {}", keyword, go_identifier, ty_str, value)
    }

    fn is_go_const_eligible(expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Literal { .. } => true,
            Expression::Binary { left, right, .. } => {
                Self::is_go_const_eligible(left) && Self::is_go_const_eligible(right)
            }
            Expression::Unary {
                operator: UnaryOperator::Negative | UnaryOperator::Not,
                expression,
                ..
            } => Self::is_go_const_eligible(expression),
            _ => false,
        }
    }

    fn extract_receiver<'a>(
        &mut self,
        function_definition: &'a FunctionDefinition,
        has_receiver: bool,
    ) -> (&'a [Binding], Option<Type>) {
        let default = (&function_definition.params[..], None);

        if !has_receiver || function_definition.params.is_empty() {
            return default;
        }

        let Pattern::Identifier { identifier, .. } = &function_definition.params[0].pattern else {
            return default;
        };

        if identifier != "self" {
            return default;
        }

        let receiver_ty = &function_definition.params[0].ty;
        let _ty_str = self.go_type_as_string(receiver_ty);

        (&function_definition.params[1..], Some(receiver_ty.clone()))
    }

    pub(crate) fn emit_impl_block(
        &mut self,
        receiver_name: &str,
        ty: &Type,
        methods: &[Expression],
        generics: &[Generic],
    ) -> String {
        let qualified_type = format!("{}.{}", self.current_module, receiver_name);

        methods
            .iter()
            .filter_map(|method| {
                let (function, is_public, method_doc) = match method {
                    Expression::Function {
                        doc,
                        visibility,
                        name_span,
                        ..
                    } => {
                        if self.ctx.unused.is_unused_definition(name_span) {
                            return None;
                        }
                        (
                            method.to_function_definition(),
                            matches!(visibility, Visibility::Public),
                            doc.clone(),
                        )
                    }
                    _ => return None,
                };

                let has_self = function.params.first().is_some_and(|p| {
                    matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
                });

                let is_ufcs = self.ctx.ufcs_methods.contains(&(
                    qualified_type.clone(),
                    function.name.to_string(),
                ));

                let should_export =
                    is_public || self.method_needs_export(&function.name);

                let is_free_function = if !has_self {
                    true // static method
                } else {
                    is_ufcs
                };

                let code = if is_free_function {
                    let mut free_function = function.clone();
                    let method_name = if should_export {
                        go_name::capitalize_first(&function.name)
                    } else {
                        function.name.to_string()
                    };
                    free_function.name = format!("{}_{}", receiver_name, method_name).into();
                    let mut combined_generics = generics.to_vec();
                    combined_generics.extend(free_function.generics.iter().cloned());
                    free_function.generics = combined_generics;
                    self.emit_function(&free_function, None, should_export)
                } else {
                    self.emit_function(
                        &function,
                        Some((receiver_name.to_string(), ty.clone())),
                        should_export,
                    )
                };

                if code.is_empty() {
                    None
                } else {
                    let method_doc_comment = self.emit_doc(&method_doc);
                    Some(format!("{}{}", method_doc_comment, code))
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Computes the Go receiver generics string for a generic type (e.g., `[T, U]`).
/// Unlike the full generics string which includes constraints (`[T any, U any]`),
/// the receiver only names the type parameters.
fn receiver_generics_string(generics: &[Generic]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        let params: Vec<&str> = generics.iter().map(|g| g.name.as_str()).collect();
        format!("[{}]", params.join(", "))
    }
}

impl Emitter<'_> {
    pub(crate) fn register_prelude_make_functions(&mut self) {
        for prelude_type in crate::go::PreludeType::enum_types() {
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

        let return_type = Type::Constructor {
            id: enum_name.clone().into(),
            params: generics
                .iter()
                .map(|g| Type::Constructor {
                    id: g.name.clone(),
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

fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Constructor { id, .. } => id == "Option" || id.ends_with(".Option"),
        _ => false,
    }
}
