use crate::control_flow::fallible as tags;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::types::Type;

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::{CoercionPlan, LayoutBridge};
use crate::abi::wrapping::WrapperTarget;
use crate::control_flow::fallible::{
    OPTION_SOME_TAG, PARTIAL_ERR_TAG, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::{
    Definition, ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, Statement,
};
use crate::ir::go_expression::BinaryOp;
use crate::ir::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::{PartialVariant, PreludeVariant, prelude_constructor};
use crate::state::expression::ExpressionContext;
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::program::{Adaptation, SlotOrigin};

pub(crate) fn multi_value_return(values: Vec<GoExpression>) -> Statement {
    LoweredStatement::Return(values).into()
}

pub(crate) fn tag_check(
    condition: GoExpression,
    setup: Vec<Statement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    tag_check_with_initializer(None, condition, setup, then_values)
}

pub(crate) fn tag_check_with_initializer(
    initializer: Option<Definition>,
    condition: GoExpression,
    setup: Vec<Statement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    let mut statements = setup;
    statements.push(multi_value_return(then_values));
    LoweredStatement::If(IfPlan {
        initializer,
        condition,
        then_body: LoweredBlock { statements },
        else_arm: ElseArm::None,
        else_if_line: None,
        origin: IfOrigin::User,
    })
}

fn tag_condition(value: &GoExpression, tag: &str) -> GoExpression {
    GoExpression::binary(
        GoExpression::selector(value.clone(), ENUM_TAG_FIELD.to_string()),
        BinaryOp::Eq,
        GoExpression::generated(GeneratedPackage::Prelude, tag),
    )
}

fn field(value: &GoExpression, field: &str) -> GoExpression {
    GoExpression::selector(value.clone(), field.to_string())
}

impl Planner<'_> {
    fn plan_abi_to_tagged(
        &mut self,
        raw_value: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> (Vec<Statement>, GoExpression) {
        let (wrap, outcome) =
            self.plan_abi_wrapping(raw_value, abi, result_ty, None, WrapperTarget::FreshSlot);
        (wrap, outcome.expect("wrapper produced no slot"))
    }

    pub(crate) fn lower_abi_to_tagged_return(
        &mut self,
        raw_value: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> Vec<Statement> {
        let (statements, outcome) =
            self.plan_abi_wrapping(raw_value, abi, result_ty, None, WrapperTarget::Return);
        debug_assert!(outcome.is_none(), "Return target emits its own returns");
        statements
    }

    pub(crate) fn lowered_err_values(
        &mut self,
        shape: &LoweredReturnAbi,
        return_ty: &Type,
        err_expr: GoExpression,
    ) -> Vec<GoExpression> {
        match shape {
            LoweredReturnAbi::BareError => vec![err_expr],
            LoweredReturnAbi::Result { .. } | LoweredReturnAbi::Partial { .. } => {
                let ok_ty = self.facts.ok_payload(return_ty);
                let mut values = self.lowered_payload_zeros(shape, &ok_ty);
                values.push(err_expr);
                values
            }
            LoweredReturnAbi::Tuple { .. } => {
                unreachable!("a tuple return has no failure path")
            }
            LoweredReturnAbi::Option(_) => {
                unreachable!("Option's failure constructor `None` carries no payload")
            }
        }
    }

    pub(crate) fn lowered_none_values(
        &mut self,
        shape: &LoweredReturnAbi,
        return_ty: &Type,
    ) -> Vec<GoExpression> {
        match shape {
            LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
                let inner = self.facts.ok_payload(return_ty);
                let mut values = self.lowered_payload_zeros(shape, &inner);
                values.push(GoExpression::literal("false".to_string()));
                values
            }
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable) => vec![GoExpression::nil()],
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
                vec![GoExpression::literal(value.to_string())]
            }
            _ => unreachable!("only Option's `None` lacks a payload"),
        }
    }

    pub(crate) fn lowered_payload_values(
        &mut self,
        shape: &LoweredReturnAbi,
        payload_ty: &Type,
        payload_expr: GoExpression,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        if shape.has_flattened_payload() {
            let mut statements = Vec::new();
            let tuple = self.stable_source(&mut statements, "tup", payload_expr);
            let (projection, values) = self.lowered_tuple_values(&tuple, payload_ty, None);
            statements.extend(projection);
            (statements, values)
        } else {
            (Vec::new(), vec![payload_expr])
        }
    }

    fn lowered_payload_zeros(
        &mut self,
        shape: &LoweredReturnAbi,
        payload_ty: &Type,
    ) -> Vec<GoExpression> {
        if shape.has_flattened_payload() {
            self.facts
                .tuple_element_types(payload_ty)
                .iter()
                .map(|slot_ty| {
                    if self.facts.is_nullable_option(slot_ty) {
                        GoExpression::nil()
                    } else {
                        self.go_zero(slot_ty)
                    }
                })
                .collect()
        } else {
            vec![self.go_zero(payload_ty)]
        }
    }

    pub(crate) fn lower_result_return(
        &mut self,
        result_value: GoExpression,
        hint: &'static str,
        return_ty: &Type,
        shape: &LoweredReturnAbi,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let p = &self.stable_source(&mut statements, hint, result_value);
        let ok_ty = || self.facts.ok_payload(return_ty);
        let arms = match shape {
            LoweredReturnAbi::BareError | LoweredReturnAbi::Result { .. } => {
                let (ok_setup, ok_payload) =
                    self.lowered_payload_values(shape, &ok_ty(), field(p, tags::RESULT_OK_FIELD));
                vec![
                    tag_check(
                        tag_condition(p, RESULT_OK_TAG),
                        ok_setup,
                        shape.ok_values(ok_payload),
                    )
                    .into(),
                    multi_value_return(self.lowered_err_values(
                        shape,
                        return_ty,
                        field(p, tags::RESULT_ERR_FIELD),
                    )),
                ]
            }
            LoweredReturnAbi::Partial { .. } => {
                let ok_ty = ok_ty();
                let (ok_setup, ok_payload) =
                    self.lowered_payload_values(shape, &ok_ty, field(p, tags::PARTIAL_OK_FIELD));
                let (both_setup, mut both_values) =
                    self.lowered_payload_values(shape, &ok_ty, field(p, tags::PARTIAL_OK_FIELD));
                both_values.push(field(p, tags::PARTIAL_ERR_FIELD));
                let mut statements = vec![
                    tag_check(
                        tag_condition(p, PARTIAL_OK_TAG),
                        ok_setup,
                        shape.ok_values(ok_payload),
                    )
                    .into(),
                    tag_check(
                        tag_condition(p, PARTIAL_ERR_TAG),
                        Vec::new(),
                        self.lowered_err_values(
                            shape,
                            return_ty,
                            field(p, tags::PARTIAL_ERR_FIELD),
                        ),
                    )
                    .into(),
                ];
                statements.extend(both_setup);
                statements.push(multi_value_return(both_values));
                statements
            }
            LoweredReturnAbi::Option(_) => {
                let (some_setup, some_payload) =
                    self.lowered_payload_values(shape, &ok_ty(), field(p, tags::OPTION_SOME_FIELD));
                vec![
                    tag_check(
                        tag_condition(p, OPTION_SOME_TAG),
                        some_setup,
                        shape.ok_values(some_payload),
                    )
                    .into(),
                    multi_value_return(self.lowered_none_values(shape, return_ty)),
                ]
            }
            LoweredReturnAbi::Tuple { .. } => self.lower_tuple_return(p, return_ty),
        };
        statements.extend(arms);
        statements
    }

    fn lower_tuple_return(
        &mut self,
        result_value: &GoExpression,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let (mut statements, fields) = self.lowered_tuple_values(result_value, return_ty, None);
        statements.push(multi_value_return(fields));
        statements
    }

    fn lowered_tuple_values(
        &mut self,
        tuple_value: &GoExpression,
        tuple_ty: &Type,
        adapted: Option<(&Type, &[Adaptation])>,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        let slot_tys = self.facts.tuple_element_types(tuple_ty);
        let source_tys = adapted.map(|(source, _)| self.facts.tuple_element_types(source));
        let mut statements = Vec::new();
        let fields = slot_tys
            .iter()
            .enumerate()
            .map(|(i, slot_ty)| {
                let raw = field(tuple_value, TUPLE_FIELDS[i]);
                let adaptation = adapted.and_then(|(_, adaptations)| adaptations.get(i));
                if let (Some(adaptation), Some(source_tys)) = (adaptation, &source_tys) {
                    if matches!(adaptation, Adaptation::ImplicitSome { .. })
                        && self.facts.is_nullable_option(slot_ty)
                    {
                        return raw;
                    }
                    let coercion =
                        CoercionPlan::adapted(self, adaptation, &source_tys[i], slot_ty, None);
                    let (setup, raw) = coercion.lower(self, raw);
                    statements.extend(setup);
                    return raw;
                }
                if self.facts.is_nullable_option(slot_ty) {
                    let inner = self.go_type(&slot_ty.ok_type());
                    self.plan_option_payload_wrap(
                        &mut statements,
                        raw,
                        &inner,
                        &LayoutBridge::Identity,
                        false,
                    )
                } else {
                    raw
                }
            })
            .collect();
        (statements, fields)
    }

    pub(crate) fn lowered_tuple_literal_values(
        &mut self,
        tuple: &Expression,
        tuple_ty: &Type,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        let Expression::Tuple { elements, .. } = tuple else {
            unreachable!("only a tuple literal lowers into return slots")
        };
        let slot_tys = self.facts.tuple_element_types(tuple_ty);
        let adaptations = match &self.facts.checked(tuple).value_use().adaptation {
            Adaptation::Tuple(adaptations) => adaptations.clone(),
            _ => Vec::new(),
        };
        let stages: Vec<ValuePlan> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| match slot_tys.get(i) {
                Some(slot_ty) if self.facts.is_nullable_option(slot_ty) => {
                    let adaptation = adaptations.get(i).unwrap_or(&Adaptation::Identity);
                    self.plan_nullable_slot_value(e, adaptation, slot_ty)
                }
                slot => {
                    let origin = slot.map_or(SlotOrigin::Lisette, |ty| {
                        self.function_type_origin(ty, SlotOrigin::Lisette)
                    });
                    self.plan_composite_value(
                        e,
                        ExpressionContext::value().with_function_slot_origin(origin),
                    )
                }
            })
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "ret");
        let mut statements = sequenced.setup;
        let parts = self.coerce_elements_to_slots(
            &mut statements,
            tuple,
            sequenced.values,
            &slot_tys,
            true,
        );
        (statements, parts)
    }

    pub(crate) fn reencode_return(
        &mut self,
        call: GoExpression,
        source: &CallableReturnAbi,
        target: &CallableReturnAbi,
        return_ty: &Type,
    ) -> Vec<Statement> {
        match target.lowered() {
            None => self.lower_abi_to_tagged_return(call, source, return_ty),
            Some(target) => {
                let (mut statements, tagged) = self.plan_abi_to_tagged(call, source, return_ty);
                statements.extend(self.lower_result_return(tagged, "res", return_ty, target));
                statements
            }
        }
    }

    pub(crate) fn try_lower_layout_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.return_ctx();
        let return_ty = return_ctx.ty()?.clone();
        let shape = return_ctx.lowered_shape();
        if matches!(
            shape,
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        ) {
            let adaptation = &self.facts.checked(expression).value_use().adaptation;
            if matches!(adaptation, Adaptation::ImplicitSome { .. }) {
                let (mut statements, value) = self
                    .plan_nullable_slot_value(expression, adaptation, &return_ty)
                    .into_parts();
                statements.push(plain_return(value));
                return Some(statements);
            }
        }
        let source_ty = expression.get_type();
        if self.facts.alias_declaration(&source_ty).is_some()
            || self.facts.alias_declaration(&return_ty).is_some()
            || (source_ty != return_ty && !matches!(expression, Expression::Tuple { .. }))
        {
            let abi = shape.clone().map_or_else(
                || CallableReturnAbi::unlowered(&return_ty),
                CallableReturnAbi::Lowered,
            );
            let source = self.emitted_value_layout(expression, ExpressionContext::value());
            let target = self.callable_result_layout(
                &return_ty,
                SlotOrigin::Lisette,
                Some(&return_ty),
                &abi,
            );
            let coercion = CoercionPlan::bridge(self, &source, &target);
            if !coercion.is_identity() {
                let (mut statements, value) = self
                    .plan_value(expression, ExpressionContext::value())
                    .into_parts();
                let (setup, value) = coercion.lower(self, value);
                statements.extend(setup);
                if let Some(shape) = &shape {
                    statements.extend(self.lower_result_return(value, "v", &return_ty, shape));
                } else {
                    statements.push(plain_return(value));
                }
                return Some(statements);
            }
        }
        match shape? {
            LoweredReturnAbi::Partial {
                payload: PayloadLayout::Packed,
            } => Some(self.lower_partial_tail(expression)),
            LoweredReturnAbi::Tuple { arity, .. } => Some(self.lower_tuple_tail(expression, arity)),
            _ => None,
        }
    }

    fn lower_tuple_tail(&mut self, expression: &Expression, arity: usize) -> Vec<Statement> {
        use Expression;
        let return_ty = self.return_ctx().expect_ty();
        if let Expression::Tuple { elements, .. } = expression
            && elements.len() == arity
        {
            let (mut statements, parts) = self.lowered_tuple_literal_values(expression, &return_ty);
            statements.push(multi_value_return(parts));
            return statements;
        }

        if let Some(plan) = self.plan_call(expression)
            && self.unbridged_tuple_arity(&plan, &return_ty) == Some(arity)
            && self.facts.same_type(&expression.get_type(), &return_ty)
            && plan
                .resolved
                .abi
                .function_layout(self)
                .same_result_representation(&self.function_layout(
                    Vec::new(),
                    &self.value_layout(&return_ty, SlotOrigin::Lisette),
                    CallableReturnAbi::Lowered(LoweredReturnAbi::Tuple { arity }),
                    Some(&return_ty),
                ))
        {
            let (mut statements, call) = self
                .plan_call_expression(expression, None, ExpressionContext::value())
                .into_parts();
            statements.push(plain_return(call));
            return statements;
        }

        let (mut statements, value) = self
            .plan_value(expression, ExpressionContext::value())
            .into_parts();
        let tup =
            GoExpression::identifier(self.hoist_tmp_value_statement(&mut statements, "tup", value));
        let source_ty = expression.get_type();
        let adaptations = match &self.facts.checked(expression).value_use().adaptation {
            Adaptation::Tuple(adaptations) => adaptations.clone(),
            _ => Vec::new(),
        };
        let adapted = (!adaptations.is_empty()).then_some((&source_ty, adaptations.as_slice()));
        let (projection, fields) = self.lowered_tuple_values(&tup, &return_ty, adapted);
        statements.extend(projection);
        statements.push(multi_value_return(fields));
        statements
    }

    fn lower_partial_tail(&mut self, expression: &Expression) -> Vec<Statement> {
        use Expression;
        let return_ty = self.return_ctx().expect_ty();

        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && let Some(PreludeVariant::Partial(variant)) = prelude_constructor(callee)
        {
            let mut statements = Vec::new();
            let ret = match variant {
                PartialVariant::Ok => {
                    let (setup, v) = self
                        .plan_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                    multi_value_return(vec![v, GoExpression::nil()])
                }
                PartialVariant::Err => {
                    let (setup, e) = self
                        .plan_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                    let ok_ty = self.facts.ok_payload(&return_ty);
                    multi_value_return(vec![self.go_zero(&ok_ty), e])
                }
                PartialVariant::Both => {
                    let stages = args
                        .iter()
                        .map(|arg| self.plan_composite_value(arg, ExpressionContext::value()))
                        .collect();
                    let sequenced =
                        self.sequence_values(stages, CaptureBoundary::SiblingSequence, "ret");
                    statements.extend(sequenced.setup);
                    multi_value_return(sequenced.values)
                }
            };
            statements.push(ret);
            return statements;
        }

        let (mut statements, value) = self
            .plan_value(expression, ExpressionContext::value())
            .into_parts();
        statements.extend(self.lower_result_return(
            value,
            "v",
            &return_ty,
            &LoweredReturnAbi::Partial {
                payload: PayloadLayout::Packed,
            },
        ));
        statements
    }

    fn plan_nullable_slot_value(
        &mut self,
        expression: &Expression,
        adaptation: &Adaptation,
        slot_ty: &Type,
    ) -> ValuePlan {
        use Expression;
        if matches!(adaptation, Adaptation::ImplicitSome { .. }) {
            return self.plan_nullable_payload(expression, &self.facts.ok_payload(slot_ty));
        }
        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && prelude_constructor(callee) == Some(PreludeVariant::Some)
        {
            debug_assert_eq!(args.len(), 1, "Some(...) takes exactly one arg");
            return self
                .plan_nullable_payload(&args[0], &self.facts.ok_payload(slot_ty))
                .with_constructor_stability();
        }
        if prelude_constructor(expression) == Some(PreludeVariant::None) {
            return ValuePlan::literal("nil".to_string());
        }
        let value = self.plan_value(expression, ExpressionContext::value());
        let inner = self.go_type(&slot_ty.ok_type());
        value.map_expression(|setup, value| {
            self.plan_option_payload_wrap(setup, value, &inner, &LayoutBridge::Identity, false)
        })
    }
}
