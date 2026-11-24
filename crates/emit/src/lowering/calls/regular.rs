use super::arguments::CallArgsContext;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::calls::dispatch::callee_go_name;
use crate::lowering::calls::dispatch::callee_is_go_builtin;
use crate::lowering::calls::dispatch::go_builtin_name;
use crate::lowering::calls::dispatch::{CallArgShape, CallTypeArgsRequest};
use crate::lowering::expressions::literals::format_string_lowers_to_sprintf;
use syntax::program::ReceiverCoercion;

use crate::Planner;
use crate::lowering::calls::call_plan::{CallPlan, ResolvedCallee};
use crate::lowering::expressions::staging::LaterStages;
use crate::lowering::expressions::staging::SpreadSequenceOptions;
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::{SequencedValues, ValuePlan};
use crate::target::ir::bodies::Statement;
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::{GoExpressionNode, Operand};
use crate::target::ir::values::{ConstantKind, GoExpression};
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, Literal};
use syntax::types::Type;

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

fn go_builtin_conversion(type_args: &GoType) -> Option<GoType> {
    let [argument] = type_args.arguments() else {
        return None;
    };
    Some(argument.clone())
}

pub(super) fn receiver_type_binding(
    facts: &EmitFacts<'_>,
    callee_expression: &Expression,
    callee: &ResolvedCallee<'_>,
) -> Option<(Type, Type)> {
    if callee.receiver_offset != 1 {
        return None;
    }
    let Expression::DotAccess {
        expression: receiver,
        ..
    } = callee_expression
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
    Some((
        declared,
        facts.peel_go_alias(&facts.without_references(&receiver.get_type())),
    ))
}

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

enum FmtArgument {
    Sprintf,
    Sprint,
}

enum FmtPrint {
    Print,
    Println,
}

impl FmtPrint {
    fn from_callee(callee: &GoExpressionNode) -> Option<Self> {
        match fmt_member(callee)? {
            "Print" => Some(Self::Print),
            "Println" => Some(Self::Println),
            _ => None,
        }
    }
}

fn call_arguments_of<'n>(expression: &'n GoExpressionNode, callee: &str) -> Option<&'n [Operand]> {
    let GoExpressionNode::Call {
        callee: called,
        arguments,
        ..
    } = expression
    else {
        return None;
    };
    (fmt_member(called) == Some(callee)).then_some(arguments.as_slice())
}

impl<'a> Planner<'a> {
    fn call_of(
        &self,
        function: &Expression,
        callee: GoExpression,
        arguments: Vec<GoExpression>,
    ) -> GoExpression {
        if self.callee_lowers_to_type_construction(function) {
            GoExpression::type_name_call(callee, arguments)
        } else if self.is_pure_constructor_callee(function) {
            GoExpression::pure_call(callee, arguments)
        } else {
            GoExpression::call(callee, arguments)
        }
    }

    pub(super) fn plan_regular_call(
        &mut self,
        call_expression: &Expression,
        call_plan: &CallPlan<'a>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let Expression::Call {
            expression: function,
            args,
            type_arguments,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("plan_regular_call requires a Call expression");
        };
        let spread = spread.as_deref();

        if let Some(plan) =
            self.collapse_errors_new_format_arg(function, args, spread, expression_ctx)
        {
            return plan;
        }

        if let Some(plan) =
            self.try_plan_go_named_call(function, args, spread, call_plan, expression_ctx)
        {
            return plan;
        }

        let callee_staged = self.plan_operand(function, expression_ctx.callee());
        let (mut setup, mut callee) = callee_staged.into_parts();

        let mut type_args_string = self.call_type_arguments(CallTypeArgsRequest {
            function,
            callee: &call_plan.resolved,
            written_type_arguments: type_arguments.annotations().len(),
            call_ty,
            arg_shape: CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
            slot: expression_ctx.expected_slot_type(),
            receiver_passed: false,
        });
        let builtin_conversion = callee_is_go_builtin(&self.facts, function)
            .then(|| go_builtin_conversion(&type_args_string))
            .flatten();
        if builtin_conversion.is_some() {
            type_args_string = GoType::default();
        }
        if !type_args_string.is_empty() {
            callee = callee.without_instantiation();
        }

        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            capture_boundary: expression_ctx.capture_boundary(),
            retired_receiver: (args.len() == 1
                && self.callee_lowers_to_type_construction(function))
            .then(|| expression_ctx.retired_receiver())
            .flatten(),
            callee_is_builtin: callee_is_go_builtin(&self.facts, function),
            callee_pins_type_args: !type_args_string.is_empty(),
            receiver_binding: receiver_type_binding(&self.facts, function, &call_plan.resolved),
        };
        let sequenced_args = self.plan_call_args(args, &args_ctx, None);
        let constant_result = go_builtin_name(&self.facts, function)
            .filter(|_| builtin_conversion.is_none())
            .and_then(|builtin| builtin_constant(builtin, &sequenced_args.values));
        let SequencedValues {
            setup: args_setup,
            values: arguments,
            ..
        } = sequenced_args;

        if type_args_string.is_empty()
            && self.callee_needs_pin(function, &call_plan.resolved, &args_setup, &arguments)
        {
            let pinned = self.hoist_tmp_value_statement(&mut setup, "callee", callee);
            callee = GoExpression::identifier(pinned);
        }

        let call = match self.collapse_fmt_print(&callee, args, &arguments) {
            Some(collapsed) => collapsed,
            None => self.call_of(
                function,
                GoExpression::instantiation(callee, type_args_string),
                arguments,
            ),
        };
        let call = match builtin_conversion {
            Some(go_type) => GoExpression::conversion(go_type, call),
            None => call,
        };

        setup.extend(args_setup);

        let expression = call.with_constant(constant_result);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression)
        } else {
            ValuePlan::plain_call(setup, expression)
        }
    }

    fn callee_needs_pin(
        &self,
        function: &Expression,
        resolved: &ResolvedCallee<'_>,
        args_setup: &[Statement],
        arguments: &[GoExpression],
    ) -> bool {
        let arguments_effects = arguments
            .iter()
            .fold(Effects::default(), |effects, argument| {
                effects.union(argument.effects())
            });
        !pointer_receiver_on_value_local(&self.facts, function, resolved)
            && LaterStages::sequenced(
                args_setup,
                arguments_effects.with_known_reads(arguments_effects.reads()),
            )
            .can_change(self.place_read_stability(function))
    }

    fn try_plan_go_named_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        call_plan: &CallPlan<'a>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        let go_name = callee_go_name(&self.facts, function)?.to_string();
        let arg_ctx = match (expression_ctx.retired_receiver(), args.len()) {
            (Some(retired), 1) if self.callee_lowers_to_type_construction(function) => {
                ExpressionContext::value().with_retired_receiver(retired)
            }
            _ => ExpressionContext::value(),
        };
        let stages: Vec<ValuePlan> = args.iter().map(|a| self.plan_operand(a, arg_ctx)).collect();
        let spread_stage =
            spread.map(|spread| self.plan_operand(spread, ExpressionContext::value()));
        let sequenced = self.sequence_with_spread_values(
            stages,
            spread_stage,
            SpreadSequenceOptions {
                wrap_to_any: call_plan
                    .resolved
                    .abi
                    .spread_wraps_to_any(&self.facts, spread),
                combine: call_plan.resolved.abi.variadic_combine(0),
                boundary: expression_ctx.capture_boundary(),
            },
        );
        let callee = GoExpression::external_name(go_name);
        let expression = self.call_of(function, callee, sequenced.values);
        Some(if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(sequenced.setup, expression)
        } else {
            ValuePlan::plain_call(sequenced.setup, expression)
        })
    }

    fn collapse_errors_new_format_arg(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        if spread.is_some()
            || self.facts.checked(function).callee().definition.as_deref() != Some("go:errors.New")
        {
            return None;
        }
        let [argument] = args else {
            return None;
        };
        let Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } = argument
        else {
            return None;
        };
        if !format_string_lowers_to_sprintf(&self.facts, parts) {
            return None;
        }
        let staged = self.plan_operand(argument, ExpressionContext::value());
        let mut sequenced =
            self.sequence_values(vec![staged], expression_ctx.capture_boundary(), "arg");
        let value = sequenced
            .values
            .pop()
            .expect("sequenced exactly one argument");
        let call = match call_arguments_of(value.node(), "Sprintf") {
            Some(arguments) => GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                arguments
                    .iter()
                    .cloned()
                    .map(GoExpression::from_operand)
                    .collect(),
            ),
            None => GoExpression::call(
                GoExpression::qualified(self.package_use_for_package("go:errors"), "New"),
                vec![value],
            ),
        };
        Some(ValuePlan::plain_call(sequenced.setup, call))
    }

    fn classify_fmt_argument(&self, expression: &Expression) -> Option<FmtArgument> {
        match expression {
            Expression::Literal {
                literal: Literal::FormatString(parts),
                ..
            } => {
                format_string_lowers_to_sprintf(&self.facts, parts).then_some(FmtArgument::Sprintf)
            }
            Expression::Call {
                expression: callee,
                args: arguments,
                spread,
                ..
            } => match (
                self.facts.checked(callee).callee().definition.as_deref(),
                arguments.as_slice(),
                spread,
            ) {
                (Some("go:fmt.Sprintf"), _, _) => Some(FmtArgument::Sprintf),
                (Some("go:fmt.Sprint"), [_], None) => Some(FmtArgument::Sprint),
                _ => None,
            },
            _ => None,
        }
    }

    fn collapse_fmt_print(
        &self,
        callee: &GoExpression,
        args: &[Expression],
        arguments: &[GoExpression],
    ) -> Option<GoExpression> {
        let print = FmtPrint::from_callee(callee.node())?;
        let ([arg_expression], [argument]) = (args, arguments) else {
            return None;
        };

        match self.classify_fmt_argument(arg_expression)? {
            FmtArgument::Sprintf => {
                let mut inner = call_arguments_of(argument.node(), "Sprintf")?.to_vec();
                if let FmtPrint::Println = print {
                    let Some(GoExpressionNode::Literal(format)) =
                        inner.first_mut().map(AsMut::as_mut)
                    else {
                        return None;
                    };
                    let close_quote = find_go_string_literal_close(format)?;
                    format.insert_str(close_quote, "\\n");
                }
                Some(GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Fmt, "Printf"),
                    inner.into_iter().map(GoExpression::from_operand).collect(),
                ))
            }
            FmtArgument::Sprint => {
                let inner = call_arguments_of(argument.node(), "Sprint")?;
                Some(GoExpression::call(
                    callee.clone(),
                    inner
                        .iter()
                        .cloned()
                        .map(GoExpression::from_operand)
                        .collect(),
                ))
            }
        }
    }
}

fn fmt_member(node: &GoExpressionNode) -> Option<&str> {
    match node {
        GoExpressionNode::Qualified { package, name } if package.package().path() == "fmt" => {
            Some(name)
        }
        _ => None,
    }
}

fn pointer_receiver_on_value_local(
    facts: &EmitFacts<'_>,
    callee_expression: &Expression,
    callee: &ResolvedCallee<'_>,
) -> bool {
    if receiver_type_binding(facts, callee_expression, callee).is_none() {
        return false;
    }
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee_expression
    else {
        return false;
    };
    matches!(&**receiver, Expression::Identifier { .. })
        && facts.dot_receiver_coercion(receiver, member) == Some(ReceiverCoercion::AutoAddress)
}
