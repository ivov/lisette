use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{PatternIssue, UnusedExpressionKind};
use syntax::ast::{BindingId, BindingKind, DeadCodeCause, Span};

#[derive(Debug, Default)]
pub struct Facts {
    next_id: BindingId,
    pub bindings: HashMap<BindingId, BindingFact>,
    pub dead_code: Vec<DeadCodeFact>,
    pub pattern_issues: Vec<PatternIssue>,
    pub unused_expressions: Vec<UnusedExpressionFact>,
    pub discarded_tail_expressions: Vec<DiscardedTailFact>,
    pub overused_references: Vec<OverusedReferenceFact>,
    pub unused_type_params: Vec<UnusedTypeParamFact>,
    pub type_params_only_in_bound: Vec<TypeParamOnlyInBoundFact>,
    pub always_failing_try_blocks: Vec<Span>,
    pub expression_only_fstrings: Vec<Span>,
    /// Spans of or-patterns with binding errors, used to suppress contradictory lints.
    pub or_pattern_error_spans: HashSet<Span>,
    /// Tracks usage locations for find-references in LSP
    pub usages: Vec<Usage>,
    usage_set: HashSet<(Span, Span)>,
    /// Tracks methods used via interface satisfaction during type checking.
    /// Maps (module_id, method_name) -> usage locations
    pub interface_satisfied_methods: HashMap<(String, String), Vec<Span>>,
}

impl Facts {
    pub fn new() -> Self {
        Self::default()
    }

    fn new_binding_id(&mut self) -> BindingId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add_binding(
        &mut self,
        name: String,
        span: Span,
        kind: BindingKind,
        is_typedef: bool,
        is_struct_field: bool,
        is_as_alias: bool,
    ) -> BindingId {
        let id = self.new_binding_id();
        self.bindings.insert(
            id,
            BindingFact {
                name,
                span,
                kind,
                used: false,
                mutated: false,
                is_typedef,
                is_struct_field,
                is_as_alias,
            },
        );
        id
    }

    pub fn mark_used(&mut self, id: BindingId) {
        if let Some(fact) = self.bindings.get_mut(&id) {
            fact.used = true;
        }
    }

    pub fn mark_mutated(&mut self, id: BindingId) {
        if let Some(fact) = self.bindings.get_mut(&id) {
            fact.mutated = true;
        }
    }

    pub fn binding_checkpoint(&self) -> BindingId {
        self.next_id
    }

    pub fn remove_bindings_from(&mut self, checkpoint: BindingId) {
        self.bindings.retain(|id, _| *id < checkpoint);
        self.next_id = checkpoint;
    }

    pub fn add_dead_code(&mut self, span: Span, cause: DeadCodeCause) {
        self.dead_code.push(DeadCodeFact { span, cause });
    }

    pub fn add_unused_expression(&mut self, span: Span, kind: UnusedExpressionKind) {
        self.unused_expressions
            .push(UnusedExpressionFact { span, kind });
    }

    pub fn add_discarded_tail(&mut self, span: Span, kind: DiscardedTailKind, return_type: String) {
        self.discarded_tail_expressions.push(DiscardedTailFact {
            span,
            kind,
            return_type,
        });
    }

    pub fn add_overused_reference(&mut self, span: Span, name: Option<String>) {
        self.overused_references
            .push(OverusedReferenceFact { span, name });
    }

    pub fn add_unused_type_param(&mut self, name: String, span: Span, is_typedef: bool) {
        self.unused_type_params.push(UnusedTypeParamFact {
            name,
            span,
            is_typedef,
        });
    }

    pub fn add_type_param_only_in_bound(&mut self, name: String, span: Span, is_typedef: bool) {
        self.type_params_only_in_bound
            .push(TypeParamOnlyInBoundFact {
                name,
                span,
                is_typedef,
            });
    }

    pub fn add_always_failing_try_block(&mut self, span: Span) {
        self.always_failing_try_blocks.push(span);
    }

    pub fn add_expression_only_fstring(&mut self, span: Span) {
        self.expression_only_fstrings.push(span);
    }

    pub fn add_usage(&mut self, usage_span: Span, definition_span: Span) {
        if self.usage_set.insert((usage_span, definition_span)) {
            self.usages.push(Usage {
                usage_span,
                definition_span,
            });
        }
    }

    pub fn mark_method_used_for_interface(
        &mut self,
        module_id: String,
        method_name: String,
        usage_span: Span,
    ) {
        self.interface_satisfied_methods
            .entry((module_id, method_name))
            .or_default()
            .push(usage_span);
    }
}

#[derive(Debug, Clone)]
pub struct BindingFact {
    pub name: String,
    pub span: Span,
    pub kind: BindingKind,
    pub used: bool,
    pub mutated: bool,
    pub is_typedef: bool,
    /// If true, this binding is a shorthand in a struct pattern (e.g., `Point { x }`)
    pub is_struct_field: bool,
    /// If true, this binding was introduced by an `as` alias (e.g., `Point { .. } as p`)
    pub is_as_alias: bool,
}

#[derive(Debug, Clone)]
pub struct DeadCodeFact {
    pub span: Span,
    pub cause: DeadCodeCause,
}

#[derive(Debug, Clone)]
pub struct UnusedExpressionFact {
    pub span: Span,
    pub kind: UnusedExpressionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardedTailKind {
    Result,
    Option,
    Partial,
}

#[derive(Debug, Clone)]
pub struct DiscardedTailFact {
    pub span: Span,
    pub kind: DiscardedTailKind,
    pub return_type: String,
}

#[derive(Debug, Clone)]
pub struct OverusedReferenceFact {
    pub span: Span,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnusedTypeParamFact {
    pub name: String,
    pub span: Span,
    pub is_typedef: bool,
}

#[derive(Debug, Clone)]
pub struct TypeParamOnlyInBoundFact {
    pub name: String,
    pub span: Span,
    pub is_typedef: bool,
}

/// Records a usage of a symbol, linking the usage location to its definition.
/// Used by LSP for find-references.
#[derive(Debug, Clone)]
pub struct Usage {
    pub usage_span: Span,
    pub definition_span: Span,
}
