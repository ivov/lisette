use crate::Planner;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::context::expression::ExpressionContext;
use crate::patterns::matching::{OptionArms, OptionFusePlan, ResultFusePlan};
use crate::plan::bodies::{AssignForm, ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan};
use crate::plan::placement::collapse_declared_temp;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::{Expression, Literal, Pattern};
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

struct MapLambda<'a> {
    param: Option<&'a str>,
    body: &'a Expression,
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

fn map_lambda(function: &Expression) -> Option<MapLambda<'_>> {
    let Expression::Lambda { params, body, .. } = function.unwrap_parens() else {
        return None;
    };
    let [param] = params.as_slice() else {
        return None;
    };
    let param = match &param.pattern {
        Pattern::Identifier { identifier, .. } => Some(identifier.as_str()),
        Pattern::WildCard { .. } => None,
        _ => return None,
    };
    (!escapes_lambda(body)).then_some(MapLambda { param, body })
}

impl Planner<'_> {
    fn prelude_method_call<'a>(
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

    /// `x, ok := call` then `if !ok { x = default }`, with `x` as `slot` names it.
    fn lower_default_into_slot(
        &mut self,
        fuse: FusedCall<'_>,
        default: &Expression,
        slot: CommaOkValueSlot,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut statements, failure, value) = match fuse {
            FusedCall::Result(fuse) => {
                let pair = fuse.bind(self, slot, None);
                let failure = self.pair_failure_condition(&pair);
                let value = pair
                    .value()
                    .expect("a payload slot was requested")
                    .to_string();
                (pair.statements, failure, value)
            }
            FusedCall::Option(fuse) => {
                let bound = fuse.bind(self, slot);
                let failure = bound.none_condition(self);
                let value = bound
                    .value_name()
                    .expect("a payload slot was requested")
                    .to_string();
                (bound.statements, failure, value)
            }
        };
        let literal_default = is_literal_default(default);
        let value_plan = self.lower_value(default, ExpressionContext::value());
        let mut default = if literal_default {
            value_plan
        } else {
            self.eager_operand(default, value_plan, "default")
        };
        statements.append(&mut default.setup);
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
            name: go_name.to_string(),
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
