mod expression;
mod pattern;
mod sequence;
mod top_level_item;

use crate::comments::Comments;
use crate::lindig::{Document, concat, join};
use syntax::ast::{Attribute, Expression, ImportAlias, Visibility};

pub struct Formatter<'a> {
    pub(super) comments: Comments<'a>,
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
}
