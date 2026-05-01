use crate::INDENT_WIDTH;
use crate::comments::{Comments, prepend_comments};
use crate::lindig::{Document, concat, flex_break, join, strict_break};
use syntax::ast::{
    Annotation, Attribute, AttributeArg, BinaryOperator, Binding, EnumVariant, Expression,
    FormatStringPart, Generic, ImportAlias, Literal, MatchArm, ParentInterface, Pattern,
    RestPattern, SelectArm, SelectArmPattern, Span, StructFieldAssignment, StructFieldDefinition,
    StructFieldPattern, StructKind, StructSpread, UnaryOperator, VariantFields, Visibility,
};

pub struct Formatter<'a> {
    comments: Comments<'a>,
}

type StructFieldEntry<'a> = (Option<Document<'a>>, Document<'a>, Option<Document<'a>>);

impl<'a> Formatter<'a> {
    pub fn new(comments: Comments<'a>) -> Self {
        Self { comments }
    }

    pub fn module(&mut self, top_level_items: &'a [Expression]) -> Document<'a> {
        let (imports, rest): (Vec<_>, Vec<_>) = top_level_items
            .iter()
            .partition(|e| matches!(e, Expression::ModuleImport { .. }));

        let mut docs = Vec::new();

        if !imports.is_empty() {
            docs.push(self.sort_imports(&imports));
        }

        let mut prev_end: Option<u32> = None;
        for (i, item) in rest.iter().enumerate() {
            let start = Self::item_leading_edge(item);

            let (same_line_trailing, leading) = match prev_end {
                Some(anchor) => self.comments.take_split_by_newline_after(anchor, start),
                None => (None, self.comments.take_comments_before(start)),
            };

            if let Some(t) = same_line_trailing {
                docs.push(Document::str(" "));
                docs.push(t);
            }

            if let Some(comment_doc) = leading {
                if !docs.is_empty() {
                    docs.push(Document::Newline);
                    docs.push(Document::Newline);
                }
                docs.push(comment_doc.force_break());
                docs.push(Document::Newline);
            } else if !docs.is_empty() || i > 0 {
                docs.push(Document::Newline);
                docs.push(Document::Newline);
            }

            docs.push(self.definition(item));
            let span = item.get_span();
            prev_end = Some(span.byte_offset + span.byte_length);
        }

        if let Some(comment_doc) = self.comments.take_trailing_comments() {
            if !docs.is_empty() {
                docs.push(Document::Newline);
                docs.push(Document::Newline);
            }
            docs.push(comment_doc);
        }

        if !docs.is_empty() {
            docs.push(Document::Newline);
        }

        concat(docs)
    }

    fn sort_imports(&mut self, imports: &[&'a Expression]) -> Document<'a> {
        if imports.is_empty() {
            return Document::Sequence(vec![]);
        }

        let mut leading_comments: Option<Document<'a>> = None;
        let mut leading_has_blank_line = false;
        let mut go_imports: Vec<&'a Expression> = Vec::new();
        let mut local_imports: Vec<&'a Expression> = Vec::new();

        for (i, import) in imports.iter().enumerate() {
            let start = import.get_span().byte_offset;
            let has_blank_line = self.comments.take_empty_lines_before(start);

            let comments = self.comments.take_comments_before(start);
            if i == 0 && comments.is_some() {
                leading_comments = comments;
                leading_has_blank_line = has_blank_line;
            }

            if let Expression::ModuleImport { name, .. } = import {
                if name.starts_with("go:") {
                    go_imports.push(import);
                } else {
                    local_imports.push(import);
                }
            }
        }

        fn import_sort_key(imp: &&Expression) -> (String, String) {
            if let Expression::ModuleImport { name, alias, .. } = imp {
                let sort_path = match alias {
                    Some(ImportAlias::Named(a, _)) => a.to_string(),
                    Some(ImportAlias::Blank(_)) => "_".to_string(),
                    None => {
                        let path = name.split_once(':').map(|(_, p)| p).unwrap_or(name);
                        path.to_string()
                    }
                };
                (sort_path, name.to_string())
            } else {
                (String::new(), String::new())
            }
        }

        go_imports.sort_by_key(import_sort_key);
        local_imports.sort_by_key(import_sort_key);

        let mut group_docs: Vec<Document<'a>> = Vec::new();

        if !go_imports.is_empty() {
            let docs: Vec<_> = go_imports.iter().map(|imp| self.definition(imp)).collect();
            group_docs.push(join(docs, Document::Newline));
        }

        if !local_imports.is_empty() {
            let docs: Vec<_> = local_imports
                .iter()
                .map(|imp| self.definition(imp))
                .collect();
            group_docs.push(join(docs, Document::Newline));
        }

        let imports_doc = join(group_docs, concat([Document::Newline, Document::Newline]));

        match leading_comments {
            Some(c) => {
                let separator = if leading_has_blank_line {
                    concat([Document::Newline, Document::Newline])
                } else {
                    Document::Newline
                };
                c.force_break().append(separator).append(imports_doc)
            }
            None => imports_doc,
        }
    }

    fn definition(&mut self, expression: &'a Expression) -> Document<'a> {
        let start = expression.get_span().byte_offset;
        let doc_comments_doc = self.comments.take_doc_comments_before(start);

        let attrs = match expression {
            Expression::Function { attributes, .. } | Expression::Struct { attributes, .. } => {
                self.attributes(attributes)
            }
            _ => Document::Sequence(vec![]),
        };
        let between_attrs_and_keyword = self.comments.take_comments_before(start);

        let (vis, inner) = match expression {
            Expression::Function {
                name,
                generics,
                params,
                return_annotation,
                body,
                visibility,
                ..
            } => (
                *visibility,
                self.function(name, generics, params, return_annotation, body),
            ),

            Expression::Struct {
                name,
                generics,
                fields,
                kind,
                visibility,
                span,
                ..
            } => (
                *visibility,
                self.struct_definition(name, generics, fields, span, *kind),
            ),

            Expression::Enum {
                name,
                generics,
                variants,
                visibility,
                span,
                ..
            } => (
                *visibility,
                self.enum_definition(name, generics, variants, span),
            ),

            Expression::ValueEnum {
                name,
                underlying_ty,
                variants,
                visibility,
                span,
                ..
            } => (
                *visibility,
                self.value_enum_definition(name, underlying_ty.as_ref(), variants, span),
            ),

            Expression::TypeAlias {
                name,
                generics,
                annotation,
                visibility,
                ..
            } => (*visibility, Self::type_alias(name, generics, annotation)),

            Expression::Interface {
                name,
                generics,
                parents,
                method_signatures,
                visibility,
                span,
                ..
            } => (
                *visibility,
                self.interface(name, generics, parents, method_signatures, span),
            ),

            Expression::ImplBlock {
                annotation,
                generics,
                methods,
                span,
                ..
            } => (
                Visibility::Private,
                self.impl_block(annotation, generics, methods, span.end()),
            ),

            Expression::Const {
                identifier,
                annotation,
                expression,
                visibility,
                ..
            } => (
                *visibility,
                self.const_definition(identifier, annotation.as_ref(), expression),
            ),

            Expression::VariableDeclaration {
                name,
                annotation,
                visibility,
                ..
            } => (
                *visibility,
                Document::str("var ")
                    .append(Document::string(name.to_string()))
                    .append(": ")
                    .append(Self::annotation(annotation)),
            ),

            Expression::ModuleImport { name, alias, .. } => {
                let alias_doc = match alias {
                    Some(ImportAlias::Named(a, _)) => Document::string(a.to_string()).append(" "),
                    Some(ImportAlias::Blank(_)) => Document::str("_ "),
                    None => Document::str(""),
                };

                (
                    Visibility::Private,
                    Document::str("import ")
                        .append(alias_doc)
                        .append("\"")
                        .append(Document::string(name.to_string()))
                        .append("\""),
                )
            }

            _ => (Visibility::Private, self.expression(expression)),
        };

        let vis_inner = match Self::visibility(vis) {
            Some(pub_doc) => pub_doc.append(inner),
            None => inner,
        };
        let definition_doc = match between_attrs_and_keyword {
            Some(c) => attrs
                .append(c.force_break())
                .append(Document::Newline)
                .append(vis_inner),
            None => attrs.append(vis_inner),
        };

        match doc_comments_doc {
            Some(doc) => doc.append(Document::Newline).append(definition_doc),
            None => definition_doc,
        }
    }

    fn visibility(vis: Visibility) -> Option<Document<'a>> {
        match vis {
            Visibility::Public => Some(Document::str("pub ")),
            Visibility::Private => None,
        }
    }

    fn function(
        &mut self,
        name: &'a str,
        generics: &'a [Generic],
        params: &'a [Binding],
        return_annotation: &'a Annotation,
        body: &'a Expression,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);

        let params_docs: Vec<_> = params.iter().map(|p| self.binding(p)).collect();

        let params_doc = Self::wrap_params(params_docs);

        let return_doc = if return_annotation.is_unknown() {
            Document::Sequence(vec![])
        } else {
            Document::str(" -> ").append(Self::annotation(return_annotation))
        };

        let signature = Document::str("fn ")
            .append(Document::string(name.to_string()))
            .append(generics_doc)
            .append(params_doc)
            .append(return_doc)
            .group();

        if matches!(body, Expression::NoOp) {
            signature
        } else {
            signature.append(" ").append(self.as_block(body))
        }
    }

    fn wrap_params(params_docs: Vec<Document<'a>>) -> Document<'a> {
        if params_docs.is_empty() {
            return Document::str("()");
        }

        let params_doc = join(params_docs, strict_break(",", ", "));

        Document::str("(")
            .append(strict_break("", ""))
            .append(params_doc)
            .nest(INDENT_WIDTH)
            .append(strict_break(",", ""))
            .append(")")
            .group()
    }

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

    fn literal(&mut self, literal: &'a Literal) -> Document<'a> {
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

    fn slice(&mut self, elements: &'a [Expression]) -> Document<'a> {
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

    fn format_string(&mut self, parts: &'a [FormatStringPart]) -> Document<'a> {
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

    fn block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
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

        let (same_line, standalone) = self.comments.take_split_at_line_start(block_end);
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

    fn let_(
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

    fn return_(&mut self, expression: &'a Expression) -> Document<'a> {
        if matches!(expression, Expression::Unit { .. }) {
            Document::str("return")
        } else {
            Document::str("return ").append(self.expression(expression))
        }
    }

    fn if_(
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

    fn if_let(
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

    fn as_block(&mut self, expression: &'a Expression) -> Document<'a> {
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
    fn as_inline_block(&mut self, expression: &'a Expression) -> Document<'a> {
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

    fn match_(
        &mut self,
        subject: &'a Expression,
        arms: &'a [MatchArm],
        span: &Span,
    ) -> Document<'a> {
        let arms_docs: Vec<_> = arms
            .iter()
            .map(|arm| {
                let start = arm.pattern.get_span().byte_offset;
                let comments = self.comments.take_comments_before(start);
                let pattern = self.pattern(&arm.pattern);
                let expression = self.expression(&arm.expression);

                let pattern_with_guard = if let Some(guard) = &arm.guard {
                    pattern.append(" if ").append(self.expression(guard))
                } else {
                    pattern
                };

                let arm_doc = pattern_with_guard
                    .append(" => ")
                    .append(expression)
                    .append(",");
                prepend_comments(arm_doc, comments)
            })
            .collect();

        let header = Document::str("match ").append(self.expression(subject));
        let body = self.with_trailing_comments(join(arms_docs, Document::Newline), span.end());
        Self::braced_body(header, body)
    }

    fn loop_(&mut self, body: &'a Expression) -> Document<'a> {
        Document::str("loop ").append(self.as_block(body))
    }

    fn while_(&mut self, condition: &'a Expression, body: &'a Expression) -> Document<'a> {
        Document::str("while ")
            .append(self.expression(condition))
            .append(" ")
            .append(self.as_block(body))
    }

    fn while_let(
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

    fn for_(
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

    fn binary_operator(
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

    fn pipeline(&mut self, left: &'a Expression, right: &'a Expression) -> Document<'a> {
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

    fn unary_operator(
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

    fn call(
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

    fn format_type_args(type_args: &'a [Annotation]) -> Document<'a> {
        if type_args.is_empty() {
            Document::Sequence(vec![])
        } else {
            let types: Vec<_> = type_args.iter().map(Self::annotation).collect();
            Document::str("<")
                .append(join(types, Document::str(", ")))
                .append(">")
        }
    }

    fn format_call_with_head(
        &mut self,
        head: Document<'a>,
        args: &'a [Expression],
        spread: &'a Option<Expression>,
    ) -> Document<'a> {
        if args.is_empty() && spread.is_none() {
            return head.append("()");
        }

        if let Some(spread_expr) = spread {
            let spread_doc = Document::str("..").append(self.expression(spread_expr));
            if args.is_empty() {
                return head
                    .append("(")
                    .append(spread_doc.group().next_break_fits(true))
                    .append(")")
                    .next_break_fits(false)
                    .group();
            }
            let init_docs: Vec<_> = args.iter().map(|a| self.expression(a)).collect();
            let init_doc = join(init_docs, strict_break(",", ", "));
            return head
                .append("(")
                .append(strict_break("", ""))
                .append(init_doc)
                .append(strict_break(",", ", "))
                .append(spread_doc.group().next_break_fits(true))
                .nest(INDENT_WIDTH)
                .append(strict_break(",", ""))
                .append(")")
                .next_break_fits(false)
                .group();
        }

        let Some((last, init)) = args
            .split_last()
            .filter(|(last, _)| is_inlinable_arg(last, args.len()))
        else {
            let arg_docs: Vec<_> = args.iter().map(|a| self.expression(a)).collect();
            let args_doc = join(arg_docs, strict_break(",", ", "));

            return head
                .append("(")
                .append(strict_break("", ""))
                .append(args_doc)
                .nest(INDENT_WIDTH)
                .append(strict_break(",", ""))
                .append(")")
                .group();
        };

        let last_doc = self.expression(last).group().next_break_fits(true);

        if init.is_empty() {
            head.append("(")
                .append(last_doc)
                .append(")")
                .next_break_fits(false)
                .group()
        } else {
            let init_docs: Vec<_> = init.iter().map(|a| self.expression(a)).collect();
            let init_doc = join(init_docs, strict_break(",", ", "));

            head.append("(")
                .append(strict_break("", ""))
                .append(init_doc)
                .append(strict_break(",", ", "))
                .append(last_doc)
                .nest(INDENT_WIDTH)
                .append(strict_break(",", ""))
                .append(")")
                .next_break_fits(false)
                .group()
        }
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

    fn dot_access(&mut self, expression: &'a Expression, member: &'a str) -> Document<'a> {
        self.expression(expression).append(".").append(member)
    }

    fn indexed_access(
        &mut self,
        expression: &'a Expression,
        index: &'a Expression,
    ) -> Document<'a> {
        self.expression(expression)
            .append("[")
            .append(self.expression(index))
            .append("]")
    }

    fn tuple(&mut self, elements: &'a [Expression]) -> Document<'a> {
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

    fn struct_call(
        &mut self,
        name: &'a str,
        fields: &'a [StructFieldAssignment],
        spread: &'a StructSpread,
    ) -> Document<'a> {
        let mut field_docs: Vec<_> = fields
            .iter()
            .map(|f| {
                if let Expression::Identifier { value, .. } = &*f.value
                    && value == &f.name
                {
                    return Document::string(f.name.to_string());
                }
                Document::string(f.name.to_string())
                    .append(": ")
                    .append(self.expression(&f.value))
            })
            .collect();

        match spread {
            StructSpread::None => {}
            StructSpread::From(spread_expression) => {
                let start = spread_expression.get_span().byte_offset;
                let comments = self.comments.take_comments_before(start);
                let spread_doc = Document::str("..").append(self.expression(spread_expression));
                field_docs.push(prepend_comments(spread_doc, comments));
            }
            StructSpread::ZeroFill { span } => {
                let comments = self.comments.take_comments_before(span.byte_offset);
                field_docs.push(prepend_comments(Document::str(".."), comments));
            }
        }

        if field_docs.is_empty() {
            return Document::str(name).append(" {}");
        }

        let fields_doc = join(field_docs, strict_break(",", ", "));

        Document::str(name)
            .append(" {")
            .append(strict_break(" ", " "))
            .append(fields_doc)
            .nest(INDENT_WIDTH)
            .append(strict_break(",", " "))
            .append("}")
            .group()
    }

    fn assignment(
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

    fn lambda(
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

    fn task(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("task ").append(self.expression(expression))
    }

    fn defer_(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("defer ").append(self.expression(expression))
    }

    fn try_block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
        Document::str("try ").append(self.block(items, span))
    }

    fn recover_block(&mut self, items: &'a [Expression], span: &Span) -> Document<'a> {
        Document::str("recover ").append(self.block(items, span))
    }

    fn range(
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

    fn cast(&mut self, expression: &'a Expression, target_type: &'a Annotation) -> Document<'a> {
        self.expression(expression)
            .append(" as ")
            .append(Self::annotation(target_type))
    }

    fn select(&mut self, arms: &'a [SelectArm], span: &Span) -> Document<'a> {
        let arms_docs: Vec<_> = arms.iter().map(|arm| self.select_arm(arm)).collect();
        let body = self.with_trailing_comments(join(arms_docs, Document::Newline), span.end());
        Self::braced_body(Document::str("select"), body)
    }

    fn select_arm(&mut self, arm: &'a SelectArm) -> Document<'a> {
        let start = match &arm.pattern {
            SelectArmPattern::Receive { binding, .. } => binding.get_span().byte_offset,
            SelectArmPattern::Send {
                send_expression, ..
            } => send_expression.get_span().byte_offset,
            SelectArmPattern::MatchReceive {
                receive_expression, ..
            } => receive_expression.get_span().byte_offset,
            SelectArmPattern::WildCard { body } => body.get_span().byte_offset,
        };
        self.with_leading_comments(start, |s| match &arm.pattern {
            SelectArmPattern::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => Document::str("let ")
                .append(s.pattern(binding))
                .append(" = ")
                .append(s.expression(receive_expression))
                .append(" => ")
                .append(s.expression(body))
                .append(","),
            SelectArmPattern::Send {
                send_expression,
                body,
            } => s
                .expression(send_expression)
                .append(" => ")
                .append(s.expression(body))
                .append(","),
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => {
                let arms_docs: Vec<_> = arms
                    .iter()
                    .map(|a| {
                        let pattern = s.pattern(&a.pattern);
                        let expression = s.expression(&a.expression);
                        let pattern_with_guard = if let Some(guard) = &a.guard {
                            pattern.append(" if ").append(s.expression(guard))
                        } else {
                            pattern
                        };
                        pattern_with_guard
                            .append(" => ")
                            .append(expression)
                            .append(",")
                    })
                    .collect();
                let header = Document::str("match ").append(s.expression(receive_expression));
                Self::braced_body(header, join(arms_docs, Document::Newline)).append(",")
            }
            SelectArmPattern::WildCard { body } => Document::str("_")
                .append(" => ")
                .append(s.expression(body))
                .append(","),
        })
    }

    fn propagate_(&mut self, expression: &'a Expression) -> Document<'a> {
        self.expression(expression).append("?")
    }

    fn ref_(&mut self, expression: &'a Expression) -> Document<'a> {
        Document::str("&").append(self.expression(expression))
    }

    fn raw_go(text: &'a str) -> Document<'a> {
        Document::str("@rawgo(\"")
            .append(Document::str(text))
            .append("\")")
    }

    fn struct_definition(
        &mut self,
        name: &'a str,
        generics: &'a [Generic],
        fields: &'a [StructFieldDefinition],
        span: &Span,
        kind: StructKind,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);
        let header = Document::str("struct ").append(name).append(generics_doc);
        let struct_end = span.byte_offset + span.byte_length;

        if kind == StructKind::Tuple {
            let type_docs: Vec<_> = fields
                .iter()
                .map(|f| Self::annotation(&f.annotation))
                .collect();
            return header
                .append("(")
                .append(join(type_docs, Document::str(", ")))
                .append(")");
        }

        let with_field_attrs = fields.iter().any(|f| !f.attributes.is_empty());
        let with_pub_fields = fields.iter().any(|f| f.visibility.is_public());

        if fields.is_empty() {
            return self.empty_struct_body(header, struct_end);
        }

        let (field_entries, trailing, with_comments) =
            self.struct_fields_with_comments(fields, struct_end);

        if with_comments || with_field_attrs || with_pub_fields {
            let mut fields_docs: Vec<Document<'a>> = field_entries
                .into_iter()
                .map(|(leading, field, same_line_trailing)| {
                    let mut doc = match leading {
                        Some(c) => c.append(Document::Newline).append(field),
                        None => field,
                    };
                    doc = doc.append(",");
                    if let Some(t) = same_line_trailing {
                        doc = doc.append(" ").append(t);
                    }
                    doc
                })
                .collect();
            if let Some(t) = trailing {
                fields_docs.push(t);
            }
            return Self::braced_body(header, join(fields_docs, Document::Newline));
        }

        let fields_docs: Vec<_> = field_entries
            .into_iter()
            .map(|(_, field, _)| field)
            .collect();
        Self::flexible_struct_body(header, fields_docs)
    }

    fn empty_struct_body(&mut self, header: Document<'a>, end: u32) -> Document<'a> {
        match self.comments.take_comments_before(end) {
            Some(c) => header
                .append(" {")
                .append(Document::Newline.append(c).nest(INDENT_WIDTH))
                .append(Document::Newline)
                .append("}")
                .force_break(),
            None => header.append(" {}"),
        }
    }

    fn struct_fields_with_comments(
        &mut self,
        fields: &'a [StructFieldDefinition],
        struct_end: u32,
    ) -> (Vec<StructFieldEntry<'a>>, Option<Document<'a>>, bool) {
        let mut entries = Vec::new();
        let mut with_comments = false;
        let mut prev_anchor: Option<u32> = None;

        for field in fields {
            let leading_edge = field
                .attributes
                .first()
                .map(|a| a.span.byte_offset)
                .unwrap_or(field.name_span.byte_offset);
            let (trailing_for_prev, leading) = match prev_anchor {
                Some(anchor) => self
                    .comments
                    .take_split_by_newline_after(anchor, leading_edge),
                None => (None, self.comments.take_comments_before(leading_edge)),
            };

            with_comments = with_comments || trailing_for_prev.is_some() || leading.is_some();

            if let Some(t) = trailing_for_prev
                && let Some(last) = entries.last_mut()
            {
                let (_, _, slot): &mut (_, _, Option<Document<'a>>) = last;
                *slot = Some(t);
            }

            let field_attrs = self.field_attributes(&field.attributes);

            let field_definition = if field.visibility.is_public() {
                Document::str("pub ")
                    .append(Document::string(field.name.to_string()))
                    .append(": ")
                    .append(Self::annotation(&field.annotation))
            } else {
                Document::string(field.name.to_string())
                    .append(": ")
                    .append(Self::annotation(&field.annotation))
            };

            entries.push((leading, field_attrs.append(field_definition), None));

            let ann_span = field.annotation.get_span();
            prev_anchor = Some(ann_span.byte_offset + ann_span.byte_length);
        }

        let (last_trailing, struct_trailing) = match prev_anchor {
            Some(anchor) => self
                .comments
                .take_split_by_newline_after(anchor, struct_end),
            None => (None, self.comments.take_comments_before(struct_end)),
        };
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.2 = Some(t);
        }
        with_comments = with_comments || struct_trailing.is_some();

        (entries, struct_trailing, with_comments)
    }

    fn field_attributes(&mut self, attrs: &'a [Attribute]) -> Document<'a> {
        if attrs.is_empty() {
            return Document::Sequence(vec![]);
        }

        let attribute_docs: Vec<_> = attrs.iter().map(|a| self.attribute(a)).collect();
        join(attribute_docs, Document::Newline).append(Document::Newline)
    }

    fn braced_body(header: Document<'a>, body: Document<'a>) -> Document<'a> {
        header
            .append(" {")
            .append(Document::Newline.append(body).nest(INDENT_WIDTH))
            .append(Document::Newline)
            .append("}")
            .force_break()
    }

    /// Drains comments before `end` and appends them as standalone trailing
    /// content inside a container body, separated from the preceding items by
    /// a blank line.
    fn with_trailing_comments(&mut self, body: Document<'a>, end: u32) -> Document<'a> {
        match self.comments.take_comments_before(end) {
            Some(trailing) => body
                .append(Document::Newline)
                .append(Document::Newline)
                .append(trailing.force_break()),
            None => body,
        }
    }

    /// Drains comments before `start` and prepends them to the document built
    /// by `build`. Use at the entry of any first-class child node emitter
    /// (pattern, binding, enum_variant, etc.) so that leading comments stay
    /// with the node the user wrote them on.
    fn with_leading_comments(
        &mut self,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) -> Document<'a> {
        let comments = self.comments.take_comments_before(start);
        let doc = build(self);
        prepend_comments(doc, comments)
    }

    fn item_leading_edge(item: &'a Expression) -> u32 {
        let attrs: &[Attribute] = match item {
            Expression::Function { attributes, .. } | Expression::Struct { attributes, .. } => {
                attributes
            }
            _ => &[],
        };
        attrs
            .first()
            .map(|a| a.span.byte_offset)
            .unwrap_or_else(|| item.get_span().byte_offset)
    }

    fn flexible_struct_body(header: Document<'a>, items: Vec<Document<'a>>) -> Document<'a> {
        let items_doc = join(items, strict_break(",", ", "));
        header
            .append(" {")
            .append(strict_break("", " "))
            .append(items_doc)
            .nest(INDENT_WIDTH)
            .append(strict_break(",", " "))
            .append("}")
            .group()
    }

    fn enum_definition(
        &mut self,
        name: &'a str,
        generics: &'a [Generic],
        variants: &'a [EnumVariant],
        span: &Span,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);
        let header = Document::str("enum ").append(name).append(generics_doc);

        if variants.is_empty() {
            return header.append(" {}");
        }

        let variants_docs: Vec<_> = variants.iter().map(|v| self.enum_variant(v)).collect();
        let body = self.with_trailing_comments(join(variants_docs, Document::Newline), span.end());
        Self::braced_body(header, body)
    }

    fn value_enum_definition(
        &mut self,
        name: &'a str,
        underlying_ty: Option<&'a syntax::ast::Annotation>,
        variants: &'a [syntax::ast::ValueEnumVariant],
        span: &Span,
    ) -> Document<'a> {
        let header = if let Some(ty) = underlying_ty {
            Document::str("enum ")
                .append(name)
                .append(": ")
                .append(Self::annotation(ty))
        } else {
            Document::str("enum ").append(name)
        };

        if variants.is_empty() {
            return header.append(" {}");
        }

        let variants_docs: Vec<_> = variants
            .iter()
            .map(|v| self.value_enum_variant(v))
            .collect();

        let body = self.with_trailing_comments(join(variants_docs, Document::Newline), span.end());
        Self::braced_body(header, body)
    }

    fn enum_variant(&mut self, variant: &'a EnumVariant) -> Document<'a> {
        self.with_leading_comments(variant.name_span.byte_offset, |_| {
            let name = Document::string(variant.name.to_string());
            match &variant.fields {
                VariantFields::Unit => name.append(","),
                VariantFields::Tuple(fields) => {
                    let field_docs: Vec<_> = fields
                        .iter()
                        .map(|f| Self::annotation(&f.annotation))
                        .collect();
                    name.append("(")
                        .append(join(field_docs, Document::str(", ")))
                        .append("),")
                }
                VariantFields::Struct(fields) => {
                    let field_docs: Vec<_> = fields
                        .iter()
                        .map(|f| {
                            Document::string(f.name.to_string())
                                .append(": ")
                                .append(Self::annotation(&f.annotation))
                        })
                        .collect();
                    name.append(" { ")
                        .append(join(field_docs, Document::str(", ")))
                        .append(" },")
                }
            }
        })
    }

    fn value_enum_variant(&mut self, variant: &'a syntax::ast::ValueEnumVariant) -> Document<'a> {
        self.with_leading_comments(variant.name_span.byte_offset, |s| {
            let name = Document::string(variant.name.to_string());
            let value_doc = s.literal(&variant.value);
            name.append(" = ").append(value_doc).append(",")
        })
    }

    fn type_alias(
        name: &'a str,
        generics: &'a [Generic],
        annotation: &'a Annotation,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);

        let base = Document::str("type ").append(name).append(generics_doc);

        if annotation.is_opaque() {
            base
        } else {
            base.append(" = ").append(Self::annotation(annotation))
        }
    }

    fn interface(
        &mut self,
        name: &'a str,
        generics: &'a [Generic],
        parents: &'a [ParentInterface],
        methods: &'a [Expression],
        span: &Span,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);

        let mut body_docs = Vec::new();

        for parent in parents {
            body_docs.push(Document::str("impl ").append(Self::annotation(&parent.annotation)));
        }

        for method in methods {
            body_docs.push(self.interface_method(method));
        }

        let header = Document::str("interface ")
            .append(name)
            .append(generics_doc);

        if body_docs.is_empty() {
            return header.append(" {}");
        }

        let body = self.with_trailing_comments(join(body_docs, Document::Newline), span.end());
        Self::braced_body(header, body)
    }

    fn interface_method(&mut self, method: &'a Expression) -> Document<'a> {
        self.with_leading_comments(method.get_span().byte_offset, |s| match method {
            Expression::Function {
                name,
                generics,
                params,
                return_annotation,
                attributes,
                ..
            } => {
                let attrs_doc = s.attributes(attributes);
                let generics_doc = Self::generics(generics);

                let params_docs: Vec<_> = params.iter().map(|p| s.binding(p)).collect();
                let params_doc = Self::wrap_params(params_docs);

                let return_doc = if return_annotation.is_unknown() {
                    Document::Sequence(vec![])
                } else {
                    Document::str(" -> ").append(Self::annotation(return_annotation))
                };

                attrs_doc
                    .append(Document::str("fn "))
                    .append(Document::string(name.to_string()))
                    .append(generics_doc)
                    .append(params_doc)
                    .append(return_doc)
            }
            _ => Document::Sequence(vec![]),
        })
    }

    fn impl_block(
        &mut self,
        annotation: &'a Annotation,
        generics: &'a [Generic],
        methods: &'a [Expression],
        impl_end: u32,
    ) -> Document<'a> {
        let generics_doc = Self::generics(generics);
        let header = Document::str("impl")
            .append(generics_doc)
            .append(" ")
            .append(Self::annotation(annotation));

        if methods.is_empty() {
            return header.append(" {}");
        }

        let mut docs = Vec::with_capacity(methods.len() * 5);

        for (i, m) in methods.iter().enumerate() {
            let start = m.get_span().byte_offset;

            if i > 0 {
                docs.push(Document::Newline);
                docs.push(Document::Newline);
            }

            if let Some(comment_doc) = self.comments.take_comments_before(start) {
                docs.push(comment_doc.force_break());
                docs.push(Document::Newline);
            }

            docs.push(self.definition(m));
        }

        if let Some(trailing) = self.comments.take_comments_before(impl_end) {
            docs.push(Document::Newline);
            docs.push(Document::Newline);
            docs.push(trailing.force_break());
        }

        Self::braced_body(header, concat(docs))
    }

    fn const_definition(
        &mut self,
        name: &'a str,
        annotation: Option<&'a Annotation>,
        value: &'a Expression,
    ) -> Document<'a> {
        let type_doc = match annotation {
            Some(ann) => Document::str(": ").append(Self::annotation(ann)),
            None => Document::Sequence(vec![]),
        };

        Document::str("const ")
            .append(name)
            .append(type_doc)
            .append(" = ")
            .append(self.expression(value))
    }

    fn pattern(&mut self, pat: &'a Pattern) -> Document<'a> {
        let start = pat.get_span().byte_offset;
        let comments = self.comments.take_comments_before(start);
        let doc = match pat {
            Pattern::Literal { literal, .. } => self.literal(literal),
            Pattern::Unit { .. } => Document::str("()"),
            Pattern::WildCard { .. } => Document::str("_"),
            Pattern::Identifier { identifier, .. } => Document::string(identifier.to_string()),

            Pattern::EnumVariant {
                identifier,
                fields,
                rest,
                ..
            } => {
                if fields.is_empty() && !rest {
                    Document::string(identifier.to_string())
                } else {
                    let mut field_docs: Vec<_> = fields
                        .iter()
                        .map(|f| {
                            let start = f.get_span().byte_offset;
                            let comments = self.comments.take_comments_before(start);
                            prepend_comments(self.pattern(f), comments)
                        })
                        .collect();
                    if *rest {
                        field_docs.push(Document::str(".."));
                    }
                    Document::string(identifier.to_string())
                        .append("(")
                        .append(strict_break("", ""))
                        .append(join(field_docs, strict_break(",", ", ")))
                        .nest(INDENT_WIDTH)
                        .append(strict_break(",", ""))
                        .append(")")
                        .group()
                }
            }

            Pattern::Struct {
                identifier,
                fields,
                rest,
                ..
            } => {
                if fields.is_empty() && !rest {
                    Document::string(identifier.to_string()).append(" {}")
                } else {
                    let mut field_docs: Vec<_> = fields
                        .iter()
                        .map(|f| {
                            let start = f.value.get_span().byte_offset;
                            let comments = self.comments.take_comments_before(start);
                            prepend_comments(self.struct_field_pattern(f), comments)
                        })
                        .collect();
                    if *rest {
                        field_docs.push(Document::str(".."));
                    }
                    Document::string(identifier.to_string())
                        .append(" {")
                        .append(strict_break(" ", " "))
                        .append(join(field_docs, strict_break(",", ", ")))
                        .nest(INDENT_WIDTH)
                        .append(strict_break(",", " "))
                        .append("}")
                        .group()
                }
            }

            Pattern::Tuple { elements, .. } => {
                let elements_docs: Vec<_> = elements
                    .iter()
                    .map(|e| {
                        let start = e.get_span().byte_offset;
                        let comments = self.comments.take_comments_before(start);
                        prepend_comments(self.pattern(e), comments)
                    })
                    .collect();
                Document::str("(")
                    .append(strict_break("", ""))
                    .append(join(elements_docs, strict_break(",", ", ")))
                    .nest(INDENT_WIDTH)
                    .append(strict_break(",", ""))
                    .append(")")
                    .group()
            }

            Pattern::Slice { prefix, rest, .. } => {
                let mut all_docs: Vec<Document<'a>> = Vec::new();

                for pattern in prefix {
                    let start = pattern.get_span().byte_offset;
                    let comments = self.comments.take_comments_before(start);
                    all_docs.push(prepend_comments(self.pattern(pattern), comments));
                }

                match rest {
                    RestPattern::Absent => {}
                    RestPattern::Discard(_) => {
                        all_docs.push(Document::str(".."));
                    }
                    RestPattern::Bind { name, .. } => {
                        all_docs
                            .push(Document::str("..").append(Document::string(name.to_string())));
                    }
                }

                Document::str("[")
                    .append(strict_break("", ""))
                    .append(join(all_docs, strict_break(",", ", ")))
                    .nest(INDENT_WIDTH)
                    .append(strict_break(",", ""))
                    .append("]")
                    .group()
            }

            Pattern::Or { patterns, .. } => {
                let pattern_docs: Vec<_> = patterns.iter().map(|p| self.pattern(p)).collect();
                join(pattern_docs, strict_break(" |", " | ")).group()
            }

            Pattern::AsBinding { pattern, name, .. } => self
                .pattern(pattern)
                .append(" as ")
                .append(Document::string(name.to_string())),
        };
        prepend_comments(doc, comments)
    }

    fn struct_field_pattern(&mut self, field: &'a StructFieldPattern) -> Document<'a> {
        if let Pattern::Identifier { identifier, .. } = &field.value
            && identifier == &field.name
        {
            return Document::string(field.name.to_string());
        }

        Document::string(field.name.to_string())
            .append(": ")
            .append(self.pattern(&field.value))
    }

    fn binding(&mut self, binding: &'a Binding) -> Document<'a> {
        self.with_leading_comments(binding.pattern.get_span().byte_offset, |s| {
            let pattern_doc = if binding.mutable {
                Document::str("mut ").append(s.pattern(&binding.pattern))
            } else {
                s.pattern(&binding.pattern)
            };
            match &binding.annotation {
                Some(annotation) => pattern_doc
                    .append(": ")
                    .append(Self::annotation(annotation)),
                None => pattern_doc,
            }
        })
    }

    fn annotation(annotation: &'a Annotation) -> Document<'a> {
        match annotation {
            Annotation::Constructor { name, params, .. } => {
                if params.is_empty() {
                    if name == "Unit" {
                        Document::str("()")
                    } else {
                        Document::string(name.to_string())
                    }
                } else {
                    let param_docs: Vec<_> = params.iter().map(Self::annotation).collect();
                    Document::string(name.to_string())
                        .append("<")
                        .append(join(param_docs, Document::str(", ")))
                        .append(">")
                }
            }
            Annotation::Function {
                params,
                return_type,
                ..
            } => {
                let param_docs: Vec<_> = params.iter().map(Self::annotation).collect();
                Document::str("fn(")
                    .append(join(param_docs, Document::str(", ")))
                    .append(") -> ")
                    .append(Self::annotation(return_type))
            }
            Annotation::Unknown => Document::str("_"),
            Annotation::Tuple { elements, .. } => {
                let elem_docs: Vec<_> = elements.iter().map(Self::annotation).collect();
                Document::str("(")
                    .append(join(elem_docs, Document::str(", ")))
                    .append(")")
            }
            Annotation::Opaque { .. } => Document::Sequence(vec![]),
        }
    }

    fn generics(generics: &'a [Generic]) -> Document<'a> {
        if generics.is_empty() {
            return Document::Sequence(vec![]);
        }

        let generics_docs: Vec<_> = generics
            .iter()
            .map(|g| {
                if g.bounds.is_empty() {
                    Document::string(g.name.to_string())
                } else {
                    let bounds: Vec<_> = g.bounds.iter().map(Self::annotation).collect();
                    Document::string(g.name.to_string())
                        .append(": ")
                        .append(join(bounds, Document::str(" + ")))
                }
            })
            .collect();

        Document::str("<")
            .append(join(generics_docs, Document::str(", ")))
            .append(">")
    }

    fn attribute(&mut self, attribute: &'a Attribute) -> Document<'a> {
        self.with_leading_comments(attribute.span.byte_offset, |_| {
            let name = Document::string(attribute.name.clone());
            if attribute.args.is_empty() {
                Document::str("#[").append(name).append("]")
            } else {
                let args_docs: Vec<_> = attribute.args.iter().map(Self::attribute_arg).collect();
                Document::str("#[")
                    .append(name)
                    .append("(")
                    .append(join(args_docs, Document::str(", ")))
                    .append(")]")
            }
        })
    }

    fn attribute_arg(arg: &'a AttributeArg) -> Document<'a> {
        match arg {
            AttributeArg::Flag(name) => Document::string(name.clone()),
            AttributeArg::NegatedFlag(name) => {
                Document::str("!").append(Document::string(name.clone()))
            }
            AttributeArg::String(s) => Document::string(format!("\"{}\"", s)),
            AttributeArg::Raw(s) => Document::string(format!("`{}`", s)),
        }
    }

    fn attributes(&mut self, attrs: &'a [Attribute]) -> Document<'a> {
        if attrs.is_empty() {
            return Document::Sequence(vec![]);
        }

        let attribute_docs: Vec<_> = attrs.iter().map(|a| self.attribute(a)).collect();
        join(attribute_docs, Document::Newline).append(Document::Newline)
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
