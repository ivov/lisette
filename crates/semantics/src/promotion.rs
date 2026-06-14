use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

use syntax::ast::{Generic, Visibility};
use syntax::program::{DefinitionBody, MethodSignatures};
use syntax::types::{CompoundKind, Symbol, Type, build_substitution_map, substitute};

use crate::call_classification::is_ufcs_method_type;
use crate::store::Store;

#[derive(Clone, Debug)]
pub enum MemberKind {
    Field {
        ty: Type,
        visibility: Visibility,
    },
    /// `ty` already carries the effective receiver: value embeds keep the
    /// declared receiver, promoted methods are re-pointed at the embedder.
    Method {
        ty: Type,
    },
}

#[derive(Clone, Debug)]
pub struct ResolvedMember {
    pub name: EcoString,
    pub depth: usize,
    pub embed_path: Vec<EcoString>,
    pub declaring_type: Symbol,
    pub indirect: bool,
    pub kind: MemberKind,
}

#[derive(Clone, Debug)]
pub enum Resolution {
    Found(ResolvedMember),
    Ambiguous { sources: Vec<Symbol> },
    NotFound,
}

pub fn has_direct_embed(store: &Store, ty: &Type) -> bool {
    let Type::Nominal { id, .. } = store.deep_resolve_alias(&ty.strip_refs()) else {
        return false;
    };
    store
        .fields_of(id.as_str())
        .is_some_and(|fields| fields.iter().any(|f| f.embedded))
}

pub fn resolve_selector(store: &Store, outer: &Type, name: &str) -> Resolution {
    let entries = walk(store, outer);
    resolve_in_entries(store, &entries, outer, name)
}

pub fn promoted_method_set(store: &Store, outer: &Type) -> MethodSignatures {
    let entries = walk(store, outer);

    let mut names: HashSet<EcoString> = HashSet::default();
    for entry in &entries {
        collect_member_names(store, &entry.ty, &mut names);
    }

    let mut result = MethodSignatures::default();
    for name in names {
        if let Resolution::Found(member) = resolve_in_entries(store, &entries, outer, &name)
            && let MemberKind::Method { ty } = member.kind
        {
            result.insert(name, ty);
        }
    }
    result
}

#[derive(Clone)]
struct Entry {
    ty: Type,
    depth: usize,
    /// A pointer edge was crossed on the path to this subobject.
    indirect: bool,
    /// Reached by more than one path at this depth, so its members collide.
    multiples: bool,
    embed_path: Vec<EcoString>,
}

fn walk(store: &Store, outer: &Type) -> Vec<Entry> {
    let mut visited: Vec<Entry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::default();

    let Some(root) = nominal_entry(store, outer.clone(), 0, false, false, Vec::new()) else {
        return visited;
    };
    let mut current = vec![root];
    let mut depth = 0;

    while !current.is_empty() {
        let mut next: Vec<Entry> = Vec::new();

        for entry in &current {
            // Seen at a shallower depth: shadows here, and breaks cycles.
            if !seen.insert(type_key(&entry.ty)) {
                continue;
            }
            visited.push(entry.clone());

            // Interfaces contribute their method set but have no fields to descend.
            let Some(id) = entry.ty.get_qualified_id() else {
                continue;
            };
            if store.get_interface(id).is_some() {
                continue;
            }
            let Some(fields) = store.fields_of(id) else {
                continue;
            };
            for field in fields {
                if !field.embedded {
                    continue;
                }
                let field_ty = instantiate_field(store, &entry.ty, &field.ty);
                let resolved_field = store.deep_resolve_alias(&field_ty);
                let (target, is_pointer) = deref_once(&resolved_field);
                let mut path = entry.embed_path.clone();
                path.push(field.name.clone());
                if let Some(child) = nominal_entry(
                    store,
                    target,
                    depth + 1,
                    entry.indirect || is_pointer,
                    entry.multiples,
                    path,
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
fn resolve_in_entries(store: &Store, entries: &[Entry], outer: &Type, name: &str) -> Resolution {
    let mut by_depth: BTreeMap<usize, Vec<(&Entry, Candidate)>> = BTreeMap::new();
    for entry in entries {
        if let Some(candidate) = entry_candidate(store, &entry.ty, name) {
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
        return Resolution::Found(build_member(outer, name, entry, candidate));
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
    detail: CandidateDetail,
}

enum CandidateDetail {
    Field { ty: Type, visibility: Visibility },
    Method { ty: Type },
}

/// The field or method a type declares under `name`. A method shadows a
/// same-named field, as gc checks attached methods first.
fn entry_candidate(store: &Store, ty: &Type, name: &str) -> Option<Candidate> {
    let id = ty.get_qualified_id()?;

    if store.get_interface(id).is_some() {
        let methods = store.get_all_methods(ty, &Default::default());
        let method_ty = methods.get(name)?.clone();
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            detail: CandidateDetail::Method { ty: method_ty },
        });
    }

    if let Some(method_ty) = store.get_own_methods(id).and_then(|m| m.get(name)) {
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            detail: CandidateDetail::Method {
                ty: instantiate_method(store, ty, method_ty)?,
            },
        });
    }

    if let Some(field) = store
        .fields_of(id)
        .and_then(|fields| fields.iter().find(|f| f.name == name))
    {
        return Some(Candidate {
            declaring_type: Symbol::from_raw(id),
            detail: CandidateDetail::Field {
                ty: instantiate_field(store, ty, &field.ty),
                visibility: field.visibility,
            },
        });
    }

    None
}

fn build_member(outer: &Type, name: &str, entry: &Entry, candidate: &Candidate) -> ResolvedMember {
    let kind = match &candidate.detail {
        CandidateDetail::Field { ty, visibility } => MemberKind::Field {
            ty: ty.clone(),
            visibility: *visibility,
        },
        CandidateDetail::Method { ty } => {
            // Only promoted methods are re-pointed; a depth-0 receiver is already
            // the outer type, and rewriting it would break generics. A promoted
            // method stays pointer-only when its receiver is a pointer and no
            // pointer edge was crossed.
            let method_ty = if entry.depth == 0 {
                ty.clone()
            } else if !entry.indirect && method_has_pointer_receiver(ty) {
                ty.with_replaced_first_param(&ref_of(outer))
            } else {
                ty.with_replaced_first_param(outer)
            };
            MemberKind::Method { ty: method_ty }
        }
    };

    ResolvedMember {
        name: name.into(),
        depth: entry.depth,
        embed_path: entry.embed_path.clone(),
        declaring_type: candidate.declaring_type.clone(),
        indirect: entry.indirect,
        kind,
    }
}

/// Every field and method name a type exposes.
fn collect_member_names(store: &Store, ty: &Type, names: &mut HashSet<EcoString>) {
    let Some(id) = ty.get_qualified_id() else {
        return;
    };
    if store.get_interface(id).is_some() {
        for key in store.get_all_methods(ty, &Default::default()).keys() {
            names.insert(key.clone());
        }
        return;
    }
    if let Some(methods) = store.get_own_methods(id) {
        for key in methods.keys() {
            names.insert(key.clone());
        }
    }
    if let Some(fields) = store.fields_of(id) {
        for field in fields {
            names.insert(field.name.clone());
        }
    }
}

/// Build an entry for `target` if it resolves (through aliases) to a nominal type.
fn nominal_entry(
    store: &Store,
    target: Type,
    depth: usize,
    indirect: bool,
    multiples: bool,
    embed_path: Vec<EcoString>,
) -> Option<Entry> {
    let resolved = store.deep_resolve_alias(&target);
    if !matches!(resolved, Type::Nominal { .. }) {
        return None;
    }
    Some(Entry {
        ty: resolved,
        depth,
        indirect,
        multiples,
        embed_path,
    })
}

/// gc's `consolidateMultiples`: dedup by type, flagging any reached by more than
/// one path so its members resolve as ambiguous.
fn consolidate(list: Vec<Entry>) -> Vec<Entry> {
    let mut result: Vec<Entry> = Vec::with_capacity(list.len());
    let mut index_of: HashMap<String, usize> = HashMap::default();
    for entry in list {
        let key = type_key(&entry.ty);
        if let Some(&i) = index_of.get(&key) {
            result[i].multiples = true;
        } else {
            index_of.insert(key, result.len());
            result.push(entry);
        }
    }
    result
}

/// Type identity for `seen`/`multiples`: qualified id plus any type arguments.
fn type_key(ty: &Type) -> String {
    match ty {
        Type::Nominal { id, params, .. } if params.is_empty() => id.as_str().to_string(),
        Type::Nominal { id, params, .. } => {
            let args: Vec<String> = params.iter().map(type_key).collect();
            format!("{}<{}>", id, args.join(","))
        }
        other => other.to_string(),
    }
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
    let body = match method_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    matches!(body, Type::Function(f) if f.params.first().is_some_and(Type::is_ref))
}

fn ref_of(ty: &Type) -> Type {
    Type::Compound {
        kind: CompoundKind::Ref,
        args: vec![ty.clone()],
    }
}

fn declaring_generics<'a>(store: &'a Store, id: &str) -> &'a [Generic] {
    match store.get_definition(id).map(|d| &d.body) {
        Some(
            DefinitionBody::Struct { generics, .. }
            | DefinitionBody::Enum { generics, .. }
            | DefinitionBody::TypeAlias { generics, .. },
        ) => generics,
        Some(DefinitionBody::Interface { definition }) => &definition.generics,
        _ => &[],
    }
}

fn instantiate_field(store: &Store, container: &Type, member_ty: &Type) -> Type {
    let Some(id) = container.get_qualified_id() else {
        return member_ty.clone();
    };
    let args = container.get_type_params().unwrap_or_default();
    if args.is_empty() {
        return member_ty.clone();
    }
    substitute(
        member_ty,
        &build_substitution_map(declaring_generics(store, id), args),
    )
}

fn instantiate_method(store: &Store, container: &Type, method_ty: &Type) -> Option<Type> {
    let Some(id) = container.get_qualified_id() else {
        return Some(method_ty.clone());
    };
    let arity = declaring_generics(store, id).len();
    if is_ufcs_method_type(method_ty, arity) {
        return None;
    }
    let args = container.get_type_params().unwrap_or_default();
    if args.is_empty() || arity == 0 {
        return Some(method_ty.clone());
    }
    let Type::Forall { vars, body } = method_ty else {
        return Some(method_ty.clone());
    };
    let map: HashMap<EcoString, Type> = vars.iter().cloned().zip(args.iter().cloned()).collect();
    Some(substitute(body, &map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::{Annotation, Span, StructFieldDefinition, StructKind};
    use syntax::program::Visibility as ProgVis;
    use syntax::program::{Attributes, Definition, DefinitionBody, Interface};

    const MODULE: &str = "m";

    fn nominal(name: &str) -> Type {
        Type::Nominal {
            id: Symbol::from_parts(MODULE, name),
            params: vec![],
            underlying_ty: None,
        }
    }

    fn value_method(owner: &str) -> Type {
        Type::function(
            vec![nominal(owner)],
            vec![false],
            vec![],
            Box::new(Type::string()),
        )
    }

    fn pointer_method(owner: &str) -> Type {
        Type::function(
            vec![ref_of(&nominal(owner))],
            vec![false],
            vec![],
            Box::new(Type::string()),
        )
    }

    /// An interface method as stored after registration: receiver already stripped.
    fn interface_method() -> Type {
        Type::function(vec![], vec![], vec![], Box::new(Type::string()))
    }

    fn field(name: &str, ty: Type, embedded: bool) -> StructFieldDefinition {
        StructFieldDefinition {
            doc: None,
            attributes: vec![],
            name: name.into(),
            name_span: Span::dummy(),
            annotation: Annotation::Unknown,
            visibility: Visibility::Public,
            ty,
            embedded,
        }
    }

    struct Builder {
        store: Store,
    }

    impl Builder {
        fn new() -> Self {
            let mut store = Store::new();
            store.add_module(MODULE);
            Builder { store }
        }

        fn insert(&mut self, name: &str, body: DefinitionBody) -> &mut Self {
            let def = Definition {
                visibility: ProgVis::Public,
                ty: nominal(name),
                name: Some(name.into()),
                name_span: None,
                doc: None,
                body,
            };
            self.store
                .get_module_mut(MODULE)
                .unwrap()
                .definitions
                .insert(Symbol::from_parts(MODULE, name), def);
            self
        }

        fn struct_(
            &mut self,
            name: &str,
            fields: Vec<StructFieldDefinition>,
            methods: Vec<(&str, Type)>,
        ) -> &mut Self {
            let mut method_map = MethodSignatures::default();
            for (n, t) in methods {
                method_map.insert(n.into(), t);
            }
            self.insert(
                name,
                DefinitionBody::Struct {
                    generics: vec![],
                    fields,
                    kind: StructKind::Record,
                    methods: method_map,
                    constructor: None,
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
            let mut method_map = MethodSignatures::default();
            for (n, t) in methods {
                method_map.insert(n.into(), t);
            }
            self.insert(
                name,
                DefinitionBody::Struct {
                    generics: generics
                        .into_iter()
                        .map(|g| Generic {
                            name: g.into(),
                            bounds: vec![],
                            span: Span::dummy(),
                        })
                        .collect(),
                    fields,
                    kind: StructKind::Record,
                    methods: method_map,
                    constructor: None,
                    attributes: Attributes::default(),
                },
            )
        }

        fn interface(&mut self, name: &str, methods: Vec<&str>, parents: Vec<&str>) -> &mut Self {
            let mut method_map = MethodSignatures::default();
            for n in methods {
                method_map.insert(n.into(), interface_method());
            }
            self.insert(
                name,
                DefinitionBody::Interface {
                    definition: Interface {
                        name: name.into(),
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
            id: Symbol::from_parts(MODULE, name),
            params: args,
            underlying_ty: None,
        }
    }

    fn generic_value_method(owner: &str, impl_var: &str, ret: Type) -> Type {
        Type::Forall {
            vars: vec![impl_var.into()],
            body: Box::new(Type::function(
                vec![generic_nominal(owner, vec![param(impl_var)])],
                vec![false],
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
            MemberKind::Method { ty } => ty.get_function_params().unwrap()[0].is_ref(),
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
        assert_eq!(member.embed_path, vec![EcoString::from("N0")]);
        assert!(!member.indirect);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn value_embed_of_pointer_method_is_pointer_only() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![vembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "pm"));
        assert!(!member.indirect);
        assert!(is_pointer_receiver(&member));
    }

    #[test]
    fn pointer_embed_puts_pointer_method_in_value_set() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("pm", pointer_method("N0"))]);
        b.struct_("N1", vec![pembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "pm"));
        assert!(member.indirect);
        assert!(!is_pointer_receiver(&member));
    }

    #[test]
    fn pointer_embed_of_value_method_is_value() {
        let mut b = Builder::new();
        b.struct_("N0", vec![], vec![("m", value_method("N0"))]);
        b.struct_("N1", vec![pembed("N0")], vec![]);
        let member = found(resolve_selector(&b.store, &nominal("N1"), "m"));
        assert!(member.indirect);
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
        assert!(!member.indirect);
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
        assert!(member.indirect);
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
        assert!(!member.indirect);
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
        assert!(!set.get("m").unwrap().get_function_params().unwrap()[0].is_ref());
        assert!(set.get("pm").unwrap().get_function_params().unwrap()[0].is_ref());
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
            MemberKind::Method { ty } => ty.get_function_ret().unwrap().clone(),
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
                vec![generic_nominal(
                    "Box",
                    vec![Type::Compound {
                        kind: CompoundKind::Slice,
                        args: vec![param("V")],
                    }],
                )],
                vec![false],
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
            vec![generic_nominal("Box", vec![Type::int()])],
            vec![false],
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
