use super::Formatter;
use super::sequence::PatternEntry;
use crate::INDENT_WIDTH;
use crate::comments::prepend_comments;
use crate::lindig::{Document, join, strict_break};
use syntax::ast::{Pattern, RestPattern, StructFieldPattern};

impl<'a> Formatter<'a> {
    pub(super) fn pattern(&mut self, pat: &'a Pattern) -> Document<'a> {
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
                    if *rest {
                        let rest_pos = self
                            .comments
                            .next_pair_at(b'.', span.byte_offset, span.end())
                            .unwrap_or(span.end());
                        self.push_pattern_entry(&mut entries, rest_pos, |_| Document::str(".."));
                    }
                    let joined = Self::join_pattern_entries(entries, "");
                    Document::string(identifier.to_string())
                        .append("(")
                        .append(strict_break("", ""))
                        .append(joined.body)
                        .nest(INDENT_WIDTH)
                        .append(joined.close_separator)
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
                    if *rest {
                        let rest_pos = self
                            .comments
                            .next_pair_at(b'.', span.byte_offset, span.end())
                            .unwrap_or(span.end());
                        self.push_pattern_entry(&mut entries, rest_pos, |_| Document::str(".."));
                    }
                    let joined = Self::join_pattern_entries(entries, " ");
                    Document::string(identifier.to_string())
                        .append(" {")
                        .append(strict_break("", " "))
                        .append(joined.body)
                        .nest(INDENT_WIDTH)
                        .append(joined.close_separator)
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
                let joined = Self::join_pattern_entries(entries, "");
                Document::str("(")
                    .append(strict_break("", ""))
                    .append(joined.body)
                    .nest(INDENT_WIDTH)
                    .append(joined.close_separator)
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
                match rest {
                    RestPattern::Absent => {}
                    RestPattern::Discard(rest_span) => {
                        self.push_pattern_entry(&mut entries, rest_span.byte_offset, |_| {
                            Document::str("..")
                        });
                    }
                    RestPattern::Bind {
                        name,
                        span: rest_span,
                    } => {
                        self.push_pattern_entry(&mut entries, rest_span.byte_offset, |_| {
                            Document::str("..").append(Document::string(name.to_string()))
                        });
                    }
                }
                let joined = Self::join_pattern_entries(entries, "");
                Document::str("[")
                    .append(strict_break("", ""))
                    .append(joined.body)
                    .nest(INDENT_WIDTH)
                    .append(joined.close_separator)
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
}
