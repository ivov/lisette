use crate::expressions::access::struct_call::emit_struct_literal;
use crate::names::generics::extract_type_mapping;
use rustc_hash::FxHashMap as HashMap;
use std::borrow::Cow;

use super::NativeCallContext;
use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::is_prelude_container_type;
use crate::abi::transition::multi_value_return;
use crate::calls::native::{native_method_is_pure, native_method_lowers_to_plain_call};
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, define,
};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::plan::go_expression::{CompositeLayout, FunctionLiteralLayout};
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, Stability, ValuePlan};
use crate::types::native::NativeGoType;
use syntax::EcoString;
use syntax::ast::{Expression, Literal, ResolvedCallTypeArguments, StructFields};
use syntax::program::{CallKind, Definition, DefinitionBody, resolved_definition};
use syntax::types::{CompoundKind, FunctionParameter, Type, build_substitution_map, substitute};

struct TupleStructTarget {
    go_ty: String,
    field_tys: Vec<Type>,
}

fn is_checked_literal_key(key: &Expression) -> bool {
    matches!(
        key.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Boolean(_) | Literal::String { .. },
            ..
        }
    )
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
    params: &[FunctionParameter],
    receiver_count: usize,
    arg_shape: CallArgShape,
) -> bool {
    let variadic_idx = params
        .iter()
        .position(|p| p.ty.is_native(CompoundKind::VarArgs))
        .filter(|&i| i == params.len() - 1);
    let variadic_has_args = arg_shape.has_spread
        || variadic_idx.is_some_and(|i| arg_shape.value_count + receiver_count > i);

    vars.iter().all(|var| {
        let param_ty = Type::Parameter(var.clone());
        params.iter().enumerate().any(|(i, pt)| {
            pt.ty.contains_type(&param_ty) && (Some(i) != variadic_idx || variadic_has_args)
        })
    })
}

fn extract_return_type_param(function: &Expression) -> Option<Type> {
    let ty = function.get_type();
    let f = ty.as_function_type()?;
    let Type::Nominal { params, .. } = f.return_type.as_ref() else {
        return None;
    };
    params.first().cloned()
}

impl<'a> Planner<'a> {
    fn resolve_element_type(
        &mut self,
        function: &Expression,
        type_args: ResolvedCallTypeArguments<'_>,
        call_ty: Option<&Type>,
    ) -> String {
        let element = self.resolve_element_lisette_type(function, type_args, call_ty);
        self.use_go_type(&element)
    }

    fn resolve_element_lisette_type<'t>(
        &self,
        function: &Expression,
        type_args: ResolvedCallTypeArguments<'t>,
        call_ty: Option<&'t Type>,
    ) -> Cow<'t, Type> {
        if let Some(first) = type_args.first() {
            return Cow::Borrowed(first);
        }
        if let Some(call_result_ty) = call_ty
            && let Some(first) = call_result_ty.get_type_params().and_then(|ps| ps.first())
        {
            return Cow::Borrowed(first);
        }
        Cow::Owned(
            extract_return_type_param(function)
                .expect("constructor must have constructor return type"),
        )
    }

    fn resolve_map_types(
        &mut self,
        function: &Expression,
        type_args: ResolvedCallTypeArguments<'_>,
        call_ty: Option<&Type>,
    ) -> (String, String) {
        let (key, value) = self.resolve_map_lisette_types(function, type_args, call_ty);
        (self.use_go_type(&key), self.use_go_type(&value))
    }

    fn resolve_map_lisette_types(
        &self,
        function: &Expression,
        type_args: ResolvedCallTypeArguments<'_>,
        call_ty: Option<&Type>,
    ) -> (Type, Type) {
        if type_args.len() >= 2 {
            return (type_args[0].clone(), type_args[1].clone());
        }
        if let Some(call_result_ty) = call_ty
            && let Some(params) = call_result_ty.get_type_params()
            && params.len() >= 2
        {
            return (params[0].clone(), params[1].clone());
        }
        let ty = function.get_type();
        let Some(f) = ty.as_function_type() else {
            unreachable!("MapNew must be a function");
        };
        let params = f
            .return_type
            .get_type_params()
            .expect("MapNew must return a type with type arguments");
        (params[0].clone(), params[1].clone())
    }

    fn stage_size_argument(
        &mut self,
        ctx: &NativeCallContext,
    ) -> (Vec<LoweredStatement>, GoExpression, EvaluationEffect) {
        match ctx.args.first() {
            Some(a) => {
                let staged = self.plan_operand(a, ExpressionContext::value());
                (staged.setup, staged.expression, staged.evaluation.effect)
            }
            None => (
                Vec::new(),
                GoExpression::literal("0".to_string()),
                EvaluationEffect::Pure,
            ),
        }
    }

    fn try_lower_native_constructor(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
        match (ctx.native_type, ctx.method) {
            (NativeGoType::Channel, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some(ValuePlan::observable_call(
                    Vec::new(),
                    GoExpression::call(
                        GoExpression::name("make".to_string()),
                        vec![GoExpression::type_name(format!("chan {}", element))],
                    ),
                    self.native_constructor_effect(ctx, EvaluationEffect::Pure),
                ))
            }
            (NativeGoType::Channel, "buffered") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                let (setup, capacity, argument_effect) = self.stage_size_argument(ctx);
                Some(ValuePlan::observable_call(
                    setup,
                    GoExpression::call(
                        GoExpression::name("make".to_string()),
                        vec![
                            GoExpression::type_name(format!("chan {}", element)),
                            capacity,
                        ],
                    ),
                    self.native_constructor_effect(ctx, argument_effect),
                ))
            }
            (NativeGoType::Map, "new") => {
                let (key, val) =
                    self.resolve_map_types(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some(ValuePlan::observable_call(
                    Vec::new(),
                    GoExpression::call(
                        GoExpression::name("make".to_string()),
                        vec![GoExpression::type_name(format!("map[{}]{}", key, val))],
                    ),
                    self.native_constructor_effect(ctx, EvaluationEffect::Pure),
                ))
            }
            (NativeGoType::Map, "from") => self.try_lower_map_from_pairs(ctx),
            (NativeGoType::Slice, "new") => {
                let element =
                    self.resolve_element_type(ctx.function, ctx.resolved_type_args, ctx.call_ty);
                Some(ValuePlan::computed(
                    Vec::new(),
                    GoExpression::empty_composite(format!("[]{}", element)),
                    self.native_constructor_effect(ctx, EvaluationEffect::Pure),
                ))
            }
            (NativeGoType::Slice, "make") => {
                let element_ty = self.resolve_element_lisette_type(
                    ctx.function,
                    ctx.resolved_type_args,
                    ctx.call_ty,
                );
                let element = self.use_go_type(&element_ty);
                let (setup, length, argument_effect) = self.stage_size_argument(ctx);
                let value = if self.element_go_zero_ok(&element_ty) {
                    GoExpression::call(
                        GoExpression::name("make".to_string()),
                        vec![GoExpression::type_name(format!("[]{}", element)), length],
                    )
                } else {
                    let zero = self.lisette_zero(&element_ty);
                    let slice_type = format!("[]{element}");
                    let slice = || GoExpression::name("s".to_string());
                    let index = || GoExpression::name("i".to_string());
                    GoExpression::immediate_call(
                        slice_type.clone(),
                        LoweredBlock {
                            statements: vec![
                                define(
                                    "s".to_string(),
                                    GoExpression::call(
                                        GoExpression::name("make".to_string()),
                                        vec![GoExpression::type_name(slice_type), length],
                                    ),
                                ),
                                LoweredStatement::Loop(LoopPlan {
                                    prologue: Vec::new(),
                                    kind: LoopKind::Generated { label: None },
                                    header: LoopHeader::Range {
                                        key: Some("i".to_string()),
                                        value: None,
                                        iterable: slice(),
                                    },
                                    body: LoweredBlock {
                                        statements: vec![assign(
                                            GoExpression::index(slice(), index()),
                                            zero,
                                        )],
                                    },
                                }),
                                plain_return(slice()),
                            ],
                        },
                        FunctionLiteralLayout::MultiLine,
                    )
                };
                Some(ValuePlan::observable_call(
                    setup,
                    value,
                    self.native_constructor_effect(ctx, argument_effect),
                ))
            }
            (NativeGoType::Array, "new") => {
                let peeled = ctx.call_ty.map(|t| self.facts.peel_alias(t));
                if let Some(Type::Array { length, element }) = &peeled {
                    Some(ValuePlan::computed(
                        Vec::new(),
                        self.array_zero(*length, element),
                        self.native_constructor_effect(ctx, EvaluationEffect::Pure),
                    ))
                } else {
                    None
                }
            }
            (NativeGoType::Array, "from") => self.try_lower_array_from(ctx),
            _ => None,
        }
    }

    /// Lower `Array.from<T, N>(slice)` to a length-guarded comma-ok pair.
    fn try_lower_array_from(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
        // A comma-ok context passes no `call_ty`, so fall back to the callee.
        let callee_ty = ctx.function.get_type();
        let returned = callee_ty
            .as_function_type()
            .map(|f| f.return_type.as_ref())
            .or(ctx.call_ty)?;
        let option_ty = self.facts.peel_alias(returned);
        let inner = option_ty.get_type_params()?.first()?;
        let Type::Array { length, element } = self.facts.peel_alias(inner) else {
            return None;
        };
        let argument = ctx.args.first()?;

        let element_go = self.use_go_type(&element);
        let array_go = format!("[{length}]{element_go}");

        let staged = self.plan_operand(argument, ExpressionContext::value());
        let argument_effect = staged.evaluation.effect;
        let (setup, source) = staged.into_parts();

        let slice = || GoExpression::name("s".to_string());
        let literal = |text: String| GoExpression::literal(text);
        let value = GoExpression::call(
            GoExpression::function_literal(
                format!("s []{element_go}"),
                format!("({array_go}, bool)"),
                LoweredBlock {
                    statements: vec![
                        LoweredStatement::If(IfPlan::plain(
                            GoExpression::binary(
                                GoExpression::call(
                                    GoExpression::name("len".to_string()),
                                    vec![slice()],
                                ),
                                "!=",
                                literal(length.to_string()),
                            ),
                            LoweredBlock {
                                statements: vec![multi_value_return(vec![
                                    GoExpression::empty_composite(array_go.clone()),
                                    literal("false".to_string()),
                                ])],
                            },
                            ElseArm::None,
                        )),
                        multi_value_return(vec![
                            GoExpression::conversion(array_go, slice()),
                            literal("true".to_string()),
                        ]),
                    ],
                },
                FunctionLiteralLayout::MultiLine,
            ),
            vec![source],
        );

        Some(ValuePlan::observable_call(
            setup,
            value,
            self.native_constructor_effect(ctx, argument_effect),
        ))
    }

    fn try_lower_map_from_pairs(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
        if ctx.spread.is_some() {
            return None;
        }
        let [argument] = ctx.args else {
            return None;
        };
        let Expression::Literal {
            literal: Literal::Slice(entries),
            ..
        } = argument.unwrap_parens()
        else {
            return None;
        };
        let pairs = entries
            .iter()
            .map(|entry| match entry.unwrap_parens() {
                Expression::Tuple { elements, .. } => match elements.as_slice() {
                    [key, value] => Some((key, value)),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;

        if !pairs.iter().all(|(key, _)| is_checked_literal_key(key)) {
            return None;
        }

        let (key_ty, value_ty) =
            self.resolve_map_lisette_types(ctx.function, ctx.resolved_type_args, ctx.call_ty);
        let key_go_ty = self.use_go_type(&key_ty);
        let value_go_ty = self.use_go_type(&value_ty);
        let map_ty = format!("map[{}]{}", key_go_ty, value_go_ty);

        if pairs.is_empty() {
            return Some(ValuePlan::computed(
                Vec::new(),
                GoExpression::empty_composite(map_ty),
                self.native_constructor_effect(ctx, EvaluationEffect::Pure),
            ));
        }

        let stages: Vec<ValuePlan> = pairs
            .iter()
            .flat_map(|(key, value)| [*key, *value])
            .map(|e| self.lower_composite_value(e, ExpressionContext::value()))
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "entry");
        let effect = sequenced.effect;
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();

        let mut lowered = Vec::with_capacity(pairs.len());
        let mut widest = 0;
        for (key, value) in &pairs {
            let staged_key = values.next().expect("each pair stages a key");
            let staged_value = values.next().expect("each pair stages a value");
            let (key_setup, coerced_key) =
                CoercionPlan::internal(self, &key.get_type(), &key_ty).lower(self, staged_key);
            setup.extend(key_setup);
            let value_coercion = CoercionPlan::internal(self, &value.get_type(), &value_ty);
            let is_whole_literal =
                value_coercion.is_identity() && staged_value.is_composite_literal();
            let (value_setup, coerced_value) = value_coercion.lower(self, staged_value);
            setup.extend(value_setup);
            widest = widest
                .max(coerced_key.rendered().len() + coerced_value.rendered().len() + ": ".len());
            let coerced_value = if is_whole_literal {
                coerced_value.elide_composite_type(&value_go_ty)
            } else {
                coerced_value
            };
            lowered.push((Some(coerced_key), coerced_value));
        }

        let layout = CompositeLayout::for_elements(lowered.len(), widest);
        Some(ValuePlan::computed(
            setup,
            GoExpression::composite(Some(map_ty), lowered, layout),
            self.native_constructor_effect(ctx, effect),
        ))
    }

    fn native_constructor_effect(
        &self,
        ctx: &NativeCallContext,
        argument_effect: EvaluationEffect,
    ) -> EvaluationEffect {
        if self.is_pure_constructor_callee(ctx.function) {
            EvaluationEffect::PureCall.combine(argument_effect)
        } else {
            EvaluationEffect::EffectfulCall
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
    ) -> Option<GoExpression> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            call_kind,
            type_arguments,
            ..
        } = call_expression
        else {
            return None;
        };
        let function = callee.unwrap_parens();
        let spread = spread.as_deref();
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");

        let call_kind = (!self.is_local_binding(function)).then_some(*call_kind)?;
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
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
            result_name: None,
        };
        self.try_emit_negated_native_method(setup, &native_ctx)
    }

    /// Lower a call expression to typed setup plus the value text.
    pub(crate) fn lower_call(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self
            .plan_call(call_expression)
            .expect("plan_call yields Some for a Call expression");
        self.lower_call_with_plan(call_expression, call_ty, ctx, plan)
    }

    /// Lower a call whose plan the caller already built.
    pub(crate) fn lower_call_with_plan(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        plan: CallPlan<'a>,
    ) -> ValuePlan {
        let Expression::Call {
            expression: callee,
            args,
            type_arguments,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("lower_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = spread.as_deref();
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");

        if let Some(predicate) = self.lower_fused_predicate_value(call_expression, false) {
            return predicate;
        }
        if let Some(defaulted) = self.lower_defaulted_call_value(call_expression) {
            return defaulted;
        }

        match &plan.resolved.origin {
            CallableOrigin::TupleStructConstructor => {
                if let Some(result) = self.try_lower_tuple_struct_call(function, args, call_ty, ctx)
                {
                    return result;
                }
            }
            CallableOrigin::AssertType => {
                let (setup, value) = self.lower_assert_type(function, args, resolved_type_args);
                return ValuePlan::plain_call(setup, value, EvaluationEffect::EffectfulCall);
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
                    capture_boundary: ctx.capture_boundary(),
                    retired_receiver: ctx.retired_receiver(),
                    result_name: None,
                };
                return self.lower_native_call(&native_ctx, &plan.resolved.origin);
            }
            CallableOrigin::ReceiverMethodUfcs { is_public } => {
                let method = extract_receiver_ufcs_method(function);
                return self
                    .lower_receiver_method_ufcs(function, args, &method, *is_public, spread);
            }
            CallableOrigin::GoInterop | CallableOrigin::Regular => {}
        }

        self.lower_regular_call(call_expression, &plan, call_ty, ctx)
    }

    pub(super) fn lower_native_call(
        &mut self,
        ctx: &NativeCallContext,
        origin: &CallableOrigin,
    ) -> ValuePlan {
        if let Some(result) = self.try_lower_native_constructor(ctx) {
            return result;
        }
        let result = self.lower_native_method(ctx);
        let effect = if matches!(origin, CallableOrigin::NativeConstructor(_)) {
            self.native_constructor_effect(ctx, result.argument_effect)
        } else if matches!(
            origin,
            CallableOrigin::NativeMethod(_) | CallableOrigin::NativeMethodIdentifier(_)
        ) && native_method_is_pure(ctx.native_type, ctx.method)
        {
            EvaluationEffect::PureCall.combine(result.argument_effect)
        } else {
            EvaluationEffect::EffectfulCall
        };
        let receiver = match ctx.function {
            Expression::DotAccess { expression, .. } => Some(expression.as_ref()),
            _ => ctx.args.first(),
        };
        // Channel length can change without rebinding the receiver.
        let reads_fixed_length = matches!(ctx.method, "length" | "capacity")
            && !matches!(origin, CallableOrigin::NativeConstructor(_))
            && !matches!(
                ctx.native_type,
                NativeGoType::Channel | NativeGoType::Sender | NativeGoType::Receiver
            )
            && receiver.is_some_and(|receiver| self.is_unmutated_identifier(receiver));
        if result.setup.iter().any(|statement| {
            result
                .value
                .as_identifier()
                .is_some_and(|name| statement.binds_name(name))
        }) {
            return ValuePlan::captured_with_effect(result.setup, result.value.rendered(), effect);
        }
        let receiver_arity = if matches!(origin, CallableOrigin::NativeMethodIdentifier(_)) {
            ctx.args.len().saturating_sub(1)
        } else {
            ctx.args.len()
        };
        let plain_call = !matches!(origin, CallableOrigin::NativeConstructor(_))
            && native_method_lowers_to_plain_call(ctx.native_type, ctx.method, receiver_arity);
        let mut plan = if plain_call {
            ValuePlan::plain_call(result.setup, result.value, effect)
        } else {
            ValuePlan::computed(result.setup, result.value, effect)
        };
        if reads_fixed_length {
            plan.evaluation.stability = Stability::Fixed;
        }
        plan
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
                    .is_some_and(|t| self.is_function_alias(t))
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
    ) -> Option<ValuePlan> {
        let target = self.resolve_tuple_struct_target(function, call_ty)?;

        let arg_ctx = match (ctx.retired_receiver(), args.len()) {
            (Some(retired), 1) => ExpressionContext::value().with_retired_receiver(retired),
            _ => ExpressionContext::value(),
        };
        let stages: Vec<ValuePlan> = args
            .iter()
            .map(|a| self.lower_composite_value(a, arg_ctx))
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "arg");
        let effect = EvaluationEffect::PureCall.combine(sequenced.effect);
        let mut setup = sequenced.setup;

        let mut field_pairs = Vec::with_capacity(target.field_tys.len());
        for (i, ((field_ty, arg), value)) in target
            .field_tys
            .iter()
            .zip(args.iter())
            .zip(sequenced.values)
            .enumerate()
        {
            let value_ty = arg.get_type();
            let coercion = CoercionPlan::internal(self, &value_ty, field_ty);
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            field_pairs.push((format!("F{}", i), coerced));
        }

        Some(ValuePlan::computed(
            setup,
            emit_struct_literal(&target.go_ty, field_pairs),
            effect,
        ))
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
                    fields: StructFields::Tuple(fields),
                    generics,
                    ..
                },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };

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

        let go_ty = self.use_go_type(&return_ty);
        Some(TupleStructTarget { go_ty, field_tys })
    }

    fn lower_assert_type(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: ResolvedCallTypeArguments<'_>,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let target_ty = if !type_args.is_empty() {
            self.use_go_type(&type_args[0])
        } else {
            let param = extract_return_type_param(function)
                .expect("AssertType must have constructor return type");
            self.use_go_type(&param)
        };
        let (setup, arguments) = match args.first() {
            Some(a) => {
                let staged = self.lower_composite_value(a, ExpressionContext::value());
                (staged.setup, vec![staged.expression])
            }
            None => (Vec::new(), Vec::new()),
        };
        (
            setup,
            GoExpression::call(
                GoExpression::instantiation(
                    GoExpression::generated(GeneratedPackage::Prelude, "AssertType"),
                    format!("[{target_ty}]"),
                ),
                arguments,
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
        if !is_prelude_container_type(ty) {
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
            .any(|p| self.facts.is_interface_or_unknown(p) || self.is_function_alias(p))
            .then(|| self.format_type_args(params))
    }
}

pub(crate) fn extract_native_method_name(function: &Expression) -> &str {
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

pub(super) fn callee_is_go_builtin(callee: &Expression) -> bool {
    go_builtin_name(callee).is_some()
}

pub(super) fn go_builtin_name(callee: &Expression) -> Option<&str> {
    resolved_definition(callee)
        .filter(|qualified| go_name::is_prelude_go_builtin(qualified))
        .and_then(|qualified| qualified.strip_prefix("prelude."))
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
