use rustc_hash::FxHashSet as HashSet;

use crate::EmitEffects;
use crate::Planner;
use crate::names::constraints::{GenericConstraintTable, classify_bound_annotation};
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Annotation, Binding, Expression, Generic, Pattern, Visibility};
use syntax::program::{DefinitionBody, File, Interface};
use syntax::types::{CompoundKind, Symbol, Type, unqualified_name};

impl Planner<'_> {
    pub(crate) fn collect_generic_constraints(&mut self, files: &[&File], fx: &mut EmitEffects) {
        let mut table = GenericConstraintTable::default();

        self.seed_global_definitions(&mut table);
        self.seed_local_functions(files, &mut table);
        self.seed_local_impl_blocks(files, &mut table, fx);

        self.for_each_definition_type(|key, names, ty| {
            collect_demands_from_type(ty, key, names, &mut table);
        });
        self.collect_demands_from_local_functions(files, &mut table);
        self.collect_demands_from_local_impl_blocks(files, &mut table);

        self.propagate_constraints(&mut table);

        self.module.set_generic_constraints(table);
    }

    fn seed_global_definitions(&self, table: &mut GenericConstraintTable) {
        for (id, definition) in self.facts.iter_definitions() {
            let key = id.as_str();
            match &definition.body {
                DefinitionBody::Struct { generics, .. }
                | DefinitionBody::Enum { generics, .. }
                | DefinitionBody::TypeAlias { generics, .. } => {
                    table.ensure_seeded(key, generics);
                }
                DefinitionBody::Interface {
                    definition: Interface { name, generics, .. },
                    ..
                } => {
                    // Go has no F-bound syntax; strip `T: Cloner<T>`-style bounds.
                    let filtered = strip_self_referential_bounds(generics, name);
                    table.ensure_seeded(key, &filtered);
                }
                DefinitionBody::Value { .. } | DefinitionBody::ValueEnum { .. } => {}
            }
        }
    }

    fn seed_local_functions(&self, files: &[&File], table: &mut GenericConstraintTable) {
        for file in files {
            for item in &file.items {
                if let Expression::Function { name, generics, .. } = item {
                    let key = self.facts.qualified_current(name);
                    table.ensure_seeded(&key, generics);
                }
            }
        }
    }

    fn seed_local_impl_blocks(
        &mut self,
        files: &[&File],
        table: &mut GenericConstraintTable,
        fx: &mut EmitEffects,
    ) {
        let impl_generics_lists: Vec<Vec<Generic>> = files
            .iter()
            .flat_map(|f| &f.items)
            .filter_map(|item| match item {
                Expression::ImplBlock {
                    generics: impl_generics,
                    ..
                } => Some(impl_generics.clone()),
                _ => None,
            })
            .collect();
        for impl_generics in &impl_generics_lists {
            self.record_bound_imports(impl_generics, fx);
        }

        for file in files {
            for item in &file.items {
                let Expression::ImplBlock {
                    receiver_name,
                    generics: impl_generics,
                    methods,
                    ..
                } = item
                else {
                    continue;
                };
                let receiver_key = self.facts.qualified_current(receiver_name);
                for method in methods {
                    let Expression::Function {
                        generics: method_generics,
                        ..
                    } = method
                    else {
                        continue;
                    };
                    let layout = self.impl_method_emission_layout(
                        &receiver_key,
                        receiver_name,
                        impl_generics,
                        method,
                    );
                    match layout {
                        ImplMethodLayout::Receiver { method_key } => {
                            for impl_g in impl_generics {
                                for bound in &impl_g.bounds {
                                    table.add_explicit(
                                        &receiver_key,
                                        impl_g.name.as_str(),
                                        classify_bound_annotation(bound),
                                    );
                                }
                            }
                            table.ensure_seeded(&method_key, method_generics);
                        }
                        ImplMethodLayout::FreeFunction {
                            free_fn_key,
                            combined_generics,
                        } => {
                            table.ensure_seeded(&free_fn_key, &combined_generics);
                        }
                    }
                }
            }
        }
    }

    fn impl_method_emission_layout(
        &self,
        receiver_key: &str,
        receiver_name: &str,
        impl_generics: &[Generic],
        method: &Expression,
    ) -> ImplMethodLayout {
        let Expression::Function {
            name: method_name,
            generics: method_generics,
            visibility,
            ..
        } = method
        else {
            unreachable!("callers filter to Function expressions");
        };

        let function = method.to_function_definition();
        let has_self = function.params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
        });
        let is_ufcs = self.facts.is_ufcs_method(receiver_key, method_name);
        if has_self && !is_ufcs {
            return ImplMethodLayout::Receiver {
                method_key: self
                    .facts
                    .qualified_current_member(receiver_name, method_name),
            };
        }

        let is_public = matches!(visibility, Visibility::Public);
        let should_export = is_public || self.method_needs_export(method_name);
        let exported_method_name = if should_export {
            go_name::snake_to_camel(method_name)
        } else {
            method_name.to_string()
        };
        let free_fn_name = format!("{}_{}", receiver_name, exported_method_name);
        let free_fn_key = self.facts.qualified_current(&free_fn_name);

        let mut combined_generics = impl_generics.to_vec();
        combined_generics.extend(method_generics.iter().cloned());

        ImplMethodLayout::FreeFunction {
            free_fn_key,
            combined_generics,
        }
    }

    fn for_each_definition_type<F>(&self, mut visit: F)
    where
        F: FnMut(&str, &[&str], &Type),
    {
        for (id, definition) in self.facts.iter_definitions() {
            let key = id.as_str();
            match &definition.body {
                DefinitionBody::Struct {
                    generics, fields, ..
                } => {
                    let names = generic_names(generics);
                    for f in fields {
                        visit(key, &names, &f.ty);
                    }
                }
                DefinitionBody::Enum {
                    generics, variants, ..
                } => {
                    let names = generic_names(generics);
                    for v in variants {
                        for f in v.fields.iter() {
                            visit(key, &names, &f.ty);
                        }
                    }
                }
                DefinitionBody::TypeAlias { generics, .. } => {
                    let names = generic_names(generics);
                    let body = definition.ty.unwrap_forall();
                    visit(key, &names, body);
                }
                DefinitionBody::Interface {
                    definition: iface, ..
                } => {
                    let names = generic_names(&iface.generics);
                    for method_ty in iface.methods.values() {
                        visit(key, &names, method_ty);
                    }
                    for parent in &iface.parents {
                        visit(key, &names, parent);
                    }
                }
                DefinitionBody::Value { .. } | DefinitionBody::ValueEnum { .. } => {}
            }
        }
    }

    fn collect_demands_from_local_functions(
        &self,
        files: &[&File],
        table: &mut GenericConstraintTable,
    ) {
        for file in files {
            for item in &file.items {
                let Expression::Function {
                    name,
                    generics,
                    params,
                    return_type,
                    body,
                    ..
                } = item
                else {
                    continue;
                };
                let key = self.facts.qualified_current(name);
                let names = generic_names(generics);
                for p in params {
                    collect_demands_from_type(&p.ty, &key, &names, table);
                }
                collect_demands_from_type(return_type, &key, &names, table);
                self.collect_demands_from_expression(body, &key, &names, table);
            }
        }
    }

    fn collect_demands_from_local_impl_blocks(
        &self,
        files: &[&File],
        table: &mut GenericConstraintTable,
    ) {
        for file in files {
            for item in &file.items {
                let Expression::ImplBlock {
                    receiver_name,
                    generics: impl_generics,
                    methods,
                    ..
                } = item
                else {
                    continue;
                };
                let receiver_key = self.facts.qualified_current(receiver_name);
                let receiver_names = generic_names(impl_generics);

                for method in methods {
                    let Expression::Function {
                        generics: method_generics,
                        params,
                        return_type,
                        body,
                        ..
                    } = method
                    else {
                        continue;
                    };
                    let layout = self.impl_method_emission_layout(
                        &receiver_key,
                        receiver_name,
                        impl_generics,
                        method,
                    );
                    match layout {
                        ImplMethodLayout::Receiver { method_key } => {
                            let method_names = generic_names(method_generics);
                            self.collect_signature_demands(
                                params,
                                return_type,
                                body,
                                &receiver_key,
                                &receiver_names,
                                table,
                            );
                            self.collect_signature_demands(
                                params,
                                return_type,
                                body,
                                &method_key,
                                &method_names,
                                table,
                            );
                        }
                        ImplMethodLayout::FreeFunction {
                            free_fn_key,
                            combined_generics,
                        } => {
                            let combined_names = generic_names(&combined_generics);
                            self.collect_signature_demands(
                                params,
                                return_type,
                                body,
                                &free_fn_key,
                                &combined_names,
                                table,
                            );
                        }
                    }
                }
            }
        }
    }

    /// Collect generic demands from a function signature (params, return type,
    /// body) against one `symbol`/`local_generics` pair.
    fn collect_signature_demands(
        &self,
        params: &[Binding],
        return_type: &Type,
        body: &Expression,
        symbol: &str,
        local_generics: &[&str],
        table: &mut GenericConstraintTable,
    ) {
        for p in params {
            collect_demands_from_type(&p.ty, symbol, local_generics, table);
        }
        collect_demands_from_type(return_type, symbol, local_generics, table);
        self.collect_demands_from_expression(body, symbol, local_generics, table);
    }

    fn collect_demands_from_expression(
        &self,
        expression: &Expression,
        symbol: &str,
        local_generics: &[&str],
        table: &mut GenericConstraintTable,
    ) {
        // `get_type()` catches `Map.new<T, int>()` inside a non-map context
        // like `.is_empty()` or an if-condition.
        collect_demands_from_type(&expression.get_type(), symbol, local_generics, table);
        // Let bindings carry an explicit type that `children()` does not walk.
        if let Expression::Let { binding, .. } = expression {
            collect_demands_from_type(&binding.ty, symbol, local_generics, table);
        }
        for child in expression.children() {
            self.collect_demands_from_expression(child, symbol, local_generics, table);
        }
    }

    fn propagate_constraints(&self, table: &mut GenericConstraintTable) {
        loop {
            let edges = self.collect_propagation_edges(table);
            let mut changed = false;
            for edge in edges {
                if table.mark_inferred_comparable(&edge.from_symbol, &edge.from_param) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn collect_propagation_edges(&self, table: &GenericConstraintTable) -> Vec<PropagationEdge> {
        let mut edges = Vec::new();
        self.for_each_definition_type(|key, names, ty| {
            scan_propagation(ty, key, names, table, &mut edges);
        });
        edges
    }
}

struct PropagationEdge {
    from_symbol: String,
    from_param: EcoString,
}

enum ImplMethodLayout {
    Receiver {
        method_key: String,
    },
    FreeFunction {
        free_fn_key: String,
        combined_generics: Vec<Generic>,
    },
}

fn collect_demands_from_type(
    ty: &Type,
    symbol: &str,
    local_generics: &[&str],
    table: &mut GenericConstraintTable,
) {
    let mut stack: Vec<&Type> = vec![ty];
    while let Some(current) = stack.pop() {
        if let Some(key_ty) = map_key_of(current)
            && let Type::Parameter(name) = key_ty
            && local_generics.contains(&name.as_str())
        {
            table.mark_inferred_comparable(symbol, name);
        }
        for child in current.children() {
            stack.push(child);
        }
        // `Type::children()` excludes function bounds; include them so bound
        // type expressions can also impose constraints.
        if let Type::Function(f) = current {
            for b in &f.bounds {
                stack.push(&b.ty);
            }
        }
    }
}

fn scan_propagation(
    ty: &Type,
    symbol: &str,
    local_generics: &[&str],
    table: &GenericConstraintTable,
    edges: &mut Vec<PropagationEdge>,
) {
    let mut stack: Vec<&Type> = vec![ty];
    let mut visited_nominals: HashSet<Symbol> = HashSet::default();

    while let Some(current) = stack.pop() {
        if let Type::Nominal { id, params, .. } = current {
            // Direct `Map<P, _>` is already handled by the initial collection;
            // here we only chase nominal callees with constrained positions.
            if !is_map_id(id)
                && let Some(callee_sets) = table.get(id.as_str())
            {
                for (i, set) in callee_sets.iter().enumerate() {
                    if !set.requires_comparable() {
                        continue;
                    }
                    let Some(arg) = params.get(i) else { continue };
                    if let Type::Parameter(name) = arg
                        && local_generics.contains(&name.as_str())
                    {
                        edges.push(PropagationEdge {
                            from_symbol: symbol.to_string(),
                            from_param: name.clone(),
                        });
                    }
                }
            }
            // Skip re-entry into the same nominal in chains like `Alias<Alias<T>>`.
            if visited_nominals.insert(id.clone()) {
                for p in params {
                    stack.push(p);
                }
            }
            continue;
        }
        for child in current.children() {
            stack.push(child);
        }
        if let Type::Function(f) = current {
            for b in &f.bounds {
                stack.push(&b.ty);
            }
        }
    }
}

fn map_key_of(ty: &Type) -> Option<&Type> {
    if let Type::Compound {
        kind: CompoundKind::Map,
        args,
    } = ty
        && !args.is_empty()
    {
        return Some(&args[0]);
    }
    if let Type::Nominal { id, params, .. } = ty
        && is_map_id(id)
        && !params.is_empty()
    {
        return Some(&params[0]);
    }
    None
}

fn is_map_id(id: &Symbol) -> bool {
    let s = id.as_str();
    s == "Map" || s.ends_with(".Map")
}

fn generic_names(generics: &[Generic]) -> Vec<&str> {
    generics.iter().map(|g| g.name.as_str()).collect()
}

fn strip_self_referential_bounds(generics: &[Generic], interface_name: &str) -> Vec<Generic> {
    generics
        .iter()
        .map(|g| Generic {
            name: g.name.clone(),
            bounds: g
                .bounds
                .iter()
                .filter(|ann| !bound_references_interface(ann, interface_name))
                .cloned()
                .collect(),
            span: g.span,
        })
        .collect()
}

fn bound_references_interface(annotation: &Annotation, interface_name: &str) -> bool {
    let Annotation::Constructor { name, .. } = annotation else {
        return false;
    };
    unqualified_name(name) == interface_name
}
