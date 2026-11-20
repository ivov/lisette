use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::coercion::{BridgeDirection, CoercionPlan, LayoutBridge};
use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::abi::transition::multi_value_return;
use crate::abi::wrapping::{WrapperTarget, non_nil};
use crate::control_flow::fallible as tags;
use crate::control_flow::fallible::{OPTION_SOME_TAG, prelude_call};
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement,
    Statement, assign, declare, define_many, expression_statement,
};
use crate::ir::go_expression::{BinaryOp, FunctionLiteralLayout, GoExpressionNode, GoParameter};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{GoExpression, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::names::packages::PackageUse;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::parse::TUPLE_FIELDS;
use syntax::program::SlotOrigin;
use syntax::types::{CompoundKind, Type};

fn is_some(option: GoExpression) -> GoExpression {
    GoExpression::binary(
        GoExpression::selector(option, ENUM_TAG_FIELD.to_string()),
        BinaryOp::Eq,
        GoExpression::generated(GeneratedPackage::Prelude, OPTION_SOME_TAG),
    )
}

fn some_payload(option: GoExpression) -> GoExpression {
    GoExpression::selector(option, tags::OPTION_SOME_FIELD.to_string())
}

impl Planner<'_> {
    pub(crate) fn try_plan_option_literal_into_layout(
        &mut self,
        expression: &Expression,
        target: &ValueLayout,
    ) -> Option<ValuePlan> {
        let source = self.value_layout(&expression.get_type(), SlotOrigin::Lisette);
        let CoercionPlan::Layout(bridge) = CoercionPlan::bridge(self, &source, target) else {
            return None;
        };
        let (payload, pointee) = match &bridge {
            LayoutBridge::UnwrapNullableOption { payload, .. } => (payload, None),
            LayoutBridge::UnwrapPointerOption {
                payload,
                target_payload,
                ..
            } => (payload, Some(target_payload)),
            _ => return None,
        };
        let expression = expression.unwrap_parens();
        if prelude_constructor(expression) == Some(PreludeVariant::None) {
            return Some(ValuePlan::literal("nil".to_string()));
        }
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
        if spread.is_some() || prelude_constructor(callee) != Some(PreludeVariant::Some) {
            return None;
        }
        if let LayoutBridge::UnwrapNullableOption { target_payload, .. } = &bridge {
            let context = ExpressionContext::value().with_function_slot_origin(SlotOrigin::GoField);
            let value = self.peel_go_slot_alias_cast(inner, context);
            let emitted = self.emitted_value_layout(value, context);
            let direct = CoercionPlan::bridge(self, &emitted, target_payload);
            if matches!(value.unwrap_parens(), Expression::Lambda { .. }) && direct.is_identity() {
                return Some(self.plan_composite_value(value, context));
            }
            if self.go_slot_declaration_layout(value, context).is_some() {
                return Some(self.plan_composite_value(value, context).map_expression(
                    |setup, value| {
                        let (bridge_setup, value) = direct.lower(self, value);
                        setup.extend(bridge_setup);
                        value
                    },
                ));
            }
        }
        let inner = self.plan_nullable_payload(inner, &expression.get_type().ok_type());
        Some(inner.map_expression(|setup, value| {
            let value = if payload.is_identity() {
                value
            } else {
                self.plan_layout_bridge(setup, value, payload, true)
            };
            let Some(pointee) = pointee else {
                return value;
            };
            let go_type = pointee.go_type(self);
            let copy = self.fresh_declared("ptr");
            setup.push(
                LoweredStatement::VarDecl {
                    name: copy.clone(),
                    go_type,
                    value: Some(value),
                }
                .into(),
            );
            GoExpression::address_of(GoExpression::identifier(copy))
        }))
    }

    pub(crate) fn plan_nullable_payload(
        &mut self,
        payload: &Expression,
        payload_ty: &Type,
    ) -> ValuePlan {
        let origin = self.function_type_origin(payload_ty, SlotOrigin::Lisette);
        if !origin.declared_by_go() {
            return self.plan_composite_value(payload, ExpressionContext::value());
        }
        let slot = self.function_slot_bridge(payload, payload_ty);
        let context = ExpressionContext::value().with_function_slot_origin(origin);
        self.plan_composite_value(payload, context)
            .map_expression(|setup, value| {
                let (slot_setup, value) = slot.lower(self, value);
                setup.extend(slot_setup);
                value
            })
    }

    pub(crate) fn plan_option_payload_wrap(
        &mut self,
        statements: &mut Vec<Statement>,
        option_value: GoExpression,
        slot_ty: &GoType,
        payload_bridge: &LayoutBridge,
        address: bool,
    ) -> GoExpression {
        let option = self.stable_source(statements, "opt", option_value);
        let slot = self.fresh_declared(if address { "ptr" } else { "unwrap" });
        statements.push(declare(slot.clone(), slot_ty.clone()));
        let body = self.project_some_into(
            GoExpression::identifier(slot.clone()),
            option.clone(),
            payload_bridge,
            address,
        );
        statements
            .push(LoweredStatement::If(IfPlan::plain(is_some(option), body, ElseArm::None)).into());
        GoExpression::identifier(slot)
    }

    fn project_some_into(
        &mut self,
        slot: GoExpression,
        option: GoExpression,
        payload_bridge: &LayoutBridge,
        address: bool,
    ) -> LoweredBlock {
        let mut statements = Vec::new();
        let payload =
            self.plan_layout_bridge(&mut statements, some_payload(option), payload_bridge, true);
        let payload = if address {
            GoExpression::address_of(payload)
        } else {
            payload
        };
        statements.push(assign(slot, payload));
        LoweredBlock { statements }
    }

    fn wrap_nilable_into(
        &mut self,
        slot: GoExpression,
        raw: GoExpression,
        target: &ValueLayout,
        payload_bridge: &LayoutBridge,
        pointer: bool,
    ) -> LoweredStatement {
        let payload_type = target
            .payload()
            .expect("Option has a payload layout")
            .go_type(self);
        let type_args = GoType::type_argument(&payload_type);
        let raw_payload = if pointer {
            GoExpression::dereference(raw.clone())
        } else {
            raw.clone()
        };
        let mut then_statements = Vec::new();
        let payload =
            self.plan_layout_bridge(&mut then_statements, raw_payload, payload_bridge, true);
        let some = prelude_call("MakeOptionSome", type_args.clone(), vec![payload]);
        then_statements.push(assign(slot.clone(), some));
        let none = prelude_call("MakeOptionNone", type_args, Vec::new());
        let condition = if pointer {
            non_nil(raw)
        } else {
            self.facts
                .option_nil_guard(target.logical_type())
                .non_nil(raw)
        };
        LoweredStatement::If(IfPlan::plain(
            condition,
            LoweredBlock {
                statements: then_statements,
            },
            ElseArm::from_body(
                LoweredBlock {
                    statements: vec![assign(slot, none)],
                },
                false,
            ),
        ))
    }

    pub(crate) fn plan_layout_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        bridge: &LayoutBridge,
        capture: bool,
    ) -> GoExpression {
        match bridge {
            LayoutBridge::Identity => value,
            LayoutBridge::Conversion { target } => {
                let target = target.go_type(self);
                GoExpression::conversion(target, value)
            }
            LayoutBridge::UnwrapNullableOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(self);
                self.plan_option_payload_wrap(statements, value, &slot_type, payload, false)
            }
            LayoutBridge::UnwrapPointerOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(self);
                let slot_type = GoType::pointer_to(&slot_type);
                self.plan_option_payload_wrap(statements, value, &slot_type, payload, true)
            }
            LayoutBridge::WrapNullableOption {
                target, payload, ..
            } => {
                if payload.is_identity() {
                    let inner = target.payload().expect("Option has a payload layout");
                    let nil = self
                        .facts
                        .option_nil_guard(target.logical_type())
                        .is_nil(value.clone());
                    let value = prelude_call(
                        "OptionFromNilable",
                        GoType::type_argument(&inner.go_type(self)),
                        vec![value, nil],
                    );
                    if capture {
                        GoExpression::identifier(
                            self.hoist_tmp_value_statement(statements, "option", value),
                        )
                    } else {
                        value
                    }
                } else {
                    self.plan_option_wrap_with_bridge(statements, value, target, payload, false)
                }
            }
            LayoutBridge::WrapPointerOption {
                target, payload, ..
            } => {
                if payload.is_identity() {
                    let inner_type = target
                        .payload()
                        .expect("Option has a payload layout")
                        .go_type(self);
                    let value = prelude_call(
                        "OptionFromPointer",
                        GoType::type_argument(&inner_type),
                        vec![value],
                    );
                    GoExpression::identifier(
                        self.hoist_tmp_value_statement(statements, "option", value),
                    )
                } else {
                    self.plan_option_wrap_with_bridge(statements, value, target, payload, true)
                }
            }
            LayoutBridge::Reference { pointee } => {
                let source = self.stable_source(statements, "src", value);
                let pointee = self.plan_layout_bridge(
                    statements,
                    GoExpression::dereference(source),
                    pointee,
                    true,
                );
                GoExpression::identifier(self.hoist_tmp_value_statement(
                    statements,
                    "ref",
                    GoExpression::address_of(pointee),
                ))
            }
            LayoutBridge::Function { source, target, .. } => {
                self.plan_function_layout_bridge(statements, value, source, target, false)
            }
            LayoutBridge::NilablePayload { target, payload } => {
                self.plan_nilable_payload_bridge(statements, value, target, payload)
            }
            LayoutBridge::Immutable { target, elements } => {
                self.plan_immutable_bridge(statements, value, target, elements)
            }
            LayoutBridge::Aggregate { .. } => {
                self.plan_aggregate_layout_bridge(statements, value, bridge)
            }
        }
    }

    fn plan_nilable_payload_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        target: &ValueLayout,
        payload: &LayoutBridge,
    ) -> GoExpression {
        let pointer = match target {
            ValueLayout::NullableOption { .. } => false,
            ValueLayout::PointerOption { .. } => true,
            _ => unreachable!("nilable payload bridges require a nilable option layout"),
        };
        let source = self.stable_source(statements, "raw", value);
        let output = self.fresh_declared("adapted");
        let go_type = target.go_type(self);
        statements.push(declare(output.clone(), go_type));
        let mut body = Vec::new();
        let inner = if pointer {
            GoExpression::dereference(source.clone())
        } else {
            source.clone()
        };
        let adapted = self.plan_layout_bridge(&mut body, inner, payload, true);
        let adapted = if pointer {
            let local = self.hoist_tmp_value_statement(&mut body, "value", adapted);
            GoExpression::address_of(GoExpression::identifier(local))
        } else {
            adapted
        };
        body.push(assign(GoExpression::identifier(output.clone()), adapted));
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil(source),
                LoweredBlock { statements: body },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(output)
    }

    fn plan_immutable_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        target: &ValueLayout,
        elements: &[LayoutBridge],
    ) -> GoExpression {
        let source = self.stable_source(statements, "src", value);
        let go_type = target.go_type(self);
        let output = self.fresh_declared("adapted");
        statements.push(declare(output.clone(), go_type));
        let output = GoExpression::identifier(output);
        let fields = immutable_bridge_fields(target);
        assert_eq!(
            fields.len(),
            elements.len(),
            "immutable bridge has one plan per payload"
        );
        if !matches!(target, ValueLayout::Tuple { .. }) {
            statements.push(assign(
                GoExpression::selector(output.clone(), ENUM_TAG_FIELD.to_string()),
                GoExpression::selector(source.clone(), ENUM_TAG_FIELD.to_string()),
            ));
        }
        for ((field, guard), bridge) in fields.into_iter().zip(elements) {
            let mut body = Vec::new();
            let value = self.plan_layout_bridge(
                &mut body,
                GoExpression::selector(source.clone(), field.to_string()),
                bridge,
                true,
            );
            body.push(assign(
                GoExpression::selector(output.clone(), field.to_string()),
                value,
            ));
            if let Some((tag, op)) = guard {
                statements.push(
                    LoweredStatement::If(IfPlan {
                        initializer: None,
                        condition: GoExpression::binary(
                            GoExpression::selector(source.clone(), ENUM_TAG_FIELD.to_string()),
                            op,
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

    fn plan_option_wrap_with_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        raw_value: GoExpression,
        target: &ValueLayout,
        payload_bridge: &LayoutBridge,
        pointer: bool,
    ) -> GoExpression {
        let source = self.stable_source(statements, "raw", raw_value);
        let option_type = target.go_type(self);
        let option_type_string = option_type;
        let option = self.fresh_declared("option");
        statements.push(declare(option.clone(), option_type_string));
        statements.push(
            self.wrap_nilable_into(
                GoExpression::identifier(option.clone()),
                source,
                target,
                payload_bridge,
                pointer,
            )
            .into(),
        );
        GoExpression::identifier(option)
    }

    fn plan_aggregate_layout_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        bridge: &LayoutBridge,
    ) -> GoExpression {
        let LayoutBridge::Aggregate {
            source,
            target,
            key,
            element,
        } = bridge
        else {
            unreachable!("plan_aggregate_layout_bridge requires an Aggregate bridge");
        };
        let source_layout: &ValueLayout = source;
        let target_layout: &ValueLayout = target;
        let element_bridge: &LayoutBridge = element;
        let key_bridge = key.as_deref();
        let source = self.stable_source(statements, "src", value);
        let direction = key_bridge
            .and_then(LayoutBridge::direction)
            .or_else(|| element_bridge.direction());
        let output_hint = match direction {
            Some(BridgeDirection::ToGo) => "unwrapped",
            Some(BridgeDirection::FromGo) => "wrapped",
            None => "adapted",
        };
        let target_type = target_layout.go_type(self);
        let output = if matches!(target_layout, ValueLayout::Array { .. }) {
            let output = self.fresh_declared(output_hint);
            statements.push(declare(output.clone(), target_type));
            output
        } else {
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
            self.hoist_tmp_value_statement(statements, output_hint, make)
        };
        let index = self.fresh_declared("i");
        let element = self.fresh_declared("v");
        let mut key_statements = Vec::new();
        let output_index = match key_bridge {
            Some(bridge) => self.plan_layout_bridge(
                &mut key_statements,
                GoExpression::identifier(index.clone()),
                bridge,
                true,
            ),
            None => GoExpression::identifier(index.clone()),
        };
        let mut body = self.plan_aggregate_element_body(
            GoExpression::index(GoExpression::identifier(output.clone()), output_index),
            GoExpression::identifier(element.clone()),
            element_bridge,
            source_layout,
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

    fn plan_aggregate_element_body(
        &mut self,
        slot: GoExpression,
        element: GoExpression,
        bridge: &LayoutBridge,
        source_layout: &ValueLayout,
    ) -> LoweredBlock {
        match bridge {
            LayoutBridge::UnwrapNullableOption { payload, .. }
            | LayoutBridge::UnwrapPointerOption { payload, .. } => {
                let pointer = matches!(bridge, LayoutBridge::UnwrapPointerOption { .. });
                let then_block =
                    self.project_some_into(slot.clone(), element.clone(), payload, pointer);
                let needs_nil_else = matches!(source_layout, ValueLayout::Map { .. }) || pointer;
                let else_arm = if needs_nil_else {
                    ElseArm::from_body(
                        LoweredBlock {
                            statements: vec![assign(slot, GoExpression::nil())],
                        },
                        false,
                    )
                } else {
                    ElseArm::None
                };
                LoweredBlock {
                    statements: vec![
                        LoweredStatement::If(IfPlan::plain(is_some(element), then_block, else_arm))
                            .into(),
                    ],
                }
            }
            LayoutBridge::WrapNullableOption {
                target, payload, ..
            }
            | LayoutBridge::WrapPointerOption {
                target, payload, ..
            } => {
                let pointer = matches!(bridge, LayoutBridge::WrapPointerOption { .. });
                LoweredBlock {
                    statements: vec![
                        self.wrap_nilable_into(slot, element, target, payload, pointer)
                            .into(),
                    ],
                }
            }
            LayoutBridge::Conversion { .. }
            | LayoutBridge::Aggregate { .. }
            | LayoutBridge::Immutable { .. }
            | LayoutBridge::NilablePayload { .. }
            | LayoutBridge::Reference { .. }
            | LayoutBridge::Function { .. } => {
                let mut inner_statements = Vec::new();
                let inner = self.plan_layout_bridge(&mut inner_statements, element, bridge, true);
                inner_statements.push(assign(slot, inner));
                LoweredBlock {
                    statements: inner_statements,
                }
            }
            LayoutBridge::Identity => LoweredBlock {
                statements: vec![assign(slot, element)],
            },
        }
    }

    pub(crate) fn plan_function_layout_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
        capture_callee: bool,
    ) -> GoExpression {
        let is_declaration = |node: &GoExpressionNode| match node {
            GoExpressionNode::Identifier(name) => name.is_external(),
            GoExpressionNode::Qualified { package, name } => {
                *package == PackageUse::generated(GeneratedPackage::Prelude)
                    || self
                        .facts
                        .definition(&format!("go:{}.{}", package.package().path(), name))
                        .is_some_and(|definition| !definition.is_variable())
            }
            _ => false,
        };
        let names_package_function = match value.node() {
            GoExpressionNode::Instantiation { base, .. } => is_declaration(base),
            node => is_declaration(node),
        };
        let never_nil = capture_callee
            || names_package_function
            || matches!(value.node(), GoExpressionNode::FunctionLiteral { .. });
        let callee = if names_package_function && !capture_callee {
            value
        } else {
            GoExpression::identifier(self.hoist_tmp_value_statement(statements, "cb", value))
        };
        let mut body = Vec::new();
        let mut parameters = Vec::new();
        let mut names = Vec::new();
        for (index, target) in target.parameters.iter().enumerate() {
            let name = self
                .scope
                .helper_local(&format!("arg{index}"), self.package);
            let target_type = target.go_type(self);
            parameters.push(GoParameter::new(name.clone(), target_type));
            names.push(GoExpression::identifier(name));
        }
        let arguments = self.plan_bridged_arguments(&mut body, names, source, target);
        let call = GoExpression::call(callee.clone(), arguments);
        self.plan_function_result_bridge(&mut body, call, source, target);
        let result = target.result_go_type(self).unwrap_or_default();
        let callback = GoExpression::function_literal(
            parameters,
            result,
            LoweredBlock { statements: body },
            FunctionLiteralLayout::MultiLine,
        );
        if never_nil {
            return callback;
        }
        let adapted = self.fresh_declared("adapted");
        let go_type = target.go_type(self);
        statements.push(declare(adapted.clone(), go_type));
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil(callee),
                LoweredBlock {
                    statements: vec![assign(GoExpression::identifier(adapted.clone()), callback)],
                },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(adapted)
    }

    pub(crate) fn plan_bridged_arguments(
        &mut self,
        statements: &mut Vec<Statement>,
        parameters: Vec<GoExpression>,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Vec<GoExpression> {
        parameters
            .into_iter()
            .zip(source.parameters.iter().zip(&target.parameters))
            .map(|(parameter, (source, target))| {
                let bridge = self.resolve_layout_bridge(target, source);
                let argument = self.plan_layout_bridge(statements, parameter, &bridge, true);
                if source.logical_type().is_native(CompoundKind::VarArgs) {
                    GoExpression::spread(argument)
                } else {
                    argument
                }
            })
            .collect()
    }

    fn fallible_payload_bridge(
        &self,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Option<GoResultBridge> {
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
        self.payload_result_bridge(&source.result_layout, &target.result_layout)
    }

    pub(crate) fn payload_result_bridge(
        &self,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> Option<GoResultBridge> {
        let value = self.resolve_layout_bridge(source.payload()?, target.payload()?);
        let error = match (source.underlying(), target.underlying()) {
            (
                ValueLayout::Fallible { error: source, .. },
                ValueLayout::Fallible { error: target, .. },
            ) => self.resolve_layout_bridge(source, target),
            _ => LayoutBridge::Identity,
        };
        let bridge = GoResultBridge::Payload { value, error };
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn plan_function_result_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if source.return_abi == CallableReturnAbi::Void
            && target.return_abi != CallableReturnAbi::Void
        {
            statements.push(expression_statement(call));
            statements.push(if source.result_layout.logical_type().is_never() {
                LoweredStatement::UnreachablePanic.into()
            } else {
                plain_return(GoExpression::unit())
            });
            return;
        }
        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Tuple { .. })
        ) && target.return_abi == CallableReturnAbi::Value
        {
            let target_slots = target.results[0]
                .tuple_elements()
                .expect("a tagged tuple result has tuple element layouts");
            let bridges = source
                .results
                .iter()
                .zip(target_slots)
                .map(|(source, target)| self.resolve_layout_bridge(source, target))
                .collect();
            let (body, _) = self.plan_abi_wrapping(
                call,
                &source.return_abi,
                source.result_layout.logical_type(),
                Some(GoResultBridge::Tuple(bridges)),
                WrapperTarget::Return,
            );
            statements.extend(body);
            return;
        }
        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        ) && target.return_abi == CallableReturnAbi::Value
        {
            let source = ValueLayout::NullableOption {
                option_type: source.result_layout.logical_type().clone(),
                payload: Box::new(source.results[0].clone()),
            };
            let bridge = self.resolve_layout_bridge(&source, &target.results[0]);
            let raw = self.hoist_tmp_value_statement(statements, "raw", call);
            let raw = GoExpression::identifier(raw);
            let value = self.plan_layout_bridge(statements, raw, &bridge, false);
            statements.push(plain_return(value));
            return;
        }
        if let Some(bridge) = self.fallible_payload_bridge(source, target) {
            let (body, _) = self.plan_abi_wrapping(
                call,
                &source.return_abi,
                source.result_layout.logical_type(),
                Some(bridge),
                WrapperTarget::Return,
            );
            statements.extend(body);
            return;
        }
        if !source.return_abi.same_logical_contract(&target.return_abi) {
            self.plan_reencoded_result_bridge(statements, call, source, target);
            return;
        }
        self.plan_same_contract_result_bridge(statements, call, source, target);
    }

    fn plan_reencoded_result_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if target.return_abi.is_lowered() {
            let tagged =
                self.value_layout(source.result_layout.logical_type(), SlotOrigin::Lisette);
            let intermediate =
                self.function_layout(Vec::new(), &tagged, target.return_abi.clone(), None);
            if !intermediate.same_result_representation(target) {
                let body = self.reencode_return(
                    call,
                    &source.return_abi,
                    &intermediate.return_abi,
                    source.result_layout.logical_type(),
                );
                let result = intermediate
                    .result_go_type(self)
                    .expect("a lowered result has a Go signature");
                let call = GoExpression::call(
                    GoExpression::function_literal(
                        Vec::new(),
                        result,
                        LoweredBlock { statements: body },
                        FunctionLiteralLayout::MultiLine,
                    ),
                    Vec::new(),
                );
                self.plan_function_result_bridge(statements, call, &intermediate, target);
                return;
            }
        }
        statements.extend(self.reencode_return(
            call,
            &source.return_abi,
            &target.return_abi,
            source.result_layout.logical_type(),
        ));
    }

    fn plan_same_contract_result_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        match source.return_abi.lowered() {
            None => {
                if source.results.is_empty() {
                    statements.push(expression_statement(call));
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
                    statements.push(expression_statement(call));
                    return;
                }
                let bridge = self.resolve_layout_bridge(&source.results[0], &target.results[0]);
                let value = self.plan_layout_bridge(statements, call, &bridge, true);
                statements.push(plain_return(value));
            }
            Some(LoweredReturnAbi::BareError) => statements.push(plain_return(call)),
            Some(
                LoweredReturnAbi::Result { .. }
                | LoweredReturnAbi::Partial { .. }
                | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }),
            ) => self.plan_status_result_bridge(statements, call, source, target),
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                let raw = self.hoist_tmp_value_statement(statements, "raw", call);
                let condition = self
                    .facts
                    .option_nil_guard(source.result_layout.logical_type())
                    .is_nil(GoExpression::identifier(raw.clone()));
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
                let bridge = self.resolve_layout_bridge(&source.results[0], &target.results[0]);
                let value = self.plan_layout_bridge(
                    statements,
                    GoExpression::identifier(raw),
                    &bridge,
                    true,
                );
                statements.push(plain_return(value));
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
                        let bridge = self.resolve_layout_bridge(source, target);
                        self.plan_layout_bridge(statements, value, &bridge, true)
                    })
                    .collect::<Vec<_>>();
                statements.push(multi_value_return(values));
            }
        }
    }

    fn plan_status_result_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        let source_flat = source.return_abi.has_flattened_payload();
        let mut values = self.create_temp_vars("ret", source.results.len() + 1);
        statements.push(define_many(values.clone(), call));
        let auxiliary = values.pop().expect("a lowered callable has a status slot");

        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Partial { .. })
        ) && !source_flat
        {
            let ok_type = source.result_layout.logical_type().ok_type();
            if let Some(condition) =
                self.partial_ok_nil_check(&ok_type, GoExpression::identifier(values[0].clone()))
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

        let mut values = self.bridge_payload_slots(
            statements,
            names(&values),
            &source.results,
            &target.results,
            target.return_abi.has_flattened_payload(),
        );
        values.push(GoExpression::identifier(auxiliary));
        statements.push(multi_value_return(values));
    }

    fn bridge_payload_slots(
        &mut self,
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
        source: &[ValueLayout],
        target: &[ValueLayout],
        target_flat: bool,
    ) -> Vec<GoExpression> {
        let source_flat = values.len() > 1;
        if !source_flat && !target_flat {
            let bridge = self.resolve_layout_bridge(&source[0], &target[0]);
            let [value] = <[GoExpression; 1]>::try_from(values)
                .unwrap_or_else(|_| unreachable!("a packed payload is one value"));
            return vec![self.plan_layout_bridge(statements, value, &bridge, true)];
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
                    GoExpression::selector(packed.clone(), TUPLE_FIELDS[index].to_string())
                })
                .collect()
        };
        let bridged: Vec<GoExpression> = elements
            .into_iter()
            .zip(source_slots.iter().zip(target_slots))
            .map(|(value, (source, target))| {
                let bridge = self.resolve_layout_bridge(source, target);
                self.plan_layout_bridge(statements, value, &bridge, true)
            })
            .collect();
        if target_flat {
            bridged
        } else {
            vec![self.plan_tuple_from_vars(statements, bridged)]
        }
    }
}

fn packed_tuple_elements(slots: &[ValueLayout]) -> &[ValueLayout] {
    slots[0]
        .tuple_elements()
        .expect("a payload bridged against a flattened one is a tuple")
}

fn names(names: &[GoIdentifier]) -> Vec<GoExpression> {
    names
        .iter()
        .map(|name| GoExpression::identifier(name.clone()))
        .collect()
}

#[derive(Debug, Clone)]
pub(crate) enum GoResultBridge {
    Tuple(Vec<LayoutBridge>),
    Whole(CoercionPlan),
    Payload {
        value: LayoutBridge,
        error: LayoutBridge,
    },
}
impl GoResultBridge {
    pub(crate) fn is_identity(&self) -> bool {
        match self {
            Self::Tuple(bridges) => bridges.iter().all(LayoutBridge::is_identity),
            Self::Whole(bridge) => bridge.is_identity(),
            Self::Payload { value, error } => value.is_identity() && error.is_identity(),
        }
    }
}

fn immutable_bridge_fields(
    target: &ValueLayout,
) -> Vec<(&'static str, Option<(&'static str, BinaryOp)>)> {
    match target {
        ValueLayout::Tuple { elements, .. } => TUPLE_FIELDS[..elements.len()]
            .iter()
            .map(|field| (*field, None))
            .collect(),
        ValueLayout::TaggedOption { .. } => {
            vec![(
                tags::OPTION_SOME_FIELD,
                Some((OPTION_SOME_TAG, BinaryOp::Eq)),
            )]
        }
        ValueLayout::Fallible { fallible_type, .. } if fallible_type.is_partial() => {
            vec![
                (
                    tags::PARTIAL_OK_FIELD,
                    Some((tags::PARTIAL_ERR_TAG, BinaryOp::Ne)),
                ),
                (
                    tags::PARTIAL_ERR_FIELD,
                    Some((tags::PARTIAL_OK_TAG, BinaryOp::Ne)),
                ),
            ]
        }
        ValueLayout::Fallible { .. } => vec![
            (
                tags::RESULT_OK_FIELD,
                Some((tags::RESULT_OK_TAG, BinaryOp::Eq)),
            ),
            (
                tags::RESULT_ERR_FIELD,
                Some((tags::RESULT_ERR_TAG, BinaryOp::Eq)),
            ),
        ],
        _ => unreachable!("immutable bridges require a tuple or tagged container"),
    }
}
