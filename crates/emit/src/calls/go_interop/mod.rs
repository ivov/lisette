mod nullable;
mod wrappers;

use crate::Emitter;
use crate::names::go_name;
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Debug, Clone)]
pub(crate) enum GoCallStrategy {
    /// (T1, T2, ...) → Tuple struct. Arity ≥ 2, no error/bool suffix.
    Tuple { arity: usize },
    /// (T, error) → Result<T, Error>.
    Result,
    /// (T, bool) → Option<T>. Comma-ok pattern (non-nullable or `#[go(comma_ok)]`).
    CommaOk,
    /// Single return of pointer/interface type → Option<Ref<T>> via nil check.
    NullableReturn,
    /// (T, error) → Partial<T, error>. Non-exclusive returns where both value and error
    /// may be simultaneously meaningful (e.g. io.Reader.Read).
    Partial,
}

impl GoCallStrategy {
    pub(crate) fn is_multi_return(&self) -> bool {
        !matches!(self, GoCallStrategy::NullableReturn)
    }
}

impl Emitter<'_> {
    pub(crate) fn classify_go_return_type(
        &self,
        return_ty: &Type,
        go_hints: &[String],
    ) -> Option<GoCallStrategy> {
        if return_ty.is_partial() {
            return Some(GoCallStrategy::Partial);
        }

        if return_ty.is_result() {
            return Some(GoCallStrategy::Result);
        }

        if return_ty.is_option() {
            if !self.is_nullable_option(return_ty) {
                return Some(GoCallStrategy::CommaOk);
            }
            if go_hints.iter().any(|s| s == "comma_ok") {
                return Some(GoCallStrategy::CommaOk);
            }
            return Some(GoCallStrategy::NullableReturn);
        }

        if let Some(arity) = return_ty.tuple_arity()
            && arity >= 2
        {
            return Some(GoCallStrategy::Tuple { arity });
        }

        None
    }

    pub(crate) fn resolve_go_call_strategy(
        &self,
        expression: &Expression,
    ) -> Option<GoCallStrategy> {
        let Expression::Call {
            expression: callee,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let inner = callee.unwrap_parens();

        if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = inner
            && Self::is_go_receiver(receiver_expression)
        {
            if let Some(qualified_name) = self.go_qualified_name(receiver_expression, member)
                && let Some(strategy) = self.module.go_call_strategies.get(&qualified_name)
            {
                return Some(strategy.clone());
            }
            let go_hints = self
                .go_qualified_name(receiver_expression, member)
                .and_then(|name| self.ctx.definitions.get(name.as_str()))
                .map(|d| d.go_hints())
                .unwrap_or_default();
            return self.classify_go_return_type(ty, go_hints);
        }

        None
    }

    pub(crate) fn emit_go_wrapped_call(
        &mut self,
        output: &mut String,
        expression: &Expression,
        strategy: &GoCallStrategy,
        result_ty: &Type,
    ) -> String {
        match strategy {
            GoCallStrategy::Tuple { arity } => {
                self.emit_go_tuple_call_wrapped(output, expression, *arity)
            }
            GoCallStrategy::Result => {
                self.emit_go_result_call_wrapped(output, expression, result_ty)
            }
            GoCallStrategy::CommaOk => {
                self.emit_go_option_call_wrapped(output, expression, result_ty)
            }
            GoCallStrategy::NullableReturn => {
                self.emit_go_single_return_option_wrapped(output, expression, result_ty)
            }
            GoCallStrategy::Partial => {
                self.emit_go_partial_call_wrapped(output, expression, result_ty)
            }
        }
    }

    fn has_go_hint(&self, receiver_expression: &Expression, member: &str, hint: &str) -> bool {
        let Some(qualified_name) = self.go_qualified_name(receiver_expression, member) else {
            return false;
        };

        self.ctx
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.go_hints().iter().any(|s| s == hint))
            .unwrap_or(false)
    }

    pub(crate) fn has_go_array_return(
        &self,
        receiver_expression: &Expression,
        member: &str,
    ) -> bool {
        self.has_go_hint(receiver_expression, member, "array_return")
    }

    fn go_qualified_name(&self, receiver_expression: &Expression, member: &str) -> Option<String> {
        let ty = receiver_expression.get_type();

        if let Some(module_path) = ty.as_import_namespace() {
            return Some(format!("{}.{}", module_path, member));
        }

        if let Type::Nominal { id, .. } = ty.strip_refs()
            && go_name::is_go_import(&id)
        {
            return Some(format!("{}.{}", id, member));
        }

        None
    }

    pub(crate) fn is_go_receiver(expression: &Expression) -> bool {
        let ty = expression.get_type();

        if let Some(module_id) = ty.as_import_namespace()
            && module_id.starts_with(go_name::GO_IMPORT_PREFIX)
        {
            return true;
        }

        // Check for Go object pattern: type is go:* (possibly wrapped in Ref<>)
        if let Type::Nominal { id, .. } = ty.strip_refs()
            && go_name::is_go_import(&id)
        {
            return true;
        }

        false
    }

    pub(crate) fn emit_go_call_discarded(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
    ) -> Option<String> {
        let Expression::Call {
            expression: callee, ..
        } = call_expression
        else {
            return None;
        };

        let has_strategy = self.resolve_go_call_strategy(call_expression).is_some();

        let has_array_return = if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = callee.unwrap_parens()
            && Self::is_go_receiver(receiver_expression)
        {
            self.has_go_array_return(receiver_expression, member)
        } else {
            false
        };

        if !has_strategy && !has_array_return {
            return None;
        }

        self.skip_array_return_wrap = has_array_return;
        let call_str = self.emit_call(output, call_expression, None);
        self.skip_array_return_wrap = false;

        Some(call_str)
    }

    pub(super) fn create_temp_vars(&mut self, hint: &str, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let v = self.fresh_var(Some(hint));
                self.declare(&v);
                v
            })
            .collect()
    }

    pub(super) fn build_tuple_literal(&mut self, vars: &[String], _tuple_ty: &Type) -> String {
        self.flags.needs_stdlib = true;
        format!("lisette.MakeTuple{}({})", vars.len(), vars.join(", "))
    }

    pub(super) fn emit_tuple_from_vars(
        &mut self,
        output: &mut String,
        vars: &[String],
        tuple_ty: &Type,
    ) -> String {
        let constructor = self.build_tuple_literal(vars, tuple_ty);
        let tuple_var = self.fresh_var(Some("tup"));
        self.declare(&tuple_var);
        write_line!(output, "{} := {}", tuple_var, constructor);
        tuple_var
    }
}
