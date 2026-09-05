use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

use syntax::ast::{Generic, Visibility};
use syntax::go_names;
use syntax::program::{
    Definition, DefinitionBody, Method, Methods, method_for_type, methods_for_type,
};
use syntax::types::{
    self, CompoundKind, Symbol, Type, build_named_substitution_map, build_substitution_map,
    substitute,
};

use crate::store::Store;

#[derive(Clone, Debug)]
pub enum MemberKind {
    Field {
        ty: Type,
        visibility: Visibility,
    },
    /// `ty` already carries the effective receiver: value embeds keep the
    /// declared receiver, promoted methods are re-pointed at the embedder.
    Method(Method),
}

#[derive(Clone, Debug)]
pub struct ResolvedMember {
    pub(crate) depth: usize,
    pub declaring_type: Symbol,
    pub kind: MemberKind,
}

#[derive(Clone, Debug)]
pub enum Resolution {
    Found(ResolvedMember),
    Ambiguous { sources: Vec<Symbol> },
    NotFound,
}

pub fn has_direct_embed(store: &Store, ty: &Type) -> bool {
    has_embed(ty, |id| store.get_definition(id))
}

/// Whether `ty` embeds anything: the cheap precondition for `walk`.
fn has_embed<'d, F>(ty: &Type, lookup: F) -> bool
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let Type::Nominal { id, .. } = types::peel_alias(&ty.strip_refs(), lookup) else {
        return false;
    };
    lookup(id.as_str())
        .and_then(Definition::fields)
        .is_some_and(|fields| fields.iter().any(|field| field.is_embedded()))
}

pub fn resolve_selector(store: &Store, outer: &Type, name: &str) -> Resolution {
    let lookup = |id: &str| store.get_definition(id);
    let entries = walk(outer, lookup);
    resolve_in_entries(&entries, outer, name, lookup)
}

/// One promoted or own method, without building the whole promoted set.
pub(crate) fn promoted_method(store: &Store, outer: &Type, name: &str) -> Option<Method> {
    match resolve_selector(store, outer, name) {
        Resolution::Found(ResolvedMember {
            kind: MemberKind::Method(method),
            ..
        }) => Some(method),
        Resolution::Found(_) | Resolution::Ambiguous { .. } | Resolution::NotFound => None,
    }
}

pub(crate) fn promoted_method_set(store: &Store, outer: &Type) -> Methods {
    member_set(outer, |id| store.get_definition(id))
        .into_iter()
        .filter_map(|(name, member)| match member.kind {
            MemberKind::Method(method) => Some((name, method)),
            MemberKind::Field { .. } => None,
        })
        .collect()
}

/// The members `outer` gains from its embeds. Its own members are already
/// reachable, and an ambiguous name is not a valid selector.
pub fn promoted_members<'d, F>(outer: &Type, lookup: F) -> Vec<(EcoString, ResolvedMember)>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    if !has_embed(outer, lookup) {
        return Vec::new();
    }

    member_set(outer, lookup)
        .into_iter()
        .filter(|(_, member)| member.depth > 0)
        .collect()
}

/// Every selector `outer` resolves, own and promoted, one winner per name.
fn member_set<'d, F>(outer: &Type, lookup: F) -> Vec<(EcoString, ResolvedMember)>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let entries = walk(outer, lookup);

    let mut names: HashSet<EcoString> = HashSet::default();
    for entry in &entries {
        collect_member_names(&entry.ty, lookup, &mut names);
    }

    names
        .into_iter()
        .filter_map(
            |name| match resolve_in_entries(&entries, outer, &name, lookup) {
                Resolution::Found(member) => Some((name, member)),
                Resolution::Ambiguous { .. } | Resolution::NotFound => None,
            },
        )
        .collect()
}

/// Shallowest depth at which any field of `outer` emits `selector` as its Go name.
pub(crate) fn field_selector_depth(store: &Store, outer: &Type, selector: &str) -> Option<usize> {
    let lookup = |id: &str| store.get_definition(id);
    let mut min: Option<usize> = None;
    for entry in walk(outer, lookup) {
        let Some(id) = entry.ty.get_qualified_id() else {
            continue;
        };
        let Some(definition) = lookup(id) else {
            continue;
        };
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            continue;
        };
        let forces_export = definition.is_serialized();
        let claimed = fields
            .iter()
            .any(|field| go_names::struct_field_go_name(field, forces_export) == selector);
        if claimed && min.is_none_or(|m| entry.depth < m) {
            min = Some(entry.depth);
        }
    }
    min
}

#[derive(Clone)]
struct Entry {
    ty: Type,
    depth: usize,
    /// A pointer edge was crossed on the path to this subobject.
    indirect: bool,
    /// Reached by more than one path at this depth, so its members collide.
    multiples: bool,
}

fn walk<'d, F>(outer: &Type, lookup: F) -> Vec<Entry>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let mut visited: Vec<Entry> = Vec::new();
    let mut seen: HashSet<Type> = HashSet::default();

    let Some(root) = nominal_entry(outer.clone(), 0, false, false, lookup) else {
        return visited;
    };
    let mut current = vec![root];
    let mut depth = 0;

    while !current.is_empty() {
        let mut next: Vec<Entry> = Vec::new();

        for entry in &current {
            // Seen at a shallower depth: shadows here, and breaks cycles.
            if !seen.insert(entry.ty.clone()) {
                continue;
            }
            visited.push(entry.clone());

            // Interfaces contribute their method set but have no fields to descend.
            let Some(id) = entry.ty.get_qualified_id() else {
                continue;
            };
            if lookup(id).is_some_and(Definition::is_interface) {
                continue;
            }
            let Some(fields) = lookup(id).and_then(Definition::fields) else {
                continue;
            };
            for field in fields {
                if !field.is_embedded() {
                    continue;
                }
                let field_ty = instantiate_field(&entry.ty, &field.ty, lookup);
                let resolved_field = types::peel_alias(&field_ty, lookup);
                let (target, is_pointer) = deref_once(&resolved_field);
                if let Some(child) = nominal_entry(
                    target,
                    depth + 1,
                    entry.indirect || is_pointer,
                    entry.multiples,
                    lookup,
                ) {
                    next.push(child);
                }
            }
        }

        current = consolidate(next);
        depth += 1;
    }

    visited
}

/// Resolve `name` to its shallowest candidate; a lone non-`multiples` hit wins,
/// anything else is ambiguous.
fn resolve_in_entries<'d, F>(entries: &[Entry], outer: &Type, name: &str, lookup: F) -> Resolution
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let mut by_depth: BTreeMap<usize, Vec<(&Entry, Candidate)>> = BTreeMap::new();
    for entry in entries {
        if let Some(candidate) = entry_candidate(&entry.ty, name, lookup) {
            by_depth
                .entry(entry.depth)
                .or_default()
                .push((entry, candidate));
        }
    }

    let Some((_, candidates)) = by_depth.into_iter().next() else {
        return Resolution::NotFound;
    };

    if let [(entry, candidate)] = candidates.as_slice()
        && !entry.multiples
    {
        return Resolution::Found(build_member(outer, entry, candidate));
    }

    let mut sources: Vec<Symbol> = candidates
        .iter()
        .map(|(_, c)| c.declaring_type.clone())
        .collect();
    sources.sort();
    sources.dedup();
    Resolution::Ambiguous { sources }
}

struct Candidate {
    declaring_type: Symbol,
    kind: MemberKind,
}

/// The field or method a type declares under `name`. A method shadows a
/// same-named field, as gc checks attached methods first.
fn entry_candidate<'d, F>(ty: &Type, name: &str, lookup: F) -> Option<Candidate>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let id = ty.get_qualified_id()?;

    if lookup(id).is_some_and(Definition::is_interface) {
        let method = method_for_type(ty, &Default::default(), lookup, name)?;
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            kind: MemberKind::Method(method),
        });
    }

    if !lookup(id).is_some_and(|d| d.is_ufcs_method(name))
        && let Some(method) = lookup(id)
            .and_then(Definition::methods)
            .and_then(|m| m.get(name))
    {
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            kind: MemberKind::Method(method.with_type(instantiate_method(ty, &method.ty, lookup))),
        });
    }

    if let Some(field) = lookup(id)
        .and_then(Definition::fields)
        .and_then(|fields| fields.iter().find(|f| f.name == name))
    {
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            kind: MemberKind::Field {
                ty: instantiate_field(ty, &field.ty, lookup),
                visibility: field.visibility,
            },
        });
    }

    None
}

fn build_member(outer: &Type, entry: &Entry, candidate: &Candidate) -> ResolvedMember {
    let kind = match &candidate.kind {
        MemberKind::Field { ty, visibility } => MemberKind::Field {
            ty: ty.clone(),
            visibility: *visibility,
        },
        MemberKind::Method(method) => {
            let receiver_writable = receiver_type(&method.ty).is_some_and(Type::is_writable);
            let method_ty = if entry.depth == 0 {
                method.ty.clone()
            } else {
                let replacement = if !entry.indirect && method_has_pointer_receiver(&method.ty) {
                    ref_of(outer)
                } else {
                    outer.clone()
                };
                let replacement = if receiver_writable {
                    replacement.make_writable()
                } else {
                    replacement
                };
                method.ty.with_replaced_first_param(&replacement)
            };
            MemberKind::Method(method.with_type(method_ty))
        }
    };

    ResolvedMember {
        depth: entry.depth,
        declaring_type: candidate.declaring_type.clone(),
        kind,
    }
}

/// Every field and method name a type exposes.
fn collect_member_names<'d, F>(ty: &Type, lookup: F, names: &mut HashSet<EcoString>)
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let Some(id) = ty.get_qualified_id() else {
        return;
    };
    if lookup(id).is_some_and(Definition::is_interface) {
        for key in methods_for_type(ty, &Default::default(), lookup).keys() {
            names.insert(key.clone());
        }
        return;
    }
    if let Some(methods) = lookup(id).and_then(Definition::methods) {
        for key in methods.keys() {
            names.insert(key.clone());
        }
    }
    if let Some(fields) = lookup(id).and_then(Definition::fields) {
        for field in fields {
            names.insert(field.name.clone());
        }
    }
}

/// Build an entry for `target` if it resolves (through aliases) to a nominal type.
fn nominal_entry<'d, F>(
    target: Type,
    depth: usize,
    indirect: bool,
    multiples: bool,
    lookup: F,
) -> Option<Entry>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let resolved = types::peel_alias(&target, lookup);
    if !matches!(resolved, Type::Nominal { .. }) {
        return None;
    }
    Some(Entry {
        ty: resolved,
        depth,
        indirect,
        multiples,
    })
}

/// gc's `consolidateMultiples`: dedup by type, flagging any reached by more than
/// one path so its members resolve as ambiguous.
fn consolidate(list: Vec<Entry>) -> Vec<Entry> {
    let mut result: Vec<Entry> = Vec::with_capacity(list.len());
    let mut index_of: HashMap<Type, usize> = HashMap::default();
    for entry in list {
        if let Some(&i) = index_of.get(&entry.ty) {
            result[i].multiples = true;
        } else {
            index_of.insert(entry.ty.clone(), result.len());
            result.push(entry);
        }
    }
    result
}

/// Strip one `Ref`, reporting whether it was present (a pointer edge).
fn deref_once(ty: &Type) -> (Type, bool) {
    if ty.is_ref() {
        (ty.inner().unwrap_or(Type::Error), true)
    } else {
        (ty.clone(), false)
    }
}

fn method_has_pointer_receiver(method_ty: &Type) -> bool {
    matches!(receiver_type(method_ty), Some(ty) if ty.is_ref())
}

fn receiver_type(method_ty: &Type) -> Option<&Type> {
    let body = match method_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    match body {
        Type::Function(f) => f.params.first().map(|param| &param.ty),
        _ => None,
    }
}

fn ref_of(ty: &Type) -> Type {
    Type::compound(CompoundKind::Ref, vec![ty.clone()])
}

fn declaring_generics<'d, F>(id: &str, lookup: F) -> &'d [Generic]
where
    F: Fn(&str) -> Option<&'d Definition>,
{
    match lookup(id).map(|d| &d.body) {
        Some(
            DefinitionBody::Struct { generics, .. }
            | DefinitionBody::Enum { generics, .. }
            | DefinitionBody::TypeAlias { generics, .. },
        ) => generics,
        Some(DefinitionBody::Interface { definition }) => &definition.generics,
        _ => &[],
    }
}

fn instantiate_field<'d, F>(container: &Type, member_ty: &Type, lookup: F) -> Type
where
    F: Fn(&str) -> Option<&'d Definition>,
{
    let Some(id) = container.get_qualified_id() else {
        return member_ty.clone();
    };
    let args = container.get_type_params().unwrap_or_default();
    if args.is_empty() {
        return member_ty.clone();
    }
    substitute(
        member_ty,
        &build_substitution_map(declaring_generics(id, lookup), args),
    )
}

fn instantiate_method<'d, F>(container: &Type, method_ty: &Type, lookup: F) -> Type
where
    F: Fn(&str) -> Option<&'d Definition>,
{
    let Some(id) = container.get_qualified_id() else {
        return method_ty.clone();
    };
    let arity = declaring_generics(id, lookup).len();
    let args = container.get_type_params().unwrap_or_default();
    if args.is_empty() || arity == 0 {
        return method_ty.clone();
    }
    let Type::Forall { vars, body } = method_ty else {
        return method_ty.clone();
    };
    let map = build_named_substitution_map(vars, args);
    substitute(body, &map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::{Annotation, Span, StructFieldDefinition, StructFieldKind, StructFields};
    use syntax::program::MethodOrigin;
    use syntax::program::Visibility as ProgVis;
    use syntax::program::{Attributes, Definition, DefinitionBody, Interface};
    use syntax::types::FunctionParameter;

    const PACKAGE: &str = "m";

    fn nominal(name: &str) -> Type {
        Type::Nominal {
            id: Symbol::from_parts(PACKAGE, name),
            params: vec![],
            writable: false,
        }
    }

    fn value_method(owner: &str) -> Type {
        Type::function(
            vec![FunctionParameter::new(nominal(owner))],
            vec![],
            Box::new(Type::string()),
        )
    }

    fn pointer_method(owner: &str) -> Type {
        Type::function(
            vec![FunctionParameter::new(ref_of(&nominal(owner)))],
            vec![],
            Box::new(Type::string()),
        )
    }

    fn interface_method() -> Type {
        Type::function(vec![], vec![], Box::new(Type::string()))
    }

    fn field(name: &str, ty: Type, embedded: bool) -> StructFieldDefinition {
        StructFieldDefinition {
            doc: None,
            name: name.into(),
            name_span: Span::dummy(),
            annotation: Annotation::Unknown,
            visibility: Visibility::Public,
            ty,
            kind: if embedded {
                StructFieldKind::Embedded
            } else {
                StructFieldKind::Named { attributes: vec![] }
            },
        }
    }

    struct Builder {
        store: Store,
    }

    impl Builder {
        fn new() -> Self {
            let mut store = Store::new();
            store.add_package(PACKAGE);
            Builder { store }
        }

        fn insert(&mut self, name: &str, body: DefinitionBody) -> &mut Self {
            let def = Definition {
                visibility: ProgVis::Public,
                ty: nominal(name),
                name_span: None,
                doc: None,
                body,
            };
            self.store
                .get_package_mut(PACKAGE)
                .unwrap()
                .definitions
                .insert(Symbol::from_parts(PACKAGE, name), def);
            self
        }

        fn struct_(
            &mut self,
            name: &str,
            fields: Vec<StructFieldDefinition>,
            methods: Vec<(&str, Type)>,
        ) -> &mut Self {
            let mut method_map = Methods::default();
            for (n, t) in methods {
                method_map.insert(
                    n.into(),
                    Method {
                        source_name: n.into(),
                        ty: t,
                        visibility: ProgVis::Public,
                        origin: MethodOrigin::Declared,
                        name_span: None,
                        doc: None,
                        allowed_lints: vec![],
                        go_hints: vec![],
                        superseded_by: None,
                    },
                );
            }
            self.insert(
                name,
                DefinitionBody::Struct {
                    generics: vec![],
                    fields: StructFields::Record(fields),
                    methods: method_map,
                    attributes: Attributes::default(),
                },
            )
        }

        fn generic_struct(
            &mut self,
            name: &str,
            generics: Vec<&str>,
            fields: Vec<StructFieldDefinition>,
            methods: Vec<(&str, Type)>,
        ) -> &mut Self {
            let mut method_map = Methods::default();
            for (n, t) in methods {
                method_map.insert(
                    n.into(),
                    Method {
                        source_name: n.into(),
                        ty: t,
                        visibility: ProgVis::Public,
                        origin: MethodOrigin::Declared,
                        name_span: None,
                        doc: None,
                        allowed_lints: vec![],
                        go_hints: vec![],
                        superseded_by: None,
                    },
                );
            }
            self.insert(
                name,
                DefinitionBody::Struct {
                    generics: generics
                        .into_iter()
                        .map(|g| Generic::new(g, vec![], Span::dummy()))
                        .collect(),
                    fields: StructFields::Record(fields),
                    methods: method_map,
                    attributes: Attributes::default(),
                },
            )
        }

        fn interface(&mut self, name: &str, methods: Vec<&str>, parents: Vec<&str>) -> &mut Self {
            let mut method_map = Methods::default();
            for n in methods {
                method_map.insert(
                    n.into(),
                    Method {
                        source_name: n.into(),
                        ty: interface_method(),
                        visibility: ProgVis::Public,
                        origin: MethodOrigin::Declared,
                        name_span: None,
                        doc: None,
                        allowed_lints: vec![],
                        go_hints: vec![],
                        superseded_by: None,
                    },
                );
            }
            self.insert(
                name,
                DefinitionBody::Interface {
                    definition: Interface {
                        generics: vec![],
                        parents: parents.into_iter().map(nominal).collect(),
                        methods: method_map,
                    },
                },
            )
        }
    }

    fn vembed(target: &str) -> StructFieldDefinition {
        field(target, nominal(target), true)
    }

    fn pembed(target: &str) -> StructFieldDefinition {
        field(target, ref_of(&nominal(target)), true)
    }

    fn param(name: &str) -> Type {
        Type::Parameter(name.into())
    }

    fn generic_nominal(name: &str, args: Vec<Type>) -> Type {
        Type::Nominal {
            id: Symbol::from_parts(PACKAGE, name),
            params: args,
            writable: false,
        }
    }

    fn generic_value_method(owner: &str, impl_var: &str, ret: Type) -> Type {
        Type::Forall {
            vars: vec![impl_var.into()],
            body: Box::new(Type::function(
                vec![FunctionParameter::new(generic_nominal(
                    owner,
                    vec![param(impl_var)],
                ))],
                vec![],
                Box::new(ret),
            )),
        }
    }

    fn found(resolution: Resolution) -> ResolvedMember {
        match resolution {
            Resolution::Found(member) => member,
            other => panic!("expected Found, got {other:?}"),
        }
    }

    fn is_pointer_receiver(member: &ResolvedMember) -> bool {
        match &member.kind {
            MemberKind::Method(method) => method.ty.get_function_params().unwrap()[0].ty.is_ref(),
            other => panic!("expected a method, got {other:?}"),
        }
    }

    #[test]
    fn direct_method_at_depth_zero() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        let member = found(resolve_selector(&b.store, &nominal("N0"), "m"));
        assert_eq!(member.depth, 0);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn value_embed_promotes_value_method() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "m"));
        assert_eq!(member.depth, 1);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn value_embed_of_pointer_method_is_pointer_only() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "pm"));
        assert!(is_pointer_receiver(&member));
    }

    #[test]
    fn pointer_embed_puts_pointer_method_in_value_set() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![pembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "pm"));
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn pointer_embed_of_value_method_is_value() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![pembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "m"));
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn three_value_edges_keep_pointer_method_pointer_only() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N2", vec![vembed("N1")], vec![]);
        b.struct_("N3", vec![vembed("N2")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N3"), "pm"));
        assert_eq!(member.depth, 3);
        assert!(is_pointer_receiver(&member));
    }

    #[test]
    fn pointer_edge_mid_three_level_path_puts_pointer_method_in_value_set() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N2", vec![pembed("N1")], vec![]);
        b.struct_("N3", vec![vembed("N2")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N3"), "pm"));
        assert_eq!(member.depth, 3);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn value_method_promotes_through_three_value_edges() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N2", vec![vembed("N1")], vec![]);
        b.struct_("N3", vec![vembed("N2")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N3"), "m"));
        assert_eq!(member.depth, 3);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn diamond_is_ambiguous() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N2", vec![vembed("N0")], vec![]);
        b.struct_("N3", vec![vembed("N1"), vembed("N2")], vec![]);
        assert!(matches!(
            resolve_selector(&b.store, &nominal("N3"), "m"),
            Resolution::Ambiguous { .. }
        ));
    }

    #[test]
    fn shallower_path_shadows_deeper_diamond() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N3", vec![vembed("N0"), vembed("N1")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N3"), "m"));
        assert_eq!(member.depth, 1);
    }

    #[test]
    fn own_member_shadows_promoted() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![("m", value_method("N1"))]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "m"));
        assert_eq!(member.depth, 0);
        assert_eq!(member.declaring_type.as_str(), "m.N1");
    }

    #[test]
    fn field_promotes() {
        let mut b = Builder::new();
        b.struct_("N0", vec![field("f", Type::int(), false)], vec![]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "f"));
        assert_eq!(member.depth, 1);
        assert!(matches!(member.kind, MemberKind::Field { .. }));
    }

    #[test]
    fn field_and_method_collide_across_embeds() {
        let mut b = Builder::new();
        b.struct_("A", vec![field("x", Type::int(), false)], vec![]);
        b.struct_("B", vec![], vec![("x", value_method("B"))]);
        b.struct_("N2", vec![vembed("A"), vembed("B")], vec![]);
        assert!(matches!(
            resolve_selector(&b.store, &nominal("N2"), "x"),
            Resolution::Ambiguous { .. }
        ));
    }

    #[test]
    fn pointer_cycle_terminates_and_resolves() {
        let mut b = Builder::new();
        b.struct_("N0", vec![pembed("N1")], vec![("a", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![("bb", value_method("N1"))]);
        assert_eq!(
            found(resolve_selector(&b.store, &nominal("N0"), "a")).depth,
            0
        );
        assert_eq!(
            found(resolve_selector(&b.store, &nominal("N0"), "bb")).depth,
            1
        );
        assert_eq!(
            found(resolve_selector(&b.store, &nominal("N1"), "a")).depth,
            1
        );
        assert!(matches!(
            resolve_selector(&b.store, &nominal("N0"), "absent"),
            Resolution::NotFound
        ));
    }

    #[test]
    fn embedded_interface_promotes_value_callable() {
        let mut b = Builder::new();
        b.interface("I", vec!["speak"], vec![]);
        b.struct_("N2", vec![vembed("I")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N2"), "speak"));
        assert_eq!(member.depth, 1);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn struct_embedding_interface_and_struct_with_same_method_is_ambiguous() {
        let mut b = Builder::new();
        b.interface("I", vec!["speak"], vec![]);
        b.struct_("S", vec![], vec![("speak", value_method("S"))]);
        b.struct_("N2", vec![vembed("I"), vembed("S")], vec![]);
        assert!(matches!(
            resolve_selector(&b.store, &nominal("N2"), "speak"),
            Resolution::Ambiguous { .. }
        ));
        assert!(!promoted_method_set(&b.store, &nominal("N2")).contains_key("speak"));
    }

    #[test]
    fn method_set_includes_promoted_excludes_ambiguous() {
        let mut b = Builder::new();
        b.struct_(
            "N0",
            vec![],
            vec![("m", value_method("N0")), ("pm", pointer_method("N0"))],
        );
        b.struct_("N1", vec![vembed("N0")], vec![("o", value_method("N1"))]);
        let set = promoted_method_set(&b.store, &nominal("N1"));
        assert!(set.contains_key("o"));
        assert!(set.contains_key("m"));
        assert!(set.contains_key("pm"));
        assert!(
            !set.get("m").unwrap().ty.get_function_params().unwrap()[0]
                .ty
                .is_ref()
        );
        assert!(
            set.get("pm").unwrap().ty.get_function_params().unwrap()[0]
                .ty
                .is_ref()
        );
    }

    #[test]
    fn method_set_drops_ambiguous_diamond_member() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        b.struct_("N2", vec![vembed("N0")], vec![]);
        b.struct_("N3", vec![vembed("N1"), vembed("N2")], vec![]);
        assert!(!promoted_method_set(&b.store, &nominal("N3")).contains_key("m"));
    }

    #[test]
    fn has_direct_embed_detects_embeds() {
        let mut b = Builder::new();
        b.struct_("N0", vec![field("f", Type::int(), false)], vec![]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        assert!(!has_direct_embed(&b.store, &nominal("N0")));
        assert!(has_direct_embed(&b.store, &nominal("N1")));
        assert!(has_direct_embed(&b.store, &ref_of(&nominal("N1"))));
    }

    fn method_return(member: &ResolvedMember) -> Type {
        match &member.kind {
            MemberKind::Method(method) => method.ty.get_function_ret().unwrap().clone(),
            other => panic!("expected a method, got {other:?}"),
        }
    }

    #[test]
    fn generic_embed_promotes_field_at_instantiation() {
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![field("value", param("T"), false)],
            vec![],
        );
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::int()]),
                true,
            )],
            vec![],
        );
        let member = found(resolve_selector(&b.store, &nominal("Outer"), "value"));
        match member.kind {
            MemberKind::Field { ty, .. } => assert_eq!(ty, Type::int()),
            other => panic!("expected a field, got {other:?}"),
        }
    }

    #[test]
    fn generic_embed_promotes_method_at_instantiation() {
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![],
            vec![("get", generic_value_method("Box", "T", param("T")))],
        );
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::string()]),
                true,
            )],
            vec![],
        );
        let member = found(resolve_selector(&b.store, &nominal("Outer"), "get"));
        assert_eq!(method_return(&member), Type::string());
    }

    #[test]
    fn generic_embedder_flows_its_param_into_the_target() {
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![],
            vec![("get", generic_value_method("Box", "T", param("T")))],
        );
        b.generic_struct(
            "Outer",
            vec!["U"],
            vec![field("Box", generic_nominal("Box", vec![param("U")]), true)],
            vec![],
        );
        let member = found(resolve_selector(
            &b.store,
            &generic_nominal("Outer", vec![Type::int()]),
            "get",
        ));
        assert_eq!(method_return(&member), Type::int());
    }

    #[test]
    fn renamed_impl_param_is_captured() {
        // struct param is `T`, but the method's impl var is `V` (`impl<V> Box<V>`).
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![],
            vec![("get", generic_value_method("Box", "V", param("V")))],
        );
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::int()]),
                true,
            )],
            vec![],
        );
        let member = found(resolve_selector(&b.store, &nominal("Outer"), "get"));
        assert_eq!(method_return(&member), Type::int());
    }

    #[test]
    fn specialized_impl_method_is_skipped() {
        let specialized = Type::Forall {
            vars: vec!["V".into()],
            body: Box::new(Type::function(
                vec![FunctionParameter::new(generic_nominal(
                    "Box",
                    vec![Type::Compound {
                        kind: CompoundKind::Slice,
                        args: vec![param("V")],
                        writable: false,
                    }],
                ))],
                vec![],
                Box::new(param("V")),
            )),
        };
        let mut b = Builder::new();
        b.generic_struct("Box", vec!["T"], vec![], vec![("weird", specialized)]);
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::int()]),
                true,
            )],
            vec![],
        );
        assert!(matches!(
            resolve_selector(&b.store, &nominal("Outer"), "weird"),
            Resolution::NotFound
        ));
    }

    /// A concrete `impl Box<int> { fn only_int(self: Box<int>) -> int }`, stored
    /// without a `Forall` because it binds no type variables.
    fn concrete_int_method() -> Type {
        Type::function(
            vec![FunctionParameter::new(generic_nominal(
                "Box",
                vec![Type::int()],
            ))],
            vec![],
            Box::new(Type::int()),
        )
    }

    #[test]
    fn specialized_impl_does_not_promote_onto_other_instantiation() {
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![],
            vec![("only_int", concrete_int_method())],
        );
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::string()]),
                true,
            )],
            vec![],
        );
        assert!(matches!(
            resolve_selector(&b.store, &nominal("Outer"), "only_int"),
            Resolution::NotFound
        ));
    }

    #[test]
    fn specialized_impl_method_not_promoted_onto_matching_instantiation() {
        let mut b = Builder::new();
        b.generic_struct(
            "Box",
            vec!["T"],
            vec![],
            vec![("only_int", concrete_int_method())],
        );
        b.struct_(
            "Outer",
            vec![field(
                "Box",
                generic_nominal("Box", vec![Type::int()]),
                true,
            )],
            vec![],
        );
        assert!(matches!(
            resolve_selector(&b.store, &nominal("Outer"), "only_int"),
            Resolution::NotFound
        ));
    }
}
