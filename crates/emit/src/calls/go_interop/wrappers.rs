use crate::Planner;
use crate::abi::callable::{CallableAbi, CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::{LayoutBridge, resolve_layout_bridge};
use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::abi::transition::multi_value_return;
use crate::control_flow::fallible::{
    Fallible, FalliblePlanner, PARTIAL_BOTH_CTOR, PARTIAL_ERR_CTOR, PARTIAL_OK_CTOR, prelude_call,
};
use crate::control_flow::propagation::plain_return;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, assign, define, define_many,
    expression_statement,
};
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::types::{FunctionParameter, Type};

pub(crate) fn is_nil(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, "==", GoExpression::nil())
}

pub(crate) fn non_nil(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, "!=", GoExpression::nil())
}

pub(crate) fn is_nil_interface(value: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Prelude, "IsNilInterface"),
        vec![value],
    )
}

#[derive(Clone, Copy)]
pub(crate) enum NilGuard {
    /// Pointer ok-type: `ptr == nil`.
    Pointer,
    /// Non-error interface ok-type: `lisette.IsNilInterface(v)`.
    Interface,
}

impl NilGuard {
    pub(crate) fn is_nil(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => is_nil(value),
            NilGuard::Interface => is_nil_interface(value),
        }
    }

    pub(crate) fn non_nil(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => non_nil(value),
            NilGuard::Interface => GoExpression::unary("!", is_nil_interface(value)),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum WrapperTarget<'a> {
    /// Allocate a fresh `var slot T` and write `slot = X` per branch.
    FreshSlot,
    /// Write `slot = X` per branch into the caller-provided slot name.
    Slot(&'a str),
    /// Emit `return X` per branch; caller skips its trailing return.
    Return,
}

/// `Some(slot_name)` when the wrapper wrote into a fresh or named slot; `None`
/// when it wrote a `return` statement and the caller should not emit its own.
pub(crate) type WrapperOutcome = Option<String>;

pub(super) enum ResolvedSink {
    Slot(String),
    Return,
}

/// `slot = value` or `return value`.
fn leaf_statement(sink: &ResolvedSink, value: GoExpression) -> LoweredStatement {
    match sink {
        ResolvedSink::Slot(name) => assign(GoExpression::name(name.clone()), value),
        ResolvedSink::Return => plain_return(value),
    }
}

/// A single-statement branch body for a wrapper-dispatch `If`.
pub(super) fn leaf_block(sink: &ResolvedSink, value: GoExpression) -> LoweredBlock {
    LoweredBlock {
        statements: vec![leaf_statement(sink, value)],
    }
}

fn tuple_slots(layout: &ValueLayout) -> &[ValueLayout] {
    match layout {
        ValueLayout::Tuple { elements, .. } => elements,
        ValueLayout::Named { underlying, .. } => tuple_slots(underlying),
        _ => unreachable!("a tuple payload is a tuple layout"),
    }
}

fn names(names: &[String]) -> Vec<GoExpression> {
    names
        .iter()
        .map(|name| GoExpression::name(name.clone()))
        .collect()
}

impl Planner<'_> {
    pub(crate) fn plan_function_layout_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> GoExpression {
        debug_assert!(source.return_abi.same_logical_contract(&target.return_abi));
        let function = self.hoist_tmp_value_statement(statements, "cb", value);
        let mut body = Vec::new();
        let mut parameters = Vec::new();
        let mut arguments = Vec::new();

        for (index, (source, target)) in
            source.parameters.iter().zip(&target.parameters).enumerate()
        {
            let name = format!("arg{index}");
            let target_type = target.go_type(self);
            let target_type = self.use_rendered_go_type(target_type);
            parameters.push(format!("{name} {target_type}"));
            let bridge = resolve_layout_bridge(self, target, source);
            let argument = self.plan_layout_bridge(&mut body, GoExpression::name(name), &bridge);
            if source.logical_type().get_name() == Some("VarArgs") {
                arguments.push(GoExpression::spread(argument));
            } else {
                arguments.push(argument);
            }
        }

        let call = GoExpression::call(GoExpression::name(function), arguments);
        self.plan_function_result_bridge(&mut body, call, source, target);
        let result = target
            .result_go_type(self)
            .map(|result| self.use_rendered_go_type(result))
            .unwrap_or_default();
        GoExpression::function_literal(
            parameters.join(", "),
            result,
            LoweredBlock { statements: body },
            FunctionLiteralLayout::MultiLine,
        )
    }

    fn plan_function_result_bridge(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        match &source.return_abi {
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct => {
                if source.result.logical_type().is_unit() {
                    statements.push(expression_statement(call));
                    return;
                }
                let bridge = resolve_layout_bridge(self, &source.result, &target.result);
                let value = self.plan_layout_bridge(statements, call, &bridge);
                statements.push(plain_return(value));
            }
            CallableReturnAbi::BareError => statements.push(plain_return(call)),
            CallableReturnAbi::Result { .. }
            | CallableReturnAbi::Partial { .. }
            | CallableReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
                let source_payload = source
                    .payload
                    .as_deref()
                    .expect("lowered callable has a payload layout");
                let target_payload = target
                    .payload
                    .as_deref()
                    .expect("lowered callable target has a payload layout");
                let source_flat = source.return_abi.has_flattened_payload();
                let slot_count = if source_flat {
                    tuple_slots(source_payload).len()
                } else {
                    1
                };
                let mut values = self.create_temp_vars("ret", slot_count + 1);
                statements.push(define_many(values.clone(), call));
                let auxiliary = values.pop().expect("a lowered callable has a status slot");

                if matches!(source.return_abi, CallableReturnAbi::Partial { .. }) && !source_flat {
                    let ok_type = source.result.logical_type().ok_type();
                    if let Some(condition) =
                        self.partial_ok_nil_check(&ok_type, GoExpression::name(values[0].clone()))
                    {
                        statements.push(LoweredStatement::If(IfPlan::plain(
                            condition,
                            LoweredBlock {
                                statements: vec![multi_value_return(vec![
                                    GoExpression::nil(),
                                    GoExpression::name(auxiliary.clone()),
                                ])],
                            },
                            ElseArm::None,
                        )));
                    }
                }

                let mut values = self.bridge_payload_slots(
                    statements,
                    names(&values),
                    source_payload,
                    target_payload,
                    target.return_abi.has_flattened_payload(),
                );
                values.push(GoExpression::name(auxiliary));
                statements.push(multi_value_return(values));
            }
            CallableReturnAbi::Option(OptionReturnAbi::Nullable) => {
                let raw = self.hoist_tmp_value_statement(statements, "raw", call);
                let condition = if self.is_interface_option(source.result.logical_type()) {
                    is_nil_interface(GoExpression::name(raw.clone()))
                } else {
                    is_nil(GoExpression::name(raw.clone()))
                };
                statements.push(LoweredStatement::If(IfPlan::plain(
                    condition,
                    LoweredBlock {
                        statements: vec![plain_return(GoExpression::nil())],
                    },
                    ElseArm::None,
                )));
                let source_payload = source
                    .payload
                    .as_deref()
                    .expect("nullable option callable has a payload layout");
                let target_payload = target
                    .payload
                    .as_deref()
                    .expect("nullable option target has a payload layout");
                let bridge = resolve_layout_bridge(self, source_payload, target_payload);
                let value = self.plan_layout_bridge(statements, GoExpression::name(raw), &bridge);
                statements.push(plain_return(value));
            }
            CallableReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => {
                statements.push(plain_return(call))
            }
            CallableReturnAbi::Tuple { arity } => {
                let values = self.create_temp_vars("ret", *arity);
                statements.push(define_many(values.clone(), call));
                let values = names(&values);
                let (
                    ValueLayout::Tuple {
                        elements: source, ..
                    },
                    ValueLayout::Tuple {
                        elements: target, ..
                    },
                ) = (source.result.as_ref(), target.result.as_ref())
                else {
                    statements.push(multi_value_return(values));
                    return;
                };
                let values = values
                    .into_iter()
                    .zip(source.iter().zip(target))
                    .map(|(value, (source, target))| {
                        let bridge = resolve_layout_bridge(self, source, target);
                        self.plan_layout_bridge(statements, value, &bridge)
                    })
                    .collect::<Vec<_>>();
                statements.push(multi_value_return(values));
            }
        }
    }

    /// `values` holds one Go value per source slot: the packed payload, or one
    /// value per tuple element when the source is flattened.
    fn bridge_payload_slots(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        values: Vec<GoExpression>,
        source: &ValueLayout,
        target: &ValueLayout,
        target_flat: bool,
    ) -> Vec<GoExpression> {
        let source_flat = values.len() > 1;
        if !source_flat && !target_flat {
            let bridge = resolve_layout_bridge(self, source, target);
            let [value] = <[GoExpression; 1]>::try_from(values)
                .unwrap_or_else(|_| unreachable!("a packed payload is one value"));
            return vec![self.plan_layout_bridge(statements, value, &bridge)];
        }
        let source_slots = tuple_slots(source);
        let target_slots = tuple_slots(target);
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
                let bridge = resolve_layout_bridge(self, source, target);
                self.plan_layout_bridge(statements, value, &bridge)
            })
            .collect();
        if target_flat {
            bridged
        } else {
            vec![self.plan_tuple_from_vars(statements, bridged)]
        }
    }

    /// Prepare a wrapper sink: declare `var slot T` (slot targets) or route
    /// writes to `return`.
    pub(super) fn push_wrapper_slot(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        target: WrapperTarget<'_>,
        type_str: &str,
        name_hint: &'static str,
    ) -> (ResolvedSink, WrapperOutcome) {
        match target {
            WrapperTarget::FreshSlot => {
                let var = self.fresh_var(Some(name_hint));
                self.declare(&var);
                statements.push(LoweredStatement::VarDecl {
                    name: var.clone(),
                    go_type: type_str.to_string(),
                    value: None,
                });
                (ResolvedSink::Slot(var.clone()), Some(var))
            }
            WrapperTarget::Slot(name) => {
                statements.push(LoweredStatement::VarDecl {
                    name: name.to_string(),
                    go_type: type_str.to_string(),
                    value: None,
                });
                self.declare(name);
                let owned = name.to_string();
                (ResolvedSink::Slot(owned.clone()), Some(owned))
            }
            WrapperTarget::Return => (ResolvedSink::Return, None),
        }
    }

    /// Destructure a Go multi-return into error and value temps. A `Flattened`
    /// tuple ok type (Go-imported `(T1, ..., Tn, error)`) gets N+1 temps and a
    /// rebuilt Lisette tuple; a `Packed` one (Lisette `(Tuple_n[...], error)`)
    /// gets 2 temps like any other ok type.
    fn push_go_returns(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        call: GoExpression,
        ok_ty: &Type,
        layout: PayloadLayout,
    ) -> (String, GoExpression) {
        if layout.is_flattened()
            && let Type::Tuple(elements) = ok_ty
        {
            let tuple_arity = elements.len();
            let temp_vars = self.create_temp_vars("ret", tuple_arity + 1);
            statements.push(define_many(temp_vars.clone(), call));
            let tuple = self.plan_tuple_from_vars(statements, names(&temp_vars[..tuple_arity]));
            (temp_vars.last().unwrap().clone(), tuple)
        } else {
            let val_var = self.fresh_var(Some("ret"));
            self.declare(&val_var);
            let err_var = self.fresh_var(Some("ret"));
            self.declare(&err_var);
            statements.push(define_many(vec![val_var.clone(), err_var.clone()], call));
            (err_var, GoExpression::name(val_var))
        }
    }

    /// Single-leaf write for wrappers that fold to one constructor expression.
    pub(super) fn push_simple_wrapper_value(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        target: WrapperTarget<'_>,
        name_hint: &'static str,
        value: GoExpression,
    ) -> WrapperOutcome {
        match target {
            WrapperTarget::FreshSlot => {
                Some(self.hoist_tmp_value_statement(statements, name_hint, value))
            }
            WrapperTarget::Slot(name) => {
                self.declare(name);
                statements.push(define(name.to_string(), value));
                Some(name.to_string())
            }
            WrapperTarget::Return => {
                statements.push(plain_return(value));
                None
            }
        }
    }
}

impl Planner<'_> {
    /// Lower a `(T, error)` Go return into a tagged `Partial`.
    pub(crate) fn lower_partial_wrapping(
        &mut self,
        call: GoExpression,
        partial_ty: &Type,
        layout: PayloadLayout,
        payload_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let ok_ty = partial_ty.ok_type();
        let err_ty = partial_ty.err_type();
        let ok_ty_str = self.use_go_type(&ok_ty);
        let err_ty_str = self.use_go_type(&err_ty);

        let mut statements = Vec::new();
        let (err_var, val_value) = self.push_go_returns(&mut statements, call, &ok_ty, layout);
        let err = || GoExpression::name(err_var.clone());
        let val = || val_value.clone();
        let nil_check = self.partial_ok_nil_check(&ok_ty, val());

        let type_params = format!("[{}, {}]", ok_ty_str, err_ty_str);
        let result_ty_str = self.use_rendered_go_type(GoType::stdlib(format!(
            "{}.Partial{type_params}",
            go_name::GO_STDLIB_PKG
        )));
        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut both_setup, both_value) = self.plan_optional_payload_bridge(val(), payload_bridge);
        let both = prelude_call(
            PARTIAL_BOTH_CTOR,
            type_params.clone(),
            vec![both_value, err()],
        );
        both_setup.push(leaf_statement(&sink, both));
        let both_body = LoweredBlock {
            statements: both_setup,
        };

        let (mut ok_setup, ok_value) = self.plan_optional_payload_bridge(val(), payload_bridge);
        ok_setup.push(leaf_statement(
            &sink,
            prelude_call(PARTIAL_OK_CTOR, type_params.clone(), vec![ok_value]),
        ));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let then_body = if let Some(check) = nil_check {
            let inner = IfPlan::plain(
                check,
                leaf_block(
                    &sink,
                    prelude_call(PARTIAL_ERR_CTOR, type_params, vec![err()]),
                ),
                ElseArm::from_body(both_body, false),
            );
            LoweredBlock {
                statements: vec![LoweredStatement::If(inner)],
            }
        } else {
            both_body
        };

        let else_arm = ElseArm::from_body(ok_body, false);

        statements.push(LoweredStatement::If(IfPlan::plain(
            non_nil(err()),
            then_body,
            else_arm,
        )));
        (statements, outcome)
    }

    /// Whether a `Partial` ok value can be Go nil, which makes the bare `Err`
    /// variant reachable (a nil value alongside a non-nil error).
    pub(crate) fn partial_ok_is_nilable(&self, ok_ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ok_ty);
        self.facts.is_nilable_go_type(ok_ty) || peeled.is_slice()
    }

    pub(crate) fn partial_ok_nil_guard(&self, ok_ty: &Type) -> Option<NilGuard> {
        self.partial_ok_is_nilable(ok_ty).then(|| {
            if self.facts.as_interface(ok_ty).is_some() {
                NilGuard::Interface
            } else {
                NilGuard::Pointer
            }
        })
    }

    pub(crate) fn partial_ok_nil_check(
        &mut self,
        ok_ty: &Type,
        value: GoExpression,
    ) -> Option<GoExpression> {
        let guard = self.partial_ok_nil_guard(ok_ty)?;
        Some(guard.is_nil(value))
    }

    fn go_result_needs_nil_guard(&self, ok_ty: &Type) -> bool {
        ok_ty.is_ref()
            || self
                .facts
                .as_interface(ok_ty)
                .as_deref()
                .is_some_and(|id| id != go_name::PRELUDE_ERROR_ID)
    }

    pub(crate) fn result_nil_guard(&self, ok_ty: &Type) -> Option<NilGuard> {
        if !self.go_result_needs_nil_guard(ok_ty) {
            return None;
        }
        Some(if self.facts.is_interface(ok_ty) {
            NilGuard::Interface
        } else {
            NilGuard::Pointer
        })
    }

    /// Lower a `(T, error)` Go return into a tagged `Result`.
    pub(crate) fn lower_result_wrapping(
        &mut self,
        call: GoExpression,
        result_ty: &Type,
        layout: PayloadLayout,
        payload_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let fallible = Fallible::from_type(result_ty).expect("Result type expected");
        debug_assert!(!fallible.ok_ty().is_unit());

        let mut statements = Vec::new();
        let ok_ty = fallible.ok_ty();
        let (err_var, ok_value) = self.push_go_returns(&mut statements, call, ok_ty, layout);
        let err = || GoExpression::name(err_var.clone());
        let ok = || ok_value.clone();

        let result_ty_str = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.full_type_string()
        };

        let needs_nil_guard = self.go_result_needs_nil_guard(ok_ty);

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut ok_setup, ok_value) = self.plan_optional_payload_bridge(ok(), payload_bridge);
        let ok_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_success(ok_value)
        };
        ok_setup.push(leaf_statement(&sink, ok_wrapper));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let err_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.emit_failure(Some(err()))
        };
        let then_body = leaf_block(&sink, err_wrapper);

        let else_arm = if needs_nil_guard {
            let nil_check = if ok_ty.is_tuple() {
                GoExpression::selector(ok(), "First".to_string())
            } else {
                ok()
            };
            let nil_condition = if self.facts.is_interface(ok_ty) {
                is_nil_interface(nil_check)
            } else {
                is_nil(nil_check)
            };
            let nil_err = {
                let mut fe = FalliblePlanner::new(self, &fallible);
                fe.emit_failure(Some(unexpected_nil_error()))
            };
            ElseArm::ElseIf(Box::new(IfPlan::plain(
                nil_condition,
                leaf_block(&sink, nil_err),
                ElseArm::from_body(ok_body, false),
            )))
        } else {
            ElseArm::from_body(ok_body, false)
        };

        statements.push(LoweredStatement::If(IfPlan::plain(
            non_nil(err()),
            then_body,
            else_arm,
        )));
        (statements, outcome)
    }

    /// Lower a bare `error` Go return into a tagged `Result<(), E>`.
    pub(crate) fn lower_bare_error_wrapping(
        &mut self,
        call: GoExpression,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let fallible = Fallible::from_type(result_ty).expect("Result type expected");
        debug_assert!(fallible.ok_ty().is_unit());
        self.lower_unit_result_wrapping(call, &fallible, target)
    }

    fn plan_optional_payload_bridge(
        &mut self,
        value: GoExpression,
        bridge: Option<&LayoutBridge>,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match bridge {
            Some(bridge) => self.plan_layout_bridge(&mut statements, value, bridge),
            None => value,
        };
        (statements, value)
    }

    fn lower_unit_result_wrapping(
        &mut self,
        call: GoExpression,
        fallible: &Fallible,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let err_var = self.hoist_tmp_value_statement(&mut statements, "ret", call);
        let err = || GoExpression::name(err_var.clone());

        let result_ty_str = {
            let mut fe = FalliblePlanner::new(self, fallible);
            fe.full_type_string()
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let err_wrapper = {
            let mut fe = FalliblePlanner::new(self, fallible);
            fe.emit_failure(Some(err()))
        };
        let then_body = leaf_block(&sink, err_wrapper);

        let ok_wrapper = {
            let mut fe = FalliblePlanner::new(self, fallible);
            fe.emit_success(GoExpression::empty_composite("struct{}".to_string()))
        };
        let else_arm = ElseArm::from_body(leaf_block(&sink, ok_wrapper), false);

        statements.push(LoweredStatement::If(IfPlan::plain(
            non_nil(err()),
            then_body,
            else_arm,
        )));
        (statements, outcome)
    }

    fn hoist_go_fn_if_needed(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
    ) -> GoExpression {
        let go_fn = self.capture_operand_into(setup, expression);

        let is_go_package_fn = matches!(
            expression.unwrap_parens(),
            Expression::DotAccess { expression, .. }
            if expression.get_type().as_import_namespace()
                .is_some_and(|m| m.starts_with(go_name::GO_IMPORT_PREFIX))
        );
        if is_go_package_fn {
            return go_fn;
        }

        if is_order_sensitive(expression) {
            GoExpression::name(self.hoist_tmp_value_statement(setup, "fn", go_fn))
        } else {
            go_fn
        }
    }

    pub(crate) fn build_wrapper_params(
        &mut self,
        params: &[FunctionParameter],
    ) -> (Vec<String>, Vec<GoExpression>) {
        let mut param_strs = Vec::new();
        let mut arguments = Vec::new();
        let last_index = params.len().saturating_sub(1);
        for (i, param) in params.iter().enumerate() {
            let name = format!("arg{}", i);
            let ty_str = self.use_go_type(&param.ty);
            param_strs.push(format!("{} {}", name, ty_str));
            let argument = GoExpression::name(name);
            if i == last_index && param.ty.get_name() == Some("VarArgs") {
                arguments.push(GoExpression::spread(argument));
            } else {
                arguments.push(argument);
            }
        }
        (param_strs, arguments)
    }

    /// Common wrapper-builder prologue: returns `(return_type, param_strs,
    /// call)` for a go-fn expression, or `None` for non-function types.
    fn wrapper_call_parts(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
    ) -> Option<(Type, Vec<String>, GoExpression)> {
        let fn_type = expression.get_type();
        let f = fn_type.as_function_type()?;
        let (params, return_type) = (f.params.clone(), (*f.return_type).clone());
        let go_fn = self.hoist_go_fn_if_needed(setup, expression);
        let (param_strs, arguments) = self.build_wrapper_params(&params);
        let call = GoExpression::call(go_fn, arguments);
        Some((return_type, param_strs, call))
    }

    pub(crate) fn emit_go_fn_wrapper(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        abi: &CallableAbi,
    ) -> GoExpression {
        let (return_type, param_strs, call) = self
            .wrapper_call_parts(setup, expression)
            .expect("expected function type");

        let ret_ty_str = self.use_go_type(&return_type);

        let mut statements = Vec::new();
        let outcome = match &abi.result {
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct => {
                unreachable!("passthrough Go function needs no wrapper")
            }
            CallableReturnAbi::Tuple { arity } => {
                let temp_vars = self.create_temp_vars("ret", *arity);
                statements.push(define_many(temp_vars.clone(), call));
                Some(self.plan_tuple_from_vars(&mut statements, names(&temp_vars)))
            }
            result => {
                let payload_bridge = self.go_return_payload_bridge(abi, &return_type);
                let (wrap, outcome) = self.lower_abi_wrapping_with_payload_bridge(
                    call,
                    result,
                    &return_type,
                    payload_bridge.as_ref(),
                    WrapperTarget::Return,
                );
                statements.extend(wrap);
                outcome.map(GoExpression::name)
            }
        };

        if let Some(result) = outcome {
            statements.push(plain_return(result));
        }

        GoExpression::function_literal(
            param_strs.join(", "),
            ret_ty_str,
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        )
    }

    /// Closure that bundles a raw `(T1, T2, error)` return into the slot's `(Tuple, error)` shape.
    pub(crate) fn emit_go_fn_lowered_tuple_adapter(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
    ) -> GoExpression {
        let (return_type, param_strs, call) = self
            .wrapper_call_parts(setup, expression)
            .expect("expected function type");

        let ok_ty = return_type.ok_type();
        let err_ty = return_type.err_type();
        let ret_ty_str = format!(
            "({}, {})",
            self.use_go_type(&ok_ty),
            self.use_go_type(&err_ty)
        );
        let arity = ok_ty.tuple_arity().expect("tuple ok type");

        let mut statements = Vec::new();
        let temp_vars = self.create_temp_vars("ret", arity + 1);
        statements.push(define_many(temp_vars.clone(), call));
        let tuple = self.plan_tuple_from_vars(&mut statements, names(&temp_vars[..arity]));
        statements.push(multi_value_return(vec![
            tuple,
            GoExpression::name(temp_vars[arity].clone()),
        ]));

        GoExpression::function_literal(
            param_strs.join(", "),
            ret_ty_str,
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        )
    }

    pub(crate) fn emit_go_fn_sentinel_adapter(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        sentinel: i64,
    ) -> GoExpression {
        let (return_type, param_strs, call) = self
            .wrapper_call_parts(setup, expression)
            .expect("expected function type");

        let inner_ty_str = self.use_go_type(&return_type.ok_type());
        let ret_var = self.fresh_var(Some("ret"));
        self.declare(&ret_var);
        let ret = || GoExpression::name(ret_var.clone());

        let statements = vec![
            define(ret_var.clone(), call),
            multi_value_return(vec![
                ret(),
                GoExpression::binary(ret(), "!=", GoExpression::literal(sentinel.to_string())),
            ]),
        ];

        GoExpression::function_literal(
            param_strs.join(", "),
            format!("({}, bool)", inner_ty_str),
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        )
    }
}

pub(crate) fn unexpected_nil_error() -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Errors, "New"),
        vec![GoExpression::literal("\"unexpected nil\"".to_string())],
    )
}
