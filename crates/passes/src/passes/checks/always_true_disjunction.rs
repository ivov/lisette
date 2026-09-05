use crate::passes::comparison::{
    Bound, expressions_equivalent, in_scope_integer_literal_comparison, is_side_effect_free,
    is_skippable_boolean, tighter,
};
use crate::passes::walk::{ClaimKind, NodeCtx};
use syntax::ast::{BinaryOperator, Expression, Span};

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let Expression::Binary {
        operator: BinaryOperator::Or,
        span: root_span,
        ..
    } = expression
    else {
        return;
    };

    if ctx.is_claimed(ClaimKind::AlwaysTrueDisjunction, root_span) {
        return;
    }

    let mut disjuncts = Vec::new();
    collect_disjuncts(expression, root_span, &mut disjuncts, ctx);

    // Falsifications combine only across side-effect-free disjuncts: a disjunct
    // that could mutate state may change the compared operand, ending the run.
    let mut false_sets: Vec<FalseSet> = Vec::new();
    let mut always_true = false;
    for disjunct in disjuncts {
        if !is_side_effect_free(disjunct) {
            always_true |= false_sets.iter().any(|set| set.is_empty());
            false_sets.clear();
            continue;
        }
        if let Some((operand, range, constraint)) = falsifying_constraint(disjunct) {
            match false_sets
                .iter_mut()
                .find(|set| expressions_equivalent(set.operand, operand))
            {
                Some(set) => set.add(constraint),
                None => {
                    let mut set = FalseSet::new(operand, range);
                    set.add(constraint);
                    false_sets.push(set);
                }
            }
        } else if !is_skippable_boolean(disjunct) {
            // Reasoning across an out-of-scope or type-invalid disjunct is unsound.
            always_true |= false_sets.iter().any(|set| set.is_empty());
            false_sets.clear();
        }
    }
    always_true |= false_sets.iter().any(|set| set.is_empty());

    if always_true {
        ctx.sink
            .push(diagnostics::infer::always_true_disjunction(root_span));
    }
}

/// Flattens an `||` chain, claiming nested spans so sub-chains are not re-reported.
fn collect_disjuncts<'a>(
    expression: &'a Expression,
    root_span: &Span,
    disjuncts: &mut Vec<&'a Expression>,
    ctx: &mut NodeCtx,
) {
    match expression.unwrap_parens() {
        Expression::Binary {
            operator: BinaryOperator::Or,
            left,
            right,
            span,
            ..
        } => {
            if span != root_span {
                ctx.claim(ClaimKind::AlwaysTrueDisjunction, *span);
            }
            collect_disjuncts(left, root_span, disjuncts, ctx);
            collect_disjuncts(right, root_span, disjuncts, ctx);
        }
        other => disjuncts.push(other),
    }
}

enum Constraint {
    Bounded {
        low: Option<Bound>,
        high: Option<Bound>,
    },
    Excluded(i128),
}

/// The constraint under which a comparison is false, with its operand and the
/// operand type's value range. Floats are excluded: `NaN` falsifies every
/// ordered comparison at once, so `f < 5 || f >= 5` is not a tautology.
fn falsifying_constraint(
    expression: &Expression,
) -> Option<(&Expression, (i128, i128), Constraint)> {
    use BinaryOperator::*;
    let (operand, operator, bound) = in_scope_integer_literal_comparison(expression)?;
    let range = operand
        .get_type()
        .as_simple()
        .and_then(|kind| kind.integer_range())?;

    // Each arm negates its comparison: `x < b` is false exactly when `x >= b`.
    let constraint = match operator {
        LessThan => Constraint::Bounded {
            low: Some(Bound::new(bound, true)),
            high: None,
        },
        LessThanOrEqual => Constraint::Bounded {
            low: Some(Bound::new(bound, false)),
            high: None,
        },
        GreaterThan => Constraint::Bounded {
            low: None,
            high: Some(Bound::new(bound, true)),
        },
        GreaterThanOrEqual => Constraint::Bounded {
            low: None,
            high: Some(Bound::new(bound, false)),
        },
        Equal => Constraint::Excluded(bound),
        NotEqual => Constraint::Bounded {
            low: Some(Bound::new(bound, true)),
            high: Some(Bound::new(bound, true)),
        },
        _ => return None,
    };

    Some((operand, range, constraint))
}

/// The integer values falsifying every disjunct in a run, seeded with the
/// operand type's range: values outside the domain cannot falsify anything.
struct FalseSet<'a> {
    operand: &'a Expression,
    low: Bound,
    high: Bound,
    excluded: Vec<i128>,
}

impl<'a> FalseSet<'a> {
    fn new(operand: &'a Expression, (min, max): (i128, i128)) -> Self {
        FalseSet {
            operand,
            low: Bound::new(min, true),
            high: Bound::new(max, true),
            excluded: Vec::new(),
        }
    }

    fn add(&mut self, constraint: Constraint) {
        match constraint {
            Constraint::Bounded { low, high } => {
                self.low = tighter(Some(self.low), low, |a, b| a > b).unwrap_or(self.low);
                self.high = tighter(Some(self.high), high, |a, b| a < b).unwrap_or(self.high);
            }
            Constraint::Excluded(value) => self.excluded.push(value),
        }
    }

    // Integer operands, so exclusive bounds shrink to the nearest integer.
    fn is_empty(&self) -> bool {
        let low = if self.low.inclusive {
            self.low.value
        } else {
            self.low.value + 1
        };
        let high = if self.high.inclusive {
            self.high.value
        } else {
            self.high.value - 1
        };
        if low > high {
            return true;
        }
        high - low < self.excluded.len() as i128
            && (low..=high).all(|value| self.excluded.contains(&value))
    }
}
