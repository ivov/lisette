use syntax::ast::{BindingId, Expression, Literal, Span, StructFieldDefinition, UnaryOperator};
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::checker::infer::context::ValueSource;

/// What governs a write to a place.
pub(super) enum WriteTarget {
    /// The write goes through a `Slice`, `Map`, or `Ref`, whose own type must be writable.
    Through { governing: Type },
    /// The write goes directly into a binding, which must be mutable.
    Binding { name: String },
    /// Neither a reference nor a binding root.
    Other,
}

impl InferCtx<'_> {
    /// Classifies the place a write targets. Slice and map indexing and
    /// dereferencing go through a reference, field and array access stay in the value.
    pub(super) fn classify_write_target(&self, target: &Expression) -> WriteTarget {
        match target.unwrap_parens() {
            Expression::Identifier { value, .. } => WriteTarget::Binding {
                name: value.to_string(),
            },
            Expression::IndexedAccess {
                expression: base, ..
            } => {
                let governing = self.governing_index_type(base);
                match governing {
                    Some(governing) => WriteTarget::Through { governing },
                    // An `Array` index writes the array's own memory.
                    None => self.classify_write_target(base),
                }
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: base,
                ..
            } => {
                let base_ty = self
                    .store
                    .peel_alias(&base.get_type().resolve_in(&self.env));
                WriteTarget::Through { governing: base_ty }
            }
            Expression::DotAccess {
                expression: base, ..
            } => {
                // Field access auto-derefs, so the innermost `Ref` governs.
                let base_ty = self
                    .store
                    .peel_alias(&base.get_type().resolve_in(&self.env));
                if base_ty.is_ref() {
                    WriteTarget::Through {
                        governing: self.innermost_ref(base_ty),
                    }
                } else {
                    self.classify_write_target(base)
                }
            }
            _ => WriteTarget::Other,
        }
    }

    /// The base's type when indexing it writes into shared storage.
    fn governing_index_type(&self, base: &Expression) -> Option<Type> {
        let base_ty = self
            .store
            .peel_alias(&base.get_type().resolve_in(&self.env));
        match &base_ty {
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                ..
            } => Some(base_ty),
            _ => None,
        }
    }

    /// The `Ref` layer closest to the pointee, which governs writes.
    fn innermost_ref(&self, ref_ty: Type) -> Type {
        let mut current = ref_ty;
        loop {
            let inner = match current.inner() {
                Some(inner) => self.store.peel_alias(&inner.resolve_in(&self.env)),
                None => return current,
            };
            if inner.is_ref() {
                current = inner;
            } else {
                return current;
            }
        }
    }

    /// Whether fields read from a value of this type keep their `mut`.
    /// A read-only value hands out everything read-only.
    pub(super) fn owner_grants_write(&self, receiver_ty: &Type) -> bool {
        let resolved = self.store.peel_alias(&receiver_ty.resolve_in(&self.env));
        match &resolved {
            Type::Compound {
                kind: CompoundKind::Ref,
                ..
            } => {
                let innermost = self.innermost_ref(resolved);
                if !innermost.is_writable() {
                    return false;
                }
                match innermost.inner() {
                    Some(inner) => match self.store.peel_alias(&inner.resolve_in(&self.env)) {
                        Type::Nominal { writable, .. } => writable,
                        _ => true,
                    },
                    None => true,
                }
            }
            other => other.is_writable(),
        }
    }

    /// The field's declared type, demoted when the owner is read-only.
    /// Demoting before substitution spares instantiated type parameters.
    pub(super) fn granted_component_type(&self, declared: &Type, owner_ty: &Type) -> Type {
        if self.owner_grants_write(owner_ty) {
            declared.clone()
        } else {
            self.store.demoted(declared)
        }
    }

    /// Taking a permission-carrying component out of a writable owner puts the
    /// owner binding's `mut` to use.
    pub(super) fn mark_component_grant(
        &mut self,
        owner: &Expression,
        owner_ty: &Type,
        declared: &Type,
    ) {
        if self.owner_grants_write(owner_ty) && self.store.demotion_changes(declared) {
            self.mark_place_root_mutated(owner);
        }
    }

    pub(super) fn mark_scrutinee_grant(&mut self, subject: &Expression, subject_ty: &Type) {
        let peeled = self.store.peel_alias(&subject_ty.strip_refs());
        if let Type::Nominal { id, .. } = &peeled
            && self.store.nominal_declares_writable_components(id)
            && self.owner_grants_write(subject_ty)
        {
            self.mark_place_root_mutated(subject);
        }
    }

    pub(super) fn mark_place_root_mutated(&mut self, place: &Expression) {
        if let Some(root) = super::aliasing::place_root_name(place.unwrap_parens())
            && let Some(binding_id) = self.scopes.lookup_binding_id(&root)
        {
            self.facts.mark_alias_mutated(binding_id);
        }
    }

    /// Whether a write to this place is permitted. `&` yields a writable
    /// pointer exactly when it is.
    pub(super) fn place_permits_write(&self, place: &Expression) -> bool {
        match self.classify_write_target(place) {
            WriteTarget::Through { governing } => governing.is_writable(),
            WriteTarget::Binding { name } => self.scopes.lookup_mutable(&name),
            WriteTarget::Other => false,
        }
    }

    /// The expected type for a construction component's initializer.
    pub(super) fn component_expected_type(
        &mut self,
        declared: &Type,
        initializer: &Expression,
    ) -> Type {
        if !self.store.demotion_changes(declared) {
            return declared.clone();
        }
        if self.demoting_would_pin_qualifier(initializer)
            || self.initializer_fits_declared(declared, initializer)
        {
            declared.clone()
        } else {
            self.store.demoted(declared)
        }
    }

    /// Whether every gated component received a value carrying its declared permissions.
    pub(super) fn components_grant_write<'c>(
        &self,
        components: impl Iterator<Item = (Type, Option<&'c Expression>)>,
        missing_grants: bool,
    ) -> bool {
        let mut gated = false;
        for (declared, value) in components {
            if !self.store.demotion_changes(&declared) {
                continue;
            }
            gated = true;
            let provides = match value {
                Some(value) => {
                    self.value_provides_declared_permissions(&declared, &value.get_type())
                }
                None => missing_grants,
            };
            if !provides {
                return false;
            }
        }
        gated
    }

    fn initializer_fits_declared(&mut self, declared: &Type, initializer: &Expression) -> bool {
        if !matches!(
            initializer.unwrap_parens(),
            Expression::Literal {
                literal: Literal::Slice(_),
                ..
            }
        ) {
            return false;
        }
        let declared = declared.clone();
        let initializer = initializer.clone();
        self.probe(|this| {
            let before = this.sink.checkpoint();
            let _ = this.with_value_context(|state| state.infer_expression(initializer, &declared));
            !this.sink.has_changed_since(before)
        })
    }

    fn demoting_would_pin_qualifier(&mut self, expression: &Expression) -> bool {
        let callee = match expression.unwrap_parens() {
            Expression::Lambda { .. } => return true,
            Expression::Call {
                expression: callee, ..
            } => callee.as_ref(),
            identifier @ Expression::Identifier { .. } => identifier,
            _ => return false,
        };
        let callee = callee.clone();
        self.probe(|this| {
            let callee_ty = this.new_type_var();
            let _ = this.with_value_context(|s| s.infer_expression(callee, &callee_ty));
            let resolved = callee_ty.resolve_in(&this.env);
            let result_ty = match resolved.unwrap_forall() {
                Type::Function(function) => function.return_type.as_ref().clone(),
                value => value.clone(),
            };
            this.qualifier_undetermined(&result_ty)
        })
    }

    fn qualifier_undetermined(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env);
        match &resolved {
            Type::Var { .. } | Type::Parameter(_) => true,
            _ => resolved
                .children()
                .into_iter()
                .any(|child| self.qualifier_undetermined(child)),
        }
    }

    /// Whether the value carries every permission the declared type spells.
    pub(super) fn value_provides_declared_permissions(
        &self,
        declared: &Type,
        value: &Type,
    ) -> bool {
        let declared = self.store.peel_alias(&declared.resolve_in(&self.env));
        if !self.store.demotion_changes(&declared) {
            return true;
        }
        let value = self.store.peel_alias(&value.resolve_in(&self.env));
        if value.is_error() || matches!(value, Type::Never) {
            return true;
        }
        let pairwise = |declared: &[Type], value: &[Type]| {
            declared.len() == value.len()
                && declared
                    .iter()
                    .zip(value)
                    .all(|(d, v)| self.value_provides_declared_permissions(d, v))
        };
        match (&declared, &value) {
            (
                Type::Compound {
                    writable,
                    args: declared_args,
                    ..
                },
                Type::Compound {
                    args: value_args, ..
                },
            ) => (!writable || value.is_writable()) && pairwise(declared_args, value_args),
            (
                Type::Nominal {
                    writable,
                    params: declared_params,
                    ..
                },
                Type::Nominal {
                    params: value_params,
                    ..
                },
            ) => (!writable || value.is_writable()) && pairwise(declared_params, value_params),
            (Type::Tuple(declared_elements), Type::Tuple(value_elements)) => {
                pairwise(declared_elements, value_elements)
            }
            (
                Type::Array {
                    element: declared_element,
                    ..
                },
                Type::Array {
                    element: value_element,
                    ..
                },
            ) => self.value_provides_declared_permissions(declared_element, value_element),
            (Type::Function(declared_fn), Type::Function(value_fn)) => self
                .value_provides_declared_permissions(
                    &declared_fn.return_type,
                    &value_fn.return_type,
                ),
            (
                Type::Forall {
                    body: declared_body,
                    ..
                },
                Type::Forall {
                    body: value_body, ..
                },
            ) => self.value_provides_declared_permissions(declared_body, value_body),
            _ => false,
        }
    }
}

impl InferCtx<'_> {
    pub(super) fn write_context(
        &self,
        target: &Expression,
    ) -> Option<diagnostics::infer::WriteContext> {
        if let Some(element) = self.element_write_declaration(target) {
            return Some(diagnostics::infer::WriteContext::Element(element));
        }
        let hop = self.write_hop(target)?;
        self.value_context(hop)
            .or_else(|| self.binding_context(&super::aliasing::place_root_name(target)?))
    }

    pub(super) fn value_context(
        &self,
        value: &Expression,
    ) -> Option<diagnostics::infer::WriteContext> {
        match value.unwrap_parens() {
            Expression::DotAccess {
                expression: owner,
                member,
                ..
            } => match self.struct_field(owner, member) {
                Some(field) if !self.store.peel_alias(&field.ty).is_writable() => {
                    Some(diagnostics::infer::WriteContext::Field(member.to_string()))
                }
                Some(_) => Some(diagnostics::infer::WriteContext::ReadOnlyOwner {
                    owner: super::aliasing::render_place(owner.unwrap_parens()),
                    field: member.to_string(),
                    origin: self.owner_origin(owner),
                }),
                None => None,
            },
            identifier @ Expression::Identifier { .. } => {
                self.binding_context(&identifier.get_var_name()?)
            }
            call @ Expression::Call { .. } => {
                let callee = super::aliasing::render_place(call);
                let callee = callee.strip_suffix("()")?;
                Some(diagnostics::infer::WriteContext::CallResult(
                    callee.to_string(),
                ))
            }
            _ => None,
        }
    }

    fn binding_context(&self, name: &str) -> Option<diagnostics::infer::WriteContext> {
        let binding_id = self.scopes.lookup_binding_id(name)?;
        if let Some(collection) = self.loop_element_bindings.get(&binding_id) {
            return Some(diagnostics::infer::WriteContext::LoopElement {
                binding: name.to_string(),
                collection: collection.clone(),
            });
        }
        let kind = self
            .facts
            .bindings
            .get(&binding_id)
            .map(|binding| binding.kind)?;
        let declared = self.scopes.lookup_value(name)?;
        if kind.is_param() && !self.store.peel_alias(declared).is_writable() {
            return Some(diagnostics::infer::WriteContext::Parameter(
                name.to_string(),
            ));
        }
        match self.value_sources.get(&binding_id)? {
            ValueSource::Place(source) => Some(diagnostics::infer::WriteContext::AliasOf {
                binding: name.to_string(),
                source: source.clone(),
            }),
            ValueSource::Call(callee) => {
                Some(diagnostics::infer::WriteContext::CallResult(callee.clone()))
            }
        }
    }

    fn struct_field(&self, owner: &Expression, member: &str) -> Option<&StructFieldDefinition> {
        let owner_ty = self.owner_type(owner)?;
        self.store
            .fields_of(owner_ty.get_qualified_id()?)?
            .iter()
            .find(|field| field.name == member)
    }

    /// An argument is classified before it is inferred.
    fn owner_type(&self, owner: &Expression) -> Option<Type> {
        let from_tree = owner.get_type().resolve_in(&self.env);
        if !from_tree.is_uninferred() && !from_tree.is_error() {
            return Some(self.store.peel_alias(&from_tree.strip_refs()));
        }
        let name = owner.unwrap_parens().get_var_name()?;
        let declared = self.scopes.lookup_value(&name)?.resolve_in(&self.env);
        Some(self.store.peel_alias(&declared.strip_refs()))
    }

    fn write_hop<'e>(&self, target: &'e Expression) -> Option<&'e Expression> {
        match target.unwrap_parens() {
            Expression::IndexedAccess { expression, .. } => Some(expression.unwrap_parens()),
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => Some(expression.unwrap_parens()),
            Expression::DotAccess { expression, .. } => Some(expression.unwrap_parens()),
            _ => None,
        }
    }

    pub(super) fn read_only_write_key(&self, target: &Expression) -> Option<(BindingId, String)> {
        let hop = self.write_hop(target)?;
        let root = super::aliasing::place_root_name(hop)?;
        let binding_id = self.scopes.lookup_binding_id(&root)?;
        Some((binding_id, super::aliasing::place_key(hop)?))
    }

    fn element_write_declaration(
        &self,
        target: &Expression,
    ) -> Option<diagnostics::infer::ElementDeclaration> {
        let Expression::IndexedAccess {
            expression: element_base,
            ..
        } = target.unwrap_parens()
        else {
            return None;
        };
        let Expression::IndexedAccess {
            expression: container,
            ..
        } = element_base.unwrap_parens()
        else {
            return None;
        };
        let container_ty = self
            .store
            .peel_alias(&container.get_type().resolve_in(&self.env));
        if !container_ty.is_writable() {
            return None;
        }
        let Expression::DotAccess {
            expression: owner,
            member,
            ..
        } = container.unwrap_parens()
        else {
            return None;
        };
        let owner_ty = self
            .store
            .peel_alias(&owner.get_type().resolve_in(&self.env).strip_refs());
        let field = self
            .store
            .fields_of(owner_ty.get_qualified_id()?)?
            .iter()
            .find(|field| &field.name == member)?;
        let (element_index, index) = match container_ty.as_compound()? {
            (CompoundKind::Map, _) => (1, "k"),
            _ => (0, "i"),
        };
        let Type::Compound { kind, args, .. } = &field.ty else {
            return None;
        };
        let mut deepened = args.clone();
        *deepened.get_mut(element_index)? = deepened.get(element_index)?.clone().make_writable();
        let replacement = Type::qualified_compound(*kind, deepened, true);
        let annotation_span = field.annotation.get_span();
        let declared_end = annotation_span.byte_offset + annotation_span.byte_length;
        let declaration_span = (annotation_span.file_id == target.get_span().file_id).then(|| {
            Span::new(
                field.name_span.file_id,
                field.name_span.byte_offset,
                declared_end.saturating_sub(field.name_span.byte_offset),
            )
        });
        Some(diagnostics::infer::ElementDeclaration {
            name: field.name.to_string(),
            replacement_type: replacement.to_string(),
            place: element_place(&field.name, container, element_base.unwrap_parens()),
            index,
            declaration_span,
        })
    }
}

fn element_place(name: &str, container: &Expression, element_base: &Expression) -> String {
    let container_place = super::aliasing::render_place(container.unwrap_parens());
    let element = super::aliasing::render_place(element_base);
    match element.strip_prefix(&container_place) {
        Some(index) => format!("{name}{index}"),
        None => element,
    }
}

impl InferCtx<'_> {
    pub(super) fn write_hop_place(&self, target: &Expression) -> String {
        self.write_hop(target)
            .map(super::aliasing::render_place)
            .unwrap_or_default()
    }

    pub(super) fn report_read_only_write(&mut self, target: &Expression) -> bool {
        match self.read_only_write_key(target) {
            Some(key) => self.reported_read_only_writes.insert(key),
            None => true,
        }
    }
}

impl InferCtx<'_> {
    fn owner_origin(&self, owner: &Expression) -> Option<String> {
        let name = owner.unwrap_parens().get_var_name()?;
        let binding_id = self.scopes.lookup_binding_id(&name)?;
        match self.value_sources.get(&binding_id)? {
            ValueSource::Call(callee) => Some(callee.clone()),
            ValueSource::Place(_) => None,
        }
    }

    pub(super) fn value_source(&self, value: &Expression, name: &str) -> Option<ValueSource> {
        match value.unwrap_parens() {
            call @ Expression::Call { .. } => {
                let rendered = super::aliasing::render_place(call);
                (!rendered.is_empty()).then_some(ValueSource::Call(rendered))
            }
            place => super::aliasing::place_root_name(place)
                .filter(|root| root != name)
                .map(ValueSource::Place),
        }
    }
}
