use crate::calls::dispatch::{CallArgShape, all_type_params_inferrable};
use crate::calls::native::apply_inline_import;
use crate::calls::regular::effective_param_type;
use crate::plan::calls::plan_variadic_spread;
use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::types::native::NativeGoType;
use syntax::ast::{Expression, Literal};
use syntax::program::ReceiverCoercion;
use syntax::types::Type;

impl Planner<'_> {
    #[allow(clippy::too_many_arguments)]
    fn ufcs_type_args(
        &mut self,
        function: &Expression,
        qualified_name: &str,
        member: &str,
        receiver_ty: &Type,
        type_args: &[Type],
        arg_shape: CallArgShape,
    ) -> Option<String> {
        let method_key = format!("{}.{}", qualified_name, member);
        let definition_ty = self.facts.definition(method_key.as_str())?.ty().clone();

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

        let receiver_count = match function.get_type() {
            Type::Function(inst) if inst.params.len() + 1 == f.params.len() => 1,
            _ => 0,
        };
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
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::DotAccess {
            expression: receiver,
            member,
            receiver_coercion: coercion,
            ..
        } = function
        else {
            unreachable!("lower_ufcs_call called on non-DotAccess");
        };

        let receiver_ty = self.facts.peel_alias(&receiver.get_type().strip_refs());
        let Type::Nominal {
            id: qualified_name, ..
        } = &receiver_ty
        else {
            unreachable!("UFCS receiver must be a constructor type");
        };

        let (mut setup, receiver_arg, emitted_args) =
            self.lower_ufcs_call_args(function, receiver, args, spread);
        let receiver_arg =
            self.apply_receiver_coercion(&mut setup, receiver, receiver_arg, *coercion);

        if let Some(inlined) =
            try_inline_native_ufcs(self, receiver, member, &receiver_arg, &emitted_args)
        {
            return (setup, inlined);
        }

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let fn_name = self.build_ufcs_qualified_call(
            function,
            &receiver_ty,
            qualified_name,
            member,
            type_args,
            CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
        );
        (setup, format!("{}({})", fn_name, new_args.join(", ")))
    }

    fn lower_ufcs_call_args(
        &mut self,
        function: &Expression,
        receiver: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
    ) -> (Vec<LoweredStatement>, String, Vec<String>) {
        // The DotAccess function type curries `self` out, so its params line
        // up 1:1 with the user args. Pair each so a function-typed param
        // suppresses the Go-fn-value identity short-circuit before dispatch
        // into prelude helpers like `lisette.OptionAndThen`.
        let formal_params: Vec<Type> = match function.get_type().unwrap_forall() {
            Type::Function(f) => f.params.clone(),
            _ => Vec::new(),
        };
        let declared_params =
            self.ufcs_declared_user_params(receiver, function, formal_params.len());
        let suppress_declared = declared_params
            .is_none()
            .then(|| self.callee_declared_params(function, args.len()))
            .flatten();
        let mut all_stages: Vec<StagedExpression> =
            Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
        all_stages.push(self.stage_operand(receiver, ExpressionContext::value()));
        for (i, arg) in args.iter().enumerate() {
            let declared = declared_params.and_then(|p| effective_param_type(i, p));
            let suppress_decl = suppress_declared.and_then(|p| effective_param_type(i, p));
            all_stages.push(self.stage_ufcs_arg(
                arg,
                declared,
                suppress_decl,
                formal_params.get(i),
            ));
        }
        let combine = plan_variadic_spread(function, spread).map(|p| p.combine(1));

        let (setup, all_values) = self.sequence_args_with_spread_adapter(
            all_stages,
            spread,
            declared_params,
            false,
            combine,
        );
        let receiver_arg = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();
        (setup, receiver_arg, emitted_args)
    }

    fn stage_ufcs_arg(
        &mut self,
        arg: &Expression,
        declared_param: Option<&Type>,
        suppress_declared: Option<&Type>,
        formal_param: Option<&Type>,
    ) -> StagedExpression {
        let Some(declared) = declared_param else {
            return self.stage_prelude_arg(arg, suppress_declared, formal_param);
        };
        let mut setup: Vec<LoweredStatement> = Vec::new();
        if let Some(value) = self.try_adapt_lowered_fn_arg_shape(&mut setup, arg, Some(declared)) {
            return StagedExpression::from_typed_setup(setup, value, arg);
        }
        self.stage_composite(arg, ExpressionContext::value())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_ufcs_qualified_call(
        &mut self,
        function: &Expression,
        receiver_ty: &Type,
        qualified_name: &str,
        member: &str,
        type_args: &[Type],
        arg_shape: CallArgShape,
    ) -> String {
        let type_args_string = self
            .ufcs_type_args(
                function,
                qualified_name,
                member,
                receiver_ty,
                type_args,
                arg_shape,
            )
            .unwrap_or_default();

        let method_key = format!("{}.{}", qualified_name, member);
        let is_public = self
            .facts
            .definition(method_key.as_str())
            .map(|d| d.visibility().is_public())
            .unwrap_or(false)
            || self.method_needs_export(member);

        let qualified_method_name = self.qualify_method_call(qualified_name, member, is_public);
        format!("{}{}", qualified_method_name, type_args_string)
    }

    fn apply_receiver_coercion(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        receiver: &Expression,
        receiver_arg: String,
        coercion: Option<ReceiverCoercion>,
    ) -> String {
        match coercion {
            Some(ReceiverCoercion::AutoAddress) => {
                if matches!(receiver.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.hoist_tmp_value_statement(setup, "ref", &receiver_arg);
                    format!("&{}", tmp)
                } else {
                    format!("&{}", receiver_arg)
                }
            }
            Some(ReceiverCoercion::AutoDeref) => format!("*{}", receiver_arg),
            None => receiver_arg,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_receiver_method_ufcs(
        &mut self,
        function: &Expression,
        args: &[Expression],
        type_args: &[Type],
        method: &str,
        is_public: bool,
        spread: Option<&Expression>,
    ) -> (Vec<LoweredStatement>, String) {
        let go_method = if is_public {
            go_name::snake_to_camel(method)
        } else {
            go_name::escape_keyword(method).into_owned()
        };

        let stages: Vec<StagedExpression> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value()))
            .collect();

        let combine = plan_variadic_spread(function, spread).map(|p| p.combine(0));
        let (setup, emitted_all) =
            self.sequence_with_spread_structured(stages, spread, false, "_arg", combine);

        let receiver = emitted_all[0].clone();
        let emitted_rest: Vec<String> = emitted_all[1..].to_vec();

        let type_args_string = self.format_type_args(type_args);

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

        (
            setup,
            format!(
                "{}.{}{}({})",
                receiver,
                go_method,
                type_args_string,
                emitted_rest.join(", ")
            ),
        )
    }
}

impl<'a> Planner<'a> {
    fn ufcs_declared_user_params(
        &self,
        receiver: &Expression,
        function: &Expression,
        arg_count: usize,
    ) -> Option<&'a [Type]> {
        let receiver_ty = receiver.get_type().strip_refs();
        let Type::Nominal { id, .. } = &receiver_ty else {
            return None;
        };
        if id.starts_with("prelude.") || go_name::is_go_import(id.as_str()) {
            return None;
        }
        let declared = self
            .callee_definition(function)?
            .ty()
            .unwrap_forall()
            .get_function_params()?;
        let self_offset = declared.len().saturating_sub(arg_count);
        declared.get(self_offset..)
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
