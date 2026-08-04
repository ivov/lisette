use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use semantics::checker::promotion::{self, MemberKind, Resolution};
use semantics::store::Store;
use syntax::ast::{Annotation, Attribute, Expression, Pattern, SelectArm, Span, StructSpread};
use syntax::program::File;
use syntax::program::{DefinitionBody, DotAccessKind, EqualityIndex, Package};
use syntax::types::{CompoundKind, Symbol, Type, unqualified_name};

use super::reference_graph::{EnumVariantId, PackageItemId, ReferenceGraph, StructFieldId};

pub struct AliasMap<'a> {
    aliases: HashMap<String, ImportSpans>,
    file_id: u32,
    store: &'a Store,
}

struct ImportSpans {
    name: Span,
    statement: Span,
}

impl<'a> AliasMap<'a> {
    pub fn build(file: &File, store: &'a Store) -> Self {
        let mut aliases = HashMap::default();

        for import in file.imports() {
            if let Some(effective) = import.effective_alias(&store.go_package_names) {
                aliases.insert(
                    effective,
                    ImportSpans {
                        name: import.name_span,
                        statement: import.span,
                    },
                );
            }
        }

        Self {
            aliases,
            file_id: file.id,
            store,
        }
    }

    fn resolve(&self, package: &Package, name: &str) -> Option<PackageItemId> {
        let qualified_name = Symbol::from_parts(&package.id, name);
        if package.definitions.contains_key(qualified_name.as_str()) {
            return Some(PackageItemId::new(name));
        }
        self.aliases
            .contains_key(name)
            .then(|| PackageItemId::import(self.file_id, name))
    }

    fn is_import_alias(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
    }

    pub(super) fn imports(&self) -> impl Iterator<Item = (&str, Span, Span)> {
        self.aliases
            .iter()
            .map(|(alias, spans)| (alias.as_str(), spans.name, spans.statement))
    }
}

fn deref_for_keying(ty: &Type, aliases: &AliasMap) -> Type {
    aliases.store.peel_refs_and_aliases(ty).0
}

// ctx is `None` at the top level. Function/Const nest inside an enclosing item and inherit
// it when present; every other item kind below always self-derives from its own name.
fn item_ctx(ctx: Option<&PackageItemId>, name: &str) -> PackageItemId {
    ctx.cloned().unwrap_or_else(|| PackageItemId::new(name))
}

pub(super) fn walk_expression(
    package: &Package,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    match expression {
        Expression::Identifier { value, .. } => {
            walk_identifier(package, value, graph, alias_map, ctx);
        }

        Expression::Call { .. } => {
            walk_call(package, expression, graph, alias_map, ctx);
        }

        Expression::StructCall { .. } => {
            walk_struct_call(package, expression, graph, alias_map, ctx);
        }

        Expression::DotAccess { .. } => {
            walk_dot_access(package, expression, graph, alias_map, ctx);
        }

        Expression::Function { name, .. } => {
            let fn_ctx = item_ctx(ctx, name);
            walk_callable_body(package, expression, graph, alias_map, &fn_ctx);
        }

        Expression::Const {
            identifier,
            annotation,
            expression,
            ..
        } => {
            let const_ctx = item_ctx(ctx, identifier);
            if let Some(ann) = annotation {
                walk_annotation(package, ann, graph, alias_map, &const_ctx);
            }
            if let Some(value) = expression.value() {
                walk_expression(package, value, graph, alias_map, Some(&const_ctx));
            }
        }

        Expression::Enum {
            name,
            variants,
            attributes,
            ..
        } => {
            let enum_ctx = PackageItemId::new(name);
            let synthesizes_equality =
                has_synthesized_equality(package, name, attributes, alias_map);
            for v in variants {
                for f in &v.fields {
                    walk_annotation(package, &f.annotation, graph, alias_map, &enum_ctx);
                    if synthesizes_equality {
                        mark_equals_roots(graph, &f.ty, package, alias_map);
                    }
                }
            }
        }

        Expression::Struct {
            name,
            generics,
            fields,
            attributes,
            ..
        } => {
            let struct_ctx = PackageItemId::new(name);
            for g in generics {
                for bound in g.bounds() {
                    walk_annotation(package, bound, graph, alias_map, &struct_ctx);
                }
            }
            let synthesizes_equality =
                has_synthesized_equality(package, name, attributes, alias_map);
            for f in fields {
                walk_annotation(package, &f.annotation, graph, alias_map, &struct_ctx);
                if synthesizes_equality {
                    mark_equals_roots(graph, &f.ty, package, alias_map);
                }
            }
        }

        Expression::TypeAlias {
            name, annotation, ..
        } => {
            let alias_ctx = PackageItemId::new(name);
            walk_annotation(package, annotation, graph, alias_map, &alias_ctx);
        }

        Expression::Interface {
            name,
            method_signatures,
            parents,
            ..
        } => {
            let iface_ctx = PackageItemId::new(name);
            for p in parents {
                walk_annotation(package, &p.annotation, graph, alias_map, &iface_ctx);
            }
            for sig in method_signatures {
                walk_expression(package, sig, graph, alias_map, Some(&iface_ctx));
            }
        }

        Expression::Lambda { params, body, .. } => {
            for p in params {
                walk_pattern(package, &p.pattern, graph, alias_map, ctx);
                if let Some(from) = ctx {
                    walk_type_or_annotation(
                        package,
                        &p.ty,
                        p.annotation.as_ref(),
                        graph,
                        alias_map,
                        from,
                    );
                }
            }
            walk_expression(package, body, graph, alias_map, ctx);
        }

        Expression::Let {
            binding,
            value,
            mode,
            ..
        } => {
            walk_pattern(package, &binding.pattern, graph, alias_map, ctx);
            if let Some(from) = ctx {
                walk_type_or_annotation(
                    package,
                    &binding.ty,
                    binding.annotation.as_ref(),
                    graph,
                    alias_map,
                    from,
                );
            }
            walk_expression(package, value, graph, alias_map, ctx);
            if let Some(eb) = mode.else_block() {
                walk_expression(package, eb, graph, alias_map, ctx);
            }
        }

        Expression::ImplBlock {
            annotation,
            methods,
            generics,
            receiver_name,
            ..
        } => {
            if let Some(from) = ctx {
                walk_annotation(package, annotation, graph, alias_map, from);
            }
            let impl_id = PackageItemId::new(receiver_name);
            let impl_context = ctx.unwrap_or(&impl_id);
            for g in generics {
                for bound in g.bounds() {
                    walk_annotation(package, bound, graph, alias_map, impl_context);
                }
            }
            for m in methods {
                if let Expression::Function { name, .. } = m {
                    let method_ctx = PackageItemId::method(name, receiver_name);
                    walk_callable_body(package, m, graph, alias_map, &method_ctx);
                } else {
                    walk_expression(package, m, graph, alias_map, ctx);
                }
            }
        }

        Expression::Match { subject, arms, .. } => {
            walk_expression(package, subject, graph, alias_map, ctx);
            for arm in arms {
                walk_pattern(package, &arm.pattern, graph, alias_map, ctx);
                if let Some(g) = &arm.guard {
                    walk_expression(package, g, graph, alias_map, ctx);
                }
                walk_expression(package, &arm.expression, graph, alias_map, ctx);
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            ..
        } => {
            walk_expression(package, scrutinee, graph, alias_map, ctx);
            walk_pattern(package, pattern, graph, alias_map, ctx);
            walk_expression(package, consequence, graph, alias_map, ctx);
            if let Some(alternative) = alternative.expression() {
                walk_expression(package, alternative, graph, alias_map, ctx);
            }
        }

        Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } => {
            walk_expression(package, scrutinee, graph, alias_map, ctx);
            walk_pattern(package, pattern, graph, alias_map, ctx);
            walk_expression(package, body, graph, alias_map, ctx);
        }

        Expression::For {
            binding,
            iterable,
            body,
            ..
        } => {
            walk_pattern(package, &binding.pattern, graph, alias_map, ctx);
            walk_expression(package, iterable, graph, alias_map, ctx);
            walk_expression(package, body, graph, alias_map, ctx);
        }

        Expression::Select { arms, .. } => {
            walk_select(package, arms, graph, alias_map, ctx);
        }

        Expression::Cast {
            expression,
            target_type,
            ..
        } => {
            if let Some(from) = ctx {
                walk_annotation(package, target_type, graph, alias_map, from);
            }
            walk_expression(package, expression, graph, alias_map, ctx);
        }

        _ => {
            for child in expression.children() {
                walk_expression(package, child, graph, alias_map, ctx);
            }
        }
    }
}

fn walk_identifier(
    package: &Package,
    value: &str,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    add_ref(graph, ctx, alias_map, package, extract_base_name(value));
    let mut segments = value.split('.');
    let first = segments.next().unwrap_or("");
    // Static method values are represented as qualified identifiers.
    if let Some(second) = segments.next()
        && is_upper(first)
    {
        if is_upper(second) {
            graph.mark_enum_variant_used(EnumVariantId::new(first, second));
        }
        add_ref(graph, ctx, alias_map, package, first);
        if let Some(from) = ctx {
            let method_name = value.rsplit('.').next().unwrap_or("");
            graph.add_reference(from, PackageItemId::method(method_name, first));
        }
    }
}

fn walk_call(
    package: &Package,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        type_arguments,
        ..
    } = expression
    else {
        return;
    };
    if let Expression::Identifier { value, .. } = callee.as_ref() {
        let mut segments = value.split('.');
        let first = segments.next().unwrap_or("");
        if segments.next().is_some() && is_upper(first) {
            add_ref(graph, ctx, alias_map, package, first);
            if let Some(from) = ctx {
                let method_name = value.rsplit('.').next().unwrap_or("");
                graph.add_reference(from, PackageItemId::method(method_name, first));
            }
        }
        if let Some(from) = ctx
            && (value.as_str() == "Slice.equals" || value.as_str() == "Map.equals")
            && let Some(receiver) = args.first()
            && is_container_receiver(&receiver.get_type(), alias_map)
        {
            add_equals_references(graph, from, &receiver.get_type(), package, alias_map);
        }
    }
    walk_expression(package, callee, graph, alias_map, ctx);
    for arg in args {
        walk_expression(package, arg, graph, alias_map, ctx);
    }
    if let Some(spread_expr) = spread {
        walk_expression(package, spread_expr, graph, alias_map, ctx);
    }
    if let Some(from) = ctx {
        for type_arg in type_arguments.annotations() {
            walk_annotation(package, type_arg, graph, alias_map, from);
        }
    }
}

fn walk_struct_call(
    package: &Package,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    let Expression::StructCall {
        name,
        field_assignments,
        spread,
        ty,
        ..
    } = expression
    else {
        return;
    };
    let mut segments = name.split('.');
    let p0 = segments.next().unwrap_or("");
    let p1 = segments.next();
    let p2 = segments.next();
    if !is_upper(p0) {
        add_ref(graph, ctx, alias_map, package, p0);
    } else {
        add_ref(graph, ctx, alias_map, package, extract_base_name(name));
    }
    if is_upper(p0) && p1.is_some_and(is_upper) {
        graph.mark_enum_variant_used(EnumVariantId::new(p0, p1.unwrap()));
    } else if p1.is_some_and(is_upper) && p2.is_some_and(is_upper) {
        graph.mark_enum_variant_used(EnumVariantId::new(p1.unwrap(), p2.unwrap()));
    }
    for f in field_assignments {
        walk_expression(package, &f.value, graph, alias_map, ctx);
    }
    match spread {
        StructSpread::None => {}
        StructSpread::From(spread_expression) => {
            walk_expression(package, spread_expression, graph, alias_map, ctx);
            if let Some(ty_name) = qualified_type_name(&spread_expression.get_type(), alias_map) {
                let explicit: HashSet<&str> =
                    field_assignments.iter().map(|f| f.name.as_str()).collect();
                if let Some(def) = package.definitions.get(ty_name.as_str())
                    && let DefinitionBody::Struct { fields, .. } = &def.body
                {
                    for field in fields {
                        if !explicit.contains(field.name.as_str()) {
                            graph.mark_struct_field_used(StructFieldId::new(
                                ty_name.as_str(),
                                &field.name,
                            ));
                        }
                    }
                }
            }
        }
        StructSpread::Autofill { .. } => {
            if let Some(ty_name) = qualified_type_name(ty, alias_map) {
                let explicit: HashSet<&str> =
                    field_assignments.iter().map(|f| f.name.as_str()).collect();
                if let Some(def) = package.definitions.get(ty_name.as_str())
                    && let DefinitionBody::Struct { fields, .. } = &def.body
                {
                    for field in fields {
                        if !explicit.contains(field.name.as_str()) {
                            graph.mark_struct_field_used(StructFieldId::new(
                                ty_name.as_str(),
                                &field.name,
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn walk_dot_access(
    package: &Package,
    dot_access: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    let Expression::DotAccess {
        expression,
        member,
        resolution,
        ..
    } = dot_access
    else {
        unreachable!("walk_dot_access called with non-DotAccess expression");
    };
    walk_expression(package, expression, graph, alias_map, ctx);
    let receiver_ty = expression.get_type();
    if let Some(ty_name) = qualified_type_name(&receiver_ty, alias_map) {
        graph.mark_struct_field_used(StructFieldId::new(ty_name.as_str(), member));
    }
    mark_promoted_field_read(&receiver_ty, member, graph, alias_map);
    if let Some(from) = ctx
        && is_method_access(resolution.kind())
        && credits_local_method(&receiver_ty, package, alias_map)
    {
        let to = method_node(member, &receiver_ty, alias_map);
        graph.add_reference(from, to);
    }
    if let Some(from) = ctx
        && member == "equals"
        && is_container_receiver(&receiver_ty, alias_map)
    {
        add_equals_references(graph, from, &receiver_ty, package, alias_map);
    }
}

fn walk_select(
    package: &Package,
    arms: &[SelectArm],
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    for arm in arms {
        match arm {
            SelectArm::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => {
                walk_pattern(package, binding, graph, alias_map, ctx);
                walk_expression(package, receive_expression, graph, alias_map, ctx);
                walk_expression(package, body, graph, alias_map, ctx);
            }
            SelectArm::Send {
                send_expression,
                body,
            } => {
                walk_expression(package, send_expression, graph, alias_map, ctx);
                walk_expression(package, body, graph, alias_map, ctx);
            }
            SelectArm::MatchReceive {
                receive_expression,
                arms: match_arms,
            } => {
                walk_expression(package, receive_expression, graph, alias_map, ctx);
                for match_arm in match_arms {
                    walk_pattern(package, &match_arm.pattern, graph, alias_map, ctx);
                    walk_expression(package, &match_arm.expression, graph, alias_map, ctx);
                }
            }
            SelectArm::WildCard { body } => {
                walk_expression(package, body, graph, alias_map, ctx);
            }
        }
    }
}

fn walk_pattern(
    package: &Package,
    pattern: &Pattern,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    match pattern {
        Pattern::EnumVariant {
            identifier,
            fields,
            ty,
            ..
        } => {
            mark_constructor_pattern(package, identifier, ty, graph, alias_map, ctx);
            for f in fields {
                walk_pattern(package, f, graph, alias_map, ctx);
            }
        }
        Pattern::Struct {
            identifier,
            fields,
            ty,
            ..
        } => {
            mark_constructor_pattern(package, identifier, ty, graph, alias_map, ctx);
            let key = qualified_type_name(ty, alias_map).or_else(|| {
                (!identifier.contains('.')).then(|| Symbol::from_parts(&package.id, identifier))
            });
            for f in fields {
                walk_pattern(package, &f.value, graph, alias_map, ctx);
                if let Some(key) = &key {
                    graph.mark_struct_field_used(StructFieldId::new(key.as_str(), &f.name));
                }
            }
        }
        Pattern::Tuple { elements, .. } => {
            for e in elements {
                walk_pattern(package, e, graph, alias_map, ctx);
            }
        }
        Pattern::Slice { prefix, .. } => {
            for p in prefix {
                walk_pattern(package, p, graph, alias_map, ctx);
            }
        }
        Pattern::Or { patterns, .. } => {
            for p in patterns {
                walk_pattern(package, p, graph, alias_map, ctx);
            }
        }
        Pattern::AsBinding { pattern, .. } => {
            walk_pattern(package, pattern, graph, alias_map, ctx);
        }
        Pattern::Literal { .. }
        | Pattern::Identifier { .. }
        | Pattern::WildCard { .. }
        | Pattern::Unit { .. } => {}
    }
}

fn walk_type_or_annotation(
    package: &Package,
    ty: &Type,
    annotation: Option<&Annotation>,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &PackageItemId,
) {
    if let Some(a) = annotation {
        walk_annotation(package, a, graph, alias_map, from);
    } else {
        walk_type(package, ty, graph, alias_map, from);
    }
}

fn walk_annotation(
    package: &Package,
    ann: &Annotation,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &PackageItemId,
) {
    match ann {
        Annotation::Constructor { name, params, .. } => {
            let base_name = extract_base_name(name);
            if let Some(to) = alias_map.resolve(package, base_name) {
                graph.add_reference(from, to);
            }
            for p in params {
                walk_annotation(package, p, graph, alias_map, from);
            }
        }
        Annotation::Function {
            params,
            return_type,
            ..
        } => {
            for p in params {
                walk_annotation(package, &p.annotation, graph, alias_map, from);
            }
            walk_annotation(package, return_type, graph, alias_map, from);
        }
        Annotation::Tuple { elements, .. } => {
            for e in elements {
                walk_annotation(package, e, graph, alias_map, from);
            }
        }
        Annotation::Unknown | Annotation::Opaque { .. } | Annotation::Constant { .. } => {}
    }
}

fn walk_type(
    package: &Package,
    ty: &Type,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &PackageItemId,
) {
    match ty {
        Type::Nominal { id, params, .. } => {
            // Keep a local qualified type from being mistaken for an import.
            let package_prefix = format!("{}.", package.id);
            let local_id = id.strip_prefix(&package_prefix).unwrap_or(id);
            let base_name = extract_base_name(local_id);
            if let Some(to) = alias_map.resolve(package, base_name) {
                graph.add_reference(from, to);
            }
            for p in params {
                walk_type(package, p, graph, alias_map, from);
            }
        }
        Type::Function(f) => {
            for p in &f.params {
                walk_type(package, &p.ty, graph, alias_map, from);
            }
            walk_type(package, &f.return_type, graph, alias_map, from);
        }
        Type::Forall { body, .. } => walk_type(package, body, graph, alias_map, from),
        Type::Tuple(elems) => {
            for e in elems {
                walk_type(package, e, graph, alias_map, from);
            }
        }
        Type::Compound { args, .. } => {
            for a in args {
                walk_type(package, a, graph, alias_map, from);
            }
        }
        Type::Array { element, .. } => walk_type(package, element, graph, alias_map, from),
        Type::Simple(_)
        | Type::Var { .. }
        | Type::Uninferred
        | Type::Ignored
        | Type::Parameter(_)
        | Type::Never
        | Type::Error
        | Type::ImportNamespace(_)
        | Type::ReceiverPlaceholder => {}
    }
}

fn add_ref(
    graph: &mut ReferenceGraph,
    ctx: Option<&PackageItemId>,
    alias_map: &AliasMap,
    package: &Package,
    name: &str,
) {
    if let Some(from) = ctx
        && let Some(to) = alias_map.resolve(package, name)
    {
        graph.add_reference(from, to);
    }
}

fn mark_constructor_pattern(
    package: &Package,
    identifier: &str,
    ty: &Type,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&PackageItemId>,
) {
    if let Some((alias, _)) = identifier.split_once('.')
        && alias_map.is_import_alias(alias)
    {
        add_ref(graph, ctx, alias_map, package, alias);
        return;
    }

    let enum_name = type_name(ty, alias_map).or_else(|| {
        identifier
            .split_once('.')
            .map(|(first, _)| first.to_string())
    });
    if let Some(enum_name) = enum_name {
        add_ref(graph, ctx, alias_map, package, &enum_name);
        graph.mark_enum_variant_used(EnumVariantId::new(&enum_name, unqualified_name(identifier)));
    }
}

fn is_method_access(kind: Option<DotAccessKind>) -> bool {
    matches!(
        kind,
        Some(
            DotAccessKind::InstanceMethod { .. }
                | DotAccessKind::InstanceMethodValue { .. }
                | DotAccessKind::StaticMethod { .. }
        )
    )
}

fn walk_callable_body(
    package: &Package,
    function: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    fn_ctx: &PackageItemId,
) {
    let Expression::Function {
        generics,
        params,
        return_annotation,
        body,
        ..
    } = function
    else {
        return;
    };
    for g in generics {
        for bound in g.bounds() {
            walk_annotation(package, bound, graph, alias_map, fn_ctx);
        }
    }
    for p in params {
        walk_pattern(package, &p.pattern, graph, alias_map, Some(fn_ctx));
        walk_type_or_annotation(
            package,
            &p.ty,
            p.annotation.as_ref(),
            graph,
            alias_map,
            fn_ctx,
        );
    }
    walk_annotation(package, return_annotation, graph, alias_map, fn_ctx);
    if let Some(body) = body.definition() {
        walk_expression(package, body, graph, alias_map, Some(fn_ctx));
    }
}

fn method_node(member: &str, receiver_ty: &Type, aliases: &AliasMap) -> PackageItemId {
    match type_name(receiver_ty, aliases) {
        Some(name) => PackageItemId::method(member, &name),
        None => PackageItemId::new(member),
    }
}

fn credits_local_method(receiver_ty: &Type, package: &Package, aliases: &AliasMap) -> bool {
    let current = match deref_for_keying(receiver_ty, aliases) {
        Type::Function(f) => (*f.return_type).clone(),
        other => other,
    };
    match current {
        Type::Nominal { id, .. } => id.as_str().starts_with(&format!("{}.", package.id)),
        _ => false,
    }
}

fn has_equality_attr(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "equality")
}

fn has_synthesized_equality(
    package: &Package,
    name: &str,
    attributes: &[Attribute],
    alias_map: &AliasMap,
) -> bool {
    let owner = format!("{}.{}", package.id, name);
    has_equality_attr(attributes) && alias_map.store.equality_index.is_synthesized(&owner)
}

fn add_equals_references(
    graph: &mut ReferenceGraph,
    from: &PackageItemId,
    ty: &Type,
    package: &Package,
    aliases: &AliasMap,
) {
    let mut targets = Vec::new();
    let ty = ty.strip_refs();
    equals_targets(
        &ty,
        package,
        aliases.store,
        &aliases.store.equality_index,
        &mut targets,
    );
    for to in targets {
        graph.add_reference(from, to);
    }
}

fn mark_equals_roots(graph: &mut ReferenceGraph, ty: &Type, package: &Package, aliases: &AliasMap) {
    let mut targets = Vec::new();
    equals_targets(
        ty,
        package,
        aliases.store,
        &aliases.store.equality_index,
        &mut targets,
    );
    for target in targets {
        graph.mark_as_used(target);
    }
}

pub(super) fn equals_targets(
    ty: &Type,
    package: &Package,
    store: &Store,
    index: &EqualityIndex,
    out: &mut Vec<PackageItemId>,
) {
    let mut current = ty.clone();
    let mut seen = HashSet::default();
    while let Type::Nominal { id, .. } = &current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(next) = store.underlying_type(&current) else {
            break;
        };
        current = next;
    }
    match &current {
        Type::Compound {
            kind: CompoundKind::Slice,
            args,
        } => {
            if let Some(element) = args.first() {
                equals_targets(element, package, store, index, out);
            }
        }
        Type::Compound {
            kind: CompoundKind::Map,
            args,
        } => {
            if let Some(value) = args.get(1) {
                equals_targets(value, package, store, index, out);
            }
        }
        Type::Nominal { id, .. }
            if id.as_str().starts_with(&format!("{}.", package.id))
                && index.usable_from(id.as_str(), &package.id) =>
        {
            out.push(PackageItemId::equals_method(unqualified_name(id)));
        }
        _ => {}
    }
}

fn is_container_receiver(receiver_ty: &Type, aliases: &AliasMap) -> bool {
    let current = deref_for_keying(receiver_ty, aliases);
    current.is_slice() || current.is_map()
}

fn mark_promoted_field_read(
    receiver_ty: &Type,
    member: &str,
    graph: &mut ReferenceGraph,
    aliases: &AliasMap,
) {
    let receiver = receiver_ty.strip_refs();
    if !promotion::has_direct_embed(aliases.store, &receiver) {
        return;
    }
    if let Resolution::Found(resolved) =
        promotion::resolve_selector(aliases.store, &receiver, member)
        && matches!(resolved.kind, MemberKind::Field { .. })
    {
        graph.mark_struct_field_used(StructFieldId::new(resolved.declaring_type.as_str(), member));
    }
}

fn type_name(ty: &Type, aliases: &AliasMap) -> Option<String> {
    match deref_for_keying(ty, aliases) {
        Type::Nominal { id, .. } => Some(unqualified_name(&id).to_string()),
        _ => None,
    }
}

fn qualified_type_name(ty: &Type, aliases: &AliasMap) -> Option<Symbol> {
    match deref_for_keying(ty, aliases) {
        Type::Nominal { id, .. } => Some(id),
        _ => None,
    }
}

pub(crate) fn is_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

fn extract_base_name(name: &str) -> &str {
    let mut segments = name.split('.');
    let p0 = segments.next().unwrap_or("");
    let Some(p1) = segments.next() else {
        return p0;
    };
    let Some(_p2) = segments.next() else {
        return if is_upper(p1) { p0 } else { p1 };
    };
    if segments.next().is_none() {
        return p1;
    }
    name.split('.')
        .find(|p| is_upper(p))
        .or_else(|| name.split('.').next_back())
        .unwrap_or("")
}
