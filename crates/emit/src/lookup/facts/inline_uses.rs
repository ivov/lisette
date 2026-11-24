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
            SelectArm::WildCard { body, .. } => {
                self.walk(body, AccessRole::Capture);
            }
        }
    }
}
