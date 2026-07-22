use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::Cell;
use syntax::ast::BindingId;
use syntax::ast::Span;
use syntax::types::{Symbol, Type};

#[derive(Debug, Clone, Default)]
pub struct DepthCounter(Cell<usize>);

impl DepthCounter {
    pub(crate) fn new() -> Self {
        Self(Cell::new(0))
    }
    fn with_value(n: usize) -> Self {
        Self(Cell::new(n))
    }
    fn get(&self) -> usize {
        self.0.get()
    }
    pub(crate) fn increment(&self) {
        self.0.set(self.0.get() + 1);
    }
    pub(crate) fn decrement(&self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
    pub(crate) fn is_active(&self) -> bool {
        self.0.get() > 0
    }
    fn reset(&self) -> usize {
        let prev = self.0.get();
        self.0.set(0);
        prev
    }
    fn restore(&self, depth: usize) {
        self.0.set(depth);
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
pub enum CarrierKind {
    Result,
    Option,
}

#[derive(Debug, Clone)]
pub struct TryBlockContext {
    pub(crate) ok_ty: Type,
    pub(crate) err_ty: Type,
    pub(crate) carrier: Cell<Option<CarrierKind>>,
    pub(crate) has_question_mark: Cell<bool>,
    pub(crate) loop_depth: DepthCounter,
}

#[derive(Debug, Clone)]
pub struct RecoverBlockContext {
    pub(crate) loop_depth: DepthCounter,
}

#[derive(Debug, Clone)]
pub struct Scope {
    /// variable name -> type
    pub(crate) values: HashMap<String, Type>,
    pub(crate) mutables: Option<HashSet<String>>,
    pub(crate) consts: Option<HashSet<String>>,
    pub(crate) type_params: Option<HashMap<String, usize>>,
    pub(crate) trait_bounds: Option<HashMap<Symbol, Vec<Type>>>,
    pub(crate) fn_return_type: Option<Type>,
    deferred_map_key_checks: Vec<(Type, Span, bool)>,
    pub(crate) try_block_context: Option<TryBlockContext>,
    pub(crate) recover_block_context: Option<RecoverBlockContext>,
    loop_break_type: Option<Type>,
    loop_depth: DepthCounter,
    defer_block_depth: DepthCounter,
    negation_depth: DepthCounter,
    type_param_depth: DepthCounter,
    use_context: Cell<UseContext>,
    in_test_handle: bool,
    test_fn_name: Option<EcoString>,
    /// variable name -> binding ID (for linting)
    pub(crate) name_to_binding: HashMap<String, BindingId>,
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
            mutables: None,
            consts: None,
            type_params: None,
            trait_bounds: None,
            fn_return_type: None,
            deferred_map_key_checks: Vec::new(),
            try_block_context: None,
            recover_block_context: None,
            loop_break_type: None,
            loop_depth: DepthCounter::new(),
            defer_block_depth: DepthCounter::new(),
            negation_depth: DepthCounter::new(),
            type_param_depth: DepthCounter::new(),
            use_context: Cell::new(UseContext::Statement),
            in_test_handle: false,
            test_fn_name: None,
            name_to_binding: HashMap::default(),
        }
    }
}

pub struct Scopes {
    stack: Vec<Scope>,
    /// True when inferring inside a compound expression (call arg, binary
    /// operand, etc.). Used to reject `Err(x)?`/`None?` and similar control-flow
    /// in positions where they can never produce a value.
    in_subexpression: Cell<bool>,
    /// True when inferring the base of a dot-access chain. Suppresses the
    /// record-struct-as-value error when the struct name is a type qualifier
    /// (e.g. `lib.Point` in `lib.Point.sum`).
    dot_access_base: Cell<bool>,
    /// True while inferring a `let` binding's right-hand side. Suppresses the generic
    /// "used as a value" rejection there so `bindings.rs` can raise the specific
    /// "cannot bind a type or module to a variable" error instead.
    let_binding_rhs: Cell<bool>,
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
            in_subexpression: Cell::new(false),
            dot_access_base: Cell::new(false),
            let_binding_rhs: Cell::new(false),
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
        let current = self.current();
        let mut scope = Scope::new();
        scope.loop_break_type = current.loop_break_type.clone();
        scope.loop_depth = DepthCounter::with_value(current.loop_depth.get());
        scope.defer_block_depth = DepthCounter::with_value(current.defer_block_depth.get());
        scope.negation_depth = DepthCounter::with_value(current.negation_depth.get());
        scope.type_param_depth = DepthCounter::with_value(current.type_param_depth.get());
        scope.use_context = Cell::new(current.use_context.get());
        scope.in_test_handle = current.in_test_handle;
        scope.test_fn_name = current.test_fn_name.clone();
        self.stack.push(scope);
    }

    pub(crate) fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// Look up a value by walking the scope stack from top to bottom.
    pub(crate) fn lookup_value(&self, name: &str) -> Option<&Type> {
        for scope in self.stack.iter().rev() {
            if let Some(ty) = scope.values.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Check if a variable is marked mutable in any enclosing scope.
    pub(crate) fn lookup_mutable(&self, name: &str) -> bool {
        self.stack
            .iter()
            .rev()
            .any(|s| s.mutables.as_ref().is_some_and(|m| m.contains(name)))
    }

    /// Whether `name` is a block-local `const` in any enclosing scope.
    pub(crate) fn lookup_const(&self, name: &str) -> bool {
        self.stack
            .iter()
            .rev()
            .any(|s| s.consts.as_ref().is_some_and(|c| c.contains(name)))
    }

    /// Look up a binding ID by walking the scope stack from top to bottom.
    pub(crate) fn lookup_binding_id(&self, name: &str) -> Option<BindingId> {
        for scope in self.stack.iter().rev() {
            if let Some(id) = scope.name_to_binding.get(name) {
                return Some(*id);
            }
        }
        None
    }

    /// Whether resolving `name` crosses a function scope, meaning captured.
    pub(crate) fn binding_crosses_function_boundary(&self, name: &str) -> bool {
        let mut crossed = false;
        for scope in self.stack.iter().rev() {
            if scope.name_to_binding.contains_key(name) {
                return crossed;
            }
            if scope.fn_return_type.is_some() {
                crossed = true;
            }
        }
        false
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
        self.current_mut().in_test_handle = true;
    }

    pub(crate) fn has_test_handle(&self) -> bool {
        self.current().in_test_handle
    }

    pub(crate) fn set_test_fn_name(&mut self, name: EcoString) {
        self.current_mut().test_fn_name = Some(name);
    }

    pub(crate) fn test_fn_name(&self) -> Option<&str> {
        self.current().test_fn_name.as_deref()
    }

    pub(crate) fn increment_loop_depth(&self) {
        self.current().loop_depth.increment();
    }

    pub(crate) fn decrement_loop_depth(&self) {
        self.current().loop_depth.decrement();
    }

    pub(crate) fn is_inside_loop(&self) -> bool {
        self.current().loop_depth.is_active()
    }

    pub(crate) fn set_loop_break_type(&mut self, ty: Type) {
        self.current_mut().loop_break_type = Some(ty);
    }

    pub(crate) fn clear_loop_break_type(&mut self) {
        self.current_mut().loop_break_type = None;
    }

    pub(crate) fn loop_break_type(&self) -> Option<&Type> {
        self.current().loop_break_type.as_ref()
    }

    pub(crate) fn increment_defer_block_depth(&self) {
        self.current().defer_block_depth.increment();
    }

    pub(crate) fn decrement_defer_block_depth(&self) {
        self.current().defer_block_depth.decrement();
    }

    pub(crate) fn is_inside_defer_block(&self) -> bool {
        self.current().defer_block_depth.is_active()
    }

    pub(crate) fn defer_block_loop_depth(&self) -> usize {
        self.current().loop_depth.get()
    }

    pub(crate) fn increment_negation_depth(&self) {
        self.current().negation_depth.increment();
    }

    pub(crate) fn decrement_negation_depth(&self) {
        self.current().negation_depth.decrement();
    }

    pub(crate) fn is_inside_negation(&self) -> bool {
        self.current().negation_depth.is_active()
    }

    pub(crate) fn reset_loop_depth(&self) -> usize {
        self.current().loop_depth.reset()
    }

    pub(crate) fn restore_loop_depth(&self, depth: usize) {
        self.current().loop_depth.restore(depth);
    }

    pub(crate) fn set_value_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Value);
        prev
    }

    pub(crate) fn set_statement_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Statement);
        prev
    }

    pub(crate) fn restore_use_context(&self, ctx: UseContext) {
        self.current().use_context.set(ctx);
    }

    pub(crate) fn is_value_context(&self) -> bool {
        self.current().use_context.get() == UseContext::Value
    }

    pub(crate) fn set_callee_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Callee);
        prev
    }

    pub(crate) fn is_callee_context(&self) -> bool {
        self.current().use_context.get() == UseContext::Callee
    }

    pub(crate) fn set_assignment_target_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::AssignmentTarget);
        prev
    }

    pub(crate) fn is_assignment_target_context(&self) -> bool {
        self.current().use_context.get() == UseContext::AssignmentTarget
    }

    pub(crate) fn set_in_subexpression(&self, value: bool) -> bool {
        self.in_subexpression.replace(value)
    }

    pub(crate) fn is_dot_access_base(&self) -> bool {
        self.dot_access_base.get()
    }

    pub(crate) fn set_dot_access_base(&self, value: bool) -> bool {
        self.dot_access_base.replace(value)
    }

    pub(crate) fn is_let_binding_rhs(&self) -> bool {
        self.let_binding_rhs.get()
    }

    pub(crate) fn set_let_binding_rhs(&self, value: bool) -> bool {
        self.let_binding_rhs.replace(value)
    }

    pub(crate) fn increment_type_param_depth(&self) {
        self.current().type_param_depth.increment();
    }

    pub(crate) fn decrement_type_param_depth(&self) {
        self.current().type_param_depth.decrement();
    }

    pub(crate) fn is_inside_type_param(&self) -> bool {
        self.current().type_param_depth.is_active()
    }

    pub(crate) fn set_impl_receiver_type(&mut self, ty: Option<Type>) {
        self.impl_receiver_type = ty;
    }

    pub(crate) fn impl_receiver_type(&self) -> Option<&Type> {
        self.impl_receiver_type.as_ref()
    }
}
