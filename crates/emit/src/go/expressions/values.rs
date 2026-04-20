use rustc_hash::FxHashSet as HashSet;

use syntax::program::Definition;

use crate::Emitter;
use crate::go::is_order_sensitive;
use crate::go::names::go_name;
use crate::go::types::emitter::Position;
use crate::go::utils::Staged;
use crate::go::write_line;
use std::fmt::Write;
use syntax::ast::{Expression, FormatStringPart, Literal, Visibility};
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
            return self.emit_go_fn_wrapper(output, expression, &strategy);
        }

        if self.is_go_array_return_value(expression) {
            return self.emit_array_return_wrapper(output, expression);
        }

        self.emit_operand(output, expression)
    }

    pub(crate) fn emit_composite_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> String {
        if expression.get_type().resolve().is_unit()
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
            Expression::Identifier { value, ty, .. } => self.emit_identifier(value, ty),
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
                } else {
                    self.emit_call(output, expression, Some(ty))
                }
            }
            Expression::DotAccess {
                expression,
                member,
                ty,
                span,
            } => self.emit_dot_access(output, expression, member, ty, *span),
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
            Expression::Tuple { elements, ty, .. } => {
                let inferred_slot_types: Vec<Type> = match ty.resolve() {
                    Type::Tuple(slots) => slots,
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

                let elem_expressions: Vec<String> = elements
                    .iter()
                    .zip(elem_expressions)
                    .enumerate()
                    .map(|(i, (expr, emitted))| match slot_types.get(i) {
                        Some(slot) => {
                            self.maybe_wrap_as_go_interface(emitted, &expr.get_type(), slot)
                        }
                        None => emitted,
                    })
                    .collect();

                self.flags.needs_stdlib = true;
                let arity = elem_expressions.len();

                let needs_explicit_type_args = !slot_types.is_empty()
                    && slot_types.iter().any(|t| self.as_interface(t).is_some());

                if needs_explicit_type_args {
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
                } else {
                    format!(
                        "lisette.MakeTuple{}({})",
                        arity,
                        elem_expressions.join(", ")
                    )
                }
            }
            Expression::If { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. } => {
                let result_var = self.declare_result_var(output, ty);
                let saved_target_ty = self.assign_target_ty.replace(ty.clone());
                self.with_position(Position::Assign(result_var.clone()), |this| {
                    this.emit_branching_directly(output, expression);
                });
                self.assign_target_ty = saved_target_ty;
                result_var
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

    fn emit_cast(
        &mut self,
        output: &mut String,
        expression: &Expression,
        target_type: &syntax::ast::Annotation,
        ty: &Type,
    ) -> String {
        let inner = self.emit_operand(output, expression);

        if let Type::Constructor { id, .. } = &self.peel_alias(ty)
            && matches!(
                self.ctx.definitions.get(id.as_str()),
                Some(Definition::Interface { .. })
            )
        {
            let source_ty = expression.get_type();
            return self.maybe_wrap_as_go_interface(inner, &source_ty, ty);
        }

        let go_type = self.annotation_to_go_type(target_type);

        format!("{}({})", go_type, inner)
    }

    fn emit_reference(&mut self, output: &mut String, inner: &Expression, ty: &Type) -> String {
        if inner.get_type().resolve().is_unit()
            && matches!(inner.unwrap_parens(), Expression::Call { .. })
        {
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
        if inner.get_type().resolve() == ty.resolve() {
            emitted
        } else if self.is_go_unaddressable(inner)
            || matches!(inner.get_type().resolve(), Type::Function { .. })
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
                && self.is_newtype_struct(&inner.get_type().resolve().strip_refs())
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
            let unwrapped = self.maybe_unwrap_go_nullable(
                output,
                &rhs_staged.value,
                &value.get_type().resolve(),
            );
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

    fn emit_literal(&mut self, output: &mut String, literal: &Literal, ty: &Type) -> String {
        match literal {
            Literal::Integer { value, text } => match text {
                Some(original) => original.clone(),
                None => value.to_string(),
            },
            Literal::Float { value, text } => match text {
                Some(t) => t.clone(),
                None => {
                    let s = value.to_string();
                    if s.contains('.') || s.contains('e') || s.contains('E') {
                        s
                    } else {
                        format!("{}.0", s)
                    }
                }
            },
            Literal::Imaginary(coef) => {
                if *coef == coef.trunc() && coef.abs() < 1e15 {
                    format!("{}i", *coef as i64)
                } else {
                    format!("{}i", coef)
                }
            }
            Literal::Boolean(b) => b.to_string(),
            Literal::String(s) => {
                format!("\"{}\"", convert_escape_sequences(s))
            }
            Literal::Char(c) => {
                format!("'{}'", convert_escape_sequences(c))
            }
            Literal::FormatString(parts) => self.emit_format_string(output, parts),
            Literal::Slice(elems) => {
                let stages: Vec<Staged> = elems.iter().map(|e| self.stage_composite(e)).collect();
                let elements = self.sequence(output, stages, "_v");

                let elem_lisette_ty = ty
                    .get_type_params()
                    .expect("Slice type must have type args")
                    .first()
                    .expect("Slice type must have element type")
                    .clone();
                let elem_ty = self.go_type_as_string(&elem_lisette_ty);

                let elements: Vec<String> = elems
                    .iter()
                    .zip(elements)
                    .map(|(expr, emitted)| {
                        self.maybe_wrap_as_go_interface(emitted, &expr.get_type(), &elem_lisette_ty)
                    })
                    .collect();

                if elements.len() > 1 && elements.iter().any(|e| e.len() > 30) {
                    let indented = elements
                        .iter()
                        .map(|e| format!("\t{}", e))
                        .collect::<Vec<_>>()
                        .join(",\n");
                    format!("[]{}{{\n{},\n}}", elem_ty, indented)
                } else {
                    format!("[]{}{{ {} }}", elem_ty, elements.join(", "))
                }
            }
        }
    }

    fn emit_format_string(&mut self, output: &mut String, parts: &[FormatStringPart]) -> String {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        // Stage all expression parts for eval-order sequencing
        let stages: Vec<Staged> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.stage_composite(e))
                } else {
                    None
                }
            })
            .collect();
        let emitted = self.sequence(output, stages, "_fmtarg");

        let mut format_string = String::new();
        let mut args = Vec::new();
        let mut expression_idx = 0;

        for part in parts {
            match part {
                FormatStringPart::Text(text) => {
                    let unescaped = text.replace("{{", "{").replace("}}", "}");
                    let unescaped = convert_escape_sequences(&unescaped);
                    if has_interpolation {
                        format_string.push_str(&unescaped.replace('%', "%%"));
                    } else {
                        format_string.push_str(&unescaped);
                    }
                }
                FormatStringPart::Expression(expression) => {
                    let format_verb = if expression.get_type().resolve().is_rune() {
                        "%c"
                    } else {
                        "%v"
                    };
                    format_string.push_str(format_verb);
                    args.push(emitted[expression_idx].clone());
                    expression_idx += 1;
                }
            }
        }

        if args.is_empty() {
            return format!("\"{}\"", format_string);
        }

        self.flags.needs_fmt = true;
        if format_string == "%v" && args.len() == 1 {
            return format!("fmt.Sprint({})", args[0]);
        }
        format!("fmt.Sprintf(\"{}\", {})", format_string, args.join(", "))
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
        let resolved = ty.resolve();
        if resolved.is_unit() || matches!(resolved, Type::Variable(_) | Type::Forall { .. }) {
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
                if !matches!(ty.resolve(), Type::Function { .. }) =>
            {
                if self.scope.bindings.get(value).is_some() {
                    return false;
                }
                if let Type::Constructor { id, .. } = ty.resolve() {
                    matches!(
                        self.ctx.definitions.get(id.as_str()),
                        Some(Definition::Enum { .. })
                    )
                } else {
                    false
                }
            }

            Expression::DotAccess { expression, ty, .. }
                if !matches!(ty.resolve(), Type::Function { .. }) =>
            {
                if let Type::Constructor { id, .. } = ty.resolve() {
                    if !matches!(
                        self.ctx.definitions.get(id.as_str()),
                        Some(Definition::Enum { .. })
                    ) {
                        return false;
                    }
                    let receiver_ty = expression.get_type().resolve();
                    if let Type::Constructor {
                        id: receiver_id, ..
                    } = &receiver_ty
                    {
                        matches!(
                            self.ctx.definitions.get(receiver_id.as_str()),
                            Some(Definition::Enum { .. } | Definition::TypeAlias { .. })
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

pub(crate) fn convert_escape_sequences(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.peek() == Some(&'\\') {
                result.push('\\');
                result.push('\\');
                chars.next();
            } else if matches!(chars.peek(), Some('0'..='7')) {
                let mut value: u16 = 0;
                for _ in 0..3 {
                    match chars.peek() {
                        Some(&d @ '0'..='7') => {
                            value = value * 8 + (d as u16 - b'0' as u16);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                write!(result, "\\x{:02x}", value).unwrap();
            } else if chars.peek() == Some(&'u') && {
                let mut lookahead = chars.clone();
                lookahead.next();
                lookahead.peek() == Some(&'{')
            } {
                chars.next(); // consume 'u'
                chars.next(); // consume '{'
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let codepoint = u32::from_str_radix(&hex, 16).unwrap_or(0);
                if codepoint <= 0xFFFF {
                    write!(result, "\\u{:04X}", codepoint).unwrap();
                } else {
                    write!(result, "\\U{:08X}", codepoint).unwrap();
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}
