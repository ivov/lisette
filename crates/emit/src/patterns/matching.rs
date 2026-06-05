use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::AbiShape;
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::tree_emitter::TreePlanner;
use crate::plan::bodies::{LoweredBlock, LoweredStatement, PlacePlan};
use crate::plan::calls::CallReturnShape;
use crate::state::bindings::BindingValue;
use crate::write_line;
use syntax::ast::{Expression, MatchArm, Pattern};

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
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let mut statements: Vec<LoweredStatement> = Vec::new();

        if subject.get_type().is_never() {
            let mut buffer = String::new();
            self.emit_statement(&mut buffer, subject, fx);
            statements.push(LoweredStatement::RawGo(buffer));
            return LoweredBlock { statements };
        }

        let mut fusion_buffer = String::new();
        if self.try_emit_fused_lowered_match(&mut fusion_buffer, subject, arms, place, fx) {
            statements.push(LoweredStatement::RawGo(fusion_buffer));
            return LoweredBlock { statements };
        }

        let subject_ty = subject.get_type();
        let mut setup_buffer = String::new();
        let (subject_var, declaration) =
            self.emit_match_subject_var(&mut setup_buffer, subject, arms, fx);
        if !setup_buffer.is_empty() {
            statements.push(LoweredStatement::RawGo(setup_buffer));
        }

        let block = self.lower_match_tree(arms, subject_var.clone(), subject_ty, place, fx);
        let used = block.references_var(&subject_var);

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
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let tree_emitter = TreePlanner::new(self, arms, subject_var, subject_ty, fx);
        tree_emitter.lower(place)
    }

    /// Fuse the lift+match into one `if err == nil { ... } else { ... }`
    /// when the scrutinee is a lowered call with simple `Ok`/`Err` arms.
    fn try_emit_fused_lowered_match(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> bool {
        let Some(plan) = self.plan_call(subject) else {
            return false;
        };
        let CallReturnShape::Lowered(shape) = plan.return_shape else {
            return false;
        };
        // Match-fusion only handles `Result`'s binary `Ok`/`Err` arms;
        // Partial (3-way) and Option (Some/None) fall through to lift-then-match.
        if !matches!(shape, AbiShape::ResultTuple | AbiShape::BareError) {
            return false;
        }
        let Some((ok_arm, err_arm)) = classify_result_arms(arms) else {
            return false;
        };

        // Err always carries a payload; Ok may not under BareError.
        let ok_binding = simple_payload_binding(ok_arm);
        let err_binding = simple_payload_binding(err_arm);
        if err_binding.is_none() {
            return false;
        }
        if ok_binding.is_none() && !ok_arm_payload_is_omitted(ok_arm, &shape) {
            return false;
        }
        let ok_name = ok_binding.filter(|n| *n != "_");
        let err_name = err_binding.filter(|n| *n != "_");

        let val_var = if matches!(shape, AbiShape::ResultTuple) && ok_name.is_some() {
            let v = self.fresh_var(Some("ret"));
            self.declare(&v);
            Some(v)
        } else {
            None
        };
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);
        let call_str = self.emit_call(output, subject, None, ExpressionContext::value(), fx);
        match &val_var {
            Some(v) => write_line!(output, "{}, {} := {}", v, err_var, call_str),
            None => match shape {
                AbiShape::ResultTuple => write_line!(output, "_, {} := {}", err_var, call_str),
                AbiShape::BareError => write_line!(output, "{} := {}", err_var, call_str),
                AbiShape::PartialTuple
                | AbiShape::CommaOk
                | AbiShape::NullableReturn
                | AbiShape::Tuple { .. } => unreachable!("rejected above"),
            },
        }

        write_line!(output, "if {} == nil {{", err_var);
        self.emit_fused_arm(
            output,
            ok_name.zip(val_var.as_deref()),
            &ok_arm.expression,
            place,
            fx,
        );
        output.push_str("} else {\n");
        self.emit_fused_arm(
            output,
            err_name.map(|n| (n, err_var.as_str())),
            &err_arm.expression,
            place,
            fx,
        );
        output.push_str("}\n");
        true
    }

    fn emit_fused_arm(
        &mut self,
        output: &mut String,
        binding: Option<(&str, &str)>,
        body: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) {
        self.scope.push_binding_frame();
        let bound = binding.map(|(name, value)| {
            let go_name = self.scope.bind(name, name);
            self.declare(&go_name);
            (go_name, value.to_string())
        });
        let body_block = self.lower_block_to_place(body, place, fx);
        if let Some((go_name, value)) = &bound {
            write_line!(output, "{} := {}", go_name, value);
            if !body_block.references_var(go_name) {
                write_line!(output, "_ = {}", go_name);
            }
        }
        Renderer.render_lowered_block(output, &body_block);
        self.scope.pop_binding_frame();
    }

    fn emit_match_subject_var(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
        fx: &mut EmitEffects,
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
            return (
                self.emit_operand(output, subject, ExpressionContext::value(), fx),
                SubjectDeclaration::None,
            );
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let expression = self.emit_composite_value(output, subject, ExpressionContext::value(), fx);
        let declaration = SubjectDeclaration::Deferred {
            var: var.clone(),
            expression,
        };
        (var, declaration)
    }
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

/// `Some(name)` for `Variant(identifier)`, `Some("_")` for `Variant(_)`, `None`
/// for empty/unit/complex payloads.
fn simple_payload_binding(arm: &MatchArm) -> Option<&str> {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return None;
    };
    if fields.len() != 1 {
        return None;
    }
    match &fields[0] {
        Pattern::Identifier { identifier, .. } => Some(identifier.as_str()),
        Pattern::WildCard { .. } => Some("_"),
        _ => None,
    }
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
