use rustc_hash::FxHashSet as HashSet;

use crate::Bindings;
use crate::ReturnContext;
use crate::context::lowering::LoopContext;
use crate::plan::bodies::LoopId;
use crate::state::bindings::{BindingSnapshot, BindingValue, InlineExpr};

pub(crate) struct ScopeState {
    next_var: usize,
    next_loop_id: u32,
    bindings: Bindings,
    declared: Vec<HashSet<String>>,
    type_param_go_names: HashSet<String>,
    loop_stack: Vec<LoopContext>,
    return_ctx_stack: Vec<ReturnContext>,
    test_handle_stack: Vec<String>,
    assign_targets: HashSet<String>,
    go_const_bindings: Vec<HashSet<String>>,
    /// Go identifiers referenced during lowering, for structural liveness.
    use_frames: Vec<HashSet<String>>,
}

pub(crate) struct IsolatedFunctionFrame {
    declared: Vec<HashSet<String>>,
}

impl ScopeState {
    pub(crate) fn new() -> Self {
        Self {
            next_var: 0,
            next_loop_id: 0,
            bindings: Bindings::new(),
            declared: vec![HashSet::default()],
            type_param_go_names: HashSet::default(),
            loop_stack: Vec::new(),
            return_ctx_stack: Vec::new(),
            test_handle_stack: Vec::new(),
            assign_targets: HashSet::default(),
            go_const_bindings: vec![HashSet::default()],
            use_frames: Vec::new(),
        }
    }

    pub(crate) fn enter_use_region(&mut self) {
        self.use_frames.push(HashSet::default());
    }

    /// Pop and return the region's uses, merging them into the enclosing region.
    pub(crate) fn exit_use_region(&mut self) -> HashSet<String> {
        let frame = self
            .use_frames
            .pop()
            .expect("a use region must be entered before it is exited");
        if let Some(parent) = self.use_frames.last_mut() {
            parent.extend(frame.iter().cloned());
        }
        frame
    }

    pub(crate) fn record_go_use(&mut self, go_name: &str) {
        if let Some(frame) = self.use_frames.last_mut() {
            frame.insert(go_name.to_string());
        }
    }

    pub(crate) fn reset_for_top_level(&mut self) {
        self.next_var = 0;
        self.next_loop_id = 0;
        self.bindings.reset();
        self.declared.clear();
        self.declared.push(HashSet::default());
        self.type_param_go_names.clear();
        self.go_const_bindings.truncate(1);
    }

    pub(crate) fn declare_type_param(&mut self, go_name: &str) {
        self.type_param_go_names.insert(go_name.to_string());
        self.declare_go_name(go_name);
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
        self.bindings.snapshot()
    }

    pub(crate) fn replace_binding_snapshot(
        &mut self,
        snapshot: BindingSnapshot,
    ) -> BindingSnapshot {
        self.bindings.replace_snapshot(snapshot)
    }

    pub(crate) fn declare_go_name(&mut self, go_name: &str) {
        self.declared
            .last_mut()
            .expect("scope state always retains a declaration frame")
            .insert(go_name.to_string());
    }

    pub(crate) fn try_declare_go_name(&mut self, go_name: &str) -> bool {
        let current = self
            .declared
            .last_mut()
            .expect("scope state always retains a declaration frame");
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
        self.bindings.save();
        self.declared.push(HashSet::default());
        self.go_const_bindings.push(HashSet::default());
    }

    pub(crate) fn exit_block(&mut self) {
        self.bindings.restore();
        pop_keep_base(&mut self.declared);
        pop_keep_base(&mut self.go_const_bindings);
    }

    pub(crate) fn enter_isolated_function(&mut self) -> IsolatedFunctionFrame {
        let saved = IsolatedFunctionFrame {
            declared: std::mem::take(&mut self.declared),
        };
        self.declared = vec![self.type_param_go_names.clone()];
        self.bindings.save();
        saved
    }

    pub(crate) fn exit_isolated_function(&mut self, frame: IsolatedFunctionFrame) {
        self.bindings.restore();
        self.declared = frame.declared;
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

    pub(crate) fn fresh_go_name_checkpoint(&self) -> usize {
        self.next_var
    }

    pub(crate) fn restore_fresh_go_name_checkpoint(&mut self, checkpoint: usize) {
        self.next_var = checkpoint;
    }

    pub(crate) fn push_loop(&mut self, result_var: String) {
        let id = LoopId(self.next_loop_id);
        self.next_loop_id += 1;
        self.loop_stack.push(LoopContext { id, result_var });
    }

    pub(crate) fn pop_loop(&mut self) {
        self.loop_stack
            .pop()
            .expect("a loop context must be pushed before it is popped");
    }

    pub(crate) fn push_return_ctx(&mut self, ctx: ReturnContext) {
        self.return_ctx_stack.push(ctx);
    }

    pub(crate) fn pop_return_ctx(&mut self) {
        self.return_ctx_stack
            .pop()
            .expect("a return context must be pushed before it is popped");
    }

    pub(crate) fn push_test_handle(&mut self, name: String) {
        self.test_handle_stack.push(name);
    }

    pub(crate) fn pop_test_handle(&mut self) {
        self.test_handle_stack
            .pop()
            .expect("a test handle must be pushed before it is popped");
    }

    pub(crate) fn current_test_handle(&self) -> Option<&str> {
        self.test_handle_stack.last().map(String::as_str)
    }

    pub(crate) fn current_return_ctx(&self) -> Option<ReturnContext> {
        self.return_ctx_stack.last().cloned()
    }

    pub(crate) fn current_loop_result_var(&self) -> Option<&str> {
        self.loop_stack.last().map(|c| c.result_var.as_str())
    }

    pub(crate) fn current_loop_id(&self) -> Option<LoopId> {
        self.loop_stack.last().map(|context| context.id)
    }

    pub(crate) fn activate_assign_target(&mut self, var: &str) -> bool {
        self.assign_targets.insert(var.to_string())
    }

    pub(crate) fn deactivate_assign_target(&mut self, var: &str) {
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
        self.go_const_bindings
            .last_mut()
            .expect("scope state always retains a const frame")
            .insert(go_identifier);
    }

    pub(crate) fn is_go_const_binding(&self, go_identifier: &str) -> bool {
        self.go_const_bindings
            .iter()
            .any(|frame| frame.contains(go_identifier))
    }
}

fn pop_keep_base<T>(stack: &mut Vec<T>) {
    assert!(stack.len() > 1, "cannot pop a stack's base frame");
    let _ = stack.pop();
}
