use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::value_layout::{ResultProducer, ValueLayout};
use crate::lowering::abi::coercion_plan::LayoutChange;
use crate::lowering::abi::layout_change::PayloadLayoutChanges;
use crate::lowering::calls::go_interop::{LoweredProducer, build_tuple_literal};
use crate::lowering::control_flow::propagation::plain_return;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, declare, define,
    define_many,
};
use crate::target::ir::go_expression::{GoBinaryOperator, GoUnaryOperator};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{partial_variant, prelude_call};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use syntax::program::SlotOrigin;
use syntax::types::{GoNilForm, Type};

pub(in crate::lowering) fn nil_condition(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, GoBinaryOperator::Equal, GoExpression::nil())
}

pub(in crate::lowering) fn non_nil_condition(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, GoBinaryOperator::NotEqual, GoExpression::nil())
}

fn nil_interface_condition(value: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Prelude, "IsNilInterface"),
        vec![value],
    )
}

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) enum NilGuard {
    Pointer,
    Interface,
    Sentinel(i64),
}

impl NilGuard {
    pub(in crate::lowering) fn nil_condition(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => nil_condition(value),
            NilGuard::Interface => nil_interface_condition(value),
            NilGuard::Sentinel(sentinel) => GoExpression::binary(
                value,
                GoBinaryOperator::Equal,
                GoExpression::literal(sentinel.to_string()),
            ),
        }
    }

    pub(in crate::lowering) fn non_nil_condition(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => non_nil_condition(value),
            NilGuard::Interface => {
                GoExpression::unary(GoUnaryOperator::Not, nil_interface_condition(value))
            }
            NilGuard::Sentinel(sentinel) => GoExpression::binary(
                value,
                GoBinaryOperator::NotEqual,
                GoExpression::literal(sentinel.to_string()),
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum WrapperTarget<'a> {
    FreshSlot,
    Slot(&'a GoIdentifier),
    Return,
}

pub(in crate::lowering) type WrapperOutcome = Option<GoIdentifier>;

pub(super) enum ResolvedSink {
    Slot(GoIdentifier),
    Return,
}

fn leaf_statement(sink: &ResolvedSink, value: GoExpression) -> Statement {
    match sink {
        ResolvedSink::Slot(name) => assign(GoExpression::identifier(name.clone()), value),
        ResolvedSink::Return => plain_return(value),
    }
}

pub(super) fn leaf_block(sink: &ResolvedSink, value: GoExpression) -> LoweredBlock {
    LoweredBlock {
        statements: vec![leaf_statement(sink, value)],
    }
}

impl Planner<'_> {
    pub(super) fn push_wrapper_slot(
        &mut self,
        statements: &mut Vec<Statement>,
        target: WrapperTarget<'_>,
        type_str: &GoType,
        name_hint: &'static str,
    ) -> (ResolvedSink, WrapperOutcome) {
        match target {
            WrapperTarget::FreshSlot => {
                let variable = self.fresh_declared(name_hint);
                statements.push(declare(variable.clone(), type_str.clone()));
                (ResolvedSink::Slot(variable.clone()), Some(variable))
            }
            WrapperTarget::Slot(name) => {
                statements.push(declare(name.clone(), type_str.clone()));
                self.scope.declare_go_name(name.spelling());
                let owned = name.clone();
                (ResolvedSink::Slot(owned.clone()), Some(owned))
            }
            WrapperTarget::Return => (ResolvedSink::Return, None),
        }
    }

    fn push_go_returns(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        payload: &ValueLayout,
        layout: PayloadLayout,
    ) -> (GoIdentifier, GoExpression) {
        if layout.is_flattened()
            && let Some(elements) = payload.tuple_elements()
        {
            let tuple_arity = elements.len();
            let temp_vars = self.create_temp_vars("ret", tuple_arity + 1);
            statements.push(define_many(temp_vars.clone(), call));
            let tuple = self.plan_tuple_from_vars(
                statements,
                temp_vars[..tuple_arity]
                    .iter()
                    .map(|name| GoExpression::identifier(name.clone()))
                    .collect(),
            );

            (temp_vars.last().unwrap().clone(), tuple)
        } else {
            let value_var = self.fresh_declared("ret");
            let error_variable = self.fresh_declared("ret");
            statements.push(define_many(
                vec![value_var.clone(), error_variable.clone()],
                call,
            ));
            (error_variable, GoExpression::identifier(value_var))
        }
    }

    pub(super) fn push_simple_wrapper_value(
        &mut self,
        statements: &mut Vec<Statement>,
        target: WrapperTarget<'_>,
        name_hint: &'static str,
        value: GoExpression,
    ) -> WrapperOutcome {
        match target {
            WrapperTarget::FreshSlot => Some(self.hoist_temp(statements, name_hint, value)),
            WrapperTarget::Slot(name) => {
                self.scope.declare_go_name(name.spelling());
                statements.push(define(name.clone(), value));
                Some(name.clone())
            }
            WrapperTarget::Return => {
                statements.push(plain_return(value));
                None
            }
        }
    }

    pub(in crate::lowering) fn lower_partial_wrapping(
        &mut self,
        call: GoExpression,
        partial_layout: &ValueLayout,
        layout: PayloadLayout,
        changes: Option<&PayloadLayoutChanges>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let payload_layout_change = changes.map(|changes| &changes.value);
        let error_layout_change = changes.map(|changes| &changes.error);
        let ValueLayout::Fallible { payload, error, .. } = partial_layout.underlying() else {
            unreachable!("Partial wrapping requires a fallible layout")
        };
        let ok_ty = payload.logical_type();
        let ok_ty_str = payload.go_type(self);
        let err_ty_str = error.go_type(self);

        let mut statements = Vec::new();
        let (error_variable, payload_variable) =
            self.push_go_returns(&mut statements, call, payload, layout);
        let error_value = || GoExpression::identifier(error_variable.clone());
        let payload_value = || payload_variable.clone();
        let nil_check = self.partial_ok_nil_check(ok_ty, payload_value());

        let type_params = [ok_ty_str, err_ty_str];
        let result_ty_str = GoType::prelude("Partial", &type_params);
        let type_params = GoType::type_arguments(&type_params);
        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let both_body = self.changed_leaf_block(
            &sink,
            partial_variant("Both").constructor,
            type_params.clone(),
            vec![
                (payload_value(), payload_layout_change),
                (error_value(), error_layout_change),
            ],
        );

        let ok_body = self.changed_leaf_block(
            &sink,
            partial_variant("Ok").constructor,
            type_params.clone(),
            vec![(payload_value(), payload_layout_change)],
        );

        let then_body = if let Some(check) = nil_check {
            let error_body = self.changed_leaf_block(
                &sink,
                partial_variant("Err").constructor,
                type_params,
                vec![(error_value(), error_layout_change)],
            );

            let inner = IfPlan::plain(check, error_body, ElseArm::block(both_body));
            LoweredBlock {
                statements: vec![LoweredStatement::If(inner).into()],
            }
        } else {
            both_body
        };

        let else_arm = ElseArm::block(ok_body);

        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil_condition(error_value()),
                then_body,
                else_arm,
            ))
            .into(),
        );
        (statements, outcome)
    }

    pub(in crate::lowering) fn partial_ok_nil_check(
        &self,
        ok_ty: &Type,
        value: GoExpression,
    ) -> Option<GoExpression> {
        let guard = partial_ok_nil_guard(&self.facts, ok_ty)?;
        Some(guard.nil_condition(value))
    }

    pub(in crate::lowering) fn layout_nil_guard(&self, layout: &ValueLayout) -> Option<NilGuard> {
        match layout.underlying() {
            ValueLayout::PointerOption { .. } => Some(NilGuard::Pointer),
            ValueLayout::NullableOption { option_type, .. } => {
                Some(option_nil_guard(&self.facts, option_type))
            }
            _ => None,
        }
    }

    pub(in crate::lowering) fn lowered_payload_nil_guard(
        &self,
        shape: &LoweredReturnAbi,
        producer: ResultProducer,
        result_ty: &Type,
    ) -> Option<NilGuard> {
        match shape {
            LoweredReturnAbi::Result { .. } if producer == ResultProducer::Go => {
                self.result_nil_guard(&self.facts.ok_payload(result_ty))
            }
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
                Some(NilGuard::Sentinel(*value))
            }
            LoweredReturnAbi::Option(_) => nullable_option_nil_guard(&self.facts, result_ty),
            _ => None,
        }
    }

    fn result_nil_guard(&self, ok_ty: &Type) -> Option<NilGuard> {
        match nil_guard(&self.facts, ok_ty)? {
            NilGuard::Pointer => self
                .facts
                .is_reference_underneath(ok_ty)
                .then_some(NilGuard::Pointer),
            NilGuard::Interface => {
                (!self.facts.is_prelude_error(ok_ty)).then_some(NilGuard::Interface)
            }
            NilGuard::Sentinel(_) => None,
        }
    }

    pub(in crate::lowering) fn result_from_pair(
        &self,
        call: GoExpression,
        result_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_changes: Option<&PayloadLayoutChanges>,
        nil_guard: Option<NilGuard>,
    ) -> Result<GoExpression, GoExpression> {
        let Some(fallible) = self
            .facts
            .fallible(result_layout.underlying().logical_type())
        else {
            return Err(call);
        };

        let ok_ty = fallible.ok_ty();
        let err_is_error = fallible
            .err_ty()
            .is_some_and(|err_ty| self.facts.is_prelude_error(err_ty));
        if !err_is_error
            || payload_changes.is_some()
            || (layout.is_flattened() && ok_ty.is_tuple())
            || nil_guard.is_some()
        {
            return Err(call);
        }
        let payload = result_layout
            .payload()
            .expect("Result layout has a payload");
        Ok(prelude_call(
            "ResultFromPair",
            GoType::type_arguments(&[payload.go_type(self)]),
            vec![call],
        ))
    }

    pub(in crate::lowering) fn lower_result_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        producer: LoweredProducer<'_>,
        changes: Option<&PayloadLayoutChanges>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let Some(shape @ LoweredReturnAbi::Result { payload: layout }) = producer.abi.lowered()
        else {
            unreachable!("Result wrapping requires a lowered Result ABI")
        };
        let layout = *layout;
        let nil_guard =
            self.lowered_payload_nil_guard(shape, producer.producer, result_layout.logical_type());
        let ValueLayout::Fallible {
            fallible_type,
            payload,
            error,
        } = result_layout.underlying()
        else {
            unreachable!("Result wrapping requires a fallible layout")
        };

        let fallible = self
            .facts
            .fallible(fallible_type)
            .expect("Result type expected");

        let mut statements = Vec::new();
        let call = match self.result_from_pair(call, result_layout, layout, changes, nil_guard) {
            Ok(value) => {
                let outcome =
                    self.push_simple_wrapper_value(&mut statements, target, "result", value);
                return (statements, outcome);
            }
            Err(call) => call,
        };

        let (error_variable, ok_value) =
            self.push_go_returns(&mut statements, call, payload, layout);
        let error_value = || GoExpression::identifier(error_variable.clone());
        let ok = || ok_value.clone();

        let result_ty_str = result_layout.underlying().go_type(self);
        let type_params = GoType::type_arguments(&[payload.go_type(self), error.go_type(self)]);

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let ok_body = self.changed_leaf_block(
            &sink,
            fallible.ok_constructor(),
            type_params.clone(),
            vec![(ok(), changes.map(|changes| &changes.value))],
        );

        let then_body = self.changed_leaf_block(
            &sink,
            fallible.err_constructor(),
            type_params.clone(),
            vec![(error_value(), changes.map(|changes| &changes.error))],
        );

        let else_arm = if let Some(nil_guard) = nil_guard {
            let nil_condition = nil_guard.nil_condition(ok());
            let nil_err = prelude_call(
                fallible.err_constructor(),
                type_params,
                vec![unexpected_nil_error()],
            );
            ElseArm::ElseIf(Box::new(IfPlan::plain(
                nil_condition,
                leaf_block(&sink, nil_err),
                ElseArm::block(ok_body),
            )))
        } else {
            ElseArm::block(ok_body)
        };

        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil_condition(error_value()),
                then_body,
                else_arm,
            ))
            .into(),
        );
        (statements, outcome)
    }

    fn plan_optional_payload_layout_change(
        &mut self,
        value: GoExpression,
        change: Option<&LayoutChange>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match change {
            Some(change) => self.plan_layout_change(&mut statements, value, change),
            None => value,
        };
        (statements, value)
    }

    fn changed_leaf_block(
        &mut self,
        sink: &ResolvedSink,
        constructor: &str,
        type_params: GoType,
        values: Vec<(GoExpression, Option<&LayoutChange>)>,
    ) -> LoweredBlock {
        let mut statements = Vec::new();
        let mut arguments = Vec::with_capacity(values.len());
        for (value, change) in values {
            let (setup, value) = self.plan_optional_payload_layout_change(value, change);
            statements.extend(setup);
            arguments.push(value);
        }
        statements.push(leaf_statement(
            sink,
            prelude_call(constructor, type_params, arguments),
        ));
        LoweredBlock { statements }
    }

    pub(in crate::lowering) fn lower_unit_result_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        error_layout_change: Option<&LayoutChange>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let error_variable = self.hoist_temp(&mut statements, "ret", call);
        let error_value = || GoExpression::identifier(error_variable.clone());

        let ValueLayout::Fallible {
            fallible_type,
            payload,
            error,
        } = result_layout.underlying()
        else {
            unreachable!("unit Result wrapping requires a fallible layout")
        };

        let fallible = self
            .facts
            .fallible(fallible_type)
            .expect("Result type expected");
        let result_ty_str = result_layout.underlying().go_type(self);
        let type_params = GoType::type_arguments(&[payload.go_type(self), error.go_type(self)]);

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let then_body = self.changed_leaf_block(
            &sink,
            fallible.err_constructor(),
            type_params.clone(),
            vec![(error_value(), error_layout_change)],
        );

        let ok_wrapper = {
            prelude_call(
                fallible.ok_constructor(),
                type_params,
                vec![self.unit_value(None, None)],
            )
        };

        let else_arm = ElseArm::block(leaf_block(&sink, ok_wrapper));

        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil_condition(error_value()),
                then_body,
                else_arm,
            ))
            .into(),
        );
        (statements, outcome)
    }

    pub(in crate::lowering) fn lower_sentinel_wrapping(
        &mut self,
        call: GoExpression,
        option_ty: &Type,
        sentinel: i64,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let raw = self.hoist_temp(&mut statements, "ret", call);
        let raw = || GoExpression::identifier(raw.clone());
        let option_layout = self.facts.value_layout(option_ty, SlotOrigin::Lisette);
        let inner_ty_str = option_layout
            .payload()
            .expect("a sentinel Option has a payload")
            .go_type(self);
        let value = prelude_call(
            "OptionFromCommaOk",
            GoType::type_argument(&inner_ty_str),
            vec![
                raw(),
                GoExpression::binary(
                    raw(),
                    GoBinaryOperator::NotEqual,
                    GoExpression::literal(sentinel.to_string()),
                ),
            ],
        );

        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
    }

    pub(in crate::lowering) fn option_from_comma_ok(
        &self,
        call: GoExpression,
        option_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_changes: Option<&PayloadLayoutChanges>,
    ) -> Result<GoExpression, GoExpression> {
        let option_ty = option_layout.underlying().logical_type();
        let payload = option_layout
            .payload()
            .expect("Option wrapping requires a payload layout");
        if payload_changes.is_some()
            || matches!(
                self.facts.value_layout(option_ty, SlotOrigin::GoReturn),
                ValueLayout::NullableOption { .. }
            )
            || (layout.is_flattened() && payload.logical_type().tuple_arity().is_some())
        {
            return Err(call);
        }
        Ok(GoExpression::pure_call(
            GoExpression::instantiation(
                GoExpression::generated(GeneratedPackage::Prelude, "OptionFromCommaOk"),
                GoType::type_argument(&payload.go_type(self)),
            ),
            vec![call],
        ))
    }

    pub(in crate::lowering) fn lower_comma_ok_wrapping(
        &mut self,
        call: GoExpression,
        option_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_changes: Option<&PayloadLayoutChanges>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let call = match self.option_from_comma_ok(call, option_layout, layout, payload_changes) {
            Ok(value) => {
                let outcome =
                    self.push_simple_wrapper_value(&mut statements, target, "option", value);
                return (statements, outcome);
            }
            Err(call) => call,
        };

        let option_ty = option_layout.underlying().logical_type();
        let payload = option_layout
            .payload()
            .expect("Option wrapping requires a payload layout");
        let inner_ty = payload.logical_type();
        let inner_ty_str = payload.go_type(self);
        let inner_tuple_arity = inner_ty.tuple_arity();

        let fallible = self
            .facts
            .fallible(option_ty)
            .expect("Option type expected");

        let value_vars = if layout.is_flattened()
            && let Some(arity) = inner_tuple_arity
        {
            self.create_temp_vars("ret", arity)
        } else {
            self.create_temp_vars("ret", 1)
        };

        let value_variable = self.fresh_declared("ret");

        let mut all_vars = value_vars.clone();
        all_vars.push(value_variable.clone());
        statements.push(define_many(all_vars, call));

        let first_value = GoExpression::identifier(value_vars[0].clone());
        let value_expression = if layout.is_flattened() && inner_tuple_arity.is_some() {
            let values = value_vars
                .iter()
                .map(|variable| GoExpression::identifier(variable.clone()))
                .collect();
            build_tuple_literal(values)
        } else {
            first_value.clone()
        };

        let (mut payload_setup, value_expression) = self.plan_optional_payload_layout_change(
            value_expression,
            payload_changes.map(|changes| &changes.value),
        );

        let option_ty_str = option_layout.underlying().go_type(self);

        let ok = GoExpression::identifier(value_variable);
        let condition = match nullable_option_nil_guard(&self.facts, option_ty) {
            Some(guard) => GoExpression::binary(
                ok,
                GoBinaryOperator::And,
                guard.non_nil_condition(first_value),
            ),
            None => ok,
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &option_ty_str, "option");

        let type_params = GoType::type_argument(&inner_ty_str);
        let some_wrapper = prelude_call(
            fallible.ok_constructor(),
            type_params.clone(),
            vec![value_expression],
        );
        let none_wrapper = prelude_call(fallible.err_constructor(), type_params, Vec::new());

        let mut then_body = leaf_block(&sink, some_wrapper);
        payload_setup.append(&mut then_body.statements);
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                condition,
                LoweredBlock {
                    statements: payload_setup,
                },
                ElseArm::block(leaf_block(&sink, none_wrapper)),
            ))
            .into(),
        );
        (statements, outcome)
    }

    pub(in crate::lowering) fn lower_nil_check_option_wrap(
        &mut self,
        raw_value: GoExpression,
        layout: &ValueLayout,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let value = self.option_from_nilable(raw_value, layout);
        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
    }

    pub(in crate::lowering) fn option_from_nilable(
        &self,
        value: GoExpression,
        option_layout: &ValueLayout,
    ) -> GoExpression {
        let inner = option_layout
            .payload()
            .expect("Option has a payload layout");
        let nil = option_nil_guard(&self.facts, option_layout.logical_type())
            .nil_condition(value.clone());
        prelude_call(
            "OptionFromNilable",
            GoType::type_argument(&inner.go_type(self)),
            vec![value, nil],
        )
    }
}

pub(in crate::lowering) fn unexpected_nil_error() -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Errors, "New"),
        vec![GoExpression::literal("\"unexpected nil\"".to_string())],
    )
}

pub(in crate::lowering) fn nil_guard(facts: &EmitFacts<'_>, ty: &Type) -> Option<NilGuard> {
    match facts.go_nil_form(ty) {
        GoNilForm::Never => None,
        GoNilForm::Pointer => Some(NilGuard::Pointer),
        GoNilForm::Interface => Some(NilGuard::Interface),
    }
}

pub(in crate::lowering) fn partial_ok_nil_guard(
    facts: &EmitFacts<'_>,
    ok_ty: &Type,
) -> Option<NilGuard> {
    nil_guard(facts, ok_ty).or_else(|| {
        facts
            .partial_ok_is_nilable(ok_ty)
            .then_some(NilGuard::Pointer)
    })
}

fn nullable_option_nil_guard(facts: &EmitFacts<'_>, option_ty: &Type) -> Option<NilGuard> {
    if !facts.is_option(option_ty) {
        return None;
    }
    nil_guard(facts, &facts.ok_payload(option_ty))
}

pub(in crate::lowering) fn option_nil_guard(facts: &EmitFacts<'_>, option_ty: &Type) -> NilGuard {
    nullable_option_nil_guard(facts, option_ty).unwrap_or(NilGuard::Pointer)
}
