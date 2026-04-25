use crate::Emitter;
use crate::names::go_name;
use crate::patterns::tree_emitter::TreeEmitter;
use crate::types::abi::AbiShape;
use crate::utils::DiscardGuard;
use crate::write_line;
use syntax::ast::{Expression, MatchArm, Pattern};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_match(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
        ty: &Type,
    ) {
        if subject.get_type().is_never() {
            self.emit_statement(output, subject);
            return;
        }
        if self.try_emit_fused_lowered_match(output, subject, arms) {
            return;
        }
        let subject_ty = subject.get_type();
        let subject_var = self.emit_match_subject_var(output, subject, arms);
        let guard = if matches!(subject, Expression::Literal { .. }) {
            None
        } else {
            Some(DiscardGuard::new(output, &subject_var))
        };
        let tree_emitter = TreeEmitter::new(self, arms, ty, subject_var, subject_ty);
        tree_emitter.emit(output);
        if let Some(guard) = guard {
            guard.finish(output);
        }
    }

    /// Fuse the lift+match into one `if err == nil { ... } else { ... }`
    /// when the scrutinee is a lowered call with simple `Ok`/`Err` arms.
    fn try_emit_fused_lowered_match(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
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

        let val_var = match shape {
            AbiShape::ResultTuple => {
                let v = self.fresh_var(Some("ret"));
                self.declare(&v);
                Some(v)
            }
            AbiShape::BareError => None,
            AbiShape::PartialTuple
            | AbiShape::CommaOk
            | AbiShape::NullableReturn
            | AbiShape::Tuple { .. } => unreachable!("rejected above"),
        };
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);
        let call_str = self.emit_call(output, subject, None);
        match &val_var {
            Some(v) => write_line!(output, "{}, {} := {}", v, err_var, call_str),
            None => write_line!(output, "{} := {}", err_var, call_str),
        }

        write_line!(output, "if {} == nil {{", err_var);
        self.scope.bindings.save();
        if let (Some(name), Some(val)) = (ok_binding, &val_var)
            && name != "_"
        {
            self.bind_fused(output, name, val);
        }
        self.emit_in_position(output, &ok_arm.expression);
        self.scope.bindings.restore();
        output.push_str("} else {\n");
        self.scope.bindings.save();
        if let Some(name) = err_binding
            && name != "_"
        {
            self.bind_fused(output, name, &err_var);
        }
        self.emit_in_position(output, &err_arm.expression);
        self.scope.bindings.restore();
        output.push_str("}\n");
        true
    }

    fn bind_fused(&mut self, output: &mut String, name: &str, value: &str) {
        let go_name = self.scope.bindings.add(name, name);
        self.declare(&go_name);
        write_line!(output, "{} := {}", go_name, value);
    }

    fn emit_match_subject_var(
        &mut self,
        output: &mut String,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> String {
        if let Expression::Identifier { value, .. } = subject {
            let name = value.to_string();
            let has_collision = arms
                .iter()
                .any(|arm| Emitter::pattern_binds_name(&arm.pattern, &name));
            if !has_collision && !name.contains('.') {
                return self
                    .scope
                    .bindings
                    .get(&name)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| go_name::escape_reserved(&name).into_owned());
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            return self.emit_operand(output, subject);
        }
        let var = self.fresh_var(Some("subject"));
        self.declare(&var);
        let subject_expression = self.emit_composite_value(output, subject);
        write_line!(output, "{} := {}", var, subject_expression);
        var
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
