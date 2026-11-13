use std::iter;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::{Binding, BindingId, Expression, LetMode, Pattern, collect_pattern_bindings};
use syntax::program::{DotAccessResolution, ReceiverCoercion};
use syntax::types::Type;

/// Pairs each `let` with the same-block binding of the same type that it hides,
/// when no closure or shared storage can see the hidden variable afterwards.
#[derive(Default)]
pub(crate) struct ReusableBindings {
    hidden: HashMap<BindingId, BindingId>,
}

impl ReusableBindings {
    pub(crate) fn of_function(params: &[Binding], body: &Expression) -> Self {
        let mut hidden = HashMap::default();
        let mut shared = HashSet::default();
        collect(body, params, false, &mut hidden, &mut shared);
        hidden.retain(|_, id| !shared.contains(id));
        Self { hidden }
    }

    pub(crate) fn hidden_by(&self, id: BindingId) -> Option<BindingId> {
        self.hidden.get(&id).copied()
    }
}

type BlockBindings = HashMap<String, (BindingId, Type)>;

fn simple_binding(binding: &Binding) -> Option<(&str, BindingId)> {
    match &binding.pattern {
        Pattern::Identifier {
            identifier,
            binding: Some(id),
            ..
        } => Some((identifier, *id)),
        _ => None,
    }
}

fn record(bindings: &mut BlockBindings, binding: &Binding) {
    if let Some((name, id)) = simple_binding(binding) {
        bindings.insert(name.to_string(), (id, binding.ty.clone()));
    }
}

fn collect(
    expression: &Expression,
    params: &[Binding],
    captured: bool,
    hidden: &mut HashMap<BindingId, BindingId>,
    shared: &mut HashSet<BindingId>,
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
                    pair_let(&mut bindings, binding, value, mode, hidden);
                }
            }
            captured
        }
        Expression::Lambda { params, body, .. } => {
            collect(body, params, true, hidden, shared);
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
            resolution,
            ..
        } = callee.unwrap_parens() =>
        {
            if resolution.receiver_coercion() == Some(ReceiverCoercion::AutoAddress) {
                shared.extend(receiver.root_binding_id());
            }
            for child in iter::once(receiver.as_ref())
                .chain(args)
                .chain(spread.as_deref())
            {
                collect(child, &[], captured, hidden, shared);
            }
            return;
        }
        // A method value can capture its receiver.
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
        collect(child, &[], captured, hidden, shared);
    }
}

fn pair_let(
    bindings: &mut BlockBindings,
    binding: &Binding,
    value: &Expression,
    mode: &LetMode,
    hidden: &mut HashMap<BindingId, BindingId>,
) {
    match simple_binding(binding) {
        Some((name, id)) if matches!(mode, LetMode::Plain) && value.get_type() == binding.ty => {
            if let Some((hidden_id, ty)) = bindings.get(name)
                && *ty == binding.ty
            {
                hidden.insert(id, *hidden_id);
            }
            record(bindings, binding);
        }
        _ => {
            for (name, _) in collect_pattern_bindings(&binding.pattern) {
                bindings.remove(&name);
            }
        }
    }
}
