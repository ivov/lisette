use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::EcoString;
use syntax::ast::{Attribute, Expression, Generic, Span, VariantFields};
use syntax::program::{Definition, DefinitionBody, EqualityUnusableReason};
use syntax::types::{Symbol, Type, substitute};

use super::{TaskState, wrap_with_impl_generics};
use crate::checker::infer::expressions::comparison::{
    bound_implied, check_not_equatable, param_is_comparable,
};
use crate::store::Store;

fn equals_visibility(store: &Store, id: &str) -> Option<String> {
    let method_key = format!("{id}.equals");
    match store.get_definition(&method_key) {
        Some(method) if method.visibility.is_public() => None,
        _ => store.module_for_qualified_name(id).map(str::to_string),
    }
}

/// How a hand-written `equals` on a `#[equality]` type bears on synthesis.
enum UserEquals {
    /// No `equals`: gate the fields and synthesize.
    None,
    /// A valid full-type override (its type variables, for the bound comparison).
    ValidReceiver(Vec<EcoString>),
    /// A partial or concrete receiver (`impl Box<int>`, `impl<T> Pair<T, T>`) or extra
    /// generics: does not cover every instantiation, so it cannot derive equality.
    Specialized,
    /// Wrong shape: cannot be the derived method and would collide with one.
    Conflict,
}

impl TaskState<'_> {
    pub fn register_equality(&mut self, store: &mut Store, items: &[Expression]) {
        let module_id = self.cursor.module_id.clone();
        let is_d_lis = self.is_d_lis(store);
        let mut candidates = Vec::new();
        for item in items {
            collect_equality_candidates(item, is_d_lis, &mut candidates);
        }
        for candidate in candidates {
            self.process_equality_candidate(store, &module_id, candidate);
        }
    }

    pub(super) fn register_module_equality(&mut self, store: &mut Store, module_id: &str) {
        let candidates = {
            let module = store.get_module(module_id).expect("module must exist");
            let mut candidates = Vec::new();
            for file in module.files.values().chain(module.typedefs.values()) {
                let is_d_lis = file.is_d_lis();
                for item in &file.items {
                    collect_equality_candidates(item, is_d_lis, &mut candidates);
                }
            }
            candidates
        };

        for candidate in candidates {
            self.process_equality_candidate(store, module_id, candidate);
        }
    }

    fn collect_bound_mismatched_equals(&mut self, store: &Store, module_id: &str) -> Vec<String> {
        let candidates: Vec<(Symbol, Vec<EcoString>)> = {
            let module = store.get_module(module_id).expect("module must exist");
            module
                .definitions
                .iter()
                .filter_map(|(qualified, definition)| {
                    let (methods, arity) = match &definition.body {
                        DefinitionBody::Struct {
                            methods, generics, ..
                        }
                        | DefinitionBody::Enum {
                            methods, generics, ..
                        } => (methods, generics.len()),
                        _ => return None,
                    };
                    let method_ty = methods.get("equals")?;
                    let vars = method_ty.equals_receiver_vars(qualified.as_str(), arity)?;
                    Some((qualified.clone(), vars))
                })
                .collect()
        };
        let mut mismatched = Vec::new();
        for (qualified, vars) in &candidates {
            if self.equals_bounds_mismatch(store, qualified, vars) {
                mismatched.push(qualified.to_string());
            }
        }
        mismatched
    }

    fn process_equality_candidate(
        &mut self,
        store: &mut Store,
        module_id: &str,
        candidate: EqualityCandidate,
    ) {
        let EqualityCandidate {
            attribute_span,
            kind,
        } = candidate;
        let TypeCandidate {
            name,
            is_d_lis,
            has_args,
        } = match kind {
            CandidateKind::Misplaced => {
                self.sink
                    .push(diagnostics::attribute::equality_not_a_struct_or_enum(
                        &attribute_span,
                    ));
                return;
            }
            CandidateKind::Type(type_candidate) => type_candidate,
        };

        if has_args {
            self.sink
                .push(diagnostics::attribute::equality_with_arguments(
                    &attribute_span,
                ));
            return;
        }
        if is_d_lis {
            self.sink
                .push(diagnostics::attribute::equality_in_typedef(&attribute_span));
            return;
        }

        let qualified = Symbol::from_parts(module_id, &name);
        if is_tuple_struct(store, &qualified) {
            self.sink
                .push(diagnostics::attribute::equality_on_tuple_struct(
                    &attribute_span,
                ));
            return;
        }

        match user_equals(store, &qualified) {
            UserEquals::ValidReceiver(vars) => {
                if self.is_ufcs_method(qualified.as_str(), "equals") {
                    self.sink
                        .push(diagnostics::attribute::equality_specialized_equals(
                            &attribute_span,
                        ));
                    return;
                }
                if self.equals_bounds_mismatch(store, &qualified, &vars)
                    && !store.bound_conflict_types.contains(qualified.as_str())
                {
                    self.sink
                        .push(diagnostics::attribute::equality_bounded_equals(
                            &attribute_span,
                        ));
                }
                return;
            }
            UserEquals::Conflict => {
                self.sink
                    .push(diagnostics::attribute::equality_conflicting_equals(
                        &attribute_span,
                    ));
                return;
            }
            UserEquals::Specialized => {
                self.sink
                    .push(diagnostics::attribute::equality_specialized_equals(
                        &attribute_span,
                    ));
                return;
            }
            UserEquals::None => {}
        }

        if has_hidden_user_equals(store, &qualified) {
            self.sink
                .push(diagnostics::attribute::equality_conflicting_equals(
                    &attribute_span,
                ));
            return;
        }

        self.synthesize_equals(store, module_id, &qualified);
        self.facts.equality_derivations.push(qualified.to_string());
    }

    /// Build the equality verdict and gate derivations. Run once after registration + UFCS.
    pub fn finalize_equality(&mut self, store: &mut Store) {
        self.record_equality_index(store);
        self.validate_equality_derivations(store);
    }

    fn validate_equality_derivations(&mut self, store: &Store) {
        let derivations = std::mem::take(&mut self.facts.equality_derivations);
        for id in &derivations {
            let qualified = Symbol::from_raw(id.as_str());
            let module_id = store
                .module_for_qualified_name(id)
                .map(str::to_string)
                .unwrap_or_default();
            let name = syntax::types::unqualified_name(id).to_string();
            self.gate_equality_derivation(store, &name, &qualified, &module_id);
        }
    }

    fn gate_equality_derivation(
        &mut self,
        store: &Store,
        type_name: &str,
        qualified: &Symbol,
        module_id: &str,
    ) {
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return;
        };
        let (generics, fields): (Vec<Generic>, Vec<(EcoString, Span, Type)>) = match &definition
            .body
        {
            DefinitionBody::Struct {
                generics, fields, ..
            } => (
                generics.clone(),
                fields
                    .iter()
                    .map(|f| (f.name.clone(), f.name_span, f.ty.clone()))
                    .collect(),
            ),
            DefinitionBody::Enum {
                generics, variants, ..
            } => {
                let mut specs: Vec<(EcoString, Span, Type)> = Vec::new();
                for variant in variants {
                    match &variant.fields {
                        VariantFields::Tuple(fields) => {
                            for field in fields {
                                specs.push((
                                    variant.name.clone(),
                                    variant.name_span,
                                    field.ty.clone(),
                                ));
                            }
                        }
                        VariantFields::Struct(fields) => {
                            for field in fields {
                                specs.push((field.name.clone(), field.name_span, field.ty.clone()));
                            }
                        }
                        VariantFields::Unit => {}
                    }
                }
                (generics.clone(), specs)
            }
            _ => return,
        };

        self.scopes.push();
        self.put_in_scope(&generics);
        let before = self.sink.len();
        for g in &generics {
            let qualified_g = self.qualify_name(&g.name);
            for bound in &g.bounds {
                let bound_ty = self.register_bound_annotation(store, bound, &g.span);
                self.scopes
                    .current_mut()
                    .trait_bounds
                    .get_or_insert_with(HashMap::default)
                    .entry(qualified_g.clone())
                    .or_default()
                    .push(bound_ty);
            }
        }
        self.sink.truncate(before);

        for (field_name, field_span, field_ty) in &fields {
            let reason = check_not_equatable(&self.env, store, field_ty, module_id, &|name| {
                param_is_comparable(&self.scopes, &self.env, name)
            });
            if let Some(reason) = reason {
                self.sink
                    .push(diagnostics::attribute::cannot_derive_equality(
                        type_name, field_name, field_span, reason,
                    ));
            }
        }

        self.scopes.pop();
    }

    fn record_equality_index(&mut self, store: &mut Store) {
        let module_ids: Vec<String> = store.modules.keys().cloned().collect();
        let bound_mismatch: HashSet<String> = module_ids
            .iter()
            .flat_map(|module_id| self.collect_bound_mismatched_equals(store, module_id))
            .collect();
        let synthesized: HashSet<String> =
            self.facts.equality_derivations.iter().cloned().collect();

        let ids: Vec<Symbol> = store
            .modules
            .values()
            .flat_map(|module| module.definitions.iter())
            .filter_map(|(qualified, definition)| match &definition.body {
                DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. }
                    if methods.contains_key("equals") =>
                {
                    Some(qualified.clone())
                }
                _ => None,
            })
            .collect();

        for id in ids {
            let id_str = id.as_str();
            let visibility = equals_visibility(store, id_str);
            let classification = user_equals(store, &id);
            if bound_mismatch.contains(id_str) {
                store.equality_index.insert_unusable(
                    id.to_string(),
                    EqualityUnusableReason::BoundMismatch,
                    visibility,
                );
            } else if self.is_ufcs_method(id_str, "equals")
                && matches!(
                    classification,
                    UserEquals::ValidReceiver(_) | UserEquals::Specialized
                )
            {
                store.equality_index.insert_unusable(
                    id.to_string(),
                    EqualityUnusableReason::UfcsLowered,
                    visibility,
                );
            } else if matches!(classification, UserEquals::ValidReceiver(_)) {
                store.equality_index.insert_method(
                    id.to_string(),
                    visibility,
                    synthesized.contains(id_str),
                );
            }
        }
    }

    fn equals_bounds_mismatch(
        &mut self,
        store: &Store,
        qualified: &Symbol,
        vars: &[EcoString],
    ) -> bool {
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return false;
        };
        let (generics, method_ty) = match &definition.body {
            DefinitionBody::Struct {
                generics, methods, ..
            }
            | DefinitionBody::Enum {
                generics, methods, ..
            } => {
                let Some(method) = methods.get("equals") else {
                    return false;
                };
                (generics.clone(), method.clone())
            }
            _ => return false,
        };
        if generics.is_empty() {
            return false;
        }
        let method_bounds = method_bounds_by_var(store, &method_ty);
        let empty: Vec<Type> = Vec::new();
        let alpha: HashMap<EcoString, Type> = vars
            .iter()
            .zip(&generics)
            .map(|(var, generic)| (var.clone(), Type::Parameter(generic.name.clone())))
            .collect();

        self.scopes.push();
        self.put_in_scope(&generics);
        let before = self.sink.len();
        let mut mismatch = false;
        for (position, generic) in generics.iter().enumerate() {
            let mut type_bounds: Vec<Type> = Vec::new();
            for bound in &generic.bounds {
                if let Some(ty) = self.resolve_type_bound(store, bound, &generic.span, qualified) {
                    type_bounds.push(ty);
                }
            }
            let method_set = method_bounds.get(&vars[position]).unwrap_or(&empty);
            if !method_set
                .iter()
                .all(|mb| bound_implied(store, &type_bounds, &substitute(mb, &alpha)))
            {
                mismatch = true;
                break;
            }
        }
        self.sink.truncate(before);
        self.scopes.pop();
        mismatch
    }

    fn type_generic_bounds(
        &mut self,
        store: &Store,
        qualified: &Symbol,
        generics: &[Generic],
    ) -> Vec<syntax::types::Bound> {
        self.scopes.push();
        self.put_in_scope(generics);
        let before = self.sink.len();
        let mut bounds = Vec::new();
        for generic in generics {
            for bound in &generic.bounds {
                if let Some(bound_ty) =
                    self.resolve_type_bound(store, bound, &generic.span, qualified)
                {
                    bounds.push(syntax::types::Bound {
                        param_name: generic.name.clone(),
                        generic: Type::Parameter(generic.name.clone()),
                        ty: bound_ty,
                    });
                }
            }
        }
        self.sink.truncate(before);
        self.scopes.pop();
        bounds
    }

    fn synthesize_equals(&mut self, store: &mut Store, module_id: &str, qualified: &Symbol) {
        let Some(scheme) = store.get_type(qualified.as_str()).cloned() else {
            return;
        };
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return;
        };
        let Some(generics) = type_generics(definition) else {
            return;
        };
        let visibility = definition.visibility().clone();
        let name_span = definition.name_span();

        let receiver_ty = match scheme {
            Type::Forall { body, .. } => *body,
            other => other,
        };
        let fn_ty = Type::function(
            vec![receiver_ty.clone(), receiver_ty],
            vec![false, false],
            Default::default(),
            Box::new(Type::bool()),
        );
        let impl_bounds = self.type_generic_bounds(store, qualified, &generics);
        let method_ty = wrap_with_impl_generics(&fn_ty, &generics, &impl_bounds);

        let equals_key = qualified.with_segment("equals");
        let module = store.get_module_mut(module_id).expect("module must exist");
        if let Some(methods) = module
            .definitions
            .get_mut(qualified.as_str())
            .and_then(Definition::methods_mut)
        {
            methods.insert("equals".into(), method_ty.clone());
        }
        module
            .definitions
            .entry(equals_key)
            .or_insert_with(|| Definition {
                visibility,
                ty: method_ty,
                name: None,
                name_span,
                doc: None,
                body: DefinitionBody::Value {
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                    const_value: None,
                },
            });
    }
}

/// The bounds a method type carries, keyed by its type-variable name.
fn method_bounds_by_var(store: &Store, method_ty: &Type) -> HashMap<EcoString, Vec<Type>> {
    let func = match method_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let mut map: HashMap<EcoString, Vec<Type>> = HashMap::default();
    if let Type::Function(f) = func {
        for bound in &f.bounds {
            let resolved = store.deep_resolve_alias(&bound.ty);
            if resolved.get_qualified_id().is_some() {
                map.entry(bound.param_name.clone())
                    .or_default()
                    .push(resolved);
            }
        }
    }
    map
}

fn is_tuple_struct(store: &Store, qualified: &Symbol) -> bool {
    matches!(
        store.get_definition(qualified.as_str()).map(|d| &d.body),
        Some(DefinitionBody::Struct {
            kind: syntax::ast::StructKind::Tuple,
            ..
        })
    )
}

fn has_hidden_user_equals(store: &Store, qualified: &Symbol) -> bool {
    let in_method_set = matches!(
        store.get_definition(qualified.as_str()).map(|d| &d.body),
        Some(DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. })
            if methods.contains_key("equals")
    );
    if in_method_set {
        return false;
    }
    let equals_key = qualified.with_segment("equals");
    store
        .get_definition(equals_key.as_str())
        .is_some_and(|d| d.name_span().is_some())
}

fn user_equals(store: &Store, qualified: &Symbol) -> UserEquals {
    let Some(definition) = store.get_definition(qualified.as_str()) else {
        return UserEquals::None;
    };
    let (methods, generics_len) = match &definition.body {
        DefinitionBody::Struct {
            methods, generics, ..
        }
        | DefinitionBody::Enum {
            methods, generics, ..
        } => (methods, generics.len()),
        _ => return UserEquals::None,
    };
    let Some(method_ty) = methods.get("equals") else {
        return UserEquals::None;
    };
    if let Some(vars) = method_ty.equals_receiver_vars(qualified.as_str(), generics_len) {
        UserEquals::ValidReceiver(vars)
    } else if method_ty.is_equals_signature() {
        UserEquals::Specialized
    } else {
        UserEquals::Conflict
    }
}

fn type_generics(definition: &Definition) -> Option<Vec<Generic>> {
    match &definition.body {
        DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
            Some(generics.clone())
        }
        _ => None,
    }
}

struct EqualityCandidate {
    attribute_span: Span,
    kind: CandidateKind,
}

enum CandidateKind {
    Type(TypeCandidate),
    Misplaced,
}

struct TypeCandidate {
    name: String,
    is_d_lis: bool,
    has_args: bool,
}

fn equality_attribute(attributes: &[Attribute]) -> Option<&Attribute> {
    attributes.iter().find(|a| a.name == "equality")
}

fn misplaced_candidate(attribute: &Attribute) -> EqualityCandidate {
    EqualityCandidate {
        attribute_span: attribute.span,
        kind: CandidateKind::Misplaced,
    }
}

fn collect_method_attributes(methods: &[Expression], out: &mut Vec<EqualityCandidate>) {
    for method in methods {
        if let Expression::Function { attributes, .. } = method {
            out.extend(equality_attribute(attributes).map(misplaced_candidate));
        }
    }
}

fn collect_equality_candidates(
    item: &Expression,
    is_d_lis: bool,
    out: &mut Vec<EqualityCandidate>,
) {
    match item {
        Expression::Struct {
            attributes,
            name,
            fields,
            ..
        } => {
            if let Some(attribute) = equality_attribute(attributes) {
                out.push(EqualityCandidate {
                    attribute_span: attribute.span,
                    kind: CandidateKind::Type(TypeCandidate {
                        name: name.to_string(),
                        is_d_lis,
                        has_args: !attribute.args.is_empty(),
                    }),
                });
            }
            for field in fields {
                out.extend(equality_attribute(&field.attributes).map(misplaced_candidate));
            }
        }
        Expression::Enum {
            attributes, name, ..
        } => {
            if let Some(attribute) = equality_attribute(attributes) {
                out.push(EqualityCandidate {
                    attribute_span: attribute.span,
                    kind: CandidateKind::Type(TypeCandidate {
                        name: name.to_string(),
                        is_d_lis,
                        has_args: !attribute.args.is_empty(),
                    }),
                });
            }
        }
        Expression::Function { attributes, .. } => {
            out.extend(equality_attribute(attributes).map(misplaced_candidate));
        }
        Expression::TypeAlias { attributes, .. } => {
            out.extend(equality_attribute(attributes).map(misplaced_candidate));
        }
        Expression::ImplBlock { methods, .. } => collect_method_attributes(methods, out),
        Expression::Interface {
            method_signatures, ..
        } => collect_method_attributes(method_signatures, out),
        _ => {}
    }
}
