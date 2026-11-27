use super::arguments::CallArgumentsContext;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::calls::dispatch::callee_go_name;
use crate::lowering::calls::dispatch::callee_is_go_builtin;
use crate::lowering::calls::dispatch::go_builtin_name;
use crate::lowering::calls::type_arguments::{CallArgumentShape, CallTypeArgumentsRequest};
use crate::lowering::expressions::literals::format_string_lowers_to_sprintf;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::go_expression::GoLiteral;
use syntax::program::ReceiverCoercion;

use crate::Planner;
use crate::lowering::calls::call_plan::{CallPlan, ResolvedCallee};
use crate::lowering::expressions::operand_order::SpreadSequenceOptions;
use crate::lowering::expressions::operand_order::{LaterOperands, SequencedArray};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::{SequencedValues, ValuePlan};
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

fn go_builtin_conversion(type_args: Option<&GoType>) -> Option<GoType> {
    let [argument] = type_args?.arguments() else {
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
        .as_function_type()?
        .params
        .first()?
        .ty
        .clone();

    Some((declared, facts.receiver_go_target(&receiver.get_type())))
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

struct RegularCallParts {
    setup: Vec<Statement>,
    callee: GoExpression,
    type_args: Option<GoType>,
    arguments: Vec<GoExpression>,
    builtin_conversion: Option<GoType>,
    constant_result: Option<ConstantKind>,
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
    pub(super) fn plan_direct_call(
        &mut self,
        call_plan: &CallPlan<'a, '_>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let function = call_plan.site.callee;
        let args = call_plan.site.args;
        let spread = call_plan.site.spread;

        if let Some(plan) = self
            .collapse_errors_new_format_arg(function, args, spread, expression_ctx)
            .or_else(|| {
                self.try_plan_go_named_call(function, args, spread, call_plan, expression_ctx)
            })
        {
            return plan;
        }

        let parts = self.plan_direct_call_parts(call_plan, call_ty, expression_ctx);
        self.queries().assemble_direct_call(function, args, parts)
    }

    fn plan_direct_call_parts(
        &mut self,
        call_plan: &CallPlan<'a, '_>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> RegularCallParts {
        let function = call_plan.site.callee;
        let callee_operand = self.plan_operand(function, expression_ctx.callee());
        let (mut setup, mut callee) = callee_operand.into_parts();

        let (type_args_string, builtin_conversion) =
            self.queries()
                .direct_call_type_arguments(call_plan, call_ty, expression_ctx);
        if type_args_string.is_some() {
            callee = callee.without_instantiation();
        }

        let args_ctx = self.direct_call_arguments_context(
            call_plan,
            expression_ctx,
            type_args_string.as_ref(),
        );
        let sequenced_args = self.plan_call_args(&args_ctx, None);
        let constant_result = go_builtin_name(&self.facts, function)
            .filter(|_| builtin_conversion.is_none())
            .and_then(|builtin| builtin_constant(builtin, &sequenced_args.values));
        let SequencedValues {
            setup: args_setup,
            values: arguments,
            ..
        } = sequenced_args;

        if type_args_string.is_none()
            && self.queries().callee_needs_pin(
                function,
                &call_plan.resolved,
                &args_setup,
                &arguments,
            )
        {
            let pinned = self.hoist_temp(&mut setup, "callee", callee);
            callee = GoExpression::identifier(pinned);
        }
        setup.extend(args_setup);

        RegularCallParts {
            setup,
            callee,
            type_args: type_args_string,
            arguments,
            builtin_conversion,
            constant_result,
        }
    }

    fn direct_call_arguments_context<'p>(
        &self,
        call_plan: &'p CallPlan<'a, 'p>,
        expression_ctx: ExpressionContext<'p>,
        type_args: Option<&GoType>,
    ) -> CallArgumentsContext<'p, 'a> {
        let function = call_plan.site.callee;
        CallArgumentsContext {
            plan: call_plan,
            spread: call_plan.site.spread,
            capture_boundary: expression_ctx.capture_boundary(),
            assigned_place: (call_plan.site.args.len() == 1
                && self.queries().callee_lowers_to_type_construction(function))
            .then(|| expression_ctx.assigned_place())
            .flatten(),
            callee_is_builtin: callee_is_go_builtin(&self.facts, function),
            callee_pins_type_args: type_args.is_some(),
            receiver_binding: receiver_type_binding(&self.facts, function, &call_plan.resolved),
        }
    }

    fn try_plan_go_named_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        call_plan: &CallPlan<'a, '_>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        let go_name = callee_go_name(&self.facts, function)?.to_string();
        let arg_ctx = match (expression_ctx.assigned_place(), args.len()) {
            (Some(assigned), 1) if self.queries().callee_lowers_to_type_construction(function) => {
                ExpressionContext::value().with_assigned_place(assigned)
            }
            _ => ExpressionContext::value(),
        };

        let operands: Vec<ValuePlan> = args.iter().map(|a| self.plan_operand(a, arg_ctx)).collect();
        let spread_operand =
            spread.map(|spread| self.plan_operand(spread, ExpressionContext::value()));
        let sequenced = self.sequence_with_spread_values(
            operands,
            spread_operand,
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
        let expression = self.queries().call_of(function, callee, sequenced.values);
        Some(
            if self.queries().callee_lowers_to_type_construction(function) {
                ValuePlan::computed(sequenced.setup, expression)
            } else {
                ValuePlan::plain_call(sequenced.setup, expression)
            },
        )
    }

    fn collapse_errors_new_format_arg(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        if spread.is_some()
            || self.facts.checked(function).callee().definition() != Some("go:errors.New")
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
        let operand = self.plan_operand(argument, ExpressionContext::value());
        let SequencedArray {
            setup,
            values: [value],
            ..
        } = self.sequence_value_array([operand], expression_ctx.capture_boundary(), "arg");
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
                GoExpression::qualified(self.queries().package_use_for_package("go:errors"), "New"),
                vec![value],
            ),
        };

        Some(ValuePlan::plain_call(setup, call))
    }
}

fn fmt_member(node: &GoExpressionNode) -> Option<&str> {
    let GoExpressionNode::Qualified { package, name } = node else {
        return None;
    };
    (package.package().path() == "fmt").then_some(name)
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

impl<'a> PlannerQueries<'_, 'a> {
    fn direct_call_type_arguments(
        &self,
        call_plan: &CallPlan<'_, '_>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> (Option<GoType>, Option<GoType>) {
        let function = call_plan.site.callee;
        let type_args = self.call_type_arguments(CallTypeArgumentsRequest {
            function,
            callee: &call_plan.resolved,
            written_type_arguments: call_plan.site.written_type_arguments(),
            call_ty,
            arg_shape: CallArgumentShape {
                value_count: call_plan.site.args.len(),
                has_spread: call_plan.site.spread.is_some(),
            },
            slot: expression_ctx.expected_slot_type(),
            receiver_passed: false,
        });

        let builtin_conversion = callee_is_go_builtin(self.facts, function)
            .then(|| go_builtin_conversion(type_args.as_ref()))
            .flatten();
        match builtin_conversion {
            Some(_) => (None, builtin_conversion),
            None => (type_args, None),
        }
    }

    fn assemble_direct_call(
        &self,
        function: &Expression,
        args: &[Expression],
        parts: RegularCallParts,
    ) -> ValuePlan {
        let RegularCallParts {
            setup,
            callee,
            type_args,
            arguments,
            builtin_conversion,
            constant_result,
        } = parts;
        let call = match self.collapse_fmt_print(&callee, args, &arguments) {
            Some(collapsed) => collapsed,
            None => self.call_of(
                function,
                GoExpression::instantiation(callee, type_args),
                arguments,
            ),
        };

        let call = match builtin_conversion {
            Some(go_type) => GoExpression::conversion(go_type, call),
            None => call,
        };

        let expression = call.with_constant(constant_result);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression)
        } else {
            ValuePlan::plain_call(setup, expression)
        }
    }

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
        !pointer_receiver_on_value_local(self.facts, function, resolved)
            && LaterOperands::sequenced(
                args_setup,
                arguments_effects.with_known_reads(arguments_effects.reads()),
            )
            .can_change(self.place_read_stability(function))
    }

    fn classify_fmt_argument(&self, expression: &Expression) -> Option<FmtArgument> {
        if let Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } = expression
        {
            return format_string_lowers_to_sprintf(self.facts, parts)
                .then_some(FmtArgument::Sprintf);
        }
        let Expression::Call {
            expression: callee,
            args: arguments,
            spread,
            ..
        } = expression
        else {
            return None;
        };
        match (
            self.facts.checked(callee).callee().definition(),
            arguments.as_slice(),
            spread,
        ) {
            (Some("go:fmt.Sprintf"), _, _) => Some(FmtArgument::Sprintf),
            (Some("go:fmt.Sprint"), [_], None) => Some(FmtArgument::Sprint),
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
                    let Some(GoExpressionNode::Literal(GoLiteral::Token(format))) =
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
