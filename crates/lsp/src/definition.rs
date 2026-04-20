use rustc_hash::FxHashMap;
use syntax::ast::{Expression, MatchArm, Pattern, TypedPattern};

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
    use syntax::program::Definition;

    if let Some(Definition::Struct { fields, .. }) = snapshot.definitions().get(type_id) {
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
    let resolved = ty.resolve();
    let type_id = type_name(&resolved);

    field_assignments
        .iter()
        .find(|fa| offset_in_span(offset, &fa.name_span))
        .and_then(|fa| type_id.and_then(|tid| find_struct_field_span(tid, &fa.name, snapshot)))
        .or_else(|| {
            lookup_definition_span(name, file, snapshot).or_else(|| {
                type_id.and_then(|tid| snapshot.definitions().get(tid).and_then(|d| d.name_span()))
            })
        })
}

pub(crate) fn resolve_dot_access_definition(
    expression: &Expression,
    member: &str,
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
        type_name(&expression.get_type().resolve()).and_then(|type_id| {
            let name = format!("{}.{}", type_id, member);
            try_lookup(&name).or_else(|| find_struct_field_span(type_id, member, snapshot))
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
            if module_name.starts_with("go:") {
                return None;
            }
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
            if module_name.starts_with("go:") {
                return None;
            }
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

    result.or_else(resolve_by_type)
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
                    let variant_last = variant_name.rsplit('.').next().unwrap_or(variant_name);
                    let qualified = format!("{}.{}", enum_name, variant_last);
                    snapshot
                        .definitions()
                        .get(qualified.as_str())
                        .and_then(|d| d.name_span())
                }
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
            identifier, fields, ..
        } => {
            if let Some(field) = fields
                .iter()
                .find(|f| offset_in_span(offset, &f.value.get_span()))
                && let Some(TypedPattern::Struct { struct_fields, .. }) = typed_pattern
                && let Some(sf) = struct_fields.iter().find(|sf| sf.name == field.name)
            {
                return Some(sf.name_span);
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

                Expression::ValueEnum {
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
