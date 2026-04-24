use crate::names::generics::extract_type_mapping;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::NativeCallContext;
use crate::Emitter;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use crate::utils::Staged;
use syntax::ast::{Annotation, Expression, StructKind};
use syntax::program::{CallKind, Definition};
use syntax::types::Type;

fn extract_return_type_param(function: &Expression) -> Option<Type> {
    let ty = function.get_type();
    let Type::Function { return_type, .. } = ty.unwrap_forall() else {
        return None;
    };
    let Type::Nominal { params, .. } = return_type.as_ref() else {
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
            && let Some(first) = call_result_ty
                .get_type_params()
                .and_then(|ps| ps.first().cloned())
        {
            return self.go_type_as_string(&first);
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
            && let Some(params) = call_result_ty.get_type_params()
            && params.len() >= 2
        {
            return (
                self.go_type_as_string(&params[0]),
                self.go_type_as_string(&params[1]),
            );
        }
        let ty = function.get_type();
        let Type::Function { return_type, .. } = ty.unwrap_forall() else {
            unreachable!("MapNew must be a function");
        };
        let params = return_type
            .get_type_params()
            .expect("MapNew must return a type with type arguments");
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
            call_kind,
            ..
        } = call_expression
        else {
            unreachable!("emit_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = (**spread).as_ref();

        let call_kind = call_kind.filter(|_| !self.is_local_binding(function));

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

    pub(super) fn infer_return_only_type_args(&mut self, function: &Expression) -> Option<String> {
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

        let instantiated_ty = function.get_type();
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

    pub(super) fn get_recursive_enum_pointer_indices(
        &mut self,
        function: &Expression,
    ) -> HashSet<usize> {
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
                && let Type::Nominal { id, .. } = return_type.as_ref()
            {
                return Some(id.to_string());
            }
            None
        }

        match function {
            Expression::Identifier { value, ty, .. } => {
                let enum_id = enum_id_from_type(ty)?;
                let variant = value.split('.').next_back().unwrap_or(value);
                let enum_name = enum_id.split('.').next_back().unwrap_or(&enum_id);
                let qualified = format!("{}.{}", enum_name, variant);
                if self.module.make_functions.contains_key(&qualified) {
                    return Some((enum_id, variant.to_string()));
                }
                if let Type::Function { params, .. } = ty.unwrap_forall() {
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

        let Type::Function { return_type, .. } = ty.unwrap_forall() else {
            return None;
        };
        let return_ty = return_type.as_ref().clone();

        let return_ty = call_ty.cloned().unwrap_or(return_ty);

        let Type::Nominal { id, .. } = &return_ty else {
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
    pub(super) fn get_callee_go_name(&self, function: &Expression) -> Option<&str> {
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

    pub(crate) fn prelude_container_type_args(&mut self, ty: &Type) -> Option<String> {
        if !ty.is_option() && !ty.is_result() && !ty.is_partial() {
            return None;
        }
        let Type::Nominal { params, .. } = ty else {
            return None;
        };
        if params.is_empty() {
            return None;
        }
        params
            .iter()
            .any(|p| self.as_interface(p).is_some() || self.is_go_function_alias(p))
            .then(|| self.format_type_args(params))
    }

    pub(super) fn is_prelude_variant_constructor(callee: &Expression) -> bool {
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
