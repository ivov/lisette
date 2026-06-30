use syntax::ast::{Annotation, Expression, Span};
use syntax::program::{Definition, DefinitionBody};

use crate::analysis::find_module_by_alias;
use crate::definition::{
    get_root_expression, resolve_dot_access_definition, resolve_enum_in_pattern,
    resolve_match_pattern_definition,
};
use crate::offset_in_span;
use crate::patterns::get_pattern_element_type;
use crate::snapshot::AnalysisSnapshot;
use crate::traversal::find_expression_at;
use crate::type_name;

/// Hover for top-level declarations and the annotation trees inside them.
pub(crate) fn resolve_declaration_hover(
    expression: &Expression,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<(syntax::types::Type, Span)> {
    let name_hover = |name: &str, name_span: Span| -> Option<(syntax::types::Type, Span)> {
        if !offset_in_span(offset, &name_span) {
            return None;
        }
        let qualified = format!("{}.{}", file.module_id, name);
        let definition = snapshot.definitions().get(qualified.as_str())?;
        Some((definition.ty().clone(), name_span))
    };

    match expression {
        Expression::TypeAlias {
            name,
            name_span,
            annotation,
            ..
        } => resolve_annotation_hover(annotation, offset, file, snapshot)
            .or_else(|| name_hover(name, *name_span)),
        Expression::Function {
            name,
            name_span,
            params,
            return_annotation,
            ..
        } => params
            .iter()
            .filter_map(|p| p.annotation.as_ref())
            .find_map(|a| resolve_annotation_hover(a, offset, file, snapshot))
            .or_else(|| resolve_annotation_hover(return_annotation, offset, file, snapshot))
            .or_else(|| name_hover(name, *name_span)),
        Expression::Enum {
            name, name_span, ..
        }
        | Expression::Struct {
            name, name_span, ..
        }
        | Expression::Interface {
            name, name_span, ..
        } => name_hover(name, *name_span),
        _ => None,
    }
}

fn resolve_annotation_hover(
    annotation: &Annotation,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<(syntax::types::Type, Span)> {
    if !offset_in_span(offset, &annotation.get_span()) {
        return None;
    }
    let recurse = |child| resolve_annotation_hover(child, offset, file, snapshot);
    match annotation {
        Annotation::Constructor { name, params, span } => params
            .iter()
            .find_map(recurse)
            .or_else(|| resolve_constructor_name_hover(name, *span, offset, file, snapshot)),
        Annotation::Function {
            params,
            return_type,
            ..
        } => params
            .iter()
            .find_map(recurse)
            .or_else(|| recurse(return_type.as_ref())),
        Annotation::Tuple { elements, .. } => elements.iter().find_map(recurse),
        Annotation::Unknown | Annotation::Opaque { .. } | Annotation::Constant { .. } => None,
    }
}

fn resolve_constructor_name_hover(
    name: &str,
    span: Span,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<(syntax::types::Type, Span)> {
    let cursor_in_name = (offset - span.byte_offset) as usize;
    let dot_pos = name.find('.').unwrap_or(name.len());

    if cursor_in_name > dot_pos {
        let (qualifier, simple) = name.split_once('.')?;
        let module_name = find_module_by_alias(file, qualifier, &snapshot.result.go_package_names)?;
        let qualified = format!("{}.{}", module_name, simple);
        let definition = snapshot.definitions().get(qualified.as_str())?;
        let simple_offset = span.byte_offset + dot_pos as u32 + 1;
        let simple_span = Span::new(span.file_id, simple_offset, simple.len() as u32);
        return Some((definition.ty().clone(), simple_span));
    }

    let first = &name[..dot_pos];
    let first_span = Span::new(span.file_id, span.byte_offset, dot_pos as u32);
    let ty = lookup_type_by_name(first, file, snapshot)?;
    Some((ty, first_span))
}

fn lookup_type_by_name(
    name: &str,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<syntax::types::Type> {
    let candidates = [
        format!("{}.{}", file.module_id, name),
        name.to_string(),
        format!("prelude.{}", name),
    ];
    for qualified in &candidates {
        if let Some(def) = snapshot.definitions().get(qualified.as_str()) {
            return Some(def.ty().clone());
        }
    }
    for import in file.imports() {
        if import.name.starts_with("go:") {
            continue;
        }
        let qualified = format!("{}.{}", import.name, name);
        if let Some(def) = snapshot.definitions().get(qualified.as_str()) {
            return Some(def.ty().clone());
        }
    }
    None
}

/// Extract the type and span for hover display at the given offset within an expression.
pub(crate) fn get_hover_type_and_span(
    expression: &Expression,
    offset: u32,
) -> (syntax::types::Type, Span) {
    fn get_binding_type(
        binding: &syntax::ast::Binding,
        offset: u32,
    ) -> Option<(syntax::types::Type, Span)> {
        get_pattern_element_type(
            &binding.pattern,
            binding.typed_pattern.as_ref(),
            &binding.ty,
            offset,
        )
    }

    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            if let Some(result) = get_binding_type(binding, offset) {
                return result;
            }
        }

        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                if let Some(result) = get_binding_type(param, offset) {
                    return result;
                }
            }
        }

        Expression::Match { subject, arms, .. } => {
            for arm in arms {
                if let Some(result) = get_pattern_element_type(
                    &arm.pattern,
                    arm.typed_pattern.as_ref(),
                    &subject.get_type(),
                    offset,
                ) {
                    return result;
                }
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            typed_pattern,
            ..
        } => {
            if offset_in_span(offset, &pattern.get_span()) {
                if let Some(result) = get_pattern_element_type(
                    pattern,
                    typed_pattern.as_ref(),
                    &scrutinee.get_type(),
                    offset,
                ) {
                    return result;
                }
                let ty = pattern.get_type().unwrap_or_else(|| scrutinee.get_type());
                return (ty, pattern.get_span());
            }
        }

        Expression::WhileLet {
            pattern,
            scrutinee,
            typed_pattern,
            ..
        } => {
            if offset_in_span(offset, &pattern.get_span()) {
                if let Some(result) = get_pattern_element_type(
                    pattern,
                    typed_pattern.as_ref(),
                    &scrutinee.get_type(),
                    offset,
                ) {
                    return result;
                }
                let ty = pattern.get_type().unwrap_or_else(|| scrutinee.get_type());
                return (ty, pattern.get_span());
            }
        }

        Expression::StructCall {
            field_assignments, ..
        } => {
            if let Some(fa) = field_assignments
                .iter()
                .find(|fa| offset_in_span(offset, &fa.name_span))
            {
                return (fa.value.get_type(), fa.name_span);
            }
        }

        Expression::Struct { fields, .. } => {
            if let Some(field) = fields.iter().find(|f| offset_in_span(offset, &f.name_span)) {
                return (field.ty.clone(), field.name_span);
            }
        }

        _ => {}
    }

    (expression.get_type(), expression.get_span())
}

/// Extract the doc comment from an AST expression at a given offset.
///
/// For expressions with sub-items (enum variants, struct fields), checks whether
/// the offset lands on a sub-item and returns that sub-item's doc instead.
fn extract_doc_from_expression(expression: &Expression, offset: u32) -> Option<String> {
    match expression {
        Expression::Const { doc, .. } | Expression::VariableDeclaration { doc, .. } => doc.clone(),

        Expression::Function { doc, name_span, .. }
        | Expression::TypeAlias { doc, name_span, .. }
        | Expression::Interface { doc, name_span, .. } => offset_in_span(offset, name_span)
            .then(|| doc.clone())
            .flatten(),

        Expression::Enum { doc, variants, .. } => variants
            .iter()
            .find(|v| offset_in_span(offset, &v.name_span))
            .and_then(|v| v.doc.clone())
            .or_else(|| doc.clone()),

        Expression::Struct { doc, fields, .. } => fields
            .iter()
            .find(|f| offset_in_span(offset, &f.name_span))
            .and_then(|f| f.doc.clone())
            .or_else(|| doc.clone()),

        _ => None,
    }
}

/// Recover the doc comment from the AST expression at a definition's span.
fn find_doc_at_definition_span(
    definition_span: Span,
    snapshot: &AnalysisSnapshot,
) -> Option<String> {
    let file = snapshot.files().get(&definition_span.file_id)?;
    let expression = find_expression_at(&file.items, definition_span.byte_offset)?;
    extract_doc_from_expression(expression, definition_span.byte_offset)
}

/// Resolve doc for a dot access by looking up the Definition directly.
/// Handles Go stdlib imports (where `resolve_dot_access_definition` returns None)
/// and any other case where the AST-based approach fails.
fn resolve_dot_access_doc(
    expression: &Expression,
    member: &str,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<String> {
    if let Some(type_id) = type_name(&expression.get_type()) {
        let qualified = format!("{}.{}", type_id, member);
        if let Some(def) = snapshot.definitions().get(qualified.as_str())
            && let Some(doc) = def.doc()
        {
            return Some(doc.clone());
        }
    }

    let root = get_root_expression(expression);
    let alias = match root.unwrap_parens() {
        Expression::Identifier {
            value,
            binding_id: None,
            ..
        } => value.as_str(),
        _ => return None,
    };

    let module_name = find_module_by_alias(file, alias, &snapshot.result.go_package_names)?;

    let qualified = if matches!(expression, Expression::DotAccess { .. }) {
        if let Some(dotted) = expression.as_dotted_path()
            && let Some(root_id) = expression.root_identifier()
        {
            dotted
                .strip_prefix(root_id)
                .map(|rest| format!("{}{}.{}", module_name, rest, member))
                .unwrap_or_else(|| format!("{}.{}", module_name, member))
        } else {
            format!("{}.{}", module_name, member)
        }
    } else {
        format!("{}.{}", module_name, member)
    };

    snapshot
        .definitions()
        .get(qualified.as_str())?
        .doc()
        .cloned()
}

/// Resolve the doc comment for the hovered expression.
pub(crate) fn get_hover_doc(
    expression: &Expression,
    offset: u32,
    file: &syntax::program::File,
    snapshot: &AnalysisSnapshot,
) -> Option<String> {
    if let Some(doc) = extract_doc_from_expression(expression, offset) {
        return Some(doc);
    }

    match expression {
        Expression::Identifier {
            qualified: Some(qname),
            ..
        } => {
            let definition = snapshot.definitions().get(qname.as_str())?;
            definition
                .name_span()
                .and_then(|span| find_doc_at_definition_span(span, snapshot))
                .or_else(|| definition.doc().cloned())
        }

        Expression::DotAccess {
            expression: base,
            member,
            span,
            ..
        } => resolve_dot_access_definition(base, member, *span, file, snapshot)
            .and_then(|span| find_doc_at_definition_span(span, snapshot))
            .or_else(|| resolve_dot_access_doc(base, member, file, snapshot)),

        Expression::StructCall {
            field_assignments,
            ty,
            ..
        } => {
            let type_id = type_name(ty)?;

            if let Some(fa) = field_assignments
                .iter()
                .find(|fa| offset_in_span(offset, &fa.name_span))
            {
                if let Some(Definition {
                    body: DefinitionBody::Struct { fields, .. },
                    ..
                }) = snapshot.definitions().get(type_id.as_str())
                {
                    return fields
                        .iter()
                        .find(|f| f.name == fa.name)
                        .and_then(|f| f.doc.clone());
                }
                return None;
            }

            let span = snapshot.definitions().get(type_id.as_str())?.name_span()?;
            find_doc_at_definition_span(span, snapshot)
        }

        Expression::Match { arms, .. } => {
            let span = resolve_match_pattern_definition(arms, offset, file, snapshot)?;
            find_doc_at_definition_span(span, snapshot)
        }

        Expression::IfLet {
            pattern,
            typed_pattern,
            ..
        }
        | Expression::WhileLet {
            pattern,
            typed_pattern,
            ..
        } => {
            let span =
                resolve_enum_in_pattern(pattern, typed_pattern.as_ref(), offset, file, snapshot)?;
            find_doc_at_definition_span(span, snapshot)
        }

        _ => None,
    }
}
