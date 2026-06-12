use rustc_hash::FxHashSet;
use tower_lsp::lsp_types::*;

use syntax::ast::Expression;
use syntax::attributes::{AttributeInfo, AttributeTarget, attributes_for};
use syntax::lex::{Lexer, Token, TokenKind as Tk};
use syntax::program::DefinitionBody;
use syntax::types::Type;

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
        DefinitionBody::Enum { .. } => CompletionItemKind::ENUM,
        DefinitionBody::Interface { .. } => CompletionItemKind::INTERFACE,
        DefinitionBody::TypeAlias { .. } => CompletionItemKind::TYPE_PARAMETER,
        DefinitionBody::Value { .. } => {
            if matches!(
                &definition.ty,
                syntax::types::Type::Function(_) | syntax::types::Type::Forall { .. }
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
        body: syntax::program::DefinitionBody::Interface { definition },
        ..
    }) = snapshot.definitions().get(type_id)
    {
        let mut visited = FxHashSet::default();
        let mut seen = FxHashSet::default();
        collect_interface_methods(
            type_id,
            definition,
            snapshot,
            &mut visited,
            &mut seen,
            &mut items,
        );
        return items;
    }

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

fn collect_interface_methods(
    interface_id: &str,
    interface: &syntax::program::Interface,
    snapshot: &AnalysisSnapshot,
    visited: &mut FxHashSet<String>,
    seen: &mut FxHashSet<String>,
    items: &mut Vec<CompletionItem>,
) {
    if !visited.insert(interface_id.to_string()) {
        return;
    }
    for (name, method_ty) in &interface.methods {
        if seen.insert(name.to_string()) {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(method_ty.to_string()),
                ..Default::default()
            });
        }
    }
    for parent in &interface.parents {
        if let Some(parent_id) = parent.get_qualified_id()
            && let Some(syntax::program::Definition {
                body: syntax::program::DefinitionBody::Interface { definition },
                ..
            }) = snapshot.definitions().get(parent_id)
        {
            collect_interface_methods(parent_id, definition, snapshot, visited, seen, items);
        }
    }
}

pub(crate) fn get_type_completions(
    type_id: &str,
    snapshot: &AnalysisSnapshot,
    current_module: &str,
) -> Vec<CompletionItem> {
    let target = alias_target(type_id, snapshot);
    let method_id = target.as_deref().unwrap_or(type_id);

    let mut items = enum_variant_items(method_id, snapshot).unwrap_or_default();

    let same_module = id_is_in_module(method_id, current_module);
    let method_prefix = format!("{method_id}.");
    for (qname, definition) in snapshot.definitions().iter() {
        if let Some(method_name) = qname.strip_prefix(method_prefix.as_str())
            && !method_name.contains('.')
            && matches!(definition.body, DefinitionBody::Value { .. })
            && !is_instance_method(definition.ty(), method_id)
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

fn id_is_in_module(qualified_id: &str, module: &str) -> bool {
    qualified_id.starts_with(module) && qualified_id.as_bytes().get(module.len()) == Some(&b'.')
}

fn enum_variant_items(type_id: &str, snapshot: &AnalysisSnapshot) -> Option<Vec<CompletionItem>> {
    let to_item = |name: &str| CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::ENUM_MEMBER),
        ..Default::default()
    };
    match &snapshot.definitions().get(type_id)?.body {
        DefinitionBody::Enum { variants, .. } => {
            Some(variants.iter().map(|v| to_item(&v.name)).collect())
        }
        _ => None,
    }
}

/// Returns the target id of a non-generic alias.
fn alias_target(type_id: &str, snapshot: &AnalysisSnapshot) -> Option<String> {
    let def = snapshot.definitions().get(type_id)?;
    let DefinitionBody::TypeAlias { generics, .. } = &def.body else {
        return None;
    };
    if !generics.is_empty() {
        return None;
    }
    let target = match &def.ty {
        Type::Nominal { id, .. } => id.to_string(),
        Type::Simple(kind) => format!("prelude.{}", kind.leaf_name()),
        Type::Compound { kind, .. } => format!("prelude.{}", kind.leaf_name()),
        _ => return None,
    };
    (target != type_id).then_some(target)
}

fn is_instance_method(ty: &syntax::types::Type, type_id: &str) -> bool {
    let func_ty = match ty {
        syntax::types::Type::Forall { body, .. } => body,
        other => other,
    };
    match func_ty {
        syntax::types::Type::Function(f) if !f.params.is_empty() => {
            type_name(&f.params[0]).is_some_and(|name| name == type_id)
        }
        _ => false,
    }
}

pub(crate) fn attribute_completions(source: &str, offset: usize) -> Option<Vec<CompletionItem>> {
    let tokens = Lexer::new(source, 0).lex().tokens;
    let split = tokens.partition_point(|t| (t.byte_offset as usize) < offset);
    let (before, after) = tokens.split_at(split);

    if !in_attribute_name_position(before) {
        return None;
    }

    let items = match enclosing_context(before) {
        EnclosingContext::Struct => collect(attributes_for(AttributeTarget::StructField)),
        EnclosingContext::Parenthesized | EnclosingContext::Enum | EnclosingContext::Function => {
            Vec::new()
        }
        EnclosingContext::Impl => match following_item(after).item {
            FollowingItem::Target(AttributeTarget::Function) | FollowingItem::Unknown => {
                collect(attributes_for(AttributeTarget::Function))
            }
            _ => Vec::new(),
        },
        // An interface accepts an attribute only on a bare `fn`, not `pub fn`.
        EnclosingContext::Interface => match following_item(after) {
            Following {
                item: FollowingItem::Target(AttributeTarget::Function),
                is_pub: false,
            }
            | Following {
                item: FollowingItem::Unknown,
                is_pub: false,
            } => collect(attributes_for(AttributeTarget::Function)),
            _ => Vec::new(),
        },
        EnclosingContext::TopLevel => match following_item(after).item {
            FollowingItem::Target(target) => collect(attributes_for(target)),
            FollowingItem::Invalid => Vec::new(),
            FollowingItem::Unknown => collect(top_level_attributes()),
        },
    };

    Some(items)
}

fn collect<'a>(infos: impl Iterator<Item = &'a AttributeInfo>) -> Vec<CompletionItem> {
    infos.map(attribute_item).collect()
}

fn attribute_item(info: &AttributeInfo) -> CompletionItem {
    CompletionItem {
        label: info.name.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(info.detail.to_string()),
        ..Default::default()
    }
}

fn top_level_attributes() -> impl Iterator<Item = &'static AttributeInfo> {
    syntax::attributes::ATTRIBUTES.iter().filter(|a| {
        a.applies_to(AttributeTarget::Struct)
            || a.applies_to(AttributeTarget::Enum)
            || a.applies_to(AttributeTarget::Function)
    })
}

fn in_attribute_name_position(before: &[Token]) -> bool {
    let end = match before.last() {
        Some(t) if t.kind == Tk::Identifier => before.len() - 1,
        _ => before.len(),
    };
    end >= 2 && before[end - 1].kind == Tk::LeftSquareBracket && before[end - 2].kind == Tk::Hash
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EnclosingContext {
    Parenthesized,
    Struct,
    Enum,
    Impl,
    Interface,
    Function,
    TopLevel,
}

fn enclosing_context(before: &[Token]) -> EnclosingContext {
    enum Frame {
        Paren,
        Brace(EnclosingContext),
    }
    let mut stack: Vec<Frame> = Vec::new();
    let mut pending = EnclosingContext::TopLevel;
    for token in before {
        match token.kind {
            Tk::LeftCurlyBrace => {
                stack.push(Frame::Brace(pending));
                pending = EnclosingContext::TopLevel;
            }
            Tk::LeftParen => stack.push(Frame::Paren),
            Tk::RightCurlyBrace | Tk::RightParen => {
                stack.pop();
            }
            Tk::Semicolon => pending = EnclosingContext::TopLevel,
            Tk::Struct => pending = EnclosingContext::Struct,
            Tk::Enum => pending = EnclosingContext::Enum,
            Tk::Impl => pending = EnclosingContext::Impl,
            Tk::Interface => pending = EnclosingContext::Interface,
            Tk::Function => pending = EnclosingContext::Function,
            _ => {}
        }
    }
    for frame in stack.iter().rev() {
        match frame {
            Frame::Paren => return EnclosingContext::Parenthesized,
            Frame::Brace(EnclosingContext::TopLevel) => continue,
            Frame::Brace(context) => return *context,
        }
    }
    EnclosingContext::TopLevel
}

enum FollowingItem {
    Target(AttributeTarget),
    /// A declaration that rejects attributes (`interface`, `impl`, `embed`, `const`, ...).
    Invalid,
    Unknown,
}

/// The following item, plus whether it carries `pub` (rejected on an interface
/// method).
struct Following {
    item: FollowingItem,
    is_pub: bool,
}

/// Classifies the definition following the in-progress attribute.
fn following_item(after: &[Token]) -> Following {
    let kind = |i: usize| after.get(i).map(|t| t.kind);
    let mut i = 0;

    // Skip the rest of the current attribute: optional name, `( ... )`, and `]`.
    // `embed` is never an attribute name; it starts a following embedding item.
    if kind(i) == Some(Tk::Identifier) && after.get(i).is_none_or(|t| t.text != "embed") {
        i += 1;
    }
    if kind(i) == Some(Tk::LeftParen) {
        i = skip_balanced(after, i, Tk::LeftParen, Tk::RightParen);
    }
    if kind(i) == Some(Tk::RightSquareBracket) {
        i += 1;
    }

    let mut is_pub = false;
    loop {
        let item = match kind(i) {
            Some(Tk::Comment | Tk::Semicolon) => {
                i += 1;
                continue;
            }
            // A doc comment must come before the attribute, not after it.
            Some(Tk::DocComment) => FollowingItem::Invalid,
            Some(Tk::Hash) if kind(i + 1) == Some(Tk::LeftSquareBracket) => {
                i = skip_stacked_attribute(after, i);
                continue;
            }
            Some(Tk::Pub) => {
                is_pub = true;
                i += 1;
                continue;
            }
            Some(Tk::Struct) => FollowingItem::Target(AttributeTarget::Struct),
            Some(Tk::Enum) => FollowingItem::Target(AttributeTarget::Enum),
            Some(Tk::Function) => FollowingItem::Target(AttributeTarget::Function),
            Some(Tk::Interface | Tk::Impl | Tk::Const | Tk::Var | Tk::Import | Tk::Type) => {
                FollowingItem::Invalid
            }
            // `embed` is a contextual keyword lexed as an identifier; an interface
            // embedding or embedded field takes no attribute.
            Some(Tk::Identifier) if after.get(i).is_some_and(|t| t.text == "embed") => {
                FollowingItem::Invalid
            }
            _ => FollowingItem::Unknown,
        };
        return Following { item, is_pub };
    }
}

fn skip_balanced(after: &[Token], mut i: usize, open: Tk, close: Tk) -> usize {
    let mut depth = 0;
    while i < after.len() {
        if after[i].kind == open {
            depth += 1;
        } else if after[i].kind == close {
            depth -= 1;
        }
        i += 1;
        if depth == 0 {
            break;
        }
    }
    i
}

/// Index past a stacked `#[ ... ]` (a `]` inside a string arg is its own token).
fn skip_stacked_attribute(after: &[Token], mut i: usize) -> usize {
    while i < after.len() && after[i].kind != Tk::RightSquareBracket {
        i += 1;
    }
    if i < after.len() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod attribute_completion_tests {
    use super::*;

    /// Runs `attribute_completions` with the cursor at the `|` marker (which is
    /// stripped before scanning) and returns the offered labels, or `None` when
    /// the cursor is not in attribute position.
    fn labels_at(src_with_cursor: &str) -> Option<Vec<String>> {
        let offset = src_with_cursor
            .find('|')
            .expect("test input needs a `|` cursor");
        let source = src_with_cursor.replacen('|', "", 1);
        attribute_completions(&source, offset)
            .map(|items| items.into_iter().map(|i| i.label).collect())
    }

    #[test]
    fn top_level_struct_offers_serialization_tag_and_display() {
        let labels = labels_at("#[|\nstruct Point { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(labels.contains(&"display".to_string()));
        assert!(labels.contains(&"tag".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn top_level_enum_offers_iterate_display_and_json() {
        let labels = labels_at("#[|\nenum Direction { North, South }").unwrap();
        assert!(labels.contains(&"iterate".to_string()));
        assert!(labels.contains(&"display".to_string()));
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"xml".to_string()));
        assert!(!labels.contains(&"tag".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn struct_field_offers_serialization_and_tag_not_display() {
        let labels = labels_at("struct S {\n  #[|\n  x: int\n}").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(labels.contains(&"tag".to_string()));
        assert!(!labels.contains(&"display".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn method_in_impl_offers_allow_only() {
        let labels = labels_at("impl S {\n  #[|\n  fn run(self) {}\n}").unwrap();
        assert_eq!(labels, vec!["allow".to_string()]);
    }

    #[test]
    fn method_in_interface_offers_allow_only() {
        let labels = labels_at("interface I {\n  #[|\n  fn run(self)\n}").unwrap();
        assert_eq!(labels, vec!["allow".to_string()]);
    }

    #[test]
    fn interface_parent_embed_offers_nothing() {
        let labels = labels_at("interface Child {\n  #[|\n  embed Parent\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn comment_between_attribute_and_enum_resolves_target() {
        let labels = labels_at("#[|\n// note\nenum E { A }").unwrap();
        assert!(labels.contains(&"iterate".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn doc_comment_after_attribute_offers_nothing() {
        let labels = labels_at("#[|\n/// doc\nstruct S {}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn interface_pub_fn_offers_nothing() {
        let labels = labels_at("interface I {\n  #[|\n  pub fn run(self)\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn impl_pub_fn_offers_allow() {
        let labels = labels_at("impl S {\n  #[|\n  pub fn run(self) {}\n}").unwrap();
        assert_eq!(labels, vec!["allow".to_string()]);
    }

    #[test]
    fn interface_partial_pub_offers_nothing() {
        let labels = labels_at("interface I {\n  #[|\n  pub\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn attribute_arg_backtick_paren_resolves_target() {
        let labels = labels_at("#[|tag(`json:\")\"`)]\nstruct S { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn stacked_attribute_string_bracket_resolves_target() {
        let labels = labels_at("#[|\n#[json(\"x]\")]\nstruct S { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
        assert!(!labels.contains(&"allow".to_string()));
    }

    #[test]
    fn function_params_offer_nothing() {
        let labels = labels_at("fn f(#[| x: int) {}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn tuple_struct_field_offers_nothing() {
        let labels = labels_at("struct S(#[| int)").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn method_param_offers_nothing() {
        let labels = labels_at("impl S {\n  fn m(#[| x: int) {}\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn enum_variant_position_offers_nothing() {
        let labels = labels_at("enum E {\n  #[|\n  A\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn function_body_offers_nothing() {
        let labels = labels_at("fn main() {\n  #[|\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn function_body_inside_nested_block_offers_nothing() {
        let labels = labels_at("fn main() {\n  if true {\n    #[|\n  }\n}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn unknown_top_level_target_offers_full_union() {
        let labels = labels_at("#[|").unwrap();
        for expected in ["json", "display", "iterate", "allow", "tag"] {
            assert!(labels.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn before_attribute_rejecting_declaration_offers_nothing() {
        for decl in ["interface S {}", "impl S {}", "const X = 1", "type A = int"] {
            let labels = labels_at(&format!("#[|\n{decl}")).unwrap();
            assert!(
                labels.is_empty(),
                "expected nothing before `{decl}`, got {labels:?}"
            );
        }
    }

    #[test]
    fn partial_name_still_resolves_target() {
        let labels = labels_at("#[js|\nstruct Point { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
    }

    #[test]
    fn stacked_attributes_resolve_to_following_item() {
        let labels = labels_at("#[display]\n#[|\nstruct Point { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
    }

    #[test]
    fn pub_modifier_is_skipped() {
        let labels = labels_at("#[|\npub struct Point { x: int }").unwrap();
        assert!(labels.contains(&"json".to_string()));
        assert!(!labels.contains(&"iterate".to_string()));
    }

    #[test]
    fn not_in_attribute_position_yields_none() {
        assert!(labels_at("let x = |5").is_none());
        assert!(labels_at("fn main() { let y = |x }").is_none());
    }

    #[test]
    fn hash_inside_string_is_not_an_attribute() {
        assert!(labels_at("fn main() { let s = \"#[|\" }").is_none());
    }

    #[test]
    fn closed_attribute_is_not_in_name_position() {
        assert!(labels_at("#[json]|\nstruct Point { x: int }").is_none());
    }

    #[test]
    fn cursor_in_argument_list_is_not_name_position() {
        assert!(labels_at("struct S {\n  #[json(|\n  x: int\n}").is_none());
    }
}
