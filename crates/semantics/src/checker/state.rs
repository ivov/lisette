use super::resolution::ImportState;
use super::*;
use syntax::program::TestFunction;

#[derive(Debug, Clone)]
pub struct Cursor {
    location: CursorLocation,
}

#[derive(Debug, Clone)]
enum CursorLocation {
    Package { package_id: String },
    File { package_id: String, file_id: u32 },
}

impl Cursor {
    fn package(package_id: impl Into<String>) -> Self {
        Self {
            location: CursorLocation::Package {
                package_id: package_id.into(),
            },
        }
    }

    pub fn package_id(&self) -> &str {
        match &self.location {
            CursorLocation::Package { package_id } | CursorLocation::File { package_id, .. } => {
                package_id
            }
        }
    }

    pub(super) fn file_id(&self) -> Option<u32> {
        match &self.location {
            CursorLocation::Package { .. } => None,
            CursorLocation::File { file_id, .. } => Some(*file_id),
        }
    }

    pub(super) fn file(package_id: impl Into<String>, file_id: u32) -> Self {
        Self {
            location: CursorLocation::File {
                package_id: package_id.into(),
                file_id,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct InferredFile {
    pub(super) id: u32,
    pub(super) items: Vec<Expression>,
}

/// The parts of a checker task that survive after its local inference state is
/// discarded.
pub(crate) struct TaskOutput {
    facts: Facts,
    pending: PendingWork,
    sink: LocalSink,
}

#[derive(Default)]
pub(crate) struct PendingWork {
    pub(crate) equality_attributes: Vec<EqualityAttributes>,
    pub(crate) pre_inference_bound_checks: Vec<(Type, Type, Span)>,
    pub(crate) post_inference_bound_checks: Vec<(Type, Type, Span)>,
    pub(crate) test_functions: Vec<TestFunction>,
}

impl PendingWork {
    fn merge(&mut self, other: Self) {
        self.equality_attributes.extend(other.equality_attributes);
        self.pre_inference_bound_checks
            .extend(other.pre_inference_bound_checks);
        self.post_inference_bound_checks
            .extend(other.post_inference_bound_checks);
        self.test_functions.extend(other.test_functions);
    }
}

/// A consistent read-only snapshot from which parallel checker tasks start.
pub(crate) struct TaskSeed {
    binding_ids: Arc<BindingIdAllocator>,
    project_kind: crate::analysis::ProjectKind,
    script: Option<crate::analysis::ScriptUnit>,
}

impl TaskSeed {
    pub(crate) fn spawn(&self) -> TaskState {
        TaskState::new(
            self.binding_ids.clone(),
            LocalSink::new(),
            self.project_kind,
            crate::store::ENTRY_PACKAGE_ID,
            self.script,
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
    pub(crate) script: Option<crate::analysis::ScriptUnit>,
    pub(crate) pending: PendingWork,
}

impl TaskState {
    fn new(
        binding_ids: Arc<BindingIdAllocator>,
        sink: LocalSink,
        project_kind: crate::analysis::ProjectKind,
        package_id: impl Into<String>,
        script: Option<crate::analysis::ScriptUnit>,
    ) -> Self {
        Self {
            env: TypeEnv::new(),
            scopes: Scopes::new(),
            cursor: Cursor::package(package_id),
            imports: ImportState::new(),
            sink,
            facts: Facts::new(binding_ids),
            project_kind,
            script,
            pending: PendingWork::default(),
        }
    }

    pub fn for_package(package_id: impl Into<String>) -> Self {
        Self::new(
            Arc::new(BindingIdAllocator::new()),
            LocalSink::new(),
            crate::analysis::ProjectKind::Binary,
            package_id,
            None,
        )
    }

    pub(crate) fn with_sink(
        sink: LocalSink,
        project_kind: crate::analysis::ProjectKind,
        script: Option<crate::analysis::ScriptUnit>,
    ) -> Self {
        Self::new(
            Arc::new(BindingIdAllocator::new()),
            sink,
            project_kind,
            crate::store::ENTRY_PACKAGE_ID,
            script,
        )
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
            script: self.script,
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
            script: _,
            pending,
        } = self;
        TaskOutput {
            facts,
            pending,
            sink,
        }
    }

    pub(crate) fn absorb_outputs(&mut self, outputs: Vec<TaskOutput>) {
        let mut sinks = Vec::with_capacity(outputs.len());
        for output in outputs {
            let TaskOutput {
                facts,
                pending,
                sink,
            } = output;
            self.facts.merge(facts);
            self.pending.merge(pending);
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
