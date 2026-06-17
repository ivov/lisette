pub(crate) mod generics;
pub(crate) mod unused_expressions;

use rayon::prelude::*;
use std::sync::Arc;
use syntax::program::{File, Module};

use semantics::context::AnalysisContext;
use semantics::facts::LocalFacts;

use super::PARALLEL_THRESHOLD;

pub(crate) fn run_all(analysis: &AnalysisContext) -> LocalFacts {
    let store = analysis.store;

    let mut work: Vec<(&Module, &File)> = store
        .modules
        .values()
        .map(Arc::as_ref)
        .flat_map(|m| m.files.values().map(move |f| (m, f)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    if work.len() < PARALLEL_THRESHOLD {
        let mut local = LocalFacts::default();
        for (module, file) in &work {
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
        }
        return local;
    }

    let locals: Vec<LocalFacts> = work
        .par_iter()
        .map(|(module, file)| {
            let mut local = LocalFacts::default();
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
            local
        })
        .collect();

    let mut merged = LocalFacts::default();
    for local in locals {
        merged.merge(local);
    }
    merged
}
