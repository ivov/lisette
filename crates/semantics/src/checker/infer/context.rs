use std::ops::{Deref, DerefMut};

use rustc_hash::FxHashSet;
use syntax::ast::Span;
use syntax::types::Type;

use crate::checker::type_env::SpeculationOutcome;
use crate::checker::{FileContext, TaskState};
use crate::store::Store;

#[derive(Debug, Default)]
struct DepthCounter(usize);

impl DepthCounter {
    fn increment(&mut self) {
        self.0 += 1;
    }

    fn decrement(&mut self) {
        self.0 = self
            .0
            .checked_sub(1)
            .expect("depth counter must be incremented before it is decremented");
    }

    fn is_active(&self) -> bool {
        self.0 > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum UseContext {
    #[default]
    Statement,
    Value,
    Callee,
    AssignmentTarget,
}

#[derive(Debug)]
pub(super) enum LoopContext {
    Statement,
    Value(Type),
}

#[derive(Debug, Default)]
struct TraversalContext {
    loops: Vec<LoopContext>,
    loop_boundaries: Vec<usize>,
    defer_block_depth: DepthCounter,
    negation_depth: DepthCounter,
    invariant_depth: DepthCounter,
    use_context: UseContext,
    dot_access_base: bool,
    in_pattern: bool,
    let_binding_rhs: bool,
}

#[derive(Debug, Clone)]
pub(super) struct BranchArm {
    pub(super) ty: Type,
    pub(super) span: Span,
}

pub(super) struct BranchSubsumption {
    pub(super) result_ty: Type,
    pub(super) arms: Vec<BranchArm>,
}

pub(super) struct SelectExhaustivenessCheck {
    pub(super) result_ty: Type,
    pub(super) span: Span,
}

#[derive(Default)]
pub(super) struct FileChecks {
    pub(super) branch_subsumptions: Vec<BranchSubsumption>,
    pub(super) select_exhaustiveness: Vec<SelectExhaustivenessCheck>,
}

impl FileChecks {
    pub(super) fn is_empty(&self) -> bool {
        self.branch_subsumptions.is_empty() && self.select_exhaustiveness.is_empty()
    }
}

pub struct InferCtx<'a> {
    pub(super) state: &'a mut TaskState,
    pub(crate) store: &'a Store,
    pub(super) file_checks: FileChecks,
    pub(crate) satisfying_stack: FxHashSet<(String, String)>,
    traversal: TraversalContext,
}

impl<'a> InferCtx<'a> {
    pub fn new(state: &'a mut TaskState, store: &'a Store) -> Self {
        Self {
            state,
            store,
            file_checks: FileChecks::default(),
            satisfying_stack: FxHashSet::default(),
            traversal: TraversalContext::default(),
        }
    }

    pub(super) fn with_loop<T>(
        &mut self,
        context: LoopContext,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.traversal.loops.push(context);
        let result = f(self);
        assert!(
            self.traversal.loops.len() > self.visible_loop_start(),
            "a visible loop must be entered before it is exited"
        );
        self.traversal.loops.pop();
        result
    }

    pub(super) fn is_inside_loop(&self) -> bool {
        self.traversal.loops.len() > self.visible_loop_start()
    }

    pub(super) fn loop_depth(&self) -> usize {
        self.traversal.loops.len() - self.visible_loop_start()
    }

    pub(super) fn loop_break_type(&self) -> Option<&Type> {
        match self
            .traversal
            .loops
            .get(self.visible_loop_start()..)
            .and_then(|loops| loops.last())
        {
            Some(LoopContext::Value(ty)) => Some(ty),
            Some(LoopContext::Statement) | None => None,
        }
    }

    fn enter_loop_boundary(&mut self) {
        self.traversal
            .loop_boundaries
            .push(self.traversal.loops.len());
    }

    fn exit_loop_boundary(&mut self) {
        let boundary = self
            .traversal
            .loop_boundaries
            .pop()
            .expect("a loop boundary must be entered before it is exited");
        assert_eq!(
            self.traversal.loops.len(),
            boundary,
            "loops entered within a boundary must exit before the boundary"
        );
    }

    fn visible_loop_start(&self) -> usize {
        self.traversal.loop_boundaries.last().copied().unwrap_or(0)
    }

    pub(super) fn is_inside_defer_block(&self) -> bool {
        self.traversal.defer_block_depth.is_active()
    }

    pub(super) fn is_inside_negation(&self) -> bool {
        self.traversal.negation_depth.is_active()
    }

    pub(super) fn is_value_context(&self) -> bool {
        self.traversal.use_context == UseContext::Value
    }

    pub(super) fn is_callee_context(&self) -> bool {
        self.traversal.use_context == UseContext::Callee
    }

    pub(super) fn is_assignment_target_context(&self) -> bool {
        self.traversal.use_context == UseContext::AssignmentTarget
    }

    pub(super) fn is_dot_access_base(&self) -> bool {
        self.traversal.dot_access_base
    }

    pub(super) fn is_in_pattern(&self) -> bool {
        self.traversal.in_pattern
    }

    pub(super) fn is_let_binding_rhs(&self) -> bool {
        self.traversal.let_binding_rhs
    }

    pub(super) fn is_inside_invariant_position(&self) -> bool {
        self.traversal.invariant_depth.is_active()
    }

    pub(crate) fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push();
        let result = f(self);
        self.scopes.pop();
        result
    }

    pub(super) fn with_use_context<T>(
        &mut self,
        context: UseContext,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = std::mem::replace(&mut self.traversal.use_context, context);
        let result = f(self);
        self.traversal.use_context = previous;
        result
    }

    pub(super) fn with_value_context<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.with_use_context(UseContext::Value, f)
    }

    pub(super) fn with_dot_access_base<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = std::mem::replace(&mut self.traversal.dot_access_base, true);
        let result = f(self);
        self.traversal.dot_access_base = previous;
        result
    }

    pub(super) fn with_pattern<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = std::mem::replace(&mut self.traversal.in_pattern, true);
        let result = f(self);
        self.traversal.in_pattern = previous;
        result
    }

    pub(super) fn with_let_binding_rhs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = std::mem::replace(&mut self.traversal.let_binding_rhs, true);
        let result = f(self);
        self.traversal.let_binding_rhs = previous;
        result
    }

    pub(super) fn with_file_context<T>(
        &mut self,
        context: FileContext<'_>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = self.state.enter_file_context(self.store, context);
        let result = f(self);
        self.state.exit_file_context(saved);
        result
    }

    pub(crate) fn speculatively<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let diagnostics_before = self.sink.checkpoint();
        let speculation = self.env.begin_speculation();
        match f(self) {
            Ok(value) => {
                self.env
                    .end_speculation(speculation, SpeculationOutcome::Commit);
                Ok(value)
            }
            Err(error) => {
                self.env
                    .end_speculation(speculation, SpeculationOutcome::Rollback);
                self.sink.rollback(diagnostics_before);
                Err(error)
            }
        }
    }

    pub(crate) fn without_diagnostics<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let diagnostics_before = self.sink.checkpoint();
        let result = f(self);
        self.sink.rollback(diagnostics_before);
        result
    }

    pub(crate) fn tracking_diagnostics<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, bool) {
        let checkpoint = self.sink.checkpoint();
        let result = f(self);
        (result, self.sink.has_changed_since(checkpoint))
    }

    pub(crate) fn without_enclosing_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.enter_loop_boundary();
        let result = f(self);
        self.exit_loop_boundary();
        result
    }

    pub(crate) fn in_defer_block<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.traversal.defer_block_depth.increment();
        let result = self.without_enclosing_loop(f);
        self.traversal.defer_block_depth.decrement();
        result
    }

    pub(crate) fn in_negation<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.traversal.negation_depth.increment();
        let result = f(self);
        self.traversal.negation_depth.decrement();
        result
    }

    pub(super) fn in_invariant_position<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.traversal.invariant_depth.increment();
        let result = f(self);
        self.traversal.invariant_depth.decrement();
        result
    }

    pub(crate) fn with_temporary_bindings<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let checkpoint = self.facts.binding_checkpoint();
        let result = f(self);
        self.facts.remove_bindings_from(checkpoint);
        result
    }
}

impl Deref for InferCtx<'_> {
    type Target = TaskState;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl DerefMut for InferCtx<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_speculation_rolls_back_diagnostics() {
        let mut task = TaskState::for_package(crate::store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let result: Result<(), ()> = {
            let mut ctx = InferCtx::new(&mut task, &store);
            ctx.sink
                .push(diagnostics::LisetteDiagnostic::error("before"));
            ctx.speculatively(|ctx| {
                ctx.sink
                    .push(diagnostics::LisetteDiagnostic::error("speculative"));
                Err(())
            })
        };

        assert!(result.is_err());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "before");
    }

    #[test]
    fn successful_speculation_keeps_diagnostics() {
        let mut task = TaskState::for_package(crate::store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let result: Result<(), ()> = InferCtx::new(&mut task, &store).speculatively(|ctx| {
            ctx.sink
                .push(diagnostics::LisetteDiagnostic::error("reported"));
            Ok(())
        });

        assert!(result.is_ok());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "reported");
    }
}
