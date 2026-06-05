use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;
use std::rc::Rc;

use crate::Bindings;
use crate::ReturnContext;
use crate::context::lowering::LoopContext;
use crate::escape_reserved;
use crate::state::bindings::{BindingValue, InlineExpr};

pub(crate) struct ScopeState {
    next_var: usize,
    bindings: Bindings,
    declared: Vec<HashSet<String>>,
    scope_depth: usize,
    loop_stack: Vec<LoopContext>,
    return_ctx_stack: Vec<Rc<ReturnContext>>,
    assign_targets: HashSet<String>,
    go_const_bindings: Vec<HashSet<String>>,
}

pub(crate) struct BindingSnapshot {
    inner: HashMap<String, BindingValue>,
}

pub(crate) struct IsolatedFunctionFrame {
    declared: Vec<HashSet<String>>,
    scope_depth: usize,
}

impl ScopeState {
    pub(crate) fn new() -> Self {
        Self {
            next_var: 0,
            bindings: Bindings::new(),
            declared: vec![HashSet::default()],
            scope_depth: 0,
            loop_stack: Vec::new(),
            return_ctx_stack: Vec::new(),
            assign_targets: HashSet::default(),
            go_const_bindings: vec![HashSet::default()],
        }
    }

    pub(crate) fn reset_for_top_level(&mut self) {
        self.next_var = 0;
        self.bindings.reset();
        self.declared.clear();
        self.declared.push(HashSet::default());
        self.go_const_bindings.truncate(1);
    }

    pub(crate) fn bind(
        &mut self,
        lisette_name: impl Into<String>,
        go_name: impl Into<String>,
    ) -> String {
        self.bindings.bind_go_name(lisette_name, go_name)
    }

    pub(crate) fn bind_inline_expr(&mut self, lisette_name: impl Into<String>, expr: InlineExpr) {
        self.bindings.bind_inline_expr(lisette_name, expr);
    }

    pub(crate) fn remove_binding(&mut self, lisette_name: &str) {
        self.bindings.remove(lisette_name);
    }

    pub(crate) fn resolve_identifier_binding(&self, lisette_name: &str) -> Option<&BindingValue> {
        self.bindings.get(lisette_name)
    }

    pub(crate) fn resolve_binding_go_name(&self, lisette_name: &str) -> Option<&str> {
        self.bindings.get_go_name(lisette_name)
    }

    /// Falls back to the keyword-escaped form when the name is unbound or
    /// inline-bound; callers needing a usable local must hoist a fresh temp.
    pub(crate) fn resolve_or_escape_go_name(&self, lisette_name: &str) -> String {
        self.bindings
            .get_go_name(lisette_name)
            .map(String::from)
            .unwrap_or_else(|| escape_reserved(lisette_name).into_owned())
    }

    pub(crate) fn has_binding_for_go_name(&self, go_name: &str) -> bool {
        self.bindings.has_go_name(go_name)
    }

    pub(crate) fn push_binding_frame(&mut self) {
        self.bindings.save();
    }

    pub(crate) fn pop_binding_frame(&mut self) {
        self.bindings.restore();
    }

    pub(crate) fn binding_snapshot(&self) -> BindingSnapshot {
        BindingSnapshot {
            inner: self.bindings.snapshot(),
        }
    }

    pub(crate) fn restore_binding_snapshot(&mut self, snapshot: BindingSnapshot) {
        self.bindings.restore_snapshot(snapshot.inner);
    }

    pub(crate) fn declare_go_name(&mut self, go_name: &str) {
        if let Some(current) = self.declared.last_mut() {
            current.insert(go_name.to_string());
        }
    }

    pub(crate) fn try_declare_go_name(&mut self, go_name: &str) -> bool {
        let Some(current) = self.declared.last_mut() else {
            return true;
        };
        if current.contains(go_name) {
            false
        } else {
            current.insert(go_name.to_string());
            true
        }
    }

    pub(crate) fn is_go_name_declared(&self, go_name: &str) -> bool {
        self.declared.iter().any(|s| s.contains(go_name))
    }

    pub(crate) fn current_block_declared_nonempty(&self) -> bool {
        self.declared.last().is_some_and(|s| !s.is_empty())
    }

    pub(crate) fn enter_block(&mut self) {
        self.scope_depth += 1;
        self.bindings.save();
        self.declared.push(HashSet::default());
        self.go_const_bindings.push(HashSet::default());
    }

    pub(crate) fn exit_block(&mut self) {
        self.scope_depth = self.scope_depth.saturating_sub(1);
        self.bindings.restore();
        pop_keep_base(&mut self.declared);
        pop_keep_base(&mut self.go_const_bindings);
    }

    pub(crate) fn enter_isolated_function(&mut self) -> IsolatedFunctionFrame {
        let saved = IsolatedFunctionFrame {
            declared: std::mem::take(&mut self.declared),
            scope_depth: self.scope_depth,
        };
        self.declared = vec![HashSet::default()];
        self.scope_depth = 0;
        self.bindings.save();
        saved
    }

    pub(crate) fn exit_isolated_function(&mut self, frame: IsolatedFunctionFrame) {
        self.bindings.restore();
        self.declared = frame.declared;
        self.scope_depth = frame.scope_depth;
    }

    pub(crate) fn fresh_go_name(&mut self, hint: Option<&str>) -> String {
        loop {
            self.next_var += 1;
            let name = match hint {
                Some(h) => format!("{}_{}", h, self.next_var),
                None => format!("tmp_{}", self.next_var),
            };
            if !self.bindings.has_go_name(&name) && !self.is_go_name_declared(&name) {
                return name;
            }
        }
    }

    pub(crate) fn push_loop(&mut self, ctx: LoopContext) {
        self.loop_stack.push(ctx);
    }

    pub(crate) fn pop_loop(&mut self) {
        self.loop_stack.pop();
    }

    pub(crate) fn push_return_ctx(&mut self, ctx: Rc<ReturnContext>) {
        self.return_ctx_stack.push(ctx);
    }

    pub(crate) fn pop_return_ctx(&mut self) {
        self.return_ctx_stack.pop();
    }

    pub(crate) fn current_return_ctx(&self) -> Option<Rc<ReturnContext>> {
        self.return_ctx_stack.last().cloned()
    }

    pub(crate) fn current_loop_result_var(&self) -> Option<&str> {
        self.loop_stack.last().map(|c| c.result_var.as_str())
    }

    pub(crate) fn current_loop_label(&self) -> Option<&str> {
        self.loop_stack.last().and_then(|c| c.label.as_deref())
    }

    pub(crate) fn set_current_loop_label(&mut self, label: String) {
        if let Some(ctx) = self.loop_stack.last_mut() {
            ctx.label = Some(label);
        }
    }

    pub(crate) fn try_acquire_assign_target(&mut self, var: &str) -> bool {
        self.assign_targets.insert(var.to_string())
    }

    pub(crate) fn release_assign_target(&mut self, var: &str) {
        self.assign_targets.remove(var);
    }

    pub(crate) fn is_active_assign_target(&self, var: &str) -> bool {
        self.assign_targets.contains(var)
    }

    pub(crate) fn push_const_frame(&mut self) {
        self.go_const_bindings.push(HashSet::default());
    }

    pub(crate) fn pop_const_frame(&mut self) {
        pop_keep_base(&mut self.go_const_bindings);
    }

    pub(crate) fn record_go_const_binding(&mut self, go_identifier: String) {
        if let Some(top) = self.go_const_bindings.last_mut() {
            top.insert(go_identifier);
        }
    }

    pub(crate) fn is_go_const_binding(&self, go_identifier: &str) -> bool {
        self.go_const_bindings
            .iter()
            .any(|frame| frame.contains(go_identifier))
    }
}

fn pop_keep_base<T>(stack: &mut Vec<T>) {
    if stack.len() > 1 {
        stack.pop();
    }
}
