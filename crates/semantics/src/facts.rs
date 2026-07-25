use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use syntax::EcoString;
use syntax::ast::{BindingId, BindingKind, DeadCodeCause, Span};
use syntax::program::{BindingMutation, TestFunction};
use syntax::types::Type;

#[derive(Debug, Default)]
pub struct BindingIdAllocator {
    next: AtomicU32,
}

impl BindingIdAllocator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn reserve(&self) -> BindingId {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn snapshot(&self) -> BindingId {
        self.next.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Facts {
    allocator: Arc<BindingIdAllocator>,

    // LSP-consumed; reshaping these affects crates/lsp/.
    pub bindings: HashMap<BindingId, BindingFact>,
    pub usages: HashSet<Usage>,

    pub dead_code: Vec<DeadCodeFact>,
    pub overused_references: Vec<OverusedReferenceFact>,
    pub always_failing_try_blocks: Vec<Span>,
    pub expression_only_fstrings: Vec<ExpressionOnlyFstringFact>,
    pub interface_satisfied_methods: HashMap<(String, String), Vec<InterfaceSatisfaction>>,
    pub(crate) equality_derivations: Vec<String>,
    pub(crate) test_functions: Vec<TestFunction>,

    pub(crate) deferred: DeferredChecks,

    /// Value-position `match`/`select` arms that did not reconcile, drained and
    /// checked against the use-site result type at the end of `infer_file`.
    pub(crate) branch_subsumptions: Vec<BranchSubsumption>,

    /// Value-position selects with one shorthand receive and no default,
    /// checked for exhaustiveness once the result type is pinned.
    pub(crate) select_exhaustiveness_checks: Vec<SelectExhaustivenessCheck>,

    /// Suppresses contradictory lints from or-patterns whose binding sets disagree.
    pub or_pattern_error_spans: HashSet<Span>,

    /// Spans of binary expressions the checker rejected, so lints can skip them.
    pub type_error_spans: HashSet<Span>,

    /// Span of every inferred function, so lints can attribute errors to
    /// the containing function.
    pub function_spans: Vec<Span>,

    /// Resolved type for each generic-bound annotation, keyed by the
    /// annotation's span. Lets emit render bounds from the resolved type
    /// instead of re-resolving the annotation.
    pub bound_types: HashMap<Span, Type>,
}

#[derive(Debug, Default)]
pub struct DeferredChecks {
    pub generic_calls: Vec<GenericCallCheck>,
    pub generic_bounds: Vec<GenericBoundObligation>,
    pub empty_collections: Vec<EmptyCollectionCheck>,
    pub empty_literals: Vec<EmptyLiteralCheck>,
    pub slice_makes: Vec<SliceMakeCheck>,
    pub statement_tails: Vec<StatementTailCheck>,
}

impl DeferredChecks {
    fn merge(&mut self, other: Self) {
        self.generic_calls.extend(other.generic_calls);
        self.generic_bounds.extend(other.generic_bounds);
        self.empty_collections.extend(other.empty_collections);
        self.empty_literals.extend(other.empty_literals);
        self.slice_makes.extend(other.slice_makes);
        self.statement_tails.extend(other.statement_tails);
    }
}

#[derive(Debug, Clone)]
pub struct InterfaceSatisfaction {
    pub impl_type_name: String,
    /// The interface matches by source spelling, so renaming breaks it.
    spelling_pinned: bool,
}

#[derive(Debug, Clone)]
pub struct GenericCallCheck {
    /// A type that must be fully resolved once inference finishes; if it still has
    /// unbound type variables, the call's generic parameter couldn't be inferred.
    pub ty: Type,
    pub span: Span,
    /// Qualifies `ty`'s variable ids, which are unique only per inference context.
    pub module_id: String,
}

#[derive(Debug, Clone)]
pub struct GenericBoundObligation {
    pub argument: Type,
    pub required: Type,
    pub span: Span,
    pub module_id: String,
    pub param_name: EcoString,
    pub(crate) available_bounds: Vec<(EcoString, Vec<Type>)>,
    pub origin: GenericBoundOrigin,
}

#[derive(Debug, Clone)]
pub enum GenericBoundOrigin {
    Construction {
        name: EcoString,
        enclosing_return_type: Option<Type>,
    },
    FunctionReference {
        name: EcoString,
    },
}

#[derive(Debug, Clone)]
pub struct EmptyCollectionCheck {
    pub name: String,
    pub ty: Type,
    pub span: Span,
    pub module_id: String,
}

/// An empty slice literal, rejected if its element type stays unbound.
#[derive(Debug, Clone)]
pub struct EmptyLiteralCheck {
    pub ty: Type,
    pub span: Span,
    pub module_id: String,
}

#[derive(Debug, Clone)]
pub struct SliceMakeCheck {
    pub ty: Type,
    pub span: Span,
    pub module_id: String,
}

#[derive(Debug, Clone)]
pub struct StatementTailCheck {
    pub expected_ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BranchSubsumption {
    pub(crate) result_ty: Type,
    pub(crate) arms: Vec<(Type, Span)>,
}

#[derive(Debug, Clone)]
pub struct SelectExhaustivenessCheck {
    pub(crate) result_ty: Type,
    pub(crate) span: Span,
}

impl Facts {
    pub fn new(allocator: Arc<BindingIdAllocator>) -> Self {
        Self {
            allocator,
            bindings: HashMap::default(),
            dead_code: Vec::new(),
            overused_references: Vec::new(),
            always_failing_try_blocks: Vec::new(),
            expression_only_fstrings: Vec::new(),
            deferred: DeferredChecks::default(),
            branch_subsumptions: Vec::new(),
            select_exhaustiveness_checks: Vec::new(),
            or_pattern_error_spans: HashSet::default(),
            type_error_spans: HashSet::default(),
            function_spans: Vec::new(),
            usages: HashSet::default(),
            interface_satisfied_methods: HashMap::default(),
            equality_derivations: Vec::new(),
            test_functions: Vec::new(),
            bound_types: HashMap::default(),
        }
    }

    pub(crate) fn allocator(&self) -> Arc<BindingIdAllocator> {
        self.allocator.clone()
    }

    pub fn take_deferred_checks(&mut self) -> DeferredChecks {
        std::mem::take(&mut self.deferred)
    }

    pub(crate) fn add_binding(
        &mut self,
        name: String,
        span: Span,
        kind: BindingKind,
        origin: BindingOrigin,
    ) -> BindingId {
        let id = self.allocator.reserve();
        self.bindings.insert(
            id,
            BindingFact {
                name,
                span,
                kind,
                used: false,
                mutation: None,
                origin,
            },
        );
        id
    }

    pub(crate) fn mark_used(&mut self, id: BindingId) {
        if let Some(fact) = self.bindings.get_mut(&id) {
            fact.used = true;
        }
    }

    pub fn binding_id_at(&self, span: Span) -> Option<BindingId> {
        self.bindings
            .iter()
            .find_map(|(&id, binding)| (binding.span == span).then_some(id))
    }

    pub(crate) fn mark_mutated(&mut self, id: BindingId) {
        if let Some(fact) = self.bindings.get_mut(&id) {
            fact.mutation = Some(fact.mutation.map_or(BindingMutation::Direct, |mutation| {
                mutation.merged_with(BindingMutation::Direct)
            }));
        }
    }

    /// The binding is mutated through an alias (address taken, mutable
    /// capture, mut argument or receiver), so a call can rebind it.
    pub(crate) fn mark_alias_mutated(&mut self, id: BindingId) {
        if let Some(fact) = self.bindings.get_mut(&id) {
            fact.mutation = Some(
                fact.mutation
                    .map_or(BindingMutation::ThroughAlias, |mutation| {
                        mutation.merged_with(BindingMutation::ThroughAlias)
                    }),
            );
        }
    }

    pub(crate) fn add_function_span(&mut self, span: Span) {
        self.function_spans.push(span);
    }

    pub(crate) fn binding_checkpoint(&self) -> BindingId {
        self.allocator.snapshot()
    }

    pub(crate) fn remove_bindings_from(&mut self, checkpoint: BindingId) {
        self.bindings.retain(|id, _| *id < checkpoint);
    }

    pub(crate) fn add_dead_code(&mut self, span: Span, cause: DeadCodeCause) {
        self.dead_code.push(DeadCodeFact { span, cause });
    }

    pub(crate) fn add_overused_reference(&mut self, span: Span, name: Option<String>) {
        self.overused_references
            .push(OverusedReferenceFact { span, name });
    }

    pub(crate) fn add_always_failing_try_block(&mut self, span: Span) {
        self.always_failing_try_blocks.push(span);
    }

    pub(crate) fn add_expression_only_fstring(&mut self, span: Span, needs_parens: bool) {
        self.expression_only_fstrings
            .push(ExpressionOnlyFstringFact { span, needs_parens });
    }

    pub(crate) fn add_usage(&mut self, usage_span: Span, definition_span: Span) {
        self.usages.insert(Usage {
            usage_span,
            definition_span,
        });
    }

    pub(crate) fn mark_method_used_for_interface(
        &mut self,
        module_id: String,
        method_name: String,
        impl_type_name: String,
        spelling_pinned: bool,
    ) {
        self.interface_satisfied_methods
            .entry((module_id, method_name))
            .or_default()
            .push(InterfaceSatisfaction {
                impl_type_name,
                spelling_pinned,
            });
    }

    /// Whether `type_name`'s `method_name` satisfies an interface that matches
    /// by source spelling, so the naming lint must not suggest a rename.
    pub fn method_spelling_pinned_by_interface(
        &self,
        module_id: &str,
        method_name: &str,
        type_name: &str,
    ) -> bool {
        self.interface_satisfied_methods
            .get(&(module_id.to_string(), method_name.to_string()))
            .is_some_and(|satisfactions| {
                satisfactions
                    .iter()
                    .any(|s| s.spelling_pinned && s.impl_type_name == type_name)
            })
    }

    pub(crate) fn merge(&mut self, other: Facts) {
        debug_assert!(
            Arc::ptr_eq(&self.allocator, &other.allocator),
            "Facts::merge requires a shared BindingIdAllocator",
        );

        let Facts {
            allocator: _,
            bindings,
            dead_code,
            overused_references,
            always_failing_try_blocks,
            expression_only_fstrings,
            deferred,
            branch_subsumptions,
            select_exhaustiveness_checks,
            or_pattern_error_spans,
            type_error_spans,
            function_spans,
            usages,
            interface_satisfied_methods,
            equality_derivations,
            test_functions,
            bound_types,
        } = other;
        self.equality_derivations.extend(equality_derivations);
        self.test_functions.extend(test_functions);
        self.bound_types.extend(bound_types);

        self.bindings.extend(bindings);
        self.dead_code.extend(dead_code);
        self.overused_references.extend(overused_references);
        self.always_failing_try_blocks
            .extend(always_failing_try_blocks);
        self.expression_only_fstrings
            .extend(expression_only_fstrings);
        self.deferred.merge(deferred);
        self.branch_subsumptions.extend(branch_subsumptions);
        self.select_exhaustiveness_checks
            .extend(select_exhaustiveness_checks);
        self.or_pattern_error_spans.extend(or_pattern_error_spans);
        self.type_error_spans.extend(type_error_spans);
        self.function_spans.extend(function_spans);

        self.usages.extend(usages);

        for (key, impl_types) in interface_satisfied_methods {
            self.interface_satisfied_methods
                .entry(key)
                .or_default()
                .extend(impl_types);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExpressionOnlyFstringFact {
    pub span: Span,
    pub needs_parens: bool,
}

#[derive(Debug, Clone)]
pub struct BindingFact {
    pub name: String,
    pub span: Span,
    pub kind: BindingKind,
    pub used: bool,
    pub mutation: Option<BindingMutation>,
    pub origin: BindingOrigin,
}

#[derive(Debug, Clone, Copy)]
pub enum BindingOrigin {
    Name {
        in_typedef: bool,
        shorthand_field: bool,
    },
    AsAlias {
        shorthand_field: bool,
    },
}

impl BindingOrigin {
    pub fn is_typedef(self) -> bool {
        matches!(
            self,
            Self::Name {
                in_typedef: true,
                ..
            }
        )
    }

    pub fn is_struct_field(self) -> bool {
        match self {
            Self::Name {
                shorthand_field, ..
            }
            | Self::AsAlias { shorthand_field } => shorthand_field,
        }
    }

    pub fn is_as_alias(self) -> bool {
        matches!(self, Self::AsAlias { .. })
    }
}

#[derive(Debug, Clone)]
pub struct DeadCodeFact {
    pub span: Span,
    pub cause: DeadCodeCause,
}

#[derive(Debug, Clone)]
pub struct OverusedReferenceFact {
    pub span: Span,
    pub name: Option<String>,
}

/// Records a usage of a symbol, linking the usage location to its definition.
/// Used by LSP for find-references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            BindingOrigin::Name {
                in_typedef: false,
                shorthand_field: false,
            },
        );
        let b_id = b.add_binding(
            "b".into(),
            span(1),
            BindingKind::Let { mutable: false },
            BindingOrigin::Name {
                in_typedef: false,
                shorthand_field: false,
            },
        );
        assert_ne!(a_id, b_id);

        a.merge(b);
        assert_eq!(a.bindings.len(), 2);
        assert!(a.bindings.contains_key(&a_id));
        assert!(a.bindings.contains_key(&b_id));
    }

    #[test]
    fn direct_mutation_does_not_downgrade_alias_mutation() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut facts = Facts::new(allocator);
        let id = facts.add_binding(
            "value".into(),
            span(0),
            BindingKind::Let { mutable: true },
            BindingOrigin::Name {
                in_typedef: false,
                shorthand_field: false,
            },
        );

        facts.mark_alias_mutated(id);
        facts.mark_mutated(id);

        assert_eq!(
            facts.bindings[&id].mutation,
            Some(BindingMutation::ThroughAlias)
        );
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
    fn merge_concatenates_interface_method_impl_types() {
        let allocator = Arc::new(BindingIdAllocator::new());
        let mut a = Facts::new(allocator.clone());
        let mut b = Facts::new(allocator);

        a.mark_method_used_for_interface("m".into(), "f".into(), "A".into(), true);
        b.mark_method_used_for_interface("m".into(), "f".into(), "B".into(), false);
        b.mark_method_used_for_interface("m".into(), "g".into(), "C".into(), true);

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
