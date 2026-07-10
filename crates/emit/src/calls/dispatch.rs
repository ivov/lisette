use crate::expressions::access::struct_call::emit_struct_literal;
use crate::names::generics::extract_type_mapping;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::NativeCallContext;
use crate::Planner;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::CallableOrigin;
use crate::types::native::NativeGoType;
use syntax::EcoString;
use syntax::ast::{Expression, StructKind};
use syntax::program::{CallKind, Definition, DefinitionBody};
use syntax::types::{
    CompoundKind, SimpleKind, SubstitutionMap, Symbol, Type, build_substitution_map, substitute,
};

struct TupleStructTarget {
    go_ty: String,
    field_tys: Vec<Type>,
}

/// The shape of a call's value arguments, used to decide which type parameters
/// Go can infer. `value_count` excludes any receiver argument.
#[derive(Clone, Copy)]
pub(crate) struct CallArgShape {
    pub value_count: usize,
    pub has_spread: bool,
}

pub(crate) fn all_type_params_inferrable(
    vars: &[EcoString],
    params: &[Type],
    receiver_count: usize,
    arg_shape: CallArgShape,
) -> bool {
    let variadic_idx = params
        .iter()
        .position(|p| p.is_native(CompoundKind::VarArgs))
        .filter(|&i| i == params.len() - 1);
    let variadic_has_args = arg_shape.has_spread
        || variadic_idx.is_some_and(|i| arg_shape.value_count + receiver_count > i);

    vars.iter().all(|var| {
        let param_ty = Type::Parameter(var.clone());
        params.iter().enumerate().any(|(i, pt)| {
            pt.contains_type(&param_ty) && (Some(i) != variadic_idx || variadic_has_args)
        })
    })
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
    let f = ty.as_function_type()?;
    let Type::Nominal { params, .. } = f.return_type.as_ref() else {
        return None;
    };
    params.first().cloned()
}

impl Planner<'_> {
    fn resolve_element_type(
        &mut self,
        function: &Expression,
        type_args: &[Type],
        call_ty: Option<&Type>,
    ) -> String {
        if !type_args.is_empty() {
            return self.go_type_string(&type_args[0]);
        }
        if let Some(call_result_ty) = call_ty
            && let Some(first) = call_result_ty
                .get_type_params()
                .and_then(|ps| ps.first().cloned())
        {
            return self.go_type_string(&first);
        }
        let param = extract_return_type_param(function)
            .expect("constructor must have constructor return type");
        self.go_type_string(&param)
    }

    fn resolve_map_types(
        &mut self,
        function: &Expression,
        type_args: &[Type],
        call_ty: Option<&Type>,
    ) -> (String, String) {
        if type_args.len() >= 2 {
            return (
                self.go_type_string(&type_args[0]),
                self.go_type_string(&type_args[1]),
            );
        }
        if let Some(call_result_ty) = call_ty
            && let Some(params) = call_result_ty.get_type_params()
            && params.len() >= 2
        {
            return (
                self.go_type_string(&params[0]),
                self.go_type_string(&params[1]),
            );
        }
        let ty = function.get_type();
        let Some(f) = ty.as_function_type() else {
            unreachable!("MapNew must be a function");
        };
        let params = f
            .return_type
            .get_type_params()
            .expect("MapNew must return a type with type arguments");
        (
            self.go_type_string(&params[0]),
            self.go_type_string(&params[1]),
        )
    }

    fn try_lower_native_constructor(
        &mut self,
        ctx: &NativeCallContext,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        match (ctx.native_type, ctx.method) {
            (NativeGoType::Channel, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some((Vec::new(), format!("make(chan {})", element)))
            }
            (NativeGoType::Channel, "buffered") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                let (setup, capacity) = match ctx.args.first() {
                    Some(a) => {
                        let staged = self.stage_operand(a, ExpressionContext::value());
                        (staged.setup, staged.value)
                    }
                    None => (Vec::new(), "0".to_string()),
                };
                Some((setup, format!("make(chan {}, {})", element, capacity)))
            }
            (NativeGoType::Map, "new") => {
                let (key, val) =
                    self.resolve_map_types(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some((Vec::new(), format!("make(map[{}]{})", key, val)))
            }
            (NativeGoType::Slice, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some((Vec::new(), format!("[]{}{{}}", element)))
            }
            (NativeGoType::Array, "new") => {
                let peeled = ctx.call_ty.map(|t| self.facts.peel_alias(t));
                if let Some(Type::Array { length, element }) = &peeled {
                    Some((Vec::new(), self.array_zero(*length, element)))
                } else {
                    None
                }
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
        setup: &mut Vec<LoweredStatement>,
        call_expression: &Expression,
    ) -> Option<String> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            call_kind,
            resolved_type_args,
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
            resolved_type_args,
            call_ty: None,
            native_type: &native_type,
            method,
        };
        match call_kind {
            CallKind::NativeMethod(_) => {
                self.try_emit_negated_native_method_dot_access(setup, &native_ctx)
            }
            CallKind::NativeMethodIdentifier(_) => {
                self.try_emit_negated_native_method_identifier(setup, &native_ctx)
            }
            _ => unreachable!(),
        }
    }

    /// Lower a call expression to typed setup plus the value text.
    pub(crate) fn lower_call(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Call {
            expression: callee,
            args,
            resolved_type_args,
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

        match &plan.resolved.origin {
            CallableOrigin::TupleStructConstructor => {
                if let Some(result) = self.try_lower_tuple_struct_call(function, args, call_ty, ctx)
                {
                    return result;
                }
            }
            CallableOrigin::AssertType => {
                return self.lower_assert_type(function, args, resolved_type_args);
            }
            CallableOrigin::UfcsMethod => {
                return self.lower_ufcs_call(function, args, resolved_type_args, spread, &plan);
            }
            CallableOrigin::NativeConstructor(kind)
            | CallableOrigin::NativeMethod(kind)
            | CallableOrigin::NativeMethodIdentifier(kind) => {
                let native_type = NativeGoType::from_kind(*kind);
                let method = extract_native_method_name(function);
                let native_ctx = NativeCallContext {
                    function,
                    args,
                    spread,
                    resolved_type_args,
                    call_ty,
                    native_type: &native_type,
                    method,
                };
                return self.lower_native_call(&native_ctx);
            }
            CallableOrigin::ReceiverMethodUfcs { is_public } => {
                let method = extract_receiver_ufcs_method(function);
                return self.lower_receiver_method_ufcs(
                    function,
                    args,
                    resolved_type_args,
                    &method,
                    *is_public,
                    spread,
                );
            }
            CallableOrigin::GoInterop | CallableOrigin::Regular => {}
        }

        self.lower_regular_call(call_expression, &plan, call_ty, ctx)
    }

    fn lower_native_call(&mut self, ctx: &NativeCallContext) -> (Vec<LoweredStatement>, String) {
        if let Some(result) = self.try_lower_native_constructor(ctx) {
            return result;
        }
        if let Expression::DotAccess { .. } = ctx.function {
            self.lower_native_method_dot_access(ctx)
        } else {
            self.lower_native_method_identifier(ctx)
        }
    }

    pub(super) fn infer_return_only_type_args(
        &mut self,
        function: &Expression,
        declared: Option<&Type>,
        arg_shape: CallArgShape,
    ) -> Option<String> {
        let Type::Forall { vars, body } = declared? else {
            return None;
        };
        let Type::Function(f) = body.as_ref() else {
            return None;
        };
        let generic_params = &f.params;
        let all_inferrable = all_type_params_inferrable(vars, generic_params, 0, arg_shape);

        let instantiated_ty = function.get_type();
        let mut mapping: HashMap<String, Type> = HashMap::default();
        extract_type_mapping(body, &instantiated_ty, &mut mapping);

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

        Some(self.format_type_args(&resolved))
    }

    /// Plan a tuple-struct constructor as a struct literal. `None` when this
    /// is not a tuple struct or should fall through to regular call handling.
    fn try_lower_tuple_struct_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let target = self.resolve_tuple_struct_target(function, call_ty)?;

        let stages: Vec<StagedExpression> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value()))
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
            let (coercion_setup, coerced) = coercion.lower(self, value);
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
    ) -> Option<TupleStructTarget> {
        let ty = function.get_type();
        let f = ty.as_function_type()?;
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

        let go_ty = self.go_type_string(&return_ty);
        Some(TupleStructTarget { go_ty, field_tys })
    }

    fn lower_assert_type(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: &[Type],
    ) -> (Vec<LoweredStatement>, String) {
        let target_ty = if !type_args.is_empty() {
            self.go_type_string(&type_args[0])
        } else {
            let param = extract_return_type_param(function)
                .expect("AssertType must have constructor return type");
            self.go_type_string(&param)
        };
        let (setup, arg_expression) = match args.first() {
            Some(a) => self
                .lower_composite_value(a, ExpressionContext::value())
                .into_parts(),
            None => (Vec::new(), String::new()),
        };
        self.require_stdlib();
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
            .any(|p| self.facts.is_interface(p) || self.is_function_alias(p))
            .then(|| self.format_type_args(params))
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
