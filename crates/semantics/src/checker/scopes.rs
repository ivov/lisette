use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::Cell;
use syntax::ast::BindingId;
use syntax::ast::Span;
use syntax::types::{Symbol, Type};

#[derive(Debug, Clone, Default)]
pub struct DepthCounter(Cell<usize>);

impl DepthCounter {
    pub fn new() -> Self {
        Self(Cell::new(0))
    }
    pub fn with_value(n: usize) -> Self {
        Self(Cell::new(n))
    }
    pub fn get(&self) -> usize {
        self.0.get()
    }
    pub fn increment(&self) {
        self.0.set(self.0.get() + 1);
    }
    pub fn decrement(&self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
    pub fn is_active(&self) -> bool {
        self.0.get() > 0
    }
    pub fn reset(&self) -> usize {
        let prev = self.0.get();
        self.0.set(0);
        prev
    }
    pub fn restore(&self, depth: usize) {
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
    pub ok_ty: Type,
    pub err_ty: Type,
    pub carrier: Cell<Option<CarrierKind>>,
    pub has_question_mark: Cell<bool>,
    pub try_span: Span,
    pub loop_depth: DepthCounter,
}

#[derive(Debug, Clone)]
pub struct RecoverBlockContext {
    pub inner_ty: Type,
    pub recover_span: Span,
    pub loop_depth: DepthCounter,
}

#[derive(Debug, Clone)]
pub struct Scope {
    /// variable name -> type
    pub values: HashMap<String, Type>,
    pub mutables: Option<HashSet<String>>,
    pub type_params: Option<HashMap<String, usize>>,
    pub trait_bounds: Option<HashMap<Symbol, Vec<Type>>>,
    pub fn_return_type: Option<Type>,
    pub try_block_context: Option<TryBlockContext>,
    pub recover_block_context: Option<RecoverBlockContext>,
    pub loop_break_type: Option<Type>,
    pub loop_depth: DepthCounter,
    pub defer_block_depth: DepthCounter,
    pub negation_depth: DepthCounter,
    pub type_param_depth: DepthCounter,
    pub use_context: Cell<UseContext>,
    /// variable name -> binding ID (for linting)
    pub name_to_binding: HashMap<String, BindingId>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            values: HashMap::default(),
            mutables: None,
            type_params: None,
            trait_bounds: None,
            fn_return_type: None,
            try_block_context: None,
            recover_block_context: None,
            loop_break_type: None,
            loop_depth: DepthCounter::new(),
            defer_block_depth: DepthCounter::new(),
            negation_depth: DepthCounter::new(),
            type_param_depth: DepthCounter::new(),
            use_context: Cell::new(UseContext::Statement),
            name_to_binding: HashMap::default(),
        }
    }
}

pub struct Scopes {
    stack: Vec<Scope>,
    /// True when inferring the body of a match/select arm. Consumed by
    /// `infer_break`/`infer_continue` to decide whether the enclosing loop
    /// needs a Go label (since Go switch cases do not fall through).
    in_match_arm: Cell<bool>,
    /// One entry per enclosing loop; set to `true` when a break/continue is
    /// encountered inside a match arm. The top is popped by the loop's
    /// inference function and recorded on the Loop/While/For/WhileLet AST node.
    loop_needs_label_stack: std::cell::RefCell<Vec<bool>>,
    /// True when inferring inside a compound expression (call arg, binary
    /// operand, etc.). Used to reject `Err(x)?`/`None?` and similar control-flow
    /// in positions where they can never produce a value.
    in_subexpression: Cell<bool>,
    /// True when inferring the base of a dot-access chain. Suppresses the
    /// record-struct-as-value error when the struct name is a type qualifier
    /// (e.g. `lib.Point` in `lib.Point.sum`).
    dot_access_base: Cell<bool>,
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
    pub fn new() -> Self {
        Scopes {
            stack: vec![Scope::new()],
            in_match_arm: Cell::new(false),
            loop_needs_label_stack: std::cell::RefCell::new(Vec::new()),
            in_subexpression: Cell::new(false),
            dot_access_base: Cell::new(false),
            impl_receiver_type: None,
        }
    }

    pub fn current(&self) -> &Scope {
        self.stack.last().expect("scope stack must not be empty")
    }

    pub fn current_mut(&mut self) -> &mut Scope {
        self.stack
            .last_mut()
            .expect("scope stack must not be empty")
    }

    pub fn push(&mut self) {
        let current = self.current();
        let loop_depth = current.loop_depth.get();
        let defer_block_depth = current.defer_block_depth.get();
        let negation_depth = current.negation_depth.get();
        let type_param_depth = current.type_param_depth.get();
        let use_context = current.use_context.get();
        let loop_break_type = current.loop_break_type.clone();
        self.stack.push(Scope {
            values: HashMap::default(),
            mutables: None,
            type_params: None,
            trait_bounds: None,
            fn_return_type: None,
            try_block_context: None,
            recover_block_context: None,
            loop_break_type,
            loop_depth: DepthCounter::with_value(loop_depth),
            defer_block_depth: DepthCounter::with_value(defer_block_depth),
            negation_depth: DepthCounter::with_value(negation_depth),
            type_param_depth: DepthCounter::with_value(type_param_depth),
            use_context: Cell::new(use_context),
            name_to_binding: HashMap::default(),
        });
    }

    pub fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    pub fn reset(&mut self) {
        self.stack.clear();
        self.stack.push(Scope::new());
        self.in_match_arm.set(false);
        self.loop_needs_label_stack.borrow_mut().clear();
        self.in_subexpression.set(false);
        self.dot_access_base.set(false);
        self.impl_receiver_type = None;
    }

    /// Look up a value by walking the scope stack from top to bottom.
    pub fn lookup_value(&self, name: &str) -> Option<&Type> {
        for scope in self.stack.iter().rev() {
            if let Some(ty) = scope.values.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Check if a variable is marked mutable in any enclosing scope.
    pub fn lookup_mutable(&self, name: &str) -> bool {
        self.stack
            .iter()
            .rev()
            .any(|s| s.mutables.as_ref().is_some_and(|m| m.contains(name)))
    }

    /// Look up a binding ID by walking the scope stack from top to bottom.
    pub fn lookup_binding_id(&self, name: &str) -> Option<BindingId> {
        for scope in self.stack.iter().rev() {
            if let Some(id) = scope.name_to_binding.get(name) {
                return Some(*id);
            }
        }
        None
    }

    /// Look up a type parameter by walking the scope stack from top to bottom.
    pub fn lookup_type_param(&self, name: &str) -> Option<usize> {
        for scope in self.stack.iter().rev() {
            if let Some(idx) = scope.type_params.as_ref().and_then(|tp| tp.get(name)) {
                return Some(*idx);
            }
        }
        None
    }

    /// Look up the enclosing function's return type.
    pub fn lookup_fn_return_type(&self) -> Option<&Type> {
        for scope in self.stack.iter().rev() {
            if let Some(ref ty) = scope.fn_return_type {
                return Some(ty);
            }
        }
        None
    }

    /// Look up the enclosing try block context, stopping at function boundaries.
    pub fn lookup_try_block_context(&self) -> Option<&TryBlockContext> {
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
    pub fn lookup_recover_block_context(&self) -> Option<&RecoverBlockContext> {
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

    pub fn collect_all_value_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in &self.stack {
            names.extend(scope.values.keys().cloned());
        }
        names
    }

    pub fn collect_all_trait_bounds(&self) -> HashMap<Symbol, Vec<Type>> {
        let mut all_bounds = HashMap::default();
        // Walk from bottom to top so inner scopes override outer
        for scope in &self.stack {
            if let Some(ref bounds) = scope.trait_bounds {
                for (key, value) in bounds {
                    all_bounds.insert(key.clone(), value.clone());
                }
            }
        }
        all_bounds
    }

    pub fn for_each_bound_on_param<F: FnMut(&Type)>(&self, param_name: &str, mut visit: F) {
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

    pub fn increment_loop_depth(&self) {
        self.current().loop_depth.increment();
    }

    pub fn decrement_loop_depth(&self) {
        self.current().loop_depth.decrement();
    }

    pub fn is_inside_loop(&self) -> bool {
        self.current().loop_depth.is_active()
    }

    pub fn set_loop_break_type(&mut self, ty: Type) {
        self.current_mut().loop_break_type = Some(ty);
    }

    pub fn clear_loop_break_type(&mut self) {
        self.current_mut().loop_break_type = None;
    }

    pub fn loop_break_type(&self) -> Option<&Type> {
        self.current().loop_break_type.as_ref()
    }

    pub fn increment_defer_block_depth(&self) {
        self.current().defer_block_depth.increment();
    }

    pub fn decrement_defer_block_depth(&self) {
        self.current().defer_block_depth.decrement();
    }

    pub fn is_inside_defer_block(&self) -> bool {
        self.current().defer_block_depth.is_active()
    }

    pub fn defer_block_loop_depth(&self) -> usize {
        self.current().loop_depth.get()
    }

    pub fn increment_negation_depth(&self) {
        self.current().negation_depth.increment();
    }

    pub fn decrement_negation_depth(&self) {
        self.current().negation_depth.decrement();
    }

    pub fn is_inside_negation(&self) -> bool {
        self.current().negation_depth.is_active()
    }

    pub fn reset_loop_depth(&self) -> usize {
        self.current().loop_depth.reset()
    }

    pub fn restore_loop_depth(&self, depth: usize) {
        self.current().loop_depth.restore(depth);
    }

    pub fn set_value_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Value);
        prev
    }

    pub fn set_statement_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Statement);
        prev
    }

    pub fn restore_use_context(&self, ctx: UseContext) {
        self.current().use_context.set(ctx);
    }

    pub fn is_value_context(&self) -> bool {
        self.current().use_context.get() == UseContext::Value
    }

    pub fn set_callee_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::Callee);
        prev
    }

    pub fn is_callee_context(&self) -> bool {
        self.current().use_context.get() == UseContext::Callee
    }

    pub fn set_assignment_target_context(&self) -> UseContext {
        let prev = self.current().use_context.get();
        self.current().use_context.set(UseContext::AssignmentTarget);
        prev
    }

    pub fn is_assignment_target_context(&self) -> bool {
        self.current().use_context.get() == UseContext::AssignmentTarget
    }

    pub fn is_in_match_arm(&self) -> bool {
        self.in_match_arm.get()
    }

    pub fn set_in_match_arm(&self, value: bool) -> bool {
        self.in_match_arm.replace(value)
    }

    pub fn push_loop_needs_label(&self) {
        self.loop_needs_label_stack.borrow_mut().push(false);
    }

    pub fn pop_loop_needs_label(&self) -> bool {
        self.loop_needs_label_stack
            .borrow_mut()
            .pop()
            .expect("loop_needs_label_stack must not be empty when popping")
    }

    pub fn mark_current_loop_needs_label(&self) {
        if let Some(flag) = self.loop_needs_label_stack.borrow_mut().last_mut() {
            *flag = true;
        }
    }

    pub fn is_in_subexpression(&self) -> bool {
        self.in_subexpression.get()
    }

    pub fn set_in_subexpression(&self, value: bool) -> bool {
        self.in_subexpression.replace(value)
    }

    pub fn is_dot_access_base(&self) -> bool {
        self.dot_access_base.get()
    }

    pub fn set_dot_access_base(&self, value: bool) -> bool {
        self.dot_access_base.replace(value)
    }

    pub fn increment_type_param_depth(&self) {
        self.current().type_param_depth.increment();
    }

    pub fn decrement_type_param_depth(&self) {
        self.current().type_param_depth.decrement();
    }

    pub fn is_inside_type_param(&self) -> bool {
        self.current().type_param_depth.is_active()
    }

    pub fn set_impl_receiver_type(&mut self, ty: Option<Type>) {
        self.impl_receiver_type = ty;
    }

    pub fn impl_receiver_type(&self) -> Option<&Type> {
        self.impl_receiver_type.as_ref()
    }
}
