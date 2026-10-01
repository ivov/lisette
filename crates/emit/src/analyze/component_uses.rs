use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{
    BindingId, Expression, IdentifierResolution, MatchArm, Pattern, collect_pattern_bindings,
};

const STATUS_METHODS: &[&str] = &["is_some", "is_none", "is_ok", "is_err"];
const PAYLOAD_METHODS: &[&str] = &["unwrap_or", "map_or"];

#[derive(Clone)]
pub(crate) struct ComponentDemand {
    pub(crate) needs_value: bool,
    pub(crate) needs_whole_value: bool,
    pub(crate) read_indices: HashSet<usize>,
}

pub(crate) fn component_demand<'a, I>(region: I, binding_id: BindingId) -> Option<ComponentDemand>
where
    I: IntoIterator<Item = &'a Expression>,
{
    let mut walker = Walker {
        binding_id,
        supported: 0,
        blocked: false,
        needs_value: false,
        needs_whole_value: false,
        read_indices: HashSet::default(),
    };
    walker.walk_items(region);
    (walker.supported > 0 && !walker.blocked).then_some(ComponentDemand {
        needs_value: walker.needs_value || walker.needs_whole_value,
        needs_whole_value: walker.needs_whole_value,
        read_indices: walker.read_indices,
    })
}

struct Walker {
    binding_id: BindingId,
    supported: usize,
    blocked: bool,
    needs_value: bool,
    needs_whole_value: bool,
    read_indices: HashSet<usize>,
}

impl Walker {
    fn names_the_local(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => self.binding_id == *id,
            _ => false,
        }
    }

    fn targets_local(&self, target: &Expression) -> bool {
        target.root_binding_id() == Some(self.binding_id)
    }

    fn walk(&mut self, expression: &Expression) {
        if self.blocked {
            return;
        }
        match expression {
            Expression::Identifier { .. } if self.names_the_local(expression) => {
                self.needs_whole_value = true;
                return;
            }
            Expression::Assignment { target, value, .. } => {
                if self.targets_local(target) {
                    self.blocked = true;
                    return;
                }
                self.walk(target);
                self.walk(value);
                return;
            }
            Expression::Reference { expression, .. } if self.names_the_local(expression) => {
                self.blocked = true;
                return;
            }
            Expression::Match { subject, arms, .. } => {
                if self.names_the_local(subject) {
                    if !arms.iter().all(|arm| is_component_arm(&arm.pattern)) {
                        self.blocked = true;
                        return;
                    }
                    self.supported += usize::from(!arms.iter().any(MatchArm::has_guard));
                    self.needs_value |= arms.iter().any(|arm| binds_payload(&arm.pattern));
                    self.needs_whole_value |= arms.iter().any(|arm| arm.has_guard());
                } else {
                    self.walk(subject);
                }
                self.walk_arms(arms);
                return;
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                if self.names_the_local(scrutinee) {
                    if !is_component_arm(pattern) {
                        self.blocked = true;
                        return;
                    }
                    self.supported += 1;
                    self.needs_value |= binds_payload(pattern);
                } else {
                    self.walk(scrutinee);
                }
                self.walk(consequence);
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative);
                }
                return;
            }
            Expression::Propagate { expression, .. } if self.names_the_local(expression) => {
                self.needs_whole_value = true;
                return;
            }
            Expression::Block { items, .. }
            | Expression::TryBlock { items, .. }
            | Expression::RecoverBlock { items, .. } => {
                self.walk_items(items);
                return;
            }
            Expression::WhileLet {
                scrutinee, body, ..
            } => {
                self.walk(scrutinee);
                self.walk(body);
                return;
            }
            Expression::For { iterable, body, .. } => {
                self.walk(iterable);
                self.walk(body);
                return;
            }
            Expression::Lambda { body, .. } => {
                self.walk(body);
                return;
            }
            Expression::Function { body, .. } => {
                if let Some(body) = body.definition() {
                    self.walk(body);
                }
                return;
            }
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } if self.names_the_local(receiver) && member.parse::<usize>().is_ok() => {
                self.supported += 1;
                self.needs_value = true;
                if let Ok(index) = member.parse::<usize>() {
                    self.read_indices.insert(index);
                }
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

    fn walk_items<'e>(&mut self, items: impl IntoIterator<Item = &'e Expression>) {
        let items: Vec<&Expression> = items.into_iter().collect();
        for item in items {
            self.walk(item);
        }
    }

    fn walk_arms(&mut self, arms: &[MatchArm]) {
        for arm in arms {
            if let Some(guard) = &arm.guard {
                self.walk(guard);
            }
            self.walk(&arm.expression);
        }
    }
}

fn is_component_arm(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::WildCard { .. } => true,
        Pattern::EnumVariant {
            identifier,
            fields,
            rest: false,
            ..
        } => {
            let variant = identifier.rsplit('.').next().unwrap_or(identifier);
            matches!(variant, "Some" | "None" | "Ok" | "Err")
                && fields.len() <= 1
                && fields.iter().all(|field| {
                    matches!(field, Pattern::WildCard { .. } | Pattern::Identifier { .. })
                })
        }
        _ => false,
    }
}

fn binds_payload(pattern: &Pattern) -> bool {
    let is_err = matches!(
        pattern,
        Pattern::EnumVariant { identifier, .. } if matches!(identifier.as_str(), "Err" | "Result.Err")
    );
    !is_err && !collect_pattern_bindings(pattern).is_empty()
}
