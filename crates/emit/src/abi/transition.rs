use syntax::types::Type;

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::LayoutBridge;
use crate::abi::tuple_element_types;
use crate::calls::go_interop::WrapperTarget;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{
    OPTION_SOME_TAG, PARTIAL_ERR_TAG, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::control_flow::propagation::plain_return;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, ReturnForm, define, define_many,
};
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;

/// A bare `return v0, v1, ...` statement leaf.
pub(crate) fn multi_value_return(values: Vec<GoExpression>) -> LoweredStatement {
    LoweredStatement::Return(ReturnForm::Multi { values })
}

/// An `if <condition> { <setup...> return <then_values...> }` tag-check leaf (no else).
pub(crate) fn tag_check(
    condition: GoExpression,
    setup: Vec<LoweredStatement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    tag_check_with_initializer(None, condition, setup, then_values)
}

pub(crate) fn tag_check_with_initializer(
    initializer: Option<Definition>,
    condition: GoExpression,
    setup: Vec<LoweredStatement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    let mut statements = setup;
    statements.push(multi_value_return(then_values));
    LoweredStatement::If(IfPlan {
        condition_setup: Vec::new(),
        initializer,
        condition,
        then_body: LoweredBlock { statements },
        else_arm: ElseArm::None,
    })
}

fn has_tag(value: &GoExpression, tag: &str) -> GoExpression {
    GoExpression::binary(
        GoExpression::selector(value.clone(), "Tag".to_string()),
        "==",
        GoExpression::generated(GeneratedPackage::Prelude, tag),
    )
}

fn field(value: &GoExpression, field: &str) -> GoExpression {
    GoExpression::selector(value.clone(), field.to_string())
}

/// The lowered Go-return values for an `Err`-with-payload failure, in the
/// enclosing function's lowered shape (e.g. `[zero, err]`).
pub(crate) fn lowered_err_values(
    planner: &mut Planner,
    shape: &CallableReturnAbi,
    return_ty: &Type,
    err_expr: GoExpression,
) -> Vec<GoExpression> {
    match shape {
        CallableReturnAbi::BareError => vec![err_expr],
        CallableReturnAbi::Result { .. } | CallableReturnAbi::Partial { .. } => {
            let ok_ty = planner.facts.peel_alias(return_ty).ok_type();
            let mut values = lowered_payload_zeros(planner, shape, &ok_ty);
            values.push(err_expr);
            values
        }
        CallableReturnAbi::Tuple { .. } => {
            unreachable!("a tuple return has no failure path")
        }
        CallableReturnAbi::Tagged | CallableReturnAbi::Direct | CallableReturnAbi::Option(_) => {
            unreachable!("Option's failure constructor `None` carries no payload")
        }
    }
}

/// The lowered Go-return values for a success-constructor payload, in the
/// enclosing function's lowered shape (e.g. `[ok, nil]`). `payload` holds
/// the already-lowered payload slots.
pub(crate) fn lowered_ok_values(
    shape: &CallableReturnAbi,
    mut payload: Vec<GoExpression>,
) -> Vec<GoExpression> {
    match shape {
        CallableReturnAbi::BareError => vec![GoExpression::nil()],
        CallableReturnAbi::Result { .. } | CallableReturnAbi::Partial { .. } => {
            payload.push(GoExpression::nil());
            payload
        }
        CallableReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
            payload.push(GoExpression::literal("true".to_string()));
            payload
        }
        CallableReturnAbi::Option(OptionReturnAbi::Nullable) => payload,
        CallableReturnAbi::Tuple { .. } => {
            unreachable!("a tuple return has its own emission path")
        }
        CallableReturnAbi::Tagged
        | CallableReturnAbi::Direct
        | CallableReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => {
            unreachable!("not a lowered Lisette return ABI")
        }
    }
}

/// The lowered Go-return values for a bare `None`, in an Option-shaped fn's
/// lowered shape (e.g. `[zero, false]`).
pub(crate) fn lowered_none_values(
    planner: &mut Planner,
    shape: &CallableReturnAbi,
    return_ty: &Type,
) -> Vec<GoExpression> {
    match shape {
        CallableReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
            let inner = planner.facts.peel_alias(return_ty).ok_type();
            let mut values = lowered_payload_zeros(planner, shape, &inner);
            values.push(GoExpression::literal("false".to_string()));
            values
        }
        CallableReturnAbi::Option(OptionReturnAbi::Nullable) => vec![GoExpression::nil()],
        _ => unreachable!("only Option's `None` lacks a payload"),
    }
}

pub(crate) fn lowered_payload_values(
    planner: &mut Planner,
    shape: &CallableReturnAbi,
    payload_ty: &Type,
    payload_expr: GoExpression,
) -> (Vec<LoweredStatement>, Vec<GoExpression>) {
    if shape.has_flattened_payload() {
        let mut statements = Vec::new();
        let tuple = planner.stable_source(&mut statements, "tup", payload_expr);
        let (projection, values) = lowered_tuple_values(planner, &tuple, payload_ty);
        statements.extend(projection);
        (statements, values)
    } else {
        (Vec::new(), vec![payload_expr])
    }
}

fn lowered_payload_zeros(
    planner: &mut Planner,
    shape: &CallableReturnAbi,
    payload_ty: &Type,
) -> Vec<GoExpression> {
    if shape.has_flattened_payload() {
        tuple_element_types(&planner.facts.peel_alias(payload_ty))
            .iter()
            .map(|slot_ty| {
                if planner.facts.is_nullable_option(slot_ty) {
                    GoExpression::nil()
                } else {
                    planner.zero_value_expression(slot_ty)
                }
            })
            .collect()
    } else {
        vec![planner.zero_value_expression(payload_ty)]
    }
}

/// Destructure a Lisette tagged value into a lowered Go-tuple return,
/// as structured tag-check `IfPlan`s and `Return` leaves.
pub(crate) fn emit_lowered_result_return(
    planner: &mut Planner,
    result_value: &GoExpression,
    return_ty: &Type,
    shape: &CallableReturnAbi,
) -> Vec<LoweredStatement> {
    let p = result_value;
    let ok_ty = || planner.facts.peel_alias(return_ty).ok_type();
    match shape {
        CallableReturnAbi::BareError | CallableReturnAbi::Result { .. } => {
            let (ok_setup, ok_payload) =
                lowered_payload_values(planner, shape, &ok_ty(), field(p, "OkVal"));
            vec![
                tag_check(
                    has_tag(p, RESULT_OK_TAG),
                    ok_setup,
                    lowered_ok_values(shape, ok_payload),
                ),
                multi_value_return(lowered_err_values(
                    planner,
                    shape,
                    return_ty,
                    field(p, "ErrVal"),
                )),
            ]
        }
        CallableReturnAbi::Partial { .. } => {
            let ok_ty = ok_ty();
            let (ok_setup, ok_payload) =
                lowered_payload_values(planner, shape, &ok_ty, field(p, "OkVal"));
            let (both_setup, mut both_values) =
                lowered_payload_values(planner, shape, &ok_ty, field(p, "OkVal"));
            both_values.push(field(p, "ErrVal"));
            let mut statements = vec![
                tag_check(
                    has_tag(p, PARTIAL_OK_TAG),
                    ok_setup,
                    lowered_ok_values(shape, ok_payload),
                ),
                tag_check(
                    has_tag(p, PARTIAL_ERR_TAG),
                    Vec::new(),
                    lowered_err_values(planner, shape, return_ty, field(p, "ErrVal")),
                ),
            ];
            statements.extend(both_setup);
            statements.push(multi_value_return(both_values));
            statements
        }
        CallableReturnAbi::Option(OptionReturnAbi::CommaOk { .. } | OptionReturnAbi::Nullable) => {
            let (some_setup, some_payload) =
                lowered_payload_values(planner, shape, &ok_ty(), field(p, "SomeVal"));
            vec![
                tag_check(
                    has_tag(p, OPTION_SOME_TAG),
                    some_setup,
                    lowered_ok_values(shape, some_payload),
                ),
                multi_value_return(lowered_none_values(planner, shape, return_ty)),
            ]
        }
        CallableReturnAbi::Tuple { .. } => {
            emit_lowered_tuple_return(planner, result_value, return_ty)
        }
        CallableReturnAbi::Tagged
        | CallableReturnAbi::Direct
        | CallableReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => {
            unreachable!("not a lowered Lisette return ABI")
        }
    }
}

fn emit_lowered_tuple_return(
    planner: &mut Planner,
    result_value: &GoExpression,
    return_ty: &Type,
) -> Vec<LoweredStatement> {
    let (mut statements, fields) = lowered_tuple_values(planner, result_value, return_ty);
    statements.push(multi_value_return(fields));
    statements
}

/// Project each field of a lowered tuple value, unwrapping any
/// nullable-Option slot to its bare Go nilable.
fn lowered_tuple_values(
    planner: &mut Planner,
    tuple_value: &GoExpression,
    tuple_ty: &Type,
) -> (Vec<LoweredStatement>, Vec<GoExpression>) {
    let slot_tys = tuple_element_types(&planner.facts.peel_alias(tuple_ty));
    let mut statements = Vec::new();
    let fields = slot_tys
        .iter()
        .enumerate()
        .map(|(i, slot_ty)| {
            let raw = field(tuple_value, TUPLE_FIELDS[i]);
            if planner.facts.is_nullable_option(slot_ty) {
                let inner = planner.use_go_type(&slot_ty.ok_type());
                planner.plan_option_projection(
                    &mut statements,
                    raw,
                    &inner,
                    &LayoutBridge::Identity,
                    false,
                )
            } else {
                raw
            }
        })
        .collect();
    (statements, fields)
}

/// Lower each element of a tuple literal into its lowered return slot.
pub(crate) fn lowered_tuple_literal_values(
    planner: &mut Planner,
    elements: &[Expression],
    tuple_ty: &Type,
) -> (Vec<LoweredStatement>, Vec<GoExpression>) {
    let slot_tys = tuple_element_types(&planner.facts.peel_alias(tuple_ty));
    let stages: Vec<ValuePlan> = elements
        .iter()
        .enumerate()
        .map(|(i, e)| match slot_tys.get(i) {
            Some(slot_ty) if planner.facts.is_nullable_option(slot_ty) => {
                lower_nullable_slot_value(planner, e, slot_ty)
            }
            _ => planner.lower_composite_value(e, ExpressionContext::value()),
        })
        .collect();
    let sequenced = planner.sequence_values(stages, CaptureBoundary::SiblingSequence, "ret");
    let mut statements = sequenced.setup;
    let parts =
        planner.coerce_elements_to_slots(&mut statements, elements, sequenced.values, &slot_tys);
    (statements, parts)
}

impl Planner<'_> {
    /// Wrap a callable's physical Go result into the Lisette-visible value.
    pub(crate) fn lower_abi_to_tagged(
        &mut self,
        raw_value: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        if abi.is_passthrough() {
            return (Vec::new(), raw_value);
        }
        if let CallableReturnAbi::Tuple { arity } = abi {
            let mut statements = Vec::new();
            let temps = self.create_temp_vars("ret", *arity);
            statements.push(define_many(temps.clone(), raw_value));
            let slot_tys = tuple_element_types(&self.facts.peel_alias(result_ty));
            let values: Vec<GoExpression> = temps
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let value = GoExpression::name(value);
                    match slot_tys
                        .get(index)
                        .filter(|slot_ty| self.facts.is_nullable_option(slot_ty))
                    {
                        Some(slot_ty) => {
                            self.plan_nil_check_option_wrap(&mut statements, value, slot_ty)
                        }
                        None => value,
                    }
                })
                .collect();
            let tuple = self.plan_tuple_from_vars(&mut statements, values);
            return (statements, tuple);
        }

        let (wrap, outcome) =
            self.lower_abi_wrapping(raw_value, abi, result_ty, WrapperTarget::FreshSlot);
        (
            wrap,
            GoExpression::name(outcome.expect("wrapper produced no slot")),
        )
    }

    /// Wrap a callable's physical Go result and return it in each wrapper branch.
    pub(crate) fn lower_abi_to_tagged_return(
        &mut self,
        raw_value: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> Vec<LoweredStatement> {
        if abi.is_passthrough() || matches!(abi, CallableReturnAbi::Tuple { .. }) {
            let (mut statements, value) = self.lower_abi_to_tagged(raw_value, abi, result_ty);
            statements.push(plain_return(value));
            return statements;
        }
        let (statements, outcome) =
            self.lower_abi_wrapping(raw_value, abi, result_ty, WrapperTarget::Return);
        debug_assert!(outcome.is_none(), "Return target emits its own returns");
        statements
    }
}

/// Wrap a tagged-return callback into a Go body producing the lowered Go
/// return shape. Returns `(go_return_type, body)`.
fn emit_return_adapter(
    planner: &mut Planner,
    inner_call: GoExpression,
    lisette_return_type: &Type,
) -> Option<(String, Vec<LoweredStatement>)> {
    let return_type = lisette_return_type;

    if return_type.is_result() {
        let shape = if return_type.ok_type().is_unit() {
            CallableReturnAbi::BareError
        } else {
            CallableReturnAbi::Result {
                payload: PayloadLayout::Packed,
            }
        };
        return Some(emit_shape_return_adapter(
            planner,
            inner_call,
            return_type,
            &shape,
            "res",
        ));
    }
    if return_type.is_partial() {
        let shape = CallableReturnAbi::Partial {
            payload: PayloadLayout::Packed,
        };
        return Some(emit_shape_return_adapter(
            planner,
            inner_call,
            return_type,
            &shape,
            "res",
        ));
    }
    if return_type.is_option() {
        let encoding = if planner.facts.is_nilable_go_type(&return_type.ok_type()) {
            OptionReturnAbi::Nullable
        } else {
            OptionReturnAbi::CommaOk {
                payload: PayloadLayout::Packed,
            }
        };
        let shape = CallableReturnAbi::Option(encoding);
        return Some(emit_shape_return_adapter(
            planner,
            inner_call,
            return_type,
            &shape,
            "opt",
        ));
    }
    if return_type.tuple_arity().is_some_and(|n| n >= 2) {
        return emit_tuple_return_adapter(planner, inner_call, return_type);
    }
    None
}

/// Returns `(go_return_type, body)` for a tagged result destructured into `shape`.
fn emit_shape_return_adapter(
    planner: &mut Planner,
    inner_call: GoExpression,
    return_type: &Type,
    shape: &CallableReturnAbi,
    prefix: &str,
) -> (String, Vec<LoweredStatement>) {
    let go_return = planner.render_lowered_return_ty(shape, return_type);
    let result = planner.fresh_var(Some(prefix));
    planner.declare(&result);
    let mut body = vec![define(result.clone(), inner_call)];
    body.extend(emit_lowered_result_return(
        planner,
        &GoExpression::name(result),
        return_type,
        shape,
    ));
    (go_return, body)
}

/// Arity-2+ tuple → Go multi-return. Each slot recurses through
/// `emit_return_adapter`, wrapping in an IIFE when the slot itself needs
/// adapter-style unwrapping.
fn emit_tuple_return_adapter(
    planner: &mut Planner,
    inner_call: GoExpression,
    return_type: &Type,
) -> Option<(String, Vec<LoweredStatement>)> {
    let tuple_params: Vec<Type> = match return_type {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => return None,
    };
    let arity = tuple_params.len();
    let tup = planner.fresh_var(Some("tup"));
    planner.declare(&tup);

    let mut body = vec![define(tup.clone(), inner_call)];
    let tup = GoExpression::name(tup);
    let mut ret_types: Vec<String> = Vec::with_capacity(arity);
    let mut field_exprs: Vec<GoExpression> = Vec::with_capacity(arity);

    for (i, slot_ty) in tuple_params.iter().enumerate() {
        let raw_field = field(&tup, TUPLE_FIELDS[i]);
        match emit_return_adapter(planner, raw_field.clone(), slot_ty) {
            Some((inner_ret, inner_body)) => {
                let sub = planner.fresh_var(Some("sub"));
                planner.declare(&sub);
                body.push(define(
                    sub.clone(),
                    GoExpression::immediate_call(
                        inner_ret.clone(),
                        LoweredBlock {
                            statements: inner_body,
                        },
                        FunctionLiteralLayout::MultiLine,
                    ),
                ));
                field_exprs.push(GoExpression::name(sub));
                ret_types.push(inner_ret);
            }
            None => {
                ret_types.push(planner.use_go_type(slot_ty));
                field_exprs.push(raw_field);
            }
        }
    }

    body.push(multi_value_return(field_exprs));
    Some((format!("({})", ret_types.join(", ")), body))
}

/// Wrap a Lisette tagged-shape function value into a Go closure that
/// presents the lowered Go ABI to callers. Identity when the return type
/// has no lowered shape.
pub(crate) fn emit_lisette_callback_wrapper(
    planner: &mut Planner,
    setup: &mut Vec<LoweredStatement>,
    fn_value: GoExpression,
    fn_type: &Type,
) -> GoExpression {
    let Type::Function(f) = fn_type else {
        return fn_value;
    };
    let params = &f.params;

    let return_type = f.return_type.as_ref();

    let (param_strs, arguments) = planner.build_wrapper_params(params);
    let params_str = param_strs.join(", ");

    let cb_var = planner.hoist_tmp_value_statement(setup, "cb", fn_value.clone());

    let mut prelude = Vec::new();
    let inner_args: Vec<GoExpression> = arguments
        .into_iter()
        .zip(params.iter())
        .map(|(argument, param)| lower_arg_to_tagged(planner, &mut prelude, argument, &param.ty))
        .collect();

    let call = GoExpression::call(GoExpression::name(cb_var), inner_args);

    // Option<fn> adaptation only fires in interface-method shims. Here
    // a closure-valued Option means the caller owns the nil check.
    if let Type::Nominal { id, params: ps, .. } = return_type
        && id == "Option"
        && let Some(inner) = ps.first()
        && matches!(inner.unwrap_forall(), Type::Function(_))
    {
        return fn_value;
    }

    let Some((go_ret, body)) = emit_return_adapter(planner, call, return_type) else {
        return fn_value;
    };

    prelude.extend(body);
    GoExpression::function_literal(
        params_str,
        go_ret,
        LoweredBlock {
            statements: prelude,
        },
        FunctionLiteralLayout::MultiLine,
    )
}

/// Wrap a lowered-return fn into a closure re-presenting the return in
/// `target_shape` (tagged when `None`). Pipes through tagged form so any
/// (arg, target) shape pair works.
pub(crate) fn emit_fn_arg_shape_adapter(
    planner: &mut Planner,
    setup: &mut Vec<LoweredStatement>,
    fn_value: GoExpression,
    arg_fn_type: &Type,
    arg_abi: &CallableReturnAbi,
    target_abi: &CallableReturnAbi,
) -> Option<GoExpression> {
    let params = arg_fn_type.get_function_params()?;
    let arg_ret = arg_fn_type.get_function_ret()?;

    let cb_var = planner.hoist_tmp_value_statement(setup, "cb", fn_value);
    let (param_strs, arguments) = planner.build_wrapper_params(params);
    let inner_call = GoExpression::call(GoExpression::name(cb_var), arguments);

    let outer_ret = planner.render_lowered_return_ty(target_abi, arg_ret);

    let body = if target_abi.is_passthrough() {
        planner.lower_abi_to_tagged_return(inner_call, arg_abi, arg_ret)
    } else {
        let (mut body, tagged) = planner.lower_abi_to_tagged(inner_call, arg_abi, arg_ret);
        body.extend(emit_lowered_result_return(
            planner, &tagged, arg_ret, target_abi,
        ));
        body
    };

    Some(GoExpression::function_literal(
        param_strs.join(", "),
        outer_ret,
        LoweredBlock { statements: body },
        FunctionLiteralLayout::MultiLine,
    ))
}

/// Convert a fn-typed wrapper arg from lowered Go ABI back to tagged for
/// the inner call. Identity for non-fn args and for fn args with no
/// lowered return.
pub(crate) fn lower_arg_to_tagged(
    planner: &mut Planner,
    prelude: &mut Vec<LoweredStatement>,
    argument: GoExpression,
    param_ty: &Type,
) -> GoExpression {
    let unwrapped = param_ty.unwrap_forall();
    let Type::Function(f) = unwrapped else {
        return argument;
    };
    let inner_params = &f.params;
    let inner_ret = f.return_type.as_ref();
    let Some(abi) = planner.classify_direct_emission(inner_ret) else {
        return argument;
    };

    let (inner_param_strs, inner_arguments) = planner.build_wrapper_params(inner_params);
    let inner_call = GoExpression::call(argument, inner_arguments);
    let tagged_ret = planner.use_go_type(inner_ret);

    let body = planner.lower_abi_to_tagged_return(inner_call, &abi, inner_ret);

    let tagged_var = planner.fresh_var(Some("tagged"));
    planner.declare(&tagged_var);
    prelude.push(define(
        tagged_var.clone(),
        GoExpression::function_literal(
            inner_param_strs.join(", "),
            tagged_ret,
            LoweredBlock { statements: body },
            FunctionLiteralLayout::MultiLine,
        ),
    ));
    GoExpression::name(tagged_var)
}

/// Tail return for packed `Partial` and `Tuple` ABIs. A flattened `Partial`
/// takes the generic wrapped-return path.
pub(crate) fn try_emit_lowered_tail_return(
    planner: &mut Planner,
    expression: &Expression,
) -> Option<Vec<LoweredStatement>> {
    let shape = planner.return_ctx().lowered_shape()?;
    match shape {
        CallableReturnAbi::Partial {
            payload: PayloadLayout::Packed,
        } => Some(emit_lowered_partial_tail(planner, expression)),
        CallableReturnAbi::Tuple { arity, .. } => {
            Some(emit_lowered_tuple_tail(planner, expression, arity))
        }
        _ => None,
    }
}

fn lowered_tail_fallback(
    planner: &mut Planner,
    expression: &Expression,
    return_ty: &Type,
    shape: &CallableReturnAbi,
    hoist_hint: Option<&str>,
) -> Vec<LoweredStatement> {
    let (mut statements, value) = planner
        .lower_value(expression, ExpressionContext::value())
        .into_parts();
    let value = match hoist_hint {
        Some(hint) => {
            GoExpression::name(planner.hoist_tmp_value_statement(&mut statements, hint, value))
        }
        None => value,
    };
    statements.extend(emit_lowered_result_return(
        planner, &value, return_ty, shape,
    ));
    statements
}

fn emit_lowered_tuple_tail(
    planner: &mut Planner,
    expression: &Expression,
    arity: usize,
) -> Vec<LoweredStatement> {
    use Expression;
    let return_ty = planner.return_ctx().expect_ty();
    if let Expression::Tuple { elements, .. } = expression
        && elements.len() == arity
    {
        let (mut statements, parts) = lowered_tuple_literal_values(planner, elements, &return_ty);
        statements.push(multi_value_return(parts));
        return statements;
    }

    lowered_tail_fallback(
        planner,
        expression,
        &return_ty,
        &CallableReturnAbi::Tuple { arity },
        Some("tup"),
    )
}

fn emit_lowered_partial_tail(
    planner: &mut Planner,
    expression: &Expression,
) -> Vec<LoweredStatement> {
    use Expression;
    let return_ty = planner.return_ctx().expect_ty();

    if let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
        && let Some(variant) = callee.as_partial_constructor()
    {
        let mut statements = Vec::new();
        let ret = match variant {
            "Ok" => {
                let (setup, v) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                multi_value_return(vec![v, GoExpression::nil()])
            }
            "Err" => {
                let (setup, e) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let ok_ty = planner.facts.peel_alias(&return_ty).ok_type();
                multi_value_return(vec![planner.zero_value_expression(&ok_ty), e])
            }
            "Both" => {
                let (setup_v, v) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup_v);
                let (setup_e, e) = planner
                    .lower_composite_value(&args[1], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup_e);
                multi_value_return(vec![v, e])
            }
            _ => unreachable!("as_partial_constructor only returns Ok/Err/Both"),
        };
        statements.push(ret);
        return statements;
    }

    lowered_tail_fallback(
        planner,
        expression,
        &return_ty,
        &CallableReturnAbi::Partial {
            payload: PayloadLayout::Packed,
        },
        None,
    )
}

/// `Some(x)`/`None` collapse to `x`/`nil`; other Option expressions
/// project at runtime.
fn lower_nullable_slot_value(
    planner: &mut Planner,
    expression: &Expression,
    slot_ty: &Type,
) -> ValuePlan {
    use Expression;
    if let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
        && let Some(kind) = callee.as_option_constructor()
    {
        return match kind {
            Ok(()) => {
                debug_assert_eq!(args.len(), 1, "Some(...) takes exactly one arg");
                planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .with_pure_constructor_evaluation()
            }
            Err(()) => ValuePlan::evaluated_literal(
                Vec::new(),
                "nil".to_string(),
                EvaluationEffect::PureCall,
            ),
        };
    }
    if expression.is_none_literal() {
        return ValuePlan::evaluated_literal(
            Vec::new(),
            "nil".to_string(),
            EvaluationEffect::PureCall,
        );
    }
    let value = planner.lower_value(expression, ExpressionContext::value());
    let inner = planner.use_go_type(&slot_ty.ok_type());
    value.map_expression_as_computed(|setup, value| {
        planner.plan_option_projection(setup, value, &inner, &LayoutBridge::Identity, false)
    })
}
