use std::iter;

use crate::lookup::facts::emit_facts::EmitFacts;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{Binding, BindingId, Expression, LetMode, Pattern};
use syntax::program::{DotAccessResolution, ReceiverCoercion};
use syntax::types::Type;

#[derive(Clone, Default)]
pub(crate) struct ReusableBindings {
    hidden: FxHashMap<BindingId, BindingId>,
}

impl ReusableBindings {
    pub(crate) fn of_function(
        params: &[Binding],
        body: &Expression,
        facts: &EmitFacts<'_>,
    ) -> Self {
        let mut hidden = FxHashMap::default();
        let mut shared = FxHashSet::default();
        collect(body, params, false, facts, &mut hidden, &mut shared);
        hidden.retain(|_, id| !shared.contains(id));
        Self { hidden }
    }

    pub(crate) fn hidden_by(&self, id: BindingId) -> Option<BindingId> {
        self.hidden.get(&id).copied()
    }
}

type BlockBindings = FxHashMap<BindingId, Type>;

fn simple_binding(binding: &Binding) -> Option<BindingId> {
    match &binding.pattern {
        Pattern::Identifier {
            binding: Some(id), ..
        } => Some(*id),
        _ => None,
    }
}

fn record(bindings: &mut BlockBindings, binding: &Binding) {
    if let Some(id) = simple_binding(binding) {
        bindings.insert(id, binding.ty.clone());
    }
}

fn collect(
    expression: &Expression,
    params: &[Binding],
    captured: bool,
    facts: &EmitFacts<'_>,
    hidden: &mut FxHashMap<BindingId, BindingId>,
    shared: &mut FxHashSet<BindingId>,
) {
    let captured = match expression {
        Expression::Block { items, .. } => {
            let mut bindings = BlockBindings::default();
            for param in params {
                record(&mut bindings, param);
            }
            for item in items {
                if let Expression::Let {
                    binding,
                    value,
                    mode,
                    ..
                } = item
                {
                    pair_let(&mut bindings, binding, value, mode, hidden, facts);
                }
            }
            captured
        }
        Expression::Lambda { params, body, .. } => {
            collect(body, params, true, facts, hidden, shared);
            return;
        }
        Expression::Task { .. } | Expression::Defer { .. } => true,
        Expression::Identifier { resolution, .. } => {
            if captured && let Some(id) = resolution.binding_id() {
                shared.insert(id);
            }
            captured
        }
        Expression::Reference { expression, .. } => {
            shared.extend(expression.root_binding_id());
            captured
        }
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } if let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = &**callee =>
        {
            if facts.dot_receiver_coercion(receiver, member) == Some(ReceiverCoercion::AutoAddress)
            {
                shared.extend(receiver.root_binding_id());
            }
            for child in iter::once(receiver.as_ref())
                .chain(args)
                .chain(spread.as_deref())
            {
                collect(child, &[], captured, facts, hidden, shared);
            }
            return;
        }
        Expression::DotAccess {
            expression,
            resolution:
                DotAccessResolution::InstanceMethod { .. }
                | DotAccessResolution::InstanceMethodValue { .. },
            ..
        } => {
            shared.extend(expression.root_binding_id());
            captured
        }
        _ => captured,
    };
    for child in expression.children() {
        collect(child, &[], captured, facts, hidden, shared);
    }
}

fn pair_let(
    bindings: &mut BlockBindings,
    binding: &Binding,
    value: &Expression,
    mode: &LetMode,
    hidden: &mut FxHashMap<BindingId, BindingId>,
    facts: &EmitFacts<'_>,
) {
    let Some(id) = simple_binding(binding) else {
        return;
    };
    if !matches!(mode, LetMode::Plain) || !facts.same_type(&value.get_type(), &binding.ty) {
        return;
    }
    if let Some(hidden_id) = facts.hidden_binding(id)
        && let Some(ty) = bindings.get(&hidden_id)
        && facts.same_type(ty, &binding.ty)
    {
        hidden.insert(id, hidden_id);
    }
    record(bindings, binding);
}
