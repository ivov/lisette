use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::calls::predicates::strip_negations;
use crate::lowering::control_flow::targets::legalize_source_loop;
use crate::lowering::expressions::staging::SequenceStage;
use crate::lowering::expressions::staging::is_unmutated_identifier;
use crate::lowering::patterns::binding_decls::pattern_has_bindings;
use crate::lowering::patterns::sites::PatternSubject;
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::ValuePlan;
use crate::lowering::statements::blocks::directed_at;
use crate::target::ir::bodies::{
    ElseArm, GoUses, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock,
    LoweredStatement, Statement, define, discard, with_setup,
};
use crate::target::ir::go_expression::{BinaryOp, UnaryOp};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{CaptureBoundary, ConstantKind, GoExpression};
use crate::target::names::go_name;
use syntax::ast::{Binding, Expression, Literal, Pattern};
use syntax::program::{CallOperation, NativeTypeKind};
use syntax::types::{IterableKind, RangeKind, Type};

fn range_header(
    key: Option<&GoIdentifier>,
    value: Option<&GoIdentifier>,
    iterable: GoExpression,
) -> LoopHeader {
    let named = |name: Option<&GoIdentifier>| name.filter(|name| name.spelling() != "_").cloned();
    LoopHeader::Range {
        key: named(key),
        value: named(value),
        iterable,
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_for_statement(
        &mut self,
        full_expression: &Expression,
    ) -> Statement {
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
        let kind = self.facts.checked(full_expression).iterable_kind();
        let shape = classify_for(&self.facts, binding, iterable, kind);

        let line = self.facts.source_position(&full_expression.get_span());
        let plan = self.with_loop(None, None, |this| {
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
                } => this.lower_iterate_for(binding, iterable, yields, simple_pattern, body),
            };

            with_setup(
                prologue,
                LoweredStatement::Loop(this.build_source_loop(header, lowered_body)),
            )
        });

        Statement { line, kind: plan }
    }

    fn capture_operand_into(
        &mut self,
        prologue: &mut Vec<Statement>,
        expr: &Expression,
    ) -> GoExpression {
        let plan = self.plan_operand(expr, ExpressionContext::value());
        let (setup, value) = plan.into_parts();
        prologue.extend(setup);
        value
    }

    fn lower_stored_range_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        range_shape: RangeKind,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let iterable_ty = iterable.get_type();
            let range_var = if this.facts.is_reference(&iterable_ty) {
                let reference = this.capture_operand_into(&mut prologue, iterable);
                let range = this.facts.dereference_refs(&iterable_ty, reference);
                GoExpression::identifier(this.hoist_temp(&mut prologue, "range", range))
            } else if is_unmutated_identifier(&this.facts, iterable) {
                this.capture_operand_into(&mut prologue, iterable)
            } else {
                this.capture_value_at_boundary(
                    &mut prologue,
                    iterable,
                    "range",
                    CaptureBoundary::Delayed,
                )
            };

            let loop_var = this.bind_loop_pattern(&binding.pattern, Some("i"));
            let bound = |field: &str| GoExpression::selector(range_var.clone(), field.to_string());
            let compare = |operator: BinaryOp| {
                GoExpression::binary(
                    GoExpression::identifier(loop_var.clone()),
                    operator,
                    bound(go_name::RANGE_END_FIELD),
                )
            };

            let condition = match range_shape {
                RangeKind::Range => Some(compare(BinaryOp::Lt)),
                RangeKind::RangeInclusive => Some(compare(BinaryOp::Le)),
                RangeKind::RangeFrom => None,
                RangeKind::RangeTo | RangeKind::RangeToInclusive => {
                    unreachable!("RangeTo/RangeToInclusive are not iterable")
                }
            };

            let header = LoopHeader::Counted {
                variable: loop_var,
                start: bound(go_name::RANGE_START_FIELD),
                condition,
            };
            let lowered_body = this.lower_block_as_body(body);
            (prologue, header, lowered_body)
        })
    }

    fn lower_runes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let receiver_ty = receiver.get_type();
            let receiver = this.capture_operand_into(&mut prologue, receiver);
            let receiver = this.facts.dereference_refs(&receiver_ty, receiver);
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = range_header(None, Some(&loop_var), receiver);
            let lowered_body = this.lower_block_as_body(body);
            (prologue, header, lowered_body)
        })
    }

    fn lower_bytes_for(
        &mut self,
        binding: &Binding,
        receiver: &Expression,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        self.with_scope(|this| {
            let mut prologue = Vec::new();
            let receiver_ty = receiver.get_type();
            let receiver_var = if this.facts.is_reference(&receiver_ty) {
                let reference = this.capture_operand_into(&mut prologue, receiver);
                let value = this.facts.dereference_refs(&receiver_ty, reference);
                GoExpression::identifier(this.hoist_temp(&mut prologue, "s", value))
            } else if is_unmutated_identifier(&this.facts, receiver) {
                this.capture_operand_into(&mut prologue, receiver)
            } else {
                this.capture_value_at_boundary(
                    &mut prologue,
                    receiver,
                    "s",
                    CaptureBoundary::Delayed,
                )
            };

            let index_var = this.fresh_local("i");
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = LoopHeader::Counted {
                variable: index_var.clone(),
                start: GoExpression::literal("0".to_string()),
                condition: Some(GoExpression::binary(
                    GoExpression::identifier(index_var.clone()),
                    BinaryOp::Lt,
                    GoExpression::call(
                        GoExpression::external_name("len".to_string()),
                        vec![receiver_var.clone()],
                    ),
                )),
            };

            let mut lowered_body = this.lower_block_as_body(body);
            if loop_var.spelling() != "_" {
                lowered_body.statements.insert(
                    0,
                    define(
                        loop_var,
                        GoExpression::index(receiver_var, GoExpression::identifier(index_var)),
                    ),
                );
            }
            (prologue, header, lowered_body)
        })
    }

    fn capture_iterable_operand(
        &mut self,
        iterable: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let mut prologue = Vec::new();
        let iter_raw = self.capture_operand_into(&mut prologue, iterable);
        let iter_expression = self.facts.dereference_refs(&iterable.get_type(), iter_raw);
        (prologue, iter_expression)
    }

    fn lower_iterate_for(
        &mut self,
        binding: &Binding,
        iterable: &Expression,
        yields: RangeYield,
        simple_pattern: bool,
        body: &Expression,
    ) -> (Vec<Statement>, LoopHeader, LoweredBlock) {
        let (prologue, iter_expression) = self.capture_iterable_operand(iterable);

        let (header, lowered_body) = self.with_scope(|this| {
            if !simple_pattern {
                return this.lower_pattern_site_body(binding, yields, iter_expression, body);
            }
            let loop_var = this.bind_loop_pattern(&binding.pattern, None);
            let header = match yields {
                RangeYield::Single => range_header(Some(&loop_var), None, iter_expression),
                RangeYield::Pair => range_header(None, Some(&loop_var), iter_expression),
            };
            (header, this.lower_block_as_body(body))
        });

        (prologue, header, lowered_body)
    }

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

        let (prologue, iter_expression) = self.capture_iterable_operand(iterable);

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

    fn lower_map_tuple_simple_body(
        &mut self,
        first: &Pattern,
        second: &Pattern,
        iter_expression: GoExpression,
        body: &Expression,
    ) -> (LoopHeader, LoweredBlock) {
        let first_is_discard = matches!(first, Pattern::WildCard { .. })
            || self.facts.go_name_for_binding(first).is_none();
        let second_is_discard = matches!(second, Pattern::WildCard { .. })
            || self.facts.go_name_for_binding(second).is_none();
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

        let key_var = self.fresh_local("key");
        let value_var = self.fresh_local("value");
        let key_identifier = key_var.clone();
        let value_identifier = value_var.clone();
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
        let item_var = self.fresh_local("item");
        let item_identifier = item_var.clone();
        let header = match yields {
            RangeYield::Single => range_header(Some(&item_identifier), None, iter_expression),
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

        let start_plan = start.as_ref().map_or_else(
            || ValuePlan::constant("0".to_string(), ConstantKind::Int),
            |start| self.plan_operand(start, ExpressionContext::value()),
        );
        let counts_from_zero = !*inclusive && start_plan.expression().as_literal() == Some("0");
        let mut stages = vec![SequenceStage {
            plan: start_plan,
            prefix: "start",
            capture: false,
            line: None,
        }];

        if let Some(end) = end {
            let plan = self.plan_operand(end, ExpressionContext::value());
            let capture = !counts_from_zero && !plan.can_delay();
            stages.push(SequenceStage {
                plan,
                prefix: "bound",
                capture,
                line: None,
            });
        }
        let sequenced = self.sequence_stages(stages, CaptureBoundary::SiblingSequence);
        let prologue = sequenced.setup;
        let mut operands = sequenced.values.into_iter();
        let start_expression = operands.next().expect("range start");
        let end_expression = operands.next();

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

    fn bind_loop_pattern(
        &mut self,
        pattern: &Pattern,
        fallback: Option<&'static str>,
    ) -> GoIdentifier {
        if let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = pattern
            && let Some(go_name) = self.facts.go_name_for_binding(pattern)
        {
            let preferred = go_name.preferred();
            let go_name = if self.scope.has_binding_for_go_name(&preferred)
                || self.shadows_declaration(&preferred)
            {
                self.fresh_local(&go_name)
            } else {
                self.scope.new_local_identifier(preferred)
            };

            return self
                .scope
                .bind_new_local(identifier, binding.as_slice(), go_name);
        }
        match fallback {
            Some(hint) => self.fresh_local(hint),
            None => GoIdentifier::blank(),
        }
    }

    pub(in crate::lowering) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::Loop { body, .. } = expression else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let result = GoExpression::identifier(result_var.clone());
        let plan = self.with_loop(Some(result), Some(ty.clone()), |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        });
        ValuePlan::captured(
            vec![declaration, LoweredStatement::Loop(plan).into()],
            result_var,
        )
    }

    pub(in crate::lowering) fn lower_while_let_statement(
        &mut self,
        expression: &Expression,
    ) -> Statement {
        let Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } = expression
        else {
            unreachable!("lower_while_let_statement requires a WhileLet expression");
        };

        let body = self.with_loop(None, None, |this| {
            this.lower_while_let(pattern, scrutinee, body)
        });
        directed_at(&self.facts, expression, LoweredStatement::Body(body))
    }

    pub(in crate::lowering) fn lower_infinite_loop(&mut self, body: &Expression) -> LoopPlan {
        self.with_loop(None, None, |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        })
    }

    pub(in crate::lowering) fn lower_while(
        &mut self,
        condition: &Expression,
        body: &Expression,
    ) -> LoopPlan {
        self.with_loop(None, None, |this| {
            let (target, polarity) = strip_negations(condition);
            if let Some(exit) = this.try_plan_fused_predicate_value(target, polarity.flip()) {
                let (setup, failure) = exit.into_parts();
                return this.lower_loop_with_exit_test(setup, failure, body);
            }
            let (setup, rendered) = this.plan_condition(condition);
            if !setup.is_empty() {
                let exit = GoExpression::unary(UnaryOp::Not, rendered);
                return this.lower_loop_with_exit_test(setup, exit, body);
            }
            let header = if matches!(
                condition,
                Expression::Literal {
                    literal: Literal::Boolean(true),
                    ..
                }
            ) {
                LoopHeader::Infinite
            } else {
                LoopHeader::While(rendered)
            };

            this.lower_loop_with_header(header, body)
        })
    }

    fn lower_loop_with_exit_test(
        &mut self,
        setup: Vec<Statement>,
        exit: GoExpression,
        body: &Expression,
    ) -> LoopPlan {
        let mut statements = setup;
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                exit,
                LoweredBlock {
                    statements: vec![LoweredStatement::Break(LoopTransfer::Unlabeled).into()],
                },
                ElseArm::None,
            ))
            .into(),
        );
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        statements.extend(lowered_body.statements);
        self.build_source_loop(LoopHeader::Infinite, LoweredBlock { statements })
    }

    pub(in crate::lowering) fn lower_loop_with_header(
        &mut self,
        header: LoopHeader,
        body: &Expression,
    ) -> LoopPlan {
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        self.build_source_loop(header, lowered_body)
    }

    pub(in crate::lowering) fn build_source_loop(
        &mut self,
        header: LoopHeader,
        mut body: LoweredBlock,
    ) -> LoopPlan {
        let target = self
            .scope
            .current_loop_id()
            .expect("source loop plan requires an active loop context");
        let label = legalize_source_loop(&mut body, target);
        LoopPlan {
            kind: LoopKind::Source { label },
            header,
            body,
        }
    }
}

enum ForShape<'a> {
    Range,
    StoredRange(RangeKind),
    StringView(StringViewKind, &'a Expression),
    MapTuple,
    Iterate {
        yields: RangeYield,
        simple_pattern: bool,
    },
}

#[derive(Clone, Copy)]
enum RangeYield {
    Single,
    Pair,
}

#[derive(Clone, Copy)]
enum StringViewKind {
    Bytes,
    Runes,
}

fn classify_for<'a>(
    facts: &EmitFacts<'_>,
    binding: &'a Binding,
    iterable: &'a Expression,
    kind: IterableKind,
) -> ForShape<'a> {
    if matches!(iterable, Expression::Range { .. }) {
        return ForShape::Range;
    }
    if let IterableKind::Range(range) = kind
        && range.has_start()
    {
        return ForShape::StoredRange(range);
    }
    if let Some((kind, receiver)) = recognize_string_view_loop(facts, binding, iterable) {
        return ForShape::StringView(kind, receiver);
    }
    if let Pattern::Tuple { elements, .. } = &binding.pattern
        && elements.len() == 2
        && kind.yields_key_value()
    {
        return ForShape::MapTuple;
    }
    let yields = if matches!(kind, IterableKind::Channel | IterableKind::Seq) {
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

fn recognize_string_view_loop<'a>(
    facts: &EmitFacts<'_>,
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
        ..
    } = expression.as_ref()
    else {
        return None;
    };

    let CallOperation::Native {
        kind: NativeTypeKind::String,
        method,
    } = &facts.checked(expression).function_value()?.operation
    else {
        return None;
    };

    match method.as_str() {
        "bytes" => Some((StringViewKind::Bytes, receiver.as_ref())),
        "runes" => Some((StringViewKind::Runes, receiver.as_ref())),
        _ => None,
    }
}
