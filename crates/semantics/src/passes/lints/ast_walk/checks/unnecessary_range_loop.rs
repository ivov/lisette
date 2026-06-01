use diagnostics::LisetteDiagnostic;
use syntax::ast::{BindingId, Expression};

pub fn check_unnecessary_range_loop(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::For {
        iterable,
        body,
        binding_id: Some(index_id),
        ..
    } = expression
    else {
        return;
    };
    let Expression::Range {
        start: Some(start),
        end: Some(end),
        inclusive: false,
        span,
        ..
    } = iterable.unwrap_parens()
    else {
        return;
    };
    if start.as_integer() != Some(0) {
        return;
    }
    let Some((collection, collection_id)) = length_receiver(end) else {
        return;
    };

    let mut walk = Walk {
        index_id: *index_id,
        collection_id,
        found: false,
        blocked: false,
    };
    walk.visit(body);

    if walk.found && !walk.blocked {
        diagnostics.push(diagnostics::lint::unnecessary_range_loop(span, collection));
    }
}

fn length_receiver(expression: &Expression) -> Option<(&str, BindingId)> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
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
    } = callee.unwrap_parens()
    else {
        return None;
    };
    if member != "length" {
        return None;
    }
    let Expression::Identifier {
        value,
        binding_id: Some(binding_id),
        ..
    } = receiver.unwrap_parens()
    else {
        return None;
    };
    if !receiver.get_type().is_slice() {
        return None;
    }
    Some((value.as_str(), *binding_id))
}

struct Walk {
    index_id: BindingId,
    collection_id: BindingId,
    found: bool,
    blocked: bool,
}

impl Walk {
    fn visit(&mut self, expression: &Expression) {
        if self.blocked {
            return;
        }
        match expression {
            Expression::Assignment { target, value, .. } => {
                if touches_slice_place(target) {
                    self.blocked = true;
                    return;
                }
                self.visit(target);
                self.visit(value);
            }
            Expression::Reference {
                expression: inner, ..
            } => {
                if touches_slice_place(inner) {
                    self.blocked = true;
                    return;
                }
                self.visit(inner);
            }
            Expression::Call { .. }
            | Expression::Function { .. }
            | Expression::Lambda { .. }
            | Expression::Task { .. }
            | Expression::Defer { .. } => {
                self.blocked = true;
            }
            Expression::IndexedAccess {
                expression: receiver,
                index,
                ..
            } => {
                if receiver.binding_id() == Some(self.collection_id)
                    && index.binding_id() == Some(self.index_id)
                {
                    self.found = true;
                    return;
                }
                self.visit(receiver);
                self.visit(index);
            }
            other => {
                if other.binding_id() == Some(self.index_id) {
                    self.blocked = true;
                    return;
                }
                for child in other.children() {
                    self.visit(child);
                }
            }
        }
    }
}

fn touches_slice_place(expression: &Expression) -> bool {
    let expression = expression.unwrap_parens();
    if expression.get_type().is_slice() {
        return true;
    }
    match expression {
        Expression::IndexedAccess { expression, .. } | Expression::DotAccess { expression, .. } => {
            touches_slice_place(expression)
        }
        _ => false,
    }
}
