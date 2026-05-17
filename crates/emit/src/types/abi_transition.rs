use syntax::types::Type;

use crate::Emitter;
use crate::calls::go_interop::WrapperTarget;
use crate::control_flow::fallible::{
    OPTION_SOME_TAG, PARTIAL_ERR_TAG, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::expressions::context::ExpressionContext;
use crate::types::abi::{AbiShape, tuple_element_types};
use crate::write_line;
use syntax::parse::TUPLE_FIELDS;

/// Lower an `Err`-with-payload value into the early-return body for the
/// enclosing function's lowered shape.
pub(crate) fn format_lowered_err_return(
    emitter: &mut Emitter,
    shape: &AbiShape,
    return_ty: &Type,
    err_expr: &str,
) -> String {
    match shape {
        AbiShape::BareError => format!("return {}", err_expr),
        AbiShape::ResultTuple => {
            let ok_ty = emitter.facts.peel_alias(return_ty).ok_type();
            let ok_ty_str = emitter.go_type_as_string(&ok_ty);
            format!("return *new({}), {}", ok_ty_str, err_expr)
        }
        AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
            unreachable!("not reached for shapes with their own emission paths")
        }
        AbiShape::CommaOk | AbiShape::NullableReturn => {
            unreachable!("Option's failure constructor `None` carries no payload")
        }
    }
}

/// Lower a success-constructor payload into the tail-return body.
pub(crate) fn format_lowered_ok_return(shape: &AbiShape, ok_expr: &str) -> String {
    match shape {
        AbiShape::BareError => "return nil".to_string(),
        AbiShape::ResultTuple => format!("return {}, nil", ok_expr),
        AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
            unreachable!("not reached for shapes with their own emission paths")
        }
        AbiShape::CommaOk => format!("return {}, true", ok_expr),
        AbiShape::NullableReturn => format!("return {}", ok_expr),
    }
}

/// Lower a bare `None` into the early-return body for an Option-shaped fn.
pub(crate) fn format_lowered_none_return(
    emitter: &mut Emitter,
    shape: &AbiShape,
    return_ty: &Type,
) -> String {
    match shape {
        AbiShape::CommaOk => {
            let inner = emitter.facts.peel_alias(return_ty).ok_type();
            let inner_str = emitter.go_type_as_string(&inner);
            format!("return *new({}), false", inner_str)
        }
        AbiShape::NullableReturn => "return nil".to_string(),
        _ => unreachable!("only Option's `None` lacks a payload"),
    }
}

/// Destructure a Lisette tagged value into a lowered Go-tuple return.
pub(crate) fn emit_lowered_result_return(
    emitter: &mut Emitter,
    output: &mut String,
    result_value: &str,
    return_ty: &Type,
    shape: &AbiShape,
) {
    emitter.requirements.require_stdlib();
    let ok_ty_str = match shape {
        AbiShape::ResultTuple | AbiShape::PartialTuple | AbiShape::CommaOk => {
            let ok_ty = emitter.facts.peel_alias(return_ty).ok_type();
            Some(emitter.go_type_as_string(&ok_ty))
        }
        _ => None,
    };
    match shape {
        AbiShape::BareError => {
            write_line!(
                output,
                "if {p}.Tag == {ok} {{\nreturn nil\n}}\nreturn {p}.ErrVal",
                p = result_value,
                ok = RESULT_OK_TAG,
            );
        }
        AbiShape::ResultTuple => {
            let t = ok_ty_str.as_deref().unwrap();
            write_line!(
                output,
                "if {p}.Tag == {ok} {{\nreturn {p}.OkVal, nil\n}}\nreturn *new({t}), {p}.ErrVal",
                p = result_value,
                ok = RESULT_OK_TAG,
            );
        }
        AbiShape::PartialTuple => {
            let t = ok_ty_str.as_deref().unwrap();
            write_line!(
                output,
                "if {p}.Tag == {ok} {{\nreturn {p}.OkVal, nil\n}}\n\
                 if {p}.Tag == {err} {{\nreturn *new({t}), {p}.ErrVal\n}}\n\
                 return {p}.OkVal, {p}.ErrVal",
                p = result_value,
                ok = PARTIAL_OK_TAG,
                err = PARTIAL_ERR_TAG,
            );
        }
        AbiShape::CommaOk => {
            let t = ok_ty_str.as_deref().unwrap();
            write_line!(
                output,
                "if {p}.Tag == {some} {{\nreturn {p}.SomeVal, true\n}}\n\
                 return *new({t}), false",
                p = result_value,
                some = OPTION_SOME_TAG,
            );
        }
        AbiShape::NullableReturn => {
            write_line!(
                output,
                "if {p}.Tag == {some} {{\nreturn {p}.SomeVal\n}}\nreturn nil",
                p = result_value,
                some = OPTION_SOME_TAG,
            );
        }
        AbiShape::Tuple { arity } => {
            emit_lowered_tuple_return(emitter, output, result_value, return_ty, *arity);
        }
    }
}

/// `Tuple` shape return: project each tuple field, unwrapping any
/// nullable-Option slot to its bare Go nilable.
fn emit_lowered_tuple_return(
    emitter: &mut Emitter,
    output: &mut String,
    result_value: &str,
    return_ty: &Type,
    arity: usize,
) {
    let peeled = emitter.facts.peel_alias(return_ty);
    let slot_tys = tuple_element_types(&peeled);
    let any_nullable = slot_tys.iter().any(|t| emitter.facts.is_nullable_option(t));
    if !any_nullable {
        let fields: Vec<String> = (0..arity)
            .map(|i| format!("{}.{}", result_value, syntax::parse::TUPLE_FIELDS[i]))
            .collect();
        write_line!(output, "return {}", fields.join(", "));
        return;
    }
    let fields: Vec<String> = (0..arity)
        .map(|i| {
            let raw = format!("{}.{}", result_value, syntax::parse::TUPLE_FIELDS[i]);
            slot_tys
                .get(i)
                .filter(|t| emitter.facts.is_nullable_option(t))
                .map(|t| {
                    let inner = emitter.go_type_as_string(&t.ok_type());
                    emitter.emit_option_projection(output, &raw, "unwrap", &inner, false)
                })
                .unwrap_or(raw)
        })
        .collect();
    write_line!(output, "return {}", fields.join(", "));
}

/// Wrap a lowered-callee `call_str` (Go-shaped return) into the Lisette
/// tagged shape declared by `result_ty`.
pub(crate) fn emit_callee_abi_wrapping(
    emitter: &mut Emitter,
    output: &mut String,
    shape: &AbiShape,
    call_str: &str,
    result_ty: &Type,
) -> String {
    match shape {
        AbiShape::PartialTuple => emitter
            .emit_partial_wrapping(output, call_str, result_ty, WrapperTarget::FreshSlot)
            .expect("wrapper produced no slot"),
        AbiShape::CommaOk => emitter
            .emit_comma_ok_wrapping(output, call_str, result_ty, false, WrapperTarget::FreshSlot)
            .expect("wrapper produced no slot"),
        AbiShape::NullableReturn => {
            let raw_var = emitter.hoist_tmp_value(output, "raw", call_str);
            emitter
                .emit_nil_check_option_wrap(output, &raw_var, result_ty, WrapperTarget::FreshSlot)
                .expect("wrapper produced no slot")
        }
        AbiShape::ResultTuple | AbiShape::BareError => emitter
            .emit_result_wrapping(output, call_str, result_ty, WrapperTarget::FreshSlot)
            .expect("wrapper produced no slot"),
        AbiShape::Tuple { arity } => {
            let temps = emitter.create_temp_vars("ret", *arity);
            write_line!(output, "{} := {}", temps.join(", "), call_str);
            let slot_tys = tuple_element_types(&emitter.facts.peel_alias(result_ty));
            let wrapped: Vec<String> = temps
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    slot_tys
                        .get(i)
                        .filter(|slot_ty| emitter.facts.is_nullable_option(slot_ty))
                        .map(|slot_ty| {
                            emitter
                                .emit_nil_check_option_wrap(
                                    output,
                                    v,
                                    slot_ty,
                                    WrapperTarget::FreshSlot,
                                )
                                .expect("wrapper produced no slot")
                        })
                        .unwrap_or_else(|| v.clone())
                })
                .collect();
            emitter.emit_tuple_from_vars(output, &wrapped, result_ty)
        }
    }
}

/// Wrap a Lisette tagged-return callback `inner_call` into a Go function
/// body that produces the lowered Go return shape. Returns `(go_return_type,
/// body)` so callers can build the surrounding closure header.
pub(crate) fn emit_return_adapter(
    emitter: &mut Emitter,
    inner_call: &str,
    lisette_return_type: &Type,
) -> Option<(String, String)> {
    let return_type = lisette_return_type;

    if return_type.is_result() {
        emitter.requirements.require_stdlib();
        return Some(emit_result_return_adapter(emitter, inner_call, return_type));
    }
    if return_type.is_partial() {
        emitter.requirements.require_stdlib();
        return Some(emit_partial_return_adapter(
            emitter,
            inner_call,
            return_type,
        ));
    }
    if return_type.is_option() {
        emitter.requirements.require_stdlib();
        return Some(emit_option_return_adapter(emitter, inner_call, return_type));
    }
    if return_type.tuple_arity().is_some_and(|n| n >= 2) {
        emitter.requirements.require_stdlib();
        return emit_tuple_return_adapter(emitter, inner_call, return_type);
    }
    None
}

/// `Result<(), error>` → `error`; `Result<T, error>` → `(T, error)`.
fn emit_result_return_adapter(
    emitter: &mut Emitter,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let err_ty_str = emitter.go_type_as_string(&err_ty);
    let res = emitter.fresh_var(Some("res"));
    emitter.declare(&res);

    let mut b = format!("{res} := {inner_call}\n");
    let ok_tag = RESULT_OK_TAG;
    if ok_ty.is_unit() {
        write_line!(
            b,
            "if {res}.Tag == {ok_tag} {{\nreturn nil\n}}\nreturn {res}.ErrVal"
        );
        return (err_ty_str, b);
    }
    let ok_ty_str = emitter.go_type_as_string(&ok_ty);
    write_line!(
        b,
        "if {res}.Tag == {ok_tag} {{\nreturn {res}.OkVal, nil\n}}\n\
         return *new({ok_ty_str}), {res}.ErrVal"
    );
    (format!("({ok_ty_str}, {err_ty_str})"), b)
}

/// `Partial<T, error>` → `(T, error)`, distinguishing Ok/Err/both branches.
fn emit_partial_return_adapter(
    emitter: &mut Emitter,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let ok_ty = return_type.ok_type();
    let err_ty = return_type.err_type();
    let ok_ty_str = emitter.go_type_as_string(&ok_ty);
    let err_ty_str = emitter.go_type_as_string(&err_ty);
    let res = emitter.fresh_var(Some("res"));
    emitter.declare(&res);

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
    emitter: &mut Emitter,
    inner_call: &str,
    return_type: &Type,
) -> (String, String) {
    let inner = return_type.ok_type();
    let some_tag = OPTION_SOME_TAG;
    let opt = emitter.fresh_var(Some("opt"));
    emitter.declare(&opt);

    let is_nilable = emitter.facts.is_nilable_go_type(&inner);
    if is_nilable {
        let go_ret = emitter.go_type_as_string(&inner);
        let b = format!(
            "{opt} := {inner_call}\n\
             if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal\n}}\n\
             return nil\n"
        );
        return (go_ret, b);
    }

    let inner_ty_str = emitter.go_type_as_string(&inner);
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
    emitter: &mut Emitter,
    inner_call: &str,
    return_type: &Type,
) -> Option<(String, String)> {
    let tuple_params: Vec<Type> = match return_type {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => return None,
    };
    let arity = tuple_params.len();
    let tup = emitter.fresh_var(Some("tup"));
    emitter.declare(&tup);

    let mut body = format!("{tup} := {inner_call}\n");
    let mut ret_types: Vec<String> = Vec::with_capacity(arity);
    let mut field_exprs: Vec<String> = Vec::with_capacity(arity);

    for (i, slot_ty) in tuple_params.iter().enumerate() {
        let raw_field = format!("{tup}.{}", TUPLE_FIELDS[i]);
        match emit_return_adapter(emitter, &raw_field, slot_ty) {
            Some((inner_ret, inner_body)) => {
                let sub = emitter.fresh_var(Some("sub"));
                emitter.declare(&sub);
                body.push_str(&format!(
                    "{sub} := func() {inner_ret} {{\n{inner_body}}}()\n"
                ));
                field_exprs.push(sub);
                ret_types.push(inner_ret);
            }
            None => {
                ret_types.push(emitter.go_type_as_string(slot_ty));
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
    emitter: &mut Emitter,
    output: &mut String,
    fn_value: &str,
    fn_type: &Type,
) -> String {
    let Type::Function {
        params,
        return_type,
        ..
    } = fn_type
    else {
        return fn_value.to_string();
    };

    let return_type = return_type.as_ref();

    let (param_strs, arg_names) = emitter.build_wrapper_params(params);
    let params_str = param_strs.join(", ");

    let cb_var = emitter.hoist_tmp_value(output, "cb", fn_value);

    let mut prelude = String::new();
    let inner_args: Vec<String> = arg_names
        .iter()
        .zip(params.iter())
        .map(|(name, param_ty)| lower_arg_to_tagged(emitter, &mut prelude, name, param_ty))
        .collect();

    let call_str = format!("{}({})", cb_var, inner_args.join(", "));

    // Option<fn> adaptation only fires in interface-method shims. Here
    // a closure-valued Option means the caller owns the nil check.
    if let Type::Nominal { id, params: ps, .. } = return_type
        && id == "Option"
        && let Some(inner) = ps.first()
        && matches!(inner.unwrap_forall(), Type::Function { .. })
    {
        return fn_value.to_string();
    }

    let Some((go_ret, body)) = emit_return_adapter(emitter, &call_str, return_type) else {
        return fn_value.to_string();
    };

    format!("func({params_str}) {go_ret} {{\n{prelude}{body}}}")
}

/// Convert a fn-typed wrapper arg from lowered Go ABI back to tagged for
/// the inner call. Identity for non-fn args and for fn args with no
/// lowered return.
pub(crate) fn lower_arg_to_tagged(
    emitter: &mut Emitter,
    prelude: &mut String,
    arg_name: &str,
    param_ty: &Type,
) -> String {
    let unwrapped = param_ty.unwrap_forall();
    let Type::Function {
        params: inner_params,
        return_type: inner_ret,
        ..
    } = unwrapped
    else {
        return arg_name.to_string();
    };
    let inner_ret = inner_ret.as_ref();
    let Some(shape) = emitter.classify_direct_emission(inner_ret) else {
        return arg_name.to_string();
    };

    let (inner_param_strs, inner_arg_names) = emitter.build_wrapper_params(inner_params);
    let inner_call = format!("{}({})", arg_name, inner_arg_names.join(", "));
    let tagged_ret = emitter.go_type_as_string(inner_ret);

    let mut body = String::new();
    let result_var = emit_callee_abi_wrapping(emitter, &mut body, &shape, &inner_call, inner_ret);
    write_line!(body, "return {}", result_var);

    let tagged_var = emitter.fresh_var(Some("tagged"));
    emitter.declare(&tagged_var);
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

/// Tail return for `PartialTuple` and `Tuple` ABIs, which need per-shape
/// handling beyond the generic `emit_wrapped_return` path. Returns `true`
/// when this path handled the emission.
pub(crate) fn try_emit_lowered_tail_return(
    emitter: &mut Emitter,
    output: &mut String,
    expression: &syntax::ast::Expression,
    return_ctx: &crate::ReturnContext,
) -> bool {
    let Some(shape) = return_ctx.lowered_shape() else {
        return false;
    };
    match shape {
        AbiShape::PartialTuple => {
            emit_lowered_partial_tail(emitter, output, expression, return_ctx)
        }
        AbiShape::Tuple { arity } => {
            emit_lowered_tuple_tail(emitter, output, expression, arity, return_ctx)
        }
        _ => false,
    }
}

fn emit_lowered_tuple_tail(
    emitter: &mut Emitter,
    output: &mut String,
    expression: &syntax::ast::Expression,
    arity: usize,
    return_ctx: &crate::ReturnContext,
) -> bool {
    use crate::expressions::emission::EmittedExpression;
    use syntax::ast::Expression;
    if let Expression::Tuple { elements, .. } = expression
        && elements.len() == arity
    {
        let return_ty = return_ctx.expect_ty();
        let slot_tys = tuple_element_types(&emitter.facts.peel_alias(&return_ty));
        let stages: Vec<EmittedExpression> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut setup = String::new();
                let value = match slot_tys.get(i) {
                    Some(slot_ty) if emitter.facts.is_nullable_option(slot_ty) => {
                        emit_nullable_slot_value(emitter, &mut setup, e, slot_ty)
                    }
                    _ => emitter.emit_composite_value(&mut setup, e, ExpressionContext::value()),
                };
                EmittedExpression::new(setup, value, e)
            })
            .collect();
        let parts = emitter.sequence(output, stages, "_ret");
        write_line!(output, "return {}", parts.join(", "));
        return true;
    }

    let return_ty = return_ctx.expect_ty();
    let value = emitter.emit_value(output, expression, ExpressionContext::value());
    let temp = emitter.hoist_tmp_value(output, "tup", &value);
    emit_lowered_result_return(
        emitter,
        output,
        &temp,
        &return_ty,
        &AbiShape::Tuple { arity },
    );
    true
}

fn emit_lowered_partial_tail(
    emitter: &mut Emitter,
    output: &mut String,
    expression: &syntax::ast::Expression,
    return_ctx: &crate::ReturnContext,
) -> bool {
    use syntax::ast::Expression;
    let return_ty = return_ctx.expect_ty();

    if let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
        && let Some(variant) = callee.as_partial_constructor()
    {
        match variant {
            "Ok" => {
                let v = emitter.emit_composite_value(output, &args[0], ExpressionContext::value());
                write_line!(output, "return {}, nil", v);
            }
            "Err" => {
                let e = emitter.emit_composite_value(output, &args[0], ExpressionContext::value());
                let ok_ty = emitter.facts.peel_alias(&return_ty).ok_type();
                let ok_ty_str = emitter.go_type_as_string(&ok_ty);
                write_line!(output, "return *new({}), {}", ok_ty_str, e);
            }
            "Both" => {
                let v = emitter.emit_composite_value(output, &args[0], ExpressionContext::value());
                let e = emitter.emit_composite_value(output, &args[1], ExpressionContext::value());
                write_line!(output, "return {}, {}", v, e);
            }
            _ => unreachable!("as_partial_constructor only returns Ok/Err/Both"),
        }
        return true;
    }

    let value = emitter.emit_value(output, expression, ExpressionContext::value());
    emit_lowered_result_return(emitter, output, &value, &return_ty, &AbiShape::PartialTuple);
    true
}

/// `Some(x)`/`None` collapse to `x`/`nil`; other Option expressions
/// project at runtime.
fn emit_nullable_slot_value(
    emitter: &mut Emitter,
    output: &mut String,
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
                emitter.emit_composite_value(output, &args[0], ExpressionContext::value())
            }
            Err(()) => "nil".to_string(),
        };
    }
    if expression.is_none_literal() {
        return "nil".to_string();
    }
    let value = emitter.emit_value(output, expression, ExpressionContext::value());
    let inner = emitter.go_type_as_string(&slot_ty.ok_type());
    emitter.emit_option_projection(output, &value, "unwrap", &inner, false)
}
