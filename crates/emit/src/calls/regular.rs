use super::arguments::CallArgsContext;
use crate::calls::dispatch::{
    CallArgShape, all_type_params_inferrable, callee_is_go_builtin, go_builtin_name,
    is_prelude_variant_constructor,
};

use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::LaterStages;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name::GeneratedPackage;
use crate::plan::calls::{CallPlan, ResolvedCallee};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{
    ConstantKind, EvaluationEffect, GoExpression, SequencedValues, ValuePlan,
};
use syntax::ast::{Expression, Literal, ResolvedCallTypeArguments};
use syntax::types::Type;

struct CallTypeArgsRequest<'e, 'c> {
    function: &'e Expression,
    callee: &'e ResolvedCallee<'c>,
    type_args: ResolvedCallTypeArguments<'e>,
    call_ty: Option<&'e Type>,
    arg_shape: CallArgShape,
    ctx: ExpressionContext<'e>,
}

fn builtin_constant(builtin: &str, values: &[GoExpression]) -> Option<ConstantKind> {
    let mut kinds = values.iter().map(GoExpression::constant_kind);
    let first = kinds.next()??;
    let joined = kinds.try_fold(first, |joined, kind| {
        let kind = kind?;
        joined.join(kind).or((joined == kind).then_some(kind))
    })?;
    match builtin {
        "min" | "max" => Some(joined),
        "complex" => Some(ConstantKind::Complex),
        "real" | "imag" => Some(ConstantKind::Float),
        _ => None,
    }
}

fn go_builtin_conversion(type_args: &str) -> Option<String> {
    let inner = type_args.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty() && !inner.contains(',')).then(|| inner.to_string())
}

fn receiver_type_binding(
    callee_expression: &Expression,
    callee: &ResolvedCallee<'_>,
) -> Option<(Type, Type)> {
    if callee.receiver_offset != 1 {
        return None;
    }
    let Expression::DotAccess {
        expression: receiver,
        ..
    } = callee_expression.unwrap_parens()
    else {
        return None;
    };
    let declared = callee
        .declared_type()?
        .unwrap_forall()
        .get_function_params()?
        .first()?
        .ty
        .clone();
    Some((declared, receiver.get_type().strip_refs()))
}

/// Arguments cannot change the address a pointer receiver takes of a local.
fn pointer_receiver_on_value_local(
    callee_expression: &Expression,
    callee: &ResolvedCallee<'_>,
) -> bool {
    let Some((declared, _)) = receiver_type_binding(callee_expression, callee) else {
        return false;
    };
    let Expression::DotAccess {
        expression: receiver,
        ..
    } = callee_expression.unwrap_parens()
    else {
        return false;
    };
    declared.is_ref()
        && matches!(receiver.unwrap_parens(), Expression::Identifier { .. })
        && !receiver.get_type().is_ref()
}

/// Escape-aware close-quote search; plain `find` would collide with `\"` inside the literal.
fn find_go_string_literal_close(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn is_errors_new_callee(function: &Expression) -> bool {
    let Expression::DotAccess {
        expression, member, ..
    } = function
    else {
        return false;
    };
    member == "New" && expression.get_type().as_import_namespace() == Some("go:errors")
}

enum FmtArgument {
    Sprintf,
    Sprint,
}

fn classify_fmt_argument(planner: &Planner, expression: &Expression) -> Option<FmtArgument> {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => planner
            .format_string_lowers_to_sprintf(parts)
            .then_some(FmtArgument::Sprintf),
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => match (
            callee.unwrap_parens().as_dotted_path().as_deref(),
            args.as_slice(),
            spread,
        ) {
            (Some("fmt.Sprintf"), _, _) => Some(FmtArgument::Sprintf),
            (Some("fmt.Sprint"), [_], None) => Some(FmtArgument::Sprint),
            _ => None,
        },
        _ => None,
    }
}

enum FmtPrint {
    Print,
    Println,
}

impl FmtPrint {
    fn from_callee(callee: &str) -> Option<Self> {
        match callee {
            "fmt.Print" => Some(Self::Print),
            "fmt.Println" => Some(Self::Println),
            _ => None,
        }
    }
}

/// The arguments of `callee(...)` when the expression is a call to that name.
fn call_arguments_of<'n>(
    expression: &'n GoExpressionNode,
    callee: &str,
) -> Option<&'n [GoExpressionNode]> {
    let GoExpressionNode::Call {
        callee: called,
        arguments,
    } = expression
    else {
        return None;
    };
    (called.print() == callee).then_some(arguments.as_slice())
}

/// Collapse redundant fmt wrappers:
/// - `fmt.Print{ln}(fmt.Sprintf(...))` → `fmt.Printf(..., "\n")`
/// - `fmt.Print{ln}(fmt.Sprint(x))` → `fmt.Print{ln}(x)`
fn collapse_fmt_print(
    planner: &Planner,
    callee: &GoExpression,
    args: &[Expression],
    arguments: &[GoExpression],
) -> Option<GoExpression> {
    let print = FmtPrint::from_callee(&callee.rendered())?;
    let ([arg_expression], [argument]) = (args, arguments) else {
        return None;
    };

    match classify_fmt_argument(planner, arg_expression)? {
        FmtArgument::Sprintf => {
            let mut inner = call_arguments_of(argument.node(), "fmt.Sprintf")?.to_vec();
            if let FmtPrint::Println = print {
                let Some(GoExpressionNode::Literal(format)) = inner.first_mut() else {
                    return None;
                };
                let close_quote = find_go_string_literal_close(format)?;
                format.insert_str(close_quote, "\\n");
            }
            Some(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Printf"),
                inner.into_iter().map(GoExpression::from_node).collect(),
            ))
        }
        FmtArgument::Sprint => {
            let inner = call_arguments_of(argument.node(), "fmt.Sprint")?;
            Some(GoExpression::call(
                callee.clone(),
                inner.iter().cloned().map(GoExpression::from_node).collect(),
            ))
        }
    }
}

impl<'a> Planner<'a> {
    /// Lower a regular call: typed setup plus the call value text.
    pub(super) fn lower_regular_call(
        &mut self,
        call_expression: &Expression,
        call_plan: &CallPlan<'a>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let Expression::Call {
            expression: callee,
            args,
            type_arguments,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("lower_regular_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = spread.as_deref();
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");

        if let Some(plan) =
            self.collapse_errors_new_format_arg(function, args, spread, expression_ctx)
        {
            return plan;
        }

        if let Some(go_name) = self.get_callee_go_name(function).map(str::to_string) {
            let arg_ctx = match (expression_ctx.retired_receiver(), args.len()) {
                (Some(retired), 1) if self.callee_lowers_to_type_construction(function) => {
                    ExpressionContext::value().with_retired_receiver(retired)
                }
                _ => ExpressionContext::value(),
            };
            let stages: Vec<ValuePlan> =
                args.iter().map(|a| self.plan_operand(a, arg_ctx)).collect();
            let wrap_to_any = spread_needs_any_wrap(&self.facts, function, spread);
            let combine = call_plan.variadic_combine(0);
            let sequenced = self.sequence_with_spread_values(
                stages,
                spread,
                None,
                SpreadSequenceOptions {
                    wrap_to_any,
                    combine,
                    boundary: expression_ctx.capture_boundary(),
                },
            );
            let effect = self.regular_call_effect(function, sequenced.effect);
            let expression = GoExpression::call(GoExpression::name(go_name), sequenced.values);
            return if self.callee_lowers_to_type_construction(function) {
                ValuePlan::observable_call(sequenced.setup, expression, effect)
            } else {
                ValuePlan::plain_call(sequenced.setup, expression, effect)
            };
        }

        let callee_staged = self.plan_operand(function, expression_ctx.callee());
        let callee_effect = callee_staged.evaluation.effect;
        let ValuePlan {
            mut setup,
            expression: mut callee,
            ..
        } = callee_staged;

        let mut type_args_string = self.resolve_call_type_args(CallTypeArgsRequest {
            function,
            callee: &call_plan.resolved,
            type_args: resolved_type_args,
            call_ty,
            arg_shape: CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
            ctx: expression_ctx,
        });
        let builtin_conversion = callee_is_go_builtin(function)
            .then(|| go_builtin_conversion(&type_args_string))
            .flatten();
        if builtin_conversion.is_some() {
            type_args_string = String::new();
        }
        if !type_args_string.is_empty() {
            callee = callee.without_instantiation();
        }

        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            wrap_spread_to_any: spread_needs_any_wrap(&self.facts, function, spread),
            combine_variadic: call_plan.variadic_combine(0),
            capture_boundary: expression_ctx.capture_boundary(),
            retired_receiver: (args.len() == 1
                && self.callee_lowers_to_type_construction(function))
            .then(|| expression_ctx.retired_receiver())
            .flatten(),
            callee_is_builtin: callee_is_go_builtin(function),
            callee_pins_type_args: !type_args_string.is_empty(),
            receiver_binding: receiver_type_binding(function, &call_plan.resolved),
        };
        let sequenced_args = self.emit_call_args(args, &args_ctx);
        let args_effect = sequenced_args.effect;
        let constant_result = go_builtin_name(function)
            .filter(|_| builtin_conversion.is_none())
            .and_then(|builtin| builtin_constant(builtin, &sequenced_args.values));
        let SequencedValues {
            setup: args_setup,
            values: arguments,
            ..
        } = sequenced_args;

        let callee_needs_pin = setup.is_empty()
            && type_args_string.is_empty()
            && !pointer_receiver_on_value_local(function, &call_plan.resolved)
            && LaterStages::sequenced(&args_setup, args_effect)
                .can_change(self.place_read_stability(function));
        if callee_needs_pin {
            let pinned = self.hoist_tmp_value_statement(&mut setup, "callee", callee);
            callee = GoExpression::name(pinned);
        }

        let call = match collapse_fmt_print(self, &callee, args, &arguments) {
            Some(collapsed) => collapsed,
            None => GoExpression::call(
                GoExpression::instantiation(callee, type_args_string),
                arguments,
            ),
        };
        let call = match builtin_conversion {
            Some(go_type) => GoExpression::conversion(go_type, call),
            None => call,
        };

        setup.extend(args_setup);

        let effect = self
            .regular_call_effect(function, args_effect)
            .combine(callee_effect);
        let expression = call.with_constant(constant_result);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression, effect)
        } else {
            ValuePlan::plain_call(setup, expression, effect)
        }
    }

    /// Emit `errors.New(f"...")` as `fmt.Errorf(...)`: compiler-built format strings
    /// cannot contain `%w`, and bypassing the callee drops an unused `errors` import.
    fn collapse_errors_new_format_arg(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        if spread.is_some() || !is_errors_new_callee(function) {
            return None;
        }
        let [argument] = args else {
            return None;
        };
        let Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } = argument.unwrap_parens()
        else {
            return None;
        };
        if !self.format_string_lowers_to_sprintf(parts) {
            return None;
        }
        let staged = self.plan_operand(argument, ExpressionContext::value());
        let mut sequenced =
            self.sequence_values(vec![staged], expression_ctx.capture_boundary(), "arg");
        let effect = self.regular_call_effect(function, sequenced.effect);
        let value = sequenced
            .values
            .pop()
            .expect("sequenced exactly one argument");
        let call = match call_arguments_of(value.node(), "fmt.Sprintf") {
            Some(arguments) => GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                arguments
                    .iter()
                    .cloned()
                    .map(GoExpression::from_node)
                    .collect(),
            ),
            None => GoExpression::call(
                GoExpression::qualified(self.package_use_for_package("go:errors"), "New"),
                vec![value],
            ),
        };
        Some(ValuePlan::plain_call(sequenced.setup, call, effect))
    }

    fn regular_call_effect(
        &self,
        function: &Expression,
        argument_effect: EvaluationEffect,
    ) -> EvaluationEffect {
        if self.is_pure_constructor_callee(function) {
            EvaluationEffect::PureCall.combine(argument_effect)
        } else {
            EvaluationEffect::EffectfulCall
        }
    }

    fn callee_collapsed_recipe(&self, callee: &ResolvedCallee<'_>) -> Option<String> {
        callee
            .declaration?
            .go_type_param_recipe()
            .map(str::to_string)
    }

    /// True when Go can infer every type parameter of a collapsed callee from
    /// its value parameters. A var present only in the return type, or only in a
    /// trailing `VarArgs<T>` the call leaves empty, is not inferable, so the
    /// recipe must be rebuilt.
    fn collapsed_callee_fully_inferable(
        &self,
        callee: &ResolvedCallee<'_>,
        arg_shape: CallArgShape,
    ) -> bool {
        let Some(Type::Forall { vars, body }) = callee.declared_type() else {
            return false;
        };
        let Type::Function(f) = body.as_ref() else {
            return false;
        };
        all_type_params_inferrable(vars, &f.params, 0, arg_shape)
    }

    fn reconstruct_collapsed_call_type_args(
        &mut self,
        callee: &ResolvedCallee<'_>,
        recipe: &str,
    ) -> Option<String> {
        let Type::Forall { body, .. } = callee.declared_type()? else {
            return None;
        };
        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(body, &callee.instantiated, &mut mapping);
        self.reconstruct_collapsed_type_args(recipe, &mapping)
    }

    fn resolve_call_type_args(&mut self, request: CallTypeArgsRequest<'_, '_>) -> String {
        let CallTypeArgsRequest {
            function,
            callee,
            type_args,
            call_ty,
            arg_shape,
            ctx,
        } = request;
        if callee_curries_receiver(callee) {
            return String::new();
        }

        let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
        if let Some(recipe) = self.callee_collapsed_recipe(callee) {
            if has_value_args && self.collapsed_callee_fully_inferable(callee, arg_shape) {
                return String::new();
            }
            return self
                .reconstruct_collapsed_call_type_args(callee, &recipe)
                .unwrap_or_default();
        }

        let mut type_args_string = self.format_resolved_type_args(type_args);

        let slot_ty = ctx.expected_slot_type();

        if type_args_string.is_empty()
            && let Some(inferred) =
                self.infer_return_only_type_args(function, callee.declared_type(), arg_shape)
        {
            type_args_string = match slot_ty {
                Some(t) => self.prelude_container_type_args(t).unwrap_or(inferred),
                None => inferred,
            };
        }

        if type_args_string.is_empty() && is_prelude_variant_constructor(function) {
            let mut candidate = call_ty.and_then(|t| self.prelude_container_type_args(t));
            if candidate.is_none() {
                candidate = slot_ty.and_then(|t| self.prelude_container_type_args(t));
            }
            type_args_string = candidate.unwrap_or_default();
        }

        type_args_string
    }
}

fn callee_curries_receiver(callee: &ResolvedCallee<'_>) -> bool {
    let Some(Type::Forall { body, .. }) = callee.declared_type() else {
        return false;
    };
    let Type::Function(declared_fn) = body.as_ref() else {
        return false;
    };
    callee
        .instantiated
        .as_function_type()
        .is_some_and(|instantiated_fn| instantiated_fn.params.len() < declared_fn.params.len())
}

/// The element type of a `VarArgs<T>`, or the type itself when not variadic.
fn spread_needs_any_wrap(
    facts: &crate::EmitFacts<'_>,
    function: &Expression,
    spread: Option<&Expression>,
) -> bool {
    let Some(spread_expr) = spread else {
        return false;
    };
    let Some(function_ty) = facts.resolve_to_function_type(&function.get_type()) else {
        return false;
    };
    let Some(variadic_element) = function_ty.is_variadic() else {
        return false;
    };
    if !facts.resolves_to_unknown(&variadic_element) {
        return false;
    }
    spread_expr
        .get_type()
        .inner()
        .is_some_and(|ty| !facts.resolves_to_unknown(&ty))
}
