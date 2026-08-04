use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::BindingId;
use syntax::ast::Span;
use syntax::types::{Symbol, Type};

#[derive(Debug, Clone, Default)]
pub struct DepthCounter(usize);

impl DepthCounter {
    pub(crate) fn increment(&mut self) {
        self.0 += 1;
    }
    pub(crate) fn decrement(&mut self) {
        self.0 = self
            .0
            .checked_sub(1)
            .expect("depth counter must be incremented before it is decremented");
    }
    pub(crate) fn is_active(&self) -> bool {
        self.0 > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UseContext {
    #[default]
    Statement,
    Value,
    Callee,
    AssignmentTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryCarrier {
    Result,
    Option,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TryUsage {
    #[default]
    Unused,
    Unknown,
    Carrier(TryCarrier),
}

impl TryUsage {
    /// Record one `?` operand and report whether it conflicts with a carrier
    /// already established by an earlier operand.
    pub(crate) fn observe(&mut self, observed: Option<TryCarrier>) -> bool {
        match (*self, observed) {
            (Self::Unused, None) => *self = Self::Unknown,
            (Self::Unused | Self::Unknown, Some(carrier)) => *self = Self::Carrier(carrier),
            (Self::Carrier(TryCarrier::Result), Some(TryCarrier::Option))
            | (Self::Carrier(TryCarrier::Option), Some(TryCarrier::Result)) => return true,
            _ => {}
        }
        false
    }

    pub(crate) fn was_used(self) -> bool {
        self != Self::Unused
    }
}

#[derive(Debug, Clone)]
pub struct TryBlockContext {
    pub(crate) ok_ty: Type,
    pub(crate) err_ty: Type,
    pub(crate) usage: TryUsage,
    pub(crate) entry_loop_depth: usize,
}

#[derive(Debug, Clone)]
pub struct RecoverBlockContext {
    pub(crate) entry_loop_depth: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum LoopContext {
    Statement,
    Value(Type),
}

#[derive(Debug, Clone, Default)]
enum PropagationContext {
    #[default]
    None,
    Try(TryBlockContext),
    Recover(RecoverBlockContext),
}

#[derive(Debug, Clone)]
pub(crate) enum DeferredMapKeyCheck {
    Comparable { key: Type, span: Span },
    Bounds { key: Type, span: Span },
}

#[derive(Debug, Clone, Default)]
enum TestContext {
    #[default]
    None,
    Handle,
    Function(EcoString),
}

/// Traversal state inherited by nested lexical scopes and restored on pop.
#[derive(Debug, Clone, Default)]
struct InheritedContext {
    loops: Vec<LoopContext>,
    defer_block_depth: DepthCounter,
    negation_depth: DepthCounter,
    invariant_depth: DepthCounter,
    use_context: UseContext,
    dot_access_base: bool,
    let_binding_rhs: bool,
    test_context: TestContext,
}

#[derive(Debug, Clone, Copy)]
enum ScopedValueKind {
    Value,
    Binding { id: BindingId, mutable: bool },
    Const,
}

#[derive(Debug, Clone)]
struct ScopedValue {
    ty: Type,
    kind: ScopedValueKind,
}

#[derive(Debug, Clone, Default)]
struct GenericContext {
    parameters: HashMap<String, GenericParameter>,
}

#[derive(Debug, Clone)]
struct GenericParameter {
    index: usize,
    bounds: Option<(Symbol, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub struct Scope {
    values: HashMap<String, ScopedValue>,
    generics: Option<GenericContext>,
    pub(crate) fn_return_type: Option<Type>,
    is_lambda: bool,
    deferred_map_key_checks: Vec<DeferredMapKeyCheck>,
    propagation_context: PropagationContext,
    impl_receiver_type: Option<Type>,
    inherited: InheritedContext,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    fn new() -> Self {
        Scope {
            values: HashMap::default(),
            generics: None,
            fn_return_type: None,
            is_lambda: false,
            deferred_map_key_checks: Vec::new(),
            propagation_context: PropagationContext::None,
            impl_receiver_type: None,
            inherited: InheritedContext::default(),
        }
    }

    pub(crate) fn insert_value(&mut self, name: String, ty: Type) {
        self.values.insert(
            name,
            ScopedValue {
                ty,
                kind: ScopedValueKind::Value,
            },
        );
    }

    pub(crate) fn insert_value_if_absent(&mut self, name: String, ty: Type) {
        self.values.entry(name).or_insert(ScopedValue {
            ty,
            kind: ScopedValueKind::Value,
        });
    }

    pub(crate) fn insert_binding(&mut self, name: String, ty: Type, id: BindingId, mutable: bool) {
        self.values.insert(
            name,
            ScopedValue {
                ty,
                kind: ScopedValueKind::Binding { id, mutable },
            },
        );
    }

    pub(crate) fn insert_const(&mut self, name: String, ty: Type) {
        self.values.insert(
            name,
            ScopedValue {
                ty,
                kind: ScopedValueKind::Const,
            },
        );
    }

    fn generics_mut(&mut self) -> &mut GenericContext {
        self.generics.get_or_insert_with(GenericContext::default)
    }
}

pub struct Scopes {
    stack: Vec<Scope>,
}

impl Default for Scopes {
    fn default() -> Self {
        Self::new()
    }
}

impl Scopes {
    pub(crate) fn new() -> Self {
        Scopes {
            stack: vec![Scope::new()],
        }
    }

    pub(crate) fn current(&self) -> &Scope {
        self.stack.last().expect("scope stack must not be empty")
    }

    pub(crate) fn current_mut(&mut self) -> &mut Scope {
        self.stack
            .last_mut()
            .expect("scope stack must not be empty")
    }

    pub(crate) fn push(&mut self) {
        let scope = Scope {
            inherited: self.current().inherited.clone(),
            ..Scope::new()
        };
        self.stack.push(scope);
    }

    pub(crate) fn pop(&mut self) {
        assert!(self.stack.len() > 1, "root scope cannot be popped");
        self.stack.pop();
    }

    /// Look up a value by walking the scope stack from top to bottom.
    pub(crate) fn lookup_value(&self, name: &str) -> Option<&Type> {
        self.lookup_scoped_value(name).map(|value| &value.ty)
    }

    /// Check whether the visible value is a mutable binding.
    pub(crate) fn lookup_mutable(&self, name: &str) -> bool {
        matches!(
            self.lookup_scoped_value(name).map(|value| value.kind),
            Some(ScopedValueKind::Binding { mutable: true, .. })
        )
    }

    /// Whether the visible value is a block-local `const`.
    pub(crate) fn lookup_const(&self, name: &str) -> bool {
        matches!(
            self.lookup_scoped_value(name).map(|value| value.kind),
            Some(ScopedValueKind::Const)
        )
    }

    /// Look up a binding ID by walking the scope stack from top to bottom.
    pub(crate) fn lookup_binding_id(&self, name: &str) -> Option<BindingId> {
        match self.lookup_scoped_value(name)?.kind {
            ScopedValueKind::Binding { id, .. } => Some(id),
            ScopedValueKind::Value | ScopedValueKind::Const => None,
        }
    }

    pub(crate) fn mark_lambda_scope(&mut self) {
        self.current_mut().is_lambda = true;
    }

    pub(crate) fn shadowed_capturable_binding(&self, name: &str) -> Option<BindingId> {
        let mut crossed_lambda = false;
        for scope in self.stack.iter().rev() {
            if let Some(value) = scope.values.get(name) {
                return match value.kind {
                    ScopedValueKind::Binding { id, .. } if crossed_lambda => Some(id),
                    _ => None,
                };
            }
            crossed_lambda |= scope.is_lambda;
        }
        None
    }

    /// Whether resolving `name` crosses a function scope, meaning captured.
    pub(crate) fn binding_crosses_function_boundary(&self, name: &str) -> bool {
        let mut crossed = false;
        for scope in self.stack.iter().rev() {
            if let Some(value) = scope.values.get(name) {
                return crossed && matches!(value.kind, ScopedValueKind::Binding { .. });
            }
            if scope.fn_return_type.is_some() {
                crossed = true;
            }
        }
        false
    }

    fn lookup_scoped_value(&self, name: &str) -> Option<&ScopedValue> {
        self.stack
            .iter()
            .rev()
            .find_map(|scope| scope.values.get(name))
    }

    /// Look up a type parameter by walking the scope stack from top to bottom.
    pub(crate) fn lookup_type_param(&self, name: &str) -> Option<usize> {
        self.stack.iter().rev().find_map(|scope| {
            scope
                .generics
                .as_ref()?
                .parameters
                .get(name)
                .map(|parameter| parameter.index)
        })
    }

    pub(crate) fn insert_type_param(&mut self, name: String, index: usize) {
        self.current_mut().generics_mut().parameters.insert(
            name,
            GenericParameter {
                index,
                bounds: None,
            },
        );
    }

    pub(crate) fn insert_trait_bound(&mut self, parameter: Symbol, bound: Type) {
        let name = parameter.last_segment().to_string();
        let generic = self
            .current_mut()
            .generics_mut()
            .parameters
            .get_mut(&name)
            .expect("a generic parameter must be in scope before recording its bounds");
        let (existing_parameter, bounds) = generic
            .bounds
            .get_or_insert_with(|| (parameter.clone(), Vec::new()));
        assert_eq!(
            existing_parameter, &parameter,
            "one generic parameter cannot have multiple qualified identities"
        );
        if !bounds.contains(&bound) {
            bounds.push(bound);
        }
    }

    /// Look up the enclosing function's return type.
    pub(crate) fn lookup_fn_return_type(&self) -> Option<&Type> {
        for scope in self.stack.iter().rev() {
            if let Some(ref ty) = scope.fn_return_type {
                return Some(ty);
            }
        }
        None
    }

    pub(crate) fn defer_map_key_check(&mut self, check: DeferredMapKeyCheck) {
        if let Some(scope) = self
            .stack
            .iter_mut()
            .rev()
            .find(|scope| scope.fn_return_type.is_some())
        {
            scope.deferred_map_key_checks.push(check);
        }
    }

    pub(crate) fn take_deferred_map_key_checks(&mut self) -> Vec<DeferredMapKeyCheck> {
        std::mem::take(&mut self.current_mut().deferred_map_key_checks)
    }

    /// Look up the enclosing try block context, stopping at function boundaries.
    pub(crate) fn lookup_try_block_context(&self) -> Option<&TryBlockContext> {
        for scope in self.stack.iter().rev() {
            if let PropagationContext::Try(context) = &scope.propagation_context {
                return Some(context);
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
    }

    pub(crate) fn lookup_try_block_context_mut(&mut self) -> Option<&mut TryBlockContext> {
        for scope in self.stack.iter_mut().rev() {
            if let PropagationContext::Try(context) = &mut scope.propagation_context {
                return Some(context);
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
    }

    /// Look up the enclosing recover block context, stopping at function boundaries.
    pub(crate) fn lookup_recover_block_context(&self) -> Option<&RecoverBlockContext> {
        for scope in self.stack.iter().rev() {
            if let PropagationContext::Recover(context) = &scope.propagation_context {
                return Some(context);
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
    }

    pub(crate) fn set_try_block_context(&mut self, context: TryBlockContext) {
        self.current_mut().propagation_context = PropagationContext::Try(context);
    }

    pub(crate) fn current_try_block_context(&self) -> Option<&TryBlockContext> {
        match &self.current().propagation_context {
            PropagationContext::Try(context) => Some(context),
            PropagationContext::None | PropagationContext::Recover(_) => None,
        }
    }

    pub(crate) fn set_recover_block_context(&mut self, context: RecoverBlockContext) {
        self.current_mut().propagation_context = PropagationContext::Recover(context);
    }

    pub(crate) fn collect_all_value_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in &self.stack {
            names.extend(scope.values.keys().cloned());
        }
        names
    }

    pub(crate) fn collect_all_trait_bounds(&self) -> HashMap<Symbol, Vec<Type>> {
        let mut all_bounds: HashMap<Symbol, Vec<Type>> = HashMap::default();
        // Walk from bottom to top so inner scopes override outer
        for scope in &self.stack {
            if let Some(generics) = &scope.generics {
                all_bounds.retain(|parameter, _| {
                    !generics.parameters.contains_key(parameter.last_segment())
                });
                for parameter in generics.parameters.values() {
                    if let Some((qualified, bounds)) = &parameter.bounds {
                        all_bounds.insert(qualified.clone(), bounds.clone());
                    }
                }
            }
        }
        all_bounds
    }

    pub(crate) fn for_each_bound_on_param<F: FnMut(&Type)>(&self, param_name: &str, mut visit: F) {
        for scope in self.stack.iter().rev() {
            if let Some(parameter) = scope
                .generics
                .as_ref()
                .and_then(|generics| generics.parameters.get(param_name))
            {
                if let Some((_, bounds)) = &parameter.bounds {
                    for bound in bounds {
                        visit(bound);
                    }
                }
                return;
            }
        }
    }

    pub(crate) fn mark_test_handle(&mut self) {
        if matches!(self.current().inherited.test_context, TestContext::None) {
            self.current_mut().inherited.test_context = TestContext::Handle;
        }
    }

    pub(crate) fn has_test_handle(&self) -> bool {
        !matches!(self.current().inherited.test_context, TestContext::None)
    }

    pub(crate) fn set_test_fn_name(&mut self, name: EcoString) {
        self.current_mut().inherited.test_context = TestContext::Function(name);
    }

    pub(crate) fn test_fn_name(&self) -> Option<&str> {
        match &self.current().inherited.test_context {
            TestContext::Function(name) => Some(name),
            TestContext::None | TestContext::Handle => None,
        }
    }

    pub(crate) fn enter_loop(&mut self, context: LoopContext) {
        self.current_mut().inherited.loops.push(context);
    }

    pub(crate) fn exit_loop(&mut self) {
        self.current_mut()
            .inherited
            .loops
            .pop()
            .expect("a loop must be entered before it is exited");
    }

    pub(crate) fn is_inside_loop(&self) -> bool {
        !self.current().inherited.loops.is_empty()
    }

    pub(crate) fn loop_depth(&self) -> usize {
        self.current().inherited.loops.len()
    }

    pub(crate) fn loop_break_type(&self) -> Option<&Type> {
        match self.current().inherited.loops.last() {
            Some(LoopContext::Value(ty)) => Some(ty),
            Some(LoopContext::Statement) | None => None,
        }
    }

    pub(crate) fn increment_defer_block_depth(&mut self) {
        self.current_mut().inherited.defer_block_depth.increment();
    }

    pub(crate) fn decrement_defer_block_depth(&mut self) {
        self.current_mut().inherited.defer_block_depth.decrement();
    }

    pub(crate) fn is_inside_defer_block(&self) -> bool {
        self.current().inherited.defer_block_depth.is_active()
    }

    pub(crate) fn increment_negation_depth(&mut self) {
        self.current_mut().inherited.negation_depth.increment();
    }

    pub(crate) fn decrement_negation_depth(&mut self) {
        self.current_mut().inherited.negation_depth.decrement();
    }

    pub(crate) fn is_inside_negation(&self) -> bool {
        self.current().inherited.negation_depth.is_active()
    }

    pub(crate) fn take_loops(&mut self) -> Vec<LoopContext> {
        std::mem::take(&mut self.current_mut().inherited.loops)
    }

    pub(crate) fn restore_loops(&mut self, loops: Vec<LoopContext>) {
        self.current_mut().inherited.loops = loops;
    }

    pub(crate) fn replace_use_context(&mut self, context: UseContext) -> UseContext {
        let prev = self.current().inherited.use_context;
        self.current_mut().inherited.use_context = context;
        prev
    }

    pub(crate) fn restore_use_context(&mut self, ctx: UseContext) {
        self.current_mut().inherited.use_context = ctx;
    }

    pub(crate) fn is_value_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::Value
    }

    pub(crate) fn is_callee_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::Callee
    }

    pub(crate) fn is_assignment_target_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::AssignmentTarget
    }

    pub(crate) fn is_dot_access_base(&self) -> bool {
        self.current().inherited.dot_access_base
    }

    pub(crate) fn replace_dot_access_base(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.current_mut().inherited.dot_access_base, value)
    }

    pub(crate) fn is_let_binding_rhs(&self) -> bool {
        self.current().inherited.let_binding_rhs
    }

    pub(crate) fn replace_let_binding_rhs(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.current_mut().inherited.let_binding_rhs, value)
    }

    pub(crate) fn enter_invariant_position(&mut self) {
        self.current_mut().inherited.invariant_depth.increment();
    }

    pub(crate) fn exit_invariant_position(&mut self) {
        self.current_mut().inherited.invariant_depth.decrement();
    }

    pub(crate) fn is_inside_invariant_position(&self) -> bool {
        self.current().inherited.invariant_depth.is_active()
    }

    pub(crate) fn set_impl_receiver_type(&mut self, ty: Type) {
        self.current_mut().impl_receiver_type = Some(ty);
    }

    pub(crate) fn impl_receiver_type(&self) -> Option<&Type> {
        self.stack
            .iter()
            .rev()
            .find_map(|scope| scope.impl_receiver_type.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadowing_replaces_all_value_metadata() {
        let mut scopes = Scopes::new();
        scopes
            .current_mut()
            .insert_binding("value".into(), Type::Error, 1, true);

        scopes.push();
        scopes
            .current_mut()
            .insert_binding("value".into(), Type::Error, 2, false);

        assert_eq!(scopes.lookup_binding_id("value"), Some(2));
        assert!(!scopes.lookup_mutable("value"));
        assert!(!scopes.lookup_const("value"));

        scopes
            .current_mut()
            .insert_const("value".into(), Type::Error);

        assert_eq!(scopes.lookup_binding_id("value"), None);
        assert!(!scopes.lookup_mutable("value"));
        assert!(scopes.lookup_const("value"));
    }

    #[test]
    fn non_binding_shadow_stops_capture_lookup() {
        let mut scopes = Scopes::new();
        scopes
            .current_mut()
            .insert_binding("value".into(), Type::Error, 1, true);

        scopes.push();
        scopes.current_mut().fn_return_type = Some(Type::Error);
        scopes
            .current_mut()
            .insert_value("value".into(), Type::Error);

        assert_eq!(scopes.lookup_binding_id("value"), None);
        assert!(!scopes.binding_crosses_function_boundary("value"));
    }

    #[test]
    fn named_test_context_always_provides_a_handle() {
        let mut scopes = Scopes::new();

        scopes.set_test_fn_name("example".into());

        assert!(scopes.has_test_handle());
        assert_eq!(scopes.test_fn_name(), Some("example"));
    }

    #[test]
    fn known_try_carrier_replaces_an_unknown_observation() {
        let mut usage = TryUsage::default();
        usage.observe(None);

        let mismatched = usage.observe(Some(TryCarrier::Result));

        assert_eq!(
            (usage, mismatched),
            (TryUsage::Carrier(TryCarrier::Result), false)
        );
    }

    #[test]
    fn conflicting_try_carrier_does_not_replace_the_first_carrier() {
        let mut usage = TryUsage::Carrier(TryCarrier::Result);

        let mismatched = usage.observe(Some(TryCarrier::Option));

        assert_eq!(
            (usage, mismatched),
            (TryUsage::Carrier(TryCarrier::Result), true)
        );
    }

    #[test]
    fn impl_receiver_lifetime_is_tied_to_its_scope() {
        let mut scopes = Scopes::new();
        scopes.push();
        scopes.set_impl_receiver_type(Type::Error);
        scopes.push();

        assert!(scopes.impl_receiver_type().is_some());

        scopes.pop();
        scopes.pop();
        assert!(scopes.impl_receiver_type().is_none());
    }

    #[test]
    fn dot_access_base_can_also_be_a_callee() {
        let mut scopes = Scopes::new();
        scopes.replace_use_context(UseContext::Callee);
        scopes.replace_dot_access_base(true);

        assert!(scopes.is_callee_context());
        assert!(scopes.is_dot_access_base());
    }

    #[test]
    fn nested_recover_preserves_enclosing_try_context() {
        let mut scopes = Scopes::new();
        scopes.set_try_block_context(TryBlockContext {
            ok_ty: Type::Error,
            err_ty: Type::Error,
            usage: TryUsage::Unused,
            entry_loop_depth: 1,
        });
        scopes.push();
        scopes.set_recover_block_context(RecoverBlockContext {
            entry_loop_depth: 2,
        });

        assert_eq!(
            scopes
                .lookup_try_block_context()
                .map(|ctx| ctx.entry_loop_depth),
            Some(1)
        );
        assert_eq!(
            scopes
                .lookup_recover_block_context()
                .map(|ctx| ctx.entry_loop_depth),
            Some(2)
        );
    }

    #[test]
    fn inner_type_parameter_shadows_outer_bounds_without_declaring_its_own() {
        let mut scopes = Scopes::new();
        scopes.insert_type_param("T".into(), 0);
        scopes.insert_trait_bound(Symbol::from_parts("package", "T"), Type::Error);
        scopes.push();
        scopes.insert_type_param("T".into(), 0);

        let bounds = scopes.collect_all_trait_bounds();

        assert!(bounds.is_empty());
    }
}
