use super::resolution::ImportState;
use super::*;

#[derive(Debug, Clone)]
pub struct Cursor {
    pub module_id: String,
    pub(super) file_id: Option<u32>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            module_id: "std".to_string(),
            file_id: None,
        }
    }
}

impl Cursor {
    fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub(super) struct InferredFile {
    pub(super) id: u32,
    pub(super) items: Vec<Expression>,
}

/// The parts of a checker task that survive after its local inference state is
/// discarded.
pub(crate) struct TaskOutput {
    facts: Facts,
    inferred_files: Vec<InferredFile>,
    pending_equality_attributes: Vec<DerivedAttributes>,
    pending_generic_bound_checks: Vec<(Type, Type, Span)>,
    pending_interface_bound_checks: Vec<(Type, Type, Span)>,
    sink: LocalSink,
}

/// A consistent read-only snapshot from which parallel checker tasks start.
pub(crate) struct TaskSeed {
    binding_ids: Arc<BindingIdAllocator>,
    project_kind: crate::analysis::ProjectKind,
}

impl TaskSeed {
    pub(crate) fn spawn(&self) -> TaskState {
        TaskState::new(
            self.binding_ids.clone(),
            LocalSink::new(),
            self.project_kind,
        )
    }
}

/// Per-task mutable state. Semantic reads come directly from a shared [`Store`].
pub struct TaskState {
    pub env: TypeEnv,
    pub(crate) scopes: Scopes,
    pub cursor: Cursor,
    pub(super) imports: ImportState,
    pub sink: LocalSink,
    pub facts: Facts,
    pub(crate) project_kind: crate::analysis::ProjectKind,
    /// Recursion guard for interface satisfaction. Prevents
    /// `collect_interface_violations` from diverging when a bound on `T`
    /// transitively requires checking `T` against the same interface.
    pub(super) satisfying_stack: rustc_hash::FxHashSet<(String, String)>,
    /// Typed ASTs produced by inference, keyed by their canonical stored file.
    pub(super) inferred_files: Vec<InferredFile>,
    /// Equality synthesis waits until registration has completed every type definition.
    pub(crate) pending_equality_attributes: Vec<DerivedAttributes>,
    pub(crate) pending_generic_bound_checks: Vec<(Type, Type, Span)>,
    /// Interface bounds on concrete type arguments named in annotations. Drained
    /// once after inference, since body annotations register during it.
    pub(crate) pending_interface_bound_checks: Vec<(Type, Type, Span)>,
}

impl TaskState {
    fn new(
        binding_ids: Arc<BindingIdAllocator>,
        sink: LocalSink,
        project_kind: crate::analysis::ProjectKind,
    ) -> Self {
        Self {
            env: TypeEnv::new(),
            scopes: Scopes::new(),
            cursor: Cursor::new(),
            imports: ImportState::new(),
            sink,
            facts: Facts::new(binding_ids),
            project_kind,
            satisfying_stack: rustc_hash::FxHashSet::default(),
            inferred_files: Vec::new(),
            pending_equality_attributes: Vec::new(),
            pending_generic_bound_checks: Vec::new(),
            pending_interface_bound_checks: Vec::new(),
        }
    }

    pub fn with_fresh_allocator() -> Self {
        Self::new(
            Arc::new(BindingIdAllocator::new()),
            LocalSink::new(),
            crate::analysis::ProjectKind::Binary,
        )
    }

    pub(crate) fn with_sink(sink: LocalSink, project_kind: crate::analysis::ProjectKind) -> Self {
        Self::new(Arc::new(BindingIdAllocator::new()), sink, project_kind)
    }

    pub(crate) fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push();
        let result = f(self);
        self.scopes.pop();
        result
    }

    pub(crate) fn worker_seed(&self) -> TaskSeed {
        TaskSeed {
            binding_ids: self.facts.allocator(),
            project_kind: self.project_kind,
        }
    }

    pub(crate) fn into_output(self) -> TaskOutput {
        let Self {
            env: _,
            scopes: _,
            cursor: _,
            imports: _,
            sink,
            facts,
            project_kind: _,
            satisfying_stack: _,
            inferred_files,
            pending_equality_attributes,
            pending_generic_bound_checks,
            pending_interface_bound_checks,
        } = self;
        TaskOutput {
            facts,
            inferred_files,
            pending_equality_attributes,
            pending_generic_bound_checks,
            pending_interface_bound_checks,
            sink,
        }
    }

    pub(crate) fn absorb_outputs(&mut self, outputs: Vec<TaskOutput>) {
        let mut sinks = Vec::with_capacity(outputs.len());
        for output in outputs {
            let TaskOutput {
                facts,
                inferred_files,
                pending_equality_attributes,
                pending_generic_bound_checks,
                pending_interface_bound_checks,
                sink,
            } = output;
            self.facts.merge(facts);
            self.inferred_files.extend(inferred_files);
            self.pending_equality_attributes
                .extend(pending_equality_attributes);
            self.pending_generic_bound_checks
                .extend(pending_generic_bound_checks);
            self.pending_interface_bound_checks
                .extend(pending_interface_bound_checks);
            sinks.push(sink);
        }
        self.sink.extend(LocalSink::merge(sinks));
    }

    pub fn new_type_var(&mut self) -> Type {
        let id = self.env.fresh();
        Type::Var { id, hint: None }
    }

    pub(super) fn new_type_var_with_hint(&mut self, hint: &str) -> Type {
        let hint: EcoString = hint.into();
        let id = self.env.fresh();
        Type::Var {
            id,
            hint: Some(hint),
        }
    }

    pub(super) fn type_from_literal_expression(&mut self, expression: &Expression) -> Option<Type> {
        use syntax::ast::{Expression, Literal};
        match expression {
            Expression::Literal { literal, .. } => match literal {
                Literal::Integer { .. } => Some(self.type_int()),
                Literal::Float { .. } => Some(self.type_float()),
                Literal::Boolean(_) => Some(self.type_bool()),
                Literal::String { .. } => Some(self.type_string()),
                Literal::Char(_) => Some(self.type_char()),
                _ => None,
            },
            Expression::Unary { expression, .. } => self.type_from_literal_expression(expression),
            _ => None,
        }
    }

    pub(super) fn instantiate(&mut self, ty: &Type) -> (Type, SubstitutionMap) {
        match ty {
            Type::Forall { vars, body } => {
                let map: SubstitutionMap = vars
                    .iter()
                    .map(|name| {
                        let id = self.env.fresh();
                        let fresh_var = Type::Var {
                            id,
                            hint: Some(name.clone()),
                        };
                        (name.clone(), fresh_var)
                    })
                    .collect();

                (substitute(body, &map), map)
            }
            _ => (ty.clone(), HashMap::default()),
        }
    }
}
