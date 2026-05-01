use super::Formatter;
use super::sequence::{PatternEntry, SiblingEntry};
use crate::INDENT_WIDTH;
use crate::comments::prepend_comments;
use crate::lindig::{Document, concat, flex_break, join, strict_break};
use syntax::ast::{
    Annotation, BinaryOperator, Binding, Expression, FormatStringPart, Literal, MatchArm, Pattern,
    SelectArm, SelectArmPattern, Span, StructFieldAssignment, StructSpread, UnaryOperator,
};

impl<'a> Formatter<'a> {
    pub fn expression(&mut self, expression: &'a Expression) -> Document<'a> {
        let start = expression.get_span().byte_offset;
        let comments = self.comments.take_comments_before(start);

        let doc = match expression {
            Expression::Literal { literal, .. } => self.literal(literal),
            Expression::Identifier { value, .. } => Document::string(value.to_string()),
            Expression::Unit { .. } => Document::str("()"),
            Expression::Break { value, .. } => {
                if let Some(val) = value {
                    Document::str("break ").append(self.expression(val))
                } else {
                    Document::str("break")
                }
            }
            Expression::Continue { .. } => Document::str("continue"),
            Expression::NoOp => Document::Sequence(vec![]),

            Expression::Paren { expression, .. } => Document::str("(")
                .append(self.expression(expression))
                .append(")"),

            Expression::Block { items, span, .. } => self.block(items, span),

            Expression::Let {
                binding,
                value,
                mutable,
                else_block,
                ..
            } => self.let_(binding, value, *mutable, else_block.as_deref()),

            Expression::Return { expression, .. } => self.return_(expression),

            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => self.if_(condition, consequence, alternative),

            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } => self.if_let(pattern, scrutinee, consequence, alternative),

            Expression::Match {
                subject,
                arms,
                span,
                ..
            } => self.match_(subject, arms, span),

            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => self.binary_operator(operator, left, right),

            Expression::Unary {
                operator,
                expression,
                ..
            } => self.unary_operator(operator, expression),

            Expression::Call {
                expression,
                args,
                spread,
                type_args,
                ..
            } => self.call(expression, args, spread, type_args),

            Expression::DotAccess {
                expression, member, ..
            } => self.dot_access(expression, member),

            Expression::IndexedAccess {
                expression, index, ..
            } => self.indexed_access(expression, index),

            Expression::Tuple { elements, .. } => self.tuple(elements),

            Expression::StructCall {
                name,
                field_assignments,
                spread,
                ..
            } => self.struct_call(name, field_assignments, spread),

            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => self.assignment(target, value, *compound_operator),

            Expression::Loop { body, .. } => self.loop_(body),

            Expression::While {
                condition, body, ..
            } => self.while_(condition, body),

            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                ..
            } => self.while_let(pattern, scrutinee, body),

            Expression::For {
                binding,
                iterable,
                body,
                ..
            } => self.for_(binding, iterable, body),

            Expression::Task { expression, .. } => self.task(expression),
            Expression::Defer { expression, .. } => self.defer_(expression),
            Expression::Select { arms, span, .. } => self.select(arms, span),
            Expression::Propagate { expression, .. } => self.propagate_(expression),
            Expression::Reference { expression, .. } => self.ref_(expression),
            Expression::RawGo { text } => Self::raw_go(text),

            Expression::TryBlock { items, span, .. } => self.try_block(items, span),
            Expression::RecoverBlock { items, span, .. } => self.recover_block(items, span),
            Expression::Range {
                start,
                end,
                inclusive,
                ..
            } => self.range(start, end, *inclusive),
            Expression::Cast {
                expression,
                target_type,
                ..
            } => self.cast(expression, target_type),

            Expression::Lambda {
                params,
                return_annotation,
                body,
                span,
                ..
            } => self.lambda(params, return_annotation, body, span),

            _ => self.definition(expression),
        };

        prepend_comments(doc, comments)
    }

    pub(super) fn literal(&mut self, literal: &'a Literal) -> Document<'a> {
        match literal {
            Literal::Integer { value, text } => {
                if let Some(original) = text {
                    Document::string(original.clone())
                } else {
                    Document::string(value.to_string())
                }
            }
            Literal::Float { value, text } => match text {
                Some(t) => Document::string(t.clone()),
                None => {
                    let s = value.to_string();
                    if s.contains('.') || s.contains('e') || s.contains('E') {
                        Document::string(s)
                    } else {
                        Document::string(format!("{}.0", s))
                    }
                }
            },
            Literal::Imaginary(coef) => {
                if *coef == coef.trunc() && coef.abs() < 1e15 {
                    Document::string(format!("{}i", *coef as i64))
                } else {
                    Document::string(format!("{}i", coef))
                }
            }
            Literal::Boolean(b) => Document::str(if *b { "true" } else { "false" }),
            Literal::String { value, raw: true } if value.contains('\n') => {
                Document::verbatim(format!("r\"{value}\""))
            }
            Literal::String { value, raw: true } => Document::string(format!("r\"{value}\"")),
            Literal::String { value, raw: false } if value.contains('\n') => {
                Document::verbatim(format!("\"{value}\""))
            }
            Literal::String { value, raw: false } => Document::string(format!("\"{value}\"")),
            Literal::Char(c) => Document::string(format!("'{c}'")),
            Literal::Slice(elements) => self.slice(elements),
            Literal::FormatString(parts) => self.format_string(parts),
        }
    }

    pub(super) fn slice(&mut self, elements: &'a [Expression]) -> Document<'a> {
        if elements.is_empty() {
            return Document::str("[]");
        }

        let elements_docs: Vec<_> = elements.iter().map(|e| self.expression(e)).collect();
        let elements_doc = join(elements_docs, strict_break(",", ", "));

        Document::str("[")
            .append(strict_break("", ""))
            .append(elements_doc)
            .nest(INDENT_WIDTH)
            .append(strict_break(",", ""))
            .append("]")
            .group()
    }

    pub(super) fn format_string(&mut self, parts: &'a [FormatStringPart]) -> Document<'a> {
        let mut docs = vec![Document::str("f\"")];

        for part in parts {
            match part {
                FormatStringPart::Text(s) if s.contains('\n') => {
                    docs.push(Document::verbatim(s.clone()))
                }
                FormatStringPart::Text(s) => docs.push(Document::string(s.clone())),
                FormatStringPart::Expression(e) => {
                    docs.push(Document::str("{"));
                    docs.push(self.expression(e));
                    docs.push(Document::str("}"));
                }
            }
        }

        docs.push(Document::str("\""));
        concat(docs)
    }

    pub(super) fn block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
        let block_end = span.byte_offset + span.byte_length;

        if items.is_empty() {
            return match self.comments.take_comments_before(block_end) {
                Some(c) => Document::str("{")
                    .append(Document::Newline.append(c).nest(INDENT_WIDTH))
                    .append(Document::Newline)
                    .append("}")
                    .force_break(),
                None => Document::str("{}"),
            };
        }

        let mut docs = Vec::new();

        for (i, item) in items.iter().enumerate() {
            let start = item.get_span().byte_offset;

            if i > 0 {
                if self.comments.take_empty_lines_before(start) {
                    docs.push(Document::Newline);
                    docs.push(Document::Newline);
                } else {
                    docs.push(Document::Newline);
                }
            }

            docs.push(self.expression(item));
        }

        let (same_line, standalone, _) = self.comments.take_split_at_line_start(block_end);
        if let Some(t) = same_line {
            docs.push(Document::str(" "));
            docs.push(t);
        }
        if let Some(t) = standalone {
            docs.push(Document::Newline);
            docs.push(t.force_break());
        }

        let body = concat(docs);

        Document::str("{")
            .append(Document::Newline.append(body).nest(INDENT_WIDTH))
            .append(Document::Newline)
            .append("}")
            .force_break()
    }

    pub(super) fn let_(
        &mut self,
        binding: &'a Binding,
        value: &'a Expression,
        mutable: bool,
        else_block: Option<&'a Expression>,
    ) -> Document<'a> {
        let keyword = if mutable { "let mut " } else { "let " };

        let base = Document::str(keyword)
            .append(self.binding(binding))
            .append(" = ")
            .append(self.expression(value));

        if let Some(else_expression) = else_block {
            base.append(" else ").append(self.as_block(else_expression))
        } else {
            base
        }
    }

    pub(super) fn return_(&mut self, expression: &'a Expression) -> Document<'a> {
        if matches!(expression, Expression::Unit { .. }) {
            Document::str("return")
        } else {
            Document::str("return ").append(self.expression(expression))
        }
    }

    pub(super) fn if_(
        &mut self,
        condition: &'a Expression,
        consequence: &'a Expression,
        alternative: &'a Expression,
    ) -> Document<'a> {
        let if_doc = Document::str("if ")
            .append(self.expression(condition))
            .append(" ")
            .append(self.as_inline_block(consequence));

        match alternative {
            Expression::Unit { .. } => if_doc,
            Expression::If { .. } | Expression::IfLet { .. } => {
                if_doc.append(" else ").append(self.expression(alternative))
            }
            _ => if_doc
                .append(" else ")
                .append(self.as_inline_block(alternative)),
        }
        .group()
    }

    pub(super) fn if_let(
        &mut self,
        pattern: &'a Pattern,
        scrutinee: &'a Expression,
        consequence: &'a Expression,
        alternative: &'a Expression,
    ) -> Document<'a> {
        let if_let_doc = Document::str("if let ")
            .append(self.pattern(pattern))
            .append(" = ")
            .append(self.expression(scrutinee))
            .append(" ")
            .append(self.as_inline_block(consequence));

        match alternative {
            Expression::Unit { .. } => if_let_doc,
            Expression::If { .. } | Expression::IfLet { .. } => if_let_doc
                .append(" else ")
                .append(self.expression(alternative)),
            _ => if_let_doc
                .append(" else ")
                .append(self.as_inline_block(alternative)),
        }
        .group()
    }

    pub(super) fn as_block(&mut self, expression: &'a Expression) -> Document<'a> {
        match expression {
            Expression::Block { items, span, .. } => self.block(items, span),
            Expression::NoOp => Document::Sequence(vec![]),
            _ => Document::str("{ ")
                .append(self.expression(expression))
                .append(" }"),
        }
    }

    /// Like as_block, but allows single-expression blocks to stay inline.
    /// Used for if/else branches where `{ expression }` should stay on one line
    /// when the containing group fits, and expand to multi-line when it doesn't.
    pub(super) fn as_inline_block(&mut self, expression: &'a Expression) -> Document<'a> {
        match expression {
            Expression::Block { items, span, .. } => {
                if items.len() == 1 && !self.comments.has_comments_in_range(*span) {
                    let expression = self.expression(&items[0]);
                    return Document::str("{")
                        .append(strict_break("", " ").append(expression).nest(INDENT_WIDTH))
                        .append(strict_break("", " "))
                        .append("}");
                }
                self.block(items, span)
            }
            Expression::NoOp => Document::Sequence(vec![]),
            _ => {
                let expression = self.expression(expression);
                Document::str("{")
                    .append(strict_break("", " ").append(expression).nest(INDENT_WIDTH))
                    .append(strict_break("", " "))
                    .append("}")
            }
        }
    }

    pub(super) fn match_arm_entries(&mut self, arms: &'a [MatchArm]) -> Vec<SiblingEntry<'a>> {
        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(arms.len());
        for arm in arms {
            let start = arm.pattern.get_span().byte_offset;
            self.push_sibling_entry(&mut entries, start, |s| {
                let pattern = s.pattern(&arm.pattern);
                let expression = s.expression(&arm.expression);
                let pattern_with_guard = if let Some(guard) = &arm.guard {
                    pattern.append(" if ").append(s.expression(guard))
                } else {
                    pattern
                };
                pattern_with_guard
                    .append(" => ")
                    .append(expression)
                    .append(",")
            });
        }
        entries
    }

    pub(super) fn match_(
        &mut self,
        subject: &'a Expression,
        arms: &'a [MatchArm],
        span: &Span,
    ) -> Document<'a> {
        let entries = self.match_arm_entries(arms);

        let header = Document::str("match ").append(self.expression(subject));
        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(header, body)
    }

    pub(super) fn loop_(&mut self, body: &'a Expression) -> Document<'a> {
        Document::str("loop ").append(self.as_block(body))
    }

    pub(super) fn while_(
        &mut self,
        condition: &'a Expression,
        body: &'a Expression,
    ) -> Document<'a> {
        Document::str("while ")
            .append(self.expression(condition))
            .append(" ")
            .append(self.as_block(body))
    }

    pub(super) fn while_let(
        &mut self,
        pattern: &'a Pattern,
        scrutinee: &'a Expression,
        body: &'a Expression,
    ) -> Document<'a> {
        Document::str("while let ")
            .append(self.pattern(pattern))
            .append(" = ")
            .append(self.expression(scrutinee))
            .append(" ")
            .append(self.as_block(body))
    }

    pub(super) fn for_(
        &mut self,
        binding: &'a Binding,
        iterable: &'a Expression,
        body: &'a Expression,
    ) -> Document<'a> {
        Document::str("for ")
            .append(self.binding(binding))
            .append(" in ")
            .append(self.expression(iterable))
            .append(" ")
            .append(self.as_block(body))
    }

    pub(super) fn binary_operator(
        &mut self,
        operator: &BinaryOperator,
        left_operand: &'a Expression,
        right_operand: &'a Expression,
    ) -> Document<'a> {
        use BinaryOperator::*;

        if matches!(operator, Pipeline) {
            return self.pipeline(left_operand, right_operand);
        }

        let operator_string = match operator {
            Addition => "+",
            Subtraction => "-",
            Multiplication => "*",
            Division => "/",
            Remainder => "%",
            LessThan => "<",
            LessThanOrEqual => "<=",
            GreaterThan => ">",
            GreaterThanOrEqual => ">=",
            Equal => "==",
            NotEqual => "!=",
            And => "&&",
            Or => "||",
            Pipeline => unreachable!(),
        };

        self.expression(left_operand)
            .append(" ")
            .append(operator_string)
            .append(strict_break("", " "))
            .append(self.expression(right_operand))
            .group()
    }

    pub(super) fn pipeline(&mut self, left: &'a Expression, right: &'a Expression) -> Document<'a> {
        let mut segments = vec![right];
        let mut current = left;

        while let Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left: l,
            right: r,
            ..
        } = current
        {
            segments.push(r);
            current = l;
        }
        segments.push(current);
        segments.reverse();

        if segments.len() == 2 {
            return self
                .expression(segments[0])
                .append(flex_break("", " "))
                .append("|> ")
                .append(self.expression(segments[1]))
                .nest_if_broken(INDENT_WIDTH)
                .group();
        }

        let docs: Vec<_> = segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                if i == 0 {
                    self.expression(seg)
                } else {
                    Document::Newline.append("|> ").append(self.expression(seg))
                }
            })
            .collect();

        concat(docs).nest(INDENT_WIDTH)
    }

    pub(super) fn unary_operator(
        &mut self,
        operator: &UnaryOperator,
        expression: &'a Expression,
    ) -> Document<'a> {
        match operator {
            UnaryOperator::Negative => Document::str("-").append(self.expression(expression)),
            UnaryOperator::Not => Document::str("!").append(self.expression(expression)),
            UnaryOperator::Deref => self.expression(expression).append(".*"),
        }
    }

    pub(super) fn call(
        &mut self,
        callee: &'a Expression,
        args: &'a [Expression],
        spread: &'a Option<Expression>,
        type_args: &'a [Annotation],
    ) -> Document<'a> {
        if let Expression::DotAccess {
            expression: inner,
            member,
            span,
            ..
        } = callee
        {
            let (root, mut chain_segments) = collect_method_chain(inner);
            let member_start = span.byte_offset + span.byte_length - member.len() as u32;
            chain_segments.push(MethodChainSegment {
                member,
                member_start,
                args,
                spread,
                type_args,
            });
            if chain_segments.len() >= 2 {
                return self.format_method_chain(root, &chain_segments);
            }
            // Single-segment chain: probe-format the root to drain any inner-receiver
            // comments, then check if comments remain before the member. If so, there
            // are genuine inter-segment comments and we should use chain formatting.
            let snapshot = self.comments.cursor_snapshot();
            let root_doc = self.expression(root);
            let has_inter_segment_comments = self
                .comments
                .has_comments_before(chain_segments[0].member_start);
            if has_inter_segment_comments {
                return self.format_method_chain_with_root(root_doc, &chain_segments);
            }
            self.comments.restore_cursor(snapshot);
        }

        let head = self
            .expression(callee)
            .append(Self::format_type_args(type_args));
        self.format_call_with_head(head, args, spread)
    }

    pub(super) fn format_type_args(type_args: &'a [Annotation]) -> Document<'a> {
        if type_args.is_empty() {
            Document::Sequence(vec![])
        } else {
            let types: Vec<_> = type_args.iter().map(Self::annotation).collect();
            Document::str("<")
                .append(join(types, Document::str(", ")))
                .append(">")
        }
    }

    pub(super) fn format_call_with_head(
        &mut self,
        head: Document<'a>,
        args: &'a [Expression],
        spread: &'a Option<Expression>,
    ) -> Document<'a> {
        if args.is_empty() && spread.is_none() {
            return head.append("()");
        }

        if let Some(spread_expr) = spread {
            if args.is_empty() {
                let spread_doc = Document::str("..").append(self.expression(spread_expr));
                return head
                    .append("(")
                    .append(spread_doc.group().next_break_fits(true))
                    .append(")")
                    .next_break_fits(false)
                    .group();
            }
            let mut entries = self.call_arg_entries(args);
            let dots_pos = spread_expr.get_span().byte_offset.saturating_sub(2);
            let spread_leading = self.split_for_rest(&mut entries, dots_pos);
            let spread_doc = Document::str("..").append(self.expression(spread_expr));
            let (body, close_sep) =
                Self::join_pattern_entries(entries, Some((spread_leading, spread_doc)), "");
            return head
                .append("(")
                .append(strict_break("", ""))
                .append(body)
                .nest(INDENT_WIDTH)
                .append(close_sep)
                .append(")")
                .next_break_fits(false)
                .group();
        }

        let Some((last, init)) = args
            .split_last()
            .filter(|(last, _)| is_inlinable_arg(last, args.len()))
        else {
            let entries = self.call_arg_entries(args);
            let (body, close_sep) = Self::join_pattern_entries(entries, None, "");
            return head
                .append("(")
                .append(strict_break("", ""))
                .append(body)
                .nest(INDENT_WIDTH)
                .append(close_sep)
                .append(")")
                .group();
        };

        if init.is_empty() {
            let last_doc = self.expression(last).group().next_break_fits(true);
            head.append("(")
                .append(last_doc)
                .append(")")
                .next_break_fits(false)
                .group()
        } else {
            let mut entries = self.call_arg_entries(init);
            let last_start = last.get_span().byte_offset;
            let last_leading = self.split_for_rest(&mut entries, last_start);
            let last_doc = self.expression(last).group().next_break_fits(true);
            let (body, close_sep) =
                Self::join_pattern_entries(entries, Some((last_leading, last_doc)), "");
            head.append("(")
                .append(strict_break("", ""))
                .append(body)
                .nest(INDENT_WIDTH)
                .append(close_sep)
                .append(")")
                .next_break_fits(false)
                .group()
        }
    }

    pub(super) fn call_arg_entries(&mut self, args: &'a [Expression]) -> Vec<PatternEntry<'a>> {
        let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(args.len());
        for arg in args {
            self.push_pattern_entry(&mut entries, arg.get_span().byte_offset, |s| {
                s.expression(arg)
            });
        }
        entries
    }

    fn format_method_chain(
        &mut self,
        root: &'a Expression,
        segments: &[MethodChainSegment<'a>],
    ) -> Document<'a> {
        let root_doc = self.expression(root);
        self.format_method_chain_with_root(root_doc, segments)
    }

    fn format_method_chain_with_root(
        &mut self,
        root_doc: Document<'a>,
        segments: &[MethodChainSegment<'a>],
    ) -> Document<'a> {
        let segment_docs: Vec<Document<'a>> = segments
            .iter()
            .map(|seg| {
                let comments = self.comments.take_comments_before(seg.member_start);
                let head = Document::str(".")
                    .append(seg.member)
                    .append(Self::format_type_args(seg.type_args));
                let call_doc = strict_break("", "")
                    .append(self.format_call_with_head(head, seg.args, seg.spread));
                match comments {
                    Some(c) => strict_break("", "")
                        .append(c)
                        .force_break()
                        .append(call_doc),
                    None => call_doc,
                }
            })
            .collect();

        root_doc
            .append(concat(segment_docs).nest_if_broken(INDENT_WIDTH))
            .group()
    }

    pub(super) fn dot_access(
        &mut self,
        expression: &'a Expression,
        member: &'a str,
    ) -> Document<'a> {
        self.expression(expression).append(".").append(member)
    }

    pub(super) fn indexed_access(
        &mut self,
        expression: &'a Expression,
        index: &'a Expression,
    ) -> Document<'a> {
        self.expression(expression)
            .append("[")
            .append(self.expression(index))
            .append("]")
    }

    pub(super) fn tuple(&mut self, elements: &'a [Expression]) -> Document<'a> {
        if elements.is_empty() {
            return Document::str("()");
        }

        let elements_docs: Vec<_> = elements.iter().map(|e| self.expression(e)).collect();
        let elements_doc = join(elements_docs, strict_break(",", ", "));

        Document::str("(")
            .append(strict_break("", ""))
            .append(elements_doc)
            .nest(INDENT_WIDTH)
            .append(strict_break(",", ""))
            .append(")")
            .group()
    }

    pub(super) fn struct_call(
        &mut self,
        name: &'a str,
        fields: &'a [StructFieldAssignment],
        spread: &'a StructSpread,
    ) -> Document<'a> {
        let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(fields.len());
        for f in fields {
            self.push_pattern_entry(&mut entries, f.name_span.byte_offset, |s| {
                if let Expression::Identifier { value, .. } = &*f.value
                    && value == &f.name
                {
                    Document::string(f.name.to_string())
                } else {
                    Document::string(f.name.to_string())
                        .append(": ")
                        .append(s.expression(&f.value))
                }
            });
        }

        let rest_info = match spread {
            StructSpread::None => None,
            StructSpread::From(spread_expression) => {
                let dots_pos = spread_expression.get_span().byte_offset.saturating_sub(2);
                let leading = self.split_for_rest(&mut entries, dots_pos);
                Some((
                    leading,
                    Document::str("..").append(self.expression(spread_expression)),
                ))
            }
            StructSpread::ZeroFill { span } => {
                let leading = self.split_for_rest(&mut entries, span.byte_offset);
                Some((leading, Document::str("..")))
            }
        };

        if entries.is_empty() && rest_info.is_none() {
            return Document::str(name).append(" {}");
        }

        let (body, close_sep) = Self::join_pattern_entries(entries, rest_info, " ");

        Document::str(name)
            .append(" {")
            .append(strict_break(" ", " "))
            .append(body)
            .nest(INDENT_WIDTH)
            .append(close_sep)
            .append("}")
            .group()
    }

    pub(super) fn assignment(
        &mut self,
        target: &'a Expression,
        value: &'a Expression,
        compound_operator: Option<BinaryOperator>,
    ) -> Document<'a> {
        if let Some(op) = compound_operator
            && let Some(op_str) = match op {
                BinaryOperator::Addition => Some("+="),
                BinaryOperator::Subtraction => Some("-="),
                BinaryOperator::Multiplication => Some("*="),
                BinaryOperator::Division => Some("/="),
                BinaryOperator::Remainder => Some("%="),
                _ => None,
            }
            && let Expression::Binary { right, .. } = value
        {
            return self
                .expression(target)
                .append(" ")
                .append(op_str)
                .append(" ")
                .append(self.expression(right));
        }

        self.expression(target)
            .append(" = ")
            .append(self.expression(value))
    }

    pub(super) fn lambda(
        &mut self,
        params: &'a [Binding],
        return_annotation: &'a Annotation,
        body: &'a Expression,
        _span: &'a Span,
    ) -> Document<'a> {
        let params_docs: Vec<_> = params.iter().map(|p| self.binding(p)).collect();

        let params_doc = if params_docs.is_empty() {
            Document::str("||")
        } else {
            Document::str("|")
                .append(strict_break("", ""))
                .append(join(params_docs, strict_break(",", ", ")))
                .nest(INDENT_WIDTH)
                .append(strict_break(",", ""))
                .append("|")
                .group()
        };

        let return_doc = if return_annotation.is_unknown() {
            Document::Sequence(vec![])
        } else {
            Document::str(" -> ").append(Self::annotation(return_annotation))
        };

        let body_doc = self.expression(body);

        params_doc.append(return_doc).append(" ").append(body_doc)
    }

    pub(super) fn task(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("task ").append(self.expression(expression))
    }

    pub(super) fn defer_(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("defer ").append(self.expression(expression))
    }

    pub(super) fn try_block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
        Document::str("try ").append(self.block(items, span))
    }

    pub(super) fn recover_block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
        Document::str("recover ").append(self.block(items, span))
    }

    pub(super) fn range(
        &mut self,
        start: &'a Option<Box<Expression>>,
        end: &'a Option<Box<Expression>>,
        inclusive: bool,
    ) -> Document<'a> {
        let start_doc = match start {
            Some(e) => self.expression(e),
            None => Document::Sequence(vec![]),
        };
        let end_doc = match end {
            Some(e) => self.expression(e),
            None => Document::Sequence(vec![]),
        };
        let op = if inclusive { "..=" } else { ".." };
        start_doc.append(op).append(end_doc)
    }

    pub(super) fn cast(
        &mut self,
        expression: &'a Expression,
        target_type: &'a Annotation,
    ) -> Document<'a> {
        self.expression(expression)
            .append(" as ")
            .append(Self::annotation(target_type))
    }

    pub(super) fn select(&mut self, arms: &'a [SelectArm], span: &Span) -> Document<'a> {
        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(arms.len());
        for (i, arm) in arms.iter().enumerate() {
            let start = Self::select_arm_start(arm);
            let upper_bound = arms
                .get(i + 1)
                .map(Self::select_arm_start)
                .unwrap_or_else(|| span.end());
            self.push_sibling_entry(&mut entries, start, |s| s.select_arm_body(arm, upper_bound));
        }
        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(Document::str("select"), body)
    }

    pub(super) fn select_arm_start(arm: &'a SelectArm) -> u32 {
        match &arm.pattern {
            SelectArmPattern::Receive { binding, .. } => binding.get_span().byte_offset,
            SelectArmPattern::Send {
                send_expression, ..
            } => send_expression.get_span().byte_offset,
            SelectArmPattern::MatchReceive {
                receive_expression, ..
            } => receive_expression.get_span().byte_offset,
            SelectArmPattern::WildCard { body } => body.get_span().byte_offset,
        }
    }

    pub(super) fn select_arm_body(&mut self, arm: &'a SelectArm, upper_bound: u32) -> Document<'a> {
        match &arm.pattern {
            SelectArmPattern::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => Document::str("let ")
                .append(self.pattern(binding))
                .append(" = ")
                .append(self.expression(receive_expression))
                .append(" => ")
                .append(self.expression(body))
                .append(","),
            SelectArmPattern::Send {
                send_expression,
                body,
            } => self
                .expression(send_expression)
                .append(" => ")
                .append(self.expression(body))
                .append(","),
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => {
                let header = Document::str("match ").append(self.expression(receive_expression));
                let last_arm_end = arms
                    .last()
                    .map(|a| a.expression.get_span().end())
                    .unwrap_or(0);
                // MatchReceive lacks a body span; find the inner `}` in source.
                let body_end = self
                    .comments
                    .next_byte_at(b'}', last_arm_end, upper_bound)
                    .unwrap_or(last_arm_end);
                let entries = self.match_arm_entries(arms);
                let body = self.join_sibling_body(entries, body_end);
                Self::braced_body(header, body).append(",")
            }
            SelectArmPattern::WildCard { body } => Document::str("_")
                .append(" => ")
                .append(self.expression(body))
                .append(","),
        }
    }

    pub(super) fn propagate_(&mut self, expression: &'a Expression) -> Document<'a> {
        self.expression(expression).append("?")
    }

    pub(super) fn ref_(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("&").append(self.expression(expression))
    }

    pub(super) fn raw_go(text: &'a str) -> Document<'a> {
        Document::str("@rawgo(\"")
            .append(Document::str(text))
            .append("\")")
    }
}

struct MethodChainSegment<'a> {
    member: &'a str,
    member_start: u32,
    args: &'a [Expression],
    spread: &'a Option<Expression>,
    type_args: &'a [Annotation],
}

fn collect_method_chain(expression: &Expression) -> (&Expression, Vec<MethodChainSegment<'_>>) {
    let mut segments = Vec::new();
    let mut current = expression;

    while let Expression::Call {
        expression,
        args,
        spread,
        type_args,
        ..
    } = current
    {
        let Expression::DotAccess {
            expression: inner,
            member,
            span,
            ..
        } = expression.as_ref()
        else {
            break;
        };
        let member_start = span.byte_offset + span.byte_length - member.len() as u32;
        segments.push(MethodChainSegment {
            member,
            member_start,
            args,
            spread,
            type_args,
        });
        current = inner;
    }

    segments.reverse();
    (current, segments)
}

fn is_inlinable_arg(expression: &Expression, arity: usize) -> bool {
    matches!(
        expression,
        Expression::Lambda { .. }
            | Expression::Block { .. }
            | Expression::Match { .. }
            | Expression::Tuple { .. }
            | Expression::Literal {
                literal: Literal::Slice(_),
                ..
            }
    ) || matches!(expression, Expression::Call { .. } if arity == 1)
}
