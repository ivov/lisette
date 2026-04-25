use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{
    Annotation, Expression, ImportAlias, Pattern, SelectArm, SelectArmPattern,
    StructFieldAssignment,
};
use syntax::program::File;
use syntax::program::{Definition, Module};
use syntax::types::{Symbol, Type};

use super::reference_graph::{EnumVariantId, ModuleItemId, ReferenceGraph, StructFieldId};

pub struct AliasMap {
    aliases: HashMap<String, ModuleItemId>,
}

impl AliasMap {
    pub fn build(
        module: &Module,
        files: &HashMap<u32, File>,
        go_package_names: &HashMap<String, String>,
    ) -> Self {
        let mut aliases = HashMap::default();

        for file in files.values() {
            for import in file.imports() {
                if matches!(import.alias, Some(ImportAlias::Blank(_))) {
                    continue;
                }
                if let Some(effective) = import.effective_alias(go_package_names) {
                    aliases.insert(effective.clone(), ModuleItemId::new(&module.id, &effective));
                }
            }
        }

        Self { aliases }
    }

    fn resolve(&self, module: &Module, name: &str) -> Option<ModuleItemId> {
        let qualified_name = Symbol::from_parts(&module.id, name);
        if module.definitions.contains_key(qualified_name.as_str()) {
            return Some(ModuleItemId::new(&module.id, name));
        }
        self.aliases.get(name).cloned()
    }
}

pub fn extract_references(
    module: &Module,
    expression: &Expression,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
) {
    let ctx = match expression {
        Expression::Function { name, .. } => Some(ModuleItemId::new(&module.id, name)),
        Expression::Const { identifier, .. } => Some(ModuleItemId::new(&module.id, identifier)),
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

        Expression::StructCall {
            name,
            field_assignments,
            spread,
            ..
        } => {
            walk_struct_call(
                module,
                name,
                field_assignments,
                spread,
                graph,
                alias_map,
                ctx,
            );
        }

        Expression::DotAccess {
            expression, member, ..
        } => {
            walk_expression(module, expression, graph, alias_map, ctx);
            if let Some(ty_name) = type_name(&expression.get_type()) {
                graph.mark_struct_field_used(StructFieldId::new(&ty_name, member));
            }
            // Also track method references: when calling Type.method() or instance.method(),
            // add a reference to the method if it exists in the module.
            // The add_reference is a no-op if the target doesn't exist.
            if let Some(from) = ctx {
                let method_id = ModuleItemId::new(&module.id, member);
                graph.add_reference(from, method_id);
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
            let fn_ctx = ModuleItemId::new(&module.id, name);
            for g in generics {
                for bound in &g.bounds {
                    walk_annotation(module, bound, graph, alias_map, &fn_ctx);
                }
            }
            for p in params {
                walk_pattern(module, &p.pattern, graph, alias_map, Some(&fn_ctx));
                walk_type_or_annotation(
                    module,
                    &p.ty,
                    p.annotation.as_ref(),
                    graph,
                    alias_map,
                    &fn_ctx,
                );
            }
            walk_annotation(module, return_annotation, graph, alias_map, &fn_ctx);
            walk_expression(module, body, graph, alias_map, Some(&fn_ctx));
        }

        Expression::Const {
            identifier,
            annotation,
            expression,
            ..
        } => {
            let const_ctx = ModuleItemId::new(&module.id, identifier);
            if let Some(ann) = annotation {
                walk_annotation(module, ann, graph, alias_map, &const_ctx);
            }
            walk_expression(module, expression, graph, alias_map, Some(&const_ctx));
        }

        Expression::Enum { name, variants, .. } => {
            let enum_ctx = ModuleItemId::new(&module.id, name);
            for v in variants {
                for f in &v.fields {
                    walk_annotation(module, &f.annotation, graph, alias_map, &enum_ctx);
                }
            }
        }

        Expression::Struct {
            name,
            generics,
            fields,
            ..
        } => {
            let struct_ctx = ModuleItemId::new(&module.id, name);
            for g in generics {
                for bound in &g.bounds {
                    walk_annotation(module, bound, graph, alias_map, &struct_ctx);
                }
            }
            for f in fields {
                walk_annotation(module, &f.annotation, graph, alias_map, &struct_ctx);
            }
        }

        Expression::TypeAlias {
            name, annotation, ..
        } => {
            let alias_ctx = ModuleItemId::new(&module.id, name);
            walk_annotation(module, annotation, graph, alias_map, &alias_ctx);
        }

        Expression::Interface {
            name,
            method_signatures,
            parents,
            ..
        } => {
            let iface_ctx = ModuleItemId::new(&module.id, name);
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
            let impl_id = ModuleItemId::new(&module.id, receiver_name);
            let impl_context = ctx.unwrap_or(&impl_id);
            for g in generics {
                for bound in &g.bounds {
                    walk_annotation(module, bound, graph, alias_map, impl_context);
                }
            }
            for m in methods {
                walk_expression(module, m, graph, alias_map, ctx);
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
    add_ref(graph, ctx, alias_map, module, &extract_base_name(value));
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() >= 2 && is_upper(parts[0]) && is_upper(parts[1]) {
        graph.mark_enum_variant_used(EnumVariantId::new(parts[0], parts[1]));
    }
    // Handle "Type.method" identifiers (method used as value).
    // The type checker desugars `Type.method` to `Identifier("Type.method")`.
    // Add references to both the type and the method so they aren't
    // falsely flagged as unused.
    if parts.len() >= 2 && is_upper(parts[0]) {
        add_ref(graph, ctx, alias_map, module, parts[0]);
        if let Some(from) = ctx {
            let method_name = parts.last().unwrap_or(&"");
            let method_id = ModuleItemId::new(&module.id, method_name);
            graph.add_reference(from, method_id);
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
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() >= 2 && is_upper(parts[0]) {
            add_ref(graph, ctx, alias_map, module, parts[0]);
            if let Some(from) = ctx {
                let method_name = parts.last().unwrap_or(&"");
                let method_id = ModuleItemId::new(&module.id, method_name);
                graph.add_reference(from, method_id);
            }
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
    name: &str,
    field_assignments: &[StructFieldAssignment],
    spread: &Option<Expression>,
    graph: &mut ReferenceGraph,
    alias_map: &AliasMap,
    ctx: Option<&ModuleItemId>,
) {
    let parts: Vec<&str> = name.split('.').collect();
    if !parts.is_empty() && !is_upper(parts[0]) {
        add_ref(graph, ctx, alias_map, module, parts[0]);
    } else {
        add_ref(graph, ctx, alias_map, module, &extract_base_name(name));
    }
    if parts.len() >= 2 && is_upper(parts[0]) && is_upper(parts[1]) {
        graph.mark_enum_variant_used(EnumVariantId::new(parts[0], parts[1]));
    } else if parts.len() >= 3 && is_upper(parts[1]) && is_upper(parts[2]) {
        graph.mark_enum_variant_used(EnumVariantId::new(parts[1], parts[2]));
    }
    for f in field_assignments {
        walk_expression(module, &f.value, graph, alias_map, ctx);
    }
    if let Some(spread_expression) = spread.as_ref() {
        walk_expression(module, spread_expression, graph, alias_map, ctx);
        if let Some(ty_name) = type_name(&spread_expression.get_type()) {
            let explicit: HashSet<&str> =
                field_assignments.iter().map(|f| f.name.as_str()).collect();
            let qname = Symbol::from_parts(&module.id, &ty_name);
            if let Some(Definition::Struct { fields, .. }) = module.definitions.get(qname.as_str())
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
            let variant_name = identifier.split('.').next_back().unwrap_or(identifier);

            let enum_name = type_name(ty).or_else(|| {
                let parts: Vec<&str> = identifier.split('.').collect();
                (parts.len() >= 2).then(|| parts[0].to_string())
            });

            if let Some(ref enum_name) = enum_name {
                add_ref(graph, ctx, alias_map, module, enum_name);
                graph.mark_enum_variant_used(EnumVariantId::new(enum_name, variant_name));
            }

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
            add_ref(graph, ctx, alias_map, module, identifier);
            // Mark enum variant as used for struct variant patterns (e.g., Enum.Variant { ... })
            let variant_name = identifier.split('.').next_back().unwrap_or(identifier);
            let enum_name = type_name(ty).or_else(|| {
                let parts: Vec<&str> = identifier.split('.').collect();
                (parts.len() >= 2).then(|| parts[0].to_string())
            });
            if let Some(ref enum_name) = enum_name {
                graph.mark_enum_variant_used(EnumVariantId::new(enum_name, variant_name));
            }
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
            if let Some(to) = alias_map.resolve(module, &base_name) {
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
            if let Some(to) = alias_map.resolve(module, &base_name) {
                graph.add_reference(from, to);
            }
            for p in params {
                walk_type(module, p, graph, alias_map, from);
            }
        }
        Type::Function {
            params,
            return_type,
            ..
        } => {
            for p in params {
                walk_type(module, p, graph, alias_map, from);
            }
            walk_type(module, return_type, graph, alias_map, from);
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

fn type_name(ty: &Type) -> Option<String> {
    let mut current = ty.strip_refs();
    while let Some(next) = current.get_underlying().cloned() {
        current = next;
    }
    match current {
        Type::Nominal { id, .. } => id.split('.').next_back().map(String::from),
        _ => None,
    }
}

pub(super) fn is_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

fn extract_base_name(name: &str) -> String {
    let parts: Vec<&str> = name.split('.').collect();
    match parts.len() {
        1 => parts[0].to_string(),
        2 if is_upper(parts[1]) => parts[0].to_string(),
        2 => parts[1].to_string(),
        3 => parts[1].to_string(),
        _ => parts
            .iter()
            .find(|p| is_upper(p))
            .map(|s| s.to_string())
            .unwrap_or_else(|| parts.last().unwrap_or(&"").to_string()),
    }
}
