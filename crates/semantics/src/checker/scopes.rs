use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::BindingId;
use syntax::ast::Span;
use syntax::types::{Symbol, Type};

#[derive(Debug, Clone, Default)]
pub struct DepthCounter(usize);

impl DepthCounter {
    pub(crate) fn new() -> Self {
        Self(0)
    }
    fn get(&self) -> usize {
        self.0
    }
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
    fn reset(&mut self) -> usize {
        let prev = self.0;
        self.0 = 0;
        prev
    }
    fn restore(&mut self, depth: usize) {
        self.0 = depth;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TryCarrier {
    #[default]
    Unset,
    Unknown,
    Result,
    Option,
}

impl TryCarrier {
    /// Record one `?` operand and report whether it conflicts with a carrier
    /// already established by an earlier operand.
    pub(crate) fn observe(&mut self, observed: Option<Self>) -> bool {
        match (*self, observed) {
            (Self::Unset, None) => *self = Self::Unknown,
            (Self::Unset | Self::Unknown, Some(carrier)) => *self = carrier,
            (Self::Result, Some(Self::Option)) | (Self::Option, Some(Self::Result)) => return true,
            _ => {}
        }
        false
    }

    pub(crate) fn was_used(self) -> bool {
        self != Self::Unset
    }
}

#[derive(Debug, Clone)]
pub struct TryBlockContext {
    pub(crate) ok_ty: Type,
    pub(crate) err_ty: Type,
    pub(crate) carrier: TryCarrier,
    pub(crate) loop_depth: DepthCounter,
}

#[derive(Debug, Clone)]
pub struct RecoverBlockContext {
    pub(crate) loop_depth: DepthCounter,
}

/// Traversal state inherited by nested lexical scopes and restored on pop.
#[derive(Debug, Clone, Default)]
struct InheritedContext {
    loop_break_type: Option<Type>,
    loop_depth: DepthCounter,
    defer_block_depth: DepthCounter,
    negation_depth: DepthCounter,
    invariant_depth: DepthCounter,
    use_context: UseContext,
    in_test_handle: bool,
    test_fn_name: Option<EcoString>,
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

#[derive(Debug, Clone)]
pub struct Scope {
    values: HashMap<String, ScopedValue>,
    pub(crate) type_params: Option<HashMap<String, usize>>,
    pub(crate) trait_bounds: Option<HashMap<Symbol, Vec<Type>>>,
    pub(crate) fn_return_type: Option<Type>,
    deferred_map_key_checks: Vec<(Type, Span, bool)>,
    pub(crate) try_block_context: Option<TryBlockContext>,
    pub(crate) recover_block_context: Option<RecoverBlockContext>,
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
            type_params: None,
            trait_bounds: None,
            fn_return_type: None,
            deferred_map_key_checks: Vec::new(),
            try_block_context: None,
            recover_block_context: None,
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
}

pub struct Scopes {
    stack: Vec<Scope>,
    /// True when inferring the base of a dot-access chain. Suppresses the
    /// record-struct-as-value error when the struct name is a type qualifier
    /// (e.g. `lib.Point` in `lib.Point.sum`).
    dot_access_base: bool,
    /// True while inferring a `let` binding's right-hand side. Suppresses the generic
    /// "used as a value" rejection there so `bindings.rs` can raise the specific
    /// "cannot bind a type or module to a variable" error instead.
    let_binding_rhs: bool,
    /// The enclosing impl block's receiver type, used to resolve `self`
    /// parameter annotations inside the impl's methods. `None` outside impls.
    /// Singleton because Lisette does not allow nested impl blocks.
    impl_receiver_type: Option<Type>,
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
            dot_access_base: false,
            let_binding_rhs: false,
            impl_receiver_type: None,
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
        for scope in self.stack.iter().rev() {
            if let Some(idx) = scope.type_params.as_ref().and_then(|tp| tp.get(name)) {
                return Some(*idx);
            }
        }
        None
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

    pub(crate) fn defer_map_key_check(&mut self, key: Type, span: Span, check_concrete: bool) {
        if let Some(scope) = self
            .stack
            .iter_mut()
            .rev()
            .find(|scope| scope.fn_return_type.is_some())
        {
            scope
                .deferred_map_key_checks
                .push((key, span, check_concrete));
        }
    }

    pub(crate) fn take_deferred_map_key_checks(&mut self) -> Vec<(Type, Span, bool)> {
        std::mem::take(&mut self.current_mut().deferred_map_key_checks)
    }

    /// Look up the enclosing try block context, stopping at function boundaries.
    pub(crate) fn lookup_try_block_context(&self) -> Option<&TryBlockContext> {
        for scope in self.stack.iter().rev() {
            if scope.try_block_context.is_some() {
                return scope.try_block_context.as_ref();
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
    }

    pub(crate) fn lookup_try_block_context_mut(&mut self) -> Option<&mut TryBlockContext> {
        for scope in self.stack.iter_mut().rev() {
            if scope.try_block_context.is_some() {
                return scope.try_block_context.as_mut();
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
            if scope.recover_block_context.is_some() {
                return scope.recover_block_context.as_ref();
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
    }

    pub(crate) fn lookup_recover_block_context_mut(&mut self) -> Option<&mut RecoverBlockContext> {
        for scope in self.stack.iter_mut().rev() {
            if scope.recover_block_context.is_some() {
                return scope.recover_block_context.as_mut();
            }
            if scope.fn_return_type.is_some() {
                return None;
            }
        }
        None
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
            if let Some(type_params) = &scope.type_params {
                all_bounds
                    .retain(|parameter, _| !type_params.contains_key(parameter.last_segment()));
            }
            if let Some(ref bounds) = scope.trait_bounds {
                for (key, value) in bounds {
                    all_bounds.insert(key.clone(), value.clone());
                }
            }
        }
        all_bounds
    }

    pub(crate) fn for_each_bound_on_param<F: FnMut(&Type)>(&self, param_name: &str, mut visit: F) {
        for scope in self.stack.iter().rev() {
            let introduces = scope
                .type_params
                .as_ref()
                .is_some_and(|tp| tp.contains_key(param_name));
            if !introduces {
                continue;
            }
            if let Some(ref bounds) = scope.trait_bounds {
                for (key, types) in bounds {
                    if key.last_segment() == param_name {
                        for ty in types {
                            visit(ty);
                        }
                    }
                }
            }
            return;
        }
    }

    pub(crate) fn mark_test_handle(&mut self) {
        self.current_mut().inherited.in_test_handle = true;
    }

    pub(crate) fn has_test_handle(&self) -> bool {
        self.current().inherited.in_test_handle
    }

    pub(crate) fn set_test_fn_name(&mut self, name: EcoString) {
        self.current_mut().inherited.test_fn_name = Some(name);
    }

    pub(crate) fn test_fn_name(&self) -> Option<&str> {
        self.current().inherited.test_fn_name.as_deref()
    }

    pub(crate) fn increment_loop_depth(&mut self) {
        self.current_mut().inherited.loop_depth.increment();
    }

    pub(crate) fn decrement_loop_depth(&mut self) {
        self.current_mut().inherited.loop_depth.decrement();
    }

    pub(crate) fn is_inside_loop(&self) -> bool {
        self.current().inherited.loop_depth.is_active()
    }

    pub(crate) fn set_loop_break_type(&mut self, ty: Type) {
        self.current_mut().inherited.loop_break_type = Some(ty);
    }

    pub(crate) fn clear_loop_break_type(&mut self) {
        self.current_mut().inherited.loop_break_type = None;
    }

    pub(crate) fn loop_break_type(&self) -> Option<&Type> {
        self.current().inherited.loop_break_type.as_ref()
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

    pub(crate) fn defer_block_loop_depth(&self) -> usize {
        self.current().inherited.loop_depth.get()
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

    pub(crate) fn reset_loop_depth(&mut self) -> usize {
        self.current_mut().inherited.loop_depth.reset()
    }

    pub(crate) fn restore_loop_depth(&mut self, depth: usize) {
        self.current_mut().inherited.loop_depth.restore(depth);
    }

    pub(crate) fn set_value_context(&mut self) -> UseContext {
        let prev = self.current().inherited.use_context;
        self.current_mut().inherited.use_context = UseContext::Value;
        prev
    }

    pub(crate) fn set_statement_context(&mut self) -> UseContext {
        let prev = self.current().inherited.use_context;
        self.current_mut().inherited.use_context = UseContext::Statement;
        prev
    }

    pub(crate) fn restore_use_context(&mut self, ctx: UseContext) {
        self.current_mut().inherited.use_context = ctx;
    }

    pub(crate) fn is_value_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::Value
    }

    pub(crate) fn set_callee_context(&mut self) -> UseContext {
        let prev = self.current().inherited.use_context;
        self.current_mut().inherited.use_context = UseContext::Callee;
        prev
    }

    pub(crate) fn is_callee_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::Callee
    }

    pub(crate) fn set_assignment_target_context(&mut self) -> UseContext {
        let prev = self.current().inherited.use_context;
        self.current_mut().inherited.use_context = UseContext::AssignmentTarget;
        prev
    }

    pub(crate) fn is_assignment_target_context(&self) -> bool {
        self.current().inherited.use_context == UseContext::AssignmentTarget
    }

    pub(crate) fn is_dot_access_base(&self) -> bool {
        self.dot_access_base
    }

    pub(crate) fn set_dot_access_base(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.dot_access_base, value)
    }

    pub(crate) fn is_let_binding_rhs(&self) -> bool {
        self.let_binding_rhs
    }

    pub(crate) fn set_let_binding_rhs(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.let_binding_rhs, value)
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

    pub(crate) fn set_impl_receiver_type(&mut self, ty: Option<Type>) {
        self.impl_receiver_type = ty;
    }

    pub(crate) fn impl_receiver_type(&self) -> Option<&Type> {
        self.impl_receiver_type.as_ref()
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
}
