use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::ReturnContext;
use crate::context::lowering::LoopContext;
use crate::plan::bodies::LoopId;
use crate::state::bindings::{BindingSnapshot, BindingValue, InlineExpr};

pub(crate) struct ScopeState {
    next_var: usize,
    next_loop_id: u32,
    frames: Vec<ScopeFrame>,
    loop_stack: Vec<LoopContext>,
    return_ctx_stack: Vec<ReturnContext>,
    test_handle_stack: Vec<String>,
    assign_targets: HashSet<String>,
    /// Go identifiers referenced during lowering, for structural liveness.
    use_frames: Vec<HashSet<String>>,
}

struct ScopeFrame {
    bindings: HashMap<String, BindingValue>,
    declarations: DeclarationScope,
    /// Conditions the branch being lowered has already tested in this scope.
    established: Vec<String>,
}

enum DeclarationScope {
    /// A semantic binding scope that emits no Go braces. Declarations belong
    /// to the nearest enclosing Go scope.
    Transparent,
    Block(Declarations),
    /// A nested function cannot see declarations from its enclosing function.
    Function(Declarations),
}

type Declarations = HashMap<String, DeclarationKind>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    Local,
    TypeParameter,
}

impl ScopeState {
    pub(crate) fn new() -> Self {
        Self {
            next_var: 0,
            next_loop_id: 0,
            frames: vec![ScopeFrame {
                bindings: HashMap::default(),
                declarations: DeclarationScope::Block(HashMap::default()),
                established: Vec::new(),
            }],
            loop_stack: Vec::new(),
            return_ctx_stack: vec![ReturnContext::None],
            test_handle_stack: Vec::new(),
            assign_targets: HashSet::default(),
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
        *self = Self::new();
    }

    pub(crate) fn declare_type_param(&mut self, go_name: &str) {
        self.current_declarations_mut()
            .insert(go_name.to_string(), DeclarationKind::TypeParameter);
    }

    pub(crate) fn bind(
        &mut self,
        lisette_name: impl Into<String>,
        go_name: impl Into<String>,
    ) -> String {
        let go_name = crate::escape_reserved(&go_name.into()).into_owned();
        self.current_bindings_mut()
            .insert(lisette_name.into(), BindingValue::GoName(go_name.clone()));
        go_name
    }

    pub(crate) fn bind_inline_expr(&mut self, lisette_name: impl Into<String>, expr: InlineExpr) {
        self.current_bindings_mut()
            .insert(lisette_name.into(), BindingValue::InlineExpr(expr));
    }

    pub(crate) fn bind_value(&mut self, lisette_name: impl Into<String>, value: BindingValue) {
        self.current_bindings_mut()
            .insert(lisette_name.into(), value);
    }

    pub(crate) fn mark_go_const(&mut self, lisette_name: &str) {
        let Some(value) = self.current_bindings_mut().get_mut(lisette_name) else {
            return;
        };
        if let BindingValue::GoName(name) = value {
            let name = std::mem::take(name);
            *value = BindingValue::GoConst(name);
        }
    }

    pub(crate) fn remove_binding(&mut self, lisette_name: &str) {
        self.current_bindings_mut().remove(lisette_name);
    }

    pub(crate) fn resolve_identifier_binding(&self, lisette_name: &str) -> Option<&BindingValue> {
        self.current_bindings().get(lisette_name)
    }

    pub(crate) fn resolve_binding_go_name(&self, lisette_name: &str) -> Option<&str> {
        self.current_bindings()
            .get(lisette_name)
            .and_then(BindingValue::as_go_name)
    }

    /// Falls back to the keyword-escaped form when the name is unbound or
    /// inline-bound; callers needing a usable local must hoist a fresh temp.
    pub(crate) fn has_binding_for_go_name(&self, go_name: &str) -> bool {
        self.current_bindings()
            .values()
            .filter_map(BindingValue::as_go_name)
            .any(|candidate| candidate == go_name)
    }

    pub(crate) fn push_binding_frame(&mut self) {
        self.push_frame(DeclarationScope::Transparent);
    }

    pub(crate) fn pop_binding_frame(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Transparent
            ),
            "a binding frame must be pushed before it is popped"
        );
        self.pop_frame();
    }

    pub(crate) fn binding_snapshot(&self) -> BindingSnapshot {
        BindingSnapshot::new(self.current_bindings().clone())
    }

    pub(crate) fn replace_binding_snapshot(
        &mut self,
        snapshot: BindingSnapshot,
    ) -> BindingSnapshot {
        BindingSnapshot::new(std::mem::replace(
            self.current_bindings_mut(),
            snapshot.into_inner(),
        ))
    }

    pub(crate) fn declare_go_name(&mut self, go_name: &str) {
        self.current_declarations_mut()
            .insert(go_name.to_string(), DeclarationKind::Local);
    }

    pub(crate) fn try_declare_go_name(&mut self, go_name: &str) -> bool {
        let current = self.current_declarations_mut();
        if current.contains_key(go_name) {
            false
        } else {
            current.insert(go_name.to_string(), DeclarationKind::Local);
            true
        }
    }

    pub(crate) fn is_go_name_declared(&self, go_name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            match &frame.declarations {
                DeclarationScope::Transparent => {}
                DeclarationScope::Block(names) if names.contains_key(go_name) => return true,
                DeclarationScope::Block(_) => {}
                DeclarationScope::Function(names) => return names.contains_key(go_name),
            }
        }
        false
    }

    pub(crate) fn current_block_declared_nonempty(&self) -> bool {
        !self.current_declarations().is_empty()
    }

    pub(crate) fn enter_block(&mut self) {
        self.push_frame(DeclarationScope::Block(HashMap::default()));
    }

    pub(crate) fn exit_block(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Block(_)
            ),
            "a block must be entered before it is exited"
        );
        self.pop_frame();
    }

    /// Record that the block being lowered runs only when `condition` holds.
    pub(crate) fn establish_condition(&mut self, condition: String) {
        self.frames
            .iter_mut()
            .rev()
            .find(|frame| !matches!(frame.declarations, DeclarationScope::Transparent))
            .expect("scope state always retains a declaration scope")
            .established
            .push(condition);
    }

    pub(crate) fn is_condition_established(&self, condition: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame
                .established
                .iter()
                .any(|established| established == condition)
            {
                return true;
            }
            if matches!(frame.declarations, DeclarationScope::Function(_)) {
                return false;
            }
        }
        false
    }

    pub(crate) fn enter_isolated_function(&mut self) {
        self.push_frame(DeclarationScope::Function(self.visible_type_params()));
    }

    pub(crate) fn exit_isolated_function(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Function(_)
            ),
            "an isolated function must be entered before it is exited"
        );
        self.pop_frame();
    }

    pub(crate) fn fresh_go_name(&mut self, hint: Option<&str>) -> String {
        loop {
            self.next_var += 1;
            let name = match hint {
                Some(h) => format!("{}_{}", h, self.next_var),
                None => format!("tmp_{}", self.next_var),
            };
            if !self.has_binding_for_go_name(&name) && !self.is_go_name_declared(&name) {
                return name;
            }
        }
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
        pop_keep_base(&mut self.return_ctx_stack);
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

    pub(crate) fn current_return_ctx(&self) -> ReturnContext {
        self.return_ctx_stack
            .last()
            .expect("scope state always retains a return context")
            .clone()
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

    fn push_frame(&mut self, declarations: DeclarationScope) {
        self.frames.push(ScopeFrame {
            bindings: self.current_bindings().clone(),
            declarations,
            established: Vec::new(),
        });
    }

    fn pop_frame(&mut self) {
        pop_keep_base(&mut self.frames);
    }

    fn current_frame(&self) -> &ScopeFrame {
        self.frames
            .last()
            .expect("scope state always retains a frame")
    }

    fn current_frame_mut(&mut self) -> &mut ScopeFrame {
        self.frames
            .last_mut()
            .expect("scope state always retains a frame")
    }

    fn current_bindings(&self) -> &HashMap<String, BindingValue> {
        &self.current_frame().bindings
    }

    fn current_bindings_mut(&mut self) -> &mut HashMap<String, BindingValue> {
        &mut self.current_frame_mut().bindings
    }

    fn current_declarations(&self) -> &Declarations {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(names) | DeclarationScope::Function(names) => Some(names),
            })
            .expect("scope state always retains a declaration scope")
    }

    fn current_declarations_mut(&mut self) -> &mut Declarations {
        self.frames
            .iter_mut()
            .rev()
            .find_map(|frame| match &mut frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(names) | DeclarationScope::Function(names) => Some(names),
            })
            .expect("scope state always retains a declaration scope")
    }

    fn visible_type_params(&self) -> Declarations {
        self.frames
            .iter()
            .filter_map(|frame| match &frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(declarations)
                | DeclarationScope::Function(declarations) => Some(declarations),
            })
            .flat_map(|declarations| declarations.iter())
            .filter(|(_, kind)| **kind == DeclarationKind::TypeParameter)
            .map(|(name, kind)| (name.clone(), *kind))
            .collect()
    }
}

fn pop_keep_base<T>(stack: &mut Vec<T>) {
    assert!(stack.len() > 1, "cannot pop a stack's base frame");
    let _ = stack.pop();
}
