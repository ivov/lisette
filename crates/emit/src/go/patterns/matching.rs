use crate::Emitter;
use crate::go::names::go_name;
use crate::go::patterns::tree_emitter::TreeEmitter;
use crate::go::utils::DiscardGuard;
use crate::go::write_line;
use syntax::ast::{Expression, MatchArm};
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
