use std::cell::Cell;
use std::ops::{Deref, DerefMut};

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{BindingId, ExpressionId, Span};
use syntax::program::ValueUseKey;
use syntax::types::Type;

use crate::checker::type_env::SpeculationOutcome;
use crate::checker::{EnvResolve, FileContext, TaskState};
use crate::facts::{Facts, StatementTailCheck};
use crate::store::Store;
use std::mem;

pub(crate) struct UnificationAttempt<'context, 'state> {
    context: &'context mut InferCtx<'state>,
}

impl UnificationAttempt<'_, '_> {
    pub(crate) fn try_unify(
        &mut self,
        expected: &Type,
        actual: &Type,
        span: &Span,
    ) -> Result<(), super::unify::UnifyError> {
        self.context.try_unify(expected, actual, span)
    }

    pub(crate) fn resolve(&self, ty: &Type) -> Type {
        ty.resolve_in(&self.context.env)
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

#[derive(Debug, Clone)]
pub(super) enum LoopContext {
    Statement,
    Value(Type),
}

#[derive(Debug, Clone)]
struct LoopFrames {
    stack: Vec<LoopFrame>,
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, Default)]
struct TraversalContext {
    loops: LoopFrames,
    in_rejected_go_builtin_args: bool,
    in_constant_expression: bool,
    in_invariant_position: bool,
    /// Below a writable layer and in nominal arguments, stores flow back at the actual type.
    qualifier_invariant: bool,
    /// In parameter position an implementation may demand less permission.
    flipped_qualifier_variance: bool,
    use_context: UseContext,
    dot_access_base: bool,
    in_pattern: bool,
    let_binding_rhs: bool,
    expectation: Option<Expectation>,
}

#[derive(Clone)]
pub(super) enum ValueSource {
    Place { name: String, ty: Type },
    Call { callee: Option<String>, span: Span },
    GoFieldCopy(diagnostics::infer::WriteContext),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LetMutability {
    NoFix,
    AddMut,
    RestoreWriteWithMut,
}

impl LetMutability {
    pub(super) fn can_add_mut(self) -> bool {
        !matches!(self, Self::NoFix)
    }

    pub(super) fn was_demoted(self) -> bool {
        matches!(self, Self::RestoreWriteWithMut)
    }
}

#[derive(Clone)]
pub(super) struct LetInference {
    pub(super) mutability: LetMutability,
    pub(super) source: Option<ValueSource>,
    pub(super) annotated: bool,
}

#[derive(Clone)]
pub(super) struct LoopElementInference {
    pub(super) collection: Option<String>,
    pub(super) was_demoted: bool,
    pub(super) reads: Vec<Span>,
    pub(super) writes: Vec<Span>,
}

#[derive(Clone)]
pub(super) enum BindingInference {
    Let(LetInference),
    LoopElement(LoopElementInference),
}

impl BindingInference {
    pub(super) fn as_let(&self) -> Option<&LetInference> {
        match self {
            Self::Let(inference) => Some(inference),
            Self::LoopElement(_) => None,
        }
    }

    pub(super) fn as_loop_element(&self) -> Option<&LoopElementInference> {
        match self {
            Self::LoopElement(inference) => Some(inference),
            Self::Let(_) => None,
        }
    }

    pub(super) fn as_loop_element_mut(&mut self) -> Option<&mut LoopElementInference> {
        match self {
            Self::LoopElement(inference) => Some(inference),
            Self::Let(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Expectation {
    pub(super) role: ExpectationRole,
    pub(super) span: Span,
    pub(super) expected_ty: Type,
    pub(super) value_context: Option<diagnostics::infer::WriteContext>,
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
    FallibleBlockTail {
        keyword: &'static str,
    },
    Receiver,
}

impl Expectation {
    pub(super) fn help(&self, expected_name: &str, actual_name: &str) -> Option<String> {
        let help = match &self.role {
            ExpectationRole::CallArgument {
                callee_label,
                index,
                parameter_name: Some(parameter_name),
            } => format!(
                "Convert the value to `{}`, or change the parameter type. {} expects `{}` for its {} argument `{}`.",
                expected_name,
                callee_label,
                expected_name,
                ordinal(*index),
                parameter_name,
            ),
            ExpectationRole::CallArgument {
                callee_label,
                index,
                parameter_name: None,
            } => format!(
                "Convert the value to `{}`. {} expects `{}` for its {} argument.",
                expected_name,
                callee_label,
                expected_name,
                ordinal(*index),
            ),
            ExpectationRole::TailReturn { function_name, .. } => format!(
                "Change the value, or declare `-> {}`. `{}` declares return type `{}`, but this tail expression has type `{}`.",
                actual_name, function_name, expected_name, actual_name,
            ),
            ExpectationRole::MatchArm => {
                "Every `match` arm must produce the same type.".to_string()
            }
            ExpectationRole::FallibleBlockTail { keyword } => format!(
                "The last expression of a `{}` block is its success value.",
                keyword,
            ),
            ExpectationRole::Receiver => return None,
        };
        Some(help)
    }

    pub(super) fn argument_requirement(&self, expected_name: &str) -> Option<String> {
        let ExpectationRole::CallArgument {
            callee_label,
            index,
            parameter_name,
        } = &self.role
        else {
            return None;
        };
        let argument = match parameter_name {
            Some(name) => format!("{} argument `{}`", ordinal(*index), name),
            None => format!("{} argument", ordinal(*index)),
        };
        Some(format!(
            "{callee_label} expects `{expected_name}` for its {argument}"
        ))
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
    /// The value uses that produce this arm's value.
    pub(super) values: Vec<ValueUseKey>,
}

#[derive(Clone, Copy)]
pub(super) enum ArmAgreement {
    NotCompared,
    Conflicting,
}

pub(super) struct BranchSubsumption {
    pub(super) result_ty: Type,
    pub(super) arms: Vec<BranchArm>,
    pub(super) arm_agreement: ArmAgreement,
}

pub(super) struct SelectExhaustivenessCheck {
    pub(super) result_ty: Type,
    pub(super) span: Span,
}

#[derive(Default)]
pub(super) struct FileObligations {
    pub(super) branch_subsumptions: Vec<BranchSubsumption>,
    pub(super) select_exhaustiveness: Vec<SelectExhaustivenessCheck>,
    pub(super) statement_tails: Vec<StatementTailCheck>,
}

pub(super) enum FileChecks {
    Collecting(FileObligations),
    Discharged,
}

impl Default for FileChecks {
    fn default() -> Self {
        Self::Collecting(FileObligations::default())
    }
}

impl FileChecks {
    pub(super) fn pending_mut(&mut self) -> &mut FileObligations {
        match self {
            Self::Collecting(pending) => pending,
            Self::Discharged => panic!("cannot enqueue work after the file inference phase"),
        }
    }

    pub(super) fn branches(&self) -> &[BranchSubsumption] {
        match self {
            Self::Collecting(pending) => &pending.branch_subsumptions,
            Self::Discharged => &[],
        }
    }

    fn checkpoint(&self) -> Option<(usize, usize, usize)> {
        match self {
            Self::Collecting(pending) => Some((
                pending.branch_subsumptions.len(),
                pending.select_exhaustiveness.len(),
                pending.statement_tails.len(),
            )),
            Self::Discharged => None,
        }
    }

    fn rollback(&mut self, checkpoint: Option<(usize, usize, usize)>) {
        if let Some((branches, selects, tails)) = checkpoint {
            let pending = self.pending_mut();
            pending.branch_subsumptions.truncate(branches);
            pending.select_exhaustiveness.truncate(selects);
            pending.statement_tails.truncate(tails);
        } else {
            *self = Self::Discharged;
        }
    }

    pub(super) fn discharge(&mut self) -> FileObligations {
        match mem::replace(self, Self::Discharged) {
            Self::Collecting(pending) => pending,
            Self::Discharged => panic!("file obligations must be discharged exactly once"),
        }
    }
}

pub struct InferCtx<'a> {
    pub(super) state: &'a mut TaskState,
    pub(crate) store: &'a Store,
    pub(super) file_checks: FileChecks,
    pub(crate) satisfying_stack: FxHashSet<(Type, Type)>,
    pub(super) reported_immutable: FxHashSet<BindingId>,
    pub(super) binding_inference: FxHashMap<BindingId, BindingInference>,
    pub(super) reported_read_only_writes:
        FxHashSet<(BindingId, super::expressions::aliasing::PlaceKey)>,
    /// By span, for the diagnostic when the construction misses a writable expectation.
    pub(super) read_only_constructions: FxHashMap<Span, Vec<diagnostics::infer::ReadOnlyComponent>>,
    traversal: TraversalContext,
    generated_expressions: Cell<u32>,
}

impl<'a> InferCtx<'a> {
    pub fn new(state: &'a mut TaskState, store: &'a Store) -> Self {
        Self {
            state,
            store,
            file_checks: FileChecks::default(),
            satisfying_stack: FxHashSet::default(),
            reported_immutable: FxHashSet::default(),
            binding_inference: FxHashMap::default(),
            reported_read_only_writes: FxHashSet::default(),
            read_only_constructions: FxHashMap::default(),
            traversal: TraversalContext::default(),
            generated_expressions: Cell::new(0),
        }
    }

    /// A new occurrence for a node that inference builds.
    pub(crate) fn fresh_expression_id(&self) -> ExpressionId {
        let index = self.generated_expressions.get();
        self.generated_expressions.set(index + 1);
        let file_id = self
            .cursor
            .file_id()
            .expect("inference builds nodes only inside a file");
        ExpressionId::generated(file_id, index)
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

    pub(super) fn is_inside_rejected_go_builtin_args(&self) -> bool {
        self.traversal.in_rejected_go_builtin_args
    }

    pub(super) fn is_inside_constant_expression(&self) -> bool {
        self.traversal.in_constant_expression
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

    pub(super) fn with_dot_access_base<T>(
        &mut self,
        is_base: bool,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = mem::replace(&mut self.traversal.dot_access_base, is_base);
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

    pub(crate) fn unify_speculatively<T, E>(
        &mut self,
        f: impl FnOnce(&mut UnificationAttempt<'_, '_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let speculation = self.env.begin_speculation();
        let diagnostics = self.sink.checkpoint();

        let delta = Facts::new(self.facts.allocator());
        let previous = mem::replace(&mut self.state.facts, delta);
        let result = f(&mut UnificationAttempt { context: self });
        let delta = mem::replace(&mut self.state.facts, previous);
        let outcome = if result.is_ok() {
            self.facts.merge(delta);
            SpeculationOutcome::Commit
        } else {
            self.sink.rollback(diagnostics);
            SpeculationOutcome::Rollback
        };
        self.env.end_speculation(speculation, outcome);
        result
    }

    /// Whether unification accepts a value of type `actual` where `expected` is expected, keeping no binding, fact or diagnostic.
    pub(crate) fn admits(&mut self, expected: &Type, actual: &Type, span: &Span) -> bool {
        let speculation = self.env.begin_speculation();
        let diagnostics = self.sink.checkpoint();
        let delta = Facts::new(self.facts.allocator());
        let previous = mem::replace(&mut self.state.facts, delta);
        let admitted = self.try_unify(expected, actual, span).is_ok();
        self.state.facts = previous;
        self.sink.rollback(diagnostics);
        self.env
            .end_speculation(speculation, SpeculationOutcome::Rollback);
        admitted
    }

    /// Runs `f` and discards every unification and diagnostic it produced.
    pub(crate) fn probe<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.with_speculation(|this| (f(this), SpeculationOutcome::Rollback))
    }

    pub(crate) fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        self.with_speculation(|this| {
            let value = f(this);
            let outcome = if value.is_some() {
                SpeculationOutcome::Commit
            } else {
                SpeculationOutcome::Rollback
            };
            (value, outcome)
        })
    }

    fn with_speculation<T>(&mut self, f: impl FnOnce(&mut Self) -> (T, SpeculationOutcome)) -> T {
        let Self {
            state,
            store: _,
            file_checks,
            satisfying_stack,
            reported_immutable,
            binding_inference,
            reported_read_only_writes,
            read_only_constructions,
            traversal,
            generated_expressions: _,
        } = self;
        let TaskState {
            env: _,
            scopes,
            cursor,
            imports,
            sink: _,
            facts,
            project_kind,
            script,
            pending,
        } = &mut **state;

        let mut delta = Facts::new(facts.allocator());
        delta.bindings.extend(
            scopes
                .binding_ids()
                .filter_map(|id| facts.bindings.get(&id).map(|fact| (id, fact.clone()))),
        );
        let mut saved_task = TaskState {
            env: Default::default(),
            scopes: scopes.clone(),
            cursor: cursor.clone(),
            imports: imports.clone(),
            sink: Default::default(),
            facts: mem::replace(facts, delta),
            project_kind: *project_kind,
            script: *script,
            pending: mem::take(pending),
        };
        let diagnostics = state.sink.checkpoint();
        let local_bindings = state
            .scopes
            .binding_ids()
            .filter_map(|id| {
                binding_inference
                    .get(&id)
                    .map(|inference| (id, inference.clone()))
            })
            .collect();
        let saved_binding_inference = mem::replace(binding_inference, local_bindings);
        let file_checkpoint = file_checks.checkpoint();
        let saved_context = (
            satisfying_stack.clone(),
            reported_immutable.clone(),
            reported_read_only_writes.clone(),
            read_only_constructions.clone(),
            traversal.clone(),
        );
        let speculation = self.env.begin_speculation();
        let (value, outcome) = f(self);
        match outcome {
            SpeculationOutcome::Commit => {
                let local = mem::replace(&mut self.binding_inference, saved_binding_inference);
                self.binding_inference.extend(local);
                let delta = mem::replace(&mut self.state.facts, saved_task.facts);
                self.facts.merge(delta);
                let pending = mem::replace(&mut self.state.pending, saved_task.pending);
                self.state.pending.merge(pending);
                self.env
                    .end_speculation(speculation, SpeculationOutcome::Commit);
            }
            SpeculationOutcome::Rollback => {
                self.env
                    .end_speculation(speculation, SpeculationOutcome::Rollback);

                saved_task.env = mem::take(&mut self.state.env);
                self.sink.rollback(diagnostics);
                saved_task.sink = mem::take(&mut self.state.sink);
                *self.state = saved_task;
                self.binding_inference = saved_binding_inference;
                self.file_checks.rollback(file_checkpoint);
                (
                    self.satisfying_stack,
                    self.reported_immutable,
                    self.reported_read_only_writes,
                    self.read_only_constructions,
                    self.traversal,
                ) = saved_context;
            }
        }
        value
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

    pub(super) fn in_rejected_go_builtin_args<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_rejected_go_builtin_args, true);
        let result = f(self);
        self.traversal.in_rejected_go_builtin_args = previous;
        result
    }

    pub(crate) fn in_constant_expression<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_constant_expression, true);
        let result = f(self);
        self.traversal.in_constant_expression = previous;
        result
    }

    pub(super) fn in_invariant_position<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = mem::replace(&mut self.traversal.in_invariant_position, true);
        let result = f(self);
        self.traversal.in_invariant_position = previous;
        result
    }

    pub(super) fn is_qualifier_invariant(&self) -> bool {
        self.traversal.qualifier_invariant
    }

    pub(super) fn in_qualifier_invariant<T>(
        &mut self,
        invariant: bool,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let entered = self.traversal.qualifier_invariant || invariant;
        let previous = mem::replace(&mut self.traversal.qualifier_invariant, entered);
        let result = f(self);
        self.traversal.qualifier_invariant = previous;
        result
    }

    pub(super) fn is_qualifier_variance_flipped(&self) -> bool {
        self.traversal.flipped_qualifier_variance
    }

    pub(super) fn in_flipped_qualifier_variance<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.traversal.flipped_qualifier_variance;
        self.traversal.flipped_qualifier_variance = !previous;
        let result = f(self);
        self.traversal.flipped_qualifier_variance = previous;
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
    use crate::checker::EnvResolve;
    use crate::checker::infer::unify::UnifyError;
    use crate::store;

    #[test]
    #[should_panic(expected = "cannot enqueue work after the file inference phase")]
    fn discharged_file_rejects_new_obligations() {
        let mut checks = FileChecks::default();
        let _completed = checks.discharge();
        checks
            .pending_mut()
            .select_exhaustiveness
            .push(SelectExhaustivenessCheck {
                result_ty: Type::int(),
                span: Span::dummy(),
            });
    }

    #[test]
    fn abandoned_attempt_preserves_facts_usages_and_diagnostics() {
        use crate::facts::BindingOrigin;
        use syntax::ast::BindingKind;

        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut context = InferCtx::new(&mut task, &store);
        let binding = context.facts.add_binding(
            "value".into(),
            Span::dummy(),
            BindingKind::Let { mutable: true },
            BindingOrigin::Name {
                in_typedef: false,
                shorthand_field: false,
            },
            None,
        );
        context
            .scopes
            .current_mut()
            .insert_binding("value".into(), Type::int(), binding, true);
        context
            .sink
            .push(diagnostics::LisetteDiagnostic::error("before"));
        let before = format!("{:?}", context.facts);
        context.probe(|context| {
            assert_eq!(context.facts.bindings[&binding].name, "value");
            assert!(!context.facts.bindings[&binding].used);
            context.facts.mark_used(binding);
            assert!(context.facts.bindings[&binding].used);
            context.facts.mark_mutated(binding);
            context.facts.add_usage(Span::dummy(), Span::dummy());
            context.facts.function_spans.push(Span::dummy());
            context.with_speculation(|context| {
                context.facts.mark_method_used_for_interface(
                    "pkg".into(),
                    "method".into(),
                    "Owner".into(),
                    true,
                );
                context
                    .sink
                    .push(diagnostics::LisetteDiagnostic::error("nested commit"));
                ((), SpeculationOutcome::Commit)
            });
        });
        assert_eq!(format!("{:?}", context.facts), before);
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "before");
    }

    #[test]
    fn unification_attempt_rolls_back_partial_binding_and_nested_facts() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut context = InferCtx::new(&mut task, &store);
        let variable = context.new_type_var();
        let before = format!("{:?}", context.facts);
        let result = context.unify_speculatively(|attempt| {
            attempt.try_unify(&variable, &Type::int(), &Span::dummy())?;

            attempt.context.unify_speculatively(|nested| {
                nested.context.facts.add_usage(Span::dummy(), Span::dummy());
                nested
                    .context
                    .sink
                    .push(diagnostics::LisetteDiagnostic::error("nested"));
                Ok::<_, UnifyError>(())
            })?;
            attempt.try_unify(&variable, &Type::string(), &Span::dummy())
        });
        assert!(result.is_err());
        assert_eq!(variable.resolve_in(&context.env), variable);
        assert_eq!(format!("{:?}", context.facts), before);
        assert!(task.sink.into_diagnostics().is_empty());
    }

    #[test]
    fn failed_speculation_rolls_back_diagnostics() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let result: Result<(), ()> = {
            let mut context = InferCtx::new(&mut task, &store);
            context
                .sink
                .push(diagnostics::LisetteDiagnostic::error("before"));
            context.with_speculation(|context| {
                context
                    .sink
                    .push(diagnostics::LisetteDiagnostic::error("speculative"));
                (Err(()), SpeculationOutcome::Rollback)
            })
        };

        assert!(result.is_err());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "before");
    }

    #[test]
    fn abandoned_attempt_rolls_back_a_nested_commit_without_losing_outer_changes() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut context = InferCtx::new(&mut task, &store);
        let outer = context.new_type_var();
        let inner = context.new_type_var();
        let span = Span::dummy();

        context.with_speculation(|context| {
            context.try_unify(&outer, &Type::int(), &span).unwrap();
            let observed = context.probe(|context| {
                context.with_speculation(|context| {
                    context.try_unify(&inner, &Type::string(), &span).unwrap();
                    context
                        .sink
                        .push(diagnostics::LisetteDiagnostic::error("probe"));
                    (inner.resolve_in(&context.env), SpeculationOutcome::Commit)
                })
            });
            assert_eq!(observed, Type::string());
            assert_eq!(inner.resolve_in(&context.env), inner);
            ((), SpeculationOutcome::Commit)
        });

        assert_eq!(outer.resolve_in(&context.env), Type::int());
        assert!(task.sink.into_diagnostics().is_empty());
    }

    #[test]
    fn successful_speculation_keeps_diagnostics() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let result: Result<(), ()> = InferCtx::new(&mut task, &store).with_speculation(|context| {
            context
                .sink
                .push(diagnostics::LisetteDiagnostic::error("reported"));
            (Ok(()), SpeculationOutcome::Commit)
        });

        assert!(result.is_ok());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "reported");
    }

    #[test]
    fn unification_resolves_bound_variables_before_comparing_either_side() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut ctx = InferCtx::new(&mut task, &store);
        let a = ctx.new_type_var();
        let b = ctx.new_type_var();
        let span = Span::dummy();
        ctx.try_unify(&a, &b, &span).unwrap();
        ctx.try_unify(&Type::int(), &b, &span).unwrap();

        assert_eq!(a.resolve_in(&ctx.env), Type::int());
        assert_eq!(
            ctx.try_unify(&a, &Type::string(), &span),
            Err(UnifyError::TypeMismatch)
        );
        assert_eq!(
            ctx.try_unify(&Type::string(), &a, &span),
            Err(UnifyError::TypeMismatch)
        );
    }

    #[test]
    fn unification_rejects_recursive_bindings_through_variable_chains() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut ctx = InferCtx::new(&mut task, &store);
        let a = ctx.new_type_var();
        let b = ctx.new_type_var();
        let span = Span::dummy();
        ctx.try_unify(&a, &b, &span).unwrap();

        assert_eq!(
            ctx.try_unify(&b, &Type::Tuple(vec![a.clone()]), &span),
            Err(UnifyError::InfiniteType)
        );
        assert_eq!(b.resolve_in(&ctx.env), b);
        assert_eq!(ctx.try_unify(&a, &a, &span), Ok(()));
    }

    #[test]
    fn error_unification_reaches_variables_inside_arrays() {
        let mut task = TaskState::for_package(store::ENTRY_PACKAGE_ID);
        let store = Store::new();
        let mut ctx = InferCtx::new(&mut task, &store);
        let element = ctx.new_type_var();
        let array = Type::Array {
            length: 2,
            element: Box::new(element.clone()),
        };

        ctx.try_unify(&Type::Error, &array, &Span::dummy()).unwrap();

        assert_eq!(element.resolve_in(&ctx.env), Type::Error);
    }
}
