use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::go::types::emitter::Position;
use crate::go::utils::{group_params, optimize_function_body, requires_temp_var};
use crate::go::write_line;
use syntax::ast::{Binding, Expression, Pattern, TypedPattern};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_function_body(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
    ) {
        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            std::slice::from_ref(body)
        };

        let Some((last, rest)) = items.split_last() else {
            return;
        };

        for item in rest {
            self.emit_statement(output, item);
        }

        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );

        let needs_return = should_return
            && !matches!(last, Expression::Return { .. })
            && !is_statement_only
            && !last.get_type().is_unit()
            && !last.get_type().is_never();

        if !needs_return {
            self.emit_statement(output, last);
            if should_return && last.get_type().is_never() && !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            if should_return
                && is_statement_only
                && self
                    .current_return_context
                    .as_ref()
                    .is_some_and(|ty| !ty.is_unit())
            {
                let return_ty = self.current_return_context.as_ref().unwrap();
                let zero = self.zero_value(return_ty);
                write_line!(output, "return {}", zero);
            }
            return;
        }

        if self.emit_wrapped_return(output, last) {
            return;
        }

        self.with_position(Position::Tail, |this| {
            if !requires_temp_var(last) {
                let expression = this.emit_value(output, last);
                let expression = this.adapt_return_to_context(last, expression);
                output.push_str(&this.wrap_value(&expression));
            } else {
                match last {
                    Expression::If { .. }
                    | Expression::Match { .. }
                    | Expression::Select { .. } => {
                        this.emit_branching_directly(output, last);
                    }
                    Expression::IfLet { .. } => {
                        unreachable!("IfLet should be desugared to Match before emit")
                    }
                    Expression::Block { .. }
                    | Expression::Loop { .. }
                    | Expression::Propagate { .. } => {
                        let expression = this.emit_operand(output, last);
                        output.push_str(&this.wrap_value(&expression));
                    }
                    _ => unreachable!("requires_temp_var returned true for unexpected expression"),
                }
            }
        });
    }

    pub(crate) fn emit_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
    ) -> String {
        let saved_declared = std::mem::take(&mut self.scope.declared);
        let saved_scope_depth = self.scope.scope_depth;
        self.scope.declared = vec![HashSet::default()];
        self.scope.scope_depth = 0;

        self.scope.bindings.save();

        let mut destructure_bindings: Vec<(String, &Pattern, Option<&TypedPattern>)> = vec![];

        let param_pairs: Vec<(String, String)> = params
            .iter()
            .map(|p| {
                let name = if let Pattern::Identifier { identifier, .. } = &p.pattern {
                    if let Some(go_name) = self.go_name_for_binding(&p.pattern) {
                        let go_id = self.scope.bindings.add(identifier, go_name);
                        self.declare(&go_id);
                        go_id
                    } else {
                        self.scope.bindings.add(identifier, "_");
                        "_".to_string()
                    }
                } else if matches!(&p.pattern, Pattern::WildCard { .. }) {
                    "_".to_string()
                } else {
                    let temp_name = self.fresh_var(Some("arg"));
                    self.declare(&temp_name);
                    destructure_bindings.push((
                        temp_name.clone(),
                        &p.pattern,
                        p.typed_pattern.as_ref(),
                    ));
                    temp_name
                };
                (name, self.go_type_as_string(&p.ty))
            })
            .collect();

        let has_return = matches!(ty, Type::Function { return_type, .. }
            if { let resolved = return_type.resolve(); !resolved.is_unit() && !resolved.is_variable() });

        let return_ty_string = if has_return {
            match ty {
                Type::Function { return_type, .. } => {
                    format!(" {}", self.go_type_as_string(return_type))
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };

        let should_return = has_return;

        let saved_return_context = self.current_return_context.clone();
        if let Type::Function { return_type, .. } = ty {
            self.current_return_context = Some(return_type.as_ref().clone());
        }

        let mut body_string = String::new();

        for (temp_name, pattern, typed) in &destructure_bindings {
            self.emit_pattern_bindings(&mut body_string, temp_name, pattern, *typed);
        }

        self.emit_function_body(&mut body_string, body, should_return);
        optimize_function_body(&mut body_string);

        self.scope.declared = saved_declared;
        self.scope.scope_depth = saved_scope_depth;
        self.scope.bindings.restore();

        self.current_return_context = saved_return_context;

        format!(
            "func({}){} {{\n{}}}",
            group_params(&param_pairs),
            return_ty_string,
            body_string
        )
    }

    pub(crate) fn is_go_never(expression: &Expression) -> bool {
        match expression {
            Expression::Return { .. } => true,
            Expression::Call { expression, .. } => {
                matches!(&**expression, Expression::Identifier { value, .. } if value == "panic")
            }
            _ => false,
        }
    }
}
