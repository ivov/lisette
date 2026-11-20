use super::NativeCallContext;
use crate::Planner;
use crate::abi::fallible_producer::{FallibleProducer, OptionFusePlan};
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, Statement, assign,
    declare, define,
};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{GoExpression, ValuePlan};
use crate::patterns::matching::{FusedName, OptionArms, field_binding};
use crate::plan::placement::collapse_declared_temp;
use crate::state::expression::ExpressionContext;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::program::SlotOrigin;
use syntax::types::Type;

struct DefaultedCall<'a> {
    producer: FallibleProducer<'a>,
    map: Option<MapLambda<'a>>,
    default: &'a Expression,
}

pub(crate) struct MapLambda<'a> {
    pub(crate) param: Option<FusedName<'a>>,
    pub(crate) body: &'a Expression,
}

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

fn runs_no_code(expression: &Expression) -> bool {
    match expression {
        Expression::Literal {
            literal: Literal::FormatString(_),
            ..
        } => false,
        Expression::Literal { .. }
        | Expression::Identifier { .. }
        | Expression::Paren { .. }
        | Expression::Unary { .. }
        | Expression::DotAccess { .. }
        | Expression::StructCall { .. } => expression.children().into_iter().all(runs_no_code),
        Expression::Binary { operator, .. } => {
            *operator != BinaryOperator::Pipeline
                && expression.children().into_iter().all(runs_no_code)
        }
        _ => false,
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
            return (!escapes_lambda(body)).then_some(MapLambda { param: None, body });
        }
        [param] => param,
        _ => return None,
    };
    let param = field_binding(&param.pattern)?;
    (!escapes_lambda(body)).then_some(MapLambda { param, body })
}

impl<'p> Planner<'p> {
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

    fn defaulted_call<'a>(&self, expression: &'a Expression) -> Option<DefaultedCall<'a>>
    where
        'p: 'a,
    {
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
        if map.is_some() && !is_literal_default(default) {
            return None;
        }
        let receiver_ty = self.facts.peel_alias(&receiver.get_type());
        if (receiver_ty.is_option() || receiver_ty.is_result())
            && self.facts.resolves_to_unit(&receiver_ty.ok_type())
        {
            return None;
        }
        let producer = if receiver_ty.is_option() {
            let producer = self.option_fuse_plan(receiver)?;
            if matches!(producer, OptionFusePlan::Index { .. })
                && map.is_none()
                && !runs_no_code(default)
            {
                return None;
            }
            FallibleProducer::Option(producer)
        } else if receiver_ty.is_result() && map.is_none() {
            let producer = self.result_fuse_plan(receiver)?;
            if !producer.carries_payload() {
                return None;
            }
            FallibleProducer::Result(producer)
        } else {
            return None;
        };
        Some(DefaultedCall {
            producer,
            map,
            default,
        })
    }

    fn zero_defaulted_map_read(
        &mut self,
        call: &DefaultedCall<'_>,
        ty: &Type,
    ) -> Option<(Vec<Statement>, GoExpression)> {
        let FallibleProducer::Option(OptionFusePlan::CommaOk { subject, source }) = &call.producer
        else {
            return None;
        };
        if call.map.is_some()
            || !source.is_map_index()
            || !is_go_zero_literal(call.default, &self.facts.peel_alias(ty))
        {
            return None;
        }
        Some(self.plan_map_index_pair(subject))
    }

    fn plan_default_into_slot(
        &mut self,
        producer: FallibleProducer<'_>,
        default: &Expression,
        ty: &Type,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, GoIdentifier) {
        let (bound, value) = match producer {
            FallibleProducer::Option(OptionFusePlan::Index { call }) => {
                return self.plan_defaulted_index_into_slot(&call, default, ty, slot);
            }
            producer => producer.payload(self, slot),
        };
        let failure = bound.failure_condition();
        let mut statements = bound.statements;
        let literal_default = is_literal_default(default);
        let origin = self.function_type_origin(ty, SlotOrigin::Lisette);
        let value_plan = self.plan_value(
            default,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let value_plan = if literal_default {
            value_plan
        } else {
            self.eager_operand(default, value_plan, "default")
        };
        let (default_setup, default_value) = value_plan.into_parts();
        statements.extend(default_setup);
        let default_value =
            self.apply_type_coercion(&mut statements, Some(ty), default, default_value);
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: failure.initializer,
                condition: failure.condition,
                then_body: LoweredBlock {
                    statements: vec![assign(
                        GoExpression::identifier(value.clone()),
                        default_value,
                    )],
                },
                else_arm: ElseArm::None,
                else_if_line: None,
                origin: IfOrigin::User,
            })
            .into(),
        );
        (statements, value)
    }

    fn plan_defaulted_index_into_slot(
        &mut self,
        call: &NativeCallContext<'_>,
        default: &Expression,
        ty: &Type,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, GoIdentifier) {
        let (mut statements, index) = self.lower_bounds_checked_index(call);
        let value = match slot {
            CommaOkValueSlot::Named(name) => name,
            _ => self.fresh_declared("ret"),
        };
        let origin = self.function_type_origin(ty, SlotOrigin::Lisette);
        let plan = self.plan_value(
            default,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let coercion = self.value_slot_coercion(default, ty);
        let typed = self.declaration_needs_go_type(
            default,
            plan.expression().constant_kind(),
            &coercion,
            ty,
        );
        let (setup, default) = plan.into_parts();
        statements.extend(setup);
        let (coercion_setup, default) = coercion.lower(self, default);
        statements.extend(coercion_setup);
        statements.push(if typed {
            LoweredStatement::VarDecl {
                name: value.clone(),
                go_type: self.go_type(ty),
                value: Some(default),
            }
            .into()
        } else {
            define(value.clone(), default)
        });
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: None,
                condition: index.condition(true),
                then_body: LoweredBlock {
                    statements: vec![assign(
                        GoExpression::identifier(value.clone()),
                        index.element(),
                    )],
                },
                else_arm: ElseArm::None,
                else_if_line: None,
                origin: IfOrigin::User,
            })
            .into(),
        );
        (statements, value)
    }

    fn lower_mapped_default_into(
        &mut self,
        producer: OptionFusePlan<'_>,
        map: MapLambda<'_>,
        default: &Expression,
        target: &GoIdentifier,
        target_ty: &Type,
    ) -> Vec<Statement> {
        let arms = OptionArms {
            some_binding: map.param,
            some_body: map.body,
            none_body: default,
        };
        let target = GoExpression::identifier(target.clone());
        self.lower_fused_option_arms(
            producer,
            arms,
            &PlacePlan::Assign {
                local: &target,
                target_ty: Some(target_ty),
            },
        )
    }

    pub(crate) fn try_lower_defaulted_call_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let call = self.defaulted_call(value)?;
        if let Some((mut statements, read)) = self.zero_defaulted_map_read(&call, &value.get_type())
        {
            self.declare(go_name.spelling());
            statements.push(define(go_name.clone(), read));
            return Some(statements);
        }
        self.declare(go_name.spelling());
        let ty = value.get_type();
        let Some(map) = call.map else {
            let slot = CommaOkValueSlot::Named(go_name.clone());
            return Some(
                self.plan_default_into_slot(call.producer, call.default, &ty, slot)
                    .0,
            );
        };
        let FallibleProducer::Option(producer) = call.producer else {
            unreachable!("a mapped chain is recognized on Option receivers only");
        };
        let mut statements = vec![declare(go_name.clone(), self.go_type(&ty))];
        statements.extend(self.lower_mapped_default_into(
            producer,
            map,
            call.default,
            go_name,
            &ty,
        ));
        Some(statements)
    }

    pub(crate) fn try_plan_defaulted_call_value(
        &mut self,
        expression: &Expression,
    ) -> Option<ValuePlan> {
        let call = self.defaulted_call(expression)?;
        if let Some((setup, read)) = self.zero_defaulted_map_read(&call, &expression.get_type()) {
            return Some(ValuePlan::computed(setup, read));
        }
        let ty = expression.get_type();
        let Some(map) = call.map else {
            let (statements, value) = self.plan_default_into_slot(
                call.producer,
                call.default,
                &ty,
                CommaOkValueSlot::Temp,
            );
            return Some(ValuePlan::captured(statements, value));
        };
        let FallibleProducer::Option(producer) = call.producer else {
            unreachable!("a mapped chain is recognized on Option receivers only");
        };
        let (result_var, declaration) = self.operand_temp_declaration(&ty);
        let mut statements = vec![declaration];
        statements.extend(self.lower_mapped_default_into(
            producer,
            map,
            call.default,
            &result_var,
            &ty,
        ));
        collapse_declared_temp(
            &mut statements,
            &result_var,
            self.facts.short_declaration_keeps_type(&ty),
        );
        Some(ValuePlan::captured(statements, result_var))
    }
}
