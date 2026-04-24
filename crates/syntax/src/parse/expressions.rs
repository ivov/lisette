use ecow::EcoString;

use super::{MAX_TUPLE_ARITY, ParseError, Parser};
use crate::ast::{
    Annotation, Attribute, BinaryOperator, Binding, Expression, FormatStringPart, ImportAlias,
    Literal, SelectArm, SelectArmPattern, Span, StructFieldAssignment, UnaryOperator, Visibility,
};
use crate::lex::TokenKind::{self, *};
use crate::types::Type;

impl<'source> Parser<'source> {
    pub fn parse_expression(&mut self) -> Expression {
        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            self.resync_on_error();
            return Expression::Unit {
                ty: Type::uninferred(),
                span,
            };
        }
        let result = self.pratt_parse(0);
        self.leave_recursion();
        result
    }

    pub fn parse_atomic_expression(&mut self) -> Expression {
        if self.keyword_in_value_position() {
            return self.recover_keyword_as_identifier();
        }

        match self.current_token().kind {
            Integer | Imaginary | Boolean | Char | String | Float => self.parse_literal(),
            FormatStringStart => self.parse_format_string(),
            LeftParen => self.parse_parenthesized_expression(),
            LeftCurlyBrace => self.parse_block_expression(),
            LeftSquareBracket => self.parse_slice_literal(),
            Identifier => self.parse_identifier(),
            Function => self.parse_function(None, vec![]),
            Match => self.parse_match(),
            If => self.parse_if(),
            Pipe | PipeDouble => self.parse_lambda(),
            Task => self.parse_task(),
            Defer => self.parse_defer(),
            Try => self.parse_try_block(),
            Recover => self.parse_recover_block(),
            Select => self.parse_select(),
            Loop => self.parse_loop(),
            Return => self.parse_return(),
            Break => self.parse_break(),
            Continue => self.parse_continue(),
            DotDot | DotDotEqual => self.parse_range(None, self.current_token()),

            LeftAngleBracket
                if self.stream.peek_ahead(1).kind == Minus
                    && self.current_token().byte_offset + self.current_token().byte_length
                        == self.stream.peek_ahead(1).byte_offset =>
            {
                let start = self.current_token();
                let span = Span::new(self.file_id, start.byte_offset, start.byte_length + 1);
                self.track_error_at(
                    span,
                    "invalid syntax for channel receive",
                    "Use `select { let v = ch.receive() => ... }` to receive from a channel",
                );
                self.resync_on_error();
                Expression::Unit {
                    ty: Type::uninferred(),
                    span,
                }
            }

            _ => self.unexpected_token("expr"),
        }
    }

    pub fn parse_range(
        &mut self,
        start: Option<Box<Expression>>,
        span_start: crate::lex::Token<'source>,
    ) -> Expression {
        if matches!(start.as_deref(), Some(Expression::Range { .. })) {
            self.track_error("not allowed", "Chained range operators are not supported");
        }

        let inclusive = self.is(DotDotEqual);

        self.next();

        let has_end = !matches!(
            self.current_token().kind,
            RightCurlyBrace
                | RightSquareBracket
                | RightParen
                | LeftCurlyBrace
                | Semicolon
                | Comma
                | EOF
        );

        if inclusive && !has_end {
            self.track_error(
                "expected end value",
                "Inclusive ranges require an end value.",
            );
        }

        let end = if has_end {
            Some(Box::new(self.parse_range_end()))
        } else {
            None
        };

        Expression::Range {
            start,
            end,
            inclusive,
            ty: Type::uninferred(),
            span: self.span_from_tokens(span_start),
        }
    }

    fn parse_literal(&mut self) -> Expression {
        let start = self.current_token();

        let literal = match self.current_token().kind {
            Integer => {
                let text = self.current_token().text;
                let literal = self.parse_integer_text(text);
                self.next();
                literal
            }
            Float => {
                let raw = self.current_token().text;
                let cleaned = raw.replace('_', "");
                let f: f64 = cleaned.parse().unwrap_or_else(|_| {
                    self.track_error(
                        format!("float literal '{}' is out of range", raw),
                        "Value must be a valid 64-bit floating point number.",
                    );
                    0.0
                });
                let text = if raw.contains('e') || raw.contains('E') || raw.contains('_') {
                    Some(raw.to_string())
                } else {
                    None
                };
                self.next();
                Literal::Float { value: f, text }
            }
            Imaginary => {
                let text = self.current_token().text;
                let coef: f64 = text[..text.len() - 1]
                    .replace('_', "")
                    .parse()
                    .unwrap_or_else(|_| {
                        self.track_error(
                            format!("imaginary literal '{}' is out of range", text),
                            "Value must be a valid 64-bit floating point number.",
                        );
                        0.0
                    });
                self.next();
                Literal::Imaginary(coef)
            }
            Boolean => {
                let b = self.current_token().text == "true";
                self.next();
                Literal::Boolean(b)
            }
            String => {
                let s = self.current_token().text;
                self.next();
                let s_stripped = if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
                    &s[1..s.len() - 1]
                } else {
                    debug_assert!(false, "lexer produced String token without quotes: {:?}", s);
                    s
                };
                Literal::String(s_stripped.to_string())
            }
            Char => {
                let c = self.current_token().text;
                self.next();
                let c_stripped = if c.len() >= 2 && c.starts_with('\'') && c.ends_with('\'') {
                    &c[1..c.len() - 1]
                } else {
                    debug_assert!(false, "lexer produced Char token without quotes: {:?}", c);
                    c
                };
                Literal::Char(c_stripped.to_string())
            }
            _ => return self.unexpected_token("literal"),
        };

        Expression::Literal {
            literal,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_slice_literal(&mut self) -> Expression {
        let start = self.current_token();
        let (expressions, _) =
            self.collect_delimited_expressions(LeftSquareBracket, RightSquareBracket);

        Expression::Literal {
            literal: Literal::Slice(expressions),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_identifier(&mut self) -> Expression {
        let start = self.current_token();
        let text = self.current_token().text;

        if text == "go" {
            let next = self.stream.peek_ahead(1).kind;
            if next == LeftCurlyBrace || next == Identifier {
                self.track_error(
                    "invalid syntax",
                    "Use `task { ... }` or `task my_function()` to spawn a concurrent task.",
                );
            }
        }

        self.ensure(Identifier);

        Expression::Identifier {
            value: text.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
            binding_id: None,
            qualified: None,
        }
    }

    pub fn parse_struct_call(&mut self, expression: Expression) -> Expression {
        let name = self.make_expression_name(&expression);
        let name_span = expression.get_span();
        let start_offset = name_span.byte_offset; // Start from the name, not the brace

        self.ensure(LeftCurlyBrace);

        let mut field_assignments = vec![];
        let mut spread = None;
        let mut seen_fields: Vec<(EcoString, Span)> = vec![];

        while self.is_not(RightCurlyBrace) {
            if self.is(DotDot) {
                if spread.is_some() {
                    self.track_error(
                        "spread must be last",
                        "Move the `..spread` to the end of the struct.",
                    );
                    break;
                }

                self.ensure(DotDot);

                if self.is(RightCurlyBrace) || self.is(Comma) {
                    self.track_error(
                        "not allowed",
                        "Use `..struct` to spread the fields of one struct into another",
                    );
                } else {
                    spread = Some(self.parse_expression());
                }

                self.expect_comma_or(RightCurlyBrace);
                continue;
            }

            if spread.is_some() {
                self.track_error(
                    "field after spread",
                    "The `..spread` must be the last element in a struct expression. Move explicit fields before the spread.",
                );
                break;
            }

            let field_name_token = self.current_token();
            let field_name_span = self.span_from_token(field_name_token);
            let field_name = self.read_identifier();

            if let Some((_, first_span)) = seen_fields.iter().find(|(n, _)| n == &field_name) {
                self.error_duplicate_struct_field(&field_name, *first_span, field_name_span);
            } else {
                seen_fields.push((field_name.clone(), field_name_span));
            }

            let field_value = if self.advance_if(Colon) {
                self.parse_expression()
            } else {
                Expression::Identifier {
                    value: field_name.clone(),
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(field_name_token),
                    binding_id: None,
                    qualified: None,
                }
            };

            field_assignments.push(StructFieldAssignment {
                name: field_name,
                name_span: field_name_span,
                value: Box::new(field_value),
            });

            self.expect_comma_or(RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        Expression::StructCall {
            ty: Type::uninferred(),
            name,
            field_assignments,
            spread: spread.into(),
            span: self.span_from_offset(start_offset),
        }
    }

    pub fn parse_index_expression(&mut self, expression: Expression) -> Expression {
        let start = self.current_token();

        self.ensure(LeftSquareBracket);

        let index = self.parse_expression();

        self.ensure(RightSquareBracket);

        Expression::IndexedAccess {
            ty: Type::uninferred(),
            expression: expression.into(),
            index: index.into(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_function_call(
        &mut self,
        expression: Expression,
        type_args: Vec<Annotation>,
    ) -> Expression {
        let start_offset = expression.get_span().byte_offset;
        let (args, spread) = self.collect_call_args();

        Expression::Call {
            ty: Type::uninferred(),
            expression: expression.into(),
            args,
            spread: spread.into(),
            type_args,
            span: self.span_from_offset(start_offset),
            call_kind: None,
        }
    }

    fn collect_call_args(&mut self) -> (Vec<Expression>, Option<Expression>) {
        self.ensure(LeftParen);
        let mut args = vec![];

        while !self.at_eof() && !self.is(RightParen) {
            if self.handle_fn_as_lambda_in_call(&mut args) {
                continue;
            }
            if self.at_item_boundary()
                && !matches!(self.stream.peek_ahead(1).kind, RightParen | Comma)
            {
                break;
            }
            if self.is(DotDot) {
                return (args, Some(self.parse_spread_arg()));
            }
            args.push(self.parse_expression());
            self.expect_comma_or(RightParen);
        }

        self.advance_if(RightParen);
        (args, None)
    }

    fn handle_fn_as_lambda_in_call(&mut self, args: &mut Vec<Expression>) -> bool {
        if !(self.is(Function) && self.stream.peek_ahead(1).kind == LeftParen) {
            return false;
        }
        let start = self.current_token();
        let span = Span::new(self.file_id, start.byte_offset, start.byte_length + 1);
        let error = ParseError::new("Syntax error", span, "expected a lambda")
            .with_parse_code("fn_as_lambda")
            .with_help("Use a lambda instead: `|x| x * 2`");
        self.errors.push(error);
        self.resync_on_error();
        args.push(Expression::Unit {
            ty: Type::uninferred(),
            span,
        });
        true
    }

    fn parse_spread_arg(&mut self) -> Expression {
        self.ensure(DotDot);
        let spread = self.parse_expression();
        self.expect_comma_or(RightParen);
        if !self.is(RightParen) && !self.at_eof() {
            self.track_error(
                "argument after spread",
                "The `..spread` must be the last argument in the call.",
            );
            while !self.at_eof() && !self.is(RightParen) {
                self.next();
            }
        }
        self.advance_if(RightParen);
        spread
    }

    pub fn parse_type_args(&mut self) -> Vec<Annotation> {
        self.ensure(LeftAngleBracket);

        let mut type_args = vec![];

        loop {
            if self.at_eof() {
                break;
            }

            type_args.push(self.parse_annotation());

            if self.is(RightAngleBracket) {
                break;
            }

            self.ensure(Comma);
        }

        self.ensure(RightAngleBracket);

        type_args
    }

    pub fn parse_binary_operator(&mut self) -> BinaryOperator {
        let operator = match self.current_token().kind {
            Plus => BinaryOperator::Addition,
            Minus => BinaryOperator::Subtraction,
            Star => BinaryOperator::Multiplication,
            Slash => BinaryOperator::Division,
            LeftAngleBracket => BinaryOperator::LessThan,
            LessThanOrEqual => BinaryOperator::LessThanOrEqual,
            RightAngleBracket => BinaryOperator::GreaterThan,
            GreaterThanOrEqual => BinaryOperator::GreaterThanOrEqual,
            Percent => BinaryOperator::Remainder,
            EqualDouble => BinaryOperator::Equal,
            NotEqual => BinaryOperator::NotEqual,
            AmpersandDouble => BinaryOperator::And,
            PipeDouble => BinaryOperator::Or,
            Pipeline => BinaryOperator::Pipeline,

            _ => {
                self.track_error(format!(
                    "expected binary operator, found {}",
                    self.current_token().kind
                ), "Binary operators: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`.");
                BinaryOperator::Addition // meaningless fallback
            }
        };

        self.next();

        operator
    }

    fn parse_parenthesized_expression(&mut self) -> Expression {
        let start = self.current_token();

        let (expressions, has_trailing_comma) =
            self.collect_delimited_expressions(LeftParen, RightParen);
        let span = self.span_from_tokens(start);

        match expressions.len() {
            0 => Expression::Unit {
                ty: Type::uninferred(),
                span,
            },
            1 => {
                if has_trailing_comma {
                    self.error_tuple_arity(1, span);
                }
                let expression = expressions.into_iter().next().expect("len is 1");
                Expression::Paren {
                    ty: Type::uninferred(),
                    expression: expression.into(),
                    span,
                }
            }
            n => {
                if n > MAX_TUPLE_ARITY {
                    self.error_tuple_arity(n, span);
                }
                Expression::Tuple {
                    ty: Type::uninferred(),
                    elements: expressions,
                    span,
                }
            }
        }
    }

    pub fn parse_try(&mut self, expression: Expression) -> Expression {
        let start_offset = expression.get_span().byte_offset;

        self.ensure(QuestionMark);

        Expression::Propagate {
            ty: Type::uninferred(),
            expression: expression.into(),
            span: self.span_from_offset(start_offset),
        }
    }

    fn parse_lambda(&mut self) -> Expression {
        let start = self.current_token();

        let params = if self.is(Pipe) {
            self.parse_lambda_params()
        } else {
            self.next();
            vec![]
        };

        let has_return_type = self.is(Arrow);
        let return_annotation = if has_return_type {
            self.next();
            self.parse_annotation()
        } else {
            Annotation::Unknown
        };

        if has_return_type && self.current_token().kind != LeftCurlyBrace {
            self.track_error(
                "not allowed",
                "A lambda with a return type requires a block body",
            );
        }

        let body = self.parse_expression();

        Expression::Lambda {
            params,
            return_annotation,
            body: body.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_block_expression(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(LeftCurlyBrace);

        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            let mut brace_depth = 1u32;
            while brace_depth > 0 && !self.at_eof() {
                match self.current_token().kind {
                    LeftCurlyBrace => brace_depth += 1,
                    RightCurlyBrace => brace_depth -= 1,
                    _ => {}
                }
                if brace_depth > 0 {
                    self.next();
                }
            }
            self.advance_if(RightCurlyBrace);
            return Expression::Block {
                ty: Type::uninferred(),
                items: vec![],
                span,
            };
        }

        let mut items = vec![];

        while self.is_not(RightCurlyBrace) && !self.too_many_errors() {
            let position = self.position();
            let item = self.parse_block_item();

            self.advance_if(Semicolon);

            items.push(item);
            if self.position() == position {
                self.next();
            }
        }

        let span = self.close_brace_span(start, start);

        self.leave_recursion();

        Expression::Block {
            ty: Type::uninferred(),
            items,
            span,
        }
    }

    pub fn parse_function_params(&mut self) -> Vec<Binding> {
        self.ensure(LeftParen);

        let mut params = vec![];

        while self.is_not(RightParen) {
            params.push(self.parse_binding_with_type());
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        params
    }

    pub fn parse_lambda_params(&mut self) -> Vec<Binding> {
        self.ensure(Pipe);

        let mut params = vec![];

        while self.is_not(Pipe) {
            params.push(self.parse_binding());
            self.expect_comma_or(Pipe);
        }

        self.ensure(Pipe);

        params
    }

    pub fn parse_function(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Expression {
        let start = self.current_token();

        self.ensure(Function);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);

        let name = self.read_identifier_sequence();

        let name_span = Span::new(name_span.file_id, name_span.byte_offset, name.len() as u32);

        let generics = self.parse_generics();
        let params = self.parse_function_params();
        let return_annotation = self.parse_function_return_annotation();

        let body = if self.is(LeftCurlyBrace) {
            self.parse_block_expression()
        } else {
            Expression::NoOp
        };

        Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics,
            params,
            return_annotation,
            return_type: Type::uninferred(),
            visibility: Visibility::Private,
            body: body.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_field_access(&mut self, expression: Expression) -> Expression {
        self.ensure(Dot);

        let expression_start = expression.get_span().byte_offset;
        let start = self.current_token();

        if self.advance_if(Star) {
            return Expression::Unary {
                ty: Type::uninferred(),
                operator: UnaryOperator::Deref,
                expression: expression.into(),
                span: self.span_from_tokens(start),
            };
        }

        if self.is(Integer) {
            let text = self.current_token().text;
            let index: u32 = text.parse().unwrap_or_else(|_| {
                self.track_error(
                    format!("tuple index '{}' is too large", text),
                    "Maximum index is `4294967295`.",
                );
                0
            });

            self.ensure(Integer);

            return Expression::DotAccess {
                ty: Type::uninferred(),
                expression: expression.into(),
                member: index.to_string().into(),
                span: self.span_from_offset(expression_start),
                dot_access_kind: None,
                receiver_coercion: None,
            };
        }

        let field = self.current_token().text;

        self.ensure(Identifier);

        Expression::DotAccess {
            ty: Type::uninferred(),
            expression: expression.into(),
            member: field.into(),
            span: self.span_from_offset(expression_start),
            dot_access_kind: None,
            receiver_coercion: None,
        }
    }

    pub fn collect_delimited_expressions(
        &mut self,
        open: TokenKind,
        close: TokenKind,
    ) -> (Vec<Expression>, bool) {
        self.ensure(open);

        let mut expressions = vec![];
        let mut has_trailing_comma = false;
        loop {
            if self.at_eof() || self.is(close) {
                break;
            }

            if self.is(Function) && self.stream.peek_ahead(1).kind == LeftParen {
                let start = self.current_token();
                let span = Span::new(self.file_id, start.byte_offset, start.byte_length + 1);
                let error = ParseError::new("Syntax error", span, "expected a lambda")
                    .with_parse_code("fn_as_lambda")
                    .with_help("Use a lambda instead: `|x| x * 2`");
                self.errors.push(error);
                self.resync_on_error();
                expressions.push(Expression::Unit {
                    ty: Type::uninferred(),
                    span,
                });
                continue;
            }

            if self.at_item_boundary() {
                let next = self.stream.peek_ahead(1).kind;
                if next != close && next != Comma {
                    break;
                }
            }
            expressions.push(self.parse_expression());
            has_trailing_comma = self.is(Comma);
            self.expect_comma_or(close);
        }

        self.advance_if(close);

        (expressions, has_trailing_comma)
    }

    fn make_expression_name(&mut self, expression: &Expression) -> EcoString {
        let mut parts = Vec::new();
        let mut current = expression;

        loop {
            match current {
                Expression::Identifier { value, .. } => {
                    parts.push(value.clone());
                    break;
                }
                Expression::DotAccess {
                    expression, member, ..
                } => {
                    parts.push(member.clone());
                    current = expression;
                }
                _ => {
                    self.track_error(
                        "unexpected expression",
                        "Expected an identifier or dotted path.",
                    );
                    return "_".into();
                }
            }
        }

        parts.reverse();
        parts.join(".").into()
    }

    pub fn parse_let(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Let);

        let (mutable, mut_span) = if self.is(Mut) {
            let mut_token = self.current_token();
            let span = Span::new(self.file_id, mut_token.byte_offset, mut_token.byte_length);
            self.next(); // consume `mut`
            (true, Some(span))
        } else {
            (false, None)
        };

        let binding = self.parse_binding_allowing_or();

        self.ensure(Equal);

        let expression = self.parse_expression();

        let (else_block, else_span) = if self.is(Else) {
            let else_token = self.current_token();
            let span = Span::new(self.file_id, else_token.byte_offset, else_token.byte_length);
            self.next(); // consume `else`
            (Some(Box::new(self.parse_block_expression())), Some(span))
        } else {
            (None, None)
        };

        Expression::Let {
            binding: Box::new(binding),
            value: expression.into(),
            mutable,
            mut_span,
            else_block,
            else_span,
            typed_pattern: None,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_import(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Import);

        let alias = if self.current_token().kind == Identifier {
            let alias_token = self.current_token();
            let alias_text = alias_token.text;
            let alias_span = Span::new(
                self.file_id,
                alias_token.byte_offset,
                alias_token.byte_length,
            );

            if alias_text == "_" {
                self.next();
                Some(ImportAlias::Blank(alias_span))
            } else if self.stream.peek_ahead(1).kind == String {
                self.next();
                Some(ImportAlias::Named(alias_text.into(), alias_span))
            } else {
                None
            }
        } else {
            None
        };

        let name_token = self.current_token();

        if name_token.kind != String {
            let (label, help) = if name_token.kind == Identifier
                && self.stream.peek_ahead(1).kind == Colon
            {
                let module_name = name_token.text;
                (
                    "expected double quotes".to_string(),
                    format!(
                        "Wrap the import path in double quotes: `import \"{0}:...\"`",
                        module_name
                    ),
                )
            } else if name_token.kind == Identifier {
                let module_name = name_token.text;
                (
                    "expected double quotes".to_string(),
                    format!(
                        "Wrap the import path in double quotes: `import \"{}\"`",
                        module_name
                    ),
                )
            } else {
                (
                    "expected module path".to_string(),
                    "Wrap the import path in double quotes, e.g. `import \"go:os\"`".to_string(),
                )
            };

            self.track_error(label, help);
            self.resync_on_error();
            return Expression::Unit {
                ty: Type::uninferred(),
                span: self.span_from_tokens(start),
            };
        }

        self.next();

        let raw = name_token.text;
        let name: EcoString = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
            raw[1..raw.len() - 1].into()
        } else {
            debug_assert!(
                false,
                "lexer produced String token without quotes: {:?}",
                raw
            );
            raw.into()
        };
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);

        Expression::ModuleImport {
            name,
            name_span,
            alias,
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_assignment(&mut self) -> Expression {
        let start = self.current_token();

        let lhs = self.parse_expression();

        let compound_operator = match self.current_token().kind {
            PlusEqual => Some(BinaryOperator::Addition),
            MinusEqual => Some(BinaryOperator::Subtraction),
            StarEqual => Some(BinaryOperator::Multiplication),
            SlashEqual => Some(BinaryOperator::Division),
            PercentEqual => Some(BinaryOperator::Remainder),
            _ => None,
        };

        if let Some(operator) = compound_operator {
            if !self.is_valid_assignment_target(&lhs) {
                self.track_error(
                    "invalid assignment target",
                    "Only variables, fields, and indices can be assigned to.",
                );
            }
            self.next();
            let rhs = self.parse_expression();
            return Expression::Assignment {
                target: lhs.clone().into(),
                value: Expression::Binary {
                    left: lhs.into(),
                    operator,
                    right: rhs.into(),
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                }
                .into(),
                compound_operator: Some(operator),
                span: self.span_from_tokens(start),
            };
        }

        if self.current_token().kind == Colon && self.stream.peek_ahead(1).kind == Equal {
            let span = Span::new(self.file_id, self.current_token().byte_offset, 2);
            self.track_error_at(
                span,
                "Go-style short declaration",
                "Use `let x = ...` instead of `:=` for variable declarations",
            );
            self.next();
            self.next();
            let _ = self.parse_expression();
            return lhs;
        }

        if !self.is(Equal) {
            return lhs;
        }

        if !self.is_valid_assignment_target(&lhs) {
            self.track_error(
                "invalid assignment target",
                "Only variables, fields, and indices can be assigned to.",
            );
        }

        self.ensure(Equal);

        Expression::Assignment {
            target: lhs.into(),
            value: self.parse_expression().into(),
            compound_operator: None,
            span: self.span_from_tokens(start),
        }
    }

    fn is_valid_assignment_target(&self, expression: &Expression) -> bool {
        use Expression::*;

        matches!(
            expression,
            Identifier { .. }
                | DotAccess { .. }
                | IndexedAccess { .. }
                | Unary {
                    operator: UnaryOperator::Deref,
                    ..
                }
        )
    }

    fn parse_format_string(&mut self) -> Expression {
        let start = self.current_token();
        self.ensure(FormatStringStart);

        let mut parts = Vec::new();

        loop {
            if self.at_eof() || self.at_item_boundary() {
                break;
            }
            match self.current_token().kind {
                FormatStringText => {
                    let text = self.current_token().text;
                    self.next();
                    parts.push(FormatStringPart::Text(text.to_string()));
                }
                FormatStringInterpolationStart => {
                    self.ensure(FormatStringInterpolationStart);
                    let expression = self.parse_expression();
                    parts.push(FormatStringPart::Expression(Box::new(expression)));
                    if self.is(Colon) {
                        let start_offset = self.current_token().byte_offset;
                        self.next();
                        while !self.at_eof()
                            && !self.is(FormatStringInterpolationEnd)
                            && !self.is(FormatStringEnd)
                            && !self.at_item_boundary()
                        {
                            self.next();
                        }
                        let span = self.span_from_offset(start_offset);
                        let error = ParseError::new(
                            "Format specifiers not supported",
                            span,
                            "not supported in format strings",
                        )
                        .with_parse_code("format_specifier")
                        .with_help(
                            "Use `fmt.Sprintf` for formatted output, e.g. `fmt.Sprintf(\"%02x\", n)`",
                        );
                        self.errors.push(error);
                    }
                    self.advance_if(FormatStringInterpolationEnd);
                }
                FormatStringEnd => {
                    self.ensure(FormatStringEnd);
                    break;
                }
                _ => break,
            }
        }

        Expression::Literal {
            literal: Literal::FormatString(parts),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_task(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Task);

        let expression = if self.is(LeftCurlyBrace) {
            self.parse_block_expression()
        } else {
            self.parse_expression()
        };

        if !matches!(
            expression,
            Expression::Call { .. } | Expression::Block { .. }
        ) {
            let span = expression.get_span();
            let error = ParseError::new("Invalid `task`", span, "expected `()`")
                .with_parse_code("task_missing_parens")
                .with_help("Add parens to call the function");

            self.errors.push(error);
        }

        Expression::Task {
            expression: Box::new(expression),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_defer(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Defer);

        let expression = if self.is(LeftCurlyBrace) {
            self.parse_block_expression()
        } else {
            self.parse_expression()
        };

        if !matches!(
            expression,
            Expression::Call { .. } | Expression::Block { .. }
        ) {
            let span = expression.get_span();
            let error = ParseError::new("Invalid `defer`", span, "expected `()`")
                .with_parse_code("defer_missing_parens")
                .with_help("Add parens to call the function");

            self.errors.push(error);
        }

        Expression::Defer {
            expression: Box::new(expression),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_try_block(&mut self) -> Expression {
        let start = self.current_token();
        let try_keyword_span = Span::new(self.file_id, start.byte_offset, start.byte_length);

        self.ensure(Try);

        if !self.is(LeftCurlyBrace) {
            let span = self.span_from_tokens(start);
            let error = ParseError::new("Invalid `try`", span, "requires a block")
                .with_parse_code("syntax_error")
                .with_help("Use `try { expression }` instead of `try expression`");
            self.errors.push(error);
            let expression = self.parse_expression();
            return Expression::TryBlock {
                items: vec![expression],
                ty: Type::uninferred(),
                try_keyword_span,
                span: self.span_from_tokens(start),
            };
        }

        let brace_token = self.current_token();
        self.ensure(LeftCurlyBrace);

        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            let mut brace_depth = 1u32;
            while brace_depth > 0 && !self.at_eof() {
                match self.current_token().kind {
                    LeftCurlyBrace => brace_depth += 1,
                    RightCurlyBrace => brace_depth -= 1,
                    _ => {}
                }
                if brace_depth > 0 {
                    self.next();
                }
            }
            self.advance_if(RightCurlyBrace);
            return Expression::TryBlock {
                items: vec![],
                ty: Type::uninferred(),
                try_keyword_span,
                span,
            };
        }

        let mut items = vec![];

        while self.is_not(RightCurlyBrace) && !self.too_many_errors() {
            let position = self.position();
            let item = self.parse_block_item();

            self.advance_if(Semicolon);

            items.push(item);
            if self.position() == position {
                self.next();
            }
        }

        let span = self.close_brace_span(start, brace_token);

        self.leave_recursion();

        Expression::TryBlock {
            items,
            ty: Type::uninferred(),
            try_keyword_span,
            span,
        }
    }

    pub fn parse_recover_block(&mut self) -> Expression {
        let start = self.current_token();
        let recover_keyword_span = Span::new(self.file_id, start.byte_offset, start.byte_length);

        self.ensure(Recover);

        if !self.is(LeftCurlyBrace) {
            let span = self.span_from_tokens(start);
            let error = ParseError::new("Invalid `recover`", span, "requires a block")
                .with_parse_code("syntax_error")
                .with_help("Use `recover { expression }` instead of `recover expression`");
            self.errors.push(error);
            let expression = self.parse_expression();
            return Expression::RecoverBlock {
                items: vec![expression],
                ty: Type::uninferred(),
                recover_keyword_span,
                span: self.span_from_tokens(start),
            };
        }

        let brace_token = self.current_token();
        self.ensure(LeftCurlyBrace);

        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            let mut brace_depth = 1u32;
            while brace_depth > 0 && !self.at_eof() {
                match self.current_token().kind {
                    LeftCurlyBrace => brace_depth += 1,
                    RightCurlyBrace => brace_depth -= 1,
                    _ => {}
                }
                if brace_depth > 0 {
                    self.next();
                }
            }
            self.advance_if(RightCurlyBrace);
            return Expression::RecoverBlock {
                items: vec![],
                ty: Type::uninferred(),
                recover_keyword_span,
                span,
            };
        }

        let mut items = vec![];

        while self.is_not(RightCurlyBrace) && !self.too_many_errors() {
            let position = self.position();
            let item = self.parse_block_item();

            self.advance_if(Semicolon);

            items.push(item);
            if self.position() == position {
                self.next();
            }
        }

        let span = self.close_brace_span(start, brace_token);

        self.leave_recursion();

        Expression::RecoverBlock {
            items,
            ty: Type::uninferred(),
            recover_keyword_span,
            span,
        }
    }

    pub fn parse_select(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Select);
        self.ensure(LeftCurlyBrace);

        let mut arms = Vec::new();

        while self.is_not(RightCurlyBrace) {
            let arm = self.parse_select_arm();
            arms.push(arm);

            if self.is(RightCurlyBrace) {
                break;
            }

            self.ensure(Comma);
        }

        self.ensure(RightCurlyBrace);

        Expression::Select {
            arms,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_select_arm(&mut self) -> SelectArm {
        match self.current_token().kind {
            Let => {
                self.ensure(Let);
                let binding = self.parse_pattern();
                self.ensure(Equal);
                let receive_expression = Box::new(self.parse_expression());
                self.ensure(ArrowDouble);
                let body = Box::new(self.parse_expression());
                SelectArm {
                    pattern: SelectArmPattern::Receive {
                        binding: Box::new(binding),
                        typed_pattern: None,
                        receive_expression,
                        body,
                    },
                }
            }
            Match => {
                let match_expression = self.parse_match();
                if let Expression::Match { subject, arms, .. } = match_expression {
                    SelectArm {
                        pattern: SelectArmPattern::MatchReceive {
                            receive_expression: subject,
                            arms,
                        },
                    }
                } else {
                    self.ensure(ArrowDouble);
                    let body = Box::new(self.parse_expression());
                    SelectArm {
                        pattern: SelectArmPattern::Send {
                            send_expression: Box::new(match_expression),
                            body,
                        },
                    }
                }
            }
            Identifier if self.current_token().text == "_" => {
                self.next();
                self.ensure(ArrowDouble);
                let body = Box::new(self.parse_expression());
                SelectArm {
                    pattern: SelectArmPattern::WildCard { body },
                }
            }
            _ => {
                let send_expression = Box::new(self.parse_expression());
                self.ensure(ArrowDouble);
                let body = Box::new(self.parse_expression());
                SelectArm {
                    pattern: SelectArmPattern::Send {
                        send_expression,
                        body,
                    },
                }
            }
        }
    }

    pub fn with_control_flow_header<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let old = self.in_control_flow_header;
        self.in_control_flow_header = true;
        let result = f(self);
        self.in_control_flow_header = old;
        result
    }

    fn keyword_in_value_position(&self) -> bool {
        if !self.current_token().kind.is_keyword() {
            return false;
        }

        match self.current_token().kind {
            Return | Break | Continue => false,

            Match | If | Task | Defer | Try | Recover | Select | Loop | Function => matches!(
                self.stream.peek_ahead(1).kind,
                RightParen
                    | Comma
                    | Dot
                    | Semicolon
                    | RightCurlyBrace
                    | RightSquareBracket
                    | ArrowDouble
                    | QuestionMark
                    | EOF
                    | Plus
                    | Star
                    | Slash
                    | Percent
                    | EqualDouble
                    | NotEqual
                    | LeftAngleBracket
                    | RightAngleBracket
                    | LessThanOrEqual
                    | GreaterThanOrEqual
                    | AmpersandDouble
                    | Pipeline
                    | Equal
                    | PlusEqual
                    | MinusEqual
                    | StarEqual
                    | SlashEqual
                    | PercentEqual
                    | DotDot
                    | DotDotEqual
                    | As
            ),

            _ => true,
        }
    }

    fn recover_keyword_as_identifier(&mut self) -> Expression {
        let token = self.current_token();
        let keyword = token.text.to_string();
        let span = self.span_from_token(token);
        let error = ParseError::new("Reserved keyword", span, "reserved keyword")
            .with_parse_code("keyword_as_identifier")
            .with_help(format!("Rename `{}`", keyword));
        self.errors.push(error);
        self.next();
        Expression::Identifier {
            value: keyword.into(),
            ty: Type::uninferred(),
            span,
            binding_id: None,
            qualified: None,
        }
    }
}
