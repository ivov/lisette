use crate::Planner;
use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};
use crate::lowering::abi::coercion_plan::{
    AggregateLayoutChange, CoercionPlan, ImmutableContainer, LayoutChange, LayoutChangeDirection,
};
use crate::lowering::abi::go_result_wrapping::option_nil_guard;
use crate::lowering::abi::go_result_wrapping::{WrapperTarget, non_nil_condition};
use crate::lowering::abi::go_returns::{multi_value_return, tag_condition};
use crate::lowering::calls::go_interop::LoweredProducer;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::output::finish::function_literals::function_literal;
use crate::target::ir::bodies::effect_statement;
use crate::target::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement,
    Statement, assign, declare, define_many,
};
use crate::target::ir::go_expression::{
    FunctionLiteralLayout, GoBinaryOperator, GoExpressionNode, GoParameter,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude as tags;
use crate::target::ir::prelude::prelude_call;
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::ast::Expression;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::program::{GO_IMPORT_PREFIX, SlotOrigin};
use syntax::types::{CompoundKind, Type};

fn some_condition(option: &GoExpression) -> GoExpression {
    tag_condition(option, tags::option_variant("Some").tag)
}

fn some_literal_argument(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args: arguments,
        spread,
        ..
    } = expression
    else {
        return None;
    };
    let [inner] = arguments.as_slice() else {
        return None;
    };
    (spread.is_none() && prelude_constructor(callee) == Some(PreludeVariant::Some)).then_some(inner)
}

fn some_payload(option: GoExpression) -> GoExpression {
    GoExpression::selector(option, tags::option_variant("Some").fields[0].to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum OptionSlot {
    Value,
    Pointer,
}

#[derive(Clone, Copy)]
struct NilableOptionWrap<'a> {
    target: &'a ValueLayout,
    target_payload: &'a ValueLayout,
    payload: &'a LayoutChange,
    slot: OptionSlot,
}

impl Planner<'_> {
    pub(in crate::lowering) fn try_plan_option_literal_into_layout(
        &mut self,
        expression: &Expression,
        target: &ValueLayout,
    ) -> Option<ValuePlan> {
        let source = self
            .facts
            .value_layout(&expression.get_type(), SlotOrigin::Lisette);
        let CoercionPlan::Layout(change) = CoercionPlan::change(&self.queries(), &source, target)
        else {
            return None;
        };
        let (payload, pointee) = match &change {
            LayoutChange::UnwrapNullableOption { payload, .. } => (payload, None),
            LayoutChange::UnwrapPointerOption {
                payload,
                target_payload,
                ..
            } => (payload, Some(target_payload)),
            _ => return None,
        };

        if prelude_constructor(expression) == Some(PreludeVariant::None) {
            return Some(ValuePlan::nil());
        }
        let inner = some_literal_argument(expression)?;
        if let LayoutChange::UnwrapNullableOption { target_payload, .. } = &change
            && let Some(plan) = self.try_plan_go_slot_payload(inner, target_payload)
        {
            return Some(plan);
        }
        let payload_ty = expression.get_type().ok_type();
        let conversion = self.value_slot_coercion(inner, &payload_ty);
        let inner = self.plan_nullable_payload(inner, &payload_ty, conversion);
        Some(inner.map_expression(|setup, value| {
            let value = if payload.is_identity() {
                value
            } else {
                self.plan_layout_change(setup, value, payload)
            };
            let Some(pointee) = pointee else {
                return value;
            };
            let go_type = pointee.go_type(&self.queries());
            let copy = self.fresh_declared("ptr");
            setup.push(
                LoweredStatement::VarDeclaration {
                    name: copy.clone(),
                    go_type,
                    value: Some(value),
                }
                .into(),
            );
            GoExpression::address_of(GoExpression::identifier(copy))
        }))
    }

    fn try_plan_go_slot_payload(
        &mut self,
        inner: &Expression,
        target_payload: &ValueLayout,
    ) -> Option<ValuePlan> {
        let context = ExpressionContext::value().with_function_slot_origin(SlotOrigin::GoField);
        let value = self.queries().peel_go_slot_alias_cast(inner, context);
        let emitted = self.queries().emitted_value_layout(value, context);
        let direct = CoercionPlan::change(&self.queries(), &emitted, target_payload);
        if matches!(value, Expression::Lambda { .. }) && direct.is_identity() {
            return Some(self.plan_composite_value(value, context));
        }
        self.queries().go_slot_declaration_layout(value, context)?;
        Some(
            self.plan_composite_value(value, context)
                .map_expression(|setup, value| {
                    let (layout_change_setup, value) = direct.lower(self, value);
                    setup.extend(layout_change_setup);
                    value
                }),
        )
    }

    pub(in crate::lowering) fn plan_nullable_payload(
        &mut self,
        payload: &Expression,
        payload_ty: &Type,
        conversion: CoercionPlan,
    ) -> ValuePlan {
        let origin = self
            .facts
            .function_type_origin(payload_ty, SlotOrigin::Lisette);
        let context = ExpressionContext::value().with_function_slot_origin(origin);
        self.plan_composite_value(payload, context)
            .map_expression(|setup, value| {
                let (slot_setup, value) = conversion.lower(self, value);
                setup.extend(slot_setup);
                value
            })
    }

    pub(in crate::lowering) fn plan_option_payload_wrap(
        &mut self,
        statements: &mut Vec<Statement>,
        option_value: GoExpression,
        slot_ty: &GoType,
        payload_layout_change: &LayoutChange,
        option_slot: OptionSlot,
    ) -> GoExpression {
        let option = self.stable_source(statements, "opt", option_value);
        let slot = self.fresh_declared(match option_slot {
            OptionSlot::Pointer => "ptr",
            OptionSlot::Value => "unwrap",
        });
        statements.push(declare(slot.clone(), slot_ty.clone()));
        let body = self.project_some_into(
            GoExpression::identifier(slot.clone()),
            option.clone(),
            payload_layout_change,
            option_slot,
        );

        statements.push(
            LoweredStatement::If(IfPlan::plain(some_condition(&option), body, ElseArm::None))
                .into(),
        );
        GoExpression::identifier(slot)
    }

    fn project_some_into(
        &mut self,
        slot: GoExpression,
        option: GoExpression,
        payload_layout_change: &LayoutChange,
        option_slot: OptionSlot,
    ) -> LoweredBlock {
        let mut statements = Vec::new();
        let payload =
            self.plan_layout_change(&mut statements, some_payload(option), payload_layout_change);
        let payload = match option_slot {
            OptionSlot::Pointer => GoExpression::address_of(payload),
            OptionSlot::Value => payload,
        };
        statements.push(assign(slot, payload));
        LoweredBlock { statements }
    }

    fn wrap_nilable_into(
        &mut self,
        slot: GoExpression,
        raw: GoExpression,
        wrap: NilableOptionWrap<'_>,
    ) -> LoweredStatement {
        let payload_type = wrap.target_payload.go_type(&self.queries());
        let type_args = GoType::type_argument(&payload_type);
        let raw_payload = match wrap.slot {
            OptionSlot::Pointer => GoExpression::dereference(raw.clone()),
            OptionSlot::Value => raw.clone(),
        };
        let mut then_statements = Vec::new();
        let payload = self.plan_layout_change(&mut then_statements, raw_payload, wrap.payload);
        let some = prelude_call(
            tags::option_variant("Some").constructor,
            type_args.clone(),
            vec![payload],
        );
        then_statements.push(assign(slot.clone(), some));
        let none = prelude_call(
            tags::option_variant("None").constructor,
            type_args,
            Vec::new(),
        );
        let condition = match wrap.slot {
            OptionSlot::Pointer => non_nil_condition(raw),
            OptionSlot::Value => {
                option_nil_guard(&self.facts, wrap.target.logical_type()).non_nil_condition(raw)
            }
        };

        LoweredStatement::If(IfPlan::plain(
            condition,
            LoweredBlock {
                statements: then_statements,
            },
            ElseArm::block(LoweredBlock {
                statements: vec![assign(slot, none)],
            }),
        ))
    }

    pub(in crate::lowering) fn plan_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        change: &LayoutChange,
    ) -> GoExpression {
        match change {
            LayoutChange::Identity => value,
            LayoutChange::Conversion { target } => {
                let target = target.go_type(&self.queries());
                GoExpression::conversion(target, value)
            }
            LayoutChange::UnwrapNullableOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(&self.queries());
                self.plan_option_payload_wrap(
                    statements,
                    value,
                    &slot_type,
                    payload,
                    OptionSlot::Value,
                )
            }
            LayoutChange::UnwrapPointerOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(&self.queries());
                let slot_type = GoType::pointer_to(&slot_type);
                self.plan_option_payload_wrap(
                    statements,
                    value,
                    &slot_type,
                    payload,
                    OptionSlot::Pointer,
                )
            }
            LayoutChange::WrapNullableOption {
                target,
                target_payload,
                payload,
            } => {
                let wrap = NilableOptionWrap {
                    target,
                    target_payload,
                    payload,
                    slot: OptionSlot::Value,
                };
                self.plan_nilable_option_wrap(statements, value, wrap)
            }
            LayoutChange::WrapPointerOption {
                target,
                target_payload,
                payload,
            } => {
                let wrap = NilableOptionWrap {
                    target,
                    target_payload,
                    payload,
                    slot: OptionSlot::Pointer,
                };
                self.plan_nilable_option_wrap(statements, value, wrap)
            }
            LayoutChange::Reference { pointee } => {
                let source = self.stable_source(statements, "src", value);
                let pointee =
                    self.plan_layout_change(statements, GoExpression::dereference(source), pointee);
                GoExpression::identifier(self.hoist_temp(
                    statements,
                    "ref",
                    GoExpression::address_of(pointee),
                ))
            }
            LayoutChange::Function { source, target, .. } => {
                self.plan_function_layout_change(statements, value, source, target)
            }
            LayoutChange::NilablePayload {
                target,
                slot,
                payload,
            } => self.plan_nilable_payload_layout_change(statements, value, target, *slot, payload),
            LayoutChange::Immutable {
                target,
                container,
                elements,
            } => self.plan_immutable_layout_change(statements, value, target, *container, elements),
            LayoutChange::Aggregate(aggregate) => {
                self.plan_aggregate_layout_change(statements, value, aggregate)
            }
        }
    }

    fn plan_nilable_payload_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        target: &ValueLayout,
        slot: OptionSlot,
        payload: &LayoutChange,
    ) -> GoExpression {
        let source = self.stable_source(statements, "raw", value);
        let output = self.fresh_declared("adapted");
        let go_type = target.go_type(&self.queries());
        statements.push(declare(output.clone(), go_type));
        let mut body = Vec::new();
        let inner = match slot {
            OptionSlot::Pointer => GoExpression::dereference(source.clone()),
            OptionSlot::Value => source.clone(),
        };
        let adapted = self.plan_layout_change(&mut body, inner, payload);
        let adapted = match slot {
            OptionSlot::Pointer => {
                let local = self.hoist_temp(&mut body, "value", adapted);
                GoExpression::address_of(GoExpression::identifier(local))
            }
            OptionSlot::Value => adapted,
        };

        body.push(assign(GoExpression::identifier(output.clone()), adapted));
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil_condition(source),
                LoweredBlock { statements: body },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(output)
    }

    fn plan_immutable_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        target: &ValueLayout,
        container: ImmutableContainer,
        elements: &[LayoutChange],
    ) -> GoExpression {
        let source = self.stable_source(statements, "src", value);
        let go_type = target.go_type(&self.queries());
        let output = self.fresh_declared("adapted");
        statements.push(declare(output.clone(), go_type));
        let output = GoExpression::identifier(output);
        let fields = immutable_layout_change_fields(container, elements.len());
        assert_eq!(
            fields.len(),
            elements.len(),
            "immutable change has one plan per payload"
        );
        if container != ImmutableContainer::Tuple {
            statements.push(assign(
                GoExpression::selector(output.clone(), ENUM_TAG_FIELD.to_string()),
                GoExpression::selector(source.clone(), ENUM_TAG_FIELD.to_string()),
            ));
        }
        for ((field, guard), change) in fields.into_iter().zip(elements) {
            let mut body = Vec::new();
            let value = self.plan_layout_change(
                &mut body,
                GoExpression::selector(source.clone(), field.to_string()),
                change,
            );
            body.push(assign(
                GoExpression::selector(output.clone(), field.to_string()),
                value,
            ));
            if let Some((tag, operator)) = guard {
                statements.push(
                    LoweredStatement::If(IfPlan {
                        initializer: None,
                        condition: GoExpression::binary(
                            GoExpression::selector(source.clone(), ENUM_TAG_FIELD.to_string()),
                            operator,
                            GoExpression::generated(GeneratedPackage::Prelude, tag),
                        ),
                        then_body: LoweredBlock { statements: body },
                        else_arm: ElseArm::None,
                        else_if_line: None,
                        origin: IfOrigin::User,
                    })
                    .into(),
                );
            } else {
                statements.extend(body);
            }
        }
        output
    }

    fn plan_nilable_option_wrap(
        &mut self,
        statements: &mut Vec<Statement>,
        raw_value: GoExpression,
        wrap: NilableOptionWrap<'_>,
    ) -> GoExpression {
        if wrap.payload.is_identity() {
            let value = match wrap.slot {
                OptionSlot::Value => self.queries().option_from_nilable(
                    raw_value,
                    wrap.target.logical_type(),
                    wrap.target_payload,
                ),
                OptionSlot::Pointer => prelude_call(
                    "OptionFromPointer",
                    GoType::type_argument(&wrap.target_payload.go_type(&self.queries())),
                    vec![raw_value],
                ),
            };
            return GoExpression::identifier(self.hoist_temp(statements, "option", value));
        }
        let source = self.stable_source(statements, "raw", raw_value);
        let option_type = wrap.target.go_type(&self.queries());
        let option = self.fresh_declared("option");
        statements.push(declare(option.clone(), option_type));
        statements.push(
            self.wrap_nilable_into(GoExpression::identifier(option.clone()), source, wrap)
                .into(),
        );
        GoExpression::identifier(option)
    }

    fn plan_aggregate_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        aggregate: &AggregateLayoutChange,
    ) -> GoExpression {
        let source = self.stable_source(statements, "src", value);
        let output = self.declare_aggregate_output(statements, aggregate, &source);

        let index = self.fresh_declared("i");
        let element = self.fresh_declared("v");
        let mut key_statements = Vec::new();
        let output_index = match aggregate.key.as_deref() {
            Some(change) => self.plan_layout_change(
                &mut key_statements,
                GoExpression::identifier(index.clone()),
                change,
            ),
            None => GoExpression::identifier(index.clone()),
        };

        let mut body = self.plan_aggregate_element_body(
            GoExpression::index(GoExpression::identifier(output.clone()), output_index),
            GoExpression::identifier(element.clone()),
            &aggregate.element,
            &aggregate.source,
        );

        if !key_statements.is_empty() {
            key_statements.append(&mut body.statements);
            body.statements = key_statements;
        }
        statements.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Range {
                    key: Some(index),
                    value: Some(element),
                    iterable: source,
                },
                body,
            })
            .into(),
        );
        GoExpression::identifier(output)
    }

    fn declare_aggregate_output(
        &mut self,
        statements: &mut Vec<Statement>,
        aggregate: &AggregateLayoutChange,
        source: &GoExpression,
    ) -> GoIdentifier {
        let direction = aggregate
            .key
            .as_deref()
            .and_then(LayoutChange::direction)
            .or_else(|| aggregate.element.direction());
        let output_hint = match direction {
            Some(LayoutChangeDirection::ToGo) => "unwrapped",
            Some(LayoutChangeDirection::FromGo) => "wrapped",
            None => "adapted",
        };
        let target_type = aggregate.target.go_type(&self.queries());
        if matches!(aggregate.target.as_ref(), ValueLayout::Array { .. }) {
            let output = self.fresh_declared(output_hint);
            statements.push(declare(output.clone(), target_type));
            return output;
        }
        let make = GoExpression::call(
            GoExpression::external_name("make".to_string()),
            vec![
                GoExpression::type_name(target_type),
                GoExpression::call(
                    GoExpression::external_name("len".to_string()),
                    vec![source.clone()],
                ),
            ],
        );
        self.hoist_temp(statements, output_hint, make)
    }

    fn plan_aggregate_element_body(
        &mut self,
        slot: GoExpression,
        element: GoExpression,
        change: &LayoutChange,
        source_layout: &ValueLayout,
    ) -> LoweredBlock {
        match change {
            LayoutChange::UnwrapNullableOption { payload, .. } => self.unwrap_option_element_body(
                slot,
                element,
                payload,
                OptionSlot::Value,
                source_layout,
            ),
            LayoutChange::UnwrapPointerOption { payload, .. } => self.unwrap_option_element_body(
                slot,
                element,
                payload,
                OptionSlot::Pointer,
                source_layout,
            ),
            LayoutChange::WrapNullableOption {
                target,
                target_payload,
                payload,
            } => {
                let wrap = NilableOptionWrap {
                    target,
                    target_payload,
                    payload,
                    slot: OptionSlot::Value,
                };
                LoweredBlock {
                    statements: vec![self.wrap_nilable_into(slot, element, wrap).into()],
                }
            }
            LayoutChange::WrapPointerOption {
                target,
                target_payload,
                payload,
            } => {
                let wrap = NilableOptionWrap {
                    target,
                    target_payload,
                    payload,
                    slot: OptionSlot::Pointer,
                };
                LoweredBlock {
                    statements: vec![self.wrap_nilable_into(slot, element, wrap).into()],
                }
            }
            LayoutChange::Conversion { .. }
            | LayoutChange::Aggregate(_)
            | LayoutChange::Immutable { .. }
            | LayoutChange::NilablePayload { .. }
            | LayoutChange::Reference { .. }
            | LayoutChange::Function { .. } => {
                let mut inner_statements = Vec::new();
                let inner = self.plan_layout_change(&mut inner_statements, element, change);
                inner_statements.push(assign(slot, inner));
                LoweredBlock {
                    statements: inner_statements,
                }
            }
            LayoutChange::Identity => LoweredBlock {
                statements: vec![assign(slot, element)],
            },
        }
    }

    fn unwrap_option_element_body(
        &mut self,
        slot: GoExpression,
        element: GoExpression,
        payload: &LayoutChange,
        option_slot: OptionSlot,
        source_layout: &ValueLayout,
    ) -> LoweredBlock {
        let then_block =
            self.project_some_into(slot.clone(), element.clone(), payload, option_slot);
        let needs_nil_else =
            matches!(source_layout, ValueLayout::Map { .. }) || option_slot == OptionSlot::Pointer;
        let else_arm = if needs_nil_else {
            ElseArm::block(LoweredBlock {
                statements: vec![assign(slot, GoExpression::nil())],
            })
        } else {
            ElseArm::None
        };

        LoweredBlock {
            statements: vec![
                LoweredStatement::If(IfPlan::plain(
                    some_condition(&element),
                    then_block,
                    else_arm,
                ))
                .into(),
            ],
        }
    }

    pub(in crate::lowering) fn plan_function_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> GoExpression {
        let names_package_function = self.names_package_function(&value);
        let never_nil = names_package_function
            || matches!(value.node(), GoExpressionNode::FunctionLiteral { .. });
        let callee = if names_package_function {
            value
        } else {
            GoExpression::identifier(self.hoist_temp(statements, "cb", value))
        };
        let mut body = Vec::new();
        let (parameters, names) = self.callback_parameters(target);
        let arguments = self.plan_layout_changed_arguments(&mut body, names, source, target);
        let call = GoExpression::call(callee.clone(), arguments);
        self.plan_function_result_layout_change(&mut body, call, source, target);
        let result = target.result_go_type(&self.queries()).unwrap_or_default();
        let callback = function_literal(
            parameters,
            result,
            LoweredBlock { statements: body },
            FunctionLiteralLayout::MultiLine,
        );

        if never_nil {
            return callback;
        }
        self.assign_callback_if_non_nil(statements, callee, callback, target)
    }

    fn names_package_function(&self, value: &GoExpression) -> bool {
        let is_declaration = |node: &GoExpressionNode| match node {
            GoExpressionNode::Identifier(name) => name.is_external(),
            GoExpressionNode::Qualified { package, name } => {
                *package == PackageUse::generated(GeneratedPackage::Prelude)
                    || self
                        .facts
                        .definition(&format!(
                            "{GO_IMPORT_PREFIX}{}.{}",
                            package.package().path(),
                            name
                        ))
                        .is_some_and(|definition| !definition.is_variable())
            }
            _ => false,
        };
        match value.node() {
            GoExpressionNode::Instantiation { base, .. } => is_declaration(base),
            node => is_declaration(node),
        }
    }

    fn callback_parameters(
        &mut self,
        target: &FunctionLayout,
    ) -> (Vec<GoParameter>, Vec<GoExpression>) {
        let mut parameters = Vec::new();
        let mut names = Vec::new();
        for (index, target) in target.parameters.iter().enumerate() {
            let name = self.scope.helper_local(&format!("arg{index}"), self.names);
            let target_type = target.go_type(&self.queries());
            parameters.push(GoParameter::new(name.clone(), target_type));
            names.push(GoExpression::identifier(name));
        }
        (parameters, names)
    }

    fn assign_callback_if_non_nil(
        &mut self,
        statements: &mut Vec<Statement>,
        callee: GoExpression,
        callback: GoExpression,
        target: &FunctionLayout,
    ) -> GoExpression {
        let adapted = self.fresh_declared("adapted");
        let go_type = target.go_type(&self.queries());
        statements.push(declare(adapted.clone(), go_type));
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil_condition(callee),
                LoweredBlock {
                    statements: vec![assign(GoExpression::identifier(adapted.clone()), callback)],
                },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(adapted)
    }

    pub(in crate::lowering) fn plan_layout_changed_arguments(
        &mut self,
        statements: &mut Vec<Statement>,
        parameters: Vec<GoExpression>,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Vec<GoExpression> {
        let spread_index = source
            .parameters
            .last()
            .filter(|last| last.logical_type().is_native(CompoundKind::VarArgs))
            .map(|_| source.parameters.len() - 1);
        parameters
            .into_iter()
            .zip(source.parameters.iter().zip(&target.parameters))
            .enumerate()
            .map(|(index, (parameter, (source, target)))| {
                let change = self.queries().resolve_layout_change(target, source);
                let argument = self.plan_layout_change(statements, parameter, &change);
                if Some(index) == spread_index {
                    GoExpression::spread(argument)
                } else {
                    argument
                }
            })
            .collect()
    }

    pub(in crate::lowering) fn plan_function_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if source.result_layout.logical_type().is_never()
            && target.return_abi != CallableReturnAbi::Void
        {
            statements.push(effect_statement(call));
            statements.push(LoweredStatement::UnreachablePanic.into());
            return;
        }
        if source.return_abi == CallableReturnAbi::Void
            && target.return_abi != CallableReturnAbi::Void
        {
            statements.push(effect_statement(call));
            statements.push(plain_return(GoExpression::unit()));
            return;
        }
        if let Some(change) = self.queries().tuple_to_value_layout_change(source, target) {
            self.push_rewrapped_result(statements, call, source, change);
            return;
        }
        if target.return_abi == CallableReturnAbi::Value
            && let Some(source) = source.single_result_layout()
        {
            self.plan_single_result_layout_change(statements, call, &source, &target.results[0]);
            return;
        }
        if let Some(change) = self
            .queries()
            .fallible_payload_layout_change(source, target)
        {
            self.push_rewrapped_result(statements, call, source, change);
            return;
        }
        if !source.return_abi.same_logical_contract(&target.return_abi) {
            self.plan_reencoded_result_layout_change(statements, call, source, target);
            return;
        }
        self.plan_same_contract_result_layout_change(statements, call, source, target);
    }

    fn push_rewrapped_result(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        change: GoResultLayoutChange,
    ) {
        let (body, _) = self.plan_abi_wrapping(
            call,
            LoweredProducer::of_layout(source),
            source.result_layout.logical_type(),
            Some(change),
            WrapperTarget::Return,
        );
        statements.extend(body);
    }

    fn plan_single_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &ValueLayout,
        target: &ValueLayout,
    ) {
        let change = self.queries().resolve_layout_change(source, target);
        let raw = self.hoist_temp(statements, "raw", call);
        let raw = GoExpression::identifier(raw);
        let value = match &change {
            LayoutChange::WrapNullableOption {
                target,
                target_payload,
                payload,
            } if payload.is_identity() => {
                self.queries()
                    .option_from_nilable(raw, target.logical_type(), target_payload)
            }
            _ => self.plan_layout_change(statements, raw, &change),
        };
        statements.push(plain_return(value));
    }

    fn plan_reencoded_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if target.return_abi.is_lowered() {
            let tagged = self
                .facts
                .value_layout(source.result_layout.logical_type(), SlotOrigin::Lisette);
            let intermediate =
                self.facts
                    .function_layout(Vec::new(), &tagged, target.return_abi.clone(), None);
            if !intermediate.same_result_representation(target) {
                let body = self.reencode_return(call, source, &intermediate.return_abi);
                let result = intermediate
                    .result_go_type(&self.queries())
                    .expect("a lowered result has a Go signature");
                let call = GoExpression::call(
                    function_literal(
                        Vec::new(),
                        result,
                        LoweredBlock { statements: body },
                        FunctionLiteralLayout::MultiLine,
                    ),
                    Vec::new(),
                );

                self.plan_function_result_layout_change(statements, call, &intermediate, target);
                return;
            }
        }
        statements.extend(self.reencode_return(call, source, &target.return_abi));
    }

    fn plan_same_contract_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        match source.return_abi.lowered() {
            None => self.plan_unlowered_result_layout_change(statements, call, source, target),
            Some(LoweredReturnAbi::BareError) => statements.push(plain_return(call)),
            Some(
                LoweredReturnAbi::Result { .. }
                | LoweredReturnAbi::Partial { .. }
                | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }),
            ) => self.plan_status_result_layout_change(statements, call, source, target),
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                self.plan_nullable_result_layout_change(statements, call, source, target)
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(_))) => {
                statements.push(plain_return(call))
            }
            Some(LoweredReturnAbi::Tuple { arity }) => {
                let values = self.create_temp_vars("ret", *arity);
                statements.push(define_many(values.clone(), call));
                let values = names(&values)
                    .into_iter()
                    .zip(source.results.iter().zip(&target.results))
                    .map(|(value, (source, target))| {
                        let change = self.queries().resolve_layout_change(source, target);
                        self.plan_layout_change(statements, value, &change)
                    })
                    .collect::<Vec<_>>();

                statements.push(multi_value_return(values));
            }
        }
    }

    fn plan_unlowered_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if source.results.is_empty() {
            statements.push(effect_statement(call));
            if !target.results.is_empty() {
                statements.push(if source.result_layout.logical_type().is_never() {
                    LoweredStatement::UnreachablePanic.into()
                } else {
                    plain_return(GoExpression::unit())
                });
            }
            return;
        }
        if target.results.is_empty() {
            statements.push(effect_statement(call));
            return;
        }
        let change = self
            .queries()
            .resolve_layout_change(&source.results[0], &target.results[0]);
        let value = self.plan_layout_change(statements, call, &change);
        statements.push(plain_return(value));
    }

    fn plan_nullable_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        let raw = self.hoist_temp(statements, "raw", call);
        let condition = option_nil_guard(&self.facts, source.result_layout.logical_type())
            .nil_condition(GoExpression::identifier(raw.clone()));
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                condition,
                LoweredBlock {
                    statements: vec![plain_return(GoExpression::nil())],
                },
                ElseArm::None,
            ))
            .into(),
        );
        let change = self
            .queries()
            .resolve_layout_change(&source.results[0], &target.results[0]);
        let value = self.plan_layout_change(statements, GoExpression::identifier(raw), &change);
        statements.push(plain_return(value));
    }

    fn plan_status_result_layout_change(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        let source_flat = source.return_abi.has_flattened_payload();
        let values = self.create_temp_vars("ret", source.results.len());
        let auxiliary = self.fresh_declared("ret");
        let mut all_values = values.clone();
        all_values.push(auxiliary.clone());
        statements.push(define_many(all_values, call));

        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Partial { .. })
        ) && !source_flat
        {
            let ok_type = source.result_layout.logical_type().ok_type();
            if let Some(condition) = self
                .queries()
                .partial_ok_nil_check(&ok_type, GoExpression::identifier(values[0].clone()))
            {
                statements.push(
                    LoweredStatement::If(IfPlan::plain(
                        condition,
                        LoweredBlock {
                            statements: vec![multi_value_return(vec![
                                GoExpression::nil(),
                                GoExpression::identifier(auxiliary.clone()),
                            ])],
                        },
                        ElseArm::None,
                    ))
                    .into(),
                );
            }
        }

        let mut values =
            self.layout_change_payload_slots(statements, names(&values), source, target);
        values.push(GoExpression::identifier(auxiliary));
        statements.push(multi_value_return(values));
    }

    fn layout_change_payload_slots(
        &mut self,
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Vec<GoExpression> {
        let source_flat = source.return_abi.has_flattened_payload();
        let target_flat = target.return_abi.has_flattened_payload();
        let (source, target) = (source.results.as_slice(), target.results.as_slice());
        if !source_flat && !target_flat {
            let change = self.queries().resolve_layout_change(&source[0], &target[0]);
            let [value] = <[GoExpression; 1]>::try_from(values)
                .unwrap_or_else(|_| unreachable!("a packed payload is one value"));
            return vec![self.plan_layout_change(statements, value, &change)];
        }
        let source_slots = if source_flat {
            source
        } else {
            packed_tuple_elements(source)
        };
        let target_slots = if target_flat {
            target
        } else {
            packed_tuple_elements(target)
        };
        let elements: Vec<GoExpression> = if source_flat {
            values
        } else {
            let packed = &values[0];
            (0..source_slots.len())
                .map(|index| {
                    GoExpression::selector(packed.clone(), go_name::tuple_field(index).to_string())
                })
                .collect()
        };

        let changed: Vec<GoExpression> = elements
            .into_iter()
            .zip(source_slots.iter().zip(target_slots))
            .map(|(value, (source, target))| {
                let change = self.queries().resolve_layout_change(source, target);
                self.plan_layout_change(statements, value, &change)
            })
            .collect();

        if target_flat {
            changed
        } else {
            vec![self.plan_tuple_from_vars(statements, changed)]
        }
    }
}

fn packed_tuple_elements(slots: &[ValueLayout]) -> &[ValueLayout] {
    slots[0]
        .tuple_elements()
        .expect("a payload changed against a flattened one is a tuple")
}

fn names(names: &[GoIdentifier]) -> Vec<GoExpression> {
    names
        .iter()
        .map(|name| GoExpression::identifier(name.clone()))
        .collect()
}

#[derive(Debug, Clone)]
pub(in crate::lowering) enum GoResultLayoutChange {
    Tuple(Vec<LayoutChange>),
    Whole(CoercionPlan),
    Payload(PayloadLayoutChanges),
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct PayloadLayoutChanges {
    pub(in crate::lowering) value: LayoutChange,
    pub(in crate::lowering) error: LayoutChange,
}

impl GoResultLayoutChange {
    pub(in crate::lowering) fn is_identity(&self) -> bool {
        match self {
            Self::Tuple(changes) => changes.iter().all(LayoutChange::is_identity),
            Self::Whole(change) => change.is_identity(),
            Self::Payload(changes) => changes.value.is_identity() && changes.error.is_identity(),
        }
    }

    pub(in crate::lowering) fn changes_layout(&self) -> bool {
        !self.is_identity()
    }
}

fn immutable_layout_change_fields(
    container: ImmutableContainer,
    arity: usize,
) -> Vec<(&'static str, Option<(&'static str, GoBinaryOperator)>)> {
    match container {
        ImmutableContainer::Tuple => (0..arity)
            .map(|index| (go_name::tuple_field(index), None))
            .collect(),
        ImmutableContainer::Option => {
            let some = tags::option_variant("Some");
            vec![(some.fields[0], Some((some.tag, GoBinaryOperator::Equal)))]
        }
        ImmutableContainer::Partial => {
            let (ok, err) = (tags::partial_variant("Ok"), tags::partial_variant("Err"));
            vec![
                (ok.fields[0], Some((err.tag, GoBinaryOperator::NotEqual))),
                (err.fields[0], Some((ok.tag, GoBinaryOperator::NotEqual))),
            ]
        }
        ImmutableContainer::Result => [tags::result_variant("Ok"), tags::result_variant("Err")]
            .into_iter()
            .map(|variant| {
                (
                    variant.fields[0],
                    Some((variant.tag, GoBinaryOperator::Equal)),
                )
            })
            .collect(),
    }
}

impl PlannerQueries<'_, '_> {
    fn tuple_to_value_layout_change(
        &self,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Option<GoResultLayoutChange> {
        if target.return_abi != CallableReturnAbi::Value
            || !matches!(
                source.return_abi.lowered(),
                Some(LoweredReturnAbi::Tuple { .. })
            )
        {
            return None;
        }
        let target_slots = target.results[0]
            .tuple_elements()
            .expect("a tagged tuple result has tuple element layouts");
        let changes = source
            .results
            .iter()
            .zip(target_slots)
            .map(|(source, target)| self.resolve_layout_change(source, target))
            .collect();
        Some(GoResultLayoutChange::Tuple(changes))
    }

    fn fallible_payload_layout_change(
        &self,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Option<GoResultLayoutChange> {
        if target.return_abi != CallableReturnAbi::Value
            || !matches!(
                source.return_abi.lowered(),
                Some(
                    LoweredReturnAbi::Result { .. }
                        | LoweredReturnAbi::Partial { .. }
                        | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })
                )
            )
        {
            return None;
        }
        self.payload_result_layout_change(&source.result_layout, &target.result_layout)
    }

    pub(in crate::lowering) fn payload_result_layout_change(
        &self,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> Option<GoResultLayoutChange> {
        let value = self.resolve_layout_change(source.payload()?, target.payload()?);
        let error = match (source.underlying(), target.underlying()) {
            (
                ValueLayout::Fallible { error: source, .. },
                ValueLayout::Fallible { error: target, .. },
            ) => self.resolve_layout_change(source, target),
            _ => LayoutChange::Identity,
        };

        let change = GoResultLayoutChange::Payload(PayloadLayoutChanges { value, error });
        change.changes_layout().then_some(change)
    }
}
