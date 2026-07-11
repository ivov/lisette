use std::collections::VecDeque;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use semantics::store::Store;
use syntax::EcoString;
use syntax::ast::{Annotation, Binding, Expression, Generic, Pattern};
use syntax::program::{
    Definition, DefinitionBody, GenericConstraint, GenericConstraints,
    GenericConstraintsByDefinition, Interface, UnusedInfo,
};
use syntax::types::{
    CompoundKind, Symbol, Type, build_substitution_map, substitute, unqualified_name,
};

pub fn collect_generic_constraints(
    store: &Store,
    ufcs_methods: &HashSet<(String, String)>,
    unused: &UnusedInfo,
) -> GenericConstraintsByDefinition {
    let collector = GenericConstraintCollector {
        store,
        ufcs_methods,
        unused,
    };
    let mut table = GenericConstraintTable::default();
    collector.seed_global_definitions(&mut table);
    collector.for_each_definition_type(|key, generics, ty| {
        collect_demands_from_type(ty, key, generics, &mut table, store);
    });
    collector.propagate_constraints(&mut table);
    collector.collect_local_definitions(&mut table);
    table.by_definition
}

struct GenericConstraintCollector<'a> {
    store: &'a Store,
    ufcs_methods: &'a HashSet<(String, String)>,
    unused: &'a UnusedInfo,
}

impl GenericConstraintCollector<'_> {
    fn definitions(&self) -> impl Iterator<Item = (&Symbol, &Definition)> {
        self.store
            .modules
            .values()
            .flat_map(|module| module.definitions.iter())
    }

    fn seed_global_definitions(&self, table: &mut GenericConstraintTable) {
        for (id, definition) in self.definitions() {
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
                DefinitionBody::Value { .. } => {}
            }
        }
    }

    fn collect_local_definitions(&self, table: &mut GenericConstraintTable) {
        for (module_id, module) in &self.store.modules {
            if module.is_internal() {
                continue;
            }
            for item in module.files.values().flat_map(|file| &file.items) {
                match item {
                    Expression::Function {
                        name,
                        generics,
                        params,
                        return_type,
                        body,
                        ..
                    } => {
                        let key = Symbol::from_parts(module_id, name);
                        table.ensure_seeded(key.as_str(), generics);
                        self.collect_signature_demands(
                            params,
                            return_type,
                            body,
                            key.as_str(),
                            generics,
                            table,
                        );
                    }
                    Expression::ImplBlock {
                        annotation,
                        receiver_name,
                        generics: impl_generics,
                        methods,
                        ..
                    } => self.collect_impl_block(
                        module_id,
                        annotation,
                        receiver_name,
                        impl_generics,
                        methods,
                        table,
                    ),
                    _ => {}
                }
            }
        }
    }

    fn collect_impl_block(
        &self,
        module_id: &str,
        annotation: &Annotation,
        receiver_name: &str,
        impl_generics: &[Generic],
        methods: &[Expression],
        table: &mut GenericConstraintTable,
    ) {
        let receiver_key = Symbol::from_parts(module_id, receiver_name);
        for method in methods {
            let Expression::Function {
                name: method_name,
                generics: method_generics,
                name_span: method_name_span,
                params,
                return_type,
                body,
                ..
            } = method
            else {
                continue;
            };
            let method_key = receiver_key.with_segment(method_name);
            if self.is_receiver_method(receiver_key.as_str(), method) {
                if !self.unused.is_unused_definition(method_name_span) {
                    self.hoist_impl_bounds_onto_receiver(
                        receiver_key.as_str(),
                        annotation,
                        impl_generics,
                        table,
                    );
                }
                table.ensure_seeded(method_key.as_str(), method_generics);
                self.collect_signature_demands(
                    params,
                    return_type,
                    body,
                    receiver_key.as_str(),
                    impl_generics,
                    table,
                );
                self.collect_signature_demands(
                    params,
                    return_type,
                    body,
                    method_key.as_str(),
                    method_generics,
                    table,
                );
            } else {
                let mut combined_generics = impl_generics.to_vec();
                combined_generics.extend(method_generics.iter().cloned());
                table.ensure_seeded(method_key.as_str(), &combined_generics);
                self.collect_signature_demands(
                    params,
                    return_type,
                    body,
                    method_key.as_str(),
                    &combined_generics,
                    table,
                );
            }
        }
    }

    fn hoist_impl_bounds_onto_receiver(
        &self,
        receiver_key: &str,
        receiver_annotation: &Annotation,
        impl_generics: &[Generic],
        table: &mut GenericConstraintTable,
    ) {
        if impl_generics.is_empty() {
            return;
        }
        let type_generics = match self.store.get_definition(receiver_key).map(|d| &d.body) {
            Some(
                DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. },
            ) => generics,
            _ => return,
        };
        let Annotation::Constructor {
            params: receiver_args,
            ..
        } = receiver_annotation
        else {
            return;
        };
        for impl_generic in impl_generics {
            let position = receiver_args.iter().position(|arg| {
                matches!(arg, Annotation::Constructor { name, params, .. }
                    if params.is_empty() && name == &impl_generic.name)
            });
            let Some(type_parameter) = position.and_then(|position| type_generics.get(position))
            else {
                continue;
            };
            for bound in &impl_generic.bounds {
                table.add_explicit(
                    receiver_key,
                    type_parameter.name.as_str(),
                    GenericConstraint::from_annotation(bound),
                );
            }
        }
    }

    fn is_receiver_method(&self, receiver_key: &str, method: &Expression) -> bool {
        let Expression::Function {
            name: method_name,
            params,
            ..
        } = method
        else {
            unreachable!("callers filter to Function expressions");
        };

        let has_self = params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
        });
        has_self
            && !self
                .ufcs_methods
                .contains(&(receiver_key.to_string(), method_name.to_string()))
    }

    fn for_each_definition_type<F>(&self, mut visit: F)
    where
        F: FnMut(&str, &[Generic], &Type),
    {
        for (id, definition) in self.definitions() {
            let key = id.as_str();
            match &definition.body {
                DefinitionBody::Struct {
                    generics, fields, ..
                } => {
                    for field in fields {
                        visit(key, generics, &field.ty);
                    }
                }
                DefinitionBody::Enum {
                    generics, variants, ..
                } => {
                    for variant in variants {
                        for field in variant.fields.iter() {
                            visit(key, generics, &field.ty);
                        }
                    }
                }
                DefinitionBody::TypeAlias { generics, .. } => {
                    let body = definition.ty.unwrap_forall();
                    visit(key, generics, body);
                }
                DefinitionBody::Interface {
                    definition: iface, ..
                } => {
                    for method_ty in iface.methods.values() {
                        visit(key, &iface.generics, method_ty);
                    }
                    for parent in &iface.parents {
                        visit(key, &iface.generics, parent);
                    }
                }
                DefinitionBody::Value { .. } => {}
            }
        }
    }

    /// Collect generic demands from a function signature and body for one definition.
    fn collect_signature_demands(
        &self,
        params: &[Binding],
        return_type: &Type,
        body: &Expression,
        definition: &str,
        local_generics: &[Generic],
        table: &mut GenericConstraintTable,
    ) {
        for parameter in params {
            collect_demands_from_type(&parameter.ty, definition, local_generics, table, self.store);
        }
        collect_demands_from_type(return_type, definition, local_generics, table, self.store);
        self.collect_demands_from_expression(body, definition, local_generics, table);
    }

    fn collect_demands_from_expression(
        &self,
        expression: &Expression,
        definition: &str,
        local_generics: &[Generic],
        table: &mut GenericConstraintTable,
    ) {
        // `get_type()` catches `Map.new<T, int>()` inside a non-map context
        // like `.is_empty()` or an if-condition.
        collect_demands_from_type(
            &expression.get_type(),
            definition,
            local_generics,
            table,
            self.store,
        );
        // Let bindings carry an explicit type that `children()` does not walk.
        if let Expression::Let { binding, .. } = expression {
            collect_demands_from_type(&binding.ty, definition, local_generics, table, self.store);
        }
        for child in expression.children() {
            self.collect_demands_from_expression(child, definition, local_generics, table);
        }
    }

    fn propagate_constraints(&self, table: &mut GenericConstraintTable) {
        let mut uses = HashMap::default();
        self.for_each_definition_type(|key, _, ty| {
            scan_propagation_uses(ty, key, table, &mut uses);
        });

        let mut comparable = table.comparable_nodes();
        let mut queue: VecDeque<_> = comparable.iter().cloned().collect();
        while let Some(required) = queue.pop_front() {
            for usage in uses.get(&required).into_iter().flatten() {
                let local_constraints = table
                    .by_definition
                    .get(usage.dependent_definition.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let dependent_parameters = comparable_key_params(
                    &usage.argument,
                    |name| {
                        local_constraints
                            .iter()
                            .any(|constraints| constraints.parameter == name)
                    },
                    self.store,
                );
                for parameter in dependent_parameters {
                    let dependent = ConstraintNode {
                        definition: usage.dependent_definition.clone(),
                        parameter,
                    };
                    if !comparable.insert(dependent.clone()) {
                        continue;
                    }
                    table.mark_inferred_comparable(
                        dependent.definition.as_str(),
                        dependent.parameter.as_str(),
                    );
                    queue.push_back(dependent);
                }
            }
        }
    }
}

#[derive(Default)]
struct GenericConstraintTable {
    by_definition: GenericConstraintsByDefinition,
}

impl GenericConstraintTable {
    fn ensure_seeded(&mut self, definition: &str, generics: &[Generic]) {
        if self.by_definition.contains_key(definition) {
            return;
        }
        let constraints = generics
            .iter()
            .map(|generic| GenericConstraints {
                parameter: generic.name.clone(),
                explicit: generic
                    .bounds
                    .iter()
                    .map(GenericConstraint::from_annotation)
                    .collect(),
                inferred_comparable: false,
            })
            .collect();
        self.by_definition
            .insert(Symbol::from_raw(definition), constraints);
    }

    fn add_explicit(
        &mut self,
        definition: &str,
        parameter: &str,
        constraint: GenericConstraint,
    ) -> bool {
        let Some(parameters) = self.by_definition.get_mut(definition) else {
            return false;
        };
        let Some(parameter) = parameters
            .iter_mut()
            .find(|candidate| candidate.parameter == parameter)
        else {
            return false;
        };
        add_explicit_constraint(parameter, constraint)
    }

    fn mark_inferred_comparable(&mut self, definition: &str, parameter: &str) -> bool {
        let Some(parameters) = self.by_definition.get_mut(definition) else {
            return false;
        };
        let Some(parameter) = parameters
            .iter_mut()
            .find(|candidate| candidate.parameter == parameter)
        else {
            return false;
        };
        if parameter.inferred_comparable {
            return false;
        }
        parameter.inferred_comparable = true;
        true
    }

    fn comparable_nodes(&self) -> HashSet<ConstraintNode> {
        self.by_definition
            .iter()
            .flat_map(|(definition, parameters)| {
                parameters
                    .iter()
                    .filter(|constraints| requires_comparable(constraints))
                    .map(|constraints| ConstraintNode {
                        definition: definition.clone(),
                        parameter: constraints.parameter.clone(),
                    })
            })
            .collect()
    }
}

fn add_explicit_constraint(
    parameter: &mut GenericConstraints,
    constraint: GenericConstraint,
) -> bool {
    if parameter.explicit.contains(&constraint) {
        return false;
    }
    if let GenericConstraint::Named(new_annotation) = &constraint
        && parameter.explicit.iter().any(|existing| {
            matches!(existing, GenericConstraint::Named(existing_annotation)
                if annotations_equivalent(existing_annotation, new_annotation))
        })
    {
        return false;
    }
    parameter.explicit.push(constraint);
    true
}

fn annotations_equivalent(left: &Annotation, right: &Annotation) -> bool {
    match (left, right) {
        (
            Annotation::Constructor {
                name: left_name,
                params: left_parameters,
                ..
            },
            Annotation::Constructor {
                name: right_name,
                params: right_parameters,
                ..
            },
        ) => {
            left_name == right_name
                && annotation_lists_equivalent(left_parameters, right_parameters)
        }
        (
            Annotation::Function {
                params: left_parameters,
                param_mutability: left_mutability,
                return_type: left_return,
                ..
            },
            Annotation::Function {
                params: right_parameters,
                param_mutability: right_mutability,
                return_type: right_return,
                ..
            },
        ) => {
            left_mutability == right_mutability
                && annotation_lists_equivalent(left_parameters, right_parameters)
                && annotations_equivalent(left_return, right_return)
        }
        (
            Annotation::Tuple {
                elements: left_elements,
                ..
            },
            Annotation::Tuple {
                elements: right_elements,
                ..
            },
        ) => annotation_lists_equivalent(left_elements, right_elements),
        (Annotation::Unknown, Annotation::Unknown)
        | (Annotation::Opaque { .. }, Annotation::Opaque { .. }) => true,
        _ => false,
    }
}

fn annotation_lists_equivalent(left: &[Annotation], right: &[Annotation]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| annotations_equivalent(left, right))
}

fn requires_comparable(constraints: &GenericConstraints) -> bool {
    constraints.inferred_comparable
        || constraints.explicit.iter().any(|constraint| {
            matches!(
                constraint,
                GenericConstraint::Comparable | GenericConstraint::Ordered
            )
        })
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ConstraintNode {
    definition: Symbol,
    parameter: EcoString,
}

struct PropagationUse {
    dependent_definition: Symbol,
    argument: Type,
}

fn collect_demands_from_type(
    ty: &Type,
    definition: &str,
    local_generics: &[Generic],
    table: &mut GenericConstraintTable,
    store: &Store,
) {
    let mut stack: Vec<&Type> = vec![ty];
    while let Some(current) = stack.pop() {
        if let Some(key_ty) = map_key_of(current) {
            for name in comparable_key_params(
                key_ty,
                |name| local_generics.iter().any(|generic| generic.name == name),
                store,
            ) {
                table.mark_inferred_comparable(definition, &name);
            }
        }
        for child in current.children() {
            stack.push(child);
        }
        // `Type::children()` excludes function bounds; include them so bound
        // type expressions can also impose constraints.
        if let Type::Function(f) = current {
            for bound in &f.bounds {
                stack.push(&bound.ty);
            }
        }
    }
}

const MAX_KEY_DEPTH: usize = 64;

fn comparable_key_params(
    key: &Type,
    is_local_generic: impl Fn(&str) -> bool + Copy,
    store: &Store,
) -> Vec<EcoString> {
    let mut parameters = Vec::new();
    collect_comparable_key_params(key, is_local_generic, store, 0, &mut parameters);
    parameters
}

fn collect_comparable_key_params<F>(
    key: &Type,
    is_local_generic: F,
    store: &Store,
    depth: usize,
    parameters: &mut Vec<EcoString>,
) where
    F: Fn(&str) -> bool + Copy,
{
    if depth >= MAX_KEY_DEPTH {
        return;
    }
    match key {
        Type::Parameter(name) if is_local_generic(name) => {
            parameters.push(name.clone());
        }
        Type::Array { element, .. } => {
            collect_comparable_key_params(element, is_local_generic, store, depth + 1, parameters);
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_comparable_key_params(
                    element,
                    is_local_generic,
                    store,
                    depth + 1,
                    parameters,
                );
            }
        }
        Type::Nominal {
            underlying_ty: Some(underlying),
            ..
        } => {
            collect_comparable_key_params(
                underlying,
                is_local_generic,
                store,
                depth + 1,
                parameters,
            );
        }
        Type::Nominal { id, params, .. } => {
            match store.get_definition(id.as_str()).map(|d| &d.body) {
                Some(DefinitionBody::Struct {
                    generics, fields, ..
                }) => {
                    let map = build_substitution_map(generics, params);
                    for field in fields {
                        let field_ty = substitute(&field.ty, &map);
                        collect_comparable_key_params(
                            &field_ty,
                            is_local_generic,
                            store,
                            depth + 1,
                            parameters,
                        );
                    }
                }
                Some(DefinitionBody::Enum {
                    generics, variants, ..
                }) => {
                    let map = build_substitution_map(generics, params);
                    for field in variants.iter().flat_map(|v| &v.fields) {
                        let field_ty = substitute(&field.ty, &map);
                        collect_comparable_key_params(
                            &field_ty,
                            is_local_generic,
                            store,
                            depth + 1,
                            parameters,
                        );
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn scan_propagation_uses(
    ty: &Type,
    definition: &str,
    table: &GenericConstraintTable,
    uses: &mut HashMap<ConstraintNode, Vec<PropagationUse>>,
) {
    let mut stack: Vec<&Type> = vec![ty];
    let mut visited_nominals: HashSet<Symbol> = HashSet::default();
    let dependent_definition = Symbol::from_raw(definition);

    while let Some(current) = stack.pop() {
        if let Type::Nominal { id, params, .. } = current {
            // Direct `Map<P, _>` is already handled by the initial collection;
            // here we only chase nominal callees with constrained positions.
            if !is_map_id(id)
                && let Some(callee_sets) = table.by_definition.get(id.as_str())
            {
                for (i, set) in callee_sets.iter().enumerate() {
                    let Some(arg) = params.get(i) else { continue };
                    let required = ConstraintNode {
                        definition: id.clone(),
                        parameter: set.parameter.clone(),
                    };
                    uses.entry(required).or_default().push(PropagationUse {
                        dependent_definition: dependent_definition.clone(),
                        argument: arg.clone(),
                    });
                }
            }
            // Skip re-entry into the same nominal in chains like `Alias<Alias<T>>`.
            if visited_nominals.insert(id.clone()) {
                for parameter in params {
                    stack.push(parameter);
                }
            }
            continue;
        }
        for child in current.children() {
            stack.push(child);
        }
        if let Type::Function(f) = current {
            for bound in &f.bounds {
                stack.push(&bound.ty);
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

fn strip_self_referential_bounds(generics: &[Generic], interface_name: &str) -> Vec<Generic> {
    generics
        .iter()
        .map(|generic| Generic {
            name: generic.name.clone(),
            bounds: generic
                .bounds
                .iter()
                .filter(|annotation| {
                    !matches!(annotation, Annotation::Constructor { name, .. }
                        if unqualified_name(name) == interface_name)
                })
                .cloned()
                .collect(),
            span: generic.span,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Span;

    fn constructor(name: &str, span: Span) -> Annotation {
        Annotation::Constructor {
            name: name.into(),
            params: vec![],
            span,
        }
    }

    #[test]
    fn builtin_constraints_are_normalized() {
        assert_eq!(
            GenericConstraint::from_annotation(&constructor("Comparable", Span::dummy())),
            GenericConstraint::Comparable
        );
        assert_eq!(
            GenericConstraint::from_annotation(&constructor("go:cmp.Ordered", Span::dummy())),
            GenericConstraint::Ordered
        );
    }

    #[test]
    fn equivalent_named_constraints_are_deduplicated_across_spans() {
        let mut constraints = GenericConstraints {
            parameter: "T".into(),
            ..Default::default()
        };
        assert!(add_explicit_constraint(
            &mut constraints,
            GenericConstraint::Named(constructor("Parent", Span::new(0, 0, 1))),
        ));
        assert!(!add_explicit_constraint(
            &mut constraints,
            GenericConstraint::Named(constructor("Parent", Span::new(0, 5, 1))),
        ));
    }
}
