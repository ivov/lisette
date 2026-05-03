use rustc_hash::FxHashSet as HashSet;

use syntax::program::DefinitionBody;

use crate::Emitter;
use crate::is_order_sensitive;
use crate::types::coercion::Coercion;
use crate::types::emitter::Position;
use crate::utils::Staged;
use crate::write_line;
use syntax::ast::{Expression, Visibility};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_doc(&self, doc: &Option<String>) -> String {
        match doc {
            Some(text) => {
                let lines: Vec<String> = text
                    .lines()
                    .map(|line| {
                        if line.is_empty() {
                            "//".to_string()
                        } else {
                            format!("// {}", line)
                        }
                    })
                    .collect();
                if lines.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", lines.join("\n"))
                }
            }
            None => String::new(),
        }
    }

    pub(crate) fn emit_top_item(&mut self, item: &Expression) -> String {
        match item {
            Expression::Function {
                doc,
                visibility,
                name_span,
                ..
            } => {
                if self.ctx.unused.is_unused_definition(name_span) {
                    return String::new();
                }
                let is_public = matches!(visibility, Visibility::Public);
                let function = item.to_function_definition();
                let doc_comment = self.emit_doc(doc);

                let code = self.emit_function(&function, None, is_public);
                format!("{}{}", doc_comment, code)
            }
            Expression::Struct {
                doc,
                attributes,
                name,
                generics,
                fields,
                kind,
                ..
            } => {
                let doc_comment = self.emit_doc(doc);
                let code = self.emit_struct_definition(name, generics, fields, kind, attributes);
                format!("{}{}", doc_comment, code)
            }
            Expression::Enum {
                doc,
                attributes,
                name,
                generics,
                ..
            } => {
                let doc_comment = self.emit_doc(doc);
                let code = self
                    .emit_enum(name, generics, attributes)
                    .unwrap_or_default();
                format!("{}{}", doc_comment, code)
            }
            Expression::ValueEnum { .. } => String::new(),
            Expression::TypeAlias {
                doc,
                name,
                generics,
                ty,
                ..
            } => {
                let doc_comment = self.emit_doc(doc);
                let code = self.emit_type_alias(name, generics, ty);
                format!("{}{}", doc_comment, code)
            }
            Expression::Interface {
                doc,
                name,
                method_signatures,
                parents,
                generics,
                visibility,
                ..
            } => {
                let doc_comment = self.emit_doc(doc);
                let is_public = matches!(visibility, Visibility::Public);
                let code =
                    self.emit_interface(name, method_signatures, parents, generics, is_public);
                format!("{}{}", doc_comment, code)
            }
            Expression::ImplBlock {
                receiver_name,
                ty,
                methods,
                generics,
                ..
            } => self.emit_impl_block(receiver_name, ty, methods, generics),
            Expression::Const {
                doc,
                identifier,
                expression,
                ty,
                ..
            } => {
                let doc_comment = self.emit_doc(doc);
                let code = self.emit_const(identifier, expression, ty);
                format!("{}{}", doc_comment, code)
            }
            _ => String::new(),
        }
    }

    pub(crate) fn declare_result_var(&mut self, output: &mut String, ty: &Type) -> String {
        let result_var = self.fresh_var(None);
        write_line!(output, "var {} {}", result_var, self.go_type_as_string(ty));
        self.declare(&result_var);
        result_var
    }

    pub(crate) fn emit_value(&mut self, output: &mut String, expression: &Expression) -> String {
        if let Some(strategy) = self.classify_go_fn_value(expression) {
            if self.go_fn_matches_lowered_slot(expression, &strategy) {
                return self.emit_operand(output, expression);
            }
            return self.emit_go_fn_wrapper(output, expression, &strategy);
        }

        if self.is_go_array_return_value(expression) {
            return self.emit_array_return_wrapper(output, expression);
        }

        self.emit_operand(output, expression)
    }

    /// Wrap a captured tagged-shape prelude fn ref into a lowered-ABI closure
    /// so its Go type matches what the rest of the pipeline expects.
    fn maybe_lower_tagged_fn_ref(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
        raw: String,
    ) -> String {
        if self.emitting_call_callee || self.suppress_go_fn_short_circuit {
            return raw;
        }
        if !Self::is_tagged_shape_fn_value(expression) {
            return raw;
        }
        let fn_ty = ty.unwrap_forall();
        let Type::Function { return_type, .. } = fn_ty else {
            return raw;
        };
        if self.classify_direct_emission(return_type).is_none() {
            return raw;
        }
        self.emit_lisette_callback_wrapper(output, &raw, fn_ty)
    }

    /// True when a Go function value's natural ABI matches the slot's
    /// lowered shape — wrapping would be identity.
    fn go_fn_matches_lowered_slot(
        &self,
        expression: &Expression,
        strategy: &crate::GoCallStrategy,
    ) -> bool {
        if self.suppress_go_fn_short_circuit {
            return false;
        }
        let fn_ty = expression.get_type();
        let Type::Function { return_type, .. } = fn_ty.unwrap_forall() else {
            return false;
        };
        let Some(shape) = self.classify_direct_emission(return_type) else {
            return false;
        };
        use crate::GoCallStrategy as G;
        use crate::types::abi::AbiShape as A;
        match (strategy, &shape) {
            (G::Result, A::ResultTuple | A::BareError)
            | (G::Partial, A::PartialTuple)
            | (G::CommaOk, A::CommaOk)
            | (G::NullableReturn, A::NullableReturn) => true,
            (G::Tuple { arity: a }, A::Tuple { arity: b }) => a == b,
            _ => false,
        }
    }

    pub(crate) fn emit_composite_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> String {
        if expression.get_type().is_unit()
            && matches!(expression.unwrap_parens(), Expression::Call { .. })
        {
            let call_str = self.emit_value(output, expression);
            if !call_str.is_empty() {
                write_line!(output, "{call_str}");
            }
            return "struct{}{}".to_string();
        }
        self.emit_value(output, expression)
    }

    pub(crate) fn emit_operand(&mut self, output: &mut String, expression: &Expression) -> String {
        match expression {
            Expression::Literal { literal, ty, .. } => self.emit_literal(output, literal, ty),
            Expression::Identifier { value, ty, .. } => {
                let raw = self.emit_identifier(value, ty);
                self.maybe_lower_tagged_fn_ref(output, expression, ty, raw)
            }
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => self.emit_binary_expression(output, operator, left, right),
            Expression::Unary {
                operator,
                expression,
                ..
            } => self.emit_unary_expression(output, operator, expression),
            Expression::Call { ty, .. } => {
                if let Some(strategy) = self.resolve_go_call_strategy(expression) {
                    self.emit_go_wrapped_call(output, expression, &strategy, ty)
                } else if let Expression::Call {
                    expression: callee, ..
                } = expression
                    && let Some(shape) = self.classify_callee_abi(callee)
                {
                    self.flags.needs_stdlib = true;
                    let call_str = self.emit_call(output, expression, Some(ty));
                    self.emit_callee_abi_wrapping(output, &shape, &call_str, ty)
                } else {
                    self.emit_call(output, expression, Some(ty))
                }
            }
            Expression::DotAccess {
                expression,
                member,
                ty,
                dot_access_kind,
                receiver_coercion,
                ..
            } => self.emit_dot_access(
                output,
                expression,
                member,
                ty,
                *dot_access_kind,
                *receiver_coercion,
            ),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.emit_index_access(output, expression, index),
            Expression::StructCall {
                name,
                field_assignments,
                spread,
                ty,
                ..
            } => self.emit_struct_call(output, name, field_assignments, spread, ty),
            Expression::Paren { expression, .. } => {
                let inner = self.emit_operand(output, expression);
                format!("({})", inner)
            }
            Expression::Reference {
                expression: inner,
                ty,
                ..
            } => self.emit_reference(output, inner, ty),
            Expression::Task { expression, .. } => {
                self.emit_async_wrapper(output, "go", expression)
            }
            Expression::Defer { expression, .. } => {
                self.emit_async_wrapper(output, "defer", expression)
            }
            Expression::RawGo { text } => text.clone(),
            Expression::Unit { .. } => "struct{}{}".to_string(),
            Expression::NoOp => String::new(),
            Expression::Lambda {
                params, body, ty, ..
            } => self.emit_lambda(params, body, ty),
            Expression::Function {
                params, body, ty, ..
            } => self.emit_lambda(params, body, ty),
            Expression::Propagate { expression, .. } => {
                self.emit_propagate(output, expression, None)
            }
            Expression::TryBlock { items, ty, .. } => self.emit_try_block(output, items, ty),
            Expression::RecoverBlock { items, ty, .. } => {
                self.emit_recover_block(output, items, ty)
            }
            Expression::Tuple { elements, ty, .. } => self.emit_tuple_value(output, elements, ty),
            Expression::If { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. } => {
                self.emit_branching_as_operand(output, expression, ty)
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Block { ty, items, .. } => {
                self.emit_block_as_operand(output, expression, ty, items)
            }
            Expression::Loop {
                body,
                ty,
                needs_label,
                ..
            } => {
                let result_var = self.declare_result_var(output, ty);
                self.push_loop(result_var.clone());
                self.emit_labeled_loop(output, "for {\n", body, *needs_label);
                self.pop_loop();
                result_var
            }
            Expression::Return {
                expression: return_expression,
                ..
            } => {
                self.emit_return(output, return_expression);
                String::new()
            }
            Expression::Range {
                start,
                end,
                inclusive,
                ty,
                ..
            } => self.emit_range_value(output, start, end, *inclusive, ty),
            Expression::Cast {
                expression,
                target_type,
                ty,
                ..
            } => self.emit_cast(output, expression, target_type, ty),
            Expression::Assignment { target, value, .. } => {
                self.emit_assignment_operand(output, target, value);
                "struct{}{}".to_string()
            }
            _ => unreachable!("unexpected expression in emit: {:?}", expression),
        }
    }

    fn emit_tuple_value(
        &mut self,
        output: &mut String,
        elements: &[Expression],
        ty: &Type,
    ) -> String {
        let inferred_slot_types: Vec<Type> = match ty {
            Type::Tuple(slots) => slots.clone(),
            _ => Vec::new(),
        };
        let slot_types = self.resolve_tuple_slot_types(inferred_slot_types);

        let stages: Vec<Staged> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let prev = std::mem::replace(
                    &mut self.current_slot_expected_ty,
                    slot_types.get(i).cloned(),
                );
                let staged = self.stage_composite(e);
                self.current_slot_expected_ty = prev;
                staged
            })
            .collect();
        let elem_expressions = self.sequence(output, stages, "_v");

        let mut wrapped_expressions: Vec<String> = Vec::with_capacity(elem_expressions.len());
        for (i, (expr, emitted)) in elements.iter().zip(elem_expressions).enumerate() {
            let value = match slot_types.get(i) {
                Some(slot) => {
                    let coercion = Coercion::resolve(self, &expr.get_type(), slot);
                    coercion.apply(self, output, emitted)
                }
                None => emitted,
            };
            wrapped_expressions.push(value);
        }
        let elem_expressions = wrapped_expressions;

        self.flags.needs_stdlib = true;
        let arity = elem_expressions.len();

        let needs_explicit_type_args =
            !slot_types.is_empty() && slot_types.iter().any(|t| self.as_interface(t).is_some());

        if !needs_explicit_type_args {
            return format!(
                "lisette.MakeTuple{}({})",
                arity,
                elem_expressions.join(", ")
            );
        }
        let slot_ty_strs: Vec<String> = slot_types
            .iter()
            .map(|t| self.go_type_as_string(t))
            .collect();
        format!(
            "lisette.MakeTuple{}[{}]({})",
            arity,
            slot_ty_strs.join(", "),
            elem_expressions.join(", ")
        )
    }

    fn emit_branching_as_operand(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
    ) -> String {
        let result_var = self.declare_result_var(output, ty);
        let saved_target_ty = self.assign_target_ty.replace(ty.clone());
        self.with_position(Position::Assign(result_var.clone()), |this| {
            this.emit_branching_directly(output, expression);
        });
        self.assign_target_ty = saved_target_ty;
        result_var
    }

    fn emit_cast(
        &mut self,
        output: &mut String,
        expression: &Expression,
        target_type: &syntax::ast::Annotation,
        ty: &Type,
    ) -> String {
        let inner = self.emit_operand(output, expression);

        if let Type::Nominal { id, .. } = &self.peel_alias(ty)
            && matches!(
                self.ctx.definitions.get(id.as_str()).map(|d| &d.body),
                Some(DefinitionBody::Interface { .. })
            )
        {
            let source_ty = expression.get_type();
            let coercion = Coercion::resolve(self, &source_ty, ty);
            return coercion.apply(self, output, inner);
        }

        let go_type = self.annotation_to_go_type(target_type);

        format!("{}({})", go_type, inner)
    }

    fn emit_reference(&mut self, output: &mut String, inner: &Expression, ty: &Type) -> String {
        if inner.get_type().is_unit() && matches!(inner.unwrap_parens(), Expression::Call { .. }) {
            let emitted = self.emit_operand(output, inner.unwrap_parens());
            if !emitted.is_empty() {
                write_line!(output, "{}", emitted);
            }
            let tmp = self.fresh_var(Some("ref"));
            self.declare(&tmp);
            write_line!(output, "{} := struct{{}}{{}}", tmp);
            return format!("&{}", tmp);
        }

        let emitted = self.emit_value(output, inner);
        if inner.get_type() == *ty {
            emitted
        } else if self.is_go_unaddressable(inner)
            || matches!(inner.get_type(), Type::Function { .. })
        {
            let tmp = self.fresh_var(Some("ref"));
            self.declare(&tmp);
            write_line!(output, "{} := {}", tmp, emitted);
            format!("&{}", tmp)
        } else {
            format!("&{}", emitted)
        }
    }

    pub(crate) fn contains_newtype_access(&self, expression: &Expression) -> bool {
        let mut current = expression;
        while let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = current
        {
            if member.parse::<usize>().is_ok()
                && self.is_newtype_struct(&inner.get_type().strip_refs())
            {
                return true;
            }
            current = inner;
        }
        false
    }

    fn emit_assignment_operand(
        &mut self,
        output: &mut String,
        target: &Expression,
        value: &Expression,
    ) {
        let rhs_staged = self.stage_composite(value);

        let target_str = if is_order_sensitive(target) {
            self.emit_left_value_capturing(output, target, !rhs_staged.setup.is_empty())
        } else {
            self.emit_left_value(output, target)
        };
        output.push_str(&rhs_staged.setup);

        if let Expression::DotAccess {
            expression: receiver,
            ty,
            ..
        } = target
            && Self::is_go_imported_type(&receiver.get_type())
            && self.is_go_nullable(ty)
        {
            let coercion = Coercion::resolve_unwrap_go_nullable(self, &value.get_type(), Some(ty));
            let unwrapped = coercion.apply(self, output, rhs_staged.value);
            write_line!(output, "{} = {}", target_str, unwrapped);
        } else {
            write_line!(output, "{} = {}", target_str, rhs_staged.value);
        }
    }

    fn emit_range_value(
        &mut self,
        output: &mut String,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        _inclusive: bool,
        ty: &Type,
    ) -> String {
        let type_string = self.go_type_as_string(ty);

        let mut stages: Vec<Staged> = Vec::new();
        let has_start = start.is_some();
        if let Some(s) = start {
            stages.push(self.stage_operand(s));
        }
        if let Some(e) = end {
            stages.push(self.stage_operand(e));
        }

        if stages.is_empty() {
            return "struct{}{}".to_string();
        }

        let values = self.sequence(output, stages, "_range");
        let mut fields = Vec::new();
        if has_start {
            fields.push(("Start".to_string(), values[0].clone()));
            if values.len() > 1 {
                fields.push(("End".to_string(), values[1].clone()));
            }
        } else {
            fields.push(("End".to_string(), values[0].clone()));
        }

        self.emit_struct_literal(&type_string, &fields)
    }

    /// Emit a block expression as an operand, returning the result variable name.
    /// Never-typed blocks diverge and produce no value.
    fn emit_block_as_operand(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
        items: &[Expression],
    ) -> String {
        if ty.is_never() {
            self.emit_block(output, expression);
            return String::new();
        }
        if ty.is_unit() || matches!(ty, Type::Var { .. } | Type::Forall { .. }) {
            self.emit_block(output, expression);
            return String::new();
        }
        let result_var = self.declare_result_var(output, ty);
        let needs_braces = items.len() > 1;
        if needs_braces {
            output.push_str("{\n");
        }
        self.emit_block_to_var_with_braces(output, expression, &result_var, needs_braces);
        if needs_braces {
            output.push_str("}\n");
        }
        result_var
    }

    pub(crate) fn with_fresh_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved_declared = std::mem::take(&mut self.scope.declared);
        self.scope.declared = vec![HashSet::default()];
        let saved_scope_depth = self.scope.scope_depth;
        self.scope.scope_depth = 0;
        self.scope.bindings.save();

        let result = f(self);

        self.scope.bindings.restore();
        self.scope.declared = saved_declared;
        self.scope.scope_depth = saved_scope_depth;
        result
    }

    fn emit_async_wrapper(
        &mut self,
        output: &mut String,
        keyword: &str,
        expression: &Expression,
    ) -> String {
        if let Expression::Block { .. } = expression {
            self.with_fresh_scope(|emitter| {
                write_line!(output, "{} func() {{", keyword);
                emitter.emit_block(output, expression);
                output.push_str("}()\n");
            });
            String::new()
        } else if let Some(call_str) = self.emit_go_call_discarded(output, expression) {
            format!("{} {}", keyword, call_str)
        } else {
            let inner = self.emit_value(output, expression);
            format!("{} {}", keyword, inner)
        }
    }
}

impl Emitter<'_> {
    fn is_go_unaddressable(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Call { .. } => true,

            Expression::Identifier { value, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function { .. }) =>
            {
                if self.scope.bindings.get(value).is_some() {
                    return false;
                }
                if let Type::Nominal { id, .. } = ty {
                    matches!(
                        self.ctx.definitions.get(id.as_str()).map(|d| &d.body),
                        Some(DefinitionBody::Enum { .. })
                    )
                } else {
                    false
                }
            }

            Expression::DotAccess { expression, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function { .. }) =>
            {
                if let Type::Nominal { id, .. } = ty {
                    if !matches!(
                        self.ctx.definitions.get(id.as_str()).map(|d| &d.body),
                        Some(DefinitionBody::Enum { .. })
                    ) {
                        return false;
                    }
                    let receiver_ty = expression.get_type();
                    if let Type::Nominal {
                        id: receiver_id, ..
                    } = &receiver_ty
                    {
                        matches!(
                            self.ctx
                                .definitions
                                .get(receiver_id.as_str())
                                .map(|d| &d.body),
                            Some(DefinitionBody::Enum { .. } | DefinitionBody::TypeAlias { .. })
                        )
                    } else {
                        false
                    }
                } else {
                    false
                }
            }

            _ => false,
        }
    }
}
