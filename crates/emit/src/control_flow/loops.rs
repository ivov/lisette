use crate::EmitEffects;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::is_order_sensitive;
use crate::patterns::binding_decls::pattern_has_bindings;
use crate::patterns::sites::PatternSubject;
use crate::plan::bodies::{LoopPlan, LoweredBlock, LoweredStatement};
use crate::types::native::NativeGoType;
use crate::types::shape::RangeShape;
use syntax::ast::{Binding, Expression, Pattern};
use syntax::types::Type;

impl Planner<'_> {
    /// Lower a `for` statement, dispatching on iterable/pattern shape.
    pub(crate) fn lower_for_statement(
        &mut self,
        full_expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let Expression::For {
            binding,
            iterable,
            body,
            needs_label,
            ..
        } = full_expression
        else {
            unreachable!("lower_for_statement requires a For expression");
        };
        let iterable = iterable.as_ref();
        let body = body.as_ref();
        let needs_label = *needs_label;
        let iterable_ty = iterable.get_type();
        let is_range = matches!(iterable, Expression::Range { .. });
        let stored_range = (!is_range)
            .then(|| self.range_shape(&iterable_ty))
            .flatten()
            .filter(|rs| {
                matches!(
                    rs,
                    RangeShape::Range | RangeShape::RangeInclusive | RangeShape::RangeFrom
                )
            });
        let string_view = recognize_string_view_loop(binding, iterable);
        let is_simple = self.for_loop_is_simple(binding, iterable);
        let map_tuple = self.for_loop_is_map_tuple(binding, iterable);

        let directive = self.maybe_line_directive(&full_expression.get_span());
        self.push_loop("_");
        self.set_current_loop_label_if_needed(needs_label);
        let label = self.current_loop_label().map(str::to_string);

        let (prologue, header, lowered_body) = if is_range {
            self.lower_range_for(binding, iterable, body, fx)
        } else if let Some(range_shape) = stored_range {
            self.lower_stored_range_for(binding, iterable, range_shape, body, fx)
        } else if let Some((kind, receiver)) = string_view {
            match kind {
                StringViewKind::Runes => self.lower_runes_for(binding, receiver, body, fx),
                StringViewKind::Bytes => self.lower_bytes_for(binding, receiver, body, fx),
            }
        } else if map_tuple {
            self.lower_map_tuple_for(binding, iterable, body, fx)
        } else if is_simple {
            self.lower_simple_for(binding, iterable, body, fx)
        } else {
            self.lower_pattern_site_for(binding, iterable, body, fx)
        };

        self.pop_loop();

        LoweredStatement::Loop(LoopPlan {
            directive,
            prologue,
            label,
            header,
            body: lowered_body,
        })
    }

    /// `for i in stored_range`.
    fn lower_stored_range_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        range_shape: RangeShape,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        self.enter_scope();
        let mut prologue = String::new();
        let range_var = if self.is_unmutated_identifier(iterable) {
            self.emit_operand(&mut prologue, iterable, ExpressionContext::value(), fx)
        } else {
            self.emit_force_capture(&mut prologue, iterable, "_range", fx)
        };
        let loop_var = self.bind_loop_pattern(&binding.pattern, Some("_i"));
        let header = match range_shape {
            RangeShape::Range => format!(
                "for {} := {}.Start; {} < {}.End; {}++ {{\n",
                loop_var, range_var, loop_var, range_var, loop_var
            ),
            RangeShape::RangeInclusive => format!(
                "for {} := {}.Start; {} <= {}.End; {}++ {{\n",
                loop_var, range_var, loop_var, range_var, loop_var
            ),
            RangeShape::RangeFrom => format!(
                "for {} := {}.Start; ; {}++ {{\n",
                loop_var, range_var, loop_var
            ),
            RangeShape::RangeTo | RangeShape::RangeToInclusive => {
                unreachable!("RangeTo/RangeToInclusive are not iterable")
            }
        };
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    /// `for r in s.runes()`.
    fn lower_runes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        self.enter_scope();
        let mut prologue = String::new();
        let receiver_str =
            self.emit_operand(&mut prologue, receiver, ExpressionContext::value(), fx);
        let loop_var = self.bind_loop_pattern(&binding.pattern, None);
        let header = if loop_var == "_" {
            format!("for range {} {{\n", receiver_str)
        } else {
            format!("for _, {} := range {} {{\n", loop_var, receiver_str)
        };
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    /// `for b in s.bytes()`.
    fn lower_bytes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        self.enter_scope();
        let mut prologue = String::new();
        let receiver_var = if self.is_unmutated_identifier(receiver) {
            self.emit_operand(&mut prologue, receiver, ExpressionContext::value(), fx)
        } else {
            self.emit_force_capture(&mut prologue, receiver, "_s", fx)
        };
        let index_var = self.fresh_var(Some("_i"));
        let loop_var = self.bind_loop_pattern(&binding.pattern, None);
        let mut header = format!(
            "for {} := 0; {} < len({}); {}++ {{\n",
            index_var, index_var, receiver_var, index_var
        );
        if loop_var != "_" {
            header.push_str(&format!(
                "{} := {}[{}]\n",
                loop_var, receiver_var, index_var
            ));
        }
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    /// Returns `(prologue, range_expression, is_channel)`. Refs are deref'd;
    /// channels yield one value per iteration (not a pair).
    fn capture_iterable_operand(
        &mut self,
        iterable: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, bool) {
        let mut prologue = String::new();
        let iter_raw = self.emit_operand(&mut prologue, iterable, ExpressionContext::value(), fx);
        let iterable_ty = iterable.get_type();
        let iter_expression = if iterable_ty.is_ref() {
            format!("*{}", iter_raw)
        } else {
            iter_raw
        };
        let is_channel = self
            .native_shape(&iterable_ty)
            .is_some_and(|s| matches!(s.kind, NativeGoType::Channel | NativeGoType::Receiver));
        (prologue, iter_expression, is_channel)
    }

    /// `for x in xs` over a non-specialized iterable.
    fn lower_simple_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        let (prologue, iter_expression, is_channel) = self.capture_iterable_operand(iterable, fx);

        self.enter_scope();
        let loop_var = self.bind_loop_pattern(&binding.pattern, None);
        let header = if loop_var == "_" {
            format!("for range {} {{\n", iter_expression)
        } else if is_channel {
            format!("for {} := range {} {{\n", loop_var, iter_expression)
        } else {
            format!("for _, {} := range {} {{\n", loop_var, iter_expression)
        };
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    /// `for (k, v) in map`. Simple identifier/wildcard pairs bind directly
    /// in the `range` header; compound patterns capture into fresh vars and
    /// destructure inside the body.
    fn lower_map_tuple_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        let Pattern::Tuple { elements, .. } = &binding.pattern else {
            unreachable!("lower_map_tuple_for requires a tuple pattern");
        };
        let first = &elements[0];
        let second = &elements[1];

        let (prologue, iter_expression, _) = self.capture_iterable_operand(iterable, fx);

        let first_is_simple =
            matches!(first, Pattern::Identifier { .. } | Pattern::WildCard { .. });
        let second_is_simple = matches!(
            second,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        );

        self.enter_scope();
        let (header, lowered_body) = if first_is_simple && second_is_simple {
            self.lower_map_tuple_simple_body(first, second, &iter_expression, body, fx)
        } else {
            self.lower_map_tuple_compound_body(
                first,
                second,
                &binding.ty,
                &iter_expression,
                body,
                fx,
            )
        };
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    /// Simple map-tuple element pair: bind directly in the `range` header.
    fn lower_map_tuple_simple_body(
        &mut self,
        first: &Pattern,
        second: &Pattern,
        iter_expression: &str,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, LoweredBlock) {
        let first_is_discard =
            matches!(first, Pattern::WildCard { .. }) || self.go_name_for_binding(first).is_none();
        let second_is_discard = matches!(second, Pattern::WildCard { .. })
            || self.go_name_for_binding(second).is_none();
        let header = if first_is_discard && second_is_discard {
            format!("for range {} {{\n", iter_expression)
        } else {
            let key = self.bind_loop_pattern(first, None);
            let value = self.bind_loop_pattern(second, None);
            format!("for {}, {} := range {} {{\n", key, value, iter_expression)
        };
        (header, self.lower_block_as_body(body, fx))
    }

    /// Compound map-tuple element pattern: capture key/value into fresh vars,
    /// destructure at the top of the body, discard the temp when unused.
    #[allow(clippy::too_many_arguments)]
    fn lower_map_tuple_compound_body(
        &mut self,
        first: &Pattern,
        second: &Pattern,
        binding_ty: &Type,
        iter_expression: &str,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, LoweredBlock) {
        let element_tys: &[Type] = match binding_ty {
            Type::Tuple(tys) => tys.as_slice(),
            _ => &[],
        };
        let first_ty = element_tys.first().unwrap_or(binding_ty);
        let second_ty = element_tys.get(1).unwrap_or(binding_ty);

        let key_var = self.fresh_var(Some("key"));
        let value_var = self.fresh_var(Some("value"));
        let header = format!(
            "for {}, {} := range {} {{\n",
            key_var, value_var, iter_expression
        );

        self.scope.enter_use_region();
        let mut bindings = String::new();
        self.emit_irrefutable_pattern_site(
            &mut bindings,
            PatternSubject::for_value(key_var.clone()),
            first,
            None,
            first_ty,
            fx,
        );
        if !bindings.is_empty() {
            self.scope.record_go_use(&key_var);
        }
        let after_key = bindings.len();
        self.emit_irrefutable_pattern_site(
            &mut bindings,
            PatternSubject::for_value(value_var.clone()),
            second,
            None,
            second_ty,
            fx,
        );
        if bindings.len() > after_key {
            self.scope.record_go_use(&value_var);
        }
        let lowered_body = self.lower_block_as_body(body, fx);
        let used = self.scope.exit_use_region();

        let mut inner = vec![LoweredStatement::RawGo(bindings)];
        inner.extend(lowered_body.statements);
        let body_block = LoweredBlock { statements: inner };

        let references_value = used.contains(&value_var);
        let references_key = used.contains(&key_var);

        // Discard guards: value first, then key (insertion order matters).
        let mut statements = Vec::new();
        if !references_value {
            statements.push(LoweredStatement::RawGo(format!("_ = {}\n", value_var)));
        }
        if !references_key {
            statements.push(LoweredStatement::RawGo(format!("_ = {}\n", key_var)));
        }
        statements.extend(body_block.statements);
        (header, LoweredBlock { statements })
    }

    /// `for <pattern> in iterable` catch-all. Bindless patterns use `for
    /// range`; binding patterns capture an `item` temp and destructure it at
    /// the top of the body.
    fn lower_pattern_site_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        let (prologue, iter_expression, is_channel) = self.capture_iterable_operand(iterable, fx);

        self.enter_scope();
        let (header, body_block) = if !pattern_has_bindings(&binding.pattern) {
            let header = format!("for range {} {{\n", iter_expression);
            (header, self.lower_block_as_body(body, fx))
        } else {
            let item_var = self.fresh_var(Some("item"));
            let header = if is_channel {
                format!("for {} := range {} {{\n", item_var, iter_expression)
            } else {
                format!("for _, {} := range {} {{\n", item_var, iter_expression)
            };
            self.scope.enter_use_region();
            let mut bindings = String::new();
            self.emit_irrefutable_pattern_site(
                &mut bindings,
                PatternSubject::for_value(item_var.clone()),
                &binding.pattern,
                binding.typed_pattern.as_ref(),
                &binding.ty,
                fx,
            );
            if !bindings.is_empty() {
                self.scope.record_go_use(&item_var);
            }
            let lowered_body = self.lower_block_as_body(body, fx);
            let used = self.scope.exit_use_region();

            let mut inner = vec![LoweredStatement::RawGo(bindings)];
            inner.extend(lowered_body.statements);
            let body_block = LoweredBlock { statements: inner };

            let references_item = used.contains(&item_var);

            let mut statements = Vec::new();
            if !references_item {
                statements.push(LoweredStatement::RawGo(format!("_ = {}\n", item_var)));
            }
            statements.extend(body_block.statements);
            (header, LoweredBlock { statements })
        };
        self.exit_scope();
        (prologue, header, body_block)
    }

    /// `for i in start..end`.
    fn lower_range_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
        fx: &mut EmitEffects,
    ) -> (String, String, LoweredBlock) {
        let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = iterable
        else {
            unreachable!("lower_range_for requires a Range iterable");
        };

        let mut prologue = String::new();
        let mut start_expression = match start {
            Some(s) => self.emit_operand(&mut prologue, s, ExpressionContext::value(), fx),
            None => "0".to_string(),
        };
        let checkpoint = prologue.len();
        let end_expression = end
            .as_ref()
            .map(|e| self.emit_force_capture(&mut prologue, e, "_bound", fx));
        if prologue.len() > checkpoint && start.as_ref().is_some_and(|s| is_order_sensitive(s)) {
            let var = self.fresh_var(Some("start"));
            self.declare(&var);
            let statement = format!("{} := {}\n", var, start_expression);
            prologue.insert_str(checkpoint, &statement);
            start_expression = var;
        }

        self.enter_scope();
        let loop_var = self.bind_loop_pattern(&binding.pattern, Some("_i"));
        let header = match end_expression {
            Some(end_expression) => {
                let operator = if *inclusive { "<=" } else { "<" };
                format!(
                    "for {} := {}; {} {} {}; {}++ {{\n",
                    loop_var, start_expression, loop_var, operator, end_expression, loop_var
                )
            }
            None => format!(
                "for {} := {}; ; {}++ {{\n",
                loop_var, start_expression, loop_var
            ),
        };
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        (prologue, header, lowered_body)
    }

    fn for_loop_is_simple(&self, binding: &Binding, iterable: &Expression) -> bool {
        if !matches!(
            &binding.pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            return false;
        }
        if matches!(iterable, Expression::Range { .. }) {
            return false;
        }
        let iterable_ty = iterable.get_type();
        if let Some(range_shape) = self.range_shape(&iterable_ty)
            && matches!(
                range_shape,
                RangeShape::Range | RangeShape::RangeInclusive | RangeShape::RangeFrom
            )
        {
            return false;
        }
        if recognize_string_view_loop(binding, iterable).is_some() {
            return false;
        }
        true
    }

    /// `for (k, v) in map` where the iterable is map-like and the pattern is a
    /// 2-tuple. Both simple and compound element patterns route through
    /// `lower_map_tuple_for`.
    fn for_loop_is_map_tuple(&self, binding: &Binding, iterable: &Expression) -> bool {
        let Pattern::Tuple { elements, .. } = &binding.pattern else {
            return false;
        };
        elements.len() == 2 && self.is_map_tuple_iterable(&iterable.get_type())
    }

    /// Extract a loop variable from a pattern, binding the identifier if present.
    /// `fallback` controls what happens when the pattern is unused or non-identifier:
    /// - `Some(hint)`: generate a fresh var (needed for C-style loops where `_` is invalid)
    /// - `None`: use `"_"` (valid in `for range` syntax)
    fn bind_loop_pattern(&mut self, pattern: &Pattern, fallback: Option<&str>) -> String {
        if let Pattern::Identifier { identifier, .. } = pattern
            && let Some(mut go_name) = self.go_name_for_binding(pattern)
        {
            if self.scope.has_binding_for_go_name(&go_name) {
                go_name = self.fresh_var(Some(&go_name));
            }
            return self.scope.bind(identifier, go_name);
        }
        match fallback {
            Some(hint) => self.fresh_var(Some(hint)),
            None => "_".to_string(),
        }
    }

    fn is_map_tuple_iterable(&self, iterable_ty: &Type) -> bool {
        self.native_shape(iterable_ty)
            .is_some_and(|s| matches!(s.kind, NativeGoType::Map | NativeGoType::EnumeratedSlice))
    }

    fn is_unmutated_identifier(&self, expression: &Expression) -> bool {
        if let Expression::Identifier {
            binding_id: Some(id),
            ..
        } = expression
        {
            !self.facts.is_mutated(*id)
        } else {
            false
        }
    }
}

#[derive(Clone, Copy)]
enum StringViewKind {
    Bytes,
    Runes,
}

/// Recognise `for x in s.bytes()` / `for x in s.runes()` for zero-alloc lowering.
fn recognize_string_view_loop<'a>(
    binding: &'a Binding,
    iterable: &'a Expression,
) -> Option<(StringViewKind, &'a Expression)> {
    if !matches!(
        &binding.pattern,
        Pattern::Identifier { .. } | Pattern::WildCard { .. }
    ) {
        return None;
    }

    let Expression::Call {
        expression, args, ..
    } = iterable
    else {
        return None;
    };

    if !args.is_empty() {
        return None;
    }

    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = expression.as_ref()
    else {
        return None;
    };

    if !receiver.get_type().has_name("string") {
        return None;
    }

    match member.as_str() {
        "bytes" => Some((StringViewKind::Bytes, receiver.as_ref())),
        "runes" => Some((StringViewKind::Runes, receiver.as_ref())),
        _ => None,
    }
}
