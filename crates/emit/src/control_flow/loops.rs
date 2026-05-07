use crate::Emitter;
use crate::is_order_sensitive;
use crate::patterns::decision_tree;
use crate::utils::DiscardGuard;
use crate::write_line;
use syntax::ast::{Binding, Expression, Pattern};

impl Emitter<'_> {
    /// Extract a loop variable from a pattern, binding the identifier if present.
    /// `fallback` controls what happens when the pattern is unused or non-identifier:
    /// - `Some(hint)`: generate a fresh var (needed for C-style loops where `_` is invalid)
    /// - `None`: use `"_"` (valid in `for range` syntax)
    fn bind_loop_pattern(&mut self, pattern: &Pattern, fallback: Option<&str>) -> String {
        if let Pattern::Identifier { identifier, .. } = pattern
            && let Some(mut go_name) = self.go_name_for_binding(pattern)
        {
            if self.scope.bindings.has_go_name(&go_name) {
                go_name = self.fresh_var(Some(&go_name));
            }
            return self.scope.bindings.add(identifier, go_name);
        }
        match fallback {
            Some(hint) => self.fresh_var(Some(hint)),
            None => "_".to_string(),
        }
    }

    pub(crate) fn emit_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
        needs_label: bool,
    ) {
        self.maybe_set_loop_label(needs_label);

        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = iterable
        {
            self.emit_range_for_loop(output, binding, start, end, *inclusive, body);
            return;
        }

        let iterable_ty = iterable.get_type();
        if let Some(ty_name) = iterable_ty.get_name()
            && matches!(ty_name, "Range" | "RangeInclusive" | "RangeFrom")
        {
            self.emit_stored_range_for_loop(output, binding, iterable, ty_name, body);
            return;
        }

        if let Some((kind, receiver)) = recognize_string_view_loop(binding, iterable) {
            match kind {
                StringViewKind::Runes => self.emit_runes_for_loop(output, binding, receiver, body),
                StringViewKind::Bytes => self.emit_bytes_for_loop(output, binding, receiver, body),
            }
            return;
        }

        let iter_expression = self.emit_operand(output, iterable);
        let iter_expression = if iterable.get_type().is_ref() {
            format!("*{}", iter_expression)
        } else {
            iter_expression
        };

        let is_channel = iterable_ty
            .get_name()
            .is_some_and(|n| n == "Channel" || n == "Receiver");

        self.enter_scope();

        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }

        match &binding.pattern {
            Pattern::Identifier { .. } => {
                self.emit_identifier_for_loop(
                    output,
                    &binding.pattern,
                    &iter_expression,
                    is_channel,
                    body,
                );
            }
            Pattern::WildCard { .. } => {
                write_line!(output, "for range {} {{", iter_expression);
                self.emit_block(output, body);
                output.push_str("}\n");
            }
            Pattern::Tuple { elements, .. }
                if elements.len() == 2
                    && iterable_ty.get_name().is_some_and(|n| {
                        n == "Map" || n == "OrderedMap" || n == "EnumeratedSlice"
                    }) =>
            {
                self.emit_map_tuple_for_loop(output, elements, &iter_expression, body);
            }
            _ => {
                self.emit_pattern_for_loop(output, binding, &iter_expression, is_channel, body);
            }
        }

        self.exit_scope();
    }

    /// For loops over an identifier-bound iterable: `for x := range xs` (or
    /// `for range xs` when the binding is discarded). Channels drop the index
    /// position from the `range` form.
    fn emit_identifier_for_loop(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        iter_expression: &str,
        is_channel: bool,
        body: &Expression,
    ) {
        let loop_var = self.bind_loop_pattern(pattern, None);
        if loop_var == "_" {
            write_line!(output, "for range {} {{", iter_expression);
        } else if is_channel {
            write_line!(output, "for {} := range {} {{", loop_var, iter_expression);
        } else {
            write_line!(
                output,
                "for _, {} := range {} {{",
                loop_var,
                iter_expression
            );
        }
        self.emit_block(output, body);
        output.push_str("}\n");
    }

    /// Tuple destructuring over a map-like iterable (`Map`, `OrderedMap`,
    /// `EnumeratedSlice`). Simple identifier/wildcard element pairs bind
    /// directly in the `range` header; compound patterns capture into fresh
    /// vars and emit decision-tree bindings inside the loop body.
    fn emit_map_tuple_for_loop(
        &mut self,
        output: &mut String,
        elements: &[Pattern],
        iter_expression: &str,
        body: &Expression,
    ) {
        let first = &elements[0];
        let second = &elements[1];

        let first_is_simple =
            matches!(first, Pattern::Identifier { .. } | Pattern::WildCard { .. });
        let second_is_simple = matches!(
            second,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        );

        if !first_is_simple || !second_is_simple {
            let key_var = self.fresh_var(Some("key"));
            let value_var = self.fresh_var(Some("value"));
            write_line!(
                output,
                "for {}, {} := range {} {{",
                key_var,
                value_var,
                iter_expression
            );
            let key_guard = DiscardGuard::new(output, &key_var);
            let value_guard = DiscardGuard::new(output, &value_var);
            let (_, key_bindings) = decision_tree::collect_pattern_info(self, first, None);
            decision_tree::emit_tree_bindings(self, output, &key_bindings, &key_var);
            let (_, value_bindings) = decision_tree::collect_pattern_info(self, second, None);
            decision_tree::emit_tree_bindings(self, output, &value_bindings, &value_var);
            self.emit_block(output, body);
            key_guard.finish(output);
            value_guard.finish(output);
            output.push_str("}\n");
            return;
        }

        let first_is_discard =
            matches!(first, Pattern::WildCard { .. }) || self.go_name_for_binding(first).is_none();
        let second_is_discard = matches!(second, Pattern::WildCard { .. })
            || self.go_name_for_binding(second).is_none();
        if first_is_discard && second_is_discard {
            write_line!(output, "for range {} {{", iter_expression);
        } else {
            let key = self.bind_loop_pattern(first, None);
            let value = self.bind_loop_pattern(second, None);
            write_line!(
                output,
                "for {}, {} := range {} {{",
                key,
                value,
                iter_expression
            );
        }
        self.emit_block(output, body);
        output.push_str("}\n");
    }

    /// Compound-pattern for loop. Captures each element into a fresh `item`
    /// var, emits decision-tree bindings inside the loop, and discards the
    /// temp via `DiscardGuard` if the pattern doesn't reference it.
    fn emit_pattern_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        iter_expression: &str,
        is_channel: bool,
        body: &Expression,
    ) {
        let (_, bindings) = decision_tree::collect_pattern_info(
            self,
            &binding.pattern,
            binding.typed_pattern.as_ref(),
        );
        if bindings.is_empty() {
            write_line!(output, "for range {} {{", iter_expression);
            self.emit_block(output, body);
            output.push_str("}\n");
            return;
        }
        let item_var = self.fresh_var(Some("item"));
        if is_channel {
            write_line!(output, "for {} := range {} {{", item_var, iter_expression);
        } else {
            write_line!(
                output,
                "for _, {} := range {} {{",
                item_var,
                iter_expression
            );
        }
        let guard = DiscardGuard::new(output, &item_var);
        decision_tree::emit_tree_bindings(self, output, &bindings, &item_var);
        self.emit_block(output, body);
        guard.finish(output);
        output.push_str("}\n");
    }

    fn emit_range_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        inclusive: bool,
        body: &Expression,
    ) {
        let mut start_expression = match start {
            Some(s) => self.emit_operand(output, s),
            None => "0".to_string(),
        };

        let checkpoint = output.len();

        let end_expression = end
            .as_ref()
            .map(|e| self.emit_force_capture(output, e, "_bound"));

        // If the bound capture produced output and the start has side effects,
        // hoist start to preserve left-to-right evaluation order.
        if output.len() > checkpoint && start.as_ref().is_some_and(|s| is_order_sensitive(s)) {
            let var = self.fresh_var(Some("start"));
            self.declare(&var);
            let statement = format!("{} := {}\n", var, start_expression);
            output.insert_str(checkpoint, &statement);
            start_expression = var;
        }

        self.enter_scope();

        let loop_var = self.bind_loop_pattern(&binding.pattern, Some("_i"));

        match end_expression {
            Some(end_expression) => {
                let operator = if inclusive { "<=" } else { "<" };
                if let Some(label) = self.current_loop_label() {
                    write_line!(output, "{}:", label);
                }
                write_line!(
                    output,
                    "for {} := {}; {} {} {}; {}++ {{",
                    loop_var,
                    start_expression,
                    loop_var,
                    operator,
                    end_expression,
                    loop_var
                );
            }
            None => {
                if let Some(label) = self.current_loop_label() {
                    write_line!(output, "{}:", label);
                }
                write_line!(
                    output,
                    "for {} := {}; ; {}++ {{",
                    loop_var,
                    start_expression,
                    loop_var
                );
            }
        }

        self.emit_block(output, body);
        output.push_str("}\n");

        self.exit_scope();
    }

    fn emit_stored_range_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        iterable: &Expression,
        ty_name: &str,
        body: &Expression,
    ) {
        self.enter_scope();

        let range_var = if self.is_unmutated_identifier(iterable) {
            self.emit_operand(output, iterable)
        } else {
            self.emit_force_capture(output, iterable, "_range")
        };
        let loop_var = self.bind_loop_pattern(&binding.pattern, Some("_i"));

        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }

        match ty_name {
            "Range" => {
                write_line!(
                    output,
                    "for {} := {}.Start; {} < {}.End; {}++ {{",
                    loop_var,
                    range_var,
                    loop_var,
                    range_var,
                    loop_var
                );
            }
            "RangeInclusive" => {
                write_line!(
                    output,
                    "for {} := {}.Start; {} <= {}.End; {}++ {{",
                    loop_var,
                    range_var,
                    loop_var,
                    range_var,
                    loop_var
                );
            }
            "RangeFrom" => {
                write_line!(
                    output,
                    "for {} := {}.Start; ; {}++ {{",
                    loop_var,
                    range_var,
                    loop_var
                );
            }
            _ => unreachable!("unexpected range kind: {}", ty_name),
        }

        self.emit_block(output, body);
        output.push_str("}\n");

        self.exit_scope();
    }

    /// `for r in s.runes()` lowers to Go's native rune-range over the string,
    /// bypassing the `[]rune(s)` allocation.
    fn emit_runes_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) {
        self.enter_scope();
        let recv_str = self.emit_operand(output, receiver);
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        let loop_var = self.bind_loop_pattern(&binding.pattern, None);
        if loop_var == "_" {
            write_line!(output, "for range {} {{", recv_str);
        } else {
            write_line!(output, "for _, {} := range {} {{", loop_var, recv_str);
        }
        self.emit_block(output, body);
        output.push_str("}\n");
        self.exit_scope();
    }

    /// `for b in s.bytes()` lowers to a C-style byte-indexed loop, bypassing
    /// the `[]byte(s)` allocation. Snapshots the receiver unless it is an
    /// unmutated identifier, since `len(s)` and `s[i]` reference it twice.
    fn emit_bytes_for_loop(
        &mut self,
        output: &mut String,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) {
        self.enter_scope();
        let recv_var = if self.is_unmutated_identifier(receiver) {
            self.emit_operand(output, receiver)
        } else {
            self.emit_force_capture(output, receiver, "_s")
        };
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        let idx_var = self.fresh_var(Some("_i"));
        let loop_var = self.bind_loop_pattern(&binding.pattern, None);
        write_line!(
            output,
            "for {} := 0; {} < len({}); {}++ {{",
            idx_var,
            idx_var,
            recv_var,
            idx_var
        );
        if loop_var != "_" {
            write_line!(output, "{} := {}[{}]", loop_var, recv_var, idx_var);
        }
        self.emit_block(output, body);
        output.push_str("}\n");
        self.exit_scope();
    }

    fn is_unmutated_identifier(&self, expression: &Expression) -> bool {
        if let Expression::Identifier {
            binding_id: Some(id),
            ..
        } = expression
        {
            !self.ctx.mutations.is_mutated(*id)
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
