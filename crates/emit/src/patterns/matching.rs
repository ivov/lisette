use crate::Planner;
use crate::abi::callable::CallableReturnAbi;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::calls::go_interop::NilGuard;
use crate::context::expression::ExpressionContext;
use crate::names::go_name::is_plain_identifier;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::tree_emitter::{MatchSubject, TreePlanner};
use crate::plan::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::plan::values::{CaptureBoundary, GoExpression, ValuePlan};
use syntax::ast::{ConstructorPatternResolution, Expression, MatchArm, Pattern};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

pub(super) struct ResultFusePlan<'a> {
    subject: &'a Expression,
    shape: CallableReturnAbi,
    nil_guard: Option<NilGuard>,
}

pub(super) struct BoundResultCall {
    pub(super) setup: Vec<LoweredStatement>,
    pub(super) value: Option<String>,
    pub(super) error: String,
    pub(super) ok_condition: String,
    pub(super) err_condition: String,
}

impl ResultFusePlan<'_> {
    pub(super) fn has_nil_guard(&self) -> bool {
        self.nil_guard.is_some()
    }

    pub(super) fn shape(&self) -> &CallableReturnAbi {
        &self.shape
    }

    pub(super) fn carries_payload(&self) -> bool {
        matches!(self.shape, CallableReturnAbi::Result { .. })
    }

    pub(super) fn bind(self, planner: &mut Planner<'_>, slot: CommaOkValueSlot) -> BoundResultCall {
        let temp = |planner: &mut Planner<'_>| {
            let var = planner.fresh_var(Some("ret"));
            planner.declare(&var);
            var
        };
        let value = self
            .carries_payload()
            .then(|| match slot {
                CommaOkValueSlot::Named(name) => Some(name),
                CommaOkValueSlot::Temp => Some(temp(planner)),
                CommaOkValueSlot::Unused => self.nil_guard.map(|_| temp(planner)),
            })
            .flatten();
        let error = planner.fresh_var(Some("err"));
        planner.declare(&error);

        let (mut setup, call_str) = planner
            .lower_call(self.subject, None, ExpressionContext::value())
            .into_parts();
        setup.push(LoweredStatement::RawGo(match &value {
            Some(var) => format!("{}, {} := {}\n", var, error, call_str),
            None => match self.shape {
                CallableReturnAbi::Result { .. } => format!("_, {} := {}\n", error, call_str),
                CallableReturnAbi::BareError => format!("{} := {}\n", error, call_str),
                CallableReturnAbi::Tagged
                | CallableReturnAbi::Direct
                | CallableReturnAbi::Partial { .. }
                | CallableReturnAbi::Option(_)
                | CallableReturnAbi::Tuple { .. } => unreachable!("rejected by recognition"),
            },
        }));

        let (ok_condition, err_condition) = match self.nil_guard {
            Some(guard) => {
                if guard.is_interface() {
                    planner.require_stdlib();
                }
                let var = value.as_deref().expect("nil guard requires the value var");
                (
                    format!("{} == nil && {}", error, guard.non_nil(var)),
                    format!("{} != nil || {}", error, guard.is_nil(var)),
                )
            }
            None => (format!("{} == nil", error), format!("{} != nil", error)),
        };

        BoundResultCall {
            setup,
            value,
            error,
            ok_condition,
            err_condition,
        }
    }
}

const UNIT_VALUE: &str = "struct{}{}";

struct ResultArm<'a> {
    arm: &'a MatchArm,
    is_catch_all: bool,
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
        expression: String,
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

        let (block, used_set) = self.capture_go_uses(|this| {
            this.lower_match_tree(
                arms,
                MatchSubject::Var(subject_var.clone()),
                subject_ty,
                place,
            )
        });
        let used = used_set.contains(&subject_var);

        match declaration {
            SubjectDeclaration::PlainDiscard { var } => {
                if !used {
                    statements.push(LoweredStatement::RawGo(format!("_ = {}\n", var)));
                }
            }
            SubjectDeclaration::Deferred { var, expression } => {
                if used {
                    statements.push(LoweredStatement::RawGo(format!(
                        "{} := {}\n",
                        var, expression
                    )));
                } else {
                    statements.push(LoweredStatement::RawGo(format!("_ = {}\n", expression)));
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
        let mut tested = vec![false; elements.len()];
        for arm in arms {
            let arm_tested = arm_tested_elements(self, &arm.pattern, elements.len())?;
            for (element, arm_element) in tested.iter_mut().zip(arm_tested) {
                *element |= arm_element;
            }
        }

        let subject_ty = subject.get_type();
        let mut statements: Vec<LoweredStatement> = Vec::new();
        let mut stages: Vec<ValuePlan> = elements
            .iter()
            .map(|element| self.stage_composite(element, ExpressionContext::value()))
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

        let mut names = Vec::with_capacity(elements.len());
        for ((value, tested), element) in sequenced.values.iter().zip(&tested).zip(elements) {
            let rendered = value.rendered();
            // An untested element still runs, but nothing may name it.
            if !tested && !is_inert_value(element, value) {
                statements.push(LoweredStatement::RawGo(format!("_ = {}\n", rendered)));
            }
            names.push(rendered);
        }

        let block = self.lower_match_tree(arms, MatchSubject::Elements(names), subject_ty, place);
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
    pub(super) fn result_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<ResultFusePlan<'a>> {
        let plan = self.plan_call(subject)?;
        let shape = plan.resolved.abi.result.clone();
        let nil_guard = match &plan.resolved.origin {
            CallableOrigin::GoInterop
                if matches!(
                    shape,
                    CallableReturnAbi::BareError | CallableReturnAbi::Result { .. }
                ) =>
            {
                let ok_ty = self.facts.peel_alias(&subject.get_type()).ok_type();
                if matches!(self.facts.peel_alias(&ok_ty), Type::Tuple(_)) {
                    return None;
                }
                if self
                    .go_return_payload_bridge(&plan.resolved.abi, &subject.get_type())
                    .is_some()
                {
                    return None;
                }
                self.result_nil_guard(&ok_ty)
            }
            CallableOrigin::GoInterop => return None,
            _ => None,
        };
        matches!(
            shape,
            CallableReturnAbi::BareError | CallableReturnAbi::Result { .. }
        )
        .then_some(ResultFusePlan {
            subject,
            shape,
            nil_guard,
        })
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
            let (mut statements, call_str) = self
                .lower_call(subject, None, ExpressionContext::value())
                .into_parts();
            statements.push(LoweredStatement::RawGo(format!("{call_str}\n")));
            return Some(statements);
        }

        let has_nil_guard = fuse.has_nil_guard();
        let carries_payload = fuse.carries_payload();
        let slot = match destination {
            Some(name) => CommaOkValueSlot::Named(name.to_string()),
            None if ok_name.is_some() => CommaOkValueSlot::Temp,
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot);
        let mut statements = bound.setup;

        let then_body = destination.is_none().then(|| {
            // A call returning only `error` has no value, so `Ok(x)` takes unit.
            let ok_value = if carries_payload {
                bound.value.as_deref()
            } else {
                Some(UNIT_VALUE)
            };
            self.lower_fused_arm(&[ok_name.zip(ok_value)], &ok.arm.expression, place)
                .0
        });
        let arm_place = if destination.is_some() {
            &PlacePlan::Statement
        } else {
            place
        };
        let (mut else_body, err_used) = self.lower_fused_arm(
            &[err_name.map(|n| (n, bound.error.as_str()))],
            &err.arm.expression,
            arm_place,
        );

        if has_nil_guard && err_used {
            self.require_errors();
            let error = &bound.error;
            else_body.statements.insert(
                0,
                LoweredStatement::RawGo(format!(
                    "if {error} == nil {{\n{error} = errors.New(\"unexpected nil\")\n}}\n"
                )),
            );
        }

        let Some(then_body) = then_body else {
            statements.push(LoweredStatement::If(IfPlan {
                condition_setup: Vec::new(),
                condition: bound.err_condition,
                then_body: else_body,
                else_arm: ElseArm::None,
            }));
            return Some(statements);
        };

        let plan = if then_body.renders_empty() && !else_body.renders_empty() {
            IfPlan {
                condition_setup: Vec::new(),
                condition: bound.err_condition,
                then_body: else_body,
                else_arm: ElseArm::None,
            }
        } else {
            IfPlan {
                condition_setup: Vec::new(),
                condition: bound.ok_condition,
                then_body,
                else_arm: ElseArm::from_body(else_body, false),
            }
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
        let val_used = ok_name.is_some() || both_val.is_some() || nilable;

        let val_var = val_used.then(|| {
            let v = self.fresh_var(Some("ret"));
            self.declare(&v);
            v
        });
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);

        let (mut statements, call_str) = self
            .lower_call(subject, None, ExpressionContext::value())
            .into_parts();
        let bind_line = match &val_var {
            Some(v) => format!("{}, {} := {}\n", v, err_var, call_str),
            None => format!("_, {} := {}\n", err_var, call_str),
        };
        statements.push(LoweredStatement::RawGo(bind_line));

        let (ok_body, _) = self.lower_fused_arm(
            &[ok_name.zip(val_var.as_deref())],
            &ok_arm.expression,
            place,
        );
        let both_body = self
            .lower_fused_arm(
                &[
                    both_val.zip(val_var.as_deref()),
                    both_err.zip(Some(err_var.as_str())),
                ],
                &both_arm.expression,
                place,
            )
            .0;

        let nil_check = val_var
            .as_deref()
            .and_then(|v| self.partial_ok_nil_check(&ok_ty, v));

        let else_arm = match nil_check {
            Some(check) => {
                let (err_body, _) = self.lower_fused_arm(
                    &[err_name.zip(Some(err_var.as_str()))],
                    &err_arm.expression,
                    place,
                );
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup: Vec::new(),
                    condition: check,
                    then_body: err_body,
                    else_arm: ElseArm::from_body(both_body, false),
                }))
            }
            None => ElseArm::from_body(both_body, false),
        };

        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition: format!("{} == nil", err_var),
            then_body: ok_body,
            else_arm,
        }));
        Some(statements)
    }

    /// Fuse the wrap+match into a direct pair test for simple `Some`/`None` arms.
    fn lower_fused_option_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let source = self.comma_ok_source(subject)?;
        let arms = classify_option_arms(arms)?;

        let slot = if arms.some_binding.is_some() {
            CommaOkValueSlot::Temp
        } else {
            CommaOkValueSlot::Unused
        };
        let pair = self.bind_comma_ok_pair(subject, source, slot);

        let (then_body, _) = self.lower_fused_arm(
            &[arms.some_binding.zip(pair.value.as_deref())],
            arms.some_body,
            place,
        );
        let (else_body, _) = self.lower_fused_arm(&[], arms.none_body, place);

        let plan = if then_body.renders_empty() && !else_body.renders_empty() {
            IfPlan {
                condition_setup: Vec::new(),
                condition: self.comma_ok_none_condition(&pair),
                then_body: else_body,
                else_arm: ElseArm::None,
            }
        } else {
            IfPlan {
                condition_setup: Vec::new(),
                condition: self.comma_ok_some_condition(&pair),
                then_body,
                else_arm: ElseArm::from_body(else_body, false),
            }
        };
        let mut statements = pair.statements;
        statements.push(LoweredStatement::If(plan));
        Some(statements)
    }

    pub(super) fn lower_fused_arm(
        &mut self,
        bindings: &[Option<(&str, &str)>],
        body: &Expression,
        place: &PlacePlan,
    ) -> (LoweredBlock, bool) {
        self.with_binding_frame(|this| {
            let bound: Vec<Option<(String, String)>> = bindings
                .iter()
                .map(|binding| {
                    binding.map(|(name, value)| {
                        let go_name = this.scope.bind(name, name);
                        this.declare(&go_name);
                        (go_name, value.to_string())
                    })
                })
                .collect();
            let (body_block, used) =
                this.capture_go_uses(|this| this.lower_block_to_place(body, place));
            let mut statements = Vec::new();
            let mut any_referenced = false;
            for (go_name, value) in bound.iter().flatten() {
                if !used.contains(go_name) {
                    continue;
                }
                statements.push(LoweredStatement::TempBind {
                    name: go_name.clone(),
                    value: value.clone(),
                });
                any_referenced = true;
            }
            statements.extend(body_block.statements);
            (LoweredBlock { statements }, any_referenced)
        })
    }

    fn lower_match_subject_var(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> (String, SubjectDeclaration) {
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
                return (var.clone(), SubjectDeclaration::PlainDiscard { var });
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            let staged = self.stage_operand(subject, ExpressionContext::value());
            let (subject_setup, value) = staged.into_parts();
            setup.extend(subject_setup);
            return (value, SubjectDeclaration::None);
        }
        let staged = self.stage_composite(subject, ExpressionContext::value());
        let rests_in_stable_name = self.plan_rests_in_stable_name(&staged);
        let (subject_setup, value) = staged.into_parts();
        setup.extend(subject_setup);
        if !any_guard && is_plain_identifier(&value) {
            return (value, SubjectDeclaration::None);
        }
        if any_guard && rests_in_stable_name {
            return (
                value.clone(),
                SubjectDeclaration::PlainDiscard { var: value },
            );
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let declaration = SubjectDeclaration::Deferred {
            var: var.clone(),
            expression: value,
        };
        (var, declaration)
    }
}

/// Whether reading the value again costs nothing and skipping it loses nothing.
fn is_inert_value(element: &Expression, value: &GoExpression) -> bool {
    match value {
        GoExpression::Literal(_) => true,
        GoExpression::CompositeLiteral { .. } => element.get_type().is_unit(),
        _ => false,
    }
}

/// Whether an element pattern tests its element, or `None` for a shape whose
/// checks this lowering does not predict.
fn element_pattern_tests(planner: &Planner, pattern: &Pattern) -> Option<bool> {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Unit { .. } => Some(false),
        Pattern::Literal { .. } => Some(true),
        // A tuple struct or a newtype carries no tag, so its checks come from
        // fields this grammar does not read.
        Pattern::EnumVariant {
            resolution,
            ty,
            fields,
            ..
        } => {
            for field in fields {
                element_pattern_tests(planner, field)?;
            }
            match resolution {
                ConstructorPatternResolution::Const { .. }
                | ConstructorPatternResolution::ConstValue { .. } => Some(true),
                ConstructorPatternResolution::EnumVariant { .. }
                    if !planner.is_tuple_struct_type(ty) =>
                {
                    Some(true)
                }
                _ => None,
            }
        }
        Pattern::Tuple { elements, .. } => elements.iter().try_fold(false, |tests, element| {
            element_pattern_tests(planner, element).map(|element| tests || element)
        }),
        // A catchall alternative leaves the whole pattern untested.
        Pattern::Or { patterns, .. } => patterns.iter().try_fold(true, |tests, alternative| {
            element_pattern_tests(planner, alternative).map(|alternative| tests && alternative)
        }),
        _ => None,
    }
}

/// Which elements an arm tests, or `None` when its shape leaves that open.
/// Or-pattern alternatives have to agree, since each becomes its own arm.
fn arm_tested_elements(planner: &Planner, pattern: &Pattern, arity: usize) -> Option<Vec<bool>> {
    match pattern {
        Pattern::WildCard { .. } => Some(vec![false; arity]),
        Pattern::Tuple { elements, .. } if elements.len() == arity => elements
            .iter()
            .map(|element| element_pattern_tests(planner, element))
            .collect(),
        Pattern::Or { patterns, .. } => {
            let mut alternatives = Vec::with_capacity(patterns.len());
            for alternative in patterns {
                alternatives.push(arm_tested_elements(planner, alternative, arity)?);
            }
            let first = alternatives.first()?.clone();
            alternatives
                .iter()
                .all(|alternative| *alternative == first)
                .then_some(first)
        }
        _ => None,
    }
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

struct OptionArms<'a> {
    /// `None` when the Some arm binds no payload.
    some_binding: Option<&'a str>,
    some_body: &'a Expression,
    none_body: &'a Expression,
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
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = pattern
    else {
        return None;
    };
    if *rest || !matches!(identifier.as_str(), "Ok" | "Result.Ok") {
        return None;
    }
    let [field] = fields.as_slice() else {
        return None;
    };
    matches!(field, Pattern::Identifier { .. } | Pattern::WildCard { .. }).then_some(field)
}

/// The single payload field of a `Some(<identifier|_>)` pattern.
pub(super) fn some_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = pattern
    else {
        return None;
    };
    if *rest || !matches!(identifier.as_str(), "Some" | "Option.Some") {
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
