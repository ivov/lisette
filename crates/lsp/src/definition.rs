use rustc_hash::FxHashMap;
use syntax::ast::{
    ConstructorPatternResolution, Expression, IdentifierResolution, MatchArm, Pattern,
    RecordPatternResolution, Span, StructFieldPattern,
};
use syntax::program::DefinitionBody;
use syntax::program::File;
use syntax::types::unqualified_name;

use crate::analysis::find_package_by_alias;
use crate::offset_in_span;
use crate::snapshot::AnalysisSnapshot;
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
) -> Option<Span> {
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

pub(crate) fn resolve_dot_access_definition(
    expression: &Expression,
    member: &str,
    dot_access_span: Span,
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    let try_lookup = |name: &str| -> Option<Span> {
        snapshot
            .definitions()
            .get(name)
            .and_then(|d| d.name_span)
            .or_else(|| {
                let qualified = format!("{}.{}", file.package_id, name);
                snapshot
                    .definitions()
                    .get(qualified.as_str())
                    .and_then(|d| d.name_span)
            })
            .or_else(|| {
                file.imports().into_iter().find_map(|import| {
                    if import
                        .effective_alias(&snapshot.analysis.emit_input.go_package_names)
                        .is_none()
                    {
                        let qualified = format!("{}.{}", import.name, name);
                        snapshot
                            .definitions()
                            .get(qualified.as_str())
                            .filter(|d| d.visibility.is_public())
                            .and_then(|d| d.name_span)
                    } else {
                        None
                    }
                })
            })
            .or_else(|| {
                let in_prelude = format!("prelude.{name}");
                snapshot
                    .definitions()
                    .get(in_prelude.as_str())
                    .and_then(|d| d.name_span)
            })
    };

    let resolve_by_type = || {
        type_name(&expression.get_type(), snapshot).and_then(|type_id| {
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
                resolution: IdentifierResolution::Binding(_),
                ..
            }
        ) {
            resolve_by_type()
        } else if let Some(package_name) = find_package_by_alias(
            file,
            root_identifier,
            &snapshot.analysis.emit_input.go_package_names,
        ) {
            let qualified = dotted_path
                .strip_prefix(root_identifier)
                .map(|rest| format!("{}{}", package_name, rest))
                .unwrap_or(dotted_path);
            snapshot
                .definitions()
                .get(qualified.as_str())
                .and_then(|d| d.name_span)
        } else {
            try_lookup(&dotted_path)
        }
    } else if let Expression::Identifier {
        value, resolution, ..
    } = expression.unwrap_parens()
        && !matches!(resolution, IdentifierResolution::Binding(_))
    {
        if let Some(package_name) = find_package_by_alias(
            file,
            value.as_str(),
            &snapshot.analysis.emit_input.go_package_names,
        ) {
            let qualified = format!("{}.{}", package_name, member);
            snapshot
                .definitions()
                .get(qualified.as_str())
                .and_then(|d| d.name_span)
        } else {
            try_lookup(&format!("{}.{}", value, member))
        }
    } else {
        None
    };

    result.or_else(resolve_by_type).or_else(|| {
        snapshot
            .usages()
            .iter()
            .find(|usage| usage.usage_span == dot_access_span)
            .map(|usage| usage.definition_span)
    })
}

/// True when the span points into a generated typedef (`go:` or prelude), which rename must refuse.
pub(crate) fn is_generated_typedef_span(snapshot: &AnalysisSnapshot, span: &Span) -> bool {
    snapshot
        .files()
        .get(&span.file_id)
        .is_some_and(|f| f.package_id.starts_with("go:") || f.package_id == "prelude")
}

/// Resolve an import alias to the import statement's span.
pub(crate) fn resolve_import_span(
    name: &str,
    file: &File,
    go_package_names: &FxHashMap<String, String>,
) -> Option<Span> {
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
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    if let Some(definition) = snapshot.definitions().get(name)
        && let Some(span) = definition.name_span
    {
        return Some(span);
    }

    let qualified = format!("{}.{}", file.package_id, name);
    if let Some(definition) = snapshot.definitions().get(qualified.as_str())
        && let Some(span) = definition.name_span
    {
        return Some(span);
    }

    for import in file.imports() {
        if import.name.starts_with("go:") {
            continue;
        }
        let imported = format!("{}.{}", import.name, name);
        if let Some(definition) = snapshot.definitions().get(imported.as_str())
            && let Some(span) = definition.name_span
        {
            return Some(span);
        }
    }

    let in_prelude = format!("prelude.{name}");
    if let Some(definition) = snapshot.definitions().get(in_prelude.as_str())
        && let Some(span) = definition.name_span
    {
        return Some(span);
    }

    None
}

/// Resolve an enum variant in a match arm pattern to its definition.
pub(crate) fn resolve_match_pattern_definition(
    arms: &[MatchArm],
    offset: u32,
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    arms.iter()
        .find_map(|arm| resolve_enum_in_pattern(&arm.pattern, offset, file, snapshot))
}

/// Resolve an enum variant in a single pattern (used by match, if-let, while-let).
pub(crate) fn resolve_enum_in_pattern(
    pattern: &Pattern,
    offset: u32,
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Option<Span> {
    if !offset_in_span(offset, &pattern.get_span()) {
        return None;
    }

    match pattern {
        Pattern::EnumVariant {
            identifier,
            fields,
            resolution,
            ..
        } => {
            let mut offset_in_field = false;
            for field in fields {
                if offset_in_span(offset, &field.get_span()) {
                    offset_in_field = true;
                    if let Some(result) = resolve_enum_in_pattern(field, offset, file, snapshot) {
                        return Some(result);
                    }
                }
            }
            if offset_in_field {
                return None;
            }

            match resolution {
                ConstructorPatternResolution::EnumVariant {
                    enum_name,
                    variant_name,
                    ..
                } => {
                    let variant_last = unqualified_name(variant_name);
                    let qualified = format!("{}.{}", enum_name, variant_last);
                    snapshot
                        .definitions()
                        .get(qualified.as_str())
                        .and_then(|d| d.name_span)
                }
                ConstructorPatternResolution::Const { qualified_name }
                | ConstructorPatternResolution::ConstValue { qualified_name, .. } => snapshot
                    .definitions()
                    .get(qualified_name.as_str())
                    .and_then(|d| d.name_span),
                ConstructorPatternResolution::Unresolved => {
                    lookup_definition_span(identifier, file, snapshot)
                }
            }
        }

        Pattern::Or { patterns, .. }
        | Pattern::Tuple {
            elements: patterns, ..
        } => patterns
            .iter()
            .find_map(|pattern| resolve_enum_in_pattern(pattern, offset, file, snapshot)),

        Pattern::Struct {
            identifier,
            fields,
            span,
            resolution,
            ..
        } => {
            if let Some(field) = fields
                .iter()
                .find(|field| offset_in_span(offset, &field.value.get_span()))
            {
                if let Some(result) = resolve_enum_in_pattern(&field.value, offset, file, snapshot)
                {
                    return Some(result);
                }
                let definition_span = record_pattern_field_span(snapshot, resolution, &field.name);
                if is_shorthand_field(field, *span, snapshot) {
                    return definition_span;
                }
                return None;
            }

            if let RecordPatternResolution::EnumVariant {
                enum_name,
                variant_name,
                ..
            } = resolution
            {
                if !offset_in_variant_token_span(*span, offset, snapshot) {
                    return None;
                }
                let variant_last = unqualified_name(variant_name);
                let qualified = format!("{}.{}", enum_name, variant_last);
                return snapshot
                    .definitions()
                    .get(qualified.as_str())
                    .and_then(|d| d.name_span);
            }
            lookup_definition_span(identifier, file, snapshot)
        }

        Pattern::Slice { prefix, .. } => prefix
            .iter()
            .find_map(|pattern| resolve_enum_in_pattern(pattern, offset, file, snapshot)),

        Pattern::AsBinding { pattern, .. } => {
            resolve_enum_in_pattern(pattern, offset, file, snapshot)
        }

        _ => None,
    }
}

pub(crate) fn record_pattern_field_span(
    snapshot: &AnalysisSnapshot,
    resolution: &RecordPatternResolution,
    field_name: &str,
) -> Option<Span> {
    match resolution {
        RecordPatternResolution::Struct { struct_name } => {
            let DefinitionBody::Struct { fields, .. } =
                &snapshot.definitions().get(struct_name.as_str())?.body
            else {
                return None;
            };
            fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| field.name_span)
        }
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
        } => {
            let DefinitionBody::Enum { variants, .. } =
                &snapshot.definitions().get(enum_name.as_str())?.body
            else {
                return None;
            };
            variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))?
                .fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| field.name_span)
        }
        RecordPatternResolution::Unresolved => None,
    }
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
pub(crate) fn is_shorthand_field(
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
