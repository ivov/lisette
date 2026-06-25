use crate::GoCallStrategy;
use crate::Planner;
use crate::abi::AbiShape;
use crate::calls::go_interop::NilGuard;
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::tree_emitter::TreePlanner;
use crate::plan::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan};
use crate::plan::calls::{CallPlan, CallReturnShape, CalleePlan};
use crate::state::bindings::BindingValue;
use syntax::ast::{Expression, MatchArm, Pattern};
use syntax::types::Type;

struct FusedShape {
    shape: AbiShape,
    nil_guard: Option<NilGuard>,
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

        if let Some(fused) = self.lower_fused_lowered_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.lower_fused_partial_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        let subject_ty = subject.get_type();
        let (subject_var, declaration) =
            self.lower_match_subject_var(&mut statements, subject, arms);

        self.scope.enter_use_region();
        let block = self.lower_match_tree(arms, subject_var.clone(), subject_ty, place);
        let used_set = self.scope.exit_use_region();
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

    fn lower_match_tree(
        &mut self,
        arms: &[MatchArm],
        subject_var: String,
        subject_ty: syntax::types::Type,
        place: &PlacePlan,
    ) -> LoweredBlock {
        let tree_emitter = TreePlanner::new(self, arms, subject_var, subject_ty);
        tree_emitter.lower(place)
    }

    /// The shape a match subject fuses against: lowered Lisette `Result` callees
    /// and single-value Go `(T, error)` calls. `None` keeps the lift-then-match
    /// path (Partial, Option, comma-ok, flattened multi-returns).
    fn fusable_result_shape(&self, subject: &Expression, plan: &CallPlan) -> Option<FusedShape> {
        let (shape, nil_guard) = match (&plan.callee, &plan.return_shape) {
            (_, CallReturnShape::Lowered(shape)) => (shape.clone(), None),
            (CalleePlan::GoInterop(GoCallStrategy::Result), _) => {
                let ok_ty = self.facts.peel_alias(&subject.get_type()).ok_type();
                if matches!(self.facts.peel_alias(&ok_ty), Type::Tuple(_)) {
                    return None;
                }
                let shape = if ok_ty.is_unit() {
                    AbiShape::BareError
                } else {
                    AbiShape::ResultTuple
                };
                (shape, self.result_nil_guard(&ok_ty))
            }
            _ => return None,
        };
        matches!(shape, AbiShape::ResultTuple | AbiShape::BareError)
            .then_some(FusedShape { shape, nil_guard })
    }

    /// Fuse the lift+match into one `if err == nil { ... } else { ... }`
    /// when the scrutinee is a lowered call with simple `Ok`/`Err` arms.
    fn lower_fused_lowered_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<LoweredStatement>> {
        let plan = self.plan_call(subject)?;
        let FusedShape { shape, nil_guard } = self.fusable_result_shape(subject, &plan)?;
        let (ok_arm, err_arm) = classify_result_arms(arms)?;

        // Err always carries a payload; Ok may not under BareError.
        let ok_binding = simple_payload_binding(ok_arm);
        let err_binding = simple_payload_binding(err_arm);
        err_binding?;
        if ok_binding.is_none() && !ok_arm_payload_is_omitted(ok_arm, &shape) {
            return None;
        }
        let ok_name = ok_binding.filter(|n| *n != "_");
        let err_name = err_binding.filter(|n| *n != "_");

        let need_val =
            matches!(shape, AbiShape::ResultTuple) && (ok_name.is_some() || nil_guard.is_some());
        let val_var = need_val.then(|| {
            let v = self.fresh_var(Some("ret"));
            self.declare(&v);
            v
        });
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);

        let (mut statements, call_str) = self.lower_call(subject, None, ExpressionContext::value());
        let bind_line = match &val_var {
            Some(v) => format!("{}, {} := {}\n", v, err_var, call_str),
            None => match shape {
                AbiShape::ResultTuple => format!("_, {} := {}\n", err_var, call_str),
                AbiShape::BareError => format!("{} := {}\n", err_var, call_str),
                AbiShape::PartialTuple
                | AbiShape::CommaOk
                | AbiShape::NullableReturn
                | AbiShape::Tuple { .. } => unreachable!("rejected above"),
            },
        };
        statements.push(LoweredStatement::RawGo(bind_line));

        let (then_body, _) = self.lower_fused_arm(
            &[ok_name.zip(val_var.as_deref())],
            &ok_arm.expression,
            place,
        );
        let (mut else_body, err_used) = self.lower_fused_arm(
            &[err_name.map(|n| (n, err_var.as_str()))],
            &err_arm.expression,
            place,
        );

        let condition = match nil_guard {
            Some(guard) => {
                let val = val_var
                    .as_deref()
                    .expect("nil guard requires the value var");
                if guard.is_interface() {
                    self.require_stdlib();
                }
                if err_used {
                    self.require_errors();
                    else_body.statements.insert(
                        0,
                        LoweredStatement::RawGo(format!(
                            "if {err_var} == nil {{\n{err_var} = errors.New(\"unexpected nil\")\n}}\n"
                        )),
                    );
                }
                format!("{} == nil && {}", err_var, guard.non_nil(val))
            }
            None => format!("{} == nil", err_var),
        };

        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition,
            then_body,
            else_arm: ElseArm::Else {
                body: else_body,
                inline: false,
            },
        }));
        Some(statements)
    }

    fn fusable_partial(&self, subject: &Expression, plan: &CallPlan) -> bool {
        let is_partial = matches!(
            (&plan.callee, &plan.return_shape),
            (_, CallReturnShape::Lowered(AbiShape::PartialTuple))
                | (CalleePlan::GoInterop(GoCallStrategy::Partial), _)
        );
        if !is_partial {
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

        let (mut statements, call_str) = self.lower_call(subject, None, ExpressionContext::value());
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
                    else_arm: ElseArm::Else {
                        body: both_body,
                        inline: false,
                    },
                }))
            }
            None => ElseArm::Else {
                body: both_body,
                inline: false,
            },
        };

        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition: format!("{} == nil", err_var),
            then_body: ok_body,
            else_arm,
        }));
        Some(statements)
    }

    fn lower_fused_arm(
        &mut self,
        bindings: &[Option<(&str, &str)>],
        body: &Expression,
        place: &PlacePlan,
    ) -> (LoweredBlock, bool) {
        self.scope.push_binding_frame();
        let bound: Vec<Option<(String, String)>> = bindings
            .iter()
            .map(|binding| {
                binding.map(|(name, value)| {
                    let go_name = self.scope.bind(name, name);
                    self.declare(&go_name);
                    (go_name, value.to_string())
                })
            })
            .collect();
        self.scope.enter_use_region();
        let body_block = self.lower_block_to_place(body, place);
        let used = self.scope.exit_use_region();
        let mut statements = Vec::new();
        let mut any_referenced = false;
        for (go_name, value) in bound.iter().flatten() {
            statements.push(LoweredStatement::TempBind {
                name: go_name.clone(),
                value: value.clone(),
            });
            if used.contains(go_name) {
                any_referenced = true;
            } else {
                statements.push(LoweredStatement::RawGo(format!("_ = {}\n", go_name)));
            }
        }
        statements.extend(body_block.statements);
        self.scope.pop_binding_frame();
        (LoweredBlock { statements }, any_referenced)
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
            let bound_to_inline = matches!(
                self.scope.resolve_identifier_binding(&name),
                Some(BindingValue::InlineExpr(_))
            );
            if !has_collision && !name.contains('.') && !bound_to_inline {
                let var = self.scope.resolve_or_escape_go_name(&name);
                return (var.clone(), SubjectDeclaration::PlainDiscard { var });
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            let staged = self.stage_operand(subject, ExpressionContext::value());
            setup.extend(staged.setup);
            return (staged.value, SubjectDeclaration::None);
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let staged = self.stage_composite(subject, ExpressionContext::value());
        setup.extend(staged.setup);
        if !any_guard && is_plain_go_identifier(&staged.value) {
            return (staged.value, SubjectDeclaration::None);
        }
        let declaration = SubjectDeclaration::Deferred {
            var: var.clone(),
            expression: staged.value,
        };
        (var, declaration)
    }
}

fn is_plain_go_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Recognize `[Ok(<...>), Err(<...>)]` (in either order, no guards).
fn classify_result_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm)> {
    if arms.len() != 2 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    let kind = |arm: &MatchArm| -> Option<&str> {
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
    let a0 = kind(&arms[0])?;
    let a1 = kind(&arms[1])?;
    match (a0, a1) {
        ("Ok", "Err") => Some((&arms[0], &arms[1])),
        ("Err", "Ok") => Some((&arms[1], &arms[0])),
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

fn field_binding(pattern: &Pattern) -> Option<&str> {
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
fn ok_arm_payload_is_omitted(arm: &MatchArm, shape: &AbiShape) -> bool {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return false;
    };
    match shape {
        AbiShape::BareError => {
            fields.is_empty() || matches!(fields.as_slice(), [Pattern::Unit { .. }])
        }
        AbiShape::ResultTuple
        | AbiShape::PartialTuple
        | AbiShape::CommaOk
        | AbiShape::NullableReturn
        | AbiShape::Tuple { .. } => false,
    }
}
