use crate::passes::comparison::{
    Bound, expressions_equivalent, flip_comparison, in_scope_comparison, is_side_effect_free,
    signed_integer_literal, tighter,
};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Span, UnaryOperator};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator: BinaryOperator::And,
        span: root_span,
        ..
    } = expression
    else {
        return;
    };

    // A nested `&&` is covered by the outermost chain that encloses it.
    if ctx.claimed_spans.borrow().contains(root_span) {
        return;
    }

    let mut conjuncts = Vec::new();
    collect_conjuncts(expression, root_span, &mut conjuncts, ctx);

    // Constraints combine only across side-effect-free conjuncts: a conjunct
    // that could mutate state may change the compared operand, ending the run.
    let mut groups: Vec<Group> = Vec::new();
    let mut impossible = false;
    for conjunct in conjuncts {
        if !is_side_effect_free(conjunct) {
            impossible |= groups.iter().any(|group| group.is_unsatisfiable());
            groups.clear();
            continue;
        }
        if let Some((operand, constraint)) = comparison_constraint(conjunct) {
            match groups
                .iter_mut()
                .find(|group| expressions_equivalent(group.operand, operand))
            {
                Some(group) => group.add(constraint),
                None => {
                    let mut group = Group::new(operand);
                    group.add(constraint);
                    groups.push(group);
                }
            }
        } else if !is_skippable_boolean(conjunct) {
            // Barrier on any conjunct not positively recognized as a value-stable
            // boolean: reasoning across an out-of-scope or type-invalid one (e.g.
            // `s < 5`, or an invalid comparison buried in `s < 5 || flag`) is unsound.
            impossible |= groups.iter().any(|group| group.is_unsatisfiable());
            groups.clear();
        }
    }
    impossible |= groups.iter().any(|group| group.is_unsatisfiable());

    if impossible {
        ctx.sink
            .push(diagnostics::infer::impossible_comparison(root_span));
    }
}

/// Flattens an `&&` chain into its conjuncts, claiming every nested `&&` span so
/// the walk does not also report a sub-chain.
fn collect_conjuncts<'a>(
    expression: &'a Expression,
    root_span: &Span,
    conjuncts: &mut Vec<&'a Expression>,
    ctx: &NodeCtx,
) {
    match expression.unwrap_parens() {
        Expression::Binary {
            operator: BinaryOperator::And,
            left,
            right,
            span,
            ..
        } => {
            if span != root_span {
                ctx.claimed_spans.borrow_mut().insert(*span);
            }
            collect_conjuncts(left, root_span, conjuncts, ctx);
            collect_conjuncts(right, root_span, conjuncts, ctx);
        }
        other => conjuncts.push(other),
    }
}

/// Whether `expression` is a value-stable boolean safe to skip in the chain: a
/// boolean identifier, field read, literal, or `!` of one. These cannot be a
/// rejected comparison, so a run of constraints may carry across them.
fn is_skippable_boolean(expression: &Expression) -> bool {
    if !expression.get_type().is_boolean() {
        return false;
    }
    match expression.unwrap_parens() {
        Expression::Identifier { .. }
        | Expression::DotAccess { .. }
        | Expression::Literal { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Not,
            expression: inner,
            ..
        } => is_skippable_boolean(inner),
        _ => false,
    }
}

enum Constraint {
    Bounded { low: Bound, high: Bound },
    Excluded(i128),
}

/// The constraint a `variable OP integer-literal` comparison puts on its operand,
/// paired with that operand. `None` for anything out of the integer scope.
fn comparison_constraint(expression: &Expression) -> Option<(&Expression, Constraint)> {
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };

    use BinaryOperator::*;
    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    if !in_scope_comparison(left, right) {
        return None;
    }
    let (operand, operator, bound) =
        match (signed_integer_literal(left), signed_integer_literal(right)) {
            (None, Some(bound)) => (left, *operator, bound),
            (Some(bound), None) => (right, flip_comparison(*operator), bound),
            _ => return None,
        };

    let constraint = match operator {
        LessThan => Constraint::Bounded {
            low: None,
            high: Some((bound, false)),
        },
        LessThanOrEqual => Constraint::Bounded {
            low: None,
            high: Some((bound, true)),
        },
        GreaterThan => Constraint::Bounded {
            low: Some((bound, false)),
            high: None,
        },
        GreaterThanOrEqual => Constraint::Bounded {
            low: Some((bound, true)),
            high: None,
        },
        Equal => Constraint::Bounded {
            low: Some((bound, true)),
            high: Some((bound, true)),
        },
        NotEqual => Constraint::Excluded(bound),
        _ => return None,
    };

    Some((operand, constraint))
}

/// The integer constraints on one operand, accumulated across the chain.
struct Group<'a> {
    operand: &'a Expression,
    low: Bound,
    high: Bound,
    excluded: Vec<i128>,
}

impl<'a> Group<'a> {
    fn new(operand: &'a Expression) -> Self {
        Group {
            operand,
            low: None,
            high: None,
            excluded: Vec::new(),
        }
    }

    fn add(&mut self, constraint: Constraint) {
        match constraint {
            Constraint::Bounded { low, high } => {
                self.low = tighter(self.low, low, |a, b| a > b);
                self.high = tighter(self.high, high, |a, b| a < b);
            }
            Constraint::Excluded(value) => self.excluded.push(value),
        }
    }

    // Bounds are a continuous interval: `x > 0 && x < 1` (empty only for
    // integers) is deliberately not flagged, since it holds for `float64`.
    fn is_unsatisfiable(&self) -> bool {
        let (Some((low, low_inclusive)), Some((high, high_inclusive))) = (self.low, self.high)
        else {
            return false;
        };
        if low > high {
            return true;
        }
        if low == high {
            // Single point `low`: empty if either side excludes it or a `!=` rules it out.
            return !(low_inclusive && high_inclusive) || self.excluded.contains(&low);
        }
        false
    }
}
