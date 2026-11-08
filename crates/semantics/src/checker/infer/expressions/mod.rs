use crate::checker::EnvResolve;
pub mod aliasing;
pub mod bindings;
mod branches;
mod call_effects;
pub mod calls;
pub mod comparison;
pub mod control_flow;
pub mod definitions;
pub mod dot_access;
pub mod functions;
pub mod impl_blocks;
pub mod indexed_access;
pub mod literals;
mod loops;
mod method_access;
pub mod operators;
pub mod patterns;
pub mod permission;
pub mod primitives;
pub mod propagate;
mod qualified_path;
pub mod select;
pub mod struct_call;

use syntax::ast::{BinaryOperator, CallTypeArguments, Expression, Span};
use syntax::program::CallKind;
use syntax::types::Type;

use crate::checker::infer::InferCtx;
use crate::checker::infer::context::UseContext;

impl InferCtx<'_> {
    /// Infer an expression nested inside another expression.
    pub fn infer_expression(&mut self, expression: Expression, expected_ty: &Type) -> Expression {
        self.infer_expression_at(expression, expected_ty, true)
    }

    /// Infer a statement or tail expression, where direct control flow is valid.
    pub fn infer_root_expression(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        self.infer_expression_at(expression, expected_ty, false)
    }

    pub(super) fn infer_expression_at(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
        is_subexpression: bool,
    ) -> Expression {
        let inferred = self.infer_expression_kind(expression, expected_ty, is_subexpression);
        // Poison what a failed expression feeds, so later uses stay silent.
        if inferred.get_type().resolve_in(&self.env).is_error() {
            self.unify(expected_ty, &Type::Error, &inferred.get_span());
        }
        inferred
    }

    fn infer_expression_kind(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
        is_subexpression: bool,
    ) -> Expression {
        if self.is_dot_access_base()
            && !matches!(
                expression,
                Expression::Identifier { .. }
                    | Expression::DotAccess { .. }
                    | Expression::Paren { .. }
            )
        {
            return self.with_dot_access_base(false, |state| {
                state.infer_expression_at(expression, expected_ty, is_subexpression)
            });
        }

        match expression {
            Expression::Literal { literal, span, .. } => {
                self.infer_literal(literal, expected_ty, span)
            }

            Expression::Block { items, span, .. } => self.infer_block(items, span, expected_ty),

            Expression::Function { .. } => self.infer_function(expression, expected_ty),

            Expression::Lambda {
                params,
                return_annotation,
                body,
                span,
                ..
            } => self.infer_lambda(params, return_annotation, body, span, expected_ty),

            Expression::Unit { ty, span } => self.infer_unit(ty, span, expected_ty),

            Expression::Identifier {
                ref value, span, ..
            } => self.infer_identifier(value.clone(), span, expected_ty),

            Expression::Let { .. } => self.infer_let_binding(expression, expected_ty),

            Expression::Call {
                expression: ref callee,
                span,
                ..
            } => {
                let is_panic =
                    matches!(&**callee, Expression::Identifier { value, .. } if value == "panic");
                let checkpoint = self.sink.checkpoint();
                let result = self.infer_function_call(expression, expected_ty);
                let reported_panic_position = self
                    .sink
                    .has_code_since(checkpoint, "infer.panic_in_expression_position");
                if is_subexpression && is_panic && !reported_panic_position {
                    self.sink
                        .push(diagnostics::infer::never_call_in_expression(span));
                }
                result
            }

            Expression::If {
                condition,
                consequence,
                alternative,
                span,
                ..
            } => self.infer_if(condition, consequence, alternative, span, expected_ty),

            Expression::IfLet { .. } => self.infer_if_let(expression, expected_ty),

            Expression::Match {
                subject,
                arms,
                span,
                ..
            } => self.infer_match(subject, arms, span, expected_ty),

            Expression::Tuple { elements, span, .. } => {
                self.infer_tuple(elements, span, expected_ty)
            }

            Expression::StructCall { .. } => self.infer_struct_call(expression, expected_ty),

            Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } => self.infer_dot_access_or_qualified_path(expression, member, span, expected_ty),

            Expression::Enum { .. } => expression,

            Expression::Struct { .. } => self.infer_struct_definition(expression),

            Expression::TypeAlias { .. } => self.infer_type_alias_definition(expression),

            Expression::VariableDeclaration { .. } => expression,

            Expression::ImplBlock {
                annotation,
                ty: _,
                methods,
                receiver_name,
                generics,
                span,
            } => self.infer_impl_block(annotation, methods, receiver_name, generics, span),

            Expression::Interface { .. } => self.infer_interface(expression),

            Expression::Assignment {
                target,
                value,
                compound_operator,
                span,
            } => self.infer_assignment(target, value, compound_operator, span),

            Expression::Return {
                expression, span, ..
            } => self.infer_return_statement(expression, span, is_subexpression),

            Expression::Propagate {
                expression, span, ..
            } => {
                let propagate = self.infer_propagate(expression, span, expected_ty);
                if is_subexpression && let Expression::Propagate { expression, .. } = &propagate {
                    self.check_failure_propagation_in_subexpression(expression, span);
                }
                propagate
            }

            Expression::TryBlock {
                items,
                try_keyword_span,
                span,
                ..
            } => self.infer_try_block(items, try_keyword_span, span, expected_ty),

            Expression::RecoverBlock {
                items,
                recover_keyword_span,
                span,
                ..
            } => self.infer_recover_block(items, recover_keyword_span, span, expected_ty),

            Expression::Binary {
                operator: BinaryOperator::Pipeline,
                left,
                right,
                span,
                ..
            } => match self.lower_pipeline(*left, *right, span) {
                Ok(lowered) => self.infer_expression_at(lowered, expected_ty, is_subexpression),
                Err(rejected) => self.infer_rejected_pipeline(rejected, expected_ty, span),
            },

            Expression::Binary { .. } | Expression::Unary { .. }
                if self.adapts_as_constant(&expression, expected_ty) =>
            {
                self.infer_constant_operation(expression, expected_ty)
            }

            Expression::Binary {
                operator,
                left,
                right,
                span,
                ..
            } => self.infer_binary(operator, left, right, expected_ty, span),

            Expression::Paren {
                expression, span, ..
            } => self.infer_paren(expression, span, expected_ty, is_subexpression),

            Expression::Unary {
                operator,
                expression,
                span,
                ..
            } => self.infer_unary(operator, expression, expected_ty, span),

            Expression::Const { .. } => self.infer_const_binding(expression),

            Expression::Loop { body, span, .. } => self.infer_loop(body, span, expected_ty),

            Expression::While {
                condition,
                body,
                span,
                ..
            } => self.infer_while(condition, body, span, expected_ty),

            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                span,
                ..
            } => self.infer_while_let(pattern, scrutinee, body, span, expected_ty),

            Expression::For {
                binding,
                iterable,
                body,
                span,
                ..
            } => self.infer_for(*binding, iterable, body, span, expected_ty),

            Expression::Reference {
                expression, span, ..
            } => self.infer_reference(expression, span, expected_ty),

            Expression::IndexedAccess {
                expression,
                index,
                span,
                from_colon_syntax,
                ..
            } => {
                if from_colon_syntax {
                    self.infer_colon_subscript(expression, index, span)
                } else {
                    self.infer_indexed_access(expression, index, span, expected_ty)
                }
            }

            Expression::Task {
                expression, span, ..
            } => {
                self.check_control_flow_in_expression("task", span, is_subexpression);
                self.infer_task(expression, span, expected_ty)
            }

            Expression::Defer {
                expression, span, ..
            } => {
                self.check_control_flow_in_expression("defer", span, is_subexpression);
                self.infer_defer(expression, span, expected_ty)
            }

            Expression::Assert {
                expression, span, ..
            } => self.infer_assert(expression, span, expected_ty),

            Expression::Select { arms, span, .. } => self.infer_select(arms, span, expected_ty),

            Expression::PackageImport {
                name,
                name_span,
                alias,
                span,
            } => Expression::PackageImport {
                name,
                name_span,
                alias,
                span,
            },

            Expression::Range {
                start,
                end,
                inclusive,
                span,
                ..
            } => self.infer_range(start, end, inclusive, span, expected_ty),

            Expression::Cast {
                expression,
                target_type,
                span,
                ..
            } => self.infer_cast(expression, target_type, span, expected_ty),

            Expression::Break { value, span } => self.infer_break(value, span, is_subexpression),
            Expression::Continue { span } => self.infer_continue(span, is_subexpression),
            Expression::RawGo { text } => Expression::RawGo { text },
        }
    }

    fn lower_pipeline(
        &mut self,
        left: Expression,
        right: Expression,
        span: Span,
    ) -> Result<Expression, RejectedPipeline> {
        let mut segments = vec![(right, span)];
        let mut current = left;
        while let Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left,
            right,
            span,
            ..
        } = current
        {
            segments.push((*right, span));
            current = *left;
        }

        let mut lowered = Ok(current);
        while let Some((right, span)) = segments.pop() {
            lowered = match lowered {
                Ok(left) => self.lower_pipeline_step(left, right, span),
                Err(mut rejected) => {
                    rejected.extend(pipeline_target_parts(right));
                    Err(rejected)
                }
            };
        }
        lowered
    }

    fn lower_pipeline_step(
        &mut self,
        left: Expression,
        right: Expression,
        span: Span,
    ) -> Result<Expression, RejectedPipeline> {
        let right = Self::unwrap_parens_owned(right);
        if matches!(
            right,
            Expression::Identifier { .. }
                | Expression::DotAccess { .. }
                | Expression::Call { .. }
                | Expression::Binary {
                    operator: BinaryOperator::Pipeline,
                    ..
                }
        ) && let Some(stray) = stray_placeholder_in_target(&right)
        {
            self.sink
                .push(diagnostics::infer::misplaced_pipeline_placeholder(stray));
            return Err(rejected_step(left, pipeline_target_parts(right)));
        }
        let right = match right {
            Expression::Binary {
                operator: BinaryOperator::Pipeline,
                left: inner_left,
                right: inner_right,
                span,
                ..
            } => match self.lower_pipeline(*inner_left, *inner_right, span) {
                Ok(lowered) => lowered,
                Err(rejected) => return Err(rejected_step(left, rejected)),
            },
            other => other,
        };

        match right {
            Expression::Identifier { .. } | Expression::DotAccess { .. } => Ok(Expression::Call {
                expression: Box::new(right),
                args: vec![left],
                spread: None,
                type_arguments: CallTypeArguments::none(),
                ty: Type::uninferred(),
                span,
                call_kind: CallKind::Unresolved,
            }),
            Expression::Call {
                expression,
                mut args,
                mut spread,
                type_arguments,
                ty,
                call_kind,
                ..
            } => {
                let (has_placeholder, repeated) = {
                    let mut placeholders = args
                        .iter()
                        .chain(spread.as_deref())
                        .filter(|arg| is_placeholder(arg))
                        .map(Expression::get_span);
                    (placeholders.next().is_some(), placeholders.next())
                };
                if let Some(repeated) = repeated {
                    self.sink
                        .push(diagnostics::infer::repeated_pipeline_placeholder(repeated));
                    return Err(rejected_step(left, pipeline_target_parts(*expression)));
                }
                if !has_placeholder {
                    args.insert(0, left);
                } else if let Some(slot) = args.iter_mut().find(|arg| is_placeholder(arg)) {
                    *slot = left;
                } else {
                    spread = Some(Box::new(left));
                }
                Ok(Expression::Call {
                    expression,
                    args,
                    spread,
                    type_arguments,
                    ty,
                    span,
                    call_kind,
                })
            }
            Expression::Propagate {
                span: propagate_span,
                ..
            } => {
                self.sink
                    .push(diagnostics::infer::propagate_in_pipeline(propagate_span));
                Err(rejected_step(left, pipeline_target_parts(right)))
            }
            other => {
                self.sink.push(diagnostics::infer::invalid_pipeline_target(
                    other.get_span(),
                ));
                Err(rejected_step(left, Vec::new()))
            }
        }
    }

    fn infer_rejected_pipeline(
        &mut self,
        rejected: RejectedPipeline,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        let elements = rejected
            .into_iter()
            .map(|part| {
                let ty = self.new_type_var();
                match part {
                    PipelinePart::Value(value) => {
                        self.with_value_context(|state| state.infer_expression(value, &ty))
                    }
                    PipelinePart::Callee(callee) => self
                        .with_use_context(UseContext::Callee, |state| {
                            state.infer_expression(callee, &ty)
                        }),
                }
            })
            .collect();
        self.unify(expected_ty, &Type::Error, &span);
        Expression::Tuple {
            elements,
            ty: Type::Error,
            span,
        }
    }

    fn unwrap_parens_owned(mut expression: Expression) -> Expression {
        loop {
            match expression {
                Expression::Paren {
                    expression: inner, ..
                } => expression = *inner,
                other => return other,
            }
        }
    }

    /// Bans `task`/`defer` as a bare subexpression, skipped when the dedicated check already fires.
    fn check_control_flow_in_expression(&mut self, kind: &str, span: Span, is_subexpression: bool) {
        if is_subexpression && !self.is_value_context() {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(kind, span));
        }
    }
}

fn is_placeholder(expression: &Expression) -> bool {
    matches!(expression, Expression::Identifier { value, .. } if value == "_")
}

enum PipelinePart {
    Value(Expression),
    Callee(Expression),
}

type RejectedPipeline = Vec<PipelinePart>;

fn rejected_step(left: Expression, target_parts: RejectedPipeline) -> RejectedPipeline {
    let mut parts = Vec::new();
    if stray_placeholder(&left).is_none() {
        parts.push(PipelinePart::Value(left));
    }
    parts.extend(target_parts);
    parts
}

fn pipeline_target_parts(target: Expression) -> RejectedPipeline {
    let callee = match target {
        Expression::Paren { expression, .. } | Expression::Propagate { expression, .. } => {
            return pipeline_target_parts(*expression);
        }
        Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left,
            right,
            ..
        } => {
            let mut parts = pipeline_target_parts_of_value(*left);
            parts.extend(pipeline_target_parts(*right));
            return parts;
        }
        Expression::Call { expression, .. } => *expression,
        callee @ (Expression::Identifier { .. } | Expression::DotAccess { .. }) => callee,
        _ => return Vec::new(),
    };
    if stray_placeholder(&callee).is_some() {
        return Vec::new();
    }
    vec![PipelinePart::Callee(callee)]
}

fn pipeline_target_parts_of_value(value: Expression) -> RejectedPipeline {
    match value {
        Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left,
            right,
            ..
        } => {
            let mut parts = pipeline_target_parts_of_value(*left);
            parts.extend(pipeline_target_parts(*right));
            parts
        }
        value => rejected_step(value, Vec::new()),
    }
}

fn stray_placeholder(expression: &Expression) -> Option<Span> {
    match expression {
        Expression::Identifier { span, .. } if is_placeholder(expression) => Some(*span),
        Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left,
            right,
            ..
        } => stray_placeholder(left).or_else(|| stray_placeholder_in_target(right.unwrap_parens())),
        _ => expression
            .children()
            .into_iter()
            .find_map(stray_placeholder),
    }
}

fn stray_placeholder_in_target(target: &Expression) -> Option<Span> {
    match target {
        Expression::Call {
            expression,
            args,
            spread,
            ..
        } => stray_placeholder(expression).or_else(|| {
            args.iter()
                .chain(spread.as_deref())
                .filter(|arg| !is_placeholder(arg))
                .find_map(stray_placeholder)
        }),
        other => stray_placeholder(other),
    }
}
