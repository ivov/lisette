use crate::ast::{BinaryOperator, Expression, MatchArm, MatchOrigin, Pattern, Span};
use crate::ast_folder::AstFolder;
use crate::parse::ParseError;
use crate::types::Type;

#[derive(Debug)]
pub struct DesugarResult {
    pub ast: Vec<Expression>,
    pub errors: Vec<ParseError>,
}

/// Desugars syntactic sugar into core AST forms.
///
/// Transforms:
/// - `x |> func` into `func(x)`
/// - `x |> func(a, b)` into `func(x, a, b)`
/// - `if let P = S { C } else { A }` into `match S { P => C, _ => A }`
pub fn desugar(expressions: Vec<Expression>) -> DesugarResult {
    let mut desugarer = Desugarer::new();
    let ast = desugarer.fold_module(expressions).unwrap(); // Infallible
    DesugarResult {
        ast,
        errors: desugarer.errors,
    }
}

struct Desugarer {
    errors: Vec<ParseError>,
}

impl Desugarer {
    fn new() -> Self {
        Self { errors: Vec::new() }
    }
}

impl AstFolder for Desugarer {
    type Error = std::convert::Infallible;

    fn fold_expression(&mut self, expression: Expression) -> Result<Expression, Self::Error> {
        if let Expression::Binary { ref left, .. } = expression
            && matches!(**left, Expression::Binary { .. })
        {
            return self.fold_binary_iterative(expression);
        }

        let expression = self.fold_expression_default(expression)?;

        Ok(self.apply_desugar(expression))
    }
}

impl Desugarer {
    fn apply_desugar(&mut self, expression: Expression) -> Expression {
        match expression {
            pipeline @ Expression::Binary {
                operator: BinaryOperator::Pipeline,
                ..
            } => self.desugar_pipeline(pipeline),

            if_let @ Expression::IfLet { .. } => self.desugar_if_let(if_let),

            other => other,
        }
    }

    fn fold_binary_iterative(
        &mut self,
        expression: Expression,
    ) -> Result<Expression, std::convert::Infallible> {
        let Expression::Binary {
            operator,
            left,
            right,
            ty,
            span,
        } = expression
        else {
            return self.fold_expression(expression);
        };

        let mut stack: Vec<(BinaryOperator, Box<Expression>, Type, Span)> =
            vec![(operator, right, ty, span)];
        let mut current = *left;
        while let Expression::Binary {
            operator: op,
            left: l,
            right: r,
            ty: t,
            span: s,
        } = current
        {
            stack.push((op, r, t, s));
            current = *l;
        }

        let mut result = self.fold_expression(current)?;
        while let Some((op, right, t, s)) = stack.pop() {
            let folded_right = self.fold_expression(*right)?;
            let binary = Expression::Binary {
                operator: op,
                left: Box::new(result),
                right: Box::new(folded_right),
                ty: t,
                span: s,
            };
            result = self.apply_desugar(binary);
        }
        Ok(result)
    }

    fn desugar_pipeline(&mut self, pipeline: Expression) -> Expression {
        let Expression::Binary {
            left, right, span, ..
        } = pipeline
        else {
            unreachable!()
        };

        let left = *left;
        let right = right.unwrap_parens().clone();

        match right {
            Expression::Identifier { .. } | Expression::DotAccess { .. } => Expression::Call {
                expression: Box::new(right),
                args: vec![left],
                spread: Box::new(None),
                type_args: vec![],
                ty: Type::uninferred(),
                span,
                call_kind: None,
            },

            Expression::Call {
                expression,
                args,
                spread,
                type_args,
                ty,
                span: _,
                call_kind,
            } => {
                let mut new_args = vec![left];
                new_args.extend(args);
                Expression::Call {
                    expression,
                    args: new_args,
                    spread,
                    type_args,
                    ty,
                    span,
                    call_kind,
                }
            }

            Expression::Propagate {
                span: propagate_span,
                ..
            } => {
                let error = ParseError::new(
                    "Invalid `?` in pipeline",
                    propagate_span,
                    "propagate operator used here",
                )
                .with_parse_code("propagate_in_pipeline")
                .with_help(
                    "Extract the `?` operation to a `let` binding: `let result = (... |> func)?`",
                );
                self.errors.push(error);
                Expression::Unit {
                    ty: Type::uninferred(),
                    span,
                }
            }

            _ => {
                let right_span = right.get_span();
                let error = ParseError::new("Invalid pipeline", right_span, "expected function")
                    .with_parse_code("invalid_pipeline_target")
                    .with_help("Pipeline only supports functions (not lambdas)");
                self.errors.push(error);
                Expression::Unit {
                    ty: Type::uninferred(),
                    span,
                }
            }
        }
    }

    fn desugar_if_let(&mut self, if_let: Expression) -> Expression {
        let Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            typed_pattern,
            else_span,
            span,
            ..
        } = if_let
        else {
            unreachable!()
        };

        let arms = vec![
            MatchArm {
                pattern,
                guard: None,
                typed_pattern,
                expression: consequence,
            },
            MatchArm {
                pattern: Pattern::WildCard {
                    span: alternative.get_span(),
                },
                guard: None,
                typed_pattern: None,
                expression: alternative,
            },
        ];

        Expression::Match {
            subject: scrutinee,
            arms,
            origin: MatchOrigin::IfLet { else_span },
            ty: Type::uninferred(),
            span,
        }
    }
}
