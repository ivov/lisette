use std::ops::{Deref, DerefMut};

use rustc_hash::FxHashSet;
use syntax::ast::Span;
use syntax::types::Type;

use crate::checker::type_env::SpeculationOutcome;
use crate::checker::{FileContext, TaskState};
use crate::store::Store;
use std::mem;

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

#[derive(Debug)]
struct LoopFrames {
    stack: Vec<LoopFrame>,
}

#[derive(Debug, Default)]
struct LoopFrame {
    loops: Vec<LoopContext>,
    is_defer: bool,
}

impl Default for LoopFrames {
    fn default() -> Self {
        Self {
            stack: vec![LoopFrame::default()],
        }
    }
}

impl LoopFrames {
    fn current(&self) -> &[LoopContext] {
        &self
            .stack
            .last()
            .expect("the root loop frame must always exist")
            .loops
    }

    fn current_mut(&mut self) -> &mut Vec<LoopContext> {
        &mut self
            .stack
            .last_mut()
            .expect("the root loop frame must always exist")
            .loops
    }

    fn enter_boundary(&mut self, is_defer: bool) {
        self.stack.push(LoopFrame {
            loops: Vec::new(),
            is_defer,
        });
    }

    fn exit_boundary(&mut self) {
        assert!(self.stack.len() > 1, "the root loop frame cannot be exited");
        let frame = self
            .stack
            .pop()
            .expect("a loop boundary must be entered before it is exited");
        assert!(
            frame.loops.is_empty(),
            "loops entered within a boundary must exit before the boundary"
        );
    }

    fn is_inside_defer(&self) -> bool {
        self.stack.iter().any(|frame| frame.is_defer)
    }
}

#[derive(Debug, Default)]
struct TraversalContext {
    loops: LoopFrames,
    in_negation: bool,
    in_invariant_position: bool,
    use_context: UseContext,
    dot_access_base: bool,
    in_pattern: bool,
    let_binding_rhs: bool,
    expectation: Option<Expectation>,
}

#[derive(Debug, Clone)]
pub(super) struct Expectation {
    pub(super) role: ExpectationRole,
    pub(super) span: Span,
    pub(super) expected_ty: Type,
}

#[derive(Debug, Clone)]
pub(super) enum ExpectationRole {
    CallArgument {
        callee_label: String,
        index: usize,
        parameter_name: Option<String>,
    },
    TailReturn {
        function_name: String,
        return_annotation_span: Span,
    },
    MatchArm,
}

impl Expectation {
    pub(super) fn help(&self, expected_name: &str, actual_name: &str) -> String {
        match &self.role {
            ExpectationRole::CallArgument {
                callee_label,
                index,
                parameter_name: Some(parameter_name),
            } => format!(
                "{} expects `{}` for its {} argument `{}`. Convert the value to `{}`, or change the parameter type.",
                callee_label,
                expected_name,
                ordinal(*index),
                parameter_name,
                expected_name,
            ),
            ExpectationRole::CallArgument {
                callee_label,
                index,
                parameter_name: None,
            } => format!(
                "{} expects `{}` for its {} argument. Convert the value to `{}`.",
                callee_label,
                expected_name,
                ordinal(*index),
                expected_name,
            ),
            ExpectationRole::TailReturn { function_name, .. } => format!(
                "`{}` declares return type `{}`, but this tail expression has type `{}`. Change the value, or declare `-> {}`.",
                function_name, expected_name, actual_name, actual_name,
            ),
            ExpectationRole::MatchArm => format!(
                "Every `match` arm must produce the same type. This arm produces `{}`, but the `match` is expected to produce `{}`.",
                actual_name, expected_name,
            ),
        }
    }
}

fn ordinal(n: usize) -> String {
    let word = match n {
        1 => "first",
        2 => "second",
        3 => "third",
        4 => "fourth",
        5 => "fifth",
        6 => "sixth",
        7 => "seventh",
        8 => "eighth",
        9 => "ninth",
        10 => "tenth",
        _ => {
            let suffix = match (n % 100, n % 10) {
                (11..=13, _) => "th",
                (_, 1) => "st",
                (_, 2) => "nd",
                (_, 3) => "rd",
                _ => "th",
            };
            return format!("{n}{suffix}");
        }
    };
    word.to_string()
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
        self.traversal.loops.current_mut().push(context);
        let result = f(self);
        self.traversal
            .loops
            .current_mut()
            .pop()
            .expect("a visible loop must be entered before it is exited");
        result
    }

    pub(super) fn is_inside_loop(&self) -> bool {
        !self.traversal.loops.current().is_empty()
    }

    pub(super) fn loop_depth(&self) -> usize {
        self.traversal.loops.current().len()
    }

    pub(super) fn loop_break_type(&self) -> Option<&Type> {
        match self.traversal.loops.current().last() {
            Some(LoopContext::Value(ty)) => Some(ty),
            Some(LoopContext::Statement) | None => None,
        }
    }

    pub(super) fn is_inside_defer_block(&self) -> bool {
        self.traversal.loops.is_inside_defer()
    }

    pub(super) fn is_inside_negation(&self) -> bool {
        self.traversal.in_negation
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
        self.traversal.in_invariant_position
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
        let previous = mem::replace(&mut self.traversal.use_context, context);
        let result = f(self);
        self.traversal.use_context = previous;
        result
    }

    pub(super) fn with_value_context<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.with_use_context(UseContext::Value, f)
    }

    pub(super) fn with_expectation<T>(
        &mut self,
        expectation: Expectation,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.traversal.expectation.replace(expectation);
        let result = f(self);
        self.traversal.expectation = previous;
        result
    }

    pub(super) fn expectation_at(&self, span: &Span) -> Option<&Expectation> {
        self.traversal
            .expectation
            .as_ref()
            .filter(|expectation| expectation.span == *span)
    }

    pub(super) fn with_dot_access_base<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.dot_access_base, true);
        let result = f(self);
        self.traversal.dot_access_base = previous;
        result
    }

    pub(super) fn with_pattern<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_pattern, true);
        let result = f(self);
        self.traversal.in_pattern = previous;
        result
    }

    pub(super) fn with_let_binding_rhs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.let_binding_rhs, true);
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
        self.traversal.loops.enter_boundary(false);
        let result = f(self);
        self.traversal.loops.exit_boundary();
        result
    }

    pub(crate) fn in_defer_block<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.traversal.loops.enter_boundary(true);
        let result = f(self);
        self.traversal.loops.exit_boundary();
        result
    }

    pub(crate) fn in_negation<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_negation, true);
        let result = f(self);
        self.traversal.in_negation = previous;
        result
    }

    pub(super) fn in_invariant_position<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_invariant_position, true);
        let result = f(self);
        self.traversal.in_invariant_position = previous;
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
    use crate::store;

    #[test]
    fn failed_speculation_rolls_back_diagnostics() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
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
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
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
