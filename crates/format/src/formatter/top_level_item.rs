use super::Formatter;
use super::sequence::SiblingEntry;
use crate::INDENT_WIDTH;
use crate::lindig::{Document, join, strict_break};
use syntax::ast::{
    Annotation, Attribute, AttributeArg, Binding, EnumVariant, Expression, Generic,
    ParentInterface, Span, StructFieldDefinition, StructKind, VariantFields,
};

impl<'a> Formatter<'a> {
    pub(super) fn function(
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

    pub(super) fn wrap_params(params_docs: Vec<Document<'a>>) -> Document<'a> {
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

    pub(super) fn struct_definition(
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

    pub(super) fn braced_body(header: Document<'a>, body: Document<'a>) -> Document<'a> {
        header
            .append(" {")
            .append(Document::Newline.append(body).nest(INDENT_WIDTH))
            .append(Document::Newline)
            .append("}")
            .force_break()
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

    pub(super) fn enum_definition(
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
            self.push_sibling_entry(&mut entries, variant.name_span.byte_offset, |s| {
                s.enum_variant_body(variant)
            });
        }
        let body = self.join_sibling_body(entries, span.end());
        Self::braced_body(header, body)
    }

    pub(super) fn value_enum_definition(
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
            self.push_sibling_entry(&mut entries, variant.name_span.byte_offset, |s| {
                let value_doc = s.literal(&variant.value);
                Document::string(variant.name.to_string())
                    .append(" = ")
                    .append(value_doc)
                    .append(",")
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

    pub(super) fn type_alias(
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

    pub(super) fn interface(
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
            self.push_sibling_entry(&mut entries, parent.span.byte_offset, |_| {
                Document::str("impl ").append(Self::annotation(&parent.annotation))
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
            self.push_sibling_entry(&mut entries, leading_edge, |s| {
                s.interface_method_body(method, keyword_start)
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

    pub(super) fn impl_block(
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
            self.push_sibling_entry(&mut entries, start, |s| s.definition(method));
        }
        // Impl methods always get a blank line between them, regardless of source.
        for entry in entries.iter_mut().skip(1) {
            entry.has_blank_above = true;
        }
        let body = self.join_sibling_body(entries, impl_end);
        Self::braced_body(header, body)
    }

    pub(super) fn const_definition(
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

    pub(super) fn binding(&mut self, binding: &'a Binding) -> Document<'a> {
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

    pub(super) fn annotation(annotation: &'a Annotation) -> Document<'a> {
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

    pub(super) fn generics(generics: &'a [Generic]) -> Document<'a> {
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

    pub(super) fn attribute(&mut self, attribute: &'a Attribute) -> Document<'a> {
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

    pub(super) fn attributes(&mut self, attrs: &'a [Attribute]) -> Document<'a> {
        if attrs.is_empty() {
            return Document::Sequence(vec![]);
        }

        let attribute_docs: Vec<_> = attrs.iter().map(|a| self.attribute(a)).collect();
        join(attribute_docs, Document::Newline).append(Document::Newline)
    }
}
