use crate::Emitter;
use crate::names::go_name;
use crate::utils::Staged;
use crate::write_line;
use syntax::ast::Expression;

impl Emitter<'_> {
    pub(crate) fn emit_or_capture(
        &mut self,
        output: &mut String,
        expression: &Expression,
        prefix: &str,
    ) -> String {
        let emit_directly = matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        );

        if emit_directly {
            return self.emit_operand(output, expression);
        }

        let temp_var = self.fresh_var(Some(prefix));
        self.declare(&temp_var);
        let expression_string = self.emit_operand(output, expression);
        write_line!(output, "{} := {}", temp_var, expression_string);
        temp_var
    }

    pub(crate) fn emit_force_capture(
        &mut self,
        output: &mut String,
        expression: &Expression,
        prefix: &str,
    ) -> String {
        if matches!(expression.unwrap_parens(), Expression::Literal { .. }) {
            return self.emit_operand(output, expression);
        }

        let temp_var = self.fresh_var(Some(prefix));
        self.declare(&temp_var);
        let expression_string = self.emit_composite_value(output, expression);
        write_line!(output, "{} := {}", temp_var, expression_string);
        temp_var
    }

    /// Emit an expression to a separate buffer, capturing setup and value.
    pub(crate) fn stage_operand(&mut self, expression: &Expression) -> Staged {
        let mut setup = String::new();
        let value = self.emit_operand(&mut setup, expression);
        Staged::new(setup, value)
    }

    /// Emit an expression as a composite value to a separate buffer.
    pub(crate) fn stage_composite(&mut self, expression: &Expression) -> Staged {
        let mut setup = String::new();
        let value = self.emit_composite_value(&mut setup, expression);
        Staged::new(setup, value)
    }

    /// Suppresses the Go-fn identity short-circuit when the formal param
    /// is function-typed (prelude generic callbacks reject multi-return).
    pub(crate) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        param_ty: Option<&syntax::types::Type>,
    ) -> Staged {
        let suppress = param_ty
            .is_some_and(|p| matches!(p.unwrap_forall(), syntax::types::Type::Function { .. }));
        let saved = std::mem::replace(&mut self.suppress_go_fn_short_circuit, suppress);
        let staged = self.stage_composite(expression);
        self.suppress_go_fn_short_circuit = saved;

        if suppress
            && !matches!(expression.unwrap_parens(), Expression::Lambda { .. })
            && !Self::is_tagged_shape_fn_value(expression)
            && self.classify_go_fn_value(expression).is_none()
            && let Some(param_ty) = param_ty
            && let syntax::types::Type::Function { return_type, .. } = param_ty.unwrap_forall()
            && self.classify_direct_emission(return_type).is_some()
        {
            let mut setup = staged.setup;
            let cb_var = self.fresh_var(Some("cb"));
            self.declare(&cb_var);
            write_line!(setup, "{} := {}", cb_var, staged.value);
            let tagged = self.lower_arg_to_tagged(&mut setup, &cb_var, param_ty);
            return Staged::new(setup, tagged);
        }

        staged
    }

    pub(crate) fn stage_native_method_args(
        &mut self,
        function: &Expression,
        args: &[Expression],
    ) -> Vec<Staged> {
        let fn_ty = function.get_type();
        let formal_params: &[syntax::types::Type] = match fn_ty.unwrap_forall() {
            syntax::types::Type::Function { params, .. } => params,
            _ => &[],
        };
        args.iter()
            .enumerate()
            .map(|(i, arg)| self.stage_prelude_arg(arg, formal_params.get(i)))
            .collect()
    }

    /// Like `sequence`, but also stages the spread as a sibling (so its
    /// setup participates in eval-order) and appends `...` to its value.
    pub(crate) fn sequence_with_spread(
        &mut self,
        output: &mut String,
        mut stages: Vec<Staged>,
        spread: Option<&Expression>,
        wrap_to_any: bool,
        prefix: &str,
    ) -> Vec<String> {
        let spread_idx = spread.map(|s| {
            stages.push(self.stage_operand(s));
            stages.len() - 1
        });
        let mut values = self.sequence(output, stages, prefix);
        if let Some(i) = spread_idx {
            if wrap_to_any {
                self.flags.needs_stdlib = true;
                values[i] = format!("{}.SliceToAny({})", go_name::GO_STDLIB_PKG, values[i]);
            }
            values[i].push_str("...");
        }
        values
    }

    /// Sequence N staged emissions preserving left-to-right eval order.
    ///
    /// When a later sibling produces setup statements (temp vars from if/match/block
    /// used as values), earlier siblings that contain calls are captured to temp vars
    /// to prevent the setup from running before the earlier call.
    pub(crate) fn sequence(
        &mut self,
        output: &mut String,
        stages: Vec<Staged>,
        prefix: &str,
    ) -> Vec<String> {
        // Fast path: when no element produces setup, just move the values out.
        if stages.iter().all(|s| s.setup.is_empty()) {
            return stages.into_iter().map(|s| s.value).collect();
        }

        let mut results = Vec::with_capacity(stages.len());
        for (i, s) in stages.iter().enumerate() {
            let later_has_setup = stages[i + 1..].iter().any(|s| !s.setup.is_empty());

            output.push_str(&s.setup);

            if later_has_setup && s.has_side_effects {
                let tmp = self.fresh_var(Some(prefix));
                self.declare(&tmp);
                write_line!(output, "{} := {}", tmp, s.value);
                results.push(tmp);
            } else {
                results.push(s.value.clone());
            }
        }
        results
    }
}
