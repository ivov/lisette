use crate::Emitter;
use crate::bindings::BindingValue;
use crate::expressions::context::ExpressionContext;
use crate::patterns::bindings::pattern_binds_name;
use crate::patterns::tree_emitter::TreeEmitter;
use crate::placement::BodyPlace;
use crate::types::abi::AbiShape;
use crate::utils::{DiscardGuard, ValueTempDiscard};
use crate::write_line;
use syntax::ast::{Expression, MatchArm, Pattern};

impl Emitter<'_> {
    pub(crate) fn emit_match(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
        place: &BodyPlace,
    ) {
        if subject.get_type().is_never() {
            self.emit_statement(output, subject);
            return;
        }
        if self.try_emit_fused_lowered_match(output, subject, arms, place) {
            return;
        }
        let subject_ty = subject.get_type();
        let (subject_var, value_guard) = self.emit_match_subject_var(output, subject, arms);
        let plain_guard = if value_guard.is_some() || matches!(subject, Expression::Literal { .. })
        {
            None
        } else {
            Some(DiscardGuard::new(output, &subject_var))
        };
        let tree_emitter = TreeEmitter::new(self, arms, subject_var, subject_ty);
        tree_emitter.emit(output, place);
        if let Some(g) = value_guard {
            g.finish(output);
        }
        if let Some(g) = plain_guard {
            g.finish(output);
        }
    }

    /// Fuse the lift+match into one `if err == nil { ... } else { ... }`
    /// when the scrutinee is a lowered call with simple `Ok`/`Err` arms.
    fn try_emit_fused_lowered_match(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
        place: &BodyPlace,
    ) -> bool {
        let Expression::Call {
            expression: callee, ..
        } = subject
        else {
            return false;
        };
        let Some(shape) = self.classify_callee_abi(callee) else {
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
        let call_str = self.emit_call(output, subject, None, ExpressionContext::value());
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
        );
        output.push_str("} else {\n");
        self.emit_fused_arm(
            output,
            err_name.map(|n| (n, err_var.as_str())),
            &err_arm.expression,
            place,
        );
        output.push_str("}\n");
        true
    }

    fn emit_fused_arm(
        &mut self,
        output: &mut String,
        binding: Option<(&str, &str)>,
        body: &Expression,
        place: &BodyPlace,
    ) {
        self.scope.push_binding_frame();
        let guard = binding.map(|(name, value)| self.bind_fused_with_guard(output, name, value));
        self.emit_body_to_place(output, body, place);
        if let Some(g) = guard {
            g.finish(output);
        }
        self.scope.pop_binding_frame();
    }

    fn bind_fused_with_guard(
        &mut self,
        output: &mut String,
        name: &str,
        value: &str,
    ) -> DiscardGuard {
        let go_name = self.scope.bind(name, name);
        self.declare(&go_name);
        write_line!(output, "{} := {}", go_name, value);
        DiscardGuard::new(output, &go_name)
    }

    fn emit_match_subject_var(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> (String, Option<ValueTempDiscard>) {
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
                return (self.scope.resolve_or_escape_go_name(&name), None);
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            return (
                self.emit_operand(output, subject, ExpressionContext::value()),
                None,
            );
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let subject_expression =
            self.emit_composite_value(output, subject, ExpressionContext::value());
        let decl_start = output.len();
        write_line!(output, "{} := {}", var, subject_expression);
        let guard = ValueTempDiscard::new(output, decl_start, &var, &subject_expression);
        (var, Some(guard))
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

/// `Some(name)` for `Variant(ident)`, `Some("_")` for `Variant(_)`, `None`
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
