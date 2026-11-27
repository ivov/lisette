//! Adapted from src/go/types/mono.go in the Go source tree.
//!
//! Copyright 2021 The Go Authors. All rights reserved.
//! Use of this source code is governed by a BSD-style
//! license that can be found at https://go.dev/LICENSE.
//!
//! Rejects generic code that Go cannot instantiate in a finite number of steps.
//!
//! For each instantiation, the check records which type parameters the type
//! argument uses, and whether the argument wraps them, as `Slice<T>` wraps `T`.
//! Go rejects the program when a chain of instantiations leads back to its
//! start and one step wraps a parameter, because each pass makes the type larger.

use std::ptr;

use diagnostics::LocalSink;
use ecow::EcoString;
use rustc_hash::{FxHashMap, FxHashSet};
use semantics::store::Store;
use syntax::ast::{Binding, EnumVariant, Expression, Generic, Pattern, Span};
use syntax::program::{
    AliasKind, CalleeName, DefinitionBody, is_go_import, resolved_callee, resolved_instantiation,
};
use syntax::types::{Symbol, Type, unqualified_name};

pub(crate) fn run(store: &Store, sink: &LocalSink) {
    let mut graph = Graph::new(store);
    graph.add_type_definitions();
    for (body, scope, span) in graph.add_functions() {
        graph.add_body(body, &scope, span);
    }
    graph.report(sink);
}

#[derive(Clone)]
enum Site {
    Call {
        function: EcoString,
        parameter: EcoString,
    },
    Declaration {
        owner: EcoString,
        inferred: bool,
    },
}

struct Growing {
    source: usize,
    target: usize,
    parameter: EcoString,
    argument: Type,
    span: Span,
    site: Site,
}

struct AliasUse {
    used: Vec<bool>,
    identity: Option<usize>,
}

type Scope = FxHashMap<EcoString, usize>;

struct Graph<'a> {
    store: &'a Store,
    vertices: FxHashMap<(EcoString, usize), usize>,
    edges: Vec<Vec<usize>>,
    growing: Vec<Growing>,
    functions: FxHashMap<EcoString, Scope>,
    ordered_parameters: FxHashMap<EcoString, (Vec<(EcoString, usize)>, usize)>,
    aliases: FxHashMap<Symbol, Option<AliasUse>>,
    summarizing: FxHashSet<Symbol>,
}

impl<'a> Graph<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            vertices: FxHashMap::default(),
            edges: Vec::new(),
            growing: Vec::new(),
            functions: FxHashMap::default(),
            ordered_parameters: FxHashMap::default(),
            aliases: FxHashMap::default(),
            summarizing: FxHashSet::default(),
        }
    }

    fn vertex(&mut self, owner: &EcoString, position: usize) -> usize {
        let next = self.edges.len();
        let vertex = *self
            .vertices
            .entry((owner.clone(), position))
            .or_insert(next);
        if vertex == next {
            self.edges.push(Vec::new());
        }
        vertex
    }

    fn type_vertex(&mut self, id: &Symbol, position: usize) -> Option<usize> {
        let generics = self.store.get_definition(id)?.body.generics()?;
        (!is_go_import(id) && position < generics.len()).then(|| self.vertex(id.as_eco(), position))
    }

    fn add_type_definitions(&mut self) {
        let store = self.store;
        let packages = store.packages().values();
        for package in packages.filter(|package| !is_go_import(&package.id)) {
            for (id, definition) in &package.definitions {
                let generics = definition.body.generics().unwrap_or_default();
                if generics.is_empty() || embeds_itself(store, id) {
                    continue;
                }
                let scope: Scope = (0..generics.len())
                    .filter_map(|position| {
                        let vertex = self.type_vertex(id, position)?;
                        Some((generics[position].name.clone(), vertex))
                    })
                    .collect();
                let span = definition.name_span.unwrap_or_else(Span::dummy);
                let is_interface = matches!(definition.body, DefinitionBody::Interface { .. });
                let mut types: Vec<(&Type, Span)> = generics
                    .iter()
                    .filter_map(Generic::resolved_bounds)
                    .flatten()
                    .filter(|bound| !is_interface || bound.get_qualified_id() != Some(id))
                    .map(|bound| (bound, span))
                    .collect();
                match &definition.body {
                    DefinitionBody::Struct { fields, .. } => {
                        types.extend(fields.iter().map(|field| (&field.ty, field.name_span)));
                    }
                    DefinitionBody::Enum { variants, .. } => {
                        types.extend(variants.iter().flat_map(variant_field_types));
                    }
                    DefinitionBody::Interface { definition } => {
                        types.extend(definition.parents.iter().map(|parent| (parent, span)));
                        types.extend(
                            definition
                                .methods
                                .values()
                                .map(|method| (&method.ty, method.name_span.unwrap_or(span))),
                        );
                    }
                    DefinitionBody::TypeAlias {
                        alias: AliasKind::Transparent { target, .. },
                        ..
                    } => types.push((target, span)),
                    _ => {}
                }
                for (ty, span) in types {
                    self.add_type(ty, &scope, span, false);
                }
            }
        }
    }

    fn add_functions(&mut self) -> Vec<(&'a Expression, Scope, Span)> {
        let store = self.store;
        let mut bodies = Vec::new();
        for package in store.packages().values() {
            for item in package.source_files().flat_map(|file| &file.items) {
                match item {
                    Expression::Function { name, generics, .. } => {
                        let symbol = Symbol::from_parts(&package.id, name);
                        let scope = self.add_function(&symbol, generics.iter(), 0);
                        bodies.extend(self.add_signature(item, &[], scope));
                    }
                    Expression::ImplBlock {
                        ty,
                        generics,
                        methods,
                        ..
                    } => self.add_impl_block(ty, generics, methods, &mut bodies),
                    _ => {}
                }
            }
        }
        bodies
    }

    fn add_impl_block(
        &mut self,
        ty: &Type,
        generics: &[Generic],
        methods: &'a [Expression],
        bodies: &mut Vec<(&'a Expression, Scope, Span)>,
    ) {
        let store = self.store;
        let is_alias = ty
            .get_qualified_id()
            .and_then(|id| store.get_definition(id))
            .is_some_and(|d| matches!(d.body, DefinitionBody::TypeAlias { .. }));
        let (receiver, _) = store.peel_refs_and_aliases(ty);
        let Type::Nominal { id, params, .. } = &receiver else {
            return;
        };
        if is_alias {
            return;
        }
        for method in methods {
            let Expression::Function {
                name,
                generics: own,
                params: parameters,
                ..
            } = method
            else {
                continue;
            };
            let symbol = id.with_segment(name);
            let (scope, impl_generics) = if is_go_method(store, id, name, parameters) {
                (self.add_go_method(&symbol, id, params), &[][..])
            } else {
                let scope = self.add_function(&symbol, generics.iter().chain(own), generics.len());
                match receiver_generics(store, id, params, generics) {
                    Some(declared) => {
                        self.add_receiver_bounds(declared, generics, &scope, method);
                        (scope, &[][..])
                    }
                    None => (scope, generics),
                }
            };
            bodies.extend(self.add_signature(method, impl_generics, scope));
        }
    }

    fn add_receiver_bounds(
        &mut self,
        declared: &[Generic],
        generics: &[Generic],
        scope: &Scope,
        method: &Expression,
    ) {
        let Expression::Function { name_span, .. } = method else {
            return;
        };
        let renamed: Scope = declared
            .iter()
            .zip(generics)
            .filter_map(|(declared, generic)| {
                Some((declared.name.clone(), *scope.get(&generic.name)?))
            })
            .collect();
        for bound in declared
            .iter()
            .filter_map(Generic::resolved_bounds)
            .flatten()
        {
            self.add_type(bound, &renamed, *name_span, false);
        }
    }

    fn add_function<'g>(
        &mut self,
        symbol: &Symbol,
        generics: impl Iterator<Item = &'g Generic>,
        own_start: usize,
    ) -> Scope {
        let parameters: Vec<(EcoString, usize)> = generics
            .enumerate()
            .map(|(position, generic)| {
                let vertex = self.vertex(symbol.as_eco(), position);
                (generic.name.clone(), vertex)
            })
            .collect();
        let scope: Scope = parameters.iter().cloned().collect();
        self.ordered_parameters
            .insert(symbol.as_eco().clone(), (parameters, own_start));
        self.functions
            .insert(symbol.as_eco().clone(), scope.clone());
        scope
    }

    fn add_go_method(&mut self, symbol: &Symbol, receiver: &Symbol, arguments: &[Type]) -> Scope {
        let mut scope = Scope::default();
        for (position, argument) in arguments.iter().enumerate() {
            if let Type::Parameter(name) = argument
                && let Some(vertex) = self.type_vertex(receiver, position)
            {
                scope.insert(name.clone(), vertex);
            }
        }
        self.functions
            .insert(symbol.as_eco().clone(), scope.clone());
        scope
    }

    fn add_signature(
        &mut self,
        function: &'a Expression,
        impl_generics: &[Generic],
        scope: Scope,
    ) -> Option<(&'a Expression, Scope, Span)> {
        let Expression::Function {
            name_span,
            generics,
            params: parameters,
            return_type,
            body,
            ..
        } = function
        else {
            return None;
        };
        if scope.is_empty() {
            return None;
        }
        let bounds = impl_generics
            .iter()
            .chain(generics)
            .filter_map(Generic::resolved_bounds)
            .flatten();
        for ty in parameters
            .iter()
            .map(|p| &p.ty)
            .chain([return_type])
            .chain(bounds)
        {
            self.add_type(ty, &scope, *name_span, false);
        }
        Some((body.definition()?, scope, *name_span))
    }

    fn add_body(&mut self, body: &Expression, scope: &Scope, span: Span) {
        let mut pending = vec![(body, None)];
        while let Some((expression, call_span)) = pending.pop() {
            let reference = match expression {
                Expression::Function { .. } => continue,
                Expression::Identifier { .. } | Expression::DotAccess { .. } => {
                    resolved_instantiation(expression)
                        .filter(|instantiation| !instantiation.is_empty())
                        .and_then(|instantiation| {
                            Some((resolved_callee(expression)?.qualified_name(), instantiation))
                        })
                }
                _ => None,
            };
            if let Some((definition, instantiation)) = reference {
                let definition: &str = &definition;
                let span = call_span.unwrap_or_else(|| expression.get_span());
                for (parameter, argument) in instantiation.iter() {
                    self.add_call(definition, parameter, argument, scope, span);
                }
            }
            if let Some(ty) = expression.type_ref() {
                self.add_type(ty, scope, span, true);
            }
            if let Expression::Call {
                expression: callee,
                type_arguments,
                span,
                ..
            } = expression
                && let Some(arguments) = type_arguments.resolved_types()
            {
                self.add_type_arguments(callee, arguments.iter(), scope, *span);
            }
            let callee = match expression {
                Expression::Call {
                    expression: callee,
                    span,
                    ..
                } => Some((callee.as_ref(), *span)),
                _ => None,
            };
            for child in expression.children().into_iter().rev() {
                let call_span = callee.filter(|(callee, _)| ptr::eq(*callee, child));
                pending.push((child, call_span.map(|(_, span)| span)));
            }
        }
    }

    fn add_type_arguments<'t>(
        &mut self,
        callee: &Expression,
        arguments: impl Iterator<Item = &'t Type>,
        scope: &Scope,
        span: Span,
    ) {
        let callee = resolved_callee(callee);
        let arguments: Vec<&Type> = arguments.collect();
        let (all, own_start) = callee
            .as_ref()
            .and_then(|callee| {
                self.ordered_parameters
                    .get(callee.qualified_name().as_str())
            })
            .cloned()
            .unwrap_or_default();
        let parameters = if arguments.len() == all.len() {
            &all[..]
        } else {
            &all[own_start.min(all.len())..]
        };
        let function = callee.as_ref().map_or("", CalleeName::last_segment);
        for (position, argument) in arguments.into_iter().enumerate() {
            self.add_type(argument, scope, span, true);
            if let Some((parameter, target)) = parameters.get(position) {
                let site = Site::Call {
                    function: function.into(),
                    parameter: parameter.clone(),
                };
                self.assign(*target, argument, scope, span, &site);
            }
        }
    }

    fn add_call(
        &mut self,
        definition: &str,
        parameter: &EcoString,
        argument: &Type,
        scope: &Scope,
        span: Span,
    ) {
        let (owner, member) = definition.rsplit_once('.').unwrap_or(("", definition));
        let function = self.functions.get(definition).or_else(|| {
            let owner = self.store.peel_alias_name(owner)?;
            self.functions.get(owner.with_segment(member).as_eco())
        });
        let Some(&target) = function.and_then(|function| function.get(parameter)) else {
            return;
        };
        let site = Site::Call {
            function: member.into(),
            parameter: parameter.clone(),
        };
        self.assign(target, argument, scope, span, &site);
    }

    fn add_type(&mut self, ty: &Type, scope: &Scope, span: Span, inferred: bool) {
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            pending.extend(ty.children().into_iter().rev());
            let Type::Nominal { id, params, .. } = ty else {
                continue;
            };
            if self
                .alias_use(id)
                .is_some_and(|alias| alias.identity.is_some())
            {
                continue;
            }
            let site = Site::Declaration {
                owner: unqualified_name(id).into(),
                inferred,
            };
            for (position, argument) in params.iter().enumerate() {
                if let Some(target) = self.type_vertex(id, position) {
                    self.assign(target, argument, scope, span, &site);
                }
            }
        }
    }

    fn assign(&mut self, target: usize, argument: &Type, scope: &Scope, span: Span, site: &Site) {
        let mut parameters = Vec::new();
        self.mentioned_parameters(argument, &mut parameters);
        let bare = match self.without_identity_aliases(argument) {
            Type::Parameter(name) => Some(name.clone()),
            _ => None,
        };
        let mut seen = FxHashSet::default();
        for parameter in parameters {
            let Some(&source) = scope.get(&parameter) else {
                continue;
            };
            if !seen.insert(source) {
                continue;
            }
            self.edges[source].push(target);
            if bare.as_ref() != Some(&parameter) {
                self.growing.push(Growing {
                    source,
                    target,
                    parameter,
                    argument: argument.clone(),
                    span,
                    site: site.clone(),
                });
            }
        }
    }

    fn mentioned_parameters(&mut self, ty: &Type, parameters: &mut Vec<EcoString>) {
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            match ty {
                Type::Parameter(name) => parameters.push(name.clone()),
                Type::Nominal { id, params, .. } if self.alias_use(id).is_some() => {
                    let alias = self.alias_use(id).expect("the alias is summarized");
                    let used = params.iter().zip(&alias.used);
                    pending.extend(used.filter_map(|(argument, &used)| used.then_some(argument)));
                }
                _ => pending.extend(ty.children()),
            }
        }
    }

    fn without_identity_aliases<'t>(&mut self, mut ty: &'t Type) -> &'t Type {
        while let Type::Nominal { id, params, .. } = ty
            && let Some(position) = self.alias_use(id).and_then(|alias| alias.identity)
        {
            ty = &params[position];
        }
        ty
    }

    fn alias_use(&mut self, id: &Symbol) -> Option<&AliasUse> {
        if !self.aliases.contains_key(id) && !self.summarizing.contains(id) {
            self.summarize_aliases(id);
        }
        self.aliases.get(id)?.as_ref()
    }

    fn summarize_aliases(&mut self, id: &Symbol) {
        let store = self.store;
        let mut pending = vec![(id.clone(), false)];
        while let Some((current, ready)) = pending.pop() {
            if self.aliases.contains_key(&current) {
                continue;
            }
            let Some(DefinitionBody::TypeAlias {
                generics,
                alias: AliasKind::Transparent { target, .. },
                ..
            }) = store
                .get_definition(&current)
                .map(|definition| &definition.body)
            else {
                self.aliases.insert(current, None);
                continue;
            };
            if !ready {
                if self.summarizing.insert(current.clone()) {
                    pending.push((current, true));
                    self.push_unsummarized(target, &mut pending);
                }
                continue;
            }
            let mut mentioned = Vec::new();
            self.mentioned_parameters(target, &mut mentioned);
            let identity = match self.without_identity_aliases(target) {
                Type::Parameter(name) => generics.iter().position(|g| &g.name == name),
                _ => None,
            };
            let used = generics
                .iter()
                .map(|g| mentioned.contains(&g.name))
                .collect();
            self.summarizing.remove(&current);
            self.aliases
                .insert(current, Some(AliasUse { used, identity }));
        }
    }

    fn push_unsummarized(&self, target: &Type, pending: &mut Vec<(Symbol, bool)>) {
        let mut nested = vec![target];
        while let Some(ty) = nested.pop() {
            if let Type::Nominal { id, .. } = ty
                && !self.aliases.contains_key(id)
                && !self.summarizing.contains(id)
            {
                pending.push((id.clone(), false));
            }
            nested.extend(ty.children());
        }
    }

    fn report(mut self, sink: &LocalSink) {
        let components = components(&self.edges);
        self.growing.sort_by_key(|edge| {
            let is_declaration = matches!(edge.site, Site::Declaration { .. });
            (is_declaration, edge.span.file_id, edge.span.byte_offset)
        });
        let mut reported = FxHashSet::default();
        let mut reported_spans = FxHashSet::default();
        for edge in &self.growing {
            let component = components[edge.source];
            if component != components[edge.target]
                || reported_spans.contains(&edge.span)
                || !reported.insert(component)
            {
                continue;
            }
            reported_spans.insert(edge.span);
            sink.push(match &edge.site {
                Site::Call {
                    function,
                    parameter,
                } => diagnostics::infer::growing_call_type_argument(
                    parameter,
                    &edge.argument,
                    function,
                    edge.span,
                ),
                Site::Declaration { owner, inferred } => {
                    let argument = if *inferred {
                        edge.argument.without_permissions()
                    } else {
                        edge.argument.clone()
                    };
                    diagnostics::infer::growing_declaration_type_argument(
                        &edge.parameter,
                        &argument,
                        owner,
                        edge.span,
                    )
                }
            });
        }
    }
}

fn receiver_generics<'s>(
    store: &'s Store,
    receiver: &Symbol,
    arguments: &[Type],
    generics: &[Generic],
) -> Option<&'s [Generic]> {
    let declared = store.get_definition(receiver)?.body.generics()?;
    let matches = declared.len() == generics.len()
        && arguments.len() == generics.len()
        && arguments
            .iter()
            .zip(generics)
            .all(|(argument, generic)| matches!(argument, Type::Parameter(name) if name == &generic.name));
    matches.then_some(declared)
}

fn is_go_method(store: &Store, receiver: &Symbol, name: &str, parameters: &[Binding]) -> bool {
    let has_receiver = parameters.first().is_some_and(|first| {
        matches!(&first.pattern, Pattern::Identifier { identifier, .. } if identifier == "self")
    });
    has_receiver && !store.is_ufcs_method(receiver, name)
}

fn embeds_itself(store: &Store, name: &Symbol) -> bool {
    let parents = |name: &str| {
        let interface = store.get_interface(name);
        let parents = interface
            .into_iter()
            .flat_map(|interface| &interface.parents);
        parents
            .filter_map(Type::get_qualified_id)
            .collect::<Vec<_>>()
    };
    let mut pending = parents(name);
    let mut visited = FxHashSet::default();
    while let Some(parent) = pending.pop() {
        if parent == name.as_str() {
            return true;
        }
        if visited.insert(parent) {
            pending.extend(parents(parent));
        }
    }
    false
}

fn variant_field_types(variant: &EnumVariant) -> impl Iterator<Item = (&Type, Span)> {
    variant.fields.iter().map(|field| {
        let span = [field.name_span, field.annotation.get_span()]
            .into_iter()
            .find(|span| !span.is_dummy());
        (&field.ty, span.unwrap_or(variant.name_span))
    })
}

fn components(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for root in 0..edges.len() {
        let mut pending = vec![(root, false)];
        while let Some((node, finished)) = pending.pop() {
            if finished {
                order.push(node);
            } else if !visited[node] {
                visited[node] = true;
                pending.push((node, true));
                pending.extend(edges[node].iter().map(|&next| (next, false)));
            }
        }
    }
    let mut reverse = vec![Vec::new(); edges.len()];
    for (from, targets) in edges.iter().enumerate() {
        for &to in targets {
            reverse[to].push(from);
        }
    }
    let mut components = vec![usize::MAX; edges.len()];
    for root in order.into_iter().rev() {
        if components[root] != usize::MAX {
            continue;
        }
        components[root] = root;
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            for &next in &reverse[node] {
                if components[next] == usize::MAX {
                    components[next] = root;
                    pending.push(next);
                }
            }
        }
    }
    components
}
