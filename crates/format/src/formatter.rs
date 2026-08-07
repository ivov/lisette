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
    hoisted: bool,
    end: u32,
    next_item: u32,
    leading: Option<Document<'a>>,
    trailing: Option<Document<'a>>,
}

impl Import<'_> {
    fn is_go(&self) -> bool {
        self.name.starts_with("go:")
    }

    fn sort_path(&self) -> &str {
        crate::import_sort_key(self.name, self.alias)
    }
}

impl<'a> Formatter<'a> {
    pub(crate) fn new(comments: Comments<'a>) -> Self {
        Self { comments }
    }

    pub(crate) fn package(&mut self, top_level_items: &'a [Expression]) -> Document<'a> {
        let mut imports = Vec::new();
        let mut rest = Vec::new();
        for (index, expression) in top_level_items.iter().enumerate() {
            if let Expression::PackageImport {
                name,
                alias,
                name_span,
                ..
            } = expression
            {
                imports.push(Import {
                    expression,
                    name,
                    alias: alias.as_ref(),
                    hoisted: !rest.is_empty(),
                    end: name_span.end(),
                    next_item: top_level_items
                        .get(index + 1)
                        .map_or(u32::MAX, Self::item_leading_edge),
                    leading: None,
                    trailing: None,
                });
            } else {
                rest.push(expression);
            }
        }

        let mut docs = Vec::new();

        if let Some(shebang) = self.comments.take_shebang() {
            docs.push(shebang);
        }

        if let Some(file_comment) = self.comments.take_file_comments() {
            if !docs.is_empty() {
                docs.push(Document::Newline);
                docs.push(Document::Newline);
            }
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

        let header = self.take_import_comments(&mut imports);

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
            if let Some(leading) = import.leading {
                import_docs.push(leading.force_break());
                import_docs.push(Document::Newline);
            }
            import_docs.push(Self::import_line(import.name, import.alias));
            if let Some(trailing) = import.trailing {
                import_docs.push(Document::str(" "));
                import_docs.push(trailing);
            }
        }
        let imports_doc = concat(import_docs);

        match header {
            Some(header) => header
                .force_break()
                .append(concat([Document::Newline, Document::Newline]))
                .append(imports_doc),
            None => imports_doc,
        }
    }

    fn take_import_comments(&mut self, imports: &mut [Import<'a>]) -> Option<Document<'a>> {
        for import in imports.iter_mut() {
            import.trailing = self
                .comments
                .take_trailing_comments_after(import.end, import.next_item);
        }

        let mut header = None;

        if !imports[0].hoisted {
            let leading = self
                .comments
                .take_leading_comments_before(imports[0].expression.get_span().byte_offset);
            header = leading.header;
            imports[0].leading = leading.attached;
        }

        for import in imports.iter_mut().skip(1) {
            if import.hoisted {
                continue;
            }
            let start = import.expression.get_span().byte_offset;
            import.leading = self.comments.take_comments_before(start);
        }

        header
    }

    fn import_line(name: &str, alias: Option<&ImportAlias>) -> Document<'a> {
        let alias_doc = match alias {
            Some(ImportAlias::Named(alias, _)) => Document::string(alias.to_string()).append(" "),
            Some(ImportAlias::Blank(_)) => Document::str("_ "),
            None => Document::str(""),
        };

        Document::str("import ")
            .append(alias_doc)
            .append("\"")
            .append(Document::string(name.to_string()))
            .append("\"")
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

            Expression::PackageImport { name, alias, .. } => {
                (Visibility::Private, Self::import_line(name, alias.as_ref()))
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
