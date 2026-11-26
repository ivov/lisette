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
        let mut collector = Collector {
            facts,
            hidden: FxHashMap::default(),
            shared: FxHashSet::default(),
        };
        collector.collect(body, params, CaptureContext::Function);
        let Collector {
            mut hidden, shared, ..
        } = collector;
        hidden.retain(|_, id| !shared.contains(id));
        Self { hidden }
    }

    pub(crate) fn hidden_by(&self, id: BindingId) -> Option<BindingId> {
        self.hidden.get(&id).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureContext {
    Function,
    Closure,
}

type BlockBindings = FxHashMap<BindingId, Type>;

fn simple_binding(binding: &Binding) -> Option<BindingId> {
    let Pattern::Identifier {
        binding: Some(id), ..
    } = &binding.pattern
    else {
        return None;
    };
    Some(*id)
}

fn record(bindings: &mut BlockBindings, binding: &Binding) {
    if let Some(id) = simple_binding(binding) {
        bindings.insert(id, binding.ty.clone());
    }
}

struct Collector<'f, 'a> {
    facts: &'f EmitFacts<'a>,
    hidden: FxHashMap<BindingId, BindingId>,
    shared: FxHashSet<BindingId>,
}

impl Collector<'_, '_> {
    #[allow(clippy::wildcard_enum_match_arm)]
    fn collect(&mut self, expression: &Expression, params: &[Binding], context: CaptureContext) {
        let context = match expression {
            Expression::Block { items, .. } => {
                self.pair_block_bindings(params, items);
                context
            }
            Expression::Lambda { params, body, .. } => {
                self.collect(body, params, CaptureContext::Closure);
                return;
            }
            Expression::Task { .. } | Expression::Defer { .. } => CaptureContext::Closure,
            Expression::Identifier { resolution, .. } => {
                if context == CaptureContext::Closure
                    && let Some(id) = resolution.binding_id()
                {
                    self.shared.insert(id);
                }
                context
            }
            Expression::Reference { expression, .. }
            | Expression::DotAccess {
                expression,
                resolution:
                    DotAccessResolution::InstanceMethod { .. }
                    | DotAccessResolution::InstanceMethodValue { .. },
                ..
            } => {
                self.shared.extend(expression.root_binding_id());
                context
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
                if self.facts.dot_receiver_coercion(receiver, member)
                    == Some(ReceiverCoercion::AutoAddress)
                {
                    self.shared.extend(receiver.root_binding_id());
                }
                for child in iter::once(receiver.as_ref())
                    .chain(args)
                    .chain(spread.as_deref())
                {
                    self.collect(child, &[], context);
                }
                return;
            }
            _ => context,
        };

        for child in expression.children() {
            self.collect(child, &[], context);
        }
    }

    fn pair_block_bindings(&mut self, params: &[Binding], items: &[Expression]) {
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
                pair_let(
                    &mut bindings,
                    binding,
                    value,
                    mode,
                    &mut self.hidden,
                    self.facts,
                );
            }
        }
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
