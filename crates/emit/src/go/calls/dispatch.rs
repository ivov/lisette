use crate::go::names::generics::extract_type_mapping;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::NativeCallContext;
use crate::Emitter;
use crate::go::names::go_name;
use crate::go::types::native::NativeGoType;
use crate::go::utils::Staged;
use crate::go::write_line;
use syntax::ast::{Annotation, Expression, StructKind, UnaryOperator};
use syntax::program::{CallKind, Definition};
use syntax::types::Type;

struct CallArgContext<'a> {
    fn_param_types: &'a [Type],
    pointer_indices: &'a HashSet<usize>,
    is_go_call: bool,
}

fn extract_return_type_param(function: &Expression) -> Option<Type> {
    let Type::Function { return_type, .. } = function.get_type().resolve() else {
        return None;
    };
    let Type::Constructor { params, .. } = return_type.as_ref() else {
        return None;
    };
    params.first().cloned()
}

impl Emitter<'_> {
    fn resolve_element_type(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
    ) -> String {
        if !type_args.is_empty() {
            return self.annotation_to_go_type(&type_args[0]);
        }
        if let Some(call_result_ty) = call_ty
            && let Type::Constructor { params, .. } = call_result_ty.resolve()
            && let Some(first) = params.first()
        {
            return self.go_type_as_string(first);
        }
        let param = extract_return_type_param(function)
            .expect("constructor must have constructor return type");
        self.go_type_as_string(&param)
    }

    fn resolve_map_types(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
    ) -> (String, String) {
        if type_args.len() >= 2 {
            return (
                self.annotation_to_go_type(&type_args[0]),
                self.annotation_to_go_type(&type_args[1]),
            );
        }
        if let Some(call_result_ty) = call_ty
            && let Type::Constructor { params, .. } = call_result_ty.resolve()
            && params.len() >= 2
        {
            return (
                self.go_type_as_string(&params[0]),
                self.go_type_as_string(&params[1]),
            );
        }
        let return_type = function.get_type().resolve();
        let Type::Function { return_type, .. } = return_type else {
            unreachable!("MapNew must be a function");
        };
        let Type::Constructor { params, .. } = return_type.as_ref() else {
            unreachable!("MapNew must return a constructor type");
        };
        (
            self.go_type_as_string(&params[0]),
            self.go_type_as_string(&params[1]),
        )
    }

    fn try_emit_native_constructor(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        match (ctx.native_type, ctx.method) {
            (NativeGoType::Channel, "new") => {
                let elem = self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty);
                Some(format!("make(chan {})", elem))
            }
            (NativeGoType::Channel, "buffered") => {
                let elem = self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty);
                let capacity = ctx
                    .args
                    .first()
                    .map(|a| self.emit_operand(output, a))
                    .unwrap_or_else(|| "0".to_string());
                Some(format!("make(chan {}, {})", elem, capacity))
            }
            (NativeGoType::Map, "new") => {
                let (key, val) = self.resolve_map_types(ctx.function, ctx.type_args, ctx.call_ty);
                Some(format!("make(map[{}]{})", key, val))
            }
            (NativeGoType::Slice, "new") => {
                let elem = self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty);
                Some(format!("[]{}{{}}", elem))
            }
            _ => None,
        }
    }

    pub(crate) fn emit_call(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        call_ty: Option<&Type>,
    ) -> String {
        let Expression::Call {
            expression: callee,
            args,
            type_args,
            spread,
            span: call_span,
            ..
        } = call_expression
        else {
            unreachable!("emit_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = (**spread).as_ref();

        let call_kind = self
            .ctx
            .resolutions
            .get_call(*call_span)
            .filter(|_| !self.is_local_binding(function));

        match call_kind {
            Some(CallKind::TupleStructConstructor) => {
                if let Some(result) =
                    self.try_emit_tuple_struct_call(output, function, args, call_ty)
                {
                    return result;
                }
            }
            Some(CallKind::AssertType) => {
                return self.emit_assert_type(output, function, args, type_args);
            }
            Some(CallKind::UfcsMethod) => {
                return self.emit_ufcs_call(output, function, args, type_args, spread);
            }
            Some(
                CallKind::NativeConstructor(kind)
                | CallKind::NativeMethod(kind)
                | CallKind::NativeMethodIdentifier(kind),
            ) => {
                let native_type = NativeGoType::from_kind(kind);
                let method = self.extract_native_method_name(function);
                let ctx = NativeCallContext {
                    function,
                    args,
                    spread,
                    type_args,
                    call_ty,
                    native_type: &native_type,
                    method,
                };
                return self.emit_native_call(output, &ctx);
            }
            Some(CallKind::ReceiverMethodUfcs { is_public }) => {
                let method = self.extract_receiver_ufcs_method(function);
                return self.emit_receiver_method_ufcs(
                    output, args, type_args, &method, is_public, spread,
                );
            }
            _ => {}
        }

        self.emit_regular_call(output, function, args, type_args, call_ty, spread)
    }

    fn extract_native_method_name<'a>(&self, function: &'a Expression) -> &'a str {
        match function {
            Expression::DotAccess { member, .. } => member,
            Expression::Identifier { value, .. } => {
                value.split_once('.').map(|(_, m)| m).unwrap_or(value)
            }
            _ => "",
        }
    }

    fn extract_receiver_ufcs_method(&self, function: &Expression) -> String {
        if let Expression::Identifier { value, .. } = function
            && let Some(last_dot) = value.rfind('.')
        {
            return value[last_dot + 1..].to_string();
        }
        String::new()
    }

    fn emit_native_call(&mut self, output: &mut String, ctx: &NativeCallContext) -> String {
        if let Some(result) = self.try_emit_native_constructor(output, ctx) {
            return result;
        }
        if let Expression::DotAccess { .. } = ctx.function {
            self.emit_native_method_dot_access(output, ctx)
        } else {
            self.emit_native_method_identifier(
                output,
                ctx.args,
                ctx.spread,
                ctx.type_args,
                ctx.native_type,
                ctx.method,
            )
        }
    }

    fn emit_regular_call(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        spread: Option<&Expression>,
    ) -> String {
        if let Some(go_name) = self.get_callee_go_name(function).map(str::to_string) {
            let stages: Vec<Staged> = args.iter().map(|a| self.stage_operand(a)).collect();
            let mut args_strings = self.sequence(output, stages, "_arg");
            self.append_spread_arg(output, &mut args_strings, spread);
            return format!("{}({})", go_name, args_strings.join(", "));
        }

        let saved = self.emitting_call_callee;
        self.emitting_call_callee = true;
        let mut function_string = self.emit_operand(output, function);
        self.emitting_call_callee = saved;

        if matches!(
            function,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            }
        ) {
            function_string = format!("({})", function_string);
        }

        let type_args_string =
            self.resolve_call_type_args(function, type_args, call_ty, &mut function_string);

        let pointer_indices = self.get_recursive_enum_pointer_indices(function);

        let fn_param_types: Vec<Type> = match function.get_type().resolve() {
            Type::Function { params, .. } => params,
            _ => vec![],
        };

        let is_go_call = matches!(
            function.unwrap_parens(),
            Expression::DotAccess { expression, .. } if Self::is_go_receiver(expression)
        );

        let mut args_strings =
            self.emit_call_args(output, args, &fn_param_types, &pointer_indices, is_go_call);

        self.append_spread_arg(output, &mut args_strings, spread);

        let mut call_str = format!(
            "{}{}({})",
            function_string,
            type_args_string,
            args_strings.join(", ")
        );

        // Collapse fmt.Print{ln}(fmt.Sprintf(...)) → fmt.Printf(...{\\n})
        if (function_string == "fmt.Print" || function_string == "fmt.Println")
            && args_strings.len() == 1
            && args_strings[0].starts_with("fmt.Sprintf(")
        {
            let inner = &args_strings[0]["fmt.Sprintf(".len()..args_strings[0].len() - 1];
            let suffix = if function_string == "fmt.Println" {
                "\\n"
            } else {
                ""
            };
            if suffix.is_empty() {
                call_str = format!("fmt.Printf({})", inner);
            } else if let Some(close_quote) = inner.find("\", ") {
                let format_str = &inner[..close_quote];
                let rest = &inner[close_quote + 1..];
                call_str = format!("fmt.Printf({}{}\"{})", format_str, suffix, rest);
            } else if inner.starts_with('"') && inner.ends_with('"') {
                let format_str = &inner[..inner.len() - 1];
                call_str = format!("fmt.Printf({}{}\")", format_str, suffix);
            }
        }

        // Collapse fmt.Print{ln}(fmt.Sprint(x)) → fmt.Print{ln}(x)
        if (function_string == "fmt.Print" || function_string == "fmt.Println")
            && args_strings.len() == 1
            && args_strings[0].starts_with("fmt.Sprint(")
            && args_strings[0].ends_with(')')
        {
            let inner = &args_strings[0]["fmt.Sprint(".len()..args_strings[0].len() - 1];
            call_str = format!("{}({})", function_string, inner);
        }

        if !self.skip_array_return_wrap
            && let Expression::DotAccess {
                expression: receiver_expression,
                member,
                ..
            } = function.unwrap_parens()
            && Self::is_go_receiver(receiver_expression)
            && self.has_go_array_return(receiver_expression, member)
        {
            let temp = self.fresh_var(Some("arr"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, call_str);
            return format!("{}[:]", temp);
        }

        call_str
    }

    fn resolve_call_type_args(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        function_string: &mut String,
    ) -> String {
        let mut type_args_string = self.format_type_args_from_annotations(type_args);

        if type_args_string.is_empty()
            && let Some(inferred) = self.infer_return_only_type_args(function)
        {
            type_args_string = inferred;
        }

        if type_args_string.is_empty()
            && let Some(call_result_ty) = call_ty
            && Self::is_prelude_variant_constructor(function)
            && let Type::Constructor { id, params, .. } = call_result_ty.resolve()
            && (id == "prelude.Option" || id == "prelude.Result")
            && !params.is_empty()
            && params
                .iter()
                .any(|p| self.as_interface(p).is_some() || self.is_go_function_alias(p))
        {
            type_args_string = self.format_type_args(&params);
        }

        if !type_args_string.is_empty()
            && let Some(bracket_start) = function_string.find('[')
        {
            function_string.truncate(bracket_start);
        }

        type_args_string
    }

    pub(crate) fn append_spread_arg(
        &mut self,
        output: &mut String,
        args_strings: &mut Vec<String>,
        spread: Option<&Expression>,
    ) {
        if let Some(spread_expr) = spread {
            args_strings.push(format!("{}...", self.emit_operand(output, spread_expr)));
        }
    }

    fn emit_call_args(
        &mut self,
        output: &mut String,
        args: &[Expression],
        fn_param_types: &[Type],
        pointer_indices: &HashSet<usize>,
        is_go_call: bool,
    ) -> Vec<String> {
        let call_arg_ctx = CallArgContext {
            fn_param_types,
            pointer_indices,
            is_go_call,
        };
        let stages: Vec<Staged> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let mut setup = String::new();
                let value = self.emit_call_arg(&mut setup, arg, i, &call_arg_ctx);
                Staged::new(setup, value)
            })
            .collect();
        self.sequence(output, stages, "_arg")
    }

    /// Classify and emit a single call argument.
    fn emit_call_arg(
        &mut self,
        output: &mut String,
        arg: &Expression,
        index: usize,
        ctx: &CallArgContext,
    ) -> String {
        let effective_param_ty = self.effective_param_type(index, ctx.fn_param_types);

        if ctx.is_go_call
            && let Some(result) = self.try_emit_callback_wrapper(output, arg, effective_param_ty)
        {
            return result;
        }

        if let Some(result) = self.try_emit_nullable_coercion(output, arg, effective_param_ty) {
            return result;
        }

        if ctx.pointer_indices.contains(&index) {
            let value = self.emit_value(output, arg);
            if matches!(arg, Expression::Reference { .. }) || arg.get_type().resolve().is_ref() {
                return value;
            }
            let temp = self.fresh_var(Some("ptr"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, value);
            return format!("&{}", temp);
        }

        let value = self.emit_composite_value(output, arg);
        match effective_param_ty {
            Some(target) => self.maybe_wrap_as_go_interface(value, &arg.get_type(), target),
            None => value,
        }
    }

    fn effective_param_type<'a>(
        &self,
        index: usize,
        fn_param_types: &'a [Type],
    ) -> Option<&'a Type> {
        fn_param_types.get(index).or_else(|| {
            fn_param_types
                .last()
                .filter(|t| t.get_name() == Some("VarArgs"))
        })
    }

    fn try_emit_callback_wrapper(
        &mut self,
        output: &mut String,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<String> {
        let param_fn_ty = effective_param_ty.and_then(|param_ty| {
            let resolved = param_ty.resolve();
            let fn_ty = match resolved {
                Type::Function { .. } => resolved,
                Type::Constructor {
                    underlying_ty: Some(ref inner),
                    ..
                } => inner.resolve(),
                _ => return None,
            };
            if let Type::Function {
                ref return_type, ..
            } = fn_ty
            {
                let ret = return_type.resolve();
                if ret.is_result() || ret.is_option() || ret.tuple_arity().is_some_and(|a| a >= 2) {
                    return Some(fn_ty);
                }
            }
            None
        })?;

        let value = self.emit_value(output, arg);
        Some(self.emit_lisette_callback_wrapper(output, &value, &param_fn_ty))
    }

    fn try_emit_nullable_coercion(
        &mut self,
        output: &mut String,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<String> {
        let param_ty = effective_param_ty?;
        let arg_ty = arg.get_type().resolve();
        if !self.is_nullable_option(&arg_ty) {
            return None;
        }
        let check_ty = if param_ty.get_name() == Some("VarArgs") {
            param_ty.inner().unwrap_or_else(|| param_ty.resolve())
        } else {
            param_ty.resolve()
        };
        let needs_coercion = self
            .as_interface(&check_ty)
            .is_some_and(|id| go_name::is_go_import(&id))
            || (check_ty.has_name("Unknown") && {
                let inner = arg_ty.ok_type();
                self.as_interface(&inner)
                    .is_some_and(|id| go_name::is_go_import(&id))
            });

        if !needs_coercion {
            return None;
        }

        if matches!(arg, Expression::Identifier { value, .. } if value == "None") {
            return Some("nil".to_string());
        }
        let value = self.emit_value(output, arg);
        Some(self.maybe_unwrap_go_nullable(output, &value, &arg.get_type().resolve()))
    }

    fn infer_return_only_type_args(&mut self, function: &Expression) -> Option<String> {
        let definition_ty = self.get_callee_definition_type(function)?;
        let Type::Forall { vars, body } = definition_ty else {
            return None;
        };
        let Type::Function {
            params: generic_params,
            ..
        } = body.as_ref()
        else {
            return None;
        };

        let all_inferable = vars.iter().all(|var| {
            let param_ty = Type::Parameter(var.clone());
            generic_params.iter().any(|pt| pt.contains_type(&param_ty))
        });
        if all_inferable {
            return None;
        }

        let instantiated_ty = function.get_type().resolve();
        let mut mapping: HashMap<String, Type> = HashMap::default();
        extract_type_mapping(&body, &instantiated_ty, &mut mapping);

        let resolved: Vec<Type> = vars
            .iter()
            .filter_map(|v| mapping.get(v.as_str()).cloned())
            .collect();

        if resolved.len() != vars.len() {
            return None;
        }

        Some(self.format_type_args(&resolved))
    }

    fn lookup_definition_type(&self, primary: &str, fallback: Option<&str>) -> Option<Type> {
        self.ctx
            .definitions
            .get(primary)
            .or_else(|| fallback.and_then(|f| self.ctx.definitions.get(f)))
            .map(|d| d.ty().clone())
    }

    fn get_callee_definition_type(&self, function: &Expression) -> Option<Type> {
        let function = function.unwrap_parens();
        match function {
            Expression::Identifier { value, .. } => {
                let qualified = format!("{}.{}", self.current_module, value);
                self.lookup_definition_type(&qualified, Some(value.as_str()))
            }
            Expression::DotAccess {
                expression, member, ..
            } => {
                if let Expression::Identifier { value, .. } = expression.as_ref() {
                    let module_name = self.resolve_alias_to_module(value);
                    let qualified = format!("{}.{}", module_name, member);
                    // Try as Type.method in current module (e.g. Box.make → main.Box.make)
                    let local = format!("{}.{}.{}", self.current_module, value, member);
                    return self.lookup_definition_type(&qualified, Some(&local));
                }
                if let Expression::DotAccess {
                    expression: inner_expression,
                    member: type_name,
                    ..
                } = expression.as_ref()
                    && let Expression::Identifier {
                        value: module_name, ..
                    } = inner_expression.as_ref()
                {
                    let module_name = self.resolve_alias_to_module(module_name);
                    let qualified = format!("{}.{}.{}", module_name, type_name, member);
                    return self.lookup_definition_type(&qualified, None);
                }
                None
            }
            _ => None,
        }
    }

    fn get_recursive_enum_pointer_indices(&mut self, function: &Expression) -> HashSet<usize> {
        let Some((enum_id, variant_name)) = self.get_make_function_info(function) else {
            return HashSet::default();
        };

        let Some(layout) = self.module.enum_layouts.get(&enum_id) else {
            return HashSet::default();
        };

        let Some(variant) = layout.get_variant(&variant_name) else {
            return HashSet::default();
        };

        variant
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.go_type.starts_with('*'))
            .map(|(i, _)| i)
            .collect()
    }

    fn get_make_function_info(&mut self, function: &Expression) -> Option<(String, String)> {
        fn enum_id_from_type(ty: &Type) -> Option<String> {
            if let Type::Function { return_type, .. } = ty.unwrap_forall()
                && let Type::Constructor { id, .. } = return_type.as_ref()
            {
                return Some(id.to_string());
            }
            None
        }

        match function {
            Expression::Identifier { value, ty, .. } => {
                let resolved_ty = ty.resolve();
                let enum_id = enum_id_from_type(&resolved_ty)?;
                let variant = value.split('.').next_back().unwrap_or(value);
                let enum_name = enum_id.split('.').next_back().unwrap_or(&enum_id);
                let qualified = format!("{}.{}", enum_name, variant);
                if self.module.make_functions.contains_key(&qualified) {
                    return Some((enum_id, variant.to_string()));
                }
                if let Type::Function { params, .. } = &resolved_ty {
                    for key in self.module.make_functions.keys() {
                        if let Some((e_name, v_name)) = key.split_once('.')
                            && e_name == enum_name
                            && let Some(layout) = self.module.enum_layouts.get(&enum_id)
                            && let Some(v) = layout.get_variant(v_name)
                            && v.fields.len() == params.len()
                        {
                            return Some((enum_id, v_name.to_string()));
                        }
                    }
                }
                None
            }
            Expression::DotAccess {
                expression,
                member,
                ty,
                ..
            } => {
                if let Expression::Identifier {
                    value: enum_name, ..
                } = expression.as_ref()
                {
                    let qualified = format!("{}.{}", enum_name, member);
                    if self.module.make_functions.contains_key(&qualified) {
                        let enum_id = enum_id_from_type(ty)?;
                        return Some((enum_id, member.to_string()));
                    }
                }
                if let Expression::DotAccess {
                    member: type_name, ..
                } = expression.as_ref()
                {
                    let qualified = format!("{}.{}", type_name, member);
                    if self.module.make_functions.contains_key(&qualified) {
                        let enum_id = enum_id_from_type(ty)?;
                        return Some((enum_id, member.to_string()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Attempts to emit a tuple struct constructor as a struct literal.
    ///
    /// Returns `None` if this isn't a tuple struct or should fall through
    /// to regular call handling.
    fn try_emit_tuple_struct_call(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        call_ty: Option<&Type>,
    ) -> Option<String> {
        let ty = function.get_type();

        let resolved = ty.resolve();
        let Type::Function { return_type, .. } = resolved.unwrap_forall() else {
            return None;
        };
        let return_ty = return_type.as_ref().clone();

        let return_ty = call_ty.map(|t| t.resolve().clone()).unwrap_or(return_ty);

        let Type::Constructor { id, .. } = return_ty.resolve() else {
            return None;
        };

        let Some(Definition::Struct {
            kind,
            fields,
            generics,
            ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
            return None;
        };

        if *kind != StructKind::Tuple {
            return None;
        }

        if fields.len() == 1 && generics.is_empty() {
            return None;
        }

        let go_ty = self.go_type_as_string(&return_ty);
        let stages: Vec<Staged> = args.iter().map(|a| self.stage_composite(a)).collect();
        let values = self.sequence(output, stages, "_arg");

        let field_pairs: Vec<(String, String)> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (format!("F{}", i), value))
            .collect();

        Some(self.emit_struct_literal(&go_ty, &field_pairs))
    }

    fn emit_assert_type(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        type_args: &[Annotation],
    ) -> String {
        let target_ty = if !type_args.is_empty() {
            self.annotation_to_go_type(&type_args[0])
        } else {
            let param = extract_return_type_param(function)
                .expect("AssertType must have constructor return type");
            self.go_type_as_string(&param)
        };
        let arg_expression = args
            .first()
            .map(|a| self.emit_composite_value(output, a))
            .unwrap_or_default();
        self.flags.needs_stdlib = true;
        format!(
            "{}.AssertType[{}]({})",
            go_name::GO_STDLIB_PKG,
            target_ty,
            arg_expression
        )
    }

    /// Look up the `#[go("name")]` override for a callee, if any.
    fn get_callee_go_name(&self, function: &Expression) -> Option<&str> {
        let Expression::Identifier { value, .. } = function else {
            return None;
        };
        if self.is_local_binding(function) {
            return None;
        }
        let qualified = format!("{}.{}", self.current_module, value);
        let prelude_qualified = format!("prelude.{}", value);
        self.ctx
            .definitions
            .get(qualified.as_str())
            .or_else(|| self.ctx.definitions.get(prelude_qualified.as_str()))
            .and_then(|d| d.go_name())
    }

    fn is_local_binding(&self, function: &Expression) -> bool {
        if let Expression::Identifier { value, .. } = function {
            self.scope.bindings.get(value).is_some()
        } else {
            false
        }
    }

    fn is_prelude_variant_constructor(callee: &Expression) -> bool {
        match callee {
            Expression::Identifier { value, .. } => {
                matches!(value.as_str(), "Some" | "Ok" | "Err")
            }
            Expression::DotAccess { member, .. } => {
                matches!(member.as_str(), "Some" | "Ok" | "Err")
            }
            _ => false,
        }
    }
}
