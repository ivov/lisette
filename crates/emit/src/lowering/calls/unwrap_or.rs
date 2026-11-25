use crate::Planner;
use crate::lowering::abi::fallible_producer::{FallibleProducer, OptionFusePlan};
use crate::lowering::calls::bounds_checks::IndexSite;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::patterns::matching::{FusedName, OptionArms, field_binding};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::placement::collapse_declared_temp;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, declare, define,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::program::{SlotOrigin, strip_prelude_prefix};
use syntax::types::{SimpleKind, Type};

struct DefaultedCall<'a> {
    producer: DefaultedProducer<'a>,
    default: &'a Expression,
}

enum DefaultedProducer<'a> {
    Plain(FallibleProducer<'a>),
    Mapped {
        producer: OptionFusePlan<'a>,
        map: MapLambda<'a>,
    },
}

pub(in crate::lowering) struct MapLambda<'a> {
    pub(in crate::lowering) param: Option<FusedName<'a>>,
    pub(in crate::lowering) body: &'a Expression,
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
    let Expression::Literal { literal, .. } = default else {
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

fn is_go_zero_literal(default: &Expression, kind: SimpleKind) -> bool {
    let Expression::Literal { literal, .. } = default else {
        return false;
    };
    match literal {
        Literal::Integer { value, .. } => *value == 0 && kind.is_arithmetic(),
        Literal::Float { value, .. } => {
            *value == 0.0 && value.is_sign_positive() && kind.is_float()
        }
        Literal::Boolean(value) => !value && kind == SimpleKind::Bool,
        Literal::String { value, .. } => value.is_empty() && kind == SimpleKind::String,
        _ => false,
    }
}

pub(in crate::lowering) fn map_lambda(function: &Expression) -> Option<MapLambda<'_>> {
    let Expression::Lambda { params, body, .. } = function else {
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
    fn zero_defaulted_map_read(
        &mut self,
        call: &DefaultedCall<'_>,
        ty: &Type,
    ) -> Option<(Vec<Statement>, GoExpression)> {
        let DefaultedProducer::Plain(FallibleProducer::Option(OptionFusePlan::CommaOk {
            subject,
            source,
        })) = &call.producer
        else {
            return None;
        };
        let receiver = source.map_index_receiver()?;
        if !self
            .facts
            .simple_kind(ty)
            .is_some_and(|kind| is_go_zero_literal(call.default, kind))
        {
            return None;
        }
        Some(self.plan_map_index_pair(*subject, receiver))
    }

    fn plan_default_into_slot(
        &mut self,
        producer: FallibleProducer<'_>,
        default: &Expression,
        ty: &Type,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, GoIdentifier) {
        let (bound, value) = match producer {
            FallibleProducer::Option(OptionFusePlan::Index { site }) => {
                return self.plan_defaulted_index_into_slot(site, default, ty, slot);
            }
            producer => producer.payload(self, slot),
        };

        let failure = bound.failure_condition();
        let mut statements = bound.statements;
        let literal_default = is_literal_default(default);
        let origin = self.facts.function_type_origin(ty, SlotOrigin::Lisette);
        let value_plan = self.plan_value(
            default,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let value_plan = if literal_default {
            value_plan
        } else {
            self.eager_operand(value_plan, "default")
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
        site: IndexSite<'_>,
        default: &Expression,
        ty: &Type,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, GoIdentifier) {
        let (mut statements, index) = self.lower_bounds_checked_index(site);
        let value = match slot {
            CommaOkValueSlot::Named(name) => name,
            _ => self.fresh_declared("ret"),
        };
        let origin = self.facts.function_type_origin(ty, SlotOrigin::Lisette);
        let plan = self.plan_value(
            default,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let coercion = self.value_slot_coercion(default, ty);
        let typed = self.queries().declaration_needs_go_type(
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
            LoweredStatement::VarDeclaration {
                name: value.clone(),
                go_type: self.queries().go_type(ty),
                value: Some(default),
            }
            .into()
        } else {
            define(value.clone(), default)
        });
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: None,
                condition: index.condition(Polarity::Positive),
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

    pub(in crate::lowering) fn try_lower_defaulted_call_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let call = self.queries().defaulted_call(value)?;
        if let Some((mut statements, read)) = self.zero_defaulted_map_read(&call, &value.get_type())
        {
            self.scope.declare_go_name(go_name.spelling());
            statements.push(define(go_name.clone(), read));
            return Some(statements);
        }
        self.scope.declare_go_name(go_name.spelling());
        let ty = value.get_type();
        let (producer, map) = match call.producer {
            DefaultedProducer::Plain(producer) => {
                let slot = CommaOkValueSlot::Named(go_name.clone());
                return Some(
                    self.plan_default_into_slot(producer, call.default, &ty, slot)
                        .0,
                );
            }
            DefaultedProducer::Mapped { producer, map } => (producer, map),
        };

        let mut statements = vec![declare(go_name.clone(), self.queries().go_type(&ty))];
        statements.extend(self.lower_mapped_default_into(
            producer,
            map,
            call.default,
            go_name,
            &ty,
        ));
        Some(statements)
    }

    pub(in crate::lowering) fn try_plan_defaulted_call_value(
        &mut self,
        expression: &Expression,
    ) -> Option<ValuePlan> {
        let call = self.queries().defaulted_call(expression)?;
        if let Some((setup, read)) = self.zero_defaulted_map_read(&call, &expression.get_type()) {
            return Some(ValuePlan::computed(setup, read));
        }
        let ty = expression.get_type();
        let (producer, map) = match call.producer {
            DefaultedProducer::Plain(producer) => {
                let (statements, value) = self.plan_default_into_slot(
                    producer,
                    call.default,
                    &ty,
                    CommaOkValueSlot::Temp,
                );

                return Some(ValuePlan::captured(statements, value));
            }
            DefaultedProducer::Mapped { producer, map } => (producer, map),
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

impl<'p> PlannerQueries<'_, 'p> {
    pub(in crate::lowering) fn prelude_method_call<'a>(
        &self,
        expression: &'a Expression,
        owner: &str,
        method: &str,
    ) -> Option<(&'a Expression, &'a [Expression])> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } = expression
        else {
            return None;
        };

        if spread.is_some() {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver,
            ..
        } = &**callee
        else {
            return None;
        };

        let definition = self
            .facts
            .checked(callee)
            .function_value()?
            .definition
            .as_deref()?;

        let selected = strip_prelude_prefix(definition)?.split_once('.')?;
        (selected == (owner, method)).then_some((receiver, args.as_slice()))
    }

    fn fallible_method_call<'a>(
        &self,
        expression: &'a Expression,
        method: &str,
    ) -> Option<(&'static str, &'a Expression, &'a [Expression])> {
        ["Option", "Result"].into_iter().find_map(|owner| {
            self.prelude_method_call(expression, owner, method)
                .map(|(receiver, args)| (owner, receiver, args))
        })
    }

    fn defaulted_call<'a>(&self, expression: &'a Expression) -> Option<DefaultedCall<'a>>
    where
        'p: 'a,
    {
        let (owner, receiver, map, default) = if let Some((owner, receiver, [default])) =
            self.fallible_method_call(expression, "unwrap_or")
        {
            match self.prelude_method_call(receiver, owner, "map") {
                Some((inner, [function])) => (owner, inner, Some(map_lambda(function)?), default),
                _ => (owner, receiver, None, default),
            }
        } else if let Some((owner, receiver, [default, function])) =
            self.fallible_method_call(expression, "map_or")
        {
            (owner, receiver, Some(map_lambda(function)?), default)
        } else {
            return None;
        };

        if map.is_some() && !is_literal_default(default) {
            return None;
        }
        if self
            .facts
            .resolves_to_unit(&self.facts.ok_payload(&receiver.get_type()))
        {
            return None;
        }
        let producer = match (owner, map) {
            ("Option", Some(map)) => DefaultedProducer::Mapped {
                producer: self.option_fuse_plan(receiver)?,
                map,
            },
            ("Option", None) => {
                let producer = self.option_fuse_plan(receiver)?;
                if matches!(producer, OptionFusePlan::Index { .. }) && !runs_no_code(default) {
                    return None;
                }
                DefaultedProducer::Plain(FallibleProducer::Option(producer))
            }
            (_, None) => {
                let producer = self.result_fuse_plan(receiver)?;
                if !producer.carries_payload() {
                    return None;
                }
                DefaultedProducer::Plain(FallibleProducer::Result(producer))
            }
            (_, Some(_)) => return None,
        };

        Some(DefaultedCall { producer, default })
    }
}
