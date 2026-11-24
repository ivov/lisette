use rustc_hash::{FxHashMap, FxHashSet};
use std::mem;

use syntax::ast::{Expression, ExpressionId, FormatStringPart, Literal, MatchArm, SelectArm, Span};
use syntax::program::{
    Adaptation, CallOperation, CheckedBodiesBuilder, CheckedCallee, CheckedValueUse,
    ConformanceWitness, Definition, DefinitionBody, DotAccessResolution, FieldKey, FieldSelection,
    InterfaceConversion, InterfaceRequirement, NativeOperation, ReceiverCoercion, ValueUseKey,
    constructor_fields, interface_requirements, resolved_definition, resolved_instantiation,
};
use syntax::types::{
    CompoundKind, FunctionType, SubstitutionMap, Symbol, Type, TypeVarId, build_substitution_map,
};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::checker::infer::equality;
use crate::checker::infer::unify::{are_go_type_aliases, simple_kinds_are_go_aliases};
use crate::facts::{GenericBoundOrigin, ValueUseObligation};
use crate::store::Store;

impl InferCtx<'_> {
    pub(crate) fn record_field_selection(
        &mut self,
        key: FieldKey,
        declared_ty: &Type,
        instance: &Type,
    ) {
        let Type::Nominal { id, params, .. } = self.store.peel_alias(&instance.strip_refs()) else {
            return;
        };
        let generics = self
            .store
            .get_definition(&id)
            .and_then(|definition| definition.body.generics())
            .unwrap_or_default();
        let substitutions = build_substitution_map(generics, &params);
        self.facts.value_uses.field_selections.push((
            key,
            FieldSelection {
                owner: id,
                declared_ty: declared_ty.clone(),
                substitutions,
            },
        ));
    }

    pub(crate) fn record_positional_fields(&mut self, call: ExpressionId, instance: &Type) {
        let instance = instance.resolve_in(&self.env);
        let store = self.store;
        let Type::Nominal { id, .. } = store.peel_alias(&instance.strip_refs()) else {
            return;
        };
        let Some(constructor) =
            constructor_fields(&id, None, &instance, |id| store.get_definition(id))
        else {
            return;
        };
        for (index, field) in constructor.fields.iter().enumerate() {
            self.record_field_selection(
                FieldKey::Positional(call, index),
                field.declared_ty,
                &instance,
            );
        }
    }

    pub(super) fn record_value_use(
        &mut self,
        expression: &Expression,
        slot: &Type,
        witnesses: Vec<ConformanceWitness>,
    ) {
        self.record_obligation(
            ValueUseKey::value(expression),
            expression.get_type(),
            slot.clone(),
            witnesses,
        );
    }

    pub(crate) fn record_obligation(
        &mut self,
        key: ValueUseKey,
        source: Type,
        slot: Type,
        witnesses: Vec<ConformanceWitness>,
    ) {
        self.facts.value_uses.uses.push(ValueUseObligation {
            key,
            source,
            slot,
            witnesses,
        });
    }

    /// Record what `callee` runs, with the type arguments it is instantiated with.
    pub(super) fn record_callee(
        &mut self,
        callee: &Expression,
        operation: CallOperation,
        type_arguments: SubstitutionMap,
    ) {
        self.record_method_callee(callee, operation, type_arguments, None);
    }

    pub(super) fn record_method_callee(
        &mut self,
        callee: &Expression,
        operation: CallOperation,
        type_arguments: SubstitutionMap,
        interface_method: Option<Box<InterfaceRequirement>>,
    ) {
        let callee = callee.unwrap_value_wrappers();
        let definition =
            resolved_definition(callee).map(|definition| unaliased_owner(self.store, definition));
        let copies_storage = matches!(
            &operation,
            CallOperation::Native { kind, method } if NativeOperation::new(*kind, method).grows_slice()
        );
        let binds_receiver = is_instance_method_access(callee)
            && definition
                .as_deref()
                .is_some_and(|definition| declares_receiver(self.store, definition));
        self.facts.value_uses.callees.push((
            ValueUseKey::value(callee),
            CheckedCallee {
                operation,
                definition,
                type_arguments,
                copies_storage,
                binds_receiver,
                interface_method,
                span: callee.get_span(),
            },
        ));
    }

    /// Record that the receiver of a method call changes its reference.
    pub(super) fn record_receiver_change(
        &mut self,
        receiver: &Expression,
        change: Option<ReceiverCoercion>,
    ) {
        if let Some(change) = change {
            self.facts
                .value_uses
                .receivers
                .push((ValueUseKey::value(receiver), change));
        }
    }

    /// Run `f`, and take the conformances it selected that no nested value use took.
    pub(crate) fn selecting_witnesses<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> T,
    ) -> (T, Vec<ConformanceWitness>) {
        let start = self.facts.value_uses.selected_witnesses.len();
        let result = f(self);
        let witnesses = self.facts.value_uses.selected_witnesses.split_off(start);
        (result, witnesses)
    }

    /// Give the conformances that a later join selected to the value uses of one arm.
    pub(super) fn attach_witnesses(
        &mut self,
        values: &[ValueUseKey],
        witnesses: &[ConformanceWitness],
    ) {
        if witnesses.is_empty() {
            return;
        }
        for obligation in &mut self.facts.value_uses.uses {
            if values.contains(&obligation.key) {
                obligation.witnesses.extend_from_slice(witnesses);
            }
        }
    }

    /// Turn the frozen obligations of one file into published adaptations, once every value use passes the check.
    pub(super) fn finalize_value_uses(&mut self, items: &[Expression], file_id: u32) {
        let obligations = mem::take(&mut self.facts.value_uses.uses);
        let callees = mem::take(&mut self.facts.value_uses.callees);
        let receivers = mem::take(&mut self.facts.value_uses.receivers);
        self.facts.value_uses.selected_witnesses.clear();
        let mut finalizer = Finalizer {
            store: self.store,
            obligations: FxHashMap::default(),
            callees: callees.into_iter().collect(),
            receivers: receivers.into_iter().collect(),
            bodies: CheckedBodiesBuilder::default(),
            failures: Vec::new(),
            mismatches: Vec::new(),
            uninferred: Vec::new(),
            loop_slots: Vec::new(),
            failure_slots: Vec::new(),
            delegated_witnesses: Vec::new(),
            delegations: Vec::new(),
            discards_next: false,
        };
        for obligation in obligations {
            let key = obligation.key;
            let previous = finalizer.obligations.insert(key, obligation);
            assert!(
                previous.is_none(),
                "internal error: two obligations share the key {key:?}"
            );
        }
        for item in items {
            finalizer.discarded_use(item);
        }
        let Finalizer {
            mut bodies,
            failures,
            mismatches,
            uninferred,
            ..
        } = finalizer;
        for (if_let, unit) in mem::take(&mut self.facts.value_uses.implicit_units) {
            bodies.insert_implicit_unit(if_let, unit);
        }
        for (binary, conversion) in mem::take(&mut self.facts.value_uses.operand_conversions) {
            bodies.insert_operand_conversion(binary, conversion);
        }
        for (literal, record) in mem::take(&mut self.facts.value_uses.struct_literals) {
            bodies.insert_struct_literal(literal, record);
        }
        for (key, selection) in mem::take(&mut self.facts.value_uses.field_selections) {
            bodies.insert_field_selection(key, selection);
        }
        let mut equalities = Vec::new();
        for item in items {
            equality::select_equalities(
                self.store,
                self.cursor.package_id(),
                bodies.records(),
                item,
                &mut equalities,
            );
        }
        for (expression, recipe) in equalities {
            self.facts
                .selections
                .equalities
                .insert(expression, recipe.clone());
            bodies.insert_equality(expression, recipe);
        }
        for (key, callee) in bodies.records().callees() {
            self.facts
                .selections
                .operations
                .insert(key.expression(), callee.operation.clone());
        }
        let has_errors = self
            .sink
            .any(|diagnostic| diagnostic.is_error() && diagnostic.file_id() == Some(file_id));
        if has_errors {
            return;
        }
        if failures.is_empty() && mismatches.is_empty() && uninferred.is_empty() {
            self.facts.checked_bodies.merge(bodies);
        }
        let mut reported_variables = if uninferred.is_empty() {
            FxHashSet::default()
        } else {
            self.deferred_variables()
        };
        for value in uninferred {
            let is_new = value.variables.iter().fold(false, |is_new, variable| {
                reported_variables.insert(*variable) | is_new
            });
            if !is_new {
                continue;
            }
            self.sink.push(match value.kind {
                UninferredKind::Closure => diagnostics::infer::closure_not_inferred(value.span),
                UninferredKind::Value => diagnostics::infer::type_must_be_known(value.span),
            });
        }
        for mismatch in mismatches {
            self.sink.push(diagnostics::infer::no_go_conversion(
                &mismatch.slot.to_string(),
                &mismatch.source.to_string(),
                mismatch.span,
            ));
        }
        for (span, reason) in failures {
            self.sink
                .push(diagnostics::infer::unchecked_value_use(reason, span));
        }
    }

    fn deferred_variables(&self) -> FxHashSet<TypeVarId> {
        let package_id = self.cursor.package_id();
        let deferred = &self.facts.deferred;
        let bounds = deferred
            .generic_bounds
            .iter()
            .filter(|obligation| {
                obligation.package_id == package_id
                    && !matches!(
                        obligation.origin,
                        GenericBoundOrigin::FunctionReference { is_call: true, .. }
                    )
            })
            .map(|obligation| &obligation.argument);
        let calls = deferred
            .generic_calls
            .iter()
            .filter(|check| check.package_id == package_id)
            .map(|check| &check.ty);
        let collections = deferred
            .empty_collections
            .iter()
            .filter(|check| check.package_id == package_id)
            .map(|check| &check.ty);
        let literals = deferred
            .empty_literals
            .iter()
            .filter(|check| check.package_id == package_id)
            .map(|check| &check.ty);
        let mut variables = Vec::new();
        for ty in bounds.chain(calls).chain(collections).chain(literals) {
            collect_variables(&ty.resolve_in(&self.env), &mut variables);
        }
        variables.into_iter().collect()
    }
}

/// `definition` with an aliased owner type replaced by the type it names.
pub(crate) fn unaliased_owner(store: &Store, definition: &str) -> Symbol {
    if let Some(Definition {
        body: DefinitionBody::TypeAlias { .. },
        ..
    }) = store.get_definition(definition)
        && let Some(id) = store.peel_alias_name(definition)
    {
        return id;
    }
    let Some((owner, member)) = definition.rsplit_once('.') else {
        return Symbol::from_raw(definition);
    };
    let owner_type = Type::Nominal {
        id: Symbol::from_raw(owner),
        params: Vec::new(),
        writable: false,
    };
    match store.peel_alias(&owner_type) {
        Type::Nominal { id, .. } => id.with_segment(member),
        _ => Symbol::from_raw(definition),
    }
}

/// Whether the method `definition` lists its receiver as its first parameter, which interface methods do not.
fn declares_receiver(store: &Store, definition: &str) -> bool {
    let Some((owner, member)) = definition.rsplit_once('.') else {
        return false;
    };
    store.get_definition(owner).is_some_and(|declaration| {
        !matches!(declaration.body, DefinitionBody::Interface { .. })
            && store.get_method(owner, member).is_some()
    })
}

/// The operation that a function value naming the declaration `definition` runs when it is called.
fn method_value_operation(store: &Store, definition: &str) -> CallOperation {
    let Some((owner, member)) = definition.rsplit_once('.') else {
        return CallOperation::Function;
    };
    let Some(method) = store.get_method(owner, member) else {
        return CallOperation::Function;
    };
    let takes_receiver = method
        .ty
        .unwrap_forall()
        .get_function_params()
        .and_then(<[_]>::first)
        .is_some_and(|receiver| {
            matches!(receiver.ty.strip_refs(), Type::Nominal { id, .. } if id.as_str() == owner)
        });
    if !takes_receiver {
        CallOperation::Function
    } else if store.is_ufcs_method(owner, member) {
        CallOperation::UfcsFunction
    } else {
        CallOperation::ReceiverMethod {
            is_public: method.visibility.is_public(),
        }
    }
}

struct Finalizer<'s> {
    store: &'s Store,
    obligations: FxHashMap<ValueUseKey, ValueUseObligation>,
    callees: FxHashMap<ValueUseKey, CheckedCallee>,
    receivers: FxHashMap<ValueUseKey, ReceiverCoercion>,
    bodies: CheckedBodiesBuilder,
    failures: Vec<(Span, &'static str)>,
    mismatches: Vec<Mismatch>,
    uninferred: Vec<Uninferred>,
    loop_slots: Vec<Option<LoopSlot>>,
    failure_slots: Vec<Type>,
    /// The conformances that enclosing delegating constructs selected for the value they produce.
    delegated_witnesses: Vec<ConformanceWitness>,
    /// For each enclosing delegating construct, whether a value it produces converts.
    delegations: Vec<bool>,
    /// Whether the position of the next value use discards its value.
    discards_next: bool,
}

impl Finalizer<'_> {
    /// Check one value use, in the slot that a delegating parent passes down if any.
    fn value_use(&mut self, expression: &Expression, inherited: Option<Type>) {
        let expression = expression.unwrap_parens();
        if matches!(expression, Expression::Lambda { .. }) && is_unresolved(&expression.get_type())
        {
            self.not_inferred(
                expression.get_span(),
                &expression.get_type(),
                UninferredKind::Closure,
                "this closure has an unresolved type",
            );
        }
        let key = ValueUseKey::value(expression);
        let is_discarded = mem::take(&mut self.discards_next);
        let (recorded, mut witnesses) = match self.obligations.get(&key) {
            Some(obligation) => (Some(obligation.slot.clone()), obligation.witnesses.clone()),
            None => {
                self.fail(expression.get_span(), "this value has no checked slot");
                (None, Vec::new())
            }
        };
        let is_inherited = inherited.is_some();
        let Some(slot) = inherited.or(recorded) else {
            self.positions(expression, &Type::ignored());
            return;
        };
        // A discarded value keeps its own type, and the values that a discarded construct produces are discarded too.
        let (use_slot, published_slot) = if is_discarded || slot.is_ignored() {
            (Type::ignored(), expression.get_type())
        } else {
            let slot = self.value_slot(slot, expression);
            (slot.clone(), slot)
        };
        if delegates(expression) {
            let mark = self.delegated_witnesses.len();
            self.delegated_witnesses.extend(witnesses);
            self.delegations.push(false);
            self.positions(expression, &use_slot);
            let converts = self.delegations.pop().unwrap_or(false);
            self.delegated_witnesses.truncate(mark);
            self.publish(
                published_slot,
                Adaptation::Delegated { converts },
                expression,
            );
            self.mark_produced(is_inherited, converts);
            return;
        }
        if is_inherited {
            witnesses.extend(self.delegated_witnesses.iter().cloned());
        }
        let adaptation = self.adaptation(
            &fallible_block_source(self.store, expression, &use_slot),
            &use_slot,
            expression.get_span(),
            &witnesses,
            false,
        );
        self.mark_produced(is_inherited, adaptation.converts());
        self.publish(published_slot, adaptation, expression);
        self.positions(expression, &use_slot);
    }

    /// The slot that `expression` goes into, where a slot that no value reaches or that already reported an error takes the type of the value.
    fn value_slot(&mut self, slot: Type, expression: &Expression) -> Type {
        let value_type = expression.get_type();
        match slot {
            Type::Never | Type::Error => value_type,
            // A value that never arrives, or that already reported an error, constrains no slot.
            Type::Var { .. } if value_type.is_never() || value_type.is_error() => value_type,
            slot if is_unresolved(&slot) => {
                self.not_inferred(
                    expression.get_span(),
                    &slot,
                    UninferredKind::Value,
                    "this value goes into a slot of unresolved type",
                );
                value_type
            }
            slot => slot,
        }
    }

    /// Check a value use whose value its position discards, such as a loop body.
    fn discarded_use(&mut self, expression: &Expression) {
        self.discards_next = true;
        self.value_use(expression, None);
    }

    /// Check the branches of an `if`, which discards the value of a branch with no alternative.
    fn branches(
        &mut self,
        consequence: &Expression,
        alternative: Option<&Expression>,
        slot: &Type,
    ) {
        match alternative {
            Some(alternative) => {
                self.value_use(consequence, Some(slot.clone()));
                self.value_use(alternative, Some(slot.clone()));
            }
            None => self.discarded_use(consequence),
        }
    }

    /// Check a value use in a position that takes the value as it is, such as a match subject.
    fn unadapted_use(&mut self, expression: &Expression) {
        self.value_use(expression, None);
        if self
            .bodies
            .records()
            .value_use(&ValueUseKey::value(expression))
            .is_some_and(|checked| checked.adaptation.converts())
        {
            self.fail(
                expression.get_span(),
                "this position takes its value as it is, but the value converts",
            );
        }
    }

    /// Tell the delegating construct that this produced value converts, when it does.
    fn mark_produced(&mut self, is_inherited: bool, converts: bool) {
        if is_inherited
            && converts
            && let Some(delegation) = self.delegations.last_mut()
        {
            *delegation = true;
        }
    }

    fn match_arms(&mut self, arms: &[MatchArm], slot: &Type) {
        for arm in arms {
            if let Some(guard) = &arm.guard {
                self.unadapted_use(guard);
            }
            self.value_use(&arm.expression, Some(slot.clone()));
        }
    }

    fn in_failure_slot(&mut self, slot: Option<Type>, walk: impl FnOnce(&mut Self)) {
        let pushed = slot.is_some();
        self.failure_slots.extend(slot);
        walk(self);
        if pushed {
            self.failure_slots.pop();
        }
    }

    fn function_body(&mut self, function: &Expression, body: &Expression) {
        let failure_slot = function
            .get_type()
            .unwrap_forall()
            .get_function_ret()
            .cloned()
            .unwrap_or(Type::Error);
        self.in_failure_slot(Some(failure_slot), |finalizer| {
            finalizer.in_loop(None, |finalizer| finalizer.value_use(body, None));
        });
    }

    /// Walk a loop body, where a `break` value goes into `slot`.
    fn in_loop(&mut self, slot: Option<LoopSlot>, walk: impl FnOnce(&mut Self)) {
        self.loop_slots.push(slot);
        walk(self);
        self.loop_slots.pop();
    }

    fn special_use(&mut self, key: ValueUseKey, span: Span, reason: &'static str, is_cast: bool) {
        let Some(obligation) = self.obligations.get(&key) else {
            self.fail(span, reason);
            return;
        };
        let (source, slot) = (obligation.source.clone(), obligation.slot.clone());
        let witnesses = obligation.witnesses.clone();
        let adaptation = self.adaptation(&source, &slot, span, &witnesses, is_cast);
        self.bodies.insert_use(
            key,
            CheckedValueUse {
                value_type: source,
                slot,
                adaptation,
                span,
            },
        );
    }

    /// Publish the selected operation of a callee.
    fn callee(&mut self, callee: &Expression) {
        let callee = callee.unwrap_value_wrappers();
        let key = ValueUseKey::value(callee);
        let Some(checked) = self.callees.get(&key).cloned() else {
            self.fail(callee.get_span(), "this call has no selected operation");
            return;
        };
        if let Some(Type::Forall { vars, .. }) = checked
            .definition
            .as_ref()
            .and_then(|definition| self.declared_type(definition))
            && vars
                .iter()
                .any(|name| checked.type_arguments.get(name).is_none())
        {
            self.fail(callee.get_span(), "this call has no checked type arguments");
        }
        self.bodies.insert_callee(key, checked);
    }

    /// Publish the declaration that a function value names, when no call selected it.
    fn function_value(&mut self, expression: &Expression) {
        let key = ValueUseKey::value(expression);
        if self.bodies.records().callee(&key).is_some()
            || self
                .store
                .resolve_to_function_type(expression.get_type().unwrap_forall())
                .is_none()
        {
            return;
        }
        let definition = resolved_definition(expression)
            .map(|definition| unaliased_owner(self.store, definition));
        let checked = CheckedCallee {
            operation: definition
                .as_deref()
                .map_or(CallOperation::Function, |definition| {
                    method_value_operation(self.store, definition)
                }),
            definition,
            type_arguments: resolved_instantiation(expression)
                .cloned()
                .unwrap_or_default(),
            copies_storage: false,
            binds_receiver: false,
            interface_method: None,
            span: expression.get_span(),
        };
        self.bodies.insert_callee(key, checked);
    }

    /// The declared type of the function or method `definition`.
    fn declared_type(&self, definition: &str) -> Option<&Type> {
        if let Some(declared) = self.store.get_definition(definition) {
            return Some(&declared.ty);
        }
        let (owner, method) = definition.rsplit_once('.')?;
        self.store
            .get_method(owner, method)
            .map(|declared| &declared.ty)
    }

    /// A method receiver whose address the method takes, or whose reference it reads through.
    fn receiver_use(&mut self, receiver: &Expression, change: ReceiverCoercion) {
        let receiver = receiver.unwrap_parens();
        let key = ValueUseKey::value(receiver);
        if !self.obligations.contains_key(&key) {
            self.fail(receiver.get_span(), "this receiver has no checked slot");
        }
        let source = receiver.get_type();
        let slot = match change {
            ReceiverCoercion::AutoAddress => Type::Compound {
                kind: CompoundKind::Ref,
                args: vec![source.clone()],
                writable: false,
            },
            ReceiverCoercion::AutoDeref => match self.store.peel_alias(&source).inner() {
                Some(pointee) => pointee,
                None => {
                    self.fail(
                        receiver.get_span(),
                        "this receiver reads through no reference",
                    );
                    source.clone()
                }
            },
        };
        self.publish(slot, Adaptation::Receiver(change), receiver);
        self.positions(receiver, &source);
    }

    fn publish(&mut self, slot: Type, adaptation: Adaptation, expression: &Expression) {
        self.bodies.insert_use(
            ValueUseKey::value(expression),
            CheckedValueUse {
                value_type: expression.get_type(),
                slot,
                adaptation,
                span: expression.get_span(),
            },
        );
    }

    fn fail(&mut self, span: Span, reason: &'static str) {
        self.failures.push((span, reason));
    }

    fn not_inferred(&mut self, span: Span, ty: &Type, kind: UninferredKind, reason: &'static str) {
        let mut variables = Vec::new();
        collect_variables(ty, &mut variables);
        if !variables.is_empty() {
            self.uninferred.push(Uninferred {
                span,
                variables,
                kind,
            });
        } else if is_unresolved(ty) {
            self.fail(span, reason);
        }
    }

    /// The value-use positions of `expression`, from the syntax of each construct.
    fn positions(&mut self, expression: &Expression, slot: &Type) {
        match expression {
            Expression::Literal { literal, .. } => match literal {
                Literal::Slice(elements) => {
                    for element in elements {
                        self.value_use(element, None);
                    }
                }
                Literal::FormatString(parts) => {
                    for part in parts {
                        if let FormatStringPart::Expression(part) = part {
                            self.value_use(part, None);
                        }
                    }
                }
                Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_) => {}
            },
            Expression::Function { body, .. } => {
                if let Some(body) = body.definition() {
                    self.function_body(expression, body);
                }
            }
            Expression::Lambda { body, .. } => self.function_body(expression, body),
            Expression::Block { items, .. }
            | Expression::TryBlock { items, .. }
            | Expression::RecoverBlock { items, .. } => {
                let delegates = matches!(expression, Expression::Block { .. });
                if !delegates {
                    let block_ty = self.store.peel_alias(&expression.get_type());
                    let needs_context = |ty: &Type| ty.is_variable() || ty.is_placeholder();
                    if needs_context(&block_ty) || needs_context(&block_ty.ok_type()) {
                        self.fail(expression.get_span(), "this block has no checked ok type");
                    }
                }
                let failure_slot = matches!(expression, Expression::TryBlock { .. })
                    .then(|| expression.get_type());
                self.in_failure_slot(failure_slot, |finalizer| {
                    if let Some((last, rest)) = items.split_last() {
                        for item in rest {
                            finalizer.discarded_use(item);
                        }
                        if is_statement(last) {
                            // The statement-tail check rejects a statement where the block must produce a value.
                            finalizer.discarded_use(last);
                        } else {
                            finalizer.value_use(last, delegates.then(|| slot.clone()));
                        }
                    }
                });
            }
            Expression::Let { value, mode, .. } => {
                self.value_use(value, None);
                if let Some(block) = mode.else_block() {
                    self.value_use(block, None);
                }
            }
            Expression::Call {
                expression: callee,
                args,
                spread,
                ..
            } => {
                // An instance method access publishes its own callee at its position.
                if !is_instance_method_access(callee.unwrap_value_wrappers()) {
                    self.callee(callee);
                }
                self.value_use(callee, None);
                for argument in args {
                    self.value_use(argument, None);
                }
                if let Some(spread) = spread {
                    self.value_use(spread, None);
                }
            }
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.unadapted_use(condition);
                self.branches(consequence, alternative.as_deref(), slot);
            }
            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.unadapted_use(scrutinee);
                self.branches(consequence, alternative.expression(), slot);
            }
            Expression::Match { subject, arms, .. } => {
                self.unadapted_use(subject);
                self.match_arms(arms, slot);
            }
            Expression::Tuple { elements, .. } => {
                for element in elements {
                    self.value_use(element, Some(element.get_type()));
                }
            }
            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                for field in field_assignments {
                    self.value_use(&field.value, None);
                }
                if let Some(spread) = spread.as_expression() {
                    self.value_use(spread, None);
                }
            }
            Expression::Propagate {
                expression: operand,
                span,
                ..
            } => {
                self.value_use(operand, None);
                let operand_ty = self.store.peel_alias(&operand.get_type());
                if let Some(failure_slot) = self.failure_slots.last()
                    && self.store.peel_alias(failure_slot).demoted() == operand_ty.demoted()
                {
                    self.bodies.insert_forwarded_failure(operand);
                }
                if operand_ty.is_result() {
                    self.special_use(
                        ValueUseKey::propagated_error(operand),
                        *span,
                        "this `?` has no checked error slot",
                        false,
                    );
                }
            }
            Expression::Cast {
                expression: operand,
                span,
                ..
            } => {
                self.value_use(operand, None);
                self.special_use(
                    ValueUseKey::cast_operand(expression.id()),
                    *span,
                    "this cast has no checked operand",
                    true,
                );
            }
            Expression::Paren {
                expression: inner, ..
            } => self.value_use(inner, Some(slot.clone())),
            Expression::Select { arms, .. } => {
                for arm in arms {
                    match arm {
                        SelectArm::Receive {
                            receive_expression: operation,
                            body,
                            ..
                        }
                        | SelectArm::Send {
                            send_expression: operation,
                            body,
                        } => {
                            self.unadapted_use(operation);
                            self.value_use(body, Some(slot.clone()));
                        }
                        SelectArm::MatchReceive {
                            receive_expression,
                            arms,
                        } => {
                            self.unadapted_use(receive_expression);
                            self.match_arms(arms, slot);
                        }
                        SelectArm::WildCard { body, .. } => {
                            self.value_use(body, Some(slot.clone()));
                        }
                    }
                }
            }
            Expression::ImplBlock { methods, .. } => {
                for method in methods {
                    self.value_use(method, None);
                }
            }
            Expression::Const { expression, .. } => {
                if let Some(value) = expression.value() {
                    self.value_use(value, None);
                }
            }
            Expression::Assignment { target, value, .. } => {
                self.unadapted_use(target);
                self.value_use(value, None);
            }
            Expression::Binary { left, right, .. } => {
                self.value_use(left, None);
                self.value_use(right, None);
            }
            Expression::IndexedAccess {
                expression: container,
                index,
                ..
            } => {
                self.value_use(container, None);
                match index.as_ref() {
                    // A range with no bounds carries no value into the slice operation.
                    Expression::Range {
                        start: None,
                        end: None,
                        ..
                    } => self.discarded_use(index),
                    _ => self.value_use(index, None),
                }
            }
            Expression::While {
                condition: head,
                body,
                ..
            }
            | Expression::WhileLet {
                scrutinee: head,
                body,
                ..
            }
            | Expression::For {
                iterable: head,
                body,
                ..
            } => {
                self.unadapted_use(head);
                self.in_loop(None, |finalizer| finalizer.discarded_use(body));
            }
            Expression::Loop { body, .. } => {
                let loop_slot = LoopSlot {
                    slot: slot.clone(),
                    delegation: self.delegations.len().checked_sub(1),
                };
                self.in_loop(Some(loop_slot), |finalizer| finalizer.discarded_use(body));
            }
            Expression::DotAccess {
                expression: receiver,
                ..
            } => {
                if is_instance_method_access(expression) {
                    self.callee(expression);
                } else {
                    self.function_value(expression);
                }
                match self.receivers.get(&ValueUseKey::value(receiver)).copied() {
                    Some(change) => self.receiver_use(receiver, change),
                    None => self.unadapted_use(receiver),
                }
            }
            Expression::Return { expression, .. } => self.value_use(expression, None),
            Expression::Unary { expression, .. }
            | Expression::Reference { expression, .. }
            | Expression::Task { expression, .. }
            | Expression::Defer { expression, .. }
            | Expression::Assert { expression, .. } => self.value_use(expression, None),
            Expression::Break { value, .. } => {
                if let Some(value) = value {
                    let loop_slot = self.loop_slots.last().cloned().flatten();
                    let delegation = loop_slot.as_ref().and_then(|slot| slot.delegation);
                    self.delegations.push(false);
                    self.value_use(value, loop_slot.map(|slot| slot.slot));
                    let converts = self.delegations.pop().unwrap_or(false);
                    if converts && let Some(index) = delegation {
                        self.delegations[index] = true;
                    }
                }
            }
            Expression::Range { start, end, .. } => {
                for bound in [start, end].into_iter().flatten() {
                    self.value_use(bound, None);
                }
            }
            Expression::Identifier { .. } => self.function_value(expression),
            Expression::Interface { .. }
            | Expression::Unit { .. }
            | Expression::Continue { .. }
            | Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::PackageImport { .. }
            | Expression::RawGo { .. } => {}
        }
    }

    /// The adaptation into `slot`, from the solved types and the conformances selected at the use, where an explicit cast converts the value itself.
    fn adaptation(
        &mut self,
        written_source: &Type,
        written_slot: &Type,
        span: Span,
        witnesses: &[ConformanceWitness],
        is_cast: bool,
    ) -> Adaptation {
        // A valueless expression or a discarded value moves no value into the slot.
        if written_source.is_ignored() || written_slot.is_ignored() {
            return Adaptation::Identity;
        }
        // A value of type never does not reach its slot.
        if written_source.is_never() {
            return Adaptation::Identity;
        }
        // The checker reported the error, and a file with errors publishes no body.
        if written_source.contains_error() || written_slot.contains_error() {
            return Adaptation::Identity;
        }
        if is_unresolved(written_source) || is_unresolved(written_slot) {
            let both = Type::Tuple(vec![written_source.clone(), written_slot.clone()]);
            self.not_inferred(
                span,
                &both,
                UninferredKind::Value,
                "this value use has an unresolved type",
            );
            return Adaptation::Identity;
        }
        let store = self.store;
        if is_go_identical(store, written_source, written_slot) {
            return Adaptation::Identity;
        }
        let source = store.peel_alias(written_source);
        let slot = store.peel_alias(written_slot);
        if let Type::Nominal { id, .. } = &slot
            && store.is_nominal_defined_type(id)
            && store
                .underlying_type(&slot)
                .is_some_and(|underlying| is_go_identical(store, &underlying, written_source))
        {
            return Adaptation::Newtype;
        }
        if let Type::Nominal { id, .. } = &source
            && store.is_nominal_defined_type(id)
            && store
                .underlying_type(&source)
                .is_some_and(|underlying| is_go_identical(store, &underlying, written_slot))
        {
            return if matches!(slot, Type::Tuple(_)) {
                Adaptation::Newtype
            } else {
                Adaptation::Identity
            };
        }
        match (&source, &slot) {
            (Type::Tuple(sources), Type::Tuple(slots)) if sources.len() == slots.len() => {
                let elements: Vec<Adaptation> = sources
                    .iter()
                    .zip(slots)
                    .map(|(source, slot)| self.adaptation(source, slot, span, witnesses, is_cast))
                    .collect();
                if !elements.iter().any(Adaptation::converts) {
                    Adaptation::Identity
                } else {
                    Adaptation::Tuple(elements)
                }
            }
            (
                Type::Array {
                    length: source_length,
                    element: source_element,
                },
                Type::Array {
                    length: slot_length,
                    element: slot_element,
                },
            ) if source_length == slot_length => {
                let element =
                    self.adaptation(source_element, slot_element, span, witnesses, is_cast);
                if !element.converts() {
                    Adaptation::Identity
                } else {
                    Adaptation::Array(Box::new(element))
                }
            }
            _ if slot.is_option()
                && !source.is_option()
                && self.is_go_package_interface(&source) =>
            {
                Adaptation::ImplicitSome {
                    payload: Box::new(self.adaptation(
                        &source,
                        &slot.ok_type(),
                        span,
                        witnesses,
                        is_cast,
                    )),
                }
            }
            _ if store.is_interface(&slot) || store.resolves_to_unknown(&slot) => {
                let witness = witnesses
                    .iter()
                    .find(|witness| {
                        store.peel_alias(&witness.receiver) == source
                            && store.peel_alias(&witness.interface) == slot
                    })
                    .cloned();
                if witness.is_none() && self.needs_witness(&source, &slot) {
                    self.fail(span, "this interface conversion has no checked conformance");
                }
                Adaptation::Interface(Box::new(InterfaceConversion {
                    source: written_source.clone(),
                    target: written_slot.clone(),
                    source_shape: store.receiver_shape(written_source),
                    target_interface: store.is_interface(&slot).then(|| slot.clone()),
                    witness,
                }))
            }
            (Type::Function(source_function), Type::Function(slot_function))
                if source_function.return_type.is_never()
                    && !slot_function.return_type.is_never()
                    && same_parameters(store, source_function, slot_function) =>
            {
                Adaptation::DivergingFunction
            }
            // The cast operation converts its operand to the cast type.
            _ if is_cast => Adaptation::Identity,
            _ => {
                self.mismatches.push(Mismatch {
                    source: written_source.clone(),
                    slot: written_slot.clone(),
                    span,
                });
                Adaptation::Identity
            }
        }
    }

    fn is_go_package_interface(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal { id, .. } if id.starts_with("go:") && self.store.is_interface(ty))
    }

    /// A named source or a type parameter satisfies a method-bearing interface through the methods that the checker selected.
    fn needs_witness(&self, source: &Type, target: &Type) -> bool {
        let store = self.store;
        let stripped = store.peel_alias(&source.strip_refs());
        matches!(stripped, Type::Nominal { .. } | Type::Parameter(_))
            && !interface_requirements(target, |id| store.get_definition(id)).is_empty()
    }
}

pub(super) fn is_instance_method_access(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::DotAccess {
            resolution: DotAccessResolution::InstanceMethod { .. },
            ..
        }
    )
}

/// The slot of a `break` value, and the delegation of the loop that receives it.
#[derive(Clone)]
struct LoopSlot {
    slot: Type,
    delegation: Option<usize>,
}

/// Constructs whose value comes from the children that produce it.
fn delegates(expression: &Expression) -> bool {
    match expression {
        Expression::Paren { .. }
        | Expression::Match { .. }
        | Expression::Select { .. }
        | Expression::Loop { .. } => true,
        Expression::Block { items, .. } => !items.is_empty(),
        Expression::If { alternative, .. } => alternative.is_some(),
        Expression::IfLet { alternative, .. } => alternative.expression().is_some(),
        _ => false,
    }
}

/// Items that produce no value, which the statement-tail check rejects as the value of a block.
fn is_statement(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Let { .. }
            | Expression::Assignment { .. }
            | Expression::Task { .. }
            | Expression::Defer { .. }
            | Expression::Assert { .. }
    )
}

/// Whether `ty` holds an inference variable that inference left unsolved, or a type that never entered inference.
/// An instantiated type parameter that no use constrains is an unconstrained type argument instead.
fn is_unresolved(ty: &Type) -> bool {
    matches!(ty, Type::Var { hint: None, .. } | Type::Uninferred)
        || ty.children().into_iter().any(is_unresolved)
}

struct Uninferred {
    span: Span,
    variables: Vec<TypeVarId>,
    kind: UninferredKind,
}

#[derive(Clone, Copy)]
enum UninferredKind {
    Closure,
    Value,
}

fn collect_variables(ty: &Type, variables: &mut Vec<TypeVarId>) {
    if let Type::Var { id, .. } = ty {
        variables.push(*id);
    }
    for child in ty.children() {
        collect_variables(child, variables);
    }
}

struct Mismatch {
    source: Type,
    slot: Type,
    span: Span,
}

fn fallible_block_source(store: &Store, expression: &Expression, slot: &Type) -> Type {
    let ty = expression.get_type();
    if !matches!(
        expression,
        Expression::TryBlock { .. } | Expression::RecoverBlock { .. }
    ) || !ty.ok_type().is_never()
    {
        return ty;
    }
    let slot = store.peel_alias(slot);
    if (slot.is_option() && ty.is_option()) || (slot.is_result() && ty.is_result()) {
        slot
    } else {
        ty
    }
}

fn is_go_identical(store: &Store, left: &Type, right: &Type) -> bool {
    let left = store.peel_alias(left);
    let right = store.peel_alias(right);
    let all_identical = |lefts: &[Type], rights: &[Type]| {
        lefts.len() == rights.len()
            && lefts
                .iter()
                .zip(rights)
                .all(|(left, right)| is_go_identical(store, left, right))
    };
    match (&left, &right) {
        (Type::Simple(left), Type::Simple(right)) => {
            left == right || simple_kinds_are_go_aliases(*left, *right)
        }
        (
            Type::Compound {
                kind: left_kind,
                args: left_arguments,
                ..
            },
            Type::Compound {
                kind: right_kind,
                args: right_arguments,
                ..
            },
        ) => left_kind == right_kind && all_identical(left_arguments, right_arguments),
        (
            Type::Nominal {
                id: left_id,
                params: left_parameters,
                ..
            },
            Type::Nominal {
                id: right_id,
                params: right_parameters,
                ..
            },
        ) => {
            (left_id == right_id || are_go_type_aliases(left_id, right_id))
                && all_identical(left_parameters, right_parameters)
        }
        (Type::Function(left), Type::Function(right)) => {
            same_parameters(store, left, right)
                && is_go_identical(store, &left.return_type, &right.return_type)
        }
        (Type::Tuple(left), Type::Tuple(right)) => all_identical(left, right),
        (
            Type::Array {
                length: left_length,
                element: left_element,
            },
            Type::Array {
                length: right_length,
                element: right_element,
            },
        ) => left_length == right_length && is_go_identical(store, left_element, right_element),
        (
            Type::Forall {
                vars: left_variables,
                body: left_body,
            },
            Type::Forall {
                vars: right_variables,
                body: right_body,
            },
        ) => left_variables == right_variables && is_go_identical(store, left_body, right_body),
        _ => left == right,
    }
}

fn same_parameters(store: &Store, left: &FunctionType, right: &FunctionType) -> bool {
    left.params.len() == right.params.len()
        && left
            .params
            .iter()
            .zip(&right.params)
            .all(|(left, right)| is_go_identical(store, &left.ty, &right.ty))
}
