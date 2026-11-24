use crate::EmitFacts;
use crate::Planner;
use crate::lowering::abi::coercion_plan::LayoutChange;
use crate::lowering::abi::layout::{ResultProducer, ValueLayout};
use crate::lowering::calls::go_interop::{LoweredProducer, build_tuple_literal};
use crate::lowering::control_flow::propagation::plain_return;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, declare, define,
    define_many,
};
use crate::target::ir::go_expression::{BinaryOp, UnaryOp};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{partial_variant, prelude_call};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use syntax::program::SlotOrigin;
use syntax::types::{GoNilForm, Type};

pub(in crate::lowering) fn nil_condition(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, BinaryOp::Eq, GoExpression::nil())
}

pub(in crate::lowering) fn non_nil_condition(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, BinaryOp::Ne, GoExpression::nil())
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
                BinaryOp::Eq,
                GoExpression::literal(sentinel.to_string()),
            ),
        }
    }

    pub(in crate::lowering) fn non_nil_condition(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => non_nil_condition(value),
            NilGuard::Interface => {
                GoExpression::unary(UnaryOp::Not, nil_interface_condition(value))
            }
            NilGuard::Sentinel(sentinel) => GoExpression::binary(
                value,
                BinaryOp::Ne,
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
                self.declare(name.spelling());
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
            let val_var = self.fresh_declared("ret");
            let error_variable = self.fresh_declared("ret");
            statements.push(define_many(
                vec![val_var.clone(), error_variable.clone()],
                call,
            ));
            (error_variable, GoExpression::identifier(val_var))
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
            WrapperTarget::FreshSlot => {
                Some(self.hoist_tmp_value_statement(statements, name_hint, value))
            }
            WrapperTarget::Slot(name) => {
                self.declare(name.spelling());
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
        changes: Option<(&LayoutChange, &LayoutChange)>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let (payload_layout_change, error_layout_change) = changes.unzip();
        let ValueLayout::Fallible { payload, error, .. } = partial_layout.underlying() else {
            unreachable!("Partial wrapping requires a fallible layout")
        };
        let ok_ty = payload.logical_type();
        let ok_ty_str = payload.go_type(self);
        let err_ty_str = error.go_type(self);

        let mut statements = Vec::new();
        let (error_variable, val_value) =
            self.push_go_returns(&mut statements, call, payload, layout);
        let error_value = || GoExpression::identifier(error_variable.clone());
        let val = || val_value.clone();
        let nil_check = self.partial_ok_nil_check(ok_ty, val());

        let type_params = [ok_ty_str, err_ty_str];
        let result_ty_str = GoType::prelude("Partial", &type_params);
        let type_params = GoType::type_arguments(&type_params);
        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut both_setup, both_value) =
            self.plan_optional_payload_layout_change(val(), payload_layout_change);
        let (error_setup, both_error) =
            self.plan_optional_payload_layout_change(error_value(), error_layout_change);
        both_setup.extend(error_setup);
        let both = prelude_call(
            partial_variant("Both").constructor,
            type_params.clone(),
            vec![both_value, both_error],
        );
        both_setup.push(leaf_statement(&sink, both));
        let both_body = LoweredBlock {
            statements: both_setup,
        };

        let (mut ok_setup, ok_value) =
            self.plan_optional_payload_layout_change(val(), payload_layout_change);
        ok_setup.push(leaf_statement(
            &sink,
            prelude_call(
                partial_variant("Ok").constructor,
                type_params.clone(),
                vec![ok_value],
            ),
        ));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let then_body = if let Some(check) = nil_check {
            let (mut error_setup, error) =
                self.plan_optional_payload_layout_change(error_value(), error_layout_change);
            error_setup.push(leaf_statement(
                &sink,
                prelude_call(partial_variant("Err").constructor, type_params, vec![error]),
            ));
            let inner = IfPlan::plain(
                check,
                LoweredBlock {
                    statements: error_setup,
                },
                ElseArm::from_body(both_body, false),
            );
            LoweredBlock {
                statements: vec![LoweredStatement::If(inner).into()],
            }
        } else {
            both_body
        };

        let else_arm = ElseArm::from_body(ok_body, false);

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
        &mut self,
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
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_layout_change: Option<&LayoutChange>,
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
            || payload_layout_change.is_some()
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
        changes: Option<(&LayoutChange, &LayoutChange)>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let Some(shape @ LoweredReturnAbi::Result { payload: layout }) = producer.abi.lowered()
        else {
            unreachable!("Result wrapping requires a lowered Result ABI")
        };
        let layout = *layout;
        let nil_guard =
            self.lowered_payload_nil_guard(shape, producer.producer, result_layout.logical_type());
        let (payload_layout_change, error_layout_change) = changes.unzip();
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
        let call = match self.result_from_pair(
            call,
            result_layout,
            layout,
            payload_layout_change,
            nil_guard,
        ) {
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

        let (mut ok_setup, ok_value) =
            self.plan_optional_payload_layout_change(ok(), payload_layout_change);
        let ok_wrapper = prelude_call(
            fallible.ok_constructor(),
            type_params.clone(),
            vec![ok_value],
        );
        ok_setup.push(leaf_statement(&sink, ok_wrapper));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let (mut error_setup, error) =
            self.plan_optional_payload_layout_change(error_value(), error_layout_change);
        let err_wrapper =
            prelude_call(fallible.err_constructor(), type_params.clone(), vec![error]);
        error_setup.push(leaf_statement(&sink, err_wrapper));
        let then_body = LoweredBlock {
            statements: error_setup,
        };

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
                ElseArm::from_body(ok_body, false),
            )))
        } else {
            ElseArm::from_body(ok_body, false)
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

    pub(in crate::lowering) fn lower_bare_error_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        error_layout_change: Option<&LayoutChange>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let fallible = self
            .facts
            .fallible(result_layout.underlying().logical_type())
            .expect("Result type expected");
        debug_assert!(fallible.ok_ty().is_unit());
        self.lower_unit_result_wrapping(call, result_layout, error_layout_change, target)
    }

    fn plan_optional_payload_layout_change(
        &mut self,
        value: GoExpression,
        change: Option<&LayoutChange>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match change {
            Some(change) => self.plan_layout_change(&mut statements, value, change, true),
            None => value,
        };
        (statements, value)
    }

    fn lower_unit_result_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        error_layout_change: Option<&LayoutChange>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let error_variable = self.hoist_tmp_value_statement(&mut statements, "ret", call);
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

        let (mut error_setup, error) =
            self.plan_optional_payload_layout_change(error_value(), error_layout_change);
        let err_wrapper =
            prelude_call(fallible.err_constructor(), type_params.clone(), vec![error]);
        error_setup.push(leaf_statement(&sink, err_wrapper));
        let then_body = LoweredBlock {
            statements: error_setup,
        };

        let ok_wrapper = {
            prelude_call(
                fallible.ok_constructor(),
                type_params,
                vec![self.unit_value(None, None)],
            )
        };
        let else_arm = ElseArm::from_body(leaf_block(&sink, ok_wrapper), false);

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
        let raw = self.hoist_tmp_value_statement(&mut statements, "ret", call);
        let raw = || GoExpression::identifier(raw.clone());
        let option_layout = self.value_layout(option_ty, SlotOrigin::Lisette);
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
                    BinaryOp::Ne,
                    GoExpression::literal(sentinel.to_string()),
                ),
            ],
        );
        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
    }

    pub(in crate::lowering) fn option_from_comma_ok(
        &mut self,
        call: GoExpression,
        option_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_layout_change: Option<&LayoutChange>,
    ) -> Result<GoExpression, GoExpression> {
        let option_ty = option_layout.underlying().logical_type();
        let payload = option_layout
            .payload()
            .expect("Option wrapping requires a payload layout");
        if payload_layout_change.is_some()
            || matches!(
                self.value_layout(option_ty, SlotOrigin::GoReturn),
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
        payload_layout_change: Option<&LayoutChange>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let call =
            match self.option_from_comma_ok(call, option_layout, layout, payload_layout_change) {
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

        let val_vars = if layout.is_flattened()
            && let Some(arity) = inner_tuple_arity
        {
            self.create_temp_vars("ret", arity)
        } else {
            self.create_temp_vars("ret", 1)
        };
        let value_variable = self.fresh_declared("ret");

        let mut all_vars = val_vars.clone();
        all_vars.push(value_variable.clone());
        statements.push(define_many(all_vars, call));

        let first_val = GoExpression::identifier(val_vars[0].clone());
        let val_expression = if layout.is_flattened() && inner_tuple_arity.is_some() {
            let values = val_vars
                .iter()
                .map(|variable| GoExpression::identifier(variable.clone()))
                .collect();
            build_tuple_literal(values)
        } else {
            first_val.clone()
        };
        let (mut payload_setup, val_expression) = match payload_layout_change {
            Some(change) => {
                let mut setup = Vec::new();
                let value = self.plan_layout_change(&mut setup, val_expression, change, true);
                (setup, value)
            }
            None => (Vec::new(), val_expression),
        };

        let option_ty_str = option_layout.underlying().go_type(self);

        let ok = GoExpression::identifier(value_variable);
        let condition = match nullable_option_nil_guard(&self.facts, option_ty) {
            Some(guard) => {
                GoExpression::binary(ok, BinaryOp::And, guard.non_nil_condition(first_val))
            }
            None => ok,
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &option_ty_str, "option");

        let type_params = GoType::type_argument(&inner_ty_str);
        let some_wrapper = prelude_call(
            fallible.ok_constructor(),
            type_params.clone(),
            vec![val_expression],
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
                ElseArm::from_body(leaf_block(&sink, none_wrapper), false),
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
        let inner = layout.payload().expect("Option has a payload layout");
        let inner_type = inner.go_type(self);
        let inner_ty_str = inner_type;
        let is_nil_check =
            option_nil_guard(&self.facts, layout.logical_type()).nil_condition(raw_value.clone());
        let value = prelude_call(
            "OptionFromNilable",
            GoType::type_argument(&inner_ty_str),
            vec![raw_value, is_nil_check],
        );
        let outcome = self.push_simple_wrapper_value(&mut statements, target, "option", value);
        (statements, outcome)
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
