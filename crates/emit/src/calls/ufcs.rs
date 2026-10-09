use crate::calls::arguments::CallArgsContext;
use crate::calls::dispatch::{CallArgShape, all_type_params_inferrable};
use crate::calls::native::native_method_lowers_to_plain_call;
use crate::calls::regular::receiver_type_binding;

use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::plan::bodies::Statement;
use crate::plan::calls::{CallPlan, ResolvedCallee};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::{Expression, Literal, ResolvedCallTypeArguments};
use syntax::program::ReceiverCoercion;
use syntax::program::{NativeTypeKind, resolved_instantiation};
use syntax::types::Type;

#[derive(Clone, Copy)]
struct UfcsCallSite<'e, 'c> {
    function: &'e Expression,
    plan: &'e CallPlan<'c>,
}

impl Planner<'_> {
    fn ufcs_type_args(
        &mut self,
        function: &Expression,
        callee: &ResolvedCallee<'_>,
        type_args: ResolvedCallTypeArguments<'_>,
        arg_shape: CallArgShape,
    ) -> Option<String> {
        let definition_ty = callee.declared_type()?;

        // A method with no type parameters lowers to a non-generic free function.
        let Type::Forall { vars, body } = &definition_ty else {
            return None;
        };
        let Type::Function(f) = body.as_ref() else {
            return None;
        };

        let receiver_count = callee.receiver_offset.min(1);
        if type_args.is_empty()
            && all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
        {
            return None;
        }
        // A method-only parameter missing from the callee type is not unified, so explicit arguments apply.
        let mut mapping = resolved_instantiation(function)?.clone();
        let offset = vars.len().checked_sub(type_args.len())?;
        for (name, argument) in vars[offset..].iter().zip(type_args.iter()) {
            mapping.insert(name.clone(), argument.clone());
        }
        self.format_generic_instantiation(definition_ty, &mapping)
    }

    pub(super) fn lower_ufcs_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: ResolvedCallTypeArguments<'_>,
        spread: Option<&Expression>,
        call_plan: &CallPlan<'_>,
    ) -> ValuePlan {
        let Expression::DotAccess {
            expression: receiver,
            member,
            resolution,
            ..
        } = function
        else {
            unreachable!("lower_ufcs_call called on non-DotAccess");
        };

        let coercion = resolution.receiver_coercion();
        let receiver_ty = self.facts.strip_and_peel(&receiver.get_type());
        let site = UfcsCallSite {
            function,
            plan: call_plan,
        };
        let type_args_string = self
            .ufcs_type_args(
                function,
                &call_plan.resolved,
                type_args,
                CallArgShape {
                    value_count: args.len(),
                    has_spread: spread.is_some(),
                },
            )
            .unwrap_or_default();

        let mut receiver_stage = self.plan_operand(receiver, ExpressionContext::value());
        if coercion == Some(ReceiverCoercion::AutoAddress) {
            receiver_stage = self.coerce_receiver_address_stage(receiver, receiver_stage);
        }
        let (setup, receiver_arg, emitted_args) = self.lower_ufcs_call_args(
            site,
            receiver_stage,
            args,
            spread,
            !type_args_string.is_empty(),
        );
        let receiver_arg = match coercion {
            Some(ReceiverCoercion::AutoDeref) => GoExpression::dereference(receiver_arg),
            Some(ReceiverCoercion::AutoAddress) | None => receiver_arg,
        };

        if let Some(inlined) =
            try_inline_native_ufcs(receiver, member, &receiver_arg, &emitted_args)
        {
            let native_type = NativeTypeKind::from_type(&receiver.get_type())
                .expect("inlined UFCS receiver has a native type");
            let plain_call =
                native_method_lowers_to_plain_call(&native_type, member, emitted_args.len());
            return if plain_call {
                ValuePlan::plain_call(setup, inlined, EvaluationEffect::EffectfulCall)
            } else {
                ValuePlan::computed(setup, inlined, EvaluationEffect::EffectfulCall)
            };
        }

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let callee = self.build_ufcs_qualified_call(
            &call_plan.resolved,
            &receiver_ty,
            member,
            type_args_string,
        );
        let expression = GoExpression::call(callee, new_args);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::observable_call(setup, expression, EvaluationEffect::EffectfulCall)
        } else {
            ValuePlan::plain_call(setup, expression, EvaluationEffect::EffectfulCall)
        }
    }

    fn lower_ufcs_call_args(
        &mut self,
        site: UfcsCallSite<'_, '_>,
        receiver_stage: ValuePlan,
        args: &[Expression],
        spread: Option<&Expression>,
        pins_type_args: bool,
    ) -> (Vec<Statement>, GoExpression, Vec<GoExpression>) {
        let UfcsCallSite { function, plan } = site;
        let callee = &plan.resolved;
        let sequenced = if callee.is_prelude_dispatch {
            // The DotAccess function type curries `self` out, so its params line
            // up 1:1 with the user args. Pair each so a function-typed param
            // suppresses the Go-fn-value identity short-circuit before dispatch
            // into prelude helpers like `lisette.OptionAndThen`.
            let mut stages = Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
            stages.push(receiver_stage);
            for (i, arg) in args.iter().enumerate() {
                let param = callee.abi.param(i);
                stages.push(self.stage_prelude_arg(
                    arg,
                    param.and_then(|param| param.declared.as_ref()),
                    param.map(|param| &param.instantiated),
                ));
            }
            let spread_stage =
                spread.map(|spread| self.plan_operand(spread, ExpressionContext::value()));
            self.sequence_with_spread_values(
                stages,
                spread_stage,
                SpreadSequenceOptions {
                    wrap_to_any: false,
                    combine: callee.abi.variadic_combine(1),
                    boundary: CaptureBoundary::SiblingSequence,
                },
            )
        } else {
            let args_ctx = CallArgsContext {
                plan,
                spread,
                wrap_spread_to_any: false,
                capture_boundary: CaptureBoundary::SiblingSequence,
                retired_receiver: None,
                callee_is_builtin: false,
                callee_pins_type_args: pins_type_args,
                receiver_binding: receiver_type_binding(function, callee),
            };
            self.emit_call_args(args, &args_ctx, Some(receiver_stage))
        };
        let mut all_values = sequenced.values;
        let receiver_arg = all_values.remove(0);
        (sequenced.setup, receiver_arg, all_values)
    }

    fn build_ufcs_qualified_call(
        &mut self,
        callee: &ResolvedCallee<'_>,
        receiver_ty: &Type,
        member: &str,
        type_args_string: String,
    ) -> GoExpression {
        let Type::Nominal {
            id: qualified_name, ..
        } = receiver_ty
        else {
            unreachable!("UFCS receiver must be a constructor type");
        };
        let is_public = callee
            .declaration
            .map(|declaration| declaration.visibility().is_public())
            .unwrap_or(false)
            || self.method_needs_export(member);

        let qualified_method_name = self.qualify_method_call(qualified_name, member, is_public);
        GoExpression::instantiation(qualified_method_name, type_args_string)
    }

    fn coerce_receiver_address_stage(
        &mut self,
        receiver: &Expression,
        stage: ValuePlan,
    ) -> ValuePlan {
        match receiver.unwrap_parens() {
            Expression::Call { .. } => stage
                .map_expression(|setup, value| {
                    let temp = self.hoist_tmp_value_statement(setup, "ref", value);
                    GoExpression::address_of(GoExpression::name(temp))
                })
                .into_addressed_location(),
            Expression::Identifier { .. } => stage
                .map_expression(|_, value| GoExpression::address_of(value))
                .into_addressed_location(),
            _ if stage.setup().is_empty() => {
                stage.map_observable_expression(|_, value| GoExpression::address_of(value))
            }
            _ => {
                let (mut setup, value) = stage.into_parts();
                let addressed = GoExpression::address_of(value);
                let temp = self.hoist_tmp_value_statement(&mut setup, "ref", addressed);
                ValuePlan::captured(setup, temp)
            }
        }
    }

    pub(super) fn lower_receiver_method_ufcs(
        &mut self,
        args: &[Expression],
        method: &str,
        is_public: bool,
        spread: Option<&Expression>,
        call_plan: &CallPlan<'_>,
    ) -> ValuePlan {
        let go_method = self.method_go_name(method, is_public);

        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            wrap_spread_to_any: false,
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
            callee_is_builtin: false,
            callee_pins_type_args: false,
            receiver_binding: None,
        };
        let sequenced = self.emit_call_args(args, &args_ctx, None);
        let mut emitted_all = sequenced.values;
        let receiver = emitted_all.remove(0);

        // The method call takes the value's own address, so `&x` unwraps.
        let addressed = match receiver.node() {
            GoExpressionNode::AddressOf(inner) => Some(GoExpression::from_node((**inner).clone())),
            _ => None,
        };
        let receiver = match addressed {
            Some(inner) if is_address_of_composite_literal(args.first()) => {
                GoExpression::address_of(inner)
            }
            Some(inner) => inner,
            None => receiver,
        };

        ValuePlan::observable_call(
            sequenced.setup,
            GoExpression::call(GoExpression::selector(receiver, go_method), emitted_all),
            EvaluationEffect::EffectfulCall,
        )
    }
}

fn try_inline_native_ufcs(
    receiver: &Expression,
    member: &str,
    receiver_arg: &GoExpression,
    emitted_args: &[GoExpression],
) -> Option<GoExpression> {
    let native_type = NativeTypeKind::from_type(&receiver.get_type())?;
    super::native::try_inline_native_method(&native_type, member, receiver_arg, emitted_args, false)
}

fn is_address_of_composite_literal(arg: Option<&Expression>) -> bool {
    let Some(Expression::Reference {
        expression: inner, ..
    }) = arg.map(Expression::unwrap_parens)
    else {
        return false;
    };
    matches!(
        inner.unwrap_parens(),
        Expression::StructCall { .. }
            | Expression::Literal {
                literal: Literal::Slice(_),
                ..
            }
    )
}
