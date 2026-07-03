//! Rejects recursion that regrows a generic type argument, mirroring the Go
//! compiler's instantiation-cycle rule so the failure surfaces as a Lisette
//! diagnostic instead of leaking from `go build`.

use diagnostics::LocalSink;
use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::{Binding, Expression, Pattern, Span};
use syntax::program::DotAccessKind;
use syntax::types::{CompoundKind, FunctionType, Symbol, Type};

use semantics::store::Store;

pub(crate) fn run(store: &Store, sink: &LocalSink) {
    let targets = collect_generic_targets(store);
    if targets.list.is_empty() {
        return;
    }

    let mut collector = EdgeCollector {
        targets: &targets,
        adjacency: vec![Vec::new(); targets.node_count],
        growing: Vec::new(),
        current: 0,
    };
    for index in 0..targets.list.len() {
        collector.current = index;
        collector.walk(targets.list[index].body);
    }

    let mut reported_spans: HashSet<Span> = HashSet::default();
    for edge in &collector.growing {
        if !reaches(&collector.adjacency, edge.target, edge.source) {
            continue;
        }
        if !reported_spans.insert(edge.span) {
            continue;
        }
        sink.push(diagnostics::infer::instantiation_cycle(
            &edge.var,
            &edge.type_arg,
            &edge.target_name,
            edge.span,
        ));
    }
}

struct GenericTarget<'a> {
    name: &'a EcoString,
    vars: Vec<&'a EcoString>,
    declared_self: Option<&'a Type>,
    declared_params: Vec<&'a Type>,
    declared_return: &'a Type,
    body: &'a Expression,
    first_node: usize,
}

#[derive(Default)]
struct Targets<'a> {
    list: Vec<GenericTarget<'a>>,
    functions: HashMap<EcoString, usize>,
    methods: HashMap<(EcoString, EcoString), usize>,
    node_count: usize,
}

impl<'a> Targets<'a> {
    fn add(&mut self, mut target: GenericTarget<'a>) -> usize {
        let index = self.list.len();
        target.first_node = self.node_count;
        self.node_count += target.vars.len();
        self.list.push(target);
        index
    }
}

fn collect_generic_targets(store: &Store) -> Targets<'_> {
    let mut module_ids: Vec<&str> = store.modules.keys().map(String::as_str).collect();
    module_ids.sort_unstable();

    let mut targets = Targets::default();
    for module_id in module_ids {
        let module = &store.modules[module_id];
        let mut files: Vec<_> = module.files.values().collect();
        files.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        for file in files {
            for item in &file.items {
                match item {
                    Expression::Function {
                        name,
                        generics,
                        params,
                        return_type,
                        body,
                        ..
                    } if !generics.is_empty() => {
                        let index = targets.add(GenericTarget {
                            name,
                            vars: generics.iter().map(|g| &g.name).collect(),
                            declared_self: None,
                            declared_params: params.iter().map(|p| &p.ty).collect(),
                            declared_return: return_type,
                            body,
                            first_node: 0,
                        });
                        let qualified = Symbol::from_parts(module_id, name).as_eco().clone();
                        targets.functions.insert(qualified, index);
                    }
                    Expression::ImplBlock {
                        receiver_name,
                        generics: impl_generics,
                        methods,
                        ..
                    } => {
                        for method in methods {
                            collect_method(
                                method,
                                impl_generics,
                                receiver_name,
                                module_id,
                                &mut targets,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    targets
}

fn collect_method<'a>(
    method: &'a Expression,
    impl_generics: &'a [syntax::ast::Generic],
    receiver_name: &EcoString,
    module_id: &str,
    targets: &mut Targets<'a>,
) {
    let Expression::Function {
        name,
        generics,
        params,
        return_type,
        body,
        ..
    } = method
    else {
        return;
    };
    let vars: Vec<&EcoString> = impl_generics
        .iter()
        .chain(generics)
        .map(|g| &g.name)
        .collect();
    if vars.is_empty() {
        return;
    }
    let (declared_self, declared_params) = split_self_param(params);
    let receiver_id = declared_self.and_then(receiver_type_id).unwrap_or_else(|| {
        Symbol::from_parts(module_id, receiver_name)
            .as_eco()
            .clone()
    });
    let index = targets.add(GenericTarget {
        name,
        vars,
        declared_self,
        declared_params,
        declared_return: return_type,
        body,
        first_node: 0,
    });
    // Static-style references collapse to a plain identifier qualified as
    // `module.Type.method`.
    let qualified = Symbol::from_raw(receiver_id.clone())
        .with_segment(name)
        .as_eco()
        .clone();
    targets.functions.insert(qualified, index);
    targets.methods.insert((receiver_id, name.clone()), index);
}

fn split_self_param(params: &[Binding]) -> (Option<&Type>, Vec<&Type>) {
    match params.split_first() {
        Some((first, rest)) if is_self_binding(first) => {
            (Some(&first.ty), rest.iter().map(|p| &p.ty).collect())
        }
        _ => (None, params.iter().map(|p| &p.ty).collect()),
    }
}

fn is_self_binding(binding: &Binding) -> bool {
    matches!(&binding.pattern, Pattern::Identifier { identifier, .. } if identifier == "self")
}

fn receiver_type_id(ty: &Type) -> Option<EcoString> {
    match ty {
        Type::Compound {
            kind: CompoundKind::Ref,
            args,
        } => args.first().and_then(receiver_type_id),
        Type::Nominal { id, .. } => Some(id.as_eco().clone()),
        _ => None,
    }
}

struct GrowingEdge {
    source: usize,
    target: usize,
    span: Span,
    var: EcoString,
    type_arg: Type,
    target_name: EcoString,
}

struct EdgeCollector<'s, 'a> {
    targets: &'s Targets<'a>,
    adjacency: Vec<Vec<usize>>,
    growing: Vec<GrowingEdge>,
    current: usize,
}

impl<'a> EdgeCollector<'_, 'a> {
    fn walk(&mut self, expression: &'a Expression) {
        match expression {
            // A nested function declaration is a hard error elsewhere, and its
            // type parameters shadow the enclosing ones, so references in its
            // body must not be attributed to the enclosing generic context.
            Expression::Function { .. } => {}
            Expression::Call {
                expression: callee,
                args,
                spread,
                span,
                ..
            } => {
                match callee.as_ref() {
                    reference @ (Expression::Identifier { .. } | Expression::DotAccess { .. }) => {
                        self.process_reference(reference, *span);
                        if let Expression::DotAccess {
                            expression: base, ..
                        } = reference
                        {
                            self.walk(base);
                        }
                    }
                    other => self.walk(other),
                }
                for argument in args {
                    self.walk(argument);
                }
                if let Some(spread) = spread.as_ref() {
                    self.walk(spread);
                }
            }
            Expression::Identifier { span, .. } => self.process_reference(expression, *span),
            Expression::DotAccess {
                expression: base,
                span,
                ..
            } => {
                self.process_reference(expression, *span);
                self.walk(base);
            }
            _ => {
                for child in expression.children() {
                    self.walk(child);
                }
            }
        }
    }

    fn process_reference(&mut self, reference: &'a Expression, span: Span) {
        match reference {
            Expression::Identifier {
                qualified: Some(qualified),
                ty,
                ..
            } => {
                if let Some(&target_index) = self.targets.functions.get(qualified)
                    && let Type::Function(instantiated) = ty
                {
                    self.match_and_emit(target_index, None, instantiated, span);
                }
            }
            Expression::DotAccess {
                expression: base,
                member,
                ty,
                dot_access_kind,
                ..
            } => self.process_method_reference(base, member, ty, dot_access_kind, span),
            _ => {}
        }
    }

    fn process_method_reference(
        &mut self,
        base: &'a Expression,
        member: &EcoString,
        ty: &'a Type,
        dot_access_kind: &Option<DotAccessKind>,
        span: Span,
    ) {
        let is_instance = match dot_access_kind {
            Some(DotAccessKind::InstanceMethod { .. })
            | Some(DotAccessKind::InstanceMethodValue { .. }) => true,
            Some(DotAccessKind::StaticMethod { .. }) => false,
            _ => return,
        };
        let base_ty = base.get_type();
        let receiver_id = receiver_type_id(&base_ty).or_else(|| match base {
            Expression::Identifier {
                qualified: Some(qualified),
                ..
            } => Some(qualified.clone()),
            _ => None,
        });
        let Some(receiver_id) = receiver_id else {
            return;
        };
        let Some(&target_index) = self.targets.methods.get(&(receiver_id, member.clone())) else {
            return;
        };
        let Type::Function(instantiated) = ty else {
            return;
        };
        let receiver = is_instance.then_some(&base_ty);
        self.match_and_emit(target_index, receiver, instantiated, span);
    }

    fn match_and_emit(
        &mut self,
        target_index: usize,
        receiver: Option<&Type>,
        instantiated: &FunctionType,
        span: Span,
    ) {
        let targets = self.targets;
        let target = &targets.list[target_index];
        let declared_self = target.declared_self.map(Type::strip_refs);
        let receiver = receiver.map(Type::strip_refs);

        let mut bindings: HashMap<EcoString, &Type> = HashMap::default();
        let actual_params: &[Type] = if instantiated.params.len() == target.declared_params.len() {
            if let (Some(declared_self), Some(receiver)) = (&declared_self, &receiver) {
                bind_type_arguments(declared_self, receiver, &target.vars, &mut bindings);
            }
            &instantiated.params
        } else if instantiated.params.len() == target.declared_params.len() + 1
            && declared_self.is_some()
        {
            // Method used with an explicit receiver slot, for example as a
            // first-class value. The receiver is the first parameter.
            if let Some(declared_self) = &declared_self {
                bind_type_arguments(
                    declared_self,
                    &instantiated.params[0],
                    &target.vars,
                    &mut bindings,
                );
            }
            &instantiated.params[1..]
        } else {
            return;
        };
        for (declared, actual) in target.declared_params.iter().zip(actual_params) {
            bind_type_arguments(declared, actual, &target.vars, &mut bindings);
        }
        bind_type_arguments(
            target.declared_return,
            &instantiated.return_type,
            &target.vars,
            &mut bindings,
        );

        let source = &targets.list[self.current];
        for (target_position, var) in target.vars.iter().enumerate() {
            let Some(&type_arg) = bindings.get(*var) else {
                continue;
            };
            let target_node = target.first_node + target_position;
            for (source_position, source_var) in source.vars.iter().enumerate() {
                let is_same = matches!(type_arg, Type::Parameter(name) if name == *source_var);
                if !is_same && !mentions_parameter(type_arg, source_var) {
                    continue;
                }
                let source_node = source.first_node + source_position;
                self.adjacency[source_node].push(target_node);
                if !is_same {
                    self.growing.push(GrowingEdge {
                        source: source_node,
                        target: target_node,
                        span,
                        var: (*var).clone(),
                        type_arg: type_arg.clone(),
                        target_name: target.name.clone(),
                    });
                }
            }
        }
    }
}

/// Recovers the type argument bound to each of the target's type parameters
/// by walking the declared and instantiated signatures in parallel.
fn bind_type_arguments<'t>(
    declared: &Type,
    actual: &'t Type,
    vars: &[&EcoString],
    bindings: &mut HashMap<EcoString, &'t Type>,
) {
    if let Type::Parameter(name) = declared {
        if vars.contains(&name) {
            bindings.entry(name.clone()).or_insert(actual);
        }
        return;
    }
    if !same_shape(declared, actual) {
        return;
    }
    let declared_children = structural_children(declared);
    let actual_children = structural_children(actual);
    if declared_children.len() == actual_children.len() {
        for (declared, actual) in declared_children.into_iter().zip(actual_children) {
            bind_type_arguments(declared, actual, vars, bindings);
        }
    }
}

fn same_shape(declared: &Type, actual: &Type) -> bool {
    match (declared, actual) {
        (Type::Compound { kind: declared, .. }, Type::Compound { kind: actual, .. }) => {
            declared == actual
        }
        (Type::Nominal { id: declared, .. }, Type::Nominal { id: actual, .. }) => {
            declared == actual
        }
        (Type::Function(_), Type::Function(_)) | (Type::Tuple(_), Type::Tuple(_)) => true,
        _ => false,
    }
}

/// Child positions matched pairwise between the declared and instantiated
/// sides. Excludes `Nominal.underlying_ty`, which can be present on only one
/// side.
fn structural_children(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Compound { args, .. } => args.iter().collect(),
        Type::Nominal { params, .. } => params.iter().collect(),
        Type::Function(function) => function
            .params
            .iter()
            .chain(std::iter::once(function.return_type.as_ref()))
            .collect(),
        Type::Tuple(elements) => elements.iter().collect(),
        _ => Vec::new(),
    }
}

fn mentions_parameter(ty: &Type, name: &EcoString) -> bool {
    match ty {
        Type::Parameter(parameter) => parameter == name,
        _ => ty
            .children()
            .iter()
            .any(|child| mentions_parameter(child, name)),
    }
}

fn reaches(adjacency: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut visited = vec![false; adjacency.len()];
    let mut stack = vec![from];
    while let Some(node) = stack.pop() {
        if node == to {
            return true;
        }
        if visited[node] {
            continue;
        }
        visited[node] = true;
        stack.extend(adjacency[node].iter().copied());
    }
    false
}
