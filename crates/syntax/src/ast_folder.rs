use crate::ast::{
    Expression, FormatStringPart, MatchArm, SelectArm, SelectArmPattern, StructSpread,
};

pub trait AstFolder {
    type Error;

    fn fold_module(
        &mut self,
        expressions: Vec<Expression>,
    ) -> Result<Vec<Expression>, Self::Error> {
        expressions
            .into_iter()
            .map(|e| self.fold_expression(e))
            .collect()
    }

    fn fold_expression(&mut self, expression: Expression) -> Result<Expression, Self::Error> {
        self.fold_expression_default(expression)
    }

    fn fold_expression_default(
        &mut self,
        expression: Expression,
    ) -> Result<Expression, Self::Error> {
        use Expression::*;

        Ok(match expression {
            Binary {
                operator,
                left,
                right,
                ty,
                span,
            } => Binary {
                operator,
                left: Box::new(self.fold_expression(*left)?),
                right: Box::new(self.fold_expression(*right)?),
                ty,
                span,
            },

            Call {
                expression,
                args,
                spread,
                type_args,
                ty,
                span,
                call_kind,
            } => Call {
                expression: Box::new(self.fold_expression(*expression)?),
                args: self.fold_vec(args)?,
                spread: Box::new((*spread).map(|e| self.fold_expression(e)).transpose()?),
                type_args,
                ty,
                span,
                call_kind,
            },

            Block { items, ty, span } => Block {
                items: self.fold_vec(items)?,
                ty,
                span,
            },

            TryBlock {
                items,
                ty,
                try_keyword_span,
                span,
            } => TryBlock {
                items: self.fold_vec(items)?,
                ty,
                try_keyword_span,
                span,
            },

            RecoverBlock {
                items,
                ty,
                recover_keyword_span,
                span,
            } => RecoverBlock {
                items: self.fold_vec(items)?,
                ty,
                recover_keyword_span,
                span,
            },

            If {
                condition,
                consequence,
                alternative,
                ty,
                span,
            } => If {
                condition: Box::new(self.fold_expression(*condition)?),
                consequence: Box::new(self.fold_expression(*consequence)?),
                alternative: Box::new(self.fold_expression(*alternative)?),
                ty,
                span,
            },

            IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                typed_pattern,
                else_span,
                ty,
                span,
            } => IfLet {
                pattern,
                scrutinee: Box::new(self.fold_expression(*scrutinee)?),
                consequence: Box::new(self.fold_expression(*consequence)?),
                alternative: Box::new(self.fold_expression(*alternative)?),
                typed_pattern,
                else_span,
                ty,
                span,
            },

            Match {
                subject,
                arms,
                origin,
                ty,
                span,
            } => Match {
                subject: Box::new(self.fold_expression(*subject)?),
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_match_arm(arm))
                    .collect::<Result<_, _>>()?,
                origin,
                ty,
                span,
            },

            Let {
                binding,
                value,
                mutable,
                mut_span,
                else_block,
                else_span,
                typed_pattern,
                ty,
                span,
            } => Let {
                binding,
                value: Box::new(self.fold_expression(*value)?),
                mutable,
                mut_span,
                else_block: else_block
                    .map(|e| self.fold_expression(*e).map(Box::new))
                    .transpose()?,
                else_span,
                typed_pattern,
                ty,
                span,
            },

            Return {
                expression,
                ty,
                span,
            } => Return {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            Propagate {
                expression,
                ty,
                span,
            } => Propagate {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            Unary {
                operator,
                expression,
                ty,
                span,
            } => Unary {
                operator,
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            Paren {
                expression,
                ty,
                span,
            } => Paren {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            DotAccess {
                expression,
                member,
                ty,
                span,
                dot_access_kind,
                receiver_coercion,
            } => DotAccess {
                expression: Box::new(self.fold_expression(*expression)?),
                member,
                ty,
                span,
                dot_access_kind,
                receiver_coercion,
            },

            IndexedAccess {
                expression,
                index,
                ty,
                span,
            } => IndexedAccess {
                expression: Box::new(self.fold_expression(*expression)?),
                index: Box::new(self.fold_expression(*index)?),
                ty,
                span,
            },

            Assignment {
                target,
                value,
                compound_operator,
                span,
            } => Assignment {
                target: Box::new(self.fold_expression(*target)?),
                value: Box::new(self.fold_expression(*value)?),
                compound_operator,
                span,
            },

            Tuple { elements, ty, span } => Tuple {
                elements: self.fold_vec(elements)?,
                ty,
                span,
            },

            StructCall {
                name,
                field_assignments,
                spread,
                ty,
                span,
            } => StructCall {
                name,
                field_assignments: field_assignments
                    .into_iter()
                    .map(|mut f| {
                        f.value = Box::new(self.fold_expression(*f.value)?);
                        Ok(f)
                    })
                    .collect::<Result<_, Self::Error>>()?,
                spread: match spread {
                    StructSpread::None => StructSpread::None,
                    StructSpread::From(e) => {
                        StructSpread::From(Box::new(self.fold_expression(*e)?))
                    }
                    StructSpread::ZeroFill { span } => StructSpread::ZeroFill { span },
                },
                ty,
                span,
            },

            Function {
                doc,
                attributes,
                name,
                name_span,
                generics,
                params,
                return_annotation,
                return_type,
                visibility,
                body,
                ty,
                span,
            } => Function {
                doc,
                attributes,
                name,
                name_span,
                generics,
                params,
                return_annotation,
                return_type,
                visibility,
                body: Box::new(self.fold_expression(*body)?),
                ty,
                span,
            },

            Lambda {
                params,
                return_annotation,
                body,
                ty,
                span,
            } => Lambda {
                params,
                return_annotation,
                body: Box::new(self.fold_expression(*body)?),
                ty,
                span,
            },

            Reference {
                expression,
                ty,
                span,
            } => Reference {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            For {
                binding,
                iterable,
                body,
                span,
                needs_label,
            } => For {
                binding,
                iterable: Box::new(self.fold_expression(*iterable)?),
                body: Box::new(self.fold_expression(*body)?),
                span,
                needs_label,
            },

            While {
                condition,
                body,
                span,
                needs_label,
            } => While {
                condition: Box::new(self.fold_expression(*condition)?),
                body: Box::new(self.fold_expression(*body)?),
                span,
                needs_label,
            },

            WhileLet {
                pattern,
                scrutinee,
                body,
                typed_pattern,
                span,
                needs_label,
            } => WhileLet {
                pattern,
                scrutinee: Box::new(self.fold_expression(*scrutinee)?),
                body: Box::new(self.fold_expression(*body)?),
                typed_pattern,
                span,
                needs_label,
            },

            Loop {
                body,
                ty,
                span,
                needs_label,
            } => Loop {
                body: Box::new(self.fold_expression(*body)?),
                ty,
                span,
                needs_label,
            },

            Task {
                expression,
                ty,
                span,
            } => Task {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            Defer {
                expression,
                ty,
                span,
            } => Defer {
                expression: Box::new(self.fold_expression(*expression)?),
                ty,
                span,
            },

            Select { arms, ty, span } => Select {
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_select_arm(arm))
                    .collect::<Result<_, _>>()?,
                ty,
                span,
            },

            ImplBlock {
                annotation,
                receiver_name,
                methods,
                generics,
                ty,
                span,
            } => ImplBlock {
                annotation,
                receiver_name,
                methods: self.fold_vec(methods)?,
                generics,
                ty,
                span,
            },

            Const {
                doc,
                identifier,
                identifier_span,
                annotation,
                expression,
                visibility,
                ty,
                span,
            } => Const {
                doc,
                identifier,
                identifier_span,
                annotation,
                expression: Box::new(self.fold_expression(*expression)?),
                visibility,
                ty,
                span,
            },

            Cast {
                expression,
                target_type,
                ty,
                span,
            } => Cast {
                expression: Box::new(self.fold_expression(*expression)?),
                target_type,
                ty,
                span,
            },

            Break { value, span } => Break {
                value: match value {
                    Some(v) => Some(Box::new(self.fold_expression(*v)?)),
                    None => None,
                },
                span,
            },

            Literal {
                literal: crate::ast::Literal::FormatString(parts),
                ty,
                span,
            } => {
                let folded_parts = parts
                    .into_iter()
                    .map(|part| match part {
                        FormatStringPart::Expression(expression) => {
                            Ok(FormatStringPart::Expression(Box::new(
                                self.fold_expression(*expression)?,
                            )))
                        }
                        other => Ok(other),
                    })
                    .collect::<Result<Vec<_>, Self::Error>>()?;
                Literal {
                    literal: crate::ast::Literal::FormatString(folded_parts),
                    ty,
                    span,
                }
            }

            Literal {
                literal: crate::ast::Literal::Slice(elements),
                ty,
                span,
            } => {
                let folded_elements = self.fold_vec(elements)?;
                Literal {
                    literal: crate::ast::Literal::Slice(folded_elements),
                    ty,
                    span,
                }
            }

            Range {
                start,
                end,
                inclusive,
                ty,
                span,
            } => Range {
                start: start
                    .map(|e| self.fold_expression(*e).map(Box::new))
                    .transpose()?,
                end: end
                    .map(|e| self.fold_expression(*e).map(Box::new))
                    .transpose()?,
                inclusive,
                ty,
                span,
            },

            Literal { .. }
            | Identifier { .. }
            | Enum { .. }
            | ValueEnum { .. }
            | Struct { .. }
            | TypeAlias { .. }
            | VariableDeclaration { .. }
            | ModuleImport { .. }
            | Interface { .. }
            | Continue { .. }
            | Unit { .. }
            | RawGo { .. }
            | NoOp => expression,
        })
    }

    fn fold_vec(&mut self, expressions: Vec<Expression>) -> Result<Vec<Expression>, Self::Error> {
        expressions
            .into_iter()
            .map(|e| self.fold_expression(e))
            .collect()
    }

    fn fold_match_arm(&mut self, mut arm: MatchArm) -> Result<MatchArm, Self::Error> {
        arm.expression = Box::new(self.fold_expression(*arm.expression)?);
        arm.guard = arm
            .guard
            .map(|g| self.fold_expression(*g).map(Box::new))
            .transpose()?;
        Ok(arm)
    }

    fn fold_select_arm(&mut self, arm: SelectArm) -> Result<SelectArm, Self::Error> {
        let pattern = match arm.pattern {
            SelectArmPattern::Receive {
                binding,
                typed_pattern,
                receive_expression,
                body,
            } => SelectArmPattern::Receive {
                binding,
                typed_pattern,
                receive_expression: Box::new(self.fold_expression(*receive_expression)?),
                body: Box::new(self.fold_expression(*body)?),
            },
            SelectArmPattern::Send {
                send_expression,
                body,
            } => SelectArmPattern::Send {
                send_expression: Box::new(self.fold_expression(*send_expression)?),
                body: Box::new(self.fold_expression(*body)?),
            },
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => SelectArmPattern::MatchReceive {
                receive_expression: Box::new(self.fold_expression(*receive_expression)?),
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_match_arm(arm))
                    .collect::<Result<_, _>>()?,
            },
            SelectArmPattern::WildCard { body } => SelectArmPattern::WildCard {
                body: Box::new(self.fold_expression(*body)?),
            },
        };
        Ok(SelectArm { pattern })
    }
}
