use crate::Planner;
use crate::calls::CallBoundary;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::LoweredStatement;
use syntax::ast::Expression;

pub(crate) enum ValuePlan {
    /// A Go expression with no setup.
    Operand(String),
    /// A Go expression preceded by setup statements (temp hoists, sequencing).
    Composite {
        setup: Vec<LoweredStatement>,
        value: String,
    },
    /// `(inner)`.
    Paren(Box<ValuePlan>),
    /// `T(inner)` — a primitive/named Go type conversion. Interface-cast
    /// coercions take the bridge path and arrive as `Operand`/`Composite`.
    Cast {
        go_type: String,
        inner: Box<ValuePlan>,
    },
    /// `op inner` — prefix `-`, `^`, or `*`. `!` takes the bridge path.
    Unary {
        op: &'static str,
        inner: Box<ValuePlan>,
    },
}

pub(crate) fn render_unary(op: &str, value: &str) -> String {
    if op == "-" && value.starts_with('-') {
        format!("-({value})")
    } else {
        format!("{op}{value}")
    }
}

/// Build a `ValuePlan`: `Operand` when there is no setup, otherwise
/// `Composite`.
pub(crate) fn value_plan_from_statements(setup: Vec<LoweredStatement>, value: String) -> ValuePlan {
    if setup.is_empty() {
        ValuePlan::Operand(value)
    } else {
        ValuePlan::Composite { setup, value }
    }
}

impl ValuePlan {
    /// Pre-rendered value text for the bridge variants; `None` for structured
    /// variants whose text comes from `render_value`.
    pub(crate) fn operand_text(&self) -> Option<&str> {
        match self {
            ValuePlan::Operand(value) | ValuePlan::Composite { value, .. } => Some(value),
            ValuePlan::Paren(_) | ValuePlan::Cast { .. } | ValuePlan::Unary { .. } => None,
        }
    }

    pub(crate) fn into_parts(self) -> (Vec<LoweredStatement>, String) {
        match self {
            ValuePlan::Operand(value) => (Vec::new(), value),
            ValuePlan::Composite { setup, value } => (setup, value),
            ValuePlan::Paren(inner) => {
                let (setup, value) = inner.into_parts();
                (setup, format!("({value})"))
            }
            ValuePlan::Cast { go_type, inner } => {
                let (setup, value) = inner.into_parts();
                (setup, format!("{go_type}({value})"))
            }
            ValuePlan::Unary { op, inner } => {
                let (setup, value) = inner.into_parts();
                (setup, render_unary(op, &value))
            }
        }
    }
}

impl Planner<'_> {
    pub(crate) fn plan_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let (setup, value) = self.lower_value(expression, ctx).into_parts();
        value_plan_from_statements(setup, value)
    }

    /// Plan a value-position expression into a structured `ValuePlan`. Leaf
    /// kinds route through `plan_operand_leaf`.
    pub(crate) fn plan_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if self.is_test_log_call(expression) {
            let (setup, call) = self.lower_test_log_call(expression);
            return value_plan_from_statements(setup, call);
        }
        match expression {
            Expression::Paren { expression, .. } => {
                ValuePlan::Paren(Box::new(self.plan_operand(expression, ctx)))
            }
            Expression::Cast { expression, ty, .. } => self.plan_cast(expression, ty, ctx),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.plan_index_access(expression, index),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => self.plan_binary(operator, left, right, ctx),
            Expression::Unary {
                operator,
                expression,
                ..
            } => self.plan_unary(operator, expression, ctx),
            Expression::Tuple { elements, ty, .. } => self.plan_tuple_value(elements, ty, false),
            Expression::Range {
                start,
                end,
                inclusive,
                ty,
                ..
            } => self.plan_range_value(start, end, *inclusive, ty),
            Expression::StructCall {
                name,
                field_assignments,
                spread,
                ty,
                ..
            } => self.plan_struct_call(name, field_assignments, spread, ty, ctx),
            Expression::Reference {
                expression: inner,
                ty,
                ..
            } => self.plan_reference(inner, ty),
            Expression::DotAccess { .. } => self.plan_dot_access(expression, ctx),
            Expression::Task {
                expression: inner, ..
            } => self.plan_async_wrapper("go", inner),
            Expression::Defer {
                expression: inner, ..
            } => self.plan_async_wrapper("defer", inner),
            Expression::TryBlock { items, ty, .. } => self.lower_try_block(items, ty),
            Expression::RecoverBlock { items, ty, .. } => self.lower_recover_block(items, ty),
            Expression::Propagate { expression, .. } => {
                let (setup, value) = self.lower_propagate(expression, None);
                value_plan_from_statements(setup, value)
            }
            Expression::If { ty, .. } => self.plan_if_as_operand_temp(expression, ty),
            Expression::Loop { ty, .. } => self.plan_loop_as_operand_temp(expression, ty),
            Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
                if !ty.is_never() =>
            {
                self.plan_branching_as_operand_temp(expression, ty)
            }
            Expression::Call { ty, .. } => match self.classify_call(expression) {
                CallBoundary::Plain => {
                    let (setup, value) = self.lower_call(expression, Some(ty), ctx);
                    value_plan_from_statements(setup, value)
                }
                CallBoundary::GoWrapped(strategy) => {
                    self.lower_go_wrapped_call(expression, &strategy, ty)
                }
                CallBoundary::LoweredCallee(_) => self.plan_operand_leaf(expression, ctx),
            },
            _ => self.plan_operand_leaf(expression, ctx),
        }
    }
}
