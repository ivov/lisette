use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use diagnostics::{PatternIssue, UnusedExpressionKind};
use syntax::ast::{BindingId, BindingKind, DeadCodeCause, Span};
use syntax::types::Type;

#[derive(Debug, Default)]
pub struct BindingIdAllocator {
    next: AtomicU32,
}

impl BindingIdAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reserve(&self) -> BindingId {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> BindingId {
        self.next.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Facts {
    allocator: Arc<BindingIdAllocator>,
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
    pub generic_call_checks: Vec<GenericCallCheck>,
    pub empty_collection_checks: Vec<EmptyCollectionCheck>,
    pub statement_tail_checks: Vec<StatementTailCheck>,
    /// Spans of or-patterns with binding errors, used to suppress contradictory lints.
    pub or_pattern_error_spans: HashSet<Span>,
    /// Tracks usage locations for find-references in LSP
    pub usages: Vec<Usage>,
    usage_set: HashSet<(Span, Span)>,
    /// Tracks methods used via interface satisfaction during type checking.
    /// Maps (module_id, method_name) -> usage locations
    pub interface_satisfied_methods: HashMap<(String, String), Vec<Span>>,
}

#[derive(Debug, Clone)]
pub struct GenericCallCheck {
    pub return_ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EmptyCollectionCheck {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StatementTailCheck {
    pub expected_ty: Type,
    pub span: Span,
}

impl Facts {
    pub fn new(allocator: Arc<BindingIdAllocator>) -> Self {
        Self {
            allocator,
            bindings: HashMap::default(),
            dead_code: Vec::new(),
            pattern_issues: Vec::new(),
            unused_expressions: Vec::new(),
            discarded_tail_expressions: Vec::new(),
            overused_references: Vec::new(),
            unused_type_params: Vec::new(),
            type_params_only_in_bound: Vec::new(),
            always_failing_try_blocks: Vec::new(),
            expression_only_fstrings: Vec::new(),
            generic_call_checks: Vec::new(),
            empty_collection_checks: Vec::new(),
            statement_tail_checks: Vec::new(),
            or_pattern_error_spans: HashSet::default(),
            usages: Vec::new(),
            usage_set: HashSet::default(),
            interface_satisfied_methods: HashMap::default(),
        }
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
        let id = self.allocator.reserve();
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
        self.allocator.snapshot()
    }

    pub fn remove_bindings_from(&mut self, checkpoint: BindingId) {
        self.bindings.retain(|id, _| *id < checkpoint);
    }

    pub fn add_dead_code(&mut self, span: Span, cause: DeadCodeCause) {
        self.dead_code.push(DeadCodeFact { span, cause });
    }

    pub fn add_unused_expression(&mut self, span: Span, kind: UnusedExpressionKind) {
        self.unused_expressions
            .push(UnusedExpressionFact { span, kind });
    }

    pub fn add_discarded_tail(
        &mut self,
        span: Span,
        return_type: String,
        expected_span: Span,
        expected_type: String,
    ) {
        self.discarded_tail_expressions.push(DiscardedTailFact {
            span,
            return_type,
            expected_span,
            expected_type,
        });
    }

    pub fn add_overused_reference(&mut self, span: Span, name: Option<String>) {
        self.overused_references
            .push(OverusedReferenceFact { span, name });
    }

    pub fn add_unused_type_param(&mut self, name: String, span: Span) {
        self.unused_type_params
            .push(UnusedTypeParamFact { name, span });
    }

    pub fn add_type_param_only_in_bound(&mut self, name: String, span: Span) {
        self.type_params_only_in_bound
            .push(TypeParamOnlyInBoundFact { name, span });
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

    pub fn merge(&mut self, other: Facts) {
        debug_assert!(
            Arc::ptr_eq(&self.allocator, &other.allocator),
            "Facts::merge requires a shared BindingIdAllocator",
        );

        let Facts {
            allocator: _,
            bindings,
            dead_code,
            pattern_issues,
            unused_expressions,
            discarded_tail_expressions,
            overused_references,
            unused_type_params,
            type_params_only_in_bound,
            always_failing_try_blocks,
            expression_only_fstrings,
            generic_call_checks,
            empty_collection_checks,
            statement_tail_checks,
            or_pattern_error_spans,
            usages,
            usage_set: _,
            interface_satisfied_methods,
        } = other;

        self.bindings.extend(bindings);
        self.dead_code.extend(dead_code);
        self.pattern_issues.extend(pattern_issues);
        self.unused_expressions.extend(unused_expressions);
        self.discarded_tail_expressions
            .extend(discarded_tail_expressions);
        self.overused_references.extend(overused_references);
        self.unused_type_params.extend(unused_type_params);
        self.type_params_only_in_bound
            .extend(type_params_only_in_bound);
        self.always_failing_try_blocks
            .extend(always_failing_try_blocks);
        self.expression_only_fstrings
            .extend(expression_only_fstrings);
        self.generic_call_checks.extend(generic_call_checks);
        self.empty_collection_checks.extend(empty_collection_checks);
        self.statement_tail_checks.extend(statement_tail_checks);
        self.or_pattern_error_spans.extend(or_pattern_error_spans);

        self.usages.reserve(usages.len());
        self.usage_set.reserve(usages.len());
        for Usage {
            usage_span,
            definition_span,
        } in usages
        {
            self.add_usage(usage_span, definition_span);
        }

        for (key, spans) in interface_satisfied_methods {
            self.interface_satisfied_methods
                .entry(key)
                .or_default()
                .extend(spans);
        }
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

#[derive(Debug, Clone)]
pub struct DiscardedTailFact {
    pub span: Span,
    pub return_type: String,
    pub expected_span: Span,
    pub expected_type: String,
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
}

#[derive(Debug, Clone)]
pub struct TypeParamOnlyInBoundFact {
    pub name: String,
    pub span: Span,
}

/// Records a usage of a symbol, linking the usage location to its definition.
/// Used by LSP for find-references.
#[derive(Debug, Clone)]
pub struct Usage {
    pub usage_span: Span,
    pub definition_span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::BindingKind;

    fn span(offset: u32) -> Span {
        Span::new(0, offset, 1)
    }

    #[test]
    fn merge_preserves_unique_binding_ids_across_tasks() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator.clone());

        let a_id = a.add_binding(
            "a".into(),
            span(0),
            BindingKind::Let { mutable: false },
            false,
            false,
            false,
        );
        let b_id = b.add_binding(
            "b".into(),
            span(1),
            BindingKind::Let { mutable: false },
            false,
            false,
            false,
        );
        assert_ne!(a_id, b_id);

        a.merge(b);
        assert_eq!(a.bindings.len(), 2);
        assert!(a.bindings.contains_key(&a_id));
        assert!(a.bindings.contains_key(&b_id));
    }

    #[test]
    fn merge_extends_vec_facts() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator);

        a.add_always_failing_try_block(span(0));
        b.add_always_failing_try_block(span(1));
        b.add_always_failing_try_block(span(2));

        a.merge(b);
        assert_eq!(a.always_failing_try_blocks.len(), 3);
    }

    #[test]
    fn merge_deduplicates_usages() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator);

        a.add_usage(span(10), span(0));
        b.add_usage(span(10), span(0));
        b.add_usage(span(20), span(0));

        a.merge(b);
        assert_eq!(a.usages.len(), 2);
    }

    #[test]
    fn merge_deduplicates_or_pattern_error_spans() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator);

        a.or_pattern_error_spans.insert(span(0));
        b.or_pattern_error_spans.insert(span(0));
        b.or_pattern_error_spans.insert(span(1));

        a.merge(b);
        assert_eq!(a.or_pattern_error_spans.len(), 2);
    }

    #[test]
    fn merge_concatenates_interface_method_spans() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator);

        a.mark_method_used_for_interface("m".into(), "f".into(), span(0));
        b.mark_method_used_for_interface("m".into(), "f".into(), span(1));
        b.mark_method_used_for_interface("m".into(), "g".into(), span(2));

        a.merge(b);
        assert_eq!(a.interface_satisfied_methods.len(), 2);
        assert_eq!(
            a.interface_satisfied_methods[&("m".into(), "f".into())].len(),
            2
        );
        assert_eq!(
            a.interface_satisfied_methods[&("m".into(), "g".into())].len(),
            1
        );
    }
}
