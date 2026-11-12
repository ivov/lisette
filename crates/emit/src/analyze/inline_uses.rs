use std::slice;

use syntax::ast::{BindingId, Expression, FormatStringPart, Literal, SelectArm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineDecision {
    Inline,
    Unused,
    Keep,
}

pub(crate) fn analyze_inline_candidate_ids(
    binding_ids: &[BindingId],
    consumers: &[&Expression],
) -> InlineDecision {
    if binding_ids.is_empty() {
        return InlineDecision::Keep;
    }
    let mut walker = Walker::new(binding_ids);
    for consumer in consumers {
        walker.walk(consumer, AccessRole::Read);
    }
    walker.decision
}

pub(crate) fn region_blocks_inline<'a, I>(trees: I, binding_id: Option<BindingId>) -> bool
where
    I: IntoIterator<Item = &'a Expression>,
{
    let Some(binding_id) = binding_id else {
        return true;
    };
    let mut walker = Walker::new(slice::from_ref(&binding_id));
    for tree in trees {
        walker.walk(tree, AccessRole::Read);
    }
    walker.decision != InlineDecision::Unused
}

struct Walker<'a> {
    binding_ids: &'a [BindingId],
    crossed_barrier: bool,
    decision: InlineDecision,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AccessRole {
    #[default]
    Read,
    Write,
    Address,
    Capture,
}

impl<'a> Walker<'a> {
    fn new(binding_ids: &'a [BindingId]) -> Self {
        Self {
            binding_ids,
            crossed_barrier: false,
            decision: InlineDecision::Unused,
        }
    }

    fn record_use(&mut self, role: AccessRole) {
        self.decision = match (self.decision, role, self.crossed_barrier) {
            (InlineDecision::Unused, AccessRole::Read, false) => InlineDecision::Inline,
            _ => InlineDecision::Keep,
        };
    }

    fn walk(&mut self, expression: &Expression, role: AccessRole) {
        match expression {
            Expression::Identifier { resolution, .. } => {
                let names_local = resolution
                    .binding_id()
                    .is_some_and(|id| self.binding_ids.contains(&id));
                if names_local {
                    self.record_use(role);
                }
            }
            Expression::Literal { literal, .. } => {
                if let Literal::FormatString(parts) = literal {
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            self.walk(expression, role);
                        }
                    }
                    self.crossed_barrier = true;
                }
            }

            Expression::Call { .. } | Expression::Propagate { .. } => {
                for child in expression.children() {
                    self.walk(child, role);
                }
                self.crossed_barrier = true;
            }
            Expression::Assignment { target, value, .. } => {
                self.walk(target, AccessRole::Write);
                self.walk(value, role);
                self.crossed_barrier = true;
            }
            Expression::Reference { expression, .. } => {
                self.walk(expression, AccessRole::Address);
            }

            Expression::Block { items, .. } => {
                self.walk_block(items, role);
            }
            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.walk(scrutinee, role);
                self.walk(consequence, role);
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative, role);
                }
            }
            Expression::Match { subject, arms, .. } => {
                self.walk(subject, role);
                for arm in arms {
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, role);
                    }
                    self.walk(&arm.expression, role);
                }
            }

            Expression::StructCall {
                field_assignments, ..
            } => {
                for fa in field_assignments {
                    self.walk(&fa.value, role);
                }
            }

            Expression::Loop { body, .. } => self.walk(body, AccessRole::Capture),
            Expression::While {
                condition, body, ..
            } => {
                let role = AccessRole::Capture;
                self.walk(condition, role);
                self.walk(body, role);
            }
            Expression::WhileLet {
                scrutinee, body, ..
            } => {
                let role = AccessRole::Capture;
                self.walk(scrutinee, role);
                self.walk(body, role);
            }
            Expression::For { iterable, body, .. } => {
                self.walk(iterable, role);
                self.walk(body, AccessRole::Capture);
            }

            Expression::Lambda { body, .. } => self.walk(body, AccessRole::Capture),
            Expression::Function { body, .. } => {
                if let Some(body) = body.definition() {
                    self.walk(body, AccessRole::Capture);
                }
            }
            Expression::Task { expression, .. } | Expression::Defer { expression, .. } => {
                self.walk(expression, AccessRole::Capture);
                self.crossed_barrier = true;
            }

            Expression::Select { arms, .. } => {
                // Mark the barrier before walking arms so uses inside any arm
                // see the select wait as preceding.
                self.crossed_barrier = true;
                for arm in arms {
                    self.walk_select_arm(arm, role);
                }
            }
            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                self.walk_block(items, role);
                self.crossed_barrier = true;
            }
            Expression::RawGo { .. } => {
                self.decision = InlineDecision::Keep;
                self.crossed_barrier = true;
            }

            Expression::Interface { .. } => {}
            _ => {
                for child in expression.children() {
                    self.walk(child, role);
                }
            }
        }
    }

    fn walk_block(&mut self, items: &[Expression], role: AccessRole) {
        for item in items {
            self.walk(item, role);
        }
    }

    fn walk_select_arm(&mut self, pattern: &SelectArm, role: AccessRole) {
        match pattern {
            SelectArm::Receive {
                receive_expression,
                body,
                ..
            } => {
                self.walk(receive_expression, role);
                self.walk(body, AccessRole::Capture);
            }
            SelectArm::Send {
                send_expression,
                body,
            } => {
                self.walk(send_expression, role);
                self.walk(body, AccessRole::Capture);
            }
            SelectArm::MatchReceive {
                receive_expression,
                arms,
            } => {
                self.walk(receive_expression, role);
                let role = AccessRole::Capture;
                for arm in arms {
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, role);
                    }
                    self.walk(&arm.expression, role);
                }
            }
            SelectArm::WildCard { body } => {
                self.walk(body, AccessRole::Capture);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::{IdentifierResolution, Span};
    use syntax::types::Type;

    fn mark_uses(expression: &mut Expression, selected: &[usize], seen: &mut usize) {
        match expression {
            Expression::Identifier {
                value, resolution, ..
            } if value == "value" => {
                if selected.contains(seen) {
                    *resolution = IdentifierResolution::Binding(BindingId::new(1));
                }
                *seen += 1;
            }
            Expression::Block { items, .. }
            | Expression::Tuple {
                elements: items, ..
            } => {
                for item in items {
                    mark_uses(item, selected, seen);
                }
            }
            Expression::Let { value, .. } => mark_uses(value, selected, seen),
            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                mark_uses(expression, selected, seen);
                for arg in args {
                    mark_uses(arg, selected, seen);
                }
                if let Some(spread) = spread {
                    mark_uses(spread, selected, seen);
                }
            }
            Expression::Literal {
                literal: Literal::FormatString(parts),
                ..
            } => {
                for part in parts {
                    if let FormatStringPart::Expression(expression) = part {
                        mark_uses(expression, selected, seen);
                    }
                }
            }
            Expression::Literal {
                literal: Literal::Slice(items),
                ..
            } => {
                for item in items {
                    mark_uses(item, selected, seen);
                }
            }
            Expression::Paren { expression, .. } => mark_uses(expression, selected, seen),
            Expression::Const { .. }
            | Expression::StructCall { .. }
            | Expression::Identifier { .. }
            | Expression::Literal { .. } => {}
            other => panic!("test helper does not handle {other:?}"),
        }
    }

    fn inline_decision(source: &str, selected: &[usize]) -> InlineDecision {
        let mut parsed = syntax::build_ast(&format!("fn test() {{ {source} }}"), 0);
        assert!(!parsed.has_errors(), "{:?}", parsed.errors);
        let Expression::Function { body, .. } = &mut parsed.ast[0] else {
            panic!("expected a function");
        };
        let body = body.definition_mut().unwrap();
        mark_uses(body, selected, &mut 0);
        analyze_inline_candidate_ids(&[BindingId::new(1)], &[body])
    }

    #[test]
    fn a_second_read_keeps_the_binding_even_before_any_barrier() {
        assert_eq!(
            inline_decision("(value, value)", &[0, 1]),
            InlineDecision::Keep
        );
    }

    #[test]
    fn a_barrier_after_the_only_read_does_not_prevent_inlining() {
        assert_eq!(
            inline_decision("value; later()", &[0]),
            InlineDecision::Inline
        );
        assert_eq!(inline_decision("later()", &[]), InlineDecision::Unused);
    }

    #[test]
    fn alternative_ids_share_one_use_count() {
        let local = |id| Expression::Identifier {
            value: "value".into(),
            ty: Type::unit(),
            span: Span::dummy(),
            resolution: IdentifierResolution::Binding(BindingId::new(id)),
        };
        let first = local(1);
        let second = local(2);
        let ids = [BindingId::new(1), BindingId::new(2)];
        assert_eq!(
            analyze_inline_candidate_ids(&ids, &[&first]),
            InlineDecision::Inline
        );
        assert_eq!(
            analyze_inline_candidate_ids(&ids, &[&second]),
            InlineDecision::Inline
        );
        assert_eq!(
            analyze_inline_candidate_ids(&ids, &[&first, &second]),
            InlineDecision::Keep
        );
        assert_eq!(
            analyze_inline_candidate_ids(&[], &[&first]),
            InlineDecision::Keep
        );
    }

    #[test]
    fn raw_go_keeps_even_an_unread_binding() {
        let raw = Expression::RawGo {
            text: "use(value)".to_string(),
        };
        assert_eq!(
            analyze_inline_candidate_ids(&[BindingId::new(1)], &[&raw]),
            InlineDecision::Keep
        );
        assert!(region_blocks_inline([&raw], Some(BindingId::new(1))));
    }

    #[test]
    fn call_argument_can_inline_before_a_later_spread_call() {
        assert_eq!(
            inline_decision("consume(value, later()...)", &[0]),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn spread_use_stays_bound_after_an_earlier_argument_call() {
        assert_eq!(
            inline_decision("consume(earlier(), value...)", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn tuple_use_stays_bound_after_an_earlier_element_call() {
        assert_eq!(
            inline_decision("(earlier(), value)", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn format_string_blocks_a_later_use() {
        assert_eq!(
            inline_decision("f\"{other}\"\nvalue", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn shadowing_starts_after_the_let_initializer() {
        assert_eq!(
            inline_decision("let value = value\nvalue", &[0]),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn block_constant_shadows_even_a_preceding_use() {
        assert_eq!(
            inline_decision("value\nconst value = 1", &[]),
            InlineDecision::Unused,
        );
    }

    #[test]
    fn slice_literal_contents_do_not_count_as_inline_uses() {
        assert_eq!(inline_decision("[value]", &[0]), InlineDecision::Unused);
    }

    #[test]
    fn struct_spread_does_not_count_as_an_inline_use() {
        assert_eq!(
            inline_decision("Record { field: other, ..value }", &[]),
            InlineDecision::Unused,
        );
    }
}
