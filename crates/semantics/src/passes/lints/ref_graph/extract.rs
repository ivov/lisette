use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{
    Annotation, Attribute, Binding, Expression, Generic, ImportAlias, Pattern, SelectArm,
    SelectArmPattern, StructSpread,
};
use syntax::program::File;
use syntax::program::{DefinitionBody, DotAccessKind, EqualityIndex, Module};
use syntax::types::{CompoundKind, Symbol, Type, unqualified_name};

use super::reference_graph::{EnumVariantId, ModuleItemId, ReferenceGraph, StructFieldId};

pub struct AliasMap {
    aliases: HashMap<String, ModuleItemId>,
}

impl AliasMap {
    pub fn build(files: &HashMap<u32, File>, go_package_names: &HashMap<String, String>) -> Self {
        let mut aliases = HashMap::default();

        for file in files.values() {
            for import in file.imports() {
                if matches!(import.alias, Some(ImportAlias::Blank(_))) {
                    continue;
                }
                if let Some(effective) = import.effective_alias(go_package_names) {
                    aliases.insert(effective.clone(), ModuleItemId::new(&effective));
                }
            }
        }

        Self { aliases }
    }

    fn resolve(&self, module: &Module, name: &str) -> Option<ModuleItemId> {
        let qualified_name = Symbol::from_parts(&module.id, name);
        if module.definitions.contains_key(qualified_name.as_str()) {
            return Some(ModuleItemId::new(name));
        }
        self.aliases.get(name).cloned()
    }

    fn is_import_alias(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
    }
}

pub fn extract_references(
    module: &Module,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
) {
    let ctx = match expression {
        Expression::Function { name, .. } => Some(ModuleItemId::new(name)),
        Expression::Const { identifier, .. } => Some(ModuleItemId::new(identifier)),
        _ => None,
    };
    walk_expression(module, expression, graph, alias_map, ctx.as_ref());
}

fn walk_expression(
    module: &Module,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    match expression {
        Expression::Identifier { value, .. } => {
            walk_identifier(module, value, graph, alias_map, ctx);
        }

        Expression::Call {
            expression: callee,
            args,
            spread,
            type_args,
            ..
        } => {
            walk_call(
                module, callee, args, spread, type_args, graph, alias_map, ctx,
            );
        }

        Expression::StructCall { .. } => {
            walk_struct_call(module, expression, graph, alias_map, ctx);
        }

        Expression::DotAccess {
            expression,
            member,
            dot_access_kind,
            ..
        } => {
            walk_expression(module, expression, graph, alias_map, ctx);
            if let Some(ty_name) = type_name(&expression.get_type()) {
                graph.mark_struct_field_used(StructFieldId::new(&ty_name, member));
            }
            if let Some(from) = ctx
                && is_method_access(dot_access_kind)
                && credits_local_method(&expression.get_type(), module)
            {
                let to = method_node(member, &expression.get_type());
                graph.add_reference(from, to);
            }
            if let Some(from) = ctx
                && member == "equals"
                && is_container_receiver(&expression.get_type())
            {
                graph.record_equals_dispatch(from.clone(), expression.get_type());
            }
        }

        Expression::Function {
            name,
            generics,
            params,
            return_annotation,
            body,
            ..
        } => {
            let fn_ctx = ModuleItemId::new(name);
            walk_callable_body(
                module,
                generics,
                params,
                return_annotation,
                body,
                graph,
                alias_map,
                &fn_ctx,
            );
        }

        Expression::Const {
            identifier,
            annotation,
            expression,
            ..
        } => {
            let const_ctx = ModuleItemId::new(identifier);
            if let Some(ann) = annotation {
                walk_annotation(module, ann, graph, alias_map, &const_ctx);
            }
            walk_expression(module, expression, graph, alias_map, Some(&const_ctx));
        }

        Expression::Enum {
            name,
            variants,
            attributes,
            ..
        } => {
            let enum_ctx = ModuleItemId::new(name);
            let owner = format!("{}.{}", module.id, name);
            let has_equality = has_equality_attr(attributes);
            for v in variants {
                for f in &v.fields {
                    walk_annotation(module, &f.annotation, graph, alias_map, &enum_ctx);
                    if has_equality {
                        graph.record_equals_root(owner.clone(), f.ty.clone());
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
            let struct_ctx = ModuleItemId::new(name);
            for g in generics {
                for bound in &g.bounds {
                    walk_annotation(module, bound, graph, alias_map, &struct_ctx);
                }
            }
            let owner = format!("{}.{}", module.id, name);
            let has_equality = has_equality_attr(attributes);
            for f in fields {
                walk_annotation(module, &f.annotation, graph, alias_map, &struct_ctx);
                if has_equality {
                    graph.record_equals_root(owner.clone(), f.ty.clone());
                }
            }
        }

        Expression::TypeAlias {
            name, annotation, ..
        } => {
            let alias_ctx = ModuleItemId::new(name);
            walk_annotation(module, annotation, graph, alias_map, &alias_ctx);
        }

        Expression::Interface {
            name,
            method_signatures,
            parents,
            ..
        } => {
            let iface_ctx = ModuleItemId::new(name);
            for p in parents {
                walk_annotation(module, &p.annotation, graph, alias_map, &iface_ctx);
            }
            for sig in method_signatures {
                walk_expression(module, sig, graph, alias_map, Some(&iface_ctx));
            }
        }

        Expression::Lambda { params, body, .. } => {
            for p in params {
                walk_pattern(module, &p.pattern, graph, alias_map, ctx);
                if let Some(from) = ctx {
                    walk_type_or_annotation(
                        module,
                        &p.ty,
                        p.annotation.as_ref(),
                        graph,
                        alias_map,
                        from,
                    );
                }
            }
            walk_expression(module, body, graph, alias_map, ctx);
        }

        Expression::Let {
            binding,
            value,
            else_block,
            ..
        } => {
            walk_pattern(module, &binding.pattern, graph, alias_map, ctx);
            if let Some(from) = ctx {
                walk_type_or_annotation(
                    module,
                    &binding.ty,
                    binding.annotation.as_ref(),
                    graph,
                    alias_map,
                    from,
                );
            }
            walk_expression(module, value, graph, alias_map, ctx);
            if let Some(eb) = else_block {
                walk_expression(module, eb, graph, alias_map, ctx);
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
                walk_annotation(module, annotation, graph, alias_map, from);
            }
            let impl_id = ModuleItemId::new(receiver_name);
            let impl_context = ctx.unwrap_or(&impl_id);
            for g in generics {
                for bound in &g.bounds {
                    walk_annotation(module, bound, graph, alias_map, impl_context);
                }
            }
            for m in methods {
                if let Expression::Function {
                    name,
                    generics,
                    params,
                    return_annotation,
                    body,
                    ..
                } = m
                {
                    let method_ctx = ModuleItemId::method(name, receiver_name);
                    walk_callable_body(
                        module,
                        generics,
                        params,
                        return_annotation,
                        body,
                        graph,
                        alias_map,
                        &method_ctx,
                    );
                } else {
                    walk_expression(module, m, graph, alias_map, ctx);
                }
            }
        }

        Expression::Match { subject, arms, .. } => {
            walk_expression(module, subject, graph, alias_map, ctx);
            for arm in arms {
                walk_pattern(module, &arm.pattern, graph, alias_map, ctx);
                if let Some(g) = &arm.guard {
                    walk_expression(module, g, graph, alias_map, ctx);
                }
                walk_expression(module, &arm.expression, graph, alias_map, ctx);
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            ..
        } => {
            walk_expression(module, scrutinee, graph, alias_map, ctx);
            walk_pattern(module, pattern, graph, alias_map, ctx);
            walk_expression(module, consequence, graph, alias_map, ctx);
            walk_expression(module, alternative, graph, alias_map, ctx);
        }

        Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } => {
            walk_expression(module, scrutinee, graph, alias_map, ctx);
            walk_pattern(module, pattern, graph, alias_map, ctx);
            walk_expression(module, body, graph, alias_map, ctx);
        }

        Expression::For {
            binding,
            iterable,
            body,
            ..
        } => {
            walk_pattern(module, &binding.pattern, graph, alias_map, ctx);
            walk_expression(module, iterable, graph, alias_map, ctx);
            walk_expression(module, body, graph, alias_map, ctx);
        }

        Expression::Select { arms, .. } => {
            walk_select(module, arms, graph, alias_map, ctx);
        }

        Expression::Cast {
            expression,
            target_type,
            ..
        } => {
            if let Some(from) = ctx {
                walk_annotation(module, target_type, graph, alias_map, from);
            }
            walk_expression(module, expression, graph, alias_map, ctx);
        }

        // All remaining expressions: recurse into children.
        _ => {
            for child in expression.children() {
                walk_expression(module, child, graph, alias_map, ctx);
            }
        }
    }
}

fn walk_identifier(
    module: &Module,
    value: &str,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    add_ref(graph, ctx, alias_map, module, extract_base_name(value));
    let mut segments = value.split('.');
    let first = segments.next().unwrap_or("");
    // Handle "Type.method" identifiers (method used as value).
    // The type checker desugars `Type.method` to `Identifier("Type.method")`.
    // Add references to both the type and the method so they aren't
    // falsely flagged as unused.
    if let Some(second) = segments.next()
        && is_upper(first)
    {
        if is_upper(second) {
            graph.mark_enum_variant_used(EnumVariantId::new(first, second));
        }
        add_ref(graph, ctx, alias_map, module, first);
        if let Some(from) = ctx {
            let method_name = value.rsplit('.').next().unwrap_or("");
            graph.add_reference(from, ModuleItemId::method(method_name, first));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_call(
    module: &Module,
    callee: &Expression,
    args: &[Expression],
    spread: &Option<Expression>,
    type_args: &[Annotation],
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    if let Expression::Identifier { value, .. } = callee {
        let mut segments = value.split('.');
        let first = segments.next().unwrap_or("");
        if segments.next().is_some() && is_upper(first) {
            add_ref(graph, ctx, alias_map, module, first);
            if let Some(from) = ctx {
                let method_name = value.rsplit('.').next().unwrap_or("");
                graph.add_reference(from, ModuleItemId::method(method_name, first));
            }
        }
        if let Some(from) = ctx
            && (value.as_str() == "Slice.equals" || value.as_str() == "Map.equals")
            && let Some(receiver) = args.first()
            && is_container_receiver(&receiver.get_type())
        {
            graph.record_equals_dispatch(from.clone(), receiver.get_type());
        }
    }
    walk_expression(module, callee, graph, alias_map, ctx);
    for arg in args {
        walk_expression(module, arg, graph, alias_map, ctx);
    }
    if let Some(spread_expr) = spread {
        walk_expression(module, spread_expr, graph, alias_map, ctx);
    }
    if let Some(from) = ctx {
        for type_arg in type_args {
            walk_annotation(module, type_arg, graph, alias_map, from);
        }
    }
}

fn walk_struct_call(
    module: &Module,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
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
        add_ref(graph, ctx, alias_map, module, p0);
    } else {
        add_ref(graph, ctx, alias_map, module, extract_base_name(name));
    }
    if is_upper(p0) && p1.is_some_and(is_upper) {
        graph.mark_enum_variant_used(EnumVariantId::new(p0, p1.unwrap()));
    } else if p1.is_some_and(is_upper) && p2.is_some_and(is_upper) {
        graph.mark_enum_variant_used(EnumVariantId::new(p1.unwrap(), p2.unwrap()));
    }
    for f in field_assignments {
        walk_expression(module, &f.value, graph, alias_map, ctx);
    }
    match spread {
        StructSpread::None => {}
        StructSpread::From(spread_expression) => {
            walk_expression(module, spread_expression, graph, alias_map, ctx);
            if let Some(ty_name) = type_name(&spread_expression.get_type()) {
                let explicit: HashSet<&str> =
                    field_assignments.iter().map(|f| f.name.as_str()).collect();
                let qname = Symbol::from_parts(&module.id, &ty_name);
                if let Some(def) = module.definitions.get(qname.as_str())
                    && let DefinitionBody::Struct { fields, .. } = &def.body
                {
                    for field in fields {
                        if !explicit.contains(field.name.as_str()) {
                            graph.mark_struct_field_used(StructFieldId::new(&ty_name, &field.name));
                        }
                    }
                }
            }
        }
        StructSpread::ZeroFill { .. } => {
            if let Some(ty_name) = type_name(ty) {
                let explicit: HashSet<&str> =
                    field_assignments.iter().map(|f| f.name.as_str()).collect();
                let qname = Symbol::from_parts(&module.id, &ty_name);
                if let Some(def) = module.definitions.get(qname.as_str())
                    && let DefinitionBody::Struct { fields, .. } = &def.body
                {
                    for field in fields {
                        if !explicit.contains(field.name.as_str()) {
                            graph.mark_struct_field_used(StructFieldId::new(&ty_name, &field.name));
                        }
                    }
                }
            }
        }
    }
}

fn walk_select(
    module: &Module,
    arms: &[SelectArm],
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    for arm in arms {
        match &arm.pattern {
            SelectArmPattern::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => {
                walk_pattern(module, binding, graph, alias_map, ctx);
                walk_expression(module, receive_expression, graph, alias_map, ctx);
                walk_expression(module, body, graph, alias_map, ctx);
            }
            SelectArmPattern::Send {
                send_expression,
                body,
            } => {
                walk_expression(module, send_expression, graph, alias_map, ctx);
                walk_expression(module, body, graph, alias_map, ctx);
            }
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms: match_arms,
            } => {
                walk_expression(module, receive_expression, graph, alias_map, ctx);
                for match_arm in match_arms {
                    walk_pattern(module, &match_arm.pattern, graph, alias_map, ctx);
                    walk_expression(module, &match_arm.expression, graph, alias_map, ctx);
                }
            }
            SelectArmPattern::WildCard { body } => {
                walk_expression(module, body, graph, alias_map, ctx);
            }
        }
    }
}

fn walk_pattern(
    module: &Module,
    pattern: &Pattern,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    match pattern {
        Pattern::EnumVariant {
            identifier,
            fields,
            ty,
            ..
        } => {
            mark_constructor_pattern(module, identifier, ty, graph, alias_map, ctx);
            for f in fields {
                walk_pattern(module, f, graph, alias_map, ctx);
            }
        }
        Pattern::Struct {
            identifier,
            fields,
            ty,
            ..
        } => {
            mark_constructor_pattern(module, identifier, ty, graph, alias_map, ctx);
            for f in fields {
                walk_pattern(module, &f.value, graph, alias_map, ctx);
                graph.mark_struct_field_used(StructFieldId::new(identifier, &f.name));
            }
        }
        Pattern::Tuple { elements, .. } => {
            for e in elements {
                walk_pattern(module, e, graph, alias_map, ctx);
            }
        }
        Pattern::Slice { prefix, .. } => {
            for p in prefix {
                walk_pattern(module, p, graph, alias_map, ctx);
            }
        }
        Pattern::Or { patterns, .. } => {
            for p in patterns {
                walk_pattern(module, p, graph, alias_map, ctx);
            }
        }
        Pattern::AsBinding { pattern, .. } => {
            walk_pattern(module, pattern, graph, alias_map, ctx);
        }
        Pattern::Literal { .. }
        | Pattern::Identifier { .. }
        | Pattern::WildCard { .. }
        | Pattern::Unit { .. } => {}
    }
}

fn walk_type_or_annotation(
    module: &Module,
    ty: &Type,
    annotation: Option<&Annotation>,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &ModuleItemId,
) {
    if let Some(a) = annotation {
        walk_annotation(module, a, graph, alias_map, from);
    } else {
        walk_type(module, ty, graph, alias_map, from);
    }
}

fn walk_annotation(
    module: &Module,
    ann: &Annotation,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &ModuleItemId,
) {
    match ann {
        Annotation::Constructor { name, params, .. } => {
            // For qualified names like "models.Item", extract the import alias "models"
            let base_name = extract_base_name(name);
            if let Some(to) = alias_map.resolve(module, base_name) {
                graph.add_reference(from, to);
            }
            for p in params {
                walk_annotation(module, p, graph, alias_map, from);
            }
        }
        Annotation::Function {
            params,
            return_type,
            ..
        } => {
            for p in params {
                walk_annotation(module, p, graph, alias_map, from);
            }
            walk_annotation(module, return_type, graph, alias_map, from);
        }
        Annotation::Tuple { elements, .. } => {
            for e in elements {
                walk_annotation(module, e, graph, alias_map, from);
            }
        }
        Annotation::Unknown | Annotation::Opaque { .. } => {}
    }
}

fn walk_type(
    module: &Module,
    ty: &Type,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    from: &ModuleItemId,
) {
    match ty {
        Type::Nominal { id, params, .. } => {
            // Type IDs from the current module are stored qualified (e.g. "_entry_.Greeter").
            // Strip the module prefix so extract_base_name sees the local name, not the
            // module id — otherwise "module.Type" is misread as "import_alias.Type" and
            // the reference is lost.
            let module_prefix = format!("{}.", module.id);
            let local_id = id.strip_prefix(&module_prefix).unwrap_or(id);
            let base_name = extract_base_name(local_id);
            if let Some(to) = alias_map.resolve(module, base_name) {
                graph.add_reference(from, to);
            }
            for p in params {
                walk_type(module, p, graph, alias_map, from);
            }
        }
        Type::Function(f) => {
            for p in &f.params {
                walk_type(module, p, graph, alias_map, from);
            }
            walk_type(module, &f.return_type, graph, alias_map, from);
        }
        Type::Forall { body, .. } => walk_type(module, body, graph, alias_map, from),
        Type::Tuple(elems) => {
            for e in elems {
                walk_type(module, e, graph, alias_map, from);
            }
        }
        Type::Compound { args, .. } => {
            for a in args {
                walk_type(module, a, graph, alias_map, from);
            }
        }
        Type::Simple(_)
        | Type::Var { .. }
        | Type::Parameter(_)
        | Type::Never
        | Type::Error
        | Type::ImportNamespace(_)
        | Type::ReceiverPlaceholder => {}
    }
}

fn add_ref(
    graph: &mut ReferenceGraph,
    ctx: Option<&ModuleItemId>,
    alias_map: &AliasMap,
    module: &Module,
    name: &str,
) {
    if let Some(from) = ctx
        && let Some(to) = alias_map.resolve(module, name)
    {
        graph.add_reference(from, to);
    }
}

fn mark_constructor_pattern(
    module: &Module,
    identifier: &str,
    ty: &Type,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    if let Some((alias, _)) = identifier.split_once('.')
        && alias_map.is_import_alias(alias)
    {
        add_ref(graph, ctx, alias_map, module, alias);
        return;
    }

    let enum_name = type_name(ty).or_else(|| {
        identifier
            .split_once('.')
            .map(|(first, _)| first.to_string())
    });
    if let Some(enum_name) = enum_name {
        add_ref(graph, ctx, alias_map, module, &enum_name);
        graph.mark_enum_variant_used(EnumVariantId::new(&enum_name, unqualified_name(identifier)));
    }
}

fn is_method_access(kind: &Option<DotAccessKind>) -> bool {
    matches!(
        kind,
        Some(
            DotAccessKind::InstanceMethod { .. }
                | DotAccessKind::InstanceMethodValue { .. }
                | DotAccessKind::StaticMethod { .. }
        )
    )
}

#[allow(clippy::too_many_arguments)]
fn walk_callable_body(
    module: &Module,
    generics: &[Generic],
    params: &[Binding],
    return_annotation: &Annotation,
    body: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    fn_ctx: &ModuleItemId,
) {
    for g in generics {
        for bound in &g.bounds {
            walk_annotation(module, bound, graph, alias_map, fn_ctx);
        }
    }
    for p in params {
        walk_pattern(module, &p.pattern, graph, alias_map, Some(fn_ctx));
        walk_type_or_annotation(
            module,
            &p.ty,
            p.annotation.as_ref(),
            graph,
            alias_map,
            fn_ctx,
        );
    }
    walk_annotation(module, return_annotation, graph, alias_map, fn_ctx);
    walk_expression(module, body, graph, alias_map, Some(fn_ctx));
}

/// The graph node for a `member` method call on `receiver_ty`, resolving the receiver to
/// its unqualified type name so `equals` is keyed per receiver type.
fn method_node(member: &str, receiver_ty: &Type) -> ModuleItemId {
    match type_name(receiver_ty) {
        Some(name) => ModuleItemId::method(member, &name),
        None => ModuleItemId::new(member),
    }
}

fn credits_local_method(receiver_ty: &Type, module: &Module) -> bool {
    let mut current = receiver_ty.strip_refs();
    while let Some(next) = current.get_underlying().cloned() {
        current = next;
    }
    current = match current {
        Type::Function(f) => (*f.return_type).clone(),
        other => other,
    };
    match current {
        Type::Nominal { id, .. } => id.as_str().starts_with(&format!("{}.", module.id)),
        _ => false,
    }
}

fn has_equality_attr(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "equality")
}

pub(super) fn equals_targets(
    ty: &Type,
    module: &Module,
    index: &EqualityIndex,
    out: &mut Vec<ModuleItemId>,
) {
    let mut current = ty.clone();
    while let Some(next) = current.get_underlying().cloned() {
        current = next;
    }
    match &current {
        Type::Compound {
            kind: CompoundKind::Slice,
            args,
        } => {
            if let Some(element) = args.first() {
                equals_targets(element, module, index, out);
            }
        }
        Type::Compound {
            kind: CompoundKind::Map,
            args,
        } => {
            if let Some(value) = args.get(1) {
                equals_targets(value, module, index, out);
            }
        }
        Type::Nominal { id, .. } => {
            if id.as_str().starts_with(&format!("{}.", module.id))
                && index.usable_from(id.as_str(), &module.id)
            {
                out.push(ModuleItemId::equals_method(unqualified_name(id)));
            }
        }
        _ => {}
    }
}

fn is_container_receiver(receiver_ty: &Type) -> bool {
    let mut current = receiver_ty.strip_refs();
    while let Some(next) = current.get_underlying().cloned() {
        current = next;
    }
    current.is_slice() || current.is_map()
}

fn type_name(ty: &Type) -> Option<String> {
    let mut current = ty.strip_refs();
    while let Some(next) = current.get_underlying().cloned() {
        current = next;
    }
    match current {
        Type::Nominal { id, .. } => Some(unqualified_name(&id).to_string()),
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
        return p0; // 1 part
    };
    let Some(_p2) = segments.next() else {
        return if is_upper(p1) { p0 } else { p1 }; // 2 parts
    };
    if segments.next().is_none() {
        return p1; // 3 parts
    }
    // 4+ parts: first uppercase segment, else the last segment.
    name.split('.')
        .find(|p| is_upper(p))
        .or_else(|| name.split('.').next_back())
        .unwrap_or("")
}
