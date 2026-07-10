use crate::Planner;
use crate::abi::callable::PayloadLayout;
use crate::abi::coercion::{BridgeDirection, CoercionPlan, LayoutBridge};
use crate::abi::layout::ValueLayout;
use crate::calls::go_interop::build_tuple_literal;
use crate::calls::go_interop::wrappers::{WrapperOutcome, WrapperTarget, leaf_block};
use crate::control_flow::fallible::{Fallible, FalliblePlanner, OPTION_SOME_TAG};
use crate::plan::bodies::{ElseArm, IfPlan, LoopPlan, LoweredBlock, LoweredStatement};
use syntax::types::Type;

impl Planner<'_> {
    /// Wrap a sentinel-call via `OptionFromCommaOk` with `raw != sentinel`.
    pub(crate) fn lower_sentinel_wrapping(
        &mut self,
        call_str: &str,
        option_ty: &Type,
        sentinel: i64,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        self.require_stdlib();
        let mut statements = Vec::new();
        let raw = self.hoist_tmp_value_statement(&mut statements, "ret", call_str);
        let inner_ty_str = self.go_type_string(&option_ty.ok_type());
        let value_expr = format!(
            "lisette.OptionFromCommaOk[{}]({}, {} != {})",
            inner_ty_str, raw, raw, sentinel
        );
        let outcome =
            self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
        (statements, outcome)
    }

    /// Wrap a comma-ok-returning call into a tagged `Option`. A `Flattened`
    /// tuple inner type comes from a Go-imported `(T1, ..., Tn, bool)`; a
    /// `Packed` one from a Lisette `(Tuple_n[...], bool)`.
    pub(crate) fn lower_comma_ok_wrapping(
        &mut self,
        call_str: &str,
        option_ty: &Type,
        layout: PayloadLayout,
        payload_bridge: Option<CoercionPlan>,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        self.require_stdlib();
        let mut statements = Vec::new();

        let inner_ty = option_ty.ok_type();
        let inner_tuple_arity = inner_ty.tuple_arity();
        let needs_nilable_validation = self.facts.is_nullable_option(option_ty);

        let needs_complex = payload_bridge.is_some()
            || needs_nilable_validation
            || (layout.is_flattened() && inner_tuple_arity.is_some());

        if !needs_complex {
            let inner_ty_str = self.go_type_string(&inner_ty);
            let value_expr = format!("lisette.OptionFromCommaOk[{}]({})", inner_ty_str, call_str);
            let outcome =
                self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
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

        let all_vars: Vec<&str> = val_vars
            .iter()
            .map(|s| s.as_str())
            .chain(std::iter::once(ok_var.as_str()))
            .collect();
        statements.push(LoweredStatement::RawGo(format!(
            "{} := {}\n",
            all_vars.join(", "),
            call_str
        )));

        let val_expression = if layout.is_flattened() && inner_tuple_arity.is_some() {
            build_tuple_literal(self, &val_vars, &inner_ty)
        } else {
            val_vars[0].clone()
        };
        let (mut payload_setup, val_expression) = match payload_bridge {
            Some(bridge) => bridge.lower(self, val_expression),
            None => (Vec::new(), val_expression),
        };

        let option_ty_str = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.full_type_string()
        };

        let condition = if self.is_interface_option(option_ty) {
            format!("{} && !lisette.IsNilInterface({})", ok_var, val_vars[0])
        } else if needs_nilable_validation {
            format!("{} && {} != nil", ok_var, val_vars[0])
        } else {
            ok_var.clone()
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &option_ty_str, "option");

        let some_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_success(&val_expression)
        };
        let none_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_failure(None)
        };

        let mut then_body = leaf_block(&sink, &some_wrapper);
        payload_setup.append(&mut then_body.statements);
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition,
            then_body: LoweredBlock {
                statements: payload_setup,
            },
            else_arm: ElseArm::Else {
                body: leaf_block(&sink, &none_wrapper),
                inline: false,
            },
        }));
        (statements, outcome)
    }

    /// Wrap a nilable Go value into a tagged `Option` via `OptionFromNilable`.
    pub(crate) fn lower_nil_check_option_wrap(
        &mut self,
        raw_value: &str,
        option_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        self.require_stdlib();
        let mut statements = Vec::new();
        let inner_ty = option_ty.ok_type();
        let inner_ty_str = self.go_type_string(&inner_ty);
        let is_nil_check = if self.is_interface_option(option_ty) {
            format!("lisette.IsNilInterface({})", raw_value)
        } else {
            format!("{} == nil", raw_value)
        };
        let value_expr = format!(
            "lisette.OptionFromNilable[{}]({}, {})",
            inner_ty_str, raw_value, is_nil_check
        );
        let outcome =
            self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
        (statements, outcome)
    }

    pub(crate) fn plan_option_projection(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        option_value: &str,
        slot_hint: &str,
        slot_ty: &str,
        address: bool,
    ) -> String {
        let opt_var = self.hoist_tmp_value_statement(statements, "opt", option_value);
        let slot_var = self.fresh_var(Some(slot_hint));
        self.declare(&slot_var);
        statements.push(LoweredStatement::VarDecl {
            name: slot_var.clone(),
            go_type: slot_ty.to_string(),
            value: None,
        });

        self.require_stdlib();
        let amp = if address { "&" } else { "" };
        let body = LoweredBlock {
            statements: vec![LoweredStatement::RawGo(format!(
                "{} = {}{}.SomeVal\n",
                slot_var, amp, opt_var
            ))],
        };
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition: format!("{}.Tag == {}", opt_var, OPTION_SOME_TAG),
            then_body: body,
            else_arm: ElseArm::None,
        }));
        slot_var
    }

    /// Wrap a Go `*T` (T value-typed) into Lisette `Option<T>`.
    pub(crate) fn plan_pointer_to_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        ptr_value: &str,
        option_ty: &Type,
    ) -> String {
        self.require_stdlib();
        let inner_ty_str = self.go_type_string(&option_ty.ok_type());
        let value_expr = format!("lisette.OptionFromPointer[{}]({})", inner_ty_str, ptr_value);
        self.hoist_tmp_value_statement(statements, "option", &value_expr)
    }

    /// `FreshSlot` form of `lower_nil_check_option_wrap` (extends `statements`
    /// and returns the option var).
    pub(crate) fn plan_nil_check_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: &str,
        option_ty: &Type,
    ) -> String {
        let (wrap_statements, outcome) =
            self.lower_nil_check_option_wrap(raw_value, option_ty, WrapperTarget::FreshSlot);
        statements.extend(wrap_statements);
        outcome.expect("FreshSlot produces a slot")
    }

    pub(crate) fn plan_layout_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: &str,
        bridge: &LayoutBridge,
    ) -> String {
        match bridge {
            LayoutBridge::Identity => value.to_string(),
            LayoutBridge::UnwrapNullableOption {
                option_type,
                target_payload,
                payload,
            } => {
                if payload.is_identity() {
                    let slot_type = target_payload.go_type(self);
                    self.plan_option_projection(statements, value, "unwrap", &slot_type, false)
                } else {
                    self.plan_option_projection_with_bridge(
                        statements,
                        value,
                        option_type,
                        target_payload,
                        payload,
                        false,
                    )
                }
            }
            LayoutBridge::UnwrapPointerOption {
                option_type,
                target_payload,
                payload,
            } => {
                if payload.is_identity() {
                    let slot_type = format!("*{}", target_payload.go_type(self));
                    self.plan_option_projection(statements, value, "ptr", &slot_type, true)
                } else {
                    self.plan_option_projection_with_bridge(
                        statements,
                        value,
                        option_type,
                        target_payload,
                        payload,
                        true,
                    )
                }
            }
            LayoutBridge::WrapNullableOption {
                option_type,
                source_payload,
                payload,
            } => {
                if payload.is_identity() {
                    self.plan_nil_check_option_wrap(statements, value, option_type)
                } else {
                    self.plan_option_wrap_with_bridge(
                        statements,
                        value,
                        option_type,
                        source_payload,
                        payload,
                        false,
                    )
                }
            }
            LayoutBridge::WrapPointerOption {
                option_type,
                source_payload,
                payload,
            } => {
                if payload.is_identity() {
                    self.plan_pointer_to_option_wrap(statements, value, option_type)
                } else {
                    self.plan_option_wrap_with_bridge(
                        statements,
                        value,
                        option_type,
                        source_payload,
                        payload,
                        true,
                    )
                }
            }
            LayoutBridge::Aggregate {
                source,
                target,
                key,
                element,
            } => self.plan_aggregate_layout_bridge(
                statements,
                value,
                source,
                target,
                key.as_deref(),
                element,
            ),
        }
    }

    fn plan_option_projection_with_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        option_value: &str,
        _option_type: &Type,
        target_payload: &ValueLayout,
        payload_bridge: &LayoutBridge,
        address: bool,
    ) -> String {
        let option = self.hoist_tmp_value_statement(statements, "opt", option_value);
        let target_type = target_payload.go_type(self);
        let slot_type = if address {
            format!("*{target_type}")
        } else {
            target_type
        };
        let slot_hint = if address { "ptr" } else { "unwrap" };
        let slot = self.fresh_var(Some(slot_hint));
        self.declare(&slot);
        statements.push(LoweredStatement::VarDecl {
            name: slot.clone(),
            go_type: slot_type,
            value: None,
        });

        let mut then_statements = Vec::new();
        let payload = self.plan_layout_bridge(
            &mut then_statements,
            &format!("{}.SomeVal", option),
            payload_bridge,
        );
        let payload = if address {
            format!("&{payload}")
        } else {
            payload
        };
        then_statements.push(LoweredStatement::RawGo(format!("{slot} = {payload}\n")));
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition: format!("{}.Tag == {}", option, OPTION_SOME_TAG),
            then_body: LoweredBlock {
                statements: then_statements,
            },
            else_arm: ElseArm::None,
        }));
        slot
    }

    fn plan_option_wrap_with_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: &str,
        option_type: &Type,
        _source_payload: &ValueLayout,
        payload_bridge: &LayoutBridge,
        pointer: bool,
    ) -> String {
        self.require_stdlib();
        let source = self.hoist_tmp_value_statement(statements, "raw", raw_value);
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

        let raw_payload = if pointer {
            format!("*{source}")
        } else {
            source.clone()
        };
        let mut then_statements = Vec::new();
        let payload = self.plan_layout_bridge(&mut then_statements, &raw_payload, payload_bridge);
        let some = {
            let mut planner = FalliblePlanner::new(self, &fallible);
            planner.emit_success(&payload)
        };
        then_statements.push(LoweredStatement::RawGo(format!("{option} = {some}\n")));
        let none = {
            let mut planner = FalliblePlanner::new(self, &fallible);
            planner.emit_failure(None)
        };
        let condition = if !pointer && self.is_interface_option(option_type) {
            format!("!lisette.IsNilInterface({source})")
        } else {
            format!("{source} != nil")
        };
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition,
            then_body: LoweredBlock {
                statements: then_statements,
            },
            else_arm: ElseArm::Else {
                body: LoweredBlock {
                    statements: vec![LoweredStatement::RawGo(format!("{option} = {none}\n"))],
                },
                inline: false,
            },
        }));
        option
    }

    fn plan_aggregate_layout_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: &str,
        source_layout: &ValueLayout,
        target_layout: &ValueLayout,
        key_bridge: Option<&LayoutBridge>,
        element_bridge: &LayoutBridge,
    ) -> String {
        self.require_stdlib();
        let source = self.hoist_tmp_value_statement(statements, "src", value);
        let direction = key_bridge
            .and_then(LayoutBridge::direction)
            .or_else(|| element_bridge.direction())
            .expect("aggregate layout bridge must contain an option bridge");
        let output_hint = match direction {
            BridgeDirection::ToGo => "unwrapped",
            BridgeDirection::FromGo => "wrapped",
        };
        let target_type = target_layout.go_type(self);
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
            self.hoist_tmp_value_statement(
                statements,
                output_hint,
                &format!("make({}, len({}))", target_type, source),
            )
        };
        let index = self.fresh_var(Some("i"));
        self.declare(&index);
        let element = self.fresh_var(Some("v"));
        self.declare(&element);
        let mut key_statements = Vec::new();
        let output_index = key_bridge.map_or_else(
            || index.clone(),
            |bridge| self.plan_layout_bridge(&mut key_statements, &index, bridge),
        );
        let mut body = self.plan_aggregate_element_bridge(
            &output,
            &output_index,
            &element,
            source_layout,
            element_bridge,
        );
        if !key_statements.is_empty() {
            key_statements.append(&mut body.statements);
            body.statements = key_statements;
        }
        statements.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            label: None,
            header: format!("for {index}, {element} := range {source} {{\n"),
            body,
        }));
        output
    }

    fn plan_aggregate_element_bridge(
        &mut self,
        output: &str,
        index: &str,
        element: &str,
        source_layout: &ValueLayout,
        bridge: &LayoutBridge,
    ) -> LoweredBlock {
        match bridge {
            LayoutBridge::UnwrapNullableOption { payload, .. }
            | LayoutBridge::UnwrapPointerOption { payload, .. } => {
                let pointer = matches!(bridge, LayoutBridge::UnwrapPointerOption { .. });
                let mut then_statements = Vec::new();
                let projected = format!("{element}.SomeVal");
                let projected = if payload.is_identity() {
                    projected
                } else {
                    self.plan_layout_bridge(&mut then_statements, &projected, payload)
                };
                let projected = if pointer {
                    format!("&{projected}")
                } else {
                    projected
                };
                then_statements.push(LoweredStatement::RawGo(format!(
                    "{output}[{index}] = {projected}\n"
                )));
                let needs_nil_else = matches!(source_layout, ValueLayout::Map { .. }) || pointer;
                let else_arm = if needs_nil_else {
                    ElseArm::Else {
                        body: LoweredBlock {
                            statements: vec![LoweredStatement::RawGo(format!(
                                "{output}[{index}] = nil\n"
                            ))],
                        },
                        inline: false,
                    }
                } else {
                    ElseArm::None
                };
                LoweredBlock {
                    statements: vec![LoweredStatement::If(IfPlan {
                        condition_setup: Vec::new(),
                        condition: format!("{element}.Tag == {OPTION_SOME_TAG}"),
                        then_body: LoweredBlock {
                            statements: then_statements,
                        },
                        else_arm,
                    })],
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
                let raw_payload = if pointer {
                    format!("*{element}")
                } else {
                    element.to_string()
                };
                let mut then_statements = Vec::new();
                let payload = if payload.is_identity() {
                    raw_payload
                } else {
                    self.plan_layout_bridge(&mut then_statements, &raw_payload, payload)
                };
                let fallible = Fallible::from_type(option_type).expect("Option type expected");
                let some = {
                    let mut planner = FalliblePlanner::new(self, &fallible);
                    planner.emit_success(&payload)
                };
                let none = {
                    let mut planner = FalliblePlanner::new(self, &fallible);
                    planner.emit_failure(None)
                };
                then_statements.push(LoweredStatement::RawGo(format!(
                    "{output}[{index}] = {some}\n"
                )));
                let condition = if !pointer && self.is_interface_option(option_type) {
                    format!("!lisette.IsNilInterface({element})")
                } else {
                    format!("{element} != nil")
                };
                LoweredBlock {
                    statements: vec![LoweredStatement::If(IfPlan {
                        condition_setup: Vec::new(),
                        condition,
                        then_body: LoweredBlock {
                            statements: then_statements,
                        },
                        else_arm: ElseArm::Else {
                            body: LoweredBlock {
                                statements: vec![LoweredStatement::RawGo(format!(
                                    "{output}[{index}] = {none}\n"
                                ))],
                            },
                            inline: false,
                        },
                    })],
                }
            }
            LayoutBridge::Aggregate { .. } => {
                let mut inner_statements = Vec::new();
                let inner = self.plan_layout_bridge(&mut inner_statements, element, bridge);
                inner_statements.push(LoweredStatement::RawGo(format!(
                    "{output}[{index}] = {inner}\n"
                )));
                LoweredBlock {
                    statements: inner_statements,
                }
            }
            LayoutBridge::Identity => LoweredBlock {
                statements: vec![LoweredStatement::RawGo(format!(
                    "{output}[{index}] = {element}\n"
                ))],
            },
        }
    }
}
