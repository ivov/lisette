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
use crate::patterns::matching::{PartialVariant, PreludeVariant, prelude_constructor};
use crate::plan::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define,
    expression_statement,
};
use crate::plan::go_expression::{BinaryOp, FunctionLiteralLayout};
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;

/// A bare `return v0, v1, ...` statement leaf.
pub(crate) fn multi_value_return(values: Vec<GoExpression>) -> Statement {
    LoweredStatement::Return(values).into()
}

/// An `if <condition> { <setup...> return <then_values...> }` tag-check leaf (no else).
pub(crate) fn tag_check(
    condition: GoExpression,
    setup: Vec<Statement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    tag_check_with_initializer(None, condition, setup, then_values)
}

pub(crate) fn tag_check_with_initializer(
    initializer: Option<Definition>,
    condition: GoExpression,
    setup: Vec<Statement>,
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
        BinaryOp::Eq,
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
        CallableReturnAbi::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_)) => {
            payload
        }
        CallableReturnAbi::Tuple { .. } => {
            unreachable!("a tuple return has its own emission path")
        }
        CallableReturnAbi::Tagged | CallableReturnAbi::Direct => {
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
        CallableReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
            vec![GoExpression::literal(value.to_string())]
        }
        _ => unreachable!("only Option's `None` lacks a payload"),
    }
}

pub(crate) fn lowered_payload_values(
    planner: &mut Planner,
    shape: &CallableReturnAbi,
    payload_ty: &Type,
    payload_expr: GoExpression,
) -> (Vec<Statement>, Vec<GoExpression>) {
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
    result_value: GoExpression,
    hint: &str,
    return_ty: &Type,
    shape: &CallableReturnAbi,
) -> Vec<Statement> {
    let mut statements = Vec::new();
    let p = &planner.stable_source(&mut statements, hint, result_value);
    let ok_ty = || planner.facts.peel_alias(return_ty).ok_type();
    let arms = match shape {
        CallableReturnAbi::BareError | CallableReturnAbi::Result { .. } => {
            let (ok_setup, ok_payload) =
                lowered_payload_values(planner, shape, &ok_ty(), field(p, "OkVal"));
            vec![
                tag_check(
                    has_tag(p, RESULT_OK_TAG),
                    ok_setup,
                    lowered_ok_values(shape, ok_payload),
                )
                .into(),
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
                )
                .into(),
                tag_check(
                    has_tag(p, PARTIAL_ERR_TAG),
                    Vec::new(),
                    lowered_err_values(planner, shape, return_ty, field(p, "ErrVal")),
                )
                .into(),
            ];
            statements.extend(both_setup);
            statements.push(multi_value_return(both_values));
            statements
        }
        CallableReturnAbi::Option(_) => {
            let (some_setup, some_payload) =
                lowered_payload_values(planner, shape, &ok_ty(), field(p, "SomeVal"));
            vec![
                tag_check(
                    has_tag(p, OPTION_SOME_TAG),
                    some_setup,
                    lowered_ok_values(shape, some_payload),
                )
                .into(),
                multi_value_return(lowered_none_values(planner, shape, return_ty)),
            ]
        }
        CallableReturnAbi::Tuple { .. } => emit_lowered_tuple_return(planner, p, return_ty),
        CallableReturnAbi::Tagged | CallableReturnAbi::Direct => {
            unreachable!("not a lowered Lisette return ABI")
        }
    };
    statements.extend(arms);
    statements
}

fn emit_lowered_tuple_return(
    planner: &mut Planner,
    result_value: &GoExpression,
    return_ty: &Type,
) -> Vec<Statement> {
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
) -> (Vec<Statement>, Vec<GoExpression>) {
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
) -> (Vec<Statement>, Vec<GoExpression>) {
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
    ) -> (Vec<Statement>, GoExpression) {
        let (wrap, outcome) =
            self.lower_abi_wrapping(raw_value, abi, result_ty, None, WrapperTarget::FreshSlot);
        (wrap, outcome.expect("wrapper produced no slot"))
    }

    /// Wrap a callable's physical Go result and return it in each wrapper branch.
    pub(crate) fn lower_abi_to_tagged_return(
        &mut self,
        raw_value: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> Vec<Statement> {
        let (statements, outcome) =
            self.lower_abi_wrapping(raw_value, abi, result_ty, None, WrapperTarget::Return);
        debug_assert!(outcome.is_none(), "Return target emits its own returns");
        statements
    }
}

/// Wrap a Lisette tagged-shape function value into a Go closure that
/// presents `target_abi` to callers. Identity when the target is not lowered.
pub(crate) fn emit_lisette_callback_wrapper(
    planner: &mut Planner,
    setup: &mut Vec<Statement>,
    fn_value: GoExpression,
    fn_type: &Type,
    target_abi: &CallableReturnAbi,
) -> GoExpression {
    let Type::Function(f) = fn_type else {
        return fn_value;
    };
    let params = &f.params;

    let return_type = f.return_type.as_ref();

    let (param_strs, arguments) = planner.build_wrapper_params(params);
    let params_str = param_strs;

    let cb_var = planner.hoist_tmp_value_statement(setup, "cb", fn_value.clone());

    let mut prelude = Vec::new();
    let inner_args: Vec<GoExpression> = arguments
        .into_iter()
        .zip(params.iter())
        .map(|(argument, param)| lower_arg_to_tagged(planner, &mut prelude, argument, &param.ty))
        .collect();

    let call = GoExpression::call(GoExpression::name(cb_var), inner_args);

    if !target_abi.is_lowered() {
        return fn_value;
    }
    let hint = match target_abi {
        CallableReturnAbi::Option(_) => "opt",
        CallableReturnAbi::Tuple { .. } => "tup",
        _ => "res",
    };
    prelude.extend(emit_lowered_result_return(
        planner,
        call,
        hint,
        return_type,
        target_abi,
    ));
    GoExpression::function_literal(
        params_str,
        planner.render_lowered_return_ty(target_abi, return_type),
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
    setup: &mut Vec<Statement>,
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
            planner, tagged, "res", arg_ret, target_abi,
        ));
        body
    };

    Some(GoExpression::function_literal(
        param_strs,
        outer_ret,
        LoweredBlock { statements: body },
        FunctionLiteralLayout::MultiLine,
    ))
}

/// Wrap a Go-void function value so it fills a slot whose result is a type parameter.
pub(crate) fn emit_unit_result_adapter(
    planner: &mut Planner,
    setup: &mut Vec<Statement>,
    fn_value: GoExpression,
    fn_type: &Type,
    slot_result: Option<String>,
) -> GoExpression {
    let Some(params) = fn_type.get_function_params() else {
        return fn_value;
    };
    let cb_var = planner.hoist_tmp_value_statement(setup, "cb", fn_value);
    let (param_strs, arguments) = planner.build_wrapper_params(params);
    let call = GoExpression::call(GoExpression::name(cb_var), arguments);
    let (result, tail) = match slot_result {
        Some(result) => (result, LoweredStatement::UnreachablePanic.into()),
        None => (
            "struct{}".to_string(),
            plain_return(GoExpression::empty_composite("struct{}".to_string())),
        ),
    };
    GoExpression::function_literal(
        param_strs,
        result,
        LoweredBlock {
            statements: vec![expression_statement(call), tail],
        },
        FunctionLiteralLayout::MultiLine,
    )
}

/// Convert a fn-typed wrapper arg from lowered Go ABI back to tagged for
/// the inner call. Identity for non-fn args and for fn args with no
/// lowered return.
pub(crate) fn lower_arg_to_tagged(
    planner: &mut Planner,
    prelude: &mut Vec<Statement>,
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
            inner_param_strs,
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
) -> Option<Vec<Statement>> {
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

fn emit_lowered_tuple_tail(
    planner: &mut Planner,
    expression: &Expression,
    arity: usize,
) -> Vec<Statement> {
    use Expression;
    let return_ty = planner.return_ctx().expect_ty();
    if let Expression::Tuple { elements, .. } = expression
        && elements.len() == arity
    {
        let (mut statements, parts) = lowered_tuple_literal_values(planner, elements, &return_ty);
        statements.push(multi_value_return(parts));
        return statements;
    }

    if let Some(plan) = planner.plan_call(expression)
        && plan.resolved.abi.result == (CallableReturnAbi::Tuple { arity })
        && planner.facts.peel_alias(&expression.get_type()) == planner.facts.peel_alias(&return_ty)
        && planner
            .go_result_bridge(&plan.resolved.abi, &return_ty)
            .is_none()
    {
        let (mut statements, call) = planner
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        statements.push(plain_return(call));
        return statements;
    }

    // Unlike the other tails, a name is copied to `tup` too.
    let (mut statements, value) = planner
        .lower_value(expression, ExpressionContext::value())
        .into_parts();
    let tup = planner.hoist_tmp_value_statement(&mut statements, "tup", value);
    statements.extend(emit_lowered_result_return(
        planner,
        GoExpression::name(tup),
        "tup",
        &return_ty,
        &CallableReturnAbi::Tuple { arity },
    ));
    statements
}

fn emit_lowered_partial_tail(planner: &mut Planner, expression: &Expression) -> Vec<Statement> {
    use Expression;
    let return_ty = planner.return_ctx().expect_ty();

    if let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
        && let Some(PreludeVariant::Partial(variant)) = prelude_constructor(callee)
    {
        let mut statements = Vec::new();
        let ret = match variant {
            PartialVariant::Ok => {
                let (setup, v) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                multi_value_return(vec![v, GoExpression::nil()])
            }
            PartialVariant::Err => {
                let (setup, e) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let ok_ty = planner.facts.peel_alias(&return_ty).ok_type();
                multi_value_return(vec![planner.zero_value_expression(&ok_ty), e])
            }
            PartialVariant::Both => {
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
        };
        statements.push(ret);
        return statements;
    }

    let (mut statements, value) = planner
        .lower_value(expression, ExpressionContext::value())
        .into_parts();
    statements.extend(emit_lowered_result_return(
        planner,
        value,
        "v",
        &return_ty,
        &CallableReturnAbi::Partial {
            payload: PayloadLayout::Packed,
        },
    ));
    statements
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
        && prelude_constructor(callee) == Some(PreludeVariant::Some)
    {
        debug_assert_eq!(args.len(), 1, "Some(...) takes exactly one arg");
        return planner
            .lower_composite_value(&args[0], ExpressionContext::value())
            .with_pure_constructor_evaluation();
    }
    if prelude_constructor(expression) == Some(PreludeVariant::None) {
        return ValuePlan::evaluated_literal(
            Vec::new(),
            "nil".to_string(),
            EvaluationEffect::PureCall,
        );
    }
    let value = planner.lower_value(expression, ExpressionContext::value());
    let inner = planner.use_go_type(&slot_ty.ok_type());
    value.map_expression(|setup, value| {
        planner.plan_option_projection(setup, value, &inner, &LayoutBridge::Identity, false)
    })
}
