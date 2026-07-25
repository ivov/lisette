mod generics;
mod unused_expressions;

use diagnostics::UnusedExpressionKind;
use rayon::prelude::*;
use std::sync::Arc;
use syntax::ast::Span;
use syntax::program::{File, Module};

use semantics::context::AnalysisContext;

use super::PARALLEL_THRESHOLD;

#[derive(Debug, Default)]
pub(crate) struct ProducedFacts {
    pub(crate) items: Vec<ProducedFact>,
}

#[derive(Debug)]
pub(crate) enum ProducedFact {
    UnusedExpression {
        span: Span,
        kind: UnusedExpressionKind,
    },
    DiscardedTail {
        span: Span,
        return_type: String,
        expected_span: Span,
        expected_type: String,
    },
    UnusedTypeParam {
        span: Span,
    },
    TypeParamOnlyInBound {
        name: String,
        span: Span,
    },
}

impl ProducedFacts {
    fn merge(&mut self, other: Self) {
        self.items.extend(other.items);
    }

    pub(super) fn add_unused_expression(&mut self, span: Span, kind: UnusedExpressionKind) {
        self.items
            .push(ProducedFact::UnusedExpression { span, kind });
    }

    pub(super) fn add_discarded_tail(
        &mut self,
        span: Span,
        return_type: String,
        expected_span: Span,
        expected_type: String,
    ) {
        self.items.push(ProducedFact::DiscardedTail {
            span,
            return_type,
            expected_span,
            expected_type,
        });
    }

    pub(super) fn add_unused_type_param(&mut self, span: Span) {
        self.items.push(ProducedFact::UnusedTypeParam { span });
    }

    pub(super) fn add_type_param_only_in_bound(&mut self, name: String, span: Span) {
        self.items
            .push(ProducedFact::TypeParamOnlyInBound { name, span });
    }
}

pub(crate) fn run_all(analysis: &AnalysisContext) -> ProducedFacts {
    let store = analysis.store;

    let mut work: Vec<(&Module, &File)> = store
        .modules
        .values()
        .map(Arc::as_ref)
        .flat_map(|module| module.source_files().map(move |file| (module, file)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    if work.len() < PARALLEL_THRESHOLD {
        let mut local = ProducedFacts::default();
        for (module, file) in &work {
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
        }
        return local;
    }

    let locals: Vec<ProducedFacts> = work
        .par_iter()
        .map(|(module, file)| {
            let mut local = ProducedFacts::default();
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
            local
        })
        .collect();

    let mut merged = ProducedFacts::default();
    for local in locals {
        merged.merge(local);
    }
    merged
}
