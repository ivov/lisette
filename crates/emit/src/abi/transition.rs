use syntax::types::Type;

use crate::Planner;
use crate::Renderer;
use crate::abi::{AbiShape, tuple_element_types};
use crate::calls::go_interop::{TupleReturnLayout, WrapperTarget};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{
    OPTION_SOME_TAG, PARTIAL_ERR_TAG, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::expressions::emission::StagedExpression;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, ReturnForm, ReturnStatementPlan,
};
use crate::write_line;
use syntax::parse::TUPLE_FIELDS;

/// A bare `return v0, v1, ...` statement leaf.
pub(crate) fn multi_value_return(values: Vec<String>) -> LoweredStatement {
    LoweredStatement::Return(ReturnStatementPlan {
        form: ReturnForm::Multi { values },
    })
}

/// An `if <condition> { return <then_values...> }` tag-check leaf (no else).
pub(crate) fn tag_check(condition: String, then_values: Vec<String>) -> LoweredStatement {
    LoweredStatement::If(IfPlan {
        condition_setup: Vec::new(),
        condition,
        then_body: LoweredBlock {
            statements: vec![multi_value_return(then_values)],
        },
        else_arm: ElseArm::None,
    })
}

/// Render a lowered tagged-return destructure as Go text, for closure/value
/// contexts (adapters) that embed it in a string body rather than a statement
/// block.
pub(crate) fn render_lowered_result_return(
    planner: &mut Planner,
    output: &mut String,
    result_value: &str,
    return_ty: &Type,
    shape: &AbiShape,
) {
    let statements = emit_lowered_result_return(planner, result_value, return_ty, shape);
    let block = LoweredBlock { statements };
    Renderer.render_lowered_block(output, &block);
}

/// Idiomatic Go zero (`0`, `""`, `nil`, ...) for a lowered failure slot.
fn lowered_zero(planner: &mut Planner, ok_ty: &Type) -> String {
    let (zero, effects) = planner.zero_value(ok_ty);
    planner.absorb_effects(&effects);
    zero
}

/// The lowered Go-return values for an `Err`-with-payload failure, in the
/// enclosing function's lowered shape (e.g. `[zero, err]`).
pub(crate) fn lowered_err_values(
    planner: &mut Planner,
    shape: &AbiShape,
    return_ty: &Type,
    err_expr: &str,
) -> Vec<String> {
    match shape {
        AbiShape::BareError => vec![err_expr.to_string()],
        AbiShape::ResultTuple => {
            let ok_ty = planner.facts.peel_alias(return_ty).ok_type();
            vec![lowered_zero(planner, &ok_ty), err_expr.to_string()]
        }
        AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
            unreachable!("not reached for shapes with their own emission paths")
        }
        AbiShape::CommaOk | AbiShape::NullableReturn => {
            unreachable!("Option's failure constructor `None` carries no payload")
        }
    }
}

/// The lowered Go-return values for a success-constructor payload, in the
/// enclosing function's lowered shape (e.g. `[ok, "nil"]`).
pub(crate) fn lowered_ok_values(shape: &AbiShape, ok_expr: &str) -> Vec<String> {
    match shape {
        AbiShape::BareError => vec!["nil".to_string()],
        AbiShape::ResultTuple => vec![ok_expr.to_string(), "nil".to_string()],
        AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
            unreachable!("not reached for shapes with their own emission paths")
        }
        AbiShape::CommaOk => vec![ok_expr.to_string(), "true".to_string()],
        AbiShape::NullableReturn => vec![ok_expr.to_string()],
    }
}

/// The lowered Go-return values for a bare `None`, in an Option-shaped fn's
/// lowered shape (e.g. `[zero, "false"]`).
pub(crate) fn lowered_none_values(
    planner: &mut Planner,
    shape: &AbiShape,
    return_ty: &Type,
) -> Vec<String> {
    match shape {
        AbiShape::CommaOk => {
            let inner = planner.facts.peel_alias(return_ty).ok_type();
            vec![lowered_zero(planner, &inner), "false".to_string()]
        }
        AbiShape::NullableReturn => vec!["nil".to_string()],
        _ => unreachable!("only Option's `None` lacks a payload"),
    }
}

/// Destructure a Lisette tagged value into a lowered Go-tuple return,
/// as structured tag-check `IfPlan`s and `Return` leaves.
pub(crate) fn emit_lowered_result_return(
    planner: &mut Planner,
    result_value: &str,
    return_ty: &Type,
    shape: &AbiShape,
) -> Vec<LoweredStatement> {
    planner.require_stdlib();
    let p = result_value;
    let ok_zero = |planner: &mut Planner| {
        let ok_ty = planner.facts.peel_alias(return_ty).ok_type();
        lowered_zero(planner, &ok_ty)
    };
    match shape {
        AbiShape::BareError => vec![
            tag_check(
                format!("{p}.Tag == {RESULT_OK_TAG}"),
                vec!["nil".to_string()],
            ),
            multi_value_return(vec![format!("{p}.ErrVal")]),
        ],
        AbiShape::ResultTuple => {
            let zero = ok_zero(planner);
            vec![
                tag_check(
                    format!("{p}.Tag == {RESULT_OK_TAG}"),
                    vec![format!("{p}.OkVal"), "nil".to_string()],
                ),
                multi_value_return(vec![zero, format!("{p}.ErrVal")]),
            ]
        }
        AbiShape::PartialTuple => {
            let zero = ok_zero(planner);
            vec![
                tag_check(
                    format!("{p}.Tag == {PARTIAL_OK_TAG}"),
                    vec![format!("{p}.OkVal"), "nil".to_string()],
                ),
                tag_check(
                    format!("{p}.Tag == {PARTIAL_ERR_TAG}"),
                    vec![zero, format!("{p}.ErrVal")],
                ),
                multi_value_return(vec![format!("{p}.OkVal"), format!("{p}.ErrVal")]),
            ]
        }
        AbiShape::CommaOk => {
            let zero = ok_zero(planner);
            vec![
                tag_check(
                    format!("{p}.Tag == {OPTION_SOME_TAG}"),
                    vec![format!("{p}.SomeVal"), "true".to_string()],
                ),
                multi_value_return(vec![zero, "false".to_string()]),
            ]
        }
        AbiShape::NullableReturn => vec![
            tag_check(
                format!("{p}.Tag == {OPTION_SOME_TAG}"),
                vec![format!("{p}.SomeVal")],
            ),
            multi_value_return(vec!["nil".to_string()]),
        ],
        AbiShape::Tuple { arity } => {
            emit_lowered_tuple_return(planner, result_value, return_ty, *arity)
        }
    }
}

/// `Tuple` shape return: project each tuple field, unwrapping any
/// nullable-Option slot to its bare Go nilable.
fn emit_lowered_tuple_return(
    planner: &mut Planner,
    result_value: &str,
    return_ty: &Type,
    arity: usize,
) -> Vec<LoweredStatement> {
    let peeled = planner.facts.peel_alias(return_ty);
    let slot_tys = tuple_element_types(&peeled);
    let any_nullable = slot_tys.iter().any(|t| planner.facts.is_nullable_option(t));
    if !any_nullable {
        let fields: Vec<String> = (0..arity)
            .map(|i| format!("{}.{}", result_value, TUPLE_FIELDS[i]))
            .collect();
        return vec![multi_value_return(fields)];
    }
    let mut statements = Vec::new();
    let fields: Vec<String> = (0..arity)
        .map(|i| {
            let raw = format!("{}.{}", result_value, TUPLE_FIELDS[i]);
            slot_tys
                .get(i)
                .filter(|t| planner.facts.is_nullable_option(t))
                .map(|t| {
                    let inner = planner.go_type_string(&t.ok_type());
                    planner.plan_option_projection(&mut statements, &raw, "unwrap", &inner, false)
                })
                .unwrap_or(raw)
        })
        .collect();
    statements.push(multi_value_return(fields));
    statements
}

/// Wrap a lowered-callee `call_str` (Go-shaped return) into the Lisette
/// tagged shape declared by `result_ty`.
pub(crate) fn lower_callee_abi_wrapping(
    planner: &mut Planner,
    shape: &AbiShape,
    call_str: &str,
    result_ty: &Type,
) -> (Vec<LoweredStatement>, String) {
    match shape {
        AbiShape::PartialTuple => {
            let (statements, outcome) = planner.lower_partial_wrapping(
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
            );
            (statements, outcome.expect("wrapper produced no slot"))
        }
        AbiShape::CommaOk => {
            let (statements, outcome) = planner.lower_comma_ok_wrapping(
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
            );
            (statements, outcome.expect("wrapper produced no slot"))
        }
        AbiShape::NullableReturn => {
            let mut statements = Vec::new();
            let raw_var = planner.hoist_tmp_value_statement(&mut statements, "raw", call_str);
            let (wrap, outcome) =
                planner.lower_nil_check_option_wrap(&raw_var, result_ty, WrapperTarget::FreshSlot);
            statements.extend(wrap);
            (statements, outcome.expect("wrapper produced no slot"))
        }
        AbiShape::ResultTuple | AbiShape::BareError => {
            let (statements, outcome) = planner.lower_result_wrapping(
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
            );
            (statements, outcome.expect("wrapper produced no slot"))
        }
        AbiShape::Tuple { arity } => {
            let mut statements = Vec::new();
            let temps = planner.create_temp_vars("ret", *arity);
            statements.push(LoweredStatement::RawGo(format!(
                "{} := {}\n",
                temps.join(", "),
                call_str
            )));
            let slot_tys = tuple_element_types(&planner.facts.peel_alias(result_ty));
            let wrapped: Vec<String> = temps
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    slot_tys
                        .get(i)
                        .filter(|slot_ty| planner.facts.is_nullable_option(slot_ty))
                        .map(|slot_ty| {
                            planner.plan_nil_check_option_wrap(&mut statements, v, slot_ty)
                        })
                        .unwrap_or_else(|| v.clone())
                })
                .collect();
            let tuple = planner.plan_tuple_from_vars(&mut statements, &wrapped, result_ty);
            (statements, tuple)
        }
    }
}

/// Wrap a tagged-return callback into a Go body producing the lowered Go
/// return shape. Returns `(go_return_type, body)`.
pub(crate) fn emit_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    lisette_return_type: &Type,
) -> Option<(String, String)> {
    let return_type = lisette_return_type;

    if return_type.is_result() {
        planner.require_stdlib();
        return Some(emit_result_return_adapter(planner, inner_call, return_type));
    }
    if return_type.is_partial() {
        planner.require_stdlib();
        return Some(emit_partial_return_adapter(
            planner,
            inner_call,
            return_type,
        ));
    }
    if return_type.is_option() {
        planner.require_stdlib();
        return Some(emit_option_return_adapter(planner, inner_call, return_type));
    }
    if return_type.tuple_arity().is_some_and(|n| n >= 2) {
        planner.require_stdlib();
        return emit_tuple_return_adapter(planner, inner_call, return_type);
    }
    None
}

/// `Result<(), error>` → `error`; `Result<T, error>` → `(T, error)`.
fn emit_result_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let err_ty_str = planner.go_type_string(&err_ty);
    let res = planner.fresh_var(Some("res"));
    planner.declare(&res);

    let mut b = format!("{res} := {inner_call}\n");
    let ok_tag = RESULT_OK_TAG;
    if ok_ty.is_unit() {
        write_line!(
            b,
            "if {res}.Tag == {ok_tag} {{\nreturn nil\n}}\nreturn {res}.ErrVal"
        );
        return (err_ty_str, b);
    }
    let ok_ty_str = planner.go_type_string(&ok_ty);
    let ok_zero = lowered_zero(planner, &ok_ty);
    write_line!(
        b,
        "if {res}.Tag == {ok_tag} {{\nreturn {res}.OkVal, nil\n}}\n\
         return {ok_zero}, {res}.ErrVal"
    );
    (format!("({ok_ty_str}, {err_ty_str})"), b)
}

/// `Partial<T, error>` → `(T, error)`, distinguishing Ok/Err/both branches.
fn emit_partial_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let ok_ty_str = planner.go_type_string(&ok_ty);
    let err_ty_str = planner.go_type_string(&err_ty);
    let ok_zero = lowered_zero(planner, &ok_ty);
    let res = planner.fresh_var(Some("res"));
    planner.declare(&res);

    let b = format!(
        "{res} := {inner_call}\n\
         if {res}.Tag == {PARTIAL_OK_TAG} {{\nreturn {res}.OkVal, nil\n}}\n\
         if {res}.Tag == {PARTIAL_ERR_TAG} {{\nreturn {ok_zero}, {res}.ErrVal\n}}\n\
         return {res}.OkVal, {res}.ErrVal\n"
    );
    (format!("({ok_ty_str}, {err_ty_str})"), b)
}

/// `Option<fn>`/`Option<Ref<T>>`/`Option<Interface>` → bare nilable Go type
/// (collapsed because Go's nil already encodes absence). Other payloads use
/// the Go-idiomatic `(T, bool)` comma-ok convention.
fn emit_option_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let inner = return_type.ok_type();
    let some_tag = OPTION_SOME_TAG;
    let opt = planner.fresh_var(Some("opt"));
    planner.declare(&opt);

    let is_nilable = planner.facts.is_nilable_go_type(&inner);
    if is_nilable {
        let go_ret = planner.go_type_string(&inner);
        let b = format!(
            "{opt} := {inner_call}\n\
             if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal\n}}\n\
             return nil\n"
        );
        return (go_ret, b);
    }

    let inner_ty_str = planner.go_type_string(&inner);
    let inner_zero = lowered_zero(planner, &inner);
    let b = format!(
        "{opt} := {inner_call}\n\
         if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal, true\n}}\n\
         return {inner_zero}, false\n"
    );
    (format!("({inner_ty_str}, bool)"), b)
}

/// Arity-2+ tuple → Go multi-return. Each slot recurses through
/// `emit_return_adapter`, wrapping in an IIFE when the slot itself needs
/// adapter-style unwrapping.
fn emit_tuple_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
) -> Option<(String, String)> {
    let tuple_params: Vec<Type> = match return_type {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => return None,
    };
    let arity = tuple_params.len();
    let tup = planner.fresh_var(Some("tup"));
    planner.declare(&tup);

    let mut body = format!("{tup} := {inner_call}\n");
    let mut ret_types: Vec<String> = Vec::with_capacity(arity);
    let mut field_exprs: Vec<String> = Vec::with_capacity(arity);

    for (i, slot_ty) in tuple_params.iter().enumerate() {
        let raw_field = format!("{tup}.{}", TUPLE_FIELDS[i]);
        match emit_return_adapter(planner, &raw_field, slot_ty) {
            Some((inner_ret, inner_body)) => {
                let sub = planner.fresh_var(Some("sub"));
                planner.declare(&sub);
                body.push_str(&format!(
                    "{sub} := func() {inner_ret} {{\n{inner_body}}}()\n"
                ));
                field_exprs.push(sub);
                ret_types.push(inner_ret);
            }
            None => {
                ret_types.push(planner.go_type_string(slot_ty));
                field_exprs.push(raw_field);
            }
        }
    }

    body.push_str(&format!("return {}\n", field_exprs.join(", ")));
    Some((format!("({})", ret_types.join(", ")), body))
}

/// Wrap a Lisette tagged-shape function value into a Go closure that
/// presents the lowered Go ABI to callers. Identity when the return type
/// has no lowered shape.
pub(crate) fn emit_lisette_callback_wrapper(
    planner: &mut Planner,
    setup: &mut Vec<LoweredStatement>,
    fn_value: &str,
    fn_type: &Type,
) -> String {
    let Type::Function(f) = fn_type else {
        return fn_value.to_string();
    };
    let params = &f.params;

    let return_type = f.return_type.as_ref();

    let (param_strs, arg_names) = planner.build_wrapper_params(params);
    let params_str = param_strs.join(", ");

    let cb_var = planner.hoist_tmp_value_statement(setup, "cb", fn_value);

    let mut prelude = String::new();
    let inner_args: Vec<String> = arg_names
        .iter()
        .zip(params.iter())
        .map(|(name, param_ty)| lower_arg_to_tagged(planner, &mut prelude, name, param_ty))
        .collect();

    let call_str = format!("{}({})", cb_var, inner_args.join(", "));

    // Option<fn> adaptation only fires in interface-method shims. Here
    // a closure-valued Option means the caller owns the nil check.
    if let Type::Nominal { id, params: ps, .. } = return_type
        && id == "Option"
        && let Some(inner) = ps.first()
        && matches!(inner.unwrap_forall(), Type::Function(_))
    {
        return fn_value.to_string();
    }

    let adapter = emit_return_adapter(planner, &call_str, return_type);
    let Some((go_ret, body)) = adapter else {
        return fn_value.to_string();
    };

    format!("func({params_str}) {go_ret} {{\n{prelude}{body}}}")
}

/// Wrap a lowered-return fn into a closure re-presenting the return in
/// `target_shape` (tagged when `None`). Pipes through tagged form so any
/// (arg, target) shape pair works.
pub(crate) fn emit_fn_arg_shape_adapter(
    planner: &mut Planner,
    output: &mut String,
    fn_value: &str,
    arg_fn_type: &Type,
    arg_shape: &AbiShape,
    target_shape: Option<&AbiShape>,
) -> Option<String> {
    let params = arg_fn_type.get_function_params()?;
    let arg_ret = arg_fn_type.get_function_ret()?;

    let cb_var = planner.hoist_tmp_value(output, "cb", fn_value);
    let (param_strs, arg_names) = planner.build_wrapper_params(params);
    let inner_call = format!("{}({})", cb_var, arg_names.join(", "));

    let outer_ret = match target_shape {
        Some(shape) => planner.render_lowered_return_ty(shape, arg_ret),
        None => planner.go_type_string(arg_ret),
    };

    let (wrap_statements, tagged) =
        lower_callee_abi_wrapping(planner, arg_shape, &inner_call, arg_ret);
    let mut body = Renderer.render_setup(&wrap_statements);
    match target_shape {
        Some(shape) => render_lowered_result_return(planner, &mut body, &tagged, arg_ret, shape),
        None => write_line!(body, "return {}", tagged),
    }

    Some(format!(
        "func({}) {} {{\n{}}}",
        param_strs.join(", "),
        outer_ret,
        body
    ))
}

/// Convert a fn-typed wrapper arg from lowered Go ABI back to tagged for
/// the inner call. Identity for non-fn args and for fn args with no
/// lowered return.
pub(crate) fn lower_arg_to_tagged(
    planner: &mut Planner,
    prelude: &mut String,
    arg_name: &str,
    param_ty: &Type,
) -> String {
    let unwrapped = param_ty.unwrap_forall();
    let Type::Function(f) = unwrapped else {
        return arg_name.to_string();
    };
    let inner_params = &f.params;
    let inner_ret = f.return_type.as_ref();
    let Some(shape) = planner.classify_direct_emission(inner_ret) else {
        return arg_name.to_string();
    };

    let (inner_param_strs, inner_arg_names) = planner.build_wrapper_params(inner_params);
    let inner_call = format!("{}({})", arg_name, inner_arg_names.join(", "));
    let tagged_ret = planner.go_type_string(inner_ret);

    let (wrap_statements, result_var) =
        lower_callee_abi_wrapping(planner, &shape, &inner_call, inner_ret);
    let mut body = Renderer.render_setup(&wrap_statements);
    write_line!(body, "return {}", result_var);

    let tagged_var = planner.fresh_var(Some("tagged"));
    planner.declare(&tagged_var);
    write_line!(
        prelude,
        "{} := func({}) {} {{\n{}}}",
        tagged_var,
        inner_param_strs.join(", "),
        tagged_ret,
        body
    );
    tagged_var
}

/// Tail return for `PartialTuple` and `Tuple` ABIs. Returns `true` when this
/// path handled the emission.
pub(crate) fn try_emit_lowered_tail_return(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
) -> Option<Vec<LoweredStatement>> {
    let shape = planner.return_ctx().lowered_shape()?;
    match shape {
        AbiShape::PartialTuple => Some(emit_lowered_partial_tail(planner, expression)),
        AbiShape::Tuple { arity } => Some(emit_lowered_tuple_tail(planner, expression, arity)),
        _ => None,
    }
}

fn lowered_tail_fallback(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
    return_ty: &Type,
    shape: &AbiShape,
    hoist_hint: Option<&str>,
) -> Vec<LoweredStatement> {
    let (mut statements, value) = planner
        .lower_value(expression, ExpressionContext::value())
        .into_parts();
    let value = match hoist_hint {
        Some(hint) => planner.hoist_tmp_value_statement(&mut statements, hint, &value),
        None => value,
    };
    statements.extend(emit_lowered_result_return(
        planner, &value, return_ty, shape,
    ));
    statements
}

fn emit_lowered_tuple_tail(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
    arity: usize,
) -> Vec<LoweredStatement> {
    use syntax::ast::Expression;
    if let Expression::Tuple { elements, .. } = expression
        && elements.len() == arity
    {
        let return_ty = planner.return_ctx().expect_ty();
        let slot_tys = tuple_element_types(&planner.facts.peel_alias(&return_ty));
        let stages: Vec<StagedExpression> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| match slot_tys.get(i) {
                Some(slot_ty) if planner.facts.is_nullable_option(slot_ty) => {
                    let mut setup = Vec::new();
                    let value = lower_nullable_slot_value(planner, &mut setup, e, slot_ty);
                    StagedExpression::from_typed_setup(setup, value, e)
                }
                _ => planner.stage_composite(e, ExpressionContext::value()),
            })
            .collect();
        let (mut statements, parts) = planner.sequence_structured(stages, "_ret");
        statements.push(multi_value_return(parts));
        return statements;
    }

    let return_ty = planner.return_ctx().expect_ty();
    lowered_tail_fallback(
        planner,
        expression,
        &return_ty,
        &AbiShape::Tuple { arity },
        Some("tup"),
    )
}

fn emit_lowered_partial_tail(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
) -> Vec<LoweredStatement> {
    use syntax::ast::Expression;
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
                multi_value_return(vec![v, "nil".to_string()])
            }
            "Err" => {
                let (setup, e) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let ok_ty = planner.facts.peel_alias(&return_ty).ok_type();
                multi_value_return(vec![lowered_zero(planner, &ok_ty), e])
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
        &AbiShape::PartialTuple,
        None,
    )
}

/// `Some(x)`/`None` collapse to `x`/`nil`; other Option expressions
/// project at runtime.
fn lower_nullable_slot_value(
    planner: &mut Planner,
    setup: &mut Vec<LoweredStatement>,
    expression: &syntax::ast::Expression,
    slot_ty: &Type,
) -> String {
    use syntax::ast::Expression;
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
                let (slot_setup, value) = planner
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                setup.extend(slot_setup);
                value
            }
            Err(()) => "nil".to_string(),
        };
    }
    if expression.is_none_literal() {
        return "nil".to_string();
    }
    let (value_setup, value) = planner
        .lower_value(expression, ExpressionContext::value())
        .into_parts();
    setup.extend(value_setup);
    let inner = planner.go_type_string(&slot_ty.ok_type());
    planner.plan_option_projection(setup, &value, "unwrap", &inner, false)
}
