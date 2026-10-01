//! Cursor resolution shared by definition, references, and rename.

use syntax::ast::{
    Annotation, Binding, Expression, Generic, IdentifierResolution, Pattern, SelectArm, Span,
};
use syntax::program::File;

use crate::analysis::{cursor_offset, find_package_by_alias, offset_in_span, type_name};
use crate::definition::{
    find_struct_field_span, is_shorthand_field, lookup_definition_span, record_pattern_field_span,
    resolve_dot_access_definition, resolve_enum_in_pattern, resolve_import_span,
};
use crate::snapshot::{AnalysisSnapshot, SnapshotPosition};
use crate::traversal::find_expression_at;

pub(crate) enum OccurrenceKind {
    Declaration,
    Reference,
    Import,
    /// A shorthand pattern names a field and introduces a separate local binding.
    ShorthandBinding {
        binding_span: Span,
    },
}

pub(crate) struct ResolvedSymbol {
    pub(crate) kind: OccurrenceKind,
    pub(crate) occurrence_span: Span,
    pub(crate) definition_span: Span,
}

impl ResolvedSymbol {
    pub(crate) fn reference_definition_span(&self) -> Span {
        match self.kind {
            OccurrenceKind::ShorthandBinding { binding_span } => binding_span,
            _ => self.definition_span,
        }
    }

    pub(crate) fn is_import(&self) -> bool {
        matches!(self.kind, OccurrenceKind::Import)
    }
}

pub(crate) fn resolve_symbol(
    snapshot: &AnalysisSnapshot,
    cursor: &SnapshotPosition<'_>,
) -> Option<ResolvedSymbol> {
    let file = cursor.document.file;
    let offset = cursor_offset(&file.source, cursor.offset)?;
    let resolver = SymbolResolver {
        snapshot,
        file,
        offset,
    };
    let expression = find_expression_at(&file.items, offset)?;
    resolver.expression(expression).or_else(|| {
        snapshot
            .binding_at(cursor.document.file_id, offset)
            .map(|binding| ResolvedSymbol {
                kind: OccurrenceKind::Declaration,
                occurrence_span: binding.span,
                definition_span: binding.span,
            })
    })
}

struct SymbolResolver<'a> {
    snapshot: &'a AnalysisSnapshot,
    file: &'a File,
    offset: u32,
}

impl SymbolResolver<'_> {
    fn declaration(&self, span: Span) -> Option<ResolvedSymbol> {
        offset_in_span(self.offset, &span).then_some(ResolvedSymbol {
            kind: OccurrenceKind::Declaration,
            occurrence_span: span,
            definition_span: span,
        })
    }

    fn expression(&self, expression: &Expression) -> Option<ResolvedSymbol> {
        match expression {
            Expression::Identifier {
                value,
                span,
                resolution,
                ..
            } => {
                let definition = match resolution {
                    IdentifierResolution::Binding(identifier) => self
                        .snapshot
                        .bindings()
                        .get(identifier)
                        .map(|binding| binding.span),
                    IdentifierResolution::Definition(name) => self
                        .snapshot
                        .definitions()
                        .get(name.as_str())
                        .and_then(|definition| definition.name_span),
                    _ => None,
                };
                self.name(value, *span, definition)
            }
            Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } => {
                let member_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - member.len() as u32,
                    member.len() as u32,
                );
                let definition = resolve_dot_access_definition(
                    expression,
                    member,
                    *span,
                    self.file,
                    self.snapshot,
                )?;
                self.name(member, member_span, Some(definition))
            }
            Expression::StructCall {
                name,
                field_assignments,
                ty,
                span,
                ..
            } => {
                if let Some(field) = field_assignments
                    .iter()
                    .find(|field| offset_in_span(self.offset, &field.name_span))
                {
                    let type_name = type_name(ty, self.snapshot)?;
                    let definition =
                        find_struct_field_span(&type_name, &field.name, self.snapshot)?;
                    return self.name(&field.name, field.name_span, Some(definition));
                }
                let definition =
                    lookup_definition_span(name, self.file, self.snapshot).or_else(|| {
                        let type_name = type_name(ty, self.snapshot)?;
                        self.snapshot
                            .definitions()
                            .get(type_name.as_str())?
                            .name_span
                    });
                self.name(name, *span, definition)
            }
            Expression::Function {
                name_span,
                params,
                return_annotation,
                generics,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.generics(generics))
                .or_else(|| params.iter().find_map(|binding| self.binding(binding)))
                .or_else(|| self.annotation(return_annotation)),
            Expression::Lambda {
                params,
                return_annotation,
                ..
            } => params
                .iter()
                .find_map(|binding| self.binding(binding))
                .or_else(|| self.annotation(return_annotation)),
            Expression::Let { binding, .. } | Expression::For { binding, .. } => {
                self.binding(binding)
            }
            Expression::Struct {
                name_span,
                fields,
                generics,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.generics(generics))
                .or_else(|| {
                    fields.iter().find_map(|field| {
                        self.declaration(field.name_span)
                            .or_else(|| self.annotation(&field.annotation))
                    })
                }),
            Expression::Enum {
                name_span,
                variants,
                generics,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.generics(generics))
                .or_else(|| {
                    variants.iter().find_map(|variant| {
                        self.declaration(variant.name_span).or_else(|| {
                            variant.fields.iter().find_map(|field| {
                                self.declaration(field.name_span)
                                    .or_else(|| self.annotation(&field.annotation))
                            })
                        })
                    })
                }),
            Expression::TypeAlias {
                name_span,
                annotation,
                generics,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.generics(generics))
                .or_else(|| self.annotation(annotation)),
            Expression::Interface {
                name_span,
                generics,
                parents,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.generics(generics))
                .or_else(|| {
                    parents
                        .iter()
                        .find_map(|parent| self.annotation(&parent.annotation))
                }),
            Expression::ImplBlock {
                annotation,
                generics,
                ..
            } => self
                .annotation(annotation)
                .or_else(|| self.generics(generics)),
            Expression::Const {
                identifier_span,
                annotation,
                ..
            } => self.declaration(*identifier_span).or_else(|| {
                annotation
                    .as_ref()
                    .and_then(|annotation| self.annotation(annotation))
            }),
            Expression::VariableDeclaration {
                name_span,
                annotation,
                ..
            } => self
                .declaration(*name_span)
                .or_else(|| self.annotation(annotation)),
            Expression::Call { type_arguments, .. } => type_arguments
                .annotations()
                .find_map(|annotation| self.annotation(annotation)),
            Expression::Cast { target_type, .. } => self.annotation(target_type),
            Expression::Match { arms, .. } => {
                arms.iter().find_map(|arm| self.pattern(&arm.pattern))
            }
            Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
                self.pattern(pattern)
            }
            Expression::Select { arms, .. } => arms.iter().find_map(|arm| match arm {
                SelectArm::Receive { binding, .. } => self.pattern(binding),
                SelectArm::MatchReceive { arms, .. } => {
                    arms.iter().find_map(|arm| self.pattern(&arm.pattern))
                }
                SelectArm::Send { .. } | SelectArm::WildCard { .. } => None,
            }),
            _ => None,
        }
    }

    fn binding(&self, binding: &Binding) -> Option<ResolvedSymbol> {
        self.pattern(&binding.pattern).or_else(|| {
            binding
                .annotation
                .as_ref()
                .and_then(|annotation| self.annotation(annotation))
        })
    }

    fn generics(&self, generics: &[Generic]) -> Option<ResolvedSymbol> {
        generics
            .iter()
            .flat_map(Generic::bounds)
            .find_map(|annotation| self.annotation(annotation))
    }

    fn annotation(&self, annotation: &Annotation) -> Option<ResolvedSymbol> {
        if !offset_in_span(self.offset, &annotation.get_span()) {
            return None;
        }
        match annotation {
            Annotation::Constructor {
                name, params, span, ..
            } => params
                .iter()
                .find_map(|annotation| self.annotation(annotation))
                .or_else(|| self.name(name, *span, None)),
            Annotation::Function {
                params,
                return_type,
                ..
            } => params
                .iter()
                .find_map(|annotation| self.annotation(annotation))
                .or_else(|| self.annotation(return_type)),
            Annotation::Tuple { elements, .. } => elements
                .iter()
                .find_map(|annotation| self.annotation(annotation)),
            Annotation::Unknown | Annotation::Opaque { .. } | Annotation::Constant { .. } => None,
        }
    }

    fn pattern(&self, pattern: &Pattern) -> Option<ResolvedSymbol> {
        let span = pattern.get_span();
        if !offset_in_span(self.offset, &span) {
            return None;
        }
        match pattern {
            Pattern::EnumVariant {
                identifier, fields, ..
            } => fields
                .iter()
                .find_map(|pattern| self.pattern(pattern))
                .or_else(|| {
                    let definition =
                        resolve_enum_in_pattern(pattern, self.offset, self.file, self.snapshot);
                    self.name(identifier, span, definition)
                }),
            Pattern::Struct {
                identifier,
                fields,
                resolution,
                ..
            } => {
                for field in fields {
                    let field_span = field.value.get_span();
                    if offset_in_span(self.offset, &field_span) {
                        if is_shorthand_field(field, span, self.snapshot)
                            && let Some(definition_span) =
                                record_pattern_field_span(self.snapshot, resolution, &field.name)
                            && let Some(binding) =
                                self.snapshot.binding_at(span.file_id, self.offset)
                        {
                            return Some(ResolvedSymbol {
                                kind: OccurrenceKind::ShorthandBinding {
                                    binding_span: binding.span,
                                },
                                occurrence_span: binding.span,
                                definition_span,
                            });
                        }
                        return self.pattern(&field.value);
                    }
                }
                let definition =
                    resolve_enum_in_pattern(pattern, self.offset, self.file, self.snapshot);
                self.name(identifier, span, definition)
            }
            Pattern::Tuple { elements, .. }
            | Pattern::Or {
                patterns: elements, ..
            } => elements.iter().find_map(|pattern| self.pattern(pattern)),
            Pattern::Slice { prefix, .. } => {
                prefix.iter().find_map(|pattern| self.pattern(pattern))
            }
            Pattern::AsBinding { pattern, .. } => self.pattern(pattern),
            Pattern::Identifier { .. } => self
                .snapshot
                .binding_at(span.file_id, self.offset)
                .and_then(|binding| self.declaration(binding.span)),
            Pattern::Literal { .. } | Pattern::Unit { .. } | Pattern::WildCard { .. } => None,
        }
    }

    /// Source text only locates segments of a name supplied by the AST.
    fn name(&self, name: &str, span: Span, definition: Option<Span>) -> Option<ResolvedSymbol> {
        let (occurrence_span, prefix) =
            name_segment_at(&self.file.source, name, span, self.offset)?;
        if prefix == name
            && let Some(definition_span) = definition
        {
            return Some(ResolvedSymbol {
                kind: OccurrenceKind::Reference,
                occurrence_span,
                definition_span,
            });
        }
        let package_names = &self.snapshot.analysis.emit_input.go_package_names;
        if let Some(definition_span) = resolve_import_span(prefix, self.file, package_names) {
            return Some(ResolvedSymbol {
                kind: OccurrenceKind::Import,
                occurrence_span,
                definition_span,
            });
        }
        let definition_span = if let Some((qualifier, member)) = prefix.split_once('.')
            && let Some(package) = find_package_by_alias(self.file, qualifier, package_names)
        {
            let qualified = format!("{package}.{member}");
            self.snapshot
                .definitions()
                .get(qualified.as_str())
                .and_then(|definition| definition.name_span)
        } else {
            lookup_definition_span(prefix, self.file, self.snapshot)
        }?;
        Some(ResolvedSymbol {
            kind: OccurrenceKind::Reference,
            occurrence_span,
            definition_span,
        })
    }
}

/// Match an AST name against its source spelling, including spaced qualifiers.
/// A comment, literal, or payload inside the containing span is never a segment.
fn name_segment_at<'a>(
    source: &str,
    name: &'a str,
    span: Span,
    offset: u32,
) -> Option<(Span, &'a str)> {
    let start = span.byte_offset as usize;
    let end = start + span.byte_length as usize;
    let mut remaining = source.get(start..end)?;
    let mut source_offset = start;
    let mut name_offset = 0;
    for segment in name.split('.') {
        let trimmed = remaining.trim_start();
        source_offset += remaining.len() - trimmed.len();
        remaining = trimmed.strip_prefix(segment)?;
        let segment_span = Span::new(span.file_id, source_offset as u32, segment.len() as u32);
        name_offset += segment.len();
        if offset_in_span(offset, &segment_span) {
            return Some((segment_span, &name[..name_offset]));
        }
        source_offset += segment.len();
        let trimmed = remaining.trim_start();
        source_offset += remaining.len() - trimmed.len() + 1;
        remaining = trimmed.strip_prefix('.')?;
        name_offset += 1;
    }
    None
}
