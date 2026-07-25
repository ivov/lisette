use crate::ast::{
    Expression, FormatStringPart, MatchArm, SelectArm, SelectArmPattern, StructSpread,
};

pub(crate) trait AstFolder {
    fn fold_module(&mut self, expressions: Vec<Expression>) -> Vec<Expression> {
        expressions
            .into_iter()
            .map(|e| self.fold_expression(e))
            .collect()
    }

    fn fold_expression(&mut self, expression: Expression) -> Expression {
        self.fold_expression_default(expression)
    }

    fn fold_expression_default(&mut self, expression: Expression) -> Expression {
        use Expression::*;

        match expression {
            Binary {
                operator,
                left,
                right,
                ty,
                span,
            } => Binary {
                operator,
                left: Box::new(self.fold_expression(*left)),
                right: Box::new(self.fold_expression(*right)),
                ty,
                span,
            },

            Call {
                expression,
                args,
                spread,
                type_arguments,
                ty,
                span,
                call_kind,
            } => Call {
                expression: Box::new(self.fold_expression(*expression)),
                args: self.fold_vec(args),
                spread: spread.map(|e| Box::new(self.fold_expression(*e))),
                type_arguments,
                ty,
                span,
                call_kind,
            },

            Block { items, ty, span } => Block {
                items: self.fold_vec(items),
                ty,
                span,
            },

            TryBlock {
                items,
                ty,
                try_keyword_span,
                span,
            } => TryBlock {
                items: self.fold_vec(items),
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
                items: self.fold_vec(items),
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
                condition: Box::new(self.fold_expression(*condition)),
                consequence: Box::new(self.fold_expression(*consequence)),
                alternative: Box::new(self.fold_expression(*alternative)),
                ty,
                span,
            },

            IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                else_span,
                ty,
                span,
            } => IfLet {
                pattern,
                scrutinee: Box::new(self.fold_expression(*scrutinee)),
                consequence: Box::new(self.fold_expression(*consequence)),
                alternative: Box::new(self.fold_expression(*alternative)),
                else_span,
                ty,
                span,
            },

            Match {
                subject,
                arms,
                ty,
                span,
            } => Match {
                subject: Box::new(self.fold_expression(*subject)),
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_match_arm(arm))
                    .collect(),
                ty,
                span,
            },

            Let {
                binding,
                value,
                mode,
                ty,
                span,
            } => Let {
                binding,
                value: Box::new(self.fold_expression(*value)),
                mode: mode.map_else(|expression, _| self.fold_expression(expression)),
                ty,
                span,
            },

            Return {
                expression,
                ty,
                span,
            } => Return {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            Propagate {
                expression,
                ty,
                span,
            } => Propagate {
                expression: Box::new(self.fold_expression(*expression)),
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
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            Paren {
                expression,
                ty,
                span,
            } => Paren {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            DotAccess {
                expression,
                member,
                ty,
                span,
                resolution,
            } => DotAccess {
                expression: Box::new(self.fold_expression(*expression)),
                member,
                ty,
                span,
                resolution,
            },

            IndexedAccess {
                expression,
                index,
                ty,
                span,
                from_colon_syntax,
            } => IndexedAccess {
                expression: Box::new(self.fold_expression(*expression)),
                index: Box::new(self.fold_expression(*index)),
                ty,
                span,
                from_colon_syntax,
            },

            Assignment {
                target,
                value,
                compound_operator,
                span,
            } => Assignment {
                target: Box::new(self.fold_expression(*target)),
                value: Box::new(self.fold_expression(*value)),
                compound_operator,
                span,
            },

            Tuple { elements, ty, span } => Tuple {
                elements: self.fold_vec(elements),
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
                        f.value = Box::new(self.fold_expression(*f.value));
                        f
                    })
                    .collect(),
                spread: match spread {
                    StructSpread::None => StructSpread::None,
                    StructSpread::From(e) => StructSpread::From(Box::new(self.fold_expression(*e))),
                    StructSpread::Autofill { span } => StructSpread::Autofill { span },
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
                body: body.map_definition(|body| self.fold_expression(body)),
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
                body: Box::new(self.fold_expression(*body)),
                ty,
                span,
            },

            Reference {
                expression,
                ty,
                span,
            } => Reference {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            For {
                binding,
                iterable,
                body,
                span,
            } => For {
                binding,
                iterable: Box::new(self.fold_expression(*iterable)),
                body: Box::new(self.fold_expression(*body)),
                span,
            },

            While {
                condition,
                body,
                span,
            } => While {
                condition: Box::new(self.fold_expression(*condition)),
                body: Box::new(self.fold_expression(*body)),
                span,
            },

            WhileLet {
                pattern,
                scrutinee,
                body,
                span,
            } => WhileLet {
                pattern,
                scrutinee: Box::new(self.fold_expression(*scrutinee)),
                body: Box::new(self.fold_expression(*body)),
                span,
            },

            Loop { body, ty, span } => Loop {
                body: Box::new(self.fold_expression(*body)),
                ty,
                span,
            },

            Task {
                expression,
                ty,
                span,
            } => Task {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            Defer {
                expression,
                ty,
                span,
            } => Defer {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            Assert {
                expression,
                ty,
                span,
            } => Assert {
                expression: Box::new(self.fold_expression(*expression)),
                ty,
                span,
            },

            Select { arms, ty, span } => Select {
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_select_arm(arm))
                    .collect(),
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
                methods: self.fold_vec(methods),
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
                expression: expression.map_value(|value| self.fold_expression(value)),
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
                expression: Box::new(self.fold_expression(*expression)),
                target_type,
                ty,
                span,
            },

            Break { value, span } => Break {
                value: value.map(|value| Box::new(self.fold_expression(*value))),
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
                        FormatStringPart::Expression(expression) => FormatStringPart::Expression(
                            Box::new(self.fold_expression(*expression)),
                        ),
                        other => other,
                    })
                    .collect();
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
                let folded_elements = self.fold_vec(elements);
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
                start: start.map(|expression| Box::new(self.fold_expression(*expression))),
                end: end.map(|expression| Box::new(self.fold_expression(*expression))),
                inclusive,
                ty,
                span,
            },

            Literal { .. }
            | Identifier { .. }
            | Enum { .. }
            | Struct { .. }
            | TypeAlias { .. }
            | VariableDeclaration { .. }
            | ModuleImport { .. }
            | Interface { .. }
            | Continue { .. }
            | Unit { .. }
            | RawGo { .. } => expression,
        }
    }

    fn fold_vec(&mut self, expressions: Vec<Expression>) -> Vec<Expression> {
        expressions
            .into_iter()
            .map(|e| self.fold_expression(e))
            .collect()
    }

    fn fold_match_arm(&mut self, mut arm: MatchArm) -> MatchArm {
        arm.expression = Box::new(self.fold_expression(*arm.expression));
        arm.guard = arm
            .guard
            .map(|guard| Box::new(self.fold_expression(*guard)));
        arm
    }

    fn fold_select_arm(&mut self, arm: SelectArm) -> SelectArm {
        let pattern = match arm.pattern {
            SelectArmPattern::Receive {
                binding,
                receive_expression,
                body,
            } => SelectArmPattern::Receive {
                binding,
                receive_expression: Box::new(self.fold_expression(*receive_expression)),
                body: Box::new(self.fold_expression(*body)),
            },
            SelectArmPattern::Send {
                send_expression,
                body,
            } => SelectArmPattern::Send {
                send_expression: Box::new(self.fold_expression(*send_expression)),
                body: Box::new(self.fold_expression(*body)),
            },
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => SelectArmPattern::MatchReceive {
                receive_expression: Box::new(self.fold_expression(*receive_expression)),
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_match_arm(arm))
                    .collect(),
            },
            SelectArmPattern::WildCard { body } => SelectArmPattern::WildCard {
                body: Box::new(self.fold_expression(*body)),
            },
        };
        SelectArm { pattern }
    }
}
