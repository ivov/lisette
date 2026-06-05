use crate::EmitEffects;
use crate::expressions::access::struct_call::emit_struct_literal;
use crate::names::generics::extract_type_mapping;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::NativeCallContext;
use crate::Planner;
use crate::Renderer;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::CalleePlan;
use crate::types::native::NativeGoType;
use syntax::ast::{Annotation, Expression, StructKind};
use syntax::program::{CallKind, Definition, DefinitionBody};
use syntax::types::{
    SimpleKind, SubstitutionMap, Symbol, Type, build_substitution_map, substitute, unqualified_name,
};

struct TupleStructTarget {
    go_ty: String,
    field_tys: Vec<Type>,
}

impl Planner<'_> {
    /// True when Go's inference would lose this alias: function aliases (infer
    /// as `func(...)`) and non-default numeric aliases (untyped literals default
    /// to `int`/`float64`/`complex128`).
    pub(crate) fn needs_explicit_args_for_go_inference(&self, ty: &Type) -> bool {
        if self.is_function_alias(ty) {
            return true;
        }
        let mut current = ty.clone();
        let mut seen: HashSet<Symbol> = HashSet::default();
        while let Type::Nominal { id, params, .. } = &current {
            if !seen.insert(id.clone()) {
                break;
            }
            let Some(definition) = self.facts.definition(id.as_str()) else {
                break;
            };
            if !matches!(definition.body, DefinitionBody::TypeAlias { .. }) {
                break;
            }
            let definition_ty = &definition.ty;
            let (vars, body) = match definition_ty {
                Type::Forall { vars, body } => (vars.clone(), body.as_ref().clone()),
                other => (vec![], other.clone()),
            };
            let map: SubstitutionMap = vars.iter().cloned().zip(params.iter().cloned()).collect();
            current = substitute(&body, &map);
        }
        let Some(numeric) = current.underlying_numeric_type() else {
            return false;
        };
        let Some(kind) = numeric.as_simple() else {
            return false;
        };
        kind.is_arithmetic()
            && !matches!(
                kind,
                SimpleKind::Int | SimpleKind::Float64 | SimpleKind::Complex128
            )
    }
}

fn extract_return_type_param(function: &Expression) -> Option<Type> {
    let ty = function.get_type();
    let Type::Function(f) = ty.unwrap_forall() else {
        return None;
    };
    let Type::Nominal { params, .. } = f.return_type.as_ref() else {
        return None;
    };
    params.first().cloned()
}

impl Planner<'_> {
    fn resolve_element_type(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> String {
        if !type_args.is_empty() {
            return self.annotation_to_go_type(&type_args[0], fx);
        }
        if let Some(call_result_ty) = call_ty
            && let Some(first) = call_result_ty
                .get_type_params()
                .and_then(|ps| ps.first().cloned())
        {
            return self.go_type_string(&first, fx);
        }
        let param = extract_return_type_param(function)
            .expect("constructor must have constructor return type");
        self.go_type_string(&param, fx)
    }

    fn resolve_map_types(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> (String, String) {
        if type_args.len() >= 2 {
            return (
                self.annotation_to_go_type(&type_args[0], fx),
                self.annotation_to_go_type(&type_args[1], fx),
            );
        }
        if let Some(call_result_ty) = call_ty
            && let Some(params) = call_result_ty.get_type_params()
            && params.len() >= 2
        {
            return (
                self.go_type_string(&params[0], fx),
                self.go_type_string(&params[1], fx),
            );
        }
        let ty = function.get_type();
        let Type::Function(f) = ty.unwrap_forall() else {
            unreachable!("MapNew must be a function");
        };
        let params = f
            .return_type
            .get_type_params()
            .expect("MapNew must return a type with type arguments");
        (
            self.go_type_string(&params[0], fx),
            self.go_type_string(&params[1], fx),
        )
    }

    fn try_lower_native_constructor(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        match (ctx.native_type, ctx.method) {
            (NativeGoType::Channel, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty, fx);
                Some((Vec::new(), format!("make(chan {})", element)))
            }
            (NativeGoType::Channel, "buffered") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty, fx);
                let (setup, capacity) = match ctx.args.first() {
                    Some(a) => {
                        let staged = self.stage_operand(a, ExpressionContext::value(), fx);
                        (staged.setup, staged.value)
                    }
                    None => (Vec::new(), "0".to_string()),
                };
                Some((setup, format!("make(chan {}, {})", element, capacity)))
            }
            (NativeGoType::Map, "new") => {
                let (key, val) =
                    self.resolve_map_types(ctx.function, ctx.type_args, ctx.call_ty, fx);
                Some((Vec::new(), format!("make(map[{}]{})", key, val)))
            }
            (NativeGoType::Slice, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.type_args, ctx.call_ty, fx);
                Some((Vec::new(), format!("[]{}{{}}", element)))
            }
            _ => None,
        }
    }

    /// Emit `call_expression` in negated form when the underlying inline rule
    /// has a `negated_template`. Used by unary-not to avoid a precedence bug
    /// for comparison-emitting calls (`!s.is_empty()` → `len(s) != 0`, not
    /// `!len(s) == 0` which Go parses as `(!len(s)) == 0`).
    pub(crate) fn try_emit_negated_call(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            call_kind,
            type_args,
            ..
        } = call_expression
        else {
            return None;
        };
        let function = callee.unwrap_parens();
        let spread = (**spread).as_ref();

        let call_kind = call_kind.filter(|_| !self.is_local_binding(function))?;
        let kind = match call_kind {
            CallKind::NativeMethod(kind) | CallKind::NativeMethodIdentifier(kind) => kind,
            _ => return None,
        };
        let native_type = NativeGoType::from_kind(kind);
        let method = extract_native_method_name(function);
        let native_ctx = NativeCallContext {
            function,
            args,
            spread,
            type_args,
            call_ty: None,
            native_type: &native_type,
            method,
        };
        match call_kind {
            CallKind::NativeMethod(_) => {
                self.try_emit_negated_native_method_dot_access(output, &native_ctx, fx)
            }
            CallKind::NativeMethodIdentifier(_) => {
                self.try_emit_negated_native_method_identifier(output, &native_ctx, fx)
            }
            _ => unreachable!(),
        }
    }

    pub(crate) fn emit_call(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        let (setup, value) = self.lower_call(call_expression, call_ty, ctx, fx);
        output.push_str(&Renderer.render_setup(&setup));
        value
    }

    /// Structured form of `emit_call`: typed setup plus the value text.
    pub(crate) fn lower_call(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Call {
            expression: callee,
            args,
            type_args,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("lower_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = (**spread).as_ref();

        let plan = self
            .plan_call(call_expression)
            .expect("plan_call yields Some for a Call expression");

        match &plan.callee {
            CalleePlan::TupleStructConstructor => {
                if let Some(result) =
                    self.try_lower_tuple_struct_call(function, args, call_ty, ctx, fx)
                {
                    return result;
                }
            }
            CalleePlan::AssertType => {
                return self.lower_assert_type(function, args, type_args, fx);
            }
            CalleePlan::UfcsMethod => {
                return self.lower_ufcs_call(function, args, type_args, spread, fx);
            }
            CalleePlan::NativeConstructor(kind)
            | CalleePlan::NativeMethod(kind)
            | CalleePlan::NativeMethodIdentifier(kind) => {
                let native_type = NativeGoType::from_kind(*kind);
                let method = extract_native_method_name(function);
                let native_ctx = NativeCallContext {
                    function,
                    args,
                    spread,
                    type_args,
                    call_ty,
                    native_type: &native_type,
                    method,
                };
                return self.lower_native_call(&native_ctx, fx);
            }
            CalleePlan::ReceiverMethodUfcs { is_public } => {
                let method = extract_receiver_ufcs_method(function);
                return self.lower_receiver_method_ufcs(
                    function, args, type_args, &method, *is_public, spread, fx,
                );
            }
            CalleePlan::GoInterop(_) | CalleePlan::Regular => {}
        }

        self.lower_regular_call(call_expression, &plan, call_ty, ctx, fx)
    }

    fn lower_native_call(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if let Some(result) = self.try_lower_native_constructor(ctx, fx) {
            return result;
        }
        if let Expression::DotAccess { .. } = ctx.function {
            self.lower_native_method_dot_access(ctx, fx)
        } else {
            self.lower_native_method_identifier(ctx, fx)
        }
    }

    pub(super) fn infer_return_only_type_args(
        &mut self,
        function: &Expression,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let definition_ty = self.get_callee_definition_type(function)?;
        let Type::Forall { vars, body } = definition_ty else {
            return None;
        };
        let Type::Function(f) = body.as_ref() else {
            return None;
        };
        let generic_params = &f.params;

        let all_inferrable = vars.iter().all(|var| {
            let param_ty = Type::Parameter(var.clone());
            generic_params.iter().any(|pt| pt.contains_type(&param_ty))
        });

        let instantiated_ty = function.get_type();
        let mut mapping: HashMap<String, Type> = HashMap::default();
        extract_type_mapping(&body, &instantiated_ty, &mut mapping);

        if all_inferrable {
            let any_needs_explicit = vars.iter().any(|v| {
                mapping
                    .get(v.as_str())
                    .is_some_and(|t| self.needs_explicit_args_for_go_inference(t))
            });
            if !any_needs_explicit {
                return None;
            }
        }

        let resolved: Vec<Type> = vars
            .iter()
            .filter_map(|v| mapping.get(v.as_str()).cloned())
            .collect();

        if resolved.len() != vars.len() {
            return None;
        }

        Some(self.format_type_args(&resolved, fx))
    }

    fn lookup_definition_type(&self, primary: &str, fallback: Option<&str>) -> Option<Type> {
        self.facts
            .definition(primary)
            .or_else(|| fallback.and_then(|f| self.facts.definition(f)))
            .map(|d| d.ty().clone())
    }

    fn get_callee_definition_type(&self, function: &Expression) -> Option<Type> {
        let function = function.unwrap_parens();
        match function {
            Expression::Identifier { value, .. } => {
                let qualified = self.facts.qualified_current(value);
                self.lookup_definition_type(&qualified, Some(value.as_str()))
            }
            Expression::DotAccess {
                expression, member, ..
            } => {
                if let Expression::Identifier { value, .. } = expression.as_ref() {
                    let module_name = self.module.module_for_alias(value).unwrap_or(value);
                    let qualified = format!("{}.{}", module_name, member);
                    let local = self.facts.qualified_current_member(value, member);
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
                    let module_name = self
                        .module
                        .module_for_alias(module_name)
                        .unwrap_or(module_name);
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

        let Some(layout) = self.module.enum_layout(&enum_id) else {
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
            if let Type::Function(f) = ty.unwrap_forall()
                && let Type::Nominal { id, .. } = f.return_type.as_ref()
            {
                return Some(id.to_string());
            }
            None
        }

        match function {
            Expression::Identifier { value, ty, .. } => {
                let enum_id = enum_id_from_type(ty)?;
                let variant = unqualified_name(value);
                let enum_name = unqualified_name(&enum_id);
                let qualified = format!("{}.{}", enum_name, variant);
                if self.facts.has_make_function_name(&qualified) {
                    return Some((enum_id, variant.to_string()));
                }
                if let Type::Function(f) = ty.unwrap_forall() {
                    for key in self.facts.make_function_keys() {
                        if let Some((e_name, v_name)) = key.split_once('.')
                            && e_name == enum_name
                            && let Some(layout) = self.module.enum_layout(&enum_id)
                            && let Some(v) = layout.get_variant(v_name)
                            && v.fields.len() == f.params.len()
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
                    if self.facts.has_make_function_name(&qualified) {
                        let enum_id = enum_id_from_type(ty)?;
                        return Some((enum_id, member.to_string()));
                    }
                }
                if let Expression::DotAccess {
                    member: type_name, ..
                } = expression.as_ref()
                {
                    let qualified = format!("{}.{}", type_name, member);
                    if self.facts.has_make_function_name(&qualified) {
                        let enum_id = enum_id_from_type(ty)?;
                        return Some((enum_id, member.to_string()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Plan a tuple-struct constructor as a struct literal. `None` when this
    /// is not a tuple struct or should fall through to regular call handling.
    fn try_lower_tuple_struct_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let target = self.resolve_tuple_struct_target(function, call_ty, fx)?;

        let stages: Vec<StagedExpression> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value(), fx))
            .collect();
        let (mut setup, values) = self.sequence_structured(stages, "_arg");

        let mut field_pairs: Vec<(String, String)> = Vec::with_capacity(target.field_tys.len());
        for (i, ((field_ty, arg), value)) in target
            .field_tys
            .iter()
            .zip(args.iter())
            .zip(values)
            .enumerate()
        {
            let value_ty = arg.get_type();
            let coercion =
                Coercion::resolve(self, &value_ty, field_ty, CoercionDirection::Internal);
            let (coercion_setup, coerced) = coercion.lower(self, value, fx);
            setup.extend(coercion_setup);
            field_pairs.push((format!("F{}", i), coerced));
        }

        Some((setup, emit_struct_literal(&target.go_ty, &field_pairs, ctx)))
    }

    /// Drill the function's return type into a tuple-struct definition,
    /// returning per-field types and the Go target type. `None` falls through
    /// to regular call handling.
    fn resolve_tuple_struct_target(
        &mut self,
        function: &Expression,
        call_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> Option<TupleStructTarget> {
        let ty = function.get_type();
        let Type::Function(f) = ty.unwrap_forall() else {
            return None;
        };
        let return_ty = call_ty
            .cloned()
            .unwrap_or_else(|| f.return_type.as_ref().clone());

        let Type::Nominal { id, params, .. } = &return_ty else {
            return None;
        };

        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    kind,
                    fields,
                    generics,
                    ..
                },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };

        if *kind != StructKind::Tuple {
            return None;
        }
        if fields.len() == 1 && generics.is_empty() {
            return None;
        }

        let field_tys: Vec<Type> = if generics.is_empty() {
            fields.iter().map(|f| f.ty.clone()).collect()
        } else {
            let subst_map = build_substitution_map(generics, params);
            fields
                .iter()
                .map(|f| substitute(&f.ty, &subst_map))
                .collect()
        };

        let go_ty = self.go_type_string(&return_ty, fx);
        Some(TupleStructTarget { go_ty, field_tys })
    }

    fn lower_assert_type(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: &[Annotation],
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let target_ty = if !type_args.is_empty() {
            self.annotation_to_go_type(&type_args[0], fx)
        } else {
            let param = extract_return_type_param(function)
                .expect("AssertType must have constructor return type");
            self.go_type_string(&param, fx)
        };
        let (setup, arg_expression) = match args.first() {
            Some(a) => self.lower_composite_value(a, ExpressionContext::value(), fx),
            None => (Vec::new(), String::new()),
        };
        fx.require_stdlib();
        (
            setup,
            format!(
                "{}.AssertType[{}]({})",
                go_name::GO_STDLIB_PKG,
                target_ty,
                arg_expression
            ),
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
        let qualified = self.facts.qualified_current(value);
        let prelude_qualified = format!("prelude.{}", value);
        self.facts
            .definition(qualified.as_str())
            .or_else(|| self.facts.definition(prelude_qualified.as_str()))
            .and_then(|d| d.go_name())
    }

    pub(crate) fn is_local_binding(&self, function: &Expression) -> bool {
        if let Expression::Identifier { value, .. } = function {
            self.scope.resolve_identifier_binding(value).is_some()
        } else {
            false
        }
    }

    pub(crate) fn prelude_container_type_args(
        &mut self,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
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
            .any(|p| self.facts.is_interface(p) || self.is_function_alias(p))
            .then(|| self.format_type_args(params, fx))
    }
}

fn extract_native_method_name(function: &Expression) -> &str {
    match function {
        Expression::DotAccess { member, .. } => member,
        Expression::Identifier { value, .. } => {
            value.split_once('.').map(|(_, m)| m).unwrap_or(value)
        }
        _ => "",
    }
}

fn extract_receiver_ufcs_method(function: &Expression) -> String {
    if let Expression::Identifier { value, .. } = function
        && let Some(last_dot) = value.rfind('.')
    {
        return value[last_dot + 1..].to_string();
    }
    String::new()
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
