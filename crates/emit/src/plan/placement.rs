use crate::Planner;
use crate::analyze::inline_uses::region_blocks_inline;
use crate::calls::native::{clip_shared_capacity, is_clip_safe_path};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::definitions::functions::{is_breakless_loop, is_go_never};
use crate::expressions::staging::SpreadSequenceOptions;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::plan::bodies::{
    AssignForm, BreakValueAction, BreakValuePlan, ElseArm, LoopHeader, LoopTransfer, LoweredBlock,
    LoweredStatement, PlacePlan, define, discard, expression_statement,
};
use crate::plan::calls::plan_variadic_spread;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{
    CaptureBoundary, ConstantKind, EvaluationEffect, GoExpression, ValuePlan,
};
use crate::statements::assignments::is_lvalue_chain;
use crate::types::native::NativeGoType;
use std::slice;
use syntax::ast::Pattern;
use syntax::ast::{Expression, Literal};
use syntax::types::Type;

/// Append `panic("unreachable")` after a branch construct in return position
/// when the branch can fall through (no exhaustive default arm). Go would
/// otherwise reject the function for missing a tail return.
pub(crate) fn unreachable_panic_if_needed(
    place: &PlacePlan,
    is_exhaustive: bool,
) -> Option<LoweredStatement> {
    (place.is_return() && !is_exhaustive).then_some(LoweredStatement::UnreachablePanic)
}

/// True when discarding `expression` is safe to omit: its value has no
/// side effects. `FormatString` and `Slice` literals are excluded since they
/// can hold sub-expressions that do.
fn is_side_effect_free_discard(expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        Expression::Literal { literal, .. } => matches!(
            literal,
            Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_)
        ),
        _ => false,
    }
}

pub(crate) fn is_unit_call(expression: &Expression) -> bool {
    expression.get_type().is_unit() && matches!(expression.unwrap_parens(), Expression::Call { .. })
}

/// A `target = value` assignment with no lvalue capture.
pub(crate) fn simple_assign(target: &GoExpression, value: ValuePlan) -> LoweredStatement {
    LoweredStatement::Assign(AssignForm::Simple {
        target_capture: Vec::new(),
        target: target.clone(),
        value,
    })
}

/// Bind the setup's trailing temp under `name` instead of copying it.
pub(crate) fn rebind_trailing_temp(
    statements: &mut [LoweredStatement],
    name: &str,
    temp: &str,
) -> bool {
    statements
        .last_mut()
        .is_some_and(|statement| statement.binds_name(temp) && statement.rename_bound_name(name))
}

/// Collapse `var x T` plus the one statement that fills it into `x := value`.
pub(crate) fn collapse_declared_temp(
    statements: &mut Vec<LoweredStatement>,
    name: &str,
    value_has_declared_type: bool,
) {
    let [
        LoweredStatement::VarDecl {
            name: declared,
            go_type,
            value: None,
        },
        filler,
    ] = statements.as_slice()
    else {
        return;
    };
    if declared != name {
        return;
    }
    let value = match filler {
        LoweredStatement::Assign(AssignForm::Simple {
            target_capture,
            target,
            value,
        }) => {
            if !infers_declared_type(go_type, &value.expression, value_has_declared_type)
                || target.as_str() != name
                || !target_capture.is_empty()
                || !value.setup.is_empty()
                || value.expression.does_work()
            {
                return;
            }
            value.expression.clone()
        }
        LoweredStatement::If(plan) => {
            if go_type != "bool" || !plan.condition_setup.is_empty() || plan.initializer.is_some() {
                return;
            }
            let ElseArm::Else {
                body: else_body, ..
            } = &plan.else_arm
            else {
                return;
            };
            let Some(then_value) = single_simple_assign_value(&plan.then_body, name) else {
                return;
            };
            let Some(else_value) = single_simple_assign_value(else_body, name) else {
                return;
            };
            let Some(joined) = join_boolean_branches(&plan.condition, &then_value, &else_value)
            else {
                return;
            };
            joined
        }
        _ => return,
    };
    statements.pop();
    statements[0] = define(name.to_string(), value);
}

fn infers_declared_type(
    go_type: &str,
    value: &GoExpression,
    value_has_declared_type: bool,
) -> bool {
    if let Some(kind) = value.constant_kind() {
        return match kind {
            ConstantKind::Int => go_type == "int",
            ConstantKind::Rune => matches!(go_type, "rune" | "int32"),
            ConstantKind::Float => go_type == "float64",
            ConstantKind::Complex => go_type == "complex128",
            ConstantKind::Bool => go_type == "bool",
            ConstantKind::String => go_type == "string",
        };
    }
    match value.node() {
        GoExpressionNode::Literal(text) => {
            matches!(go_type, "int" | "string" | "bool") && text != "nil"
        }
        GoExpressionNode::CompositeLiteral {
            go_type: Some(literal_type),
            ..
        } => literal_type == go_type,
        GoExpressionNode::CompositeLiteral { go_type: None, .. }
        | GoExpressionNode::FunctionLiteral { .. }
        | GoExpressionNode::Verbatim(_) => false,
        GoExpressionNode::Binary { operator, .. } => match operator.as_str() {
            "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" => go_type == "bool",
            "<<" | ">>" => go_type == "int",
            _ => value_has_declared_type,
        },
        GoExpressionNode::Unary { operator, .. } if operator == "!" => go_type == "bool",
        _ => value_has_declared_type && !go_type.contains("chan"),
    }
}

/// The value of a body that is exactly one plain `name = value`.
fn single_simple_assign_value(body: &LoweredBlock, name: &str) -> Option<GoExpression> {
    let [LoweredStatement::Assign(assign)] = body.statements.as_slice() else {
        return None;
    };
    let AssignForm::Simple {
        target_capture,
        target,
        value,
    } = assign
    else {
        return None;
    };
    (target.as_str() == name
        && target_capture.is_empty()
        && value.setup.is_empty()
        && !value.expression.does_work())
    .then(|| value.expression.clone())
}

fn join_boolean_branches(
    condition: &GoExpression,
    then_value: &GoExpression,
    else_value: &GoExpression,
) -> Option<GoExpression> {
    let and = |left: GoExpression, right: GoExpression| GoExpression::binary(left, "&&", right);
    let or = |left: GoExpression, right: GoExpression| GoExpression::binary(left, "||", right);
    let not = |operand: &GoExpression| GoExpression::unary("!", operand.clone());
    Some(match (then_value.as_str(), else_value.as_str()) {
        ("true", "false") => condition.clone(),
        ("false", "true") => not(condition),
        // Both-literal same-value arms would drop the condition's evaluation.
        ("true", "true") | ("false", "false") => return None,
        (_, "false") => and(condition.clone(), then_value.clone()),
        ("true", _) => or(condition.clone(), else_value.clone()),
        ("false", _) => and(not(condition), else_value.clone()),
        (_, "true") => or(not(condition), then_value.clone()),
        _ => return None,
    })
}

pub(crate) fn requires_temp_var(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::Select { .. }
    )
}

/// Match `...; let X = <CF>; X` so the caller can emit `<CF>` directly into
/// the surrounding place, skipping the `X` temp.
pub(crate) fn try_elide_tail_let(items: &[Expression]) -> Option<(&Expression, &[Expression])> {
    if items.len() < 2 {
        return None;
    }
    let last = items.last()?;
    let Expression::Identifier {
        value: tail_name, ..
    } = last
    else {
        return None;
    };
    let penultimate = &items[items.len() - 2];
    let Expression::Let {
        binding,
        value,
        mode,
        ..
    } = penultimate
    else {
        return None;
    };
    if mode.else_block().is_some() || binding.is_mutable() {
        return None;
    }
    let Pattern::Identifier { identifier, .. } = &binding.pattern else {
        return None;
    };
    if identifier != tail_name {
        return None;
    }
    // Only `If`, `IfLet`, and `Match` can be re-emitted at the surrounding place
    // via branch lowering (`lower_branching_to_block`); other shapes still stage
    // through temps so eliding the let would not save anything.
    if !matches!(
        value.as_ref(),
        Expression::If { .. } | Expression::IfLet { .. } | Expression::Match { .. }
    ) {
        return None;
    }
    let rest = &items[..items.len() - 2];
    if region_blocks_inline(rest.iter(), tail_name.as_str()) {
        return None;
    }
    Some((value.as_ref(), rest))
}

pub(crate) fn expression_contains_binding(expression: &Expression, name: &str) -> bool {
    use syntax::ast::SelectArm;
    match expression {
        Expression::IfLet {
            pattern,
            consequence,
            alternative,
            ..
        } => {
            pattern_binds_name(pattern, name)
                || expression_contains_binding(consequence, name)
                || alternative
                    .expression()
                    .is_some_and(|alternative| expression_contains_binding(alternative, name))
        }
        Expression::Match { arms, .. } => arms
            .iter()
            .any(|arm| pattern_binds_name(&arm.pattern, name)),
        Expression::Block { items, .. } => items.iter().any(|item| match item {
            Expression::Let { binding, .. } => pattern_binds_name(&binding.pattern, name),
            _ => false,
        }),
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            expression_contains_binding(consequence, name)
                || alternative
                    .as_deref()
                    .is_some_and(|alternative| expression_contains_binding(alternative, name))
        }
        Expression::Select { arms, .. } => arms.iter().any(|arm| match arm {
            SelectArm::Receive { binding, .. } => pattern_binds_name(binding, name),
            SelectArm::MatchReceive { arms, .. } => {
                arms.iter().any(|a| pattern_binds_name(&a.pattern, name))
            }
            _ => false,
        }),
        Expression::Loop { body, .. } => expression_contains_binding(body, name),
        _ => false,
    }
}

impl Planner<'_> {
    pub(crate) fn short_declaration_keeps_type(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        !self.facts.is_interface_or_unknown(&peeled)
            && !(matches!(peeled, Type::Nominal { .. })
                && self.facts.underlying_simple_kind(&peeled).is_some())
    }

    /// Lower a discarded expression into structured statements: a bare
    /// side-effecting call (`f()`), a `_ = value` discard, or a propagate.
    pub(crate) fn lower_discard_value(&mut self, value: &Expression) -> Vec<LoweredStatement> {
        let unwrapped = value.unwrap_parens();

        if is_side_effect_free_discard(unwrapped) {
            return Vec::new();
        }

        if let Expression::Propagate { expression, .. } = unwrapped {
            return self.lower_propagate(expression, Some("_")).0;
        }

        let value_ty = value.get_type();
        if value_ty.is_unit()
            || value_ty.is_variable()
            || value_ty.is_placeholder()
            || value_ty.is_never()
        {
            let staged = self.plan_operand(value, ExpressionContext::value());
            let (mut statements, staged_value) = staged.into_parts();
            if !staged_value.is_empty() {
                if matches!(unwrapped, Expression::Call { .. }) {
                    // A never-typed call (e.g. `panic(...)`) diverges.
                    statements.push(LoweredStatement::ExpressionStatement {
                        expression: staged_value,
                        diverges: value_ty.is_never(),
                    });
                } else {
                    statements.push(discard(staged_value));
                }
            }
            return statements;
        }

        if let Expression::Call { .. } = unwrapped {
            let mut statements: Vec<LoweredStatement> = Vec::new();
            if let Some(call) = self.emit_go_call_discarded(&mut statements, unwrapped) {
                statements.push(expression_statement(call));
                return statements;
            }
        }

        let staged = self.plan_operand(value, ExpressionContext::value());
        let (mut statements, staged_value) = staged.into_parts();
        statements.push(discard(staged_value));
        statements
    }

    /// Emit a unit-typed call as a statement, then store `struct{}{}` into
    /// `target`.
    fn lower_unit_call_into_var(
        &mut self,
        value: &Expression,
        target: &GoExpression,
    ) -> Vec<LoweredStatement> {
        let (mut statements, call) = self
            .lower_value(value, ExpressionContext::value())
            .into_parts();
        if !call.is_empty() {
            statements.push(expression_statement(call));
        }
        statements.push(simple_assign(
            target,
            ValuePlan::computed(
                Vec::new(),
                GoExpression::empty_composite("struct{}".to_string()),
                EvaluationEffect::Pure,
            ),
        ));
        statements
    }

    pub(crate) fn lower_assign(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<LoweredStatement> {
        let ty = expression.get_type();
        let is_fallible = ty.is_result() || ty.is_option();
        if is_fallible {
            return self.lower_option_result_assignment(target, target_ty, expression);
        }

        if let Expression::Loop { body, .. } = expression {
            let plan = self.with_loop(target.as_str(), |this| {
                this.lower_loop_with_header(LoopHeader::Infinite, body)
            });
            return vec![LoweredStatement::Loop(plan)];
        }

        if let Expression::Block { items, .. } = expression
            && items.len() > 1
        {
            let statements = self.lower_block_to_var(expression, target, target_ty, true);
            return vec![LoweredStatement::Block(LoweredBlock { statements })];
        }

        self.lower_block_to_var(expression, target, target_ty, false)
    }

    fn lower_plain_assign(
        &mut self,
        target: &GoExpression,
        expression: &Expression,
    ) -> Vec<LoweredStatement> {
        let value = self.plan_operand(expression, ExpressionContext::value());
        vec![simple_assign(target, value)]
    }

    /// Assign an `Option`/`Result`-typed expression into `target`.
    /// `Ok`/`Err`/`Some`/`None` constructors become a structured `Simple`
    /// assignment of the constructor call; everything else falls back to a plain
    /// assign or `lower_block_to_var`.
    pub(crate) fn lower_option_result_assignment(
        &mut self,
        target: &GoExpression,
        target_ty: Option<&Type>,
        expression: &Expression,
    ) -> Vec<LoweredStatement> {
        let ty = target_ty
            .map(|t| self.facts.peel_alias(t))
            .filter(|t| t.is_option() || t.is_result())
            .unwrap_or_else(|| self.facts.peel_alias(&expression.get_type()));
        let Some(fallible) = Fallible::from_type(&ty) else {
            return self.lower_plain_assign(target, expression);
        };

        let actual_expression = if let Expression::Block { items, .. } = expression {
            if items.len() == 1 {
                &items[0]
            } else {
                expression
            }
        } else {
            expression
        };

        match actual_expression {
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                let kind = fallible.classify_constructor(callee);
                let (constructor_name, constructor_arg) = match kind {
                    Some(ConstructorKind::Success) => (
                        fallible.ok_constructor(),
                        Some(args.first().expect("success constructor has an argument")),
                    ),
                    Some(ConstructorKind::Failure) if fallible.err_constructor_takes_arg() => (
                        fallible.err_constructor(),
                        Some(args.first().expect("failure constructor has an argument")),
                    ),
                    Some(ConstructorKind::Failure) => (fallible.err_constructor(), None),
                    None => {
                        return self.lower_plain_assign(target, expression);
                    }
                };
                if let Some(constructor_arg) = constructor_arg {
                    let (arg_setup, call, argument_effect) = {
                        let mut fe = FalliblePlanner::new(self, &fallible);
                        let argument = fe
                            .planner
                            .lower_composite_value(constructor_arg, ExpressionContext::value());
                        let argument_effect = argument.evaluation.effect;
                        (
                            argument.setup,
                            fe.format_constructor_call(constructor_name, Some(argument.expression)),
                            argument_effect,
                        )
                    };
                    let value = ValuePlan::plain_call(
                        arg_setup,
                        call,
                        EvaluationEffect::PureCall.combine(argument_effect),
                    );
                    vec![simple_assign(target, value)]
                } else {
                    let call = {
                        let mut fe = FalliblePlanner::new(self, &fallible);
                        fe.format_constructor_call(constructor_name, None)
                    };
                    vec![simple_assign(
                        target,
                        ValuePlan::computed(Vec::new(), call, EvaluationEffect::Pure),
                    )]
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(actual_expression)
                    == Some(ConstructorKind::Failure)
                {
                    let call = {
                        let mut fe = FalliblePlanner::new(self, &fallible);
                        fe.format_constructor_call(fallible.err_constructor(), None)
                    };
                    vec![simple_assign(
                        target,
                        ValuePlan::computed(Vec::new(), call, EvaluationEffect::Pure),
                    )]
                } else {
                    self.lower_plain_assign(target, expression)
                }
            }
            _ => self.lower_block_to_var(expression, target, None, false),
        }
    }

    /// Lower a block (or single expression) that assigns its tail into `target`.
    /// `has_go_braces` selects the scope discipline: a full Go-brace scope when
    /// the caller wraps the result in `{ }`, otherwise a binding frame.
    pub(crate) fn lower_block_to_var(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
        has_go_braces: bool,
    ) -> Vec<LoweredStatement> {
        let is_block = matches!(expression, Expression::Block { .. });
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };

        self.with_block_scope(is_block, has_go_braces, |this| {
            let Some((last, rest)) = items.split_last() else {
                return Vec::new();
            };
            this.with_assign_target(target.as_str(), |this| {
                let mut statements = Vec::new();
                for item in rest {
                    statements.push(this.lower_statement(item));
                }
                statements.extend(this.lower_assign_tail(last, target, target_ty));
                statements
            })
        })
    }

    fn with_block_scope<R>(
        &mut self,
        is_block: bool,
        has_go_braces: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if !is_block {
            return f(self);
        }
        if has_go_braces {
            self.with_scope(f)
        } else {
            self.with_binding_frame(f)
        }
    }

    /// Lower a single tail expression in assign position into `target`.
    fn lower_assign_tail(
        &mut self,
        last: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<LoweredStatement> {
        if matches!(
            last,
            Expression::Return { .. }
                | Expression::Break { .. }
                | Expression::Continue { .. }
                | Expression::Let { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Const { .. }
        ) {
            return vec![self.lower_statement(last)];
        }
        if last.get_type().is_never() {
            let mut statements = vec![self.lower_statement(last)];
            if !is_go_never(last) && !is_breakless_loop(last) {
                statements.push(LoweredStatement::UnreachablePanic);
            }
            return statements;
        }
        if is_unit_call(last) {
            return self.lower_unit_call_into_var(last, target);
        }
        if let Some(statements) = self.lower_slice_growth_to_var(target, last) {
            return statements;
        }
        if matches!(
            last,
            Expression::If { .. }
                | Expression::IfLet { .. }
                | Expression::Match { .. }
                | Expression::Select { .. }
        ) {
            let place = PlacePlan::Assign {
                local: target,
                target_ty,
            };
            return self.lower_branching_to_block(last, &place).statements;
        }
        let value = self.lower_value(last, ExpressionContext::value());
        let value = value.map_expression_as_computed(|setup, expression| {
            self.apply_type_coercion(setup, target_ty, last, expression)
        });
        vec![simple_assign(target, value)]
    }

    /// `None` when `last` is not a slice `append` or `reserve` call.
    fn lower_slice_growth_to_var(
        &mut self,
        target: &GoExpression,
        last: &Expression,
    ) -> Option<Vec<LoweredStatement>> {
        let Expression::Call {
            expression: func,
            args,
            spread,
            ..
        } = last
        else {
            return None;
        };
        let (method, receiver) = self.slice_growth_method(func)?;

        let unwrapped = receiver.unwrap_parens();
        let receiver_is_lvalue =
            is_lvalue_chain(unwrapped) && !self.contains_newtype_access(unwrapped);

        let (value, mut statements) = if receiver_is_lvalue {
            let (arguments, ordering) = self.lower_growth_args(func, args, spread.as_deref());
            let mut capture: Vec<LoweredStatement> = Vec::new();
            let receiver_lv =
                self.emit_left_value_capturing(&mut capture, unwrapped, Some(&ordering));
            let grows = !arguments.is_empty();
            let receiver = if grows && receiver_lv.as_str() != target.as_str() {
                let clippable =
                    if is_clip_safe_path(receiver_lv.as_str()) && ordering.setup.is_empty() {
                        receiver_lv
                    } else {
                        GoExpression::name(self.hoist_tmp_value_statement(
                            &mut capture,
                            "recv",
                            receiver_lv,
                        ))
                    };
                clip_shared_capacity(clippable)
            } else {
                receiver_lv
            };
            capture.extend(ordering.setup);
            let value = if method == "reserve" {
                let mut all = vec![receiver];
                all.extend(arguments);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Grow"),
                    all,
                )
            } else if arguments.is_empty() {
                receiver
            } else {
                let mut all = vec![receiver];
                all.extend(arguments);
                GoExpression::call(GoExpression::name("append".to_string()), all)
            };
            (value, capture)
        } else {
            let plan = self.lower_value(last, ExpressionContext::value());
            (plan.expression, plan.setup)
        };

        statements.push(simple_assign(
            target,
            ValuePlan::computed(Vec::new(), value, EvaluationEffect::Pure),
        ));
        Some(statements)
    }

    fn slice_growth_method<'e>(&self, func: &'e Expression) -> Option<(&'e str, &'e Expression)> {
        if let Expression::DotAccess {
            expression, member, ..
        } = func
            && matches!(member.as_str(), "append" | "reserve")
            && self.is_native_shape(&expression.get_type(), NativeGoType::Slice)
        {
            return Some((member.as_str(), expression));
        }
        None
    }

    /// The growth arguments, plus their setup and effect as the plan the
    /// receiver capture orders itself against.
    fn lower_growth_args(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
    ) -> (Vec<GoExpression>, ValuePlan) {
        let stages: Vec<ValuePlan> = args
            .iter()
            .map(|a| self.lower_composite_value(a, ExpressionContext::value()))
            .collect();
        let combine = plan_variadic_spread(&self.facts, function, spread).map(|p| p.combine(0));
        let sequenced = self.sequence_with_spread_values(
            stages,
            spread,
            None,
            SpreadSequenceOptions {
                wrap_to_any: false,
                combine,
                boundary: CaptureBoundary::SiblingSequence,
            },
        );
        let ordering =
            ValuePlan::computed(sequenced.setup, GoExpression::empty(), sequenced.effect);
        (sequenced.values, ordering)
    }

    /// Lower `last` as a tail value. Tuple literals widen slot types to the
    /// return-slot types.
    pub(crate) fn lower_tail_value(
        &mut self,
        last: &Expression,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let plan = if let Expression::Tuple { elements, ty, .. } = last {
            self.plan_tuple_value(elements, ty, true)
        } else {
            self.lower_value(last, ExpressionContext::value())
        };
        (plan.setup, plan.expression)
    }

    pub(crate) fn lower_to_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        if let Expression::Block { items, .. } = expression {
            if ty.is_never()
                || ty.is_unit()
                || ty.is_placeholder()
                || matches!(ty, Type::Var { .. } | Type::Forall { .. })
            {
                return ValuePlan::computed(
                    self.lower_block_as_body(expression).statements,
                    GoExpression::empty(),
                    EvaluationEffect::Pure,
                );
            }
            let (result_var, declaration) = self.operand_temp_declaration(ty);
            let needs_braces = items.len() > 1;
            let target = GoExpression::name(result_var.clone());
            let body = self.lower_block_to_var(expression, &target, None, needs_braces);
            let mut statements = vec![declaration];
            if needs_braces {
                statements.push(LoweredStatement::Block(LoweredBlock { statements: body }));
            } else {
                statements.extend(body);
            }
            return ValuePlan::captured(statements, result_var);
        }
        if let Expression::Loop { .. } = expression {
            return self.plan_loop_as_operand_temp(expression, ty);
        }
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let mut statements = vec![declaration];
        let target = GoExpression::name(result_var.clone());
        statements.extend(self.lower_assign(expression, &target, Some(ty)));
        ValuePlan::captured(statements, result_var)
    }

    /// Build a `BreakValuePlan` for a `break value` statement.
    pub(crate) fn build_break_value_plan(&mut self, val: &Expression) -> BreakValuePlan {
        let value = self.lower_value(val, ExpressionContext::value());
        let value_is_empty = value.is_empty();
        let is_propagate_diverged = value_is_empty && matches!(val, Expression::Propagate { .. });
        if is_propagate_diverged {
            return BreakValuePlan::Diverged { value };
        }

        let action = if let Some(result_var) = self.current_loop_result_var().map(str::to_string) {
            if is_unit_call(val) {
                BreakValueAction::UnitCallIntoResult { result_var }
            } else {
                BreakValueAction::AssignToResult { result_var }
            }
        } else {
            BreakValueAction::Discard
        };
        let target = self
            .current_loop_id()
            .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source);
        BreakValuePlan::Transfer {
            value,
            action,
            target,
        }
    }
}
