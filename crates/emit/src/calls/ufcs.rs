use crate::calls::dispatch::{CallArgShape, all_type_params_inferrable};
use crate::calls::native::{apply_inline_import, native_method_lowers_to_plain_call};
use crate::plan::calls::plan_variadic_spread;
use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::{CallPlan, ResolvedCallee};
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use crate::types::native::NativeGoType;
use syntax::ast::{Expression, Literal};
use syntax::program::ReceiverCoercion;
use syntax::types::Type;

impl Planner<'_> {
    #[allow(clippy::too_many_arguments)]
    fn ufcs_type_args(
        &mut self,
        function: &Expression,
        callee: &ResolvedCallee<'_>,
        receiver_ty: &Type,
        type_args: &[Type],
        arg_shape: CallArgShape,
    ) -> Option<String> {
        let definition_ty = callee.declared.as_ref()?;

        // A method with no type parameters lowers to a non-generic free function.
        let Type::Forall { vars, body } = &definition_ty else {
            return None;
        };
        let Type::Function(f) = body.as_ref() else {
            return None;
        };

        let mut receiver_mapping: HashMap<String, Type> = HashMap::default();
        if let Some(self_param) = f.params.first() {
            extract_type_mapping(&self_param.strip_refs(), receiver_ty, &mut receiver_mapping);
        }

        if !type_args.is_empty() {
            let impl_count = vars.len().saturating_sub(type_args.len());
            let mut go_type_strs = Vec::with_capacity(vars.len());
            for (index, var) in vars.iter().enumerate() {
                let go_type = if index < impl_count {
                    self.go_type_string(receiver_mapping.get(var.as_str())?)
                } else {
                    self.go_type_string(type_args.get(index - impl_count)?)
                };
                go_type_strs.push(go_type);
            }
            return (!go_type_strs.is_empty()).then(|| format!("[{}]", go_type_strs.join(", ")));
        }

        let receiver_count = callee.receiver_offset.min(1);
        if all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape) {
            return None;
        }

        let mut inferred_mapping: HashMap<String, Type> = HashMap::default();
        if let Type::Function(inst) = function.get_type() {
            let self_curried = inst.params.len() + 1 == f.params.len();
            let declared = if self_curried {
                &f.params[1..]
            } else {
                &f.params[..]
            };
            for (decl, conc) in declared.iter().zip(inst.params.iter()) {
                extract_type_mapping(decl, conc, &mut inferred_mapping);
            }
            extract_type_mapping(&f.return_type, &inst.return_type, &mut inferred_mapping);
        }

        let mut go_type_strs = Vec::with_capacity(vars.len());
        for var in vars {
            let resolved = receiver_mapping
                .get(var.as_str())
                .or_else(|| inferred_mapping.get(var.as_str()))?;
            go_type_strs.push(self.go_type_string(resolved));
        }
        (!go_type_strs.is_empty()).then(|| format!("[{}]", go_type_strs.join(", ")))
    }

    pub(super) fn lower_ufcs_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: &[Type],
        spread: Option<&Expression>,
        call_plan: &CallPlan<'_>,
    ) -> ValuePlan {
        let Expression::DotAccess {
            expression: receiver,
            member,
            receiver_coercion: coercion,
            ..
        } = function
        else {
            unreachable!("lower_ufcs_call called on non-DotAccess");
        };

        let receiver_ty = self.facts.strip_and_peel(&receiver.get_type());
        let Type::Nominal {
            id: qualified_name, ..
        } = &receiver_ty
        else {
            unreachable!("UFCS receiver must be a constructor type");
        };

        let (setup, receiver_arg, emitted_args, arguments_contain_deferred_evaluation) = self
            .lower_ufcs_call_args(
                function,
                receiver,
                args,
                spread,
                &call_plan.resolved,
                *coercion,
            );
        let receiver_arg = match coercion {
            Some(ReceiverCoercion::AutoDeref) => format!("*{receiver_arg}"),
            Some(ReceiverCoercion::AutoAddress) | None => receiver_arg,
        };

        if let Some(inlined) =
            try_inline_native_ufcs(self, receiver, member, &receiver_arg, &emitted_args)
        {
            let native_type = NativeGoType::from_type(&receiver.get_type())
                .expect("inlined UFCS receiver has a native type");
            let plain_call =
                native_method_lowers_to_plain_call(&native_type, member, emitted_args.len());
            let expression = if plain_call {
                GoExpression::opaque_with_deferred_evaluation(inlined, true)
            } else if member == "byte_at" {
                GoExpression::opaque_with_deferred_evaluation(
                    inlined,
                    arguments_contain_deferred_evaluation,
                )
            } else if member == "is_empty" {
                GoExpression::opaque_with_deferred_evaluation(inlined, true)
            } else {
                GoExpression::opaque_with_deferred_evaluation(
                    inlined,
                    arguments_contain_deferred_evaluation,
                )
            };
            return if plain_call {
                ValuePlan::plain_call(setup, expression, EvaluationEffect::EffectfulCall)
            } else {
                ValuePlan::computed(setup, expression, EvaluationEffect::EffectfulCall)
            };
        }

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let fn_name = self.build_ufcs_qualified_call(
            function,
            &call_plan.resolved,
            &receiver_ty,
            qualified_name,
            member,
            type_args,
            CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
        );
        let expression = GoExpression::call(
            GoExpression::opaque(fn_name),
            new_args.into_iter().map(GoExpression::opaque).collect(),
        );
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::observable_call(setup, expression, EvaluationEffect::EffectfulCall)
        } else {
            ValuePlan::plain_call(setup, expression, EvaluationEffect::EffectfulCall)
        }
    }

    fn lower_ufcs_call_args(
        &mut self,
        function: &Expression,
        receiver: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        callee: &ResolvedCallee<'_>,
        coercion: Option<ReceiverCoercion>,
    ) -> (Vec<LoweredStatement>, String, Vec<String>, bool) {
        // The DotAccess function type curries `self` out, so its params line
        // up 1:1 with the user args. Pair each so a function-typed param
        // suppresses the Go-fn-value identity short-circuit before dispatch
        // into prelude helpers like `lisette.OptionAndThen`.
        let mut all_stages: Vec<ValuePlan> =
            Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
        let mut receiver_stage = self.stage_operand(receiver, ExpressionContext::value());
        if coercion == Some(ReceiverCoercion::AutoAddress) {
            receiver_stage = self.coerce_receiver_address_stage(receiver, receiver_stage);
        }
        all_stages.push(receiver_stage);
        for (i, arg) in args.iter().enumerate() {
            let param = callee.abi.param(i);
            let declared = (!callee.is_prelude_dispatch)
                .then(|| param.and_then(|param| param.declared.as_ref()))
                .flatten();
            let suppress_decl = callee
                .is_prelude_dispatch
                .then(|| param.and_then(|param| param.declared.as_ref()))
                .flatten();
            all_stages.push(self.stage_ufcs_arg(
                arg,
                declared,
                suppress_decl,
                param.map(|param| &param.instantiated),
            ));
        }
        let combine = plan_variadic_spread(function, spread).map(|p| p.combine(1));

        let sequenced = self.sequence_args_with_spread_adapter_values(
            all_stages,
            spread,
            (!callee.is_prelude_dispatch)
                .then(|| {
                    callee
                        .declared
                        .as_ref()
                        .and_then(|ty| ty.unwrap_forall().get_function_params())
                })
                .flatten(),
            false,
            combine,
            CaptureBoundary::SiblingSequence,
        );
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, all_values) = sequenced.into_rendered();
        let receiver_arg = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();
        (
            setup,
            receiver_arg,
            emitted_args,
            contains_deferred_evaluation,
        )
    }

    fn stage_ufcs_arg(
        &mut self,
        arg: &Expression,
        declared_param: Option<&Type>,
        suppress_declared: Option<&Type>,
        formal_param: Option<&Type>,
    ) -> ValuePlan {
        let Some(declared) = declared_param else {
            return self.stage_prelude_arg(arg, suppress_declared, formal_param);
        };
        if let Some(value) = self.try_adapt_lowered_fn_arg_shape(arg, Some(declared)) {
            return value;
        }
        self.stage_composite(arg, ExpressionContext::value())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_ufcs_qualified_call(
        &mut self,
        function: &Expression,
        callee: &ResolvedCallee<'_>,
        receiver_ty: &Type,
        qualified_name: &str,
        member: &str,
        type_args: &[Type],
        arg_shape: CallArgShape,
    ) -> String {
        let type_args_string = self
            .ufcs_type_args(function, callee, receiver_ty, type_args, arg_shape)
            .unwrap_or_default();

        let is_public = callee
            .definition
            .map(|d| d.visibility.is_public())
            .unwrap_or(false)
            || self.method_needs_export(member);

        let qualified_method_name = self.qualify_method_call(qualified_name, member, is_public);
        format!("{}{}", qualified_method_name, type_args_string)
    }

    fn coerce_receiver_address_stage(
        &mut self,
        receiver: &Expression,
        mut stage: ValuePlan,
    ) -> ValuePlan {
        let value = stage.expression.rendered();
        if matches!(receiver.unwrap_parens(), Expression::Call { .. }) {
            let tmp = self.hoist_tmp_value_statement(&mut stage.setup, "ref", &value);
            stage.expression = GoExpression::opaque(format!("&{tmp}"));
            return stage.into_addressed_location();
        }
        let addressed = format!("&{value}");
        if matches!(receiver.unwrap_parens(), Expression::Identifier { .. }) {
            stage.expression = GoExpression::opaque(addressed);
            stage.into_addressed_location()
        } else if stage.setup.is_empty() {
            stage.expression = GoExpression::opaque(addressed);
            stage.make_observable_computed();
            stage
        } else {
            let tmp = self.hoist_tmp_value_statement(&mut stage.setup, "ref", &addressed);
            ValuePlan::captured(stage.setup, tmp)
        }
    }

    pub(super) fn lower_receiver_method_ufcs(
        &mut self,
        function: &Expression,
        args: &[Expression],
        method: &str,
        is_public: bool,
        spread: Option<&Expression>,
    ) -> ValuePlan {
        let go_method = if is_public {
            go_name::snake_to_camel(method)
        } else {
            go_name::escape_keyword(method).into_owned()
        };

        let stages: Vec<ValuePlan> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value()))
            .collect();

        let combine = plan_variadic_spread(function, spread).map(|p| p.combine(0));
        let sequenced = self.sequence_with_spread_values(
            stages,
            spread,
            false,
            "_arg",
            combine,
            CaptureBoundary::SiblingSequence,
        );
        let (setup, emitted_all) = sequenced.into_rendered();

        let receiver = emitted_all[0].clone();
        let emitted_rest: Vec<String> = emitted_all[1..].to_vec();

        let receiver = if let Some(stripped) = receiver.strip_prefix('&') {
            if is_address_of_composite_literal(args.first()) {
                format!("(&{})", stripped)
            } else {
                stripped.to_string()
            }
        } else if receiver.starts_with('*') {
            format!("({})", receiver)
        } else {
            receiver
        };

        ValuePlan::observable_call(
            setup,
            GoExpression::call(
                GoExpression::opaque(format!("{}.{}", receiver, go_method)),
                emitted_rest.into_iter().map(GoExpression::opaque).collect(),
            ),
            EvaluationEffect::EffectfulCall,
        )
    }
}

fn try_inline_native_ufcs(
    planner: &Planner,
    receiver: &Expression,
    member: &str,
    receiver_arg: &str,
    emitted_args: &[String],
) -> Option<String> {
    let native_type = NativeGoType::from_type(&receiver.get_type())?;
    let (inlined, extra_import) = super::native::try_inline_native_method(
        &native_type,
        member,
        receiver_arg,
        emitted_args,
        false,
    )?;
    apply_inline_import(planner, extra_import);
    Some(inlined)
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
