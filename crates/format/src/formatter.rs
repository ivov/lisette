mod expression;
mod pattern;
mod sequence;
mod top_level_item;

use crate::comments::{Comments, SplitComments};
use crate::lindig::{Document, concat};
use syntax::ast::{Attribute, Expression, ImportAlias, Visibility};

pub struct Formatter<'a> {
    comments: Comments<'a>,
}

struct Import<'a> {
    expression: &'a Expression,
    name: &'a str,
    alias: Option<&'a ImportAlias>,
}

impl Import<'_> {
    fn is_go(&self) -> bool {
        self.name.starts_with("go:")
    }

    fn sort_path(&self) -> &str {
        match self.alias {
            Some(ImportAlias::Named(alias, _)) => alias,
            Some(ImportAlias::Blank(_)) => "_",
            None => self
                .name
                .split_once(':')
                .map_or(self.name, |(_, path)| path),
        }
    }
}

impl<'a> Formatter<'a> {
    pub(crate) fn new(comments: Comments<'a>) -> Self {
        Self { comments }
    }

    pub(crate) fn module(&mut self, top_level_items: &'a [Expression]) -> Document<'a> {
        let mut imports = Vec::new();
        let mut rest = Vec::new();
        for expression in top_level_items {
            if let Expression::ModuleImport { name, alias, .. } = expression {
                imports.push(Import {
                    expression,
                    name,
                    alias: alias.as_ref(),
                });
            } else {
                rest.push(expression);
            }
        }

        let mut docs = Vec::new();

        if let Some(file_comment) = self.comments.take_file_comments() {
            docs.push(file_comment.force_break());
        }

        if !imports.is_empty() {
            if !docs.is_empty() {
                docs.push(Document::Newline);
                docs.push(Document::Newline);
            }
            docs.push(self.sort_imports(imports));
        }

        let mut prev_end: Option<u32> = None;
        for (i, item) in rest.iter().enumerate() {
            let start = Self::item_leading_edge(item);

            let split = match prev_end {
                Some(anchor) => self.comments.take_split_by_newline_after(anchor, start),
                None => SplitComments::leading(self.comments.take_comments_before(start)),
            };

            if let Some(t) = split.trailing {
                docs.push(Document::str(" "));
                docs.push(t);
            }

            if let Some(comment_doc) = split.leading {
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

    fn sort_imports(&mut self, mut imports: Vec<Import<'a>>) -> Document<'a> {
        if imports.is_empty() {
            return Document::Sequence(vec![]);
        }

        let mut leading_comments: Option<Document<'a>> = None;
        let mut leading_has_blank_line = false;

        for (i, import) in imports.iter().enumerate() {
            let start = import.expression.get_span().byte_offset;
            let comments = self.comments.take_comments_and_blank_lines_before(start);
            if i == 0 && comments.document.is_some() {
                leading_comments = comments.document;
                leading_has_blank_line = comments.has_blank_line;
            }
        }

        imports.sort_by(|left, right| {
            (!left.is_go(), left.sort_path(), left.name).cmp(&(
                !right.is_go(),
                right.sort_path(),
                right.name,
            ))
        });

        let mut import_docs = Vec::new();
        let mut previous_is_go = None;
        for import in imports {
            if previous_is_go.is_some_and(|is_go| is_go != import.is_go()) {
                import_docs.push(Document::Newline);
            }
            if previous_is_go.is_some() {
                import_docs.push(Document::Newline);
            }
            previous_is_go = Some(import.is_go());
            import_docs.push(self.definition(import.expression));
        }
        let imports_doc = concat(import_docs);

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

        let attrs = self.attributes(Self::definition_attributes(expression));
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
                visibility,
                span,
                ..
            } => (
                *visibility,
                self.struct_definition(name, generics, fields, span),
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

    fn item_leading_edge(item: &'a Expression) -> u32 {
        Self::definition_attributes(item)
            .first()
            .map(|attribute| attribute.span.byte_offset)
            .unwrap_or_else(|| item.get_span().byte_offset)
    }

    fn definition_attributes(expression: &'a Expression) -> &'a [Attribute] {
        match expression {
            Expression::Function { attributes, .. }
            | Expression::Struct { attributes, .. }
            | Expression::Enum { attributes, .. }
            | Expression::TypeAlias { attributes, .. } => attributes,
            _ => &[],
        }
    }
}
