use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::expressions::tuple_literal::TupleLiteral;
use crate::target::ir::prelude as tags;
use crate::target::names::go_name;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::types::Type;

use crate::Planner;
use crate::lowering::abi::coercion_plan::{CoercionPlan, LayoutChange};
use crate::lowering::abi::go_result_wrapping::WrapperTarget;
use crate::lowering::abi::layout_change::OptionSlot;
use crate::lowering::calls::go_interop::LoweredProducer;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::expressions::values::ElementTarget;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::{
    Definition, ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, Statement,
};
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::prelude::{PartialVariant, PreludeVariant, prelude_constructor};
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::return_abi::{
    CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout,
};
use syntax::ast::Expression;
use syntax::program::{Adaptation, SlotOrigin};

pub(in crate::lowering) fn multi_value_return(values: Vec<GoExpression>) -> Statement {
    LoweredStatement::Return(values).into()
}

pub(in crate::lowering) fn tag_check(
    condition: GoExpression,
    setup: Vec<Statement>,
    then_values: Vec<GoExpression>,
) -> LoweredStatement {
    tag_check_with_initializer(None, condition, setup, then_values)
}

pub(in crate::lowering) fn tag_check_with_initializer(
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

pub(super) fn tag_condition(value: &GoExpression, tag: &str) -> GoExpression {
    GoExpression::binary(
        GoExpression::selector(value.clone(), ENUM_TAG_FIELD.to_string()),
        GoBinaryOperator::Equal,
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
        producer: LoweredProducer<'_>,
        result_ty: &Type,
    ) -> (Vec<Statement>, GoExpression) {
        let (wrap, outcome) = self.plan_abi_wrapping(
            raw_value,
            producer,
            result_ty,
            None,
            WrapperTarget::FreshSlot,
        );

        (wrap, outcome.expect("wrapper produced no slot"))
    }

    pub(in crate::lowering) fn lower_abi_to_tagged_return(
        &mut self,
        raw_value: GoExpression,
        producer: LoweredProducer<'_>,
        result_ty: &Type,
    ) -> Vec<Statement> {
        let (statements, outcome) =
            self.plan_abi_wrapping(raw_value, producer, result_ty, None, WrapperTarget::Return);
        debug_assert!(outcome.is_none(), "Return target emits its own returns");
        statements
    }

    pub(in crate::lowering) fn lowered_err_values(
        &self,
        payload: Option<PayloadLayout>,
        return_ty: &Type,
        err_expression: GoExpression,
    ) -> Vec<GoExpression> {
        let Some(payload) = payload else {
            return vec![err_expression];
        };
        let ok_ty = self.facts.ok_payload(return_ty);
        let mut values = self.lowered_payload_zeros(payload, &ok_ty);
        values.push(err_expression);
        values
    }

    pub(in crate::lowering) fn lowered_none_values(
        &self,
        option: OptionReturnAbi,
        return_ty: &Type,
    ) -> Vec<GoExpression> {
        match option {
            OptionReturnAbi::CommaOk { payload } => {
                let inner = self.facts.ok_payload(return_ty);
                let mut values = self.lowered_payload_zeros(payload, &inner);
                values.push(GoExpression::boolean(false));
                values
            }
            OptionReturnAbi::Nullable => vec![GoExpression::nil()],
            OptionReturnAbi::Sentinel(value) => vec![GoExpression::integer(i128::from(value))],
        }
    }

    pub(in crate::lowering) fn lowered_payload_values(
        &mut self,
        shape: &LoweredReturnAbi,
        payload_ty: &Type,
        payload_expression: GoExpression,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        if shape.has_flattened_payload() {
            let mut statements = Vec::new();
            let tuple = self.stable_source(&mut statements, "tup", payload_expression);
            let (projection, values) = self.lowered_tuple_values(&tuple, payload_ty, None);
            statements.extend(projection);
            (statements, values)
        } else {
            (Vec::new(), vec![payload_expression])
        }
    }

    fn lowered_payload_zeros(
        &self,
        payload: PayloadLayout,
        payload_ty: &Type,
    ) -> Vec<GoExpression> {
        if payload.is_flattened() {
            self.facts
                .lowered_tuple_slot_layouts(payload_ty)
                .iter()
                .map(|slot| self.go_zero_of_layout(slot))
                .collect()
        } else {
            vec![self.go_zero(payload_ty)]
        }
    }

    pub(in crate::lowering) fn lower_result_return(
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
                let (ok_setup, ok_payload) = self.lowered_payload_values(
                    shape,
                    &ok_ty(),
                    field(p, tags::result_variant("Ok").fields[0]),
                );
                vec![
                    tag_check(
                        tag_condition(p, tags::result_variant("Ok").tag),
                        ok_setup,
                        shape.ok_values(ok_payload),
                    )
                    .into(),
                    multi_value_return(self.lowered_err_values(
                        shape.payload(),
                        return_ty,
                        field(p, tags::result_variant("Err").fields[0]),
                    )),
                ]
            }
            LoweredReturnAbi::Partial { .. } => self.partial_return_arms(p, return_ty, shape),
            LoweredReturnAbi::Option(option) => {
                let (some_setup, some_payload) = self.lowered_payload_values(
                    shape,
                    &ok_ty(),
                    field(p, tags::option_variant("Some").fields[0]),
                );
                vec![
                    tag_check(
                        tag_condition(p, tags::option_variant("Some").tag),
                        some_setup,
                        shape.ok_values(some_payload),
                    )
                    .into(),
                    multi_value_return(self.lowered_none_values(*option, return_ty)),
                ]
            }
            LoweredReturnAbi::Tuple { .. } => self.lower_tuple_return(p, return_ty),
        };

        statements.extend(arms);
        statements
    }

    fn partial_return_arms(
        &mut self,
        partial: &GoExpression,
        return_ty: &Type,
        shape: &LoweredReturnAbi,
    ) -> Vec<Statement> {
        let ok_ty = self.facts.ok_payload(return_ty);
        let (ok_setup, ok_payload) = self.lowered_payload_values(
            shape,
            &ok_ty,
            field(partial, tags::partial_variant("Ok").fields[0]),
        );
        let [both_ok, both_err] = tags::partial_variant("Both").fields else {
            unreachable!("Both carries a payload and an error")
        };
        let (both_setup, mut both_values) =
            self.lowered_payload_values(shape, &ok_ty, field(partial, both_ok));
        both_values.push(field(partial, both_err));
        let mut statements = vec![
            tag_check(
                tag_condition(partial, tags::partial_variant("Ok").tag),
                ok_setup,
                shape.ok_values(ok_payload),
            )
            .into(),
            tag_check(
                tag_condition(partial, tags::partial_variant("Err").tag),
                Vec::new(),
                self.lowered_err_values(
                    shape.payload(),
                    return_ty,
                    field(partial, tags::partial_variant("Err").fields[0]),
                ),
            )
            .into(),
        ];

        statements.extend(both_setup);
        statements.push(multi_value_return(both_values));
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
        let slot_tys = self.facts.tuple_or_type_arguments(tuple_ty);
        let source_ty = adapted.map_or(tuple_ty, |(source, _)| source);
        let source_tys = self.facts.tuple_or_type_arguments(source_ty);
        let source_layout = self.facts.value_layout(source_ty, SlotOrigin::Lisette);
        let source_layouts = source_layout
            .tuple_elements()
            .expect("a tuple type has a tuple layout");
        let slot_layouts = self.facts.lowered_tuple_slot_layouts(tuple_ty);
        let mut statements = Vec::new();
        let fields = slot_tys
            .iter()
            .enumerate()
            .map(|(i, slot_ty)| {
                let raw = field(tuple_value, go_name::tuple_field(i));
                let adaptation =
                    adapted.map_or(&Adaptation::Identity, |(_, adaptations)| &adaptations[i]);
                let coercion = CoercionPlan::into_go_slot(
                    &self.queries(),
                    adaptation,
                    (&source_tys[i], slot_ty),
                    (&source_layouts[i], &slot_layouts[i]),
                );
                let (setup, raw) = coercion.lower(self, raw);
                statements.extend(setup);
                raw
            })
            .collect();

        (statements, fields)
    }

    pub(in crate::lowering) fn lowered_tuple_literal_values(
        &mut self,
        literal: TupleLiteral<'_>,
        tuple_ty: &Type,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        let tuple = literal.expression;
        let elements = literal.elements;
        let slot_tys = self.facts.tuple_or_type_arguments(tuple_ty);
        let slot_layouts = self.facts.lowered_tuple_slot_layouts(tuple_ty);
        let adaptation = self.facts.checked(tuple).value_use().adaptation.clone();
        let element_adaptation = |index: usize| match &adaptation {
            Adaptation::Tuple(elements) => elements[index].clone(),
            Adaptation::Identity => Adaptation::Identity,
            other @ Adaptation::Delegated { .. }
            | other @ Adaptation::Interface(_)
            | other @ Adaptation::ImplicitSome { .. }
            | other @ Adaptation::Newtype
            | other @ Adaptation::Array(_)
            | other @ Adaptation::DivergingFunction
            | other @ Adaptation::Receiver(_) => {
                panic!("internal error: a tuple literal has the adaptation {other:?}")
            }
        };
        let operands: Vec<ValuePlan> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| match slot_tys.get(i) {
                Some(slot_ty)
                    if matches!(
                        slot_layouts.get(i),
                        Some(ValueLayout::NullableOption { .. })
                    ) =>
                {
                    self.plan_nullable_slot_value(e, &element_adaptation(i), slot_ty)
                }
                slot => {
                    let origin = slot.map_or(SlotOrigin::Lisette, |ty| {
                        self.facts.function_type_origin(ty, SlotOrigin::Lisette)
                    });
                    self.plan_composite_value(
                        e,
                        ExpressionContext::value().with_function_slot_origin(origin),
                    )
                }
            })
            .collect();

        let sequenced = self.sequence_values(operands, CaptureBoundary::SiblingSequence, "ret");
        let mut statements = sequenced.setup;
        let parts = self.coerce_elements_to_slots(
            &mut statements,
            literal,
            sequenced.values,
            &slot_tys,
            ElementTarget::LoweredReturn,
        );

        (statements, parts)
    }

    pub(in crate::lowering) fn reencode_return(
        &mut self,
        call: GoExpression,
        source: &FunctionLayout,
        target: &CallableReturnAbi,
    ) -> Vec<Statement> {
        let return_ty = source.result_layout.logical_type();
        let abi = source.return_abi();
        let producer = LoweredProducer {
            abi: &abi,
            producer: source.producer,
        };
        match target.lowered() {
            None => self.lower_abi_to_tagged_return(call, producer, return_ty),
            Some(target) => {
                let (mut statements, tagged) = self.plan_abi_to_tagged(call, producer, return_ty);
                statements.extend(self.lower_result_return(tagged, "res", return_ty, target));
                statements
            }
        }
    }

    pub(in crate::lowering) fn try_lower_layout_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.scope.current_return_ctx();
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
        if !matches!(expression, Expression::Tuple { .. }) {
            let abi = shape.clone().map_or_else(
                || CallableReturnAbi::unlowered(&return_ty),
                CallableReturnAbi::Lowered,
            );
            let source = self
                .queries()
                .emitted_value_layout(expression, ExpressionContext::value());
            let target = self.facts.callable_result_layout(
                &return_ty,
                SlotOrigin::Lisette,
                Some(&return_ty),
                &abi,
            );

            let coercion = self.value_layout_change(expression, &source, &target);
            if coercion.changes_layout() {
                self.record_converted_value(expression);
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
            LoweredReturnAbi::BareError
            | LoweredReturnAbi::Result { .. }
            | LoweredReturnAbi::Partial { .. }
            | LoweredReturnAbi::Option(_) => None,
        }
    }

    fn lower_tuple_tail(&mut self, expression: &Expression, arity: usize) -> Vec<Statement> {
        let return_ty = self.scope.current_return_ctx().expect_ty();
        if let Some(literal) = TupleLiteral::of(expression)
            && literal.elements.len() == arity
        {
            let (mut statements, parts) = self.lowered_tuple_literal_values(literal, &return_ty);
            statements.push(multi_value_return(parts));
            return statements;
        }

        if let Some(plan) = CallSite::of(expression).map(|site| self.queries().plan_call(site))
            && self.queries().kept_layout_tuple_arity(&plan, &return_ty) == Some(arity)
            && self.facts.same_type(&expression.get_type(), &return_ty)
            && plan
                .resolved
                .abi
                .function_layout(&self.facts)
                .same_result_representation(&self.facts.function_layout(
                    Vec::new(),
                    &self.facts.value_layout(&return_ty, SlotOrigin::Lisette),
                    CallableReturnAbi::Lowered(LoweredReturnAbi::Tuple { arity }),
                    Some(&return_ty),
                ))
        {
            let (mut statements, call) = self
                .plan_call_with_plan(plan, None, ExpressionContext::value())
                .into_parts();
            statements.push(plain_return(call));
            return statements;
        }

        let mut plan = self.plan_value(expression, ExpressionContext::value());
        if matches!(
            self.facts.checked(expression).value_use().adaptation,
            Adaptation::Newtype
        ) {
            plan = self.convert_to_recorded_slot(expression, plan);
        }
        let (mut statements, value) = plan.into_parts();
        let tuple = GoExpression::identifier(self.hoist_temp(&mut statements, "tup", value));
        let source_ty = expression.get_type();
        let adaptations = self.applied_element_adaptations(expression);
        let adapted = (!adaptations.is_empty()).then_some((&source_ty, adaptations.as_slice()));
        let (projection, fields) = self.lowered_tuple_values(&tuple, &return_ty, adapted);
        statements.extend(projection);
        statements.push(multi_value_return(fields));
        statements
    }

    fn lower_partial_tail(&mut self, expression: &Expression) -> Vec<Statement> {
        use Expression;
        let return_ty = self.scope.current_return_ctx().expect_ty();

        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && let Some(PreludeVariant::Partial(variant)) = prelude_constructor(callee)
        {
            let mut statements = Vec::new();
            let return_statement = match variant {
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
                    let operands = args
                        .iter()
                        .map(|arg| self.plan_composite_value(arg, ExpressionContext::value()))
                        .collect();
                    let sequenced =
                        self.sequence_values(operands, CaptureBoundary::SiblingSequence, "ret");
                    statements.extend(sequenced.setup);
                    multi_value_return(sequenced.values)
                }
            };

            statements.push(return_statement);
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
        if let Adaptation::ImplicitSome { payload } = adaptation {
            let payload_ty = self.facts.ok_payload(slot_ty);
            let conversion =
                self.adapted_slot_coercion(expression, payload, &payload_ty, &payload_ty);
            return self.plan_nullable_payload(expression, &payload_ty, conversion);
        }
        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && prelude_constructor(callee) == Some(PreludeVariant::Some)
        {
            debug_assert_eq!(args.len(), 1, "Some(...) takes exactly one arg");
            let payload_ty = self.facts.ok_payload(slot_ty);
            let conversion = self.value_slot_coercion(&args[0], &payload_ty);
            return self
                .plan_nullable_payload(&args[0], &payload_ty, conversion)
                .with_constructor_stability();
        }
        if prelude_constructor(expression) == Some(PreludeVariant::None) {
            return ValuePlan::nil();
        }
        let value = self.plan_value(expression, ExpressionContext::value());
        let inner = self.queries().go_type(&slot_ty.ok_type());
        value.map_expression(|setup, value| {
            self.plan_option_payload_wrap(
                setup,
                value,
                &inner,
                &LayoutChange::Identity,
                OptionSlot::Value,
            )
        })
    }
}
