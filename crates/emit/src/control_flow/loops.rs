use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::pattern_has_bindings;
use crate::patterns::sites::PatternSubject;
use crate::plan::bodies::GoUses;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoweredBlock, LoweredStatement, Statement, define, discard,
    with_setup,
};
use crate::plan::go_expression::BinaryOp;
use crate::plan::local::GoIdentifier;
use crate::plan::values::{CaptureBoundary, GoExpression};
use crate::types::shape::RangeShape;
use syntax::ast::{Binding, Expression, Pattern};
use syntax::program::NativeTypeKind;
use syntax::types::Type;

fn range_header(
    key: Option<&GoIdentifier>,
    value: Option<&GoIdentifier>,
    iterable: GoExpression,
) -> LoopHeader {
    let named = |name: Option<&GoIdentifier>| name.filter(|name| *name != "_").cloned();
    LoopHeader::Range {
        key: named(key),
        value: named(value),
        iterable,
    }
}

impl Planner<'_> {
    /// Lower a `for` statement, dispatching on iterable/pattern shape.
    pub(crate) fn lower_for_statement(&mut self, full_expression: &Expression) -> Statement {
        let Expression::For {
            binding,
            iterable,
            body,
            ..
        } = full_expression
        else {
            unreachable!("lower_for_statement requires a For expression");
        };
        let iterable = iterable.as_ref();
        let body = body.as_ref();
        let shape = self.classify_for(binding, iterable);

        let line = self.maybe_line_directive(&full_expression.get_span());
        let plan = self.with_loop(GoExpression::name("_".to_string()), |this| {
            let (prologue, header, lowered_body) = match shape {
                ForShape::Range => this.lower_range_for(binding, iterable, body),
                ForShape::StoredRange(range_shape) => {
                    this.lower_stored_range_for(binding, iterable, range_shape, body)
                }
                ForShape::StringView(StringViewKind::Runes, receiver) => {
                    this.lower_runes_for(binding, receiver, body)
                }
                ForShape::StringView(StringViewKind::Bytes, receiver) => {
                    this.lower_bytes_for(binding, receiver, body)
                }
                ForShape::MapTuple => this.lower_map_tuple_for(binding, iterable, body),
                ForShape::Iterate {
                    yields,
                    simple_pattern,
                } => {
                    return this.lower_iterate_for(binding, iterable, yields, simple_pattern, body);
                }
            };
            with_setup(
                prologue,
                LoweredStatement::Loop(this.build_source_loop(header, lowered_body)),
            )
        });
        Statement { line, kind: plan }
    }

    /// Plan `expr` as an operand, pushing its setup into `prologue` and
    /// returning the value.
    pub(crate) fn capture_operand_into(
        &mut self,
        prologue: &mut Vec<Statement>,
        expr: &Expression,
    ) -> GoExpression {
        let plan = self.plan_operand(expr, ExpressionContext::value());
        let (setup, value) = plan.into_parts();
        prologue.extend(setup);
        value
    }

    /// `for i in stored_range`.
    fn lower_stored_range_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        range_shape: RangeShape,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let range_var = if this.is_unmutated_identifier(iterable) {
                this.capture_operand_into(&mut prologue, iterable)
            } else {
                this.capture_value_at_boundary(
                    &mut prologue,
                    iterable,
                    "range",
                    CaptureBoundary::LoopLifetime,
                )
            };
            let loop_var = this.bind_loop_pattern(&binding.pattern, Some("i"));
            let bound = |field: &str| GoExpression::selector(range_var.clone(), field.to_string());
            let compare = |operator: BinaryOp| {
                GoExpression::binary(
                    GoExpression::identifier(loop_var.clone()),
                    operator,
                    bound("End"),
                )
            };
            let condition = match range_shape {
                RangeShape::Range => Some(compare(BinaryOp::Lt)),
                RangeShape::RangeInclusive => Some(compare(BinaryOp::Le)),
                RangeShape::RangeFrom => None,
                RangeShape::RangeTo | RangeShape::RangeToInclusive => {
                    unreachable!("RangeTo/RangeToInclusive are not iterable")
                }
            };
            let header = LoopHeader::Counted {
                variable: loop_var,
                start: bound("Start"),
                condition,
            };
            let lowered_body = this.lower_block_as_body(body);
            (prologue, header, lowered_body)
        })
    }

    /// `for r in s.runes()`.
    fn lower_runes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let receiver = this.capture_operand_into(&mut prologue, receiver);
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = range_header(None, Some(&loop_var), receiver);
            let lowered_body = this.lower_block_as_body(body);
            (prologue, header, lowered_body)
        })
    }

    /// `for b in s.bytes()`.
    fn lower_bytes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let receiver_var = if this.is_unmutated_identifier(receiver) {
                this.capture_operand_into(&mut prologue, receiver)
            } else {
                this.capture_value_at_boundary(
                    &mut prologue,
                    receiver,
                    "s",
                    CaptureBoundary::LoopLifetime,
                )
            };
            let index_var = this.fresh_var(Some("i"));
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = LoopHeader::Counted {
                variable: index_var.clone().into(),
                start: GoExpression::literal("0".to_string()),
                condition: Some(GoExpression::binary(
                    GoExpression::name(index_var.clone()),
                    BinaryOp::Lt,
                    GoExpression::call(
                        GoExpression::external_name("len".to_string()),
                        vec![receiver_var.clone()],
                    ),
                )),
            };
            let mut lowered_body = this.lower_block_as_body(body);
            if loop_var != "_" {
                lowered_body.statements.insert(
                    0,
                    define(
                        loop_var,
                        GoExpression::index(receiver_var, GoExpression::name(index_var)),
                    ),
                );
            }
            (prologue, header, lowered_body)
        })
    }

    fn capture_iterable_operand(
        &mut self,
        iterable: &Expression,
        is_channel: bool,
    ) -> (Vec<Statement>, GoExpression) {
        let mut prologue = Vec::new();
        let iter_raw = self.capture_operand_into(&mut prologue, iterable);
        let mut iter_expression = if iterable.get_type().is_ref() {
            GoExpression::dereference(iter_raw)
        } else {
            iter_raw
        };
        if is_channel {
            iter_expression = self.stable_source(&mut prologue, "ch", iter_expression);
        }
        (prologue, iter_expression)
    }

    fn lower_iterate_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        yields: RangeYield,
        simple_pattern: bool,
        body: &Expression,
    ) -> LoweredStatement {
        let is_channel = matches!(yields, RangeYield::Channel);
        let (prologue, iter_expression) = self.capture_iterable_operand(iterable, is_channel);
        let nil_guard = is_channel.then(|| {
            GoExpression::binary(
                iter_expression.clone(),
                BinaryOp::Ne,
                GoExpression::literal("nil".to_string()),
            )
        });

        let (header, lowered_body) = self.with_scope(|this| {
            if !simple_pattern {
                return this.lower_pattern_site_body(binding, yields, iter_expression, body);
            }
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = match yields {
                RangeYield::Channel | RangeYield::Single => {
                    range_header(Some(&loop_var), None, iter_expression)
                }
                RangeYield::Pair => range_header(None, Some(&loop_var), iter_expression),
            };
            (header, this.lower_block_as_body(body))
        });

        let Some(condition) = nil_guard else {
            return with_setup(
                prologue,
                LoweredStatement::Loop(self.build_source_loop(header, lowered_body)),
            );
        };
        let plan = self.build_source_loop(header, lowered_body);
        with_setup(
            prologue,
            LoweredStatement::If(IfPlan {
                initializer: None,
                condition,
                then_body: LoweredBlock {
                    statements: vec![LoweredStatement::Loop(plan).into()],
                },
                else_arm: ElseArm::None,
            }),
        )
    }

    /// `for (k, v) in map`. Simple identifier/wildcard pairs bind directly
    /// in the `range` header; compound patterns capture into fresh vars and
    /// destructure inside the body.
    fn lower_map_tuple_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        let Pattern::Tuple { elements, .. } = &binding.pattern else {
            unreachable!("lower_map_tuple_for requires a tuple pattern");
        };
        let first = &elements[0];
        let second = &elements[1];

        let (prologue, iter_expression) = self.capture_iterable_operand(iterable, false);

        let first_is_simple =
            matches!(first, Pattern::Identifier { .. } | Pattern::WildCard { .. });
        let second_is_simple = matches!(
            second,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        );

        let (header, lowered_body) = self.with_scope(|this| {
            if first_is_simple && second_is_simple {
                this.lower_map_tuple_simple_body(first, second, iter_expression, body)
            } else {
                this.lower_map_tuple_compound_body(
                    first,
                    second,
                    &binding.ty,
                    iter_expression,
                    body,
                )
            }
        });
        (prologue, header, lowered_body)
    }

    /// Simple map-tuple element pair: bind directly in the `range` header.
    fn lower_map_tuple_simple_body(
        &mut self,
        first: &Pattern,
        second: &Pattern,
        iter_expression: GoExpression,
        body: &Expression,
    ) -> (LoopHeader, LoweredBlock) {
        let first_is_discard =
            matches!(first, Pattern::WildCard { .. }) || self.go_name_for_binding(first).is_none();
        let second_is_discard = matches!(second, Pattern::WildCard { .. })
            || self.go_name_for_binding(second).is_none();
        let header = if first_is_discard && second_is_discard {
            range_header(None, None, iter_expression)
        } else if second_is_discard {
            let key = self.bind_loop_pattern(first, None);
            range_header(Some(&key), None, iter_expression)
        } else {
            let key = self.bind_loop_pattern(first, None);
            let value = self.bind_loop_pattern(second, None);
            range_header(Some(&key), Some(&value), iter_expression)
        };
        (header, self.lower_block_as_body(body))
    }

    /// Compound map-tuple element pattern: capture key/value into fresh vars,
    /// destructure at the top of the body, discard the temp when unused.
    fn lower_map_tuple_compound_body(
        &mut self,
        first: &Pattern,
        second: &Pattern,
        binding_ty: &Type,
        iter_expression: GoExpression,
        body: &Expression,
    ) -> (LoopHeader, LoweredBlock) {
        let element_tys: &[Type] = match binding_ty {
            Type::Tuple(tys) => tys.as_slice(),
            _ => &[],
        };
        let first_ty = element_tys.first().unwrap_or(binding_ty);
        let second_ty = element_tys.get(1).unwrap_or(binding_ty);

        let key_var = self.fresh_var(Some("key"));
        let value_var = self.fresh_var(Some("value"));
        let key_identifier = self.scope.generated_identifier(&key_var);
        let value_identifier = self.scope.generated_identifier(&value_var);
        let header = LoopHeader::Range {
            key: Some(key_identifier.clone()),
            value: Some(value_identifier.clone()),
            iterable: iter_expression,
        };

        let mut bindings = self.lower_irrefutable_pattern_site(
            PatternSubject::for_identifier(key_identifier.clone()),
            first,
            first_ty,
        );
        bindings.extend(self.lower_irrefutable_pattern_site(
            PatternSubject::for_identifier(value_identifier.clone()),
            second,
            second_ty,
        ));
        bindings.extend(self.lower_block_as_body(body).statements);

        let used = GoUses::of(&bindings);
        let references_value = used.contains_identifier(&value_identifier);
        let references_key = used.contains_identifier(&key_identifier);

        // Discard guards: value first, then key (insertion order matters).
        let mut statements = Vec::new();
        if !references_value {
            statements.push(discard(GoExpression::identifier(value_identifier)));
        }
        if !references_key {
            statements.push(discard(GoExpression::identifier(key_identifier)));
        }
        statements.extend(bindings);
        (header, LoweredBlock { statements })
    }

    fn lower_pattern_site_body(
        &mut self,
        binding: &Binding,
        yields: RangeYield,
        iter_expression: GoExpression,
        body: &Expression,
    ) -> (LoopHeader, LoweredBlock) {
        if !pattern_has_bindings(&binding.pattern) {
            let header = range_header(None, None, iter_expression);
            return (header, self.lower_block_as_body(body));
        }
        let item_var = self.fresh_var(Some("item"));
        let item_identifier = self.scope.generated_identifier(&item_var);
        let header = match yields {
            RangeYield::Channel | RangeYield::Single => {
                range_header(Some(&item_identifier), None, iter_expression)
            }
            RangeYield::Pair => range_header(None, Some(&item_identifier), iter_expression),
        };
        let mut bindings = self.lower_irrefutable_pattern_site(
            PatternSubject::for_identifier(item_identifier.clone()),
            &binding.pattern,
            &binding.ty,
        );
        bindings.extend(self.lower_block_as_body(body).statements);

        let references_item = GoUses::of(&bindings).contains_identifier(&item_identifier);

        let mut statements = Vec::new();
        if !references_item {
            statements.push(discard(GoExpression::identifier(item_identifier)));
        }
        statements.extend(bindings);
        (header, LoweredBlock { statements })
    }

    /// `for i in start..end`.
    fn lower_range_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = iterable
        else {
            unreachable!("lower_range_for requires a Range iterable");
        };

        let mut prologue = Vec::new();
        let (mut start_expression, start_is_observable) = match start {
            Some(start) => {
                let plan = self.plan_operand(start, ExpressionContext::value());
                let is_observable = !plan.can_delay();
                let (setup, value) = plan.into_parts();
                prologue.extend(setup);
                (value, is_observable)
            }
            None => (GoExpression::literal("0".to_string()), false),
        };
        let checkpoint = prologue.len();
        let counts_from_zero = !*inclusive && start_expression.as_literal() == Some("0");
        let end_expression = end.as_ref().map(|end| {
            self.capture_value_at_boundary(
                &mut prologue,
                end,
                "bound",
                // `for i := range n` reads `n` once.
                if counts_from_zero {
                    CaptureBoundary::SiblingSequence
                } else {
                    CaptureBoundary::LoopLifetime
                },
            )
        });
        if prologue.len() > checkpoint
            && start
                .as_ref()
                .is_some_and(|start| start_is_observable && !self.is_unmutated_identifier(start))
        {
            // The end bound's capture inserted statements after the start value;
            // hoist start into its own temp before them so it evaluates first.
            let var = self.fresh_var(Some("start"));
            self.declare(&var);
            prologue.insert(checkpoint, define(var.clone(), start_expression));
            start_expression = GoExpression::name(var);
        }

        let (header, lowered_body) = self.with_scope(|this| {
            let header = match end_expression {
                Some(end_expression) if counts_from_zero => {
                    let loop_var = this.bind_loop_pattern(&binding.pattern, None);
                    range_header(Some(&loop_var), None, end_expression)
                }
                Some(end_expression) => {
                    let loop_var = this.bind_loop_pattern(&binding.pattern, Some("i"));
                    let operator = if *inclusive {
                        BinaryOp::Le
                    } else {
                        BinaryOp::Lt
                    };
                    LoopHeader::Counted {
                        variable: loop_var.clone(),
                        start: start_expression,
                        condition: Some(GoExpression::binary(
                            GoExpression::identifier(loop_var),
                            operator,
                            end_expression,
                        )),
                    }
                }
                None => {
                    let loop_var = this.bind_loop_pattern(&binding.pattern, Some("i"));
                    LoopHeader::Counted {
                        variable: loop_var,
                        start: start_expression,
                        condition: None,
                    }
                }
            };
            (header, this.lower_block_as_body(body))
        });
        (prologue, header, lowered_body)
    }

    fn classify_for<'a>(&self, binding: &'a Binding, iterable: &'a Expression) -> ForShape<'a> {
        if matches!(iterable, Expression::Range { .. }) {
            return ForShape::Range;
        }
        let iterable_ty = iterable.get_type();
        if let Some(range_shape) = self.range_shape(&iterable_ty)
            && matches!(
                range_shape,
                RangeShape::Range | RangeShape::RangeInclusive | RangeShape::RangeFrom
            )
        {
            return ForShape::StoredRange(range_shape);
        }
        if let Some((kind, receiver)) = recognize_string_view_loop(binding, iterable) {
            return ForShape::StringView(kind, receiver);
        }
        if let Pattern::Tuple { elements, .. } = &binding.pattern
            && elements.len() == 2
            && self.is_map_tuple_iterable(&iterable_ty)
        {
            return ForShape::MapTuple;
        }
        let is_channel = self.native_shape(&iterable_ty).is_some_and(|shape| {
            matches!(shape, NativeTypeKind::Channel | NativeTypeKind::Receiver)
        });
        let yields = if is_channel {
            RangeYield::Channel
        } else if self.iter_seq_arity(&iterable_ty) == Some(1) {
            RangeYield::Single
        } else {
            RangeYield::Pair
        };
        ForShape::Iterate {
            yields,
            simple_pattern: matches!(
                &binding.pattern,
                Pattern::Identifier { .. } | Pattern::WildCard { .. }
            ),
        }
    }

    /// Extract a loop variable from a pattern, binding the identifier if present.
    /// `fallback` controls what happens when the pattern is unused or non-identifier:
    /// - `Some(hint)`: generate a fresh var (needed for C-style loops where `_` is invalid)
    /// - `None`: use `"_"` (valid in `for range` syntax)
    fn bind_loop_pattern(&mut self, pattern: &Pattern, fallback: Option<&str>) -> GoIdentifier {
        if let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = pattern
            && let Some(mut go_name) = self.go_name_for_binding(pattern)
        {
            if self.scope.has_binding_for_go_name(&go_name)
                || self.scope.is_go_name_declared(&go_name)
            {
                go_name = self.fresh_var(Some(&go_name));
            }
            return self
                .scope
                .bind_source(identifier, binding.as_slice(), go_name);
        }
        match fallback {
            Some(hint) => {
                let name = self.fresh_var(Some(hint));
                self.scope.generated_identifier(&name)
            }
            None => GoIdentifier::name("_".to_string()),
        }
    }

    fn is_map_tuple_iterable(&self, iterable_ty: &Type) -> bool {
        self.native_shape(iterable_ty).is_some_and(|shape| {
            matches!(shape, NativeTypeKind::Map | NativeTypeKind::EnumeratedSlice)
        }) || self.iter_seq_arity(iterable_ty) == Some(2)
    }

    fn iter_seq_arity(&self, iterable_ty: &Type) -> Option<usize> {
        let Type::Nominal { id, .. } = iterable_ty.strip_refs() else {
            return None;
        };
        match id.as_str() {
            "go:iter.Seq" => Some(1),
            "go:iter.Seq2" => Some(2),
            _ => None,
        }
    }
}

enum ForShape<'a> {
    Range,
    StoredRange(RangeShape),
    StringView(StringViewKind, &'a Expression),
    MapTuple,
    Iterate {
        yields: RangeYield,
        simple_pattern: bool,
    },
}

#[derive(Clone, Copy)]
enum RangeYield {
    Channel,
    Single,
    Pair,
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

    if !receiver.get_type().is_string() {
        return None;
    }

    match member.as_str() {
        "bytes" => Some((StringViewKind::Bytes, receiver.as_ref())),
        "runes" => Some((StringViewKind::Runes, receiver.as_ref())),
        _ => None,
    }
}
