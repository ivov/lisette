use syntax::types::Type;

use crate::EmitEffects;
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
        directive: String::new(),
        form: ReturnForm::Multi { values },
    })
}

/// An `if <condition> { return <then_values...> }` tag-check leaf (no else).
pub(crate) fn tag_check(condition: String, then_values: Vec<String>) -> LoweredStatement {
    LoweredStatement::If(IfPlan {
        directive: String::new(),
        condition_setup: String::new(),
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
    fx: &mut EmitEffects,
) {
    let statements = emit_lowered_result_return(planner, result_value, return_ty, shape, fx);
    let block = LoweredBlock { statements };
    Renderer.render_lowered_block(output, &block);
}

/// The lowered Go-return values for an `Err`-with-payload failure, in the
/// enclosing function's lowered shape (e.g. `["*new(T)", err]`).
pub(crate) fn lowered_err_values(
    planner: &mut Planner,
    shape: &AbiShape,
    return_ty: &Type,
    err_expr: &str,
    fx: &mut EmitEffects,
) -> Vec<String> {
    match shape {
        AbiShape::BareError => vec![err_expr.to_string()],
        AbiShape::ResultTuple => {
            let ok_ty = planner.facts.peel_alias(return_ty).ok_type();
            let ok_ty_str = planner.go_type_string(&ok_ty, fx);
            vec![format!("*new({})", ok_ty_str), err_expr.to_string()]
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
/// lowered shape (e.g. `["*new(T)", "false"]`).
pub(crate) fn lowered_none_values(
    planner: &mut Planner,
    shape: &AbiShape,
    return_ty: &Type,
    fx: &mut EmitEffects,
) -> Vec<String> {
    match shape {
        AbiShape::CommaOk => {
            let inner = planner.facts.peel_alias(return_ty).ok_type();
            let inner_str = planner.go_type_string(&inner, fx);
            vec![format!("*new({})", inner_str), "false".to_string()]
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
    fx: &mut EmitEffects,
) -> Vec<LoweredStatement> {
    fx.require_stdlib();
    let p = result_value;
    let ok_ty_str = |planner: &mut Planner, fx: &mut EmitEffects| {
        let ok_ty = planner.facts.peel_alias(return_ty).ok_type();
        planner.go_type_string(&ok_ty, fx)
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
            let t = ok_ty_str(planner, fx);
            vec![
                tag_check(
                    format!("{p}.Tag == {RESULT_OK_TAG}"),
                    vec![format!("{p}.OkVal"), "nil".to_string()],
                ),
                multi_value_return(vec![format!("*new({t})"), format!("{p}.ErrVal")]),
            ]
        }
        AbiShape::PartialTuple => {
            let t = ok_ty_str(planner, fx);
            vec![
                tag_check(
                    format!("{p}.Tag == {PARTIAL_OK_TAG}"),
                    vec![format!("{p}.OkVal"), "nil".to_string()],
                ),
                tag_check(
                    format!("{p}.Tag == {PARTIAL_ERR_TAG}"),
                    vec![format!("*new({t})"), format!("{p}.ErrVal")],
                ),
                multi_value_return(vec![format!("{p}.OkVal"), format!("{p}.ErrVal")]),
            ]
        }
        AbiShape::CommaOk => {
            let t = ok_ty_str(planner, fx);
            vec![
                tag_check(
                    format!("{p}.Tag == {OPTION_SOME_TAG}"),
                    vec![format!("{p}.SomeVal"), "true".to_string()],
                ),
                multi_value_return(vec![format!("*new({t})"), "false".to_string()]),
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
            emit_lowered_tuple_return(planner, result_value, return_ty, *arity, fx)
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
    fx: &mut EmitEffects,
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
    let mut setup = String::new();
    let fields: Vec<String> = (0..arity)
        .map(|i| {
            let raw = format!("{}.{}", result_value, TUPLE_FIELDS[i]);
            slot_tys
                .get(i)
                .filter(|t| planner.facts.is_nullable_option(t))
                .map(|t| {
                    let inner = planner.go_type_string(&t.ok_type(), fx);
                    planner.emit_option_projection(&mut setup, &raw, "unwrap", &inner, false, fx)
                })
                .unwrap_or(raw)
        })
        .collect();
    let mut statements = Vec::new();
    if !setup.is_empty() {
        statements.push(LoweredStatement::RawGo(setup));
    }
    statements.push(multi_value_return(fields));
    statements
}

/// Wrap a lowered-callee `call_str` (Go-shaped return) into the Lisette
/// tagged shape declared by `result_ty`.
pub(crate) fn emit_callee_abi_wrapping(
    planner: &mut Planner,
    output: &mut String,
    shape: &AbiShape,
    call_str: &str,
    result_ty: &Type,
    fx: &mut EmitEffects,
) -> String {
    match shape {
        AbiShape::PartialTuple => planner
            .emit_partial_wrapping(
                output,
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
                fx,
            )
            .expect("wrapper produced no slot"),
        AbiShape::CommaOk => planner
            .emit_comma_ok_wrapping(
                output,
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
                fx,
            )
            .expect("wrapper produced no slot"),
        AbiShape::NullableReturn => {
            let raw_var = planner.hoist_tmp_value(output, "raw", call_str);
            planner
                .emit_nil_check_option_wrap(
                    output,
                    &raw_var,
                    result_ty,
                    WrapperTarget::FreshSlot,
                    fx,
                )
                .expect("wrapper produced no slot")
        }
        AbiShape::ResultTuple | AbiShape::BareError => planner
            .emit_result_wrapping(
                output,
                call_str,
                result_ty,
                TupleReturnLayout::Packed,
                WrapperTarget::FreshSlot,
                fx,
            )
            .expect("wrapper produced no slot"),
        AbiShape::Tuple { arity } => {
            let temps = planner.create_temp_vars("ret", *arity);
            write_line!(output, "{} := {}", temps.join(", "), call_str);
            let slot_tys = tuple_element_types(&planner.facts.peel_alias(result_ty));
            let wrapped: Vec<String> = temps
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    slot_tys
                        .get(i)
                        .filter(|slot_ty| planner.facts.is_nullable_option(slot_ty))
                        .map(|slot_ty| {
                            planner
                                .emit_nil_check_option_wrap(
                                    output,
                                    v,
                                    slot_ty,
                                    WrapperTarget::FreshSlot,
                                    fx,
                                )
                                .expect("wrapper produced no slot")
                        })
                        .unwrap_or_else(|| v.clone())
                })
                .collect();
            planner.emit_tuple_from_vars(output, &wrapped, result_ty, fx)
        }
    }
}

/// Wrap a tagged-return callback into a Go body producing the lowered Go
/// return shape. Returns `(go_return_type, body)`.
pub(crate) fn emit_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    lisette_return_type: &Type,
    fx: &mut EmitEffects,
) -> Option<(String, String)> {
    let return_type = lisette_return_type;

    if return_type.is_result() {
        fx.require_stdlib();
        return Some(emit_result_return_adapter(
            planner,
            inner_call,
            return_type,
            fx,
        ));
    }
    if return_type.is_partial() {
        fx.require_stdlib();
        return Some(emit_partial_return_adapter(
            planner,
            inner_call,
            return_type,
            fx,
        ));
    }
    if return_type.is_option() {
        fx.require_stdlib();
        return Some(emit_option_return_adapter(
            planner,
            inner_call,
            return_type,
            fx,
        ));
    }
    if return_type.tuple_arity().is_some_and(|n| n >= 2) {
        fx.require_stdlib();
        return emit_tuple_return_adapter(planner, inner_call, return_type, fx);
    }
    None
}

/// `Result<(), error>` → `error`; `Result<T, error>` → `(T, error)`.
fn emit_result_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
    fx: &mut EmitEffects,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let err_ty_str = planner.go_type_string(&err_ty, fx);
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
    let ok_ty_str = planner.go_type_string(&ok_ty, fx);
    write_line!(
        b,
        "if {res}.Tag == {ok_tag} {{\nreturn {res}.OkVal, nil\n}}\n\
         return *new({ok_ty_str}), {res}.ErrVal"
    );
    (format!("({ok_ty_str}, {err_ty_str})"), b)
}

/// `Partial<T, error>` → `(T, error)`, distinguishing Ok/Err/both branches.
fn emit_partial_return_adapter(
    planner: &mut Planner,
    inner_call: &str,
    return_type: &Type,
    fx: &mut EmitEffects,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let ok_ty_str = planner.go_type_string(&ok_ty, fx);
    let err_ty_str = planner.go_type_string(&err_ty, fx);
    let res = planner.fresh_var(Some("res"));
    planner.declare(&res);

    let b = format!(
        "{res} := {inner_call}\n\
         if {res}.Tag == {PARTIAL_OK_TAG} {{\nreturn {res}.OkVal, nil\n}}\n\
         if {res}.Tag == {PARTIAL_ERR_TAG} {{\nreturn *new({ok_ty_str}), {res}.ErrVal\n}}\n\
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
    fx: &mut EmitEffects,
) -> (String, String) {
    let inner = return_type.ok_type();
    let some_tag = OPTION_SOME_TAG;
    let opt = planner.fresh_var(Some("opt"));
    planner.declare(&opt);

    let is_nilable = planner.facts.is_nilable_go_type(&inner);
    if is_nilable {
        let go_ret = planner.go_type_string(&inner, fx);
        let b = format!(
            "{opt} := {inner_call}\n\
             if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal\n}}\n\
             return nil\n"
        );
        return (go_ret, b);
    }

    let inner_ty_str = planner.go_type_string(&inner, fx);
    let b = format!(
        "{opt} := {inner_call}\n\
         if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal, true\n}}\n\
         return *new({inner_ty_str}), false\n"
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
    fx: &mut EmitEffects,
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
        match emit_return_adapter(planner, &raw_field, slot_ty, fx) {
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
                ret_types.push(planner.go_type_string(slot_ty, fx));
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
    output: &mut String,
    fn_value: &str,
    fn_type: &Type,
    fx: &mut EmitEffects,
) -> String {
    let Type::Function(f) = fn_type else {
        return fn_value.to_string();
    };
    let params = &f.params;

    let return_type = f.return_type.as_ref();

    let (param_strs, arg_names) = planner.build_wrapper_params(params, fx);
    let params_str = param_strs.join(", ");

    let cb_var = planner.hoist_tmp_value(output, "cb", fn_value);

    let mut prelude = String::new();
    let inner_args: Vec<String> = arg_names
        .iter()
        .zip(params.iter())
        .map(|(name, param_ty)| lower_arg_to_tagged(planner, &mut prelude, name, param_ty, fx))
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

    let adapter = emit_return_adapter(planner, &call_str, return_type, fx);
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
    fx: &mut EmitEffects,
) -> Option<String> {
    let params = arg_fn_type.get_function_params()?;
    let arg_ret = arg_fn_type.get_function_ret()?;

    let cb_var = planner.hoist_tmp_value(output, "cb", fn_value);
    let (param_strs, arg_names) = planner.build_wrapper_params(params, fx);
    let inner_call = format!("{}({})", cb_var, arg_names.join(", "));

    let outer_ret = match target_shape {
        Some(shape) => planner.render_lowered_return_ty(shape, arg_ret, fx),
        None => planner.go_type_string(arg_ret, fx),
    };

    let mut body = String::new();
    let tagged = emit_callee_abi_wrapping(planner, &mut body, arg_shape, &inner_call, arg_ret, fx);
    match target_shape {
        Some(shape) => {
            render_lowered_result_return(planner, &mut body, &tagged, arg_ret, shape, fx)
        }
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
    fx: &mut EmitEffects,
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

    let (inner_param_strs, inner_arg_names) = planner.build_wrapper_params(inner_params, fx);
    let inner_call = format!("{}({})", arg_name, inner_arg_names.join(", "));
    let tagged_ret = planner.go_type_string(inner_ret, fx);

    let mut body = String::new();
    let result_var =
        emit_callee_abi_wrapping(planner, &mut body, &shape, &inner_call, inner_ret, fx);
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
    fx: &mut EmitEffects,
) -> Option<Vec<LoweredStatement>> {
    let shape = planner.return_ctx().lowered_shape()?;
    match shape {
        AbiShape::PartialTuple => Some(emit_lowered_partial_tail(planner, expression, fx)),
        AbiShape::Tuple { arity } => Some(emit_lowered_tuple_tail(planner, expression, arity, fx)),
        _ => None,
    }
}

/// String-context bridge over `try_emit_lowered_tail_return`.
pub(crate) fn render_lowered_tail_return(
    planner: &mut Planner,
    output: &mut String,
    expression: &syntax::ast::Expression,
    fx: &mut EmitEffects,
) -> bool {
    match try_emit_lowered_tail_return(planner, expression, fx) {
        Some(statements) => {
            let block = LoweredBlock { statements };
            Renderer.render_lowered_block(output, &block);
            true
        }
        None => false,
    }
}

fn emit_lowered_tuple_tail(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
    arity: usize,
    fx: &mut EmitEffects,
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
            .map(|(i, e)| {
                let mut setup = String::new();
                let value = match slot_tys.get(i) {
                    Some(slot_ty) if planner.facts.is_nullable_option(slot_ty) => {
                        emit_nullable_slot_value(planner, &mut setup, e, slot_ty, fx)
                    }
                    _ => {
                        planner.emit_composite_value(&mut setup, e, ExpressionContext::value(), fx)
                    }
                };
                StagedExpression::new(setup, value, e)
            })
            .collect();
        let (mut statements, parts) = planner.sequence_structured(stages, "_ret");
        statements.push(multi_value_return(parts));
        return statements;
    }

    let return_ty = planner.return_ctx().expect_ty();
    let mut setup = String::new();
    let value = planner.emit_value(&mut setup, expression, ExpressionContext::value(), fx);
    let temp = planner.hoist_tmp_value(&mut setup, "tup", &value);
    let mut statements = Vec::new();
    if !setup.is_empty() {
        statements.push(LoweredStatement::RawGo(setup));
    }
    statements.extend(emit_lowered_result_return(
        planner,
        &temp,
        &return_ty,
        &AbiShape::Tuple { arity },
        fx,
    ));
    statements
}

fn emit_lowered_partial_tail(
    planner: &mut Planner,
    expression: &syntax::ast::Expression,
    fx: &mut EmitEffects,
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
        let mut setup = String::new();
        let ret = match variant {
            "Ok" => {
                let v = planner.emit_composite_value(
                    &mut setup,
                    &args[0],
                    ExpressionContext::value(),
                    fx,
                );
                multi_value_return(vec![v, "nil".to_string()])
            }
            "Err" => {
                let e = planner.emit_composite_value(
                    &mut setup,
                    &args[0],
                    ExpressionContext::value(),
                    fx,
                );
                let ok_ty = planner.facts.peel_alias(&return_ty).ok_type();
                let ok_ty_str = planner.go_type_string(&ok_ty, fx);
                multi_value_return(vec![format!("*new({})", ok_ty_str), e])
            }
            "Both" => {
                let v = planner.emit_composite_value(
                    &mut setup,
                    &args[0],
                    ExpressionContext::value(),
                    fx,
                );
                let e = planner.emit_composite_value(
                    &mut setup,
                    &args[1],
                    ExpressionContext::value(),
                    fx,
                );
                multi_value_return(vec![v, e])
            }
            _ => unreachable!("as_partial_constructor only returns Ok/Err/Both"),
        };
        let mut statements = Vec::new();
        if !setup.is_empty() {
            statements.push(LoweredStatement::RawGo(setup));
        }
        statements.push(ret);
        return statements;
    }

    let mut setup = String::new();
    let value = planner.emit_value(&mut setup, expression, ExpressionContext::value(), fx);
    let mut statements = Vec::new();
    if !setup.is_empty() {
        statements.push(LoweredStatement::RawGo(setup));
    }
    statements.extend(emit_lowered_result_return(
        planner,
        &value,
        &return_ty,
        &AbiShape::PartialTuple,
        fx,
    ));
    statements
}

/// `Some(x)`/`None` collapse to `x`/`nil`; other Option expressions
/// project at runtime.
fn emit_nullable_slot_value(
    planner: &mut Planner,
    output: &mut String,
    expression: &syntax::ast::Expression,
    slot_ty: &Type,
    fx: &mut EmitEffects,
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
                planner.emit_composite_value(output, &args[0], ExpressionContext::value(), fx)
            }
            Err(()) => "nil".to_string(),
        };
    }
    if expression.is_none_literal() {
        return "nil".to_string();
    }
    let value = planner.emit_value(output, expression, ExpressionContext::value(), fx);
    let inner = planner.go_type_string(&slot_ty.ok_type(), fx);
    planner.emit_option_projection(output, &value, "unwrap", &inner, false, fx)
}
