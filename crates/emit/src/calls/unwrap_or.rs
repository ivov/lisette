use crate::Planner;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::context::expression::ExpressionContext;
use crate::patterns::matching::{OptionArms, OptionFusePlan, ResultFusePlan};
use crate::plan::bodies::{
    AssignForm, ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, define,
};
use crate::plan::placement::collapse_declared_temp;
use crate::plan::values::{EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::{Expression, Literal, Pattern, Span};
use syntax::types::Type;

/// `call.unwrap_or(default)`, `call.map_or(default, |v| ..)`, or `call.map(|v| ..).unwrap_or(default)`.
struct DefaultedCall<'a> {
    fuse: FusedCall<'a>,
    map: Option<MapLambda<'a>>,
    default: &'a Expression,
}

enum FusedCall<'a> {
    Result(ResultFusePlan<'a>),
    Option(OptionFusePlan<'a>),
}

pub(crate) struct MapLambda<'a> {
    pub(crate) param: Option<&'a str>,
    pub(crate) param_span: Option<Span>,
    pub(crate) body: &'a Expression,
}

/// A statement that would leave the lambda if its body ran inline.
fn escapes_lambda(expression: &Expression) -> bool {
    match expression {
        Expression::Return { .. }
        | Expression::Propagate { .. }
        | Expression::Defer { .. }
        | Expression::Task { .. } => true,
        Expression::Lambda { .. } | Expression::Function { .. } => false,
        _ => expression.children().into_iter().any(escapes_lambda),
    }
}

fn is_literal_default(default: &Expression) -> bool {
    let Expression::Literal { literal, .. } = default.unwrap_parens() else {
        return false;
    };
    match literal {
        Literal::Integer { .. }
        | Literal::Float { .. }
        | Literal::Boolean(_)
        | Literal::String { .. }
        | Literal::Char(_) => true,
        Literal::Slice(items) => items.iter().all(is_literal_default),
        Literal::Imaginary(_) | Literal::FormatString(_) => false,
    }
}

fn is_go_zero_literal(default: &Expression, ty: &Type) -> bool {
    let Expression::Literal { literal, .. } = default.unwrap_parens() else {
        return false;
    };
    match literal {
        Literal::Integer { value, .. } => *value == 0 && ty.is_numeric(),
        Literal::Float { value, .. } => *value == 0.0 && value.is_sign_positive() && ty.is_float(),
        Literal::Boolean(value) => !value && ty.is_boolean(),
        Literal::String { value, .. } => value.is_empty() && ty.is_string(),
        _ => false,
    }
}

pub(crate) fn map_lambda(function: &Expression) -> Option<MapLambda<'_>> {
    let Expression::Lambda { params, body, .. } = function.unwrap_parens() else {
        return None;
    };
    let param = match params.as_slice() {
        [] => {
            return (!escapes_lambda(body)).then_some(MapLambda {
                param: None,
                param_span: None,
                body,
            });
        }
        [param] => param,
        _ => return None,
    };
    let (param, param_span) = match &param.pattern {
        Pattern::Identifier { identifier, span } => (Some(identifier.as_str()), Some(*span)),
        Pattern::WildCard { .. } => (None, None),
        _ => return None,
    };
    (!escapes_lambda(body)).then_some(MapLambda {
        param,
        param_span,
        body,
    })
}

impl Planner<'_> {
    pub(crate) fn prelude_method_call<'a>(
        &self,
        expression: &'a Expression,
        method: &str,
    ) -> Option<(&'a Expression, &'a [Expression])> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } = expression.unwrap_parens()
        else {
            return None;
        };
        if spread.is_some() {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee.unwrap_parens()
        else {
            return None;
        };
        if member != method
            || !self
                .plan_call(expression.unwrap_parens())?
                .resolved
                .is_prelude_dispatch
        {
            return None;
        }
        Some((receiver, args))
    }

    fn defaulted_call<'a>(&self, expression: &'a Expression) -> Option<DefaultedCall<'a>> {
        let (receiver, map, default) = if let Some((receiver, [default])) =
            self.prelude_method_call(expression, "unwrap_or")
        {
            match self.prelude_method_call(receiver, "map") {
                Some((inner, [function])) => (inner, Some(map_lambda(function)?), default),
                _ => (receiver, None, default),
            }
        } else if let Some((receiver, [default, function])) =
            self.prelude_method_call(expression, "map_or")
        {
            (receiver, Some(map_lambda(function)?), default)
        } else {
            return None;
        };
        // map_or evaluates its default before the mapper; map(...).unwrap_or(...) after.
        if map.is_some() && !is_literal_default(default) {
            return None;
        }
        let receiver_ty = self.facts.peel_alias(&receiver.get_type());
        if (receiver_ty.is_option() || receiver_ty.is_result()) && receiver_ty.ok_type().is_unit() {
            return None;
        }
        let fuse = if receiver_ty.is_option() {
            let fuse = self.option_fuse_plan(receiver)?;
            if matches!(fuse, OptionFusePlan::Index { .. }) {
                return None;
            }
            FusedCall::Option(fuse)
        } else if receiver_ty.is_result() && map.is_none() {
            let fuse = self.result_fuse_plan(receiver)?;
            if !fuse.carries_payload() {
                return None;
            }
            FusedCall::Result(fuse)
        } else {
            return None;
        };
        Some(DefaultedCall { fuse, map, default })
    }

    fn zero_defaulted_map_read(
        &mut self,
        call: &DefaultedCall<'_>,
        ty: &Type,
    ) -> Option<(Vec<LoweredStatement>, GoExpression)> {
        let FusedCall::Option(OptionFusePlan::CommaOk { subject, source }) = &call.fuse else {
            return None;
        };
        if call.map.is_some()
            || !source.is_map_index()
            || !is_go_zero_literal(call.default, &self.facts.peel_alias(ty))
        {
            return None;
        }
        Some(self.lower_map_index_pair(subject))
    }

    /// `x, ok := call` then `if !ok { x = default }`, with `x` as `slot` names it.
    fn lower_default_into_slot(
        &mut self,
        fuse: FusedCall<'_>,
        default: &Expression,
        slot: CommaOkValueSlot,
    ) -> (Vec<LoweredStatement>, String) {
        let mut bound = match fuse {
            FusedCall::Result(fuse) => fuse.bind(self, slot, None),
            FusedCall::Option(fuse) => fuse.bind(self, slot),
        };
        let value = bound.writable_payload(self);
        let failure = bound.failure_condition();
        let mut statements = bound.statements;
        let literal_default = is_literal_default(default);
        let value_plan = self.lower_value(default, ExpressionContext::value());
        let default = if literal_default {
            value_plan
        } else {
            self.eager_operand(default, value_plan, "default")
        };
        let (default_setup, default) = default.split_setup();
        statements.extend(default_setup);
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: failure.initializer,
            condition: failure.condition,
            then_body: LoweredBlock {
                statements: vec![LoweredStatement::Assign(AssignForm::Simple {
                    target_capture: Vec::new(),
                    target: GoExpression::name(value.clone()),
                    value: default,
                })],
            },
            else_arm: ElseArm::None,
        }));
        (statements, value)
    }

    fn lower_mapped_default_into(
        &mut self,
        fuse: OptionFusePlan<'_>,
        map: MapLambda<'_>,
        default: &Expression,
        target: &str,
        target_ty: &Type,
    ) -> Vec<LoweredStatement> {
        let arms = OptionArms {
            some_binding: map.param,
            some_binding_span: map.param_span,
            some_body: map.body,
            none_body: default,
        };
        let target = GoExpression::name(target.to_string());
        self.lower_fused_option_arms(
            fuse,
            arms,
            &PlacePlan::Assign {
                local: &target,
                target_ty: Some(target_ty),
            },
        )
    }

    /// `let x = call.unwrap_or(default)` writes straight into `x`.
    pub(crate) fn lower_defaulted_call_into(
        &mut self,
        value: &Expression,
        go_name: &str,
    ) -> Option<Vec<LoweredStatement>> {
        let call = self.defaulted_call(value)?;
        if let Some((mut statements, read)) = self.zero_defaulted_map_read(&call, &value.get_type())
        {
            self.declare(go_name);
            statements.push(define(go_name.to_string(), read));
            return Some(statements);
        }
        self.declare(go_name);
        let Some(map) = call.map else {
            let slot = CommaOkValueSlot::Named(go_name.to_string());
            return Some(
                self.lower_default_into_slot(call.fuse, call.default, slot)
                    .0,
            );
        };
        let FusedCall::Option(fuse) = call.fuse else {
            unreachable!("a mapped chain is recognized on Option receivers only");
        };
        let ty = value.get_type();
        let mut statements = vec![LoweredStatement::VarDecl {
            name: go_name.to_string().into(),
            go_type: self.use_go_type(&ty),
            value: None,
        }];
        statements.extend(self.lower_mapped_default_into(fuse, map, call.default, go_name, &ty));
        Some(statements)
    }

    pub(crate) fn lower_defaulted_call_value(
        &mut self,
        expression: &Expression,
    ) -> Option<ValuePlan> {
        let call = self.defaulted_call(expression)?;
        if let Some((setup, read)) = self.zero_defaulted_map_read(&call, &expression.get_type()) {
            return Some(ValuePlan::computed(setup, read, EvaluationEffect::Pure));
        }
        let Some(map) = call.map else {
            let (statements, value) =
                self.lower_default_into_slot(call.fuse, call.default, CommaOkValueSlot::Temp);
            return Some(ValuePlan::captured(statements, value));
        };
        let FusedCall::Option(fuse) = call.fuse else {
            unreachable!("a mapped chain is recognized on Option receivers only");
        };
        let ty = expression.get_type();
        let (result_var, declaration) = self.operand_temp_declaration(&ty);
        let mut statements = vec![declaration];
        statements.extend(self.lower_mapped_default_into(
            fuse,
            map,
            call.default,
            &result_var,
            &ty,
        ));
        collapse_declared_temp(
            &mut statements,
            &result_var,
            self.short_declaration_keeps_type(&ty),
        );
        Some(ValuePlan::captured(statements, result_var))
    }
}
