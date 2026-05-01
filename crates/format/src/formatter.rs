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

struct SiblingEntry<'a> {
    leading: Option<Document<'a>>,
    doc: Document<'a>,
    trailing: Option<Document<'a>>,
    has_blank_above: bool,
}

struct PatternEntry<'a> {
    leading: Option<Document<'a>>,
    doc: Document<'a>,
    trailing: Option<Document<'a>>,
}

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

            let (same_line_trailing, leading, _) = match prev_end {
                Some(anchor) => self.comments.take_split_by_newline_after(anchor, start),
                None => (None, self.comments.take_comments_before(start), false),
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

    fn match_arm_entries(&mut self, arms: &'a [MatchArm]) -> Vec<SiblingEntry<'a>> {
        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(arms.len());
        for arm in arms {
            let start = arm.pattern.get_span().byte_offset;
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), start);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }

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
            entries.push(SiblingEntry {
                leading,
                doc: arm_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }
        entries
    }

    fn match_(
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

    fn call_arg_entries(&mut self, args: &'a [Expression]) -> Vec<PatternEntry<'a>> {
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
        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(arms.len());
        for (i, arm) in arms.iter().enumerate() {
            let start = Self::select_arm_start(arm);
            let upper_bound = arms
                .get(i + 1)
                .map(Self::select_arm_start)
                .unwrap_or_else(|| span.end());
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), start);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            let arm_doc = self.select_arm_body(arm, upper_bound);
            entries.push(SiblingEntry {
                leading,
                doc: arm_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }
        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(Document::str("select"), body)
    }

    fn select_arm_start(arm: &'a SelectArm) -> u32 {
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

    fn select_arm_body(&mut self, arm: &'a SelectArm, upper_bound: u32) -> Document<'a> {
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
            let mut body = Document::Sequence(vec![]);
            for (i, entry) in field_entries.into_iter().enumerate() {
                if i > 0 {
                    body = body.append(Document::Newline);
                    if entry.has_blank_above {
                        body = body.append(Document::Newline);
                    }
                }
                let mut doc = match entry.leading {
                    Some(c) => c.append(Document::Newline).append(entry.doc),
                    None => entry.doc,
                };
                doc = doc.append(",");
                if let Some(t) = entry.trailing {
                    doc = doc.append(" ").append(t);
                }
                body = body.append(doc);
            }
            if let Some(t) = trailing {
                body = body
                    .append(Document::Newline)
                    .append(Document::Newline)
                    .append(t);
            }
            return Self::braced_body(header, body);
        }

        let fields_docs: Vec<_> = field_entries.into_iter().map(|entry| entry.doc).collect();
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
    ) -> (Vec<SiblingEntry<'a>>, Option<Document<'a>>, bool) {
        let mut entries: Vec<SiblingEntry<'a>> = Vec::new();
        let mut with_comments = false;
        let mut prev_anchor: Option<u32> = None;

        for field in fields {
            let leading_edge = field
                .attributes
                .first()
                .map(|a| a.span.byte_offset)
                .unwrap_or(field.name_span.byte_offset);
            let (trailing_for_prev, leading, has_blank) = match prev_anchor {
                Some(anchor) => self
                    .comments
                    .take_split_by_newline_after(anchor, leading_edge),
                None => (
                    None,
                    self.comments.take_comments_before(leading_edge),
                    false,
                ),
            };

            with_comments = with_comments || trailing_for_prev.is_some() || leading.is_some();

            if let Some(t) = trailing_for_prev
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }

            let field_attrs = self.field_attributes(&field.attributes);
            let between_attrs_and_name = self
                .comments
                .take_comments_before(field.name_span.byte_offset);

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

            let attrs_with_field = match between_attrs_and_name {
                Some(c) => field_attrs
                    .append(c.force_break())
                    .append(Document::Newline)
                    .append(field_definition),
                None => field_attrs.append(field_definition),
            };
            entries.push(SiblingEntry {
                leading,
                doc: attrs_with_field,
                trailing: None,
                has_blank_above: has_blank,
            });

            let ann_span = field.annotation.get_span();
            prev_anchor = Some(ann_span.byte_offset + ann_span.byte_length);
        }

        let (last_trailing, struct_trailing, _) = match prev_anchor {
            Some(anchor) => self
                .comments
                .take_split_by_newline_after(anchor, struct_end),
            None => (None, self.comments.take_comments_before(struct_end), false),
        };
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
            with_comments = true;
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

    /// Splits comments before `next_start` into `(prev_same_line, this_leading, has_blank)`.
    fn sibling_lead_split(
        &mut self,
        has_prev: bool,
        next_start: u32,
    ) -> (Option<Document<'a>>, Option<Document<'a>>, bool) {
        if has_prev {
            self.comments.take_split_at_line_start(next_start)
        } else {
            (None, self.comments.take_comments_before(next_start), false)
        }
    }

    /// Joins entries into a comma-separated body; returns `(body, close_sep)`.
    fn join_pattern_entries(
        entries: Vec<PatternEntry<'a>>,
        rest: Option<(Option<Document<'a>>, Document<'a>)>,
        trailing_unbroken: &'static str,
    ) -> (Document<'a>, Document<'a>) {
        let mut body = Document::Sequence(vec![]);
        let mut prev_had_trailing = false;
        let entry_count = entries.len();
        let separator = |prev_had_trailing: bool| {
            if prev_had_trailing {
                Document::Newline
            } else {
                strict_break(",", ", ")
            }
        };
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                body = body.append(separator(prev_had_trailing));
            }
            let mut elem = entry.doc;
            if let Some(c) = entry.leading {
                elem = c.append(Document::Newline).force_break().append(elem);
            }
            body = body.append(elem);
            if let Some(t) = entry.trailing {
                body = body
                    .append(Document::str(","))
                    .append(Document::str(" "))
                    .append(t.force_break());
                prev_had_trailing = true;
            } else {
                prev_had_trailing = false;
            }
        }
        if let Some((rest_leading, rest_doc)) = rest {
            if entry_count > 0 {
                body = body.append(separator(prev_had_trailing));
            }
            let mut rest_block = rest_doc;
            if let Some(c) = rest_leading {
                rest_block = c.append(Document::Newline).force_break().append(rest_block);
            }
            body = body.append(rest_block);
            prev_had_trailing = false;
        }
        let close_sep = if prev_had_trailing {
            strict_break("", trailing_unbroken)
        } else {
            strict_break(",", trailing_unbroken)
        };
        (body, close_sep)
    }

    /// Split-then-build: `build` runs after the split so its auto-drain sees the post-leading cursor.
    fn push_pattern_entry(
        &mut self,
        entries: &mut Vec<PatternEntry<'a>>,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) {
        let (last_trailing, leading, _) = self.sibling_lead_split(!entries.is_empty(), start);
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        let doc = build(self);
        entries.push(PatternEntry {
            leading,
            doc,
            trailing: None,
        });
    }

    /// Sibling split before a rest token; returns the rest's leading.
    fn split_for_rest(
        &mut self,
        entries: &mut Vec<PatternEntry<'a>>,
        rest_pos: u32,
    ) -> Option<Document<'a>> {
        let (last_trailing, rest_leading, _) =
            self.sibling_lead_split(!entries.is_empty(), rest_pos);
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        rest_leading
    }

    /// Joins sibling entries and drains body-trailing comments before `body_end`.
    fn join_sibling_body(
        &mut self,
        mut entries: Vec<SiblingEntry<'a>>,
        body_end: u32,
    ) -> Document<'a> {
        let standalone = if entries.is_empty() {
            self.comments.take_comments_before(body_end)
        } else {
            let (same_line, standalone, _) = self.comments.take_split_at_line_start(body_end);
            if let Some(t) = same_line
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            standalone
        };

        let mut body = Document::Sequence(vec![]);
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                body = body.append(Document::Newline);
                if entry.has_blank_above {
                    body = body.append(Document::Newline);
                }
            }
            if let Some(c) = entry.leading {
                body = body.append(c.force_break()).append(Document::Newline);
            }
            body = body.append(entry.doc);
            if let Some(t) = entry.trailing {
                body = body.append(Document::str(" ")).append(t);
            }
        }
        if let Some(s) = standalone {
            body = body
                .append(Document::Newline)
                .append(Document::Newline)
                .append(s.force_break());
        }
        body
    }

    /// Drains comments before `start` and prepends them to `build`'s output.
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

        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(variants.len());
        for variant in variants {
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), variant.name_span.byte_offset);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            let variant_doc = self.enum_variant_body(variant);
            entries.push(SiblingEntry {
                leading,
                doc: variant_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }
        let body = self.join_sibling_body(entries, span.end());
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

        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(variants.len());
        for variant in variants {
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), variant.name_span.byte_offset);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            let value_doc = self.literal(&variant.value);
            let variant_doc = Document::string(variant.name.to_string())
                .append(" = ")
                .append(value_doc)
                .append(",");
            entries.push(SiblingEntry {
                leading,
                doc: variant_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }
        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(header, body)
    }

    fn enum_variant_body(&mut self, variant: &'a EnumVariant) -> Document<'a> {
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
        let header = Document::str("interface ")
            .append(name)
            .append(generics_doc);

        if parents.is_empty() && methods.is_empty() {
            return header.append(" {}");
        }

        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(parents.len() + methods.len());

        for parent in parents {
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), parent.span.byte_offset);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            let parent_doc = Document::str("impl ").append(Self::annotation(&parent.annotation));
            entries.push(SiblingEntry {
                leading,
                doc: parent_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }

        for method in methods {
            let keyword_start = method.get_span().byte_offset;
            let leading_edge = match method {
                Expression::Function { attributes, .. } => attributes
                    .first()
                    .map(|a| a.span.byte_offset)
                    .unwrap_or(keyword_start),
                _ => keyword_start,
            };
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), leading_edge);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            let method_doc = self.interface_method_body(method, keyword_start);
            entries.push(SiblingEntry {
                leading,
                doc: method_doc,
                trailing: None,
                has_blank_above: has_blank,
            });
        }

        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(header, body)
    }

    fn interface_method_body(
        &mut self,
        method: &'a Expression,
        keyword_start: u32,
    ) -> Document<'a> {
        match method {
            Expression::Function {
                name,
                generics,
                params,
                return_annotation,
                attributes,
                ..
            } => {
                let attrs_doc = self.attributes(attributes);
                let between_attrs_and_keyword = self.comments.take_comments_before(keyword_start);
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
                    .append(return_doc);
                match between_attrs_and_keyword {
                    Some(c) => attrs_doc
                        .append(c.force_break())
                        .append(Document::Newline)
                        .append(signature),
                    None => attrs_doc.append(signature),
                }
            }
            _ => Document::Sequence(vec![]),
        }
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

        let mut entries: Vec<SiblingEntry<'a>> = Vec::with_capacity(methods.len());
        for method in methods {
            let start = method.get_span().byte_offset;
            let (last_trailing, leading, has_blank) =
                self.sibling_lead_split(!entries.is_empty(), start);
            if let Some(t) = last_trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            entries.push(SiblingEntry {
                leading,
                doc: self.definition(method),
                trailing: None,
                has_blank_above: has_blank,
            });
        }
        // Impl methods always get a blank line between them, regardless of source.
        for entry in entries.iter_mut().skip(1) {
            entry.has_blank_above = true;
        }
        let body = self.join_sibling_body(entries, impl_end);
        Self::braced_body(header, body)
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
                span,
                ..
            } => {
                if fields.is_empty() && !rest {
                    Document::string(identifier.to_string())
                } else {
                    let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(fields.len());
                    for f in fields {
                        self.push_pattern_entry(&mut entries, f.get_span().byte_offset, |s| {
                            s.pattern(f)
                        });
                    }
                    let rest_info = if *rest {
                        let rest_pos = self
                            .comments
                            .next_pair_at(b'.', span.byte_offset, span.end())
                            .unwrap_or(span.end());
                        let leading = self.split_for_rest(&mut entries, rest_pos);
                        Some((leading, Document::str("..")))
                    } else {
                        None
                    };
                    let (body, close_sep) = Self::join_pattern_entries(entries, rest_info, "");
                    Document::string(identifier.to_string())
                        .append("(")
                        .append(strict_break("", ""))
                        .append(body)
                        .nest(INDENT_WIDTH)
                        .append(close_sep)
                        .append(")")
                        .group()
                }
            }

            Pattern::Struct {
                identifier,
                fields,
                rest,
                span,
                ..
            } => {
                if fields.is_empty() && !rest {
                    Document::string(identifier.to_string()).append(" {}")
                } else {
                    let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(fields.len());
                    for f in fields {
                        self.push_pattern_entry(
                            &mut entries,
                            f.value.get_span().byte_offset,
                            |s| s.struct_field_pattern(f),
                        );
                    }
                    let rest_info = if *rest {
                        let rest_pos = self
                            .comments
                            .next_pair_at(b'.', span.byte_offset, span.end())
                            .unwrap_or(span.end());
                        let leading = self.split_for_rest(&mut entries, rest_pos);
                        Some((leading, Document::str("..")))
                    } else {
                        None
                    };
                    let (body, close_sep) = Self::join_pattern_entries(entries, rest_info, " ");
                    Document::string(identifier.to_string())
                        .append(" {")
                        .append(strict_break(" ", " "))
                        .append(body)
                        .nest(INDENT_WIDTH)
                        .append(close_sep)
                        .append("}")
                        .group()
                }
            }

            Pattern::Tuple { elements, .. } => {
                let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(elements.len());
                for element in elements {
                    self.push_pattern_entry(&mut entries, element.get_span().byte_offset, |s| {
                        s.pattern(element)
                    });
                }
                let (body, close_sep) = Self::join_pattern_entries(entries, None, "");
                Document::str("(")
                    .append(strict_break("", ""))
                    .append(body)
                    .nest(INDENT_WIDTH)
                    .append(close_sep)
                    .append(")")
                    .group()
            }

            Pattern::Slice { prefix, rest, .. } => {
                let mut entries: Vec<PatternEntry<'a>> = Vec::with_capacity(prefix.len());
                for pattern in prefix {
                    self.push_pattern_entry(&mut entries, pattern.get_span().byte_offset, |s| {
                        s.pattern(pattern)
                    });
                }
                let rest_info = match rest {
                    RestPattern::Absent => None,
                    RestPattern::Discard(rest_span) => {
                        let leading = self.split_for_rest(&mut entries, rest_span.byte_offset);
                        Some((leading, Document::str("..")))
                    }
                    RestPattern::Bind {
                        name,
                        span: rest_span,
                    } => {
                        let leading = self.split_for_rest(&mut entries, rest_span.byte_offset);
                        Some((
                            leading,
                            Document::str("..").append(Document::string(name.to_string())),
                        ))
                    }
                };
                let (body, close_sep) = Self::join_pattern_entries(entries, rest_info, "");
                Document::str("[")
                    .append(strict_break("", ""))
                    .append(body)
                    .nest(INDENT_WIDTH)
                    .append(close_sep)
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
