use crate::Planner;
use crate::abi::callable::PayloadLayout;
use crate::abi::coercion::{BridgeDirection, CoercionPlan, LayoutBridge};
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::calls::go_interop::build_tuple_literal;
use crate::calls::go_interop::wrappers::{
    WrapperOutcome, WrapperTarget, is_nil, is_nil_interface, leaf_block, non_nil,
};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{Fallible, FalliblePlanner, OPTION_SOME_TAG, prelude_call};
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign,
    define_many,
};
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

fn is_some(option: GoExpression) -> GoExpression {
    GoExpression::binary(
        GoExpression::selector(option, "Tag".to_string()),
        "==",
        GoExpression::generated(GeneratedPackage::Prelude, OPTION_SOME_TAG),
    )
}

fn some_payload(option: GoExpression) -> GoExpression {
    GoExpression::selector(option, "SomeVal".to_string())
}

impl Planner<'_> {
    /// `Some(e)` and `None` written straight into a nullable Go slot.
    pub(crate) fn lower_option_literal_into_layout(
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
        if expression.is_none_literal() {
            return Some(ValuePlan::literal("nil".to_string()));
        }
        let Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } = expression
        else {
            return None;
        };
        let [inner] = args.as_slice() else {
            return None;
        };
        if spread.is_some() || callee.as_option_constructor() != Some(Ok(())) {
            return None;
        }
        let inner = self.lower_composite_value(inner, ExpressionContext::value());
        Some(inner.map_expression(|setup, value| {
            let value = if payload.is_identity() {
                value
            } else {
                self.plan_layout_bridge(setup, value, payload)
            };
            let Some(pointee) = pointee else {
                return value;
            };
            let go_type = pointee.go_type(self);
            let go_type = self.use_rendered_go_type(go_type);
            let copy = self.fresh_var(Some("ptr"));
            self.declare(&copy);
            setup.push(LoweredStatement::VarDecl {
                name: copy.clone(),
                go_type,
                value: Some(value),
            });
            GoExpression::address_of(GoExpression::name(copy))
        }))
    }

    /// Wrap a sentinel-call via `OptionFromCommaOk` with `raw != sentinel`.
    pub(crate) fn lower_sentinel_wrapping(
        &mut self,
        call: GoExpression,
        option_ty: &Type,
        sentinel: i64,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let raw = self.hoist_tmp_value_statement(&mut statements, "ret", call);
        let raw = || GoExpression::name(raw.clone());
        let inner_ty_str = self.use_go_type(&option_ty.ok_type());
        let value = prelude_call(
            "OptionFromCommaOk",
            format!("[{}]", inner_ty_str),
            vec![
                raw(),
                GoExpression::binary(raw(), "!=", GoExpression::literal(sentinel.to_string())),
            ],
        );
        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
    }

    /// Wrap a comma-ok-returning call into a tagged `Option`. A `Flattened`
    /// tuple inner type comes from a Go-imported `(T1, ..., Tn, bool)`; a
    /// `Packed` one from a Lisette `(Tuple_n[...], bool)`.
    pub(crate) fn lower_comma_ok_wrapping(
        &mut self,
        call: GoExpression,
        option_ty: &Type,
        layout: PayloadLayout,
        payload_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let mut statements = Vec::new();

        let inner_ty = option_ty.ok_type();
        let inner_tuple_arity = inner_ty.tuple_arity();
        let needs_nilable_validation = self.facts.is_nullable_option(option_ty);

        let needs_complex = payload_bridge.is_some()
            || needs_nilable_validation
            || (layout.is_flattened() && inner_tuple_arity.is_some());

        if !needs_complex {
            let inner_ty_str = self.use_go_type(&inner_ty);
            let value = prelude_call(
                "OptionFromCommaOk",
                format!("[{}]", inner_ty_str),
                vec![call],
            );
            let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
            return (statements, outcome);
        }

        let fallible = Fallible::from_type(option_ty).expect("Option type expected");

        let val_vars = if layout.is_flattened()
            && let Some(arity) = inner_tuple_arity
        {
            self.create_temp_vars("ret", arity)
        } else {
            self.create_temp_vars("ret", 1)
        };
        let ok_var = self.fresh_var(Some("ret"));
        self.declare(&ok_var);

        let mut all_vars = val_vars.clone();
        all_vars.push(ok_var.clone());
        statements.push(define_many(all_vars, call));

        let first_val = GoExpression::name(val_vars[0].clone());
        let val_expression = if layout.is_flattened() && inner_tuple_arity.is_some() {
            let values = val_vars
                .iter()
                .map(|var| GoExpression::name(var.clone()))
                .collect();
            build_tuple_literal(values)
        } else {
            first_val.clone()
        };
        let (mut payload_setup, val_expression) = match payload_bridge {
            Some(bridge) => {
                let mut setup = Vec::new();
                let value = self.plan_layout_bridge(&mut setup, val_expression, bridge);
                (setup, value)
            }
            None => (Vec::new(), val_expression),
        };

        let option_ty_str = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.full_type_string()
        };

        let ok = GoExpression::name(ok_var);
        let condition = if self.is_interface_option(option_ty) {
            GoExpression::binary(
                ok,
                "&&",
                GoExpression::unary("!", is_nil_interface(first_val)),
            )
        } else if needs_nilable_validation {
            GoExpression::binary(ok, "&&", non_nil(first_val))
        } else {
            ok
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &option_ty_str, "option");

        let some_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_success(val_expression)
        };
        let none_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_failure(None)
        };

        let mut then_body = leaf_block(&sink, some_wrapper);
        payload_setup.append(&mut then_body.statements);
        statements.push(LoweredStatement::If(IfPlan::plain(
            condition,
            LoweredBlock {
                statements: payload_setup,
            },
            ElseArm::from_body(leaf_block(&sink, none_wrapper), false),
        )));
        (statements, outcome)
    }

    /// Wrap a nilable Go value into a tagged `Option` via `OptionFromNilable`.
    pub(crate) fn lower_nil_check_option_wrap(
        &mut self,
        raw_value: GoExpression,
        option_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let inner_ty = option_ty.ok_type();
        let inner_ty_str = self.use_go_type(&inner_ty);
        let is_nil_check = if self.is_interface_option(option_ty) {
            is_nil_interface(raw_value.clone())
        } else {
            is_nil(raw_value.clone())
        };
        let value = prelude_call(
            "OptionFromNilable",
            format!("[{}]", inner_ty_str),
            vec![raw_value, is_nil_check],
        );
        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
    }

    pub(crate) fn plan_option_projection(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        option_value: GoExpression,
        slot_ty: &str,
        payload_bridge: &LayoutBridge,
        address: bool,
    ) -> GoExpression {
        let option = self.stable_source(statements, "opt", option_value);
        let slot = self.fresh_var(Some(if address { "ptr" } else { "unwrap" }));
        self.declare(&slot);
        statements.push(LoweredStatement::VarDecl {
            name: slot.clone(),
            go_type: slot_ty.to_string(),
            value: None,
        });
        let body = self.project_some_into(
            GoExpression::name(slot.clone()),
            option.clone(),
            payload_bridge,
            address,
        );
        statements.push(LoweredStatement::If(IfPlan::plain(
            is_some(option),
            body,
            ElseArm::None,
        )));
        GoExpression::name(slot)
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
            self.plan_layout_bridge(&mut statements, some_payload(option), payload_bridge);
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
        option_type: &Type,
        payload_bridge: &LayoutBridge,
        pointer: bool,
    ) -> LoweredStatement {
        let fallible = Fallible::from_type(option_type).expect("Option type expected");
        let raw_payload = if pointer {
            GoExpression::dereference(raw.clone())
        } else {
            raw.clone()
        };
        let mut then_statements = Vec::new();
        let payload = self.plan_layout_bridge(&mut then_statements, raw_payload, payload_bridge);
        let some = {
            let mut planner = FalliblePlanner::new(self, &fallible);
            planner.emit_success(payload)
        };
        then_statements.push(assign(slot.clone(), some));
        let none = {
            let mut planner = FalliblePlanner::new(self, &fallible);
            planner.emit_failure(None)
        };
        let condition = if !pointer && self.is_interface_option(option_type) {
            GoExpression::unary("!", is_nil_interface(raw))
        } else {
            non_nil(raw)
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

    /// Wrap a Go `*T` (T value-typed) into Lisette `Option<T>`.
    fn plan_pointer_to_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        pointer: GoExpression,
        option_ty: &Type,
    ) -> GoExpression {
        let inner_ty_str = self.use_go_type(&option_ty.ok_type());
        let value = prelude_call(
            "OptionFromPointer",
            format!("[{}]", inner_ty_str),
            vec![pointer],
        );
        GoExpression::name(self.hoist_tmp_value_statement(statements, "option", value))
    }

    /// `FreshSlot` form of `lower_nil_check_option_wrap` (extends `statements`
    /// and returns the option var).
    pub(crate) fn plan_nil_check_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: GoExpression,
        option_ty: &Type,
    ) -> GoExpression {
        let (wrap_statements, outcome) =
            self.lower_nil_check_option_wrap(raw_value, option_ty, WrapperTarget::FreshSlot);
        statements.extend(wrap_statements);
        GoExpression::name(outcome.expect("FreshSlot produces a slot"))
    }

    pub(crate) fn plan_layout_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: GoExpression,
        bridge: &LayoutBridge,
    ) -> GoExpression {
        match bridge {
            LayoutBridge::Identity => value,
            LayoutBridge::UnwrapNullableOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(self);
                let slot_type = self.use_rendered_go_type(slot_type);
                self.plan_option_projection(statements, value, &slot_type, payload, false)
            }
            LayoutBridge::UnwrapPointerOption {
                target_payload,
                payload,
                ..
            } => {
                let slot_type = target_payload.go_type(self);
                let slot_type = format!("*{}", self.use_rendered_go_type(slot_type));
                self.plan_option_projection(statements, value, &slot_type, payload, true)
            }
            LayoutBridge::WrapNullableOption {
                option_type,
                payload,
                ..
            } => {
                if payload.is_identity() {
                    self.plan_nil_check_option_wrap(statements, value, option_type)
                } else {
                    self.plan_option_wrap_with_bridge(
                        statements,
                        value,
                        option_type,
                        payload,
                        false,
                    )
                }
            }
            LayoutBridge::WrapPointerOption {
                option_type,
                payload,
                ..
            } => {
                if payload.is_identity() {
                    self.plan_pointer_to_option_wrap(statements, value, option_type)
                } else {
                    self.plan_option_wrap_with_bridge(statements, value, option_type, payload, true)
                }
            }
            LayoutBridge::Reference { pointee } => {
                let source = self.stable_source(statements, "src", value);
                let pointee =
                    self.plan_layout_bridge(statements, GoExpression::dereference(source), pointee);
                GoExpression::name(self.hoist_tmp_value_statement(
                    statements,
                    "ref",
                    GoExpression::address_of(pointee),
                ))
            }
            LayoutBridge::Function { source, target, .. } => {
                self.plan_function_layout_bridge(statements, value, source, target)
            }
            LayoutBridge::Aggregate { .. } => {
                self.plan_aggregate_layout_bridge(statements, value, bridge)
            }
        }
    }

    fn plan_option_wrap_with_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: GoExpression,
        option_type: &Type,
        payload_bridge: &LayoutBridge,
        pointer: bool,
    ) -> GoExpression {
        let source = self.stable_source(statements, "raw", raw_value);
        let fallible = Fallible::from_type(option_type).expect("Option type expected");
        let option_type_string = {
            let mut planner = FalliblePlanner::new(self, &fallible);
            planner.full_type_string()
        };
        let option = self.fresh_var(Some("option"));
        self.declare(&option);
        statements.push(LoweredStatement::VarDecl {
            name: option.clone(),
            go_type: option_type_string,
            value: None,
        });
        statements.push(self.wrap_nilable_into(
            GoExpression::name(option.clone()),
            source,
            option_type,
            payload_bridge,
            pointer,
        ));
        GoExpression::name(option)
    }

    fn plan_aggregate_layout_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
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
            .or_else(|| element_bridge.direction())
            .expect("aggregate layout bridge must contain an option bridge");
        let output_hint = match direction {
            BridgeDirection::ToGo => "unwrapped",
            BridgeDirection::FromGo => "wrapped",
        };
        let target_type = target_layout.go_type(self);
        let target_type = self.use_rendered_go_type(target_type);
        let output = if matches!(target_layout, ValueLayout::Array { .. }) {
            let output = self.fresh_var(Some(output_hint));
            self.declare(&output);
            statements.push(LoweredStatement::VarDecl {
                name: output.clone(),
                go_type: target_type,
                value: None,
            });
            output
        } else {
            let make = GoExpression::call(
                GoExpression::name("make".to_string()),
                vec![
                    GoExpression::type_name(target_type),
                    GoExpression::call(GoExpression::name("len".to_string()), vec![source.clone()]),
                ],
            );
            self.hoist_tmp_value_statement(statements, output_hint, make)
        };
        let index = self.fresh_var(Some("i"));
        self.declare(&index);
        let element = self.fresh_var(Some("v"));
        self.declare(&element);
        let mut key_statements = Vec::new();
        let output_index = match key_bridge {
            Some(bridge) => self.plan_layout_bridge(
                &mut key_statements,
                GoExpression::name(index.clone()),
                bridge,
            ),
            None => GoExpression::name(index.clone()),
        };
        let mut body = self.plan_aggregate_element_bridge(
            GoExpression::index(GoExpression::name(output.clone()), output_index),
            GoExpression::name(element.clone()),
            source_layout,
            element_bridge,
        );
        if !key_statements.is_empty() {
            key_statements.append(&mut body.statements);
            body.statements = key_statements;
        }
        statements.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            kind: LoopKind::Generated { label: None },
            header: LoopHeader::Range {
                key: Some(index),
                value: Some(element),
                iterable: source,
            },
            body,
        }));
        GoExpression::name(output)
    }

    fn plan_aggregate_element_bridge(
        &mut self,
        slot: GoExpression,
        element: GoExpression,
        source_layout: &ValueLayout,
        bridge: &LayoutBridge,
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
                    statements: vec![LoweredStatement::If(IfPlan::plain(
                        is_some(element),
                        then_block,
                        else_arm,
                    ))],
                }
            }
            LayoutBridge::WrapNullableOption {
                option_type,
                payload,
                ..
            }
            | LayoutBridge::WrapPointerOption {
                option_type,
                payload,
                ..
            } => {
                let pointer = matches!(bridge, LayoutBridge::WrapPointerOption { .. });
                LoweredBlock {
                    statements: vec![self.wrap_nilable_into(
                        slot,
                        element,
                        option_type,
                        payload,
                        pointer,
                    )],
                }
            }
            LayoutBridge::Aggregate { .. }
            | LayoutBridge::Reference { .. }
            | LayoutBridge::Function { .. } => {
                let mut inner_statements = Vec::new();
                let inner = self.plan_layout_bridge(&mut inner_statements, element, bridge);
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
}
