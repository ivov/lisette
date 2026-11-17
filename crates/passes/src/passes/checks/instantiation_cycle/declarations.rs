use diagnostics::LocalSink;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use semantics::store::Store;
use std::ptr::from_ref;
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Span};
use syntax::program::{AliasKind, DefinitionBody};
use syntax::types::GO_IMPORT_PREFIX;
use syntax::types::{Symbol, Type, unqualified_name};

use super::graph;

type Parameters = HashMap<EcoString, bool>;

struct ParameterUses<'a> {
    store: &'a Store,

    types: HashMap<*const Type, Parameters>,
}

impl<'a> ParameterUses<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            types: HashMap::default(),
        }
    }

    fn alias(&self, ty: &'a Type) -> Option<(&'a [Generic], &'a Type)> {
        let Type::Nominal { id, .. } = ty else {
            return None;
        };
        let DefinitionBody::TypeAlias {
            generics,
            alias: AliasKind::Transparent { target, .. },
            ..
        } = &self.store.get_definition(id)?.body
        else {
            return None;
        };
        Some((generics, target))
    }

    fn cached(&self, ty: &Type) -> Parameters {
        self.types.get(&from_ref(ty)).cloned().unwrap_or_default()
    }

    fn complete(&self, current: &'a Type) -> Parameters {
        let mut uses = Parameters::default();
        if let Some((generics, target)) = self.alias(current) {
            let body = self.cached(target);
            let parameters = current
                .get_type_params()
                .expect("a nominal alias has parameters");
            for (generic, argument) in generics.iter().zip(parameters) {
                if let Some(&grows) = body.get(&generic.name) {
                    for (name, nested) in self.cached(argument) {
                        uses.entry(name)
                            .and_modify(|old| *old |= grows || nested)
                            .or_insert(grows || nested);
                    }
                }
            }
        } else {
            match current {
                Type::Parameter(name) => {
                    uses.insert(name.clone(), false);
                }
                Type::Forall { body, .. } => uses = self.cached(body),
                _ => {
                    for child in current.children() {
                        for (name, _) in self.cached(child) {
                            uses.insert(name, true);
                        }
                    }
                }
            }
        }
        uses
    }

    fn summarize(&mut self, ty: &'a Type) -> Parameters {
        enum Work<'a> {
            Visit(&'a Type),
            Complete(&'a Type),
        }
        let mut pending = vec![Work::Visit(ty)];
        let mut active = HashSet::default();
        while let Some(work) = pending.pop() {
            let current = match work {
                Work::Visit(current) => {
                    let key = from_ref(current);
                    if self.types.contains_key(&key) || !active.insert(key) {
                        continue;
                    }
                    pending.push(Work::Complete(current));
                    pending.extend(current.children().into_iter().map(Work::Visit));
                    if let Some((_, target)) = self.alias(current) {
                        pending.push(Work::Visit(target));
                    }
                    continue;
                }
                Work::Complete(current) => current,
            };
            let uses = self.complete(current);
            let key = from_ref(current);
            self.types.insert(key, uses);
            active.remove(&key);
        }
        self.cached(ty)
    }
}

struct Owner<'a> {
    name: &'a Symbol,
    generics: &'a [Generic],
    first: usize,
    types: Vec<(&'a Type, Span)>,
}

struct Growing<'a> {
    source: usize,
    target: usize,
    parameter: EcoString,
    argument: &'a Type,
    owner: &'a Symbol,
    span: Span,
    inferred: bool,
}

struct Collector<'a> {
    uses: ParameterUses<'a>,
    owners: HashMap<Symbol, (usize, usize)>,
    edges: Vec<Vec<usize>>,
    growing: Vec<Growing<'a>>,
}

impl<'a> Collector<'a> {
    fn collect_type(
        &mut self,
        ty: &'a Type,
        source: &HashMap<EcoString, usize>,
        span: Span,
        inferred: bool,
    ) {
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            pending.extend(ty.children().into_iter().rev());
            let Type::Nominal {
                id,
                params: parameters,
                ..
            } = ty
            else {
                continue;
            };
            let Some(&(first, arity)) = self.owners.get(id) else {
                continue;
            };
            for (position, argument) in parameters.iter().take(arity).enumerate() {
                for (parameter, grows) in self.uses.summarize(argument) {
                    let Some(&from) = source.get(&parameter) else {
                        continue;
                    };
                    let to = first + position;
                    self.edges[from].push(to);
                    if grows {
                        self.growing.push(Growing {
                            source: from,
                            target: to,
                            parameter,
                            argument,
                            owner: id,
                            span,
                            inferred,
                        });
                    }
                }
            }
        }
    }

    fn collect_body(
        &mut self,
        body: &'a Expression,
        source: &HashMap<EcoString, usize>,
        span: Span,
    ) {
        let mut pending = vec![body];
        while let Some(expression) = pending.pop() {
            if matches!(expression, Expression::Function { .. }) {
                continue;
            }
            if let Some(ty) = expression.type_ref() {
                self.collect_type(ty, source, span, true);
            }
            pending.extend(expression.children());
        }
    }

    fn collect_methods(&mut self, store: &'a Store, call_cycles: &[super::CallCycle]) {
        for item in store
            .packages()
            .values()
            .flat_map(|package| package.source_files())
            .flat_map(|file| &file.items)
        {
            let Expression::ImplBlock { ty, methods, .. } = item else {
                continue;
            };

            if ty
                .get_qualified_id()
                .and_then(|id| store.get_definition(id))
                .is_some_and(|definition| {
                    matches!(definition.body, DefinitionBody::TypeAlias { .. })
                })
            {
                continue;
            }
            let (receiver, _) = store.peel_refs_and_aliases(ty);
            let Type::Nominal {
                id,
                params: parameters,
                ..
            } = &receiver
            else {
                continue;
            };
            let Some(&(first, arity)) = self.owners.get(id) else {
                continue;
            };
            let source: HashMap<_, _> = parameters
                .iter()
                .take(arity)
                .enumerate()
                .filter_map(|(position, argument)| {
                    if let Type::Parameter(name) = argument {
                        Some((name.clone(), first + position))
                    } else {
                        None
                    }
                })
                .collect();
            for method in methods {
                let Expression::Function {
                    name,
                    name_span,
                    params: parameters,
                    return_type,
                    body,
                    ..
                } = method
                else {
                    continue;
                };

                if store.is_ufcs_method(id, name)
                    || !parameters.first().is_some_and(super::is_self_binding)
                {
                    continue;
                }

                let span = method.get_span();
                if call_cycles.iter().any(|cycle| {
                    cycle.span.file_id == span.file_id
                        && cycle.span.byte_offset >= span.byte_offset
                        && cycle.span.byte_offset < span.end()
                }) {
                    continue;
                }
                for parameter in parameters {
                    self.collect_type(&parameter.ty, &source, *name_span, false);
                }
                self.collect_type(return_type, &source, *name_span, false);
                if let Some(body) = body.definition() {
                    self.collect_body(body, &source, *name_span);
                }
            }
        }
    }
}

pub(super) fn check_type_declarations(
    store: &Store,
    sink: &LocalSink,
    call_cycles: &[super::CallCycle],
) {
    let mut owners: Vec<_> = store
        .packages()
        .values()
        .filter(|package| !package.id.starts_with(GO_IMPORT_PREFIX))
        .flat_map(|package| package.definitions.iter())
        .filter_map(|(name, definition)| {
            let generics = definition.body.generics()?;
            if generics.is_empty() || embedding_cycle(store, name) {
                return None;
            }
            let span = definition.name_span.unwrap_or_else(Span::dummy);
            let mut types: Vec<_> = match &definition.body {
                DefinitionBody::Interface { definition } => definition
                    .parents
                    .iter()
                    .map(|ty| (ty, span))
                    .chain(
                        definition
                            .methods
                            .values()
                            .map(|method| (&method.ty, method.name_span.unwrap_or(span))),
                    )
                    .collect(),
                DefinitionBody::Struct { fields, .. } => fields
                    .iter()
                    .map(|field| (&field.ty, field.name_span))
                    .collect(),
                DefinitionBody::Enum { variants, .. } => variants
                    .iter()
                    .flat_map(|variant| {
                        variant
                            .fields
                            .iter()
                            .map(|field| (&field.ty, field.name_span))
                    })
                    .collect(),
                DefinitionBody::TypeAlias {
                    alias: AliasKind::Transparent { target, .. },
                    ..
                } => vec![(target, span)],
                _ => vec![],
            };
            types.sort_by_key(|(_, span)| (span.file_id, span.byte_offset));
            Some(Owner {
                name,
                generics,
                first: 0,
                types,
            })
        })
        .collect();
    owners.sort_by_key(|owner| owner.name);
    let mut node_count = 0;
    for owner in &mut owners {
        owner.first = node_count;
        node_count += owner.generics.len();
    }
    let mut collector = Collector {
        uses: ParameterUses::new(store),
        owners: owners
            .iter()
            .map(|owner| (owner.name.clone(), (owner.first, owner.generics.len())))
            .collect(),
        edges: vec![vec![]; node_count],
        growing: vec![],
    };
    for owner in &owners {
        let source = owner
            .generics
            .iter()
            .enumerate()
            .map(|(index, generic)| (generic.name.clone(), owner.first + index))
            .collect();
        for (ty, span) in &owner.types {
            collector.collect_type(ty, &source, *span, false);
        }
    }
    collector.collect_methods(store, call_cycles);
    let groups = graph::components(&collector.edges);
    let mut reported = HashSet::default();

    collector.growing.sort_by_key(|edge| {
        (
            edge.span.file_id,
            edge.span.byte_offset,
            edge.source,
            edge.target,
        )
    });
    for edge in collector.growing {
        if groups[edge.source] == groups[edge.target] && reported.insert(groups[edge.source]) {
            sink.push(diagnostics::infer::growing_declaration_type_argument(
                &edge.parameter,
                &if edge.inferred {
                    edge.argument.without_permissions()
                } else {
                    edge.argument.clone()
                },
                unqualified_name(edge.owner),
                edge.span,
            ));
        }
    }
}

fn embedding_cycle(store: &Store, name: &Symbol) -> bool {
    let Some(interface) = store.get_interface(name) else {
        return false;
    };
    let mut pending: Vec<_> = interface
        .parents
        .iter()
        .filter_map(Type::get_qualified_id)
        .collect();
    let mut visited = HashSet::default();
    while let Some(parent) = pending.pop() {
        if parent == name.as_str() {
            return true;
        }
        if visited.insert(parent)
            && let Some(interface) = store.get_interface(parent)
        {
            pending.extend(interface.parents.iter().filter_map(Type::get_qualified_id));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Annotation;
    use syntax::program::{Definition, Visibility};

    #[test]
    fn branching_aliases_are_summarized_without_expansion() {
        let mut store = Store::new();
        store.add_package("main");

        for index in 0..=64 {
            let parameter = Type::Parameter("T".into());
            let target = if index == 64 {
                parameter
            } else {
                let next = Type::Nominal {
                    id: Symbol::from_raw(format!("main.P{}", index + 1)),
                    params: vec![parameter],
                    writable: false,
                };
                Type::Tuple(vec![next.clone(), next])
            };
            store.edit_package("main").unwrap().definitions.insert(
                Symbol::from_raw(format!("main.P{index}")),
                Definition {
                    visibility: Visibility::Public,
                    ty: target.clone(),
                    name_span: Some(Span::new(0, index, 1)),
                    doc: None,
                    body: DefinitionBody::TypeAlias {
                        generics: vec![Generic::new("T", vec![], Span::dummy())],
                        alias: AliasKind::Transparent {
                            annotation: Annotation::Unknown,
                            target,
                        },
                        methods: Default::default(),
                        attributes: Default::default(),
                    },
                },
            );
        }
        let ty = Type::Nominal {
            id: Symbol::from_raw("main.P0"),
            params: vec![Type::Parameter("X".into())],
            writable: false,
        };
        let mut uses = ParameterUses::new(&store);
        assert_eq!(uses.summarize(&ty).get("X"), Some(&true));
        assert!(uses.types.len() < 400);
        let sink = LocalSink::new();
        check_type_declarations(&store, &sink, &[]);
        assert!(!sink.has_errors());
    }
}
