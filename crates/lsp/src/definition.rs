use rustc_hash::FxHashMap;
use syntax::ast::{
    Annotation, Expression, MatchArm, Pattern, Span, StructFieldPattern, TypedPattern,
};
use syntax::types::unqualified_name;

use crate::analysis::find_module_by_alias;
use crate::offset_in_span;
use crate::snapshot::AnalysisSnapshot;
use crate::traversal::find_expression_at;
use crate::type_name;

pub(crate) fn get_root_expression(e: &Expression) -> &Expression {
    let mut current = e;
    while let Expression::DotAccess { expression, .. } = current {
        current = expression;
    }
    current
}

pub(crate) fn find_struct_field_span(
    type_id: &str,
    field_name: &str,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    use syntax::program::{Definition, DefinitionBody};

    if let Some(Definition {
        body: DefinitionBody::Struct { fields, .. },
        ..
    }) = snapshot.definitions().get(type_id)
    {
        fields
            .iter()
            .find(|f| f.name == field_name)
            .map(|f| f.name_span)
    } else {
        None
    }
}

pub(crate) fn resolve_struct_call_field(
    field_assignments: &[syntax::ast::StructFieldAssignment],
    name: &str,
    ty: &syntax::types::Type,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    let type_id = type_name(ty);

    field_assignments
        .iter()
        .find(|fa| offset_in_span(offset, &fa.name_span))
        .and_then(|fa| {
            type_id
                .as_deref()
                .and_then(|tid| find_struct_field_span(tid, &fa.name, snapshot))
        })
        .or_else(|| {
            lookup_definition_span(name, file, snapshot).or_else(|| {
                type_id
                    .as_deref()
                    .and_then(|tid| snapshot.definitions().get(tid).and_then(|d| d.name_span()))
            })
        })
}

pub(crate) fn resolve_dot_access_definition(
    expression: &Expression,
    member: &str,
    dot_access_span: Span,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    let try_lookup = |name: &str| -> Option<syntax::ast::Span> {
        snapshot
            .definitions()
            .get(name)
            .and_then(|d| d.name_span())
            .or_else(|| {
                let qualified = format!("{}.{}", file.module_id, name);
                snapshot
                    .definitions()
                    .get(qualified.as_str())
                    .and_then(|d| d.name_span())
            })
            .or_else(|| {
                file.imports().into_iter().find_map(|import| {
                    if import
                        .effective_alias(&snapshot.result.go_package_names)
                        .is_none()
                    {
                        let qualified = format!("{}.{}", import.name, name);
                        snapshot
                            .definitions()
                            .get(qualified.as_str())
                            .filter(|d| d.visibility().is_public())
                            .and_then(|d| d.name_span())
                    } else {
                        None
                    }
                })
            })
    };

    let resolve_by_type = || {
        type_name(&expression.get_type()).and_then(|type_id| {
            let name = format!("{}.{}", type_id, member);
            try_lookup(&name).or_else(|| find_struct_field_span(&type_id, member, snapshot))
        })
    };

    let result = if matches!(expression, Expression::DotAccess { .. })
        && let Some(dotted_path) = expression.as_dotted_path()
        && let Some(root_identifier) = expression.root_identifier()
    {
        let root_expression = get_root_expression(expression);

        if matches!(
            root_expression,
            Expression::Identifier {
                binding_id: Some(_),
                ..
            }
        ) {
            resolve_by_type()
        } else if let Some(module_name) =
            find_module_by_alias(file, root_identifier, &snapshot.result.go_package_names)
        {
            let qualified = dotted_path
                .strip_prefix(root_identifier)
                .map(|rest| format!("{}{}", module_name, rest))
                .unwrap_or(dotted_path);
            snapshot
                .definitions()
                .get(qualified.as_str())
                .and_then(|d| d.name_span())
        } else {
            try_lookup(&dotted_path)
        }
    } else if let Expression::Identifier {
        value,
        binding_id: None,
        ..
    } = expression.unwrap_parens()
    {
        if let Some(module_name) =
            find_module_by_alias(file, value.as_str(), &snapshot.result.go_package_names)
        {
            let qualified = format!("{}.{}", module_name, member);
            snapshot
                .definitions()
                .get(qualified.as_str())
                .and_then(|d| d.name_span())
        } else {
            try_lookup(&format!("{}.{}", value, member))
        }
    } else {
        None
    };

    result.or_else(resolve_by_type).or_else(|| {
        snapshot
            .facts()
            .usages
            .iter()
            .find(|usage| usage.usage_span == dot_access_span)
            .map(|usage| usage.definition_span)
    })
}

/// True when the span points into a generated `go:` typedef file. Used by rename
/// to refuse edits to typedefs, which would diverge from the regenerated content.
pub(crate) fn is_go_typedef_span(snapshot: &AnalysisSnapshot, span: &syntax::ast::Span) -> bool {
    snapshot
        .files()
        .get(&span.file_id)
        .is_some_and(|f| f.module_id.starts_with("go:"))
}

/// Resolve an import alias to the import statement's span.
pub(crate) fn resolve_import_span(
    name: &str,
    file: &syntax::program::File,
    go_package_names: &FxHashMap<String, String>,
) -> Option<syntax::ast::Span> {
    file.imports().into_iter().find_map(|import| {
        if import.effective_alias(go_package_names).as_deref() == Some(name) {
            Some(import.span)
        } else {
            None
        }
    })
}

/// Goto-def target for a cursor inside a type annotation tree.
pub(crate) fn resolve_annotation_definition(
    annotation: &Annotation,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    if !offset_in_span(offset, &annotation.get_span()) {
        return None;
    }

    let recurse = |child| resolve_annotation_definition(child, offset, file, snapshot);

    match annotation {
        Annotation::Constructor { name, span, params } => params
            .iter()
            .find_map(recurse)
            .or_else(|| resolve_constructor_name(name, *span, offset, file, snapshot)),
        Annotation::Function {
            params,
            return_type,
            ..
        } => params
            .iter()
            .find_map(recurse)
            .or_else(|| recurse(return_type.as_ref())),
        Annotation::Tuple { elements, .. } => elements.iter().find_map(recurse),
        Annotation::Unknown | Annotation::Opaque { .. } => None,
    }
}

/// Resolve a `Constructor` name's goto target. Routes the simple side through
/// the qualifier's module so a same-named local can't shadow it.
fn resolve_constructor_name(
    name: &str,
    span: Span,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    let cursor_in_name = (offset - span.byte_offset) as usize;
    let dot_pos = name.find('.').unwrap_or(name.len());

    if cursor_in_name <= dot_pos {
        let first = &name[..dot_pos];
        return resolve_import_span(first, file, &snapshot.result.go_package_names)
            .or_else(|| lookup_definition_span(first, file, snapshot));
    }

    let (qualifier, simple) = name.split_once('.')?;
    let module_name = find_module_by_alias(file, qualifier, &snapshot.result.go_package_names)?;

    let qualified = format!("{}.{}", module_name, simple);

    snapshot
        .definitions()
        .get(qualified.as_str())
        .and_then(|d| d.name_span())
}

pub(crate) fn lookup_definition_span(
    name: &str,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    if let Some(definition) = snapshot.definitions().get(name)
        && let Some(span) = definition.name_span()
    {
        return Some(span);
    }

    let qualified = format!("{}.{}", file.module_id, name);
    if let Some(definition) = snapshot.definitions().get(qualified.as_str())
        && let Some(span) = definition.name_span()
    {
        return Some(span);
    }

    for import in file.imports() {
        if import.name.starts_with("go:") {
            continue;
        }
        let imported = format!("{}.{}", import.name, name);
        if let Some(definition) = snapshot.definitions().get(imported.as_str())
            && let Some(span) = definition.name_span()
        {
            return Some(span);
        }
    }

    None
}

/// Extract the PascalCase word at the given byte offset, returning its text and byte range.
pub(crate) fn word_at_offset(source: &str, offset: u32) -> Option<(&str, usize, usize)> {
    let offset = offset as usize;
    if offset >= source.len() {
        return None;
    }

    let bytes = source.as_bytes();

    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }

    if start == end {
        return None;
    }

    let word = &source[start..end];

    if !word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return None;
    }

    Some((word, start, end))
}

pub(crate) fn resolve_word_at_offset(
    source: &str,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    let (word, _, _) = word_at_offset(source, offset)?;
    lookup_definition_span(word, file, snapshot)
}

/// Resolve an enum variant in a match arm pattern to its definition.
pub(crate) fn resolve_match_pattern_definition(
    arms: &[MatchArm],
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    arms.iter().find_map(|arm| {
        resolve_enum_in_pattern(
            &arm.pattern,
            arm.typed_pattern.as_ref(),
            offset,
            file,
            snapshot,
        )
    })
}

/// Resolve an enum variant in a single pattern (used by match, if-let, while-let).
pub(crate) fn resolve_enum_in_pattern(
    pattern: &Pattern,
    typed_pattern: Option<&TypedPattern>,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::ast::Span> {
    if !offset_in_span(offset, &pattern.get_span()) {
        return None;
    }

    match pattern {
        Pattern::EnumVariant {
            identifier, fields, ..
        } => {
            let typed_fields = match typed_pattern {
                Some(TypedPattern::EnumVariant { fields: tf, .. }) => Some(tf.as_slice()),
                _ => None,
            };
            let mut offset_in_field = false;
            for (i, field) in fields.iter().enumerate() {
                if offset_in_span(offset, &field.get_span()) {
                    offset_in_field = true;
                    let child_typed = typed_fields.and_then(|tf| tf.get(i));
                    if let Some(result) =
                        resolve_enum_in_pattern(field, child_typed, offset, file, snapshot)
                    {
                        return Some(result);
                    }
                }
            }
            if offset_in_field {
                return None;
            }

            match typed_pattern {
                Some(
                    TypedPattern::EnumVariant {
                        enum_name,
                        variant_name,
                        ..
                    }
                    | TypedPattern::EnumStructVariant {
                        enum_name,
                        variant_name,
                        ..
                    },
                ) => {
                    let variant_last = unqualified_name(variant_name);
                    let qualified = format!("{}.{}", enum_name, variant_last);
                    snapshot
                        .definitions()
                        .get(qualified.as_str())
                        .and_then(|d| d.name_span())
                }
                Some(TypedPattern::Const { qualified_name, .. }) => snapshot
                    .definitions()
                    .get(qualified_name.as_str())
                    .and_then(|d| d.name_span()),
                _ => lookup_definition_span(identifier, file, snapshot),
            }
        }

        Pattern::Or { patterns, .. } => {
            let alternatives = match typed_pattern {
                Some(TypedPattern::Or { alternatives, .. }) => Some(alternatives.as_slice()),
                _ => None,
            };
            patterns.iter().enumerate().find_map(|(i, pat)| {
                let child_typed = alternatives.and_then(|a| a.get(i));
                resolve_enum_in_pattern(pat, child_typed, offset, file, snapshot)
            })
        }

        Pattern::Struct {
            identifier,
            fields,
            span,
            ..
        } => {
            if let Some(field) = fields
                .iter()
                .find(|f| offset_in_span(offset, &f.value.get_span()))
                && let Some(TypedPattern::Struct { struct_fields, .. }) = typed_pattern
                && let Some(sf) = struct_fields.iter().find(|sf| sf.name == field.name)
            {
                return Some(sf.name_span);
            }
            if let Some(TypedPattern::EnumStructVariant {
                enum_name,
                variant_name,
                variant_fields,
                pattern_fields,
                ..
            }) = typed_pattern
            {
                if let Some(field) = fields
                    .iter()
                    .find(|f| offset_in_span(offset, &f.value.get_span()))
                {
                    let child_typed = pattern_fields
                        .iter()
                        .find(|(name, _)| name == &field.name)
                        .map(|(_, t)| t);
                    if let Some(result) =
                        resolve_enum_in_pattern(&field.value, child_typed, offset, file, snapshot)
                    {
                        return Some(result);
                    }
                    if is_shorthand_field(field, *span, snapshot)
                        && let Some(vf) = variant_fields.iter().find(|vf| vf.name == field.name)
                    {
                        return Some(vf.name_span);
                    }
                    return None;
                }
                if !offset_in_variant_token_span(*span, offset, snapshot) {
                    return None;
                }
                let variant_last = unqualified_name(variant_name);
                let qualified = format!("{}.{}", enum_name, variant_last);
                return snapshot
                    .definitions()
                    .get(qualified.as_str())
                    .and_then(|d| d.name_span());
            }
            lookup_definition_span(identifier, file, snapshot)
        }

        Pattern::Tuple { elements, .. } => {
            let typed_elements = match typed_pattern {
                Some(TypedPattern::Tuple { elements: te, .. }) => Some(te.as_slice()),
                _ => None,
            };
            elements.iter().enumerate().find_map(|(i, pat)| {
                let child_typed = typed_elements.and_then(|te| te.get(i));
                resolve_enum_in_pattern(pat, child_typed, offset, file, snapshot)
            })
        }

        Pattern::AsBinding { pattern, .. } => {
            resolve_enum_in_pattern(pattern, typed_pattern, offset, file, snapshot)
        }

        _ => None,
    }
}

/// Resolve the definition span at the given cursor offset.
///
/// Checks binding definitions first, then falls back to expression-based resolution.
/// `extra_match` handles caller-specific arms (e.g. DotAccess for references, rename guards).
pub(crate) fn resolve_definition_span(
    snapshot: &AnalysisSnapshot,
    file: &syntax::program::File,
    file_id: u32,
    offset: u32,
    extra_match: impl FnOnce(&Expression) -> Option<syntax::ast::Span>,
) -> Option<syntax::ast::Span> {
    snapshot
        .facts()
        .bindings
        .values()
        .find_map(|binding| {
            if binding.span.file_id == file_id && offset_in_span(offset, &binding.span) {
                Some(binding.span)
            } else {
                None
            }
        })
        .or_else(|| {
            let expression = find_expression_at(&file.items, offset)?;
            match expression {
                Expression::Identifier {
                    binding_id: Some(id),
                    ..
                } => snapshot.facts().bindings.get(id).map(|b| b.span),

                Expression::Function { name_span, .. }
                | Expression::Interface { name_span, .. }
                | Expression::TypeAlias { name_span, .. } => Some(*name_span),

                Expression::Struct {
                    name,
                    name_span,
                    fields,
                    ..
                } => fields
                    .iter()
                    .find(|f| offset_in_span(offset, &f.name_span))
                    .and_then(|f| {
                        let qualified = format!("{}.{}", file.module_id, name);
                        find_struct_field_span(&qualified, &f.name, snapshot)
                    })
                    .or(Some(*name_span)),

                Expression::Enum {
                    name,
                    name_span,
                    variants,
                    ..
                } => variants
                    .iter()
                    .find(|v| offset_in_span(offset, &v.name_span))
                    .and_then(|v| {
                        let qualified = format!("{}.{}.{}", file.module_id, name, v.name);
                        snapshot
                            .definitions()
                            .get(qualified.as_str())
                            .and_then(|d| d.name_span())
                    })
                    .or(Some(*name_span)),

                Expression::Const {
                    identifier_span, ..
                } => Some(*identifier_span),

                Expression::VariableDeclaration { name_span, .. } => Some(*name_span),

                Expression::StructCall {
                    name,
                    field_assignments,
                    ty,
                    ..
                } => resolve_struct_call_field(field_assignments, name, ty, offset, file, snapshot),

                other => extra_match(other),
            }
        })
}

/// True iff `offset` lies on the variant name token of an enum-struct-variant
/// pattern head. Excludes the qualifier, dots, and surrounding whitespace.
fn offset_in_variant_token_span(span: Span, offset: u32, snapshot: &AnalysisSnapshot) -> bool {
    let Some(source_file) = snapshot.files().get(&span.file_id) else {
        return false;
    };
    let start = span.byte_offset as usize;
    if start > source_file.source.len() {
        return false;
    }
    let end = (start + span.byte_length as usize).min(source_file.source.len());
    let Some((token_offset, token_len)) =
        crate::member_token_range(&source_file.source[start..end])
    else {
        return false;
    };
    let token_span = Span::new(span.file_id, span.byte_offset + token_offset, token_len);
    offset_in_span(offset, &token_span)
}

/// True iff `field` is written as shorthand (`{ x }`) rather than explicit
/// (`{ x: ... }`). Detected by scanning source preceding the value span: a `:`
/// before any structural delimiter (`,` or `{`) means explicit.
fn is_shorthand_field(
    field: &StructFieldPattern,
    pattern_span: Span,
    snapshot: &AnalysisSnapshot,
) -> bool {
    let Some(source_file) = snapshot.files().get(&pattern_span.file_id) else {
        return false;
    };
    let pattern_start = pattern_span.byte_offset as usize;
    let value_start = field.value.get_span().byte_offset as usize;
    if value_start <= pattern_start || value_start > source_file.source.len() {
        return false;
    }
    for ch in source_file.source[pattern_start..value_start].chars().rev() {
        match ch {
            ':' => return false,
            ',' | '{' => return true,
            _ => {}
        }
    }
    false
}
