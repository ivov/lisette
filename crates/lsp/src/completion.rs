use tower_lsp::lsp_types::*;

use syntax::ast::Expression;

use crate::definition::get_root_expression;
use crate::snapshot::AnalysisSnapshot;
use crate::traversal::{find_enclosing_impl_type, find_expression_at};
use crate::type_name;

pub(crate) fn get_module_prefix(source: &str, offset: usize) -> Option<&str> {
    let before = &source[..offset];
    if !before.ends_with('.') {
        return None;
    }
    let before_dot = &before[..before.len() - 1];

    let base = if before_dot.ends_with(']') {
        let bracket_start = before_dot.rfind('[')?;
        &before_dot[..bracket_start]
    } else {
        before_dot
    };

    let start = base
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    let identifier = base[start..].trim();
    if identifier.is_empty() || !identifier.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return None;
    }
    Some(identifier)
}

pub(crate) fn definition_to_completion_kind(
    definition: &syntax::program::Definition,
) -> CompletionItemKind {
    use syntax::program::DefinitionBody;
    match &definition.body {
        DefinitionBody::Struct { .. } => CompletionItemKind::STRUCT,
        DefinitionBody::Enum { .. } | DefinitionBody::ValueEnum { .. } => CompletionItemKind::ENUM,
        DefinitionBody::Interface { .. } => CompletionItemKind::INTERFACE,
        DefinitionBody::TypeAlias { .. } => CompletionItemKind::TYPE_PARAMETER,
        DefinitionBody::Value { .. } => {
            if matches!(
                &definition.ty,
                syntax::types::Type::Function { .. } | syntax::types::Type::Forall { .. }
            ) {
                CompletionItemKind::FUNCTION
            } else {
                CompletionItemKind::CONSTANT
            }
        }
    }
}

/// Extract the element type from a collection type (Slice<T>, EnumeratedSlice<T>, Map<K, V>).
fn element_type_name(ty: &syntax::types::Type) -> Option<String> {
    use syntax::types::CompoundKind;
    match ty.as_compound()? {
        (CompoundKind::Slice | CompoundKind::EnumeratedSlice, args) => {
            args.first().and_then(type_name)
        }
        (CompoundKind::Map, args) => args.get(1).and_then(type_name),
        _ => None,
    }
}

/// Resolve a variable name to its type's qualified name by scanning usages.
/// When `indexed` is true, extracts the element type for collection types.
pub(crate) fn resolve_variable_type(
    var_name: &str,
    file: &syntax::program::File,
    offset: u32,
    snapshot: &AnalysisSnapshot,
    indexed: bool,
) -> Option<String> {
    let binding =
        snapshot.facts().bindings.values().find(|b| {
            b.name == var_name && b.span.file_id == file.id && b.span.byte_offset < offset
        })?;

    let expression = find_expression_at(&file.items, binding.span.byte_offset)?;
    let borrowed_ty = match expression {
        Expression::Let {
            binding: let_binding,
            ..
        } => {
            let matches_name = match &let_binding.pattern {
                syntax::ast::Pattern::Identifier { identifier, .. } => identifier == var_name,
                syntax::ast::Pattern::AsBinding { name, .. } => name == var_name,
                _ => false,
            };
            if matches_name {
                Some(&let_binding.ty)
            } else {
                None
            }
        }
        Expression::Identifier { ty, .. } => Some(ty),
        Expression::For {
            binding: for_binding,
            ..
        } => Some(&for_binding.ty),
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            let param = params.iter().find(|p| match &p.pattern {
                syntax::ast::Pattern::Identifier { identifier, .. } => identifier == var_name,
                syntax::ast::Pattern::AsBinding { name, .. } => name == var_name,
                _ => false,
            })?;
            Some(&param.ty)
        }
        _ => None,
    };

    let owned_ty;
    let ty = if let Some(t) = borrowed_ty {
        t
    } else {
        let (t, _) = crate::hover::get_hover_type_and_span(expression, binding.span.byte_offset);
        owned_ty = t;
        &owned_ty
    };

    let (resolved, _) = syntax::types::Type::remove_vars(&[ty]);
    let ty = &resolved[0];

    if indexed {
        element_type_name(ty)
    } else {
        type_name(ty)
    }
}

pub(crate) enum DotContext {
    Instance(String),
    TypeLevel(String),
}

pub(crate) fn detect_dot_context(
    file: &syntax::program::File,
    offset: u32,
    snapshot: &AnalysisSnapshot,
) -> Option<DotContext> {
    let Expression::DotAccess {
        expression, member, ..
    } = find_expression_at(&file.items, offset.saturating_sub(1))?
    else {
        return None;
    };
    if !member.is_empty() {
        if !matches!(
            get_root_expression(expression),
            Expression::Identifier {
                binding_id: None,
                ..
            }
        ) {
            let ty = expression.get_type();
            return type_name(&ty).map(DotContext::Instance);
        }
        return None;
    }

    if let Expression::Identifier { value, .. } = expression.as_ref() {
        for prefix in [file.module_id.as_str(), "prelude"] {
            let qualified = format!("{prefix}.{value}");
            if let Some(definition) = snapshot.definitions().get(qualified.as_str())
                && definition.is_type_definition()
            {
                return Some(DotContext::TypeLevel(qualified));
            }
        }
    }

    let ty = expression.get_type();
    if let Some(type_id) = type_name(&ty) {
        return Some(DotContext::Instance(type_id));
    }

    if let Expression::Identifier { value, .. } = expression.as_ref()
        && value == "self"
        && let Some(impl_type) = find_enclosing_impl_type(&file.items, offset)
    {
        let qualified = format!("{}.{}", file.module_id, impl_type);
        return Some(DotContext::Instance(qualified));
    }

    None
}

pub(crate) fn get_instance_completions(
    type_id: &str,
    snapshot: &AnalysisSnapshot,
    same_module: bool,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    if let Some(syntax::program::Definition {
        body: syntax::program::DefinitionBody::Struct { fields, .. },
        ..
    }) = snapshot.definitions().get(type_id)
    {
        for field in fields {
            if same_module || field.visibility.is_public() {
                items.push(CompletionItem {
                    label: field.name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(field.ty.to_string()),
                    ..Default::default()
                });
            }
        }
    }

    let method_prefix = format!("{type_id}.");
    for (qname, definition) in snapshot.definitions().iter() {
        if let Some(method_name) = qname.strip_prefix(method_prefix.as_str())
            && !method_name.contains('.')
            && matches!(
                definition.body,
                syntax::program::DefinitionBody::Value { .. }
            )
            && is_instance_method(definition.ty(), type_id)
            && (same_module || definition.visibility().is_public())
        {
            items.push(CompletionItem {
                label: method_name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(definition.ty().to_string()),
                ..Default::default()
            });
        }
    }

    items
}

pub(crate) fn get_type_completions(
    type_id: &str,
    snapshot: &AnalysisSnapshot,
    same_module: bool,
) -> Vec<CompletionItem> {
    use syntax::program::DefinitionBody;

    let mut items = Vec::new();

    match snapshot.definitions().get(type_id).map(|d| &d.body) {
        Some(DefinitionBody::Enum { variants, .. }) => {
            for variant in variants {
                items.push(CompletionItem {
                    label: variant.name.to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    ..Default::default()
                });
            }
        }
        Some(DefinitionBody::ValueEnum { variants, .. }) => {
            for variant in variants {
                items.push(CompletionItem {
                    label: variant.name.to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    ..Default::default()
                });
            }
        }
        _ => {}
    }

    let method_prefix = format!("{type_id}.");
    for (qname, definition) in snapshot.definitions().iter() {
        if let Some(method_name) = qname.strip_prefix(method_prefix.as_str())
            && !method_name.contains('.')
            && matches!(definition.body, DefinitionBody::Value { .. })
            && !is_instance_method(definition.ty(), type_id)
            && (same_module || definition.visibility().is_public())
            && !items.iter().any(|i| i.label == method_name)
        {
            items.push(CompletionItem {
                label: method_name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(definition.ty().to_string()),
                ..Default::default()
            });
        }
    }

    items
}

fn is_instance_method(ty: &syntax::types::Type, type_id: &str) -> bool {
    let func_ty = match ty {
        syntax::types::Type::Forall { body, .. } => body,
        other => other,
    };
    match func_ty {
        syntax::types::Type::Function { params, .. } if !params.is_empty() => {
            type_name(&params[0]).is_some_and(|name| name == type_id)
        }
        _ => false,
    }
}
