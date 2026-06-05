use crate::EmitEffects;
use crate::Planner;
use crate::calls::CallBoundary;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::LoweredStatement;
use crate::utils::output_references_var;
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

/// Build a `ValuePlan`: `Operand` when there is no setup, otherwise
/// `Composite`.
pub(crate) fn value_plan_from_statements(setup: Vec<LoweredStatement>, value: String) -> ValuePlan {
    if setup.is_empty() {
        ValuePlan::Operand(value)
    } else {
        ValuePlan::Composite { setup, value }
    }
}

/// Wrap a captured setup buffer as `Vec<LoweredStatement>` (one `RawGo`
/// statement, or empty).
pub(crate) fn setup_from_string(setup: String) -> Vec<LoweredStatement> {
    if setup.is_empty() {
        Vec::new()
    } else {
        vec![LoweredStatement::RawGo(setup)]
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

    /// Whether `var` appears as a standalone identifier in the value text or
    /// any setup statement.
    pub(crate) fn references_var(&self, var: &str) -> bool {
        match self {
            ValuePlan::Operand(value) => output_references_var(value, var),
            ValuePlan::Composite { setup, value } => {
                output_references_var(value, var)
                    || setup.iter().any(|statement| statement.references_var(var))
            }
            ValuePlan::Paren(inner) => inner.references_var(var),
            ValuePlan::Cast { go_type, inner } => {
                output_references_var(go_type, var) || inner.references_var(var)
            }
            ValuePlan::Unary { inner, .. } => inner.references_var(var),
        }
    }
}

impl Planner<'_> {
    /// String-emit bridge: capture `emit_value`'s setup buffer as one `RawGo`
    /// statement.
    pub(crate) fn plan_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let mut setup_buffer = String::new();
        let value = self.emit_value(&mut setup_buffer, expression, ctx, fx);
        if setup_buffer.is_empty() {
            ValuePlan::Operand(value)
        } else {
            ValuePlan::Composite {
                setup: vec![LoweredStatement::RawGo(setup_buffer)],
                value,
            }
        }
    }

    /// Plan a value-position expression into a structured `ValuePlan`.
    /// Unconverted leaf kinds bridge through `emit_operand_raw` as `Operand`.
    pub(crate) fn plan_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        match expression {
            Expression::Paren { expression, .. } => {
                ValuePlan::Paren(Box::new(self.plan_operand(expression, ctx, fx)))
            }
            Expression::Cast {
                expression,
                target_type,
                ty,
                ..
            } => self.plan_cast(expression, target_type, ty, ctx, fx),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.plan_index_access(expression, index, fx),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => self.plan_binary(operator, left, right, ctx, fx),
            Expression::Unary {
                operator,
                expression,
                ..
            } => self.plan_unary(operator, expression, ctx, fx),
            Expression::Tuple { elements, ty, .. } => {
                self.plan_tuple_value(elements, ty, false, fx)
            }
            Expression::Range {
                start,
                end,
                inclusive,
                ty,
                ..
            } => self.plan_range_value(start, end, *inclusive, ty, fx),
            Expression::StructCall {
                name,
                field_assignments,
                spread,
                ty,
                ..
            } => self.plan_struct_call(name, field_assignments, spread, ty, ctx, fx),
            Expression::Reference {
                expression: inner,
                ty,
                ..
            } => self.plan_reference(inner, ty, fx),
            Expression::DotAccess { .. } => self.plan_dot_access(expression, ctx, fx),
            Expression::Task {
                expression: inner, ..
            } => self.plan_async_wrapper("go", inner, fx),
            Expression::Defer {
                expression: inner, ..
            } => self.plan_async_wrapper("defer", inner, fx),
            Expression::TryBlock { items, ty, .. } => {
                let (setup, value) = self.lower_try_block(items, ty, fx);
                value_plan_from_statements(setup, value)
            }
            Expression::RecoverBlock { items, ty, .. } => {
                let (setup, value) = self.lower_recover_block(items, ty, fx);
                value_plan_from_statements(setup, value)
            }
            Expression::If { ty, .. } => {
                let (setup, value) = self.plan_if_as_operand_temp(expression, ty, fx);
                value_plan_from_statements(setup, value)
            }
            Expression::Loop { ty, .. } => {
                let (setup, value) = self.plan_loop_as_operand_temp(expression, ty, fx);
                value_plan_from_statements(setup, value)
            }
            Expression::Match { ty, .. } | Expression::Select { ty, .. } if !ty.is_never() => {
                let (setup, value) = self.plan_branching_as_operand_temp(expression, ty, fx);
                value_plan_from_statements(setup, value)
            }
            Expression::Call { ty, .. } => match self.classify_call(expression) {
                CallBoundary::Plain => {
                    let (setup, value) = self.lower_call(expression, Some(ty), ctx, fx);
                    value_plan_from_statements(setup, value)
                }
                CallBoundary::GoWrapped(strategy) => {
                    let (setup, value) = self.lower_go_wrapped_call(expression, &strategy, ty, fx);
                    value_plan_from_statements(setup, value)
                }
                CallBoundary::LoweredCallee(_) => {
                    let mut setup_buffer = String::new();
                    let value = self.emit_operand_raw(&mut setup_buffer, expression, ctx, fx);
                    value_plan_from_statements(setup_from_string(setup_buffer), value)
                }
            },
            _ => {
                let mut setup_buffer = String::new();
                let value = self.emit_operand_raw(&mut setup_buffer, expression, ctx, fx);
                value_plan_from_statements(setup_from_string(setup_buffer), value)
            }
        }
    }
}
