use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi};
use crate::calls::NativeMethodCall;
use crate::calls::bounds::BoundsCheckedIndex;
use crate::calls::comma_ok::CommaOkSource;
use crate::calls::comma_ok::{CommaOkValueSlot, LoweredPair, PairCondition, PairKind};
use crate::calls::go_interop::{NilGuard, is_nil, non_nil, unexpected_nil_error};
use crate::calls::slice_loop::FoundSink;
use crate::calls::wrap_err::WrapMessage;
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::decision_tree;
use crate::patterns::tree_emitter::{MatchSubject, TreePlanner};
use crate::plan::bodies::GoUses;
use crate::plan::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, assign, define,
    discard, expression_statement,
};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::state::scope::PairStatusKind;
use crate::types::native::NativeGoType;
use std::mem;
use syntax::ast::{Expression, Literal, MatchArm, Pattern};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

pub(crate) struct ResultFusePlan<'a> {
    subject: &'a Expression,
    shape: CallableReturnAbi,
    nil_guard: Option<NilGuard>,
    wraps: Vec<&'a Expression>,
}

impl ResultFusePlan<'_> {
    pub(crate) fn wraps_error(&self) -> bool {
        !self.wraps.is_empty()
    }
}

pub(crate) enum OptionFusePlan<'a> {
    CommaOk {
        subject: &'a Expression,
        source: CommaOkSource,
    },
    Nullable {
        subject: &'a Expression,
        nil_guard: NilGuard,
    },
    /// `xs.get(i)` on a slice or array: a bounds test guards a direct index.
    Index { call: NativeMethodCall<'a> },
    Found {
        subject: &'a Expression,
        call: NativeMethodCall<'a>,
    },
}

pub(crate) struct BoundOption {
    pub(crate) statements: Vec<LoweredStatement>,
    source: BoundSource,
}

enum BoundSource {
    Pair(LoweredPair),
    Nullable {
        value: String,
        nil_guard: NilGuard,
        initializer_call: Option<GoExpression>,
    },
    Index {
        index: BoundsCheckedIndex,
        target: Option<String>,
    },
    Found {
        value: Option<String>,
        flag: String,
    },
}

impl BoundOption {
    /// The payload expression, valid once the some-condition holds.
    pub(crate) fn value(&self) -> Option<GoExpression> {
        match &self.source {
            BoundSource::Index { index, .. } => Some(index.element.clone()),
            _ => self
                .value_name()
                .map(|name| GoExpression::name(name.to_string())),
        }
    }

    pub(crate) fn value_name(&self) -> Option<&str> {
        match &self.source {
            BoundSource::Pair(pair) => pair.value.as_deref(),
            BoundSource::Nullable { value, .. } => Some(value),
            BoundSource::Index { .. } => None,
            BoundSource::Found { value, .. } => value.as_deref(),
        }
    }

    pub(crate) fn binds_value(&self) -> bool {
        !matches!(self.source, BoundSource::Index { .. })
    }

    /// The statement that reads a late payload into its requested name.
    pub(crate) fn late_binding(&self) -> Option<LoweredStatement> {
        let BoundSource::Index {
            index,
            target: Some(target),
        } = &self.source
        else {
            return None;
        };
        Some(define(target.clone(), index.element.clone()))
    }

    pub(super) fn discard_value(&mut self) {
        match &mut self.source {
            BoundSource::Pair(pair) => pair.discard_value(),
            BoundSource::Found {
                value: Some(value), ..
            } => {
                self.statements
                    .push(discard(GoExpression::name(value.clone())));
            }
            _ => {}
        }
    }

    pub(crate) fn some_condition(&self, planner: &mut Planner<'_>) -> PairCondition {
        self.condition(planner, true)
    }

    pub(crate) fn none_condition(&self, planner: &mut Planner<'_>) -> PairCondition {
        self.condition(planner, false)
    }

    fn condition(&self, planner: &mut Planner<'_>, success: bool) -> PairCondition {
        let plain = |condition: GoExpression| PairCondition {
            initializer: None,
            condition,
        };
        match &self.source {
            BoundSource::Pair(pair) if success => planner.pair_success_condition(pair),
            BoundSource::Pair(pair) => planner.pair_failure_condition(pair),
            BoundSource::Nullable {
                value,
                nil_guard,
                initializer_call,
            } => {
                let tested = GoExpression::name(value.clone());
                let test = if success {
                    nil_guard.non_nil(tested)
                } else {
                    nil_guard.is_nil(tested)
                };
                PairCondition {
                    initializer: initializer_call.as_ref().map(|call| Definition {
                        names: vec![value.clone()],
                        value: call.clone(),
                    }),
                    condition: test,
                }
            }
            BoundSource::Index { index, .. } if success => plain(index.in_bounds.clone()),
            BoundSource::Index { index, .. } => plain(index.out_of_bounds.clone()),
            BoundSource::Found { flag, .. } if success => plain(GoExpression::name(flag.clone())),
            BoundSource::Found { flag, .. } => {
                plain(GoExpression::unary("!", GoExpression::name(flag.clone())))
            }
        }
    }
}

impl OptionFusePlan<'_> {
    pub(crate) fn bind(self, planner: &mut Planner<'_>, slot: CommaOkValueSlot) -> BoundOption {
        match self {
            Self::CommaOk { subject, source } => {
                let mut pair = planner.bind_comma_ok_pair(subject, source, slot);
                BoundOption {
                    statements: mem::take(&mut pair.statements),
                    source: BoundSource::Pair(pair),
                }
            }
            Self::Nullable { subject, nil_guard } => {
                let (mut statements, call) = planner
                    .lower_call(subject, None, ExpressionContext::value())
                    .into_parts();
                let (value, opens_if) = match slot {
                    CommaOkValueSlot::Named(name) => (name, false),
                    CommaOkValueSlot::Arm(name) => (name, true),
                    CommaOkValueSlot::Temp | CommaOkValueSlot::Discarded => {
                        (planner.fresh_pair_value(), false)
                    }
                    CommaOkValueSlot::Unused => (planner.fresh_pair_value(), true),
                };
                let initializer_call = if opens_if {
                    Some(call)
                } else {
                    statements.push(define(value.clone(), call));
                    None
                };
                BoundOption {
                    statements,
                    source: BoundSource::Nullable {
                        value,
                        nil_guard,
                        initializer_call,
                    },
                }
            }
            Self::Index { call } => {
                let (statements, index) = planner.lower_bounds_checked_index(&call);
                let target = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    _ => None,
                };
                BoundOption {
                    statements,
                    source: BoundSource::Index { index, target },
                }
            }
            Self::Found { subject, call } => {
                let value = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    CommaOkValueSlot::Arm(name) => Some(planner.declared_arm_value_name(&name)),
                    CommaOkValueSlot::Temp => Some(planner.fresh_pair_value()),
                    CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => None,
                };
                let flag = planner.fresh_var(Some("found"));
                planner.declare(&flag);
                let statements = planner.lower_find_loop(
                    subject,
                    &call,
                    FoundSink {
                        value: value.as_deref(),
                        flag: &flag,
                    },
                );
                BoundOption {
                    statements,
                    source: BoundSource::Found { value, flag },
                }
            }
        }
    }
}

impl ResultFusePlan<'_> {
    pub(super) fn has_nil_guard(&self) -> bool {
        self.nil_guard.is_some()
    }

    pub(super) fn shape(&self) -> &CallableReturnAbi {
        &self.shape
    }

    pub(crate) fn carries_payload(&self) -> bool {
        matches!(self.shape, CallableReturnAbi::Result { .. })
    }

    pub(crate) fn bind(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&str>,
    ) -> LoweredPair {
        self.bind_wrapped(planner, slot, error_name, false).0
    }

    fn bind_wrapped(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&str>,
        read_error: bool,
    ) -> (LoweredPair, Vec<WrapMessage>) {
        let carries_value = self.carries_payload();
        let (setup, call) = planner
            .lower_call(self.subject, None, ExpressionContext::value())
            .into_parts();
        let (message_setup, messages) = planner.prepare_wrap_messages(&self.wraps, read_error);
        // Bind before any eager message setup.
        let slot = if message_setup.is_empty() {
            slot
        } else {
            match slot {
                CommaOkValueSlot::Arm(name) => {
                    CommaOkValueSlot::Named(planner.declared_arm_value_name(&name))
                }
                CommaOkValueSlot::Unused => CommaOkValueSlot::Discarded,
                slot => slot,
            }
        };
        let mut pair = planner.bind_pair(
            setup,
            call,
            slot,
            PairKind::Error {
                carries_value,
                nil_guard: self.nil_guard,
            },
            error_name,
        );
        pair.statements.extend(message_setup);
        (pair, messages)
    }
}

#[derive(Clone, Copy)]
pub(super) enum ArmBinding<'a> {
    Alias {
        name: &'a str,
        go_name: &'a str,
    },
    Copy {
        name: &'a str,
        value: &'a GoExpression,
    },
}

impl<'a> ArmBinding<'a> {
    pub(super) fn alias(name: Option<&'a str>, go_name: Option<&'a str>) -> Option<Self> {
        name.zip(go_name)
            .map(|(name, go_name)| Self::Alias { name, go_name })
    }

    pub(super) fn copy(name: Option<&'a str>, value: Option<&'a GoExpression>) -> Option<Self> {
        name.zip(value)
            .map(|(name, value)| Self::Copy { name, value })
    }
}

fn unit_value() -> GoExpression {
    GoExpression::empty_composite("struct{}".to_string())
}

struct ResultArm<'a> {
    arm: &'a MatchArm,
    is_catch_all: bool,
}

#[derive(Clone, Copy)]
enum PartialVariant {
    Ok,
    Both,
    Err,
}

struct SelectivePartialArms<'a> {
    variant: PartialVariant,
    selected: &'a MatchArm,
    fallback: &'a MatchArm,
    value_binding: Option<&'a str>,
    error_binding: Option<&'a str>,
}

/// How to render the subject declaration line, based on body usage.
enum SubjectDeclaration {
    /// Identifier path: emit `_ = <var>` when unused, else nothing.
    PlainDiscard {
        var: String,
    },
    /// Composite path: `<var> := <expression>` if used, `_ = <expression>` if not.
    Deferred {
        var: String,
        expression: GoExpression,
    },
    None,
}

impl Planner<'_> {
    pub(crate) fn lower_match_to_block(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> LoweredBlock {
        let mut statements: Vec<LoweredStatement> = Vec::new();

        if subject.get_type().is_never() {
            statements.push(self.lower_statement(subject));
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.lower_fused_lowered_match(subject, arms, place, None) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.lower_fused_partial_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.lower_fused_selective_partial_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.lower_fused_option_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(elementwise) = self.lower_tuple_subject_match(subject, arms, place) {
            statements.extend(elementwise);
            return LoweredBlock { statements };
        }

        let subject_ty = subject.get_type();
        let (subject_var, declaration) =
            self.lower_match_subject_var(&mut statements, subject, arms);

        let block = self.lower_match_tree(arms, MatchSubject::Var(subject_var), subject_ty, place);
        let used = GoUses::of(&block.statements);

        match declaration {
            SubjectDeclaration::PlainDiscard { var } => {
                if !used.contains(&var) {
                    statements.push(discard(GoExpression::name(var)));
                }
            }
            SubjectDeclaration::Deferred { var, expression } => {
                if used.contains(&var) {
                    statements.push(define(var, expression));
                } else {
                    statements.push(discard(expression));
                }
            }
            SubjectDeclaration::None => {}
        }
        statements.extend(block.statements);

        LoweredBlock { statements }
    }

    /// `match (a, b)` reads the operands where they sit, building no tuple.
    fn lower_tuple_subject_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let Expression::Tuple { elements, .. } = subject.unwrap_parens() else {
            return None;
        };
        if elements.len() > TUPLE_FIELDS.len() || arms.iter().any(MatchArm::has_guard) {
            return None;
        }
        let subject_ty = subject.get_type();
        let mut tested = vec![false; elements.len()];
        for arm in arms {
            let info = decision_tree::collect_pattern_info(self, &arm.pattern, &subject_ty);
            if info.root_assertion.is_some()
                || !info.bindings.is_empty()
                || info.requires_materialized_subject
            {
                return None;
            }
            let arm_tested = decision_tree::tested_tuple_elements(&info.checks, elements.len())?;
            for (element, arm_element) in tested.iter_mut().zip(arm_tested) {
                *element |= arm_element;
            }
        }

        let mut statements: Vec<LoweredStatement> = Vec::new();
        let mut stages: Vec<ValuePlan> = elements
            .iter()
            .map(|element| self.lower_composite_value(element, ExpressionContext::value()))
            .collect();
        for ((stage, tested), element) in stages.iter_mut().zip(&tested).zip(elements) {
            let rereadable = is_inert_value(element, &stage.expression)
                || stage.evaluation.stability.is_fixed()
                || self.plan_rests_in_stable_name(stage);
            if *tested && !rereadable {
                self.pin_staged(stage, "arg");
            }
        }
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "arg");
        statements.extend(sequenced.setup);

        let mut roots = Vec::with_capacity(elements.len());
        for ((value, tested), element) in sequenced.values.iter().zip(&tested).zip(elements) {
            // An untested element still runs, but nothing may name it.
            if !tested && !is_inert_value(element, value) {
                statements.push(discard(value.clone()));
            }
            roots.push(value.clone());
        }

        let block = self.lower_match_tree(arms, MatchSubject::Elements(roots), subject_ty, place);
        statements.extend(block.statements);
        Some(statements)
    }

    fn lower_match_tree(
        &mut self,
        arms: &[MatchArm],
        subject_var: MatchSubject,
        subject_ty: Type,
        place: &PlacePlan,
    ) -> LoweredBlock {
        let tree_emitter = TreePlanner::new(self, arms, subject_var, subject_ty);
        tree_emitter.lower(place)
    }

    /// Recognize a Lisette `Result` function or a Go `(T, error)` one.
    pub(crate) fn result_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<ResultFusePlan<'a>> {
        let lowered = self.lowered_call(subject)?;
        if !lowered.is_result() {
            return None;
        }
        let nil_guard = match lowered.origin {
            CallableOrigin::GoInterop => {
                if lowered.has_tuple_payload(self) || lowered.payload_bridge.is_some() {
                    return None;
                }
                lowered.nil_guard
            }
            _ => None,
        };
        Some(ResultFusePlan {
            subject: lowered.call,
            shape: lowered.shape,
            nil_guard,
            wraps: lowered.wraps,
        })
    }

    /// Go rejects invalid constant indexes even behind a bounds check.
    fn index_fuses(&self, receiver: &Expression, index: &Expression) -> bool {
        match index.unwrap_parens() {
            Expression::Literal {
                literal: Literal::Integer { value, .. },
                ..
            } => match self.facts.strip_and_peel(&receiver.get_type()) {
                Type::Array { length, .. } => *value < length,
                _ => true,
            },
            other => !self.is_go_constant_expression(other),
        }
    }

    pub(crate) fn option_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<OptionFusePlan<'a>> {
        if let Some(source) = self.comma_ok_source(subject) {
            return Some(OptionFusePlan::CommaOk { subject, source });
        }
        if let Some(call) = self.native_method_call(subject) {
            let indexes = matches!(
                NativeGoType::from_kind(call.kind),
                NativeGoType::Slice | NativeGoType::Array
            ) && call.method == "get"
                && call.arguments.len() == 1
                && call.spread.is_none()
                && self.index_fuses(call.receiver, &call.arguments[0]);
            if indexes {
                return Some(OptionFusePlan::Index { call });
            }
            if self.find_loop_fuses(subject, &call) {
                return Some(OptionFusePlan::Found { subject, call });
            }
        }

        let lowered = self.lowered_call(subject)?;
        if !matches!(
            lowered.shape,
            CallableReturnAbi::Option(OptionReturnAbi::Nullable)
        ) || lowered.is_bridged()
        {
            return None;
        }
        let nil_guard = lowered.nil_guard?;
        Some(OptionFusePlan::Nullable { subject, nil_guard })
    }

    /// Bind `let x = match ...` straight into `x`.
    pub(crate) fn lower_fused_result_match_into(
        &mut self,
        value: &Expression,
        go_name: &str,
    ) -> Option<Vec<LoweredStatement>> {
        let Expression::Match { subject, arms, .. } = value.unwrap_parens() else {
            return None;
        };
        self.lower_fused_lowered_match(subject, arms, &PlacePlan::Statement, Some(go_name))
    }

    /// Bind `let x = match <lowered Option source> { Some(v) => v, None => diverge }`
    /// straight into `x` and test the physical Go result.
    pub(crate) fn lower_fused_option_match_into(
        &mut self,
        value: &Expression,
        go_name: &str,
    ) -> Option<Vec<LoweredStatement>> {
        let Expression::Match { subject, arms, .. } = value.unwrap_parens() else {
            return None;
        };
        let fuse = self.option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        let payload = arms.some_binding?;
        if !arms.none_body.get_type().is_never()
            || arm_body_is_identifier(arms.some_body) != Some(payload)
            || self.facts.peel_alias(&subject.get_type()).ok_type() != value.get_type()
        {
            return None;
        }

        self.declare(go_name);
        let bound = fuse.bind(self, CommaOkValueSlot::Named(go_name.to_string()));
        let none_condition = bound.none_condition(self);
        let fail_body = self.lower_block_as_body(arms.none_body);
        let late_binding = bound.late_binding();
        let mut statements = bound.statements;
        statements.push(LoweredStatement::If(pair_if(
            none_condition,
            fail_body,
            ElseArm::None,
        )));
        statements.extend(late_binding);
        Some(statements)
    }

    /// The payload name an arm binds, or `None` when the body never reads it.
    fn arm_payload_name<'a>(&self, arm: &'a MatchArm) -> Option<&'a str> {
        let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
            return None;
        };
        let [field] = fields.as_slice() else {
            return None;
        };
        if self.facts.is_unused_binding(field) {
            return None;
        }
        simple_payload_binding(arm).filter(|name| *name != "_")
    }

    /// Test a fallible call with one `if`, building no `Result`. A
    /// `destination` makes the call write into that name, leaving only the
    /// failure arm.
    fn lower_fused_lowered_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
        destination: Option<&str>,
    ) -> Option<Vec<LoweredStatement>> {
        let fuse = self.result_fuse_plan(subject)?;
        let (ok, err) = classify_result_arms(arms)?;

        // Err always carries a payload; Ok may not under BareError.
        let ok_name = if ok.is_catch_all {
            None
        } else {
            if simple_payload_binding(ok.arm).is_none()
                && !ok_arm_payload_is_omitted(ok.arm, fuse.shape())
            {
                return None;
            }
            self.arm_payload_name(ok.arm)
        };
        let err_name = if err.is_catch_all {
            None
        } else {
            simple_payload_binding(err.arm)?;
            self.arm_payload_name(err.arm)
        };

        if let Some(name) = destination {
            // Safe only if the failure arm always leaves and the success arm
            // returns the payload unchanged.
            let payload = simple_payload_binding(ok.arm).filter(|name| *name != "_")?;
            if !fuse.carries_payload()
                || ok.is_catch_all
                || err.is_catch_all
                || !err.arm.expression.get_type().is_never()
                || arm_body_is_identifier(&ok.arm.expression) != Some(payload)
            {
                return None;
            }
            self.declare(name);
        } else if matches!(place, PlacePlan::Statement)
            && ok_name.is_none()
            && err_name.is_none()
            && arm_body_is_noop(&ok.arm.expression)
            && arm_body_is_noop(&err.arm.expression)
        {
            // Nothing reads the outcome, so keep the call and drop the test.
            let (mut statements, call) = self
                .lower_call(subject, None, ExpressionContext::value())
                .into_parts();
            statements.push(expression_statement(call));
            return Some(statements);
        }

        let has_nil_guard = fuse.has_nil_guard();
        let carries_payload = fuse.carries_payload();
        let slot = match (destination, ok_name) {
            (Some(name), _) => CommaOkValueSlot::Named(name.to_string()),
            (None, Some(name)) => CommaOkValueSlot::Arm(self.arm_value_name(name)),
            (None, None) => CommaOkValueSlot::Unused,
        };
        let (mut bound, wraps) = fuse.bind_wrapped(self, slot, err_name, err_name.is_some());
        let error = || GoExpression::name(bound.status().to_string());

        let unit = unit_value();
        let then_body = destination.is_none().then(|| {
            // A call returning only `error` has no value, so `Ok(x)` takes unit.
            let ok_binding = if carries_payload {
                ArmBinding::alias(ok_name, bound.value.as_deref())
            } else {
                ArmBinding::copy(ok_name, Some(&unit))
            };
            let (body, uses) = self.lower_fused_arm(&[ok_binding], &ok.arm.expression, place);
            (body, uses.first().copied().unwrap_or(false))
        });
        let arm_place = if destination.is_some() {
            &PlacePlan::Statement
        } else {
            place
        };
        let err_binding = ArmBinding::alias(err_name, Some(bound.status()));
        let (mut else_body, err_used) =
            self.lower_fused_arm(&[err_binding], &err.arm.expression, arm_place);

        let err_read = err_used.first().copied().unwrap_or(false) || !wraps.is_empty();
        if has_nil_guard && err_read {
            else_body.statements.insert(
                0,
                LoweredStatement::If(IfPlan::plain(
                    is_nil(error()),
                    LoweredBlock {
                        statements: vec![assign(error(), unexpected_nil_error())],
                    },
                    ElseArm::None,
                )),
            );
        }
        if !wraps.is_empty() {
            let wrapped = self.wrap_error(&wraps, error());
            let prologue = vec![assign(error(), wrapped)];
            let after_nil_guard = usize::from(has_nil_guard);
            else_body
                .statements
                .splice(after_nil_guard..after_nil_guard, prologue);
        }
        if matches!(then_body, Some((_, false))) {
            bound.discard_value();
        }
        let then_body = then_body.map(|(body, _)| body);
        let ok_condition = self.pair_success_condition(&bound);
        let err_condition = self.pair_failure_condition(&bound);
        let mut statements = bound.statements;

        let Some(then_body) = then_body else {
            statements.push(LoweredStatement::If(pair_if(
                err_condition,
                else_body,
                ElseArm::None,
            )));
            return Some(statements);
        };

        let plan = if then_body.renders_empty() && !else_body.renders_empty() {
            pair_if(err_condition, else_body, ElseArm::None)
        } else {
            pair_if(
                ok_condition,
                then_body,
                ElseArm::from_body(else_body, false),
            )
        };
        statements.push(LoweredStatement::If(plan));
        Some(statements)
    }

    fn fusable_partial(&self, subject: &Expression, plan: &CallPlan<'_>) -> bool {
        let is_partial = matches!(plan.resolved.abi.result, CallableReturnAbi::Partial { .. });
        if !is_partial {
            return false;
        }
        if self
            .go_return_payload_bridge(&plan.resolved.abi, &subject.get_type())
            .is_some()
        {
            return false;
        }
        let ok_ty = self.facts.peel_alias(&subject.get_type()).ok_type();
        !matches!(self.facts.peel_alias(&ok_ty), Type::Tuple(_))
    }

    fn lower_fused_partial_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let plan = self.plan_call(subject)?;
        if !self.fusable_partial(subject, &plan) {
            return None;
        }
        let (ok_arm, both_arm, err_arm) = classify_partial_arms(arms)?;

        let ok_binding = simple_payload_binding(ok_arm)?;
        let err_binding = simple_payload_binding(err_arm)?;
        let (both_val_binding, both_err_binding) = partial_both_bindings(both_arm)?;
        let ok_name = (ok_binding != "_").then_some(ok_binding);
        let err_name = (err_binding != "_").then_some(err_binding);
        let both_val = (both_val_binding != "_").then_some(both_val_binding);
        let both_err = (both_err_binding != "_").then_some(both_err_binding);

        let ok_ty = self.facts.peel_alias(&subject.get_type()).ok_type();
        let nilable = self.partial_ok_is_nilable(&ok_ty);

        let (mut statements, call) = self
            .lower_call(subject, None, ExpressionContext::value())
            .into_parts();
        let val_var = match ok_name.or(both_val) {
            Some(name) => Some(self.arm_value_name(name)),
            None => nilable.then(|| self.fresh_var(Some("ret"))),
        };
        let mut err_var = self.pair_status(both_err.or(err_name), PairStatusKind::Error, true);
        if val_var.as_deref() == Some(err_var.as_str()) {
            err_var = self.fresh_var(Some(&err_var));
        }
        let err = || GoExpression::name(err_var.clone());

        let (ok_body, ok_uses) = self.lower_fused_arm(
            &[ArmBinding::alias(ok_name, val_var.as_deref())],
            &ok_arm.expression,
            place,
        );
        let (both_body, both_uses) = self.lower_fused_arm(
            &[
                ArmBinding::alias(both_val, val_var.as_deref()),
                ArmBinding::alias(both_err, Some(&err_var)),
            ],
            &both_arm.expression,
            place,
        );
        let val_used = nilable || ok_uses[0] || both_uses[0];
        let bound_value = match val_var.as_deref().filter(|_| val_used) {
            Some(v) => v.to_string(),
            None => "_".to_string(),
        };
        let initializer = Definition {
            names: vec![bound_value, err_var.clone()],
            value: call,
        };

        let nil_check = val_var
            .as_deref()
            .and_then(|v| self.partial_ok_nil_check(&ok_ty, GoExpression::name(v.to_string())));

        let else_arm = match nil_check {
            Some(check) => {
                let (err_body, _) = self.lower_fused_arm(
                    &[ArmBinding::alias(err_name, Some(&err_var))],
                    &err_arm.expression,
                    place,
                );
                ElseArm::ElseIf(Box::new(IfPlan::plain(
                    check,
                    err_body,
                    ElseArm::from_body(both_body, false),
                )))
            }
            None => ElseArm::from_body(both_body, false),
        };

        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: Some(initializer),
            condition: is_nil(err()),
            then_body: ok_body,
            else_arm,
        }));
        Some(statements)
    }

    /// Fuse a single explicit `Partial` variant plus a wildcard directly
    /// against the physical `(value, error)` result of the call.
    fn lower_fused_selective_partial_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let plan = self.plan_call(subject)?;
        // Selective fusion relies on the state mapping of a raw Go
        // `(value, error)` return. Do not infer that mapping for Lisette
        // functions merely because they share the same lowered shape.
        if !matches!(plan.resolved.origin, CallableOrigin::GoInterop)
            || !self.fusable_partial(subject, &plan)
        {
            return None;
        }
        let arms = classify_selective_partial_arms(arms)?;
        let ok_ty = self.facts.peel_alias(&subject.get_type()).ok_type();
        let nil_guard = self.partial_ok_nil_guard(&ok_ty);

        let value_binding = arms.value_binding.filter(|name| *name != "_");
        let error_binding = arms.error_binding.filter(|name| *name != "_");

        let (mut statements, call) = self
            .lower_call(subject, None, ExpressionContext::value())
            .into_parts();

        // A non-nilable value is always present, so its physical ABI has no
        // distinct Err state. The wildcard arm is therefore unconditional.
        if matches!(arms.variant, PartialVariant::Err) && nil_guard.is_none() {
            statements.push(expression_statement(call));
            let fallback = self
                .lower_fused_arm(&[], &arms.fallback.expression, place)
                .0;
            statements.extend(fallback.statements);
            return Some(statements);
        }

        let condition_needs_value = nil_guard.is_some()
            && matches!(arms.variant, PartialVariant::Both | PartialVariant::Err);
        let value = match value_binding {
            Some(name) => Some(self.arm_value_name(name)),
            None => condition_needs_value.then(|| self.fresh_var(Some("ret"))),
        };
        let mut error = self.pair_status(error_binding, PairStatusKind::Error, true);
        if value.as_deref() == Some(error.as_str()) {
            error = self.fresh_var(Some(&error));
        }
        let (selected, binding_uses) = self.lower_fused_arm(
            &[
                ArmBinding::alias(value_binding, value.as_deref()),
                ArmBinding::alias(error_binding, Some(&error)),
            ],
            &arms.selected.expression,
            place,
        );
        let fallback = self
            .lower_fused_arm(&[], &arms.fallback.expression, place)
            .0;

        if selected.renders_empty() && fallback.renders_empty() {
            statements.push(expression_statement(call));
            return Some(statements);
        }

        let value_is_used = binding_uses.first().copied().unwrap_or(false);
        let value_slot = (condition_needs_value || value_is_used)
            .then(|| value.as_deref().expect("value use allocates a result slot"));
        let bound_value = match value_slot {
            Some(value) => value.to_string(),
            None => "_".to_string(),
        };
        let err = || GoExpression::name(error.clone());
        let guarded_value =
            || GoExpression::name(value.clone().expect("nil guard captures the value"));
        let condition = match arms.variant {
            PartialVariant::Ok => is_nil(err()),
            PartialVariant::Both => match nil_guard {
                Some(guard) => {
                    GoExpression::binary(non_nil(err()), "&&", guard.non_nil(guarded_value()))
                }
                None => non_nil(err()),
            },
            PartialVariant::Err => {
                let guard = nil_guard.expect("non-nilable Err returned above");
                GoExpression::binary(non_nil(err()), "&&", guard.is_nil(guarded_value()))
            }
        };
        let selected_diverges = selected.ends_with_diverge();
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: Some(Definition {
                names: vec![bound_value, error.clone()],
                value: call,
            }),
            condition,
            then_body: selected,
            else_arm: ElseArm::from_body(fallback, selected_diverges),
        }));
        Some(statements)
    }

    /// Fuse the wrap+match into a direct physical-ABI test for simple
    /// `Some`/`None` arms.
    fn lower_fused_option_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let fuse = self.option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        Some(self.lower_fused_option_arms(fuse, arms, place))
    }

    pub(crate) fn lower_fused_option_arms(
        &mut self,
        fuse: OptionFusePlan<'_>,
        arms: OptionArms<'_>,
        place: &PlacePlan,
    ) -> Vec<LoweredStatement> {
        let slot = match arms.some_binding {
            Some(name) => CommaOkValueSlot::Arm(self.arm_value_name(name)),
            None => CommaOkValueSlot::Unused,
        };
        let mut bound = fuse.bind(self, slot);

        let element = bound.value();
        let some_binding = if bound.binds_value() {
            ArmBinding::alias(arms.some_binding, bound.value_name())
        } else {
            ArmBinding::copy(arms.some_binding, element.as_ref())
        };
        let (then_body, some_uses) = self.lower_fused_arm(&[some_binding], arms.some_body, place);
        let (else_body, _) = self.lower_fused_arm(&[], arms.none_body, place);
        if !some_uses.first().copied().unwrap_or(false) {
            bound.discard_value();
        }

        let invert = then_body.renders_empty() && !else_body.renders_empty();
        let condition = if invert {
            bound.none_condition(self)
        } else {
            bound.some_condition(self)
        };
        let plan = if invert {
            pair_if(condition, else_body, ElseArm::None)
        } else {
            pair_if(condition, then_body, ElseArm::from_body(else_body, false))
        };
        let mut statements = bound.statements;
        statements.push(LoweredStatement::If(plan));
        statements
    }

    pub(super) fn lower_fused_arm(
        &mut self,
        bindings: &[Option<ArmBinding<'_>>],
        body: &Expression,
        place: &PlacePlan,
    ) -> (LoweredBlock, Vec<bool>) {
        self.with_binding_frame(|this| {
            let bound: Vec<Option<(String, Option<GoExpression>)>> = bindings
                .iter()
                .map(|binding| {
                    binding.map(|binding| match binding {
                        ArmBinding::Alias { name, go_name } => {
                            (this.scope.bind(name, go_name), None)
                        }
                        ArmBinding::Copy { name, value } => {
                            let go_name = this.scope.bind(name, name);
                            this.declare(&go_name);
                            (go_name, Some(value.clone()))
                        }
                    })
                })
                .collect();
            let body_block = this.lower_block_to_place(body, place);
            let used = GoUses::of(&body_block.statements);
            let mut statements = Vec::new();
            let binding_uses = bound
                .iter()
                .map(|binding| {
                    binding
                        .as_ref()
                        .is_some_and(|(go_name, _)| used.contains(go_name))
                })
                .collect::<Vec<_>>();
            for (binding, is_used) in bound.iter().zip(&binding_uses) {
                let Some((go_name, Some(value))) = binding else {
                    continue;
                };
                if !is_used {
                    continue;
                }
                statements.push(define(go_name.clone(), value.clone()));
            }
            statements.extend(body_block.statements);
            (LoweredBlock { statements }, binding_uses)
        })
    }

    fn lower_match_subject_var(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> (GoExpression, SubjectDeclaration) {
        let any_guard = arms.iter().any(|arm| arm.has_guard());
        if let Expression::Identifier { value, .. } = subject
            && !any_guard
        {
            let name = value.to_string();
            let has_collision = arms
                .iter()
                .any(|arm| pattern_binds_name(&arm.pattern, &name));
            if self.can_reuse_subject_identifier(&name, has_collision) {
                let var = self.reference_go_name(&name);
                return (
                    GoExpression::name(var.clone()),
                    SubjectDeclaration::PlainDiscard { var },
                );
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            let staged = self.plan_operand(subject, ExpressionContext::value());
            let (subject_setup, value) = staged.into_parts();
            setup.extend(subject_setup);
            return (value, SubjectDeclaration::None);
        }
        let staged = self.lower_composite_value(subject, ExpressionContext::value());
        let rests_in_stable_name = self.plan_rests_in_stable_name(&staged);
        let (subject_setup, value) = staged.into_parts();
        setup.extend(subject_setup);
        let reads_in_place = matches!(value.node(), GoExpressionNode::Identifier(_))
            || self.field_path_reads_in_place(subject, &value, |root| {
                arms.iter()
                    .any(|arm| pattern_binds_name(&arm.pattern, root))
            });
        if !any_guard && reads_in_place {
            return (value, SubjectDeclaration::None);
        }
        if any_guard
            && rests_in_stable_name
            && let GoExpressionNode::Identifier(var) = value.node()
        {
            let var = var.clone();
            return (value, SubjectDeclaration::PlainDiscard { var });
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let declaration = SubjectDeclaration::Deferred {
            var: var.clone(),
            expression: value,
        };
        (GoExpression::name(var), declaration)
    }
}

fn pair_if(test: PairCondition, then_body: LoweredBlock, else_arm: ElseArm) -> IfPlan {
    IfPlan {
        condition_setup: Vec::new(),
        initializer: test.initializer,
        condition: test.condition,
        then_body,
        else_arm,
    }
}

/// Whether reading the value again costs nothing and skipping it loses nothing.
fn is_inert_value(element: &Expression, value: &GoExpression) -> bool {
    value.is_literal() && (!value.is_composite_literal() || element.get_type().is_unit())
}

/// `[Ok(..), Err(..)]` in either order, plus the shape `if let` desugars to.
/// A wildcard fits only the second arm, since the checker rejects any arm
/// after a catch-all.
fn classify_result_arms(arms: &[MatchArm]) -> Option<(ResultArm<'_>, ResultArm<'_>)> {
    if arms.len() != 2 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    let kind = |arm: &MatchArm| -> Option<&'static str> {
        if matches!(arm.pattern, Pattern::WildCard { .. }) {
            return Some("_");
        }
        let Pattern::EnumVariant {
            identifier, rest, ..
        } = &arm.pattern
        else {
            return None;
        };
        if *rest {
            return None;
        }
        match identifier.as_str() {
            "Ok" | "Result.Ok" => Some("Ok"),
            "Err" | "Result.Err" => Some("Err"),
            _ => None,
        }
    };
    let explicit = |arm| ResultArm {
        arm,
        is_catch_all: false,
    };
    let catch_all = |arm| ResultArm {
        arm,
        is_catch_all: true,
    };
    match (kind(&arms[0])?, kind(&arms[1])?) {
        ("Ok", "Err") => Some((explicit(&arms[0]), explicit(&arms[1]))),
        ("Err", "Ok") => Some((explicit(&arms[1]), explicit(&arms[0]))),
        ("Ok", "_") => Some((explicit(&arms[0]), catch_all(&arms[1]))),
        ("Err", "_") => Some((catch_all(&arms[1]), explicit(&arms[0]))),
        _ => None,
    }
}

fn classify_partial_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm, &MatchArm)> {
    if arms.len() != 3 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    let kind = |arm: &MatchArm| -> Option<&'static str> {
        let Pattern::EnumVariant {
            identifier, rest, ..
        } = &arm.pattern
        else {
            return None;
        };
        if *rest {
            return None;
        }
        match identifier.as_str() {
            "Ok" | "Partial.Ok" => Some("Ok"),
            "Both" | "Partial.Both" => Some("Both"),
            "Err" | "Partial.Err" => Some("Err"),
            _ => None,
        }
    };
    let (mut ok, mut both, mut err) = (None, None, None);
    for arm in arms {
        let slot = match kind(arm)? {
            "Ok" => &mut ok,
            "Both" => &mut both,
            _ => &mut err,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(arm);
    }
    Some((ok?, both?, err?))
}

fn classify_selective_partial_arms(arms: &[MatchArm]) -> Option<SelectivePartialArms<'_>> {
    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return None;
    }
    if !matches!(arms[1].pattern, Pattern::WildCard { .. }) {
        return None;
    }
    let (selected, fallback) = (&arms[0], &arms[1]);
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = &selected.pattern
    else {
        return None;
    };
    if *rest {
        return None;
    }
    let variant = match identifier.as_str() {
        "Ok" | "Partial.Ok" if fields.len() == 1 => PartialVariant::Ok,
        "Both" | "Partial.Both" if fields.len() == 2 => PartialVariant::Both,
        "Err" | "Partial.Err" if fields.len() == 1 => PartialVariant::Err,
        _ => return None,
    };
    let (value_binding, error_binding) = match variant {
        PartialVariant::Ok => (Some(simple_payload_binding(selected)?), None),
        PartialVariant::Both => {
            let (value, error) = partial_both_bindings(selected)?;
            (Some(value), Some(error))
        }
        PartialVariant::Err => (None, Some(simple_payload_binding(selected)?)),
    };
    Some(SelectivePartialArms {
        variant,
        selected,
        fallback,
        value_binding,
        error_binding,
    })
}

pub(crate) struct OptionArms<'a> {
    /// `None` when the Some arm binds no payload.
    pub(crate) some_binding: Option<&'a str>,
    pub(crate) some_body: &'a Expression,
    pub(crate) none_body: &'a Expression,
}

enum OptionArmKind<'a> {
    Some(Option<&'a str>),
    None,
    WildCard,
}

fn option_arm_kind(arm: &MatchArm) -> Option<OptionArmKind<'_>> {
    use OptionArmKind as ArmKind;
    if matches!(arm.pattern, Pattern::WildCard { .. }) {
        return Some(ArmKind::WildCard);
    }
    if let Some(field) = some_pattern_field(&arm.pattern) {
        let binding = field_binding(field)?;
        return Some(ArmKind::Some((binding != "_").then_some(binding)));
    }
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = &arm.pattern
    else {
        return None;
    };
    (!*rest && fields.is_empty() && matches!(identifier.as_str(), "None" | "Option.None"))
        .then_some(ArmKind::None)
}

/// `[Some(<binding>), None]` in either order, plus the if-let wildcard desugars.
fn classify_option_arms(arms: &[MatchArm]) -> Option<OptionArms<'_>> {
    use OptionArmKind as ArmKind;
    if arms.len() != 2 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    match (option_arm_kind(&arms[0])?, option_arm_kind(&arms[1])?) {
        (ArmKind::Some(binding), ArmKind::None | ArmKind::WildCard) => Some(OptionArms {
            some_binding: binding,
            some_body: &arms[0].expression,
            none_body: &arms[1].expression,
        }),
        (ArmKind::None, ArmKind::Some(binding)) => Some(OptionArms {
            some_binding: binding,
            some_body: &arms[1].expression,
            none_body: &arms[0].expression,
        }),
        (ArmKind::None, ArmKind::WildCard) => Some(OptionArms {
            some_binding: None,
            some_body: &arms[1].expression,
            none_body: &arms[0].expression,
        }),
        _ => None,
    }
}

fn arm_body_is_noop(body: &Expression) -> bool {
    match body.unwrap_parens() {
        Expression::Unit { .. } => true,
        Expression::Block { items, .. } => items.is_empty(),
        _ => false,
    }
}

/// The name an arm returns when its body is nothing but that name.
fn arm_body_is_identifier(body: &Expression) -> Option<&str> {
    let body = match body.unwrap_parens() {
        Expression::Block { items, .. } => match items.as_slice() {
            [single] => single.unwrap_parens(),
            _ => return None,
        },
        other => other,
    };
    match body {
        Expression::Identifier { value, .. } => Some(value.as_str()),
        _ => None,
    }
}

/// The single payload field of an `Ok(<identifier|_>)` pattern.
pub(super) fn ok_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, &["Ok", "Result.Ok"])
}

/// The single payload field of a `Some(<identifier|_>)` pattern.
pub(super) fn some_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, &["Some", "Option.Some"])
}

fn simple_variant_field<'a>(pattern: &'a Pattern, variants: &[&str]) -> Option<&'a Pattern> {
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = pattern
    else {
        return None;
    };
    if *rest || !variants.contains(&identifier.as_str()) {
        return None;
    }
    let [field] = fields.as_slice() else {
        return None;
    };
    matches!(field, Pattern::Identifier { .. } | Pattern::WildCard { .. }).then_some(field)
}

pub(super) fn field_binding(pattern: &Pattern) -> Option<&str> {
    match pattern {
        Pattern::Identifier { identifier, .. } => Some(identifier.as_str()),
        Pattern::WildCard { .. } => Some("_"),
        _ => None,
    }
}

/// `Some(name)` for `Variant(identifier)`, `Some("_")` for `Variant(_)`, `None`
/// for empty/unit/complex payloads.
fn simple_payload_binding(arm: &MatchArm) -> Option<&str> {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return None;
    };
    if fields.len() != 1 {
        return None;
    }
    field_binding(&fields[0])
}

fn partial_both_bindings(arm: &MatchArm) -> Option<(&str, &str)> {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return None;
    };
    if fields.len() != 2 {
        return None;
    }
    Some((field_binding(&fields[0])?, field_binding(&fields[1])?))
}

/// True when an Ok arm has no value to bind: empty `Ok` or `Ok(())`,
/// only meaningful under `BareError`.
fn ok_arm_payload_is_omitted(arm: &MatchArm, shape: &CallableReturnAbi) -> bool {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return false;
    };
    match shape {
        CallableReturnAbi::BareError => {
            fields.is_empty() || matches!(fields.as_slice(), [Pattern::Unit { .. }])
        }
        CallableReturnAbi::Tagged
        | CallableReturnAbi::Direct
        | CallableReturnAbi::Result { .. }
        | CallableReturnAbi::Partial { .. }
        | CallableReturnAbi::Option(_)
        | CallableReturnAbi::Tuple { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::classify_selective_partial_arms;
    use syntax::ast::{ConstructorPatternResolution, Expression, MatchArm, Pattern, Span};
    use syntax::types::Type;

    fn variant(identifier: &str, fields: Vec<Pattern>) -> Pattern {
        Pattern::EnumVariant {
            identifier: identifier.into(),
            fields,
            rest: false,
            resolution: ConstructorPatternResolution::Unresolved,
            ty: Type::uninferred(),
            span: Span::dummy(),
        }
    }

    fn arm(pattern: Pattern) -> MatchArm {
        MatchArm {
            pattern,
            guard: None,
            expression: Box::new(Expression::Unit {
                ty: Type::unit(),
                span: Span::dummy(),
            }),
        }
    }

    #[test]
    fn selective_partial_classifier_rejects_nested_payload_pattern() {
        let arms = [
            arm(variant(
                "Partial.Ok",
                vec![variant(
                    "Some",
                    vec![Pattern::Identifier {
                        identifier: "n".into(),
                        span: Span::dummy(),
                    }],
                )],
            )),
            arm(Pattern::WildCard {
                span: Span::dummy(),
            }),
        ];

        assert!(classify_selective_partial_arms(&arms).is_none());
    }
}
