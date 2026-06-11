use ecow::EcoString;

use super::Parser;
use crate::ast::{
    Annotation, Attribute, AttributeArg, EnumFieldDefinition, EnumVariant, Expression, Generic,
    ParentInterface, Span, StructFieldDefinition, StructKind, VariantFields, Visibility,
};
use crate::lex::Token;
use crate::lex::TokenKind::*;
use crate::parse::error::ParseError;
use crate::types::Type;

impl<'source> Parser<'source> {
    pub(crate) fn parse_attributes(&mut self) -> Vec<Attribute> {
        let mut attributes = vec![];
        loop {
            self.advance_if(Semicolon);
            if !self.is(Hash) {
                break;
            }
            if let Some(attribute) = self.parse_attribute() {
                attributes.push(attribute);
            }
        }
        attributes
    }

    fn parse_attribute(&mut self) -> Option<Attribute> {
        let start = self.current_token();
        self.ensure(Hash);

        if !self.is(LeftSquareBracket) {
            self.track_error("expected `[` after `#`", "Add `[` to start the attribute");
            return None;
        }
        self.next();

        if !self.is(Identifier) {
            self.track_error(
                "expected attribute name",
                "Add an attribute name like `json` or `db`",
            );
            while self.is_not(RightSquareBracket) && !self.at_eof() {
                self.next();
            }
            self.advance_if(RightSquareBracket);
            return None;
        }

        let name = self.read_identifier();
        let args = if self.advance_if(LeftParen) {
            self.parse_attribute_args()
        } else {
            vec![]
        };

        if !self.advance_if(RightSquareBracket) {
            self.track_error("expected `]`", "Add `]` to close the attribute");
        }

        Some(Attribute {
            name: name.to_string(),
            args,
            span: self.span_from_tokens(start),
        })
    }

    fn parse_attribute_args(&mut self) -> Vec<AttributeArg> {
        let mut args = vec![];

        while self.is_not(RightParen) && !self.at_eof() {
            if let Some(arg) = self.parse_attribute_arg() {
                args.push(arg);
            }

            if !self.advance_if(Comma) {
                break;
            }
        }

        self.ensure(RightParen);
        args
    }

    fn parse_attribute_arg(&mut self) -> Option<AttributeArg> {
        if self.advance_if(Bang) {
            if self.is(Identifier) {
                return Some(AttributeArg::NegatedFlag(
                    self.read_identifier().to_string(),
                ));
            } else {
                self.track_error(
                    "expected identifier after `!`",
                    "Add an identifier like `omitempty` after `!`",
                );
                return None;
            }
        }

        if self.is(Identifier) {
            return Some(AttributeArg::Flag(self.read_identifier().to_string()));
        }

        if self.is(String) {
            let token = self.current_token();
            self.next();
            let text = token.text;
            let value = if text.len() >= 2 {
                &text[1..text.len() - 1]
            } else {
                text
            };
            return Some(AttributeArg::String(value.to_string()));
        }

        if self.is(Backtick) {
            let token = self.current_token();
            self.next();
            let text = token.text;
            let value = if text.len() >= 2 {
                &text[1..text.len() - 1]
            } else {
                text
            };
            return Some(AttributeArg::Raw(value.to_string()));
        }

        self.track_error(
            "expected attribute argument",
            "Add a flag (e.g. `omitempty`), string (e.g. `\"name\"`), or raw tag (e.g. `` `json:\"name\"` ``)",
        );
        None
    }

    pub fn parse_enum_definition(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Expression {
        let start = self.current_token();

        self.ensure(Enum);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();
        let generics = self.parse_generics();

        let underlying_start = self.current_token();
        if self.advance_if(Colon) {
            let _ = self.parse_annotation();
            let underlying_span = Span::new(
                self.file_id,
                underlying_start.byte_offset,
                underlying_start.byte_length,
            );
            let error = ParseError::new(
                "Enum with underlying type",
                underlying_span,
                "enums cannot have an underlying type",
            )
            .with_parse_code("enum_underlying_type")
            .with_help(
                "Remove the `: type` annotation. To model a Go defined primitive type, use a named primitive type such as `pub struct Weekday(int)` with package-level constants.",
            );
            self.errors.push(error);
        }

        self.ensure(LeftCurlyBrace);

        self.parse_regular_enum_body(doc, attributes, name, name_span, generics, start)
    }

    fn parse_regular_enum_body(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        start: Token<'source>,
    ) -> Expression {
        let mut variants = vec![];
        let mut seen_variants: Vec<(EcoString, Span)> = vec![];

        while self.is_not(RightCurlyBrace) {
            let start_position = self.stream.position;

            let variant_doc = self.collect_doc_comments().map(|(text, _)| text);
            if let Some(variant) = self.parse_enum_variant_with_doc(variant_doc) {
                if let Some((_, first_span)) =
                    seen_variants.iter().find(|(n, _)| n == &variant.name)
                {
                    self.error_duplicate_enum_variant(
                        &variant.name,
                        *first_span,
                        variant.name_span,
                    );
                } else {
                    seen_variants.push((variant.name.clone(), variant.name_span));
                }
                variants.push(variant);
            }
            self.expect_comma_or(RightCurlyBrace);
            self.ensure_progress(start_position, RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        Expression::Enum {
            doc,
            attributes,
            name,
            name_span,
            generics,
            variants,
            visibility: Visibility::Private,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_enum_variant_with_doc(
        &mut self,
        doc: Option<std::string::String>,
    ) -> Option<EnumVariant> {
        if self.is_not(Identifier) {
            self.track_error(
                "expected variant name",
                "Variant names must be identifiers.",
            );
            return None;
        }

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();

        if self.is(Equal) {
            let eq_token = self.current_token();
            let eq_span = Span::new(self.file_id, eq_token.byte_offset, eq_token.byte_length);
            let error = ParseError::new(
                "Assigned enum variant",
                eq_span,
                "enum variants cannot have assigned values",
            )
            .with_parse_code("enum_assigned_variant")
            .with_help(
                "Lisette enums are sum types, not Go const groups. To model a Go defined primitive type, use a named primitive type such as `pub struct Weekday(int)` with package-level constants.",
            );
            self.errors.push(error);
            self.next(); // consume `=`
            self.skip_assigned_variant_value();
        }

        let fields = self.parse_enum_variant_fields();

        Some(EnumVariant {
            doc,
            name,
            name_span,
            fields,
        })
    }

    fn skip_assigned_variant_value(&mut self) {
        self.advance_if(Minus);
        if self.is(Integer) || self.is(String) {
            self.next();
        }
    }

    fn parse_enum_variant_fields(&mut self) -> VariantFields {
        if self.advance_if(LeftParen) {
            return self.parse_tuple_variant_fields();
        }

        if self.advance_if(LeftCurlyBrace) {
            return self.parse_struct_variant_fields();
        }

        VariantFields::Unit
    }

    fn parse_tuple_variant_fields(&mut self) -> VariantFields {
        let mut fields = vec![];

        loop {
            if self.at_eof()
                || self.is(RightParen)
                || self.is(RightCurlyBrace)
                || !self.can_start_annotation()
            {
                break;
            }

            let field = EnumFieldDefinition {
                name: format!("field{}", fields.len()).into(),
                name_span: Span::dummy(),
                annotation: self.parse_annotation(),
                ty: Type::uninferred(),
            };

            fields.push(field);

            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        VariantFields::Tuple(fields)
    }

    fn parse_struct_variant_fields(&mut self) -> VariantFields {
        let mut fields = vec![];
        let mut seen_fields: Vec<(EcoString, Span)> = vec![];

        loop {
            if self.at_eof()
                || self.is(RightCurlyBrace)
                || self.at_item_boundary()
                || self.is_not(Identifier)
            {
                break;
            }

            let name_token = self.current_token();
            let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
            let name = self.read_identifier();
            self.ensure(Colon);
            let annotation = self.parse_annotation();

            if let Some((_, first_span)) = seen_fields.iter().find(|(n, _)| n == &name) {
                self.error_duplicate_struct_field(&name, *first_span, name_span);
            } else {
                seen_fields.push((name.clone(), name_span));
            }

            let field = EnumFieldDefinition {
                name,
                name_span,
                annotation,
                ty: Type::uninferred(),
            };

            fields.push(field);

            self.expect_comma_or(RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        VariantFields::Struct(fields)
    }

    pub fn parse_struct_definition(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Expression {
        let start = self.current_token();

        self.ensure(Struct);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();
        let generics = self.parse_generics();

        if self.is(LeftParen) {
            return self.parse_tuple_struct(doc, attributes, name, name_span, generics, start);
        }

        self.parse_named_struct(doc, attributes, name, name_span, generics, start)
    }

    fn parse_named_struct(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        start: Token<'source>,
    ) -> Expression {
        let mut fields = vec![];
        let mut seen_fields: Vec<(EcoString, Span)> = vec![];

        self.ensure(LeftCurlyBrace);

        while self.is_not(RightCurlyBrace) {
            let start_position = self.stream.position;

            let field_attributes = self.parse_attributes();
            let field_doc = self.collect_doc_comments().map(|(text, _)| text);
            if let Some(field) = self.parse_struct_field_with_doc(field_doc, field_attributes) {
                if let Some((_, first_span)) = seen_fields.iter().find(|(n, _)| n == &field.name) {
                    self.error_duplicate_struct_field(&field.name, *first_span, field.name_span);
                } else {
                    seen_fields.push((field.name.clone(), field.name_span));
                }
                fields.push(field);
            }
            self.expect_comma_or(RightCurlyBrace);
            self.ensure_progress(start_position, RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        Expression::Struct {
            doc,
            attributes,
            name,
            name_span,
            generics,
            fields,
            kind: StructKind::Record,
            visibility: Visibility::Private,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_tuple_struct(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        start: Token<'source>,
    ) -> Expression {
        self.ensure(LeftParen);

        let mut fields = vec![];
        let mut index = 0;

        while self.is_not(RightParen) {
            if self.at_eof() || self.at_item_boundary() || !self.can_start_annotation() {
                break;
            }

            let field_start = self.current_token();
            let annotation = self.parse_annotation();
            let field_span = self.span_from_tokens(field_start);

            fields.push(StructFieldDefinition {
                doc: None,
                attributes: vec![],
                name: format!("_{}", index).into(),
                name_span: field_span,
                annotation,
                visibility: Visibility::Private,
                ty: Type::uninferred(),
                embedded: false,
            });

            index += 1;
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        Expression::Struct {
            doc,
            attributes,
            name,
            name_span,
            generics,
            fields,
            kind: StructKind::Tuple,
            visibility: Visibility::Private,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_struct_field_with_doc(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Option<StructFieldDefinition> {
        let visibility = if self.advance_if(Pub) {
            Visibility::Public
        } else {
            Visibility::Private
        };

        if self.is(Mut) {
            self.track_error(
                "fields cannot be marked `mut`",
                "Fields cannot be marked `mut`; mutability applies to bindings (`let mut x = ...`).",
            );
            self.next();
        }

        // `embed T`, but not a field named `embed` (`embed: T`).
        if self.is(Identifier)
            && self.current_token().text == "embed"
            && self.stream.peek_ahead(1).kind != Colon
        {
            if visibility.is_public() {
                self.track_error(
                    "embedded field cannot be `pub`",
                    "An embedded field takes no `pub`; its visibility derives from the embedded type.",
                );
            }
            return self.parse_embedded_field(doc, attributes);
        }

        if self.is_not(Identifier) {
            self.track_error("expected field name", "Field names must be identifiers.");
            return None;
        }

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();

        self.ensure(Colon);

        Some(StructFieldDefinition {
            doc,
            attributes,
            visibility,
            name,
            name_span,
            annotation: self.parse_annotation(),
            ty: Type::uninferred(),
            embedded: false,
        })
    }

    fn parse_embedded_field(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Option<StructFieldDefinition> {
        let start = self.current_token();
        self.ensure(Identifier);

        let annotation = self.parse_annotation();
        let span = self.span_from_tokens(start);

        let Some(name) = Self::embedded_field_name(&annotation) else {
            self.track_error_at(
                span,
                "expected a named type after `embed`",
                "`embed` requires a named type; a function or tuple type cannot be embedded.",
            );
            return None;
        };

        if let Some(attribute) = attributes.first() {
            self.track_error_at(
                attribute.span,
                "embedded field cannot have attributes",
                "An embedded field takes no attributes; its serialization follows Go's anonymous-field inlining.",
            );
        }

        Some(StructFieldDefinition {
            doc,
            attributes: vec![],
            visibility: Visibility::Private,
            name,
            name_span: span,
            annotation,
            ty: Type::uninferred(),
            embedded: true,
        })
    }

    fn embedded_field_name(annotation: &Annotation) -> Option<EcoString> {
        let mut current = annotation;
        loop {
            let Annotation::Constructor { name, params, .. } = current else {
                return None;
            };
            let segment = name.rsplit('.').next().unwrap_or(name);
            if (segment == "Option" || segment == "Ref") && params.len() == 1 {
                current = &params[0];
                continue;
            }
            return Some(segment.into());
        }
    }

    pub fn parse_const_definition(&mut self, doc: Option<std::string::String>) -> Expression {
        let start = self.current_token();

        self.ensure(Const);

        let identifier_token = self.current_token();
        let identifier_span = Span::new(
            self.file_id,
            identifier_token.byte_offset,
            identifier_token.byte_length,
        );
        let identifier = self.read_identifier();
        let annotation = if self.advance_if(Colon) {
            Some(self.parse_annotation())
        } else {
            None
        };

        let expression = if self.advance_if(Equal) {
            self.parse_expression()
        } else {
            Expression::NoOp
        };

        Expression::Const {
            doc,
            identifier,
            identifier_span,
            annotation,
            expression: expression.into(),
            visibility: Visibility::Private,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_var_declaration(&mut self, doc: Option<std::string::String>) -> Expression {
        let start = self.current_token();

        self.ensure(Var);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();

        if self.advance_if(Equal) {
            return self.recover_var_initializer(start);
        }

        self.ensure(Colon);
        let annotation = self.parse_annotation();

        if self.advance_if(Equal) {
            return self.recover_var_initializer(start);
        }

        Expression::VariableDeclaration {
            doc,
            name,
            name_span,
            annotation,
            visibility: Visibility::Private,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn recover_var_initializer(&mut self, start: Token<'source>) -> Expression {
        self.parse_expression();
        self.error_var_initializer(self.span_from_token(start));

        Expression::Unit {
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_impl_block(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Impl);

        let generics = self.parse_generics();

        let receiver = self.parse_annotation(); // e.g. List<T>

        let (receiver_name, annotation) = match &receiver {
            Annotation::Constructor { name, .. } => (name.clone(), receiver),
            _ => {
                self.track_error("expected `impl` receiver", "Use `impl TypeName { ... }`.");
                ("".into(), Annotation::Unknown)
            }
        };

        if self.is(For) {
            self.track_error(
                "invalid syntax",
                "Lisette types satisfy interfaces automatically by having the required methods. Use `impl Type { ... }` to add methods.",
            );
            self.next();
            self.parse_annotation();
        }

        let mut methods = vec![];

        self.ensure(LeftCurlyBrace);

        while self.is_not(RightCurlyBrace) {
            self.advance_if(Semicolon);
            if self.is(RightCurlyBrace) {
                break;
            }

            let method_doc = self.collect_doc_comments();
            let method_attrs = self.parse_attributes();
            let is_public = self.advance_if(Pub);

            if self.is(Function) {
                let method = self.parse_function(method_doc.map(|(text, _)| text), method_attrs);
                let method = if is_public {
                    method.set_public()
                } else {
                    method
                };
                methods.push(method);
            } else {
                if let Some((_, span)) = method_doc {
                    self.error_detached_doc_comment(span);
                }
                self.track_error(
                    "expected `fn` in impl block",
                    "Only functions are allowed in `impl` blocks.",
                );
                self.next();
            }
        }

        self.ensure(RightCurlyBrace);

        Expression::ImplBlock {
            annotation,
            methods,
            receiver_name,
            generics,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_interface_definition(&mut self, doc: Option<std::string::String>) -> Expression {
        let start = self.current_token();

        self.ensure(Interface);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();

        let generics = self.parse_generics();

        let mut parents = vec![];
        let mut seen_parents: Vec<(EcoString, Span)> = vec![];
        let mut method_signatures = vec![];
        let mut seen_methods: Vec<(EcoString, Span)> = vec![];

        self.ensure(LeftCurlyBrace);

        while self.is_not(RightCurlyBrace) {
            self.advance_if(Semicolon);
            if self.is(RightCurlyBrace) {
                break;
            }

            let item_doc = self.collect_doc_comments();
            let method_attrs = self.parse_attributes();
            if !self.is(Function)
                && let Some(attribute) = method_attrs.first()
            {
                self.error_misplaced_attribute(attribute.span);
            }
            match self.current_token().kind {
                Function => {
                    let method =
                        self.parse_interface_method(item_doc.map(|(text, _)| text), method_attrs);
                    if let Expression::Function {
                        ref name,
                        ref name_span,
                        ..
                    } = method
                    {
                        if let Some((_, first_span)) = seen_methods.iter().find(|(n, _)| n == name)
                        {
                            self.error_duplicate_interface_method(name, *first_span, *name_span);
                        } else {
                            seen_methods.push((name.clone(), *name_span));
                        }
                    }
                    method_signatures.push(method);
                    self.advance_if(Semicolon);
                }

                Impl => {
                    let keyword = self.current_token();
                    let keyword_span =
                        Span::new(self.file_id, keyword.byte_offset, keyword.byte_length);
                    self.ensure(Impl);
                    self.parse_interface_parent(
                        item_doc,
                        Some(keyword_span),
                        &mut seen_parents,
                        &mut parents,
                    );
                }

                Identifier if self.current_token().text == "embed" => {
                    self.next();
                    self.parse_interface_parent(item_doc, None, &mut seen_parents, &mut parents);
                }

                _ => {
                    if let Some((_, span)) = item_doc {
                        self.error_detached_doc_comment(span);
                    }
                    self.track_error(
                        "expected `fn` or `embed`",
                        "Only method signatures (`fn`) and embeddings (`embed`) are allowed in interfaces.",
                    );
                    self.next();
                }
            }
        }

        self.ensure(RightCurlyBrace);

        Expression::Interface {
            doc,
            name,
            name_span,
            generics,
            parents,
            method_signatures,
            visibility: Visibility::Private,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_interface_parent(
        &mut self,
        item_doc: Option<(std::string::String, Span)>,
        impl_keyword_span: Option<Span>,
        seen_parents: &mut Vec<(EcoString, Span)>,
        parents: &mut Vec<ParentInterface>,
    ) {
        if let Some((_, span)) = item_doc {
            self.error_detached_doc_comment(span);
        }

        let parent_start = self.current_token();
        let annotation = self.parse_annotation();
        let parent_span = self.span_from_tokens(parent_start);

        if let Annotation::Constructor { name, .. } = &annotation {
            if let Some((_, first_span)) = seen_parents.iter().find(|(n, _)| n == name.as_str()) {
                self.error_duplicate_impl_parent(*first_span, parent_span);
            } else {
                seen_parents.push((name.clone(), parent_span));
            }
        }

        parents.push(ParentInterface {
            annotation,
            ty: Type::uninferred(),
            span: parent_span,
            impl_keyword_span,
        });
        self.advance_if(Semicolon);
    }
}
