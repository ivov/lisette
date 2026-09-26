use syntax::ast::{Expression, collect_pattern_bindings};

const STATUS_METHODS: &[&str] = &["is_some", "is_none", "is_ok", "is_err"];
const PAYLOAD_METHODS: &[&str] = &["unwrap_or", "map_or"];

pub(crate) struct ComponentDemand {
    pub(crate) needs_value: bool,
}

pub(crate) fn component_demand<'a, I>(region: I, lisette_name: &str) -> Option<ComponentDemand>
where
    I: IntoIterator<Item = &'a Expression>,
{
    let mut walker = Walker {
        name: lisette_name,
        supported: 0,
        blocked: false,
        needs_value: false,
    };
    for tree in region {
        walker.walk(tree);
    }
    (walker.supported > 0 && !walker.blocked).then_some(ComponentDemand {
        needs_value: walker.needs_value,
    })
}

struct Walker<'a> {
    name: &'a str,
    supported: usize,
    blocked: bool,
    needs_value: bool,
}

impl Walker<'_> {
    fn names_the_local(&self, expression: &Expression) -> bool {
        matches!(
            expression.unwrap_parens(),
            Expression::Identifier { value, .. } if value == self.name
        )
    }

    fn walk(&mut self, expression: &Expression) {
        if self.blocked {
            return;
        }
        match expression {
            Expression::Identifier { value, .. } if value == self.name => {
                self.blocked = true;
                return;
            }
            Expression::Match { subject, arms, .. } if self.names_the_local(subject) => {
                self.supported += 1;
                self.needs_value |= arms
                    .iter()
                    .any(|arm| !collect_pattern_bindings(&arm.pattern).is_empty());
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.walk(guard);
                    }
                    self.walk(&arm.expression);
                }
                return;
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } if self.names_the_local(scrutinee) => {
                self.supported += 1;
                self.needs_value |= !collect_pattern_bindings(pattern).is_empty();
                self.walk(consequence);
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative);
                }
                return;
            }
            Expression::Propagate { expression, .. } if self.names_the_local(expression) => {
                self.supported += 1;
                self.needs_value = true;
                return;
            }
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                if let Expression::DotAccess {
                    expression: receiver,
                    member,
                    ..
                } = callee.unwrap_parens()
                    && self.names_the_local(receiver)
                    && (STATUS_METHODS.contains(&member.as_str())
                        || PAYLOAD_METHODS.contains(&member.as_str()))
                {
                    self.supported += 1;
                    self.needs_value |= PAYLOAD_METHODS.contains(&member.as_str());
                    for argument in args {
                        self.walk(argument);
                    }
                    return;
                }
            }
            _ => {}
        }
        for child in expression.children() {
            self.walk(child);
        }
    }
}
